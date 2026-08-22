//! In-process video playback through libmpv.
//!
//! mpv runs inside the desktop process: no external binary, no PATH
//! lookup, no IPC socket. mpv still opens its own player window (the
//! render-API-into-GPUI step comes later); control goes straight
//! through the client API. One background thread pumps mpv's event
//! queue so a closed player window is noticed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use libmpv2::events::Event;
use libmpv2::Mpv;

use super::ipc::Snapshot;
use super::PlayTarget;

pub struct EmbeddedPlayer {
    mpv: Arc<Mpv>,
    finished: Arc<AtomicBool>,
}

/// Options every player window gets. Mirrors the flags the spawned
/// mpv used, plus the bindings/OSC that libmpv leaves off by default.
const PLAYER_OPTIONS: &[(&str, &str)] = &[
    ("force-window", "yes"),
    ("keep-open", "always"),
    ("idle", "yes"),
    ("input-default-bindings", "yes"),
    ("input-vo-keyboard", "yes"),
    ("osc", "yes"),
    ("border", "yes"),
    ("keepaspect-window", "no"),
    ("ytdl", "no"),
    ("cache", "yes"),
    ("cache-secs", "300"),
    ("demuxer-max-bytes", "2G"),
    ("demuxer-max-back-bytes", "500M"),
    ("demuxer-readahead-secs", "120"),
    ("network-timeout", "600"),
    (
        "stream-lavf-o",
        "reconnect=1,reconnect_streamed=1,reconnect_delay_max=30",
    ),
    ("hr-seek", "yes"),
    ("title", "StreamX"),
];

impl EmbeddedPlayer {
    /// Create a player and start the target. Errors carry mpv's own
    /// message so initialization problems are diagnosable.
    pub fn launch(target: &PlayTarget) -> Result<Self, String> {
        let header = match target {
            PlayTarget::Http { token: Some(t), .. } => Some(format!("Authorization: Bearer {t}")),
            _ => None,
        };
        let mpv = Mpv::with_initializer(|init| {
            for (k, v) in PLAYER_OPTIONS {
                init.set_option(k, *v)?;
            }
            if let Some(h) = &header {
                init.set_option("http-header-fields", h.as_str())?;
            }
            Ok(())
        })
        .map_err(|e| format!("libmpv initialization failed: {e}"))?;

        let mpv = Arc::new(mpv);
        let finished = Arc::new(AtomicBool::new(false));
        let url = target.display();
        mpv.command("loadfile", &[url.as_str(), "replace"])
            .map_err(|e| format!("mpv loadfile failed: {e}"))?;

        let pump_mpv = Arc::clone(&mpv);
        let pump_done = Arc::clone(&finished);
        std::thread::Builder::new()
            .name("mpv-events".into())
            .spawn(move || loop {
                match pump_mpv.wait_event(1.0) {
                    Some(Ok(Event::Shutdown)) => {
                        pump_done.store(true, Ordering::Relaxed);
                        break;
                    }
                    Some(Ok(Event::EndFile(reason))) => {
                        tracing::info!(?reason, "mpv: end of file");
                    }
                    Some(Err(e)) => {
                        tracing::warn!("mpv event error: {e}");
                    }
                    _ => {}
                }
                if pump_done.load(Ordering::Relaxed) {
                    break;
                }
            })
            .map_err(|e| format!("failed to start mpv event thread: {e}"))?;

        Ok(Self { mpv, finished })
    }

    /// Whether mpv has a configured video output (a window exists).
    pub fn vo_configured(&self) -> bool {
        self.mpv.get_property("vo-configured").unwrap_or(false)
    }

    /// True once the user closed the player window (mpv shut down).
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        let _ = self.mpv.command("quit", &[]);
        self.finished.store(true, Ordering::Relaxed);
    }

    pub fn toggle_pause(&self) -> Result<(), String> {
        let paused: bool = self.mpv.get_property("pause").map_err(|e| e.to_string())?;
        self.mpv
            .set_property("pause", !paused)
            .map_err(|e| e.to_string())
    }

    pub fn seek(&self, seconds: f64, relative: bool) -> Result<(), String> {
        let secs = format!("{seconds}");
        let mode = if relative { "relative" } else { "absolute" };
        self.mpv
            .command("seek", &[secs.as_str(), mode])
            .map_err(|e| e.to_string())
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            paused: self.mpv.get_property("pause").unwrap_or(false),
            time_pos: self.mpv.get_property("time-pos").unwrap_or(0.0),
            duration: self.mpv.get_property("duration").unwrap_or(0.0),
        }
    }
}

impl Drop for EmbeddedPlayer {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Relaxed);
    }
}
