//! In-process `Api` impl used by the desktop app in Embedded mode.
//!
//! Every method in this file reaches into the server's data stores
//! directly — no HTTP, no JSON. The HTTP server still runs alongside
//! (so phones / browsers on the LAN work), but the desktop client
//! skips the TCP loopback entirely.

use std::sync::Arc;

use async_trait::async_trait;
use streamx_api::client::{Api, BrowseParams, ClientError, ClientResult, HttpClient};
use streamx_api::types::{
    CreateStreamRequest, CreateStreamResponse, FavouritesResponse, LoginResponse,
    MusicVideoResult as WireMusic, MusicVideoSearchResponse, Playlist, PlaylistTrack,
    ResolveMagnetResponse, SearchResponse, StreamStatus, TorrentFile, TvSearchResponse, User,
    VersionResponse, WatchHistoryItem, WatchHistoryResponse,
};

use crate::error::Error as ServerError;
use crate::runner::ServerComponents;
use crate::server::auth::{create_jwt, hash_password, validate_jwt};
use crate::server::static_files::{BUILD_HASH, VERSION};

pub struct LocalApi {
    /// Loopback client — retained only for token storage so `set_token`
    /// and `token()` behave the same as the HTTP backend.
    http: HttpClient,
    components: Arc<ServerComponents>,
    /// Tokio runtime handle. Every async method spawns its body here so
    /// callers can live on a non-tokio executor (e.g. GPUI) without the
    /// server's `tokio::spawn` calls panicking.
    handle: tokio::runtime::Handle,
}

impl LocalApi {
    pub fn new(components: Arc<ServerComponents>, loopback_url: String) -> Self {
        Self {
            http: HttpClient::new(loopback_url),
            components,
            handle: tokio::runtime::Handle::current(),
        }
    }

    /// Spawn `fut` on the stored tokio runtime and await the result.
    /// Ensures server internals (torrent engine, db, reqwest) always run
    /// in a tokio context regardless of where the Api call originates.
    async fn run<F, T>(&self, fut: F) -> ClientResult<T>
    where
        F: std::future::Future<Output = ClientResult<T>> + Send + 'static,
        T: Send + 'static,
    {
        self.handle
            .spawn(fut)
            .await
            .map_err(|e| ClientError::Backend(format!("join error: {e}")))?
    }

    pub fn tokio_handle(&self) -> tokio::runtime::Handle {
        self.handle.clone()
    }

    /// Fetch a poster by its `/proxy/{id}/{path}` URL. Serves from disk
    /// cache or upstream; caches the result. Used by the desktop
    /// AssetSource to avoid HTTP loopback for image loads.
    pub async fn fetch_proxy(&self, proxy_path: &str) -> ClientResult<(Vec<u8>, &'static str)> {
        let rest = proxy_path
            .strip_prefix("/proxy/")
            .ok_or_else(|| ClientError::Backend("not a proxy path".into()))?;
        let (id_str, sub) = rest
            .split_once('/')
            .ok_or_else(|| ClientError::Backend("malformed proxy path".into()))?;
        let id: u32 = id_str
            .parse()
            .map_err(|_| ClientError::Backend("bad provider id".into()))?;

        let (bytes, ext) = crate::server::proxy::fetch_proxy_bytes(
            id,
            sub,
            &self.components.http_client,
            &self.components.config.data_dir,
            &self.components.config.providers,
        )
        .await
        .map_err(err_to_client)?;
        Ok((bytes, ext))
    }

