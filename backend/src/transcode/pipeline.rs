use crate::config::TranscodeConfig;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    pub playlist_path: PathBuf,
    pub status: watch::Receiver<TranscodeStatus>,
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

    pub async fn start_transcode(
        &self,
        stream_id: &str,
        input_path: &str,
        media_info: &MediaInfo,
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

        let mut cmd = self.build_ffmpeg_command(
            input_path,
            media_info,
            &playlist_path,
            &segment_pattern,
            &output_dir,
        );

        let child = cmd.spawn().map_err(|e| Error::Transcode {
            message: format!("Failed to spawn ffmpeg: {e}"),
        })?;

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
            playlist_path,
            status: status_rx,
        })
    }

    pub async fn start_transcode_cpu(
        &self,
        stream_id: &str,
        input_path: &str,
        media_info: &MediaInfo,
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

        let mut cmd = tokio::process::Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("warning");
        cmd.arg("-probesize").arg("5000000");
        cmd.arg("-analyzeduration").arg("3000000");
        cmd.arg("-fflags").arg("+genpts+igndts+discardcorrupt");
        cmd.arg("-i").arg(input_path);
        cmd.arg("-c:v").arg("libx264");
        cmd.arg("-preset").arg(&self.config.preset);
        cmd.arg("-crf").arg(self.config.crf.to_string());
        cmd.arg("-tune").arg("zerolatency");
        cmd.arg("-threads").arg("0");

        if media_info.has_hdr10 || media_info.has_dolby_vision || media_info.has_hdr10_plus {
            cmd.arg("-vf").arg(
                "zscale=t=linear:npl=100,format=gbrpf32le,\
                 zscale=p=bt709,tonemap=tonemap=hable:desat=0,\
                 zscale=t=bt709:m=bt709:r=tv,format=yuv420p",
            );
        }

        if media_info.needs_audio_transcode {
            cmd.arg("-c:a").arg("aac");
            cmd.arg("-b:a").arg(&self.config.audio_bitrate);
            let channels = if media_info.audio_channels.unwrap_or(2) > 2 {
                "6"
            } else {
                "2"
            };
            cmd.arg("-ac").arg(channels);
        } else {
            cmd.arg("-c:a").arg("copy");
        }

        cmd.arg("-sn");
        cmd.arg("-avoid_negative_ts").arg("make_zero");
        cmd.arg("-max_muxing_queue_size").arg("4096");
        cmd.arg("-f").arg("hls");
        cmd.arg("-hls_time").arg("2");
        cmd.arg("-hls_init_time").arg("1");
        cmd.arg("-hls_list_size").arg("0");
        cmd.arg("-hls_segment_type").arg("mpegts");
        cmd.arg("-hls_flags")
            .arg("independent_segments+append_list");
        cmd.arg("-hls_playlist_type").arg("vod");
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg("-hls_segment_filename").arg(&segment_pattern);
        cmd.arg(&playlist_path);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = cmd.spawn().map_err(|e| Error::Transcode {
            message: format!("Failed to spawn ffmpeg (CPU fallback): {e}"),
        })?;

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
            playlist_path,
            status: status_rx,
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

        let _init_filename = "init.mp4";

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
            .arg("-hls_playlist_type")
            .arg("event")
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
            playlist_path,
            status: status_rx,
        })
    }

    fn build_ffmpeg_command(
        &self,
        input_path: &str,
        media_info: &MediaInfo,
        playlist_path: &Path,
        segment_pattern: &Path,
        _output_dir: &Path,
    ) -> Command {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y");
        cmd.arg("-hide_banner");
        cmd.arg("-loglevel").arg("warning");

        cmd.arg("-probesize").arg("5000000");
        cmd.arg("-analyzeduration").arg("3000000");
        cmd.arg("-fflags").arg("+genpts+igndts+discardcorrupt");

        for flag in gpu::hw_decode_flags(&self.hw_accel) {
            cmd.arg(flag);
        }

        cmd.arg("-i").arg(input_path);

        let video_encoder = gpu::encoder_for_hw(&self.hw_accel);
        cmd.arg("-c:v").arg(video_encoder);

        if media_info.has_hdr10 || media_info.has_dolby_vision || media_info.has_hdr10_plus {
            match &self.hw_accel {
                HwAccel::Nvenc => {
                    cmd.arg("-vf").arg(
                        "scale_cuda=format=nv12,hwdownload,format=nv12,\
                         tonemap=hable:desat=0,format=yuv420p,hwupload_cuda",
                    );
                }
                HwAccel::Vaapi => {
                    cmd.arg("-vf")
                        .arg("tonemap_vaapi=t=bt709:m=bt709:p=bt709,scale_vaapi=format=nv12");
                }
                _ => {
                    cmd.arg("-vf").arg(
                        "zscale=t=linear:npl=100,format=gbrpf32le,\
                         zscale=p=bt709,tonemap=tonemap=hable:desat=0,\
                         zscale=t=bt709:m=bt709:r=tv,format=yuv420p",
                    );
                }
            }
        }

        match &self.hw_accel {
            HwAccel::Nvenc => {
                cmd.arg("-preset").arg("p4");
                cmd.arg("-rc").arg("vbr");
                cmd.arg("-cq").arg(self.config.crf.to_string());
            }
            HwAccel::None => {
                cmd.arg("-preset").arg(&self.config.preset);
                cmd.arg("-crf").arg(self.config.crf.to_string());
                cmd.arg("-tune").arg("zerolatency");
                match self.config.threads {
                    Some(threads) => {
                        cmd.arg("-threads").arg(threads.to_string());
                    }
                    None => {
                        cmd.arg("-threads").arg("0");
                    }
                }
            }
            _ => {
                cmd.arg("-preset").arg(&self.config.preset);
            }
        }

        if let Some(ref max_br) = self.config.max_bitrate {
            cmd.arg("-maxrate").arg(max_br);
            let bufsize_kbps = parse_bitrate(max_br).saturating_mul(2) / 1000;
            cmd.arg("-bufsize").arg(format!("{bufsize_kbps}k"));
        }

        if media_info.needs_audio_transcode {
            cmd.arg("-c:a").arg("aac");
            cmd.arg("-b:a").arg(&self.config.audio_bitrate);
            let target_channels = if media_info.audio_channels.unwrap_or(2) > 2 {
                "6"
            } else {
                "2"
            };
            cmd.arg("-ac").arg(target_channels);
        } else {
            cmd.arg("-c:a").arg("copy");
        }

        cmd.arg("-sn");

        cmd.arg("-avoid_negative_ts").arg("make_zero");
        cmd.arg("-max_muxing_queue_size").arg("4096");

        let _init_filename = "init.mp4";

        cmd.arg("-f").arg("hls");
        cmd.arg("-hls_time").arg("2");
        cmd.arg("-hls_init_time").arg("1");
        cmd.arg("-hls_list_size").arg("0");
        cmd.arg("-hls_segment_type").arg("mpegts");
        cmd.arg("-hls_flags")
            .arg("independent_segments+append_list");
        cmd.arg("-hls_playlist_type").arg("vod");
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg("-hls_segment_filename").arg(segment_pattern);
        cmd.arg(playlist_path);

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
}
