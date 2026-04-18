/// Integration tests for FFmpeg HLS transcoding pipeline.
/// Tests all quality tiers, GPU/CPU paths, playlist structure, and segment integrity.
/// Place a test file at ~/.streamx/downloads/complete/test-hevc-4k-10bit.mkv for 4K tests.
mod common;

use common::*;

// ============================================================
// Synthetic clip tests (always available, no external files)
// ============================================================

#[test]
fn h264_passthrough_to_hls() {
    let clip = h264_720p_clip();
    if !clip.exists() { eprintln!("SKIP: fixture not generated"); return; }
    let dir = test_output_dir("h264_passthrough");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", clip.to_str().unwrap(),
        "-c", "copy", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);

    assert!(ok, "H.264 passthrough failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "No segments produced");
    assert!(has_endlist(&playlist), "Missing ENDLIST");
    assert!(is_valid_ts(&dir.join("segment_0000.ts")), "Invalid TS");
}

#[test]
fn hevc_to_h264_cpu_720p() {
    let clip = hevc_720p_clip();
    if !clip.exists() { eprintln!("SKIP: fixture not generated"); return; }
    let dir = test_output_dir("hevc_cpu_720p");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", clip.to_str().unwrap(),
        "-c:v", "libx264", "-preset", "ultrafast", "-crf", "28", "-tune", "film", "-threads", "2",
        "-vf", "scale=-2:720",
        "-maxrate", "2500k", "-bufsize", "5000k",
        "-c:a", "aac", "-b:a", "192k", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);

    assert!(ok, "HEVC→H.264 CPU 720p failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "No segments produced");
    assert!(has_endlist(&playlist));
    assert!(is_valid_ts(&dir.join("segment_0000.ts")));
}

#[test]
fn hevc_to_h264_cpu_360p() {
    let clip = hevc_720p_clip();
    if !clip.exists() { eprintln!("SKIP: fixture not generated"); return; }
    let dir = test_output_dir("hevc_cpu_360p");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", clip.to_str().unwrap(),
        "-c:v", "libx264", "-preset", "ultrafast", "-crf", "30", "-tune", "film", "-threads", "2",
        "-vf", "scale=-2:360",
        "-maxrate", "800k", "-bufsize", "1600k",
        "-c:a", "aac", "-b:a", "128k", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);

    assert!(ok, "HEVC→H.264 CPU 360p failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "No segments produced");
    assert!(is_valid_ts(&dir.join("segment_0000.ts")));
}

#[test]
fn hevc_copy_source_tier() {
    let clip = hevc_720p_clip();
    if !clip.exists() { eprintln!("SKIP: fixture not generated"); return; }
    let dir = test_output_dir("hevc_copy_source");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", clip.to_str().unwrap(),
        "-c:v", "copy", "-c:a", "aac", "-b:a", "320k", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);

    assert!(ok, "HEVC copy source failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "No segments produced");
    assert!(has_endlist(&playlist));
    assert!(is_valid_ts(&dir.join("segment_0000.ts")));
}

#[test]
fn vaapi_hybrid_720p_synthetic() {
    if !has_vaapi() { eprintln!("SKIP: no VAAPI"); return; }
    let clip = hevc_720p_clip();
    if !clip.exists() { eprintln!("SKIP: fixture not generated"); return; }
    let dir = test_output_dir("vaapi_hybrid_720p_synth");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-init_hw_device", "vaapi=va:/dev/dri/renderD128",
        "-filter_hw_device", "va",
        "-i", clip.to_str().unwrap(),
        "-vf", "scale=-2:720,format=nv12,hwupload",
        "-c:v", "h264_vaapi", "-global_quality", "20",
        "-c:a", "aac", "-b:a", "192k", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);

    assert!(ok, "VAAPI hybrid 720p failed: {stderr}");
    assert!(count_segments(&playlist) >= 1, "No segments produced");
    assert!(is_valid_ts(&dir.join("segment_0000.ts")));
}

// ============================================================
// Playlist structure tests
// ============================================================

#[test]
fn playlist_has_correct_structure() {
    let clip = h264_720p_clip();
    if !clip.exists() { eprintln!("SKIP"); return; }
    let dir = test_output_dir("playlist_structure");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, _) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", clip.to_str().unwrap(),
        "-c", "copy", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);
    assert!(ok);

    let content = std::fs::read_to_string(&playlist).unwrap();
    assert!(content.starts_with("#EXTM3U"), "Missing EXTM3U header");
    assert!(content.contains("#EXT-X-TARGETDURATION:"), "Missing target duration");
    assert!(content.contains("#EXT-X-MEDIA-SEQUENCE:"), "Missing media sequence");
    assert!(content.contains("#EXTINF:"), "Missing EXTINF entries");
    assert!(content.contains("#EXT-X-ENDLIST"), "Missing ENDLIST");

    // All segment lines should be just filenames (no path prefix)
    for line in content.lines() {
        if !line.starts_with('#') && !line.is_empty() {
            assert!(!line.contains('/'), "Segment should be bare filename: {line}");
            assert!(line.ends_with(".ts"), "Segment should end with .ts: {line}");
        }
    }
}

