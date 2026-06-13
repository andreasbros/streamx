use crate::db::favourites::AddFavouriteRequest;
use crate::db::metadata::MediaMetadata;
use crate::db::playlists::AddTrackRequest;
use crate::error::Error;
use crate::server::auth::{create_guest_token, AuthenticatedUser};
use crate::server::proxy;
use crate::server::AppState;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

pub use streamx_api::types::{CreateStreamRequest, SearchRequest, SearchResponse};

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
    AuthenticatedUser(claims): AuthenticatedUser,
    Json(body): Json<SearchRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let query = body.query.trim();
    if query.is_empty() || query.len() > 500 {
        return Err(Error::BadRequest {
            message: "Query must be between 1 and 500 characters".to_string(),
        });
    }

    let page = body.page.max(1);
    let results = state.search_provider.search(query, page).await?;
    let result_count = results.len() as i32;

    state
        .db
        .add_search(&claims.user_id, query, result_count)
        .await?;

    Ok(Json(SearchResponse { results }))
}

pub async fn browse(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<impl IntoResponse, Error> {
    let sort_by = params
        .get("sort_by")
        .map(|s| s.as_str())
        .unwrap_or("date_added");
    let genre = params.get("genre").map(|s| s.as_str());
    let minimum_rating = params.get("minimum_rating").and_then(|s| s.parse().ok());
    let limit = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10u32)
        .min(20);
    let page = params
        .get("page")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1u32);

    let results = state
        .search_provider
        .browse(sort_by, genre, minimum_rating, limit, page)
        .await?;

    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn search_history(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
) -> std::result::Result<impl IntoResponse, Error> {
    let searches = state.db.get_search_history(&claims.user_id).await?;
    Ok(Json(serde_json::json!({ "searches": searches })))
}

