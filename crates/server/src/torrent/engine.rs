use crate::config::TorrentConfig;
use crate::db::downloads::Download;
use crate::db::Database;
use crate::error::{Error, Result};
use crate::torrent::types::TorrentFile;
use chrono::Utc;
use librqbit::{
    dht::PersistentDhtConfig, AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent,
    PeerConnectionOptions, Session, SessionOptions, TorrentStatsState,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Sanitize a filename from a torrent to prevent path traversal and shell injection.
/// - Decodes HTML entities (&amp; &lt; etc.)
/// - Allows: letters (unicode), digits, spaces, hyphens, underscores, dots,
///   commas, parentheses, square brackets, ampersands, plus, exclamation
/// - Strips: slashes, backslashes, quotes, backticks, null bytes, control chars
/// - Collapses multiple spaces/dots
/// - Trims leading/trailing dots and spaces
/// - Falls back to "unnamed" if result is empty
fn sanitize_filename(raw: &str) -> String {
    // Decode common HTML entities
    let decoded = raw
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "")
        .replace("&#39;", "")
        .replace("&apos;", "")
        .replace("&#x27;", "")
        .replace("&nbsp;", " ");

    let mut result = String::with_capacity(decoded.len());

    for ch in decoded.chars() {
        match ch {
            // Always allow
            'a'..='z' | 'A'..='Z' | '0'..='9' => result.push(ch),
            ' ' | '-' | '_' | '.' | ',' | '(' | ')' | '[' | ']' | '&' | '+' | '!' => {
                result.push(ch)
            }
            // Safe unicode letters (accented, CJK, etc.)
            c if c.is_alphabetic() && !c.is_control() => result.push(c),
            // Replace dangerous chars with underscore
            '/' | '\\' | '\'' | '"' | '`' | '\0' | ':' | ';' | '|' | '*' | '?' | '<' | '>'
            | '{' | '}' | '$' | '~' => result.push('_'),
            // Control chars / other - skip
            c if c.is_control() => {}
            // Everything else becomes underscore
            _ => result.push('_'),
        }
    }

    // Collapse multiple underscores/spaces/dots
    let mut prev = ' ';
    let collapsed: String = result
        .chars()
        .filter(|&c| {
            let dominated =
                (c == ' ' || c == '_' || c == '.') && (prev == ' ' || prev == '_' || prev == '.');
            if !dominated || (c == '.' && prev != '.') {
                prev = c;
                true
            } else {
                false
            }
        })
        .collect();

    // Trim leading/trailing dots and spaces (prevent hidden files, trailing dots on Windows)
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c == ' ' || c == '_');

    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed.to_string()
    }
}

struct ActiveHandle {
    torrent_id: usize,
    handle: Arc<ManagedTorrent>,
    file_index: usize,
}

/// Build a librqbit session with the engine's standard options. Shared
/// by initial startup and `restart_session`.
async fn build_session(
    config: &TorrentConfig,
    partial_dir: &Path,
    dht_dir: &Path,
    socks5: Option<String>,
) -> Result<Arc<Session>> {
    let dht_config = PersistentDhtConfig {
        config_filename: Some(dht_dir.join("dht.json")),
        ..Default::default()
    };

    let peer_opts = PeerConnectionOptions {
        connect_timeout: Some(std::time::Duration::from_secs(5)),
        read_write_timeout: Some(std::time::Duration::from_secs(10)),
        ..Default::default()
    };

    let opts = SessionOptions {
        disable_dht: !config.dht,
        disable_dht_persistence: false,
        dht_config: Some(dht_config),
        listen_port_range: Some(4240..4300),
        enable_upnp_port_forwarding: true,
        socks_proxy_url: socks5,
        peer_opts: Some(peer_opts),
        fastresume: true,
        ..Default::default()
    };

    Session::new_with_opts(partial_dir.to_path_buf(), opts)
        .await
        .map_err(|e| Error::Torrent {
            message: format!("Failed to initialize torrent session: {e}"),
        })
}