    /// Decode the stored JWT to get the current user id. Returns
    /// `Unauthorized` when nothing is logged in.
    ///
    /// Embedded mode is native/local: an *expired* token with a valid
    /// signature identifies the same local user, so instead of failing
    /// the call it is transparently renewed for a fresh session.
    async fn user_id(&self) -> ClientResult<String> {
        let secret = self.components.config.auth.jwt_secret.clone();
        if let Some(token) = self.http.token() {
            if let Ok(claims) = validate_jwt(&token, &secret) {
                return Ok(claims.user_id);
            }
            if let Ok(claims) = crate::server::auth::validate_jwt_allow_expired(&token, &secret) {
                let hours = self.session_hours()?;
                let is_admin = matches!(claims.role, crate::server::auth::Role::Admin);
                if let Ok(fresh) =
                    create_jwt(&claims.user_id, &claims.username, is_admin, &secret, hours)
                {
                    tracing::info!("embedded session token expired; renewed in place");
                    self.http.set_token(Some(fresh));
                }
                return Ok(claims.user_id);
            }
        }
        // No token, or one signed by another server (e.g. a thin-client
        // session). Embedded mode is native/local: identify as the local
        // default user and mint a fresh session instead of failing.
        let db = self.components.database.clone();
        let user = self
            .handle
            .spawn(async move { db.local_default_user().await })
            .await
            .map_err(|e| ClientError::Backend(format!("join error: {e}")))?
            .map_err(err_to_client)?
            .ok_or(ClientError::Unauthorized)?;
        let hours = self.session_hours()?;
        if let Ok(fresh) = create_jwt(&user.id, &user.username, user.is_admin, &secret, hours) {
            tracing::info!(username = %user.username, "embedded session self-issued for local user");
            self.http.set_token(Some(fresh));
        }
        Ok(user.id)
    }

    fn session_hours(&self) -> ClientResult<i64> {
        let trimmed = self.components.config.auth.session_duration.trim();
        if let Some(days) = trimmed.strip_suffix('d') {
            let d: u64 = days.parse().map_err(|_| {
                ClientError::Backend(format!("Invalid session duration: {trimmed}"))
            })?;
            Ok((d as i64) * 24)
        } else if let Some(hours) = trimmed.strip_suffix('h') {
            let h: u64 = hours.parse().map_err(|_| {
                ClientError::Backend(format!("Invalid session duration: {trimmed}"))
            })?;
            Ok(h as i64)
        } else {
            Ok(168)
        }
    }
}

/// Map server-side errors to ClientError so the same wire-level error
/// surface is preserved across the two transports.
fn err_to_client(e: ServerError) -> ClientError {
    match e {
        ServerError::Auth { message } => {
            let _ = message;
            ClientError::Unauthorized
        }
        ServerError::Unauthorized { message } => {
            let _ = message;
            ClientError::Unauthorized
        }
        other => ClientError::Backend(other.to_string()),
    }
}

#[async_trait]
impl Api for LocalApi {
    fn base_url(&self) -> String {
        "in-process".to_string()
    }

    fn token(&self) -> Option<String> {
        self.http.token()
    }

    fn set_token(&self, token: Option<String>) {
        self.http.set_token(token);
    }

    // -------- meta --------

    async fn version(&self) -> ClientResult<VersionResponse> {
        Ok(VersionResponse {
            version: VERSION.to_string(),
            hash: BUILD_HASH.to_string(),
        })
    }

    // -------- auth --------

    async fn login(&self, username: &str, password: &str) -> ClientResult<LoginResponse> {
        let components = self.components.clone();
        let username = username.trim().to_lowercase();
        let password = password.to_string();
        let hours = self.session_hours()?;
        self.run(async move {
            let user = components
                .database
                .find_user_by_username(&username)
                .await
                .map_err(err_to_client)?
                .ok_or(ClientError::Unauthorized)?;
            let ok = bcrypt::verify(&password, &user.password_hash)
                .map_err(|e| ClientError::Backend(e.to_string()))?;
            if !ok {
                return Err(ClientError::Unauthorized);
            }
            let secret = &components.config.auth.jwt_secret;
            let token = create_jwt(&user.id, &user.username, user.is_admin, secret, hours)
                .map_err(err_to_client)?;
            Ok(LoginResponse { token })
        })
        .await
    }

    async fn needs_setup(&self) -> ClientResult<bool> {
        let components = self.components.clone();
        self.run(async move {
            components
                .database
                .user_count()
                .await
                .map(|n| n == 0)
                .map_err(err_to_client)
        })
        .await
    }