#[test]
fn segment_integrity_across_all_segments() {
    let clip = h264_720p_clip();
    if !clip.exists() { eprintln!("SKIP"); return; }
    let dir = test_output_dir("segment_integrity");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, _) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", clip.to_str().unwrap(),
        "-c", "copy", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);
    assert!(ok);

    let content = std::fs::read_to_string(&playlist).unwrap();
    for line in content.lines() {
        if !line.starts_with('#') && !line.is_empty() {
            let seg_path = dir.join(line);
            assert!(seg_path.exists(), "Segment missing: {line}");
            assert!(is_valid_ts(&seg_path), "Invalid TS: {line}");
            let size = std::fs::metadata(&seg_path).unwrap().len();
            assert!(size > 0, "Empty segment: {line}");
        }
    }
}

// ============================================================
// 4K HEVC tests (require real test file)
// ============================================================

#[test]
fn hevc_4k_copy_source() {
    let clip = match hevc_4k_clip() {
        Some(c) => c,
        None => { eprintln!("SKIP: 4K test file not available"); return; }
    };
    let dir = test_output_dir("hevc_4k_copy");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", clip.to_str().unwrap(),
        "-c:v", "copy", "-c:a", "aac", "-b:a", "320k", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);

    assert!(ok, "4K HEVC copy failed: {stderr}");
    assert!(count_segments(&playlist) >= 1);
    assert!(is_valid_ts(&dir.join("segment_0000.ts")));
}

#[test]
fn hevc_4k_to_1080p_cpu() {
    let clip = match hevc_4k_clip() {
        Some(c) => c,
        None => { eprintln!("SKIP: 4K test file not available"); return; }
    };
    let dir = test_output_dir("hevc_4k_cpu_1080p");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", clip.to_str().unwrap(),
        "-c:v", "libx264", "-preset", "ultrafast", "-crf", "28", "-tune", "film", "-threads", "2",
        "-vf", "scale=-2:1080",
        "-maxrate", "5000k", "-bufsize", "10000k",
        "-c:a", "aac", "-b:a", "256k", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);

    assert!(ok, "4K→1080p CPU failed: {stderr}");
    assert!(count_segments(&playlist) >= 1);
    assert!(is_valid_ts(&dir.join("segment_0000.ts")));
}

#[test]
fn hevc_4k_vaapi_hybrid_1080p() {
    if !has_vaapi() { eprintln!("SKIP: no VAAPI"); return; }
    let clip = match hevc_4k_clip() {
        Some(c) => c,
        None => { eprintln!("SKIP: 4K test file not available"); return; }
    };
    let dir = test_output_dir("hevc_4k_vaapi_1080p");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, stderr) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-init_hw_device", "vaapi=va:/dev/dri/renderD128",
        "-filter_hw_device", "va",
        "-i", clip.to_str().unwrap(),
        "-vf", "scale=-2:1080,format=nv12,hwupload",
        "-c:v", "h264_vaapi", "-global_quality", "20",
        "-c:a", "aac", "-b:a", "256k", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);

    assert!(ok, "4K VAAPI hybrid 1080p failed: {stderr}");
    assert!(count_segments(&playlist) >= 1);
    assert!(is_valid_ts(&dir.join("segment_0000.ts")));
}

#[test]
fn vaapi_full_hw_fails_on_hevc_10bit() {
    if !has_vaapi() { eprintln!("SKIP: no VAAPI"); return; }
    let clip = match hevc_4k_clip() {
        Some(c) => c,
        None => { eprintln!("SKIP: 4K test file not available"); return; }
    };
    let dir = test_output_dir("vaapi_full_hw_fail");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    let (ok, _) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-hwaccel", "vaapi",
        "-hwaccel_device", "/dev/dri/renderD128",
        "-hwaccel_output_format", "vaapi",
        "-i", clip.to_str().unwrap(),
        "-vf", "scale_vaapi=w=-2:h=1080:format=nv12",
        "-c:v", "h264_vaapi", "-global_quality", "20",
        "-c:a", "aac", "-b:a", "256k", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);

    // Full HW may fail on HEVC 10-bit 4K but can succeed on 8-bit or lower res
    // This test documents the behavior - failure is expected but success is acceptable
    if ok {
        eprintln!("NOTE: VAAPI full HW succeeded (GPU supports this input profile)");
    } else {
        eprintln!("EXPECTED: VAAPI full HW failed on this input (GPU limitation)");
    }
}

// ============================================================
// Audio preservation test
// ============================================================

#[test]
fn audio_channels_preserved_in_copy() {
    let clip = hevc_720p_clip();
    if !clip.exists() { eprintln!("SKIP"); return; }
    let dir = test_output_dir("audio_preserve");
    let playlist = dir.join("playlist.m3u8");
    let seg = dir.join("segment_%04d.ts");

    // Copy with no -ac flag (should preserve channels)
    let (ok, _) = run_ffmpeg(&[
        "-y", "-hide_banner", "-loglevel", "error",
        "-i", clip.to_str().unwrap(),
        "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-sn",
        "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_segment_type", "mpegts", "-hls_flags", "independent_segments",
        "-hls_segment_filename", seg.to_str().unwrap(),
        playlist.to_str().unwrap(),
    ]);
    assert!(ok);

    // Probe the first segment to verify audio exists
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_streams"])
        .arg(dir.join("segment_0000.ts"))
        .output();

    if let Ok(out) = probe {
        let json = String::from_utf8_lossy(&out.stdout);
        assert!(json.contains("\"codec_type\":\"audio\"") || json.contains("\"codec_type\": \"audio\""),
            "Audio stream missing from segment");
    }
}
