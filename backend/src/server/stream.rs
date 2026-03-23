use crate::error::Error;
use crate::server::auth::Claims;
use crate::server::AppState;
use crate::transcode::hls::PlaylistResponse;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::debug;

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
            let _ = state.torrent_engine.pause(&id).await;
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
    let _ = state.torrent_engine.pause(&id).await;
}

pub async fn url_playlist(
    State(state): State<AppState>,
    _claims: Claims,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<axum::response::Response, Error> {
    let url = params.get("url").ok_or_else(|| Error::BadRequest {
        message: "Missing 'url' parameter".to_string(),
    })?;
    let quality = params.get("quality").map(|s| s.as_str()).unwrap_or("source");

    // Use a hash of the URL as stream_id
    let stream_id = format!("url-{:x}", md5_hash(url));

    if let Err(e) = state
        .hls_pipeline
        .start_stream_url(&stream_id, url, quality)
        .await
    {
        tracing::warn!(stream_id = %stream_id, "Failed to start URL transcode: {e}");
    }

    let response = state.hls_pipeline.generate_playlist(&stream_id, quality).await?;

    match response {
        PlaylistResponse::Redirect(redir_url) => Ok(Redirect::temporary(&redir_url).into_response()),
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

pub async fn playlist(
    State(state): State<AppState>,
    _claims: Claims,
    Path(id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> std::result::Result<axum::response::Response, Error> {
    let quality = params.get("quality").map(|s| s.as_str()).unwrap_or("source");

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
                let api = librqbit::Api::new(state.torrent_engine.session().clone(), None);
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
                } else if let Err(e) = state.hls_pipeline.start_stream(&id, probe_path, quality).await {
                    tracing::warn!(stream_id = %id, "Failed to start HLS transcode (fallback): {e}");
                }
            } else if let Err(e) = state.hls_pipeline.start_stream(&id, probe_path, quality).await {
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

    Ok(([(header::CONTENT_TYPE, content_type)], data))
}

pub async fn variant_playlist(
    State(state): State<AppState>,
    Path((id, variant)): Path<(String, String)>,
) -> std::result::Result<axum::response::Response, Error> {
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

    Ok(([(header::CONTENT_TYPE, content_type)], data))
}

pub async fn stream_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> std::result::Result<axum::response::Response, Error> {
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

            if let Ok(Some((torrent_id, file_index))) =
                state.torrent_engine.get_stream_file_info(&id).await
            {
                let api = librqbit::Api::new(state.torrent_engine.session().clone(), None);
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
                            let remaining = length;
                            let stream =
                                tokio_util::io::ReaderStream::with_capacity(file_stream, 65536);
                            let body = axum::body::Body::from_stream(stream);
                            axum::response::Response::builder()
                                .status(StatusCode::PARTIAL_CONTENT)
                                .header(header::CONTENT_TYPE, "video/mp4")
                                .header(header::CONTENT_LENGTH, remaining.to_string())
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
                                tokio_util::io::ReaderStream::with_capacity(file_stream, 65536);
                            let body = axum::body::Body::from_stream(stream);
                            axum::response::Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_TYPE, "video/mp4")
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
    let content_type = match ext {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        _ => "video/mp4",
    };

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
