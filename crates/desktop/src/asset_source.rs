//! GPUI `AssetSource` for poster images.
//!
//! Disk-first and never blocking: a cache hit returns bytes immediately;
//! a miss kicks a bounded background fetch on the tokio runtime and
//! returns a transient error. When the fetch lands, the path is queued on
//! [`AppState::poster_ready`]; the tick loop evicts it from GPUI's asset
//! cache so the next frame loads it from disk. Posters therefore stream
//! in one by one as they arrive, and GPUI's fixed worker pool is never
//! parked on the network.
//!
//! Failures are recorded on [`AppState::poster_failures`] and retried
//! with backoff by the tick loop.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AssetSource, SharedString};
use once_cell::sync::OnceCell;

use streamx::config::ProviderConfig;
use streamx::server::proxy::{image_cache_path, provider_base_url};

use crate::runtime;
use crate::state::{AppState, Mode};

const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
/// Concurrent poster downloads. Generous: these are tokio tasks, not
/// GPUI worker threads.
const MAX_CONCURRENT_FETCHES: usize = 24;

static FETCH_SEMAPHORE: OnceCell<Arc<tokio::sync::Semaphore>> = OnceCell::new();

pub struct PosterAssetSource {
    state: Arc<AppState>,
    providers: OnceCell<Vec<ProviderConfig>>,
    data_dir: OnceCell<PathBuf>,
    http: OnceCell<reqwest::Client>,
}

impl PosterAssetSource {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            providers: OnceCell::new(),
            data_dir: OnceCell::new(),
            http: OnceCell::new(),
        }
    }

    /// Provider table + data dir from the server config. Loaded once,
    /// lazily; falls back to the AppState defaults when no config exists
    /// (e.g. thin-client machines).
    fn ensure_config(&self) -> (&[ProviderConfig], &PathBuf) {
        if self.providers.get().is_none() {
            let cli = streamx::cli::Cli {
                command: None,
                port: None,
                bind: None,
                data_dir: None,
                config: None,
                log_level: None,
                log_dir: None,
                open: false,
                admin_user: None,
                admin_password: None,
            };
            let (providers, data_dir) = match streamx::config::load_config(&cli) {
                Ok(c) => (c.providers.clone(), c.data_dir.clone()),
                Err(_) => (Vec::new(), self.state.data_dir.clone()),
            };
            let _ = self.providers.set(providers);
            let _ = self.data_dir.set(data_dir);
        }
        (
            self.providers.get().map(|v| v.as_slice()).unwrap_or(&[]),
            self.data_dir.get().unwrap_or(&self.state.data_dir),
        )
    }

    fn client(&self) -> &reqwest::Client {
        self.http.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(FETCH_TIMEOUT)
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default()
        })
    }

    /// Start a bounded background download for `path` unless one is
    /// already in flight. Completion queues the path for cache eviction
    /// (success) or schedules a backoff retry (failure).
    fn spawn_fetch(&self, path: &str, url: String, cache_path: PathBuf) {
        {
            let mut pending = self.state.poster_pending.lock();
            if !pending.insert(path.to_string()) {
                return;
            }
        }
        let state = self.state.clone();
        let client = self.client().clone();
        let path = path.to_string();
        runtime::spawn_detached(async move {
            let sem = FETCH_SEMAPHORE
                .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_FETCHES)))
                .clone();
            let _permit = sem.acquire_owned().await;
            let result = async {
                let resp = client.get(&url).send().await?;
                if !resp.status().is_success() {
                    anyhow::bail!("{} fetching {url}", resp.status());
                }
                let bytes = resp.bytes().await?;
                if let Some(dir) = cache_path.parent() {
                    let _ = tokio::fs::create_dir_all(dir).await;
                }
                tokio::fs::write(&cache_path, &bytes).await?;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            state.poster_pending.lock().remove(&path);
            match result {
                Ok(()) => state.poster_ready.lock().push(path),
                Err(e) => {
                    tracing::debug!(path, error = %e, "poster fetch failed; scheduling retry");
                    state.mark_poster_failure(&path);
                }
            }
        });
    }

    /// `/proxy/{provider_id}/{path}`: disk cache hit or a spawned fetch
    /// from the upstream image host; unknown providers fall back to the
    /// (possibly remote) StreamX server like the web UI does.
    fn load_proxy(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let rest = path
            .strip_prefix("/proxy/")
            .ok_or_else(|| anyhow::anyhow!("not a proxy path"))?;
        let (id_str, sub) = rest
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("malformed proxy path"))?;
        let id: u32 = id_str.parse()?;
        if sub.contains("..") {
            anyhow::bail!("invalid path");
        }

        let (providers, data_dir) = self.ensure_config();
        let (url, cache_path) = match provider_base_url(id, providers) {
            Some(base) => {
                let upstream = format!("{base}/{sub}");
                let cache = image_cache_path(data_dir, &upstream, sub);
                (upstream, cache)
            }
            None => {
                let server = self.state.server_url.read().clone();
                let via_server = format!("{server}{path}");
                let cache = image_cache_path(data_dir, &via_server, sub);
                (via_server, cache)
            }
        };

        if let Ok(bytes) = std::fs::read(&cache_path) {
            return Ok(Some(bytes));
        }
        self.spawn_fetch(path, url, cache_path);
        Ok(None)
    }

    /// `/api/posters/{file}`: bytes from the server's poster dir; thin
    /// clients fetch over HTTP into the same location.
    fn load_local_poster(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let file = path
            .strip_prefix("/api/posters/")
            .ok_or_else(|| anyhow::anyhow!("not a poster path"))?;
        if file.contains("..") || file.contains('/') {
            anyhow::bail!("invalid poster filename");
        }
        let local = self.state.downloads_dir.join("posters").join(file);
        if let Ok(bytes) = std::fs::read(&local) {
            return Ok(Some(bytes));
        }
        if *self.state.mode.read() == Mode::ThinClient {
            let server = self.state.server_url.read().clone();
            self.spawn_fetch(path, format!("{server}{path}"), local);
        } else {
            // Embedded: the server writes this file shortly after stream
            // creation; retry with backoff until it appears.
            self.state.mark_poster_failure(path);
        }
        Ok(None)
    }
}