pub struct TorrentEngine {
    /// Swappable so `restart_session` can tear down every connection and
    /// bootstrap a fresh session (and DHT) without rebuilding the engine.
    /// Sync lock: held only long enough to clone the Arc.
    session: std::sync::RwLock<Arc<Session>>,
    torrent_config: TorrentConfig,
    socks5: Option<String>,
    dht_dir: PathBuf,
    handles: Arc<RwLock<HashMap<String, ActiveHandle>>>,
    /// Info-hashes currently being added via `spawn_add_torrent` so
    /// repeated `ensure_active` calls during the librqbit handshake
    /// don't spin up duplicate add tasks. Sync mutex so the non-async
    /// `spawn_add_torrent` can insert/remove without `.await`.
    pending_adds: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    db: Database,
    partial_dir: PathBuf,
    complete_dir: PathBuf,
}

impl TorrentEngine {
    pub async fn create(
        config: &TorrentConfig,
        data_dir: &Path,
        db: Database,
        socks5: Option<String>,
    ) -> Result<Self> {
        let partial_dir = data_dir.join("downloads").join("partial");
        let complete_dir = data_dir.join("downloads").join("complete");
        let dht_dir = data_dir.join("dht");

        std::fs::create_dir_all(&partial_dir).map_err(|e| Error::Torrent {
            message: format!("Failed to create partial directory: {e}"),
        })?;
        std::fs::create_dir_all(&complete_dir).map_err(|e| Error::Torrent {
            message: format!("Failed to create complete directory: {e}"),
        })?;
        std::fs::create_dir_all(&dht_dir).map_err(|e| Error::Torrent {
            message: format!("Failed to create dht directory: {e}"),
        })?;

        let session = build_session(config, &partial_dir, &dht_dir, socks5.clone()).await?;

        info!(
            "Torrent engine initialized with librqbit {}",
            librqbit::version()
        );

        let engine = Self {
            session: std::sync::RwLock::new(session),
            torrent_config: config.clone(),
            socks5,
            dht_dir,
            handles: Arc::new(RwLock::new(HashMap::new())),
            pending_adds: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            db,
            partial_dir,
            complete_dir,
        };

        engine.spawn_progress_updater();

        Ok(engine)
    }

    /// Restart the torrent client: drop every live torrent handle, stop
    /// the librqbit session (closing all peer connections and listeners),
    /// wipe the persisted DHT table so discovery bootstraps from fresh
    /// seed nodes, start a new session and re-add whatever was active or
    /// pinned. Returns the number of re-added torrents.
    pub async fn restart_session(&self) -> Result<usize> {
        let previously_live: Vec<String> = {
            let mut handles = self.handles.write().await;
            handles.drain().map(|(hash, _)| hash).collect()
        };
        self.pending_adds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        let old_session = self.session();
        old_session.stop().await;
        info!("Torrent session stopped for restart");

        // Fresh DHT bootstrap: without the persisted routing table the
        // new session discovers peers from the seed nodes again.
        let _ = tokio::fs::remove_file(self.dht_dir.join("dht.json")).await;

        let new_session = build_session(
            &self.torrent_config,
            &self.partial_dir,
            &self.dht_dir,
            self.socks5.clone(),
        )
        .await?;
        {
            let mut slot = self.session.write().unwrap_or_else(|e| e.into_inner());
            *slot = new_session;
        }
        info!("Torrent session restarted");

        // Re-add: everything that was live plus pinned incomplete rows.
        let mut to_readd: Vec<String> = previously_live;
        if let Ok(pinned) = self.db.get_pinned_incomplete().await {
            for hash in pinned {
                if !to_readd.contains(&hash) {
                    to_readd.push(hash);
                }
            }
        }
        let mut readded = 0usize;
        for hash in to_readd {
            if let Ok(Some(dl)) = self.db.get_download(&hash).await {
                if dl.status != "complete" && !dl.magnet_uri.is_empty() {
                    self.spawn_add_torrent(
                        dl.info_hash.clone(),
                        dl.magnet_uri.clone(),
                        Some(dl.file_index),
                        dl.download_all,
                    );
                    readded += 1;
                }
            }
        }
        Ok(readded)
    }

    pub async fn add_magnet(
        &self,
        magnet_uri: &str,
        file_index: Option<usize>,
    ) -> Result<Download> {
        self.add_magnet_inner(magnet_uri, file_index, false).await
    }

    /// Add a magnet and download ALL files (for music albums).
    pub async fn add_magnet_album(&self, magnet_uri: &str) -> Result<Download> {
        self.add_magnet_inner(magnet_uri, None, true).await
    }

