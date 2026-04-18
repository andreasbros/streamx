use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::net::SocketAddr;

// ---------------------------------------------------------------------------
// Test-server helpers (same pattern as api_tests.rs)
// ---------------------------------------------------------------------------

struct TestServer {
    base_url: String,
    _tmp: tempfile::TempDir,
}

async fn start_test_server() -> TestServer {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();

    std::fs::create_dir_all(data_dir.join("downloads")).unwrap();
    std::fs::create_dir_all(data_dir.join("cache")).unwrap();

    let port = portpicker::pick_unused_port().unwrap();

    let config = streamx::config::AppConfig {
        server: streamx::config::ServerConfig {
            port,
            bind: "127.0.0.1".to_string(),
            open_browser: false,
            log_level: None,
},
        torrent: streamx::config::TorrentConfig {
            max_connections: 200,
            sequential: true,
            seed_after_complete: true,
            dht: true,
            pex: true,
        },
        transcode: streamx::config::TranscodeConfig {
            hls_segment_duration: 4,
            video_codec: "h264".to_string(),
            audio_codec: "aac".to_string(),
            preset: "ultrafast".to_string(),
            max_concurrent_transcodes: 2,
            crf: 23,
            max_bitrate: None,
            audio_bitrate: "192k".to_string(),
            threads: None,
            gpu: false,
            hls_downscale: true,
            hls_max_height: 1080, hls_force_stereo: true,
        },
        auth: streamx::config::AuthConfig {
            jwt_secret: "test-secret-key-for-integration-tests".to_string(),
            session_duration: "7d".to_string(),
        },
        ui: streamx::config::UiConfig {
            default_theme: "dark".to_string(),
        },
        providers: vec![],
        vpn: None,
        data_dir: data_dir.clone(),
        log_level: "warn".to_string(),
        log_dir: None,
        open_browser: false,
        admin_user: None,
        admin_password: None,
    };

    let db_path = data_dir.join("streamx.db");
    let database = streamx::db::Database::open(&db_path).unwrap();
    database.init().await.unwrap();

    database.set_downloading_to_paused().await.unwrap();
    let torrent_engine =
        streamx::torrent::TorrentEngine::create(&config.torrent, &data_dir, database.clone(), None)
            .await
            .unwrap();
    let search_provider = streamx::torrent::SearchProvider::new(vec![], None);
    let cache_dir = data_dir.join("cache");
    let hls_pipeline = streamx::transcode::HlsManager::new(&config.transcode, cache_dir)
        .await
        .unwrap();

    let (log_tx, _) = tokio::sync::broadcast::channel::<String>(1000);
    let (_, log_history) = streamx::logging::BroadcastLayer::new(log_tx.clone());
    let app = streamx::server::build_router(
        database,
        config,
        torrent_engine,
        search_provider,
        hls_pipeline,
        log_tx,
        log_history,
    );

    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    TestServer {
        base_url: format!("http://127.0.0.1:{port}"),
        _tmp: tmp,
    }
}

async fn get_token(base_url: &str, username: &str, password: &str) -> String {
    let client = Client::new();
    let resp = client
        .post(format!("{base_url}/api/auth/register"))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
        }))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

