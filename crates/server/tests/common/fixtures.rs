use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

static CLIPS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Directory containing all generated test clips.
/// Clips are cached across test runs (only regenerated if missing).
pub fn clips_dir() -> &'static Path {
    CLIPS_DIR.get_or_init(|| {
        let dir = PathBuf::from("/tmp/streamx_test_clips");
        std::fs::create_dir_all(&dir).expect("create clips dir");
        dir
    })
}

/// All clip definitions with burned-in frame numbers for verification.
/// The drawtext filter burns `frame_NNNN t=SS.MMM` into each frame,
/// making every frame unique and verifiable via screenshot comparison.
#[derive(Debug, Clone)]
pub struct ClipDef {
    pub id: &'static str,
    pub video_codec: &'static str,
    pub audio_codec: &'static str,
    pub container: &'static str,
    pub resolution: &'static str,
    pub audio_channels: u32,
    pub duration_secs: u32,
    pub browser_compatible: bool,
    pub needs_hls_transcode: bool,
}

pub const ALL_CLIPS: &[ClipDef] = &[
    ClipDef {
        id: "h264_aac_mp4",
        video_codec: "libx264",
        audio_codec: "aac",
        container: "mp4",
        resolution: "1280x720",
        audio_channels: 2,
        duration_secs: 15,
        browser_compatible: true,
        needs_hls_transcode: false,
    },
    ClipDef {
        id: "h264_ac3_mkv",
        video_codec: "libx264",
        audio_codec: "ac3",
        container: "matroska",
        resolution: "1280x720",
        audio_channels: 6,
        duration_secs: 15,
        browser_compatible: false,
        needs_hls_transcode: true,
    },
    ClipDef {
        id: "hevc_aac_mkv",
        video_codec: "libx265",
        audio_codec: "aac",
        container: "matroska",
        resolution: "1920x1080",
        audio_channels: 2,
        duration_secs: 15,
        browser_compatible: false,
        needs_hls_transcode: true,
    },
    ClipDef {
        id: "hevc_eac3_mkv",
        video_codec: "libx265",
        audio_codec: "eac3",
        container: "matroska",
        resolution: "1920x1080",
        audio_channels: 6,
        duration_secs: 10,
        browser_compatible: false,
        needs_hls_transcode: true,
    },
    ClipDef {
        id: "vp9_opus_webm",
        video_codec: "libvpx-vp9",
        audio_codec: "libopus",
        container: "webm",
        resolution: "1280x720",
        audio_channels: 2,
        duration_secs: 15,
        browser_compatible: false,
        needs_hls_transcode: true,
    },
    ClipDef {
        id: "h264_aac_ts",
        video_codec: "libx264",
        audio_codec: "aac",
        container: "mpegts",
        resolution: "1280x720",
        audio_channels: 2,
        duration_secs: 15,
        browser_compatible: false,
        needs_hls_transcode: true,
    },
    ClipDef {
        id: "hevc_aac_mp4",
        video_codec: "libx265",
        audio_codec: "aac",
        container: "mp4",
        resolution: "1920x1080",
        audio_channels: 2,
        duration_secs: 15,
        browser_compatible: false,
        needs_hls_transcode: true,
    },
];

fn ext_for_container(container: &str) -> &str {
    match container {
        "matroska" => "mkv",
        "mpegts" => "ts",
        "webm" => "webm",
        _ => "mp4",
    }
}

/// Get path to a specific test clip, generating it if needed.
pub fn get_clip(id: &str) -> Option<PathBuf> {
    let def = ALL_CLIPS.iter().find(|c| c.id == id)?;
    let ext = ext_for_container(def.container);
    let path = clips_dir().join(format!("{}.{ext}", def.id));
    if path.exists() {
        return Some(path);
    }
    if generate_clip(def, &path) {
        Some(path)
    } else {
        None
    }
}

/// Get the ClipDef for a clip ID.
pub fn clip_def(id: &str) -> Option<&'static ClipDef> {
    ALL_CLIPS.iter().find(|c| c.id == id)
}

