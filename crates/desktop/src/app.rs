//! Main window view: owns input entities, dispatches pages, drives async
//! bootstrap + login + search + playback.

use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, img, px, App, AppContext, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ObjectFit, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, Window,
};
// Resize borders + invisible edge strips only exist on Linux (client-side
// decorations). Keep the imports scoped to that cfg so Darwin stays warning-free.
#[cfg(target_os = "linux")]
use gpui::{CursorStyle, ResizeEdge};
use parking_lot::Mutex;
use streamx_api::client::Client;

use crate::components::{
    card, frost_card, movie_tile, primary_button, secondary_button, section_title,
};
use crate::keybindings::{translate, Shortcut};
use crate::pages::{loading_page, movie_page, stub_page};
use crate::playback;
use crate::playback::ipc::{MpvIpc, Snapshot};
use crate::playback::{Control, PlayTarget, Player};
use crate::router::Page;
use crate::runtime;
use crate::state::{AppState, BrowseData, Mode, Toast, ToastKind};
use crate::text_input::{text_input, TextInput};
use crate::theme::Theme;

#[derive(Default)]
pub struct PlayerState {
    pub stream_id: Option<String>,
    pub file_index: usize,
    pub target: Option<PlayTarget>,
    pub error: Option<String>,
    pub mpv: Option<Player>,
    pub ipc: Option<Control>,
    pub snapshot: Snapshot,
    /// Latest torrent status snapshot (progress, peers, speed).
    pub torrent: Option<streamx_api::types::StreamStatus>,
    /// Last CreateStreamRequest we kicked off — retained so a Retry
    /// button can replay it without the user navigating back.
    pub last_request: Option<streamx_api::types::CreateStreamRequest>,
}

pub struct MainView {
    state: Arc<AppState>,
    theme: Theme,
    focus_handle: FocusHandle,

    username_input: Entity<TextInput>,
    password_input: Entity<TextInput>,
    /// Maintenance op ("clean" | "wipe") awaiting its confirming click.
    confirm_maintenance: Option<&'static str>,
    repeat_input: Entity<TextInput>,
    logs_scroll: gpui::UniformListScrollHandle,
    /// Shared page scroll container. Offsets are saved per page so
    /// going back (e.g. Movie -> Search) restores the exact position.
    page_scroll: gpui::ScrollHandle,
    page_scroll_saved: std::collections::HashMap<Page, gpui::Point<gpui::Pixels>>,
    last_scroll_page: Option<Page>,
    /// Follow the log tail. Cleared when the user scrolls up; restored
    /// when they return to the bottom.
    logs_follow: std::cell::Cell<bool>,
    /// Login page shows account creation instead of sign-in.
    login_create_mode: bool,
    url_input: Entity<TextInput>,
    search_input: Entity<TextInput>,
    admin_kill_input: Entity<TextInput>,
    music_input: Entity<TextInput>,
    music_video_input: Entity<TextInput>,
    tv_input: Entity<TextInput>,

    player: Arc<Mutex<PlayerState>>,
}

/// Handle to the main window so background tasks (tick loop) can inject
/// synthetic input coming from the ui-test driver.
static MAIN_WINDOW: std::sync::OnceLock<gpui::AnyWindowHandle> = std::sync::OnceLock::new();

/// Whether the main window opened. Used by the ui-test driver so the
/// harness fails fast when the app came up headless or crashed at
/// window creation.
pub fn main_window_open() -> bool {
    MAIN_WINDOW.get().is_some()
}

impl MainView {
    pub fn new(state: Arc<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _ = MAIN_WINDOW.set(window.window_handle());
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let theme = Theme::new();
        let username_input = text_input(cx, "username");
        let password_input = cx.new(|c| {
            crate::text_input::TextInput::new(c)
                .with_placeholder("password")
                .password()
        });
        let repeat_input = cx.new(|c| {
            crate::text_input::TextInput::new(c)
                .with_placeholder("repeat password")
                .password()
        });
        let url_input = cx.new(|c| {
            crate::text_input::TextInput::new(c)
                .with_placeholder("http://localhost:8999")
                .initial(state.server_url.read().clone())
        });
        let search_input = text_input(cx, "Search movies or paste a magnet link...");
        let admin_kill_input = text_input(cx, "stream id");
        let music_input = text_input(cx, "artist or album");
        let music_video_input = text_input(cx, "music video");
        let tv_input = text_input(cx, "TV show");

        // Focus the username field only when the login page is actually
        // shown. Focusing it while booting authed leaves keyboard focus
        // on an unrendered input, which swallows every shortcut (and all
        // typing) until the user clicks somewhere.
        if matches!(state.current_page(), Page::Login) {
            let username_focus = username_input.read(cx).focus_handle(cx);
            username_focus.focus(window, cx);
        }

        // Fresh database: offer account creation instead of sign-in.
        // The in-process backend appears once the embedded server boots,
        // so poll briefly; remote/HTTP backends always answer false.
        if matches!(state.current_page(), Page::Login) {
            let probe_state = state.clone();
            cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
                for _ in 0..20 {
                    let client = probe_state.client.read().clone();
                    let needs = runtime::spawn(async move { client.needs_setup().await }).await;
                    if let Ok(true) = needs {
                        let _ = this.update(cx, |view, cx| {
                            view.login_create_mode = true;
                            cx.notify();
                        });
                        return;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                }
            })
            .detach();
        }

        let this = Self {
            state: state.clone(),
            theme,
            focus_handle,
            username_input,
            password_input,
            repeat_input,
            logs_scroll: gpui::UniformListScrollHandle::new(),
            page_scroll: gpui::ScrollHandle::new(),
            page_scroll_saved: std::collections::HashMap::new(),
            last_scroll_page: None,
            logs_follow: std::cell::Cell::new(true),
            login_create_mode: false,
            confirm_maintenance: None,
            url_input,
            search_input,
            admin_kill_input,
            music_input,
            music_video_input,
            tv_input,
            player: Arc::new(Mutex::new(PlayerState::default())),
        };

