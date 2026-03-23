/// Integration tests for FFmpeg HLS transcoding pipeline.
/// Uses an HEVC 4K 10-bit test file to validate all quality/encoder combinations.
/// Place a test file at ~/.streamx/downloads/complete/test-hevc-4k-10bit.mkv
use std::path::{Path, PathBuf};
use std::process::Command;

fn test_file() -> Option<PathBuf> {
    let candidates = [
        dirs::home_dir()
            .unwrap_or_default()
            .join(".streamx/downloads/complete/test-hevc-4k-10bit.mkv"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn has_vaapi() -> bool {
    Path::new("/dev/dri/renderD128").exists()
}

fn run_ffmpeg(args: &[&str], output: &Path) -> (bool, String) {
    let result = Command::new("ffmpeg")
        .args(args)
        .arg(output.to_str().unwrap_or(""))
        .output();

    match result {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            (out.status.success(), stderr)
        }
        Err(e) => (false, format!("Failed to run ffmpeg: {e}")),
    }
}

fn count_segments(playlist: &Path) -> usize {
    std::fs::read_to_string(playlist)
        .unwrap_or_default()
        .matches("EXTINF:")
        .count()
}

fn has_endlist(playlist: &Path) -> bool {
    std::fs::read_to_string(playlist)
        .unwrap_or_default()
        .contains("EXT-X-ENDLIST")
}

fn is_valid_ts(segment: &Path) -> bool {
    if let Ok(data) = std::fs::read(segment) {
        !data.is_empty() && data[0] == 0x47 && (data.len() < 188 || data[188] == 0x47)
    } else {
        false
    }
}

fn setup_output_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("streamx_transcode_tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ============================================================
// Source quality: HEVC video copy (no re-encode)
// ============================================================

#[test]
fn source_hevc_copy() {
    let file = match test_file() {
        Some(f) => f,
        None => { eprintln!("SKIP: test file not found"); return; }
    };
    let dir = setup_output_dir("source_hevc_copy");
    let playlist = dir.join("playlist.m3u8");
    let seg_pattern = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", file.to_str().unwrap(), "-t", "10",
        "-c:v", "copy",
        "-c:a", "aac", "-b:a", "320k",
        "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts",
        "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg_pattern.to_str().unwrap(),
    ], &playlist);

    assert!(ok, "HEVC copy failed: {stderr}");
    assert!(count_segments(&playlist) >= 2, "Too few segments");
    assert!(has_endlist(&playlist), "Missing EXT-X-ENDLIST");
    assert!(is_valid_ts(&dir.join("segment_0000.ts")), "Invalid TS segment");
}

// ============================================================
// CPU libx264 at various resolutions
// ============================================================

#[test]
fn cpu_1080p() {
    let file = match test_file() {
        Some(f) => f,
        None => { eprintln!("SKIP: test file not found"); return; }
    };
    let dir = setup_output_dir("cpu_1080p");
    let playlist = dir.join("playlist.m3u8");
    let seg_pattern = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", file.to_str().unwrap(), "-t", "5",
        "-c:v", "libx264", "-preset", "fast", "-crf", "20", "-tune", "film", "-threads", "2",
        "-vf", "scale=-2:1080",
        "-maxrate", "5000k", "-bufsize", "10000k",
        "-c:a", "aac", "-b:a", "256k",
        "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts",
        "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg_pattern.to_str().unwrap(),
    ], &playlist);

    assert!(ok, "CPU 1080p failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "Too few segments");
    assert!(is_valid_ts(&dir.join("segment_0000.ts")), "Invalid TS segment");
}

#[test]
fn cpu_720p() {
    let file = match test_file() {
        Some(f) => f,
        None => { eprintln!("SKIP: test file not found"); return; }
    };
    let dir = setup_output_dir("cpu_720p");
    let playlist = dir.join("playlist.m3u8");
    let seg_pattern = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", file.to_str().unwrap(), "-t", "5",
        "-c:v", "libx264", "-preset", "fast", "-crf", "20", "-tune", "film", "-threads", "2",
        "-vf", "scale=-2:720",
        "-maxrate", "2500k", "-bufsize", "5000k",
        "-c:a", "aac", "-b:a", "192k",
        "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts",
        "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg_pattern.to_str().unwrap(),
    ], &playlist);

    assert!(ok, "CPU 720p failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "Too few segments");
    assert!(is_valid_ts(&dir.join("segment_0000.ts")), "Invalid TS segment");
}