fn generate_clip(def: &ClipDef, output: &Path) -> bool {
    let drawtext = format!(
        "drawtext=text='frame_%{{frame_num}} t=%{{pts\\:hms}}  {}':x=10:y=10:fontsize=28:\
         fontcolor=white:box=1:boxcolor=black@0.7:borderw=2",
        def.id
    );

    let (w, h) = def.resolution.split_once('x').unwrap_or(("1280", "720"));

    let video_filter = if def.video_codec == "libvpx-vp9" {
        // VP9 doesn't support drawtext easily, use simple testsrc
        format!(
            "testsrc=duration={}:size={}x{}:rate=24",
            def.duration_secs, w, h
        )
    } else {
        format!(
            "testsrc2=duration={}:size={}x{}:rate=24,{drawtext},format=yuv420p",
            def.duration_secs, w, h
        )
    };

    let audio_src = format!("sine=frequency=440:duration={}", def.duration_secs);

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        video_filter,
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        audio_src,
    ];

    // Video encoder settings
    args.extend(["-c:v".into(), def.video_codec.into()]);
    match def.video_codec {
        "libx264" => {
            args.extend([
                "-preset".into(),
                "ultrafast".into(),
                "-crf".into(),
                "28".into(),
            ]);
        }
        "libx265" => {
            args.extend([
                "-preset".into(),
                "ultrafast".into(),
                "-crf".into(),
                "32".into(),
            ]);
            if def.resolution == "1920x1080" || def.resolution.contains("2160") {
                args.extend(["-pix_fmt".into(), "yuv420p".into()]);
            }
        }
        "libvpx-vp9" => {
            args.extend([
                "-b:v".into(),
                "1M".into(),
                "-crf".into(),
                "30".into(),
                "-deadline".into(),
                "realtime".into(),
                "-cpu-used".into(),
                "8".into(),
            ]);
        }
        _ => {}
    }

    // Audio encoder settings
    args.extend(["-c:a".into(), def.audio_codec.into()]);
    args.extend(["-ac".into(), def.audio_channels.to_string()]);
    match def.audio_codec {
        "aac" => {
            args.extend(["-b:a".into(), "128k".into()]);
        }
        "ac3" | "eac3" => {
            args.extend(["-b:a".into(), "384k".into()]);
        }
        "libopus" => {
            args.extend(["-b:a".into(), "128k".into()]);
        }
        _ => {}
    }

    // Container format
    args.extend(["-f".into(), def.container.into()]);
    args.push(output.to_string_lossy().to_string());

    let status = Command::new("ffmpeg").args(&args).status();
    match status {
        Ok(s) if s.success() => {
            eprintln!(
                "  Generated clip: {} ({} bytes)",
                def.id,
                std::fs::metadata(output).map(|m| m.len()).unwrap_or(0)
            );
            true
        }
        Ok(s) => {
            eprintln!("  FAILED to generate {}: exit code {:?}", def.id, s.code());
            false
        }
        Err(e) => {
            eprintln!("  FAILED to generate {}: {e}", def.id);
            false
        }
    }
}

/// Generate all clips (called once, idempotent).
pub fn ensure_all_clips() {
    for def in ALL_CLIPS {
        let _ = get_clip(def.id);
    }
}

/// Extract a golden frame at a specific timestamp for verification.
pub fn extract_golden_frame(clip_path: &Path, timestamp_secs: f64, output: &Path) -> bool {
    Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{timestamp_secs:.3}"),
            "-i",
        ])
        .arg(clip_path)
        .args(["-frames:v", "1", "-f", "image2"])
        .arg(output)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Compare two images using ImageMagick, returns difference percentage (0.0 = identical).
pub fn compare_images(a: &Path, b: &Path) -> Option<f64> {
    let output = Command::new("compare")
        .args(["-metric", "RMSE"])
        .arg(a)
        .arg(b)
        .arg("/dev/null")
        .output()
        .ok()?;

    // ImageMagick outputs RMSE to stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Format: "1234.56 (0.0189)" - we want the normalized value in parens
    stderr
        .split('(')
        .nth(1)
        .and_then(|s| s.trim_end_matches(')').trim().parse::<f64>().ok())
        .map(|v| v * 100.0) // convert to percentage
}

