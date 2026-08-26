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
        hls_max_height: 1080,
        hls_force_stereo: true,
    }
}

async fn create_mgr(name: &str) -> (HlsManager, PathBuf) {
    create_mgr_with(name, test_config()).await
}

async fn create_mgr_with(name: &str, config: TranscodeConfig) -> (HlsManager, PathBuf) {
    let cache_dir = test_output_dir(&format!("kill_{name}"));
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
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=duration=60:size=1920x1080:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=60",
            "-c:v",
            "libx265",
            "-preset",
            "ultrafast",
            "-crf",
            "32",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-ac",
            "2",
            "-f",
            "matroska",
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

#[cfg(unix)]
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

/// No pgrep on Windows; match command lines via CIM. The fragment goes
/// through an env var so path backslashes never meet shell quoting.
#[cfg(windows)]
fn count_ffmpeg(path_fragment: &str) -> usize {
    let script = "@(Get-CimInstance Win32_Process -Filter \"Name='ffmpeg.exe'\" | \
                  Where-Object { $_.CommandLine -like ('*' + $env:SX_FRAG + '*') }).Count";
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .env("SX_FRAG", path_fragment)
        .output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse()
            .unwrap_or(0),
        Err(_) => 0,
    }
}

/// Poll the FFmpeg process count until `pred` holds or `deadline`
/// passes. Fixed sleeps made these tests flaky under full parallel
/// load, where probing and spawning take longer than a few seconds.
async fn wait_for_ffmpeg(
    path_fragment: &str,
    deadline: std::time::Duration,
    pred: impl Fn(usize) -> bool,
) -> usize {
    let start = std::time::Instant::now();
    loop {
        let n = count_ffmpeg(path_fragment);
        if pred(n) || start.elapsed() >= deadline {
            return n;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

const SPAWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);
const KILL_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);
/// Watchdog: 30s idle threshold + 10s check interval, plus headroom.
const WATCHDOG_DEADLINE: std::time::Duration = std::time::Duration::from_secs(75);

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

    let before = wait_for_ffmpeg(&path_frag, SPAWN_DEADLINE, |n| n > 0).await;
    eprintln!("FFmpeg before drop: {before}");
    assert!(before > 0, "FFmpeg should be running");

    drop(mgr);
    let after = wait_for_ffmpeg(&path_frag, KILL_DEADLINE, |n| n == 0).await;
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

    let n1 = wait_for_ffmpeg(&path_frag, SPAWN_DEADLINE, |n| n > 0).await;
    eprintln!("FFmpeg after 1080p start: {n1}");
    assert!(n1 > 0, "1080p FFmpeg should be running");

    // Switch to 720p - should kill 1080p
    mgr.start_stream(stream_id, clip.to_str().unwrap(), "720p")
        .await
        .expect("start 720p");

    let n2 = wait_for_ffmpeg(&path_frag, KILL_DEADLINE, |n| n <= 1).await;
    eprintln!("FFmpeg after switch to 720p: {n2}");
    assert!(n2 <= 1, "Old 1080p FFmpeg should be killed, got {n2}");

    drop(mgr);
    let n3 = wait_for_ffmpeg(&path_frag, KILL_DEADLINE, |n| n == 0).await;
    assert_eq!(n3, 0, "All FFmpeg dead after drop");
}

#[tokio::test]
async fn watchdog_kills_idle() {
    let clip = long_hevc_clip();
    if !clip.exists() {
        eprintln!("SKIP: clip not generated");
        return;
    }
    // The 60s clip transcodes to completion in a few seconds at
    // `ultrafast`, which would end FFmpeg before the 30s idle
    // threshold and prove nothing. A single-threaded `veryslow` encode
    // keeps FFmpeg busy well past the watchdog window.
    let mut config = test_config();
    config.preset = "veryslow".to_string();
    config.threads = Some(1);
    let (mgr, cache) = create_mgr_with("watchdog", config).await;
    let stream_id = "test_watchdog";
    let path_frag = cache.join(stream_id).to_string_lossy().to_string();

    mgr.start_stream(stream_id, clip.to_str().unwrap(), "720p")
        .await
        .expect("start_stream");

    let before = wait_for_ffmpeg(&path_frag, SPAWN_DEADLINE, |n| n > 0).await;
    eprintln!("FFmpeg before idle: {before}");
    assert!(before > 0, "FFmpeg should be running");
    let spawned_at = std::time::Instant::now();

    // Still alive well inside the idle window: the encode is genuinely
    // long-running, so a later exit can only be the watchdog.
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    assert!(
        count_ffmpeg(&path_frag) > 0,
        "encode finished too early to exercise the watchdog"
    );

    // No playlist or segment is requested from here on, so the stream
    // goes idle and the watchdog must reap FFmpeg on its own.
    eprintln!("Waiting for the idle watchdog...");
    let after = wait_for_ffmpeg(&path_frag, WATCHDOG_DEADLINE, |n| n == 0).await;
    let reaped_after = spawned_at.elapsed();
    eprintln!("FFmpeg after idle: {after} (reaped after {reaped_after:?})");
    assert_eq!(after, 0, "Watchdog should have killed idle FFmpeg");
    assert!(
        reaped_after >= std::time::Duration::from_secs(30),
        "FFmpeg exited before the 30s idle threshold, so the watchdog was not what stopped it"
    );

    drop(mgr);
}