#[test]
fn cpu_360p() {
    let file = match test_file() {
        Some(f) => f,
        None => { eprintln!("SKIP: test file not found"); return; }
    };
    let dir = setup_output_dir("cpu_360p");
    let playlist = dir.join("playlist.m3u8");
    let seg_pattern = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", file.to_str().unwrap(), "-t", "5",
        "-c:v", "libx264", "-preset", "fast", "-crf", "22", "-tune", "film", "-threads", "2",
        "-vf", "scale=-2:360",
        "-maxrate", "800k", "-bufsize", "1600k",
        "-c:a", "aac", "-b:a", "128k",
        "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts",
        "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg_pattern.to_str().unwrap(),
    ], &playlist);

    assert!(ok, "CPU 360p failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "Too few segments");
    assert!(is_valid_ts(&dir.join("segment_0000.ts")), "Invalid TS segment");
}

// ============================================================
// VAAPI hybrid: CPU decode + GPU encode
// ============================================================

#[test]
fn vaapi_hybrid_1080p() {
    if !has_vaapi() {
        eprintln!("SKIP: no VAAPI device");
        return;
    }
    let file = match test_file() {
        Some(f) => f,
        None => { eprintln!("SKIP: test file not found"); return; }
    };
    let dir = setup_output_dir("vaapi_hybrid_1080p");
    let playlist = dir.join("playlist.m3u8");
    let seg_pattern = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-init_hw_device", "vaapi=va:/dev/dri/renderD128",
        "-filter_hw_device", "va",
        "-i", file.to_str().unwrap(), "-t", "5",
        "-vf", "scale=-2:1080,format=nv12,hwupload",
        "-c:v", "h264_vaapi", "-global_quality", "20",
        "-c:a", "aac", "-b:a", "256k",
        "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts",
        "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg_pattern.to_str().unwrap(),
    ], &playlist);

    assert!(ok, "VAAPI hybrid 1080p failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "Too few segments");
}

#[test]
fn vaapi_hybrid_720p() {
    if !has_vaapi() {
        eprintln!("SKIP: no VAAPI device");
        return;
    }
    let file = match test_file() {
        Some(f) => f,
        None => { eprintln!("SKIP: test file not found"); return; }
    };
    let dir = setup_output_dir("vaapi_hybrid_720p");
    let playlist = dir.join("playlist.m3u8");
    let seg_pattern = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-init_hw_device", "vaapi=va:/dev/dri/renderD128",
        "-filter_hw_device", "va",
        "-i", file.to_str().unwrap(), "-t", "5",
        "-vf", "scale=-2:720,format=nv12,hwupload",
        "-c:v", "h264_vaapi", "-global_quality", "20",
        "-c:a", "aac", "-b:a", "192k",
        "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts",
        "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg_pattern.to_str().unwrap(),
    ], &playlist);

    assert!(ok, "VAAPI hybrid 720p failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "Too few segments");
}

// ============================================================
// VAAPI full hardware (expected to FAIL on HEVC 10-bit input)
// ============================================================

#[test]
fn vaapi_full_hw_fails_on_hevc_10bit() {
    if !has_vaapi() {
        eprintln!("SKIP: no VAAPI device");
        return;
    }
    let file = match test_file() {
        Some(f) => f,
        None => { eprintln!("SKIP: test file not found"); return; }
    };
    let dir = setup_output_dir("vaapi_full_hw_fail");
    let playlist = dir.join("playlist.m3u8");
    let seg_pattern = dir.join("segment_%04d.ts");

    let (ok, _stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-hwaccel", "vaapi",
        "-hwaccel_device", "/dev/dri/renderD128",
        "-hwaccel_output_format", "vaapi",
        "-i", file.to_str().unwrap(), "-t", "5",
        "-vf", "scale_vaapi=w=-2:h=1080:format=nv12",
        "-c:v", "h264_vaapi", "-global_quality", "20",
        "-c:a", "aac", "-b:a", "256k",
        "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts",
        "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg_pattern.to_str().unwrap(),
    ], &playlist);

    // This SHOULD fail on hardware that can't decode HEVC 10-bit
    assert!(!ok, "Expected VAAPI full HW to fail on HEVC 10-bit 4K, but it succeeded");
}

mod dirs {
    pub fn home_dir() -> Option<std::path::PathBuf> {
        std::env::var("HOME").ok().map(std::path::PathBuf::from)
    }
}
