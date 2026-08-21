use crate::error::Error;
use crate::server::auth::Claims;
use crate::server::AppState;
use crate::transcode::hls::PlaylistResponse;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use bytes::Bytes;
use lofty::file::TaggedFileExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::{debug, info};

pub async fn stream_ws(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_stream_ws(socket, state, id))
}

async fn handle_stream_ws(mut socket: WebSocket, state: AppState, id: String) {
    state
        .ws_connections
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state.torrent_engine.note_watch(&id);
    let _ = state.torrent_engine.resume(&id).await;

    let metadata = state.db.get_metadata(&id).await.ok().flatten();
    let video_codec = metadata.as_ref().and_then(|m| m.video_codec.clone());

    // Send metadata once on connection
    if let Some(ref meta) = metadata {
        let meta_msg = serde_json::json!({
            "type": "metadata",
            "data": {
                "title": meta.title,
                "year": meta.year,
                "rating": meta.rating,
                "runtime": meta.runtime,
                "genres": meta.genres,
                "language": meta.language,
                "mpa_rating": meta.mpa_rating,
                "summary": meta.summary,
                "imdb_code": meta.imdb_code,
                "video_codec": meta.video_codec,
                "audio_channels": meta.audio_channels,
                "bit_depth": meta.bit_depth,
                "source_type": meta.source_type,
                "poster_large": meta.poster_large,
                "local_poster": meta.local_poster,
            }
        });
        let _ = socket
            .send(Message::Text(meta_msg.to_string().into()))
            .await;
    }

    if let Ok(Some(dl)) = state.torrent_engine.get_download(&id).await {
        let (peers, speed) = state.torrent_engine.get_live_stats(&id).await;
        let msg = serde_json::json!({
            "type": "status",
            "data": {
                "status": dl.status,
                "progress": dl.progress,
                "peers": peers,
                "speed": speed,
                "file_size": dl.file_size,
                "title": dl.title,
                "file_name": dl.file_name,
                "video_codec": video_codec,
            }
        });
        if socket
            .send(Message::Text(msg.to_string().into()))
            .await
            .is_err()
        {
            pause_unless_pinned(&state, &id).await;
            return;
        }

        if dl.status == "complete" {
            let file_msg = serde_json::json!({
                "type": "file_ready",
                "data": {"url": format!("/api/stream/{id}/file")}
            });
            let _ = socket
                .send(Message::Text(file_msg.to_string().into()))
                .await;
        }
    }

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut file_ready_sent = false;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let dl = match state.torrent_engine.get_download(&id).await {
                    Ok(Some(d)) => d,
                    _ => break,
                };
                let (peers, speed) = state.torrent_engine.get_live_stats(&id).await;

                let msg = serde_json::json!({
                    "type": "status",
                    "data": {
                        "status": dl.status,
                        "progress": dl.progress,
                        "peers": peers,
                        "speed": speed,
                        "file_size": dl.file_size,
                        "title": dl.title,
                        "file_name": dl.file_name,
                        "video_codec": video_codec,
                    }
                });
                if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }

                if !file_ready_sent {
                    let has_handle = state
                        .torrent_engine
                        .get_stream_file_info(&id)
                        .await
                        .ok()
                        .flatten()
                        .is_some();
                    if dl.status == "complete" || (dl.status == "downloading" && has_handle) {
                        let file_msg = serde_json::json!({
                            "type": "file_ready",
                            "data": {"url": format!("/api/stream/{id}/file")}
                        });
                        if socket.send(Message::Text(file_msg.to_string().into())).await.is_err() {
                            break;
                        }
                        file_ready_sent = true;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(_text))) => {
                        debug!(stream_id = %id, "Received client message (ignored)");
                    }
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    state
        .ws_connections
        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    pause_unless_pinned(&state, &id).await;
}