pub async fn create_stream(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
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
        .add_watch(
            &claims.user_id,
            magnet_uri,
            title,
            None,
            body.poster_url.as_deref(),
        )
        .await;

    // Store rich metadata if we have any
    let info_hash = download.info_hash.clone();
    let has_metadata = body.title.is_some() || body.poster_url.is_some();
    if has_metadata {
        let meta = MediaMetadata {
            info_hash: info_hash.clone(),
            title: body.title.clone().unwrap_or_else(|| download.title.clone()),
            year: body.year.map(|y| y as i32),
            rating: body.rating,
            runtime: body.runtime.map(|r| r as i32),
            genres: body.genres.as_ref().map(|g| g.join(",")),
            language: body.language.clone(),
            mpa_rating: body.mpa_rating.clone(),
            summary: body.summary.clone(),
            imdb_code: body.imdb_code.clone(),
            trailer_code: body.trailer_code.clone(),
            video_codec: body.video_codec.clone(),
            audio_channels: body.audio_channels.clone(),
            bit_depth: body.bit_depth.clone(),
            source_type: body.source_type.clone(),
            poster_small: body.poster_small.clone(),
            poster_medium: body.poster_medium.clone(),
            poster_large: body.poster_large.clone(),
            backdrop: body.backdrop.clone(),
            local_poster: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = state.db.upsert_metadata(&meta).await;
    }

    // Spawn background poster download
    let poster_url = body.poster_large.or(body.poster_medium).or(body.poster_url);
    if let Some(url) = poster_url {
        // Resolve proxy URLs back to absolute upstream URLs for downloading
        let download_url = proxy::resolve_proxy_url(&url, &state.config.providers);
        let db = state.db.clone();
        let data_dir = state.config.data_dir.clone();
        let hash = info_hash.clone();
        tokio::spawn(async move {
            let posters_dir = data_dir.join("downloads").join("posters");
            if let Err(e) = tokio::fs::create_dir_all(&posters_dir).await {
                tracing::warn!(info_hash = %hash, "Failed to create posters dir: {e}");
                return;
            }
            let dest = posters_dir.join(format!("{hash}.jpg"));
            if dest.exists() {
                let _ = db
                    .update_local_poster(&hash, &format!("/api/posters/{hash}.jpg"))
                    .await;
                return;
            }
            match download_poster(&download_url, &dest).await {
                Ok(()) => {
                    tracing::info!(info_hash = %hash, "Poster downloaded");
                    let _ = db
                        .update_local_poster(&hash, &format!("/api/posters/{hash}.jpg"))
                        .await;
                }
                Err(e) => {
                    tracing::warn!(info_hash = %hash, "Failed to download poster: {e}");
                }
            }
        });
    }

    Ok(Json(serde_json::json!({
        "stream_id": download.info_hash,
        "status": download.status,
        "title": download.title,
        "file_name": download.file_name,
    })))
}

/// Create a music stream that downloads ALL files in the torrent (for albums).
pub async fn create_music_stream(
    State(state): State<AppState>,
    AuthenticatedUser(_claims): AuthenticatedUser,
    Json(body): Json<CreateStreamRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let magnet_uri = body.magnet_uri.trim();
    if magnet_uri.is_empty() || magnet_uri.len() > 2048 {
        return Err(Error::BadRequest {
            message: "Invalid magnet URI".to_string(),
        });
    }

    let download = state.torrent_engine.add_magnet_album(magnet_uri).await?;

    tracing::info!(
        stream_id = %download.info_hash,
        status = %download.status,
        download_all = download.download_all,
        "create_music_stream: album stream requested"
    );

    Ok(Json(serde_json::json!({
        "stream_id": download.info_hash,
        "status": download.status,
        "title": download.title,
    })))
}

async fn download_poster(
    url: &str,
    dest: &std::path::Path,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let resp = reqwest::get(url).await?;
    let bytes = resp.bytes().await?;
    tokio::fs::write(dest, &bytes).await?;
    Ok(())
}

pub async fn get_stream(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
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
    _claims: AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.torrent_engine.pause(&id).await?;
    Ok(Json(serde_json::json!({ "status": "paused" })))
}

pub async fn resume_stream(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.torrent_engine.resume(&id).await?;
    Ok(Json(serde_json::json!({ "status": "resumed" })))
}

pub async fn delete_stream(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    // Admin only
    let user = state
        .db
        .find_user_by_id(&claims.user_id)
        .await?
        .ok_or_else(|| Error::Unauthorized {
            message: "User not found".to_string(),
        })?;
    if !user.is_admin {
        return Err(Error::Unauthorized {
            message: "Admin access required".to_string(),
        });
    }

    cleanup_stream(&state, &id).await?;
    state.db.reset_download(&id).await?;
    tracing::info!(stream_id = %id, "Admin reset stream for re-download");
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

/// Full cleanup: stop download, kill transcodes, delete files
pub async fn cleanup_stream(state: &AppState, id: &str) -> std::result::Result<(), Error> {
    // Stop and remove torrent from engine (prevents stale progress reporting)
    let _ = state.torrent_engine.stop_and_remove(id).await;

    // Kill active transcodes (drops handles -> SIGTERM FFmpeg)
    if let Err(e) = state.hls_pipeline.cleanup(id).await {
        tracing::warn!(stream_id = %id, "HLS cleanup failed (non-fatal): {e}");
    }

    // Delete files on disk
    if let Ok(Some(dl)) = state.torrent_engine.get_download(id).await {
        if let Some(ref p) = dl.partial_path {
            let path = std::path::PathBuf::from(p);
            if path.exists() {
                let parent = path.parent().map(|p| p.to_path_buf());
                let _ = tokio::fs::remove_file(&path).await;
                if let Some(dir) = parent {
                    let _ = tokio::fs::remove_dir(&dir).await;
                }
            }
        }
        if let Some(ref p) = dl.complete_path {
            let path = std::path::PathBuf::from(p);
            if path.exists() {
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
    }
    Ok(())
}

pub async fn share_stream(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<std::collections::HashMap<String, serde_json::Value>>,
) -> std::result::Result<impl IntoResponse, Error> {
    // Verify stream exists
    let _ = state
        .torrent_engine
        .get_download(&id)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Stream {id} not found"),
        })?;

    let duration_hours = body
        .get("duration_hours")
        .and_then(|v| v.as_i64())
        .unwrap_or(24 * 30)
        .min(24 * 90)
        .max(1);

    let token = create_guest_token(&id, &state.jwt_secret, duration_hours)?;
    let url = format!("/player/{id}?guest={token}");

    tracing::info!(stream_id = %id, duration_hours, "Share link created");
    Ok(Json(serde_json::json!({ "token": token, "url": url })))
}

pub async fn get_history(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
) -> std::result::Result<impl IntoResponse, Error> {
    let items = state.db.get_watch_history_enriched(&claims.user_id).await?;
    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn update_history(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
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
    _claims: AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.db.delete_watch(&id).await?;
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

pub async fn get_settings(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
) -> std::result::Result<impl IntoResponse, Error> {
    let settings = state.db.get_settings(&claims.user_id).await?;
    Ok(Json(settings))
}

pub async fn update_settings(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
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

static TRAILER_SEARCH_CACHE: std::sync::LazyLock<dashmap::DashMap<String, String>> =
    std::sync::LazyLock::new(dashmap::DashMap::new);

pub async fn trailer_search(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<Json<serde_json::Value>, Error> {
    let query = params.get("q").ok_or_else(|| Error::BadRequest {
        message: "Missing query parameter 'q'".to_string(),
    })?;

    if let Some(cached) = TRAILER_SEARCH_CACHE.get(query) {
        return Ok(Json(serde_json::json!({ "youtube_id": cached.value() })));
    }

    let search_url = format!(
        "https://www.youtube.com/results?search_query={}",
        urlencoding::encode(query)
    );

    let html = state
        .http_client
        .get(&search_url)
        .header("Accept-Language", "en")
        .send()
        .await
        .map_err(|e| Error::Internal {
            message: format!("YouTube search failed: {e}"),
        })?
        .text()
        .await
        .map_err(|e| Error::Internal {
            message: format!("Failed to read YouTube response: {e}"),
        })?;

    // Extract unique video IDs from YouTube's embedded JSON ("videoId":"XXXXXXXXXXX")
    let mut seen = std::collections::HashSet::new();
    let candidates: Vec<String> = html
        .split("\"videoId\":\"")
        .skip(1)
        .filter_map(|s| {
            let id = s.split('"').next()?;
            if id.len() == 11
                && id
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                && seen.insert(id.to_string())
            {
                Some(id.to_string())
            } else {
                None
            }
        })
        .take(10)
        .collect();

    // Also extract video titles for fuzzy matching ("title":{"runs":[{"text":"..."}]})
    let titles: Vec<String> = html
        .split("\"title\":{\"runs\":[{\"text\":\"")
        .skip(1)
        .filter_map(|s| s.split('"').next().map(String::from))
        .take(10)
        .collect();

    // Find best match: prefer results whose title contains the original search terms
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower
        .split_whitespace()
        .filter(|w| w.len() > 2 && *w != "official" && *w != "trailer")
        .collect();

    let best_id = candidates
        .iter()
        .enumerate()
        .max_by_key(|(i, _)| {
            let title = titles.get(*i).map(|t| t.to_lowercase()).unwrap_or_default();
            let word_hits = query_words.iter().filter(|w| title.contains(**w)).count();
            let has_trailer = title.contains("trailer") || title.contains("official");
            // Score: word matches * 10 + trailer keyword bonus + position penalty
            (word_hits * 10) + if has_trailer { 5 } else { 0 } + (10_usize.saturating_sub(*i))
        })
        .map(|(_, id)| id.as_str());

    match best_id {
        Some(id) => {
            tracing::info!(query, youtube_id = id, "Trailer search matched");
            TRAILER_SEARCH_CACHE.insert(query.clone(), id.to_string());
            Ok(Json(serde_json::json!({ "youtube_id": id })))
        }
        None => Err(Error::NotFound {
            message: "No video found".to_string(),
        }),
    }
}

pub async fn add_favourite(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Json(body): Json<AddFavouriteRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    if body.title.trim().is_empty() || body.title.len() > 500 {
        return Err(Error::BadRequest {
            message: "Title must be between 1 and 500 characters".to_string(),
        });
    }
    let item = state.db.add_favourite(&claims.user_id, &body).await?;
    Ok(Json(item))
}

pub async fn get_favourites(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<impl IntoResponse, Error> {
    let content_type = params.get("type").map(|s| s.as_str());
    let items = state
        .db
        .get_favourites(&claims.user_id, content_type)
        .await?;
    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn delete_favourite(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    let deleted = state.db.delete_favourite(&id, &claims.user_id).await?;
    if !deleted {
        return Err(Error::NotFound {
            message: "Favourite not found".to_string(),
        });
    }
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

pub async fn search_tv(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    Json(body): Json<SearchRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let query = body.query.trim();
    if query.is_empty() || query.len() > 500 {
        return Err(Error::BadRequest {
            message: "Query must be between 1 and 500 characters".to_string(),
        });
    }
    let results = state.search_provider.search_tv(query).await?;
    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn browse_tv(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<impl IntoResponse, Error> {
    let page = params
        .get("page")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1u32);
    let limit = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(20u32)
        .min(100);
    let results = state.search_provider.browse_tv(page, limit).await?;
    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn get_tv_show(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    axum::extract::Path(imdb_id): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<impl IntoResponse, Error> {
    if !imdb_id.starts_with("tt") || imdb_id.len() < 4 {
        return Err(Error::BadRequest {
            message: "Invalid IMDB ID".to_string(),
        });
    }
    let season = params.get("season").and_then(|s| s.parse::<u32>().ok());
    if season.is_some() {
        let seasons = state
            .search_provider
            .fetch_show_episodes(&imdb_id, season)
            .await?;
        Ok(Json(serde_json::json!({ "seasons": seasons })))
    } else {
        let season_numbers = state.search_provider.discover_seasons(&imdb_id).await?;
        Ok(Json(serde_json::json!({ "seasons": season_numbers })))
    }
}

pub async fn search_music_videos(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    Json(body): Json<SearchRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let query = body.query.trim();
    if query.is_empty() || query.len() > 500 {
        return Err(Error::BadRequest {
            message: "Query must be between 1 and 500 characters".to_string(),
        });
    }
    let results = state.search_provider.search_music_videos(query).await?;
    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn browse_music_videos(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<impl IntoResponse, Error> {
    let page = params
        .get("page")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1u32);
    let results = state.search_provider.browse_music_videos(page).await?;
    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn search_music(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    Json(body): Json<SearchRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let query = body.query.trim();
    if query.is_empty() || query.len() > 500 {
        return Err(Error::BadRequest {
            message: "Query must be between 1 and 500 characters".to_string(),
        });
    }
    let results = state.search_provider.search_music(query).await?;
    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn browse_music(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<impl IntoResponse, Error> {
    let page = params
        .get("page")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1u32);
    let results = state.search_provider.browse_music(page).await?;
    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn resolve_magnet(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    Json(body): Json<std::collections::HashMap<String, String>>,
) -> std::result::Result<impl IntoResponse, Error> {
    let detail_url = body.get("detail_url").ok_or_else(|| Error::BadRequest {
        message: "detail_url is required".to_string(),
    })?;
    let magnet = state.search_provider.get_magnet(detail_url).await?;
    match magnet {
        Some(m) => Ok(Json(serde_json::json!({ "magnet": m }))),
        None => Err(Error::NotFound {
            message: "Could not resolve magnet link".to_string(),
        }),
    }
}

pub async fn serve_poster(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    // Sanitize filename to prevent path traversal
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(Error::BadRequest {
            message: "Invalid filename".to_string(),
        });
    }

    let poster_path = state
        .config
        .data_dir
        .join("downloads")
        .join("posters")
        .join(&filename);

    if !poster_path.exists() {
        return Err(Error::NotFound {
            message: "Poster not found".to_string(),
        });
    }

    let bytes = tokio::fs::read(&poster_path)
        .await
        .map_err(|e| Error::Io { source: e })?;

    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "image/jpeg".to_string()),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    ))
}

// --- Playlist CRUD ---

#[derive(Debug, Deserialize)]
pub struct CreatePlaylistRequest {
    pub name: String,
}

pub async fn create_playlist(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Json(body): Json<CreatePlaylistRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let playlist = state
        .db
        .create_playlist(&claims.user_id, &body.name)
        .await?;
    Ok(Json(playlist))
}

pub async fn get_playlists(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
) -> std::result::Result<impl IntoResponse, Error> {
    let playlists = state.db.get_playlists(&claims.user_id).await?;
    Ok(Json(serde_json::json!({ "playlists": playlists })))
}

#[derive(Debug, Deserialize)]
pub struct RenamePlaylistRequest {
    pub name: String,
}

pub async fn rename_playlist(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<RenamePlaylistRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    state
        .db
        .rename_playlist(&id, &claims.user_id, &body.name)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_playlist(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.db.delete_playlist(&id, &claims.user_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn get_playlist_tracks(
    State(state): State<AppState>,
    AuthenticatedUser(_claims): AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    let tracks = state.db.get_playlist_tracks(&id).await?;
    Ok(Json(serde_json::json!({ "tracks": tracks })))
}

pub async fn add_playlist_track(
    State(state): State<AppState>,
    AuthenticatedUser(_claims): AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<AddTrackRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    let track = state.db.add_playlist_track(&id, &body).await?;
    Ok(Json(track))
}

pub async fn remove_playlist_track(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Path((playlist_id, track_id)): Path<(String, String)>,
) -> std::result::Result<impl IntoResponse, Error> {
    let _ = playlist_id; // validated by route
    state
        .db
        .remove_playlist_track(&track_id, &claims.user_id)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
