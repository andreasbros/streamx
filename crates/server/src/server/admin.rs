use crate::error::Error;
use crate::server::auth::AuthenticatedUser;
use crate::server::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Serialize;
use std::sync::atomic::Ordering;

#[derive(Serialize)]
struct SystemStats {
    disk: DiskStats,
    process: ProcessStats,
    users: UserStats,
    streams: Vec<ActiveStream>,
    downloads: Vec<ActiveDownload>,
}

#[derive(Serialize)]
struct DiskStats {
    total_bytes: u64,
    free_bytes: u64,
    cache_bytes: u64,
    downloads_bytes: u64,
}

#[derive(Serialize)]
struct ProcessStats {
    rss_bytes: u64,
    cpu_percent: f32,
    ffmpeg_count: u32,
}

#[derive(Serialize)]
struct UserStats {
    active_connections: u32,
}

#[derive(Serialize)]
struct ActiveStream {
    stream_id: String,
    quality: String,
    status: String,
    title: String,
    file_size: u64,
    cache_bytes: u64,
    last_activity: String,
}

#[derive(Serialize)]
struct ActiveDownload {
    stream_id: String,
    title: String,
    file_name: String,
    file_size: u64,
    progress: f64,
    speed: u64,
    peers: u32,
    status: String,
    created_at: String,
    updated_at: String,
}

pub async fn admin_monitor_ws(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, Error> {
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

    Ok(ws.on_upgrade(move |socket| handle_admin_ws(socket, state)))
}

async fn handle_admin_ws(mut socket: WebSocket, state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut prev_cpu_ticks: Option<(u64, u64)> = None;
    let mut tick_count: u32 = 0;
    let mut cached_disk = DiskStats {
        total_bytes: 0,
        free_bytes: 0,
        cache_bytes: 0,
        downloads_bytes: 0,
    };

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // Disk stats: recompute dir sizes every 10 seconds (expensive)
                if tick_count % 5 == 0 {
                    let data_dir = state.config.data_dir.clone();
                    cached_disk = tokio::task::spawn_blocking(move || {
                        collect_disk_stats(&data_dir)
                    })
                    .await
                    .unwrap_or(cached_disk);
                }

                let (rss, cpu, prev) = collect_process_stats(prev_cpu_ticks);
                prev_cpu_ticks = Some(prev);

                let ffmpeg_count = count_ffmpeg_children();

                let active_ws = state.ws_connections.load(Ordering::Relaxed);
                let active_transcodes = state.hls_pipeline.active_streams().await;

                // Detect running FFmpeg output paths to mark active transcodes
                let running_paths = detect_running_ffmpeg_outputs();

                let mut streams = Vec::with_capacity(active_transcodes.len());
                for info in &active_transcodes {
                    let (title, file_size) = match state
                        .torrent_engine
                        .get_download(&info.stream_id)
                        .await
                    {
                        Ok(Some(dl)) => (dl.title, dl.file_size),
                        _ => (String::new(), 0),
                    };
                    // Override "cached" to "running" if FFmpeg is actively writing to this tier
                    let status = if info.status == "cached" {
                        let pattern = format!("{}/{}/", info.stream_id, info.quality);
                        if running_paths.iter().any(|p| p.contains(&pattern)) {
                            "running".to_string()
                        } else {
                            info.status.clone()
                        }
                    } else {
                        info.status.clone()
                    };
                    streams.push(ActiveStream {
                        stream_id: info.stream_id.clone(),
                        quality: info.quality.clone(),
                        status,
                        title,
                        file_size,
                        cache_bytes: info.cache_bytes,
                        last_activity: info.last_activity.clone(),
                    });
                }

                let downloads = match state.torrent_engine.list_downloads().await {
                    Ok(mut all) => {
                        // Sort by created_at descending (most recent first)
                        all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                        all.truncate(20);
                        let mut dls = Vec::with_capacity(all.len());
                        for dl in all {
                            let is_active =
                                dl.status == "downloading" || dl.status == "initializing";
                            let (peers, speed) = if is_active {
                                state.torrent_engine.get_live_stats(&dl.info_hash).await
                            } else {
                                (0, 0.0)
                            };
                            dls.push(ActiveDownload {
                                stream_id: dl.info_hash,
                                title: dl.title,
                                file_name: dl.file_name,
                                file_size: dl.file_size,
                                progress: dl.progress,
                                speed: speed as u64,
                                peers,
                                status: dl.status,
                                created_at: dl.created_at,
                                updated_at: dl.updated_at,
                            });
                        }
                        dls
                    }
                    Err(_) => Vec::new(),
                };

                let stats = SystemStats {
                    disk: DiskStats { ..cached_disk },
                    process: ProcessStats {
                        rss_bytes: rss,
                        cpu_percent: cpu,
                        ffmpeg_count,
                    },
                    users: UserStats {
                        active_connections: active_ws,
                    },
                    streams,
                    downloads,
                };

                let json = match serde_json::to_string(&stats) {
                    Ok(j) => j,
                    Err(_) => continue,
                };

                if socket.send(Message::Text(json.into())).await.is_err() {
                    break;
                }

                tick_count += 1;
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

fn collect_disk_stats(data_dir: &std::path::Path) -> DiskStats {
    let (total, free) = disk_space(data_dir);
    let cache_bytes = dir_size(&data_dir.join("cache"));
    let downloads_bytes = dir_size(&data_dir.join("downloads"));

    DiskStats {
        total_bytes: total,
        free_bytes: free,
        cache_bytes,
        downloads_bytes,
    }
}

fn disk_space(path: &std::path::Path) -> (u64, u64) {
    let c_path = match std::ffi::CString::new(path.to_string_lossy().as_bytes()) {
        Ok(p) => p,
        Err(_) => return (0, 0),
    };

    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
            let total = stat.f_blocks as u64 * stat.f_frsize as u64;
            let free = stat.f_bavail as u64 * stat.f_frsize as u64;
            (total, free)
        } else {
            (0, 0)
        }
    }
}

fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_dir() {
                        stack.push(entry.path());
                    } else {
                        total += meta.len();
                    }
                }
            }
        }
    }
    total
}

fn collect_process_stats(prev: Option<(u64, u64)>) -> (u64, f32, (u64, u64)) {
    let rss = read_rss_bytes();
    let (utime, stime) = read_cpu_ticks();
    let total_now = utime + stime;
    let wall_now = wall_clock_ticks();

    let cpu = if let Some((prev_total, prev_wall)) = prev {
        let dt = total_now.saturating_sub(prev_total) as f32;
        let dw = wall_now.saturating_sub(prev_wall).max(1) as f32;
        (dt / dw * 100.0).min(100.0 * num_cpus() as f32)
    } else {
        0.0
    };

    (rss, cpu, (total_now, wall_now))
}

fn read_rss_bytes() -> u64 {
    let stat = match std::fs::read_to_string("/proc/self/stat") {
        Ok(s) => s,
        Err(_) => return 0,
    };
    // Field 23 (0-indexed) is RSS in pages
    let rss_pages: u64 = stat
        .split_whitespace()
        .nth(23)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 };
    rss_pages * page_size
}

fn read_cpu_ticks() -> (u64, u64) {
    let stat = match std::fs::read_to_string("/proc/self/stat") {
        Ok(s) => s,
        Err(_) => return (0, 0),
    };
    let fields: Vec<&str> = stat.split_whitespace().collect();
    let utime: u64 = fields.get(13).and_then(|s| s.parse().ok()).unwrap_or(0);
    let stime: u64 = fields.get(14).and_then(|s| s.parse().ok()).unwrap_or(0);
    (utime, stime)
}

fn wall_clock_ticks() -> u64 {
    let uptime = std::fs::read_to_string("/proc/uptime").unwrap_or_default();
    let secs: f64 = uptime
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) as f64 };
    (secs * ticks_per_sec) as u64
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
}

fn count_ffmpeg_children() -> u32 {
    let mut count = 0u32;
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let cmdline_path = entry.path().join("cmdline");
            if let Ok(cmdline) = std::fs::read_to_string(&cmdline_path) {
                // Count any FFmpeg process writing to a streamx cache directory
                if cmdline.contains("ffmpeg") && cmdline.contains(".streamx/cache") {
                    count += 1;
                }
            }
        }
    }
    count
}

fn detect_running_ffmpeg_outputs() -> Vec<String> {
    let mut paths = Vec::new();
    let entries = match std::fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return paths,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let cmdline_path = entry.path().join("cmdline");
        if let Ok(cmdline) = std::fs::read_to_string(&cmdline_path) {
            let args: Vec<&str> = cmdline.split('\0').collect();
            if args.first().map(|a| a.contains("ffmpeg")).unwrap_or(false) {
                // Last non-empty arg is typically the output path
                if let Some(output) = args.iter().rev().find(|a| !a.is_empty() && a.contains('/'))
                {
                    paths.push(output.to_string());
                }
            }
        }
    }
    paths
}

pub async fn kill_transcode(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Path(stream_id): Path<String>,
) -> Result<impl IntoResponse, Error> {
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

    // Kill any FFmpeg processes writing to this stream's cache
    let mut killed = 0u32;
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let pid: i32 = match name_str.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let cmdline = match std::fs::read_to_string(entry.path().join("cmdline")) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if cmdline.contains("ffmpeg") && cmdline.contains(&stream_id) {
                tracing::info!(stream_id = %stream_id, pid, "Admin killing FFmpeg process");
                unsafe { libc::kill(pid, libc::SIGTERM); }
                killed += 1;
            }
        }
    }

    // Also remove from active transcodes
    state.hls_pipeline.cleanup(&stream_id).await.ok();

    Ok(axum::Json(serde_json::json!({ "killed": killed })))
}


pub async fn admin_logs_ws(
    State(state): State<AppState>,
    AuthenticatedUser(claims): AuthenticatedUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, Error> {
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

    let after_seq: u64 = params
        .get("after")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Ok(ws.on_upgrade(move |socket| handle_logs_ws(socket, state, after_seq)))
}

async fn handle_logs_ws(mut socket: WebSocket, state: AppState, after_seq: u64) {
    // Send history entries newer than the client's last seen seq
    for line in state.log_history.recent() {
        let seq = serde_json::from_str::<serde_json::Value>(&line)
            .ok()
            .and_then(|v| v.get("seq")?.as_u64())
            .unwrap_or(0);
        if seq <= after_seq {
            continue;
        }
        if socket.send(Message::Text(line.into())).await.is_err() {
            return;
        }
    }

    let mut rx = state.log_tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(line) => {
                        if socket.send(Message::Text(line.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!("Log subscriber lagged, skipped {n} messages");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}
