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

use std::sync::Arc;

use gpui::{px, AppContext, Application, SharedString, WindowBounds, WindowKind, WindowOptions};
use streamx_desktop::{
    app::MainView,
    asset_source::PosterAssetSource,
    runtime,
    state::{AppState, Mode},
};

fn main() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let (log_tx, _keep) = tokio::sync::broadcast::channel::<String>(1000);
    let (broadcast_layer, log_history) = streamx::logging::BroadcastLayer::new(log_tx.clone());
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // Graphics/windowing crates chatter at INFO on every
        // resize (blade surface reconfigure etc.) — keep them at
        // warn so app logs stay readable and resize stays cheap.
        tracing_subscriber::EnvFilter::new(
            "warn,streamx_desktop=info,streamx=info,streamx_api=info",
        )
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(broadcast_layer)
        .init();

    runtime::init();

    // Admin-page Clean/Wipe relaunch the app with this env var so the
    // deletion runs before any server component holds the data dir.
    if let Ok(op) = std::env::var("STREAMX_MAINTENANCE") {
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
        match streamx::config::load_config(&cli) {
            Ok(config) => {
                let res = match op.as_str() {
                    "clean" => streamx::maintenance::clean(&config),
                    "wipe" => streamx::maintenance::wipe(&config),
                    other => {
                        tracing::warn!(op = other, "unknown maintenance operation; skipping");
                        Ok(())
                    }
                };
                match res {
                    Ok(()) => tracing::info!(op = %op, "maintenance complete"),
                    Err(e) => tracing::error!(op = %op, "maintenance failed: {e}"),
                }
            }
            Err(e) => tracing::error!("maintenance skipped, config failed to load: {e}"),
        }
    }

    tracing::info!("starting StreamX desktop");
    let state = AppState::with_logs(log_history.clone());
    tracing::info!(
        mode = state.mode.read().as_str(),
        server = %state.server_url.read(),
        "initial state"
    );

    if *state.mode.read() == Mode::Embedded
        && std::env::var("STREAMX_DESKTOP_NO_EMBED").ok().as_deref() != Some("1")
    {
        spawn_embedded(&state, log_tx.clone(), log_history.clone());
    }

    streamx_desktop::update_check::spawn(state.clone());
    streamx_desktop::health_check::spawn(state.clone());

    #[cfg(feature = "ui-test")]
    streamx_desktop::test_driver::maybe_start(&state);

    // Poster loading is filesystem + direct HTTP; it never waits on the
    // embedded server, so it can be installed before anything boots.
    let asset_source = PosterAssetSource::new(state.clone());

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

        // Closing the last window quits: without this the process
        // lingers in the Dock as "running" with no window to reopen.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.open_window(options, |window, cx| {
            cx.new(|cx| MainView::new(state.clone(), window, cx))
        })
        .expect("open main window");
    });
}

fn spawn_embedded(
    state: &Arc<AppState>,
    log_tx: tokio::sync::broadcast::Sender<String>,
    log_history: Arc<streamx::logging::LogHistory>,
) {
    let state = state.clone();
    runtime::spawn_detached(async move {
        // Honor the same admin-seeding env vars as the server binary, so
        // a fresh install can bootstrap its admin straight from the
        // desktop app: STREAMX_ADMIN_USER + STREAMX_ADMIN_PASSWORD.
        let env_opt = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let cli = streamx::cli::Cli {
            command: None,
            port: None,
            bind: None,
            data_dir: None,
            config: None,
            log_level: None,
            log_dir: None,
            open: false,
            admin_user: env_opt("STREAMX_ADMIN_USER"),
            admin_password: env_opt("STREAMX_ADMIN_PASSWORD"),
        };
        let config = match streamx::config::load_config(&cli) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "embedded server: failed to load config");
                *state.connection_error.write() = Some(format!("Server failed to start: {e}"));
                state.mark_dirty();
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

        let components = match streamx::runner::build_components(
            config,
            Some(log_tx),
            Some(log_history),
        )
        .await
        {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::error!(error = %e, "embedded server: build_components failed");
                return;
            }
        };

        // Install the in-process backend for API calls. Posters load
        // through the PosterAssetSource (filesystem + direct HTTP) and
        // don't depend on this.
        let local_api = Arc::new(streamx::LocalApi::new(
            components.clone(),
            loopback_url.clone(),
        ));
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
