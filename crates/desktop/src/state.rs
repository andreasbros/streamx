//! Central application state shared between the window and async tasks.

use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use streamx_api::client::Client;
use streamx_api::types::{
    FavouriteItem, MusicVideoResult, SearchResultGroup, TvSearchResultGroup, User,
    WatchHistoryItem,
};

use crate::router::Page;

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub posted_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Embedded: server runs on localhost, media files resolved from local
    /// disk (no HTTP pressure for playback).
    Embedded,
    /// Thin client: server lives elsewhere, media streamed via HTTP range.
    ThinClient,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Embedded => "embedded",
            Mode::ThinClient => "thin-client",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "thin-client" => Mode::ThinClient,
            _ => Mode::Embedded,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct BrowseData {
    pub latest: Vec<SearchResultGroup>,
    pub popular: Vec<SearchResultGroup>,
    pub top_rated: Vec<SearchResultGroup>,
    pub action: Vec<SearchResultGroup>,
    pub comedy: Vec<SearchResultGroup>,
    pub thriller: Vec<SearchResultGroup>,
    pub scifi: Vec<SearchResultGroup>,
    pub horror: Vec<SearchResultGroup>,
}

pub struct AppState {
    pub client: RwLock<Client>,
    pub token: RwLock<Option<String>>,
    pub user: RwLock<Option<User>>,

    pub mode: RwLock<Mode>,
    pub server_url: RwLock<String>,
    pub server_version: RwLock<Option<String>>,
    pub server_hash: RwLock<Option<String>>,
    pub connection_error: RwLock<Option<String>>,

    /// Login form errors (surfaced back to login_page).
    pub login_error: RwLock<Option<String>>,
    /// Set true while an auth call is in flight.
    pub login_in_flight: RwLock<bool>,

    pub query: RwLock<String>,
    pub search_results: RwLock<Vec<SearchResultGroup>>,
    pub search_in_flight: RwLock<bool>,
    pub browse: RwLock<BrowseData>,
    pub browse_loading: RwLock<bool>,

    pub selected_movie: RwLock<Option<SearchResultGroup>>,
    pub toast: RwLock<Option<Toast>>,

    pub history: RwLock<Vec<WatchHistoryItem>>,
    pub history_loading: RwLock<bool>,
    pub favourites: RwLock<Vec<FavouriteItem>>,
    pub favourites_loading: RwLock<bool>,

    pub music_query: RwLock<String>,
    pub music_results: RwLock<Vec<MusicVideoResult>>,
    pub music_loading: RwLock<bool>,

    pub music_video_query: RwLock<String>,
    pub music_video_results: RwLock<Vec<MusicVideoResult>>,
    pub music_video_loading: RwLock<bool>,

    pub tv_query: RwLock<String>,
    pub tv_results: RwLock<Vec<TvSearchResultGroup>>,
    pub tv_loading: RwLock<bool>,
    pub selected_tv_show: RwLock<Option<TvSearchResultGroup>>,

    pub drawer_open: RwLock<bool>,

    pub page_stack: RwLock<Vec<Page>>,

    /// On-disk config dir (~/.config/streamx-desktop on Linux).
    pub config_dir: PathBuf,
    /// Data dir for the server (~/.streamx). Used in Embedded mode to
    /// resolve local playback paths directly.
    pub data_dir: PathBuf,
}

const DEFAULT_LOCAL_URL: &str = "http://localhost:8999";

impl AppState {
    pub fn new() -> Arc<Self> {
        // STREAMX_DESKTOP_CONFIG_OVERRIDE lets tests scope config to a
        // tempdir so they don't pollute (or be polluted by) real use.
        let config_dir = match std::env::var("STREAMX_DESKTOP_CONFIG_OVERRIDE") {
            Ok(p) if !p.is_empty() => PathBuf::from(p),
            _ => directories::ProjectDirs::from("com", "streamx", "streamx-desktop")
                .map(|d| d.config_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".")),
        };
        let _ = std::fs::create_dir_all(&config_dir);

        // Server data dir (matches streamx-server default)
        let data_dir = directories::UserDirs::new()
            .and_then(|d| d.home_dir().to_path_buf().into())
            .map(|h: PathBuf| h.join(".streamx"))
            .unwrap_or_else(|| PathBuf::from(".streamx"));

        // Load persisted session.
        let token_file = config_dir.join("token");
        let mode_file = config_dir.join("mode");
        let url_file = config_dir.join("server_url");

