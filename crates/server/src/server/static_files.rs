use crate::embedded::Asset;
use crate::server::AppState;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};

/// Build hash for cache busting - set at compile time
pub const BUILD_HASH: &str = env!("STREAMX_BUILD_HASH");
pub const VERSION: &str = env!("STREAMX_VERSION");

pub async fn static_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let scheme =
        if host.contains("localhost") || host.starts_with("127.") || host.starts_with("192.168.") {
            "http"
        } else {
            "https"
        };
    let base_url = format!("{scheme}://{host}");
    let path = uri.path().trim_start_matches('/');

    // Versioned asset path: /assets/{hash}/... -> strip hash, resolve to real file
    let hash_prefix = format!("{BUILD_HASH}/");
    if path.starts_with("assets/") {
        if let Some(after_assets) = path.strip_prefix("assets/") {
            if let Some(stripped) = after_assets.strip_prefix(&hash_prefix) {
                // Try assets/{file} first (Vite-bundled), then {file} directly (public/ files)
                let candidates = [format!("assets/{stripped}"), stripped.to_string()];
                for candidate in &candidates {
                    if let Some(content) = Asset::get(candidate) {
                        let mime = mime_guess::from_path(candidate)
                            .first_or_octet_stream()
                            .to_string();
                        let mut response = (
                            StatusCode::OK,
                            [
                                (header::CONTENT_TYPE, mime),
                                (
                                    header::CACHE_CONTROL,
                                    "public, max-age=31536000, immutable".to_string(),
                                ),
                            ],
                            content.data.to_vec(),
                        )
                            .into_response();
                        // Service workers need scope override when served from non-root path
                        if candidate.ends_with("sw.js") {
                            response.headers_mut().insert(
                                header::HeaderName::from_static("service-worker-allowed"),
                                header::HeaderValue::from_static("/"),
                            );
                        }
                        return response;
                    }
                }
            } else {
                // Non-versioned /assets/ path (e.g. Vite content-hashed filenames in JS)
                if let Some(content) = Asset::get(path) {
                    let mime = mime_guess::from_path(path)
                        .first_or_octet_stream()
                        .to_string();
                    return (
                        StatusCode::OK,
                        [
                            (header::CONTENT_TYPE, mime),
                            (
                                header::CACHE_CONTROL,
                                "public, max-age=31536000, immutable".to_string(),
                            ),
                        ],
                        content.data.to_vec(),
                    )
                        .into_response();
                }
            }
        }
    }

    // Direct static files (backward compat + non-hashed assets like sw.js, icons)
    if let Some(content) = Asset::get(path) {
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();

        // index.html: no cache (must always be fresh, triggers versioned asset URLs)
        let cache = if path == "index.html" {
            "no-cache, no-store".to_string()
        } else {
            "public, max-age=31536000, immutable".to_string()
        };

        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)],
            content.data.to_vec(),
        )
            .into_response();
    }

    // SPA fallback: serve index.html with OG tags for player pages
    let Some(index) = Asset::get("index.html") else {
        return Html(
            "<html><body>\
             <h1>StreamX</h1>\
             <p>Frontend assets are not available. \
             Build the UI with <code>cd ui &amp;&amp; pnpm build</code> \
             and restart the server.</p>\
             </body></html>"
                .to_string(),
        )
        .into_response();
    };

    let html = String::from_utf8_lossy(&index.data).to_string();

    // Rewrite asset paths in HTML to include the build hash
    let html = rewrite_asset_paths(&html);

    // Inject OG meta tags for /player/{id} and /music/play/{id}/{fileIndex} routes
    let html = if let Some(stream_id) = parse_player_path(path) {
        if let Ok(Some(meta)) = state.db.get_metadata(&stream_id).await {
            inject_og_tags(&html, &meta, &stream_id, &base_url)
        } else {
            html
        }
    } else if let Some((stream_id, file_index)) = parse_music_path(path) {
        inject_music_og_tags(&html, &state, &stream_id, file_index, &base_url).await
    } else {
        html
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::CACHE_CONTROL, "no-cache, no-store".to_string()),
        ],
        html.into_bytes(),
    )
        .into_response()
}

/// Rewrite all static asset paths in index.html to include the build hash
fn rewrite_asset_paths(html: &str) -> String {
    html.replace("\"/assets/", &format!("\"/assets/{BUILD_HASH}/"))
        .replace("'/assets/", &format!("'/assets/{BUILD_HASH}/"))
        .replace("\"/icons/", &format!("\"/assets/{BUILD_HASH}/icons/"))
        .replace("'/icons/", &format!("'/assets/{BUILD_HASH}/icons/"))
        .replace(
            "\"/default-poster.jpg\"",
            &format!("\"/assets/{BUILD_HASH}/default-poster.jpg\""),
        )
        .replace("\"/sw.js\"", &format!("\"/assets/{BUILD_HASH}/sw.js\""))
}

fn parse_player_path(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() >= 2 && segments[0] == "player" {
        let id = segments[1];
        if id.len() >= 10 && id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(id.to_string());
        }
    }
    None
}

/// Parse /music/play/{streamId}/{fileIndex} paths
fn parse_music_path(path: &str) -> Option<(String, usize)> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() >= 4 && segments[0] == "music" && segments[1] == "play" {
        let id = segments[2];
        if id.len() >= 10 && id.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Ok(fi) = segments[3].parse::<usize>() {
                return Some((id.to_string(), fi));
            }
        }
    }
    None
}