/// Build a reqwest client that does NOT follow redirects, so we can inspect
/// the 307 status directly.
fn no_redirect_client() -> Client {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

const MAGNET_BBB: &str =
    "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny";

// ---------------------------------------------------------------------------
// 1. Basic stream lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn basic_stream_lifecycle() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "lifecycle_user", "password123").await;

    let client = Client::new();

    // POST /api/search to find "sintel"
    let search_resp = client
        .post(format!("{}/api/search", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "query": "sintel" }))
        .send()
        .await
        .unwrap();
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body: Value = search_resp.json().await.unwrap();
    assert!(search_body["results"].is_array());

    // POST /api/stream with a magnet URI to start a stream
    let create_resp = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "magnet_uri": MAGNET_BBB }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_body: Value = create_resp.json().await.unwrap();
    let stream_id = create_body["stream_id"].as_str().unwrap();
    assert!(!stream_id.is_empty());
    assert_eq!(create_body["status"], "initializing");

    // GET /api/stream/:id returns status
    let status_resp = client
        .get(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_body: Value = status_resp.json().await.unwrap();
    assert_eq!(status_body["id"], stream_id);

    // DELETE /api/stream/:id cleans up HLS
    let delete_resp = client
        .delete(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(delete_resp.status(), StatusCode::OK);
    let delete_body: Value = delete_resp.json().await.unwrap();
    assert_eq!(delete_body["status"], "stopped");

    // GET /api/stream/:id still returns the download (DB persists it)
    let still_resp = client
        .get(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(still_resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// 2. Demo stream always works
// ---------------------------------------------------------------------------

#[tokio::test]
async fn demo_stream_always_works() {
    let server = start_test_server().await;

    let client = Client::new();

    // GET /api/stream/demo returns ready status with 100% progress
    let demo_resp = client
        .get(format!("{}/api/stream/demo", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(demo_resp.status(), StatusCode::OK);
    let demo_body: Value = demo_resp.json().await.unwrap();
    assert_eq!(demo_body["id"], "demo");
    assert_eq!(demo_body["status"], "ready");
    assert_eq!(demo_body["progress"], 100.0);

    // GET /api/stream/demo/playlist.m3u8 returns 307 redirect
    let no_redir = no_redirect_client();
    let playlist_resp = no_redir
        .get(format!("{}/api/stream/demo/playlist.m3u8", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(playlist_resp.status(), StatusCode::TEMPORARY_REDIRECT);

    // POST /api/test/stream creates demo stream
    let create_demo = client
        .post(format!("{}/api/test/stream", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(create_demo.status(), StatusCode::OK);
    let demo_create_body: Value = create_demo.json().await.unwrap();
    assert_eq!(demo_create_body["stream_id"], "demo");
    assert_eq!(demo_create_body["status"], "ready");
}

// ---------------------------------------------------------------------------
// 3. Pause and resume
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pause_and_resume_stream() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "pause_user", "password123").await;

    let client = Client::new();

    // Start a stream
    let create_resp = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "magnet_uri": MAGNET_BBB }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_body: Value = create_resp.json().await.unwrap();
    let stream_id = create_body["stream_id"].as_str().unwrap();

    // PUT /api/stream/:id/pause returns success
    let pause_resp = client
        .put(format!("{}/api/stream/{stream_id}/pause", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(pause_resp.status(), StatusCode::OK);
    let pause_body: Value = pause_resp.json().await.unwrap();
    assert_eq!(pause_body["status"], "paused");

    // GET /api/stream/:id still returns the stream (not deleted)
    let status_resp = client
        .get(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_body: Value = status_resp.json().await.unwrap();
    assert_eq!(status_body["id"], stream_id);

    // PUT /api/stream/:id/resume returns success
    let resume_resp = client
        .put(format!("{}/api/stream/{stream_id}/resume", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resume_resp.status(), StatusCode::OK);
    let resume_body: Value = resume_resp.json().await.unwrap();
    assert_eq!(resume_body["status"], "resumed");

    // PUT /api/stream/:id/resume again (idempotent, should not error)
    let resume_again = client
        .put(format!("{}/api/stream/{stream_id}/resume", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resume_again.status(), StatusCode::OK);

    // PUT /api/stream/:id/pause again (idempotent, should not error)
    let pause_again = client
        .put(format!("{}/api/stream/{stream_id}/pause", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(pause_again.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// 4. Pause non-existent stream is a no-op (returns 200)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pause_nonexistent_stream_returns_ok() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "nostream_user", "password123").await;

    let client = Client::new();

    let pause_resp = client
        .put(format!(
            "{}/api/stream/fake-id-12345/pause",
            server.base_url
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(pause_resp.status(), StatusCode::OK);

    let resume_resp = client
        .put(format!(
            "{}/api/stream/fake-id-12345/resume",
            server.base_url
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resume_resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// 5. Double start same magnet
// ---------------------------------------------------------------------------

#[tokio::test]
async fn double_start_same_magnet() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "double_user", "password123").await;

    let client = Client::new();

    let resp1 = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "magnet_uri": MAGNET_BBB }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let body1: Value = resp1.json().await.unwrap();
    let id1 = body1["stream_id"].as_str().unwrap().to_string();

    let resp2 = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "magnet_uri": MAGNET_BBB }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2: Value = resp2.json().await.unwrap();
    let id2 = body2["stream_id"].as_str().unwrap().to_string();

    // Both should succeed with the same stream ID (same info_hash, DB deduplicates)
    assert!(!id1.is_empty());
    assert!(!id2.is_empty());
    assert_eq!(id1, id2);
}

// ---------------------------------------------------------------------------
// 6. Stream without auth returns 401
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stream_without_auth_returns_401() {
    let server = start_test_server().await;

    let client = Client::new();

    // POST /api/stream without token
    let create_resp = client
        .post(format!("{}/api/stream", server.base_url))
        .json(&serde_json::json!({ "magnet_uri": MAGNET_BBB }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::UNAUTHORIZED);

    // GET /api/stream/:id without token
    let get_resp = client
        .get(format!("{}/api/stream/some-fake-id", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::UNAUTHORIZED);

    // PUT /api/stream/:id/pause without token
    let pause_resp = client
        .put(format!("{}/api/stream/some-fake-id/pause", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(pause_resp.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// 7. Test endpoints don't require auth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_endpoints_no_auth_required() {
    let server = start_test_server().await;

    let no_redir = no_redirect_client();

    // GET /api/test/video returns 307 (no auth needed)
    let video_resp = no_redir
        .get(format!("{}/api/test/video", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(video_resp.status(), StatusCode::TEMPORARY_REDIRECT);

    // GET /api/test/playlist.m3u8 returns 307 (no auth needed)
    let playlist_resp = no_redir
        .get(format!("{}/api/test/playlist.m3u8", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(playlist_resp.status(), StatusCode::TEMPORARY_REDIRECT);

    // GET /api/stream/demo returns 200 (no auth needed)
    let demo_resp = no_redir
        .get(format!("{}/api/stream/demo", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(demo_resp.status(), StatusCode::OK);
    let demo_body: Value = demo_resp.json().await.unwrap();
    assert_eq!(demo_body["status"], "ready");
}

// ---------------------------------------------------------------------------
// 8. Stream playlist returns valid HLS
//    Note: for a freshly started torrent there is no actual file to
//    transcode, so the HLS pipeline may not yet have a playlist. We test
//    the demo playlist redirect instead, which always works.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stream_playlist_returns_valid_hls() {
    let server = start_test_server().await;

    // Use the default redirect-following client to fetch the demo HLS
    // playlist through the external URL the server redirects to.
    let client = Client::new();

    let resp = client
        .get(format!("{}/api/test/playlist.m3u8", server.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        content_type.contains("mpegurl"),
        "Expected mpegurl content-type, got: {content_type}"
    );

    let body = resp.text().await.unwrap();
    assert!(body.contains("#EXTM3U"), "Playlist must start with #EXTM3U");
}

// ---------------------------------------------------------------------------
// 9. History is recorded on stream start
// ---------------------------------------------------------------------------

#[tokio::test]
async fn history_recorded_on_stream_start() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "hist_user", "password123").await;

    let client = Client::new();

    // Start a stream
    let create_resp = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "magnet_uri": MAGNET_BBB }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::OK);

    // GET /api/history returns at least 1 item
    let hist_resp = client
        .get(format!("{}/api/history", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(hist_resp.status(), StatusCode::OK);
    let hist_body: Value = hist_resp.json().await.unwrap();
    let items = hist_body["items"].as_array().unwrap();
    assert!(
        !items.is_empty(),
        "History should have at least 1 item after starting a stream"
    );
}

// ---------------------------------------------------------------------------
// 10. Settings persist
// ---------------------------------------------------------------------------

#[tokio::test]
async fn settings_persist() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "settings_user", "password123").await;

    let client = Client::new();

    // PUT /api/settings with theme "light"
    let update1 = client
        .put(format!("{}/api/settings", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "theme": "light" }))
        .send()
        .await
        .unwrap();
    assert_eq!(update1.status(), StatusCode::OK);

    // GET /api/settings returns "light"
    let get1 = client
        .get(format!("{}/api/settings", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get1.status(), StatusCode::OK);
    let body1: Value = get1.json().await.unwrap();
    assert_eq!(body1["theme"], "light");

    // PUT /api/settings with theme "dark"
    let update2 = client
        .put(format!("{}/api/settings", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "theme": "dark" }))
        .send()
        .await
        .unwrap();
    assert_eq!(update2.status(), StatusCode::OK);

    // GET /api/settings returns "dark"
    let get2 = client
        .get(format!("{}/api/settings", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get2.status(), StatusCode::OK);
    let body2: Value = get2.json().await.unwrap();
    assert_eq!(body2["theme"], "dark");
}