    async fn register(&self, username: &str, password: &str) -> ClientResult<LoginResponse> {
        let components = self.components.clone();
        let username = username.trim().to_lowercase();
        let password = password.to_string();
        let hours = self.session_hours()?;
        self.run(async move {
            if username.len() < 3 || username.len() > 32 {
                return Err(ClientError::Backend(
                    "Username must be between 3 and 32 characters".into(),
                ));
            }
            if password.len() < 8 || password.len() > 128 {
                return Err(ClientError::Backend(
                    "Password must be between 8 and 128 characters".into(),
                ));
            }
            if components
                .database
                .find_user_by_username(&username)
                .await
                .map_err(err_to_client)?
                .is_some()
            {
                return Err(ClientError::Backend("Username already taken".into()));
            }
            let hash = hash_password(&password).map_err(err_to_client)?;
            let user = components
                .database
                .create_user(&username, &hash)
                .await
                .map_err(err_to_client)?;
            let secret = &components.config.auth.jwt_secret;
            let token = create_jwt(&user.id, &user.username, user.is_admin, secret, hours)
                .map_err(err_to_client)?;
            Ok(LoginResponse { token })
        })
        .await
    }

    async fn me(&self) -> ClientResult<User> {
        let components = self.components.clone();
        let uid = self.user_id().await?;
        self.run(async move {
            let user = components
                .database
                .find_user_by_id(&uid)
                .await
                .map_err(err_to_client)?
                .ok_or(ClientError::Unauthorized)?;
            Ok(User {
                id: user.id,
                username: user.username,
                is_admin: user.is_admin,
                created_at: user.created_at,
                password_hash: String::new(),
            })
        })
        .await
    }

    // -------- search / browse --------

    async fn search(&self, query: &str, page: u32) -> ClientResult<SearchResponse> {
        let components = self.components.clone();
        let q = query.trim().to_string();
        let uid = self.user_id().await.ok();
        self.run(async move {
            if q.is_empty() {
                return Err(ClientError::Backend("Query must not be empty".into()));
            }
            let sr = components
                .search_provider
                .search(&q, page.max(1))
                .await
                .map_err(err_to_client)?;
            if let Some(uid) = uid {
                let _ = components
                    .database
                    .add_search(&uid, &q, sr.results.len() as i32)
                    .await;
            }
            Ok(sr)
        })
        .await
    }

    async fn browse(&self, p: &BrowseParams) -> ClientResult<streamx_api::types::BrowseResponse> {
        let components = self.components.clone();
        let sort_by = p.sort_by.clone().unwrap_or_else(|| "date_added".into());
        let query_term = p.query_term.clone();
        let genre = p.genre.clone();
        let minimum_rating = p.minimum_rating;
        let limit = p.limit.unwrap_or(10).min(20);
        let page = p.page.unwrap_or(1);
        self.run(async move {
            components
                .search_provider
                .browse(
                    &sort_by,
                    query_term.as_deref(),
                    genre.as_deref(),
                    minimum_rating,
                    limit,
                    page,
                )
                .await
                .map_err(err_to_client)
        })
        .await
    }

    async fn search_providers(&self) -> ClientResult<Vec<streamx_api::types::ProviderInfo>> {
        Ok(self.components.search_provider.provider_infos())
    }

    // -------- streams --------

    async fn create_stream(&self, req: &CreateStreamRequest) -> ClientResult<CreateStreamResponse> {
        let components = self.components.clone();
        let magnet = req.magnet_uri.trim().to_string();
        let file_index = req.file_index;
        let poster_url = req.poster_url.clone();
        let uid = self.user_id().await.ok();
        self.run(async move {
            if magnet.is_empty() || magnet.len() > 2048 {
                return Err(ClientError::Backend("Invalid magnet URI".into()));
            }
            let download = components
                .torrent_engine
                .add_magnet(&magnet, file_index)
                .await
                .map_err(err_to_client)?;
            if let Some(uid) = uid {
                let title = if download.title.is_empty() {
                    download.info_hash.as_str()
                } else {
                    download.title.as_str()
                };
                let _ = components
                    .database
                    .add_watch(&uid, &magnet, title, None, poster_url.as_deref())
                    .await;
            }
            Ok(CreateStreamResponse {
                stream_id: download.info_hash,
                status: download.status,
                title: download.title,
                file_name: if download.file_name.is_empty() {
                    None
                } else {
                    Some(download.file_name)
                },
            })
        })
        .await
    }

