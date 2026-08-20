use crate::error::Error;
use crate::server::AppState;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

fn cache_key(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// File extension implied by an image URL path.
pub fn image_ext(path: &str) -> &'static str {
    if path.ends_with(".png") {
        "png"
    } else if path.ends_with(".webp") {
        "webp"
    } else if path.ends_with(".gif") {
        "gif"
    } else {
        "jpg"
    }
}

/// Upstream base URL for a proxy provider id.
pub fn provider_base_url(
    provider_id: u32,
    providers: &[crate::config::ProviderConfig],
) -> Option<String> {
    if provider_id == CINEMETA_PROXY_ID {
        return Some(CINEMETA_IMAGE_BASE.to_string());
    }
    providers
        .iter()
        .find(|p| p.id == provider_id)
        .map(|p| p.url.clone())
}

/// On-disk cache location for an upstream image URL. The desktop app
/// reads/writes the same files as the HTTP proxy, so the key scheme
/// must stay in sync with `fetch_proxy_bytes`.
pub fn image_cache_path(data_dir: &std::path::Path, upstream_url: &str, path: &str) -> PathBuf {
    let ext = image_ext(path);
    data_dir
        .join("cache")
        .join("img")
        .join(format!("{}.{}", cache_key(upstream_url), ext))
}

fn content_type_for(ext: &str) -> &str {
    match ext {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/jpeg",
    }
}

pub const CINEMETA_PROXY_ID: u32 = 0;
pub const CINEMETA_IMAGE_BASE: &str = "https://images.metahub.space";

/// Shared proxy fetch logic used by the HTTP handler and the desktop's
/// in-process AssetSource. Returns `(bytes, extension)`. Serves from disk
/// cache first, otherwise fetches upstream and caches.
pub async fn fetch_proxy_bytes(
    provider_id: u32,
    path: &str,
    http_client: &reqwest::Client,
    data_dir: &std::path::Path,
    providers: &[crate::config::ProviderConfig],
) -> std::result::Result<(Vec<u8>, &'static str), Error> {
    if path.contains("..") {
        return Err(Error::BadRequest {
            message: "Invalid path".to_string(),
        });
    }

    let base_url = provider_base_url(provider_id, providers).ok_or_else(|| Error::NotFound {
        message: "Unknown provider".to_string(),
    })?;

    let upstream_url = format!("{}/{}", base_url, path);
    let ext = image_ext(path);
    let cache_path = image_cache_path(data_dir, &upstream_url, path);
    let cache_dir = data_dir.join("cache").join("img");

    if cache_path.exists() {
        let bytes = tokio::fs::read(&cache_path)
            .await
            .map_err(|e| Error::Io { source: e })?;
        return Ok((bytes, ext));
    }

    let resp = http_client
        .get(&upstream_url)
        .send()
        .await
        .map_err(|_| Error::NotFound {
            message: "Failed to fetch image".to_string(),
        })?;
    if !resp.status().is_success() {
        return Err(Error::NotFound {
            message: "Image not found upstream".to_string(),
        });
    }
    let bytes = resp.bytes().await.map_err(|_| Error::Internal {
        message: "Failed to read image bytes".to_string(),
    })?;

    let _ = tokio::fs::create_dir_all(&cache_dir).await;
    let _ = tokio::fs::write(&cache_path, &bytes).await;
    Ok((bytes.to_vec(), ext))
}

/// Single handler for all providers. Route: /proxy/{provider_id}/{*path}
pub async fn proxy_image(
    State(state): State<AppState>,
    Path((provider_id, path)): Path<(u32, String)>,
) -> std::result::Result<impl IntoResponse, Error> {
    let (bytes, ext) = fetch_proxy_bytes(
        provider_id,
        &path,
        &state.http_client,
        &state.config.data_dir,
        &state.config.providers,
    )
    .await?;
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                content_type_for(ext).to_string(),
            ),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    ))
}

