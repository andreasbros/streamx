//! Central application state shared between the window and async tasks.

use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use streamx_api::client::BrowseParams;
use streamx_api::client::Client;
use streamx_api::types::{
    DownloadItem, FavouriteItem, MusicVideoResult, SearchResultGroup, TvSearchResultGroup, User,
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
    pub fn parse(s: &str) -> Self {
        match s {
            "thin-client" => Mode::ThinClient,
            _ => Mode::Embedded,
        }
    }
}

/// One home-page category (title + the browse query behind it).
#[derive(Debug, Clone)]
pub struct CategorySpec {
    pub title: &'static str,
    pub params: BrowseParams,
}

/// A poster path whose load failed. The tick loop evicts it from GPUI's
/// asset cache once `next_retry` passes so the image is fetched again,
/// with exponential backoff, like a browser retrying a broken <img>.
#[derive(Debug, Clone)]
pub struct PosterFailure {
    pub attempts: u32,
    pub next_retry: Instant,
}

const POSTER_RETRY_MAX_ATTEMPTS: u32 = 6;

/// Browse rows hold `Arc`s so per-frame rendering and click closures
/// clone pointers, not whole result groups (each carries every variant
/// with magnet URIs — deep-copying 80 of them per frame made resize and
/// typing sluggish).
#[derive(Debug, Default, Clone)]
pub struct BrowseData {
    pub this_year: Vec<Arc<SearchResultGroup>>,
    pub latest: Vec<Arc<SearchResultGroup>>,
    pub popular: Vec<Arc<SearchResultGroup>>,
    pub top_rated: Vec<Arc<SearchResultGroup>>,
    pub action: Vec<Arc<SearchResultGroup>>,
    pub comedy: Vec<Arc<SearchResultGroup>>,
    pub thriller: Vec<Arc<SearchResultGroup>>,
    pub scifi: Vec<Arc<SearchResultGroup>>,
    pub horror: Vec<Arc<SearchResultGroup>>,
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
    pub search_results: RwLock<Vec<Arc<SearchResultGroup>>>,
    pub search_in_flight: RwLock<bool>,
    /// Monotonic ids for in-flight searches, one per search domain.
    /// A response is applied only when its id is still the newest, so
    /// a slow response for an older query can never replace results of
    /// a newer one (out-of-order provider responses).
    pub search_generation: std::sync::atomic::AtomicU64,
    pub music_generation: std::sync::atomic::AtomicU64,
    pub music_video_generation: std::sync::atomic::AtomicU64,
    pub tv_generation: std::sync::atomic::AtomicU64,
    pub browse: RwLock<BrowseData>,
    pub browse_loading: RwLock<bool>,

    pub selected_movie: RwLock<Option<Arc<SearchResultGroup>>>,

    /// Category drill-down (clicking a home section title). Items append
    /// as infinite scroll fetches further pages.
    pub category: RwLock<Option<CategorySpec>>,
    pub category_items: RwLock<Vec<Arc<SearchResultGroup>>>,
    pub category_page: std::sync::atomic::AtomicU32,
    pub category_loading: RwLock<bool>,
    pub category_done: RwLock<bool>,
    /// Set by the virtualized category grid when the viewport nears the
    /// last row; the tick loop turns it into a page fetch.
    pub category_need_more: std::sync::atomic::AtomicBool,
    pub toast: RwLock<Option<Toast>>,

    /// Provider health surfaced to the UI: a request running longer
    /// than 3s marks the provider slow (url); a failed provider sets a
    /// dismissible error shown centered.
    pub provider_slow: RwLock<Option<String>>,
    pub provider_error: RwLock<Option<streamx_api::types::ProviderError>>,
    pub browse_started_at: RwLock<Option<Instant>>,
    pub provider_infos: RwLock<Vec<streamx_api::types::ProviderInfo>>,

    pub history: RwLock<Vec<WatchHistoryItem>>,
    pub history_loading: RwLock<bool>,
    pub downloads: RwLock<Vec<DownloadItem>>,
    pub downloads_loading: RwLock<bool>,
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

    /// Poster loads that failed, keyed by asset path. See [`PosterFailure`].
    pub poster_failures: Mutex<HashMap<String, PosterFailure>>,
    /// Poster paths with a download in flight (dedupe for spawn_fetch).
    pub poster_pending: Mutex<std::collections::HashSet<String>>,
    /// Poster paths whose bytes just landed on disk; the tick loop evicts
    /// them from GPUI's asset cache so they render on the next frame.
    pub poster_ready: Mutex<Vec<String>>,

