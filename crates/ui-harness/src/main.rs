//! StreamX desktop UI test harness.
//!
//! Playwright-style tests for the GPUI app: launches a hermetic backend
//! and the desktop app (built with `--features ui-test`), drives it with
//! synthetic keystrokes/clicks through the real event path, asserts on
//! app state, and captures per-scenario screenshots that are compared
//! against checked-in baselines with a cross-OS tolerance.
//!
//! Run (inside `nix develop`):
//!   cargo build -p streamx-desktop --features ui-test
//!   cargo build -p streamx
//!   cargo run -p streamx-ui-harness
//!
//! Flags:
//!   --live                use the real ~/.streamx config (network posters)
//!   --update-baselines    overwrite baselines with current captures
//!   --scenario <name>     run one scenario only
//!   --no-screenshots      state assertions only

mod capture;
mod compare;
mod driver;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use driver::Driver;
use serde_json::json;

const WINDOW_TITLE: &str = "StreamX";
const DIFF_TOLERANCE: f64 = 0.06;

struct Args {
    live: bool,
    update_baselines: bool,
    scenario: Option<String>,
    screenshots: bool,
    app: PathBuf,
    server: PathBuf,
    artifacts: PathBuf,
    baselines: PathBuf,
}

fn parse_args() -> Args {
    let target = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());
    let mut args = Args {
        live: false,
        update_baselines: false,
        scenario: None,
        screenshots: true,
        app: PathBuf::from(format!("{target}/debug/streamx-desktop")),
        server: PathBuf::from(format!("{target}/debug/streamx")),
        artifacts: PathBuf::from(format!("{target}/ui-test-artifacts")),
        baselines: PathBuf::from("crates/ui-harness/baselines"),
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--live" => args.live = true,
            "--update-baselines" => args.update_baselines = true,
            "--no-screenshots" => args.screenshots = false,
            "--scenario" => args.scenario = it.next(),
            "--app" => {
                if let Some(v) = it.next() {
                    args.app = PathBuf::from(v);
                }
            }
            "--server" => {
                if let Some(v) = it.next() {
                    args.server = PathBuf::from(v);
                }
            }
            "--artifacts" => {
                if let Some(v) = it.next() {
                    args.artifacts = PathBuf::from(v);
                }
            }
            "--baselines" => {
                if let Some(v) = it.next() {
                    args.baselines = PathBuf::from(v);
                }
            }
            other => eprintln!("ignoring unknown arg: {other}"),
        }
    }
    // Live runs render real posters and can't share hermetic baselines.
    if args.live && args.baselines == Path::new("crates/ui-harness/baselines") {
        args.baselines = PathBuf::from("crates/ui-harness/baselines-live");
    }
    args
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(38999)
}

struct Procs {
    server: Option<Child>,
    app: Child,
}

impl Drop for Procs {
    fn drop(&mut self) {
        let _ = self.app.kill();
        let _ = self.app.wait();
        if let Some(s) = self.server.as_mut() {
            let _ = s.kill();
            let _ = s.wait();
        }
    }
}