    async fn stream_status(&self, stream_id: &str) -> ClientResult<StreamStatus> {
        let components = self.components.clone();
        let sid = stream_id.to_string();
        self.run(async move {
            let download = components
                .torrent_engine
                .get_download(&sid)
                .await
                .map_err(err_to_client)?
                .ok_or_else(|| ClientError::Backend(format!("Stream {sid} not found")))?;
            let (peers, speed) = components.torrent_engine.get_live_stats(&sid).await;
            Ok(StreamStatus {
                id: download.info_hash,
                status: download.status,
                progress: download.progress as f32,
                title: download.title,
                file_name: download.file_name,
                file_size: download.file_size,
                peers,
                speed_bps: speed,
            })
        })
        .await
    }

    async fn stream_files(
        &self,
        stream_id: &str,
    ) -> ClientResult<(Vec<TorrentFile>, Option<String>)> {
        let components = self.components.clone();
        let sid = stream_id.to_string();
        self.run(async move {
            let download = components
                .torrent_engine
                .get_download(&sid)
                .await
                .map_err(err_to_client)?;
            let status = download.as_ref().map(|d| d.status.clone());
            let sorted = crate::torrent::files::sorted_torrent_files(
                &components.torrent_engine,
                &sid,
                download.as_ref(),
            )
            .await;
            let out = sorted
                .into_iter()
                .map(|f| TorrentFile {
                    index: f.seq_index,
                    path: f.path,
                    size: f.size,
                    is_video: f.is_video,
                    is_audio: f.is_audio,
                })
                .collect();
            Ok((out, status))
        })
        .await
    }

    // -------- history / favourites / playlists --------

    async fn history(&self) -> ClientResult<WatchHistoryResponse> {
        let components = self.components.clone();
        let uid = self.user_id().await?;
        self.run(async move {
            let raw = components
                .database
                .get_watch_history_enriched(&uid)
                .await
                .map_err(err_to_client)?;
            let items = raw
                .into_iter()
                .map(|e| WatchHistoryItem {
                    id: e.id,
                    magnet_uri: e.magnet_uri,
                    title: e.title,
                    file_name: e.file_name,
                    duration_seconds: e.duration_seconds,
                    watched_seconds: e.watched_seconds,
                    poster_url: e.poster_url,
                    watched_at: e.watched_at,
                    info_hash: e.info_hash,
                    file_size: e.file_size,
                    year: e.year,
                    rating: e.rating,
                    runtime: e.runtime,
                    genres: e.genres,
                    summary: e.summary,
                    imdb_code: e.imdb_code,
                })
                .collect();
            Ok(WatchHistoryResponse { items })
        })
        .await
    }

    async fn favourites(&self) -> ClientResult<FavouritesResponse> {
        let components = self.components.clone();
        let uid = self.user_id().await?;
        self.run(async move {
            let items = components
                .database
                .get_favourites(&uid, None)
                .await
                .map_err(err_to_client)?;
            Ok(FavouritesResponse { items })
        })
        .await
    }

    async fn playlists(&self) -> ClientResult<Vec<Playlist>> {
        let components = self.components.clone();
        let uid = self.user_id().await?;
        self.run(async move {
            components
                .database
                .get_playlists(&uid)
                .await
                .map_err(err_to_client)
        })
        .await
    }

    async fn playlist_tracks(&self, playlist_id: &str) -> ClientResult<Vec<PlaylistTrack>> {
        let components = self.components.clone();
        let pid = playlist_id.to_string();
        self.run(async move {
            components
                .database
                .get_playlist_tracks(&pid)
                .await
                .map_err(err_to_client)
        })
        .await
    }

    // -------- music / music videos / tv --------

    async fn search_music(&self, query: &str) -> ClientResult<MusicVideoSearchResponse> {
        let components = self.components.clone();
        let q = query.trim().to_string();
        self.run(async move {
            if q.is_empty() {
                return Err(ClientError::Backend("Query must not be empty".into()));
            }
            let raw = components
                .search_provider
                .search_music(&q)
                .await
                .map_err(err_to_client)?;
            Ok(MusicVideoSearchResponse {
                results: raw.into_iter().map(music_result).collect(),
            })
        })
        .await
    }

    async fn browse_music(&self, page: u32) -> ClientResult<MusicVideoSearchResponse> {
        let components = self.components.clone();
        self.run(async move {
            let raw = components
                .search_provider
                .browse_music(page.max(1))
                .await
                .map_err(err_to_client)?;
            Ok(MusicVideoSearchResponse {
                results: raw.into_iter().map(music_result).collect(),
            })
        })
        .await
    }