async fn inject_music_og_tags(
    html: &str,
    state: &AppState,
    stream_id: &str,
    file_index: usize,
    base_url: &str,
) -> String {
    // Get album/torrent title from download
    let album_title = state
        .torrent_engine
        .get_download(stream_id)
        .await
        .ok()
        .flatten()
        .map(|d| d.title.clone())
        .unwrap_or_default();

    // Try to get track title from file list (active handles first, then disk scan)
    let _ = state.torrent_engine.ensure_active(stream_id).await;
    let mut files = state
        .torrent_engine
        .list_torrent_files(stream_id)
        .await
        .unwrap_or_default();

    // Disk scan fallback for completed downloads
    if files.is_empty() {
        let partial = state.torrent_engine.partial_dir();
        let complete = state.torrent_engine.complete_dir();
        for base in [complete, partial] {
            let dir = base.join(&album_title);
            if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
                let mut idx = 0;
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if let Ok(meta) = entry.metadata().await {
                        if meta.is_file() {
                            let p = entry.file_name().to_string_lossy().to_string();
                            files.push(crate::torrent::types::TorrentFile {
                                index: idx,
                                path: p.clone(),
                                size: meta.len(),
                                is_video: crate::torrent::types::TorrentFile::detect_video(&p),
                                is_audio: crate::torrent::types::TorrentFile::detect_audio(&p),
                            });
                            idx += 1;
                        }
                    }
                }
                if !files.is_empty() {
                    files.sort_by(|a, b| a.path.cmp(&b.path));
                    for (i, f) in files.iter_mut().enumerate() {
                        f.index = i;
                    }
                    break;
                }
            }
        }
    }

    let track_title = files
        .iter()
        .find(|f| f.index == file_index)
        .map(|f| {
            let name = f.path.rsplit('/').next().unwrap_or(&f.path);
            let without_ext = name.rsplit_once('.').map(|(n, _)| n).unwrap_or(name);
            let trimmed = without_ext.trim_start_matches(|c: char| {
                c.is_ascii_digit() || c == '.' || c == '-' || c == ' ' || c == '_'
            });
            if trimmed.is_empty() {
                f.path.clone()
            } else {
                trimmed.to_string()
            }
        })
        .unwrap_or_else(|| format!("Track {}", file_index + 1));

    let artwork_url = format!("{base_url}/api/stream/{stream_id}/artwork/{file_index}");
    let default_poster = format!("{base_url}/assets/{BUILD_HASH}/default-poster.jpg");
    let page_url = format!("{base_url}/music/play/{stream_id}/{file_index}");

    let description = if album_title.is_empty() {
        "Listen on StreamX".to_string()
    } else {
        format!("From: {}", html_escape(&album_title))
    };

    let og_tags = format!(
        r#"<meta property="og:title" content="{title}" />
    <meta property="og:description" content="{desc}" />
    <meta property="og:image" content="{artwork}" />
    <meta property="og:image:width" content="600" />
    <meta property="og:image:height" content="600" />
    <meta property="og:type" content="music.song" />
    <meta property="og:url" content="{url}" />
    <meta property="og:audio" content="{base_url}/api/stream/{stream_id}/file/{file_index}" />
    <meta name="twitter:card" content="summary_large_image" />
    <meta name="twitter:title" content="{title}" />
    <meta name="twitter:description" content="{desc}" />
    <meta name="twitter:image" content="{fallback}" />"#,
        title = html_escape(&track_title),
        desc = description,
        artwork = artwork_url,
        url = page_url,
        fallback = default_poster,
        stream_id = stream_id,
        file_index = file_index,
    );

    html.replace("</head>", &format!("{og_tags}\n  </head>"))
}

fn inject_og_tags(
    html: &str,
    meta: &crate::db::metadata::MediaMetadata,
    stream_id: &str,
    base_url: &str,
) -> String {
    let title = if meta.title.is_empty() {
        "StreamX"
    } else {
        &meta.title
    };

    let year = meta.year.map(|y| format!(" ({y})")).unwrap_or_default();

    let description = meta
        .summary
        .as_deref()
        .unwrap_or("Stream video with StreamX");

    let default_poster = format!("/assets/{BUILD_HASH}/default-poster.jpg");
    let poster_path = meta
        .local_poster
        .as_deref()
        .or(meta.poster_large.as_deref())
        .unwrap_or(&default_poster);
    let poster = if poster_path.starts_with("http") {
        poster_path.to_string()
    } else {
        format!("{base_url}{poster_path}")
    };

    let og_tags = format!(
        r#"<meta property="og:title" content="{title}{year}" />
    <meta property="og:description" content="{desc}" />
    <meta property="og:image" content="{poster}" />
    <meta property="og:image:width" content="600" />
    <meta property="og:image:height" content="900" />
    <meta property="og:type" content="video.movie" />
    <meta property="og:url" content="{base_url}/player/{stream_id}" />
    <meta name="twitter:card" content="summary_large_image" />
    <meta name="twitter:title" content="{title}{year}" />
    <meta name="twitter:description" content="{desc}" />
    <meta name="twitter:image" content="{poster}" />"#,
        title = html_escape(title),
        year = year,
        desc = html_escape(&description[..description.len().min(200)]),
        poster = poster,
        stream_id = stream_id,
    );

    html.replace("</head>", &format!("{og_tags}\n  </head>"))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