        this.bootstrap(cx);
        this.tick(cx);
        this
    }

    fn bootstrap(&self, cx: &mut Context<Self>) {
        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            // The embedded server boots in the background; retry until it
            // answers (or ~30s pass) instead of giving up on the first
            // refused connection. Each attempt re-reads the client, so the
            // in-process backend is picked up as soon as it's installed.
            let mut connected = false;
            for _ in 0..60 {
                let client = state.client.read().clone();
                match runtime::spawn(async move { client.version().await }).await {
                    Ok(v) => {
                        *state.server_version.write() = Some(v.version);
                        *state.server_hash.write() = Some(v.hash);
                        *state.connection_error.write() = None;
                        connected = true;
                    }
                    Err(e) => {
                        *state.connection_error.write() = Some(format!("server unreachable: {e}"));
                    }
                }
                let _ = this.update(cx, |_, cx| cx.notify());
                if connected {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
            }

            if state.is_authed() {
                let client = state.client.read().clone();
                if let Ok(u) = runtime::spawn(async move { client.me().await }).await {
                    *state.user.write() = Some(u);
                }
                if matches!(state.current_page(), Page::Search) {
                    load_browse(&state).await;
                }
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }

    fn tick(&self, cx: &mut Context<Self>) {
        let player = self.player.clone();
        let state = self.state.clone();
        // Debounced live-search: track the last value seen per input and
        // when it stopped changing, fire a search.
        let mut search_db = DebounceState::default();
        let mut music_db = DebounceState::default();
        let mut mv_db = DebounceState::default();
        let mut tv_db = DebounceState::default();
        let debounce = Duration::from_millis(300);
        // Downloads refresh cadence for the Downloads and Movie pages.
        let mut last_dl_refresh = std::time::Instant::now() - Duration::from_secs(10);
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                state
                    .tick_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                {
                    let mut p = player.lock();
                    if let Some(mpv) = p.mpv.as_mut() {
                        if mpv.is_finished() {
                            crate::playback::dispose(p.mpv.take(), p.ipc.take());
                        }
                    }
                }

                // Poll mpv IPC for play-state snapshot (paused + time-pos + duration).
                let ipc_clone = player.lock().ipc.clone();
                if let Some(ipc) = ipc_clone {
                    let snap = runtime::spawn(async move { ipc.snapshot().await }).await;
                    player.lock().snapshot = snap;
                    state.mark_dirty();
                }

                // Poll torrent status (peers, speed, progress) on the Player page.
                let sid = player.lock().stream_id.clone();
                if let Some(sid) = sid {
                    let client = state.client.read().clone();
                    let sid_clone = sid.clone();
                    if let Ok(ts) =
                        runtime::spawn(async move { client.stream_status(&sid_clone).await }).await
                    {
                        player.lock().torrent = Some(ts);
                        state.mark_dirty();
                    }
                }

                // Apply synthetic input queued by the ui-test driver on
                // the UI thread, through the real event dispatch path.
                let keys: Vec<String> = std::mem::take(&mut *state.ui_keys.lock());
                if !keys.is_empty() {
                    match MAIN_WINDOW.get() {
                        Some(handle) => {
                            let result = cx.update_window(*handle, |_, window, cx| {
                                for k in &keys {
                                    match gpui::Keystroke::parse(k) {
                                        Ok(ks) => {
                                            let handled = window.dispatch_keystroke(ks, cx);
                                            tracing::debug!(key = %k, handled, "ui-test keystroke");
                                        }
                                        Err(e) => {
                                            tracing::warn!(key = %k, "ui-test keystroke parse failed: {e}")
                                        }
                                    }
                                }
                                window.refresh();
                            });
                            if let Err(e) = result {
                                tracing::warn!("ui-test key dispatch failed: {e}");
                            }
                        }
                        None => tracing::warn!("ui-test keys dropped: no window"),
                    }
                }

                // Apply driver-requested window resizes on the UI thread.
                if let Some((w, h)) = state.ui_resize.lock().take() {
                    if let Some(handle) = MAIN_WINDOW.get() {
                        let _ = cx.update_window(*handle, |_, window, _cx| {
                            window.resize(gpui::size(gpui::px(w), gpui::px(h)));
                            window.refresh();
                        });
                        state.mark_dirty();
                    }
                }

                // Serve driver screenshot requests from GPUI's own
                // renderer: pixel-identical on every platform, no OS
                // capture tooling needed.
                #[cfg(feature = "ui-test")]
                {
                    let shots: Vec<String> = std::mem::take(&mut *state.ui_shots.lock());
                    if !shots.is_empty() {
                        if let Some(handle) = MAIN_WINDOW.get() {
                            let _ = cx.update_window(*handle, |_, window, _cx| {
                                for path in &shots {
                                    match window.render_to_image() {
                                        Ok(img) => {
                                            if let Err(e) = img.save(path) {
                                                tracing::warn!("screenshot save failed: {e}");
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!("render_to_image failed: {e}")
                                        }
                                    }
                                }
                            });
                        }
                    }
                }

                // Stream in posters whose bytes just landed on disk, and
                // retry failed ones whose backoff elapsed: evicting an
                // asset makes GPUI re-run the AssetSource next frame
                // (which now finds the disk cache).
                let mut evict: Vec<String> = std::mem::take(&mut *state.poster_ready.lock());
                evict.extend(state.due_poster_retries());
                if !evict.is_empty() {
                    let _ = this.update(cx, |_, cx| {
                        for path in &evict {
                            gpui::ImageSource::Resource(gpui::Resource::Embedded(
                                SharedString::from(path.clone()),
                            ))
                            .remove_asset(cx);
                        }
                        cx.refresh_windows();
                    });
                    state.mark_dirty();
                }

                // Infinite scroll on the category page: the virtualized
                // grid flags when the viewport nears its last row (which
                // also fires when content is shorter than the viewport,
                // filling the first screen automatically).
                if state
                    .category_need_more
                    .swap(false, std::sync::atomic::Ordering::Relaxed)
                    && matches!(state.current_page(), Page::CategoryBrowse)
                    && !*state.category_loading.read()
                    && !*state.category_done.read()
                {
                    let st = state.clone();
                    runtime::spawn_detached(async move { load_category_page(&st).await });
                }

                // Logs page: repaint only when the ring buffer changed,
                // and follow the tail unless the user scrolled away.
                if matches!(state.current_page(), Page::Logs) {
                    let len = state.logs.len();
                    let seen = state
                        .logs_seen
                        .swap(len, std::sync::atomic::Ordering::Relaxed);
                    let _ = this.update(cx, |view, cx| {
                        // At-bottom detection decides follow mode: a user
                        // scrolled to the tail follows, anywhere else the
                        // view stays put while lines keep arriving.
                        let (at_bottom, scrollable) = {
                            let sc = view.logs_scroll.0.borrow();
                            match sc.last_item_size {
                                Some(sz) if sz.contents.height > sz.item.height => {
                                    let offset_y = -sc.base_handle.offset().y;
                                    let gap =
                                        sz.contents.height - (offset_y + sz.item.height);
                                    (gap < gpui::px(28.0), true)
                                }
                                _ => (true, false),
                            }
                        };
                        if scrollable {
                            view.logs_follow.set(at_bottom);
                        }
                        if seen != len {
                            if view.logs_follow.get() {
                                view.logs_scroll.scroll_to_bottom();
                            }
                            view.state.mark_dirty();
                            cx.notify();
                        }
                    });
                }

                // Keep download progress fresh on the pages that show it.
                if matches!(state.current_page(), Page::Downloads | Page::Movie)
                    && last_dl_refresh.elapsed() >= Duration::from_secs(2)
                    && !*state.downloads_loading.read()
                {
                    last_dl_refresh = std::time::Instant::now();
                    let st = state.clone();
                    runtime::spawn_detached(async move { load_downloads(&st).await });
                }

                // The embedded backend renews expired session tokens in
                // place; mirror the fresh token into persisted state so
                // playback URLs and the next app start use it too.
                {
                    let client_token = state.client.read().token();
                    if let Some(t) = client_token {
                        let differs = state.token.read().as_deref() != Some(t.as_str());
                        if differs {
                            state.set_token(Some(t));
                        }
                    }
                }

                // Auto-dismiss toasts after 3 seconds.
                {
                    let clear = state
                        .toast
                        .read()
                        .as_ref()
                        .map(|t| t.posted_at.elapsed() > Duration::from_secs(3))
                        .unwrap_or(false);
                    if clear {
                        state.clear_toast();
                    }
                }

                // Snapshot value + submitted flag for each search input.
                let inputs = this
                    .update(cx, |this, cx| {
                        let mut snap = |e: &Entity<TextInput>| -> (String, bool) {
                            let v = e.read(cx).value().to_string();
                            let s = e.read(cx).submitted;
                            if s {
                                e.update(cx, |input, _| input.submitted = false);
                            }
                            (v, s)
                        };
                        (
                            snap(&this.search_input),
                            snap(&this.music_input),
                            snap(&this.music_video_input),
                            snap(&this.tv_input),
                        )
                    })
                    .ok();
                let Some(((sv, ss), (mv, ms), (mvv, mvs), (tv, ts))) = inputs else {
                    break;
                };
                *state.search_input_mirror.write() = sv.clone();

                fire_debounced(&sv, ss, &mut search_db, debounce, |q| {
                    let st = state.clone();
                    runtime::spawn_detached(async move { run_search(st, q).await });
                });
                fire_debounced(&mv, ms, &mut music_db, debounce, |q| {
                    let st = state.clone();
                    runtime::spawn_detached(async move { run_music_search(st, q).await });
                });
                fire_debounced(&mvv, mvs, &mut mv_db, debounce, |q| {
                    let st = state.clone();
                    runtime::spawn_detached(async move { run_music_video_search(st, q).await });
                });
                fire_debounced(&tv, ts, &mut tv_db, debounce, |q| {
                    let st = state.clone();
                    runtime::spawn_detached(async move { run_tv_search(st, q).await });
                });

                // Repaint only when something rendered actually changed.
                // The previous unconditional notify forced a full
                // re-render of every page at 10Hz, which made resize,
                // typing and scrolling feel sluggish.
                if state.take_dirty() && this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
                if this.update(cx, |_, _| ()).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn submit_login(&mut self, cx: &mut Context<Self>) {
        if *self.state.login_in_flight.read() {
            return;
        }
        let username = self.username_input.read(cx).value().to_string();
        let password = self.password_input.read(cx).value().to_string();
        let url = self.url_input.read(cx).value().to_string();
        if username.is_empty() || password.is_empty() {
            *self.state.login_error.write() = Some("username and password required".to_string());
            cx.notify();
            return;
        }
        *self.state.login_in_flight.write() = true;
        *self.state.login_error.write() = None;

        self.state.set_server_url(url);

        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let res = runtime::spawn(async move { client.login(&username, &password).await }).await;
            match res {
                Ok(resp) => {
                    state.set_token(Some(resp.token));
                    *state.login_error.write() = None;
                    state.replace_page(Page::Search);

                    let client = state.client.read().clone();
                    if let Ok(u) = runtime::spawn(async move { client.me().await }).await {
                        *state.user.write() = Some(u);
                    }
                    load_browse(&state).await;
                }
                Err(e) => {
                    *state.login_error.write() = Some(format!("{e}"));
                }
            }
            *state.login_in_flight.write() = false;
            state.mark_dirty();
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    fn submit_register(&mut self, cx: &mut Context<Self>) {
        if *self.state.login_in_flight.read() {
            return;
        }
        let username = self.username_input.read(cx).value().to_string();
        let password = self.password_input.read(cx).value().to_string();
        let repeat = self.repeat_input.read(cx).value().to_string();
        // Mirror the server's rules so mistakes surface before the call.
        let error = if username.len() < 3 || username.len() > 32 {
            Some("username must be 3-32 characters")
        } else if password.len() < 8 || password.len() > 128 {
            Some("password must be 8-128 characters")
        } else if password != repeat {
            Some("passwords do not match")
        } else {
            None
        };
        if let Some(e) = error {
            *self.state.login_error.write() = Some(e.to_string());
            cx.notify();
            return;
        }
        *self.state.login_in_flight.write() = true;
        *self.state.login_error.write() = None;

        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let res =
                runtime::spawn(async move { client.register(&username, &password).await }).await;
            match res {
                Ok(resp) => {
                    state.set_token(Some(resp.token));
                    *state.login_error.write() = None;
                    state.replace_page(Page::Search);
                    let client = state.client.read().clone();
                    if let Ok(u) = runtime::spawn(async move { client.me().await }).await {
                        *state.user.write() = Some(u);
                    }
                    load_browse(&state).await;
                }
                Err(e) => {
                    *state.login_error.write() = Some(format!("{e}"));
                }
            }
            *state.login_in_flight.write() = false;
            state.mark_dirty();
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    fn start_playback(&mut self, variant_idx: usize, cx: &mut Context<Self>) {
        let movie = match self.state.selected_movie.read().clone() {
            Some(m) => m,
            None => return,
        };
        let variant = match movie.variants.get(variant_idx) {
            Some(v) => v.clone(),
            None => return,
        };

        let magnet_preview = variant
            .magnet
            .split("&dn=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .unwrap_or("<no dn>");
        tracing::info!(
            title = %movie.title,
            variant_idx,
            quality = %variant.quality.clone().unwrap_or_default(),
            magnet_dn = %magnet_preview,
            "starting playback"
        );

        let req = streamx_api::types::CreateStreamRequest {
            magnet_uri: variant.magnet.clone(),
            file_index: None,
            poster_url: None,
            title: Some(movie.title.clone()),
            year: movie.year,
            rating: movie.rating,
            runtime: movie.runtime,
            genres: Some(movie.genres.clone()),
            language: movie.language.clone(),
            video_codec: variant.video_codec.clone(),
            audio_channels: variant.audio_channels.clone(),
            source_type: variant.source_type.clone(),
            summary: movie.summary.clone(),
            imdb_code: movie.imdb_code.clone(),
            mpa_rating: movie.mpa_rating.clone(),
            bit_depth: variant.bit_depth.clone(),
            trailer_code: movie.trailer_code.clone(),
            poster_small: movie.poster_small.clone(),
            poster_medium: movie.poster_medium.clone(),
            poster_large: movie.poster_large.clone(),
            backdrop: movie.backdrop.clone(),
        };
        self.play_request(req, cx);
    }

    /// Retry the last playback attempt using the stored CreateStreamRequest.
    fn retry_playback(&mut self, cx: &mut Context<Self>) {
        let req = self.player.lock().last_request.clone();
        if let Some(req) = req {
            self.play_request(req, cx);
        }
    }

    /// Pull out the current player when it is an embedded one whose
    /// window is still open, so the next playback can `loadfile` into
    /// the same mpv window. Everything else is disposed.
    fn take_reusable_player(
        &mut self,
    ) -> Option<std::sync::Arc<playback::embedded::EmbeddedPlayer>> {
        let mut prev = self.player.lock();
        match (prev.mpv.take(), prev.ipc.take()) {
            (Some(Player::Embedded(p)), _) if !p.is_finished() => Some(p),
            (mpv, ipc) => {
                crate::playback::dispose(mpv, ipc);
                None
            }
        }
    }

    /// Play a movie's YouTube trailer in mpv. Uses the direct trailer id
    /// when the catalog has one; otherwise resolves via the server-side
    /// YouTube search (same fallback as the web app).
    fn play_trailer_for(
        &mut self,
        group: &streamx_api::types::SearchResultGroup,
        cx: &mut Context<Self>,
    ) {
        let title = group.title.clone();
        let year = group.year;
        let code = group.trailer_code.clone().filter(|t| !t.is_empty());

        let reuse = self.take_reusable_player();
        {
            let mut prev = self.player.lock();
            *prev = PlayerState::default();
            if let Some(p) = &reuse {
                prev.mpv = Some(Player::Embedded(p.clone()));
                prev.ipc = Some(Control::Embedded(p.clone()));
            }
        }

        let state = self.state.clone();
        let player = self.player.clone();
        let theme = self.theme;
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let id = match code {
                Some(c) => Ok(c),
                None => {
                    let client = state.client.read().clone();
                    let q = match year {
                        Some(y) => format!("{title} {y} official trailer"),
                        None => format!("{title} official trailer"),
                    };
                    runtime::spawn(async move { client.trailer_search(&q).await })
                        .await
                        .map_err(|e| format!("trailer search failed: {e}"))
                }
            };
            let id = match id {
                Ok(id) => id,
                Err(e) => {
                    state.show_toast(e, ToastKind::Error);
                    let _ = this.update(cx, |_, cx| cx.notify());
                    return;
                }
            };
            let target = playback::PlayTarget::Web {
                url: format!("https://www.youtube.com/watch?v={id}"),
            };

            let launch_target = target.clone();
            let launch_theme = theme;
            let launched = if let Some(p) = reuse {
                let t = launch_target.clone();
                match runtime::spawn(async move { p.play_target(&t).map(|_| p) }).await {
                    Ok(p) => Ok((Player::Embedded(p.clone()), Some(Control::Embedded(p)))),
                    Err(e) => {
                        tracing::warn!("player reuse failed ({e}); launching fresh");
                        runtime::spawn(
                            async move { playback::launch(&launch_target, &launch_theme) },
                        )
                        .await
                    }
                }
            } else {
                runtime::spawn(async move { playback::launch(&launch_target, &launch_theme) }).await
            };
            match launched {
                Ok((instance, control)) => {
                    let _ = this.update(cx, |_, _| crate::playback::reset_dock_icon());
                    let this_icon = this.clone();
                    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
                        for _ in 0..8 {
                            cx.background_executor()
                                .timer(Duration::from_millis(500))
                                .await;
                            if this_icon
                                .update(cx, |_, _| crate::playback::reset_dock_icon())
                                .is_err()
                            {
                                break;
                            }
                        }
                    })
                    .detach();
                    let mut p = player.lock();
                    p.target = Some(target);
                    p.mpv = Some(instance);
                    p.ipc = control;
                }
                Err(e) => {
                    state.show_toast(format!("Trailer playback failed: {e}"), ToastKind::Error);
                }
            }
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Generic path: build a CreateStreamRequest, navigate to Player, then
    /// poll stream_files + resolve + launch mpv. Used by movie variants,
    /// music/music-video tracks, and surround-sound demos.
    fn play_request(
        &mut self,
        req: streamx_api::types::CreateStreamRequest,
        cx: &mut Context<Self>,
    ) {
        // Reuse a live embedded player so the new video replaces the
        // same mpv window; anything else (spawned mpv, closed window)
        // is disposed so we don't leave orphans behind.
        let reuse = self.take_reusable_player();
        {
            let mut prev = self.player.lock();
            *prev = PlayerState::default();
            prev.last_request = Some(req.clone());
            if let Some(p) = &reuse {
                prev.mpv = Some(Player::Embedded(p.clone()));
                prev.ipc = Some(Control::Embedded(p.clone()));
            }
        }
        // Navigate only if we aren't already on the Player page (so
        // Retry from the Player page doesn't push another Player entry
        // onto the nav stack).
        if !matches!(self.state.current_page(), Page::Player) {
            self.state.navigate(Page::Player);
        }

        let state = self.state.clone();
        let player = self.player.clone();
        let theme = self.theme;

        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let create_req = req;
            let expected_title = create_req.title.clone().unwrap_or_default();
            let resp = match runtime::spawn(async move { client.create_stream(&create_req).await })
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    player.lock().error = Some(format!("create_stream failed: {e}"));
                    let _ = this.update(cx, |_, cx| cx.notify());
                    return;
                }
            };
            tracing::info!(
                stream_id = %resp.stream_id,
                returned_title = %resp.title,
                expected_title = %expected_title,
                "create_stream ok"
            );
            player.lock().stream_id = Some(resp.stream_id.clone());

            let mut file_index = 0usize;
            let mut ready = false;
            // Up to 2 minutes of 1s polls for metadata. Rare magnets
            // can take that long to find peers that have the metadata
            // pieces (BEP-9).
            for _ in 0..120 {
                let client = state.client.read().clone();
                let id = resp.stream_id.clone();
                match runtime::spawn(async move { client.stream_files(&id).await }).await {
                    Ok((files, _status)) if !files.is_empty() => {
                        // Prefer video if present, otherwise the largest audio file,
                        // otherwise the first file.
                        let pick = files
                            .iter()
                            .filter(|f| f.is_video)
                            .max_by_key(|f| f.size)
                            .or_else(|| files.iter().filter(|f| f.is_audio).max_by_key(|f| f.size))
                            .or_else(|| files.first());
                        if let Some(f) = pick {
                            file_index = f.index;
                            ready = true;
                        }
                        break;
                    }
                    _ => {
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                    }
                }
            }
            if !ready {
                player.lock().error =
                    Some("stream metadata timeout - server did not resolve torrent".into());
                let _ = this.update(cx, |_, cx| cx.notify());
                return;
            }
            player.lock().file_index = file_index;

            let client = state.client.read().clone();
            let resolve_res = playback::resolve(&state, client, &resp.stream_id, file_index).await;
            let target = match resolve_res {
                Ok(t) => t,
                Err(e) => {
                    player.lock().error = Some(e);
                    let _ = this.update(cx, |_, cx| cx.notify());
                    return;
                }
            };

            // Launch on a tokio worker thread: libmpv's macOS window
            // creation dispatches onto the main queue, so launching from
            // this foreground task can deadlock (audio-only files hit it
            // reliably with force-window=immediate).
            let launch_target = target.clone();
            let launch_theme = theme;
            let launched = if let Some(p) = reuse.clone() {
                let t = launch_target.clone();
                match runtime::spawn(async move { p.play_target(&t).map(|_| p) }).await {
                    Ok(p) => Ok((Player::Embedded(p.clone()), Some(Control::Embedded(p)))),
                    Err(e) => {
                        tracing::warn!("player reuse failed ({e}); launching fresh");
                        runtime::spawn(
                            async move { playback::launch(&launch_target, &launch_theme) },
                        )
                        .await
                    }
                }
            } else {
                runtime::spawn(async move { playback::launch(&launch_target, &launch_theme) }).await
            };
            match launched {
                Ok((instance, control)) => {
                    // mpv steals the Dock icon when its window appears;
                    // take it back now and again once the window exists.
                    let _ = this.update(cx, |_, _| crate::playback::reset_dock_icon());
                    let this_icon = this.clone();
                    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
                        for _ in 0..8 {
                            cx.background_executor()
                                .timer(Duration::from_millis(500))
                                .await;
                            if this_icon
                                .update(cx, |_, _| crate::playback::reset_dock_icon())
                                .is_err()
                            {
                                break;
                            }
                        }
                    })
                    .detach();
                    let socket = match &instance {
                        Player::Spawned(m) => Some(m.socket_path.clone()),
                        Player::Embedded(_) => None,
                    };
                    {
                        let mut p = player.lock();
                        p.target = Some(target);
                        p.mpv = Some(instance);
                        p.ipc = control;
                    }
                    // Spawned fallback: connect IPC once mpv creates its socket.
                    if let Some(socket) = socket {
                        let player_ref = player.clone();
                        runtime::spawn_detached(async move {
                            match MpvIpc::connect(&socket).await {
                                Ok(ipc) => {
                                    player_ref.lock().ipc = Some(Control::Ipc(ipc));
                                }
                                Err(e) => {
                                    tracing::warn!("mpv IPC connect failed: {e}");
                                }
                            }
                        });
                    }
                }
                Err(e) => {
                    player.lock().error = Some(e);
                }
            }
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Play a raw magnet: resolves via `resolve_magnet` first if needed.
    fn play_magnet(
        &mut self,
        magnet: Option<String>,
        title: String,
        api_base: Option<&'static str>,
        detail_url: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(m) = magnet {
            let req = streamx_api::types::CreateStreamRequest {
                magnet_uri: m,
                title: Some(title),
                ..Default::default()
            };
            self.play_request(req, cx);
            return;
        }
        let Some(api_base) = api_base else { return };
        let Some(detail) = detail_url else { return };

        {
            let mut prev = self.player.lock();
            crate::playback::dispose(prev.mpv.take(), prev.ipc.take());
            *prev = PlayerState::default();
        }
        self.state.navigate(Page::Player);

        let state = self.state.clone();
        let player = self.player.clone();
        let this_self = cx.entity();
        cx.spawn(async move |_weak, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let detail_clone = detail.clone();
            let resolved =
                runtime::spawn(async move { client.resolve_magnet(api_base, &detail_clone).await })
                    .await;
            let magnet = match resolved {
                Ok(r) => r.magnet,
                Err(e) => {
                    player.lock().error = Some(format!("resolve_magnet failed: {e}"));
                    this_self.update(cx, |_, cx| cx.notify());
                    return;
                }
            };
            let req = streamx_api::types::CreateStreamRequest {
                magnet_uri: magnet,
                title: Some(title),
                ..Default::default()
            };
            this_self.update(cx, |view, cx| view.play_request(req, cx));
        })
        .detach();
    }

    fn handle_shortcut(&mut self, s: Shortcut, window: &mut Window, cx: &mut Context<Self>) {
        match s {
            Shortcut::Back => {
                // Close drawer first if open; otherwise pop the page stack.
                if *self.state.drawer_open.read() {
                    *self.state.drawer_open.write() = false;
                } else {
                    let _ = self.state.back();
                }
                cx.notify();
            }
            Shortcut::Activate => {
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
                if matches!(self.state.current_page(), Page::Search) {
                    let fh = self.search_input.read(cx).focus_handle(cx);
                    fh.focus(window, cx);
                    cx.notify();
                }
            }
            Shortcut::ToggleMenu => {
                let mut d = self.state.drawer_open.write();
                *d = !*d;
                drop(d);
                cx.notify();
            }
            _ => {}
        }
    }

    // ---------- renderers ----------

    fn login_page_view(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let mode = *self.state.mode.read();
        let server_ok = self.state.server_version.read().is_some();
        let conn_err = self.state.connection_error.read().clone();
        let login_err = self.state.login_error.read().clone();
        let in_flight = *self.state.login_in_flight.read();

        let mode_pill = |label: &'static str, this_mode: Mode| -> gpui::Stateful<gpui::Div> {
            let selected = mode == this_mode;
            div()
                .id(SharedString::from(format!("mode-{}", this_mode.as_str())))
                .px(px(theme.space_3()))
                .py(px(theme.space_2()))
                .rounded(px(theme.radius_md()))
                .bg(if selected {
                    theme.accent()
                } else {
                    theme.bg_elevated()
                })
                .text_color(if selected {
                    theme.fg_on_accent()
                } else {
                    theme.fg_secondary()
                })
                .text_size(px(theme.fs_1()))
                .border_1()
                .border_color(if selected {
                    theme.accent()
                } else {
                    theme.border_default()
                })
                .cursor_pointer()
                .child(div().child(SharedString::from(label)))
                .on_click(cx.listener(move |this, _ev, _w, cx| {
                    this.state.set_mode(this_mode);
                    cx.notify();
                }))
        };

        // Thin client ships later; releases run Embedded only.
        let disabled_pill = div()
            .id("mode-thin-disabled")
            .px(px(theme.space_3()))
            .py(px(theme.space_2()))
            .rounded(px(theme.radius_md()))
            .bg(theme.bg_elevated())
            .text_color(theme.fg_muted())
            .text_size(px(theme.fs_1()))
            .border_1()
            .border_color(theme.border_subtle())
            .child(div().child("Thin client (coming soon)"));
        let mode_row = div()
            .flex()
            .gap(px(theme.space_2()))
            .mb(px(theme.space_2()))
            .child(mode_pill("Embedded (local files)", Mode::Embedded))
            .child(disabled_pill);

        let url_row = if mode == Mode::ThinClient {
            div()
                .flex()
                .flex_col()
                .gap(px(theme.space_1()))
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child("Server URL"),
                )
                .child(self.url_input.clone())
                .into_any_element()
        } else {
            div()
                .text_size(px(theme.fs_1()))
                .text_color(theme.fg_muted())
                .child(SharedString::from(format!(
                    "Embedded server: {}",
                    self.state.server_url.read()
                )))
                .into_any_element()
        };

        let version_line: SharedString = match self.state.server_version.read().clone() {
            Some(v) => SharedString::from(format!("server v{v}")),
            None => SharedString::from(if server_ok {
                "connected"
            } else {
                "server offline"
            }),
        };

        let create = self.login_create_mode;
        let submit_label: SharedString = match (create, in_flight) {
            (true, true) => SharedString::from("Creating account… ⟳"),
            (true, false) => SharedString::from("Create account"),
            (false, true) => SharedString::from("Signing in… ⟳"),
            (false, false) => SharedString::from("Sign in"),
        };

        card(&theme)
            .w(px(480.0))
            .flex()
            .flex_col()
            .gap(px(theme.space_3()))
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child("StreamX Desktop"),
            )
            .child(
                div()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_muted())
                    .child(version_line),
            )
            .child(mode_row)
            .child(url_row)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme.space_1()))
                    .child(
                        div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .child("Username"),
                    )
                    .child(self.username_input.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme.space_1()))
                    .child(
                        div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .child("Password"),
                    )
                    .child(self.password_input.clone()),
            )
            .when(create, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(theme.space_1()))
                        .child(
                            div()
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.fg_muted())
                                .child("Repeat password"),
                        )
                        .child(self.repeat_input.clone()),
                )
            })
            .child(
                primary_button("login-submit", submit_label, &theme).on_click(cx.listener(
                    |this, _ev, _w, cx| {
                        if this.login_create_mode {
                            this.submit_register(cx);
                        } else {
                            this.submit_login(cx);
                        }
                    },
                )),
            )
            .child(
                div()
                    .id("login-mode-toggle")
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.accent_text())
                    .cursor_pointer()
                    .child(if create {
                        "Have an account? Sign in"
                    } else {
                        "New here? Create an account"
                    })
                    .on_click(cx.listener(|this, _ev, _w, cx| {
                        this.login_create_mode = !this.login_create_mode;
                        *this.state.login_error.write() = None;
                        cx.notify();
                    })),
            )
            .when_some(login_err.or(conn_err), |el, e: String| {
                el.child(
                    div()
                        .p(px(theme.space_2()))
                        .rounded(px(theme.radius_sm()))
                        .bg(theme.bg_elevated())
                        .border_1()
                        .border_color(theme.error())
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.error())
                        .child(SharedString::from(e)),
                )
            })
    }

    fn search_page_view(&self, viewport_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let layout = crate::components::tile_layout(viewport_w - 2.0 * theme.space_5());
        let query = self.state.query.read().clone();
        let browse = self.state.browse.read().clone();
        let results = self.state.search_results.read().clone();
        let loading = *self.state.browse_loading.read();
        let searching = *self.state.search_in_flight.read();

        let hint = if searching {
            "searching… ⟳"
        } else if loading {
            "loading…"
        } else {
            "press / to search · Esc back · M menu"
        };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(theme.space_3()))
            .mb(px(theme.space_4()))
            .child(
                div()
                    .flex_1()
                    .max_w(px(480.0 * theme.scale()))
                    .child(self.input_with_clear(self.search_input.clone(), "clear-search", cx)),
            )
            .child(
                div()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_muted())
                    .child(SharedString::from(hint)),
            );

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .child(header);

        if !query.is_empty() {
            root = root.child(
                section_title(
                    SharedString::from(format!("Results for \"{}\"", query)),
                    &theme,
                )
                .mb(px(theme.space_3())),
            );
            if results.is_empty() {
                root = root.child(
                    div()
                        .text_size(px(theme.fs_2()))
                        .text_color(theme.fg_muted())
                        .child(if searching {
                            "Searching…"
                        } else {
                            "No results."
                        }),
                );
            } else {
                root = root.child(
                    virtual_tile_grid(
                        "search-grid",
                        results.clone(),
                        layout,
                        theme,
                        self.state.clone(),
                        cx.entity().downgrade(),
                        false,
                    )
                    .flex_1()
                    .min_h_0(),
                );
            }
        } else {
            // Virtualized vertical list of the 8 sections: only visible
            // rows build tiles, so wheel/trackpad scrolling stays fluid.
            let specs = category_specs();
            let sections: Vec<(crate::state::CategorySpec, Vec<Arc<_>>)> = vec![
                (specs[0].clone(), browse.this_year.clone()),
                (specs[1].clone(), browse.latest.clone()),
                (specs[2].clone(), browse.popular.clone()),
                (specs[3].clone(), browse.top_rated.clone()),
                (specs[4].clone(), browse.action.clone()),
                (specs[5].clone(), browse.comedy.clone()),
                (specs[6].clone(), browse.thriller.clone()),
                (specs[7].clone(), browse.scifi.clone()),
                (specs[8].clone(), browse.horror.clone()),
            ];
            let state = self.state.clone();
            let weak = cx.entity().downgrade();
            let block_h =
                theme.fs_5() * 1.6 + theme.space_2() + layout.total_h + 20.0 + theme.space_4();
            root = root.child(
                gpui::uniform_list(
                    "home-sections",
                    sections.len(),
                    move |range, _window, _cx| {
                        range
                            .map(|i| {
                                let (spec, groups) = &sections[i];
                                home_section_block(
                                    spec,
                                    groups,
                                    layout,
                                    theme,
                                    block_h,
                                    state.clone(),
                                    weak.clone(),
                                )
                            })
                            .collect()
                    },
                )
                .flex_1()
                .min_h_0(),
            );
        }

        root
    }

    /// Category drill-down: an infinitely scrolling grid of tiles for
    /// one home section, mirroring the web app's /browse/:category page.
    fn category_page_view(&self, viewport_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let layout = crate::components::tile_layout(viewport_w - 2.0 * theme.space_5());
        let title = self
            .state
            .category
            .read()
            .as_ref()
            .map(|s| s.title)
            .unwrap_or("Browse");
        let items = self.state.category_items.read().clone();
        let loading = *self.state.category_loading.read();
        let done = *self.state.category_done.read();

        let grid = virtual_tile_grid(
            "cat-grid",
            items.clone(),
            layout,
            theme,
            self.state.clone(),
            cx.entity().downgrade(),
            true,
        );

        let footer: &'static str = if loading {
            "Loading more…"
        } else if done && items.is_empty() {
            "Nothing here."
        } else if done {
            "That's everything."
        } else {
            ""
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .gap(px(theme.space_2()))
            .child(self.back_hint(cx))
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child(SharedString::from(title)),
            )
            .child(grid.flex_1().min_h_0())
            .child(
                div()
                    .py(px(theme.space_2()))
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_muted())
                    .child(footer),
            )
    }

    fn player_page_view(&self, viewport_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        // Hero poster: ~22% of the window, clamped so it is prominent on
        // big displays without swallowing small ones.
        let hero_w = (viewport_w * 0.22).clamp(200.0, 460.0);
        let hero_h = hero_w * 1.5;
        let p = self.player.lock();
        let stream_id = p.stream_id.clone().unwrap_or_default();
        let target = p.target.as_ref().map(|t| t.display()).unwrap_or_default();
        let error = p.error.clone();
        let playing = p.mpv.is_some();
        let has_ipc = p.ipc.is_some();
        let snap = p.snapshot.clone();
        let file_index = p.file_index;
        let torrent = p.torrent.clone();
        drop(p);

        // Hero metadata comes from the actual play request, never from
        // the last browsed movie — playing a surround demo or a music
        // track must not wear a stale movie's title and poster.
        let req = self.player.lock().last_request.clone();
        let backdrop_url = req.as_ref().and_then(|r| {
            r.backdrop
                .clone()
                .or_else(|| r.poster_large.clone())
                .or_else(|| r.poster_medium.clone())
        });
        let poster_url = req.as_ref().and_then(|r| {
            r.poster_medium
                .clone()
                .or_else(|| r.poster_large.clone())
                .or_else(|| r.poster_small.clone())
        });
        let year = req.as_ref().and_then(|r| r.year);
        let rating = req.as_ref().and_then(|r| r.rating);
        let runtime = req.as_ref().and_then(|r| r.runtime);
        let genres = req
            .as_ref()
            .and_then(|r| r.genres.clone())
            .unwrap_or_default();
        let summary = req.as_ref().and_then(|r| r.summary.clone());
        let title = req
            .and_then(|r| r.title)
            .unwrap_or_else(|| "Unknown title".into());

        // Content sits in front of a full-bleed backdrop with a dark
        // overlay for readability. Matches web Player.tsx poster behaviour.
        let mut content = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .p(px(theme.space_5()))
            .flex()
            .flex_col()
            .gap(px(theme.space_3()));

        content = content.child(self.back_hint(cx)).child(
            frost_card(&theme)
                .flex()
                .gap(px(theme.space_4()))
                .items_start()
                .child({
                    // Poster thumbnail via the shared poster cache.
                    let mut poster = div()
                        .w(px(hero_w))
                        .h(px(hero_h))
                        .rounded(px(theme.radius_md()))
                        .overflow_hidden()
                        .bg(theme.bg_panel())
                        .border_1()
                        .border_color(theme.border_subtle())
                        .flex_shrink_0();
                    if let Some(url) = poster_url {
                        let src = crate::components::poster_image_source(&url);
                        poster = poster.child(
                            img(src)
                                .w(px(hero_w))
                                .h(px(hero_h))
                                .object_fit(ObjectFit::Cover),
                        );
                    }
                    poster
                })
                .child({
                    let mut meta = div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(theme.space_2()))
                        .child(
                            div()
                                .text_size(px(theme.fs_6()))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(theme.fg_primary())
                                .child(SharedString::from(title.clone())),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(theme.space_3()))
                                .text_size(px(theme.fs_2()))
                                .text_color(theme.fg_secondary())
                                .child(SharedString::from(
                                    year.map(|y| y.to_string()).unwrap_or_default(),
                                ))
                                .child(SharedString::from(
                                    rating.map(|r| format!("★ {:.1}", r)).unwrap_or_default(),
                                ))
                                .child(SharedString::from(
                                    runtime.map(|r| format!("{} min", r)).unwrap_or_default(),
                                ))
                                .child(SharedString::from(if genres.is_empty() {
                                    String::new()
                                } else {
                                    genres.join(" · ")
                                })),
                        );
                    if let Some(s) = summary.as_ref() {
                        meta = meta.child(
                            div()
                                .max_w(px(720.0))
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.fg_secondary())
                                .child(SharedString::from(s.clone())),
                        );
                    }
                    meta
                }),
        );

        // Torrent status card (progress, peers, speed, size).
        if let Some(ts) = torrent {
            let fmt_speed = |bps: f64| -> String {
                if bps >= 1_000_000.0 {
                    format!("{:.1} MB/s", bps / 1_000_000.0)
                } else if bps >= 1_000.0 {
                    format!("{:.0} KB/s", bps / 1_000.0)
                } else {
                    format!("{:.0} B/s", bps)
                }
            };
            let fmt_size = |bytes: u64| -> String {
                let b = bytes as f64;
                if b >= 1_000_000_000.0 {
                    format!("{:.1} GB", b / 1_000_000_000.0)
                } else if b >= 1_000_000.0 {
                    format!("{:.0} MB", b / 1_000_000.0)
                } else if b >= 1_000.0 {
                    format!("{:.0} KB", b / 1_000.0)
                } else {
                    format!("{} B", bytes)
                }
            };
            let status_color = match ts.status.as_str() {
                "complete" | "ready" => theme.success(),
                "downloading" | "transcoding" | "initializing" => theme.accent(),
                "paused" => theme.fg_muted(),
                _ => theme.fg_muted(),
            };
            let progress_pct = ts.progress.clamp(0.0, 100.0);
            content = content.child(
                frost_card(&theme)
                    .flex()
                    .flex_col()
                    .gap(px(theme.space_2()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(theme.space_3()))
                            .child(
                                div()
                                    .px(px(theme.space_2()))
                                    .py(px(2.0))
                                    .rounded(px(theme.radius_sm()))
                                    .text_size(px(theme.fs_1()))
                                    .text_color(status_color)
                                    .border_1()
                                    .border_color(status_color)
                                    .child(SharedString::from(ts.status.clone())),
                            )
                            .child(
                                div()
                                    .text_size(px(theme.fs_1()))
                                    .text_color(theme.fg_secondary())
                                    .child(SharedString::from(format!(
                                        "{:.1}% · {} peers · {} · {}",
                                        progress_pct,
                                        ts.peers,
                                        fmt_speed(ts.speed_bps),
                                        fmt_size(ts.file_size),
                                    ))),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(4.0))
                            .rounded(px(theme.radius_sm()))
                            .bg(theme.bg_elevated())
                            .child(
                                div()
                                    .h(px(4.0))
                                    .w(gpui::relative(progress_pct / 100.0))
                                    .rounded(px(theme.radius_sm()))
                                    .bg(theme.accent()),
                            ),
                    )
                    .when(!ts.file_name.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.fg_muted())
                                .child(SharedString::from(ts.file_name.clone())),
                        )
                    }),
            );
        }

        if let Some(err) = error {
            content = content
                .child(
                    div()
                        .p(px(theme.space_3()))
                        .rounded(px(theme.radius_md()))
                        .border_1()
                        .border_color(theme.error())
                        .text_color(theme.error())
                        .text_size(px(theme.fs_2()))
                        .child(SharedString::from(err)),
                )
                .child(
                    primary_button("player-retry-err", "↻ Retry", &theme)
                        .on_click(cx.listener(|this, _ev, _w, cx| this.retry_playback(cx))),
                );
        } else if !target.is_empty() {
            let status = if playing {
                "Playing in mpv window"
            } else {
                "mpv exited"
            };
            let paused_label = if snap.paused { "Paused" } else { "Playing" };

            let fmt_time = |s: f64| -> String {
                let total = s.max(0.0) as u64;
                format!(
                    "{:02}:{:02}:{:02}",
                    total / 3600,
                    (total / 60) % 60,
                    total % 60
                )
            };
            let time_line = SharedString::from(format!(
                "{} / {}",
                fmt_time(snap.time_pos),
                fmt_time(snap.duration),
            ));
            let progress = if snap.duration > 0.0 {
                (snap.time_pos / snap.duration).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };

            content = content
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.space_3()))
                        .child(
                            div()
                                .text_size(px(theme.fs_2()))
                                .text_color(if playing {
                                    theme.accent()
                                } else {
                                    theme.fg_muted()
                                })
                                .child(SharedString::from(status)),
                        )
                        .child(
                            primary_button(
                                "player-retry",
                                if playing { "↻ Restart" } else { "▶ Play" },
                                &theme,
                            )
                            .on_click(cx.listener(|this, _ev, _w, cx| this.retry_playback(cx))),
                        ),
                )
                .when(has_ipc, |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(theme.space_2()))
                            .mt(px(theme.space_2()))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(theme.space_2()))
                                    .child(
                                        primary_button(
                                            "player-toggle-pause",
                                            if snap.paused { "Play" } else { "Pause" },
                                            &theme,
                                        )
                                        .on_click(
                                            cx.listener(|this, _ev, _w, _cx| {
                                                let ipc = this.player.lock().ipc.clone();
                                                if let Some(ipc) = ipc {
                                                    runtime::spawn_detached(async move {
                                                        let _ = ipc.toggle_pause().await;
                                                    });
                                                }
                                            }),
                                        ),
                                    )
                                    .child(
                                        primary_button("player-seek-back", "-10s", &theme)
                                            .on_click(cx.listener(|this, _ev, _w, _cx| {
                                                let ipc = this.player.lock().ipc.clone();
                                                if let Some(ipc) = ipc {
                                                    runtime::spawn_detached(async move {
                                                        let _ = ipc.seek(-10.0, true).await;
                                                    });
                                                }
                                            })),
                                    )
                                    .child(
                                        primary_button("player-seek-fwd", "+30s", &theme).on_click(
                                            cx.listener(|this, _ev, _w, _cx| {
                                                let ipc = this.player.lock().ipc.clone();
                                                if let Some(ipc) = ipc {
                                                    runtime::spawn_detached(async move {
                                                        let _ = ipc.seek(30.0, true).await;
                                                    });
                                                }
                                            }),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(theme.fs_1()))
                                            .text_color(theme.fg_muted())
                                            .child(SharedString::from(paused_label)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(theme.fs_1()))
                                            .text_color(theme.fg_muted())
                                            .child(time_line),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .h(px(4.0))
                                    .rounded(px(theme.radius_sm()))
                                    .bg(theme.bg_elevated())
                                    .child(
                                        div()
                                            .h(px(4.0))
                                            .w(gpui::relative(progress))
                                            .rounded(px(theme.radius_sm()))
                                            .bg(theme.accent()),
                                    ),
                            ),
                    )
                })
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child(SharedString::from(format!("stream: {stream_id}"))),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child(SharedString::from(format!("file index: {file_index}"))),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_secondary())
                        .child(SharedString::from(target)),
                );
        } else {
            content = content.child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_muted())
                    .child("Preparing stream…"),
            );
        }

        // Layered backdrop: backdrop image (if any), dark overlay, content.
        let mut outer = div()
            .id("player-root")
            .size_full()
            .relative()
            .bg(theme.bg_app());
        if let Some(url) = backdrop_url {
            let src = crate::components::poster_image_source(&url);
            outer = outer.child(
                img(src)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            );
        }
        outer = outer
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .bg(theme.bg_overlay()),
            )
            .child(content);
        outer
    }

    fn movie_page_view(&self, viewport_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let hero_w = (viewport_w * 0.18).clamp(180.0, 380.0);
        let hero_h = hero_w * 1.5;
        let movie = self.state.selected_movie.read().clone();
        let Some(m) = movie else {
            return movie_page(&self.state, &theme).into_any_element();
        };

        // Mirrors the web Movie page: full-bleed dimmed backdrop, poster
        // on the left, title + badges + genres + summary on the right,
        // then the variant cards.
        let backdrop_url = m
            .backdrop
            .clone()
            .or_else(|| m.poster_large.clone())
            .or_else(|| m.poster.clone());
        let poster_url = m
            .poster_large
            .clone()
            .or_else(|| m.poster.clone())
            .or_else(|| m.poster_medium.clone())
            .or_else(|| m.poster_small.clone());

        let badge_row = {
            let mut row = div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap(px(theme.space_2()));
            if let Some(r) = m.rating.filter(|r| *r > 0.0) {
                row = row.child(crate::components::badge(
                    SharedString::from(format!("★ {r:.1}")),
                    theme.favourite(),
                    &theme,
                ));
            }
            if let Some(rt) = m.runtime.filter(|r| *r > 0) {
                row = row.child(crate::components::badge(
                    SharedString::from(format!("{} min", rt)),
                    theme.fg_secondary(),
                    &theme,
                ));
            }
            if let Some(mpa) = m.mpa_rating.clone().filter(|s| !s.is_empty()) {
                row = row.child(crate::components::badge(
                    SharedString::from(mpa),
                    theme.fg_secondary(),
                    &theme,
                ));
            }
            if let Some(lang) = m.language.clone().filter(|l| l != "en" && !l.is_empty()) {
                row = row.child(crate::components::badge(
                    SharedString::from(lang.to_uppercase()),
                    theme.fg_secondary(),
                    &theme,
                ));
            }
            row
        };

        let genre_row = {
            let mut row = div().flex().flex_wrap().gap(px(theme.space_1()));
            for g in &m.genres {
                row = row.child(crate::components::badge(
                    SharedString::from(g.clone()),
                    theme.accent(),
                    &theme,
                ));
            }
            row
        };

        let mut meta_col = div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(theme.space_2()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme.space_2()))
                    .child(
                        div()
                            .text_size(px(theme.fs_6()))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.fg_primary())
                            .child(SharedString::from(m.title.clone())),
                    )
                    .when_some(m.year, |el, y| {
                        el.child(
                            div()
                                .text_size(px(theme.fs_3()))
                                .text_color(theme.fg_muted())
                                .child(SharedString::from(format!("({y})"))),
                        )
                    }),
            )
            .child(badge_row);
        if !m.genres.is_empty() {
            meta_col = meta_col.child(genre_row);
        }
        if let Some(s) = m.summary.clone().filter(|s| !s.is_empty()) {
            meta_col = meta_col.child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_secondary())
                    .max_w(px(720.0))
                    .child(SharedString::from(s)),
            );
        }

        let has_direct_trailer = m
            .trailer_code
            .as_deref()
            .map(|t| !t.is_empty())
            .unwrap_or(false);

        let mut hero = div().flex().items_start().gap(px(theme.space_4()));
        if let Some(url) = poster_url {
            let src = crate::components::poster_image_source(&url);
            let m_trailer = m.clone();
            hero = hero.child(
                div()
                    .relative()
                    .w(px(hero_w))
                    .h(px(hero_h))
                    .flex_shrink_0()
                    .child(
                        div()
                            .w(px(hero_w))
                            .h(px(hero_h))
                            .rounded(px(theme.radius_md()))
                            .overflow_hidden()
                            .bg(theme.bg_panel())
                            .border_1()
                            .border_color(theme.border_subtle())
                            .child(
                                img(src)
                                    .w(px(hero_w))
                                    .h(px(hero_h))
                                    .object_fit(ObjectFit::Cover),
                            ),
                    )
                    .child(crate::components::trailer_overlay(
                        "movie-poster-trailer",
                        has_direct_trailer,
                        36.0,
                        Box::new(cx.listener(move |this, _ev, _w, cx| {
                            this.play_trailer_for(m_trailer.as_ref(), cx);
                        })),
                    )),
            );
        }
        {
            // Same red/grey affordance as the web app's Watch Trailer button.
            let m_trailer = m.clone();
            let color = if has_direct_trailer {
                theme.error()
            } else {
                theme.fg_muted()
            };
            meta_col = meta_col.child(
                div().flex().child(
                    div()
                        .id("movie-watch-trailer")
                        .px(px(theme.space_3()))
                        .py(px(theme.space_2()))
                        .rounded(px(theme.radius_md()))
                        .border_1()
                        .border_color(color)
                        .text_color(color)
                        .text_size(px(theme.fs_2()))
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.8))
                        .on_click(cx.listener(move |this, _ev, _w, cx| {
                            this.play_trailer_for(m_trailer.as_ref(), cx);
                        }))
                        .child("▶ Watch Trailer"),
                ),
            );
        }
        hero = hero.child(meta_col);

        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .flex()
            .flex_col()
            .gap(px(theme.space_2()));

        root = root.child(self.back_hint(cx)).child(hero).child(
            div()
                .text_size(px(theme.fs_5()))
                .font_weight(gpui::FontWeight::BOLD)
                .mt(px(theme.space_4()))
                .text_color(theme.fg_primary())
                .child("Available Qualities"),
        );

        let downloads = self.state.downloads.read().clone();
        for (i, v) in m.variants.iter().enumerate() {
            let quality = v.quality.clone().unwrap_or_else(|| "?".into());
            let codec = v.video_codec.clone().unwrap_or_default();
            let audio = v.audio_channels.clone().unwrap_or_default();
            let size = v.size.clone();
            let seeds = v.seeds;
            let leeches = v.leeches;
            let hash = info_hash_from_magnet(&v.magnet);
            let dl = hash.as_ref().and_then(|h| {
                downloads
                    .iter()
                    .find(|d| d.info_hash.eq_ignore_ascii_case(h))
                    .cloned()
            });
            let dl_status = dl.as_ref().map(|d| d.status.clone());
            let dl_progress = dl.as_ref().map(|d| d.progress).unwrap_or(0.0);
            let hash_s = hash.clone().unwrap_or_default();
            let confirming = !hash_s.is_empty()
                && self.state.confirm_delete.read().as_deref() == Some(hash_s.as_str());

            let row = div()
                .flex()
                .items_center()
                .gap(px(theme.space_3()))
                .p(px(theme.space_3()))
                .rounded(px(theme.radius_md()))
                .bg(theme.bg_surface())
                .border_1()
                .border_color(theme.border_subtle())
                .child(
                    div()
                        .px(px(theme.space_2()))
                        .py(px(2.0))
                        .rounded(px(theme.radius_sm()))
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.accent())
                        .border_1()
                        .border_color(theme.accent())
                        .child(SharedString::from(quality)),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child(SharedString::from(codec)),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child(SharedString::from(audio)),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_primary())
                        .child(SharedString::from(size)),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.success())
                        .child(SharedString::from(format!("↑{seeds}"))),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.error())
                        .child(SharedString::from(format!("↓{leeches}"))),
                )
                .child(
                    primary_button(SharedString::from(format!("play-{i}")), "Play", &theme)
                        .on_click(cx.listener(move |this, _ev, _w, cx| {
                            this.start_playback(i, cx);
                        })),
                );

            // Download lifecycle controls, driven by the DB status:
            // none → Download; downloading → progress + Stop; paused/error →
            // Resume; complete → Downloaded. Any known download can be
            // deleted (files + records) behind a two-click confirmation.
            let mut controls = div().flex().items_center().gap(px(theme.space_2()));
            match dl_status.as_deref() {
                None => {
                    controls = controls.child(
                        secondary_button(
                            SharedString::from(format!("start-dl-{i}")),
                            "⬇ Download",
                            &theme,
                        )
                        .on_click(cx.listener(move |this, _ev, _w, cx| {
                            this.start_background_download(i, cx);
                        })),
                    );
                }
                Some("complete") => {
                    controls = controls.child(crate::components::badge(
                        "Downloaded",
                        theme.success(),
                        &theme,
                    ));
                }
                Some("paused") | Some("error") => {
                    let label = if dl_status.as_deref() == Some("error") {
                        "Failed".to_string()
                    } else {
                        format!("Paused · {dl_progress:.0}%")
                    };
                    controls = controls.child(crate::components::badge(
                        SharedString::from(label),
                        theme.fg_secondary(),
                        &theme,
                    ));
                    let h = hash_s.clone();
                    controls = controls.child(
                        secondary_button(
                            SharedString::from(format!("resume-dl-{i}")),
                            "Resume",
                            &theme,
                        )
                        .on_click(cx.listener(move |this, _ev, _w, cx| {
                            this.resume_background_download(h.clone(), cx);
                        })),
                    );
                }
                _ => {
                    controls = controls.child(crate::components::badge(
                        SharedString::from(format!("{dl_progress:.0}%")),
                        theme.accent(),
                        &theme,
                    ));
                    let h = hash_s.clone();
                    controls = controls.child(
                        secondary_button(
                            SharedString::from(format!("stop-dl-{i}")),
                            "Stop",
                            &theme,
                        )
                        .on_click(cx.listener(move |this, _ev, _w, cx| {
                            this.cancel_background_download(h.clone(), cx);
                        })),
                    );
                }
            }
            if dl_status.is_some() && !hash_s.is_empty() {
                let h = hash_s.clone();
                let label = if confirming {
                    "Confirm delete"
                } else {
                    "Delete"
                };
                controls = controls.child(
                    crate::components::danger_button(
                        SharedString::from(format!("delete-dl-{i}")),
                        label,
                        &theme,
                    )
                    .on_click(cx.listener(move |this, _ev, _w, cx| {
                        if confirming {
                            *this.state.confirm_delete.write() = None;
                            this.delete_download(h.clone(), cx);
                        } else {
                            *this.state.confirm_delete.write() = Some(h.clone());
                            cx.notify();
                        }
                    })),
                );
            }
            let row = row.child(controls);

            root = root.child(row);
        }

        // Layering like the web Movie page: backdrop image + dim overlay
        // pinned to the FULL viewport (they never end mid-screen, however
        // short the variant list is), with the content scrolling over
        // them — CSS background-attachment: fixed, in gpui terms.
        let mut outer = div()
            .id("movie-root")
            .size_full()
            .relative()
            .bg(theme.bg_app());
        if let Some(url) = backdrop_url {
            let src = crate::components::poster_image_source(&url);
            outer = outer.child(
                img(src)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            );
            outer = outer.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .bg(theme.bg_overlay()),
            );
        }
        outer
            .child(
                div()
                    .id("movie-scroll")
                    .size_full()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(root),
            )
            .into_any_element()
    }

    /// Kick off an async data load for a page if data isn't already
    /// cached or in flight.
    fn ensure_loaded_for(&self, page: Page) {
        let state = self.state.clone();
        match page {
            Page::Search => {
                if state.browse.read().latest.is_empty() && !*state.browse_loading.read() {
                    runtime::spawn_detached(async move { load_browse(&state).await });
                }
            }
            Page::History => {
                if !*state.history_loading.read() {
                    runtime::spawn_detached(async move { load_history(&state).await });
                }
            }
            Page::Downloads => {
                if !*state.downloads_loading.read() {
                    runtime::spawn_detached(async move { load_downloads(&state).await });
                }
            }
            Page::Favourites => {
                if !*state.favourites_loading.read() {
                    runtime::spawn_detached(async move { load_favourites(&state).await });
                }
            }
            Page::MusicSearch => {
                if state.music_results.read().is_empty() && !*state.music_loading.read() {
                    runtime::spawn_detached(async move { load_music(&state).await });
                }
            }
            Page::MusicVideoSearch => {
                if state.music_video_results.read().is_empty() && !*state.music_video_loading.read()
                {
                    runtime::spawn_detached(async move { load_music_videos(&state).await });
                }
            }
            Page::TvSearch if state.tv_results.read().is_empty() && !*state.tv_loading.read() => {
                runtime::spawn_detached(async move { load_tv(&state).await });
            }
            _ => {}
        }
    }

    fn history_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let items = self.state.history.read().clone();
        let loading = *self.state.history_loading.read();

        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .gap(px(theme.space_2()))
            .child(self.back_hint(cx))
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child("History"),
            );

        if loading {
            root = root.child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_muted())
                    .child("Loading history…"),
            );
        } else if items.is_empty() {
            root = root.child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_muted())
                    .child("Nothing watched yet."),
            );
        } else {
            let mut list = div()
                .flex()
                .flex_col()
                .gap(px(theme.space_2()))
                .mt(px(theme.space_3()));
            for item in items {
                let watched = item.watched_seconds.unwrap_or(0);
                let total = item.duration_seconds.unwrap_or(0);
                let progress = if total > 0 {
                    (watched as f32 / total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let meta = format!(
                    "{} · watched {}",
                    item.year
                        .map(|y| y.to_string())
                        .unwrap_or_else(|| "—".into()),
                    item.watched_at,
                );
                let _ = cx;
                list = list.child(
                    div()
                        .p(px(theme.space_3()))
                        .rounded(px(theme.radius_md()))
                        .bg(theme.bg_surface())
                        .border_1()
                        .border_color(theme.border_subtle())
                        .flex()
                        .flex_col()
                        .gap(px(theme.space_1()))
                        .child(
                            div()
                                .text_size(px(theme.fs_2()))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.fg_primary())
                                .child(SharedString::from(item.title.clone())),
                        )
                        .child(
                            div()
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.fg_muted())
                                .child(SharedString::from(meta)),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(4.0))
                                .rounded(px(theme.radius_sm()))
                                .bg(theme.bg_elevated())
                                .child(
                                    div()
                                        .h(px(4.0))
                                        .w(gpui::relative(progress))
                                        .rounded(px(theme.radius_sm()))
                                        .bg(theme.accent()),
                                ),
                        ),
                );
            }
            root = root.child(list);
        }

        root
    }

    /// Start a background download for the selected movie's variant:
    /// create the stream with full metadata, then pin it so it keeps
    /// downloading after the app closes.
    fn start_background_download(&mut self, variant_idx: usize, cx: &mut Context<Self>) {
        let movie = match self.state.selected_movie.read().clone() {
            Some(m) => m,
            None => return,
        };
        let variant = match movie.variants.get(variant_idx) {
            Some(v) => v.clone(),
            None => return,
        };
        let req = streamx_api::types::CreateStreamRequest {
            magnet_uri: variant.magnet.clone(),
            file_index: None,
            poster_url: movie.poster_large.clone().or_else(|| movie.poster.clone()),
            title: Some(movie.title.clone()),
            year: movie.year,
            rating: movie.rating,
            runtime: movie.runtime,
            genres: Some(movie.genres.clone()),
            language: movie.language.clone(),
            video_codec: variant.video_codec.clone(),
            audio_channels: variant.audio_channels.clone(),
            source_type: variant.source_type.clone(),
            summary: movie.summary.clone(),
            imdb_code: movie.imdb_code.clone(),
            mpa_rating: movie.mpa_rating.clone(),
            bit_depth: variant.bit_depth.clone(),
            trailer_code: movie.trailer_code.clone(),
            poster_small: movie.poster_small.clone(),
            poster_medium: movie.poster_medium.clone(),
            poster_large: movie.poster_large.clone(),
            backdrop: movie.backdrop.clone(),
        };
        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let create_req = req;
            let result = runtime::spawn(async move {
                let resp = client.create_stream(&create_req).await?;
                client.pin_download(&resp.stream_id).await?;
                Ok::<_, streamx_api::client::ClientError>(resp.stream_id)
            })
            .await;
            match result {
                Ok(_) => state.show_toast("Download started.", ToastKind::Success),
                Err(e) => state.show_toast(format!("Download failed: {e}"), ToastKind::Error),
            }
            load_downloads(&state).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    fn cancel_background_download(&mut self, hash: String, cx: &mut Context<Self>) {
        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let h = hash.clone();
            let res = runtime::spawn(async move { client.unpin_download(&h).await }).await;
            match res {
                Ok(()) => state.show_toast("Download cancelled.", ToastKind::Info),
                Err(e) => state.show_toast(format!("Cancel failed: {e}"), ToastKind::Error),
            }
            load_downloads(&state).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    fn resume_background_download(&mut self, hash: String, cx: &mut Context<Self>) {
        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let h = hash.clone();
            let res = runtime::spawn(async move { client.pin_download(&h).await }).await;
            match res {
                Ok(()) => state.show_toast("Download resumed.", ToastKind::Success),
                Err(e) => state.show_toast(format!("Resume failed: {e}"), ToastKind::Error),
            }
            load_downloads(&state).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Fetch the rebuilt movie group for a download and open the
    /// standard movie page, whatever the download state.
    fn open_download_movie(&mut self, hash: String, cx: &mut Context<Self>) {
        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let h = hash.clone();
            let res = runtime::spawn(async move { client.download_movie(&h).await }).await;
            match res {
                Ok(group) => {
                    *state.selected_movie.write() = Some(std::sync::Arc::new(group));
                    state.navigate(Page::Movie);
                }
                Err(e) => state.show_toast(format!("Open failed: {e}"), ToastKind::Error),
            }
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    fn delete_download(&mut self, hash: String, cx: &mut Context<Self>) {
        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let h = hash.clone();
            let res = runtime::spawn(async move { client.delete_stream(&h).await }).await;
            match res {
                Ok(()) => state.show_toast("Download deleted.", ToastKind::Success),
                Err(e) => state.show_toast(format!("Delete failed: {e}"), ToastKind::Error),
            }
            load_downloads(&state).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    fn logs_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        // Snapshot once per frame; the ring is capped server-side, and
        // the list below is virtualized, so a full buffer costs one
        // bounded clone per repaint of this page only.
        let lines = std::sync::Arc::new(self.state.logs.recent());
        let count = lines.len();

        let header = div()
            .flex()
            .items_center()
            .gap(px(theme.space_2()))
            .child(
                div()
                    .flex_1()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child("Logs"),
            )
            .child(
                div()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_muted())
                    .child(SharedString::from(format!("{count} lines"))),
            )
            .child({
                let lines = lines.clone();
                secondary_button("logs-copy", "Copy", &theme).on_click(cx.listener(
                    move |this, _ev, _w, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(lines.join("\n")));
                        this.state.show_toast("Logs copied.", ToastKind::Success);
                    },
                ))
            })
            .child(
                secondary_button("logs-clear", "Clear", &theme).on_click(cx.listener(
                    move |this, _ev, _w, cx| {
                        this.state.logs.clear();
                        this.logs_follow.set(true);
                        this.state.mark_dirty();
                        cx.notify();
                    },
                )),
            );

        let widest = (0..count).max_by_key(|i| lines[*i].len());
        let list_lines = lines.clone();
        let list = gpui::uniform_list("logs-list", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let line = &list_lines[i];
                    // Level from the formatted line drives the color;
                    // errors and warnings must stand out when scanning.
                    let color = if line.contains("ERROR") {
                        theme.error()
                    } else if line.contains("WARN") {
                        theme.favourite()
                    } else if line.contains("DEBUG") || line.contains("TRACE") {
                        theme.fg_muted()
                    } else {
                        theme.fg_secondary()
                    };
                    div()
                        .px(px(theme.space_2()))
                        .py(px(1.0))
                        .text_size(px(theme.fs_1()))
                        .text_color(color)
                        .whitespace_nowrap()
                        .child(SharedString::from(line.clone()))
                        .into_any_element()
                })
                .collect()
        })
        .track_scroll(&self.logs_scroll)
        .with_width_from_item(widest)
        .with_horizontal_sizing_behavior(gpui::ListHorizontalSizingBehavior::Unconstrained)
        .h_full();

        // Scroll position indicator (wheel/trackpad scrolls the list).
        let thumb = {
            let sc = self.logs_scroll.0.borrow();
            sc.last_item_size.and_then(|sz| {
                if sz.contents.height <= sz.item.height {
                    return None;
                }
                let viewport = f32::from(sz.item.height).max(1.0);
                let content = f32::from(sz.contents.height).max(1.0);
                let offset_y = (-f32::from(sc.base_handle.offset().y)).max(0.0);
                let frac_h = (viewport / content).clamp(0.05, 1.0);
                let frac_top = (offset_y / content).clamp(0.0, 1.0 - frac_h);
                Some((frac_top, frac_h))
            })
        };

        div()
            .w_full()
            .h_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .gap(px(theme.space_2()))
            .child(self.back_hint(cx))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .rounded(px(theme.radius_md()))
                    .bg(theme.bg_surface())
                    .border_1()
                    .border_color(theme.border_subtle())
                    .overflow_hidden()
                    .child(list)
                    .when_some(thumb, |el, (frac_top, frac_h)| {
                        el.child(
                            div()
                                .absolute()
                                .right(px(2.0))
                                .top(gpui::relative(frac_top))
                                .h(gpui::relative(frac_h))
                                .w(px(4.0))
                                .rounded(px(2.0))
                                .bg(theme.border_strong()),
                        )
                    }),
            )
    }

    /// Wrap a search input with a small x clear button pinned inside
    /// its right edge, shown only while the field has text. Clearing
    /// restores the page's browse view (movies instantly; other domains
    /// via the debounced empty-query fire).
    fn input_with_clear(
        &self,
        input: Entity<TextInput>,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = self.theme;
        let has_text = !input.read(cx).value().is_empty();
        let mut wrap = div().relative().w_full().child(input.clone());
        if has_text {
            let clear_input = input;
            wrap = wrap.child(
                div()
                    .absolute()
                    .right(px(6.0))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id(SharedString::from(id))
                            .w(px(20.0 * theme.scale()))
                            .h(px(20.0 * theme.scale()))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.bg_elevated()).text_color(theme.fg_primary()))
                            .on_mouse_down(MouseButton::Left, |_ev, _w, cx| {
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(move |this, _ev, _w, cx| {
                                cx.stop_propagation();
                                clear_input.update(cx, |i, _| i.set_value(""));
                                if id == "clear-search" {
                                    *this.state.query.write() = String::new();
                                    *this.state.search_results.write() = Vec::new();
                                    this.state.mark_dirty();
                                }
                                cx.notify();
                            }))
                            .child("\u{2715}"),
                    ),
            );
        }
        wrap
    }

    fn downloads_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let items = self.state.downloads.read().clone();
        let loading = *self.state.downloads_loading.read();
        let is_admin = self
            .state
            .user
            .read()
            .as_ref()
            .map(|u| u.is_admin)
            .unwrap_or(false);

        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .gap(px(theme.space_2()))
            .child(self.back_hint(cx))
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child("Downloads"),
            );

        if items.is_empty() {
            root = root.child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_muted())
                    .child(if loading {
                        "Loading downloads…"
                    } else {
                        "No downloads yet. Use the Download button on a movie."
                    }),
            );
            return root;
        }

        let fmt_speed = |bps: f64| -> String {
            if bps >= 1_000_000.0 {
                format!("{:.1} MB/s", bps / 1_000_000.0)
            } else if bps >= 1_000.0 {
                format!("{:.0} KB/s", bps / 1_000.0)
            } else {
                format!("{:.0} B/s", bps)
            }
        };
        let fmt_size = |bytes: u64| -> String {
            let b = bytes as f64;
            if b >= 1_000_000_000.0 {
                format!("{:.1} GB", b / 1_000_000_000.0)
            } else if b >= 1_000_000.0 {
                format!("{:.0} MB", b / 1_000_000.0)
            } else {
                format!("{} B", bytes)
            }
        };

        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(theme.space_2()))
            .mt(px(theme.space_3()));
        for (i, dl) in items.iter().enumerate() {
            let complete = dl.status == "complete";
            let active = dl.status == "downloading" || dl.status == "initializing";
            let status_color = if complete {
                theme.success()
            } else if active {
                theme.accent()
            } else {
                theme.fg_muted()
            };
            let name = if dl.title.is_empty() {
                if dl.file_name.is_empty() {
                    dl.info_hash.clone()
                } else {
                    dl.file_name.clone()
                }
            } else {
                dl.title.clone()
            };
            let progress = (dl.progress / 100.0).clamp(0.0, 1.0) as f32;
            let hash = dl.info_hash.clone();

            let mut meta_line = format!("{:.1}%", dl.progress);
            if dl.file_size > 0 {
                meta_line.push_str(&format!(" · {}", fmt_size(dl.file_size)));
            }
            if active {
                meta_line.push_str(&format!(" · {} peers · {}", dl.peers, fmt_speed(dl.speed)));
            }

            let open_hash = hash.clone();
            let mut row = div()
                .id(SharedString::from(format!("dl-row-{i}")))
                .flex()
                .flex_col()
                .gap(px(theme.space_1()))
                .p(px(theme.space_3()))
                .rounded(px(theme.radius_md()))
                .bg(theme.bg_surface())
                .border_1()
                .border_color(theme.border_subtle())
                .cursor_pointer()
                .hover(move |s| s.border_color(theme.border_strong()))
                .on_click(cx.listener(move |this, _ev, _w, cx| {
                    this.open_download_movie(open_hash.clone(), cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.space_2()))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(theme.fs_2()))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.fg_primary())
                                .child(SharedString::from(name)),
                        )
                        .child(
                            div()
                                .px(px(theme.space_2()))
                                .py(px(2.0))
                                .rounded(px(theme.radius_sm()))
                                .text_size(px(theme.fs_1()))
                                .text_color(status_color)
                                .border_1()
                                .border_color(status_color)
                                .child(SharedString::from(dl.status.clone())),
                        )
                        .when(dl.pinned && !complete, |el| {
                            el.child(
                                div()
                                    .text_size(px(theme.fs_1()))
                                    .text_color(theme.accent_text())
                                    .child("background"),
                            )
                        }),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(4.0))
                        .rounded(px(theme.radius_sm()))
                        .bg(theme.bg_elevated())
                        .child(
                            div()
                                .h(px(4.0))
                                .w(gpui::relative(progress))
                                .rounded(px(theme.radius_sm()))
                                .bg(if complete {
                                    theme.success()
                                } else {
                                    theme.accent()
                                }),
                        ),
                );

            let mut actions = div().flex().items_center().gap(px(theme.space_2())).child(
                div()
                    .flex_1()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_muted())
                    .child(SharedString::from(meta_line)),
            );
            // Status-driven controls: anything actively pulling (pinned
            // or viewer-driven) gets Stop; paused and errored rows get
            // Resume, which re-adds dead torrents to the session.
            if active {
                let h = hash.clone();
                actions = actions.child(
                    secondary_button(SharedString::from(format!("dl-stop-{i}")), "Stop", &theme)
                        .on_click(cx.listener(move |this, _ev, _w, cx| {
                            cx.stop_propagation();
                            this.cancel_background_download(h.clone(), cx);
                        })),
                );
            } else if !complete {
                let h = hash.clone();
                actions = actions.child(
                    secondary_button(
                        SharedString::from(format!("dl-resume-{i}")),
                        "Resume",
                        &theme,
                    )
                    .on_click(cx.listener(move |this, _ev, _w, cx| {
                        cx.stop_propagation();
                        this.resume_background_download(h.clone(), cx);
                    })),
                );
            }
            if is_admin {
                let h = hash.clone();
                actions = actions.child(
                    secondary_button(
                        SharedString::from(format!("dl-delete-{i}")),
                        "🗑 Delete",
                        &theme,
                    )
                    .on_click(cx.listener(move |this, _ev, _w, cx| {
                        cx.stop_propagation();
                        this.delete_download(h.clone(), cx);
                    })),
                );
            }
            row = row.child(actions);
            list = list.child(row);
        }
        root.child(list)
    }

    fn favourites_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let items = self.state.favourites.read().clone();
        let loading = *self.state.favourites_loading.read();

        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .gap(px(theme.space_2()))
            .child(self.back_hint(cx))
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child("Favourites"),
            );

        if loading {
            root = root.child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_muted())
                    .child("Loading favourites…"),
            );
        } else if items.is_empty() {
            root = root.child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_muted())
                    .child("No favourites yet. Star a title to pin it here."),
            );
        } else {
            let mut grid = div()
                .flex()
                .flex_wrap()
                .gap(px(theme.space_3()))
                .mt(px(theme.space_3()));
            for (i, fav) in items.iter().enumerate() {
                let title: SharedString = fav.title.clone().into();
                let year = fav.year.map(|y| y.to_string()).unwrap_or_default();
                let rating = fav
                    .rating
                    .map(|r| format!("★ {:.1}", r))
                    .unwrap_or_default();
                let query = fav.title.clone();

                // Poster tile like the web Favourites grid; text-only
                // fallback when no poster URL was captured.
                let poster_box = match fav.poster_url.clone().filter(|p| !p.is_empty()) {
                    Some(url) => {
                        let src = crate::components::poster_image_source(&url);
                        div()
                            .w(px(120.0))
                            .h(px(180.0))
                            .rounded(px(theme.radius_md()))
                            .overflow_hidden()
                            .bg(theme.bg_panel())
                            .child(
                                img(src)
                                    .w(px(120.0))
                                    .h(px(180.0))
                                    .object_fit(ObjectFit::Cover),
                            )
                    }
                    None => div()
                        .w(px(120.0))
                        .h(px(180.0))
                        .rounded(px(theme.radius_md()))
                        .bg(theme.bg_panel())
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(theme.fs_6()))
                        .text_color(theme.fg_muted())
                        .child("★"),
                };

                grid = grid.child(
                    div()
                        .id(SharedString::from(format!("fav-{}", i)))
                        .w(px(120.0))
                        .cursor_pointer()
                        .flex()
                        .flex_col()
                        .gap(px(theme.space_1()))
                        .child(poster_box)
                        .child(
                            div()
                                .max_h(px(36.0))
                                .overflow_hidden()
                                .text_size(px(theme.fs_1()))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.fg_primary())
                                .child(title),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(theme.space_2()))
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.fg_muted())
                                .child(SharedString::from(year))
                                .child(
                                    div()
                                        .text_color(theme.favourite())
                                        .child(SharedString::from(rating)),
                                ),
                        )
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            // Re-search for this title and jump to Search page.
                            this.state.navigate(Page::Search);
                            this.search_input.update(cx, |input, _| {
                                input.set_value(query.clone());
                                input.submitted = true;
                            });
                            let fh = this.search_input.read(cx).focus_handle(cx);
                            fh.focus(window, cx);
                            cx.notify();
                        })),
                );
            }
            root = root.child(grid);
        }

        root
    }

    fn settings_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let mode = *self.state.mode.read();
        let server_url = self.state.server_url.read().clone();
        let version = self.state.server_version.read().clone();
        let hash = self.state.server_hash.read().clone();
        let user = self.state.user.read().clone();

        let back_hint = self.back_hint(cx);
        let mode_pill = |label: &'static str, this_mode: Mode| -> gpui::Stateful<gpui::Div> {
            let selected = mode == this_mode;
            div()
                .id(SharedString::from(format!(
                    "settings-mode-{}",
                    this_mode.as_str()
                )))
                .px(px(theme.space_3()))
                .py(px(theme.space_2()))
                .rounded(px(theme.radius_md()))
                .bg(if selected {
                    theme.accent()
                } else {
                    theme.bg_elevated()
                })
                .text_color(if selected {
                    theme.fg_on_accent()
                } else {
                    theme.fg_secondary()
                })
                .text_size(px(theme.fs_1()))
                .border_1()
                .border_color(if selected {
                    theme.accent()
                } else {
                    theme.border_default()
                })
                .cursor_pointer()
                .child(div().child(SharedString::from(label)))
                .on_click(cx.listener(move |this, _ev, _w, cx| {
                    this.state.set_mode(this_mode);
                    cx.notify();
                }))
        };

        let version_text: SharedString = match (version, hash) {
            (Some(v), Some(h)) => SharedString::from(format!("v{v} · {}", &h[..h.len().min(8)])),
            (Some(v), None) => SharedString::from(format!("v{v}")),
            _ => SharedString::from("server unreachable"),
        };
        let user_text: SharedString = user
            .map(|u| {
                SharedString::from(format!(
                    "@{} · {}",
                    u.username,
                    if u.is_admin { "admin" } else { "user" }
                ))
            })
            .unwrap_or_else(|| SharedString::from("not signed in"));

        div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .gap(px(theme.space_3()))
            .child(back_hint)
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child("Settings"),
            )
            .child(card(&theme).flex().flex_col().gap(px(theme.space_2()))
                .child(
                    div()
                        .text_size(px(theme.fs_3()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.fg_primary())
                        .child("Mode"),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child("Embedded resolves local files, Thin client streams over HTTP."),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(theme.space_2()))
                        .child(mode_pill("Embedded", Mode::Embedded))
                        .child(mode_pill("Thin client", Mode::ThinClient)),
                ),
            )
            .child(card(&theme).flex().flex_col().gap(px(theme.space_2()))
                .child(
                    div()
                        .text_size(px(theme.fs_3()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.fg_primary())
                        .child("Server"),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_2()))
                        .text_color(theme.fg_secondary())
                        .child(SharedString::from(server_url)),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child(version_text),
                ),
            )
            .child(card(&theme).flex().flex_col().gap(px(theme.space_2()))
                .child(
                    div()
                        .text_size(px(theme.fs_3()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.fg_primary())
                        .child("Maintenance"),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child("Restart the torrent client: closes all peer connections and rediscovers seed nodes. Active and background downloads are re-added."),
                )
                .child(
                    secondary_button("settings-restart-torrent", "Restart torrent client", &theme)
                        .on_click(cx.listener(|this, _ev, _w, cx| {
                            let state = this.state.clone();
                            state.show_toast("Restarting torrent client…", ToastKind::Info);
                            cx.spawn(async move |view, cx: &mut gpui::AsyncApp| {
                                let client = state.client.read().clone();
                                let res = runtime::spawn(async move { client.restart_torrent().await }).await;
                                match res {
                                    Ok(()) => state.show_toast("Torrent client restarted.", ToastKind::Success),
                                    Err(e) => state.show_toast(format!("Restart failed: {e}"), ToastKind::Error),
                                }
                                let _ = view.update(cx, |_, cx| cx.notify());
                            })
                            .detach();
                        })),
                ),
            )
            .child(card(&theme).flex().flex_col().gap(px(theme.space_2()))
                .child(
                    div()
                        .text_size(px(theme.fs_3()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.fg_primary())
                        .child("Account"),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_2()))
                        .text_color(theme.fg_secondary())
                        .child(user_text),
                )
                .child(
                    primary_button("settings-logout", "Log out", &theme)
                        .on_click(cx.listener(|this, _ev, _w, cx| {
                            this.state.set_token(None);
                            *this.state.user.write() = None;
                            this.state.replace_page(Page::Login);
                            cx.notify();
                        })),
                ),
            )
    }

    fn music_list_page(
        &self,
        header_title: &'static str,
        input: Entity<TextInput>,
        results: Vec<streamx_api::types::MusicVideoResult>,
        loading: bool,
        api_base: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.theme;

        let hint = if loading {
            "loading… ⟳"
        } else {
            "Enter to search · Esc back"
        };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(theme.space_3()))
            .mb(px(theme.space_4()))
            .child(div().flex_1().max_w(px(480.0)).child(self.input_with_clear(
                input,
                if api_base == "music" {
                    "clear-music"
                } else {
                    "clear-music-videos"
                },
                cx,
            )))
            .child(
                div()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_muted())
                    .child(SharedString::from(hint)),
            );

        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .mb(px(theme.space_3()))
                    .child(header_title),
            )
            .child(header);

        if results.is_empty() {
            root = root.child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_muted())
                    .child(if loading {
                        "Searching…"
                    } else {
                        "No results."
                    }),
            );
        } else {
            let mut list = div().flex().flex_col().gap(px(theme.space_2()));
            for (i, r) in results.iter().enumerate() {
                let title = SharedString::from(r.title.clone());
                let size = SharedString::from(r.size.clone());
                let seeds = r.seeds;
                let leeches = r.leeches;
                let magnet = r.magnet.clone();
                let detail_url = r.detail_url.clone();
                let title_owned = r.title.clone();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.space_3()))
                        .p(px(theme.space_3()))
                        .rounded(px(theme.radius_md()))
                        .bg(theme.bg_surface())
                        .border_1()
                        .border_color(theme.border_subtle())
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(theme.fs_2()))
                                .text_color(theme.fg_primary())
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.fg_muted())
                                .child(size),
                        )
                        .child(
                            div()
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.success())
                                .child(SharedString::from(format!("↑{seeds}"))),
                        )
                        .child(
                            div()
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.error())
                                .child(SharedString::from(format!("↓{leeches}"))),
                        )
                        .child(
                            primary_button(
                                SharedString::from(format!("{api_base}-play-{i}")),
                                "Play",
                                &theme,
                            )
                            .on_click(cx.listener(
                                move |this, _ev, _w, cx| {
                                    this.play_magnet(
                                        magnet.clone(),
                                        title_owned.clone(),
                                        Some(api_base),
                                        Some(detail_url.clone()),
                                        cx,
                                    );
                                },
                            )),
                        ),
                );
            }
            root = root.child(list);
        }

        root
    }

    fn music_search_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let results = self.state.music_results.read().clone();
        let loading = *self.state.music_loading.read();
        self.music_list_page(
            "Music",
            self.music_input.clone(),
            results,
            loading,
            "music",
            cx,
        )
    }

    fn music_video_search_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let results = self.state.music_video_results.read().clone();
        let loading = *self.state.music_video_loading.read();
        self.music_list_page(
            "Music Videos",
            self.music_video_input.clone(),
            results,
            loading,
            "music-videos",
            cx,
        )
    }

    fn tv_search_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let results = self.state.tv_results.read().clone();
        let loading = *self.state.tv_loading.read();
        let hint = if loading {
            "loading… ⟳"
        } else {
            "Enter to search · Esc back"
        };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(theme.space_3()))
            .mb(px(theme.space_4()))
            .child(div().flex_1().max_w(px(480.0)).child(self.input_with_clear(
                self.tv_input.clone(),
                "clear-tv",
                cx,
            )))
            .child(
                div()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_muted())
                    .child(SharedString::from(hint)),
            );

        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .mb(px(theme.space_3()))
                    .child("TV Shows"),
            )
            .child(header);

        if results.is_empty() {
            root = root.child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_muted())
                    .child(if loading {
                        "Searching…"
                    } else {
                        "No results."
                    }),
            );
        } else {
            let mut list = div().flex().flex_col().gap(px(theme.space_2()));
            for (i, g) in results.iter().enumerate() {
                let show_name = SharedString::from(g.show_name.clone());
                let season_count = g.seasons.len();
                let ep_count: usize = g.seasons.iter().map(|s| s.episodes.len()).sum();
                let meta =
                    SharedString::from(format!("{season_count} seasons · {ep_count} episodes"));
                let clone = g.clone();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("tv-row-{i}")))
                        .flex()
                        .items_center()
                        .gap(px(theme.space_3()))
                        .p(px(theme.space_3()))
                        .rounded(px(theme.radius_md()))
                        .bg(theme.bg_surface())
                        .border_1()
                        .border_color(theme.border_subtle())
                        .cursor_pointer()
                        .hover(|s| s.border_color(theme.accent()))
                        .on_click(cx.listener(move |this, _ev, _w, cx| {
                            *this.state.selected_tv_show.write() = Some(clone.clone());
                            this.state.navigate(Page::TvShow);
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(theme.fs_2()))
                                .text_color(theme.fg_primary())
                                .child(show_name),
                        )
                        .child(
                            div()
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.fg_muted())
                                .child(meta),
                        ),
                );
            }
            root = root.child(list);
        }

        root
    }

    fn tv_show_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let show = self.state.selected_tv_show.read().clone();

        let Some(show) = show else {
            return div()
                .w_full()
                .p(px(theme.space_5()))
                .bg(theme.bg_app())
                .flex()
                .flex_col()
                .gap(px(theme.space_2()))
                .child(
                    div()
                        .text_size(px(theme.fs_2()))
                        .text_color(theme.fg_muted())
                        .child("No show selected."),
                );
        };

        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .gap(px(theme.space_3()))
            .child(self.back_hint(cx))
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child(SharedString::from(show.show_name.clone())),
            );

        for season in &show.seasons {
            let mut col = div()
                .flex()
                .flex_col()
                .gap(px(theme.space_1()))
                .mt(px(theme.space_3()))
                .child(
                    div()
                        .text_size(px(theme.fs_3()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.fg_primary())
                        .child(SharedString::from(format!("Season {}", season.season))),
                );
            for ep in &season.episodes {
                let ep_title = format!(
                    "S{:02}E{:02}  {}",
                    season.season,
                    ep.episode,
                    ep.title.as_deref().unwrap_or("")
                );
                let mut ep_row = div()
                    .flex()
                    .flex_col()
                    .gap(px(theme.space_1()))
                    .p(px(theme.space_2()))
                    .rounded(px(theme.radius_sm()))
                    .bg(theme.bg_surface())
                    .border_1()
                    .border_color(theme.border_subtle())
                    .child(
                        div()
                            .text_size(px(theme.fs_2()))
                            .text_color(theme.fg_primary())
                            .child(SharedString::from(ep_title)),
                    );
                if ep.variants.is_empty() {
                    ep_row = ep_row.child(
                        div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .child("No variants available."),
                    );
                } else {
                    let mut variants = div().flex().gap(px(theme.space_2())).flex_wrap();
                    for (vi, v) in ep.variants.iter().enumerate() {
                        let q = v.quality.clone().unwrap_or_default();
                        let magnet = v.magnet.clone();
                        let title = format!(
                            "{} · S{:02}E{:02}",
                            show.show_name, season.season, ep.episode
                        );
                        let size_mb = v.size_bytes / (1024 * 1024);
                        let label = if q.is_empty() {
                            format!("Play ({} MB)", size_mb)
                        } else {
                            format!("{} ({} MB)", q, size_mb)
                        };
                        variants = variants.child(
                            primary_button(
                                SharedString::from(format!(
                                    "tv-play-{}-{}-{}",
                                    season.season, ep.episode, vi
                                )),
                                SharedString::from(label),
                                &theme,
                            )
                            .on_click(cx.listener(
                                move |this, _ev, _w, cx| {
                                    this.play_magnet(
                                        Some(magnet.clone()),
                                        title.clone(),
                                        None,
                                        None,
                                        cx,
                                    );
                                },
                            )),
                        );
                    }
                    ep_row = ep_row.child(variants);
                }
                col = col.child(ep_row);
            }
            root = root.child(col);
        }

        root
    }

    fn surround_sound_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .gap(px(theme.space_2()))
            .child(self.back_hint(cx))
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child("Surround Sound"),
            )
            .child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_secondary())
                    .child("Demo tracks for testing 5.1 / 7.1 speaker configurations."),
            );

        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(theme.space_2()))
            .mt(px(theme.space_3()));
        for (i, demo) in SURROUND_DEMOS.iter().enumerate() {
            let magnet = demo.magnet.to_string();
            let title = demo.title.to_string();
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme.space_3()))
                    .p(px(theme.space_3()))
                    .rounded(px(theme.radius_md()))
                    .bg(theme.bg_surface())
                    .border_1()
                    .border_color(theme.border_subtle())
                    .child(
                        div()
                            .px(px(theme.space_2()))
                            .py(px(2.0))
                            .rounded(px(theme.radius_sm()))
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.accent())
                            .border_1()
                            .border_color(theme.accent())
                            .child(SharedString::from(demo.format)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(theme.fs_2()))
                            .text_color(theme.fg_primary())
                            .child(SharedString::from(demo.title)),
                    )
                    .child(
                        div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .child(SharedString::from(demo.quality)),
                    )
                    .child(
                        div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .child(SharedString::from(demo.size)),
                    )
                    .child(
                        primary_button(SharedString::from(format!("ss-play-{i}")), "Play", &theme)
                            .on_click(cx.listener(move |this, _ev, _w, cx| {
                                this.play_magnet(
                                    Some(magnet.clone()),
                                    title.clone(),
                                    None,
                                    None,
                                    cx,
                                );
                            })),
                    ),
            );
        }
        root = root.child(list);
        root
    }

    fn admin_kill(&mut self, cx: &mut Context<Self>) {
        let id = self.admin_kill_input.read(cx).value().trim().to_string();
        if id.is_empty() {
            self.state
                .show_toast("Enter a stream id first.", ToastKind::Info);
            cx.notify();
            return;
        }
        let state = self.state.clone();
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let res = runtime::spawn(async move { client.admin_kill_stream(&id).await }).await;
            match res {
                Ok(()) => state.show_toast("Stream killed.", ToastKind::Success),
                Err(e) => state.show_toast(format!("Kill failed: {e}"), ToastKind::Error),
            }
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    fn admin_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let user = self.state.user.read().clone();
        let is_admin = user.as_ref().map(|u| u.is_admin).unwrap_or(false);

        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .gap(px(theme.space_3()))
            .child(self.back_hint(cx))
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child("Admin"),
            );

        if !is_admin {
            root = root.child(
                card(&theme)
                    .flex()
                    .flex_col()
                    .gap(px(theme.space_2()))
                    .child(
                        div()
                            .text_size(px(theme.fs_2()))
                            .text_color(theme.error())
                            .child("Admin access required."),
                    )
                    .child(
                        div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .child("Sign in as an admin user to see this page."),
                    ),
            );
            return root;
        }

        root.child(
            card(&theme)
                .flex()
                .flex_col()
                .gap(px(theme.space_2()))
                .child(
                    div()
                        .text_size(px(theme.fs_3()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.fg_primary())
                        .child("Kill stream"),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child("Stop an active transcode by its stream id."),
                )
                .child(self.admin_kill_input.clone())
                .child(
                    primary_button("admin-kill-submit", "Kill stream", &theme)
                        .on_click(cx.listener(|this, _ev, _w, cx| this.admin_kill(cx))),
                ),
        )
        .child(
            card(&theme)
                .flex()
                .flex_col()
                .gap(px(theme.space_2()))
                .child(
                    div()
                        .text_size(px(theme.fs_3()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.fg_primary())
                        .child("Monitor"),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child("Live server metrics and logs stream over WebSocket. Desktop WebSocket support lands later; use the web UI for live dashboards."),
                ),
        )
        .child(
            card(&theme)
                .flex()
                .flex_col()
                .gap(px(theme.space_2()))
                .child(
                    div()
                        .text_size(px(theme.fs_3()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.fg_primary())
                        .child("Maintenance"),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_1()))
                        .text_color(theme.fg_muted())
                        .child(
                            "Clean removes the transcode cache and every downloaded file; \
                             your account, history, favourites, and settings stay. Wipe \
                             removes everything except the config file: accounts, history, \
                             favourites, database, cache, and downloads are gone. Both \
                             restart the app.",
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.space_2()))
                        .child({
                            let confirming = self.confirm_maintenance == Some("clean");
                            let label = if confirming { "Confirm clean + restart" } else { "Clean" };
                            crate::components::danger_button("admin-clean", label, &theme).on_click(
                                cx.listener(move |this, _ev, _w, cx| {
                                    if confirming {
                                        relaunch_with_maintenance("clean");
                                    } else {
                                        this.confirm_maintenance = Some("clean");
                                        cx.notify();
                                    }
                                }),
                            )
                        })
                        .child({
                            let confirming = self.confirm_maintenance == Some("wipe");
                            let label = if confirming { "Confirm wipe + restart" } else { "Wipe" };
                            crate::components::danger_button("admin-wipe", label, &theme).on_click(
                                cx.listener(move |this, _ev, _w, cx| {
                                    if confirming {
                                        relaunch_with_maintenance("wipe");
                                    } else {
                                        this.confirm_maintenance = Some("wipe");
                                        cx.notify();
                                    }
                                }),
                            )
                        })
                        .when(self.confirm_maintenance.is_some(), |el| {
                            el.child(
                                secondary_button("admin-maint-cancel", "Cancel", &theme).on_click(
                                    cx.listener(|this, _ev, _w, cx| {
                                        this.confirm_maintenance = None;
                                        cx.notify();
                                    }),
                                ),
                            )
                        }),
                ),
        )
    }

    fn drawer_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let is_admin = self
            .state
            .user
            .read()
            .as_ref()
            .map(|u| u.is_admin)
            .unwrap_or(false);

        let link = |label: &'static str, page: Page| {
            div()
                .id(SharedString::from(format!("nav-{}", label)))
                .px(px(theme.space_3()))
                .py(px(theme.space_3()))
                .rounded(px(theme.radius_md()))
                .text_size(px(theme.fs_2()))
                .text_color(theme.fg_secondary())
                .cursor_pointer()
                .hover(|s| s.bg(theme.bg_elevated()).text_color(theme.fg_primary()))
                .child(SharedString::from(label))
                .on_click(cx.listener(move |this, _ev, _w, cx| {
                    *this.state.drawer_open.write() = false;
                    // Push (not replace) so the header back arrow and Esc
                    // walk history exactly like the web app's browser back.
                    this.state.navigate(page);
                    this.ensure_loaded_for(page);
                    cx.notify();
                }))
        };

        let mut items = div()
            .flex()
            .flex_col()
            .gap(px(theme.space_1()))
            .child(link("Movies", Page::Search))
            .child(link("TV Shows", Page::TvSearch))
            .child(link("Favourites", Page::Favourites))
            .child(link("Downloads", Page::Downloads))
            .child(link("History", Page::History))
            .child(link("Surround Sound", Page::SurroundSound))
            .child(link("Settings", Page::Settings))
            .child(link("Logs", Page::Logs));

        if is_admin {
            items = items.child(link("Admin", Page::Admin));
        }

        // Full-screen overlay with a 280px panel on the left. `occlude`
        // stops clicks from reaching page elements underneath — without
        // it a drawer click also activates whatever tile/button sits
        // below it, navigating somewhere unrelated.
        div()
            .occlude()
            .absolute()
            .inset_0()
            .bg(theme.bg_overlay())
            .flex()
            .child(
                div()
                    .occlude()
                    .w(px(280.0 * theme.scale()))
                    .h_full()
                    .bg(theme.bg_surface())
                    .border_r_1()
                    .border_color(theme.border_subtle())
                    .p(px(theme.space_4()))
                    .flex()
                    .flex_col()
                    .gap(px(theme.space_3()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(theme.space_2()))
                            .child(
                                gpui::svg()
                                    .path("logo.svg")
                                    .w(px(48.0 * theme.scale()))
                                    .h(px(48.0 * theme.scale()))
                                    .text_color(theme.fg_primary()),
                            )
                            .child(
                                div()
                                    .text_size(px(theme.fs_5()))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme.accent_text())
                                    .child("StreamX"),
                            ),
                    )
                    .child(items)
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .child(SharedString::from(format!(
                                "v{} \u{b7} {}",
                                streamx::server::static_files::VERSION,
                                streamx::server::static_files::BUILD_HASH
                            ))),
                    ),
            )
            // Clicking outside closes the drawer.
            .child(
                div()
                    .id("drawer-scrim")
                    .flex_1()
                    .h_full()
                    .on_click(cx.listener(|this, _ev, _w, cx| {
                        *this.state.drawer_open.write() = false;
                        cx.notify();
                    })),
            )
    }

    /// Clickable back hint used at the top of each page. Esc still works;
    /// this lets the user click too.
    fn back_hint(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        div()
            .id("back-hint")
            .text_size(px(theme.fs_1()))
            .text_color(theme.fg_muted())
            .cursor_pointer()
            .hover(|s| s.text_color(theme.fg_secondary()))
            .child("← Esc to go back")
            .on_click(cx.listener(|this, _ev, _w, cx| {
                if this.state.back() {
                    cx.notify();
                }
            }))
    }

    /// Custom title bar. Draggable, double-click maximizes, houses the
    /// min/max/close buttons. Required when running with client-side
    /// decorations on Linux/Wayland.
    fn title_bar(&self, _window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let control = |id: &'static str, icon: &'static str, is_close: bool| {
            let hover_bg = if is_close {
                theme.error()
            } else {
                theme.bg_elevated()
            };
            let hover_fg = if is_close {
                theme.fg_on_accent()
            } else {
                theme.fg_primary()
            };
            div()
                .id(SharedString::from(format!("win-ctrl-{id}")))
                .w(px(28.0 * theme.scale()))
                .h(px(20.0 * theme.scale()))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme.radius_sm()))
                .text_size(px(12.0))
                .text_color(theme.fg_secondary())
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
                .child(icon)
                .on_mouse_down(MouseButton::Left, |_ev, _w, cx| {
                    cx.stop_propagation();
                })
                .on_click(move |_ev, window, _cx| match id {
                    "minimize" => window.minimize_window(),
                    "maximize" => window.zoom_window(),
                    "close" => window.remove_window(),
                    _ => {}
                })
        };

        let bar = div()
            .id("title-bar")
            .w_full()
            .h(px(32.0 * theme.scale()))
            .flex()
            .items_center()
            .justify_between()
            .px(px(theme.space_3()))
            .bg(theme.bg_surface())
            .border_b_1()
            .border_color(theme.border_subtle())
            .on_mouse_down(MouseButton::Left, |_ev, window, _cx| {
                window.start_window_move();
            })
            .on_click(cx.listener(|_this, ev: &gpui::ClickEvent, window, _cx| {
                if ev.click_count() == 2 {
                    window.zoom_window();
                }
            }));

        let name = div()
            .flex()
            .items_center()
            .gap(px(theme.space_2()))
            .text_size(px(theme.fs_1()))
            .text_color(theme.fg_muted())
            .child("StreamX");

        // macOS: the native traffic lights overlay the top-left corner
        // (the titlebar is transparent, not removed), so the app name
        // lives on the right and no custom controls are drawn.
        #[cfg(target_os = "macos")]
        {
            let _ = control;
            bar.child(div().w(px(78.0))).child(name)
        }

        // Linux (client-side decorations): name on the left, custom
        // min/max/close controls on the right.
        #[cfg(not(target_os = "macos"))]
        {
            bar.child(name).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .on_mouse_down(MouseButton::Left, |_ev, _w, cx| {
                        cx.stop_propagation();
                    })
                    .child(control("minimize", "−", false))
                    .child(control("maximize", "□", false))
                    .child(control("close", "✕", true)),
            )
        }
    }

    /// App header: drawer button, logo (click → home), current page title,
    /// forward/back nav arrows, user badge. Lives under the title bar.
    fn app_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let page = self.state.current_page();
        let can_go_back = self.state.page_stack.read().len() > 1;
        let user = self.state.user.read().clone();

        let nav_arrow = |id: &'static str, icon: &'static str, enabled: bool| {
            let color = if enabled {
                theme.fg_secondary()
            } else {
                theme.fg_disabled()
            };
            div()
                .id(SharedString::from(format!("nav-{id}")))
                .w(px(28.0 * theme.scale()))
                .h(px(28.0 * theme.scale()))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme.radius_sm()))
                .text_size(px(theme.fs_3()))
                .text_color(color)
                .when(enabled, |el| {
                    el.cursor_pointer()
                        .hover(move |s| s.bg(theme.bg_elevated()))
                })
                .child(icon)
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(theme.space_4()))
            .py(px(theme.space_2()))
            .border_b_1()
            .border_color(theme.border_subtle())
            .bg(theme.bg_surface())
            // LEFT: logo → home, current page name.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme.space_3()))
                    .child(
                        div()
                            .id("logo-home")
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.8))
                            .child(
                                gpui::svg()
                                    .path("logo.svg")
                                    .w(px(44.0 * theme.scale()))
                                    .h(px(44.0 * theme.scale()))
                                    .text_color(theme.fg_primary()),
                            )
                            .on_click(cx.listener(|this, _ev, _w, cx| {
                                // Logo is a full "go home": clear any
                                // search so the browse view shows, from
                                // every page.
                                this.search_input.update(cx, |input, _| input.set_value(""));
                                *this.state.query.write() = String::new();
                                *this.state.search_results.write() = Vec::new();
                                this.state.navigate(Page::Search);
                                this.ensure_loaded_for(Page::Search);
                                this.state.mark_dirty();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(theme.fs_1()))
                            .text_color(theme.fg_muted())
                            .child(SharedString::from(page.title())),
                    ),
            )
            // RIGHT: back/forward arrows, user badge, hamburger (matches
            // the web UI which keeps the menu on the right).
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme.space_2()))
                    .child(nav_arrow("back", "◀", can_go_back).on_click(cx.listener(
                        |this, _ev, _w, cx| {
                            if this.state.back() {
                                cx.notify();
                            }
                        },
                    )))
                    .child(nav_arrow("fwd", "▶", false))
                    .when(user.is_some(), |el| {
                        let u = user
                            .as_ref()
                            .map(|u| u.username.clone())
                            .unwrap_or_default();
                        el.child(
                            div()
                                .text_size(px(theme.fs_1()))
                                .text_color(theme.fg_secondary())
                                .px(px(theme.space_2()))
                                .child(SharedString::from(format!("@{u}"))),
                        )
                    })
                    .child(
                        div()
                            .id("menu-button")
                            .px(px(theme.space_3()))
                            .py(px(theme.space_1()))
                            .rounded(px(theme.radius_sm()))
                            .text_size(px(theme.fs_3()))
                            .text_color(theme.fg_secondary())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.bg_elevated()).text_color(theme.fg_primary()))
                            .child("☰")
                            .on_click(cx.listener(|this, _ev, _w, cx| {
                                let mut d = this.state.drawer_open.write();
                                *d = !*d;
                                drop(d);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// 8 invisible strips around the edges that start a window resize
    /// on mouse-down. Only needed when running with client-side
    /// decorations (Linux/Wayland).
    #[cfg(target_os = "linux")]
    fn resize_borders() -> Vec<gpui::AnyElement> {
        let e = 6.0;
        let c = 12.0;
        fn strip(
            id: &'static str,
            cursor: CursorStyle,
            edge: ResizeEdge,
            base: gpui::Div,
        ) -> gpui::AnyElement {
            base.id(SharedString::from(id))
                .cursor(cursor)
                .on_mouse_down(MouseButton::Left, move |_ev, window, _cx| {
                    window.start_window_resize(edge);
                })
                .into_any_element()
        }
        vec![
            strip(
                "rz-top",
                CursorStyle::ResizeUpDown,
                ResizeEdge::Top,
                div().absolute().top_0().left(px(c)).right(px(c)).h(px(e)),
            ),
            strip(
                "rz-bot",
                CursorStyle::ResizeUpDown,
                ResizeEdge::Bottom,
                div()
                    .absolute()
                    .bottom_0()
                    .left(px(c))
                    .right(px(c))
                    .h(px(e)),
            ),
            strip(
                "rz-left",
                CursorStyle::ResizeLeftRight,
                ResizeEdge::Left,
                div().absolute().left_0().top(px(c)).bottom(px(c)).w(px(e)),
            ),
            strip(
                "rz-right",
                CursorStyle::ResizeLeftRight,
                ResizeEdge::Right,
                div().absolute().right_0().top(px(c)).bottom(px(c)).w(px(e)),
            ),
            strip(
                "rz-tl",
                CursorStyle::ResizeUpLeftDownRight,
                ResizeEdge::TopLeft,
                div().absolute().top_0().left_0().w(px(c)).h(px(c)),
            ),
            strip(
                "rz-tr",
                CursorStyle::ResizeUpRightDownLeft,
                ResizeEdge::TopRight,
                div().absolute().top_0().right_0().w(px(c)).h(px(c)),
            ),
            strip(
                "rz-bl",
                CursorStyle::ResizeUpRightDownLeft,
                ResizeEdge::BottomLeft,
                div().absolute().bottom_0().left_0().w(px(c)).h(px(c)),
            ),
            strip(
                "rz-br",
                CursorStyle::ResizeUpLeftDownRight,
                ResizeEdge::BottomRight,
                div().absolute().bottom_0().right_0().w(px(c)).h(px(c)),
            ),
        ]
    }

    fn toast_view(&self, toast: &Toast) -> impl IntoElement {
        let theme = self.theme;
        let border = match toast.kind {
            ToastKind::Info => theme.accent(),
            ToastKind::Success => theme.success(),
            ToastKind::Error => theme.error(),
        };
        div()
            .absolute()
            .top(px(theme.space_4()))
            .right(px(theme.space_4()))
            .max_w(px(360.0))
            .p(px(theme.space_3()))
            .rounded(px(theme.radius_md()))
            .bg(theme.bg_surface())
            .border_1()
            .border_color(border)
            .text_size(px(theme.fs_2()))
            .text_color(theme.fg_primary())
            .child(SharedString::from(toast.message.clone()))
    }
}

async fn load_browse(state: &Arc<AppState>) {
    use streamx_api::client::BrowseParams;

    *state.browse_loading.write() = true;
    state.mark_dirty();
    let client: Client = state.client.read().clone();
    let sections: [(&str, BrowseParams); 9] = [
        (
            "this_year",
            BrowseParams {
                sort_by: Some("download_count".into()),
                query_term: Some(current_year_title().into()),
                limit: Some(24),
                ..Default::default()
            },
        ),
        (
            "latest",
            BrowseParams {
                sort_by: Some("date_added".into()),
                limit: Some(24),
                ..Default::default()
            },
        ),
        (
            "popular",
            BrowseParams {
                sort_by: Some("download_count".into()),
                limit: Some(24),
                ..Default::default()
            },
        ),
        (
            "top_rated",
            BrowseParams {
                sort_by: Some("rating".into()),
                minimum_rating: Some(8),
                limit: Some(24),
                ..Default::default()
            },
        ),
        (
            "action",
            BrowseParams {
                sort_by: Some("download_count".into()),
                genre: Some("action".into()),
                limit: Some(24),
                ..Default::default()
            },
        ),
        (
            "comedy",
            BrowseParams {
                sort_by: Some("download_count".into()),
                genre: Some("comedy".into()),
                limit: Some(24),
                ..Default::default()
            },
        ),
        (
            "thriller",
            BrowseParams {
                sort_by: Some("download_count".into()),
                genre: Some("thriller".into()),
                limit: Some(24),
                ..Default::default()
            },
        ),
        (
            "scifi",
            BrowseParams {
                sort_by: Some("download_count".into()),
                genre: Some("sci-fi".into()),
                limit: Some(24),
                ..Default::default()
            },
        ),
        (
            "horror",
            BrowseParams {
                sort_by: Some("download_count".into()),
                genre: Some("horror".into()),
                limit: Some(24),
                ..Default::default()
            },
        ),
    ];

    // All rows fetch in parallel so the home page fills in one round
    // trip instead of eight sequential ones.
    let mut handles = Vec::new();
    for (name, p) in sections {
        let c = client.clone();
        handles.push((name, runtime::spawn(async move { c.browse(&p).await })));
    }
    let mut out = BrowseData::default();
    for (name, fut) in handles {
        if let Ok(rows) = fut.await {
            let rows: Vec<std::sync::Arc<streamx_api::types::SearchResultGroup>> =
                rows.into_iter().map(std::sync::Arc::new).collect();
            match name {
                "this_year" => out.this_year = rows,
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
    state.mark_dirty();
}

/// Debounce + change-detection state for one live-search input.
/// `last_seen` tracks the newest value (restarts the timer only when the
/// user actually types), `last_fired` tracks what was last searched.
#[derive(Default)]
pub struct DebounceState {
    last_seen: String,
    last_fired: String,
    typed_at: Option<std::time::Instant>,
}

/// Fires immediately on Enter (`submitted=true`); otherwise fires once the
/// value has been stable for `debounce` and differs from the last search.
/// An emptied field fires with an empty query so the page resets to the
/// browse view, matching the web UI.
pub fn fire_debounced<F: FnOnce(String)>(
    current: &str,
    submitted: bool,
    st: &mut DebounceState,
    debounce: Duration,
    run: F,
) {
    let trimmed = current.trim();
    if trimmed != st.last_seen {
        st.last_seen = trimmed.to_string();
        st.typed_at = Some(std::time::Instant::now());
    }
    if submitted && !trimmed.is_empty() {
        st.last_fired = trimmed.to_string();
        st.typed_at = None;
        run(trimmed.to_string());
        return;
    }
    let ready = st
        .typed_at
        .map(|t| t.elapsed() >= debounce)
        .unwrap_or(false);
    if !ready {
        return;
    }
    st.typed_at = None;
    if trimmed == st.last_fired {
        return;
    }
    st.last_fired = trimmed.to_string();
    run(trimmed.to_string());
}

async fn run_music_search(state: Arc<AppState>, query: String) {
    let generation = state
        .music_generation
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        + 1;
    if query.trim().is_empty() {
        *state.music_query.write() = String::new();
        load_music(&state).await;
        return;
    }
    *state.music_loading.write() = true;
    state.mark_dirty();
    *state.music_query.write() = query.clone();
    let client = state.client.read().clone();
    let result = client.search_music(&query).await;
    if state
        .music_generation
        .load(std::sync::atomic::Ordering::SeqCst)
        != generation
    {
        return;
    }
    match result {
        Ok(resp) => *state.music_results.write() = resp.results,
        Err(e) => state.show_toast(format!("Music search failed: {e}"), ToastKind::Error),
    }
    *state.music_loading.write() = false;
    state.mark_dirty();
}

async fn run_music_video_search(state: Arc<AppState>, query: String) {
    let generation = state
        .music_video_generation
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        + 1;
    if query.trim().is_empty() {
        *state.music_video_query.write() = String::new();
        load_music_videos(&state).await;
        return;
    }
    *state.music_video_loading.write() = true;
    state.mark_dirty();
    *state.music_video_query.write() = query.clone();
    let client = state.client.read().clone();
    let result = client.search_music_videos(&query).await;
    if state
        .music_video_generation
        .load(std::sync::atomic::Ordering::SeqCst)
        != generation
    {
        return;
    }
    match result {
        Ok(resp) => *state.music_video_results.write() = resp.results,
        Err(e) => state.show_toast(format!("Music video search failed: {e}"), ToastKind::Error),
    }
    *state.music_video_loading.write() = false;
    state.mark_dirty();
}

async fn run_tv_search(state: Arc<AppState>, query: String) {
    let generation = state
        .tv_generation
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        + 1;
    if query.trim().is_empty() {
        *state.tv_query.write() = String::new();
        load_tv(&state).await;
        return;
    }
    *state.tv_loading.write() = true;
    state.mark_dirty();
    *state.tv_query.write() = query.clone();
    let client = state.client.read().clone();
    let result = client.search_tv(&query).await;
    if state
        .tv_generation
        .load(std::sync::atomic::Ordering::SeqCst)
        != generation
    {
        return;
    }
    match result {
        Ok(resp) => *state.tv_results.write() = resp.results,
        Err(e) => state.show_toast(format!("TV search failed: {e}"), ToastKind::Error),
    }
    *state.tv_loading.write() = false;
    state.mark_dirty();
}

async fn load_music(state: &Arc<AppState>) {
    *state.music_loading.write() = true;
    state.mark_dirty();
    let client = state.client.read().clone();
    match client.browse_music(1).await {
        Ok(resp) => *state.music_results.write() = resp.results,
        Err(e) => state.show_toast(format!("Music browse failed: {e}"), ToastKind::Error),
    }
    *state.music_loading.write() = false;
    state.mark_dirty();
}

async fn load_music_videos(state: &Arc<AppState>) {
    *state.music_video_loading.write() = true;
    state.mark_dirty();
    let client = state.client.read().clone();
    match client.browse_music_videos(1).await {
        Ok(resp) => *state.music_video_results.write() = resp.results,
        Err(e) => state.show_toast(format!("Music video browse failed: {e}"), ToastKind::Error),
    }
    *state.music_video_loading.write() = false;
    state.mark_dirty();
}

async fn load_tv(state: &Arc<AppState>) {
    *state.tv_loading.write() = true;
    state.mark_dirty();
    let client = state.client.read().clone();
    match client.browse_tv(1).await {
        Ok(resp) => *state.tv_results.write() = resp.results,
        Err(e) => state.show_toast(format!("TV browse failed: {e}"), ToastKind::Error),
    }
    *state.tv_loading.write() = false;
    state.mark_dirty();
}

struct SurroundDemo {
    title: &'static str,
    format: &'static str,
    quality: &'static str,
    size: &'static str,
    magnet: &'static str,
}

const SURROUND_DEMOS: &[SurroundDemo] = &[
    SurroundDemo {
        title: "Big Buck Bunny - Sunflower (AC3 5.1, 60fps)",
        format: "AC3 5.1",
        quality: "1080p 60fps",
        size: "~355 MB",
        magnet: "magnet:?xt=urn:btih:565DB305A27FFB321FCC7B064AFD7BD73AEDDA2B&dn=bbb_sunflower_1080p_60fps_normal.mp4&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A80%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&ws=http%3A%2F%2Fdistribution.bbb3d.renderfarming.net%2Fvideo%2Fmp4%2Fbbb_sunflower_1080p_60fps_normal.mp4",
    },
    SurroundDemo {
        title: "Big Buck Bunny 4K UHD (FLAC 5.1, x265, 60fps)",
        format: "FLAC 5.1",
        quality: "4K 60fps",
        size: "~616 MB",
        magnet: "magnet:?xt=urn:btih:5B8C29A1E13D409422089CF113851DEC9E2F4E97&dn=Big+Buck+Bunny+4K+UHD+HFR+60+fps+FLAC+WEBRip+2160p+X265&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A80%2Fannounce",
    },
    SurroundDemo {
        title: "Sintel (AC3 5.1, 1024p)",
        format: "AC3 5.1",
        quality: "1024p",
        size: "~129 MB",
        magnet: "magnet:?xt=urn:btih:6a9759bffd5c0af65319979fb7832189f4f3c35d&dn=sintel.mp4&tr=wss%3A%2F%2Ftracker.btorrent.xyz&tr=wss%3A%2F%2Ftracker.fastcast.nz&tr=wss%3A%2F%2Ftracker.openwebtorrent.com&ws=https%3A%2F%2Ffastcast.nz%2Fdownloads%2Fsintel-1024-surround.mp4&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2Fsintel-1024-surround.mp4",
    },
    SurroundDemo {
        title: "5.1 Surround PCM Channel Test",
        format: "PCM 5.1",
        quality: "1080p",
        size: "~100 MB",
        magnet: "magnet:?xt=urn:btih:59bd2de84ca4c56f5d158974eb01e2a260b36792&dn=Surround+Sound+Test+PCM+5.1&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/surround-sound-test-pcm-5.1/",
    },
    SurroundDemo {
        title: "DTS 5.1 Channel Check",
        format: "DTS 5.1",
        quality: "1080p",
        size: "~150 MB",
        magnet: "magnet:?xt=urn:btih:52b9bd8592de146ea0069edb0485af274ecdcbd7&dn=DTS+5.1+Surround+Sound+Test&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/best-5.1-surround-sound-test-by-dts/",
    },
];

/// The eight home categories, shared by the home rows and the category
/// drill-down page.
/// Restart the app with a maintenance operation: the fresh process
/// performs it before any server component opens the data dir.
fn relaunch_with_maintenance(op: &str) {
    match std::env::current_exe() {
        Ok(exe) => match std::process::Command::new(exe)
            .env("STREAMX_MAINTENANCE", op)
            .spawn()
        {
            Ok(_) => std::process::exit(0),
            Err(e) => tracing::error!("maintenance relaunch failed to spawn: {e}"),
        },
        Err(e) => tracing::error!("maintenance relaunch: current_exe failed: {e}"),
    }
}

/// The current year as a leaked static string: browse sections and
/// category titles want `&'static str`, and the year changes at most
/// once per process lifetime.
fn current_year_title() -> &'static str {
    static YEAR: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    YEAR.get_or_init(|| chrono::Utc::now().format("%Y").to_string())
}

pub fn category_specs() -> [crate::state::CategorySpec; 9] {
    use streamx_api::client::BrowseParams;
    let p = |sort: &str, genre: Option<&str>, min: Option<u32>| BrowseParams {
        sort_by: Some(sort.into()),
        genre: genre.map(String::from),
        minimum_rating: min,
        ..Default::default()
    };
    [
        crate::state::CategorySpec {
            title: current_year_title(),
            params: streamx_api::client::BrowseParams {
                sort_by: Some("download_count".into()),
                query_term: Some(current_year_title().into()),
                ..Default::default()
            },
        },
        crate::state::CategorySpec {
            title: "Latest",
            params: p("date_added", None, None),
        },
        crate::state::CategorySpec {
            title: "Most Popular",
            params: p("download_count", None, None),
        },
        crate::state::CategorySpec {
            title: "Top Rated",
            params: p("rating", None, Some(8)),
        },
        crate::state::CategorySpec {
            title: "Action",
            params: p("download_count", Some("action"), None),
        },
        crate::state::CategorySpec {
            title: "Comedy",
            params: p("download_count", Some("comedy"), None),
        },
        crate::state::CategorySpec {
            title: "Thriller",
            params: p("download_count", Some("thriller"), None),
        },
        crate::state::CategorySpec {
            title: "Sci-Fi",
            params: p("download_count", Some("sci-fi"), None),
        },
        crate::state::CategorySpec {
            title: "Horror",
            params: p("download_count", Some("horror"), None),
        },
    ]
}

/// Open a category drill-down page and fetch its first page.
pub async fn open_category(state: Arc<AppState>, spec: crate::state::CategorySpec) {
    *state.category.write() = Some(spec);
    state.category_items.write().clear();
    state
        .category_page
        .store(0, std::sync::atomic::Ordering::Relaxed);
    *state.category_done.write() = false;
    state.navigate(Page::CategoryBrowse);
    state.mark_dirty();
    load_category_page(&state).await;
}

/// Fetch the next page of the open category; appends deduped items.
pub async fn load_category_page(state: &Arc<AppState>) {
    {
        let mut loading = state.category_loading.write();
        if *loading || *state.category_done.read() {
            return;
        }
        *loading = true;
    }
    state.mark_dirty();
    let spec = state.category.read().clone();
    let Some(spec) = spec else {
        *state.category_loading.write() = false;
        return;
    };
    let next = state
        .category_page
        .load(std::sync::atomic::Ordering::Relaxed)
        + 1;
    let mut params = spec.params.clone();
    params.limit = Some(20);
    params.page = Some(next);
    let client = state.client.read().clone();
    match client.browse(&params).await {
        Ok(rows) if !rows.is_empty() => {
            let mut items = state.category_items.write();
            let existing: std::collections::HashSet<String> = items
                .iter()
                .map(|g| format!("{}-{:?}", g.title, g.year))
                .collect();
            let mut added = 0usize;
            for g in rows {
                let key = format!("{}-{:?}", g.title, g.year);
                if !existing.contains(&key) {
                    items.push(std::sync::Arc::new(g));
                    added += 1;
                }
            }
            drop(items);
            state
                .category_page
                .store(next, std::sync::atomic::Ordering::Relaxed);
            // A page of pure duplicates means the provider stopped
            // paginating; don't spin on it.
            if added == 0 {
                *state.category_done.write() = true;
            }
        }
        Ok(_) => {
            *state.category_done.write() = true;
        }
        Err(e) => {
            state.show_toast(format!("Browse failed: {e}"), ToastKind::Error);
            *state.category_done.write() = true;
        }
    }
    *state.category_loading.write() = false;
    state.mark_dirty();
}

/// One home section: clickable heading + horizontally scrolling tile
/// strip. Fixed height (`block_h`) so the sections can live in a
/// virtualized uniform list.
#[allow(clippy::too_many_arguments)]
fn home_section_block(
    spec: &crate::state::CategorySpec,
    groups: &[Arc<streamx_api::types::SearchResultGroup>],
    layout: crate::components::TileLayout,
    theme: Theme,
    block_h: f32,
    state: Arc<AppState>,
    weak: gpui::WeakEntity<MainView>,
) -> gpui::Div {
    let title = spec.title;
    let gap = crate::components::TILE_GAP * crate::theme::ui_scale();

    let mut strip = div()
        .id(SharedString::from(format!("row-scroll-{title}")))
        .flex()
        .gap(px(gap))
        .overflow_x_scroll()
        .pb(px(theme.space_2()))
        .min_h(px(layout.total_h + 20.0));

    if groups.is_empty() {
        for i in 0..8u32 {
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("skel-{title}-{i}")))
                    .w(px(layout.tile_w))
                    .h(px(layout.poster_h))
                    .rounded(px(theme.radius_md()))
                    .bg(theme.bg_panel())
                    .flex_shrink_0(),
            );
        }
    } else {
        for (i, g) in groups.iter().enumerate() {
            let g_click = g.clone();
            let g_trailer = g.clone();
            let weak = weak.clone();
            let weak_trailer = weak.clone();
            strip = strip.child(
                movie_tile(
                    g.as_ref(),
                    &theme,
                    format!("row-{title}-{i}"),
                    layout,
                    Some(Box::new(move |_ev, _w, cx| {
                        let _ = weak_trailer.update(cx, |this, cx| {
                            this.play_trailer_for(g_trailer.as_ref(), cx);
                        });
                    })),
                )
                .on_click(move |_ev, _window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        *this.state.selected_movie.write() = Some(g_click.clone());
                        this.state.navigate(Page::Movie);
                        cx.notify();
                    });
                }),
            );
        }
    }

    let spec_for_click = spec.clone();
    let weak_for_click = weak.clone();
    let heading = div()
        .id(SharedString::from(format!("cat-open-{title}")))
        .flex()
        .items_center()
        .gap(px(theme.space_1()))
        .cursor_pointer()
        .hover(|s| s.opacity(0.8))
        .child(section_title(SharedString::from(title), &theme))
        .child(
            div()
                .text_size(px(theme.fs_2()))
                .text_color(theme.fg_muted())
                .child("⌄"),
        )
        .on_click(move |_ev, _window, cx| {
            let state = state.clone();
            let spec = spec_for_click.clone();
            runtime::spawn_detached(async move { open_category(state, spec).await });
            let _ = weak_for_click.update(cx, |_, cx| cx.notify());
        });

    div()
        .h(px(block_h))
        .flex()
        .flex_col()
        .gap(px(theme.space_2()))
        .child(heading)
        .child(strip)
}

