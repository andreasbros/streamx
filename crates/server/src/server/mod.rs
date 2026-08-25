pub mod admin;
pub mod api;
pub mod auth;
pub mod proxy;
pub mod static_files;
pub mod stream;

use crate::config::AppConfig;
use crate::db::Database;
use crate::torrent::{SearchProvider, TorrentEngine};
use crate::transcode::HlsManager;
use auth::RateLimiter;
use axum::extract::FromRef;
use axum::routing::{delete, get, post, put};
use axum::Router;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

#[derive(Clone, FromRef)]
pub struct AppState {
    pub db: Database,
    pub config: Arc<AppConfig>,
    pub jwt_secret: String,
    pub torrent_engine: Arc<TorrentEngine>,
    pub search_provider: Arc<SearchProvider>,
    pub hls_pipeline: Arc<HlsManager>,
    pub rate_limiter: RateLimiter,
    pub http_client: reqwest::Client,
    pub ws_connections: Arc<AtomicU32>,
    pub log_tx: tokio::sync::broadcast::Sender<String>,
    pub log_history: std::sync::Arc<crate::logging::LogHistory>,
}

pub fn build_state(
    db: Database,
    config: AppConfig,
    torrent_engine: Arc<TorrentEngine>,
    search_provider: Arc<SearchProvider>,
    hls_pipeline: Arc<HlsManager>,
    log_tx: tokio::sync::broadcast::Sender<String>,
    log_history: std::sync::Arc<crate::logging::LogHistory>,
) -> AppState {
    let jwt_secret = config.auth.jwt_secret.clone();

    let mut http_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) StreamX/0.1");
    if let Some(ref vpn) = config.vpn {
        if let Ok(proxy) = reqwest::Proxy::all(vpn.resolved_url()) {
            http_builder = http_builder.proxy(proxy);
            tracing::info!("HTTP client using SOCKS5 proxy");
        }
    }
    let http_client = http_builder.build().unwrap_or_default();

    AppState {
        db,
        config: Arc::new(config),
        jwt_secret,
        torrent_engine,
        search_provider,
        hls_pipeline,
        rate_limiter: RateLimiter::new(),
        http_client,
        ws_connections: Arc::new(AtomicU32::new(0)),
        log_tx,
        log_history,
    }
}

impl AppState {
    /// State view over already-built components. Lets LocalApi reuse
    /// HTTP-handler logic (e.g. cleanup_stream) in-process.
    pub fn from_components(c: &crate::runner::ServerComponents) -> Self {
        Self {
            db: c.database.clone(),
            config: c.config.clone(),
            jwt_secret: c.config.auth.jwt_secret.clone(),
            torrent_engine: c.torrent_engine.clone(),
            search_provider: c.search_provider.clone(),
            hls_pipeline: c.hls_pipeline.clone(),
            rate_limiter: RateLimiter::new(),
            http_client: c.http_client.clone(),
            ws_connections: Arc::new(AtomicU32::new(0)),
            log_tx: c.log_tx.clone(),
            log_history: c.log_history.clone(),
        }
    }
}

pub fn build_router(
    db: Database,
    config: AppConfig,
    torrent_engine: TorrentEngine,
    search_provider: SearchProvider,
    hls_pipeline: HlsManager,
    log_tx: tokio::sync::broadcast::Sender<String>,
    log_history: std::sync::Arc<crate::logging::LogHistory>,
) -> Router {
    let state = build_state(
        db,
        config,
        Arc::new(torrent_engine),
        Arc::new(search_provider),
        Arc::new(hls_pipeline),
        log_tx,
        log_history,
    );
    build_router_with_state(state)
}

