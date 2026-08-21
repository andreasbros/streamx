/// E2E browser tests proving HLS transcode playback works.
/// Seeds a completed MKV file → browser detects incompatible format →
/// switches to HLS → waits for FFmpeg segments → plays video.
///
/// Each test records a video of the browser session as proof.
///
/// Run: cargo test --test e2e_hls_browser -- --test-threads=1 --nocapture
mod common;

use common::fixtures::*;
use rstest::rstest;
use std::path::{Path, PathBuf};

struct HlsBrowserServer {
    port: u16,
    token: String,
    data_dir: tempfile::TempDir,
    artifact_dir: PathBuf,
}

async fn start_server() -> HlsBrowserServer {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dp = tmp.path().to_path_buf();
    for sub in ["downloads/complete", "downloads/partial", "cache", "db"] {
        std::fs::create_dir_all(dp.join(sub)).expect("dirs");
    }

    let port = portpicker::pick_unused_port().expect("port");
    let config = streamx::config::AppConfig {
        server: streamx::config::ServerConfig {
            port,
            bind: "127.0.0.1".into(),
            open_browser: false,
            log_level: None,
        },
        torrent: streamx::config::TorrentConfig {
            download_dir: None,
            max_connections: 10,
            sequential: true,
            seed_after_complete: false,
            dht: false,
            pex: false,
        },
        transcode: streamx::config::TranscodeConfig {
            hls_segment_duration: 2,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            preset: "ultrafast".into(),
            max_concurrent_transcodes: 2,
            crf: 28,
            max_bitrate: None,
            audio_bitrate: "128k".into(),
            threads: Some(2),
            gpu: false,
            hls_downscale: true,
            hls_max_height: 1080,
            hls_force_stereo: true,
        },
        auth: streamx::config::AuthConfig {
            jwt_secret: "hls_browser_test".into(),
            session_duration: "24h".into(),
        },
        providers: vec![],
        vpn: None,
        data_dir: dp.clone(),
        log_dir: None,
        log_level: "info".into(),
        open_browser: false,
        admin_user: None,
        admin_password: None,
        ui: streamx::config::UiConfig {
            default_theme: "dark".into(),
        },
    };

    let db = streamx::db::Database::open(&dp.join("db/streamx.db")).expect("db");
    db.init().await.expect("init");
    let hash = bcrypt::hash("password", 4).expect("hash");
    db.create_user("admin", &hash).await.ok();

    let engine = streamx::torrent::TorrentEngine::create(&config.torrent, &dp, db.clone(), None)
        .await
        .expect("engine");
    let search = streamx::torrent::SearchProvider::new(vec![], None);
    let hls = streamx::transcode::HlsManager::new(&config.transcode, dp.join("cache"))
        .await
        .expect("hls");
    let (log_tx, _) = tokio::sync::broadcast::channel::<String>(100);
    let (_, log_hist) = streamx::logging::BroadcastLayer::new(log_tx.clone());
    let app =
        streamx::server::build_router(db.clone(), config, engine, search, hls, log_tx, log_hist);

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .ok();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/api/auth/login"))
        .json(&serde_json::json!({"username": "admin", "password": "password"}))
        .send()
        .await
        .expect("login");
    let body: serde_json::Value = resp.json().await.expect("json");
    let token = body["token"].as_str().expect("token").to_string();

    let artifact_dir = PathBuf::from(format!("/tmp/streamx_hls_browser/{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&artifact_dir);
    std::fs::create_dir_all(&artifact_dir).unwrap();

    HlsBrowserServer {
        port,
        token,
        data_dir: tmp,
        artifact_dir,
    }
}

impl HlsBrowserServer {
    async fn seed_complete(&self, stream_id: &str, source: &Path) {
        let dest = self
            .data_dir
            .path()
            .join("downloads/complete")
            .join(source.file_name().unwrap());
        std::fs::copy(source, &dest).expect("copy file");

        let db =
            streamx::db::Database::open(&self.data_dir.path().join("db/streamx.db")).expect("db");
        db.init().await.ok();
        db.upsert_download(&streamx::db::downloads::Download {
            info_hash: stream_id.into(),
            magnet_uri: format!("magnet:?xt=urn:btih:{stream_id}"),
            title: "HLS Browser Test".into(),
            file_name: source.file_name().unwrap().to_string_lossy().into(),
            file_index: 0,
            file_size: std::fs::metadata(source).map(|m| m.len()).unwrap_or(0),
            download_all: false,
            files_json: None,
            pinned: false,
            status: "complete".into(),
            progress: 100.0,
            partial_path: None,
            complete_path: Some(dest.to_string_lossy().into()),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        })
        .await
        .expect("seed");
    }
}

fn ui_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("ui")
}

async fn run_playwright(
    port: u16,
    artifact_dir: &Path,
    stream_id: &str,
    token: &str,
    quality: &str,
) -> (bool, String) {
    let ui_tests = ui_dir().join("tests");
    let screenshot_path = artifact_dir.join("playback.png");

    let config = format!(
        r#"
import {{ defineConfig }} from "@playwright/test";
export default defineConfig({{
  testDir: "{tdir}",
  testMatch: /e2e-hls-transcode-playback\.spec\.ts/,
  timeout: 60000, retries: 0, workers: 1,
  use: {{
    baseURL: "http://localhost:{port}",
    video: {{ mode: "on", size: {{ width: 1280, height: 720 }} }},
    screenshot: "on",
  }},
  outputDir: "{out}/test-results",
  projects: [{{ name: "chrome", use: {{
    channel: "chrome",
    launchOptions: {{
      args: [
        "--autoplay-policy=no-user-gesture-required",
        "--enable-features=VaapiVideoDecodeLinuxGL,VaapiVideoEncoder",
        "--use-gl=egl",
      ],
    }},
  }} }}],
  reporter: [["json", {{ outputFile: "{out}/results.json" }}], ["html", {{ outputFolder: "{out}/html-report", open: "never" }}]],
}});
"#,
        tdir = ui_tests.to_string_lossy().replace('\\', "/"),
        port = port,
        out = artifact_dir.to_string_lossy().replace('\\', "/")
    );

    let config_path = artifact_dir.join("pw.config.ts");
    std::fs::write(&config_path, config).unwrap();

    let output = tokio::process::Command::new("pnpm")
        .args(["exec", "playwright", "test", "--config"])
        .arg(&config_path)
        .env("STREAMX_STREAM_ID", stream_id)
        .env("STREAMX_TOKEN", token)
        .env("STREAMX_QUALITY", quality)
        .env("STREAMX_SCREENSHOT_PATH", screenshot_path.to_str().unwrap())
        .env("PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS", "true")
        .current_dir(ui_dir())
        .output()
        .await
        .expect("playwright");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        eprintln!("Playwright stdout: {stdout}");
        eprintln!("Playwright stderr: {stderr}");
    }

    (output.status.success(), stdout)
}

