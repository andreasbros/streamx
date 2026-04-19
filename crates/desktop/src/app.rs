//! Main window view: owns input entities, dispatches pages, drives async
//! bootstrap + login + search + playback.

use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, img, px, App, AppContext, Context, CursorStyle, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ObjectFit, ParentElement, Render,
    ResizeEdge, SharedString, StatefulInteractiveElement, Styled, StyledImage, Window,
};
use parking_lot::Mutex;
use streamx_api::client::Client;

use crate::components::{
    card, frost_card, movie_tile, primary_button, section_title, TILE_POSTER_H, TILE_POSTER_W,
    TILE_TOTAL_H,
};
use crate::keybindings::{translate, Shortcut};
use crate::pages::{loading_page, movie_page, stub_page};
use crate::playback;
use crate::playback::ipc::{MpvIpc, Snapshot};
use crate::playback::{MpvInstance, PlayTarget};
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
    pub mpv: Option<MpvInstance>,
    pub ipc: Option<MpvIpc>,
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
    url_input: Entity<TextInput>,
    search_input: Entity<TextInput>,
    admin_kill_input: Entity<TextInput>,
    music_input: Entity<TextInput>,
    music_video_input: Entity<TextInput>,
    tv_input: Entity<TextInput>,

    player: Arc<Mutex<PlayerState>>,
}