    async fn search_music_videos(&self, query: &str) -> ClientResult<MusicVideoSearchResponse> {
        let components = self.components.clone();
        let q = query.trim().to_string();
        self.run(async move {
            if q.is_empty() {
                return Err(ClientError::Backend("Query must not be empty".into()));
            }
            let raw = components
                .search_provider
                .search_music_videos(&q)
                .await
                .map_err(err_to_client)?;
            Ok(MusicVideoSearchResponse {
                results: raw.into_iter().map(music_result).collect(),
            })
        })
        .await
    }

    async fn browse_music_videos(&self, page: u32) -> ClientResult<MusicVideoSearchResponse> {
        let components = self.components.clone();
        self.run(async move {
            let raw = components
                .search_provider
                .browse_music_videos(page.max(1))
                .await
                .map_err(err_to_client)?;
            Ok(MusicVideoSearchResponse {
                results: raw.into_iter().map(music_result).collect(),
            })
        })
        .await
    }

    async fn search_tv(&self, query: &str) -> ClientResult<TvSearchResponse> {
        let components = self.components.clone();
        let q = query.trim().to_string();
        self.run(async move {
            if q.is_empty() {
                return Err(ClientError::Backend("Query must not be empty".into()));
            }
            let results = components
                .search_provider
                .search_tv(&q)
                .await
                .map_err(err_to_client)?;
            Ok(TvSearchResponse { results })
        })
        .await
    }

    async fn browse_tv(&self, page: u32) -> ClientResult<TvSearchResponse> {
        let components = self.components.clone();
        self.run(async move {
            let results = components
                .search_provider
                .browse_tv(page.max(1), 20)
                .await
                .map_err(err_to_client)?;
            Ok(TvSearchResponse { results })
        })
        .await
    }

    async fn resolve_magnet(
        &self,
        _api_base: &str,
        detail_url: &str,
    ) -> ClientResult<ResolveMagnetResponse> {
        let components = self.components.clone();
        let url = detail_url.to_string();
        self.run(async move {
            let magnet = components
                .search_provider
                .get_magnet(&url)
                .await
                .map_err(err_to_client)?
                .ok_or_else(|| ClientError::Backend("Could not resolve magnet".into()))?;
            Ok(ResolveMagnetResponse { magnet })
        })
        .await
    }

    async fn pin_download(&self, stream_id: &str) -> ClientResult<()> {
        let components = self.components.clone();
        let sid = stream_id.to_string();
        let _ = self.user_id().await?;
        self.run(async move {
            let dl = components
                .torrent_engine
                .get_download(&sid)
                .await
                .map_err(err_to_client)?
                .ok_or_else(|| ClientError::Backend(format!("Stream {sid} not found")))?;
            components
                .database
                .set_download_pinned(&sid, true)
                .await
                .map_err(err_to_client)?;
            if dl.status != "complete" {
                components
                    .torrent_engine
                    .resume(&sid)
                    .await
                    .map_err(err_to_client)?;
            }
            Ok(())
        })
        .await
    }

    async fn unpin_download(&self, stream_id: &str) -> ClientResult<()> {
        let components = self.components.clone();
        let sid = stream_id.to_string();
        let _ = self.user_id().await?;
        self.run(async move {
            components
                .database
                .set_download_pinned(&sid, false)
                .await
                .map_err(err_to_client)?;
            if components.torrent_engine.watched_within(&sid, 30) {
                tracing::info!(stream_id = %sid, "unpinned; connected viewer keeps it active");
            } else {
                let _ = components.torrent_engine.pause(&sid).await;
            }
            Ok(())
        })
        .await
    }