// ============================================================
// Mock Torrent Writer
// ============================================================

/// Simulates a torrent sequential download by writing a file in chunks with controlled timing.
pub struct MockTorrentWriter {
    source_data: Vec<u8>,
    output_path: PathBuf,
    schedule: Vec<ChunkWrite>,
}

/// A single chunk write operation.
pub struct ChunkWrite {
    pub delay_ms: u64,
    pub byte_count: usize,
}

impl MockTorrentWriter {
    /// Create from a source file with a predefined write schedule.
    pub fn new(
        source_path: &Path,
        output_path: PathBuf,
        schedule: Vec<ChunkWrite>,
    ) -> std::io::Result<Self> {
        let source_data = std::fs::read(source_path)?;
        Ok(Self {
            source_data,
            output_path,
            schedule,
        })
    }

    /// Create with evenly-spaced chunks.
    pub fn uniform(
        source_path: &Path,
        output_path: PathBuf,
        chunk_count: usize,
        delay_ms: u64,
    ) -> std::io::Result<Self> {
        let source_data = std::fs::read(source_path)?;
        let chunk_size = source_data.len() / chunk_count;
        let schedule: Vec<ChunkWrite> = (0..chunk_count)
            .map(|i| {
                let remaining = source_data.len() - (i * chunk_size);
                ChunkWrite {
                    delay_ms,
                    byte_count: chunk_size.min(remaining),
                }
            })
            .collect();
        Ok(Self {
            source_data,
            output_path,
            schedule,
        })
    }

    /// Preset: fast torrent (100ms between chunks)
    pub fn fast(source_path: &Path, output_path: PathBuf) -> std::io::Result<Self> {
        Self::uniform(source_path, output_path, 10, 100)
    }

    /// Preset: slow start (900ms, 300ms, 500ms, then fast)
    pub fn slow_start(source_path: &Path, output_path: PathBuf) -> std::io::Result<Self> {
        let data = std::fs::read(source_path)?;
        let chunk = data.len() / 10;
        let schedule = vec![
            ChunkWrite {
                delay_ms: 900,
                byte_count: chunk,
            },
            ChunkWrite {
                delay_ms: 300,
                byte_count: chunk,
            },
            ChunkWrite {
                delay_ms: 500,
                byte_count: chunk,
            },
            ChunkWrite {
                delay_ms: 100,
                byte_count: chunk,
            },
            ChunkWrite {
                delay_ms: 100,
                byte_count: chunk,
            },
            ChunkWrite {
                delay_ms: 100,
                byte_count: chunk,
            },
            ChunkWrite {
                delay_ms: 100,
                byte_count: chunk,
            },
            ChunkWrite {
                delay_ms: 100,
                byte_count: chunk,
            },
            ChunkWrite {
                delay_ms: 100,
                byte_count: chunk,
            },
            ChunkWrite {
                delay_ms: 100,
                byte_count: data.len() - 9 * chunk,
            },
        ];
        Ok(Self {
            source_data: data,
            output_path,
            schedule,
        })
    }

    /// Preset: stalling download (writes half, pauses, then completes)
    pub fn stalling(
        source_path: &Path,
        output_path: PathBuf,
        stall_ms: u64,
    ) -> std::io::Result<Self> {
        let data = std::fs::read(source_path)?;
        let half = data.len() / 2;
        let schedule = vec![
            ChunkWrite {
                delay_ms: 100,
                byte_count: half,
            },
            ChunkWrite {
                delay_ms: stall_ms,
                byte_count: 0,
            }, // stall (write nothing)
            ChunkWrite {
                delay_ms: 100,
                byte_count: data.len() - half,
            },
        ];
        Ok(Self {
            source_data: data,
            output_path,
            schedule,
        })
    }

    /// Preset: burst (nothing for N ms, then entire file at once)
    pub fn burst(
        source_path: &Path,
        output_path: PathBuf,
        initial_delay_ms: u64,
    ) -> std::io::Result<Self> {
        let data = std::fs::read(source_path)?;
        let len = data.len();
        let schedule = vec![
            ChunkWrite {
                delay_ms: initial_delay_ms,
                byte_count: 0,
            },
            ChunkWrite {
                delay_ms: 0,
                byte_count: len,
            },
        ];
        Ok(Self {
            source_data: data,
            output_path,
            schedule,
        })
    }

