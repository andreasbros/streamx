//! Polls the backend's health endpoint so a stalled downloads volume
//! (dying or disconnected external disk) is surfaced as a warning pill
//! instead of silently breaking downloads and playback.

use crate::state::AppState;
use std::sync::Arc;

const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

pub fn spawn(state: Arc<AppState>) {
    crate::runtime::spawn_detached(async move {
        loop {
            let client = state.client.read().clone();
            // Unreachable server is handled by the connection UI, not
            // this pill; only a definite report flips the flag.
            let stalled = match client.health().await {
                Ok(h) => !h.storage_ok,
                Err(_) => false,
            };
            let changed = {
                let mut slot = state.storage_stalled.write();
                let changed = *slot != stalled;
                *slot = stalled;
                changed
            };
            if changed {
                if stalled {
                    tracing::warn!("backend reports download storage not responding");
                } else {
                    tracing::info!("download storage recovered");
                }
                state.mark_dirty();
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}
