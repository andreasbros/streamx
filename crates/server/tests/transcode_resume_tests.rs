/// Interrupted-transcode resume: a variant or passthrough cache whose
/// playlist lacks EXT-X-ENDLIST (FFmpeg crashed mid-run or the idle
/// watchdog stopped it) must be resumed from the last completed segment,
/// not served forever truncated.
mod common;

use common::*;
use std::path::{Path, PathBuf};
use streamx::config::TranscodeConfig;
use streamx::transcode::HlsManager;

const KEEP_SEGMENTS: usize = 12;

fn test_transcode_config() -> TranscodeConfig {
    TranscodeConfig {
        hls_segment_duration: 2,
        video_codec: "h264".to_string(),
        audio_codec: "aac".to_string(),
        preset: "ultrafast".to_string(),
        max_concurrent_transcodes: 2,
        crf: 28,
        max_bitrate: None,
        audio_bitrate: "128k".to_string(),
        threads: Some(2),
        gpu: false,
        hls_downscale: true,
        hls_max_height: 1080,
        hls_force_stereo: true,
    }
}

/// A clip long enough to leave >RESUME_MIN_SEGMENTS segments after the cut.
/// `browser_compatible` picks h264/mp4 (passthrough, ~2s source-keyframe
/// segments) vs mpeg4/mkv (transcode; x264 output keyframes land ~10s apart
/// so the clip must be much longer to produce enough segments).
fn long_clip(browser_compatible: bool, tag: &str) -> Option<PathBuf> {
    let (name, vcodec, secs) = if browser_compatible {
        ("resume_long_h264.mp4", "libx264", 44u32)
    } else {
        ("resume_long_mpeg4.mkv", "mpeg4", 240u32)
    };
    let path = test_output_dir(&format!("resume_clip_{tag}")).join(name);
    let dur = secs.to_string();
    let (ok, err) = run_ffmpeg(&[
        "-y",
        "-f",
        "lavfi",
        "-i",
        &format!("testsrc2=duration={dur}:size=640x360:rate=24"),
        "-f",
        "lavfi",
        "-i",
        &format!("sine=frequency=440:duration={dur}"),
        "-c:v",
        vcodec,
        "-g",
        "48",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-shortest",
        path.to_str()?,
    ]);
    if !ok {
        eprintln!("SKIP: could not generate clip: {err}");
        return None;
    }
    Some(path)
}

fn parse_extinf_sum(playlist: &str) -> f64 {
    playlist
        .lines()
        .filter_map(|l| l.strip_prefix("#EXTINF:"))
        .filter_map(|l| l.split(',').next())
        .filter_map(|d| d.trim().parse::<f64>().ok())
        .sum()
}

fn segment_uris(playlist: &str) -> Vec<String> {
    playlist
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(String::from)
        .collect()
}