    async fn add_magnet_inner(
        &self,
        magnet_uri: &str,
        file_index: Option<usize>,
        download_all: bool,
    ) -> Result<Download> {
        let hash = extract_info_hash(magnet_uri).ok_or_else(|| Error::BadRequest {
            message: "Could not extract info hash from magnet URI".to_string(),
        })?;

        if let Some(existing) = self.db.get_download(&hash).await? {
            if existing.status != "error" {
                info!(
                    info_hash = %hash,
                    status = %existing.status,
                    download_all = existing.download_all,
                    "add_magnet: returning existing download (no re-add)"
                );
                return Ok(existing);
            }
        }

        info!(
            info_hash = %hash,
            file_index = ?file_index,
            download_all,
            "add_magnet: creating new download"
        );

        let now = Utc::now().to_rfc3339();
        let dl = Download {
            info_hash: hash.clone(),
            magnet_uri: magnet_uri.to_string(),
            title: String::new(),
            file_name: String::new(),
            file_index: file_index.unwrap_or(0),
            file_size: 0,
            download_all,
            status: "initializing".to_string(),
            progress: 0.0,
            partial_path: None,
            complete_path: None,
            created_at: now.clone(),
            updated_at: now,
            files_json: None,
            pinned: false,
        };
        self.db.upsert_download(&dl).await?;

        self.spawn_add_torrent(
            hash.clone(),
            magnet_uri.to_string(),
            file_index,
            download_all,
        );

        Ok(dl)
    }

    pub async fn get_download(&self, info_hash: &str) -> Result<Option<Download>> {
        let mut dl = match self.db.get_download(info_hash).await? {
            Some(d) => d,
            None => return Ok(None),
        };

        let handles = self.handles.read().await;
        if let Some(active) = handles.get(info_hash) {
            let stats = active.handle.stats();
            let progress = if stats.total_bytes > 0 {
                (stats.progress_bytes as f64 / stats.total_bytes as f64) * 100.0
            } else {
                0.0
            };
            dl.progress = progress;
            if stats.finished && dl.status == "downloading" {
                dl.status = "complete".to_string();
            }
        }

        Ok(Some(dl))
    }

    /// Get live stats (peers, speed) for an active download.
    pub async fn get_live_stats(&self, info_hash: &str) -> (u32, f64) {
        let handles = self.handles.read().await;
        if let Some(active) = handles.get(info_hash) {
            let stats = active.handle.stats();
            let peers = stats
                .live
                .as_ref()
                .map(|l| l.snapshot.peer_stats.live as u32)
                .unwrap_or(0);
            let speed = stats
                .live
                .as_ref()
                .map(|l| l.download_speed.mbps * 1024.0 * 1024.0) // MiB/s -> bytes/s
                .unwrap_or(0.0);
            (peers, speed)
        } else {
            (0, 0.0)
        }
    }

    pub async fn ensure_active(&self, info_hash: &str) -> Result<()> {
        {
            let handles = self.handles.read().await;
            if handles.contains_key(info_hash) {
                info!(info_hash = %info_hash, "ensure_active: already live in session");
                return Ok(());
            }
        }
        // Skip re-spawn while an add is in flight (librqbit handshake
        // may take 30+ seconds; polling callers would otherwise kick
        // off one task per poll).
        {
            let pending = self.pending_adds.lock().unwrap_or_else(|e| e.into_inner());
            if pending.contains(info_hash) {
                info!(info_hash = %info_hash, "ensure_active: add already in flight");
                return Ok(());
            }
        }

        let dl = self
            .db
            .get_download(info_hash)
            .await?
            .ok_or_else(|| Error::NotFound {
                message: format!("Download {info_hash} not found"),
            })?;

        // "complete" in the DB might be a lie — the user may have
        // cleared downloads/{complete,partial}/ or moved files. Verify
        // the expected on-disk path exists before trusting the row;
        // otherwise re-activate and let librqbit re-download.
        if dl.status == "complete" {
            let complete = self.complete_dir.join(&dl.title).join(&dl.file_name);
            let flat = self.complete_dir.join(&dl.file_name);
            if !dl.file_name.is_empty()
                && (tokio::fs::metadata(&complete).await.is_ok()
                    || tokio::fs::metadata(&flat).await.is_ok())
            {
                info!(
                    info_hash = %info_hash,
                    download_all = dl.download_all,
                    "ensure_active: complete on disk, not re-adding (only checked default file_index)"
                );
                return Ok(());
            }
            tracing::warn!(
                info_hash = %info_hash,
                title = %dl.title,
                "marked complete but file missing; re-activating torrent"
            );
            let _ = self
                .db
                .update_download_status(info_hash, "downloading")
                .await;
        }

        info!(
            info_hash = %info_hash,
            file_index = dl.file_index,
            download_all = dl.download_all,
            status = %dl.status,
            "ensure_active: re-adding torrent to session"
        );
        self.spawn_add_torrent(
            dl.info_hash.clone(),
            dl.magnet_uri.clone(),
            Some(dl.file_index),
            dl.download_all,
        );
        Ok(())
    }

