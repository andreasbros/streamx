//! OS-specific window screenshot backends. The app renders identically
//! across platforms, so captures feed one shared baseline set; only the
//! capture mechanism differs per OS.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

#[cfg(target_os = "linux")]
pub fn screenshot_window(window_title: &str, out: &Path) -> Result<()> {
    // The harness launches the app without WAYLAND_DISPLAY so GPUI uses
    // X11 (XWayland on Wayland sessions); xdotool + ImageMagick then work
    // everywhere.
    let ids = Command::new("xdotool")
        .args(["search", "--name", window_title])
        .output()
        .context("xdotool not found; enter `nix develop`")?;
    let ids = String::from_utf8_lossy(&ids.stdout);
    let Some(id) = ids.lines().last().map(str::trim).filter(|s| !s.is_empty()) else {
        bail!("no window titled {window_title:?} found");
    };
    let status = Command::new("import")
        .args(["-window", id, &out.to_string_lossy()])
        .status()
        .context("ImageMagick `import` not found; enter `nix develop`")?;
    if !status.success() {
        bail!("import failed for window {id}");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn screenshot_window(_window_title: &str, out: &Path) -> Result<()> {
    // Full-screen capture; requires Screen Recording permission for the
    // terminal/sshd-launched process. Window-precise capture can follow
    // once a CGWindowID helper is added.
    let status = Command::new("screencapture")
        .args(["-x", &out.to_string_lossy()])
        .status()
        .context("screencapture failed to launch")?;
    if !status.success() {
        bail!("screencapture failed (grant Screen Recording permission?)");
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn screenshot_window(_window_title: &str, _out: &Path) -> Result<()> {
    bail!("Windows capture backend not implemented yet");
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn screenshot_window(_window_title: &str, _out: &Path) -> Result<()> {
    bail!("no capture backend for this OS");
}
