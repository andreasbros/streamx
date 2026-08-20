pub mod fixtures;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

static FIXTURE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Directory containing test video fixtures.
/// Uses a fixed path to survive across nix shell invocations.
pub fn fixture_dir() -> &'static Path {
    FIXTURE_DIR.get_or_init(|| {
        let dir = PathBuf::from("/tmp/streamx_test_fixtures");
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        generate_fixtures(&dir);
        dir
    })
}

/// 10-second 720p H.264+AAC test clip (browser-compatible, ~2MB)
pub fn h264_720p_clip() -> PathBuf {
    fixture_dir().join("test_h264_720p.mp4")
}

/// 10-second 720p HEVC+AAC test clip in MKV (needs transcode, ~1MB)
pub fn hevc_720p_clip() -> PathBuf {
    fixture_dir().join("test_hevc_720p.mkv")
}

/// 5-second 4K HEVC 10-bit clip from real test file (if available)
pub fn hevc_4k_clip() -> Option<PathBuf> {
    let src = dirs::home_dir()
        .unwrap_or_default()
        .join(".streamx/downloads/complete/test-hevc-4k-10bit.mkv");
    if !src.exists() {
        return None;
    }
    let dst = fixture_dir().join("test_hevc_4k_clip.mkv");
    if dst.exists() {
        return Some(dst);
    }
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            src.to_str().unwrap_or(""),
            "-t",
            "5",
            "-c",
            "copy",
        ])
        .arg(dst.to_str().unwrap_or(""))
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        Some(dst)
    } else {
        None
    }
}

fn generate_fixtures(dir: &Path) {
    // H.264 720p MP4 (browser-compatible)
    let h264 = dir.join("test_h264_720p.mp4");
    if !h264.exists() {
        let _ = Command::new("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=10:size=1280x720:rate=24",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=10",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "28",
                "-c:a",
                "aac",
                "-b:a",
                "64k",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&h264)
            .status();
    }

    // HEVC 720p MKV (needs transcode for browser)
    let hevc = dir.join("test_hevc_720p.mkv");
    if !hevc.exists() {
        let _ = Command::new("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=10:size=1280x720:rate=24",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=10",
                "-c:v",
                "libx265",
                "-preset",
                "ultrafast",
                "-crf",
                "32",
                "-c:a",
                "aac",
                "-b:a",
                "64k",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&hevc)
            .status();
    }
}

/// Check MPEG-TS segment validity (sync byte 0x47 every 188 bytes)
pub fn is_valid_ts(path: &Path) -> bool {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return false,
    };
    if data.is_empty() || data[0] != 0x47 {
        return false;
    }
    let offsets = [0, 188, 376];
    offsets.iter().all(|&o| o >= data.len() || data[o] == 0x47)
}

/// Check fMP4/CMAF segment validity (ISO BMFF box structure)
pub fn is_valid_fmp4(path: &Path) -> bool {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return false,
    };
    if data.len() < 8 {
        return false;
    }
    let box_type = &data[4..8];
    // Valid box types for fMP4 init and media segments
    [
        b"ftyp", b"moov", b"moof", b"styp", b"sidx", b"mdat", b"free",
    ]
    .iter()
    .any(|t| box_type == *t)
}

/// Count EXTINF entries in an HLS playlist
pub fn count_segments(playlist: &Path) -> usize {
    std::fs::read_to_string(playlist)
        .unwrap_or_default()
        .matches("EXTINF:")
        .count()
}

/// Check if playlist has EXT-X-ENDLIST
pub fn has_endlist(playlist: &Path) -> bool {
    std::fs::read_to_string(playlist)
        .unwrap_or_default()
        .contains("EXT-X-ENDLIST")
}

/// Check if playlist segment paths include quality prefix
pub fn segments_have_prefix(playlist: &Path, prefix: &str) -> bool {
    let content = std::fs::read_to_string(playlist).unwrap_or_default();
    content
        .lines()
        .any(|line| !line.starts_with('#') && !line.is_empty() && line.starts_with(prefix))
}

/// Run FFmpeg with args, return (success, stderr)
pub fn run_ffmpeg(args: &[&str]) -> (bool, String) {
    match Command::new("ffmpeg").args(args).output() {
        Ok(out) => (
            out.status.success(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        ),
        Err(e) => (false, format!("spawn failed: {e}")),
    }
}

/// Create a temporary output directory for a test
pub fn test_output_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from("/tmp/streamx_test_output").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test output dir");
    dir
}

/// Check if VAAPI device is available AND working
pub fn has_vaapi() -> bool {
    if !Path::new("/dev/dri/renderD128").exists() {
        return false;
    }
    // Quick test: try to init VAAPI device
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-init_hw_device",
            "vaapi=va:/dev/dri/renderD128",
            "-f",
            "lavfi",
            "-i",
            "nullsrc=s=64x64:d=0.1",
            "-vf",
            "format=nv12,hwupload",
            "-c:v",
            "h264_vaapi",
            "-frames:v",
            "1",
            "-f",
            "null",
            "/dev/null",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub mod dirs {
    pub fn home_dir() -> Option<std::path::PathBuf> {
        std::env::var("HOME").ok().map(std::path::PathBuf::from)
    }
}
