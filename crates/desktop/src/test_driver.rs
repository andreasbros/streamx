//! UI-test remote driver (feature `ui-test`).
//!
//! When `STREAMX_UI_TEST_PORT` is set, the app listens on
//! 127.0.0.1:{port} for newline-delimited JSON commands and answers each
//! with one JSON line. The protocol drives the same `AppState` the UI
//! renders from, so it behaves identically on every OS; screenshots and
//! raw input stay in the external harness (`crates/ui-harness`).
//!
//! Commands:
//!   {"cmd":"ping"}                          -> {"ok":true,"pong":true}
//!   {"cmd":"page"}                          -> current page title
//!   {"cmd":"navigate","page":"Downloads"}   -> push a page
//!   {"cmd":"back"}                          -> pop the page stack
//!   {"cmd":"search","text":"batman"}        -> run a movie search
//!   {"cmd":"state"}                         -> browse/search/poster stats
//!   {"cmd":"quit"}                          -> exit the process

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::router::Page;
use crate::runtime;
use crate::state::AppState;

fn page_from_name(name: &str) -> Option<Page> {
    match name.to_ascii_lowercase().as_str() {
        "login" => Some(Page::Login),
        "search" | "movies" | "home" => Some(Page::Search),
        "movie" => Some(Page::Movie),
        "player" => Some(Page::Player),
        "history" => Some(Page::History),
        "downloads" => Some(Page::Downloads),
        "favourites" => Some(Page::Favourites),
        "settings" => Some(Page::Settings),
        "admin" => Some(Page::Admin),
        "music" | "musicsearch" => Some(Page::MusicSearch),
        "tv" | "tvsearch" => Some(Page::TvSearch),
        "musicvideos" | "musicvideosearch" => Some(Page::MusicVideoSearch),
        "surround" | "surroundsound" => Some(Page::SurroundSound),
        _ => None,
    }
}

async fn handle_command(state: &Arc<AppState>, cmd: &Value) -> Value {
    match cmd.get("cmd").and_then(|c| c.as_str()) {
        Some("ping") => json!({"ok": true, "pong": true}),
        Some("page") => json!({"ok": true, "page": state.current_page().title()}),
        Some("navigate") => {
            let name = cmd.get("page").and_then(|p| p.as_str()).unwrap_or("");
            match page_from_name(name) {
                Some(page) => {
                    state.navigate(page);
                    json!({"ok": true, "page": state.current_page().title()})
                }
                None => json!({"ok": false, "error": format!("unknown page: {name}")}),
            }
        }
        Some("back") => {
            let moved = state.back();
            json!({"ok": true, "moved": moved, "page": state.current_page().title()})
        }
        Some("search") => {
            let text = cmd
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            crate::app::run_search(state.clone(), text).await;
            json!({
                "ok": true,
                "results": state.search_results.read().len(),
            })
        }
        Some("state") => {
            let browse = state.browse.read();
            let tile_counts = json!({
                "latest": browse.latest.len(),
                "popular": browse.popular.len(),
                "top_rated": browse.top_rated.len(),
                "action": browse.action.len(),
                "comedy": browse.comedy.len(),
                "thriller": browse.thriller.len(),
                "scifi": browse.scifi.len(),
                "horror": browse.horror.len(),
            });
            let posters_with_urls: usize = browse
                .latest
                .iter()
                .chain(browse.popular.iter())
                .filter(|g| {
                    g.poster_medium.is_some()
                        || g.poster_large.is_some()
                        || g.poster_small.is_some()
                        || g.poster.is_some()
                })
                .count();
            drop(browse);
            json!({
                "ok": true,
                "page": state.current_page().title(),
                "window_open": crate::app::main_window_open(),
                "authed": state.is_authed(),
                "search_input": state.search_input_mirror.read().clone(),
                "query": state.query.read().clone(),
                "browse_loading": *state.browse_loading.read(),
                "tiles": tile_counts,
                "posters_with_urls": posters_with_urls,
                "poster_failures": state.poster_failures.lock().len(),
                "poster_pending": state.poster_pending.lock().len(),
                "ui_scale": crate::theme::ui_scale(),
                "ticks": state.tick_count.load(std::sync::atomic::Ordering::Relaxed),
                "search_results": state.search_results.read().len(),
                "downloads": state.downloads.read().len(),
                "category_items": state.category_items.read().len(),
                "connection_error": state.connection_error.read().clone(),
            })
        }
        Some("category") => {
            let name = cmd.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let spec = crate::app::category_specs()
                .into_iter()
                .find(|s| s.title.eq_ignore_ascii_case(name));
            match spec {
                Some(spec) => {
                    crate::app::open_category(state.clone(), spec).await;
                    json!({"ok": true, "items": state.category_items.read().len()})
                }
                None => json!({"ok": false, "error": format!("unknown category: {name}")}),
            }
        }
        Some("resize") => {
            let w = cmd.get("w").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let h = cmd.get("h").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            if w < 200.0 || h < 200.0 {
                return json!({"ok": false, "error": "size too small"});
            }
            *state.ui_resize.lock() = Some((w, h));
            // The tick loop applies it within ~100ms.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            json!({"ok": true})
        }
        Some("keys") => {
            let keys: Vec<String> = cmd
                .get("keys")
                .and_then(|k| k.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let n = keys.len();
            state.ui_keys.lock().extend(keys);
            json!({"ok": true, "queued": n})
        }
        Some("type") => {
            let text = cmd.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let keys: Vec<String> = text
                .chars()
                .map(|c| match c {
                    ' ' => "space".to_string(),
                    'A'..='Z' => format!("shift-{}", c.to_ascii_lowercase()),
                    _ => c.to_string(),
                })
                .collect();
            let n = keys.len();
            state.ui_keys.lock().extend(keys);
            json!({"ok": true, "queued": n})
        }
        Some("screenshot") => {
            let path = cmd
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            if path.is_empty() {
                return json!({"ok": false, "error": "missing path"});
            }
            let _ = std::fs::remove_file(&path);
            state.ui_shots.lock().push(path.clone());
            // The UI thread writes the file within a tick or two.
            for _ in 0..40 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if std::path::Path::new(&path).exists() {
                    return json!({"ok": true, "path": path});
                }
            }
            json!({"ok": false, "error": "screenshot was not produced (render_to_image unsupported on this backend?)"})
        }
        Some("quit") => {
            tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                std::process::exit(0);
            });
            json!({"ok": true, "quitting": true})
        }
        other => json!({"ok": false, "error": format!("unknown cmd: {other:?}")}),
    }
}

/// Start the driver if `STREAMX_UI_TEST_PORT` is set. Called from main
/// before the GPUI event loop; the listener lives on the tokio runtime.
pub fn maybe_start(state: &Arc<AppState>) {
    let Ok(port) = std::env::var("STREAMX_UI_TEST_PORT") else {
        return;
    };
    let Ok(port) = port.parse::<u16>() else {
        tracing::warn!("STREAMX_UI_TEST_PORT is not a valid port");
        return;
    };
    let state = state.clone();
    let _ = runtime::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("ui-test driver bind failed: {e}");
                return;
            }
        };
        tracing::info!(port, "ui-test driver listening");
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let state = state.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let reply = match serde_json::from_str::<Value>(&line) {
                        Ok(cmd) => handle_command(&state, &cmd).await,
                        Err(e) => json!({"ok": false, "error": format!("bad json: {e}")}),
                    };
                    let mut out = reply.to_string();
                    out.push('\n');
                    if write.write_all(out.as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
}
