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
    let mut results = state.search_provider.search(query, page).await?;
    filter_web_only(&state, &mut results).await;
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
    let query_term = params.get("query_term").map(|s| s.as_str());
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

    let mut results = state
        .search_provider
        .browse(sort_by, query_term, genre, minimum_rating, limit, page)
        .await?;
    filter_web_only(&state, &mut results).await;

    Ok(Json(serde_json::json!({ "results": results })))
}

/// When the `web_only` server setting is on, keep only WEB source
/// variants and drop titles that have none.
async fn filter_web_only(
    state: &AppState,
    results: &mut Vec<streamx_api::types::SearchResultGroup>,
) {
    let web_only = state
        .db
        .get_server_settings()
        .await
        .map(|s| s.web_only)
        .unwrap_or(false);
    if !web_only {
        return;
    }
    for group in results.iter_mut() {
        group
            .variants
            .retain(|v| v.source_type.as_deref() == Some("web"));
    }
    results.retain(|g| !g.variants.is_empty());
}

/// GET /api/settings/server — visible to every authenticated user so the
/// UI can gate Play buttons and the transcode selector.
pub async fn get_server_settings(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
) -> std::result::Result<impl IntoResponse, Error> {
    let settings = state.db.get_server_settings().await?;
    Ok(Json(settings))
}

