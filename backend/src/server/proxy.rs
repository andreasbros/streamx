use crate::error::Error;
use crate::server::AppState;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

fn cache_key(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn ext_from_path(path: &str) -> &str {
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

fn content_type_for(ext: &str) -> &str {
    match ext {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/jpeg",
    }
}

pub const CINEMETA_PROXY_ID: u32 = 0;
const CINEMETA_IMAGE_BASE: &str = "https://images.metahub.space";

fn img_cache_dir(state: &AppState) -> PathBuf {
    state.config.data_dir.join("cache").join("img")
}

fn base_url_for_proxy(state: &AppState, provider_id: u32) -> Option<String> {
    if provider_id == CINEMETA_PROXY_ID {
        return Some(CINEMETA_IMAGE_BASE.to_string());
    }
    state
        .config
        .provider_by_id(provider_id)
        .map(|p| p.url.clone())
}

/// Single handler for all providers. Route: /proxy/{provider_id}/{*path}
pub async fn proxy_image(
    State(state): State<AppState>,
    Path((provider_id, path)): Path<(u32, String)>,
) -> std::result::Result<impl IntoResponse, Error> {
    let base_url = base_url_for_proxy(&state, provider_id).ok_or_else(|| Error::NotFound {
        message: "Unknown provider".to_string(),
    })?;

    if path.contains("..") {
        return Err(Error::BadRequest {
            message: "Invalid path".to_string(),
        });
    }

    let upstream_url = format!("{}/{}", base_url, path);
    let ext = ext_from_path(&path);
    let key = cache_key(&upstream_url);
    let cache_dir = img_cache_dir(&state);
    let cache_path = cache_dir.join(format!("{key}.{ext}"));

    // Serve from disk cache
    if cache_path.exists() {
        let bytes = tokio::fs::read(&cache_path)
            .await
            .map_err(|e| Error::Io { source: e })?;
        return Ok((
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
        ));
    }

    // Fetch upstream
    let resp = state
        .http_client
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
        bytes.to_vec(),
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
