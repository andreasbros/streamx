pub mod api;
pub mod auth;
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
}

pub fn build_router(
    db: Database,
    config: AppConfig,
    torrent_engine: TorrentEngine,
    search_provider: SearchProvider,
    hls_pipeline: HlsManager,
) -> Router {
    let jwt_secret = config.auth.jwt_secret.clone();

    let state = AppState {
        db,
        config: Arc::new(config),
        jwt_secret,
        torrent_engine: Arc::new(torrent_engine),
        search_provider: Arc::new(search_provider),
        hls_pipeline: Arc::new(hls_pipeline),
        rate_limiter: RateLimiter::new(),
    };

    let auth_routes = Router::new()
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        .route("/me", get(auth::me));

    let search_routes = Router::new()
        .route("/", post(api::search))
        .route("/history", get(api::search_history));

    let stream_routes = Router::new()
        .route("/", post(api::create_stream))
        .route("/{id}", get(api::get_stream))
        .route("/{id}", delete(api::delete_stream))
        .route("/{id}/pause", put(api::pause_stream))
        .route("/{id}/resume", put(api::resume_stream))
        .route("/{id}/ws", get(stream::stream_ws))
        .route("/{id}/playlist.m3u8", get(stream::playlist))
        .route("/{id}/file", get(stream::stream_file))
        .route("/{id}/{segment}", get(stream::segment));

    let history_routes = Router::new()
        .route("/", get(api::get_history))
        .route("/{id}", put(api::update_history))
        .route("/{id}", delete(api::delete_history));

    let settings_routes = Router::new()
        .route("/", get(api::get_settings))
        .route("/", put(api::update_settings));

    let test_routes = Router::new()
        .route("/video", get(api::test_video))
        .route("/playlist.m3u8", get(api::test_hls_playlist))
        .route("/segment/{index}", get(api::test_segment))
        .route("/stream", post(api::create_demo_stream));

    let demo_routes: Router<AppState> = Router::new()
        .route("/", get(api::get_demo_stream))
        .route("/playlist.m3u8", get(api::demo_playlist));

    let api_routes = Router::new()
        .nest("/auth", auth_routes)
        .nest("/search", search_routes)
        .nest("/stream/demo", demo_routes)
        .nest("/stream", stream_routes)
        .nest("/history", history_routes)
        .nest("/settings", settings_routes)
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

    Router::new()
        .nest("/api", api_routes)
        .fallback(static_files::static_handler)
        .layer(CompressionLayer::new())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
