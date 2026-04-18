/// Tests that FFmpeg processes are killed when TranscodeHandle is dropped.
mod common;

use common::*;
use std::path::PathBuf;
use streamx::config::TranscodeConfig;
use streamx::transcode::HlsManager;

fn test_config() -> TranscodeConfig {
    TranscodeConfig {
        hls_segment_duration: 2,
        video_codec: "h264".to_string(),
        audio_codec: "aac".to_string(),
        preset: "ultrafast".to_string(),
        max_concurrent_transcodes: 4,
        crf: 28,
        max_bitrate: None,
        audio_bitrate: "128k".to_string(),
        threads: Some(1),
        gpu: false,
        hls_downscale: true,
        hls_max_height: 1080, hls_force_stereo: true,
    }
}

async fn create_mgr(name: &str) -> (HlsManager, PathBuf) {
    let cache_dir = test_output_dir(&format!("kill_{name}"));
    let config = test_config();
    let mgr = HlsManager::new(&config, cache_dir.clone())
        .await
        .expect("create HlsManager");
    (mgr, cache_dir)
}

/// Generate a 60s HEVC clip that takes long enough to transcode
fn long_hevc_clip() -> PathBuf {
    let path = PathBuf::from("/tmp/streamx_test_clips/hevc_long_60s.mkv");
    if path.exists() {
        return path;
    }
    std::fs::create_dir_all(path.parent().unwrap()).ok();
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y", "-hide_banner", "-loglevel", "error",
            "-f", "lavfi", "-i", "testsrc2=duration=60:size=1920x1080:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=60",
            "-c:v", "libx265", "-preset", "ultrafast", "-crf", "32",
            "-c:a", "aac", "-b:a", "128k", "-ac", "2",
            "-f", "matroska",
        ])
        .arg(&path)
        .status();
    if status.map(|s| s.success()).unwrap_or(false) {
        eprintln!(
            "Generated 60s HEVC clip: {} bytes",
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        );
    }
    path
}

fn count_ffmpeg(path_fragment: &str) -> usize {
    let output = std::process::Command::new("pgrep")
        .args(["-f", &format!("ffmpeg.*{path_fragment}")])
        .output();
    match output {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.lines().filter(|l| !l.is_empty()).count()
        }
        Err(_) => 0,
    }
}

#[tokio::test]
async fn drop_handle_kills_ffmpeg() {
    let clip = long_hevc_clip();
    if !clip.exists() {
        eprintln!("SKIP: clip not generated");
        return;
    }
    let (mgr, cache) = create_mgr("drop_kill").await;
    let stream_id = "test_drop_kill";
    let path_frag = cache.join(stream_id).to_string_lossy().to_string();

    mgr.start_stream(stream_id, clip.to_str().unwrap(), "720p")
        .await
        .expect("start_stream");

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let before = count_ffmpeg(&path_frag);
    eprintln!("FFmpeg before drop: {before}");
    assert!(before > 0, "FFmpeg should be running");

    drop(mgr);
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let after = count_ffmpeg(&path_frag);
    eprintln!("FFmpeg after drop: {after}");
    assert_eq!(after, 0, "FFmpeg should be killed on drop");
}

#[tokio::test]
async fn quality_switch_kills_previous() {
    let clip = long_hevc_clip();
    if !clip.exists() {
        eprintln!("SKIP: clip not generated");
        return;
    }
    let (mgr, cache) = create_mgr("qswitch").await;
    let stream_id = "test_qswitch";
    let path_frag = cache.join(stream_id).to_string_lossy().to_string();

    // Start 1080p (slow transcode - HEVC->H.264)
    mgr.start_stream(stream_id, clip.to_str().unwrap(), "1080p")
        .await
        .expect("start 1080p");

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let n1 = count_ffmpeg(&path_frag);
    eprintln!("FFmpeg after 1080p start: {n1}");
    assert!(n1 > 0, "1080p FFmpeg should be running");

    // Switch to 720p - should kill 1080p
    mgr.start_stream(stream_id, clip.to_str().unwrap(), "720p")
        .await
        .expect("start 720p");

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let n2 = count_ffmpeg(&path_frag);
    eprintln!("FFmpeg after switch to 720p: {n2}");
    assert!(n2 <= 1, "Old 1080p FFmpeg should be killed, got {n2}");

    drop(mgr);
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert_eq!(count_ffmpeg(&path_frag), 0, "All FFmpeg dead after drop");
}

#[tokio::test]
async fn watchdog_kills_idle() {
    let clip = long_hevc_clip();
    if !clip.exists() {
        eprintln!("SKIP: clip not generated");
        return;
    }
    let (mgr, cache) = create_mgr("watchdog").await;
    let stream_id = "test_watchdog";
    let path_frag = cache.join(stream_id).to_string_lossy().to_string();

    mgr.start_stream(stream_id, clip.to_str().unwrap(), "720p")
        .await
        .expect("start_stream");

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let before = count_ffmpeg(&path_frag);
    eprintln!("FFmpeg before idle: {before}");
    assert!(before > 0, "FFmpeg should be running");

    // Wait for watchdog (30s idle + 10s check interval)
    eprintln!("Waiting 45s for watchdog...");
    tokio::time::sleep(std::time::Duration::from_secs(45)).await;

    let after = count_ffmpeg(&path_frag);
    eprintln!("FFmpeg after idle: {after}");
    assert_eq!(after, 0, "Watchdog should have killed idle FFmpeg");

    drop(mgr);
}