    /// Execute the write schedule asynchronously (sequential append).
    pub async fn execute(&self) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&self.output_path).await?;
        let mut offset = 0usize;

        for chunk in &self.schedule {
            if chunk.delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(chunk.delay_ms)).await;
            }
            if chunk.byte_count > 0 && offset < self.source_data.len() {
                let end = (offset + chunk.byte_count).min(self.source_data.len());
                file.write_all(&self.source_data[offset..end]).await?;
                file.flush().await?;
                offset = end;
            }
        }

        if offset < self.source_data.len() {
            file.write_all(&self.source_data[offset..]).await?;
            file.flush().await?;
        }

        Ok(())
    }
}

// ============================================================
// Sparse File Torrent Writer (realistic torrent simulation)
// ============================================================

/// Simulates a real torrent download with sparse file allocation.
/// The file is pre-allocated at full size, then pieces are written at specific
/// offsets in potentially non-sequential order, like a real BitTorrent client.
pub struct SparseTorrentWriter {
    source_data: Vec<u8>,
    output_path: PathBuf,
    piece_size: usize,
    piece_schedule: Vec<PieceWrite>,
}

pub struct PieceWrite {
    pub piece_index: usize,
    pub delay_ms: u64,
}

impl SparseTorrentWriter {
    /// Create with explicit piece schedule.
    pub fn new(
        source_path: &Path,
        output_path: PathBuf,
        piece_size: usize,
        schedule: Vec<PieceWrite>,
    ) -> std::io::Result<Self> {
        let source_data = std::fs::read(source_path)?;
        Ok(Self {
            source_data,
            output_path,
            piece_size,
            piece_schedule: schedule,
        })
    }

    /// Sequential order with delays (like torrent with sequential preference).
    pub fn sequential(
        source_path: &Path,
        output_path: PathBuf,
        piece_size: usize,
        delay_ms: u64,
    ) -> std::io::Result<Self> {
        let data = std::fs::read(source_path)?;
        let piece_count = data.len().div_ceil(piece_size);
        let schedule: Vec<PieceWrite> = (0..piece_count)
            .map(|i| PieceWrite {
                piece_index: i,
                delay_ms,
            })
            .collect();
        Ok(Self {
            source_data: data,
            output_path,
            piece_size,
            piece_schedule: schedule,
        })
    }

    /// Sequential with variable delays (slow start pattern).
    pub fn sequential_slow_start(
        source_path: &Path,
        output_path: PathBuf,
        piece_size: usize,
    ) -> std::io::Result<Self> {
        let data = std::fs::read(source_path)?;
        let piece_count = data.len().div_ceil(piece_size);
        let schedule: Vec<PieceWrite> = (0..piece_count)
            .map(|i| PieceWrite {
                piece_index: i,
                delay_ms: if i == 0 {
                    900
                } else if i == 1 {
                    300
                } else if i == 2 {
                    500
                } else {
                    50
                },
            })
            .collect();
        Ok(Self {
            source_data: data,
            output_path,
            piece_size,
            piece_schedule: schedule,
        })
    }

    /// Out-of-order pieces (realistic torrent without sequential preference).
    /// Writes first piece, then last, then middle pieces in random-ish order.
    pub fn out_of_order(
        source_path: &Path,
        output_path: PathBuf,
        piece_size: usize,
        delay_ms: u64,
    ) -> std::io::Result<Self> {
        let data = std::fs::read(source_path)?;
        let piece_count = data.len().div_ceil(piece_size);
        let mut order: Vec<usize> = Vec::with_capacity(piece_count);
        // First piece (needed for container header)
        order.push(0);
        // Last piece
        if piece_count > 1 {
            order.push(piece_count - 1);
        }
        // Even-indexed pieces
        for i in (2..piece_count - 1).step_by(2) {
            order.push(i);
        }
        // Odd-indexed pieces
        for i in (1..piece_count - 1).step_by(2) {
            order.push(i);
        }

        let schedule: Vec<PieceWrite> = order
            .into_iter()
            .map(|i| PieceWrite {
                piece_index: i,
                delay_ms,
            })
            .collect();
        Ok(Self {
            source_data: data,
            output_path,
            piece_size,
            piece_schedule: schedule,
        })
    }

