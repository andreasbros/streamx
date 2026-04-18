# StreamX E2E Test Plan

## Test Fixture Architecture

### 1. Synthetic Test Clips (FFmpeg-generated, deterministic)

Each clip has a unique visual pattern per frame (frame counter burned in) so the
last frame can be verified via screenshot comparison.

| ID | Codec | Container | Resolution | Audio | Duration | Use Case |
|----|-------|-----------|------------|-------|----------|----------|
| `h264_aac_mp4` | H.264 | MP4 | 720p | AAC stereo | 15s | Browser-compatible passthrough |
| `h264_ac3_mkv` | H.264 | MKV | 720p | AC3 5.1 | 15s | Container needs remux, surround audio |
| `hevc_aac_mkv` | HEVC | MKV | 1080p | AAC stereo | 15s | Needs transcode (HEVC in MKV) |
| `hevc_eac3_mkv` | HEVC 10-bit | MKV | 2160p | EAC3 5.1 | 10s | 4K HDR-like, hardest case |
| `vp9_opus_webm` | VP9 | WebM | 720p | Opus stereo | 15s | VP9 transcode path |
| `h264_aac_ts` | H.264 | MPEG-TS | 720p | AAC stereo | 15s | Already in TS container |
| `hevc_aac_mp4` | HEVC | MP4 | 1080p | AAC stereo | 15s | HEVC in MP4 (Safari passthrough?) |

All clips use `drawtext` filter to burn frame number + timestamp into each frame,
making every frame visually unique and verifiable.

### 2. Mock Torrent Download Simulator

A Rust struct that writes a video file progressively, simulating torrent sequential download:

```rust
struct MockTorrentWriter {
    source_data: Vec<u8>,
    output_path: PathBuf,
    schedule: Vec<ChunkSchedule>,
}

struct ChunkSchedule {
    delay_ms: u64,     // wait before writing this chunk
    byte_count: usize, // how many bytes to append
}
```

Modes:
- **Fast sequential**: 100ms chunks, simulates well-seeded torrent
- **Slow start**: 900ms first chunk, then 300ms, then 500ms (buffering test)
- **Stalling**: Writes half, pauses 5s, then completes (tests player recovery)
- **Custom**: User-defined per-chunk delays

### 3. HLS Segment Delay Simulator

Controls when each HLS segment becomes available:

```rust
struct SegmentSchedule {
    segment_index: u32,
    available_after_ms: u64,  // delay from transcode start
    playlist_update_ms: u64,  // when playlist includes this segment
}
```

This wraps the real FFmpeg transcode but controls segment visibility by:
1. Running FFmpeg normally (writes segments to a staging dir)
2. A watchdog moves segments to the served dir on schedule
3. Playlist is rewritten to only include available segments

### 4. Frame Verification

Each test clip burns `frame_NNNN` into the video. The test:
1. Records browser playback via Playwright
2. Extracts the last frame from the recording
3. Compares with expected frame image (golden file)
4. Uses perceptual hash (pHash) to handle compression artifacts

## Test Matrix

### A. Direct Play (no HLS)

| Test ID | Source | Expected Behavior | Verify |
|---------|--------|-------------------|--------|
| `direct_h264_mp4` | h264_aac_mp4 | Plays via range requests | Frame at 10s matches |
| `direct_h264_mkv` | h264_ac3_mkv | Falls back to HLS (MKV unsupported) | HLS badge appears |
| `direct_hevc_mp4` | hevc_aac_mp4 | Plays if browser supports HEVC, else HLS | Codec detection works |

### B. HLS Transcode Quality Tiers

| Test ID | Source | Quality | Expected FFmpeg | Verify |
|---------|--------|---------|-----------------|--------|
| `hls_source_h264` | h264_ac3_mkv | source | `-c:v copy -c:a aac` | Passthrough, no re-encode |
| `hls_source_hevc` | hevc_aac_mkv | source | `-c:v copy -c:a copy` | HEVC copy |
| `hls_720p_hevc` | hevc_eac3_mkv | 720p | `-c:v libx264 -vf scale=-2:720` | CPU transcode |
| `hls_360p_hevc` | hevc_eac3_mkv | 360p | `-c:v libx264 -vf scale=-2:360` | Downscale |
| `hls_1080p_4k` | hevc_eac3_mkv | 1080p | Scale from 4K to 1080p | 4K input handling |
| `hls_vaapi_720p` | hevc_aac_mkv | 720p | VAAPI hybrid encode | GPU path |

