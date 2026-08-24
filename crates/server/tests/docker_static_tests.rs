//! The static musl server must be genuinely self-contained: it has to
//! boot and stream inside a vanilla Alpine container with nothing
//! installed. Runs the artifact the release ships, not a dev build.
//!
//! Requires a running Docker daemon (Docker Desktop or colima on
//! macOS) and the static binary:
//!
//! ```bash
//! nix build .#streamx-x86_64-linux-musl   # or aarch64 on Apple Silicon
//! cargo test -p streamx --test docker_static_tests -- --ignored
//! ```
//!
//! `STREAMX_STATIC_BIN` overrides the binary path; by default the
//! `result*` symlink matching the host architecture is used (Docker on
//! macOS runs Linux containers of the host's architecture, so the musl
//! Linux binary is always the right one).

use std::path::PathBuf;
use std::time::Duration;

use testcontainers::{
    core::{ContainerPort, ExecCommand, IntoContainerPort},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};

const PORT: u16 = 8080;
const ALPINE: (&str, &str) = ("alpine", "3.22");

/// Big Buck Bunny, the canonical public-domain test torrent (webtorrent
/// fixture): heavily seeded, live trackers, and a webseed so data flows
/// even when no BitTorrent peers answer.
const TEST_MAGNET: &str = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=udp%3A%2F%2Ftracker.torrent.eu.org%3A451&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F";

