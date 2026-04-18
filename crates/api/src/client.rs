//! Thin HTTP client used by the StreamX desktop app (and any future CLI).
//!
//! Only compiled when the `client` feature is enabled.

use crate::routes;
use crate::types::{
    CreateStreamRequest, CreateStreamResponse, FavouritesResponse, LoginRequest, LoginResponse,
    Playlist, PlaylistTrack, SearchRequest, SearchResponse, SearchResultGroup, User,
    VersionResponse, WatchHistoryResponse,
};
use reqwest::{Client as HttpClient, StatusCode};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("unauthorized")]
    Unauthorized,
    #[error("server returned {status}: {body}")]
    Server { status: StatusCode, body: String },
}

pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Debug, Clone)]
pub struct Client {
    http: HttpClient,
    base_url: String,
    token: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BrowseParams {
    pub sort_by: Option<String>,
    pub genre: Option<String>,
    pub minimum_rating: Option<u32>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
}

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

impl Client {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: HttpClient::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: None,
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn set_token(&mut self, token: Option<String>) {
        self.token = token;
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
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

    // ---------- meta ----------

    pub async fn version(&self) -> ClientResult<VersionResponse> {
        Self::decode(self.http.get(self.url(routes::VERSION)).send().await?).await
    }

    // ---------- auth ----------

    pub async fn login(&self, username: &str, password: &str) -> ClientResult<LoginResponse> {
        let body = LoginRequest {
            username: username.to_string(),
            password: password.to_string(),
        };
        Self::decode(
            self.http
                .post(self.url(routes::AUTH_LOGIN))
                .json(&body)
                .send()
                .await?,
        )
        .await
    }

    pub async fn register(&self, username: &str, password: &str) -> ClientResult<LoginResponse> {
        let body = LoginRequest {
            username: username.to_string(),
            password: password.to_string(),
        };
        Self::decode(
            self.http
                .post(self.url(routes::AUTH_REGISTER))
                .json(&body)
                .send()
                .await?,
        )
        .await
    }

    pub async fn me(&self) -> ClientResult<User> {
        Self::decode(self.authed(self.http.get(self.url(routes::AUTH_ME))).send().await?).await
    }

    // ---------- search / browse ----------

    pub async fn search(&self, query: &str, page: u32) -> ClientResult<SearchResponse> {
        let body = SearchRequest {
            query: query.to_string(),
            page,
        };
        Self::decode(
            self.authed(self.http.post(self.url("/api/search")).json(&body))
                .send()
                .await?,
        )
        .await
    }

    pub async fn browse(&self, p: &BrowseParams) -> ClientResult<Vec<SearchResultGroup>> {
        let mut req = self.http.get(self.url("/api/search/browse"));
        let mut qp: Vec<(&str, String)> = Vec::new();
        if let Some(v) = &p.sort_by {
            qp.push(("sort_by", v.clone()));
        }
        if let Some(v) = &p.genre {
            qp.push(("genre", v.clone()));
        }
        if let Some(v) = p.minimum_rating {
            qp.push(("minimum_rating", v.to_string()));
        }
        if let Some(v) = p.limit {
            qp.push(("limit", v.to_string()));
        }
        if let Some(v) = p.page {
            qp.push(("page", v.to_string()));
        }
        if !qp.is_empty() {
            req = req.query(&qp);
        }
        let env: BrowseEnvelope = Self::decode(self.authed(req).send().await?).await?;
        Ok(env.results)
    }

    // ---------- streams ----------

    pub async fn create_stream(
        &self,
        req: &CreateStreamRequest,
    ) -> ClientResult<CreateStreamResponse> {
        Self::decode(
            self.authed(self.http.post(self.url("/api/stream")).json(req))
                .send()
                .await?,
        )
        .await
    }

    pub async fn stream_files(
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

    // ---------- history / favourites ----------

    pub async fn history(&self) -> ClientResult<WatchHistoryResponse> {
        Self::decode(self.authed(self.http.get(self.url("/api/history"))).send().await?).await
    }

    pub async fn favourites(&self) -> ClientResult<FavouritesResponse> {
        Self::decode(self.authed(self.http.get(self.url("/api/favourites"))).send().await?).await
    }

    // ---------- playlists ----------

    pub async fn playlists(&self) -> ClientResult<Vec<Playlist>> {
        let env: PlaylistsEnvelope = Self::decode(
            self.authed(self.http.get(self.url(routes::PLAYLISTS))).send().await?,
        )
        .await?;
        Ok(env.playlists)
    }

    pub async fn playlist_tracks(
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
}
