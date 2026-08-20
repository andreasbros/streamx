/// Tests that FFmpeg produces identical HLS fMP4 segments regardless of how the
/// source file was written (sequential vs sparse/out-of-order chunks).
///
/// This proves the torrent download pattern doesn't affect transcode output.
mod common;

use common::fixtures::*;
use rstest::rstest;
use std::path::PathBuf;
use streamx::config::TranscodeConfig;
use streamx::transcode::hls::PlaylistResponse;
use streamx::transcode::HlsManager;

/// Checks that the given data starts with a valid ISO BMFF box header.
/// fMP4 segments begin with a 4-byte size followed by a 4-byte box type.
fn is_valid_fmp4(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }
    let box_type = &data[4..8];
    matches!(
        box_type,
        b"styp" | b"moof" | b"mdat" | b"ftyp" | b"moov" | b"sidx"
    )
}

fn test_config() -> TranscodeConfig {
    TranscodeConfig {
        hls_segment_duration: 2,
        video_codec: "h264".to_string(),
        audio_codec: "aac".to_string(),
        preset: "ultrafast".to_string(),
        max_concurrent_transcodes: 2,
        crf: 28,
        max_bitrate: None,
        audio_bitrate: "128k".to_string(),
        threads: Some(1), // single thread for determinism
        gpu: false,
        hls_downscale: true,
        hls_max_height: 1080,
        hls_force_stereo: true,
    }
}

async fn transcode_to_segments(
    source_path: &std::path::Path,
    quality: &str,
    test_name: &str,
) -> (Vec<u8>, Vec<String>) {
    let cache_dir = PathBuf::from(format!("/tmp/streamx_sparse_test/{test_name}"));
    let _ = std::fs::remove_dir_all(&cache_dir);
    std::fs::create_dir_all(&cache_dir).unwrap();

    let mgr = HlsManager::new(&test_config(), cache_dir.clone())
        .await
        .unwrap();
    let stream_id = format!("sparse_{test_name}");

    mgr.start_stream(&stream_id, source_path.to_str().unwrap(), quality)
        .await
        .unwrap();

    // Wait for transcode to complete
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let resp = mgr.generate_playlist(&stream_id, quality).await.unwrap();
        if let PlaylistResponse::Content(content) = &resp {
            if content.contains("EXT-X-ENDLIST") {
                break;
            }
        }
    }

    // Read first segment
    let seg_dirs = [
        cache_dir.join(&stream_id).join(quality),
        cache_dir.join(&stream_id),
    ];
    let mut first_segment = Vec::new();
    let mut segment_names = Vec::new();

    for dir in &seg_dirs {
        let seg_path = dir.join("segment_0000.m4s");
        if seg_path.exists() {
            first_segment = std::fs::read(&seg_path).unwrap_or_default();
            // Collect all segment names
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.ends_with(".m4s") {
                        segment_names.push(name);
                    }
                }
            }
            segment_names.sort();
            break;
        }
    }

    (first_segment, segment_names)
}

// ============================================================
// Core test: sequential write vs sparse write produce same segments
// ============================================================

#[rstest]
#[case::h264_mkv("h264_ac3_mkv")]
#[case::hevc_mkv("hevc_aac_mkv")]
#[tokio::test]
async fn sequential_vs_sparse_same_segments(#[case] clip_id: &str) {
    let source_clip = match get_clip(clip_id) {
        Some(c) => c,
        None => {
            eprintln!("SKIP: clip not generated");
            return;
        }
    };

    // 1. Write file sequentially (baseline)
    let seq_path = PathBuf::from(format!("/tmp/streamx_sparse_test/seq_{clip_id}.mkv"));
    let _ = std::fs::create_dir_all(seq_path.parent().unwrap());
    std::fs::copy(&source_clip, &seq_path).unwrap();

    let (seq_seg, seq_names) =
        transcode_to_segments(&seq_path, "source", &format!("seq_{clip_id}")).await;

    assert!(!seq_seg.is_empty(), "Sequential: no segment produced");
    assert!(is_valid_fmp4(&seq_seg), "Sequential: invalid fMP4 segment");
    eprintln!(
        "[{clip_id}] Sequential: {} segments, first={} bytes",
        seq_names.len(),
        seq_seg.len()
    );

    // 2. Write file via sparse torrent writer (same data, different write order)
    let sparse_path = PathBuf::from(format!("/tmp/streamx_sparse_test/sparse_{clip_id}.mkv"));
    let piece_size = 32 * 1024; // 32KB pieces
    let writer =
        SparseTorrentWriter::out_of_order(&source_clip, sparse_path.clone(), piece_size, 0)
            .unwrap();
    writer.execute().await.unwrap();

    // Verify the sparse file is identical to the original
    let sparse_data = std::fs::read(&sparse_path).unwrap();
    let source_data = std::fs::read(&source_clip).unwrap();
    assert_eq!(
        sparse_data.len(),
        source_data.len(),
        "Sparse file size mismatch"
    );
    assert_eq!(
        sparse_data, source_data,
        "Sparse file content mismatch - torrent simulation broken"
    );

    // 3. Transcode the sparse-written file
    let (sparse_seg, sparse_names) =
        transcode_to_segments(&sparse_path, "source", &format!("sparse_{clip_id}")).await;

    assert!(!sparse_seg.is_empty(), "Sparse: no segment produced");
    assert!(is_valid_fmp4(&sparse_seg), "Sparse: invalid fMP4 segment");
    eprintln!(
        "[{clip_id}] Sparse: {} segments, first={} bytes",
        sparse_names.len(),
        sparse_seg.len()
    );

    // 4. Compare: same number of segments
    assert_eq!(
        seq_names.len(),
        sparse_names.len(),
        "Segment count differs: seq={} sparse={}",
        seq_names.len(),
        sparse_names.len()
    );

    // 5. Compare: first segment identical
    assert_eq!(
        seq_seg.len(),
        sparse_seg.len(),
        "First segment size differs: seq={} sparse={}",
        seq_seg.len(),
        sparse_seg.len()
    );
    assert_eq!(
        seq_seg, sparse_seg,
        "First segment content differs between sequential and sparse write"
    );

    eprintln!("[{clip_id}] PASS: Sequential and sparse produce identical segments");
}