    /// Set by async mutators; the tick loop only repaints when something
    /// actually changed instead of forcing a re-render every 100ms.
    pub dirty: std::sync::atomic::AtomicBool,
    /// Tick-loop heartbeat, exposed to the ui-test driver so a stalled
    /// loop is diagnosable from test output.
    pub tick_count: std::sync::atomic::AtomicU64,

    /// Synthetic input queue, filled by the ui-test driver and drained
    /// on the UI thread each tick. Keystroke strings use GPUI's
    /// `Keystroke::parse` syntax ("b", "enter", "shift-a", "cmd-k").
    pub ui_keys: Mutex<Vec<String>>,
    /// Screenshot requests (output paths) from the ui-test driver,
    /// served on the UI thread via `Window::render_to_image`.
    pub ui_shots: Mutex<Vec<String>>,
    /// Pending window resize from the ui-test driver (logical pixels).
    pub ui_resize: Mutex<Option<(f32, f32)>>,
    /// Mirror of the search input's current text, refreshed by the tick
    /// loop so the ui-test driver can assert on real typed content.
    pub search_input_mirror: RwLock<String>,

    /// On-disk config dir (~/.config/streamx-desktop on Linux).
    pub config_dir: PathBuf,
    /// Data dir for the server (~/.streamx). Used in Embedded mode to
    /// resolve local playback paths directly.
    pub data_dir: PathBuf,
    /// Torrent data root, honoring `torrent.download_dir` from the
    /// server config file. Local playback and posters resolve here.
    pub downloads_dir: PathBuf,
    /// Two-step delete: info_hash awaiting a confirming second click.
    pub confirm_delete: RwLock<Option<String>>,
    /// In-memory ring of recent log lines (shared with the embedded
    /// server's tracing) rendered by the Logs page.
    pub logs: Arc<streamx::logging::LogHistory>,
    /// Log length last rendered; the tick loop repaints the Logs page
    /// only when this differs, so logging never drives extra frames.
    pub logs_seen: std::sync::atomic::AtomicUsize,
}

const DEFAULT_LOCAL_URL: &str = "http://localhost:8999";

impl AppState {
    pub fn new() -> Arc<Self> {
        Self::with_logs(streamx::logging::LogHistory::new_shared())
    }

    pub fn with_logs(logs: Arc<streamx::logging::LogHistory>) -> Arc<Self> {
        // STREAMX_DESKTOP_CONFIG_OVERRIDE lets tests scope config to a
        // tempdir so they don't pollute (or be polluted by) real use.
        let config_dir = match std::env::var("STREAMX_DESKTOP_CONFIG_OVERRIDE") {
            Ok(p) if !p.is_empty() => PathBuf::from(p),
            _ => directories::ProjectDirs::from("com", "streamx", "streamx-desktop")
                .map(|d| d.config_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".")),
        };
        let _ = std::fs::create_dir_all(&config_dir);

        // Server data dir (matches streamx-server default). STREAMX_DATA_DIR
        // redirects it, mirroring the server's env override, so tests can
        // isolate every on-disk path.
        let data_dir = match std::env::var("STREAMX_DATA_DIR") {
            Ok(d) if !d.is_empty() => PathBuf::from(d),
            _ => directories::UserDirs::new()
                .and_then(|d| d.home_dir().to_path_buf().into())
                .map(|h: PathBuf| h.join(".streamx"))
                .unwrap_or_else(|| PathBuf::from(".streamx")),
        };