    async fn list_downloads(&self) -> ClientResult<Vec<streamx_api::types::DownloadItem>> {
        let components = self.components.clone();
        let _ = self.user_id().await?;
        self.run(async move {
            let downloads = components
                .database
                .list_downloads()
                .await
                .map_err(err_to_client)?;
            let mut items = Vec::with_capacity(downloads.len());
            for dl in downloads {
                let (peers, speed) = components
                    .torrent_engine
                    .get_live_stats(&dl.info_hash)
                    .await;
                let title = if dl.title.is_empty() {
                    components
                        .database
                        .get_metadata(&dl.info_hash)
                        .await
                        .ok()
                        .flatten()
                        .map(|m| m.title)
                        .filter(|t| !t.is_empty())
                        .unwrap_or_default()
                } else {
                    dl.title.clone()
                };
                items.push(streamx_api::types::DownloadItem {
                    info_hash: dl.info_hash,
                    magnet_uri: dl.magnet_uri,
                    title,
                    file_name: dl.file_name,
                    file_size: dl.file_size,
                    status: dl.status,
                    progress: dl.progress,
                    pinned: dl.pinned,
                    download_all: dl.download_all,
                    created_at: dl.created_at,
                    updated_at: dl.updated_at,
                    peers,
                    speed,
                });
            }
            Ok(items)
        })
        .await
    }

    async fn trailer_search(&self, query: &str) -> ClientResult<String> {
        let components = self.components.clone();
        let q = query.to_string();
        let _ = self.user_id().await?;
        self.run(async move {
            let state = crate::server::AppState::from_components(&components);
            crate::server::api::resolve_trailer_id(&state, &q)
                .await
                .map_err(err_to_client)
        })
        .await
    }

    async fn delete_stream(&self, stream_id: &str) -> ClientResult<()> {
        let components = self.components.clone();
        let sid = stream_id.to_string();
        let uid = self.user_id().await?;
        self.run(async move {
            let user = components
                .database
                .find_user_by_id(&uid)
                .await
                .map_err(err_to_client)?;
            if !user.map(|u| u.is_admin).unwrap_or(false) {
                return Err(ClientError::Unauthorized);
            }
            let state = crate::server::AppState::from_components(&components);
            crate::server::api::cleanup_stream(&state, &sid)
                .await
                .map_err(err_to_client)?;
            components
                .database
                .delete_download(&sid)
                .await
                .map_err(err_to_client)?;
            Ok(())
        })
        .await
    }

    async fn restart_torrent(&self) -> ClientResult<()> {
        let components = self.components.clone();
        let uid = self.user_id().await?;
        self.run(async move {
            let user = components
                .database
                .find_user_by_id(&uid)
                .await
                .map_err(err_to_client)?;
            if !user.map(|u| u.is_admin).unwrap_or(false) {
                return Err(ClientError::Unauthorized);
            }
            components
                .torrent_engine
                .restart_session()
                .await
                .map_err(err_to_client)?;
            Ok(())
        })
        .await
    }

    async fn admin_kill_stream(&self, stream_id: &str) -> ClientResult<()> {
        let components = self.components.clone();
        let sid = stream_id.to_string();
        let uid = self.user_id().await?;
        // Admin check can happen on the caller thread because it's
        // synchronous after `user_id()`.
        let is_admin = self
            .run({
                let components = components.clone();
                let uid = uid.clone();
                async move {
                    let u = components
                        .database
                        .find_user_by_id(&uid)
                        .await
                        .map_err(err_to_client)?;
                    Ok(u.map(|u| u.is_admin).unwrap_or(false))
                }
            })
            .await?;
        if !is_admin {
            return Err(ClientError::Unauthorized);
        }
        let _ = components;
        // Mirrors admin::kill_transcode: SIGTERM any ffmpeg child whose
        // cmdline contains the stream id. /proc-scan, Linux-only.
        #[cfg(target_os = "linux")]
        {
            if let Ok(entries) = std::fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if !name_str.chars().all(|c| c.is_ascii_digit()) {
                        continue;
                    }
                    let pid: i32 = match name_str.parse() {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    let cmdline = match std::fs::read_to_string(entry.path().join("cmdline")) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    if cmdline.contains("ffmpeg") && cmdline.contains(&sid) {
                        tracing::info!(stream_id = %sid, pid, "LocalApi killing FFmpeg process");
                        unsafe {
                            libc::kill(pid, libc::SIGTERM);
                        }
                    }
                }
            }
        }
        let _ = sid;
        Ok(())
    }
}

/// Convert an internal search-provider music hit to the wire type.
fn music_result(r: crate::torrent::provider::MusicVideoResult) -> WireMusic {
    WireMusic {
        title: r.title,
        magnet: r.magnet,
        seeds: r.seeds,
        leeches: r.leeches,
        size: r.size,
        detail_url: r.detail_url,
        date: r.date,
    }
}