fn static_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("STREAMX_STATIC_BIN") {
        return Some(PathBuf::from(p));
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let arch_link = if cfg!(target_arch = "aarch64") {
        "result-aarch64-musl"
    } else {
        "result-x86_64-musl"
    };
    for link in [arch_link, "result"] {
        let candidate = workspace.join(link).join("bin/streamx");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

async fn wait_http_ok(url: &str, timeout: Duration) -> Result<reqwest::Response, String> {
    let client = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match client.get(url).send().await {
            Ok(r) if r.status().is_success() => return Ok(r),
            _ if tokio::time::Instant::now() > deadline => {
                return Err(format!("timed out waiting for {url}"));
            }
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}

/// Boot the static server inside a stock distro image (nothing
/// installed) and verify the embedded web UI is served and the
/// embedded ffmpeg extracted and runnable.
async fn boot_server_in(
    image: &str,
    tag: &str,
    binary: PathBuf,
) -> (testcontainers::ContainerAsync<GenericImage>, String) {
    let container = GenericImage::new(image, tag)
        // No log-line wait: the server logs to its log file, not the
        // console. Readiness is polled over HTTP instead.
        .with_exposed_port(ContainerPort::Tcp(PORT))
        .with_entrypoint("/bin/sh")
        .with_copy_to("/streamx", binary)
        .with_env_var("STREAMX_DATA_DIR", "/data")
        .with_cmd([
            "-c".to_string(),
            format!(
                "chmod +x /streamx && exec /streamx --port {PORT} --bind 0.0.0.0 \
                 --admin-user admin --admin-password password"
            ),
        ])
        .start()
        .await
        .unwrap_or_else(|e| panic!("start {image}:{tag} container (docker running?): {e}"));

    let host_port = container
        .get_host_port_ipv4(PORT.tcp())
        .await
        .expect("mapped port");
    let base = format!("http://127.0.0.1:{host_port}");

    let index = wait_http_ok(&base, Duration::from_secs(60))
        .await
        .unwrap_or_else(|e| panic!("web UI on {image}:{tag}: {e}"));
    let html = index.text().await.expect("index body");
    assert!(
        html.contains("<div id=\"root\""),
        "embedded web UI not served on {image}:{tag}: {html:.200}"
    );

    let mut ffmpeg = container
        .exec(ExecCommand::new(["/data/cache/bin/ffmpeg", "-version"]))
        .await
        .expect("exec extracted ffmpeg");
    let out = ffmpeg.stdout_to_vec().await.expect("ffmpeg stdout");
    assert!(
        String::from_utf8_lossy(&out).contains("ffmpeg version"),
        "extracted ffmpeg did not run on {image}:{tag}"
    );

    (container, base)
}

fn checked_static_binary() -> PathBuf {
    let Some(binary) = static_binary() else {
        panic!(
            "static binary not found; run `nix build .#streamx-x86_64-linux-musl \
             --out-link result-x86_64-musl` (or set STREAMX_STATIC_BIN)"
        );
    };
    // The artifact itself must satisfy the fully-static policy before
    // it goes anywhere near a container.
    let linkage =
        streamx_linkcheck::assert_policy(&binary, &streamx_linkcheck::Policy::FullyStatic)
            .expect("binary violates the fully-static linkage policy");
    assert!(linkage.libraries.is_empty());
    binary
}

/// The static server must boot identically on every major distro
/// family: musl-static means no glibc floor, no packages, no
/// surprises. Alpine is covered by the streaming test below.
#[tokio::test]
#[ignore = "needs a Docker daemon and the nix-built musl binary"]
async fn static_server_boots_across_stock_distros() {
    let binary = checked_static_binary();
    for (image, tag) in [
        ("ubuntu", "24.04"),
        ("debian", "12"),
        ("rockylinux", "9"),
        ("fedora", "41"),
    ] {
        let (container, base) = boot_server_in(image, tag, binary.clone()).await;
        drop(base);
        container.stop().await.ok();
    }
}

#[tokio::test]
#[ignore = "needs a Docker daemon, the nix-built musl binary, and network"]
async fn static_server_boots_and_streams_in_vanilla_alpine() {
    let binary = checked_static_binary();
    let (container, base) = boot_server_in(ALPINE.0, ALPINE.1, binary).await;
    let _ = &container;
    let client = reqwest::Client::new();

    // Auth: the seeded admin can log in (same call the web UI makes).
    let login: serde_json::Value = client
        .post(format!("{base}/api/auth/login"))
        .json(&serde_json::json!({"username": "admin", "password": "password"}))
        .send()
        .await
        .expect("login request")
        .json()
        .await
        .expect("login json");
    let token = login["token"].as_str().expect("login token").to_string();

    // Torrent engine connects and fetches metadata for a real magnet.
    let created: serde_json::Value = client
        .post(format!("{base}/api/stream"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"magnet_uri": TEST_MAGNET, "title": "Big Buck Bunny"}))
        .send()
        .await
        .expect("create stream")
        .json()
        .await
        .expect("create stream json");
    let stream_id = created["id"]
        .as_str()
        .map(str::to_string)
        .or_else(|| created["stream_id"].as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("no stream id in {created}"));

    // Wait for the torrent metadata (peer/webseed contact) to resolve
    // into a file list.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(240);
    let files: Vec<serde_json::Value> = loop {
        let resp = client
            .get(format!("{base}/api/stream/{stream_id}/files"))
            .bearer_auth(&token)
            .send()
            .await
            .expect("files request");
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                let list = v["files"].as_array().cloned().unwrap_or_default();
                if !list.is_empty() {
                    break list;
                }
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "torrent metadata did not arrive; no peer/webseed connectivity from the container"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    let file_index = files
        .iter()
        .filter_map(|f| f["index"].as_u64())
        .next()
        .expect("file index");

    // Play the stream the way the web player does: ranged request
    // against the file endpoint must yield real bytes (pieces are
    // fetched on demand).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    loop {
        let resp = client
            .get(format!("{base}/api/stream/{stream_id}/file/{file_index}"))
            .bearer_auth(&token)
            .header("Range", "bytes=0-65535")
            .send()
            .await
            .expect("stream request");
        let status = resp.status();
        if status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT {
            let bytes = resp.bytes().await.expect("stream bytes");
            if !bytes.is_empty() {
                assert!(bytes.iter().any(|b| *b != 0), "stream returned only zeros");
                break;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no stream bytes arrived within the deadline"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// The distributable desktop binary must load on a stock desktop
/// distribution using only distro packages: standard system loader,
/// glibc floor satisfied, every allowlisted library resolvable. The
/// app is expected to reach its own startup logging (windowing then
/// fails in a headless container, which is fine: loader, glibc, and
/// library resolution have all succeeded by that point).
#[tokio::test]
#[ignore = "needs a Docker daemon, the nix-built desktop dist binary, and network for apt"]
#[cfg(target_arch = "x86_64")]
async fn desktop_dist_loads_on_stock_ubuntu() {
    use testcontainers::core::WaitFor;

    let binary = match std::env::var("STREAMX_DESKTOP_DIST_BIN") {
        Ok(p) => PathBuf::from(p),
        Err(_) => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../result-desktop-dist/bin/streamx-desktop"),
    };
    assert!(
        binary.exists(),
        "desktop dist binary not found; run `nix build .#streamx-desktop-dist \
         --out-link result-desktop-dist` (or set STREAMX_DESKTOP_DIST_BIN)"
    );
    streamx_linkcheck::assert_policy(
        &binary,
        &streamx_linkcheck::Policy::LinuxDist {
            allowed_sonames: streamx_linkcheck::linux_desktop_allowlist(),
        },
    )
    .expect("desktop dist binary violates the linux-dist policy");

    let container = GenericImage::new("ubuntu", "24.04")
        .with_wait_for(WaitFor::message_on_stdout("starting StreamX desktop"))
        .with_entrypoint("/bin/sh")
        .with_copy_to("/app", binary)
        .with_cmd([
            "-c".to_string(),
            "apt-get update -qq && apt-get install -y -qq \
             libx11-6 libxcb1 libxext6 libxfixes3 libxrandr2 libxkbcommon0 \
             libxkbcommon-x11-0 libwayland-client0 libwayland-cursor0 \
             libvulkan1 libasound2t64 >/dev/null && \
             chmod +x /app && timeout 20 /app 2>&1; true"
                .to_string(),
        ])
        .with_startup_timeout(Duration::from_secs(300))
        .start()
        .await
        .expect("desktop dist did not reach startup logging on stock Ubuntu 24.04");
    container.stop().await.ok();
}
