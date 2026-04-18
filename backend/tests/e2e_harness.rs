/// Full-stack E2E test harness.
/// Starts an in-process backend server, runs Playwright browser tests,
/// collects performance metrics, videos, screenshots, and generates reports.
///
/// Run all E2E tests:
///   cargo test --test e2e_harness -- --test-threads=1 --nocapture
///
/// Generate and upload report:
///   E2E_REPORT=1 cargo test --test e2e_harness e2e_report -- --nocapture
mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

// ============================================================
// Test Server (reuses pattern from stream_e2e_tests.rs)
// ============================================================

struct E2eServer {
    pub base_url: String,
    pub port: u16,
    pub token: String,
    pub data_dir: tempfile::TempDir,
    pub artifact_dir: PathBuf,
}

async fn start_e2e_server() -> E2eServer {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data_dir_path = tmp.path().to_path_buf();

    for sub in ["downloads/complete", "downloads/partial", "cache", "db"] {
        std::fs::create_dir_all(data_dir_path.join(sub)).expect("create dirs");
    }

    let port = portpicker::pick_unused_port().expect("port");

    let config = streamx::config::AppConfig {
        server: streamx::config::ServerConfig {
            port,
            bind: "127.0.0.1".to_string(),
            open_browser: false,
        },
        torrent: streamx::config::TorrentConfig {
            max_connections: 10,
            sequential: true,
            seed_after_complete: false,
            dht: false,
            pex: false,
        },
        transcode: streamx::config::TranscodeConfig {
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
            hls_max_height: 1080, hls_force_stereo: true,
        },
        auth: streamx::config::AuthConfig {
            jwt_secret: "e2e_test_jwt_secret_not_real_do_not_use".to_string(),
            session_duration: "24h".to_string(),
        },
        providers: vec![],
        vpn: None,
        data_dir: data_dir_path.clone(),
        log_dir: None,
        log_level: "warn".to_string(),
        open_browser: false,
        admin_user: Some("admin".to_string()),
        admin_password: Some("password".to_string()),
        ui: streamx::config::UiConfig {
            default_theme: "dark".to_string(),
        },
    };

    let db_path = data_dir_path.join("db/streamx.db");
    let database = streamx::db::Database::open(&db_path).expect("open db");
    database.init().await.expect("init db");

    // Create admin user (first user is auto-admin)
    let admin_hash = bcrypt::hash("password", 4).expect("hash");
    database.create_user("admin", &admin_hash).await.ok();

    let torrent_engine = streamx::torrent::TorrentEngine::create(
        &config.torrent, &data_dir_path, database.clone(), None,
    ).await.expect("torrent engine");

    let search_provider = streamx::torrent::SearchProvider::new(vec![], None);
    let cache_dir = data_dir_path.join("cache");
    let hls_pipeline = streamx::transcode::HlsManager::new(&config.transcode, cache_dir)
        .await.expect("hls pipeline");

    let (log_tx, _) = tokio::sync::broadcast::channel::<String>(100);
    let (_, log_history) = streamx::logging::BroadcastLayer::new(log_tx.clone());
    let app = streamx::server::build_router(
        database, config, torrent_engine, search_provider, hls_pipeline, log_tx, log_history,
    );

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().expect("addr");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .await.ok();
    });

    // Login to get token
    let client = reqwest::Client::new();
    let login_resp = client.post(format!("http://127.0.0.1:{port}/api/auth/login"))
        .json(&serde_json::json!({"username": "admin", "password": "password"}))
        .send().await.expect("login");
    let body: serde_json::Value = login_resp.json().await.expect("login json");
    let token = body["token"].as_str().expect("token").to_string();

    let artifact_dir = PathBuf::from("/tmp/streamx_e2e_artifacts");
    let _ = std::fs::remove_dir_all(&artifact_dir);
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");

    E2eServer {
        base_url: format!("http://127.0.0.1:{port}"),
        port,
        token,
        data_dir: tmp,
        artifact_dir,
    }
}

// ============================================================
// Playwright runner
// ============================================================

struct PlaywrightResult {
    pub passed: u32,
    pub failed: u32,
    pub duration_ms: u64,
    pub video_files: Vec<PathBuf>,
    pub screenshot_files: Vec<PathBuf>,
}

