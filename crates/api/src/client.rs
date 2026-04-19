//! Typed client used by the StreamX desktop app.
//!
//! Two backends share the [`Api`] trait:
//!   - [`HttpClient`] talks to a remote server over HTTP (reqwest).
//!   - `LocalApi` (in the server crate) calls server internals directly,
//!     skipping serialization and TCP. Both are wrapped in the public
//!     [`Client`] struct which the desktop code uses.

use crate::routes;
use crate::types::{
    CreateStreamRequest, CreateStreamResponse, FavouritesResponse, LoginRequest, LoginResponse,
    MusicVideoSearchResponse, Playlist, PlaylistTrack, ResolveMagnetResponse, SearchRequest,
    SearchResponse, SearchResultGroup, StreamStatus, TvSearchResponse, User, VersionResponse,
    WatchHistoryResponse,
};
use async_trait::async_trait;
use parking_lot::RwLock;
use reqwest::{Client as HttpInner, StatusCode};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("unauthorized")]
    Unauthorized,
    #[error("server returned {status}: {body}")]
    Server { status: StatusCode, body: String },
    #[error("{0}")]
    Backend(String),
}

pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Debug, Clone, Default)]
pub struct BrowseParams {
    pub sort_by: Option<String>,
    pub genre: Option<String>,
    pub minimum_rating: Option<u32>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
}

/// Transport-agnostic API surface. Both the HTTP client and the in-process
/// LocalApi implement this.
#[async_trait]
pub trait Api: Send + Sync {
    fn base_url(&self) -> String;
    fn token(&self) -> Option<String>;
    fn set_token(&self, token: Option<String>);

    async fn version(&self) -> ClientResult<VersionResponse>;
    async fn login(&self, username: &str, password: &str) -> ClientResult<LoginResponse>;
    async fn register(&self, username: &str, password: &str) -> ClientResult<LoginResponse>;
    async fn me(&self) -> ClientResult<User>;
    async fn search(&self, query: &str, page: u32) -> ClientResult<SearchResponse>;
    async fn browse(&self, params: &BrowseParams) -> ClientResult<Vec<SearchResultGroup>>;
    async fn create_stream(
        &self,
        req: &CreateStreamRequest,
    ) -> ClientResult<CreateStreamResponse>;
    async fn stream_files(
        &self,
        stream_id: &str,
    ) -> ClientResult<(Vec<crate::types::TorrentFile>, Option<String>)>;
    async fn stream_status(&self, stream_id: &str) -> ClientResult<StreamStatus>;
    async fn history(&self) -> ClientResult<WatchHistoryResponse>;
    async fn favourites(&self) -> ClientResult<FavouritesResponse>;
    async fn playlists(&self) -> ClientResult<Vec<Playlist>>;
    async fn playlist_tracks(&self, playlist_id: &str) -> ClientResult<Vec<PlaylistTrack>>;
    async fn search_music(&self, query: &str) -> ClientResult<MusicVideoSearchResponse>;
    async fn browse_music(&self, page: u32) -> ClientResult<MusicVideoSearchResponse>;
    async fn search_music_videos(&self, query: &str) -> ClientResult<MusicVideoSearchResponse>;
    async fn browse_music_videos(&self, page: u32) -> ClientResult<MusicVideoSearchResponse>;
    async fn search_tv(&self, query: &str) -> ClientResult<TvSearchResponse>;
    async fn browse_tv(&self, page: u32) -> ClientResult<TvSearchResponse>;
    async fn resolve_magnet(
        &self,
        api_base: &str,
        detail_url: &str,
    ) -> ClientResult<ResolveMagnetResponse>;
    async fn admin_kill_stream(&self, stream_id: &str) -> ClientResult<()>;
}

/// Public cloneable handle used across the desktop app. Internally holds
/// an `Arc<dyn Api>`, so both HTTP and in-process backends are swappable.
#[derive(Clone)]
pub struct Client {
    inner: Arc<dyn Api + Send + Sync>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url())
            .finish()
    }
}

