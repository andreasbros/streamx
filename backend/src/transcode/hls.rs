use crate::config::TranscodeConfig;
use crate::error::{Error, Result};
use bytes::Bytes;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ActiveStreamInfo {
    pub stream_id: String,
    pub quality: String,
    pub status: String,
    pub cache_bytes: u64,
    pub last_activity: String,
}

use super::pipeline::{TranscodeHandle, TranscodePipeline};
use super::probe;

const DEMO_HLS_URL: &str = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";
const SEGMENT_CACHE_MAX: usize = 50;
const TRANSCODE_HISTORY_MAX: usize = 100;

#[derive(Clone)]
pub struct TranscodeHistoryEntry {
    pub stream_id: String,
    pub quality: String,
    pub status: String,
    pub cache_bytes: u64,
    pub started_at: String,
    pub finished_at: String,
}

struct SegmentCache {
    segments: VecDeque<(String, Bytes)>,
    max_size: usize,
}

impl SegmentCache {
    fn new(max_size: usize) -> Self {
        Self {
            segments: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    fn get(&self, key: &str) -> Option<Bytes> {
        self.segments
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    fn insert(&mut self, key: String, data: Bytes) {
        if self.segments.iter().any(|(k, _)| k == &key) {
            return;
        }
        if self.segments.len() >= self.max_size {
            self.segments.pop_front();
        }
        self.segments.push_back((key, data));
    }
}

pub struct HlsManager {
    pipeline: TranscodePipeline,
    active: Arc<RwLock<HashMap<String, TranscodeHandle>>>,
    segment_cache: Arc<RwLock<SegmentCache>>,
    transcode_history: Arc<std::sync::Mutex<VecDeque<TranscodeHistoryEntry>>>,
    last_access: Arc<dashmap::DashMap<String, std::time::Instant>>,
    cache_dir: PathBuf,
}

impl Drop for HlsManager {
    fn drop(&mut self) {
        // Clear the active map to trigger Drop on all TranscodeHandles,
        // which sends SIGKILL to running FFmpeg processes.
        // (The watchdog task also holds an Arc to `active`, so just dropping
        //  the manager wouldn't drop the map without this explicit clear.)
        if let Ok(mut active) = self.active.try_write() {
            let count = active.len();
            active.clear();
            if count > 0 {
                tracing::info!(count, "HlsManager dropped: killed all active transcodes");
            }
        }
    }
}

pub enum PlaylistResponse {
    Content(String),
    Redirect(String),
}

impl HlsManager {
    pub async fn new(config: &TranscodeConfig, cache_dir: PathBuf) -> Result<Self> {
        let pipeline = TranscodePipeline::new(config.clone(), cache_dir.clone()).await?;
        let manager = Self {
            pipeline,
            transcode_history: Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(
                TRANSCODE_HISTORY_MAX,
            ))),
            active: Arc::new(RwLock::new(HashMap::new())),
            segment_cache: Arc::new(RwLock::new(SegmentCache::new(SEGMENT_CACHE_MAX))),
            last_access: Arc::new(dashmap::DashMap::new()),
            cache_dir,
        };

        // Spawn watchdog to stop idle transcodes after 30s of no access
        let active = manager.active.clone();
        let last_access = manager.last_access.clone();
        let history = manager.transcode_history.clone();
        let cache_dir_wd = manager.cache_dir.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                let idle_keys: Vec<String> = {
                    let active_map = active.read().await;
                    active_map
                        .keys()
                        .filter(|key| {
                            let idle = last_access
                                .get(*key)
                                .map(|t| t.elapsed() > std::time::Duration::from_secs(30))
                                .unwrap_or(true);
                            idle
                        })
                        .cloned()
                        .collect()
                };

                for key in idle_keys {
                    tracing::info!(stream_key = %key, "Stopping idle transcode (no access for 30s)");
                    // Drop the handle -> triggers SIGTERM -> FFmpeg exits gracefully
                    let handle = active.write().await.remove(&key);
                    if let Some(h) = handle {
                        let status = h.status.borrow().clone();
                        let (sid, quality) = key.split_once('/').unwrap_or((&key, "source"));
                        let tier_dir = cache_dir_wd.join(sid).join(quality);
                        let status_str = match &status {
                            super::pipeline::TranscodeStatus::Running => "stopped",
                            super::pipeline::TranscodeStatus::Complete => "complete",
                            super::pipeline::TranscodeStatus::Failed(_) => "failed",
                        };
                        if let Ok(mut hist) = history.lock() {
                            if hist.len() >= TRANSCODE_HISTORY_MAX {
                                hist.pop_front();
                            }
                            hist.push_back(TranscodeHistoryEntry {
                                stream_id: sid.to_string(),
                                quality: quality.to_string(),
                                status: status_str.to_string(),
                                cache_bytes: tier_dir_size(&tier_dir),
                                started_at: String::new(),
                                finished_at: chrono::Utc::now()
                                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            });
                        }
                    }
                    last_access.remove(&key);
                }
            }
        });

        Ok(manager)
    }

    pub async fn start_stream(
        &self,
        stream_id: &str,
        file_path: &str,
        quality: &str,
    ) -> Result<()> {
        let active_key = format!("{stream_id}/{quality}");
        {
            let active = self.active.read().await;
            if let Some(handle) = active.get(&active_key) {
                let status = handle.status.borrow().clone();
                match status {
                    crate::transcode::pipeline::TranscodeStatus::Failed(_) => {
                        drop(active);
                        self.active.write().await.remove(&active_key);
                        tracing::info!(stream_id, quality, "Removed failed transcode, will retry");
                    }
                    _ => return Ok(()),
                }
            }
        }

        // Stop other running qualities for this stream - only one quality at a time
        {
            let prefix = format!("{stream_id}/");
            let other_keys: Vec<String> = {
                let active = self.active.read().await;
                active
                    .keys()
                    .filter(|k| k.starts_with(&prefix) && *k != &active_key)
                    .cloned()
                    .collect()
            };
            if !other_keys.is_empty() {
                let mut active = self.active.write().await;
                for key in &other_keys {
                    tracing::info!(stream_id, quality, old_key = %key, "Stopping previous quality transcode");
                    active.remove(key);
                }
            }
        }

        let stream_dir = self.cache_dir.join(stream_id);

        // Check for cached variant playlist with valid segments
        let tier_dir = stream_dir.join(quality);
        let variant_playlist = tier_dir.join("playlist.m3u8");
        if variant_playlist.exists() {
            let content = tokio::fs::read_to_string(&variant_playlist)
                .await
                .unwrap_or_default();
            let has_endlist = content.contains("EXT-X-ENDLIST");
            let seg_count = content.matches("EXTINF:").count();

            if has_endlist && seg_count > 0 {
                // Completed transcode - verify first and last segments are valid
                let segments: Vec<&str> = content
                    .lines()
                    .filter(|l| !l.starts_with('#') && !l.is_empty())
                    .collect();
                let all_valid = segments.iter().take(1).chain(segments.iter().rev().take(1)).all(|seg| {
                    let path = tier_dir.join(seg);
                    match std::fs::read(&path) {
                        Ok(data) => is_valid_fmp4(&data),
                        Err(_) => false,
                    }
                });
                if all_valid {
                    tracing::info!(stream_id, quality, seg_count, "Valid completed cache found");
                    return Ok(());
                }
                // Cache has corrupt segments - delete and re-transcode
                tracing::warn!(stream_id, quality, "Cached segments corrupt, re-transcoding");
                let _ = tokio::fs::remove_dir_all(&tier_dir).await;
                let _ = tokio::fs::create_dir_all(&tier_dir).await;
            } else if !has_endlist && seg_count > 10 {
                // Incomplete transcode with enough segments to start playback
                tracing::info!(stream_id, quality, seg_count, "Partial cache found, skipping");
                return Ok(());
            }
        }

        // Check for passthrough (flat playlist.m3u8)
        let flat_playlist = stream_dir.join("playlist.m3u8");
        if flat_playlist.exists() {
            let content = tokio::fs::read_to_string(&flat_playlist)
                .await
                .unwrap_or_default();
            if content.matches("EXTINF:").count() > 10 {
                tracing::info!(stream_id, "Valid cached passthrough found, skipping");
                return Ok(());
            }
        }

        let info = probe::probe(file_path).await?;

        let handle = if probe::is_browser_compatible(&info) {
            tracing::info!(stream_id, "Source is browser-compatible, using passthrough");
            self.pipeline
                .start_passthrough(stream_id, file_path)
                .await?
        } else {
            tracing::info!(
                stream_id,
                quality,
                video_codec = ?info.video_codec,
                audio_codec = ?info.audio_codec,
                "Transcoding at requested quality"
            );
            if self.pipeline.gpu_enabled() {
                match self
                    .pipeline
                    .start_transcode(stream_id, file_path, &info, quality)
                    .await
                {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::warn!(stream_id, "GPU transcode failed, falling back to CPU: {e}");
                        let qdir = self.cache_dir.join(stream_id).join(quality);
                        let _ = tokio::fs::remove_dir_all(&qdir).await;
                        self.pipeline
                            .start_transcode_cpu(stream_id, file_path, &info, quality)
                            .await?
                    }
                }
            } else {
                self.pipeline
                    .start_transcode_cpu(stream_id, file_path, &info, quality)
                    .await?
            }
        };

        self.active
            .write()
            .await
            .insert(active_key, handle);

        Ok(())
    }

    /// Start HLS transcoding from an async reader (torrent stream).
    /// `file_path` is used only for probing (the beginning is on disk for sequential downloads).
    /// The actual data is read from `reader` via pipe to FFmpeg stdin.
    pub async fn start_stream_piped<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
        &self,
        stream_id: &str,
        file_path: &str,
        reader: R,
        quality: &str,
    ) -> Result<()> {
        let active_key = format!("{stream_id}/{quality}");
        {
            let active = self.active.read().await;
            if let Some(handle) = active.get(&active_key) {
                let status = handle.status.borrow().clone();
                match status {
                    crate::transcode::pipeline::TranscodeStatus::Failed(_) => {
                        drop(active);
                        self.active.write().await.remove(&active_key);
                        tracing::info!(stream_id, quality, "Removed failed piped transcode, will retry");
                    }
                    _ => return Ok(()),
                }
            }
        }

        let stream_dir = self.cache_dir.join(stream_id);

        // Check cached variant
        let variant_playlist = stream_dir.join(quality).join("playlist.m3u8");
        if variant_playlist.exists() {
            let content = tokio::fs::read_to_string(&variant_playlist)
                .await
                .unwrap_or_default();
            if content.matches("EXTINF:").count() > 10 {
                tracing::info!(stream_id, quality, "Valid cached quality found, skipping piped");
                return Ok(());
            }
        }

        // Check passthrough
        let flat_playlist = stream_dir.join("playlist.m3u8");
        if flat_playlist.exists() {
            let content = tokio::fs::read_to_string(&flat_playlist)
                .await
                .unwrap_or_default();
            if content.matches("EXTINF:").count() > 10 {
                tracing::info!(stream_id, "Valid cached passthrough found, skipping piped");
                return Ok(());
            }
        }

        let info = probe::probe(file_path).await?;

        let handle = if probe::is_browser_compatible(&info) {
            tracing::info!(stream_id, "Source is browser-compatible, piped passthrough");
            self.pipeline
                .start_passthrough_piped(stream_id, reader)
                .await?
        } else {
            tracing::info!(
                stream_id,
                quality,
                video_codec = ?info.video_codec,
                audio_codec = ?info.audio_codec,
                "Piped transcode at requested quality"
            );
            self.pipeline
                .start_transcode_piped(stream_id, reader, &info, quality)
                .await?
        };

        self.active
            .write()
            .await
            .insert(active_key, handle);

        Ok(())
    }

    /// Start HLS transcoding from a remote HTTPS URL.
    /// FFmpeg reads the URL directly as input.
    pub async fn start_stream_url(
        &self,
        stream_id: &str,
        url: &str,
        quality: &str,
    ) -> Result<()> {
        let active_key = format!("{stream_id}/{quality}");
        {
            let active = self.active.read().await;
            if let Some(handle) = active.get(&active_key) {
                let status = handle.status.borrow().clone();
                match status {
                    crate::transcode::pipeline::TranscodeStatus::Failed(_) => {
                        drop(active);
                        self.active.write().await.remove(&active_key);
                    }
                    _ => return Ok(()),
                }
            }
        }

        let stream_dir = self.cache_dir.join(stream_id);
        let variant_playlist = stream_dir.join(quality).join("playlist.m3u8");
        if variant_playlist.exists() {
            let content = tokio::fs::read_to_string(&variant_playlist)
                .await
                .unwrap_or_default();
            if content.matches("EXTINF:").count() > 10 {
                tracing::info!(stream_id, quality, "Valid cached URL transcode found");
                return Ok(());
            }
        }

        // Probe the URL
        let info = probe::probe(url).await?;

        let handle = if probe::is_browser_compatible(&info) {
            tracing::info!(stream_id, "URL source is browser-compatible, passthrough");
            self.pipeline.start_passthrough(stream_id, url).await?
        } else {
            tracing::info!(
                stream_id,
                quality,
                video_codec = ?info.video_codec,
                "Transcoding URL source"
            );
            if self.pipeline.gpu_enabled() {
                match self
                    .pipeline
                    .start_transcode(stream_id, url, &info, quality)
                    .await
                {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::warn!(stream_id, "GPU transcode failed for URL, CPU fallback: {e}");
                        self.pipeline
                            .start_transcode_cpu(stream_id, url, &info, quality)
                            .await?
                    }
                }
            } else {
                self.pipeline
                    .start_transcode_cpu(stream_id, url, &info, quality)
                    .await?
            }
        };

        self.active.write().await.insert(active_key, handle);
        Ok(())
    }

    pub async fn generate_playlist(
        &self,
        stream_id: &str,
        quality: &str,
    ) -> Result<PlaylistResponse> {
        if stream_id == "demo" {
            return Ok(PlaylistResponse::Redirect(DEMO_HLS_URL.to_string()));
        }

        // Touch last access for watchdog
        let access_key = format!("{stream_id}/{quality}");
        self.last_access
            .insert(access_key, std::time::Instant::now());

        let stream_dir = self.cache_dir.join(stream_id);

        // For passthrough (browser-compatible), serve flat playlist
        let flat_path = stream_dir.join("playlist.m3u8");
        if flat_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&flat_path).await {
                return Ok(PlaylistResponse::Content(content));
            }
        }

        // Serve the variant playlist, rewriting segment paths to include quality prefix
        let variant_path = stream_dir.join(quality).join("playlist.m3u8");
        if let Ok(content) = tokio::fs::read_to_string(&variant_path).await {
            let has_endlist = content.contains("EXT-X-ENDLIST");
            let rewritten = content
                .lines()
                .map(|line| {
                    if !line.starts_with('#') && !line.is_empty() {
                        // Prefix segment filenames with quality dir
                        format!("{quality}/{line}")
                    } else if line.contains("EXT-X-MAP:URI=") {
                        // Rewrite init segment URI (fMP4 only, MPEG-TS doesn't have this)
                        line.replace("URI=\"", &format!("URI=\"{quality}/"))
                    } else if line.starts_with("#EXT-X-MEDIA-SEQUENCE") && !has_endlist {
                        // Growing playlist (transcode in progress): add EVENT type
                        // so the player starts from the beginning, not the live edge
                        format!("{line}\n#EXT-X-PLAYLIST-TYPE:EVENT")
                    } else if line == "#EXT-X-DISCONTINUITY" {
                        // Remove spurious discontinuity tag added by FFmpeg append_list.
                        // There's no actual discontinuity in a continuous transcode and
                        // Safari starts from the wrong position when it sees this.
                        String::new()
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(PlaylistResponse::Content(rewritten));
        }

        let placeholder = [
            "#EXTM3U",
            "#EXT-X-VERSION:3",
            "#EXT-X-TARGETDURATION:2",
            "#EXT-X-MEDIA-SEQUENCE:0",
            "",
        ]
        .join("\n");

        Ok(PlaylistResponse::Content(placeholder))
    }

    pub async fn get_segment(&self, stream_id: &str, segment_name: &str) -> Result<Option<Bytes>> {
        if segment_name.contains("..") || segment_name.contains('/') || segment_name.contains('\\')
        {
            return Err(Error::BadRequest {
                message: "Invalid segment name".to_string(),
            });
        }

        let cache_key = format!("{stream_id}/{segment_name}");

        {
            let cache = self.segment_cache.read().await;
            if let Some(data) = cache.get(&cache_key) {
                return Ok(Some(data));
            }
        }

        let path = self.cache_dir.join(stream_id).join(segment_name);
        match tokio::fs::read(&path).await {
            Ok(data) => {
                if !is_valid_segment(&data, segment_name) {
                    tracing::warn!(stream_id, segment_name, "Corrupt segment detected, deleting");
                    let _ = tokio::fs::remove_file(&path).await;
                    return Ok(None);
                }
                let bytes = Bytes::from(data);
                let mut cache = self.segment_cache.write().await;
                cache.insert(cache_key, bytes.clone());
                Ok(Some(bytes))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Transcode {
                message: format!("Failed to read segment: {e}"),
            }),
        }
    }

    pub async fn get_variant_segment(
        &self,
        stream_id: &str,
        variant: &str,
        segment_name: &str,
    ) -> Result<Option<Bytes>> {
        // Touch last access for watchdog
        let access_key = format!("{stream_id}/{variant}");
        self.last_access
            .insert(access_key, std::time::Instant::now());

        if !variant.chars().all(|c| c.is_alphanumeric()) {
            return Err(Error::BadRequest {
                message: "Invalid variant name".to_string(),
            });
        }
        if segment_name.contains("..") || segment_name.contains('/') || segment_name.contains('\\')
        {
            return Err(Error::BadRequest {
                message: "Invalid segment name".to_string(),
            });
        }

        let cache_key = format!("{stream_id}/{variant}/{segment_name}");

        {
            let cache = self.segment_cache.read().await;
            if let Some(data) = cache.get(&cache_key) {
                return Ok(Some(data));
            }
        }

        let path = self
            .cache_dir
            .join(stream_id)
            .join(variant)
            .join(segment_name);
        match tokio::fs::read(&path).await {
            Ok(data) => {
                if !is_valid_segment(&data, segment_name) {
                    tracing::warn!(stream_id, variant, segment_name, "Corrupt segment detected, deleting");
                    let _ = tokio::fs::remove_file(&path).await;
                    return Ok(None);
                }
                let bytes = Bytes::from(data);
                let mut cache = self.segment_cache.write().await;
                cache.insert(cache_key, bytes.clone());
                Ok(Some(bytes))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Transcode {
                message: format!("Failed to read variant segment: {e}"),
            }),
        }
    }

    pub async fn get_variant_playlist(
        &self,
        stream_id: &str,
        variant: &str,
    ) -> Result<Option<String>> {
        if !variant.chars().all(|c| c.is_alphanumeric()) {
            return Err(Error::BadRequest {
                message: "Invalid variant name".to_string(),
            });
        }

        let path = self
            .cache_dir
            .join(stream_id)
            .join(variant)
            .join("playlist.m3u8");
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Transcode {
                message: format!("Failed to read variant playlist: {e}"),
            }),
        }
    }

    pub async fn active_streams(&self) -> Vec<ActiveStreamInfo> {
        let active = self.active.read().await;
        let cache_dir = self.cache_dir.clone();

        // Move completed/failed to history
        let mut finished_keys = Vec::new();
        for (key, handle) in active.iter() {
            let status = handle.status.borrow().clone();
            match status {
                crate::transcode::pipeline::TranscodeStatus::Complete
                | crate::transcode::pipeline::TranscodeStatus::Failed(_) => {
                    let (sid, quality) = key.split_once('/').unwrap_or((key, "source"));
                    let tier_dir = cache_dir.join(sid).join(quality);
                    let status_str = match &status {
                        crate::transcode::pipeline::TranscodeStatus::Complete => "complete",
                        _ => "failed",
                    };
                    let entry = TranscodeHistoryEntry {
                        stream_id: sid.to_string(),
                        quality: quality.to_string(),
                        status: status_str.to_string(),
                        cache_bytes: tier_dir_size(&tier_dir),
                        started_at: String::new(),
                        finished_at: chrono::Utc::now()
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    };
                    if let Ok(mut history) = self.transcode_history.lock() {
                        if history.len() >= TRANSCODE_HISTORY_MAX {
                            history.pop_front();
                        }
                        history.push_back(entry);
                    }
                    finished_keys.push(key.clone());
                }
                _ => {}
            }
        }
        drop(active);

        // Remove finished from active
        if !finished_keys.is_empty() {
            let mut active = self.active.write().await;
            for key in &finished_keys {
                active.remove(key);
            }
        }

        // Build list: active first, then history (newest first)
        let active = self.active.read().await;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut result: Vec<ActiveStreamInfo> = active
            .iter()
            .map(|(key, _handle)| {
                let (sid, quality) = key.split_once('/').unwrap_or((key, "source"));
                let tier_dir = cache_dir.join(sid).join(quality);
                ActiveStreamInfo {
                    stream_id: sid.to_string(),
                    quality: quality.to_string(),
                    status: "running".to_string(),
                    cache_bytes: tier_dir_size(&tier_dir),
                    last_activity: now.clone(),
                }
            })
            .collect();

        if let Ok(history) = self.transcode_history.lock() {
            for entry in history.iter().rev() {
                result.push(ActiveStreamInfo {
                    stream_id: entry.stream_id.clone(),
                    quality: entry.quality.clone(),
                    status: entry.status.clone(),
                    cache_bytes: entry.cache_bytes,
                    last_activity: entry.finished_at.clone(),
                });
            }
        }

        // Scan cache directory for existing transcodes not in active/history
        let mut seen: std::collections::HashSet<String> = result
            .iter()
            .map(|s| format!("{}/{}", s.stream_id, s.quality))
            .collect();

        if let Ok(entries) = std::fs::read_dir(&cache_dir) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let stream_id = entry.file_name().to_string_lossy().to_string();
                if let Ok(tier_entries) = std::fs::read_dir(entry.path()) {
                    for tier in tier_entries.flatten() {
                        if !tier.path().is_dir() {
                            continue;
                        }
                        let quality = tier.file_name().to_string_lossy().to_string();
                        let playlist = tier.path().join("playlist.m3u8");
                        if !playlist.exists() {
                            continue;
                        }
                        let key = format!("{stream_id}/{quality}");
                        if seen.contains(&key) {
                            continue;
                        }
                        seen.insert(key);
                        let mtime = std::fs::metadata(&playlist)
                            .ok()
                            .and_then(|m| m.modified().ok())
                            .map(|t| {
                                let dt: chrono::DateTime<chrono::Utc> = t.into();
                                dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                            })
                            .unwrap_or_default();
                        result.push(ActiveStreamInfo {
                            stream_id: stream_id.clone(),
                            quality,
                            status: "cached".to_string(),
                            cache_bytes: tier_dir_size(&tier.path()),
                            last_activity: mtime,
                        });
                    }
                }
            }
        }

        // Sort by last_activity descending (running first since they have "now" timestamp)
        result.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        result
    }

    pub async fn cleanup(&self, stream_id: &str) -> Result<()> {
        // Remove all active handles for this stream (keys are {stream_id}/{quality})
        let prefix = format!("{stream_id}/");
        let keys: Vec<String> = {
            let active = self.active.read().await;
            active.keys().filter(|k| k.starts_with(&prefix) || *k == stream_id).cloned().collect()
        };
        if !keys.is_empty() {
            let mut active = self.active.write().await;
            for key in &keys {
                active.remove(key);
            }
        }

        let dir = self.cache_dir.join(stream_id);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir)
                .await
                .map_err(|e| Error::Transcode {
                    message: format!("Failed to cleanup cache for {stream_id}: {e}"),
                })?;
        }

        Ok(())
    }
}

