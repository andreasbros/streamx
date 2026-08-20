/// Integration tests for the HLS pipeline (HlsManager + TranscodePipeline).
/// Tests the full flow: start_stream -> generate_playlist -> get_segment.
/// Segments are fMP4 (.m4s) with an init.mp4 init segment.
mod common;

use common::*;
use std::path::PathBuf;
use streamx::config::TranscodeConfig;
use streamx::transcode::hls::PlaylistResponse;
use streamx::transcode::HlsManager;

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

async fn create_hls_manager(name: &str) -> (HlsManager, PathBuf) {
    let cache_dir = test_output_dir(&format!("hls_{name}"));
    let config = test_transcode_config();
    let manager = HlsManager::new(&config, cache_dir.clone())
        .await
        .expect("create HlsManager");
    (manager, cache_dir)
}

// ============================================================
// H.264 passthrough (browser-compatible source)
// ============================================================

#[tokio::test]
async fn passthrough_h264_mp4() {
    let clip = h264_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP: fixture not generated");
        return;
    }
    let (mgr, cache_dir) = create_hls_manager("passthrough_h264").await;
    let stream_id = "test_passthrough";

    mgr.start_stream(stream_id, clip.to_str().unwrap(), "source")
        .await
        .expect("start_stream");

    // Wait for FFmpeg to produce segments
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let resp = mgr
        .generate_playlist(stream_id, "source")
        .await
        .expect("playlist");
    match resp {
        PlaylistResponse::Content(content) => {
            assert!(content.contains("#EXTM3U"), "Missing EXTM3U");
            assert!(content.contains("#EXTINF:"), "No segments in playlist");
            // Passthrough uses flat playlist (no quality prefix)
            for line in content.lines() {
                if !line.starts_with('#') && !line.is_empty() {
                    assert!(
                        !line.contains('/'),
                        "Passthrough should have bare filenames: {line}"
                    );
                }
            }
        }
        PlaylistResponse::Redirect(_) => panic!("Expected content, got redirect"),
    }

    // Verify segment can be fetched (fMP4 .m4s)
    let seg = mgr
        .get_segment(stream_id, "segment_0000.m4s")
        .await
        .expect("get_segment");
    assert!(seg.is_some(), "Segment 0 missing");
    let data = seg.unwrap();
    assert!(data.len() > 8, "Segment too small for fMP4 box header");
    let box_type = &data[4..8];
    assert!(
        box_type == b"styp" || box_type == b"moof" || box_type == b"ftyp",
        "Invalid fMP4 box type: {:?}",
        std::str::from_utf8(box_type).unwrap_or("<non-utf8>")
    );

    mgr.cleanup(stream_id).await.expect("cleanup");
    assert!(!cache_dir.join(stream_id).exists(), "Cache not cleaned up");
}

// ============================================================
// HEVC transcode (needs transcoding for browser)
// ============================================================

#[tokio::test]
async fn transcode_hevc_source_copies_video() {
    let clip = hevc_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP: fixture not generated");
        return;
    }
    let (mgr, cache_dir) = create_hls_manager("hevc_source_copy").await;
    let stream_id = "test_hevc_source";

    mgr.start_stream(stream_id, clip.to_str().unwrap(), "source")
        .await
        .expect("start_stream");

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let resp = mgr
        .generate_playlist(stream_id, "source")
        .await
        .expect("playlist");
    match resp {
        PlaylistResponse::Content(content) => {
            assert!(content.contains("#EXTM3U"));
            assert!(content.contains("#EXTINF:"));
            // Transcoded variant: segment paths should include quality prefix
            for line in content.lines() {
                if !line.starts_with('#') && !line.is_empty() {
                    assert!(
                        line.starts_with("source/"),
                        "Missing quality prefix: {line}"
                    );
                }
            }
        }
        PlaylistResponse::Redirect(_) => panic!("Expected content, got redirect"),
    }

    // Verify variant segment (fMP4 .m4s)
    let seg = mgr
        .get_variant_segment(stream_id, "source", "segment_0000.m4s")
        .await
        .expect("get_variant_segment");
    assert!(seg.is_some(), "Variant segment missing");
    let data = seg.unwrap();
    assert!(data.len() > 8, "Segment too small for fMP4 box header");
    let box_type = &data[4..8];
    assert!(
        box_type == b"styp" || box_type == b"moof" || box_type == b"ftyp",
        "Invalid fMP4 box type: {:?}",
        std::str::from_utf8(box_type).unwrap_or("<non-utf8>")
    );

    // Verify segment cache works (second call returns same data)
    let seg2 = mgr
        .get_variant_segment(stream_id, "source", "segment_0000.m4s")
        .await
        .expect("cached get");
    assert_eq!(
        seg2.unwrap().len(),
        data.len(),
        "Cache returned different size"
    );
}

