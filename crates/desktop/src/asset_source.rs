//! GPUI `AssetSource` that resolves `/proxy/{id}/{path}` image URLs by
//! calling the in-process `LocalApi`. Keeps poster loading off the
//! HTTP loopback so embedded mode never has to round-trip through TCP.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use gpui::{AssetSource, SharedString};

use streamx::LocalApi;

use crate::runtime;

/// How long `load` will wait for the embedded server to finish
/// bootstrapping before giving up on a poster. The first home-page
/// render usually beats the server by a few hundred milliseconds.
const API_READY_TIMEOUT: Duration = Duration::from_secs(20);
const API_READY_POLL: Duration = Duration::from_millis(25);

pub struct LocalApiAssetSource {
    /// Filled once the embedded server finishes bootstrapping.
    api: Arc<OnceLock<Arc<LocalApi>>>,
}

impl LocalApiAssetSource {
    pub fn new(api: Arc<OnceLock<Arc<LocalApi>>>) -> Self {
        Self { api }
    }

    /// Wait (bounded) for the embedded server to fill the api slot.
    ///
    /// GPUI memoizes the result of the first `load` call per image
    /// source forever (`App::fetch_asset` only re-runs after an
    /// explicit `remove_asset`). So returning `Ok(None)` while the
    /// server is still booting permanently caches a blank poster.
    /// Waiting here guarantees the first load resolves to real bytes.
    /// Runs on a GPUI asset-loader background thread, so blocking is
    /// safe and does not stall the tokio runtime that sets the slot.
    fn wait_for_api(&self) -> Option<Arc<LocalApi>> {
        if let Some(api) = self.api.get() {
            return Some(api.clone());
        }
        let start = Instant::now();
        loop {
            std::thread::sleep(API_READY_POLL);
            if let Some(api) = self.api.get() {
                tracing::info!(
                    waited_ms = start.elapsed().as_millis() as u64,
                    "asset load: embedded api became ready"
                );
                return Some(api.clone());
            }
            if start.elapsed() >= API_READY_TIMEOUT {
                tracing::warn!(
                    timeout_s = API_READY_TIMEOUT.as_secs(),
                    "asset load: embedded api not ready; poster will be blank"
                );
                return None;
            }
        }
    }
}

impl AssetSource for LocalApiAssetSource {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if !path.starts_with("/proxy/") {
            return Ok(None);
        }
        let Some(api) = self.wait_for_api() else {
            return Ok(None);
        };

        let path_owned = path.to_string();
        // Run the async fetch on the tokio runtime. GPUI's asset loader
        // runs on its own thread pool (separate from tokio), so using
        // the runtime handle here is safe.
        let result = runtime::block_on(async move { api.fetch_proxy(&path_owned).await });
        match result {
            Ok((bytes, _ext)) => Ok(Some(Cow::Owned(bytes))),
            Err(e) => {
                tracing::warn!(path, error = %e, "asset load: fetch_proxy failed");
                Ok(None)
            }
        }
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
