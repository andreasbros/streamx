use crate::config::TorrentConfig;
use crate::db::downloads::Download;
use crate::db::Database;
use crate::error::{Error, Result};
use crate::torrent::types::TorrentFile;
use chrono::Utc;
use librqbit::{
    dht::PersistentDhtConfig, AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent,
    Session, SessionOptions, TorrentStatsState,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

struct ActiveHandle {
    torrent_id: usize,
    handle: Arc<ManagedTorrent>,
    file_index: usize,
}

pub struct TorrentEngine {
    session: Arc<Session>,
    handles: Arc<RwLock<HashMap<String, ActiveHandle>>>,
    db: Database,
    partial_dir: PathBuf,
    complete_dir: PathBuf,
}

impl TorrentEngine {
    pub async fn create(config: &TorrentConfig, data_dir: &Path, db: Database) -> Result<Self> {
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

        let dht_config = PersistentDhtConfig {
            config_filename: Some(dht_dir.join("dht.json")),
            ..Default::default()
        };

        let opts = SessionOptions {
            disable_dht: !config.dht,
            disable_dht_persistence: false,
            dht_config: Some(dht_config),
            listen_port_range: Some(4240..4260),
            enable_upnp_port_forwarding: true,
            ..Default::default()
        };

        let session = Session::new_with_opts(partial_dir.clone(), opts)
            .await
            .map_err(|e| Error::Torrent {
                message: format!("Failed to initialize torrent session: {e}"),
            })?;

        info!(
            "Torrent engine initialized with librqbit {}",
            librqbit::version()
        );

        let engine = Self {
            session,
            handles: Arc::new(RwLock::new(HashMap::new())),
            db,
            partial_dir,
            complete_dir,
        };

        engine.spawn_progress_updater();

        Ok(engine)
    }

    pub async fn add_magnet(
        &self,
        magnet_uri: &str,
        file_index: Option<usize>,
    ) -> Result<Download> {
        let hash = extract_info_hash(magnet_uri).ok_or_else(|| Error::BadRequest {
            message: "Could not extract info hash from magnet URI".to_string(),
        })?;

        if let Some(existing) = self.db.get_download(&hash).await? {
            if existing.status != "error" {
                return Ok(existing);
            }
        }

        let now = Utc::now().to_rfc3339();
        let dl = Download {
            info_hash: hash.clone(),
            magnet_uri: magnet_uri.to_string(),
            title: String::new(),
            file_name: String::new(),
            file_index: file_index.unwrap_or(0),
            file_size: 0,
            status: "initializing".to_string(),
            progress: 0.0,
            partial_path: None,
            complete_path: None,
            created_at: now.clone(),
            updated_at: now,
        };
        self.db.upsert_download(&dl).await?;

        self.spawn_add_torrent(hash.clone(), magnet_uri.to_string(), file_index);

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

        if dl.status == "complete" {
            return Ok(());
        }

        self.spawn_add_torrent(
            dl.info_hash.clone(),
            dl.magnet_uri.clone(),
            Some(dl.file_index),
        );
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
                if let Err(e) = self.session.pause(&active.handle).await {
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
                if let Err(e) = self.session.unpause(&active.handle).await {
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

    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    pub fn partial_dir(&self) -> &PathBuf {
        &self.partial_dir
    }

    pub fn complete_dir(&self) -> &PathBuf {
        &self.complete_dir
    }

    fn spawn_add_torrent(&self, info_hash: String, magnet_uri: String, file_index: Option<usize>) {
        let session = self.session.clone();
        let handles = self.handles.clone();
        let db = self.db.clone();
        let partial_dir = self.partial_dir.clone();
        let complete_dir = self.complete_dir.clone();

        tokio::spawn(async move {
            let opts = AddTorrentOptions {
                overwrite: true,
                only_files: file_index.map(|i| vec![i]),
                ..Default::default()
            };

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                session.add_torrent(AddTorrent::from_url(&magnet_uri), Some(opts)),
            )
            .await;

            let resp = match result {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    warn!(info_hash = %info_hash, "Failed to add torrent: {e}");
                    let _ = db.update_download_status(&info_hash, "error").await;
                    return;
                }
                Err(_) => {
                    warn!(info_hash = %info_hash, "Timed out adding torrent");
                    let _ = db.update_download_status(&info_hash, "error").await;
                    return;
                }
            };

            let (tid, handle) = match resp {
                AddTorrentResponse::Added(id, handle)
                | AddTorrentResponse::AlreadyManaged(id, handle) => (id, handle),
                _ => {
                    let _ = db.update_download_status(&info_hash, "error").await;
                    return;
                }
            };

            let resolved_fi = handle
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
                .unwrap_or(file_index.unwrap_or(0));

            let (title, file_name, file_size, partial_path) = handle
                .with_metadata(|meta| {
                    let name = meta.name.clone().unwrap_or_default();
                    let fi = meta.file_infos.get(resolved_fi);
                    let fname = fi
                        .map(|f| f.relative_filename.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let fsize = fi.map(|f| f.len).unwrap_or(0);
                    let pp = fi.map(|f| {
                        let rel = f.relative_filename.to_string_lossy().to_string();
                        if meta.name.is_some() {
                            partial_dir
                                .join(&name)
                                .join(&rel)
                                .to_string_lossy()
                                .to_string()
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

            handles.write().await.insert(
                info_hash.clone(),
                ActiveHandle {
                    torrent_id: tid,
                    handle: handle.clone(),
                    file_index: resolved_fi,
                },
            );

            info!(info_hash = %info_hash, title = %title, "Torrent added to session");

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

                let move_result = handle.with_metadata(|meta| {
                    let fi = match meta.file_infos.get(file_index) {
                        Some(f) => f,
                        None => return None,
                    };
                    let rel = fi.relative_filename.to_string_lossy().to_string();
                    let src = if let Some(ref name) = meta.name {
                        partial_dir.join(name).join(&rel)
                    } else {
                        partial_dir.join(&rel)
                    };
                    let dst = complete_dir.join(&rel);
                    Some((src, dst))
                });

                let (src, dst) = match move_result {
                    Ok(Some(pair)) => pair,
                    _ => {
                        let _ = db.update_download_status(&info_hash, "complete").await;
                        break;
                    }
                };

                if !src.exists() {
                    let _ = db.update_download_status(&info_hash, "complete").await;
                    break;
                }

                if let Some(parent) = dst.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                let complete_path = dst.to_string_lossy().to_string();

                if std::fs::rename(&src, &dst).is_ok() {
                    info!(info_hash = %info_hash, path = %complete_path, "File moved to complete directory");
                    let _ = db
                        .update_download_paths(&info_hash, None, Some(&complete_path))
                        .await;
                } else {
                    match std::fs::copy(&src, &dst) {
                        Ok(_) => {
                            let _ = std::fs::remove_file(&src);
                            info!(info_hash = %info_hash, path = %complete_path, "File copied to complete directory");
                            let _ = db
                                .update_download_paths(&info_hash, None, Some(&complete_path))
                                .await;
                        }
                        Err(e) => {
                            warn!(info_hash = %info_hash, "Failed to move file to complete: {e}");
                            let _ = db
                                .update_download_paths(
                                    &info_hash,
                                    Some(&src.to_string_lossy()),
                                    None,
                                )
                                .await;
                        }
                    }
                }

                let _ = db.update_download_status(&info_hash, "complete").await;
                break;
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

impl TorrentFile {
    pub fn detect_video(path: &str) -> bool {
        let lower = path.to_lowercase();
        lower.ends_with(".mp4")
            || lower.ends_with(".mkv")
            || lower.ends_with(".avi")
            || lower.ends_with(".mov")
            || lower.ends_with(".wmv")
            || lower.ends_with(".flv")
            || lower.ends_with(".webm")
            || lower.ends_with(".m4v")
            || lower.ends_with(".ts")
    }
}