// ============================================================
// Test: sparse file with delays still produces valid segments
// ============================================================

#[rstest]
#[case::sequential_50ms("sequential", 50)]
#[case::out_of_order_50ms("out_of_order", 50)]
#[case::slow_start("slow_start", 0)]
#[case::stalling("stalling", 0)]
#[tokio::test]
async fn sparse_write_pattern_produces_valid_hls(#[case] pattern: &str, #[case] _delay_ms: u64) {
    let source_clip = match get_clip("h264_ac3_mkv") {
        Some(c) => c,
        None => {
            eprintln!("SKIP");
            return;
        }
    };

    let piece_size = 32 * 1024;
    let test_name = format!("pattern_{pattern}");
    let output_path = PathBuf::from(format!("/tmp/streamx_sparse_test/{test_name}.mkv"));
    let _ = std::fs::create_dir_all(output_path.parent().unwrap());

    let writer = match pattern {
        "sequential" => {
            SparseTorrentWriter::sequential(&source_clip, output_path.clone(), piece_size, 10)
                .unwrap()
        }
        "out_of_order" => {
            SparseTorrentWriter::out_of_order(&source_clip, output_path.clone(), piece_size, 10)
                .unwrap()
        }
        "slow_start" => SparseTorrentWriter::sequential_slow_start(
            &source_clip,
            output_path.clone(),
            piece_size,
        )
        .unwrap(),
        "stalling" => {
            SparseTorrentWriter::stalling(&source_clip, output_path.clone(), piece_size, 500)
                .unwrap()
        }
        _ => panic!("Unknown pattern"),
    };

    writer.execute().await.unwrap();

    // Verify file is complete and correct
    let written = std::fs::read(&output_path).unwrap();
    let original = std::fs::read(&source_clip).unwrap();
    assert_eq!(
        written, original,
        "[{pattern}] Written file differs from original"
    );

    // Transcode and verify segments
    let (seg, names) = transcode_to_segments(&output_path, "source", &test_name).await;
    assert!(!seg.is_empty(), "[{pattern}] No segments produced");
    assert!(is_valid_fmp4(&seg), "[{pattern}] Invalid fMP4 segment");
    assert!(!names.is_empty(), "[{pattern}] No segment files");
    eprintln!(
        "[{pattern}] PASS: {} segments, first={} bytes",
        names.len(),
        seg.len()
    );
}

// ============================================================
// Test: different quality tiers from same sparse file
// ============================================================

#[rstest]
#[case::source("source")]
#[case::q720p("720p")]
#[case::q360p("360p")]
#[tokio::test]
async fn sparse_file_quality_tiers(#[case] quality: &str) {
    let source_clip = match get_clip("hevc_aac_mkv") {
        Some(c) => c,
        None => {
            eprintln!("SKIP");
            return;
        }
    };

    let piece_size = 32 * 1024;
    let test_name = format!("tier_{quality}");
    let output_path = PathBuf::from(format!("/tmp/streamx_sparse_test/{test_name}.mkv"));
    let _ = std::fs::create_dir_all(output_path.parent().unwrap());

    // Write out of order
    let writer =
        SparseTorrentWriter::out_of_order(&source_clip, output_path.clone(), piece_size, 0)
            .unwrap();
    writer.execute().await.unwrap();

    let (seg, names) = transcode_to_segments(&output_path, quality, &test_name).await;
    assert!(
        !seg.is_empty(),
        "[{quality}] No segments produced from sparse file"
    );
    assert!(is_valid_fmp4(&seg), "[{quality}] Invalid fMP4 segment");
    eprintln!(
        "[{quality}] PASS: {} segments from sparse HEVC file",
        names.len()
    );
}
