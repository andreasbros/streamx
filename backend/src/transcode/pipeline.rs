use crate::config::TranscodeConfig;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncRead;
use tokio::process::Command;
use tokio::sync::{watch, Semaphore};

use super::gpu::{self, HwAccel};
use super::probe::MediaInfo;

pub struct TranscodePipeline {
    semaphore: Arc<Semaphore>,
    hw_accel: HwAccel,
    config: TranscodeConfig,
    cache_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub enum TranscodeStatus {
    Running,
    Complete,
    Failed(String),
}

pub struct TranscodeHandle {
    pub stream_id: String,
    pub output_dir: PathBuf,
    pub master_playlist_path: PathBuf,
    pub status: watch::Receiver<TranscodeStatus>,
    /// PIDs of FFmpeg child processes - killed on drop
    child_pids: Arc<std::sync::Mutex<Vec<u32>>>,
}

impl Drop for TranscodeHandle {
    fn drop(&mut self) {
        if let Ok(pids) = self.child_pids.lock() {
            if pids.is_empty() {
                return;
            }
            // SIGTERM lets FFmpeg finalize the current segment and write EXT-X-ENDLIST
            for pid in pids.iter() {
                tracing::info!(stream_id = %self.stream_id, pid, "Stopping FFmpeg (SIGTERM)");
                unsafe { libc::kill(*pid as i32, libc::SIGTERM); }
            }
            // Wait up to 3 seconds for graceful exit
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let all_dead = pids.iter().all(|pid| unsafe { libc::kill(*pid as i32, 0) } != 0);
                if all_dead {
                    return;
                }
            }
            // Still alive after 3s - force kill
            for pid in pids.iter() {
                let alive = unsafe { libc::kill(*pid as i32, 0) } == 0;
                if alive {
                    tracing::warn!(stream_id = %self.stream_id, pid, "FFmpeg did not exit in 3s, SIGKILL");
                    unsafe { libc::kill(*pid as i32, libc::SIGKILL); }
                }
            }
        }
    }
}

struct QualityTier {
    label: &'static str,
    height: Option<u32>,
    video_bitrate: &'static str,
    audio_bitrate: &'static str,
}

const QUALITY_TIERS: &[QualityTier] = &[
    QualityTier {
        label: "360p",
        height: Some(360),
        video_bitrate: "800k",
        audio_bitrate: "128k",
    },
    QualityTier {
        label: "720p",
        height: Some(720),
        video_bitrate: "2500k",
        audio_bitrate: "192k",
    },
    QualityTier {
        label: "1080p",
        height: Some(1080),
        video_bitrate: "5000k",
        audio_bitrate: "256k",
    },
    QualityTier {
        label: "source",
        height: None,
        video_bitrate: "8000k",
        audio_bitrate: "320k",
    },
];

fn select_tier(label: &str) -> &'static QualityTier {
    QUALITY_TIERS
        .iter()
        .find(|t| t.label == label)
        .unwrap_or(&QUALITY_TIERS[QUALITY_TIERS.len() - 1])
}

/// Available quality labels for a given source height.
pub fn available_qualities(source_height: u32) -> Vec<&'static str> {
    let mut labels: Vec<&str> = QUALITY_TIERS
        .iter()
        .filter(|t| match t.height {
            Some(h) => h < source_height,
            None => true,
        })
        .map(|t| t.label)
        .collect();
    if labels.is_empty() {
        labels.push("source");
    }
    labels
}

fn generate_master_playlist(tiers: &[&QualityTier], source_height: u32) -> String {
    let mut lines = vec!["#EXTM3U".to_string()];

    for tier in tiers {
        let height = tier.height.unwrap_or(source_height);
        let width = (height as f64 * 16.0 / 9.0).round() as u32;
        let width = width + (width % 2);
        let bandwidth = parse_bitrate(tier.video_bitrate) + parse_bitrate(tier.audio_bitrate);

        lines.push(format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={bandwidth},RESOLUTION={width}x{height},NAME=\"{label}\"",
            label = tier.label
        ));
        lines.push(format!("{}/playlist.m3u8", tier.label));
    }

    lines.push(String::new());
    lines.join("\n")
}