/// Convert an external URL to a proxy URL using the given provider ID.
pub fn to_proxy_url(url: &str, provider_id: u32) -> String {
    if url.starts_with("/proxy/") || url.starts_with("/api/") {
        return url.to_string();
    }
    if let Some(proto_end) = url.find("://") {
        if let Some(slash) = url[proto_end + 3..].find('/') {
            let path = &url[proto_end + 3 + slash + 1..];
            return format!("/proxy/{}/{}", provider_id, path);
        }
    }
    url.to_string()
}

/// Convert a proxy URL back to an absolute upstream URL.
/// Looks up the provider base URL from config.
pub fn resolve_proxy_url(proxy_url: &str, providers: &[crate::config::ProviderConfig]) -> String {
    if let Some(rest) = proxy_url.strip_prefix("/proxy/") {
        if let Some(slash) = rest.find('/') {
            if let Ok(id) = rest[..slash].parse::<u32>() {
                let path = &rest[slash + 1..];
                if id == CINEMETA_PROXY_ID {
                    return format!("{CINEMETA_IMAGE_BASE}/{path}");
                }
                if let Some(provider) = providers.iter().find(|p| p.id == id) {
                    return format!("{}/{}", provider.url, path);
                }
            }
        }
    }
    proxy_url.to_string()
}

/// List available local test media files.
/// Route: /proxy/local
pub async fn list_local_files() -> std::result::Result<impl IntoResponse, Error> {
    let media_dir = PathBuf::from("test-media/surround");
    let mut files = Vec::new();

    if let Ok(mut entries) = tokio::fs::read_dir(&media_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "mp4" | "mkv" | "webm" | "mov" | "avi") {
                continue;
            }
            if let Ok(meta) = entry.metadata().await {
                files.push(serde_json::json!({
                    "name": entry.file_name().to_string_lossy(),
                    "size": meta.len(),
                    "url": format!("/proxy/local/{}", entry.file_name().to_string_lossy()),
                }));
            }
        }
    }

    Ok(axum::Json(files))
}

/// Serve local test media files with HTTP range support for video streaming.
/// Route: /proxy/local/{filename}
pub async fn local_file(
    headers: HeaderMap,
    Path(filename): Path<String>,
) -> std::result::Result<impl IntoResponse, Error> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(Error::BadRequest {
            message: "Invalid filename".to_string(),
        });
    }

    let media_dir: PathBuf = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("test-media")
        .join("surround");

    // Also check relative to working directory
    let file_path = if media_dir.join(&filename).exists() {
        media_dir.join(&filename)
    } else {
        PathBuf::from("test-media/surround").join(&filename)
    };

    let metadata = tokio::fs::metadata(&file_path)
        .await
        .map_err(|_| Error::NotFound {
            message: format!("File not found: {filename}"),
        })?;
    let file_size = metadata.len();

    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let content_type = match ext {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    };

    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            let r = s.strip_prefix("bytes=")?;
            let (start_str, end_str) = r.split_once('-')?;
            let start: u64 = start_str.parse().ok()?;
            let end: u64 = if end_str.is_empty() {
                file_size.checked_sub(1)?
            } else {
                end_str.parse().ok()?
            };
            if start > end || start >= file_size {
                return None;
            }
            Some((start, end.min(file_size - 1)))
        });

    match range {
        Some((start, end)) => {
            let length = end - start + 1;
            let mut file = tokio::fs::File::open(&file_path)
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
                .header(header::CACHE_CONTROL, "no-cache, no-store")
                .body(body)
                .map_err(|e| Error::Internal {
                    message: format!("{e}"),
                })
        }
        None => {
            let file = tokio::fs::File::open(&file_path)
                .await
                .map_err(|e| Error::Io { source: e })?;
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = axum::body::Body::from_stream(stream);
            axum::response::Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CONTENT_LENGTH, file_size.to_string())
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CACHE_CONTROL, "no-cache, no-store")
                .body(body)
                .map_err(|e| Error::Internal {
                    message: format!("{e}"),
                })
        }
    }
}
