//! Resolution of the `ffmpeg`/`ffprobe` executables.
//!
//! With the `embed-ffmpeg` cargo feature the server binary carries
//! static `ffmpeg` and `ffprobe` executables (paths provided at compile
//! time via `STREAMX_FFMPEG_BIN` / `STREAMX_FFPROBE_BIN`). At startup
//! they are extracted once into `<data>/cache/bin` with a hash check,
//! and every transcode invocation resolves to the extracted copy.
//! Without the feature, or when extraction fails, the executables are
//! looked up on `PATH` exactly as before.

use std::path::PathBuf;
use std::sync::OnceLock;

static FFMPEG: OnceLock<PathBuf> = OnceLock::new();
static FFPROBE: OnceLock<PathBuf> = OnceLock::new();

/// Program to invoke for `ffmpeg`.
pub fn ffmpeg() -> PathBuf {
    FFMPEG
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

/// Program to invoke for `ffprobe`.
pub fn ffprobe() -> PathBuf {
    FFPROBE
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("ffprobe"))
}

/// Extract the embedded executables into `data_dir/cache/bin` and pin
/// resolution to them. A failure leaves `PATH` resolution in place and
/// is reported by the caller; it never aborts startup.
#[cfg(feature = "embed-ffmpeg")]
pub fn install(data_dir: &std::path::Path) -> std::io::Result<()> {
    let bin_dir = data_dir.join("cache").join("bin");
    std::fs::create_dir_all(&bin_dir)?;

    let ffmpeg_path = extract(&bin_dir, "ffmpeg", embedded::FFMPEG)?;
    let ffprobe_path = extract(&bin_dir, "ffprobe", embedded::FFPROBE)?;

    let _ = FFMPEG.set(ffmpeg_path);
    let _ = FFPROBE.set(ffprobe_path);
    Ok(())
}

/// Without embedded bytes, prefer executables shipped next to the app
/// (the macOS `.app` carries them in `Contents/Helpers`), then `PATH`.
#[cfg(not(feature = "embed-ffmpeg"))]
pub fn install(_data_dir: &std::path::Path) -> std::io::Result<()> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some((ffmpeg_path, ffprobe_path)) = bundled_pair(dir) {
                let _ = FFMPEG.set(ffmpeg_path);
                let _ = FFPROBE.set(ffprobe_path);
            }
        }
    }
    Ok(())
}

/// `ffmpeg` + `ffprobe` shipped alongside the executable: either in the
/// same directory or in a sibling `Helpers` directory (macOS bundle
/// layout `Contents/MacOS` + `Contents/Helpers`). Both must be present;
/// resolution is all-or-nothing so versions can never mix.
fn bundled_pair(exe_dir: &std::path::Path) -> Option<(PathBuf, PathBuf)> {
    let candidates = [exe_dir.to_path_buf(), exe_dir.join("../Helpers")];
    for dir in candidates {
        let ffmpeg = dir.join("ffmpeg");
        let ffprobe = dir.join("ffprobe");
        if ffmpeg.is_file() && ffprobe.is_file() {
            return Some((ffmpeg, ffprobe));
        }
    }
    None
}

#[cfg(feature = "embed-ffmpeg")]
mod embedded {
    pub const FFMPEG: &[u8] = include_bytes!(env!("STREAMX_FFMPEG_BIN"));
    pub const FFPROBE: &[u8] = include_bytes!(env!("STREAMX_FFPROBE_BIN"));
}

/// Write `bytes` to `dir/name` unless an identical copy is already
/// there (sha256 comparison). Writes go through a temp file + rename so
/// a concurrent reader never sees a truncated executable.
#[cfg(feature = "embed-ffmpeg")]
fn extract(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    use sha2::{Digest, Sha256};

    let target = dir.join(name);
    let want = Sha256::digest(bytes);
    if let Ok(existing) = std::fs::read(&target) {
        if Sha256::digest(&existing) == want {
            return Ok(target);
        }
    }

    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_path_lookup() {
        assert_eq!(ffmpeg(), PathBuf::from("ffmpeg"));
        assert_eq!(ffprobe(), PathBuf::from("ffprobe"));
    }

    #[test]
    fn bundled_pair_requires_both_binaries() {
        // Fully private tree: `exe_dir/../Helpers` must resolve inside
        // it, never into the shared temp dir where stale files from
        // other test binaries would leak in.
        let root = std::env::temp_dir().join(format!(
            "sx_ffbin_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("MacOS");
        let helpers = root.join("Helpers");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::create_dir_all(&helpers).expect("mkdir helpers");
        assert!(bundled_pair(&dir).is_none());

        std::fs::write(helpers.join("ffmpeg"), b"x").expect("write");
        assert!(bundled_pair(&dir).is_none(), "ffprobe missing");

        std::fs::write(helpers.join("ffprobe"), b"x").expect("write");
        let (f, p) = bundled_pair(&dir).expect("both present");
        assert!(f.ends_with("Helpers/ffmpeg"));
        assert!(p.ends_with("Helpers/ffprobe"));

        std::fs::write(dir.join("ffmpeg"), b"x").expect("write");
        std::fs::write(dir.join("ffprobe"), b"x").expect("write");
        let (f, _) = bundled_pair(&dir).expect("same-dir wins");
        assert_eq!(f, dir.join("ffmpeg"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