    /// Fully stop and remove a torrent from the engine (for delete/reset flows)
    pub async fn stop_and_remove(&self, info_hash: &str) -> Result<()> {
        let handle = self.handles.write().await.remove(info_hash);
        if let Some(active) = handle {
            let tid = active.handle.id();
            let _ = self
                .session()
                .delete(librqbit::api::TorrentIdOrHash::Id(tid), false)
                .await;
            info!(info_hash = %info_hash, "Torrent stopped and removed from engine");
        }
        Ok(())
    }

    pub async fn pause(&self, info_hash: &str) -> Result<()> {
        let dl = match self.db.get_download(info_hash).await? {
            Some(d) => d,
            None => return Ok(()),
        };
        if dl.status == "complete" || dl.status == "paused" {
            return Ok(());
        }

        let handles = self.handles.read().await;
        if let Some(active) = handles.get(info_hash) {
            let stats = active.handle.stats();
            if matches!(stats.state, TorrentStatsState::Live) {
                if let Err(e) = self.session().pause(&active.handle).await {
                    warn!(info_hash = %info_hash, "Pause failed (non-fatal): {e}");
                } else {
                    info!(info_hash = %info_hash, "Torrent paused");
                }
            }
        }
        drop(handles);

        self.db.update_download_status(info_hash, "paused").await?;
        Ok(())
    }

    pub async fn resume(&self, info_hash: &str) -> Result<()> {
        let dl = match self.db.get_download(info_hash).await? {
            Some(d) => d,
            None => return Ok(()),
        };

        if dl.status == "complete" || dl.status == "downloading" {
            return Ok(());
        }

        let handles = self.handles.read().await;
        if let Some(active) = handles.get(info_hash) {
            let stats = active.handle.stats();
            if matches!(stats.state, TorrentStatsState::Paused) {
                if let Err(e) = self.session().unpause(&active.handle).await {
                    warn!(info_hash = %info_hash, "Resume failed (non-fatal): {e}");
                } else {
                    info!(info_hash = %info_hash, "Torrent resumed");
                }
            }
            drop(handles);
        } else {
            drop(handles);
            if !dl.magnet_uri.is_empty() {
                self.spawn_add_torrent(
                    dl.info_hash.clone(),
                    dl.magnet_uri.clone(),
                    Some(dl.file_index),
                    dl.download_all,
                );
            }
        }

        self.db
            .update_download_status(info_hash, "downloading")
            .await?;
        Ok(())
    }

    pub async fn get_file_path(&self, info_hash: &str) -> Result<Option<PathBuf>> {
        let dl = match self.db.get_download(info_hash).await? {
            Some(d) => d,
            None => return Ok(None),
        };

        if let Some(ref cp) = dl.complete_path {
            let path = PathBuf::from(cp);
            if path.exists() {
                return Ok(Some(path));
            }
        }

        if let Some(ref pp) = dl.partial_path {
            let path = PathBuf::from(pp);
            if path.exists() {
                return Ok(Some(path));
            }
        }

        let handles = self.handles.read().await;
        if let Some(active) = handles.get(info_hash) {
            let file_idx = active.file_index;
            let partial_dir = self.partial_dir.clone();
            let file_path = active.handle.with_metadata(|meta| {
                meta.file_infos.get(file_idx).map(|fi| {
                    let relative = fi.relative_filename.to_string_lossy().to_string();
                    let full_path = partial_dir.join(&relative);
                    if full_path.exists() {
                        return full_path;
                    }
                    if let Some(ref name) = meta.name {
                        let with_name = partial_dir.join(name).join(&relative);
                        if with_name.exists() {
                            return with_name;
                        }
                    }
                    full_path
                })
            });
            match file_path {
                Ok(Some(p)) => return Ok(Some(p)),
                _ => return Ok(None),
            }
        }

        Ok(None)
    }

