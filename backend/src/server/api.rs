use crate::error::Error;
use crate::server::auth::Claims;
use crate::server::AppState;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<crate::torrent::SearchResult>,
}

#[derive(Debug, Deserialize)]
pub struct CreateStreamRequest {
    pub magnet_uri: String,
    pub file_index: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateHistoryRequest {
    pub watched_seconds: i64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    pub theme: Option<String>,
}

pub async fn search(
    State(state): State<AppState>,
    claims: Claims,
    Json(body): Json<SearchRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let query = body.query.trim();
    if query.is_empty() || query.len() > 500 {
        return Err(Error::BadRequest {
            message: "Query must be between 1 and 500 characters".to_string(),
        });
    }

    let results = state.search_provider.search(query).await?;
    let result_count = results.len() as i32;

    state
        .db
        .add_search(&claims.user_id, query, result_count)
        .await?;

    Ok(Json(SearchResponse { results }))
}

pub async fn search_history(
    State(state): State<AppState>,
    claims: Claims,
) -> std::result::Result<impl IntoResponse, Error> {
    let searches = state.db.get_search_history(&claims.user_id).await?;
    Ok(Json(serde_json::json!({ "searches": searches })))
}

pub async fn create_stream(
    State(state): State<AppState>,
    claims: Claims,
    Json(body): Json<CreateStreamRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let magnet_uri = body.magnet_uri.trim();
    if magnet_uri.is_empty() || magnet_uri.len() > 2048 {
        return Err(Error::BadRequest {
            message: "Invalid magnet URI".to_string(),
        });
    }

    let download = state
        .torrent_engine
        .add_magnet(magnet_uri, body.file_index)
        .await?;

    let title = if download.title.is_empty() {
        &download.info_hash
    } else {
        &download.title
    };
    let _ = state
        .db
        .add_watch(&claims.user_id, magnet_uri, title, None)
        .await;

    Ok(Json(serde_json::json!({
        "stream_id": download.info_hash,
        "status": download.status,
        "title": download.title,
        "file_name": download.file_name,
    })))
}

pub async fn get_stream(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    let download = state
        .torrent_engine
        .get_download(&id)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Stream {id} not found"),
        })?;

    let (peers, speed) = state.torrent_engine.get_live_stats(&id).await;

    Ok(Json(serde_json::json!({
        "id": download.info_hash,
        "status": download.status,
        "progress": download.progress,
        "title": download.title,
        "file_name": download.file_name,
        "file_size": download.file_size,
        "partial_path": download.partial_path,
        "complete_path": download.complete_path,
        "peers": peers,
        "speed": speed,
    })))
}

pub async fn pause_stream(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.torrent_engine.pause(&id).await?;
    Ok(Json(serde_json::json!({ "status": "paused" })))
}

pub async fn resume_stream(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.torrent_engine.resume(&id).await?;
    Ok(Json(serde_json::json!({ "status": "resumed" })))
}

pub async fn delete_stream(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.hls_pipeline.cleanup(&id).await?;
    Ok(Json(serde_json::json!({ "status": "stopped" })))
}

pub async fn get_history(
    State(state): State<AppState>,
    claims: Claims,
) -> std::result::Result<impl IntoResponse, Error> {
    let items = state.db.get_watch_history(&claims.user_id).await?;
    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn update_history(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<UpdateHistoryRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    if body.watched_seconds < 0 {
        return Err(Error::BadRequest {
            message: "watched_seconds must be non-negative".to_string(),
        });
    }
    state
        .db
        .update_watch_position(&id, body.watched_seconds)
        .await?;
    Ok(Json(serde_json::json!({ "status": "updated" })))
}

pub async fn delete_history(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.db.delete_watch(&id).await?;
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

pub async fn get_settings(
    State(state): State<AppState>,
    claims: Claims,
) -> std::result::Result<impl IntoResponse, Error> {
    let settings = state.db.get_settings(&claims.user_id).await?;
    Ok(Json(settings))
}

pub async fn update_settings(
    State(state): State<AppState>,
    claims: Claims,
    Json(body): Json<UpdateSettingsRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let mut settings = state.db.get_settings(&claims.user_id).await?;

    if let Some(theme) = body.theme {
        let theme = theme.trim().to_string();
        if theme != "dark" && theme != "light" {
            return Err(Error::BadRequest {
                message: "Theme must be 'dark' or 'light'".to_string(),
            });
        }
        settings.theme = theme;
    }

    state.db.update_settings(&claims.user_id, &settings).await?;
    Ok(Json(settings))
}

const DEMO_STREAM_ID: &str = "demo";
const DEMO_HLS_URL: &str = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";
const DEMO_MP4_URL: &str =
    "https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/BigBuckBunny.mp4";

pub async fn test_video() -> impl IntoResponse {
    axum::response::Redirect::temporary(DEMO_MP4_URL)
}

pub async fn test_hls_playlist() -> impl IntoResponse {
    axum::response::Redirect::temporary(DEMO_HLS_URL)
}

pub async fn test_segment(Path(_index): Path<u32>) -> impl IntoResponse {
    axum::response::Redirect::temporary(DEMO_HLS_URL)
}

pub async fn create_demo_stream() -> impl IntoResponse {
    Json(serde_json::json!({
        "stream_id": DEMO_STREAM_ID,
        "status": "ready",
    }))
}

pub async fn get_demo_stream() -> impl IntoResponse {
    Json(serde_json::json!({
        "id": DEMO_STREAM_ID,
        "status": "ready",
        "progress": 100.0,
        "peers": 0,
        "speed": 0,
    }))
}

pub async fn demo_playlist() -> impl IntoResponse {
    axum::response::Redirect::temporary(DEMO_HLS_URL)
}
