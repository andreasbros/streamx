//! StreamX native desktop client.
//!
//! Phase 4: search + browse with routing, keybindings, design tokens, and
//! full async I/O through the tokio bridge. Video playback lands in Phase 5.
//!
//! Run (with server started on :8999):
//!   cargo run --manifest-path crates/desktop/Cargo.toml
//!
//! Environment:
//!   STREAMX_URL       default http://localhost:8999
//!   STREAMX_USERNAME  auto-login on startup
//!   STREAMX_PASSWORD  auto-login on startup

#![allow(clippy::unwrap_used)] // Phase 4 scaffold.
#![allow(clippy::expect_used)] // Phase 4 scaffold.

mod app;
mod components;
mod keybindings;
mod pages;
mod router;
mod runtime;
mod state;
mod theme;

use app::MainView;
use gpui::{px, AppContext, Application, SharedString, WindowBounds, WindowOptions};
use state::AppState;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,streamx_desktop=debug")),
        )
        .init();

    runtime::init();

    let url = std::env::var("STREAMX_URL").unwrap_or_else(|_| "http://localhost:8999".to_string());
    tracing::info!(server = %url, "starting StreamX desktop");

    let state = AppState::new(url);

    Application::new().run(move |cx| {
        let bounds = gpui::Bounds::centered(None, gpui::size(px(1100.0), px(720.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(SharedString::from("StreamX")),
                appears_transparent: false,
                ..Default::default()
            }),
            focus: true,
            show: true,
            window_min_size: Some(gpui::size(px(720.0), px(480.0))),
            app_id: Some("streamx-desktop".to_string()),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            cx.new(|cx| MainView::new(state.clone(), window, cx))
        })
        .expect("open main window");
    });
}