/// Pause a torrent when its viewer disconnects — unless the download is
/// pinned (background download), which keeps going with no client.
async fn pause_unless_pinned(state: &AppState, id: &str) {
    let pinned = state
        .torrent_engine
        .get_download(id)
        .await
        .ok()
        .flatten()
        .map(|d| d.pinned)
        .unwrap_or(false);
    if pinned {
        debug!(stream_id = %id, "client disconnected; pinned download keeps running");
        return;
    }
    let _ = state.torrent_engine.pause(id).await;
}

pub async fn url_playlist(
    State(state): State<AppState>,
    _claims: Claims,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<axum::response::Response, Error> {
    let url = params.get("url").ok_or_else(|| Error::BadRequest {
        message: "Missing 'url' parameter".to_string(),
    })?;
    let quality = params
        .get("quality")
        .map(|s| s.as_str())
        .unwrap_or("source");

    // Use a hash of the URL as stream_id
    let stream_id = format!("url-{:x}", md5_hash(url));

    if let Err(e) = state
        .hls_pipeline
        .start_stream_url(&stream_id, url, quality)
        .await
    {
        tracing::warn!(stream_id = %stream_id, "Failed to start URL transcode: {e}");
    }

    let response = state
        .hls_pipeline
        .generate_playlist(&stream_id, quality)
        .await?;

    match response {
        PlaylistResponse::Redirect(redir_url) => {
            Ok(Redirect::temporary(&redir_url).into_response())
        }
        PlaylistResponse::Content(content) => Ok((
            [
                (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
                (header::CACHE_CONTROL, "no-cache, no-store"),
            ],
            content,
        )
            .into_response()),
    }
}

fn md5_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// With the `disable_transcode` server setting on, no server-side
/// transcode is started: WEB-compatible releases play directly via
/// `/api/stream/{id}/file`, everything else is download-only.
async fn ensure_transcode_allowed(state: &AppState, id: &str) -> std::result::Result<(), Error> {
    let disabled = state
        .db
        .get_server_settings()
        .await
        .map(|s| s.disable_transcode)
        .unwrap_or(true);
    if disabled {
        tracing::debug!(stream_id = %id, "transcode request rejected: disabled by server setting");
        return Err(Error::BadRequest {
            message: "Server-side transcoding is disabled".to_string(),
        });
    }
    Ok(())
}

pub async fn playlist(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<axum::response::Response, Error> {
    state.torrent_engine.note_watch(&id);
    let quality = params
        .get("quality")
        .map(|s| s.as_str())
        .unwrap_or("source");

    ensure_transcode_allowed(&state, &id).await?;

    let download = state
        .torrent_engine
        .get_download(&id)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Stream {id} not found"),
        })?;

    let file_path = download
        .complete_path
        .as_deref()
        .or(download.partial_path.as_deref());

    // Always try file-based transcode first (works for both complete and partial/sequential downloads)
    // FFmpeg handles growing files naturally - it reads what's available and waits for more
    let mut started = false;
    if let Some(path) = file_path {
        if tokio::fs::metadata(path).await.is_ok() {
            if let Err(e) = state.hls_pipeline.start_stream(&id, path, quality).await {
                tracing::warn!(stream_id = %id, quality, "Failed to start HLS transcode: {e}");
            } else {
                started = true;
            }
        }
    }
    if !started {
        if let Ok(Some(path)) = state.torrent_engine.get_file_path(&id).await {
            if let Err(e) = state
                .hls_pipeline
                .start_stream(&id, path.to_str().unwrap_or_default(), quality)
                .await
            {
                tracing::warn!(stream_id = %id, quality, "Failed to start HLS transcode: {e}");
            } else {
                started = true;
            }
        }
    }

    // Fallback to piped transcode only if file-based didn't work
    if !started && download.status != "complete" {
        if download.status == "paused" {
            let _ = state.torrent_engine.resume(&id).await;
        } else {
            let _ = state.torrent_engine.ensure_active(&id).await;
        }

        let probe_path = file_path.map(String::from).or_else(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async { state.torrent_engine.get_file_path(&id).await.ok().flatten() })
                .map(|p| p.to_string_lossy().to_string())
        });

        if let Some(ref probe_path) = probe_path {
            if let Ok(Some((torrent_id, file_index))) =
                state.torrent_engine.get_stream_file_info(&id).await
            {
                let api = librqbit::Api::new(state.torrent_engine.session(), None);
                if let Ok(file_stream) =
                    api.api_stream(librqbit::api::TorrentIdOrHash::Id(torrent_id), file_index)
                {
                    if let Err(e) = state
                        .hls_pipeline
                        .start_stream_piped(&id, probe_path, file_stream, quality)
                        .await
                    {
                        tracing::warn!(stream_id = %id, "Failed to start piped HLS transcode: {e}");
                    }
                } else if let Err(e) = state
                    .hls_pipeline
                    .start_stream(&id, probe_path, quality)
                    .await
                {
                    tracing::warn!(stream_id = %id, "Failed to start HLS transcode (fallback): {e}");
                }
            } else if let Err(e) = state
                .hls_pipeline
                .start_stream(&id, probe_path, quality)
                .await
            {
                tracing::warn!(stream_id = %id, "Failed to start HLS transcode: {e}");
            }
        }
    }

    let response = state.hls_pipeline.generate_playlist(&id, quality).await?;

    match response {
        PlaylistResponse::Redirect(url) => Ok(Redirect::temporary(&url).into_response()),
        PlaylistResponse::Content(content) => Ok((
            [
                (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
                (header::CACHE_CONTROL, "no-cache, no-store"),
            ],
            content,
        )
            .into_response()),
    }
}