fn start_server(server_bin: &Path, port: u16, envs: &[(String, String)]) -> Result<Child> {
    let mut cmd = Command::new(server_bin);
    cmd.args(["--port", &port.to_string()]);
    cmd.args(["--admin-user", "admin", "--admin-password", "password"]);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch server {}", server_bin.display()))?;

    let http = reqwest::blocking::Client::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(resp) = http
            .get(format!("http://127.0.0.1:{port}/api/version"))
            .send()
        {
            if resp.status().is_success() {
                return Ok(child);
            }
        }
        if std::time::Instant::now() > deadline {
            bail!("hermetic server did not come up on {port}");
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn server_login(port: u16) -> Result<String> {
    let http = reqwest::blocking::Client::new();
    let resp: serde_json::Value = http
        .post(format!("http://127.0.0.1:{port}/api/auth/login"))
        .json(&json!({"username": "admin", "password": "password"}))
        .send()?
        .json()?;
    resp["token"]
        .as_str()
        .map(String::from)
        .context("no token in login response")
}

struct ScenarioResult {
    name: &'static str,
    passed: bool,
    detail: String,
    screenshot: Option<PathBuf>,
    diff: Option<f64>,
}

struct Harness {
    driver: Driver,
    args: Args,
    server_port: u16,
    results: Vec<ScenarioResult>,
}

impl Harness {
    fn settle(&self) {
        std::thread::sleep(Duration::from_millis(700));
    }

    fn shoot(&mut self, name: &'static str) -> Option<PathBuf> {
        if !self.args.screenshots {
            return None;
        }
        self.settle();
        // Record what page the app believes it is on at capture time, so
        // a stale-frame capture is distinguishable from a state bug.
        if let Ok(page) = self.driver.page() {
            eprintln!("shoot {name}: app page = {page}");
        }
        let path = self.args.artifacts.join(format!("{name}.png"));
        // GPUI's renderer first (pixel-identical everywhere); OS window
        // capture as fallback when the backend can't render offscreen.
        if self.driver.screenshot(&path).is_ok() {
            return Some(path);
        }
        match capture::screenshot_window(WINDOW_TITLE, &path) {
            Ok(()) => Some(path),
            Err(e) => {
                eprintln!("screenshot {name} failed: {e}");
                None
            }
        }
    }

    fn record(&mut self, name: &'static str, outcome: Result<String>, screenshot: Option<PathBuf>) {
        let (passed, mut detail) = match outcome {
            Ok(d) => (true, d),
            Err(e) => (false, format!("{e:#}")),
        };

        // Baseline comparison (screenshots that pass their scenario only).
        let mut diff = None;
        if passed {
            if let Some(shot) = screenshot.as_ref() {
                let baseline = self.args.baselines.join(format!("{name}.png"));
                if self.args.update_baselines {
                    let _ = std::fs::create_dir_all(&self.args.baselines);
                    let _ = std::fs::copy(shot, &baseline);
                    detail.push_str(" [baseline updated]");
                } else if baseline.exists() {
                    match compare::diff_ratio(shot, &baseline) {
                        Ok(d) => {
                            diff = Some(d);
                            if d > DIFF_TOLERANCE {
                                detail =
                                    format!("{detail} [VISUAL DIFF {d:.3} > {DIFF_TOLERANCE}]");
                            }
                        }
                        Err(e) => detail.push_str(&format!(" [diff error: {e}]")),
                    }
                } else {
                    detail.push_str(" [no baseline]");
                }
            }
        }
        let visual_ok = diff.map(|d| d <= DIFF_TOLERANCE).unwrap_or(true);
        self.results.push(ScenarioResult {
            name,
            passed: passed && visual_ok,
            detail,
            screenshot,
            diff,
        });
    }

    /// Fail a scenario whose screenshot is a wall of uniform tiles
    /// (posters/backdrop did not actually draw).
    fn assert_colorful(&mut self, name: &'static str) {
        if let Some(r) = self.results.iter_mut().find(|r| r.name == name) {
            if let Some(shot) = r.screenshot.clone() {
                match compare::colorfulness(&shot) {
                    Ok(c) if c < 0.08 => {
                        r.passed = false;
                        r.detail.push_str(&format!(
                            " [screenshot too uniform: {c:.3} — images likely blank]"
                        ));
                    }
                    Ok(c) => r.detail.push_str(&format!(" [colorfulness {c:.3}]")),
                    Err(e) => r.detail.push_str(&format!(" [colorfulness error: {e}]")),
                }
            }
        }
    }

    fn run(&mut self, name: &'static str, f: impl FnOnce(&mut Self) -> Result<String>) {
        if let Some(filter) = self.args.scenario.as_deref() {
            if filter != name {
                return;
            }
        }
        println!("--- scenario: {name}");
        let outcome = f(self);
        let shot = if outcome.is_ok() {
            self.shoot(name)
        } else {
            None
        };
        self.record(name, outcome, shot);
    }
}

/// GPUI needs a display; shells (SSH, agents, cron) often lack DISPLAY
/// and XAUTHORITY even when a desktop session is running. Harvest both
/// from a session process (gnome-shell/plasma/Xwayland) so the app opens
/// on the X11 backend, where xdotool capture works.
#[cfg(target_os = "linux")]
fn ensure_x11_display() {
    let have_display = std::env::var("DISPLAY")
        .map(|d| !d.is_empty())
        .unwrap_or(false);
    let have_auth = std::env::var("XAUTHORITY")
        .map(|d| !d.is_empty())
        .unwrap_or(false);
    if have_display && have_auth {
        return;
    }
    for proc_name in ["gnome-shell", "plasmashell", "Xwayland", "xfce4-session"] {
        let Ok(out) = Command::new("pgrep")
            .args(["-u", &whoami(), proc_name])
            .output()
        else {
            continue;
        };
        for pid in String::from_utf8_lossy(&out.stdout).lines() {
            let Ok(environ) = std::fs::read(format!("/proc/{}/environ", pid.trim())) else {
                continue;
            };
            for entry in environ.split(|b| *b == 0) {
                let entry = String::from_utf8_lossy(entry);
                if let Some(v) = entry.strip_prefix("DISPLAY=") {
                    if !have_display && !v.is_empty() {
                        std::env::set_var("DISPLAY", v);
                    }
                }
                if let Some(v) = entry.strip_prefix("XAUTHORITY=") {
                    if !have_auth && !v.is_empty() {
                        std::env::set_var("XAUTHORITY", v);
                    }
                }
            }
            if std::env::var("DISPLAY")
                .map(|d| !d.is_empty())
                .unwrap_or(false)
            {
                eprintln!(
                    "using session env from {proc_name}: DISPLAY={} XAUTHORITY={}",
                    std::env::var("DISPLAY").unwrap_or_default(),
                    std::env::var("XAUTHORITY").unwrap_or_default()
                );
                return;
            }
        }
    }
    // Last resort: probe X sockets without auth.
    for cand in 0..3 {
        if std::path::Path::new(&format!("/tmp/.X11-unix/X{cand}")).exists() {
            std::env::set_var("DISPLAY", format!(":{cand}"));
            eprintln!("using DISPLAY=:{cand} (no XAUTHORITY found)");
            return;
        }
    }
}

#[cfg(target_os = "linux")]
fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "root".into())
}