### C. Simulated Torrent Download + HLS

| Test ID | Download Pattern | HLS Quality | Expected |
|---------|-----------------|-------------|----------|
| `torrent_fast_source` | Fast sequential (100ms) | source | Plays within 5s |
| `torrent_slow_start` | 900/300/500ms chunks | source | Plays within 10s, buffers initially |
| `torrent_stall_recover` | Half, pause 5s, complete | source | Plays, stalls, recovers |
| `torrent_fast_720p` | Fast sequential | 720p | Transcode starts during download |

### D. Quality Switching

| Test ID | Start Quality | Switch To | Expected |
|---------|--------------|-----------|----------|
| `switch_source_to_720p` | source | 720p | Player reloads, new quality plays |
| `switch_720p_to_360p` | 720p | 360p | Downgrade works |
| `switch_360p_to_source` | 360p | source | Upgrade works |

### E. Player Recovery

| Test ID | Scenario | Expected |
|---------|----------|----------|
| `recovery_server_restart` | Kill server mid-play, restart | Player reconnects, resumes |
| `recovery_segment_404` | Delete one segment mid-play | Player skips to next segment |
| `recovery_corrupt_segment` | Replace segment with garbage | Corrupt detected, skipped |

### F. Audio Preservation

| Test ID | Source Audio | Expected Output Audio |
|---------|-------------|----------------------|
| `audio_stereo_copy` | AAC stereo | AAC stereo (copy) |
| `audio_51_preserve` | AC3 5.1 | AAC 5.1 (transcode, channels preserved) |
| `audio_eac3_51` | EAC3 5.1 | AAC 5.1 |

## Parameterized Test Implementation

Using `rstest` for parameterized tests:

```rust
#[rstest]
#[case::h264_mp4("h264_aac_mp4", "source", false, 10)]
#[case::hevc_mkv("hevc_aac_mkv", "source", true, 10)]
#[case::hevc_720p("hevc_aac_mkv", "720p", true, 10)]
#[case::hevc_360p("hevc_eac3_mkv", "360p", true, 10)]
#[case::h264_mkv("h264_ac3_mkv", "source", true, 10)]
#[tokio::test]
async fn hls_transcode(
    #[case] clip_id: &str,
    #[case] quality: &str,
    #[case] expect_transcode: bool,
    #[case] min_segments: usize,
) { ... }
```

## Playwright Test Structure

```typescript
// Parameterized via test.describe.each-like pattern
for (const { clipId, quality, expectHls, verifyFrame } of TEST_MATRIX) {
  test(`plays ${clipId} at ${quality}`, async ({ page }) => {
    // 1. Navigate to seeded player page
    // 2. Wait for playback (currentTime > 1)
    // 3. If expectHls, verify HLS badge
    // 4. Wait for specific frame (drawtext shows frame_NNNN)
    // 5. Take screenshot
    // 6. Compare last frame with golden image
  });
}
```

## Frame Verification Strategy

1. **Generate golden frames**: For each clip, extract frame at t=10s as PNG
2. **During test**: Playwright takes screenshot of video area at t=10s
3. **Compare**: Use ImageMagick `compare -metric RMSE` for perceptual diff
4. **Threshold**: Allow 5% difference (compression, scaling artifacts)

The `drawtext` filter ensures each frame is unique:
```
drawtext=fontfile=/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf:
  text='frame_%{frame_num} t=%{pts}':
  x=10:y=10:fontsize=24:fontcolor=white:box=1:boxcolor=black@0.5
```

## Execution Order

1. **Phase 1**: Generate all synthetic clips (one-time, cached)
2. **Phase 2**: Run backend-only HLS tests (parameterized, parallel-safe)
3. **Phase 3**: Run mock torrent + HLS tests (sequential, FFmpeg-heavy)
4. **Phase 4**: Run browser playback tests (sequential, video recorded)
5. **Phase 5**: Frame verification (post-processing, compare with golden)
6. **Phase 6**: Generate report with all videos, screenshots, metrics