pub async fn segment(
    State(state): State<AppState>,
    Path((id, segment_name)): Path<(String, String)>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.torrent_engine.note_watch(&id);
    let data: Bytes = state
        .hls_pipeline
        .get_segment(&id, &segment_name)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Segment {segment_name} not found"),
        })?;

    let content_type = if segment_name.ends_with(".m4s") || segment_name.ends_with(".mp4") {
        "video/mp4"
    } else {
        "video/mp2t"
    };

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache, no-store"),
        ],
        data,
    ))
}

pub async fn variant_playlist(
    State(state): State<AppState>,
    Path((id, variant)): Path<(String, String)>,
) -> std::result::Result<axum::response::Response, Error> {
    state.torrent_engine.note_watch(&id);
    let content = state
        .hls_pipeline
        .get_variant_playlist(&id, &variant)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Variant playlist {variant} not found for stream {id}"),
        })?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
            (header::CACHE_CONTROL, "no-cache, no-store"),
        ],
        content,
    )
        .into_response())
}

pub async fn variant_segment(
    State(state): State<AppState>,
    Path((id, variant, segment_name)): Path<(String, String, String)>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.torrent_engine.note_watch(&id);
    let data: Bytes = state
        .hls_pipeline
        .get_variant_segment(&id, &variant, &segment_name)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Variant segment {variant}/{segment_name} not found"),
        })?;

    let content_type = if segment_name.ends_with(".m4s") || segment_name.ends_with(".mp4") {
        "video/mp4"
    } else {
        "video/mp2t"
    };

    // Prevent CDN/proxy caching - init segments contain codec params
    // that change when transcode config changes (e.g. channel count)
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache, no-store"),
        ],
        data,
    ))
}