#[cfg(not(target_os = "linux"))]
fn ensure_x11_display() {}

/// Test-style export name for a scenario ("01-home" -> "home_page.jpg").
fn export_name(scenario: &str) -> String {
    match scenario {
        "01-home" => "home_page".into(),
        "02-search-typing" => "search_typing".into(),
        "03-search-clear" => "search_clear".into(),
        "04-downloads" => "downloads_page".into(),
        "05-back-navigation" => "back_navigation".into(),
        "06-settings" => "settings_page".into(),
        "07-live-posters" => "home_posters".into(),
        "08-movie-page" => "movie_page".into(),
        other => other
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '-')
            .replace('-', "_"),
    }
}

/// Copy run screenshots as JPEGs into `$TEST_SCREENSHOTS_DIR/<timestamp>/`.
/// The var comes from the environment or the repo `.env`; when absent,
/// nothing is exported.
fn export_screenshots(results: &[ScenarioResult]) {
    let dir = std::env::var("TEST_SCREENSHOTS_DIR").ok().or_else(|| {
        let env_file = std::fs::read_to_string(".env").ok()?;
        env_file.lines().find_map(|l| {
            l.trim()
                .strip_prefix("TEST_SCREENSHOTS_DIR=")
                .map(|v| v.trim().trim_matches('"').to_string())
        })
    });
    let Some(dir) = dir.filter(|d| !d.is_empty()) else {
        return;
    };
    let stamp = chrono_like_timestamp();
    let run_dir = PathBuf::from(&dir).join(&stamp);
    if let Err(e) = std::fs::create_dir_all(&run_dir) {
        eprintln!(
            "screenshot export skipped: cannot create {}: {e}",
            run_dir.display()
        );
        return;
    }
    let mut exported = 0;
    for r in results {
        let Some(src) = r.screenshot.as_ref() else {
            continue;
        };
        let dest = run_dir.join(format!("{}.jpg", export_name(r.name)));
        match image::open(src) {
            Ok(img) => match img
                .to_rgb8()
                .save_with_format(&dest, image::ImageFormat::Jpeg)
            {
                Ok(()) => exported += 1,
                Err(e) => eprintln!("export {} failed: {e}", dest.display()),
            },
            Err(e) => eprintln!("export read {} failed: {e}", src.display()),
        }
    }
    println!("exported {exported} screenshots to {}", run_dir.display());
}