/// Poll until the playlist on disk contains ENDLIST, touching the manager's
/// playlist endpoint each round so the idle watchdog keeps the run alive.
async fn wait_for_endlist(
    mgr: &HlsManager,
    stream_id: &str,
    quality: &str,
    playlist_path: &Path,
    timeout_secs: u64,
) -> Option<String> {
    for _ in 0..timeout_secs * 2 {
        let _ = mgr.generate_playlist(stream_id, quality).await;
        if let Ok(content) = tokio::fs::read_to_string(playlist_path).await {
            if content.contains("EXT-X-ENDLIST") {
                return Some(content);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    None
}

/// Truncate a completed cache so it looks like an interrupted transcode:
/// keep the first `KEEP_SEGMENTS` segments, drop ENDLIST, delete the rest.
/// `keep_endlist` mimics the idle watchdog, whose SIGTERM makes FFmpeg
/// finalize the truncated playlist with a premature ENDLIST.
async fn simulate_interruption_opts(
    seg_dir: &Path,
    playlist_path: &Path,
    keep_endlist: bool,
) -> usize {
    let content = tokio::fs::read_to_string(playlist_path)
        .await
        .expect("read playlist");
    let uris = segment_uris(&content);
    assert!(
        uris.len() > KEEP_SEGMENTS + 2,
        "clip produced only {} segments, need > {}",
        uris.len(),
        KEEP_SEGMENTS + 2
    );

    let mut kept = 0usize;
    let mut truncated = String::new();
    for line in content.lines() {
        if line.starts_with("#EXT-X-ENDLIST") {
            continue;
        }
        if !line.starts_with('#') && !line.is_empty() {
            kept += 1;
            if kept > KEEP_SEGMENTS {
                break;
            }
        }
        truncated.push_str(line);
        truncated.push('\n');
    }
    if keep_endlist {
        truncated.push_str("#EXT-X-ENDLIST\n");
    }
    tokio::fs::write(playlist_path, truncated)
        .await
        .expect("write truncated playlist");
    for uri in &uris[KEEP_SEGMENTS..] {
        let _ = tokio::fs::remove_file(seg_dir.join(uri)).await;
    }
    uris.len()
}

async fn simulate_interruption(seg_dir: &Path, playlist_path: &Path) -> usize {
    simulate_interruption_opts(seg_dir, playlist_path, false).await
}

async fn create_hls_manager(name: &str) -> (HlsManager, PathBuf) {
    let cache_dir = test_output_dir(&format!("hls_{name}"));
    let _ = tokio::fs::remove_dir_all(&cache_dir).await;
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .expect("create cache dir");
    let manager = HlsManager::new(&test_transcode_config(), cache_dir.clone())
        .await
        .expect("create HlsManager");
    (manager, cache_dir)
}

#[tokio::test]
async fn variant_transcode_resumes_after_interruption() {
    let Some(clip) = long_clip(false, "variant") else {
        return;
    };
    let (mgr, cache_dir) = create_hls_manager("resume_variant").await;
    let stream_id = "resume_variant";
    let quality = "360p";
    let tier_dir = cache_dir.join(stream_id).join(quality);
    let playlist_path = tier_dir.join("playlist.m3u8");

    mgr.start_stream(stream_id, clip.to_str().expect("clip path"), quality)
        .await
        .expect("initial start_stream");
    let full = wait_for_endlist(&mgr, stream_id, quality, &playlist_path, 120)
        .await
        .expect("initial transcode did not complete");
    let full_sum = parse_extinf_sum(&full);
    drop(mgr);

    let total = simulate_interruption(&tier_dir, &playlist_path).await;
    let first_seg = tier_dir.join("segment_0000.ts");
    let first_mtime = std::fs::metadata(&first_seg)
        .and_then(|m| m.modified())
        .expect("segment_0000 mtime");

    // Fresh manager simulates the state after a crash or app restart
    let mgr2 = HlsManager::new(&test_transcode_config(), cache_dir.clone())
        .await
        .expect("create second HlsManager");
    mgr2.start_stream(stream_id, clip.to_str().expect("clip path"), quality)
        .await
        .expect("resume start_stream");
    let resumed = wait_for_endlist(&mgr2, stream_id, quality, &playlist_path, 120)
        .await
        .expect("resumed transcode did not complete");

    let resumed_sum = parse_extinf_sum(&resumed);
    assert!(
        (resumed_sum - full_sum).abs() < 6.0,
        "resumed duration {resumed_sum:.1}s deviates from full transcode {full_sum:.1}s"
    );

    let uris = segment_uris(&resumed);
    assert!(
        uris.len() >= total - 2,
        "resumed playlist has {} segments, full had {}",
        uris.len(),
        total
    );
    let mut seen = std::collections::HashSet::new();
    for uri in &uris {
        assert!(seen.insert(uri), "duplicate segment {uri} in playlist");
        assert!(tier_dir.join(uri).exists(), "missing segment file {uri}");
    }

    // The head of the cache must have been reused, not re-encoded
    let mtime_after = std::fs::metadata(&first_seg)
        .and_then(|m| m.modified())
        .expect("segment_0000 mtime after resume");
    assert_eq!(first_mtime, mtime_after, "segment_0000 was re-encoded");
}

/// The idle watchdog SIGTERMs FFmpeg, which finalizes the playlist with a
/// premature ENDLIST. That cache must not be trusted as complete: on the
/// next view the transcode resumes and finishes the movie.
#[tokio::test]
async fn premature_endlist_cache_resumes_to_completion() {
    let Some(clip) = long_clip(false, "endlist") else {
        return;
    };
    let (mgr, cache_dir) = create_hls_manager("resume_endlist").await;
    let stream_id = "resume_endlist";
    let quality = "360p";
    let tier_dir = cache_dir.join(stream_id).join(quality);
    let playlist_path = tier_dir.join("playlist.m3u8");

    mgr.start_stream(stream_id, clip.to_str().expect("clip path"), quality)
        .await
        .expect("initial start_stream");
    let full = wait_for_endlist(&mgr, stream_id, quality, &playlist_path, 120)
        .await
        .expect("initial transcode did not complete");
    let full_sum = parse_extinf_sum(&full);
    drop(mgr);

    simulate_interruption_opts(&tier_dir, &playlist_path, true).await;

    let mgr2 = HlsManager::new(&test_transcode_config(), cache_dir.clone())
        .await
        .expect("create second HlsManager");
    mgr2.start_stream(stream_id, clip.to_str().expect("clip path"), quality)
        .await
        .expect("resume start_stream");
    // Wait until the playlist covers the full duration again, not merely
    // has ENDLIST (the truncated playlist already has one)
    let mut resumed = None;
    for _ in 0..240 {
        let _ = mgr2.generate_playlist(stream_id, quality).await;
        if let Ok(content) = tokio::fs::read_to_string(&playlist_path).await {
            if content.contains("EXT-X-ENDLIST")
                && (parse_extinf_sum(&content) - full_sum).abs() < 6.0
            {
                resumed = Some(content);
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let resumed = resumed.expect("premature-ENDLIST cache was not resumed to completion");
    for uri in segment_uris(&resumed) {
        assert!(tier_dir.join(&uri).exists(), "missing segment file {uri}");
    }
}

#[tokio::test]
async fn passthrough_resumes_after_interruption() {
    let Some(clip) = long_clip(true, "passthrough") else {
        return;
    };
    let (mgr, cache_dir) = create_hls_manager("resume_passthrough").await;
    let stream_id = "resume_passthrough";
    let stream_dir = cache_dir.join(stream_id);
    let playlist_path = stream_dir.join("playlist.m3u8");

    mgr.start_stream(stream_id, clip.to_str().expect("clip path"), "source")
        .await
        .expect("initial start_stream");
    let full = wait_for_endlist(&mgr, stream_id, "source", &playlist_path, 120)
        .await
        .expect("initial passthrough did not complete");
    let full_sum = parse_extinf_sum(&full);
    drop(mgr);

    simulate_interruption(&stream_dir, &playlist_path).await;

    let mgr2 = HlsManager::new(&test_transcode_config(), cache_dir.clone())
        .await
        .expect("create second HlsManager");
    mgr2.start_stream(stream_id, clip.to_str().expect("clip path"), "source")
        .await
        .expect("resume start_stream");
    let resumed = wait_for_endlist(&mgr2, stream_id, "source", &playlist_path, 120)
        .await
        .expect("resumed passthrough did not complete");

    // Copy-mode resume seeks to the nearest keyframe, so allow a wider margin
    let resumed_sum = parse_extinf_sum(&resumed);
    assert!(
        (resumed_sum - full_sum).abs() < 10.0,
        "resumed duration {resumed_sum:.1}s deviates from full passthrough {full_sum:.1}s"
    );
    for uri in segment_uris(&resumed) {
        assert!(stream_dir.join(&uri).exists(), "missing segment file {uri}");
    }
}
