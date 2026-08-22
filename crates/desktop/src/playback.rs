//! Media playback: resolve a stream_id + file_index to a URL or local path,
//! then launch mpv on it.
//!
//! Embedded mode prefers a local file path (no HTTP overhead, no duplicate
//! copy, works for files the server wrote to `~/.streamx/downloads/`).
//! Thin-client mode always returns an HTTP URL.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::state::{AppState, Mode};
use crate::theme::Theme;
use streamx_api::client::Client;
use streamx_api::types::TorrentFile;

/// What we hand to the player.
#[derive(Debug, Clone)]
pub enum PlayTarget {
    LocalFile(PathBuf),
    Http { url: String, token: Option<String> },
}

impl PlayTarget {
    pub fn display(&self) -> String {
        match self {
            PlayTarget::LocalFile(p) => p.display().to_string(),
            PlayTarget::Http { url, .. } => url.clone(),
        }
    }

    /// Argument for mpv. For HTTP with a bearer token we pipe it through
    /// `--http-header-fields`.
    pub fn mpv_args(&self) -> Vec<String> {
        match self {
            PlayTarget::LocalFile(p) => vec![p.display().to_string()],
            PlayTarget::Http { url, token } => {
                let mut args = vec![url.clone()];
                if let Some(t) = token {
                    args.push(format!("--http-header-fields=Authorization: Bearer {t}"));
                }
                args
            }
        }
    }
}

/// Resolve a play target for a given stream + file index, using the current
/// desktop mode.
pub async fn resolve(
    state: &AppState,
    client: Client,
    stream_id: &str,
    file_index: usize,
) -> Result<PlayTarget, String> {
    let mode = *state.mode.read();

    if mode == Mode::Embedded {
        // Ask server for the file list + status.
        let (files, status) = client
            .stream_files(stream_id)
            .await
            .map_err(|e| format!("stream_files failed: {e}"))?;
        let file: &TorrentFile = files
            .iter()
            .find(|f| f.index == file_index)
            .ok_or_else(|| format!("no file at index {file_index}"))?;

        // Only hand mpv a local file path once the torrent is complete.
        // While downloading, the file at partial/ may have missing pieces
        // (sparse holes → garbage on playback) and will be moved under
        // complete/ when the download finishes (breaking the open handle).
        // HTTP streaming via librqbit::api_stream fills pieces on demand
        // and survives the move transparently.
        let is_complete = matches!(status.as_deref(), Some("complete"));
        let candidates = candidate_paths(&state.downloads_dir, &files, file);
        if let Some(cand) = local_file_for(status.as_deref(), &candidates, |p| p.exists()) {
            tracing::info!(
                stream_id = %stream_id,
                file_index,
                chosen = %cand.display(),
                file_path = %file.path,
                "resolved to local file (download complete)"
            );
            return Ok(PlayTarget::LocalFile(cand));
        }
        if is_complete {
            // File is gone. The server's ensure_active (triggered by
            // stream_files above) has already detected this and kicked
            // off a re-download; status will flip to "downloading" once
            // librqbit finishes adding the torrent. mpv over HTTP will
            // then serve pieces as they arrive.
            tracing::warn!(
                stream_id = %stream_id,
                file_index,
                "marked complete but file missing; server is reactivating"
            );
        } else {
            tracing::info!(
                stream_id = %stream_id,
                file_index,
                status = %status.as_deref().unwrap_or("?"),
                "download in progress, using HTTP streaming"
            );
        }
    }

    let url = format!(
        "{}/api/stream/{}/file/{}",
        state.server_url.read().trim_end_matches('/'),
        stream_id,
        file_index
    );
    let token = state.token.read().clone();
    Ok(PlayTarget::Http { url, token })
}

/// Decide whether playback can open a local file directly. Only a
/// download the server reports as `complete` qualifies: while
/// downloading, paused, or errored the file may have holes and will be
/// moved when it finishes, so those always stream over HTTP.
pub fn local_file_for(
    status: Option<&str>,
    candidates: &[PathBuf],
    exists: impl Fn(&std::path::Path) -> bool,
) -> Option<PathBuf> {
    if status != Some("complete") {
        return None;
    }
    candidates.iter().find(|c| exists(c)).cloned()
}