fn apply_audio_args(cmd: &mut Command, media_info: &MediaInfo, audio_bitrate: &str, force_stereo: bool) {
    if media_info.needs_audio_transcode {
        cmd.arg("-c:a").arg("aac");
        cmd.arg("-b:a").arg(audio_bitrate);
        if force_stereo && media_info.audio_channels.map(|c| c > 2).unwrap_or(false) {
            // Downmix surround to stereo (all channels folded into L/R)
            cmd.arg("-ac").arg("2");
        }
    } else {
        cmd.arg("-c:a").arg("copy");
    }
}

impl TranscodePipeline {
    pub async fn new(config: TranscodeConfig, cache_dir: PathBuf) -> Result<Self> {
        let hw = gpu::detect_hardware().await;
        tracing::info!(?hw, "Detected hardware acceleration");

        Ok(Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_transcodes as usize)),
            hw_accel: hw,
            config,
            cache_dir,
        })
    }

    pub fn gpu_enabled(&self) -> bool {
        self.config.gpu && self.hw_accel != HwAccel::None
    }

    pub async fn start_transcode(
        &self,
        stream_id: &str,
        input_path: &str,
        media_info: &MediaInfo,
        quality: &str,
    ) -> Result<TranscodeHandle> {
        let permit =
            self.semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| Error::Transcode {
                    message: "Transcode semaphore closed".to_string(),
                })?;

        let output_dir = self.cache_dir.join(stream_id);
        tokio::fs::create_dir_all(&output_dir)
            .await
            .map_err(|e| Error::Transcode {
                message: format!("Failed to create output directory: {e}"),
            })?;

        let source_height = media_info.height.unwrap_or(1080);
        let tier = select_tier(quality);
        let tiers = vec![tier];

        for tier in &tiers {
            tokio::fs::create_dir_all(output_dir.join(tier.label))
                .await
                .map_err(|e| Error::Transcode {
                    message: format!("Failed to create tier directory {}: {e}", tier.label),
                })?;
        }

        let master_content = generate_master_playlist(&tiers, source_height);
        let master_path = output_dir.join("master.m3u8");
        tokio::fs::write(&master_path, &master_content)
            .await
            .map_err(|e| Error::Transcode {
                message: format!("Failed to write master playlist: {e}"),
            })?;

        tracing::info!(
            stream_id,
            tiers = tiers.iter().map(|t| t.label).collect::<Vec<_>>().join(","),
            "Starting multi-variant transcode (GPU)"
        );

        let (agg_tx, agg_rx) = watch::channel(TranscodeStatus::Running);
        let tier_count = tiers.len();
        let (results_tx, mut results_rx) =
            tokio::sync::mpsc::channel::<std::result::Result<(), String>>(tier_count);
        let child_pids: Arc<std::sync::Mutex<Vec<u32>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

        for tier in &tiers {
            let mut cmd = match self.build_variant_command_gpu(input_path, media_info, tier, &output_dir) {
                Ok(c) => c,
                Err(msg) => {
                    return Err(Error::Transcode { message: msg });
                }
            };

            let mut child = cmd.spawn().map_err(|e| Error::Transcode {
                message: format!("Failed to spawn ffmpeg (GPU): {e}"),
            })?;
            if let Some(pid) = child.id() {
                if let Ok(mut pids) = child_pids.lock() { pids.push(pid); }
            }

            // Wait briefly to catch immediate GPU failures (e.g. "No usable encoding profile")
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            match child.try_wait() {
                Ok(Some(exit_status)) if !exit_status.success() => {
                    let stderr_msg = if let Some(mut stderr) = child.stderr.take() {
                        let mut buf = Vec::new();
                        let _ = tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut buf).await;
                        String::from_utf8_lossy(&buf).to_string()
                    } else {
                        String::new()
                    };
                    drop(permit);
                    return Err(Error::Transcode {
                        message: format!("GPU transcode failed immediately: {stderr_msg}"),
                    });
                }
                _ => {}
            }

            let sid = stream_id.to_string();
            let label = tier.label.to_string();
            let tx = results_tx.clone();

            tokio::spawn(async move {
                let result = monitor_transcode(child, &sid).await;
                if let Err(ref msg) = result {
                    tracing::error!(stream_id = %sid, tier = %label, %msg, "Variant transcode failed");
                } else {
                    tracing::info!(stream_id = %sid, tier = %label, "Variant transcode complete");
                }
                let _ = tx.send(result).await;
            });
        }

        drop(results_tx);

        tokio::spawn(async move {
            let mut all_ok = true;
            let mut first_err = String::new();
            while let Some(result) = results_rx.recv().await {
                if let Err(msg) = result {
                    all_ok = false;
                    if first_err.is_empty() {
                        first_err = msg;
                    }
                }
            }
            let status = if all_ok {
                TranscodeStatus::Complete
            } else {
                TranscodeStatus::Failed(first_err)
            };
            let _ = agg_tx.send(status);
            drop(permit);
        });

        Ok(TranscodeHandle {
            stream_id: stream_id.to_string(),
            output_dir,
            master_playlist_path: master_path,
            status: agg_rx,
            child_pids: child_pids.clone(),
        })
    }

    pub async fn start_transcode_cpu(
        &self,
        stream_id: &str,
        input_path: &str,
        media_info: &MediaInfo,
        quality: &str,
    ) -> Result<TranscodeHandle> {
        let permit =
            self.semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| Error::Transcode {
                    message: "Transcode semaphore closed".to_string(),
                })?;

        let output_dir = self.cache_dir.join(stream_id);
        tokio::fs::create_dir_all(&output_dir)
            .await
            .map_err(|e| Error::Transcode {
                message: format!("Failed to create output directory: {e}"),
            })?;

        let source_height = media_info.height.unwrap_or(1080);
        let tier = select_tier(quality);
        let tiers = vec![tier];

        for tier in &tiers {
            tokio::fs::create_dir_all(output_dir.join(tier.label))
                .await
                .map_err(|e| Error::Transcode {
                    message: format!("Failed to create tier directory {}: {e}", tier.label),
                })?;
        }

        let master_content = generate_master_playlist(&tiers, source_height);
        let master_path = output_dir.join("master.m3u8");
        tokio::fs::write(&master_path, &master_content)
            .await
            .map_err(|e| Error::Transcode {
                message: format!("Failed to write master playlist: {e}"),
            })?;

        tracing::info!(
            stream_id,
            tiers = tiers.iter().map(|t| t.label).collect::<Vec<_>>().join(","),
            "Starting multi-variant transcode (CPU)"
        );

        let (agg_tx, agg_rx) = watch::channel(TranscodeStatus::Running);
        let tier_count = tiers.len();
        let (results_tx, mut results_rx) =
            tokio::sync::mpsc::channel::<std::result::Result<(), String>>(tier_count);
        let child_pids: Arc<std::sync::Mutex<Vec<u32>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

        for tier in &tiers {
            let mut cmd = self.build_variant_command_cpu(input_path, media_info, tier, &output_dir);
            let sid = stream_id.to_string();
            let label = tier.label.to_string();
            let tx = results_tx.clone();
            let pids = child_pids.clone();

            tokio::spawn(async move {
                let child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx
                            .send(Err(format!(
                                "Failed to spawn ffmpeg (CPU) for {label}: {e}"
                            )))
                            .await;
                        return;
                    }
                };
                if let Some(pid) = child.id() {
                    if let Ok(mut p) = pids.lock() { p.push(pid); }
                }
                let result = monitor_transcode(child, &sid).await;
                if let Err(ref msg) = result {
                    tracing::error!(stream_id = %sid, tier = %label, %msg, "Variant transcode failed");
                } else {
                    tracing::info!(stream_id = %sid, tier = %label, "Variant transcode complete");
                }
                let _ = tx.send(result).await;
            });
        }

        drop(results_tx);

        tokio::spawn(async move {
            let mut all_ok = true;
            let mut first_err = String::new();
            while let Some(result) = results_rx.recv().await {
                if let Err(msg) = result {
                    all_ok = false;
                    if first_err.is_empty() {
                        first_err = msg;
                    }
                }
            }
            let status = if all_ok {
                TranscodeStatus::Complete
            } else {
                TranscodeStatus::Failed(first_err)
            };
            let _ = agg_tx.send(status);
            drop(permit);
        });

        Ok(TranscodeHandle {
            stream_id: stream_id.to_string(),
            output_dir,
            master_playlist_path: master_path,
            status: agg_rx,
            child_pids: child_pids.clone(),
        })
    }

    pub async fn start_passthrough(
        &self,
        stream_id: &str,
        input_path: &str,
    ) -> Result<TranscodeHandle> {
        let permit =
            self.semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| Error::Transcode {
                    message: "Transcode semaphore closed".to_string(),
                })?;

        let output_dir = self.cache_dir.join(stream_id);
        tokio::fs::create_dir_all(&output_dir)
            .await
            .map_err(|e| Error::Transcode {
                message: format!("Failed to create output directory: {e}"),
            })?;

        let playlist_path = output_dir.join("playlist.m3u8");
        let segment_pattern = output_dir.join("segment_%04d.ts");

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("warning")
            .arg("-probesize")
            .arg("5000000")
            .arg("-analyzeduration")
            .arg("3000000")
            .arg("-fflags")
            .arg("+genpts+igndts+discardcorrupt")
            .arg("-i")
            .arg(input_path)
            .arg("-c")
            .arg("copy")
            .arg("-avoid_negative_ts")
            .arg("make_zero")
            .arg("-max_muxing_queue_size")
            .arg("4096")
            .arg("-f")
            .arg("hls")
            .arg("-hls_time")
            .arg("2")
            .arg("-hls_init_time")
            .arg("1")
            .arg("-hls_list_size")
            .arg("0")
            .arg("-hls_segment_type")
            .arg("mpegts")
            .arg("-hls_flags")
            .arg("independent_segments+append_list")
            .arg("-movflags")
            .arg("+faststart")
            .arg("-hls_segment_filename")
            .arg(&segment_pattern)
            .arg(&playlist_path);

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = cmd.spawn().map_err(|e| Error::Transcode {
            message: format!("Failed to spawn ffmpeg for passthrough: {e}"),
        })?;
        let child_pids: Arc<std::sync::Mutex<Vec<u32>>> = Arc::new(std::sync::Mutex::new(
            child.id().into_iter().collect()
        ));

        let (status_tx, status_rx) = watch::channel(TranscodeStatus::Running);
        let sid = stream_id.to_string();

        tokio::spawn(async move {
            let result = monitor_transcode(child, &sid).await;
            let status = match result {
                Ok(()) => TranscodeStatus::Complete,
                Err(msg) => TranscodeStatus::Failed(msg),
            };
            let _ = status_tx.send(status);
            drop(permit);
        });

        Ok(TranscodeHandle {
            stream_id: stream_id.to_string(),
            output_dir,
            master_playlist_path: playlist_path,
            status: status_rx,
            child_pids: child_pids.clone(),
        })
    }

    /// Transcode from an async reader (e.g. torrent stream) piped into FFmpeg stdin.
    /// Writes the stream to a temp file, then spawns multi-variant transcodes from it.
    pub async fn start_transcode_piped<R: AsyncRead + Unpin + Send + 'static>(
        &self,
        stream_id: &str,
        reader: R,
        media_info: &MediaInfo,
        quality: &str,
    ) -> Result<TranscodeHandle> {
        let permit =
            self.semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| Error::Transcode {
                    message: "Transcode semaphore closed".to_string(),
                })?;

        let output_dir = self.cache_dir.join(stream_id);
        tokio::fs::create_dir_all(&output_dir)
            .await
            .map_err(|e| Error::Transcode {
                message: format!("Failed to create output directory: {e}"),
            })?;

        let input_tmp = output_dir.join("input.tmp");

        let source_height = media_info.height.unwrap_or(1080);
        let tier = select_tier(quality);
        let tiers = vec![tier];

        for tier in &tiers {
            tokio::fs::create_dir_all(output_dir.join(tier.label))
                .await
                .map_err(|e| Error::Transcode {
                    message: format!("Failed to create tier directory {}: {e}", tier.label),
                })?;
        }

        let master_content = generate_master_playlist(&tiers, source_height);
        let master_path = output_dir.join("master.m3u8");
        tokio::fs::write(&master_path, &master_content)
            .await
            .map_err(|e| Error::Transcode {
                message: format!("Failed to write master playlist: {e}"),
            })?;

        tracing::info!(
            stream_id,
            tiers = tiers.iter().map(|t| t.label).collect::<Vec<_>>().join(","),
            "Starting piped multi-variant transcode"
        );

        // Write the stream to a temp file; FFmpeg reads from it and blocks on EOF
        let tmp_path = input_tmp.clone();
        let sid_pipe = stream_id.to_string();
        tokio::spawn(async move {
            let mut reader = reader;
            match tokio::fs::File::create(&tmp_path).await {
                Ok(mut file) => match tokio::io::copy(&mut reader, &mut file).await {
                    Ok(bytes) => {
                        tracing::info!(stream_id = %sid_pipe, bytes, "Finished writing piped input to temp file");
                    }
                    Err(e) => {
                        tracing::warn!(stream_id = %sid_pipe, "Error writing piped input: {e}");
                    }
                },
                Err(e) => {
                    tracing::error!(stream_id = %sid_pipe, "Failed to create temp file: {e}");
                }
            }
        });

        // Wait for initial data before starting transcodes
        let tmp_check = input_tmp.clone();
        let sid_wait = stream_id.to_string();
        let mut waited = 0u32;
        loop {
            match tokio::fs::metadata(&tmp_check).await {
                Ok(meta) if meta.len() >= 1_048_576 => break,
                _ => {
                    if waited > 300 {
                        return Err(Error::Transcode {
                            message: format!(
                                "Timed out waiting for piped input data for {sid_wait}"
                            ),
                        });
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    waited += 1;
                }
            }
        }

        let input_path_str = input_tmp.to_string_lossy().to_string();

        let (agg_tx, agg_rx) = watch::channel(TranscodeStatus::Running);
        let tier_count = tiers.len();
        let (results_tx, mut results_rx) =
            tokio::sync::mpsc::channel::<std::result::Result<(), String>>(tier_count);
        let child_pids: Arc<std::sync::Mutex<Vec<u32>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

        for tier in &tiers {
            let mut cmd =
                self.build_variant_command_cpu(&input_path_str, media_info, tier, &output_dir);
            let sid = stream_id.to_string();
            let label = tier.label.to_string();
            let tx = results_tx.clone();
            let pids = child_pids.clone();

            tokio::spawn(async move {
                let child = match cmd.spawn() {
                    Ok(c) => {
                        if let Some(pid) = c.id() {
                            if let Ok(mut p) = pids.lock() { p.push(pid); }
                        }
                        c
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Err(format!(
                                "Failed to spawn ffmpeg (piped) for {label}: {e}"
                            )))
                            .await;
                        return;
                    }
                };
                let result = monitor_transcode(child, &sid).await;
                let _ = tx.send(result).await;
            });
        }

        drop(results_tx);

        tokio::spawn(async move {
            let mut all_ok = true;
            let mut first_err = String::new();
            while let Some(result) = results_rx.recv().await {
                if let Err(msg) = result {
                    all_ok = false;
                    if first_err.is_empty() {
                        first_err = msg;
                    }
                }
            }
            let status = if all_ok {
                TranscodeStatus::Complete
            } else {
                TranscodeStatus::Failed(first_err)
            };
            let _ = agg_tx.send(status);
            drop(permit);
        });

        Ok(TranscodeHandle {
            stream_id: stream_id.to_string(),
            output_dir,
            master_playlist_path: master_path,
            status: agg_rx,
            child_pids: child_pids.clone(),
        })
    }

    /// Passthrough (no transcode) from an async reader piped into FFmpeg stdin.
    pub async fn start_passthrough_piped<R: AsyncRead + Unpin + Send + 'static>(
        &self,
        stream_id: &str,
        reader: R,
    ) -> Result<TranscodeHandle> {
        let permit =
            self.semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| Error::Transcode {
                    message: "Transcode semaphore closed".to_string(),
                })?;

        let output_dir = self.cache_dir.join(stream_id);
        tokio::fs::create_dir_all(&output_dir)
            .await
            .map_err(|e| Error::Transcode {
                message: format!("Failed to create output directory: {e}"),
            })?;

        let playlist_path = output_dir.join("playlist.m3u8");
        let segment_pattern = output_dir.join("segment_%04d.ts");

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("warning")
            .arg("-probesize")
            .arg("5000000")
            .arg("-analyzeduration")
            .arg("3000000")
            .arg("-fflags")
            .arg("+genpts+igndts+discardcorrupt")
            .arg("-i")
            .arg("pipe:0")
            .arg("-c")
            .arg("copy")
            .arg("-avoid_negative_ts")
            .arg("make_zero")
            .arg("-max_muxing_queue_size")
            .arg("4096")
            .arg("-f")
            .arg("hls")
            .arg("-hls_time")
            .arg("2")
            .arg("-hls_init_time")
            .arg("1")
            .arg("-hls_list_size")
            .arg("0")
            .arg("-hls_segment_type")
            .arg("mpegts")
            .arg("-hls_flags")
            .arg("independent_segments+append_list")
            .arg("-movflags")
            .arg("+faststart")
            .arg("-hls_segment_filename")
            .arg(&segment_pattern)
            .arg(&playlist_path);

        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| Error::Transcode {
            message: format!("Failed to spawn ffmpeg (passthrough piped): {e}"),
        })?;
        let child_pids: Arc<std::sync::Mutex<Vec<u32>>> = Arc::new(std::sync::Mutex::new(
            child.id().into_iter().collect()
        ));

        let stdin = child.stdin.take().ok_or_else(|| Error::Transcode {
            message: "Failed to get ffmpeg stdin handle".to_string(),
        })?;
        let sid_pipe = stream_id.to_string();
        tokio::spawn(async move {
            let mut reader = reader;
            let mut stdin = stdin;
            match tokio::io::copy(&mut reader, &mut stdin).await {
                Ok(bytes) => {
                    tracing::info!(stream_id = %sid_pipe, bytes, "Finished piping to ffmpeg stdin (passthrough)");
                }
                Err(e) => {
                    tracing::warn!(stream_id = %sid_pipe, "Error piping to ffmpeg stdin (passthrough): {e}");
                }
            }
            drop(stdin);
        });

        let (status_tx, status_rx) = watch::channel(TranscodeStatus::Running);
        let sid = stream_id.to_string();

        tokio::spawn(async move {
            let result = monitor_transcode(child, &sid).await;
            let status = match result {
                Ok(()) => TranscodeStatus::Complete,
                Err(msg) => TranscodeStatus::Failed(msg),
            };
            let _ = status_tx.send(status);
            drop(permit);
        });

        Ok(TranscodeHandle {
            stream_id: stream_id.to_string(),
            output_dir,
            master_playlist_path: playlist_path,
            status: status_rx,
            child_pids: child_pids.clone(),
        })
    }

    fn build_variant_command_gpu(
        &self,
        input_path: &str,
        media_info: &MediaInfo,
        tier: &QualityTier,
        output_dir: &Path,
    ) -> std::result::Result<Command, String> {
        // Always transcode to H.264 for MPEG-TS HLS (browsers can't play HEVC in TS)

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-hide_banner").arg("-loglevel").arg("warning");
        cmd.arg("-probesize").arg("5000000");
        cmd.arg("-analyzeduration").arg("3000000");
        cmd.arg("-fflags").arg("+genpts+igndts+discardcorrupt");

        // VAAPI: hybrid mode (CPU decode + GPU encode) - works for all input codecs
        // Full HW decode (-hwaccel vaapi) fails on HEVC 10-bit on many GPUs
        match &self.hw_accel {
            HwAccel::Vaapi => {
                cmd.arg("-init_hw_device").arg("vaapi=va:/dev/dri/renderD128");
                cmd.arg("-filter_hw_device").arg("va");
            }
            other => {
                for flag in gpu::hw_decode_flags(other) {
                    cmd.arg(flag);
                }
            }
        }

        cmd.arg("-i").arg(input_path);
        cmd.arg("-c:v").arg(gpu::encoder_for_hw(&self.hw_accel));

        // Scale + format conversion per accelerator
        match &self.hw_accel {
            HwAccel::Vaapi => {
                // CPU scale → nv12 → hwupload to VAAPI
                let vf = if let Some(h) = tier.height {
                    format!("scale=-2:{h},format=nv12,hwupload")
                } else {
                    "format=nv12,hwupload".to_string()
                };
                cmd.arg("-vf").arg(vf);
                cmd.arg("-global_quality").arg(self.config.crf.to_string());
            }
            HwAccel::Nvenc => {
                if let Some(h) = tier.height {
                    cmd.arg("-vf").arg(format!("scale_cuda=w=-2:h={h}:format=nv12"));
                }
                cmd.arg("-preset").arg("p4");
                cmd.arg("-rc").arg("vbr");
                cmd.arg("-cq").arg(self.config.crf.to_string());
                cmd.arg("-maxrate").arg(tier.video_bitrate);
                let bs = parse_bitrate(tier.video_bitrate).saturating_mul(2) / 1000;
                cmd.arg("-bufsize").arg(format!("{bs}k"));
            }
            _ => {
                if let Some(h) = tier.height {
                    cmd.arg("-vf").arg(format!("scale=-2:{h}"));
                }
                cmd.arg("-preset").arg(&self.config.preset);
                cmd.arg("-crf").arg(self.config.crf.to_string());
                cmd.arg("-tune").arg("film");
                match self.config.threads {
                    Some(t) => { cmd.arg("-threads").arg(t.to_string()); }
                    None => { cmd.arg("-threads").arg("0"); }
                }
                cmd.arg("-maxrate").arg(tier.video_bitrate);
                let bs = parse_bitrate(tier.video_bitrate).saturating_mul(2) / 1000;
                cmd.arg("-bufsize").arg(format!("{bs}k"));
            }
        }

        apply_audio_args(&mut cmd, media_info, tier.audio_bitrate, self.config.hls_force_stereo);

        cmd.arg("-sn");
        cmd.arg("-avoid_negative_ts").arg("make_zero");
        cmd.arg("-max_muxing_queue_size").arg("4096");

        let tier_dir = output_dir.join(tier.label);
        let playlist_path = tier_dir.join("playlist.m3u8");
        let segment_pattern = tier_dir.join("segment_%04d.ts");

        cmd.arg("-f").arg("hls");
        cmd.arg("-hls_time").arg("2");
        cmd.arg("-hls_init_time").arg("1");
        cmd.arg("-hls_list_size").arg("0");
        cmd.arg("-hls_segment_type").arg("mpegts");
        cmd.arg("-hls_flags")
            .arg("independent_segments+append_list");
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg("-hls_segment_filename").arg(&segment_pattern);
        cmd.arg(&playlist_path);

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        Ok(cmd)
    }

    fn build_variant_command_cpu(
        &self,
        input_path: &str,
        media_info: &MediaInfo,
        tier: &QualityTier,
        output_dir: &Path,
    ) -> Command {
        let mut cmd = tokio::process::Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("warning");
        cmd.arg("-probesize").arg("5000000");
        cmd.arg("-analyzeduration").arg("3000000");
        cmd.arg("-fflags").arg("+genpts+igndts+discardcorrupt");
        cmd.arg("-i").arg(input_path);

        // Always transcode to H.264 for MPEG-TS HLS
        {
            cmd.arg("-c:v").arg("libx264");
            cmd.arg("-preset").arg(&self.config.preset);
            cmd.arg("-crf").arg(self.config.crf.to_string());
            cmd.arg("-tune").arg("film");
            match self.config.threads {
                Some(threads) => { cmd.arg("-threads").arg(threads.to_string()); }
                None => { cmd.arg("-threads").arg("0"); }
            }

            let has_hdr =
                media_info.has_hdr10 || media_info.has_dolby_vision || media_info.has_hdr10_plus;

            if has_hdr {
                let scale = if let Some(h) = tier.height {
                    format!(",scale=-2:{h}")
                } else {
                    String::new()
                };
                cmd.arg("-vf").arg(format!(
                    "zscale=t=linear:npl=100,format=gbrpf32le,\
                     zscale=p=bt709,tonemap=tonemap=hable:desat=0,\
                     zscale=t=bt709:m=bt709:r=tv,format=yuv420p{scale}"
                ));
            } else if let Some(h) = tier.height {
                cmd.arg("-vf").arg(format!("scale=-2:{h}"));
            }

            cmd.arg("-maxrate").arg(tier.video_bitrate);
            let bufsize_kbps = parse_bitrate(tier.video_bitrate).saturating_mul(2) / 1000;
            cmd.arg("-bufsize").arg(format!("{bufsize_kbps}k"));
        }

        apply_audio_args(&mut cmd, media_info, tier.audio_bitrate, self.config.hls_force_stereo);

        cmd.arg("-sn");
        cmd.arg("-avoid_negative_ts").arg("make_zero");
        cmd.arg("-max_muxing_queue_size").arg("4096");

        let tier_dir = output_dir.join(tier.label);
        let playlist_path = tier_dir.join("playlist.m3u8");
        let segment_pattern = tier_dir.join("segment_%04d.ts");

        cmd.arg("-f").arg("hls");
        cmd.arg("-hls_time").arg("2");
        cmd.arg("-hls_init_time").arg("1");
        cmd.arg("-hls_list_size").arg("0");
        cmd.arg("-hls_segment_type").arg("mpegts");
        cmd.arg("-hls_flags")
            .arg("independent_segments+append_list");
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg("-hls_segment_filename").arg(&segment_pattern);
        cmd.arg(&playlist_path);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        cmd
    }
}

