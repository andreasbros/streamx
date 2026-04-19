//! GPUI `AssetSource` that resolves `/proxy/{id}/{path}` image URLs by
//! calling the in-process `LocalApi`. Keeps poster loading off the
//! HTTP loopback so embedded mode never has to round-trip through TCP.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use gpui::{AssetSource, SharedString};

use streamx::LocalApi;

use crate::runtime;

pub struct LocalApiAssetSource {
    /// Set once the embedded server finishes bootstrapping. Before it's
    /// set, `load` returns `Ok(None)` so GPUI falls through to its
    /// default (no-op) loader.
    api: Arc<OnceLock<Arc<LocalApi>>>,
}

impl LocalApiAssetSource {
    pub fn new(api: Arc<OnceLock<Arc<LocalApi>>>) -> Self {
        Self { api }
    }
}

impl AssetSource for LocalApiAssetSource {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if !path.starts_with("/proxy/") {
            return Ok(None);
        }
        let Some(api) = self.api.get().cloned() else {
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
                tracing::debug!(path, error = %e, "fetch_proxy failed");
                Ok(None)
            }
        }
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