pub async fn stream_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> std::result::Result<axum::response::Response, Error> {
    state.torrent_engine.note_watch(&id);
    let download = state
        .torrent_engine
        .get_download(&id)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Stream {id} not found"),
        })?;

    match download.status.as_str() {
        "complete" => {
            if let Some(ref cp) = download.complete_path {
                let path = std::path::PathBuf::from(cp);
                if tokio::fs::metadata(&path).await.is_ok() {
                    return serve_file_from_disk(&headers, &path).await;
                }
            }
            if let Some(ref pp) = download.partial_path {
                let path = std::path::PathBuf::from(pp);
                if tokio::fs::metadata(&path).await.is_ok() {
                    return serve_file_from_disk(&headers, &path).await;
                }
            }
            Err(Error::NotFound {
                message: format!("Complete file not found on disk for stream {id}"),
            })
        }
        "downloading" | "paused" => {
            if download.status == "paused" {
                let _ = state.torrent_engine.resume(&id).await;
            } else {
                let _ = state.torrent_engine.ensure_active(&id).await;
            }

            // Detect MIME from filename
            let stream_mime = download
                .file_name
                .rsplit('.')
                .next()
                .map(mime_for_extension)
                .unwrap_or("application/octet-stream");

            if let Ok(Some((torrent_id, file_index))) =
                state.torrent_engine.get_stream_file_info(&id).await
            {
                let api = librqbit::Api::new(state.torrent_engine.session(), None);
                if let Ok(mut file_stream) =
                    api.api_stream(librqbit::api::TorrentIdOrHash::Id(torrent_id), file_index)
                {
                    let file_size = file_stream.len();
                    let range = headers
                        .get(header::RANGE)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| parse_range(s, file_size));

                    return match range {
                        Some((start, end)) => {
                            file_stream
                                .seek(std::io::SeekFrom::Start(start))
                                .await
                                .map_err(|e| Error::Io { source: e })?;
                            let length = end - start + 1;
                            let stream =
                                tokio_util::io::ReaderStream::with_capacity(file_stream, 524288);
                            let body = axum::body::Body::from_stream(stream);
                            axum::response::Response::builder()
                                .status(StatusCode::PARTIAL_CONTENT)
                                .header(header::CONTENT_TYPE, stream_mime)
                                .header(header::CONTENT_LENGTH, length.to_string())
                                .header(
                                    header::CONTENT_RANGE,
                                    format!("bytes {start}-{end}/{file_size}"),
                                )
                                .header(header::ACCEPT_RANGES, "bytes")
                                .body(body)
                                .map_err(|e| Error::Internal {
                                    message: format!("{e}"),
                                })
                        }
                        None => {
                            let stream =
                                tokio_util::io::ReaderStream::with_capacity(file_stream, 524288);
                            let body = axum::body::Body::from_stream(stream);
                            axum::response::Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_TYPE, stream_mime)
                                .header(header::CONTENT_LENGTH, file_size.to_string())
                                .header(header::ACCEPT_RANGES, "bytes")
                                .body(body)
                                .map_err(|e| Error::Internal {
                                    message: format!("{e}"),
                                })
                        }
                    };
                }
            }

            if let Ok(Some(disk_path)) = state.torrent_engine.get_file_path(&id).await {
                if tokio::fs::metadata(&disk_path).await.is_ok() {
                    return serve_file_from_disk(&headers, &disk_path).await;
                }
            }

            Err(Error::NotFound {
                message: format!("Stream {id} file not available yet"),
            })
        }
        "initializing" => {
            let body = serde_json::json!({
                "error": "Download is still initializing, try again shortly"
            });
            Ok(axum::response::Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::RETRY_AFTER, "3")
                .body(axum::body::Body::from(body.to_string()))
                .map_err(|e| Error::Internal {
                    message: format!("{e}"),
                })?)
        }
        _ => Err(Error::NotFound {
            message: format!("Stream {id} in unexpected state: {}", download.status),
        }),
    }
}