/// PUT /api/admin/settings — admin-managed server-wide settings.
pub async fn update_server_settings(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Json(body): Json<crate::db::settings::ServerSettings>,
) -> std::result::Result<impl IntoResponse, Error> {
    crate::server::admin::require_admin(&state, &claims.user_id).await?;
    state.db.set_server_settings(&body).await?;
    tracing::info!(
        disable_transcode = body.disable_transcode,
        web_only = body.web_only,
        "Admin updated server settings"
    );
    Ok(Json(body))
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

    // WEB-only mode blocks new movie downloads of other source types.
    // Requests without a source type (music, TV, raw magnets) pass.
    if let Some(src) = body.source_type.as_deref() {
        if src != "web" {
            let web_only = state
                .db
                .get_server_settings()
                .await
                .map(|s| s.web_only)
                .unwrap_or(false);
            if web_only {
                return Err(Error::BadRequest {
                    message: "WEB-only mode is enabled: this release is not a WEB source"
                        .to_string(),
                });
            }
        }
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
        let downloads_dir = state.config.downloads_dir();
        let hash = info_hash.clone();
        tokio::spawn(async move {
            let posters_dir = downloads_dir.join("posters");
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
    state.db.delete_download(&id).await?;
    tracing::info!(stream_id = %id, "Admin deleted stream: files and DB records removed");
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

/// Full cleanup: stop download, kill transcodes, delete every file the
/// torrent put on disk (manifest files, torrent folders, legacy paths)
/// plus the downloaded poster. DB rows are the caller's concern.
pub async fn cleanup_stream(state: &AppState, id: &str) -> std::result::Result<(), Error> {
    // With the downloads volume offline, deleting would drop DB rows
    // while the files survive on the unmounted drive — orphaning them.
    if !state.torrent_engine.downloads_root_available() {
        return Err(Error::BadRequest {
            message: "Downloads volume is unavailable; mount the drive before deleting".to_string(),
        });
    }

    // Never delete a stream out from under a connected viewer: any
    // client that pulled data in the last 30s blocks the cleanup.
    if state.torrent_engine.watched_within(id, 30) {
        return Err(Error::BadRequest {
            message: "Stream is currently being watched; stop playback first".to_string(),
        });
    }

    // Read the row first: stop_and_remove and the deletes below don't
    // change it, but the manifest is needed to locate all files.
    let dl = state.torrent_engine.get_download(id).await.ok().flatten();

    // Stop and remove torrent from engine (prevents stale progress reporting)
    let _ = state.torrent_engine.stop_and_remove(id).await;

    // Kill active transcodes (drops handles -> SIGTERM FFmpeg)
    if let Err(e) = state.hls_pipeline.cleanup(id).await {
        tracing::warn!(stream_id = %id, "HLS cleanup failed (non-fatal): {e}");
    }

    if let Some(dl) = dl {
        let downloads_dir = state.config.downloads_dir();
        let partial_dir = downloads_dir.join("partial");
        let complete_dir = downloads_dir.join("complete");

        // Manifest-driven deletion covers every file of multi-file
        // torrents. Nested torrents live under a root folder that can be
        // removed recursively; flat files are removed one by one.
        let mut roots: std::collections::HashSet<String> = std::collections::HashSet::new();
        for f in dl.manifest().unwrap_or_default() {
            let rel = std::path::Path::new(&f.path);
            if rel.components().count() > 1 {
                if let Some(std::path::Component::Normal(root)) = rel.components().next() {
                    roots.insert(root.to_string_lossy().to_string());
                }
            } else {
                let _ = tokio::fs::remove_file(partial_dir.join(rel)).await;
                let _ = tokio::fs::remove_file(complete_dir.join(rel)).await;
            }
        }
        for root in roots {
            if root == ".." || root.is_empty() {
                continue;
            }
            let _ = tokio::fs::remove_dir_all(partial_dir.join(&root)).await;
            let _ = tokio::fs::remove_dir_all(complete_dir.join(&root)).await;
        }

        // Legacy explicit paths (pre-manifest rows).
        for p in [dl.partial_path.as_ref(), dl.complete_path.as_ref()]
            .into_iter()
            .flatten()
        {
            let path = std::path::PathBuf::from(p);
            if path.exists() {
                let parent = path.parent().map(|p| p.to_path_buf());
                let _ = tokio::fs::remove_file(&path).await;
                if let Some(dir) = parent {
                    let _ = tokio::fs::remove_dir(&dir).await;
                }
            }
        }

        // Downloaded poster.
        let poster = downloads_dir.join("posters").join(format!("{id}.jpg"));
        let _ = tokio::fs::remove_file(&poster).await;
    }
    Ok(())
}

/// POST /api/stream/{id}/download — pin a download so it keeps going
/// after every client disconnects, and survives server restarts.
pub async fn pin_download(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    let dl = state
        .torrent_engine
        .get_download(&id)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Stream {id} not found"),
        })?;
    state.db.set_download_pinned(&id, true).await?;
    if dl.status != "complete" {
        state.torrent_engine.resume(&id).await?;
    }
    tracing::info!(stream_id = %id, "Download pinned (background download)");
    Ok(Json(serde_json::json!({ "status": "pinned" })))
}

/// DELETE /api/stream/{id}/download — cancel a background download:
/// unpin and pause. Files stay on disk for later resume.
pub async fn unpin_download(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.db.set_download_pinned(&id, false).await?;
    // A connected viewer keeps streaming; the disconnect handler pauses
    // unpinned downloads once the last client goes away.
    if state.torrent_engine.watched_within(&id, 30) {
        tracing::info!(stream_id = %id, "Download unpinned; connected viewer keeps it active");
    } else {
        let _ = state.torrent_engine.pause(&id).await;
    }
    Ok(Json(serde_json::json!({ "status": "cancelled" })))
}

/// GET /api/downloads — the download queue for any authenticated user:
/// every known download with progress and live stats.
pub async fn list_downloads(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
) -> std::result::Result<impl IntoResponse, Error> {
    let downloads = state.db.list_downloads().await?;
    let mut items = Vec::with_capacity(downloads.len());
    for dl in downloads {
        let (peers, speed) = state.torrent_engine.get_live_stats(&dl.info_hash).await;
        // Torrent metadata takes a while to resolve; fall back to the
        // rich metadata title captured at stream creation.
        let title = if dl.title.is_empty() {
            state
                .db
                .get_metadata(&dl.info_hash)
                .await
                .ok()
                .flatten()
                .map(|m| m.title)
                .filter(|t| !t.is_empty())
                .unwrap_or_default()
        } else {
            dl.title.clone()
        };
        items.push(serde_json::json!({
            "info_hash": dl.info_hash,
            "title": title,
            "file_name": dl.file_name,
            "file_size": dl.file_size,
            "status": dl.status,
            "progress": dl.progress,
            "pinned": dl.pinned,
            "download_all": dl.download_all,
            "created_at": dl.created_at,
            "updated_at": dl.updated_at,
            "peers": peers,
            "speed": speed,
        }));
    }
    Ok(Json(serde_json::json!({ "downloads": items })))
}

/// GET /api/downloads/{id}/movie — rebuild the movie-page group for a
/// stored download so clients can open the standard movie page from the
/// download queue in any state (downloading, paused, complete, error).
pub async fn download_movie(
    State(state): State<AppState>,
    _claims: AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    let dl = state
        .db
        .get_download(&id)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Download {id} not found"),
        })?;
    let meta = state.db.get_metadata(&id).await.ok().flatten();
    Ok(Json(download_movie_group(&dl, meta)))
}

