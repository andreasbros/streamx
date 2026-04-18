//! Central application state shared between the window and async tasks.
//!
//! Pattern mirrors nocapsec: `Arc<AppState>` with `parking_lot::RwLock` fields.
//! Views read snapshots during render and call `notify()` after mutation.

use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use streamx_api::client::Client;
use streamx_api::types::{SearchResultGroup, User};

use crate::router::Page;

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

    pub server_url: RwLock<String>,
    pub server_version: RwLock<Option<String>>,
    pub server_hash: RwLock<Option<String>>,
    pub connection_error: RwLock<Option<String>>,

    // Search / browse
    pub query: RwLock<String>,
    pub search_results: RwLock<Vec<SearchResultGroup>>,
    pub search_in_flight: RwLock<bool>,
    pub browse: RwLock<BrowseData>,
    pub browse_loading: RwLock<bool>,

    // Selected movie for detail page
    pub selected_movie: RwLock<Option<SearchResultGroup>>,

    // Feedback
    pub toast: RwLock<Option<String>>,

    // Page stack (current page = last)
    pub page_stack: RwLock<Vec<Page>>,

    // On-disk config dir (~/.streamx-desktop)
    pub config_dir: PathBuf,
}

impl AppState {
    pub fn new(server_url: String) -> Arc<Self> {
        let config_dir = directories::ProjectDirs::from("com", "streamx", "streamx-desktop")
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let _ = std::fs::create_dir_all(&config_dir);

        // Load persisted token if any.
        let token_file = config_dir.join("token");
        let saved_token = std::fs::read_to_string(&token_file).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

        let mut client = Client::new(server_url.clone());
        if let Some(t) = saved_token.as_ref() {
            client.set_token(Some(t.clone()));
        }

        // Initial page: Search if already logged in, otherwise Login.
        let initial_page = if saved_token.is_some() { Page::Search } else { Page::Login };

        Arc::new(Self {
            client: RwLock::new(client),
            token: RwLock::new(saved_token),
            user: RwLock::new(None),
            server_url: RwLock::new(server_url),
            server_version: RwLock::new(None),
            server_hash: RwLock::new(None),
            connection_error: RwLock::new(None),
            query: RwLock::new(String::new()),
            search_results: RwLock::new(Vec::new()),
            search_in_flight: RwLock::new(false),
            browse: RwLock::new(BrowseData::default()),
            browse_loading: RwLock::new(false),
            selected_movie: RwLock::new(None),
            toast: RwLock::new(None),
            page_stack: RwLock::new(vec![initial_page]),
            config_dir,
        })
    }

    pub fn set_token(&self, token: Option<String>) {
        let path = self.config_dir.join("token");
        match &token {
            Some(t) => {
                let _ = std::fs::write(&path, t);
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        self.client.write().set_token(token.clone());
        *self.token.write() = token;
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
}
