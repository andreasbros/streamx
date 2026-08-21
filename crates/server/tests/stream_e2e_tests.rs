/// Server-level E2E tests for HLS streaming.
/// Starts a real HTTP server, seeds a download, and verifies the full
/// playlist → segment → playback pipeline via HTTP requests.
mod common;

use common::*;
use reqwest::StatusCode;
use std::net::SocketAddr;

struct TestServer {
    base_url: String,
    token: String,
    data_dir: tempfile::TempDir,
}

async fn start_test_server() -> TestServer {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data_dir_path = tmp.path().to_path_buf();

    std::fs::create_dir_all(data_dir_path.join("downloads/complete")).expect("create dirs");
    std::fs::create_dir_all(data_dir_path.join("downloads/partial")).expect("create dirs");
    std::fs::create_dir_all(data_dir_path.join("cache")).expect("create dirs");
    std::fs::create_dir_all(data_dir_path.join("db")).expect("create dirs");

    let port = portpicker::pick_unused_port().expect("port");

    let config = streamx::config::AppConfig {
        server: streamx::config::ServerConfig {
            port,
            bind: "127.0.0.1".to_string(),
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
        },
        auth: streamx::config::AuthConfig {
            jwt_secret: "test_secret_key_for_e2e_tests_only".to_string(),
            session_duration: "24h".to_string(),
        },
        providers: vec![],
        vpn: None,
        data_dir: data_dir_path.clone(),
        log_dir: None,
        log_level: "warn".to_string(),
        open_browser: false,
        admin_user: None,
        admin_password: None,
        ui: streamx::config::UiConfig {
            default_theme: "dark".to_string(),
        },
    };

    let db_path = data_dir_path.join("db/streamx.db");
    let database = streamx::db::Database::open(&db_path).expect("open db");
    database.init().await.expect("init db");
    database
        .set_downloading_to_paused()
        .await
        .expect("set paused");

    let torrent_engine = streamx::torrent::TorrentEngine::create(
        &config.torrent,
        &data_dir_path,
        database.clone(),
        None,
    )
    .await
    .expect("torrent engine");

    let search_provider = streamx::torrent::SearchProvider::new(vec![], None);
    let cache_dir = data_dir_path.join("cache");
    let hls_pipeline = streamx::transcode::HlsManager::new(&config.transcode, cache_dir)
        .await
        .expect("hls pipeline");

    let (log_tx, _) = tokio::sync::broadcast::channel::<String>(100);
    let (_, log_history) = streamx::logging::BroadcastLayer::new(log_tx.clone());
    let app = streamx::server::build_router(
        database.clone(),
        config,
        torrent_engine,
        search_provider,
        hls_pipeline,
        log_tx,
        log_history,
    );

    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().expect("addr");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve");
    });

    let base_url = format!("http://127.0.0.1:{port}");

    // Register and login to get token
    let client = reqwest::Client::new();
    client
        .post(format!("{base_url}/api/auth/register"))
        .json(&serde_json::json!({"username": "testuser", "password": "testpass123"}))
        .send()
        .await
        .expect("register");

    let login_resp = client
        .post(format!("{base_url}/api/auth/login"))
        .json(&serde_json::json!({"username": "testuser", "password": "testpass123"}))
        .send()
        .await
        .expect("login");
    let login_body: serde_json::Value = login_resp.json().await.expect("login json");
    let token = login_body["token"].as_str().expect("token").to_string();

    TestServer {
        base_url,
        token,
        data_dir: tmp,
    }
}