impl Client {
    /// HTTP-backed client (thin-client mode).
    pub fn http(base_url: impl Into<String>) -> Self {
        Self { inner: Arc::new(HttpClient::new(base_url)) }
    }

    /// Legacy alias.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::http(base_url)
    }

    /// Direct in-process backend (embedded mode).
    pub fn from_api(api: Arc<dyn Api + Send + Sync>) -> Self {
        Self { inner: api }
    }

    pub fn base_url(&self) -> String {
        self.inner.base_url()
    }

    pub fn token(&self) -> Option<String> {
        self.inner.token()
    }

    pub fn set_token(&mut self, token: Option<String>) {
        self.inner.set_token(token);
    }

    pub fn with_token(self, token: impl Into<String>) -> Self {
        self.inner.set_token(Some(token.into()));
        self
    }
}

// Delegate every trait method on Client. Boring but mechanical.
macro_rules! delegate {
    ($name:ident ( &self $(, $arg:ident : $ty:ty )* ) -> $ret:ty) => {
        pub async fn $name(&self $(, $arg : $ty )*) -> $ret {
            self.inner.$name($($arg),*).await
        }
    };
}

impl Client {
    delegate!(version(&self) -> ClientResult<VersionResponse>);
    delegate!(login(&self, username: &str, password: &str) -> ClientResult<LoginResponse>);
    delegate!(register(&self, username: &str, password: &str) -> ClientResult<LoginResponse>);
    delegate!(me(&self) -> ClientResult<User>);
    delegate!(search(&self, query: &str, page: u32) -> ClientResult<SearchResponse>);
    delegate!(browse(&self, params: &BrowseParams) -> ClientResult<Vec<SearchResultGroup>>);
    delegate!(create_stream(&self, req: &CreateStreamRequest) -> ClientResult<CreateStreamResponse>);
    delegate!(stream_files(&self, stream_id: &str) -> ClientResult<(Vec<crate::types::TorrentFile>, Option<String>)>);
    delegate!(stream_status(&self, stream_id: &str) -> ClientResult<StreamStatus>);
    delegate!(history(&self) -> ClientResult<WatchHistoryResponse>);
    delegate!(favourites(&self) -> ClientResult<FavouritesResponse>);
    delegate!(playlists(&self) -> ClientResult<Vec<Playlist>>);
    delegate!(playlist_tracks(&self, playlist_id: &str) -> ClientResult<Vec<PlaylistTrack>>);
    delegate!(search_music(&self, query: &str) -> ClientResult<MusicVideoSearchResponse>);
    delegate!(browse_music(&self, page: u32) -> ClientResult<MusicVideoSearchResponse>);
    delegate!(search_music_videos(&self, query: &str) -> ClientResult<MusicVideoSearchResponse>);
    delegate!(browse_music_videos(&self, page: u32) -> ClientResult<MusicVideoSearchResponse>);
    delegate!(search_tv(&self, query: &str) -> ClientResult<TvSearchResponse>);
    delegate!(browse_tv(&self, page: u32) -> ClientResult<TvSearchResponse>);
    delegate!(resolve_magnet(&self, api_base: &str, detail_url: &str) -> ClientResult<ResolveMagnetResponse>);
    delegate!(admin_kill_stream(&self, stream_id: &str) -> ClientResult<()>);
}

// ===================== HttpClient =====================

#[derive(Deserialize, Debug)]
struct BrowseEnvelope {
    #[serde(default)]
    results: Vec<SearchResultGroup>,
}

#[derive(Deserialize, Debug)]
struct FilesEnvelope {
    #[serde(default)]
    files: Vec<crate::types::TorrentFile>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize, Debug)]
struct PlaylistsEnvelope {
    #[serde(default)]
    playlists: Vec<Playlist>,
}

#[derive(Deserialize, Debug)]
struct TracksEnvelope {
    #[serde(default)]
    tracks: Vec<PlaylistTrack>,
}