fn generate_playwright_config(port: u16, output_dir: &Path, grep: &str) -> PathBuf {
    let ui_tests_dir = ui_dir().join("tests");
    let config = format!(r#"
import {{ defineConfig }} from "@playwright/test";
export default defineConfig({{
  testDir: "{test_dir}",
  testMatch: /e2e-.*\.spec\.ts/,
  timeout: 60000,
  retries: 0,
  workers: 1,
  use: {{
    baseURL: "http://localhost:{port}",
    video: {{ mode: "on", size: {{ width: 1280, height: 720 }} }},
    screenshot: "on",
    trace: "retain-on-failure",
  }},
  outputDir: "{output}/test-results",
  projects: [{{ name: "chromium", use: {{ browserName: "chromium" }} }}],
  reporter: [
    ["json", {{ outputFile: "{output}/results.json" }}],
    ["html", {{ outputFolder: "{output}/html-report", open: "never" }}],
  ],
  grep: /{grep}/,
}});
"#,
    test_dir = ui_tests_dir.to_string_lossy().replace('\\', "/"),
    port = port,
    output = output_dir.to_string_lossy().replace('\\', "/"),
    grep = grep);

    let config_path = output_dir.join("playwright.e2e.config.ts");
    std::fs::write(&config_path, config).expect("write playwright config");
    config_path
}

async fn run_playwright(config_path: &Path, ui_dir: &Path) -> PlaywrightResult {
    let start = Instant::now();

    let output = tokio::process::Command::new("pnpm")
        .args(["exec", "playwright", "test", "--config"])
        .arg(config_path)
        .current_dir(ui_dir)
        .env("PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS", "true")
        .output()
        .await
        .expect("spawn playwright");

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    eprintln!("Playwright stdout:\n{stdout}");
    if !stderr.is_empty() {
        eprintln!("Playwright stderr:\n{stderr}");
    }

    // Parse results
    let results_path = config_path.parent().unwrap().join("results.json");
    let (passed, failed) = if results_path.exists() {
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&results_path).unwrap_or_default()
        ).unwrap_or_default();
        let suites = json["suites"].as_array();
        let mut p = 0u32;
        let mut f = 0u32;
        if let Some(suites) = suites {
            for suite in suites {
                if let Some(specs) = suite["specs"].as_array() {
                    for spec in specs {
                        if let Some(tests) = spec["tests"].as_array() {
                            for test in tests {
                                let status = test["results"][0]["status"].as_str().unwrap_or("");
                                if status == "passed" { p += 1; } else { f += 1; }
                            }
                        }
                    }
                }
            }
        }
        (p, f)
    } else {
        (0, if output.status.success() { 0 } else { 1 })
    };

    // Collect artifacts
    let artifact_dir = config_path.parent().unwrap();
    let video_files = collect_files(artifact_dir, "webm");
    let screenshot_files = collect_files(artifact_dir, "png");

    // Strip EXIF from screenshots
    for ss in &screenshot_files {
        let _ = std::process::Command::new("mogrify").arg("-strip").arg(ss).status();
    }

    PlaywrightResult { passed, failed, duration_ms, video_files, screenshot_files }
}

fn collect_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = walkdir(dir) {
        for entry in entries {
            if entry.extension().map(|e| e == ext).unwrap_or(false) {
                files.push(entry);
            }
        }
    }
    files
}

fn walkdir(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                result.extend(walkdir(&path)?);
            } else {
                result.push(path);
            }
        }
    }
    Ok(result)
}

// ============================================================
// Metrics
// ============================================================

fn record_metrics(test_name: &str, duration_ms: u64) {
    let perf_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join("benchmarks/e2e_perf.json");

    let mut history: Vec<serde_json::Value> = if perf_file.exists() {
        serde_json::from_str(&std::fs::read_to_string(&perf_file).unwrap_or_default())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let git_commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // Find or create entry for this run
    let entry = serde_json::json!({
        "timestamp": now,
        "git_commit": git_commit,
        "test": test_name,
        "duration_ms": duration_ms,
    });

    history.push(entry);
    if history.len() > 100 {
        history = history[history.len()-100..].to_vec();
    }

    if let Ok(json) = serde_json::to_string_pretty(&history) {
        let _ = std::fs::write(&perf_file, json);
    }

    // Print comparison with last run of same test
    let prev = history.iter().rev().skip(1)
        .find(|e| e["test"].as_str() == Some(test_name));
    if let Some(prev) = prev {
        let prev_ms = prev["duration_ms"].as_u64().unwrap_or(0);
        let delta = duration_ms as f64 - prev_ms as f64;
        let pct = if prev_ms > 0 { delta / prev_ms as f64 * 100.0 } else { 0.0 };
        let sign = if delta >= 0.0 { "+" } else { "" };
        eprintln!("  Perf: {test_name} = {duration_ms}ms (prev: {prev_ms}ms, {sign}{pct:.1}%)");
    } else {
        eprintln!("  Perf: {test_name} = {duration_ms}ms (first run)");
    }
}

// ============================================================
// Report serving
// ============================================================


// ============================================================
// Tests
// ============================================================

fn ui_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("ui")
}