/// Build the list of likely on-disk paths for a file. Order matters -
/// `complete` first, then `partial`; nested (inside a folder named after the
/// torrent) first, then flat.
pub fn candidate_paths(
    downloads_dir: &std::path::Path,
    files: &[TorrentFile],
    target: &TorrentFile,
) -> Vec<PathBuf> {
    // Single-file torrents put the file directly at the root of partial/.
    // Multi-file torrents nest under a folder named after `meta.name`.
    // We don't have the torrent name here so we also try the longest common
    // prefix of paths as a best-effort directory guess.
    let common_dir = longest_common_dir(files);
    let tail = target.path.as_str();

    let complete = downloads_dir.join("complete");
    let partial = downloads_dir.join("partial");

    let mut out = Vec::new();

    for base in [&complete, &partial] {
        if let Some(dir) = &common_dir {
            out.push(base.join(dir).join(tail));
        }
        out.push(base.join(tail));
    }

    out
}

pub fn longest_common_dir(files: &[TorrentFile]) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let first = files[0].path.split('/').next()?.to_string();
    if files
        .iter()
        .all(|f| f.path.starts_with(&format!("{first}/")))
    {
        Some(first)
    } else {
        None
    }
}

/// Handle returned by `launch_mpv`: the spawned mpv plus the IPC socket
/// path it was told to bind. Once mpv finishes its own bootstrap (usually
/// <200ms) you can connect with `MpvIpc::connect`.
pub struct MpvInstance {
    pub child: Child,
    pub socket_path: PathBuf,
}

pub mod embedded;

/// A running player: libmpv inside this process, or a spawned mpv
/// executable when libmpv cannot start.
pub enum Player {
    Embedded(std::sync::Arc<embedded::EmbeddedPlayer>),
    Spawned(MpvInstance),
}

impl Player {
    /// True when the player window is gone.
    pub fn is_finished(&mut self) -> bool {
        match self {
            Player::Embedded(p) => p.is_finished(),
            Player::Spawned(m) => matches!(m.child.try_wait(), Ok(Some(_))),
        }
    }

    pub fn stop(&mut self) {
        match self {
            Player::Embedded(p) => p.stop(),
            Player::Spawned(m) => {
                let _ = m.child.kill();
            }
        }
    }
}

/// Transport-agnostic playback controls.
#[derive(Clone)]
pub enum Control {
    Embedded(std::sync::Arc<embedded::EmbeddedPlayer>),
    Ipc(ipc::MpvIpc),
}

impl Control {
    pub async fn toggle_pause(&self) -> Result<(), String> {
        match self {
            Control::Embedded(p) => p.toggle_pause(),
            Control::Ipc(i) => i.toggle_pause().await,
        }
    }

    pub async fn seek(&self, seconds: f64, relative: bool) -> Result<(), String> {
        match self {
            Control::Embedded(p) => p.seek(seconds, relative),
            Control::Ipc(i) => i.seek(seconds, relative).await,
        }
    }

    pub async fn snapshot(&self) -> ipc::Snapshot {
        match self {
            Control::Embedded(p) => p.snapshot(),
            Control::Ipc(i) => ipc::snapshot(i).await,
        }
    }
}

/// Start playback: libmpv in-process first, falling back to spawning
/// an mpv executable (controls then arrive once its IPC socket is up).
/// The fallback is returned with `Control::None` semantics via the
/// second tuple element being `None`.
pub fn launch(target: &PlayTarget, theme: &Theme) -> Result<(Player, Option<Control>), String> {
    match embedded::EmbeddedPlayer::launch(target) {
        Ok(p) => {
            let p = std::sync::Arc::new(p);
            tracing::info!("playback: libmpv embedded player started");
            Ok((Player::Embedded(p.clone()), Some(Control::Embedded(p))))
        }
        Err(e) => {
            tracing::warn!("embedded libmpv unavailable ({e}); falling back to mpv executable");
            let instance = launch_mpv(target, theme)?;
            Ok((Player::Spawned(instance), None))
        }
    }
}