        let saved_token = std::fs::read_to_string(&token_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let saved_mode = std::fs::read_to_string(&mode_file)
            .ok()
            .map(|s| Mode::from_str(s.trim()))
            .unwrap_or(Mode::Embedded);
        let saved_url = std::fs::read_to_string(&url_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("STREAMX_URL").ok())
            .unwrap_or_else(|| DEFAULT_LOCAL_URL.to_string());

        let mut client = Client::new(saved_url.clone());
        if let Some(t) = saved_token.as_ref() {
            client.set_token(Some(t.clone()));
        }

        let initial_page = if saved_token.is_some() {
            Page::Search
        } else {
            Page::Login
        };

        Arc::new(Self {
            client: RwLock::new(client),
            token: RwLock::new(saved_token),
            user: RwLock::new(None),
            mode: RwLock::new(saved_mode),
            server_url: RwLock::new(saved_url),
            server_version: RwLock::new(None),
            server_hash: RwLock::new(None),
            connection_error: RwLock::new(None),
            login_error: RwLock::new(None),
            login_in_flight: RwLock::new(false),
            query: RwLock::new(String::new()),
            search_results: RwLock::new(Vec::new()),
            search_in_flight: RwLock::new(false),
            browse: RwLock::new(BrowseData::default()),
            browse_loading: RwLock::new(false),
            selected_movie: RwLock::new(None),
            toast: RwLock::new(None),
            history: RwLock::new(Vec::new()),
            history_loading: RwLock::new(false),
            favourites: RwLock::new(Vec::new()),
            favourites_loading: RwLock::new(false),
            music_query: RwLock::new(String::new()),
            music_results: RwLock::new(Vec::new()),
            music_loading: RwLock::new(false),
            music_video_query: RwLock::new(String::new()),
            music_video_results: RwLock::new(Vec::new()),
            music_video_loading: RwLock::new(false),
            tv_query: RwLock::new(String::new()),
            tv_results: RwLock::new(Vec::new()),
            tv_loading: RwLock::new(false),
            selected_tv_show: RwLock::new(None),
            drawer_open: RwLock::new(false),
            page_stack: RwLock::new(vec![initial_page]),
            config_dir,
            data_dir,
        })
    }

    /// Write to a file under `config_dir`, making sure the directory
    /// exists first. Silent on failure — persistence is best-effort.
    fn persist(&self, name: &str, value: &str) {
        let _ = std::fs::create_dir_all(&self.config_dir);
        let _ = std::fs::write(self.config_dir.join(name), value);
    }

    pub fn set_token(&self, token: Option<String>) {
        let path = self.config_dir.join("token");
        match &token {
            Some(t) => {
                let _ = std::fs::create_dir_all(&self.config_dir);
                let _ = std::fs::write(&path, t);
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        self.client.write().set_token(token.clone());
        *self.token.write() = token;
    }

    pub fn set_mode(&self, mode: Mode) {
        self.persist("mode", mode.as_str());
        *self.mode.write() = mode;
    }

    pub fn set_server_url(&self, url: String) {
        self.persist("server_url", &url);
        *self.server_url.write() = url.clone();
        *self.client.write() = {
            let mut c = Client::new(url);
            c.set_token(self.token.read().clone());
            c
        };
    }

    /// Replace the client with an in-process backend (Embedded mode).
    /// Keeps the current token.
    pub fn install_in_process_client(&self, api: std::sync::Arc<dyn streamx_api::client::Api + Send + Sync>) {
        let mut client = Client::from_api(api);
        client.set_token(self.token.read().clone());
        *self.client.write() = client;
    }

    pub fn is_authed(&self) -> bool {
        self.token.read().is_some()
    }

    pub fn current_page(&self) -> Page {
        self.page_stack.read().last().cloned().unwrap_or(Page::Login)
    }

    pub fn navigate(&self, page: Page) {
        self.page_stack.write().push(page);
    }

    pub fn back(&self) -> bool {
        let mut stack = self.page_stack.write();
        if stack.len() > 1 {
            stack.pop();
            true
        } else {
            false
        }
    }

    pub fn replace_page(&self, page: Page) {
        let mut stack = self.page_stack.write();
        stack.clear();
        stack.push(page);
    }

    pub fn show_toast(&self, message: impl Into<String>, kind: ToastKind) {
        *self.toast.write() = Some(Toast {
            message: message.into(),
            kind,
            posted_at: Instant::now(),
        });
    }

    pub fn clear_toast(&self) {
        *self.toast.write() = None;
    }
}