#[derive(Debug)]
pub struct HttpClient {
    http: HttpInner,
    base_url: String,
    token: RwLock<Option<String>>,
}

impl HttpClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: HttpInner::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: RwLock::new(None),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &*self.token.read() {
            Some(t) => req.bearer_auth(t),
            None => req,
        }
    }

    async fn decode<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
    ) -> ClientResult<T> {
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(ClientError::Unauthorized);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Server { status, body });
        }
        Ok(resp.json::<T>().await?)
    }

    async fn post_search<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &str,
    ) -> ClientResult<T> {
        let body = SearchRequest { query: query.to_string(), page: 1 };
        Self::decode(
            self.authed(self.http.post(self.url(path)).json(&body))
                .send()
                .await?,
        )
        .await
    }
}

#[async_trait]
impl Api for HttpClient {
    fn base_url(&self) -> String {
        self.base_url.clone()
    }

    fn token(&self) -> Option<String> {
        self.token.read().clone()
    }

    fn set_token(&self, token: Option<String>) {
        *self.token.write() = token;
    }

    async fn version(&self) -> ClientResult<VersionResponse> {
        Self::decode(self.http.get(self.url(routes::VERSION)).send().await?).await
    }

    async fn login(&self, username: &str, password: &str) -> ClientResult<LoginResponse> {
        let body = LoginRequest {
            username: username.to_string(),
            password: password.to_string(),
        };
        Self::decode(
            self.http
                .post(self.url(routes::LOGIN))
                .json(&body)
                .send()
                .await?,
        )
        .await
    }

    async fn register(&self, username: &str, password: &str) -> ClientResult<LoginResponse> {
        let body = LoginRequest {
            username: username.to_string(),
            password: password.to_string(),
        };
        Self::decode(
            self.http
                .post(self.url(routes::REGISTER))
                .json(&body)
                .send()
                .await?,
        )
        .await
    }

    async fn me(&self) -> ClientResult<User> {
        Self::decode(self.authed(self.http.get(self.url(routes::ME))).send().await?).await
    }

    async fn search(&self, query: &str, page: u32) -> ClientResult<SearchResponse> {
        let body = SearchRequest { query: query.to_string(), page };
        Self::decode(
            self.authed(self.http.post(self.url(routes::SEARCH)).json(&body))
                .send()
                .await?,
        )
        .await
    }