/// Launch mpv on a PlayTarget. Opens a JSON IPC socket so the desktop can
/// pause/seek/query state while mpv renders the video.
pub fn launch_mpv(target: &PlayTarget, theme: &Theme) -> Result<MpvInstance, String> {
    let _ = theme;

    let socket_path = mpv_socket_path();
    let mut args = target.mpv_args();
    args.push(format!("--input-ipc-server={}", socket_path.display()));

    tracing::info!(?args, ?socket_path, "launching mpv");

    // Conservative args only — options that work on every mpv ≥0.35.
    // Inherit stdout+stderr so mpv's own diagnostics show up next to
    // ours (previously we swallowed them, which made silent failures
    // impossible to debug).
    let mpv = resolve_mpv_binary()?;
    let child = Command::new(&mpv)
        .args(&args)
        .arg("--force-window=yes")
        .arg("--keep-open=always")
        // Free window resizing: decorations on, and don't lock the window
        // shape to the video aspect (mpv letterboxes instead), so
        // horizontal / vertical / diagonal drags all work.
        .arg("--border=yes")
        .arg("--keepaspect-window=no")
        .arg("--ytdl=no")
        .arg("--cache=yes")
        .arg("--cache-secs=300")
        .arg("--demuxer-max-bytes=2G")
        .arg("--demuxer-max-back-bytes=500M")
        .arg("--demuxer-readahead-secs=120")
        .arg("--network-timeout=600")
        .arg("--stream-lavf-o=reconnect=1,reconnect_streamed=1,reconnect_delay_max=30")
        .arg("--hr-seek=yes")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn mpv at {}: {e}", mpv.display()))?;

    Ok(MpvInstance { child, socket_path })
}

/// Build-time mpv location from the Nix dev shell, if the binary was
/// built there.
const MPV_BUILD_PATH: Option<&str> = option_env!("STREAMX_MPV_BUILD_PATH");

/// Well-known install locations checked after PATH, so the app works
/// when launched from Finder or a plain terminal that lacks the Nix
/// store or Homebrew on PATH.
const MPV_KNOWN_LOCATIONS: &[&str] = &[
    "/opt/homebrew/bin/mpv",
    "/usr/local/bin/mpv",
    "/usr/bin/mpv",
    "/run/current-system/sw/bin/mpv",
    "/nix/var/nix/profiles/default/bin/mpv",
    "/Applications/mpv.app/Contents/MacOS/mpv",
];

/// Locate the mpv binary: `STREAMX_MPV` override, then PATH, then the
/// build-time path, then well-known locations.
pub fn resolve_mpv_binary() -> Result<PathBuf, String> {
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let override_path = std::env::var_os("STREAMX_MPV").map(PathBuf::from);
    resolve_mpv_from(
        override_path.as_deref(),
        &path_dirs,
        MPV_BUILD_PATH,
        home.as_deref(),
        |p| p.is_file(),
    )
}

