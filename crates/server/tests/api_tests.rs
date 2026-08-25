use reqwest::StatusCode;
use serde_json::Value;
use std::net::SocketAddr;

struct TestServer {
    base_url: String,
    data_dir: std::path::PathBuf,
    db: streamx::db::Database,
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
            download_dir: None,
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
            hls_max_height: 1080,
            hls_force_stereo: true,
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
    let db_for_tests = database.clone();

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
        data_dir,
        db: db_for_tests,
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

async fn create_stream_with_hash(base_url: &str, token: &str, hash: &str) -> String {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/stream"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "magnet_uri": format!("magnet:?xt=urn:btih:{hash}&dn=test"),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    body["stream_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn delete_stream_removes_files_and_db_rows() {
    let server = start_test_server().await;
    // First registered user is admin.
    let token = get_token(&server.base_url, "delstreamuser", "password123").await;
    let hash = "2222222222222222222222222222222222222222";
    let stream_id = create_stream_with_hash(&server.base_url, &token, hash).await;
    assert_eq!(stream_id, hash);

    // Simulate a partially downloaded multi-file torrent on disk plus
    // dependent DB rows, then verify everything is removed.
    let manifest = serde_json::json!([
        {"seq_index": 0, "native_index": 0, "path": "Album/01.mp3", "size": 10, "is_audio": true, "is_video": false},
        {"seq_index": 1, "native_index": 1, "path": "Album/02.mp3", "size": 10, "is_audio": true, "is_video": false},
        {"seq_index": 2, "native_index": 2, "path": "flat.mkv", "size": 10, "is_audio": false, "is_video": true},
    ]);
    server
        .db
        .update_download_files(hash, &manifest.to_string())
        .await
        .unwrap();

    let partial = server.data_dir.join("downloads").join("partial");
    let complete = server.data_dir.join("downloads").join("complete");
    let posters = server.data_dir.join("downloads").join("posters");
    std::fs::create_dir_all(partial.join("Album")).unwrap();
    std::fs::create_dir_all(complete.join("Album")).unwrap();
    std::fs::create_dir_all(&posters).unwrap();
    std::fs::write(partial.join("Album/01.mp3"), b"x").unwrap();
    std::fs::write(complete.join("Album/02.mp3"), b"x").unwrap();
    std::fs::write(complete.join("flat.mkv"), b"x").unwrap();
    std::fs::write(posters.join(format!("{hash}.jpg")), b"x").unwrap();

    {
        let conn = server.db.connection().lock().await;
        conn.execute(
            "INSERT OR IGNORE INTO users (id, username, password_hash, created_at, is_admin) \
             VALUES ('u1', 'seeduser', 'x', '2026-01-01', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO watch_history (id, user_id, magnet_uri, title, watched_at) \
             VALUES ('wh1', 'u1', ?1, 't', '2026-01-01')",
            rusqlite::params![format!("magnet:?xt=urn:btih:{hash}&dn=test")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favourites (id, user_id, content_type, title, info_hash, created_at) \
             VALUES ('f1', 'u1', 'movie', 't', ?1, '2026-01-01')",
            rusqlite::params![hash],
        )
        .unwrap();
    }

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "deleted");

    // DB rows gone.
    assert!(server.db.get_download(hash).await.unwrap().is_none());
    {
        let conn = server.db.connection().lock().await;
        let wh: i64 = conn
            .query_row("SELECT COUNT(*) FROM watch_history", [], |r| r.get(0))
            .unwrap();
        let fav: i64 = conn
            .query_row("SELECT COUNT(*) FROM favourites", [], |r| r.get(0))
            .unwrap();
        assert_eq!(wh, 0, "watch_history row should be deleted");
        assert_eq!(fav, 0, "favourites row should be deleted");
    }

    // Files gone (torrent folders removed recursively, flat file removed,
    // poster removed).
    assert!(!partial.join("Album").exists());
    assert!(!complete.join("Album").exists());
    assert!(!complete.join("flat.mkv").exists());
    assert!(!posters.join(format!("{hash}.jpg")).exists());

    // Stream no longer known.
    let get_resp = client
        .get(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_stream_requires_admin() {
    let server = start_test_server().await;
    let admin_token = get_token(&server.base_url, "firstadmin", "password123").await;
    let user_token = get_token(&server.base_url, "seconduser", "password123").await;
    let hash = "3333333333333333333333333333333333333333";
    let stream_id = create_stream_with_hash(&server.base_url, &admin_token, hash).await;

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{}/api/stream/{stream_id}", server.base_url))
        .header("Authorization", format!("Bearer {user_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Row untouched.
    assert!(server.db.get_download(hash).await.unwrap().is_some());
}

#[tokio::test]
async fn pin_and_unpin_download() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "pinuser", "password123").await;
    let hash = "4444444444444444444444444444444444444444";
    let stream_id = create_stream_with_hash(&server.base_url, &token, hash).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{}/api/stream/{stream_id}/download",
            server.base_url
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let dl = server.db.get_download(hash).await.unwrap().unwrap();
    assert!(dl.pinned);

    let resp = client
        .delete(format!(
            "{}/api/stream/{stream_id}/download",
            server.base_url
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let dl = server.db.get_download(hash).await.unwrap().unwrap();
    assert!(!dl.pinned);

    // Unknown stream 404s on pin.
    let resp = client
        .post(format!(
            "{}/api/stream/ffffffffffffffffffffffffffffffffffffffff/download",
            server.base_url
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn downloads_queue_lists_streams() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "queueuser", "password123").await;
    let hash = "5555555555555555555555555555555555555555";
    let stream_id = create_stream_with_hash(&server.base_url, &token, hash).await;

    let client = reqwest::Client::new();
    // Unauthenticated is rejected.
    let resp = client
        .get(format!("{}/api/downloads", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Pin it so the queue shows the background flag.
    client
        .post(format!(
            "{}/api/stream/{stream_id}/download",
            server.base_url
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    let resp = client
        .get(format!("{}/api/downloads", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let downloads = body["downloads"].as_array().unwrap();
    let item = downloads
        .iter()
        .find(|d| d["info_hash"] == hash)
        .expect("created download should appear in the queue");
    assert_eq!(item["pinned"], true);
    assert!(item["status"].is_string());
    assert!(item["progress"].is_number());
}

#[tokio::test]
async fn download_movie_rebuilds_group() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "dlmovieuser", "password123").await;
    let hash = "6666666666666666666666666666666666666666";

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/stream", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "magnet_uri": format!("magnet:?xt=urn:btih:{hash}&dn=Test.Movie.2026.1080p"),
            "title": "Test Movie",
            "year": 2026,
            "rating": 7.5,
            "genres": ["Action", "Drama"],
            "summary": "A test.",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = client
        .get(format!("{}/api/downloads/{hash}/movie", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = client
        .get(format!("{}/api/downloads/{hash}/movie", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let group: Value = resp.json().await.unwrap();
    assert_eq!(group["title"], "Test Movie");
    assert_eq!(group["year"], 2026);
    assert_eq!(group["genres"][0], "Action");
    let variants = group["variants"].as_array().unwrap();
    assert_eq!(variants.len(), 1);
    let magnet = variants[0]["magnet"].as_str().unwrap();
    assert!(magnet.contains(hash));

    let resp = client
        .get(format!(
            "{}/api/downloads/ffffffffffffffffffffffffffffffffffffffff/movie",
            server.base_url
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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

    // The endpoint redirects to an external demo asset. Assert the
    // redirect itself instead of following it, so the test doesn't
    // depend on a third-party host being up.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .get(format!("{}/api/test/video", server.base_url))
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_redirection());
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert!(location.ends_with(".mp4"));
}

#[tokio::test]
async fn test_hls_playlist_endpoint() {
    let server = start_test_server().await;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .get(format!("{}/api/test/playlist.m3u8", server.base_url))
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_redirection());
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert!(location.contains(".m3u8"));
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
            download_dir: None,
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
            hls_max_height: 1080,
            hls_force_stereo: true,
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

// ===================== Playlists =====================

async fn create_playlist(base_url: &str, token: &str, name: &str) -> String {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/playlists"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    body["id"].as_str().unwrap().to_string()
}

async fn add_track(
    base_url: &str,
    token: &str,
    playlist_id: &str,
    hash: &str,
    file_index: u32,
    title: &str,
) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .post(format!("{base_url}/api/playlists/{playlist_id}/tracks"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "info_hash": hash,
            "file_index": file_index,
            "title": title,
        }))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn playlist_crud_and_track_ordering() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "pluser", "password123").await;
    let client = reqwest::Client::new();
    let pid = create_playlist(&server.base_url, &token, "Road trip").await;

    let hash = "dddd4444dddd4444dddd4444dddd4444dddd4444";
    // Insert out of alphabetical order; playback order must follow
    // insertion positions, never titles.
    for (i, title) in [(0u32, "Zulu"), (1, "Alpha"), (2, "Mike")] {
        let resp = add_track(&server.base_url, &token, &pid, hash, i, title).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let resp = client
        .get(format!("{}/api/playlists/{pid}/tracks", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tracks = body["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 3);
    let titles: Vec<&str> = tracks
        .iter()
        .map(|t| t["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        vec!["Zulu", "Alpha", "Mike"],
        "position order, not title order"
    );
    let positions: Vec<i64> = tracks
        .iter()
        .map(|t| t["position"].as_i64().unwrap())
        .collect();
    assert_eq!(positions, vec![0, 1, 2]);

    // Duplicate (same playlist, hash, file_index) is rejected.
    let dup = add_track(&server.base_url, &token, &pid, hash, 1, "Alpha again").await;
    assert_eq!(dup.status(), StatusCode::BAD_REQUEST);

    // Remove the middle track; order of the rest is unchanged.
    let track_id = tracks[1]["id"].as_str().unwrap();
    let del = client
        .delete(format!(
            "{}/api/playlists/{pid}/tracks/{track_id}",
            server.base_url
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::OK);

    let resp = client
        .get(format!("{}/api/playlists/{pid}/tracks", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let titles: Vec<String> = body["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(titles, vec!["Zulu", "Mike"]);

    // Delete the playlist; its tracks are gone with it (FK cascade).
    let del = client
        .delete(format!("{}/api/playlists/{pid}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::OK);
    let resp = client
        .get(format!("{}/api/playlists/{pid}/tracks", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn playlists_are_private_to_their_owner() {
    let server = start_test_server().await;
    let owner = get_token(&server.base_url, "plowner", "password123").await;
    let intruder = get_token(&server.base_url, "plintruder", "password123").await;
    let pid = create_playlist(&server.base_url, &owner, "Private mix").await;
    let hash = "eeee5555eeee5555eeee5555eeee5555eeee5555";
    add_track(&server.base_url, &owner, &pid, hash, 0, "Secret song").await;

    let client = reqwest::Client::new();
    // Another user can neither read...
    let resp = client
        .get(format!("{}/api/playlists/{pid}/tracks", server.base_url))
        .header("Authorization", format!("Bearer {intruder}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    // ...nor write someone else's playlist.
    let resp = add_track(&server.base_url, &intruder, &pid, hash, 1, "Injected").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    // Nor delete it.
    let resp = client
        .delete(format!("{}/api/playlists/{pid}", server.base_url))
        .header("Authorization", format!("Bearer {intruder}"))
        .send()
        .await
        .unwrap();
    // Delete is scoped by user_id: no error but nothing deleted.
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = client
        .get(format!("{}/api/playlists/{pid}/tracks", server.base_url))
        .header("Authorization", format!("Bearer {owner}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "owner's playlist must survive"
    );

    // Unauthenticated requests are rejected outright.
    let resp = client
        .get(format!("{}/api/playlists/{pid}/tracks", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn favourite_music_pins_full_download() {
    let server = start_test_server().await;
    let token = get_token(&server.base_url, "favmusic", "password123").await;
    let hash = "abcd9999abcd9999abcd9999abcd9999abcd9999";
    let magnet = format!("magnet:?xt=urn:btih:{hash}&dn=Great%20Album");

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/favourites", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "content_type": "music",
            "title": "Great Album",
            "info_hash": hash,
            "metadata_json": serde_json::json!({ "magnet": magnet }).to_string(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The favourite triggers a pinned full-album download in the
    // background; poll the DB for the row.
    let mut pinned = false;
    for _ in 0..40 {
        if let Some(dl) = server.db.get_download(hash).await.unwrap() {
            if dl.pinned && dl.download_all {
                pinned = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(
        pinned,
        "favourited music must become a pinned full-album download"
    );
}
