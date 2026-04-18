use reqwest::StatusCode;
use serde_json::Value;
use std::net::SocketAddr;

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

async fn register_user(base_url: &str, username: &str, password: &str) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .post(format!("{base_url}/api/auth/register"))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
        }))
        .send()
        .await
        .unwrap()
}

async fn login_user(base_url: &str, username: &str, password: &str) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .post(format!("{base_url}/api/auth/login"))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
        }))
        .send()
        .await
        .unwrap()
}

async fn get_token(base_url: &str, username: &str, password: &str) -> String {
    let resp = register_user(base_url, username, password).await;
    let body: Value = resp.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn register_user_and_get_token() {
    let server = start_test_server().await;
    let resp = register_user(&server.base_url, "testuser", "password123").await;

    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
    assert!(!body["token"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn login_with_registered_user() {
    let server = start_test_server().await;
    register_user(&server.base_url, "loginuser", "password123").await;

    let resp = login_user(&server.base_url, "loginuser", "password123").await;

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
}

#[tokio::test]
async fn me_returns_user_info() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "meuser", "password123").await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/auth/me", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["username"], "meuser");
    assert!(body["id"].is_string());
    assert!(body["created_at"].is_string());
}

#[tokio::test]
async fn unauthorized_request_returns_401() {
    let server = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/auth/me", server.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn register_duplicate_user_returns_error() {
    let server = start_test_server().await;
    register_user(&server.base_url, "dupuser", "password123").await;

    let resp = register_user(&server.base_url, "dupuser", "password456").await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: Value = resp.json().await.unwrap();
    let error_msg = body["error"].as_str().unwrap();
    assert!(error_msg.contains("already taken"));
}

#[tokio::test]
async fn invalid_username_too_short_returns_400() {
    let server = start_test_server().await;
    let resp = register_user(&server.base_url, "ab", "password123").await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: Value = resp.json().await.unwrap();
    let error_msg = body["error"].as_str().unwrap();
    assert!(error_msg.contains("Username"));
}

#[tokio::test]
async fn invalid_password_too_short_returns_400() {
    let server = start_test_server().await;
    let resp = register_user(&server.base_url, "validuser", "short").await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: Value = resp.json().await.unwrap();
    let error_msg = body["error"].as_str().unwrap();
    assert!(error_msg.contains("Password"));
}

#[tokio::test]
async fn search_endpoint_returns_results() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "searchuser", "password123").await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/search", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "query": "test query" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["results"].is_array());
}

#[tokio::test]
async fn create_stream_endpoint() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "streamuser", "password123").await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "magnet_uri": "magnet:?xt=urn:btih:0000000000000000000000000000000000000000&dn=test",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["stream_id"].is_string());
    assert_eq!(body["status"], "initializing");
}

#[tokio::test]
async fn get_stream_status() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "statususer", "password123").await;

    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "magnet_uri": "magnet:?xt=urn:btih:1111111111111111111111111111111111111111&dn=test",
        }))
        .send()
        .await
        .unwrap();

    let create_body: Value = create_resp.json().await.unwrap();
    let stream_id = create_body["stream_id"].as_str().unwrap();

    let resp = client
        .get(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"], stream_id);
    assert_eq!(body["status"], "initializing");
}

#[tokio::test]
async fn delete_stream() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "delstreamuser", "password123").await;

    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "magnet_uri": "magnet:?xt=urn:btih:2222222222222222222222222222222222222222&dn=test",
        }))
        .send()
        .await
        .unwrap();

    let create_body: Value = create_resp.json().await.unwrap();
    let stream_id = create_body["stream_id"].as_str().unwrap();

    let resp = client
        .delete(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "stopped");

    let get_resp = client
        .get(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    // Download persists in DB after HLS cleanup
    assert_eq!(get_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn watch_history_crud() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "histuser", "password123").await;

    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "magnet_uri": "magnet:?xt=urn:btih:3333333333333333333333333333333333333333&dn=histtest",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::OK);

    let list_resp = client
        .get(format!("{}/api/history", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body: Value = list_resp.json().await.unwrap();
    let items = list_body["items"].as_array().unwrap();
    assert!(!items.is_empty());

    let entry_id = items[0]["id"].as_str().unwrap();

    let update_resp = client
        .put(format!("{}/api/history/{entry_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "watched_seconds": 120 }))
        .send()
        .await
        .unwrap();
    assert_eq!(update_resp.status(), StatusCode::OK);

    let del_resp = client
        .delete(format!("{}/api/history/{entry_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn settings_crud() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "settingsuser", "password123").await;

    let client = reqwest::Client::new();

    let get_resp = client
        .get(format!("{}/api/settings", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(get_resp.status(), StatusCode::OK);
    let body: Value = get_resp.json().await.unwrap();
    assert_eq!(body["theme"], "dark");

    let update_resp = client
        .put(format!("{}/api/settings", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "theme": "light" }))
        .send()
        .await
        .unwrap();

    assert_eq!(update_resp.status(), StatusCode::OK);
    let update_body: Value = update_resp.json().await.unwrap();
    assert_eq!(update_body["theme"], "light");

    let verify_resp = client
        .get(format!("{}/api/settings", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    let verify_body: Value = verify_resp.json().await.unwrap();
    assert_eq!(verify_body["theme"], "light");
}

#[tokio::test]
async fn test_video_endpoint_returns_mp4() {
    let server = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/test/video", server.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("video/mp4"));

    let body = resp.bytes().await.unwrap();
    assert!(!body.is_empty());
    assert_eq!(&body[4..8], b"ftyp");
}

#[tokio::test]
async fn test_hls_playlist_endpoint() {
    let server = start_test_server().await;

    let client = reqwest::Client::new();
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
        .unwrap();
    assert!(content_type.contains("mpegurl"));

    let body = resp.text().await.unwrap();
    assert!(body.contains("#EXTM3U"));
    assert!(body.contains("#EXT-X-TARGETDURATION"));
}

#[tokio::test]
async fn admin_user_creation_via_config() {
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
            jwt_secret: "admin-test-secret".to_string(),
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
        admin_user: Some("myadmin".to_string()),
        admin_password: Some("adminpass123".to_string()),
    };

    let db_path = data_dir.join("streamx.db");
    let database = streamx::db::Database::open(&db_path).unwrap();
    database.init().await.unwrap();

    let admin_user = config.admin_user.as_ref().unwrap();
    let admin_pass = config.admin_password.as_ref().unwrap();
    let password_hash = streamx::server::auth::hash_password(admin_pass).unwrap();
    database
        .create_user(admin_user, &password_hash)
        .await
        .unwrap();

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

    let base_url = format!("http://127.0.0.1:{port}");
    let resp = login_user(&base_url, "myadmin", "adminpass123").await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let token = body["token"].as_str().unwrap();

    let client = reqwest::Client::new();
    let me_resp = client
        .get(format!("{base_url}/api/auth/me"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(me_resp.status(), StatusCode::OK);
    let me_body: Value = me_resp.json().await.unwrap();
    assert_eq!(me_body["username"], "myadmin");
    assert_eq!(me_body["is_admin"], true);
}