async fn serve_file_from_disk(
    headers: &HeaderMap,
    file_path: &std::path::Path,
) -> std::result::Result<axum::response::Response, Error> {
    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(|e| Error::Io { source: e })?;
    let file_size = metadata.len();

    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let content_type = mime_for_extension(ext);

    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| parse_range(s, file_size));

    match range {
        Some((start, end)) => {
            let length = end - start + 1;
            let mut file = tokio::fs::File::open(file_path)
                .await
                .map_err(|e| Error::Io { source: e })?;
            file.seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|e| Error::Io { source: e })?;
            let stream = tokio_util::io::ReaderStream::new(file.take(length));
            let body = axum::body::Body::from_stream(stream);
            axum::response::Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CONTENT_LENGTH, length.to_string())
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{file_size}"),
                )
                .header(header::ACCEPT_RANGES, "bytes")
                .body(body)
                .map_err(|e| Error::Internal {
                    message: format!("{e}"),
                })
        }
        None => {
            let file = tokio::fs::File::open(file_path)
                .await
                .map_err(|e| Error::Io { source: e })?;
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = axum::body::Body::from_stream(stream);
            axum::response::Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CONTENT_LENGTH, file_size.to_string())
                .header(header::ACCEPT_RANGES, "bytes")
                .body(body)
                .map_err(|e| Error::Internal {
                    message: format!("{e}"),
                })
        }
    }
}

/// VLC streaming endpoint: original quality, no transcode.
/// For in-progress downloads: sequential stream without Content-Length (VLC waits for data).
/// For complete downloads: full file with range support.
/// Auth via path token: /stream/{id}/vlc/{token} (VLC strips query params on macOS).
pub async fn stream_vlc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, token)): Path<(String, String)>,
) -> std::result::Result<axum::response::Response, Error> {
    // Validate token from path
    let _claims = crate::server::auth::validate_jwt(&token, &state.jwt_secret).map_err(|_| {
        Error::Unauthorized {
            message: "Invalid token".to_string(),
        }
    })?;

    let download = state
        .torrent_engine
        .get_download(&id)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Stream {id} not found"),
        })?;

    let file_name = download.file_name.clone();
    let ext = file_name.rsplit('.').next().unwrap_or("mp4").to_lowercase();
    let content_type = mime_for_extension(&ext);

    match download.status.as_str() {
        "complete" => {
            // Complete: serve from disk with range support
            let path = download
                .complete_path
                .as_deref()
                .or(download.partial_path.as_deref())
                .ok_or_else(|| Error::NotFound {
                    message: "File path not found".to_string(),
                })?;
            let path = std::path::PathBuf::from(path);
            serve_file_from_disk(&headers, &path).await
        }
        "downloading" | "paused" => {
            if download.status == "paused" {
                let _ = state.torrent_engine.resume(&id).await;
            } else {
                let _ = state.torrent_engine.ensure_active(&id).await;
            }

            // Stream via librqbit: blocks on missing pieces, VLC waits naturally
            if let Ok(Some((torrent_id, file_index))) =
                state.torrent_engine.get_stream_file_info(&id).await
            {
                let api = librqbit::Api::new(state.torrent_engine.session(), None);
                if let Ok(file_stream) =
                    api.api_stream(librqbit::api::TorrentIdOrHash::Id(torrent_id), file_index)
                {
                    let stream = tokio_util::io::ReaderStream::with_capacity(file_stream, 524288);
                    let body = axum::body::Body::from_stream(stream);
                    // No Content-Length: VLC treats as live stream, waits for more data
                    return axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, content_type)
                        .header("X-Content-Type-Options", "nosniff")
                        .body(body)
                        .map_err(|e| Error::Internal {
                            message: format!("{e}"),
                        });
                }
            }

            Err(Error::NotFound {
                message: format!("Stream {id} not available for streaming yet"),
            })
        }
        _ => Err(Error::NotFound {
            message: format!("Stream {id} in state: {}", download.status),
        }),
    }
}

