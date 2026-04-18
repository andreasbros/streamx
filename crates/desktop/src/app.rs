//! Main window view. Reads from `Arc<AppState>`, dispatches to page renderers,
//! and owns the async bootstrap + keybinding translation.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, px, App, Context, FocusHandle, Focusable, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, Styled, Window,
};
use streamx_api::client::Client;

use crate::keybindings::{translate, Shortcut};
use crate::pages::{login_page, loading_page, movie_page, search_page};
use crate::router::Page;
use crate::runtime;
use crate::state::{AppState, BrowseData};
use crate::theme::Theme;

pub struct MainView {
    state: Arc<AppState>,
    theme: Theme,
    focus_handle: FocusHandle,
}

impl MainView {
    pub fn new(state: Arc<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let this = Self {
            state: state.clone(),
            theme: Theme::new(),
            focus_handle,
        };

        // Kick off startup: server version + login + initial browse.
        this.bootstrap(cx);

        // Periodic tick to redraw for pending state changes flushed into RwLock
        // by the tokio runtime. The idiomatic fix is observers, but this keeps
        // Phase 4 simple.
        cx.spawn({
            let state = state.clone();
            async move |this, cx: &mut gpui::AsyncApp| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                    let _ = state.clone(); // keep alive
                    if this
                        .update(cx, |_, cx| {
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        })
        .detach();

        this
    }

    fn bootstrap(&self, cx: &mut Context<Self>) {
        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            // 1. Fetch version
            let client = state.client.read().clone();
            let version_res = runtime::spawn(async move { client.version().await }).await;
            match version_res {
                Ok(v) => {
                    *state.server_version.write() = Some(v.version);
                    *state.server_hash.write() = Some(v.hash);
                    *state.connection_error.write() = None;
                }
                Err(e) => {
                    *state.connection_error.write() =
                        Some(format!("server unreachable: {e}"));
                }
            }
            let _ = this.update(cx, |_, cx| cx.notify());

            // 2. If not authed, try env-var login once.
            if !state.is_authed() {
                if let (Ok(user), Ok(pass)) = (
                    std::env::var("STREAMX_USERNAME"),
                    std::env::var("STREAMX_PASSWORD"),
                ) {
                    try_login(&state, &user, &pass).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                }
            }

            // 3. Fetch /me so the drawer can show username.
            if state.is_authed() {
                let client = state.client.read().clone();
                if let Ok(u) = runtime::spawn(async move { client.me().await }).await {
                    *state.user.write() = Some(u);
                }
            }

            // 4. If on Search page, load browse data.
            if matches!(state.current_page(), Page::Search) && state.is_authed() {
                load_browse(&state).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }

    fn handle_shortcut(&mut self, s: Shortcut, cx: &mut Context<Self>) {
        match s {
            Shortcut::Back => {
                if !self.state.back() {
                    // top of stack: noop
                }
                cx.notify();
            }
            Shortcut::Activate => {
                // Select first tile on Search page, or play first variant on Movie.
                if matches!(self.state.current_page(), Page::Search) {
                    let browse = self.state.browse.read().clone();
                    if let Some(first) = first_tile(&browse) {
                        *self.state.selected_movie.write() = Some(first);
                        self.state.navigate(Page::Movie);
                        cx.notify();
                    }
                }
            }
            Shortcut::FocusSearch => {
                // Phase 4b: focus a real text input. For now, toast.
                *self.state.toast.write() =
                    Some("Real search input lands in Phase 4b".to_string());
                cx.notify();
            }
            _ => {}
        }
    }
}

async fn try_login(state: &Arc<AppState>, user: &str, pass: &str) {
    let client = state.client.read().clone();
    let user = user.to_string();
    let pass = pass.to_string();
    let res = runtime::spawn(async move { client.login(&user, &pass).await }).await;
    match res {
        Ok(resp) => {
            state.set_token(Some(resp.token));
            state.replace_page(Page::Search);
            *state.connection_error.write() = None;
        }
        Err(e) => {
            *state.connection_error.write() = Some(format!("login failed: {e}"));
        }
    }
}

async fn load_browse(state: &Arc<AppState>) {
    use streamx_api::client::BrowseParams;

    *state.browse_loading.write() = true;
    let client: Client = state.client.read().clone();
    let sections: [(&str, BrowseParams); 8] = [
        ("latest", BrowseParams { sort_by: Some("date_added".into()), limit: Some(10), ..Default::default() }),
        ("popular", BrowseParams { sort_by: Some("download_count".into()), limit: Some(10), ..Default::default() }),
        ("top_rated", BrowseParams { sort_by: Some("rating".into()), minimum_rating: Some(8), limit: Some(10), ..Default::default() }),
        ("action", BrowseParams { sort_by: Some("download_count".into()), genre: Some("action".into()), limit: Some(10), ..Default::default() }),
        ("comedy", BrowseParams { sort_by: Some("download_count".into()), genre: Some("comedy".into()), limit: Some(10), ..Default::default() }),
        ("thriller", BrowseParams { sort_by: Some("download_count".into()), genre: Some("thriller".into()), limit: Some(10), ..Default::default() }),
        ("scifi", BrowseParams { sort_by: Some("download_count".into()), genre: Some("sci-fi".into()), limit: Some(10), ..Default::default() }),
        ("horror", BrowseParams { sort_by: Some("download_count".into()), genre: Some("horror".into()), limit: Some(10), ..Default::default() }),
    ];

    let mut out = BrowseData::default();
    for (name, p) in sections {
        let c = client.clone();
        let r = runtime::spawn(async move { c.browse(&p).await }).await;
        if let Ok(rows) = r {
            match name {
                "latest" => out.latest = rows,
                "popular" => out.popular = rows,
                "top_rated" => out.top_rated = rows,
                "action" => out.action = rows,
                "comedy" => out.comedy = rows,
                "thriller" => out.thriller = rows,
                "scifi" => out.scifi = rows,
                "horror" => out.horror = rows,
                _ => {}
            }
        }
    }
    *state.browse.write() = out;
    *state.browse_loading.write() = false;
}

fn first_tile(b: &BrowseData) -> Option<streamx_api::types::SearchResultGroup> {
    b.latest
        .first()
        .or_else(|| b.popular.first())
        .or_else(|| b.top_rated.first())
        .cloned()
}

impl Focusable for MainView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MainView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let page = self.state.current_page();

        let content = match page {
            Page::Login => login_page(&self.state, &theme).into_any_element(),
            Page::Search => search_page(&self.state, &theme).into_any_element(),
            Page::Movie => movie_page(&self.state, &theme).into_any_element(),
            Page::Loading => loading_page(&theme, "loading…").into_any_element(),
        };

        div()
            .track_focus(&self.focus_handle)
            .key_context("StreamX")
            .size_full()
            .bg(theme.bg_app())
            .text_color(theme.fg_primary())
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| {
                if let Some(s) = translate(ev) {
                    this.handle_shortcut(s, cx);
                }
            }))
            // Title bar strip
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(theme.space_4()))
                    .py(px(theme.space_2()))
                    .border_b_1()
                    .border_color(theme.border_subtle())
                    .bg(theme.bg_surface())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(theme.space_2()))
                            .child(
                                div()
                                    .text_size(px(theme.fs_3()))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme.accent_text())
                                    .child("StreamX"),
                            )
                            .child(
                                div()
                                    .text_size(px(theme.fs_1()))
                                    .text_color(theme.fg_muted())
                                    .child(SharedString::from(
                                        self.state
                                            .server_version
                                            .read()
                                            .clone()
                                            .map(|v| format!("v{}", v))
                                            .unwrap_or_else(|| "offline".into()),
                                    )),
                            ),
                    )
                    .child(match self.state.user.read().as_ref() {
                        Some(u) => div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_secondary())
                            .child(SharedString::from(format!("@{}", u.username))),
                        None => div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .child("not signed in"),
                    }),
            )
            .child(div().flex_1().overflow_hidden().child(content))
    }
}

