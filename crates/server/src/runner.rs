//! Library entrypoint used by the desktop app to spawn the server in-process.
//!
//! The `streamx` binary's `main.rs` composes these pieces for a standalone
//! server. The desktop crate calls [`build_components`] + [`serve_app`] to
//! run on its own tokio runtime, and also holds onto the [`ServerComponents`]
//! so a future `LocalApi` impl can call DB/engine/pipeline methods directly
//! (skipping HTTP) in embedded mode.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::info;

use crate::config::AppConfig;
use crate::db::Database;
use crate::error::{self, Result};
use crate::logging::{BroadcastLayer, LogHistory};
use crate::server;
use crate::torrent::{SearchProvider, TorrentEngine};
use crate::transcode::HlsManager;

/// Long-lived handles the server assembles during startup. Retained by
/// callers (currently the server binary + desktop in embedded mode) so
/// they can access data stores without going through HTTP.
#[derive(Clone)]
pub struct ServerComponents {
    pub database: Database,
    pub config: Arc<AppConfig>,
    pub torrent_engine: Arc<TorrentEngine>,
    pub search_provider: Arc<SearchProvider>,
    pub hls_pipeline: Arc<HlsManager>,
    pub log_tx: broadcast::Sender<String>,
    pub log_history: Arc<LogHistory>,
    /// Shared HTTP client used by proxy/image fetches. Desktop's
    /// AssetSource reuses this so poster downloads flow through the
    /// same connection pool as the server.
    pub http_client: reqwest::Client,
}

/// Handle returned when the server is spawned in-process.
pub struct EmbeddedHandle {
    pub components: ServerComponents,
    pub addr: SocketAddr,
    pub server_task: tokio::task::JoinHandle<Result<()>>,
}

/// Build all long-lived components. This is the part of startup that is
/// independent of the HTTP listener — admin creation, torrent/search/hls
/// wiring, database init. Broadcast + log history are created fresh if
/// `log_tx` is `None`; otherwise the existing pair is reused (useful for
/// binary startup which also wires tracing).
pub async fn build_components(
    config: AppConfig,
    log_tx: Option<broadcast::Sender<String>>,
    log_history: Option<Arc<LogHistory>>,
) -> Result<ServerComponents> {
    let config = Arc::new(config);

    // Embedded ffmpeg/ffprobe (embed-ffmpeg builds). PATH resolution
    // stays in place when extraction fails or the feature is off.
    if let Err(e) = crate::ffmpeg_bin::install(&config.data_dir) {
        tracing::warn!(error = %e, "embedded ffmpeg extraction failed; falling back to PATH");
    }

    // Database.
    let db_dir = config.data_dir.join("db");
    std::fs::create_dir_all(&db_dir).map_err(|e| error::Error::Config {
        message: format!("Failed to create database directory: {e}"),
    })?;
    let db_path = db_dir.join("streamx.db");
    let database = Database::open(&db_path)?;
    database.init().await?;
    info!("Database initialized");

    // Seed admin user if configured.
    if let (Some(admin_user), Some(admin_pass)) = (&config.admin_user, &config.admin_password) {
        match database.find_user_by_username(admin_user).await? {
            Some(_) => {
                info!(username = %admin_user, "Admin user already exists, skipping creation");
            }
            None => {
                let password_hash = server::auth::hash_password(admin_pass)?;
                database.create_user(admin_user, &password_hash).await?;
                info!(username = %admin_user, "Admin user created");
            }
        }
    }

    database.set_downloading_to_paused().await?;
    info!("Reset in-flight downloads to paused state");

    // Torrent + search + transcoder.
    let socks5 = config.vpn.as_ref().map(|v| v.resolved_url());
    if let Some(ref url) = socks5 {
        let safe = if let Some(at) = url.find('@') {
            let proto_end = url.find("://").unwrap_or(0) + 3;
            format!("{}***@{}", &url[..proto_end], &url[at + 1..])
        } else {
            url.clone()
        };
        info!(proxy = %safe, "VPN SOCKS5 proxy configured");
    }
    let torrent_engine = Arc::new(
        TorrentEngine::create(
            &config.torrent,
            &config.data_dir,
            database.clone(),
            socks5.clone(),
        )
        .await?,
    );
    torrent_engine.spawn_stall_watchdog();
    let search_provider = Arc::new(SearchProvider::new(config.providers.clone(), socks5));
    let cache_dir = config.data_dir.join("cache");
    let hls_pipeline = Arc::new(HlsManager::new(&config.transcode, cache_dir).await?);

    // Pinned (background) downloads resume on their own at boot; the
    // paused reset above only applies to viewer-driven downloads.
    {
        let engine = torrent_engine.clone();
        let db = database.clone();
        tokio::spawn(async move {
            match db.get_pinned_incomplete().await {
                Ok(hashes) => {
                    for hash in hashes {
                        info!(info_hash = %hash, "Resuming pinned download at boot");
                        if let Err(e) = engine.resume(&hash).await {
                            tracing::warn!(info_hash = %hash, "Pinned resume failed: {e}");
                        }
                    }
                }
                Err(e) => tracing::warn!("Could not list pinned downloads: {e}"),
            }
        });
    }

    let (log_tx, log_history) = match (log_tx, log_history) {
        (Some(tx), Some(h)) => (tx, h),
        _ => {
            let (tx, _rx) = broadcast::channel::<String>(1000);
            let (_layer, history) = BroadcastLayer::new(tx.clone());
            (tx, history)
        }
    };

    let mut http_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) StreamX/0.1");
    if let Some(ref vpn) = config.vpn {
        if let Ok(proxy) = reqwest::Proxy::all(vpn.resolved_url()) {
            http_builder = http_builder.proxy(proxy);
        }
    }
    let http_client = http_builder.build().unwrap_or_default();

    Ok(ServerComponents {
        database,
        config,
        torrent_engine,
        search_provider,
        hls_pipeline,
        log_tx,
        log_history,
        http_client,
    })
}

/// Serve the axum app built from a `ServerComponents` bundle. Consumes the
/// components (clones what's needed into the router) and returns once the
/// listener exits.
pub async fn serve_app(components: ServerComponents, bind: SocketAddr) -> Result<()> {
    let config_owned: AppConfig = (*components.config).clone();
    let state = server::build_state(
        components.database,
        config_owned,
        components.torrent_engine,
        components.search_provider,
        components.hls_pipeline,
        components.log_tx,
        components.log_history,
    );
    let app = server::build_router_with_state(state);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|source| error::Error::ServerBind {
            address: bind.to_string(),
            source,
        })?;

    info!(%bind, "Server listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|source| error::Error::ServerBind {
        address: bind.to_string(),
        source,
    })?;

    Ok(())
}

/// Convenience: build components + serve. Used by the binary.
pub async fn run_server(config: AppConfig) -> Result<()> {
    let bind_addr = format!("{}:{}", config.server.bind, config.server.port);
    let addr: SocketAddr = bind_addr.parse().map_err(|_| error::Error::Config {
        message: format!("Invalid bind address: {bind_addr}"),
    })?;

    let components = build_components(config, None, None).await?;
    serve_app(components, addr).await
}
