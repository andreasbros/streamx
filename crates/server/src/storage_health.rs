//! Watchdog for the downloads volume.
//!
//! A cleanly unmounted drive fails fast (`exists()` returns false), but a
//! dying or sleeping USB disk stays mounted while every filesystem call
//! against it blocks indefinitely. Such calls issued on runtime threads
//! wedge the whole server. This monitor probes the volume from the
//! blocking pool under a timeout and publishes a stalled flag that
//! disk-touching paths consult to fail fast, and that both UIs surface
//! to the user.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const CHECK_INTERVAL_SECS: u64 = 15;
const CHECK_TIMEOUT_SECS: u64 = 10;

pub struct StorageHealth {
    stalled: AtomicBool,
    message: std::sync::Mutex<String>,
    /// Single-flight guard: while a probe is stuck on dead storage its
    /// thread cannot be reclaimed, so no new probe is started and the
    /// volume is reported stalled instead.
    check_running: AtomicBool,
}

impl StorageHealth {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            stalled: AtomicBool::new(false),
            message: std::sync::Mutex::new(String::new()),
            check_running: AtomicBool::new(false),
        })
    }

    pub fn is_stalled(&self) -> bool {
        self.stalled.load(Ordering::SeqCst)
    }

    /// (ok, message): message is set only while stalled.
    pub fn status(&self) -> (bool, Option<String>) {
        if self.is_stalled() {
            let msg = self
                .message
                .lock()
                .map(|m| m.clone())
                .unwrap_or_else(|_| "storage not responding".to_string());
            (false, Some(msg))
        } else {
            (true, None)
        }
    }

    fn record(&self, stalled: bool, message: String) {
        let was = self.stalled.swap(stalled, Ordering::SeqCst);
        if stalled && !was {
            tracing::warn!(%message, "Storage volume is not responding");
        } else if !stalled && was {
            tracing::info!("Storage volume recovered");
        }
        if let Ok(mut m) = self.message.lock() {
            *m = message;
        }
    }

    pub fn spawn_monitor(self: &Arc<Self>, dir: PathBuf) {
        let health = self.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(CHECK_INTERVAL_SECS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                health.check_once(&dir).await;
            }
        });
    }

    pub async fn check_once(self: &Arc<Self>, dir: &Path) {
        if self.check_running.swap(true, Ordering::SeqCst) {
            self.record(true, format!("{} is not responding", dir.display()));
            return;
        }
        let owner = self.clone();
        let probe_dir = dir.to_path_buf();
        let join = tokio::task::spawn_blocking(move || {
            // Clears the single-flight guard whenever the kernel finally
            // releases this thread, even long after the timeout below.
            struct Clear(Arc<StorageHealth>);
            impl Drop for Clear {
                fn drop(&mut self) {
                    self.0.check_running.store(false, Ordering::SeqCst);
                }
            }
            let _clear = Clear(owner);
            std::fs::read_dir(&probe_dir).map(|mut entries| {
                let _ = entries.next();
            })
        });
        match tokio::time::timeout(std::time::Duration::from_secs(CHECK_TIMEOUT_SECS), join).await {
            Ok(Ok(Ok(()))) => self.record(false, String::new()),
            Ok(Ok(Err(e))) => self.record(true, format!("{}: {e}", dir.display())),
            Ok(Err(e)) => self.record(true, format!("storage check failed: {e}")),
            Err(_) => self.record(
                true,
                format!(
                    "{} did not respond within {CHECK_TIMEOUT_SECS}s",
                    dir.display()
                ),
            ),
        }
    }
}

/// Run `op` (a blocking filesystem call) on the blocking pool, giving up
/// after `timeout_secs`. `None` means the operation did not finish in
/// time; the abandoned thread is released whenever the kernel lets go.
pub async fn bounded_fs_op<T, F>(timeout_secs: u64, op: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let join = tokio::task::spawn_blocking(op);
    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), join).await {
        Ok(Ok(v)) => Some(v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn healthy_dir_is_not_stalled() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let health = StorageHealth::new();
        health.check_once(tmp.path()).await;
        assert!(!health.is_stalled());
        assert_eq!(health.status(), (true, None));
    }

    #[tokio::test]
    async fn missing_dir_is_stalled_and_recovers() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let gone = tmp.path().join("unplugged");
        let health = StorageHealth::new();

        health.check_once(&gone).await;
        assert!(health.is_stalled());
        let (ok, msg) = health.status();
        assert!(!ok);
        assert!(msg.unwrap_or_default().contains("unplugged"));

        std::fs::create_dir_all(&gone).unwrap_or_else(|e| panic!("mkdir: {e}"));
        health.check_once(&gone).await;
        assert!(!health.is_stalled());
    }

    #[tokio::test]
    async fn concurrent_check_reports_stalled() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let health = StorageHealth::new();
        // Simulate a stuck probe holding the single-flight guard
        health.check_running.store(true, Ordering::SeqCst);
        health.check_once(tmp.path()).await;
        assert!(health.is_stalled());
    }

    #[tokio::test]
    async fn bounded_fs_op_returns_value_and_times_out() {
        let v = bounded_fs_op(5, || 42).await;
        assert_eq!(v, Some(42));
        let slow = bounded_fs_op(1, || {
            std::thread::sleep(std::time::Duration::from_secs(3));
        })
        .await;
        assert!(slow.is_none());
    }
}