    async fn browse(&self, p: &BrowseParams) -> ClientResult<Vec<SearchResultGroup>> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(ref v) = p.sort_by {
            params.push(("sort_by", v.clone()));
        }
        if let Some(ref v) = p.genre {
            params.push(("genre", v.clone()));
        }
        if let Some(v) = p.minimum_rating {
            params.push(("minimum_rating", v.to_string()));
        }
        if let Some(v) = p.limit {
            params.push(("limit", v.to_string()));
        }
        if let Some(v) = p.page {
            params.push(("page", v.to_string()));
        }
        let env: BrowseEnvelope = Self::decode(
            self.authed(self.http.get(self.url(routes::BROWSE)).query(&params))
                .send()
                .await?,
        )
        .await?;
        Ok(env.results)
    }

    async fn create_stream(
        &self,
        req: &CreateStreamRequest,
    ) -> ClientResult<CreateStreamResponse> {
        Self::decode(
            self.authed(self.http.post(self.url(routes::CREATE_STREAM)).json(req))
                .send()
                .await?,
        )
        .await
    }

    async fn stream_files(
        &self,
        stream_id: &str,
    ) -> ClientResult<(Vec<crate::types::TorrentFile>, Option<String>)> {
        let env: FilesEnvelope = Self::decode(
            self.authed(self.http.get(self.url(&routes::stream_files(stream_id))))
                .send()
                .await?,
        )
        .await?;
        Ok((env.files, env.status))
    }

    async fn stream_status(&self, stream_id: &str) -> ClientResult<StreamStatus> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            status: String,
            #[serde(default)]
            progress: f32,
            #[serde(default)]
            title: String,
            #[serde(default)]
            file_name: String,
            #[serde(default)]
            file_size: u64,
            #[serde(default)]
            peers: u32,
            #[serde(default)]
            speed: f64,
        }
        let raw: Raw = Self::decode(
            self.authed(self.http.get(self.url(&format!("/api/stream/{stream_id}"))))
                .send()
                .await?,
        )
        .await?;
        Ok(StreamStatus {
            id: raw.id,
            status: raw.status,
            progress: raw.progress,
            title: raw.title,
            file_name: raw.file_name,
            file_size: raw.file_size,
            peers: raw.peers,
            speed_bps: raw.speed,
        })
    }

    async fn history(&self) -> ClientResult<WatchHistoryResponse> {
        Self::decode(self.authed(self.http.get(self.url("/api/history"))).send().await?).await
    }

    async fn favourites(&self) -> ClientResult<FavouritesResponse> {
        Self::decode(self.authed(self.http.get(self.url("/api/favourites"))).send().await?).await
    }

    async fn playlists(&self) -> ClientResult<Vec<Playlist>> {
        let env: PlaylistsEnvelope = Self::decode(
            self.authed(self.http.get(self.url(routes::PLAYLISTS))).send().await?,
        )
        .await?;
        Ok(env.playlists)
    }

    async fn playlist_tracks(
        &self,
        playlist_id: &str,
    ) -> ClientResult<Vec<PlaylistTrack>> {
        let env: TracksEnvelope = Self::decode(
            self.authed(
                self.http.get(self.url(&routes::playlist_tracks(playlist_id))),
            )
            .send()
            .await?,
        )
        .await?;
        Ok(env.tracks)
    }

    async fn search_music(&self, query: &str) -> ClientResult<MusicVideoSearchResponse> {
        self.post_search("/api/music/search", query).await
    }

    async fn browse_music(&self, page: u32) -> ClientResult<MusicVideoSearchResponse> {
        let url = self.url(&format!("/api/music/browse?page={page}"));
        Self::decode(self.authed(self.http.get(url)).send().await?).await
    }

    async fn search_music_videos(
        &self,
        query: &str,
    ) -> ClientResult<MusicVideoSearchResponse> {
        self.post_search("/api/music-videos/search", query).await
    }

    async fn browse_music_videos(
        &self,
        page: u32,
    ) -> ClientResult<MusicVideoSearchResponse> {
        let url = self.url(&format!("/api/music-videos/browse?page={page}"));
        Self::decode(self.authed(self.http.get(url)).send().await?).await
    }

    async fn search_tv(&self, query: &str) -> ClientResult<TvSearchResponse> {
        let body = SearchRequest { query: query.to_string(), page: 1 };
        Self::decode(
            self.authed(self.http.post(self.url("/api/tv/search")).json(&body))
                .send()
                .await?,
        )
        .await
    }

    async fn browse_tv(&self, page: u32) -> ClientResult<TvSearchResponse> {
        let url = self.url(&format!("/api/tv/browse?page={page}"));
        Self::decode(self.authed(self.http.get(url)).send().await?).await
    }

    async fn resolve_magnet(
        &self,
        api_base: &str,
        detail_url: &str,
    ) -> ClientResult<ResolveMagnetResponse> {
        let url = self.url(&format!("/api/{}/resolve-magnet", api_base));
        let body = serde_json::json!({ "detail_url": detail_url });
        Self::decode(self.authed(self.http.post(url).json(&body)).send().await?).await
    }

    async fn admin_kill_stream(&self, stream_id: &str) -> ClientResult<()> {
        let url = self.url(&format!("/api/admin/kill/{}", stream_id));
        let resp = self.authed(self.http.delete(url)).send().await?;
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(ClientError::Unauthorized);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Server { status, body });
        }
        Ok(())
    }
}