/// The StreamX mark, shared byte-for-byte with the web app.
const LOGO_SVG: &[u8] = include_bytes!("../../../web/src/assets/icons/logo.svg");

/// 1x1 transparent PNG served while a poster download is in flight, so
/// a cache miss is an ordinary (invisible) image instead of an asset
/// error that gpui logs at ERROR level on every miss.
const PENDING_PNG: &[u8] = include_bytes!("../assets/pending.png");
const VIDEO_SVG: &[u8] = include_bytes!("../assets/video.svg");

impl AssetSource for PosterAssetSource {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == "logo.svg" {
            return Ok(Some(Cow::Borrowed(LOGO_SVG)));
        }
        if path == "video.svg" {
            return Ok(Some(Cow::Borrowed(VIDEO_SVG)));
        }
        let result = if path.starts_with("/proxy/") {
            self.load_proxy(path)
        } else if path.starts_with("/api/posters/") {
            self.load_local_poster(path)
        } else {
            return Ok(None);
        };
        match result {
            Ok(Some(bytes)) => {
                self.state.clear_poster_failure(path);
                Ok(Some(Cow::Owned(bytes)))
            }
            // Not cached yet: a background fetch is in flight; the tick
            // loop evicts this entry once bytes land. A transparent
            // placeholder keeps the miss out of gpui's error logging.
            Ok(None) => {
                tracing::debug!(path, "poster pending; serving placeholder");
                Ok(Some(Cow::Borrowed(PENDING_PNG)))
            }
            Err(e) => {
                tracing::debug!(path, error = %e, "poster load rejected");
                self.state.mark_poster_failure(path);
                Ok(Some(Cow::Borrowed(PENDING_PNG)))
            }
        }
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