pub fn build_router_with_state(state: AppState) -> Router {
    let auth_routes = Router::new()
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        .route("/me", get(auth::me));

    let search_routes = Router::new()
        .route("/", post(api::search))
        .route("/browse", get(api::browse))
        .route("/history", get(api::search_history));

    let stream_routes = Router::new()
        .route("/url/playlist.m3u8", get(stream::url_playlist))
        .route("/", post(api::create_stream))
        .route("/music", post(api::create_music_stream))
        .route("/{id}", get(api::get_stream))
        .route("/{id}", delete(api::delete_stream))
        .route("/{id}/pause", put(api::pause_stream))
        .route("/{id}/resume", put(api::resume_stream))
        .route("/{id}/download", post(api::pin_download))
        .route("/{id}/download", delete(api::unpin_download))
        .route("/{id}/share", post(api::share_stream))
        .route("/{id}/ws", get(stream::stream_ws))
        .route("/{id}/playlist.m3u8", get(stream::playlist))
        .route("/{id}/file", get(stream::stream_file))
        .route("/{id}/files", get(stream::list_stream_files))
        .route("/{id}/file/{file_index}", get(stream::stream_file_by_index))
        .route("/{id}/artwork/{file_index}", get(stream::stream_artwork))
        .route("/{id}/vlc/{token}", get(stream::stream_vlc))
        .route(
            "/{id}/{variant}/playlist.m3u8",
            get(stream::variant_playlist),
        )
        .route("/{id}/{variant}/{segment}", get(stream::variant_segment))
        .route("/{id}/{segment}", get(stream::segment));

    let history_routes = Router::new()
        .route("/", get(api::get_history))
        .route("/{id}", put(api::update_history))
        .route("/{id}", delete(api::delete_history));

    let settings_routes = Router::new()
        .route("/", get(api::get_settings))
        .route("/", put(api::update_settings))
        .route("/server", get(api::get_server_settings));

    let favourites_routes = Router::new()
        .route("/", post(api::add_favourite))
        .route("/", get(api::get_favourites))
        .route("/{id}", delete(api::delete_favourite));

    let tv_routes = Router::new()
        .route("/search", post(api::search_tv))
        .route("/browse", get(api::browse_tv))
        .route("/show/{imdb_id}", get(api::get_tv_show));

    let music_video_routes = Router::new()
        .route("/search", post(api::search_music_videos))
        .route("/browse", get(api::browse_music_videos))
        .route("/resolve-magnet", post(api::resolve_magnet));

    let music_routes = Router::new()
        .route("/search", post(api::search_music))
        .route("/browse", get(api::browse_music))
        .route("/resolve-magnet", post(api::resolve_magnet));

    let playlist_routes = Router::new()
        .route("/", post(api::create_playlist))
        .route("/", get(api::get_playlists))
        .route("/{id}", put(api::rename_playlist))
        .route("/{id}", delete(api::delete_playlist))
        .route("/{id}/tracks", get(api::get_playlist_tracks))
        .route("/{id}/tracks", post(api::add_playlist_track))
        .route(
            "/{id}/tracks/{track_id}",
            delete(api::remove_playlist_track),
        );

    let trailer_routes = Router::new().route("/search", get(api::trailer_search));

    let poster_routes = Router::new().route("/{filename}", get(api::serve_poster));

    let test_routes = Router::new()
        .route("/video", get(api::test_video))
        .route("/playlist.m3u8", get(api::test_hls_playlist))
        .route("/segment/{index}", get(api::test_segment))
        .route("/stream", post(api::create_demo_stream));

    let demo_routes: Router<AppState> = Router::new()
        .route("/", get(api::get_demo_stream))
        .route("/playlist.m3u8", get(api::demo_playlist));

    let admin_routes = Router::new()
        .route("/monitor", get(admin::admin_monitor_ws))
        .route("/logs", get(admin::admin_logs_ws))
        .route("/kill/{stream_id}", delete(admin::kill_transcode))
        .route("/restart-torrent", post(admin::restart_torrent))
        .route("/restart-server", post(admin::restart_server))
        .route("/settings", put(api::update_server_settings));

    let version_handler = || async {
        axum::Json(serde_json::json!({
            "version": static_files::VERSION,
            "hash": static_files::BUILD_HASH,
        }))
    };

    let api_routes = Router::new()
        .route("/version", get(version_handler))
        .route("/downloads", get(api::list_downloads))
        .nest("/admin", admin_routes)
        .nest("/auth", auth_routes)
        .nest("/search", search_routes)
        .nest("/stream/demo", demo_routes)
        .nest("/stream", stream_routes)
        .nest("/history", history_routes)
        .nest("/favourites", favourites_routes)
        .nest("/tv", tv_routes)
        .nest("/music-videos", music_video_routes)
        .nest("/music", music_routes)
        .nest("/settings", settings_routes)
        .nest("/playlists", playlist_routes)
        .nest("/trailer", trailer_routes)
        .nest("/posters", poster_routes)
        .nest("/test", test_routes);

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::mirror_request())
        .allow_methods([
            http::Method::GET,
            http::Method::POST,
            http::Method::PUT,
            http::Method::DELETE,
            http::Method::OPTIONS,
        ])
        .allow_headers([
            http::header::CONTENT_TYPE,
            http::header::AUTHORIZATION,
            http::header::ACCEPT,
        ])
        .allow_credentials(true);

    let proxy_routes = Router::new()
        .route("/local", get(proxy::list_local_files))
        .route("/local/{filename}", get(proxy::local_file))
        .route("/{id}/{*path}", get(proxy::proxy_image));

    Router::new()
        .nest("/api", api_routes)
        .nest("/proxy", proxy_routes)
        .fallback(static_files::static_handler)
        .layer(CompressionLayer::new())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