    pub async fn get_stream_file_info(&self, info_hash: &str) -> Result<Option<(usize, usize)>> {
        let handles = self.handles.read().await;
        let active = match handles.get(info_hash) {
            Some(a) => a,
            None => return Ok(None),
        };
        Ok(Some((active.torrent_id, active.file_index)))
    }

    /// List all files in a torrent (for multi-file album torrents).
    pub async fn list_torrent_files(&self, info_hash: &str) -> Result<Vec<TorrentFile>> {
        let handles = self.handles.read().await;
        let active = match handles.get(info_hash) {
            Some(a) => a,
            None => return Ok(Vec::new()),
        };

        let files = active
            .handle
            .with_metadata(|meta| {
                meta.file_infos
                    .iter()
                    .enumerate()
                    .map(|(idx, fi)| {
                        let path = sanitize_filename(&fi.relative_filename.to_string_lossy());
                        TorrentFile {
                            index: idx,
                            path: path.clone(),
                            size: fi.len,
                            is_video: TorrentFile::detect_video(&path),
                            is_audio: TorrentFile::detect_audio(&path),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(files)
    }

    /// Get torrent_id + file_index for streaming a specific file within a multi-file torrent.
    pub async fn get_stream_file_info_by_index(
        &self,
        info_hash: &str,
        file_index: usize,
    ) -> Result<Option<(usize, usize)>> {
        let handles = self.handles.read().await;
        let active = match handles.get(info_hash) {
            Some(a) => a,
            None => return Ok(None),
        };
        Ok(Some((active.torrent_id, file_index)))
    }

    pub fn session(&self) -> Arc<Session> {
        self.session
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn partial_dir(&self) -> &PathBuf {
        &self.partial_dir
    }

    pub fn complete_dir(&self) -> &PathBuf {
        &self.complete_dir
    }

    fn spawn_add_torrent(
        &self,
        info_hash: String,
        magnet_uri: String,
        file_index: Option<usize>,
        download_all: bool,
    ) {
        // Mark pending synchronously before returning so the next
        // ensure_active caller sees it. If another caller already
        // reserved it, skip — we're racing with them.
        {
            let mut pending = self.pending_adds.lock().unwrap_or_else(|e| e.into_inner());
            if !pending.insert(info_hash.clone()) {
                return;
            }
        }

        let session = self.session();
        let handles = self.handles.clone();
        let pending = self.pending_adds.clone();
        let db = self.db.clone();
        let partial_dir = self.partial_dir.clone();
        let complete_dir = self.complete_dir.clone();
        let info_hash_for_cleanup = info_hash.clone();

        tokio::spawn(async move {
            // Guard that removes the pending reservation on any exit
            // path (success, error, panic).
            struct PendingGuard(
                Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
                String,
            );
            impl Drop for PendingGuard {
                fn drop(&mut self) {
                    let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
                    g.remove(&self.1);
                }
            }
            let _pending_guard = PendingGuard(pending, info_hash_for_cleanup);
            info!(
                info_hash = %info_hash,
                download_all,
                only_files = ?if download_all { None } else { file_index.map(|i| vec![i]) },
                "spawn_add_torrent: adding to librqbit session"
            );
            // Adaptive timeout: start at 30s, double on each retry up to 3 attempts
            let _ = db.update_download_status(&info_hash, "initializing").await;
            let mut resp = None;
            for attempt in 0..3u32 {
                let opts = AddTorrentOptions {
                    overwrite: true,
                    only_files: if download_all {
                        None
                    } else {
                        file_index.map(|i| vec![i])
                    },
                    ..Default::default()
                };
                let timeout_secs = 30u64 << attempt; // 30s, 60s, 120s
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(timeout_secs),
                    session.add_torrent(AddTorrent::from_url(&magnet_uri), Some(opts)),
                )
                .await;

                match result {
                    Ok(Ok(r)) => {
                        resp = Some(r);
                        break;
                    }
                    Ok(Err(e)) => {
                        warn!(info_hash = %info_hash, attempt, "Failed to add torrent: {e}");
                        if attempt == 2 {
                            let _ = db.update_download_status(&info_hash, "error").await;
                            return;
                        }
                    }
                    Err(_) => {
                        warn!(
                            info_hash = %info_hash,
                            attempt,
                            timeout_secs,
                            "Timed out adding torrent, retrying with longer timeout"
                        );
                        if attempt == 2 {
                            let _ = db.update_download_status(&info_hash, "error").await;
                            return;
                        }
                    }
                }
            }

            let resp = match resp {
                Some(r) => r,
                None => return,
            };

            let (tid, handle) = match resp {
                AddTorrentResponse::Added(id, handle)
                | AddTorrentResponse::AlreadyManaged(id, handle) => (id, handle),
                _ => {
                    let _ = db.update_download_status(&info_hash, "error").await;
                    return;
                }
            };

            let resolved_fi = if download_all {
                // For album downloads, pick the first audio file (or first file)
                handle
                    .with_metadata(|meta| {
                        meta.file_infos
                            .iter()
                            .enumerate()
                            .find(|(_, f)| {
                                TorrentFile::detect_audio(&f.relative_filename.to_string_lossy())
                            })
                            .or_else(|| meta.file_infos.iter().enumerate().next())
                            .map(|(idx, _)| idx)
                    })
                    .ok()
                    .flatten()
                    .unwrap_or(0)
            } else {
                // For single-file downloads, pick the largest video file
                handle
                    .with_metadata(|meta| {
                        meta.file_infos
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| {
                                TorrentFile::detect_video(&f.relative_filename.to_string_lossy())
                            })
                            .max_by_key(|(_, f)| f.len)
                            .map(|(idx, _)| idx)
                    })
                    .ok()
                    .flatten()
                    .unwrap_or(file_index.unwrap_or(0))
            };

            let (title, file_name, file_size, partial_path) = handle
                .with_metadata(|meta| {
                    let name = sanitize_filename(&meta.name.clone().unwrap_or_default());
                    let fi = meta.file_infos.get(resolved_fi);
                    let fname = fi
                        .map(|f| sanitize_filename(&f.relative_filename.to_string_lossy()))
                        .unwrap_or_default();
                    let fsize = fi.map(|f| f.len).unwrap_or(0);
                    let pp = fi.map(|f| {
                        let rel = sanitize_filename(&f.relative_filename.to_string_lossy());
                        // Try nested path first (multi-file torrent with folder)
                        if meta.name.is_some() {
                            let nested = partial_dir.join(&name).join(&rel);
                            if nested.exists() {
                                return nested.to_string_lossy().to_string();
                            }
                            // Try flat path (single file, no folder)
                            let flat = partial_dir.join(&rel);
                            if flat.exists() {
                                return flat.to_string_lossy().to_string();
                            }
                            // File might not exist yet during initialization
                            nested.to_string_lossy().to_string()
                        } else {
                            partial_dir.join(&rel).to_string_lossy().to_string()
                        }
                    });
                    (name, fname, fsize, pp)
                })
                .unwrap_or_default();

            let _ = db
                .update_download_metadata(
                    &info_hash,
                    &title,
                    &file_name,
                    resolved_fi,
                    file_size,
                    partial_path.as_deref(),
                )
                .await;

            // Persist the stable file manifest: alphabetical by path,
            // sequentially indexed, keeping each file's native librqbit
            // index. This is the single source of truth for per-file
            // streaming and never shifts as files move on disk.
            let manifest = handle.with_metadata(|meta| {
                let mut files: Vec<crate::db::downloads::ManifestFile> = meta
                    .file_infos
                    .iter()
                    .enumerate()
                    .map(|(native_index, fi)| {
                        let path = sanitize_filename(&fi.relative_filename.to_string_lossy());
                        crate::db::downloads::ManifestFile {
                            seq_index: 0,
                            native_index,
                            is_audio: TorrentFile::detect_audio(&path),
                            is_video: TorrentFile::detect_video(&path),
                            path,
                            size: fi.len,
                        }
                    })
                    .collect();
                files.sort_by(|a, b| a.path.cmp(&b.path));
                for (seq, f) in files.iter_mut().enumerate() {
                    f.seq_index = seq;
                }
                files
            });
            if let Ok(files) = manifest {
                if let Ok(json) = serde_json::to_string(&files) {
                    let _ = db.update_download_files(&info_hash, &json).await;
                    info!(
                        info_hash = %info_hash,
                        files = files.len(),
                        "Persisted file manifest"
                    );
                }
            }

            handles.write().await.insert(
                info_hash.clone(),
                ActiveHandle {
                    torrent_id: tid,
                    handle: handle.clone(),
                    file_index: resolved_fi,
                },
            );

            let file_count = handle
                .with_metadata(|meta| meta.file_infos.len())
                .unwrap_or(0);
            info!(
                info_hash = %info_hash,
                title = %title,
                file_count,
                resolved_file_index = resolved_fi,
                download_all,
                "Torrent added to session"
            );

            Self::watch_completion(
                info_hash,
                handle,
                resolved_fi,
                db,
                partial_dir,
                complete_dir,
            );
        });
    }

    fn watch_completion(
        info_hash: String,
        handle: Arc<ManagedTorrent>,
        file_index: usize,
        db: Database,
        partial_dir: PathBuf,
        complete_dir: PathBuf,
    ) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));
            loop {
                interval.tick().await;
                let stats = handle.stats();
                if !stats.finished {
                    continue;
                }

                info!(info_hash = %info_hash, "Download complete, moving to complete directory");

                // Build a move list for EVERY file in the torrent. For a
                // single-file movie this is one entry; for an album it is
                // every track. Moving only the primary file (the old
                // behavior) left the rest stranded in partial/, which
                // broke per-file streaming after completion.
                let moves = handle.with_metadata(|meta| {
                    let raw_name = meta.name.as_deref().unwrap_or("");
                    let name = sanitize_filename(raw_name);
                    meta.file_infos
                        .iter()
                        .enumerate()
                        .map(|(idx, fi)| {
                            let raw_rel = fi.relative_filename.to_string_lossy().to_string();
                            let rel = sanitize_filename(&raw_rel);
                            // Try nested (raw/sanitized) then flat layouts.
                            let src = if !raw_name.is_empty() {
                                let candidates = [
                                    partial_dir.join(raw_name).join(&raw_rel),
                                    partial_dir.join(&name).join(&rel),
                                    partial_dir.join(&raw_rel),
                                    partial_dir.join(&rel),
                                ];
                                candidates
                                    .iter()
                                    .find(|p| p.exists())
                                    .cloned()
                                    .unwrap_or_else(|| partial_dir.join(raw_name).join(&raw_rel))
                            } else {
                                partial_dir.join(&rel)
                            };
                            (idx, src, complete_dir.join(&rel))
                        })
                        .collect::<Vec<_>>()
                });

                let moves = match moves {
                    Ok(m) if !m.is_empty() => m,
                    _ => {
                        let _ = db.update_download_status(&info_hash, "complete").await;
                        break;
                    }
                };

                // complete_path/partial_path track the PRIMARY file only
                // (movies rely on it; albums resolve per-file via the
                // manifest + disk lookup, so the others need no row).
                let mut resolved_complete: Option<String> = None;
                let mut resolved_partial: Option<String> = None;
                let mut moved = 0usize;
                for (idx, src, dst) in &moves {
                    let is_primary = *idx == file_index;
                    if !src.exists() {
                        if is_primary && dst.exists() {
                            resolved_complete = Some(dst.to_string_lossy().to_string());
                        }
                        continue;
                    }
                    if let Some(parent) = dst.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let moved_ok = if std::fs::rename(src, dst).is_ok() {
                        true
                    } else if std::fs::copy(src, dst).is_ok() {
                        let _ = std::fs::remove_file(src);
                        true
                    } else {
                        false
                    };
                    if moved_ok {
                        moved += 1;
                        if is_primary {
                            resolved_complete = Some(dst.to_string_lossy().to_string());
                        }
                    } else {
                        warn!(info_hash = %info_hash, src = %src.display(), "Failed to move file to complete");
                        if is_primary {
                            resolved_partial = Some(src.to_string_lossy().to_string());
                        }
                    }
                }

                info!(
                    info_hash = %info_hash,
                    moved,
                    total = moves.len(),
                    "Moved files to complete directory"
                );

                if resolved_complete.is_some() || resolved_partial.is_some() {
                    let _ = db
                        .update_download_paths(
                            &info_hash,
                            resolved_partial.as_deref(),
                            resolved_complete.as_deref(),
                        )
                        .await;
                }
                let _ = db.update_download_status(&info_hash, "complete").await;
                break;
            }
        });
    }

    /// Auto-heal stalled downloads. Every 30s:
    /// - an active row (`downloading`/`initializing`) with no live session
    ///   handle and no pending add is re-added — covers adds that failed
    ///   after their retries and would otherwise sit dead until a client
    ///   reconnects or the server restarts;
    /// - a live, unfinished torrent stuck at zero peers and zero speed
    ///   for three consecutive scans (~90s) is removed and re-added,
    ///   forcing a fresh tracker announce and DHT lookup — the same
    ///   effect a restart had, without restarting.
    pub fn spawn_stall_watchdog(self: &Arc<Self>) {
        let engine = self.clone();
        tokio::spawn(async move {
            let mut zero_peer_scans: HashMap<String, u32> = HashMap::new();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let downloads = match engine.db.list_downloads().await {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                for dl in downloads
                    .iter()
                    .filter(|d| d.status == "downloading" || d.status == "initializing")
                {
                    let has_handle = engine.handles.read().await.contains_key(&dl.info_hash);
                    let is_pending = engine
                        .pending_adds
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .contains(&dl.info_hash);

                    if !has_handle {
                        zero_peer_scans.remove(&dl.info_hash);
                        if !is_pending && !dl.magnet_uri.is_empty() {
                            info!(
                                info_hash = %dl.info_hash,
                                status = %dl.status,
                                "watchdog: active download has no session handle; re-adding"
                            );
                            engine.spawn_add_torrent(
                                dl.info_hash.clone(),
                                dl.magnet_uri.clone(),
                                Some(dl.file_index),
                                dl.download_all,
                            );
                        }
                        continue;
                    }

                    let (peers, speed) = engine.get_live_stats(&dl.info_hash).await;
                    if peers == 0 && speed <= 0.0 && dl.progress < 100.0 {
                        let scans = zero_peer_scans.entry(dl.info_hash.clone()).or_insert(0);
                        *scans += 1;
                        if *scans >= 3 {
                            zero_peer_scans.remove(&dl.info_hash);
                            warn!(
                                info_hash = %dl.info_hash,
                                progress = dl.progress,
                                "watchdog: no peers for ~90s; re-adding to refresh announce + DHT"
                            );
                            let _ = engine.stop_and_remove(&dl.info_hash).await;
                            engine.spawn_add_torrent(
                                dl.info_hash.clone(),
                                dl.magnet_uri.clone(),
                                Some(dl.file_index),
                                dl.download_all,
                            );
                        }
                    } else {
                        zero_peer_scans.remove(&dl.info_hash);
                    }
                }
                zero_peer_scans.retain(|hash, _| {
                    downloads.iter().any(|d| {
                        d.info_hash == *hash
                            && (d.status == "downloading" || d.status == "initializing")
                    })
                });
            }
        });
    }

    fn spawn_progress_updater(&self) {
        let handles = self.handles.clone();
        let db = self.db.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                let map = handles.read().await;
                for (info_hash, active) in map.iter() {
                    let stats = active.handle.stats();
                    if stats.finished {
                        continue;
                    }
                    let progress = if stats.total_bytes > 0 {
                        (stats.progress_bytes as f64 / stats.total_bytes as f64) * 100.0
                    } else {
                        0.0
                    };
                    let _ = db
                        .update_download_progress(info_hash, progress, stats.total_bytes)
                        .await;
                }
                drop(map);
            }
        });
    }

    pub async fn list_downloads(&self) -> Result<Vec<Download>> {
        self.db.list_downloads().await
    }
}

pub fn extract_info_hash(magnet_uri: &str) -> Option<String> {
    let uri = magnet_uri.strip_prefix("magnet:?").unwrap_or(magnet_uri);
    uri.split('&')
        .find_map(|p| p.strip_prefix("xt=urn:btih:"))
        .map(|h| h.to_lowercase())
}