#[tokio::test]
async fn transcode_hevc_720p() {
    let clip = hevc_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP: fixture not generated");
        return;
    }
    let (mgr, _cache_dir) = create_hls_manager("hevc_720p").await;
    let stream_id = "test_hevc_720p";

    mgr.start_stream(stream_id, clip.to_str().unwrap(), "720p")
        .await
        .expect("start_stream 720p");

    tokio::time::sleep(std::time::Duration::from_secs(8)).await;

    let resp = mgr
        .generate_playlist(stream_id, "720p")
        .await
        .expect("playlist");
    match resp {
        PlaylistResponse::Content(content) => {
            assert!(content.contains("#EXTINF:"), "No segments");
            for line in content.lines() {
                if !line.starts_with('#') && !line.is_empty() {
                    assert!(line.starts_with("720p/"), "Wrong prefix: {line}");
                }
            }
        }
        _ => panic!("Expected content"),
    }

    let seg = mgr
        .get_variant_segment(stream_id, "720p", "segment_0000.m4s")
        .await
        .expect("get segment");
    assert!(seg.is_some(), "720p segment missing");
}

// ============================================================
// Quality switching creates separate directories
// ============================================================

#[tokio::test]
async fn quality_switching_separate_dirs() {
    let clip = hevc_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP: fixture not generated");
        return;
    }
    let (mgr, cache_dir) = create_hls_manager("quality_switch").await;
    let stream_id = "test_switch";

    // Start source quality
    mgr.start_stream(stream_id, clip.to_str().unwrap(), "source")
        .await
        .expect("start source");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Start 360p quality (different tier)
    mgr.start_stream(stream_id, clip.to_str().unwrap(), "360p")
        .await
        .expect("start 360p");
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

    // Both directories should exist
    let source_dir = cache_dir.join(stream_id).join("source");
    let dir_360 = cache_dir.join(stream_id).join("360p");
    assert!(source_dir.exists(), "source dir missing");
    assert!(dir_360.exists(), "360p dir missing");

    // Both should have playlists
    assert!(
        source_dir.join("playlist.m3u8").exists(),
        "source playlist missing"
    );
    assert!(
        dir_360.join("playlist.m3u8").exists(),
        "360p playlist missing"
    );
}

// ============================================================
// Cache detection (second request skips transcode)
// ============================================================

#[tokio::test]
async fn cache_hit_skips_transcode() {
    let clip = h264_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP: fixture not generated");
        return;
    }
    let (mgr, _) = create_hls_manager("cache_hit").await;
    let stream_id = "test_cache";

    // First call starts transcode
    mgr.start_stream(stream_id, clip.to_str().unwrap(), "source")
        .await
        .expect("first start");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Verify it produced segments
    let resp = mgr
        .generate_playlist(stream_id, "source")
        .await
        .expect("playlist");
    match &resp {
        PlaylistResponse::Content(c) => assert!(c.contains("#EXTINF:")),
        _ => panic!("Expected content"),
    }

    // Second call should return immediately (cache hit)
    let start = std::time::Instant::now();
    mgr.start_stream(stream_id, clip.to_str().unwrap(), "source")
        .await
        .expect("second start");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "Cache hit took too long: {elapsed:?}"
    );
}

// ============================================================
// Demo stream returns redirect
// ============================================================

#[tokio::test]
async fn demo_stream_redirects() {
    let (mgr, _) = create_hls_manager("demo").await;

    let resp = mgr
        .generate_playlist("demo", "source")
        .await
        .expect("demo playlist");
    match resp {
        PlaylistResponse::Redirect(url) => {
            assert!(
                url.contains("test-streams.mux.dev"),
                "Unexpected redirect URL: {url}"
            );
        }
        PlaylistResponse::Content(_) => panic!("Expected redirect for demo"),
    }
}