/// List all files in a torrent (for multi-file album torrents).
pub async fn list_stream_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    use crate::torrent::types::TorrentFile;

    let download = state.torrent_engine.get_download(&id).await?;
    let status = download
        .as_ref()
        .map(|d| d.status.as_str())
        .unwrap_or("unknown");

    let sorted =
        crate::torrent::files::sorted_torrent_files(&state.torrent_engine, &id, download.as_ref())
            .await;
    let files: Vec<TorrentFile> = sorted
        .into_iter()
        .map(|s| TorrentFile {
            index: s.seq_index,
            path: s.path,
            size: s.size,
            is_video: s.is_video,
            is_audio: s.is_audio,
        })
        .collect();

    Ok(axum::Json(
        serde_json::json!({ "files": files, "status": status }),
    ))
}

/// Stream a specific file within a multi-file torrent by index.
///
/// `file_index` here is the alphabetical sequential index produced
/// by `list_stream_files` / `sorted_torrent_files` — not the
/// torrent metadata's native index. We translate to the native
/// index when streaming via `librqbit::api_stream`.
pub async fn stream_file_by_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, file_index)): Path<(String, usize)>,
) -> std::result::Result<axum::response::Response, Error> {
    state.torrent_engine.note_watch(&id);
    let download = state
        .torrent_engine
        .get_download(&id)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: format!("Stream {id} not found"),
        })?;

    let sorted =
        crate::torrent::files::sorted_torrent_files(&state.torrent_engine, &id, Some(&download))
            .await;
    let entry = sorted.iter().find(|s| s.seq_index == file_index);

    info!(
        stream_id = %id,
        file_index,
        native_index = ?entry.and_then(|e| e.native_index),
        path = ?entry.map(|e| e.path.as_str()),
        total_files = sorted.len(),
        download_all = download.download_all,
        "stream_file_by_index: request"
    );

    // Active path: stream pieces directly via librqbit using the
    // native index.
    if let Some(file) = entry {
        if let Some(native_idx) = file.native_index {
            if let Ok(Some((torrent_id, _))) = state
                .torrent_engine
                .get_stream_file_info_by_index(&id, native_idx)
                .await
            {
                info!(
                    stream_id = %id,
                    file_index,
                    native_idx,
                    "stream_file_by_index: serving via active librqbit stream"
                );
                let api = librqbit::Api::new(state.torrent_engine.session(), None);
                if let Ok(mut file_stream) =
                    api.api_stream(librqbit::api::TorrentIdOrHash::Id(torrent_id), native_idx)
                {
                    let stream_mime = file
                        .path
                        .rsplit('.')
                        .next()
                        .map(mime_for_extension)
                        .unwrap_or("application/octet-stream");

                    let file_size = file_stream.len();
                    let range = headers
                        .get(header::RANGE)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| parse_range(s, file_size));

                    return match range {
                        Some((start, end)) => {
                            file_stream
                                .seek(std::io::SeekFrom::Start(start))
                                .await
                                .map_err(|e| Error::Io { source: e })?;
                            let length = end - start + 1;
                            let stream =
                                tokio_util::io::ReaderStream::with_capacity(file_stream, 524288);
                            let body = axum::body::Body::from_stream(stream);
                            axum::response::Response::builder()
                                .status(StatusCode::PARTIAL_CONTENT)
                                .header(header::CONTENT_TYPE, stream_mime)
                                .header(header::CONTENT_LENGTH, length.to_string())
                                .header(
                                    header::CONTENT_RANGE,
                                    format!("bytes {start}-{end}/{file_size}"),
                                )
                                .header(header::ACCEPT_RANGES, "bytes")
                                .body(body)
                                .map_err(|e| Error::Internal {
                                    message: format!("{e}"),
                                })
                        }
                        None => {
                            let stream =
                                tokio_util::io::ReaderStream::with_capacity(file_stream, 524288);
                            let body = axum::body::Body::from_stream(stream);
                            axum::response::Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_TYPE, stream_mime)
                                .header(header::CONTENT_LENGTH, file_size.to_string())
                                .header(header::ACCEPT_RANGES, "bytes")
                                .body(body)
                                .map_err(|e| Error::Internal {
                                    message: format!("{e}"),
                                })
                        }
                    };
                }
            }
        }
    }

    // Fallback: resolve from disk (completed or partially downloaded)
    if let Some(disk_path) = resolve_file_disk_path(&state, &id, file_index).await {
        info!(
            stream_id = %id,
            file_index,
            path = %disk_path.display(),
            "stream_file_by_index: serving from disk"
        );
        return serve_file_from_disk(&headers, &disk_path).await;
    }

    info!(
        stream_id = %id,
        file_index,
        "stream_file_by_index: not found (not active and not on disk)"
    );
    Err(Error::NotFound {
        message: format!("File index {file_index} not found for stream {id}"),
    })
}