/// `2026-08-01_123455` without pulling in chrono: seconds since epoch
/// converted via libc's localtime through the `date` command fallback.
fn chrono_like_timestamp() -> String {
    std::process::Command::new("date")
        .arg("+%Y-%m-%d_%H%M%S")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            format!(
                "run-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            )
        })
}

fn main() -> Result<()> {
    ensure_x11_display();
    let args = parse_args();
    std::fs::create_dir_all(&args.artifacts)?;

    if !args.app.exists() {
        bail!(
            "desktop binary not found at {} — build with: cargo build -p streamx-desktop --features ui-test",
            args.app.display()
        );
    }

    let tmp = tempfile::tempdir()?;
    let ui_port = free_port();

    if !args.server.exists() {
        bail!(
            "server binary not found at {} — build with: cargo build -p streamx",
            args.server.display()
        );
    }
    // Every on-disk path (config dir, db, torrents, cache, posters, dht,
    // tmp files) is redirected into the run's tempdir via STREAMX_DATA_DIR
    // — nothing in ~/.streamx is touched. Live mode only borrows the real
    // config file (read-only) so the provider list, and therefore real
    // posters, are in play.
    let server_port = free_port();
    let data_dir = tmp.path().join("data");
    let mut path_envs: Vec<(String, String)> = vec![(
        "STREAMX_DATA_DIR".into(),
        data_dir.to_string_lossy().into_owned(),
    )];
    if args.live {
        let home = std::env::var("HOME").unwrap_or_default();
        let real_cfg = PathBuf::from(&home).join(".streamx").join("config.toml");
        if !real_cfg.exists() {
            bail!("--live needs {} for the provider list", real_cfg.display());
        }
        path_envs.push((
            "STREAMX_CONFIG".into(),
            real_cfg.to_string_lossy().into_owned(),
        ));
    }
    let server_child = Some(start_server(&args.server, server_port, &path_envs)?);

    // Desktop config: thin client against the spawned server, token
    // pre-provisioned so the app boots straight to the home page.
    let cfg_dir = tmp.path().join("desktop-config");
    std::fs::create_dir_all(&cfg_dir)?;
    let token = server_login(server_port)?;
    std::fs::write(cfg_dir.join("mode"), "thin-client")?;
    std::fs::write(
        cfg_dir.join("server_url"),
        format!("http://127.0.0.1:{server_port}"),
    )?;
    std::fs::write(cfg_dir.join("token"), token)?;

    let mut app_cmd = Command::new(&args.app);
    app_cmd
        .env("STREAMX_UI_TEST_PORT", ui_port.to_string())
        .env("RUST_LOG", "warn,streamx_desktop=debug")
        // Force the X11 backend so xdotool/import capture works on both
        // X11 and Wayland (XWayland) Linux sessions.
        .env_remove("WAYLAND_DISPLAY");
    app_cmd
        .env("STREAMX_DESKTOP_CONFIG_OVERRIDE", &cfg_dir)
        .env("STREAMX_DESKTOP_NO_EMBED", "1");
    for (k, v) in &path_envs {
        app_cmd.env(k, v);
    }
    // tracing writes to stdout; panics to stderr. Capture both.
    let app_log = std::fs::File::create(args.artifacts.join("app.log"))?;
    let app_log_err = app_log.try_clone()?;
    let app_child = app_cmd
        .stdout(Stdio::from(app_log))
        .stderr(Stdio::from(app_log_err))
        .spawn()
        .with_context(|| format!("failed to launch app {}", args.app.display()))?;

    let _procs = Procs {
        server: server_child,
        app: app_child,
    };

    let driver = Driver::connect(ui_port, Duration::from_secs(30))?;
    let server_port_copy = server_port;
    let mut h = Harness {
        driver,
        args,
        server_port: server_port_copy,
        results: Vec::new(),
    };

    // 1. App boots to the authed home page with a real window.
    h.run("01-home", |h| {
        let state = h
            .driver
            .wait_for(Duration::from_secs(20), "home page + window", |s| {
                s["page"] == "Movies" && s["authed"] == true && s["window_open"] == true
            })?;
        Ok(format!(
            "home page up (tiles latest={}, poster_failures={})",
            state["tiles"]["latest"], state["poster_failures"]
        ))
    });

    // 2. Typing in the search field through real key events runs a search.
    h.run("02-search-typing", |h| {
        h.driver.keys(&["/"])?; // FocusSearch shortcut
        std::thread::sleep(Duration::from_millis(300));
        h.driver.type_text("batman")?;
        // Every typed key must land in the real input entity, and the
        // debounce must fire the search (query mirrors the fired text).
        let state =
            h.driver
                .wait_for(Duration::from_secs(10), "typed text + search fired", |s| {
                    s["search_input"] == "batman" && s["query"] == "batman"
                })?;
        Ok(format!(
            "typed 'batman' via key events; search fired (results={})",
            state["search_results"]
        ))
    });

    // 3. Clearing the field restores the browse view (empty query).
    // The field is still focused from the previous scenario; backspace
    // past the text length to prove over-deleting is harmless.
    h.run("03-search-clear", |h| {
        for _ in 0..10 {
            h.driver.keys(&["backspace"])?;
        }
        let state = h.driver.wait_for(
            Duration::from_secs(10),
            "field cleared + browse back",
            |s| s["search_input"] == "" && s["query"] == "",
        )?;
        h.driver.keys(&["escape"])?;
        std::thread::sleep(Duration::from_millis(300));
        let page = h.driver.page()?;
        if page != "Movies" {
            bail!("expected Movies after escape, got {page}");
        }
        let _ = state;
        Ok("backspaces cleared the field; browse view restored".into())
    });

    // 4. Downloads queue: create + pin a download via API, page shows it.
    // Live mode never seeds fake rows into the real database; the page
    // just has to render whatever is there.
    h.run("04-downloads", |h| {
        let seeded = !h.args.live;
        if seeded {
            let http = reqwest::blocking::Client::new();
            let token = server_login(h.server_port)?;
            let hash = "cafe0000cafe0000cafe0000cafe0000cafe0000";
            http.post(format!("http://127.0.0.1:{}/api/stream", h.server_port))
                .bearer_auth(&token)
                .json(&json!({
                    "magnet_uri": format!("magnet:?xt=urn:btih:{hash}&dn=Harness%20Movie"),
                    "title": "Harness Movie",
                }))
                .send()?
                .error_for_status()?;
            http.post(format!(
                "http://127.0.0.1:{}/api/stream/{hash}/download",
                h.server_port
            ))
            .bearer_auth(&token)
            .send()?
            .error_for_status()?;
        }
        h.driver.navigate("downloads")?;
        let state = h
            .driver
            .wait_for(Duration::from_secs(15), "downloads listed", |s| {
                s["page"] == "Downloads" && (!seeded || s["downloads"].as_u64().unwrap_or(0) >= 1)
            })?;
        Ok(format!("downloads page ({} items)", state["downloads"]))
    });

    // 5. Header back walks history like a browser.
    h.run("05-back-navigation", |h| {
        h.driver.back()?;
        std::thread::sleep(Duration::from_millis(400));
        let page = h.driver.page()?;
        if page != "Movies" {
            bail!("expected Movies after back, got {page}");
        }
        Ok("back returned to home".into())
    });

    // 6. Settings page renders (maintenance section etc.).
    h.run("06-settings", |h| {
        h.driver.navigate("settings")?;
        std::thread::sleep(Duration::from_millis(400));
        let page = h.driver.page()?;
        if page != "Settings" {
            bail!("expected Settings, got {page}");
        }
        Ok("settings page up".into())
    });

    // 10-13: responsive scaling matrix. The app derives a global UI
    // scale from the window; assert it engages and screenshot each page
    // at each size so regressions are visual AND functional.
    h.run("10-scale-small-home", |h| {
        h.driver.navigate("search")?;
        h.driver
            .send(&serde_json::json!({"cmd": "resize", "w": 800.0, "h": 600.0}))?;
        let st = h
            .driver
            .wait_for(Duration::from_secs(10), "small ui_scale", |s| {
                s["ui_scale"].as_f64().unwrap_or(1.0) < 0.95
            })?;
        Ok(format!("800x600 -> ui_scale={}", st["ui_scale"]))
    });

    h.run("11-scale-large-home", |h| {
        h.driver
            .send(&serde_json::json!({"cmd": "resize", "w": 1920.0, "h": 1000.0}))?;
        let st = h
            .driver
            .wait_for(Duration::from_secs(10), "large ui_scale", |s| {
                s["ui_scale"].as_f64().unwrap_or(1.0) > 1.3
            })?;
        Ok(format!("1920x1000 -> ui_scale={}", st["ui_scale"]))
    });

    h.run("12-scale-large-search", |h| {
        h.driver.keys(&["/"])?;
        std::thread::sleep(Duration::from_millis(300));
        h.driver.type_text("dune")?;
        h.driver
            .wait_for(Duration::from_secs(10), "typed at large scale", |s| {
                s["search_input"] == "dune"
            })?;
        Ok("search box + results view at 1.4x scale".into())
    });

    h.run("13-scale-large-settings", |h| {
        // Clear the field and WAIT for the keys to land before leaving
        // the page: keystrokes queued while the focused input is not
        // rendered are dropped by GPUI.
        for _ in 0..6 {
            h.driver.keys(&["backspace"])?;
        }
        h.driver
            .wait_for(Duration::from_secs(10), "field cleared", |s| {
                s["search_input"] == ""
            })?;
        h.driver.keys(&["escape"])?;
        std::thread::sleep(Duration::from_millis(300));
        h.driver.navigate("settings")?;
        std::thread::sleep(Duration::from_millis(400));
        let page = h.driver.page()?;
        if page != "Settings" {
            bail!("expected Settings, got {page}");
        }
        Ok("settings at 1.4x scale".into())
    });

    // Restore the default geometry for the remaining scenarios and the
    // stable baselines.
    h.driver
        .send(&serde_json::json!({"cmd": "resize", "w": 1100.0, "h": 720.0}))?;
    h.driver.navigate("search")?;
    std::thread::sleep(Duration::from_millis(500));

    // 7. Live-only: home tiles must show real posters (few/no failures,
    // and the screenshot must be visually busy, i.e. images actually drawn).
    if h.args.live {
        h.run("07-live-posters", |h| {
            h.driver.navigate("search")?;
            // Wait until every started poster download has landed
            // (pending drains to zero), not just until URLs exist.
            let state =
                h.driver
                    .wait_for(Duration::from_secs(90), "browse tiles + posters", |s| {
                        s["tiles"]["latest"].as_u64().unwrap_or(0) > 0
                            && s["browse_loading"] == false
                            && s["poster_pending"].as_u64().unwrap_or(99) == 0
                            && s["poster_failures"].as_u64().unwrap_or(99) <= 3
                    })?;
            // One paint cycle for the evicted assets to re-render.
            std::thread::sleep(Duration::from_secs(2));
            Ok(format!(
                "tiles latest={} popular={} poster_failures={} pending=0",
                state["tiles"]["latest"], state["tiles"]["popular"], state["poster_failures"]
            ))
        });
        h.assert_colorful("07-live-posters");

        // 8. Live-only: open the first movie from the home grid via the
        // keyboard (Enter activates the first tile) and verify the movie
        // page renders with poster + backdrop.
        h.run("08-movie-page", |h| {
            // Focus was already cleared in 13 (escape there would pop
            // the page stack here). Enter activates the first tile.
            h.driver.keys(&["enter"])?;
            h.driver
                .wait_for(Duration::from_secs(10), "movie page", |s| {
                    s["page"] == "Movie"
                })?;
            let _ = h
                .driver
                .wait_for(Duration::from_secs(30), "movie images", |s| {
                    s["poster_pending"].as_u64().unwrap_or(99) == 0
                });
            std::thread::sleep(Duration::from_secs(2));
            Ok("movie page open".into())
        });
        h.assert_colorful("08-movie-page");
        // The backdrop must reach the BOTTOM of the window even when the
        // variant list is short — a flat dark band there means the
        // dimmed backdrop ended mid-screen.
        if let Some(r) = h.results.iter_mut().find(|r| r.name == "08-movie-page") {
            if let Some(shot) = r.screenshot.clone() {
                match compare::region_colorfulness(&shot, 0.85, 1.0) {
                    Ok(c) if c < 0.015 => {
                        r.passed = false;
                        r.detail.push_str(&format!(
                            " [bottom band flat ({c:.3}) — backdrop does not reach the bottom]"
                        ));
                    }
                    Ok(c) => r.detail.push_str(&format!(" [bottom band {c:.3}]")),
                    Err(e) => r.detail.push_str(&format!(" [region error: {e}]")),
                }
            }
        }

        // 9. Live-only: a category drill-down fills its infinite-scroll
        // grid with real tiles.
        h.run("09-category", |h| {
            h.driver
                .send(&serde_json::json!({"cmd": "category", "name": "Most Popular"}))?;
            let state = h
                .driver
                .wait_for(Duration::from_secs(20), "category grid", |s| {
                    s["page"] == "Browse" && s["category_items"].as_u64().unwrap_or(0) >= 10
                })?;
            let _ = h
                .driver
                .wait_for(Duration::from_secs(30), "category images", |s| {
                    s["poster_pending"].as_u64().unwrap_or(99) == 0
                });
            std::thread::sleep(Duration::from_secs(2));
            Ok(format!("category grid ({} items)", state["category_items"]))
        });
        h.assert_colorful("09-category");
    }

    h.driver.quit();

    // Report.
    println!("\n=== UI harness results ===");
    let mut failed = 0;
    let mut report = Vec::new();
    for r in &h.results {
        let status = if r.passed { "PASS" } else { "FAIL" };
        if !r.passed {
            failed += 1;
        }
        let diff = r.diff.map(|d| format!(" diff={d:.3}")).unwrap_or_default();
        println!("{status}  {}{diff}  {}", r.name, r.detail);
        report.push(json!({
            "name": r.name,
            "passed": r.passed,
            "detail": r.detail,
            "screenshot": r.screenshot.as_ref().map(|p| p.to_string_lossy()),
            "diff": r.diff,
        }));
    }
    std::fs::write(
        h.args.artifacts.join("report.json"),
        serde_json::to_string_pretty(&json!({ "results": report }))?,
    )?;
    export_screenshots(&h.results);
    println!(
        "artifacts: {} ({} scenarios, {} failed)",
        h.args.artifacts.display(),
        h.results.len(),
        failed
    );

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