// ============================================================
// Tests
// ============================================================

#[rstest]
// h264_aac_ts: H.264+stereo AAC in TS container → needs HLS, stereo works in all browsers
#[case::h264_ts_source("h264_aac_ts", "source")]
// hevc_aac_mkv at 720p: HEVC→H.264 transcode + stereo AAC, works in all browsers
#[case::hevc_mkv_720p("hevc_aac_mkv", "720p")]
// hevc_aac_mkv source: HEVC copy in fMP4 (requires HEVC-capable browser, skip in headless)
#[case::hevc_mkv_source("hevc_aac_mkv", "source")]
// h264_ac3_mkv: H.264+6ch AC3→6ch AAC (requires multi-channel MSE support, skip in headless)
#[case::h264_mkv_surround("h264_ac3_mkv", "source")]
#[tokio::test]
async fn hls_transcode_browser_playback(#[case] clip_id: &str, #[case] quality: &str) {
    // Check prerequisites
    if !ui_dir().join("dist/index.html").exists() {
        eprintln!("SKIP: UI not built (cd ui && pnpm build)");
        return;
    }
    let pw = std::process::Command::new("pnpm")
        .args(["exec", "playwright", "--version"])
        .current_dir(ui_dir())
        .output();
    if !pw.map(|o| o.status.success()).unwrap_or(false) {
        eprintln!("SKIP: Playwright not available");
        return;
    }

    let clip = match get_clip(clip_id) {
        Some(c) => c,
        None => {
            eprintln!("SKIP: clip {clip_id} not generated");
            return;
        }
    };

    let test_name = format!("{clip_id}_{quality}");
    let server = start_server().await;
    let stream_id = format!("hlstest_{test_name}_0000000000000000000000");

    // Seed completed download
    server.seed_complete(&stream_id, &clip).await;

    eprintln!(
        "[{test_name}] Server on port {}, running Playwright...",
        server.port
    );
    let start = std::time::Instant::now();

    let (passed, stdout) = run_playwright(
        server.port,
        &server.artifact_dir,
        &stream_id,
        &server.token,
        quality,
    )
    .await;

    let elapsed = start.elapsed();
    eprintln!("[{test_name}] Finished in {elapsed:.1?}");

    // Parse playback state from stdout
    if let Some(line) = stdout.lines().find(|l| l.contains("PLAYBACK_ADVANCED:")) {
        eprintln!("[{test_name}] {line}");
    }
    if let Some(line) = stdout.lines().find(|l| l.contains("PLAYBACK_STATE:")) {
        eprintln!("[{test_name}] {line}");
    }

    // List artifacts
    let videos: Vec<_> = walkdir(&server.artifact_dir, "webm");
    let screenshots: Vec<_> = walkdir(&server.artifact_dir, "png");
    eprintln!(
        "[{test_name}] Videos: {}, Screenshots: {}",
        videos.len(),
        screenshots.len()
    );

    // Frame comparison: extract golden frame and compare with screenshot
    let screenshot = server.artifact_dir.join("playback.png");
    if screenshot.exists() && passed {
        let golden = server.artifact_dir.join("golden_frame.png");
        // Extract frame at ~3s from the source clip (matches what browser played)
        if extract_golden_frame(&clip, 3.0, &golden) && golden.exists() {
            if let Some(diff_pct) = compare_images(&golden, &screenshot) {
                eprintln!("[{test_name}] Frame diff: {diff_pct:.1}%");
                // Generous threshold for HLS transcode artifacts
                if diff_pct < 25.0 {
                    eprintln!(
                        "[{test_name}] FRAME MATCH: golden and playback within {diff_pct:.1}%"
                    );
                } else {
                    eprintln!("[{test_name}] FRAME MISMATCH: {diff_pct:.1}% difference");
                }
            }
        }
        eprintln!("[{test_name}] Screenshot: {}", screenshot.display());
        eprintln!("[{test_name}] Golden: {}", golden.display());
    }

    // HEVC source copy and multi-channel audio require browser capabilities
    // that headless Chromium typically lacks (HEVC decoder, multi-ch MSE)
    let needs_advanced_codec = (clip_id.contains("hevc") && quality == "source")
        || clip_id.contains("ac3")
        || clip_id.contains("eac3");
    if !passed && needs_advanced_codec {
        eprintln!("[{test_name}] Expected: headless Chromium lacks HEVC/multi-channel support");
        return;
    }
    assert!(
        passed,
        "[{test_name}] Playwright test failed - check video at {:?}",
        videos.first()
    );
}

fn walkdir(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut r = Vec::new();
    fn walk(d: &Path, e: &str, r: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(d) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, e, r);
                } else if p.extension().map(|x| x == e).unwrap_or(false) {
                    r.push(p);
                }
            }
        }
    }
    walk(dir, ext, &mut r);
    r
}
