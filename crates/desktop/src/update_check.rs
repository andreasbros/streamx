//! Hourly update check against the GitHub releases API. When a newer
//! release exists, `AppState::latest_version` holds its version and the
//! UI shows a top-right notice plus a badge in the menu footer.

use crate::state::AppState;
use std::sync::Arc;

pub const RELEASES_API: &str = "https://api.github.com/repos/andreasbros/streamx/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/andreasbros/streamx/releases/latest";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3600);

/// Check once immediately, then hourly for the app's lifetime.
pub fn spawn(state: Arc<AppState>) {
    crate::runtime::spawn_detached(async move {
        loop {
            match fetch_latest().await {
                Ok(remote) => {
                    let newer = is_newer(&remote, APP_VERSION);
                    let mut slot = state.latest_version.write();
                    let changed = match (&*slot, newer) {
                        (None, true) => {
                            *slot = Some(remote.clone());
                            true
                        }
                        (Some(prev), true) if prev != &remote => {
                            *slot = Some(remote.clone());
                            true
                        }
                        (Some(_), false) => {
                            *slot = None;
                            true
                        }
                        _ => false,
                    };
                    drop(slot);
                    if changed {
                        tracing::info!(current = APP_VERSION, latest = %remote, "update check");
                        state.mark_dirty();
                    }
                }
                Err(e) => tracing::debug!("update check failed: {e}"),
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

async fn fetch_latest() -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!("StreamX/{APP_VERSION}"))
        .build()
        .map_err(|e| e.to_string())?;
    let release: Release = client
        .get(RELEASES_API)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(release.tag_name.trim_start_matches('v').to_string())
}

/// Numeric semver comparison; anything unparsable is never "newer".
pub fn is_newer(remote: &str, current: &str) -> bool {
    match (parse(remote), parse(current)) {
        (Some(r), Some(c)) => r > c,
        _ => false,
    }
}

fn parse(v: &str) -> Option<(u32, u32, u32)> {
    // Strip any prerelease/build suffix ("-rc.1", "+abc") before
    // splitting the numeric core.
    let core = v.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut it = core.split('.');
    let maj = it.next()?.parse().ok()?;
    let min = it.next()?.parse().ok()?;
    let pat = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((maj, min, pat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_are_detected() {
        assert!(is_newer("0.3.5", "0.3.4"));
        assert!(is_newer("v0.4.0", "0.3.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
    }

    #[test]
    fn equal_or_older_is_not_newer() {
        assert!(!is_newer("0.3.4", "0.3.4"));
        assert!(!is_newer("0.3.3", "0.3.4"));
        assert!(!is_newer("0.2.9", "0.3.0"));
    }

    #[test]
    fn garbage_never_triggers() {
        assert!(!is_newer("nightly", "0.3.4"));
        assert!(!is_newer("", "0.3.4"));
        assert!(!is_newer("0.3", "0.3.4"));
        assert!(!is_newer("0.3.4.1", "0.3.4"));
        assert!(!is_newer("0.3.5", "garbage"));
    }

    #[test]
    fn prerelease_suffixes_parse_on_patch() {
        assert!(is_newer("0.3.5-rc.1", "0.3.4"));
        assert!(!is_newer("0.3.4-rc.1", "0.3.4"));
    }
}