/// Virtualized tile grid: only the visible rows build elements, so large
/// grids scroll fluidly like a web page. When `flag_need_more` is true,
/// nearing the last row flips `state.category_need_more`, which the tick
/// loop turns into an infinite-scroll page fetch.
fn virtual_tile_grid(
    id: &'static str,
    items: Vec<Arc<streamx_api::types::SearchResultGroup>>,
    layout: crate::components::TileLayout,
    theme: Theme,
    state: Arc<AppState>,
    weak: gpui::WeakEntity<MainView>,
    flag_need_more: bool,
) -> gpui::UniformList {
    let cols = layout.per_row.max(1);
    let row_count = items.len().div_ceil(cols).max(1);
    let gap = crate::components::TILE_GAP * crate::theme::ui_scale();
    gpui::uniform_list(id, row_count, move |range, _window, _cx| {
        if flag_need_more && range.end + 2 >= row_count {
            state
                .category_need_more
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        range
            .map(|row| {
                let mut row_div = div().flex().gap(px(gap)).pb(px(gap));
                let start = row * cols;
                let end = (start + cols).min(items.len());
                for (i, item) in items.iter().enumerate().take(end).skip(start) {
                    let g = item.clone();
                    let g_click = g.clone();
                    let g_trailer = g.clone();
                    let weak = weak.clone();
                    let weak_trailer = weak.clone();
                    row_div = row_div.child(
                        movie_tile(
                            g.as_ref(),
                            &theme,
                            format!("{id}-{i}"),
                            layout,
                            Some(Box::new(move |_ev, _w, cx| {
                                let _ = weak_trailer.update(cx, |this, cx| {
                                    this.play_trailer_for(g_trailer.as_ref(), cx);
                                });
                            })),
                        )
                        .on_click(move |_ev, _window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                *this.state.selected_movie.write() = Some(g_click.clone());
                                this.state.navigate(Page::Movie);
                                cx.notify();
                            });
                        }),
                    );
                }
                row_div
            })
            .collect()
    })
}

