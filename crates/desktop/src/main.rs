//! StreamX native desktop client.
//!
//! Run (with server started on :8999):
//!   cargo run --manifest-path crates/desktop/Cargo.toml
//!
//! Environment:
//!   STREAMX_URL                default http://localhost:8999
//!   STREAMX_USERNAME           auto-login on startup
//!   STREAMX_PASSWORD           auto-login on startup
//!   STREAMX_DESKTOP_NO_EMBED=1 skip spawning the embedded server even if
//!                              the saved mode is Embedded (useful when
//!                              you already run one externally)

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use std::sync::{Arc, OnceLock};

use gpui::{px, AppContext, Application, SharedString, WindowBounds, WindowKind, WindowOptions};
use streamx_desktop::{
    app::MainView,
    asset_source::LocalApiAssetSource,
    runtime,
    state::{AppState, Mode},
};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,streamx_desktop=debug,streamx=info")
            }),
        )
        .init();

    runtime::init();

    tracing::info!("starting StreamX desktop");
    let state = AppState::new();
    tracing::info!(
        mode = state.mode.read().as_str(),
        server = %state.server_url.read(),
        "initial state"
    );

    // Shared slot: the embedded server fills it once it's done bootstrapping,
    // and the AssetSource reads from it lazily. Starts empty; image loads
    // before that fall through to the default (no-op) loader.
    let local_api_slot: Arc<OnceLock<Arc<streamx::LocalApi>>> = Arc::new(OnceLock::new());

    if *state.mode.read() == Mode::Embedded
        && std::env::var("STREAMX_DESKTOP_NO_EMBED").ok().as_deref() != Some("1")
    {
        spawn_embedded(&state, local_api_slot.clone());
    }

    let asset_source = LocalApiAssetSource::new(local_api_slot.clone());

    Application::new().with_assets(asset_source).run(move |cx| {
        let bounds = gpui::Bounds::centered(None, gpui::size(px(1100.0), px(720.0)), cx);

        // Client-side decorations on Linux so we can draw our own titlebar
        // and the window manager still allows native resize/min/max via
        // the xdg-decoration protocol. Matches nocapsec's setup.
        #[cfg(target_os = "linux")]
        let window_decorations = Some(gpui::WindowDecorations::Client);
        #[cfg(not(target_os = "linux"))]
        let window_decorations = None;

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(SharedString::from("StreamX")),
                appears_transparent: true,
                ..Default::default()
            }),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            is_movable: true,
            window_min_size: Some(gpui::size(px(720.0), px(480.0))),
            window_decorations,
            app_id: Some("streamx-desktop".to_string()),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            cx.new(|cx| MainView::new(state.clone(), window, cx))
        })
        .expect("open main window");
    });
}

fn spawn_embedded(state: &Arc<AppState>, local_api_slot: Arc<OnceLock<Arc<streamx::LocalApi>>>) {
    let state = state.clone();
    let _ = runtime::spawn(async move {
        let cli = streamx::cli::Cli {
            command: None,
            port: None,
            bind: None,
            data_dir: None,
            config: None,
            log_level: None,
            log_dir: None,
            open: false,
            admin_user: None,
            admin_password: None,
        };
        let config = match streamx::config::load_config(&cli) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "embedded server: failed to load config");
                return;
            }
        };
        let bind_addr = config.server.bind.clone();
        let port = config.server.port;
        let loopback_url = format!("http://{}:{}", bind_addr, port);
        tracing::info!(
            data_dir = %config.data_dir.display(),
            port = port,
            "embedded server: building components"
        );

        let components = match streamx::runner::build_components(config, None, None).await {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::error!(error = %e, "embedded server: build_components failed");
                return;
            }
        };

        // Install the in-process backend for both the Client (API calls)
        // and the AssetSource (poster image fetches).
        let local_api = Arc::new(streamx::LocalApi::new(
            components.clone(),
            loopback_url.clone(),
        ));
        // Fill the asset slot BEFORE installing the data client. Browse
        // data only flows once the client is installed, and the first
        // render that draws posters happens right after. If the slot
        // were set second, an in-process browse fetch could return and
        // render posters in the gap, caching blank images (GPUI never
        // retries a failed asset load).
        let _ = local_api_slot.set(local_api.clone());
        state.install_in_process_client(local_api);
        tracing::info!(base_url = %loopback_url, "embedded server: in-process Api installed");

        // Start the HTTP listener for other clients (web UI, phone, etc.)
        // on the same tokio runtime. Owned copies via clone; all inner
        // handles are Arc so this is cheap.
        let components_for_serve = (*components).clone();
        let addr: std::net::SocketAddr = match format!("{bind_addr}:{port}").parse() {
            Ok(a) => a,
            Err(_) => {
                tracing::error!("embedded server: invalid bind address");
                return;
            }
        };
        if let Err(e) = streamx::runner::serve_app(components_for_serve, addr).await {
            tracing::error!(error = %e, "embedded server: serve_app exited with error");
        }
    });
}