impl TestServer {
    fn auth_client(&self) -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.token).parse().unwrap(),
        );
        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap()
    }

    /// Seed a completed download in the DB by copying a test file into the server's data dir
    async fn seed_download(&self, stream_id: &str, source_file: &std::path::Path) -> String {
        let dest_dir = self.data_dir.path().join("downloads/complete");
        let file_name = source_file
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let dest_path = dest_dir.join(&file_name);

        // Symlink instead of copy (faster)
        std::os::unix::fs::symlink(source_file, &dest_path).expect("symlink test file");

        let db_path = self.data_dir.path().join("db/streamx.db");
        let db = streamx::db::Database::open(&db_path).expect("open db");
        db.init().await.expect("init");
        db.set_server_settings(&streamx::db::settings::ServerSettings {
            disable_transcode: false,
            ..Default::default()
        })
        .await
        .expect("enable transcode");

        let dl = streamx::db::downloads::Download {
            info_hash: stream_id.to_string(),
            magnet_uri: format!("magnet:?xt=urn:btih:{stream_id}&dn=test"),
            title: "Test Stream".to_string(),
            file_name: file_name.clone(),
            file_index: 0,
            file_size: std::fs::metadata(source_file).map(|m| m.len()).unwrap_or(0),
            download_all: false,
            files_json: None,
            pinned: false,
            status: "complete".to_string(),
            progress: 100.0,
            partial_path: None,
            complete_path: Some(dest_path.to_string_lossy().to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        db.upsert_download(&dl).await.expect("upsert download");
        dest_path.to_string_lossy().to_string()
    }
}

// ============================================================
// Playlist endpoint tests
// ============================================================

#[tokio::test]
async fn playlist_requires_auth() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{}/api/stream/fake_id/playlist.m3u8",
            server.base_url
        ))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn playlist_returns_hls_for_h264() {
    let server = start_test_server().await;
    let clip = h264_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP");
        return;
    }

    let stream_id = "aabbccdd11223344aabbccdd11223344aabbccdd";
    server.seed_download(stream_id, &clip).await;

    let client = server.auth_client();

    // Request playlist (triggers passthrough transcode)
    let resp = client
        .get(format!(
            "{}/api/stream/{stream_id}/playlist.m3u8?quality=source",
            server.base_url
        ))
        .send()
        .await
        .expect("playlist request");

    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("mpegurl"),
        "Wrong content type: {content_type}"
    );

    let body = resp.text().await.expect("body");
    assert!(body.contains("#EXTM3U"), "Not an HLS playlist");

    // Wait for segments to be produced
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Re-fetch playlist (should now have segments)
    let resp2 = client
        .get(format!(
            "{}/api/stream/{stream_id}/playlist.m3u8?quality=source",
            server.base_url
        ))
        .send()
        .await
        .expect("playlist request 2");
    let body2 = resp2.text().await.expect("body2");
    assert!(
        body2.contains("#EXTINF:"),
        "No segments in playlist: {}",
        &body2[..200.min(body2.len())]
    );
}

#[tokio::test]
async fn playlist_returns_hls_for_hevc() {
    let server = start_test_server().await;
    let clip = hevc_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP");
        return;
    }

    let stream_id = "eeff00112233445566778899aabbccddeeff0011";
    server.seed_download(stream_id, &clip).await;
    let client = server.auth_client();

    // Request playlist with quality=source (should HEVC copy)
    let resp = client
        .get(format!(
            "{}/api/stream/{stream_id}/playlist.m3u8?quality=source",
            server.base_url
        ))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), StatusCode::OK);

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let resp2 = client
        .get(format!(
            "{}/api/stream/{stream_id}/playlist.m3u8?quality=source",
            server.base_url
        ))
        .send()
        .await
        .expect("request 2");
    let body = resp2.text().await.expect("body");
    assert!(body.contains("#EXTINF:"), "No segments");

    // Segment paths should include quality prefix
    for line in body.lines() {
        if !line.starts_with('#') && !line.is_empty() {
            assert!(line.starts_with("source/"), "Missing prefix: {line}");
        }
    }
}

// ============================================================
// Segment endpoint tests (MPEG-TS)
// ============================================================