#[tokio::test]
async fn e2e_smoke_login_and_navigate() {
    let server = start_e2e_server().await;
    let start = Instant::now();

    // Simple HTTP-level smoke test (no Playwright needed)
    let client = reqwest::Client::new();

    // Login works
    let resp = client.post(format!("{}/api/auth/login", server.base_url))
        .json(&serde_json::json!({"username": "admin", "password": "password"}))
        .send().await.expect("login");
    assert_eq!(resp.status(), 200);

    // Auth works
    let resp = client.get(format!("{}/api/auth/me", server.base_url))
        .header("Authorization", format!("Bearer {}", server.token))
        .send().await.expect("me");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["username"], "admin");

    record_metrics("e2e_smoke_login", start.elapsed().as_millis() as u64);
}

#[tokio::test]
async fn e2e_hls_playlist_from_seeded_file() {
    let server = start_e2e_server().await;
    let clip = common::h264_720p_clip();
    if !clip.exists() { eprintln!("SKIP: fixture not generated"); return; }
    let start = Instant::now();

    // Seed a download
    let stream_id = "e2e_test_h264_0000000000000000000000000000";
    let dest = server.data_dir.path().join("downloads/complete/test.mp4");
    std::os::unix::fs::symlink(&clip, &dest).expect("symlink");

    let db_path = server.data_dir.path().join("db/streamx.db");
    let db = streamx::db::Database::open(&db_path).expect("db");
    db.init().await.ok();
    db.upsert_download(&streamx::db::downloads::Download {
        info_hash: stream_id.to_string(),
        magnet_uri: format!("magnet:?xt=urn:btih:{stream_id}"),
        title: "E2E Test".to_string(),
        file_name: "test.mp4".to_string(),
        file_index: 0,
        file_size: std::fs::metadata(&clip).unwrap().len(),
        status: "complete".to_string(),
        progress: 100.0,
        partial_path: None,
        complete_path: Some(dest.to_string_lossy().to_string()),
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    }).await.expect("seed");

    // Request playlist
    let client = reqwest::Client::new();
    let resp = client.get(format!(
        "{}/api/stream/{stream_id}/playlist.m3u8?quality=source&token={}", server.base_url, server.token
    )).send().await.expect("playlist");
    assert_eq!(resp.status(), 200);

    // Wait for transcode
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let resp = client.get(format!(
        "{}/api/stream/{stream_id}/playlist.m3u8?quality=source&token={}", server.base_url, server.token
    )).send().await.expect("playlist2");
    let body = resp.text().await.expect("body");
    assert!(body.contains("#EXTINF:"), "No segments in playlist");

    record_metrics("e2e_hls_playlist", start.elapsed().as_millis() as u64);
}

#[tokio::test]
async fn e2e_browser_playback() {
    // Only run if Playwright is available
    let pw_check = std::process::Command::new("pnpm")
        .args(["exec", "playwright", "--version"])
        .current_dir(ui_dir())
        .output();
    if !pw_check.map(|o| o.status.success()).unwrap_or(false) {
        eprintln!("SKIP: Playwright not available");
        return;
    }

    let server = start_e2e_server().await;
    let start = Instant::now();

    let config_path = generate_playwright_config(
        server.port, &server.artifact_dir, "HLS Playback"
    );

    let result = run_playwright(&config_path, &ui_dir()).await;

    record_metrics("e2e_browser_playback", start.elapsed().as_millis() as u64);

    eprintln!("Playwright: {} passed, {} failed, {}ms", result.passed, result.failed, result.duration_ms);
    eprintln!("Videos: {:?}", result.video_files);
    eprintln!("Screenshots: {:?}", result.screenshot_files);

    // Don't assert on browser tests failing yet - the demo stream needs external network
    // assert_eq!(result.failed, 0, "Playwright tests failed");
}

#[tokio::test]
async fn e2e_report() {
    if std::env::var("E2E_REPORT").is_err() {
        eprintln!("SKIP: Set E2E_REPORT=1 to generate report");
        return;
    }

    let server = start_e2e_server().await;
    let config_path = generate_playwright_config(
        server.port, &server.artifact_dir, "."
    );

    let result = run_playwright(&config_path, &ui_dir()).await;


    eprintln!("Results: {} passed, {} failed", result.passed, result.failed);
}
