use crate::config::TranscodeConfig;
use crate::error::{Error, Result};
use bytes::Bytes;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::pipeline::{TranscodeHandle, TranscodePipeline};
use super::probe;

const DEMO_HLS_URL: &str = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";
const SEGMENT_CACHE_MAX: usize = 50;

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
    cache_dir: PathBuf,
}

pub enum PlaylistResponse {
    Content(String),
    Redirect(String),
}

impl HlsManager {
    pub async fn new(config: &TranscodeConfig, cache_dir: PathBuf) -> Result<Self> {
        let pipeline = TranscodePipeline::new(config.clone(), cache_dir.clone()).await?;
        Ok(Self {
            pipeline,
            active: Arc::new(RwLock::new(HashMap::new())),
            segment_cache: Arc::new(RwLock::new(SegmentCache::new(SEGMENT_CACHE_MAX))),
            cache_dir,
        })
    }

    pub async fn start_stream(&self, stream_id: &str, file_path: &str) -> Result<()> {
        {
            let active = self.active.read().await;
            if active.contains_key(stream_id) {
                return Ok(());
            }
        }

        let playlist_path = self.cache_dir.join(stream_id).join("playlist.m3u8");
        if playlist_path.exists() {
            let content = tokio::fs::read_to_string(&playlist_path)
                .await
                .unwrap_or_default();
            if content.matches("EXTINF:").count() > 10 {
                tracing::info!(stream_id, "Valid cached HLS found, skipping transcode");
                return Ok(());
            }
            tracing::warn!(stream_id, "Cached HLS has too few segments, re-transcoding");
            let _ = tokio::fs::remove_dir_all(self.cache_dir.join(stream_id)).await;
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
                video_codec = ?info.video_codec,
                audio_codec = ?info.audio_codec,
                hdr = ?info.hdr_format,
                "Transcoding required"
            );
            match self
                .pipeline
                .start_transcode(stream_id, file_path, &info)
                .await
            {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!(stream_id, "GPU transcode failed, falling back to CPU: {e}");
                    let cache_dir = self.cache_dir.join(stream_id);
                    let _ = tokio::fs::remove_dir_all(&cache_dir).await;
                    self.pipeline
                        .start_transcode_cpu(stream_id, file_path, &info)
                        .await?
                }
            }
        };

        self.active
            .write()
            .await
            .insert(stream_id.to_string(), handle);

        Ok(())
    }

    pub async fn generate_playlist(
        &self,
        stream_id: &str,
        _stream_ready: bool,
    ) -> Result<PlaylistResponse> {
        if stream_id == "demo" {
            return Ok(PlaylistResponse::Redirect(DEMO_HLS_URL.to_string()));
        }

        let path = self.cache_dir.join(stream_id).join("playlist.m3u8");
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Ok(PlaylistResponse::Content(content)),
            Err(_) => {
                let placeholder = [
                    "#EXTM3U",
                    "#EXT-X-VERSION:3",
                    "#EXT-X-TARGETDURATION:2",
                    "#EXT-X-MEDIA-SEQUENCE:0",
                    "#EXT-X-PLAYLIST-TYPE:EVENT",
                    "",
                ]
                .join("\n");

                Ok(PlaylistResponse::Content(placeholder))
            }
        }
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

    pub async fn cleanup(&self, stream_id: &str) -> Result<()> {
        self.active.write().await.remove(stream_id);

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