impl MainView {
    pub fn new(state: Arc<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let theme = Theme::new();
        let username_input = text_input(cx, "username");
        let password_input = cx.new(|c| {
            crate::text_input::TextInput::new(c)
                .with_placeholder("password")
                .password()
        });
        let url_input = cx.new(|c| {
            crate::text_input::TextInput::new(c)
                .with_placeholder("http://localhost:8999")
                .initial(state.server_url.read().clone())
        });
        let search_input = text_input(cx, "search · press / or Ctrl+K");
        let admin_kill_input = text_input(cx, "stream id");
        let music_input = text_input(cx, "artist or album");
        let music_video_input = text_input(cx, "music video");
        let tv_input = text_input(cx, "TV show");

        let username_focus = username_input.read(cx).focus_handle(cx);
        username_focus.focus(window, cx);

        let this = Self {
            state: state.clone(),
            theme,
            focus_handle,
            username_input,
            password_input,
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
            let client = state.client.read().clone();
            match runtime::spawn(async move { client.version().await }).await {
                Ok(v) => {
                    *state.server_version.write() = Some(v.version);
                    *state.server_hash.write() = Some(v.hash);
                    *state.connection_error.write() = None;
                }
                Err(e) => {
                    *state.connection_error.write() = Some(format!("server unreachable: {e}"));
                }
            }
            let _ = this.update(cx, |_, cx| cx.notify());

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
        let mut last_search_q: String = String::new();
        let mut last_search_typed_at: Option<std::time::Instant> = None;
        let mut last_music_q: String = String::new();
        let mut last_music_typed_at: Option<std::time::Instant> = None;
        let mut last_mv_q: String = String::new();
        let mut last_mv_typed_at: Option<std::time::Instant> = None;
        let mut last_tv_q: String = String::new();
        let mut last_tv_typed_at: Option<std::time::Instant> = None;
        let debounce = Duration::from_millis(350);
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;

                {
                    let mut p = player.lock();
                    if let Some(mpv) = p.mpv.as_mut() {
                        if let Ok(Some(_status)) = mpv.child.try_wait() {
                            p.mpv = None;
                            p.ipc = None;
                        }
                    }
                }

                // Poll mpv IPC for play-state snapshot (paused + time-pos + duration).
                let ipc_clone = player.lock().ipc.clone();
                if let Some(ipc) = ipc_clone {
                    let snap = runtime::spawn(async move { crate::playback::ipc::snapshot(&ipc).await }).await;
                    player.lock().snapshot = snap;
                }

                // Poll torrent status (peers, speed, progress) on the Player page.
                let sid = player.lock().stream_id.clone();
                if let Some(sid) = sid {
                    let client = state.client.read().clone();
                    let sid_clone = sid.clone();
                    if let Ok(ts) = runtime::spawn(async move { client.stream_status(&sid_clone).await }).await {
                        player.lock().torrent = Some(ts);
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

                fire_debounced(
                    &sv,
                    ss,
                    &mut last_search_q,
                    &mut last_search_typed_at,
                    debounce,
                    |q| {
                        let st = state.clone();
                        let _ = runtime::spawn(async move { run_search(st, q).await });
                    },
                );
                fire_debounced(
                    &mv,
                    ms,
                    &mut last_music_q,
                    &mut last_music_typed_at,
                    debounce,
                    |q| {
                        let st = state.clone();
                        let _ = runtime::spawn(async move { run_music_search(st, q).await });
                    },
                );
                fire_debounced(
                    &mvv,
                    mvs,
                    &mut last_mv_q,
                    &mut last_mv_typed_at,
                    debounce,
                    |q| {
                        let st = state.clone();
                        let _ = runtime::spawn(async move { run_music_video_search(st, q).await });
                    },
                );
                fire_debounced(
                    &tv,
                    ts,
                    &mut last_tv_q,
                    &mut last_tv_typed_at,
                    debounce,
                    |q| {
                        let st = state.clone();
                        let _ = runtime::spawn(async move { run_tv_search(st, q).await });
                    },
                );

                if this.update(cx, |_, cx| cx.notify()).is_err() {
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

    /// Generic path: build a CreateStreamRequest, navigate to Player, then
    /// poll stream_files + resolve + launch mpv. Used by movie variants,
    /// music/music-video tracks, and surround-sound demos.
    fn play_request(
        &mut self,
        req: streamx_api::types::CreateStreamRequest,
        cx: &mut Context<Self>,
    ) {
        // Kill any mpv child left over from a previous attempt so we
        // don't leave orphan windows behind.
        {
            let mut prev = self.player.lock();
            if let Some(ref mut mpv) = prev.mpv {
                let _ = mpv.child.kill();
            }
            *prev = PlayerState::default();
            prev.last_request = Some(req.clone());
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
            let resp = match runtime::spawn(async move { client.create_stream(&create_req).await }).await {
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
                            .or_else(|| {
                                files.iter().filter(|f| f.is_audio).max_by_key(|f| f.size)
                            })
                            .or_else(|| files.first());
                        if let Some(f) = pick {
                            file_index = f.index;
                            ready = true;
                        }
                        break;
                    }
                    _ => {
                        cx.background_executor()
                            .timer(Duration::from_secs(1))
                            .await;
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

            match playback::launch_mpv(&target, &theme) {
                Ok(instance) => {
                    let socket = instance.socket_path.clone();
                    {
                        let mut p = player.lock();
                        p.target = Some(target);
                        p.mpv = Some(instance);
                    }
                    // Connect IPC in the background once mpv has created the socket.
                    let player_ref = player.clone();
                    let _ = runtime::spawn(async move {
                        match MpvIpc::connect(&socket).await {
                            Ok(ipc) => {
                                player_ref.lock().ipc = Some(ipc);
                            }
                            Err(e) => {
                                tracing::warn!("mpv IPC connect failed: {e}");
                            }
                        }
                    });
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
    fn play_magnet(&mut self, magnet: Option<String>, title: String, api_base: Option<&'static str>, detail_url: Option<String>, cx: &mut Context<Self>) {
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

        *self.player.lock() = PlayerState::default();
        self.state.navigate(Page::Player);

        let state = self.state.clone();
        let player = self.player.clone();
        let this_self = cx.entity();
        cx.spawn(async move |_weak, cx: &mut gpui::AsyncApp| {
            let client = state.client.read().clone();
            let detail_clone = detail.clone();
            let resolved = runtime::spawn(async move {
                client.resolve_magnet(api_base, &detail_clone).await
            })
            .await;
            let magnet = match resolved {
                Ok(r) => r.magnet,
                Err(e) => {
                    player.lock().error = Some(format!("resolve_magnet failed: {e}"));
                    let _ = this_self.update(cx, |_, cx| cx.notify());
                    return;
                }
            };
            let req = streamx_api::types::CreateStreamRequest {
                magnet_uri: magnet,
                title: Some(title),
                ..Default::default()
            };
            let _ = this_self.update(cx, |view, cx| view.play_request(req, cx));
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
                .bg(if selected { theme.accent() } else { theme.bg_elevated() })
                .text_color(if selected { theme.fg_on_accent() } else { theme.fg_secondary() })
                .text_size(px(theme.fs_1()))
                .border_1()
                .border_color(if selected { theme.accent() } else { theme.border_default() })
                .cursor_pointer()
                .child(div().child(SharedString::from(label)))
                .on_click(cx.listener(move |this, _ev, _w, cx| {
                    this.state.set_mode(this_mode);
                    cx.notify();
                }))
        };

        let mode_row = div()
            .flex()
            .gap(px(theme.space_2()))
            .mb(px(theme.space_2()))
            .child(mode_pill("Embedded (local files)", Mode::Embedded))
            .child(mode_pill("Thin client (remote server)", Mode::ThinClient));

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
            None => SharedString::from(if server_ok { "connected" } else { "server offline" }),
        };

        let submit_label: SharedString = if in_flight {
            SharedString::from("Signing in… ⟳")
        } else {
            SharedString::from("Sign in")
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
            .child(
                primary_button("login-submit", submit_label, &theme)
                    .on_click(cx.listener(|this, _ev, _w, cx| this.submit_login(cx))),
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

    fn search_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
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
                    .max_w(px(480.0))
                    .child(self.search_input.clone()),
            )
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
            .child(header);

        if !query.is_empty() {
            root = root.child(
                section_title(
                    SharedString::from(format!("Results for \"{}\"", query)),
                    &theme,
                )
                .mb(px(theme.space_3())),
            );
            let mut grid = div()
                .flex()
                .flex_wrap()
                .gap(px(theme.space_3()));
            for (i, g) in results.iter().enumerate() {
                let clone = g.clone();
                grid = grid.child(
                    movie_tile(g, &theme, format!("search-{i}")).on_click(cx.listener(move |this, _ev, _w, cx| {
                        *this.state.selected_movie.write() = Some(clone.clone());
                        this.state.navigate(Page::Movie);
                        cx.notify();
                    })),
                );
            }
            if results.is_empty() && !searching {
                grid = grid.child(
                    div()
                        .text_size(px(theme.fs_2()))
                        .text_color(theme.fg_muted())
                        .child("No results."),
                );
            }
            root = root.child(grid);
        } else {
            let sections = [
                ("Latest", browse.latest.clone()),
                ("Most Popular", browse.popular.clone()),
                ("Top Rated", browse.top_rated.clone()),
                ("Action", browse.action.clone()),
                ("Comedy", browse.comedy.clone()),
                ("Thriller", browse.thriller.clone()),
                ("Sci-Fi", browse.scifi.clone()),
                ("Horror", browse.horror.clone()),
            ];
            let mut col = div().flex().flex_col();
            for (title, groups) in sections {
                col = col.child(self.browse_row(title, &groups, cx));
            }
            root = root.child(col);
        }

        root
    }

    fn browse_row(
        &self,
        title: &'static str,
        groups: &[streamx_api::types::SearchResultGroup],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.theme;
        // Horizontal scroll on the tile strip — trackpad / wheel works,
        // and on Wayland GPUI also surfaces touch scrolling.
        let mut row = div()
            .id(SharedString::from(format!("row-scroll-{title}")))
            .flex()
            .gap(px(theme.space_3()))
            .overflow_x_scroll()
            .pb(px(theme.space_2()))
            .min_h(px(TILE_TOTAL_H + 20.0));

        if groups.is_empty() {
            for i in 0..8u32 {
                row = row.child(
                    div()
                        .id(SharedString::from(format!("skel-{title}-{i}")))
                        .w(px(TILE_POSTER_W))
                        .h(px(TILE_POSTER_H))
                        .rounded(px(theme.radius_md()))
                        .bg(theme.bg_panel())
                        .flex_shrink_0(),
                );
            }
        } else {
            for (i, g) in groups.iter().enumerate() {
                let clone = g.clone();
                row = row.child(
                    movie_tile(g, &theme, format!("row-{title}-{i}")).on_click(cx.listener(move |this, _ev, _w, cx| {
                        *this.state.selected_movie.write() = Some(clone.clone());
                        this.state.navigate(Page::Movie);
                        cx.notify();
                    })),
                );
            }
        }

        div()
            .flex()
            .flex_col()
            .gap(px(theme.space_2()))
            .mb(px(theme.space_4()))
            .child(section_title(SharedString::from(title), &theme))
            .child(row)
    }

    fn player_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
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

        let movie = self.state.selected_movie.read().clone();
        let backdrop_url = movie
            .as_ref()
            .and_then(|m| {
                m.backdrop
                    .clone()
                    .or_else(|| m.poster_large.clone())
                    .or_else(|| m.poster_medium.clone())
            });
        let poster_url = movie
            .as_ref()
            .and_then(|m| {
                m.poster_medium
                    .clone()
                    .or_else(|| m.poster_large.clone())
                    .or_else(|| m.poster_small.clone())
            });
        let year = movie.as_ref().and_then(|m| m.year);
        let rating = movie.as_ref().and_then(|m| m.rating);
        let runtime = movie.as_ref().and_then(|m| m.runtime);
        let genres = movie
            .as_ref()
            .map(|m| m.genres.clone())
            .unwrap_or_default();
        let summary = movie.as_ref().and_then(|m| m.summary.clone());
        let title = movie.map(|m| m.title).unwrap_or_else(|| "Unknown title".into());

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

        content = content
            .child(self.back_hint(cx))
            .child(
                frost_card(&theme)
                    .flex()
                    .gap(px(theme.space_4()))
                    .items_start()
                    .child({
                        // Poster thumbnail. Loads via LocalApi in Embedded mode.
                        let mut poster = div()
                            .w(px(TILE_POSTER_W))
                            .h(px(TILE_POSTER_H))
                            .rounded(px(theme.radius_md()))
                            .overflow_hidden()
                            .bg(theme.bg_panel())
                            .border_1()
                            .border_color(theme.border_subtle())
                            .flex_shrink_0();
                        if let Some(url) = poster_url {
                            let src: gpui::ImageSource = if url.starts_with("/proxy/") {
                                gpui::ImageSource::Resource(gpui::Resource::Embedded(
                                    SharedString::from(url),
                                ))
                            } else {
                                gpui::ImageSource::from(SharedString::from(url))
                            };
                            poster = poster.child(
                                img(src)
                                    .w(px(TILE_POSTER_W))
                                    .h(px(TILE_POSTER_H))
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
                                    .child(SharedString::from(
                                        if genres.is_empty() {
                                            String::new()
                                        } else {
                                            genres.join(" · ")
                                        },
                                    )),
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
            let status = if playing { "Playing in mpv window" } else { "mpv exited" };
            let paused_label = if snap.paused { "Paused" } else { "Playing" };

            let fmt_time = |s: f64| -> String {
                let total = s.max(0.0) as u64;
                format!("{:02}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60)
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
                                .text_color(if playing { theme.accent() } else { theme.fg_muted() })
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
                                        .on_click(cx.listener(|this, _ev, _w, _cx| {
                                            let ipc = this.player.lock().ipc.clone();
                                            if let Some(ipc) = ipc {
                                                let _ = runtime::spawn(async move {
                                                    let _ = ipc.toggle_pause().await;
                                                });
                                            }
                                        })),
                                    )
                                    .child(
                                        primary_button("player-seek-back", "-10s", &theme)
                                            .on_click(cx.listener(|this, _ev, _w, _cx| {
                                                let ipc = this.player.lock().ipc.clone();
                                                if let Some(ipc) = ipc {
                                                    let _ = runtime::spawn(async move {
                                                        let _ = ipc.seek(-10.0, true).await;
                                                    });
                                                }
                                            })),
                                    )
                                    .child(
                                        primary_button("player-seek-fwd", "+30s", &theme)
                                            .on_click(cx.listener(|this, _ev, _w, _cx| {
                                                let ipc = this.player.lock().ipc.clone();
                                                if let Some(ipc) = ipc {
                                                    let _ = runtime::spawn(async move {
                                                        let _ = ipc.seek(30.0, true).await;
                                                    });
                                                }
                                            })),
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
            let src: gpui::ImageSource = if url.starts_with("/proxy/") {
                gpui::ImageSource::Resource(gpui::Resource::Embedded(SharedString::from(url)))
            } else {
                gpui::ImageSource::from(SharedString::from(url))
            };
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

    fn movie_page_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let movie = self.state.selected_movie.read().clone();
        let Some(m) = movie else {
            return movie_page(&self.state, &theme).into_any_element();
        };

        let mut root = div()
            .w_full()
            .p(px(theme.space_5()))
            .bg(theme.bg_app())
            .flex()
            .flex_col()
            .gap(px(theme.space_2()));

        root = root
            .child(self.back_hint(cx))
            .child(
                div()
                    .text_size(px(theme.fs_6()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.fg_primary())
                    .child(SharedString::from(m.title.clone())),
            )
            .child(
                div()
                    .flex()
                    .gap(px(theme.space_2()))
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_secondary())
                    .child(SharedString::from(
                        m.year.map(|y| y.to_string()).unwrap_or_default(),
                    ))
                    .child(SharedString::from(
                        m.rating.map(|r| format!("★ {:.1}", r)).unwrap_or_default(),
                    ))
                    .child(SharedString::from(if m.genres.is_empty() {
                        "".to_string()
                    } else {
                        m.genres.join(" · ")
                    })),
            )
            .child(
                div()
                    .text_size(px(theme.fs_2()))
                    .text_color(theme.fg_secondary())
                    .max_w(px(720.0))
                    .child(SharedString::from(m.summary.clone().unwrap_or_default())),
            )
            .child(
                div()
                    .text_size(px(theme.fs_5()))
                    .font_weight(gpui::FontWeight::BOLD)
                    .mt(px(theme.space_4()))
                    .child("Variants"),
            );

        for (i, v) in m.variants.iter().enumerate() {
            let quality = v.quality.clone().unwrap_or_else(|| "?".into());
            let codec = v.video_codec.clone().unwrap_or_default();
            let audio = v.audio_channels.clone().unwrap_or_default();
            let size = v.size.clone();
            let seeds = v.seeds;
            let leeches = v.leeches;

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

            root = root.child(row);
        }

        root.into_any_element()
    }

    /// Kick off an async data load for a page if data isn't already
    /// cached or in flight.
    fn ensure_loaded_for(&self, page: Page) {
        let state = self.state.clone();
        match page {
            Page::Search => {
                if state.browse.read().latest.is_empty() && !*state.browse_loading.read() {
                    let _ = runtime::spawn(async move { load_browse(&state).await });
                }
            }
            Page::History => {
                if !*state.history_loading.read() {
                    let _ = runtime::spawn(async move { load_history(&state).await });
                }
            }
            Page::Favourites => {
                if !*state.favourites_loading.read() {
                    let _ = runtime::spawn(async move { load_favourites(&state).await });
                }
            }
            Page::MusicSearch => {
                if state.music_results.read().is_empty() && !*state.music_loading.read() {
                    let _ = runtime::spawn(async move { load_music(&state).await });
                }
            }
            Page::MusicVideoSearch => {
                if state.music_video_results.read().is_empty()
                    && !*state.music_video_loading.read()
                {
                    let _ = runtime::spawn(async move { load_music_videos(&state).await });
                }
            }
            Page::TvSearch => {
                if state.tv_results.read().is_empty() && !*state.tv_loading.read() {
                    let _ = runtime::spawn(async move { load_tv(&state).await });
                }
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
                    item.year.map(|y| y.to_string()).unwrap_or_else(|| "—".into()),
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
                let rating = fav.rating.map(|r| format!("★ {:.1}", r)).unwrap_or_default();
                let query = fav.title.clone();
                grid = grid.child(
                    div()
                        .id(SharedString::from(format!("fav-{}", i)))
                        .w(px(160.0))
                        .p(px(theme.space_2()))
                        .rounded(px(theme.radius_md()))
                        .bg(theme.bg_surface())
                        .border_1()
                        .border_color(theme.border_subtle())
                        .cursor_pointer()
                        .hover(|s| s.border_color(theme.accent()))
                        .flex()
                        .flex_col()
                        .gap(px(theme.space_1()))
                        .child(
                            div()
                                .text_size(px(theme.fs_2()))
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
                                .child(SharedString::from(rating)),
                        )
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            // Re-search for this title and jump to Search page.
                            this.state.replace_page(Page::Search);
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
                .id(SharedString::from(format!("settings-mode-{}", this_mode.as_str())))
                .px(px(theme.space_3()))
                .py(px(theme.space_2()))
                .rounded(px(theme.radius_md()))
                .bg(if selected { theme.accent() } else { theme.bg_elevated() })
                .text_color(if selected { theme.fg_on_accent() } else { theme.fg_secondary() })
                .text_size(px(theme.fs_1()))
                .border_1()
                .border_color(if selected { theme.accent() } else { theme.border_default() })
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
            .map(|u| SharedString::from(format!("@{} · {}", u.username, if u.is_admin { "admin" } else { "user" })))
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

        let hint = if loading { "loading… ⟳" } else { "Enter to search · Esc back" };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(theme.space_3()))
            .mb(px(theme.space_4()))
            .child(div().flex_1().max_w(px(480.0)).child(input))
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
                    .child(if loading { "Searching…" } else { "No results." }),
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
                            .on_click(cx.listener(move |this, _ev, _w, cx| {
                                this.play_magnet(
                                    magnet.clone(),
                                    title_owned.clone(),
                                    Some(api_base),
                                    Some(detail_url.clone()),
                                    cx,
                                );
                            })),
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
        let hint = if loading { "loading… ⟳" } else { "Enter to search · Esc back" };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(theme.space_3()))
            .mb(px(theme.space_4()))
            .child(div().flex_1().max_w(px(480.0)).child(self.tv_input.clone()))
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
                    .child(if loading { "Searching…" } else { "No results." }),
            );
        } else {
            let mut list = div().flex().flex_col().gap(px(theme.space_2()));
            for (i, g) in results.iter().enumerate() {
                let show_name = SharedString::from(g.show_name.clone());
                let season_count = g.seasons.len();
                let ep_count: usize = g.seasons.iter().map(|s| s.episodes.len()).sum();
                let meta = SharedString::from(format!(
                    "{season_count} seasons · {ep_count} episodes"
                ));
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
                            .on_click(cx.listener(move |this, _ev, _w, cx| {
                                this.play_magnet(Some(magnet.clone()), title.clone(), None, None, cx);
                            })),
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
                        primary_button(
                            SharedString::from(format!("ss-play-{i}")),
                            "Play",
                            &theme,
                        )
                        .on_click(cx.listener(move |this, _ev, _w, cx| {
                            this.play_magnet(Some(magnet.clone()), title.clone(), None, None, cx);
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
                    this.state.replace_page(page);
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
            .child(link("Music", Page::MusicSearch))
            .child(link("Music Videos", Page::MusicVideoSearch))
            .child(link("Favourites", Page::Favourites))
            .child(link("History", Page::History))
            .child(link("Surround Sound", Page::SurroundSound))
            .child(link("Settings", Page::Settings));

        if is_admin {
            items = items.child(link("Admin", Page::Admin));
        }

        // Full-screen overlay with a 280px panel on the left.
        div()
            .absolute()
            .inset_0()
            .bg(theme.bg_overlay())
            .flex()
            .child(
                div()
                    .w(px(280.0))
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
                            .text_size(px(theme.fs_5()))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.accent_text())
                            .child("StreamX"),
                    )
                    .child(items),
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
            let hover_bg = if is_close { theme.error() } else { theme.bg_elevated() };
            let hover_fg = if is_close { theme.fg_on_accent() } else { theme.fg_primary() };
            div()
                .id(SharedString::from(format!("win-ctrl-{id}")))
                .w(px(28.0))
                .h(px(20.0))
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

        div()
            .id("title-bar")
            .w_full()
            .h(px(32.0))
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
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme.space_2()))
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_muted())
                    .child("StreamX"),
            )
            .child(
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

    /// App header: drawer button, logo (click → home), current page title,
    /// forward/back nav arrows, user badge. Lives under the title bar.
    fn app_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let page = self.state.current_page();
        let can_go_back = self.state.page_stack.read().len() > 1;
        let user = self.state.user.read().clone();

        let nav_arrow = |id: &'static str, icon: &'static str, enabled: bool| {
            let color = if enabled { theme.fg_secondary() } else { theme.fg_disabled() };
            div()
                .id(SharedString::from(format!("nav-{id}")))
                .w(px(28.0))
                .h(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme.radius_sm()))
                .text_size(px(theme.fs_3()))
                .text_color(color)
                .when(enabled, |el| {
                    el.cursor_pointer().hover(move |s| s.bg(theme.bg_elevated()))
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
                            .text_size(px(theme.fs_3()))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.accent_text())
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.accent()))
                            .child("StreamX")
                            .on_click(cx.listener(|this, _ev, _w, cx| {
                                this.state.replace_page(Page::Search);
                                this.ensure_loaded_for(Page::Search);
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
                    .child(
                        nav_arrow("back", "◀", can_go_back)
                            .on_click(cx.listener(|this, _ev, _w, cx| {
                                if this.state.back() {
                                    cx.notify();
                                }
                            })),
                    )
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
                div().absolute().bottom_0().left(px(c)).right(px(c)).h(px(e)),
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
    let client: Client = state.client.read().clone();
    let sections: [(&str, BrowseParams); 8] = [
        ("latest",   BrowseParams { sort_by: Some("date_added".into()),     limit: Some(10), ..Default::default() }),
        ("popular",  BrowseParams { sort_by: Some("download_count".into()), limit: Some(10), ..Default::default() }),
        ("top_rated",BrowseParams { sort_by: Some("rating".into()), minimum_rating: Some(8), limit: Some(10), ..Default::default() }),
        ("action",   BrowseParams { sort_by: Some("download_count".into()), genre: Some("action".into()),   limit: Some(10), ..Default::default() }),
        ("comedy",   BrowseParams { sort_by: Some("download_count".into()), genre: Some("comedy".into()),   limit: Some(10), ..Default::default() }),
        ("thriller", BrowseParams { sort_by: Some("download_count".into()), genre: Some("thriller".into()), limit: Some(10), ..Default::default() }),
        ("scifi",    BrowseParams { sort_by: Some("download_count".into()), genre: Some("sci-fi".into()),   limit: Some(10), ..Default::default() }),
        ("horror",   BrowseParams { sort_by: Some("download_count".into()), genre: Some("horror".into()),   limit: Some(10), ..Default::default() }),
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

/// Debounce + change-detection helper used by the tick loop.
/// Fires immediately when the user hit Enter (`submitted=true`); otherwise
/// waits for the value to stay stable for `debounce` ms with length >= 2
/// before firing. Clears `last_fired` when the field is emptied.
fn fire_debounced<F: FnOnce(String)>(
    current: &str,
    submitted: bool,
    last_fired: &mut String,
    last_typed_at: &mut Option<std::time::Instant>,
    debounce: Duration,
    run: F,
) {
    let trimmed = current.trim();
    let changed = trimmed != last_fired.as_str();
    if changed {
        *last_typed_at = Some(std::time::Instant::now());
    }
    if submitted && !trimmed.is_empty() {
        *last_fired = trimmed.to_string();
        *last_typed_at = None;
        run(trimmed.to_string());
        return;
    }
    let ready = last_typed_at
        .map(|t| t.elapsed() >= debounce)
        .unwrap_or(false);
    if ready && changed && trimmed.len() >= 2 {
        *last_fired = trimmed.to_string();
        *last_typed_at = None;
        run(trimmed.to_string());
    } else if ready && trimmed.is_empty() && !last_fired.is_empty() {
        *last_fired = String::new();
        *last_typed_at = None;
        // Fire with empty query so the page resets (state.query and
        // state.search_results cleared, browse view comes back).
        run(String::new());
    }
}


async fn run_music_search(state: Arc<AppState>, query: String) {
    *state.music_loading.write() = true;
    *state.music_query.write() = query.clone();
    let client = state.client.read().clone();
    match client.search_music(&query).await {
        Ok(resp) => *state.music_results.write() = resp.results,
        Err(e) => state.show_toast(format!("Music search failed: {e}"), ToastKind::Error),
    }
    *state.music_loading.write() = false;
}

async fn run_music_video_search(state: Arc<AppState>, query: String) {
    *state.music_video_loading.write() = true;
    *state.music_video_query.write() = query.clone();
    let client = state.client.read().clone();
    match client.search_music_videos(&query).await {
        Ok(resp) => *state.music_video_results.write() = resp.results,
        Err(e) => state.show_toast(format!("Music video search failed: {e}"), ToastKind::Error),
    }
    *state.music_video_loading.write() = false;
}

async fn run_tv_search(state: Arc<AppState>, query: String) {
    *state.tv_loading.write() = true;
    *state.tv_query.write() = query.clone();
    let client = state.client.read().clone();
    match client.search_tv(&query).await {
        Ok(resp) => *state.tv_results.write() = resp.results,
        Err(e) => state.show_toast(format!("TV search failed: {e}"), ToastKind::Error),
    }
    *state.tv_loading.write() = false;
}

async fn load_music(state: &Arc<AppState>) {
    *state.music_loading.write() = true;
    let client = state.client.read().clone();
    match client.browse_music(1).await {
        Ok(resp) => *state.music_results.write() = resp.results,
        Err(e) => state.show_toast(format!("Music browse failed: {e}"), ToastKind::Error),
    }
    *state.music_loading.write() = false;
}

async fn load_music_videos(state: &Arc<AppState>) {
    *state.music_video_loading.write() = true;
    let client = state.client.read().clone();
    match client.browse_music_videos(1).await {
        Ok(resp) => *state.music_video_results.write() = resp.results,
        Err(e) => state.show_toast(format!("Music video browse failed: {e}"), ToastKind::Error),
    }
    *state.music_video_loading.write() = false;
}

async fn load_tv(state: &Arc<AppState>) {
    *state.tv_loading.write() = true;
    let client = state.client.read().clone();
    match client.browse_tv(1).await {
        Ok(resp) => *state.tv_results.write() = resp.results,
        Err(e) => state.show_toast(format!("TV browse failed: {e}"), ToastKind::Error),
    }
    *state.tv_loading.write() = false;
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

async fn load_history(state: &Arc<AppState>) {
    *state.history_loading.write() = true;
    let client = state.client.read().clone();
    match client.history().await {
        Ok(resp) => *state.history.write() = resp.items,
        Err(e) => state.show_toast(format!("History failed: {e}"), ToastKind::Error),
    }
    *state.history_loading.write() = false;
}

async fn load_favourites(state: &Arc<AppState>) {
    *state.favourites_loading.write() = true;
    let client = state.client.read().clone();
    match client.favourites().await {
        Ok(resp) => *state.favourites.write() = resp.items,
        Err(e) => state.show_toast(format!("Favourites failed: {e}"), ToastKind::Error),
    }
    *state.favourites_loading.write() = false;
}

async fn run_search(state: Arc<AppState>, query: String) {
    // Empty query means "clear search" — restore the browse view.
    if query.trim().is_empty() {
        *state.query.write() = String::new();
        *state.search_results.write() = Vec::new();
        *state.search_in_flight.write() = false;
        return;
    }
    *state.search_in_flight.write() = true;
    *state.query.write() = query.clone();

    let client = state.client.read().clone();
    let result = client.search(&query, 1).await;
    match result {
        Ok(resp) => {
            *state.search_results.write() = resp.results;
            *state.connection_error.write() = None;
        }
        Err(e) => {
            state.show_toast(format!("Search failed: {e}"), ToastKind::Error);
            *state.search_results.write() = Vec::new();
        }
    }
    *state.search_in_flight.write() = false;
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let page = self.state.current_page();
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
            Page::Search => self.search_page_view(cx).into_any_element(),
            Page::Movie => self.movie_page_view(cx).into_any_element(),
            Page::Player => self.player_page_view(cx).into_any_element(),
            Page::Loading => loading_page(&theme, "loading…").into_any_element(),
            Page::History => self.history_page_view(cx).into_any_element(),
            Page::Favourites => self.favourites_page_view(cx).into_any_element(),
            Page::Settings => self.settings_page_view(cx).into_any_element(),
            Page::Admin => self.admin_page_view(cx).into_any_element(),
            Page::MusicSearch => self.music_search_page_view(cx).into_any_element(),
            Page::MusicPlayer => stub_page(&theme, "Now playing", "Dedicated audio player lands in Phase 5 follow-up.").into_any_element(),
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
                let focused_is_input = this.username_input.read(cx).is_focused(window)
                    || this.password_input.read(cx).is_focused(window)
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