/// Validate segment integrity based on format.
/// fMP4 (.m4s/.mp4): walk ISO BMFF boxes and verify declared sizes match file size
/// MPEG-TS (.ts): check for 0x47 sync bytes at 188-byte boundaries
fn is_valid_segment(data: &[u8], name: &str) -> bool {
    if data.len() < 8 {
        return false;
    }
    if name.ends_with(".m4s") || name.ends_with(".mp4") {
        is_valid_fmp4(data)
    } else if name.ends_with(".ts") {
        if data[0] != 0x47 {
            return false;
        }
        let pkt_size = 188;
        let check_offsets = [
            pkt_size,
            pkt_size * 2,
            (data.len() / pkt_size / 2) * pkt_size,
        ];
        for offset in check_offsets {
            if offset < data.len() && data[offset] != 0x47 {
                return false;
            }
        }
        true
    } else {
        true
    }
}

/// Walk ISO BMFF box structure to verify the file isn't truncated.
/// Each box: 4 bytes size (big-endian) + 4 bytes type. Boxes must tile
/// the entire file with no gaps. A truncated file will have a box whose
/// declared size extends past EOF.
fn is_valid_fmp4(data: &[u8]) -> bool {
    let len = data.len();
    let mut offset = 0usize;
    let mut found_mdat = false;

    while offset + 8 <= len {
        let box_size = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let box_type = &data[offset + 4..offset + 8];

        // box_size == 0 means "rest of file" (only valid for last box)
        if box_size == 0 {
            return true;
        }
        // box_size == 1 means 64-bit extended size (next 8 bytes)
        if box_size == 1 {
            if offset + 16 > len {
                return false;
            }
            let ext_size = u64::from_be_bytes([
                data[offset + 8], data[offset + 9],
                data[offset + 10], data[offset + 11],
                data[offset + 12], data[offset + 13],
                data[offset + 14], data[offset + 15],
            ]) as usize;
            if offset + ext_size > len {
                return false; // truncated
            }
            offset += ext_size;
            continue;
        }
        if box_size < 8 {
            return false; // invalid
        }
        if offset + box_size > len {
            return false; // truncated - declared size exceeds file
        }
        if box_type == b"mdat" {
            found_mdat = true;
        }
        offset += box_size;
    }

    // Media segments must have mdat; init segments have moov
    offset == len && (found_mdat || data.len() < 10000)
}

fn tier_dir_size(path: &std::path::Path) -> u64 {
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}