/// Pure resolution over injected inputs so the search order is unit
/// testable without touching the real filesystem.
pub fn resolve_mpv_from(
    override_path: Option<&std::path::Path>,
    path_dirs: &[PathBuf],
    build_path: Option<&str>,
    home: Option<&std::path::Path>,
    exists: impl Fn(&std::path::Path) -> bool,
) -> Result<PathBuf, String> {
    let mut tried: Vec<PathBuf> = Vec::new();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = override_path {
        candidates.push(p.to_path_buf());
    }
    candidates.extend(path_dirs.iter().map(|d| d.join("mpv")));
    if let Some(p) = build_path {
        candidates.push(PathBuf::from(p));
    }
    candidates.extend(MPV_KNOWN_LOCATIONS.iter().map(PathBuf::from));
    if let Some(h) = home {
        candidates.push(h.join(".nix-profile/bin/mpv"));
    }
    for c in candidates {
        if exists(&c) {
            return Ok(c);
        }
        tried.push(c);
    }
    Err(format!(
        "mpv not found. Set STREAMX_MPV to the mpv binary, install mpv, or launch from \
         `nix develop`. Looked in: {}",
        tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn mpv_socket_path() -> PathBuf {
    let tmp = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    tmp.join(format!("streamx-mpv-{pid}-{nanos}.sock"))
}

pub mod ipc {
    //! mpv JSON IPC over Unix domain socket. One request/response at a
    //! time — serialized through a tokio Mutex. Fire-and-forget commands
    //! (pause, seek) and property polling (time-pos, duration) live here.

    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use serde::{Deserialize, Serialize};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use tokio::sync::Mutex;

    #[derive(Debug, Serialize)]
    struct Request {
        command: Vec<serde_json::Value>,
        request_id: u64,
    }

    #[derive(Debug, Deserialize)]
    struct Response {
        #[serde(default)]
        data: serde_json::Value,
        #[serde(default)]
        error: String,
        #[serde(default)]
        request_id: Option<u64>,
    }

    #[derive(Clone)]
    pub struct MpvIpc {
        inner: Arc<Mutex<Inner>>,
        pub socket_path: PathBuf,
    }

    struct Inner {
        reader: BufReader<tokio::net::unix::OwnedReadHalf>,
        writer: tokio::net::unix::OwnedWriteHalf,
        next_id: u64,
    }

    impl MpvIpc {
        /// Connect to an mpv instance, retrying for up to ~2s while mpv
        /// boots and creates the socket.
        pub async fn connect(path: &Path) -> Result<Self, String> {
            for _ in 0..40 {
                match UnixStream::connect(path).await {
                    Ok(stream) => {
                        let (r, w) = stream.into_split();
                        return Ok(Self {
                            inner: Arc::new(Mutex::new(Inner {
                                reader: BufReader::new(r),
                                writer: w,
                                next_id: 1,
                            })),
                            socket_path: path.to_path_buf(),
                        });
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                }
            }
            Err(format!(
                "mpv IPC socket never appeared at {}",
                path.display()
            ))
        }

        async fn call(&self, args: Vec<serde_json::Value>) -> Result<serde_json::Value, String> {
            let mut inner = self.inner.lock().await;
            let id = inner.next_id;
            inner.next_id += 1;
            let req = Request {
                command: args,
                request_id: id,
            };
            let mut line = serde_json::to_string(&req).map_err(|e| format!("serialize: {e}"))?;
            line.push('\n');
            inner
                .writer
                .write_all(line.as_bytes())
                .await
                .map_err(|e| format!("write: {e}"))?;

            // Skip event lines that don't carry our request_id.
            loop {
                let mut buf = String::new();
                let n = inner
                    .reader
                    .read_line(&mut buf)
                    .await
                    .map_err(|e| format!("read: {e}"))?;
                if n == 0 {
                    return Err("mpv IPC closed".into());
                }
                let resp: Response = match serde_json::from_str(&buf) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if resp.request_id != Some(id) {
                    continue;
                }
                if resp.error != "success" {
                    return Err(resp.error);
                }
                return Ok(resp.data);
            }
        }

        pub async fn toggle_pause(&self) -> Result<(), String> {
            self.call(vec![
                serde_json::Value::from("cycle"),
                serde_json::Value::from("pause"),
            ])
            .await
            .map(|_| ())
        }

        pub async fn seek(&self, seconds: f64, relative: bool) -> Result<(), String> {
            let mode = if relative { "relative" } else { "absolute" };
            self.call(vec![
                serde_json::Value::from("seek"),
                serde_json::Value::from(seconds),
                serde_json::Value::from(mode),
            ])
            .await
            .map(|_| ())
        }

        pub async fn get_property_f64(&self, name: &str) -> Result<Option<f64>, String> {
            let v = self
                .call(vec![
                    serde_json::Value::from("get_property"),
                    serde_json::Value::from(name),
                ])
                .await?;
            Ok(v.as_f64())
        }

        pub async fn get_property_bool(&self, name: &str) -> Result<Option<bool>, String> {
            let v = self
                .call(vec![
                    serde_json::Value::from("get_property"),
                    serde_json::Value::from(name),
                ])
                .await?;
            Ok(v.as_bool())
        }
    }

    /// One-shot snapshot used by the Player page.
    #[derive(Debug, Default, Clone)]
    pub struct Snapshot {
        pub paused: bool,
        pub time_pos: f64,
        pub duration: f64,
    }

    pub async fn snapshot(ipc: &MpvIpc) -> Snapshot {
        Snapshot {
            paused: ipc
                .get_property_bool("pause")
                .await
                .ok()
                .flatten()
                .unwrap_or(false),
            time_pos: ipc
                .get_property_f64("time-pos")
                .await
                .ok()
                .flatten()
                .unwrap_or(0.0),
            duration: ipc
                .get_property_f64("duration")
                .await
                .ok()
                .flatten()
                .unwrap_or(0.0),
        }
    }
}