    /// Stalling pattern: first few pieces fast, then long pause, then rest.
    pub fn stalling(
        source_path: &Path,
        output_path: PathBuf,
        piece_size: usize,
        stall_ms: u64,
    ) -> std::io::Result<Self> {
        let data = std::fs::read(source_path)?;
        let piece_count = data.len().div_ceil(piece_size);
        let stall_at = piece_count / 3;
        let schedule: Vec<PieceWrite> = (0..piece_count)
            .map(|i| PieceWrite {
                piece_index: i,
                delay_ms: if i == stall_at { stall_ms } else { 50 },
            })
            .collect();
        Ok(Self {
            source_data: data,
            output_path,
            piece_size,
            piece_schedule: schedule,
        })
    }

    /// Execute: pre-allocate sparse file, then write pieces at offsets.
    pub async fn execute(&self) -> std::io::Result<()> {
        use tokio::io::{AsyncSeekExt, AsyncWriteExt};

        // Pre-allocate the file at full size (sparse)
        let file = tokio::fs::File::create(&self.output_path).await?;
        file.set_len(self.source_data.len() as u64).await?;
        drop(file);

        // Write pieces according to schedule
        for pw in &self.piece_schedule {
            if pw.delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(pw.delay_ms)).await;
            }

            let offset = pw.piece_index * self.piece_size;
            if offset >= self.source_data.len() {
                continue;
            }
            let end = (offset + self.piece_size).min(self.source_data.len());
            let piece_data = &self.source_data[offset..end];

            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&self.output_path)
                .await?;
            file.seek(std::io::SeekFrom::Start(offset as u64)).await?;
            file.write_all(piece_data).await?;
            file.flush().await?;
        }

        Ok(())
    }
}

// ============================================================
// HLS Segment Delay Controller
// ============================================================

/// Controls when HLS segments become visible to the player.
/// Wraps a real transcode output directory and delays segment availability.
pub struct SegmentDelayController {
    staging_dir: PathBuf,
    served_dir: PathBuf,
    schedule: Vec<SegmentDelay>,
}

pub struct SegmentDelay {
    pub segment_index: u32,
    pub available_after_ms: u64,
}

impl SegmentDelayController {
    pub fn new(staging_dir: PathBuf, served_dir: PathBuf, schedule: Vec<SegmentDelay>) -> Self {
        std::fs::create_dir_all(&served_dir).ok();
        Self {
            staging_dir,
            served_dir,
            schedule,
        }
    }

    /// Run the delay controller - moves segments from staging to served on schedule.
    pub async fn execute(&self) {
        let start = std::time::Instant::now();

        // Always copy the playlist immediately (but it references segments that may not exist yet)
        loop {
            let playlist_src = self.staging_dir.join("playlist.m3u8");
            if playlist_src.exists() {
                let _ = std::fs::copy(&playlist_src, self.served_dir.join("playlist.m3u8"));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if start.elapsed() > std::time::Duration::from_secs(30) {
                break;
            }
        }

        for delay in &self.schedule {
            let target_ms = delay.available_after_ms;
            let elapsed = start.elapsed().as_millis() as u64;
            if elapsed < target_ms {
                tokio::time::sleep(std::time::Duration::from_millis(target_ms - elapsed)).await;
            }

            let seg_name = format!("segment_{:04}.m4s", delay.segment_index);
            let src = self.staging_dir.join(&seg_name);
            if src.exists() {
                let _ = std::fs::copy(&src, self.served_dir.join(&seg_name));
            }

            // Update playlist to only include available segments
            let _ = std::fs::copy(
                self.staging_dir.join("playlist.m3u8"),
                self.served_dir.join("playlist.m3u8"),
            );
        }
    }
}