        let downloads_dir = streamx::config::downloads_dir_for(&data_dir);

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
            .map(|s| Mode::parse(s.trim()))
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
            search_generation: std::sync::atomic::AtomicU64::new(0),
            music_generation: std::sync::atomic::AtomicU64::new(0),
            music_video_generation: std::sync::atomic::AtomicU64::new(0),
            tv_generation: std::sync::atomic::AtomicU64::new(0),
            browse: RwLock::new(BrowseData::default()),
            browse_loading: RwLock::new(false),
            selected_movie: RwLock::new(None),
            category: RwLock::new(None),
            category_items: RwLock::new(Vec::new()),
            category_page: std::sync::atomic::AtomicU32::new(0),
            category_loading: RwLock::new(false),
            category_done: RwLock::new(false),
            category_need_more: std::sync::atomic::AtomicBool::new(false),
            toast: RwLock::new(None),
            provider_slow: RwLock::new(None),
            provider_error: RwLock::new(None),
            browse_started_at: RwLock::new(None),
            provider_infos: RwLock::new(Vec::new()),
            history: RwLock::new(Vec::new()),
            history_loading: RwLock::new(false),
            downloads: RwLock::new(Vec::new()),
            downloads_loading: RwLock::new(false),
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
            poster_failures: Mutex::new(HashMap::new()),
            poster_pending: Mutex::new(std::collections::HashSet::new()),
            poster_ready: Mutex::new(Vec::new()),
            dirty: std::sync::atomic::AtomicBool::new(true),
            tick_count: std::sync::atomic::AtomicU64::new(0),
            ui_keys: Mutex::new(Vec::new()),
            ui_shots: Mutex::new(Vec::new()),
            ui_resize: Mutex::new(None),
            search_input_mirror: RwLock::new(String::new()),
            config_dir,
            data_dir,
            downloads_dir,
            confirm_delete: RwLock::new(None),
            logs,
            logs_seen: std::sync::atomic::AtomicUsize::new(0),
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
    pub fn install_in_process_client(
        &self,
        api: std::sync::Arc<dyn streamx_api::client::Api + Send + Sync>,
    ) {
        let mut client = Client::from_api(api);
        client.set_token(self.token.read().clone());
        *self.client.write() = client;
    }

    pub fn is_authed(&self) -> bool {
        self.token.read().is_some()
    }

    pub fn current_page(&self) -> Page {
        self.page_stack
            .read()
            .last()
            .cloned()
            .unwrap_or(Page::Login)
    }

    /// Push a page like a browser history entry. No-op when already on
    /// that page; the stack is capped so long sessions can't grow it
    /// without bound.
    pub fn navigate(&self, page: Page) {
        *self.confirm_delete.write() = None;
        {
            let mut stack = self.page_stack.write();
            if stack.last() == Some(&page) {
                return;
            }
            if stack.len() >= 64 {
                stack.remove(0);
            }
            stack.push(page);
        }
        self.mark_dirty();
    }

    pub fn back(&self) -> bool {
        let moved = {
            let mut stack = self.page_stack.write();
            if stack.len() > 1 {
                stack.pop();
                true
            } else {
                false
            }
        };
        if moved {
            self.mark_dirty();
        }
        moved
    }

    pub fn replace_page(&self, page: Page) {
        {
            let mut stack = self.page_stack.write();
            stack.clear();
            stack.push(page);
        }
        self.mark_dirty();
    }

    /// Record a failed poster load, scheduling a retry with exponential
    /// backoff (2s, 4s, 8s, ... capped at 60s, max 6 attempts).
    pub fn mark_poster_failure(&self, path: &str) {
        let mut map = self.poster_failures.lock();
        let entry = map.entry(path.to_string()).or_insert(PosterFailure {
            attempts: 0,
            next_retry: Instant::now(),
        });
        entry.attempts += 1;
        let backoff = Duration::from_secs(2u64.saturating_pow(entry.attempts).min(60));
        entry.next_retry = Instant::now() + backoff;
    }

    pub fn clear_poster_failure(&self, path: &str) {
        self.poster_failures.lock().remove(path);
    }

    /// Failed poster paths whose backoff has elapsed. Each returned path
    /// gets its retry window pushed out so it isn't returned again until
    /// the reload itself fails and re-arms it.
    pub fn due_poster_retries(&self) -> Vec<String> {
        let now = Instant::now();
        let mut due = Vec::new();
        let mut map = self.poster_failures.lock();
        for (path, f) in map.iter_mut() {
            if f.attempts <= POSTER_RETRY_MAX_ATTEMPTS && f.next_retry <= now {
                f.next_retry = now + Duration::from_secs(60);
                due.push(path.clone());
            }
        }
        due
    }

    /// Flag that rendered state changed; the tick loop repaints on the
    /// next 100ms beat.
    pub fn mark_dirty(&self) {
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Consume the dirty flag.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn show_toast(&self, message: impl Into<String>, kind: ToastKind) {
        *self.toast.write() = Some(Toast {
            message: message.into(),
            kind,
            posted_at: Instant::now(),
        });
        self.mark_dirty();
    }

    pub fn clear_toast(&self) {
        *self.toast.write() = None;
        self.mark_dirty();
    }
}