async fn load_downloads(state: &Arc<AppState>) {
    *state.downloads_loading.write() = true;
    state.mark_dirty();
    let client = state.client.read().clone();
    match client.list_downloads().await {
        Ok(items) => *state.downloads.write() = items,
        Err(e) => tracing::debug!("Downloads load failed: {e}"),
    }
    *state.downloads_loading.write() = false;
    state.mark_dirty();
}

async fn load_history(state: &Arc<AppState>) {
    *state.history_loading.write() = true;
    state.mark_dirty();
    let client = state.client.read().clone();
    match client.history().await {
        Ok(resp) => *state.history.write() = resp.items,
        Err(e) => state.show_toast(format!("History failed: {e}"), ToastKind::Error),
    }
    *state.history_loading.write() = false;
    state.mark_dirty();
}

async fn load_favourites(state: &Arc<AppState>) {
    *state.favourites_loading.write() = true;
    state.mark_dirty();
    let client = state.client.read().clone();
    match client.favourites().await {
        Ok(resp) => *state.favourites.write() = resp.items,
        Err(e) => state.show_toast(format!("Favourites failed: {e}"), ToastKind::Error),
    }
    *state.favourites_loading.write() = false;
    state.mark_dirty();
}

pub async fn run_search(state: Arc<AppState>, query: String) {
    use std::sync::atomic::Ordering;
    let generation = state.search_generation.fetch_add(1, Ordering::SeqCst) + 1;
    // Empty query means "clear search" — restore the browse view.
    if query.trim().is_empty() {
        *state.query.write() = String::new();
        *state.search_results.write() = Vec::new();
        *state.search_in_flight.write() = false;
        state.mark_dirty();
        return;
    }
    *state.search_in_flight.write() = true;
    state.mark_dirty();
    *state.query.write() = query.clone();

    let client = state.client.read().clone();
    let result = client.search(&query, 1).await;
    // A newer search superseded this one while the request was in
    // flight; drop the stale response instead of replacing results.
    if state.search_generation.load(Ordering::SeqCst) != generation {
        return;
    }
    match result {
        Ok(resp) => {
            *state.search_results.write() =
                resp.results.into_iter().map(std::sync::Arc::new).collect();
            *state.connection_error.write() = None;
        }
        Err(e) => {
            state.show_toast(format!("Search failed: {e}"), ToastKind::Error);
            *state.search_results.write() = Vec::new();
        }
    }
    *state.search_in_flight.write() = false;
    state.mark_dirty();
}