async fn monitor_transcode(
    mut child: tokio::process::Child,
    stream_id: &str,
) -> std::result::Result<(), String> {
    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to wait for ffmpeg process: {e}"))?;

    if status.success() {
        tracing::info!(stream_id, "Transcode completed successfully");
        Ok(())
    } else {
        let stderr = if let Some(mut stderr) = child.stderr.take() {
            let mut buf = Vec::new();
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut buf).await;
            String::from_utf8_lossy(&buf).to_string()
        } else {
            String::new()
        };
        let code = status.code().unwrap_or(-1);
        let msg = format!("ffmpeg exited with code {code}: {stderr}");
        tracing::error!(stream_id, %msg, "Transcode failed");
        Err(msg)
    }
}

fn parse_bitrate(s: &str) -> u64 {
    let s = s.trim();
    if let Some(num_str) = s.strip_suffix('M').or_else(|| s.strip_suffix('m')) {
        num_str
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_mul(1_000_000)
    } else if let Some(num_str) = s.strip_suffix('K').or_else(|| s.strip_suffix('k')) {
        num_str.parse::<u64>().unwrap_or(0).saturating_mul(1_000)
    } else {
        s.parse::<u64>().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bitrate_megabits() {
        assert_eq!(parse_bitrate("8M"), 8_000_000);
        assert_eq!(parse_bitrate("4m"), 4_000_000);
    }

    #[test]
    fn parse_bitrate_kilobits() {
        assert_eq!(parse_bitrate("192k"), 192_000);
        assert_eq!(parse_bitrate("256K"), 256_000);
    }

    #[test]
    fn parse_bitrate_plain() {
        assert_eq!(parse_bitrate("1000000"), 1_000_000);
    }

    #[test]
    fn parse_bitrate_invalid() {
        assert_eq!(parse_bitrate("invalid"), 0);
    }

    #[test]
    fn select_tier_by_label() {
        assert_eq!(select_tier("source").label, "source");
        assert_eq!(select_tier("720p").label, "720p");
        assert_eq!(select_tier("360p").label, "360p");
        assert_eq!(select_tier("1080p").label, "1080p");
        assert_eq!(select_tier("invalid").label, "source");
    }

    #[test]
    fn available_qualities_for_heights() {
        assert_eq!(available_qualities(2160), vec!["360p", "720p", "1080p", "source"]);
        assert_eq!(available_qualities(1080), vec!["360p", "720p", "source"]);
        assert_eq!(available_qualities(480), vec!["360p", "source"]);
        assert_eq!(available_qualities(360), vec!["source"]);
    }

    #[test]
    fn master_playlist_format() {
        let tier = select_tier("720p");
        let playlist = generate_master_playlist(&[tier], 720);
        assert!(playlist.contains("#EXTM3U"));
        assert!(playlist.contains("#EXT-X-STREAM-INF:"));
        assert!(playlist.contains("720p/playlist.m3u8"));
    }
}