// ============================================================
// Cleanup removes cache directory
// ============================================================

#[tokio::test]
async fn cleanup_removes_cache() {
    let clip = h264_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP: fixture not generated");
        return;
    }
    let (mgr, cache_dir) = create_hls_manager("cleanup_test").await;
    let stream_id = "test_cleanup";

    mgr.start_stream(stream_id, clip.to_str().unwrap(), "source")
        .await
        .expect("start");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    assert!(
        cache_dir.join(stream_id).exists(),
        "Cache dir should exist before cleanup"
    );
    mgr.cleanup(stream_id).await.expect("cleanup");
    assert!(
        !cache_dir.join(stream_id).exists(),
        "Cache dir should be gone after cleanup"
    );
}

// ============================================================
// Active streams reporting
// ============================================================

#[tokio::test]
async fn active_streams_reports_running() {
    let clip = hevc_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP: fixture not generated");
        return;
    }
    let (mgr, _) = create_hls_manager("active_report").await;
    let stream_id = "test_active";

    mgr.start_stream(stream_id, clip.to_str().unwrap(), "source")
        .await
        .expect("start");

    // Give FFmpeg a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let streams = mgr.active_streams().await;
    // Should have at least one entry (either active or from cache scan)
    // The running transcode should show up
    let found = streams.iter().any(|s| s.stream_id == stream_id);
    assert!(
        found,
        "Stream not in active list: {:?}",
        streams.iter().map(|s| &s.stream_id).collect::<Vec<_>>()
    );
}

// ============================================================
// Growing file simulation (mock torrent download)
// ============================================================

#[tokio::test]
async fn growing_file_produces_segments() {
    let source = hevc_720p_clip();
    if !source.exists() {
        eprintln!("SKIP: fixture not generated");
        return;
    }
    let (mgr, cache_dir) = create_hls_manager("growing_file").await;
    let stream_id = "test_growing";

    // Simulate a growing file: use append-style writes (like a torrent sequential download)
    let growing_path = cache_dir.join("growing_input.mp4");
    let source_data = std::fs::read(&source).expect("read source");

    // Write the complete file (FFmpeg reads from a complete-on-disk file that grew sequentially)
    // In real torrent scenario, the file is pre-allocated and pieces fill in sequentially.
    // For this test, we write the first half, start transcode, then complete the file.
    let half = source_data.len() / 2;
    std::fs::write(&growing_path, &source_data[..half]).expect("write first half");

    // Start transcode (FFmpeg will read what's available)
    mgr.start_stream(stream_id, growing_path.to_str().unwrap(), "source")
        .await
        .expect("start on partial");

    // Complete the file after a delay (simulates torrent finishing)
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    std::fs::write(&growing_path, &source_data).expect("write complete");

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // The transcode from the first half should have produced some segments
    // Even if the second half wasn't read, the first half has enough data
    let resp = mgr
        .generate_playlist(stream_id, "source")
        .await
        .expect("playlist");
    match resp {
        PlaylistResponse::Content(content) => {
            assert!(content.contains("#EXTM3U"), "Missing header");
        }
        _ => panic!("Expected content"),
    }
}

// ============================================================
// Segment integrity check (corrupt fMP4 detection)
// ============================================================

#[tokio::test]
async fn corrupt_segment_returns_none() {
    let (mgr, cache_dir) = create_hls_manager("corrupt_seg").await;
    let stream_id = "test_corrupt";

    // Create a fake corrupt fMP4 segment
    let seg_dir = cache_dir.join(stream_id).join("source");
    std::fs::create_dir_all(&seg_dir).expect("create dir");
    std::fs::write(seg_dir.join("segment_0000.m4s"), b"NOT_A_VALID_FMP4_FILE")
        .expect("write corrupt");

    // Requesting it should return None (corrupt detected and deleted)
    let result = mgr
        .get_variant_segment(stream_id, "source", "segment_0000.m4s")
        .await
        .expect("get corrupt");
    assert!(result.is_none(), "Corrupt segment should return None");
    assert!(
        !seg_dir.join("segment_0000.m4s").exists(),
        "Corrupt segment should be deleted"
    );
}