#[tokio::test]
async fn variant_segment_returns_valid_mpegts() {
    let server = start_test_server().await;
    let clip = hevc_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP");
        return;
    }

    let stream_id = "1122334455667788990011223344556677889900";
    server.seed_download(stream_id, &clip).await;
    let client = server.auth_client();

    // Trigger transcode
    client
        .get(format!(
            "{}/api/stream/{stream_id}/playlist.m3u8?quality=source",
            server.base_url
        ))
        .send()
        .await
        .expect("trigger");

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Fetch segment
    let resp = client
        .get(format!(
            "{}/api/stream/{stream_id}/source/segment_0000.ts",
            server.base_url
        ))
        .send()
        .await
        .expect("segment request");

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        ct.contains("mp2t") || ct.contains("video") || ct.contains("octet-stream"),
        "Wrong content type: {ct}"
    );

    let bytes = resp.bytes().await.expect("segment bytes");
    assert!(
        bytes.len() >= 188,
        "Segment too small: {} bytes",
        bytes.len()
    );
    assert_eq!(bytes[0], 0x47, "Missing MPEG-TS sync byte");
}

// ============================================================
// Quality parameter tests
// ============================================================

#[tokio::test]
async fn quality_param_selects_tier() {
    let server = start_test_server().await;
    let clip = hevc_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP");
        return;
    }

    let stream_id = "aabb001122334455aabb001122334455aabb0011";
    server.seed_download(stream_id, &clip).await;
    let client = server.auth_client();

    // Request 360p
    client
        .get(format!(
            "{}/api/stream/{stream_id}/playlist.m3u8?quality=360p",
            server.base_url
        ))
        .send()
        .await
        .expect("360p trigger");

    tokio::time::sleep(std::time::Duration::from_secs(8)).await;

    // 360p directory should exist
    let resp = client
        .get(format!(
            "{}/api/stream/{stream_id}/360p/segment_0000.ts",
            server.base_url
        ))
        .send()
        .await
        .expect("360p segment");

    // Should be 200 (segment exists) or 404 (still transcoding)
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NOT_FOUND,
        "Unexpected status: {status}"
    );

    if status == StatusCode::OK {
        let bytes = resp.bytes().await.expect("bytes");
        assert!(bytes.len() >= 188, "Segment too small for MPEG-TS");
        assert_eq!(bytes[0], 0x47, "Missing MPEG-TS sync byte");
    }
}

// ============================================================
// Stream file endpoint (direct download)
// ============================================================

#[tokio::test]
async fn stream_file_serves_complete_download() {
    let server = start_test_server().await;
    let clip = h264_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP");
        return;
    }

    let stream_id = "ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00";
    server.seed_download(stream_id, &clip).await;
    let client = server.auth_client();

    let resp = client
        .get(format!("{}/api/stream/{stream_id}/file", server.base_url))
        .send()
        .await
        .expect("file request");

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        ct.contains("video") || ct.contains("mp4"),
        "Wrong content type: {ct}"
    );

    let bytes = resp.bytes().await.expect("file bytes");
    let clip_size = std::fs::metadata(&clip).unwrap().len();
    assert_eq!(bytes.len() as u64, clip_size, "File size mismatch");
}

#[tokio::test]
async fn stream_file_supports_range_requests() {
    let server = start_test_server().await;
    let clip = h264_720p_clip();
    if !clip.exists() {
        eprintln!("SKIP");
        return;
    }

    let stream_id = "aa11bb22cc33dd44ee55ff66aa11bb22cc33dd44";
    server.seed_download(stream_id, &clip).await;
    let client = server.auth_client();

    let resp = client
        .get(format!("{}/api/stream/{stream_id}/file", server.base_url))
        .header("Range", "bytes=0-1023")
        .send()
        .await
        .expect("range request");

    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let bytes = resp.bytes().await.expect("bytes");
    assert_eq!(
        bytes.len(),
        1024,
        "Range response wrong size: {}",
        bytes.len()
    );
}

// ============================================================
// Demo stream
// ============================================================

#[tokio::test]
async fn demo_playlist_returns_redirect() {
    let server = start_test_server().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                "Authorization",
                format!("Bearer {}", server.token).parse().unwrap(),
            );
            h
        })
        .build()
        .unwrap();

    let resp = client
        .get(format!(
            "{}/api/stream/demo/playlist.m3u8?quality=source",
            server.base_url
        ))
        .send()
        .await
        .expect("demo request");

    assert!(
        resp.status().is_redirection(),
        "Expected redirect, got: {}",
        resp.status()
    );
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert!(
        location.contains("test-streams.mux.dev"),
        "Wrong redirect: {location}"
    );
}