/// Extract the btih info hash from a magnet URI, lowercased.
pub fn info_hash_from_magnet(magnet: &str) -> Option<String> {
    let idx = magnet.find("xt=urn:btih:")?;
    let rest = &magnet[idx + "xt=urn:btih:".len()..];
    let end = rest.find('&').unwrap_or(rest.len());
    let hash = &rest[..end];
    if hash.is_empty() {
        None
    } else {
        Some(hash.to_ascii_lowercase())
    }
}

fn first_tile(b: &BrowseData) -> Option<Arc<streamx_api::types::SearchResultGroup>> {
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Refresh the global UI scale from the window size before any
        // Theme accessor runs: fonts, spacing, menus, inputs and buttons
        // all track it.
        let viewport_w: f32 = window.viewport_size().width.into();
        crate::theme::set_viewport_width(viewport_w);
        // Keyboard shortcuts need a focus target. Focus set before the
        // first render is dropped by GPUI, leaving NO dispatch path (keys
        // go nowhere until the user clicks) — reclaim the root focus
        // whenever nothing holds focus.
        if window.focused(cx).is_none() {
            self.focus_handle.focus(window, cx);
        }
        let theme = self.theme;
        let page = self.state.current_page();
        // On page transitions, stash the outgoing page's scroll offset
        // (read before this frame's layout clamps it) and restore the
        // incoming page's saved offset; layout re-clamps to content.
        if self.last_scroll_page != Some(page) {
            if let Some(prev) = self.last_scroll_page {
                self.page_scroll_saved
                    .insert(prev, self.page_scroll.offset());
            }
            let restored = self
                .page_scroll_saved
                .get(&page)
                .copied()
                .unwrap_or_default();
            self.page_scroll.set_offset(restored);
            self.last_scroll_page = Some(page);
        }
        let drawer_open = *self.state.drawer_open.read();
        let toast = self.state.toast.read().clone();

        let content = match page {
            Page::Login => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.bg_app())
                .child(self.login_page_view(window, cx))
                .into_any_element(),
            Page::Search => self.search_page_view(viewport_w, cx).into_any_element(),
            Page::CategoryBrowse => self.category_page_view(viewport_w, cx).into_any_element(),
            Page::Movie => self.movie_page_view(viewport_w, cx).into_any_element(),
            Page::Player => self.player_page_view(viewport_w, cx).into_any_element(),
            Page::Loading => loading_page(&theme, "loading…").into_any_element(),
            Page::History => self.history_page_view(cx).into_any_element(),
            Page::Downloads => self.downloads_page_view(cx).into_any_element(),
            Page::Favourites => self.favourites_page_view(cx).into_any_element(),
            Page::Settings => self.settings_page_view(cx).into_any_element(),
            Page::Logs => self.logs_page_view(cx).into_any_element(),
            Page::Admin => self.admin_page_view(cx).into_any_element(),
            Page::MusicSearch => self.music_search_page_view(cx).into_any_element(),
            Page::MusicPlayer => stub_page(
                &theme,
                "Now playing",
                "Dedicated audio player lands in Phase 5 follow-up.",
            )
            .into_any_element(),
            Page::TvSearch => self.tv_search_page_view(cx).into_any_element(),
            Page::TvShow => self.tv_show_page_view(cx).into_any_element(),
            Page::MusicVideoSearch => self.music_video_search_page_view(cx).into_any_element(),
            Page::SurroundSound => self.surround_sound_page_view(cx).into_any_element(),
        };

        let mut root = div()
            .track_focus(&self.focus_handle)
            .key_context("StreamX")
            .size_full()
            .bg(theme.bg_app())
            .text_color(theme.fg_primary())
            .flex()
            .flex_col()
            .relative()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                // Tab cycles the login form fields.
                if matches!(this.state.current_page(), Page::Login)
                    && ev.keystroke.key.as_str() == "tab"
                {
                    let order: Vec<Entity<TextInput>> = if this.login_create_mode {
                        vec![
                            this.username_input.clone(),
                            this.password_input.clone(),
                            this.repeat_input.clone(),
                        ]
                    } else {
                        vec![this.username_input.clone(), this.password_input.clone()]
                    };
                    let focused = order.iter().position(|i| i.read(cx).is_focused(window));
                    let backwards = ev.keystroke.modifiers.shift;
                    let next = match focused {
                        Some(i) if backwards => (i + order.len() - 1) % order.len(),
                        Some(i) => (i + 1) % order.len(),
                        None => 0,
                    };
                    order[next].read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                    return;
                }
                let focused_is_input = this.username_input.read(cx).is_focused(window)
                    || this.password_input.read(cx).is_focused(window)
                    || this.repeat_input.read(cx).is_focused(window)
                    || this.url_input.read(cx).is_focused(window)
                    || this.search_input.read(cx).is_focused(window)
                    || this.admin_kill_input.read(cx).is_focused(window)
                    || this.music_input.read(cx).is_focused(window)
                    || this.music_video_input.read(cx).is_focused(window)
                    || this.tv_input.read(cx).is_focused(window);
                if focused_is_input {
                    // Still let Escape close the drawer / unfocus search.
                    if ev.keystroke.key.as_str() == "escape"
                        && this.search_input.read(cx).is_focused(window)
                    {
                        let fh = this.focus_handle.clone();
                        fh.focus(window, cx);
                        cx.notify();
                    }
                    return;
                }
                if let Some(s) = translate(ev) {
                    this.handle_shortcut(s, window, cx);
                }
            }))
            .child(self.title_bar(window, cx))
            .child(self.app_header(cx))
            .child(
                div()
                    .id("page-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.page_scroll)
                    .child(content),
            );

        if let Some(t) = toast {
            root = root.child(self.toast_view(&t));
        }
        if drawer_open {
            root = root.child(self.drawer_view(cx));
        }

        // Resize borders (client-side decorations). Only render on Linux.
        #[cfg(target_os = "linux")]
        {
            for b in Self::resize_borders() {
                root = root.child(b);
            }
        }

        root
    }
}