/// Resolve a sequential `file_index` (from `list_stream_files`) to
/// a disk path for the underlying file.
async fn resolve_file_disk_path(
    state: &AppState,
    info_hash: &str,
    file_index: usize,
) -> Option<std::path::PathBuf> {
    let download = state.torrent_engine.get_download(info_hash).await.ok()??;
    let sorted = crate::torrent::files::sorted_torrent_files(
        &state.torrent_engine,
        info_hash,
        Some(&download),
    )
    .await;
    let file = sorted.iter().find(|s| s.seq_index == file_index)?;

    let partial = state.torrent_engine.partial_dir();
    let complete = state.torrent_engine.complete_dir();
    for base in [complete, partial] {
        let nested = base.join(&download.title).join(&file.path);
        if tokio::fs::metadata(&nested).await.is_ok() {
            return Some(nested);
        }
        let flat = base.join(&file.path);
        if tokio::fs::metadata(&flat).await.is_ok() {
            return Some(flat);
        }
    }
    None
}

/// Extract and serve embedded artwork from an audio file's metadata tags.
pub async fn stream_artwork(
    State(state): State<AppState>,
    Path((id, file_index)): Path<(String, usize)>,
) -> std::result::Result<axum::response::Response, Error> {
    let disk_path = resolve_file_disk_path(&state, &id, file_index).await;
    let path = match disk_path {
        Some(p) => p,
        None => {
            return Err(Error::NotFound {
                message: "File not on disk yet".to_string(),
            });
        }
    };

    let artwork = tokio::task::spawn_blocking(move || -> Option<(Vec<u8>, String)> {
        let tagged = lofty::read_from_path(&path).ok()?;
        for tag in tagged.tags() {
            if let Some(pic) = tag.pictures().first() {
                let mime = match pic.mime_type() {
                    Some(lofty::picture::MimeType::Png) => "image/png",
                    Some(lofty::picture::MimeType::Bmp) => "image/bmp",
                    Some(lofty::picture::MimeType::Gif) => "image/gif",
                    Some(lofty::picture::MimeType::Tiff) => "image/tiff",
                    _ => "image/jpeg",
                };
                return Some((pic.data().to_vec(), mime.to_string()));
            }
        }
        None
    })
    .await
    .ok()
    .flatten();

    match artwork {
        Some((data, mime)) => Ok((
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
            ],
            data,
        )
            .into_response()),
        None => Err(Error::NotFound {
            message: "No embedded artwork found".to_string(),
        }),
    }
}

fn mime_for_extension(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        // Video
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "ts" => "video/mp2t",
        // Audio
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" | "aac" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "wav" => "audio/wav",
        "wma" => "audio/x-ms-wma",
        "alac" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

fn parse_range(range_header: &str, file_size: u64) -> Option<(u64, u64)> {
    let range = range_header.strip_prefix("bytes=")?;
    let (start_str, end_str) = range.split_once('-')?;

    let start: u64 = start_str.parse().ok()?;
    let end: u64 = if end_str.is_empty() {
        file_size.checked_sub(1)?
    } else {
        end_str.parse().ok()?
    };

    if start > end || start >= file_size {
        return None;
    }
    let end = end.min(file_size - 1);
    Some((start, end))
}