/// Rebuild a `SearchResultGroup` from a download row plus the rich
/// metadata captured at stream creation, with a single variant pointing
/// at the stored magnet.
pub fn download_movie_group(
    dl: &crate::db::downloads::Download,
    meta: Option<MediaMetadata>,
) -> streamx_api::types::SearchResultGroup {
    use streamx_api::types::{SearchResult, SearchResultGroup};

    let m = meta.as_ref();
    let title = m
        .map(|m| m.title.clone())
        .filter(|t| !t.is_empty())
        .or_else(|| Some(dl.title.clone()).filter(|t| !t.is_empty()))
        .or_else(|| Some(dl.file_name.clone()).filter(|f| !f.is_empty()))
        .unwrap_or_else(|| dl.info_hash.clone());
    let magnet = if dl.magnet_uri.is_empty() {
        format!("magnet:?xt=urn:btih:{}", dl.info_hash)
    } else {
        dl.magnet_uri.clone()
    };
    let quality = quality_from_name(&dl.file_name).or_else(|| quality_from_name(&dl.title));
    let variant = SearchResult {
        magnet,
        seeds: 0,
        leeches: 0,
        size: human_size(dl.file_size),
        size_bytes: dl.file_size,
        quality,
        video_codec: m.and_then(|m| m.video_codec.clone()),
        audio_channels: m.and_then(|m| m.audio_channels.clone()),
        bit_depth: m.and_then(|m| m.bit_depth.clone()),
        source_type: m.and_then(|m| m.source_type.clone()),
    };
    SearchResultGroup {
        title,
        year: m.and_then(|m| m.year).and_then(|y| u32::try_from(y).ok()),
        rating: m.and_then(|m| m.rating),
        runtime: m
            .and_then(|m| m.runtime)
            .and_then(|r| u32::try_from(r).ok()),
        genres: m
            .and_then(|m| m.genres.clone())
            .map(|g| {
                g.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        language: m.and_then(|m| m.language.clone()),
        mpa_rating: m.and_then(|m| m.mpa_rating.clone()),
        summary: m.and_then(|m| m.summary.clone()),
        imdb_code: m.and_then(|m| m.imdb_code.clone()),
        trailer_code: m.and_then(|m| m.trailer_code.clone()),
        poster: m.and_then(|m| {
            m.poster_large
                .clone()
                .or_else(|| m.poster_medium.clone())
                .or_else(|| m.poster_small.clone())
        }),
        poster_small: m.and_then(|m| m.poster_small.clone()),
        poster_medium: m.and_then(|m| m.poster_medium.clone()),
        poster_large: m.and_then(|m| m.poster_large.clone()),
        backdrop: m.and_then(|m| m.backdrop.clone()),
        variants: vec![variant],
    }
}

fn quality_from_name(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    for q in ["2160p", "1080p", "720p", "480p"] {
        if lower.contains(q) {
            return Some(q.to_string());
        }
    }
    if lower.contains("4k") {
        return Some("2160p".to_string());
    }
    None
}

fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if bytes > 0 {
        format!("{:.0} KB", (b / 1e3).max(1.0))
    } else {
        String::new()
    }
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
        .clamp(1, 24 * 90);

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

    // Favourited music (albums/playlists) must become fully available:
    // start a pinned full-album download in the background so every
    // track lands on disk and survives disconnects and restarts.
    if body.content_type.as_deref() == Some("music") {
        let magnet = body
            .metadata_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
            .and_then(|v| v.get("magnet").and_then(|m| m.as_str()).map(String::from));
        if let Some(magnet) = magnet {
            let state = state.clone();
            tokio::spawn(async move {
                match state.torrent_engine.add_magnet_album(&magnet).await {
                    Ok(dl) => {
                        let _ = state.db.set_download_pinned(&dl.info_hash, true).await;
                        let _ = state.torrent_engine.resume(&dl.info_hash).await;
                        tracing::info!(
                            info_hash = %dl.info_hash,
                            "Favourited music pinned for full download"
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Favourite music download failed to start: {e}");
                    }
                }
            });
        }
    }

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

    let poster_path = state.config.downloads_dir().join("posters").join(&filename);

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

/// 404 unless the playlist exists and belongs to the caller. Not-found
/// (rather than 401) so playlist ids of other users are unguessable.
async fn ensure_playlist_owner(
    state: &AppState,
    playlist_id: &str,
    user_id: &str,
) -> std::result::Result<(), Error> {
    if !state.db.playlist_owned_by(playlist_id, user_id).await? {
        return Err(Error::NotFound {
            message: "Playlist not found".to_string(),
        });
    }
    Ok(())
}

pub async fn get_playlist_tracks(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    ensure_playlist_owner(&state, &id, &claims.user_id).await?;
    let tracks = state.db.get_playlist_tracks(&id).await?;
    Ok(Json(serde_json::json!({ "tracks": tracks })))
}

pub async fn add_playlist_track(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<AddTrackRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    ensure_playlist_owner(&state, &id, &claims.user_id).await?;
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

#[cfg(test)]
mod download_movie_tests {
    use super::*;
    use crate::db::downloads::Download;

    fn dl_row() -> Download {
        Download {
            info_hash: "abc123".to_string(),
            magnet_uri: String::new(),
            title: String::new(),
            file_name: "Some.Movie.2026.1080p.WEB.mkv".to_string(),
            file_index: 0,
            file_size: 2_400_000_000,
            download_all: false,
            status: "downloading".to_string(),
            progress: 42.0,
            partial_path: None,
            complete_path: None,
            created_at: String::new(),
            updated_at: String::new(),
            files_json: None,
            pinned: true,
        }
    }

    fn meta() -> MediaMetadata {
        MediaMetadata {
            info_hash: "abc123".to_string(),
            title: "Some Movie".to_string(),
            year: Some(2026),
            rating: Some(7.1),
            runtime: Some(120),
            genres: Some("Action, Drama".to_string()),
            language: Some("en".to_string()),
            mpa_rating: None,
            summary: Some("Plot.".to_string()),
            imdb_code: Some("tt1234567".to_string()),
            trailer_code: None,
            video_codec: Some("x265".to_string()),
            audio_channels: Some("5.1".to_string()),
            bit_depth: Some("10".to_string()),
            source_type: Some("web".to_string()),
            poster_small: Some("https://p/s.jpg".to_string()),
            poster_medium: None,
            poster_large: Some("https://p/l.jpg".to_string()),
            backdrop: None,
            local_poster: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn builds_group_from_metadata() {
        let g = download_movie_group(&dl_row(), Some(meta()));
        assert_eq!(g.title, "Some Movie");
        assert_eq!(g.year, Some(2026));
        assert_eq!(g.genres, vec!["Action", "Drama"]);
        assert_eq!(g.poster.as_deref(), Some("https://p/l.jpg"));
        assert_eq!(g.variants.len(), 1);
        let v = &g.variants[0];
        assert_eq!(v.magnet, "magnet:?xt=urn:btih:abc123");
        assert_eq!(v.quality.as_deref(), Some("1080p"));
        assert_eq!(v.video_codec.as_deref(), Some("x265"));
        assert_eq!(v.size_bytes, 2_400_000_000);
        assert_eq!(v.size, "2.40 GB");
    }

    #[test]
    fn falls_back_without_metadata() {
        let mut dl = dl_row();
        dl.magnet_uri = "magnet:?xt=urn:btih:abc123&dn=x".to_string();
        let g = download_movie_group(&dl, None);
        assert_eq!(g.title, "Some.Movie.2026.1080p.WEB.mkv");
        assert_eq!(g.year, None);
        assert!(g.genres.is_empty());
        assert_eq!(g.variants[0].magnet, "magnet:?xt=urn:btih:abc123&dn=x");
        assert_eq!(g.variants[0].quality.as_deref(), Some("1080p"));
    }

    #[test]
    fn quality_detection_variants() {
        assert_eq!(
            quality_from_name("Movie.4K.HDR.mkv").as_deref(),
            Some("2160p")
        );
        assert_eq!(quality_from_name("Movie.720p.mkv").as_deref(), Some("720p"));
        assert_eq!(quality_from_name("Movie.mkv"), None);
    }
}
