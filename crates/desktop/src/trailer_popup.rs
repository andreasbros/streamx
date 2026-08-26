//! Small popup window for YouTube trailers.
//!
//! macOS: a plain titled `NSWindow` hosting a `WKWebView` (system
//! WebKit, no extra dependencies). The page is a minimal wrapper with a
//! full-window YouTube embed iframe, so only the video player shows;
//! no YouTube site chrome. A single window is reused across trailers,
//! and closing it stops playback.
//!
//! Linux and Windows: a Chromium-family browser in `--app` mode, a
//! chromeless window showing only the wrapper page, which the StreamX
//! server serves (`/api/trailer/page/{id}`) so the embed has a real
//! embedding origin. Nothing new is linked, so the linkage policies
//! are untouched; closing the window ends playback with it. When no
//! capable browser is found, `open` returns `false` and the caller
//! falls back to the default browser.

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)] // objc 0.2 macros carry a legacy cargo-clippy cfg
mod imp {
    use objc::declare::ClassDecl;
    use objc::runtime::{Class, Object, Sel, NO, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CString;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Once;

    static WINDOW: AtomicUsize = AtomicUsize::new(0);
    static WEBVIEW: AtomicUsize = AtomicUsize::new(0);
    static REGISTER: Once = Once::new();

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }
    unsafe impl objc::Encode for Rect {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
        }
    }

    unsafe fn ns_string(s: &str) -> *mut Object {
        let c = CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    /// Closing the popup must stop playback: hiding the window leaves
    /// the WKWebView running (audio keeps playing). Blank the webview
    /// before the normal close.
    extern "C" fn close_stops_playback(this: &Object, _sel: Sel) {
        unsafe {
            let wv = WEBVIEW.load(Ordering::Relaxed) as *mut Object;
            if !wv.is_null() {
                let blank = ns_string("about:blank");
                let url: *mut Object = msg_send![class!(NSURL), URLWithString: blank];
                let req: *mut Object = msg_send![class!(NSURLRequest), requestWithURL: url];
                let _: *mut Object = msg_send![wv, loadRequest: req];
            }
            let _: () = msg_send![super(this, class!(NSWindow)), close];
        }
    }

    fn window_class() -> &'static Class {
        REGISTER.call_once(|| {
            let mut decl = ClassDecl::new("StreamXTrailerWindow", class!(NSWindow))
                .expect("class registration");
            unsafe {
                decl.add_method(
                    sel!(close),
                    close_stops_playback as extern "C" fn(&Object, Sel),
                );
            }
            decl.register();
        });
        Class::get("StreamXTrailerWindow").unwrap_or_else(|| class!(NSWindow))
    }

    /// Main thread only (AppKit). Returns false if the window could not
    /// be created.
    pub fn open(html: &str, base_url: &str, title: &str) -> bool {
        unsafe {
            let mut win = WINDOW.load(Ordering::Relaxed) as *mut Object;
            let mut web = WEBVIEW.load(Ordering::Relaxed) as *mut Object;
            if win.is_null() {
                let config: *mut Object = msg_send![class!(WKWebViewConfiguration), new];
                if config.is_null() {
                    return false;
                }
                // Allow the embed player to autoplay with sound.
                let _: () = msg_send![config, setMediaTypesRequiringUserActionForPlayback: 0u64];
                let prefs: *mut Object = msg_send![config, preferences];
                if !prefs.is_null() {
                    let responds: bool = msg_send![
                        prefs,
                        respondsToSelector: sel!(setElementFullscreenEnabled:)
                    ];
                    if responds {
                        let _: () = msg_send![prefs, setElementFullscreenEnabled: YES];
                    }
                }
                let frame = Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 960.0,
                    h: 540.0,
                };
                let wv: *mut Object = msg_send![class!(WKWebView), alloc];
                let wv: *mut Object = msg_send![wv, initWithFrame:frame configuration:config];
                if wv.is_null() {
                    return false;
                }
                // A normal Safari UA so YouTube serves the standard
                // player. The WebKit cookie store persists, so consent
                // or sign-in done here sticks across popups.
                let ua = ns_string(
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                     AppleWebKit/605.1.15 (KHTML, like Gecko) \
                     Version/17.4 Safari/605.1.15",
                );
                let _: () = msg_send![wv, setCustomUserAgent: ua];
                // titled | closable | miniaturizable | resizable
                let style: u64 = 1 | 2 | 4 | 8;
                let w: *mut Object = msg_send![window_class(), alloc];
                let w: *mut Object = msg_send![w,
                    initWithContentRect:frame
                    styleMask:style
                    backing:2u64
                    defer:NO];
                if w.is_null() {
                    return false;
                }
                let _: () = msg_send![w, setReleasedWhenClosed: NO];
                let _: () = msg_send![w, setContentView: wv];
                let _: () = msg_send![w, center];
                WINDOW.store(w as usize, Ordering::Relaxed);
                WEBVIEW.store(wv as usize, Ordering::Relaxed);
                win = w;
                web = wv;
            }
            let _: () = msg_send![win, setTitle: ns_string(title)];
            let ns_base: *mut Object = msg_send![class!(NSURL), URLWithString: ns_string(base_url)];
            let _: *mut Object = msg_send![web, loadHTMLString: ns_string(html) baseURL: ns_base];
            let nil: *mut Object = std::ptr::null_mut();
            let _: () = msg_send![win, makeKeyAndOrderFront: nil];
            true
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod browser {
    use std::path::PathBuf;

    /// Chromium-family commands able to open an `--app` window, most
    /// specific first. `lookup` resolves environment variables so the
    /// list is testable without mutating the process environment.
    pub(super) fn candidates(lookup: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(over) = lookup("STREAMX_TRAILER_BROWSER") {
            if !over.is_empty() {
                out.push(PathBuf::from(over));
            }
        }
        #[cfg(target_os = "windows")]
        {
            // Edge ships with every Windows 10+; Chrome as a fallback.
            for base in ["ProgramFiles(x86)", "ProgramFiles"] {
                if let Some(dir) = lookup(base) {
                    out.push(PathBuf::from(&dir).join("Microsoft/Edge/Application/msedge.exe"));
                }
            }
            for base in ["ProgramFiles", "LocalAppData"] {
                if let Some(dir) = lookup(base) {
                    out.push(PathBuf::from(&dir).join("Google/Chrome/Application/chrome.exe"));
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            for name in [
                "chromium",
                "chromium-browser",
                "google-chrome",
                "google-chrome-stable",
                "brave",
                "brave-browser",
                "microsoft-edge",
                "vivaldi",
            ] {
                out.push(PathBuf::from(name));
            }
        }
        out
    }

    /// Spawn the first candidate that starts. Bare names resolve via
    /// PATH; absolute candidates are skipped when the file is absent.
    pub(super) fn open_app_window(url: &str) -> bool {
        for cmd in candidates(|k| std::env::var(k).ok()) {
            if cmd.is_absolute() && !cmd.is_file() {
                continue;
            }
            let spawned = std::process::Command::new(&cmd)
                .arg(format!("--app={url}"))
                .arg("--window-size=960,540")
                .spawn();
            if let Ok(child) = spawned {
                tracing::info!(browser = %cmd.display(), "trailer: app-mode popup opened");
                drop(child);
                return true;
            }
        }
        false
    }
}

/// Wrapper page URL on the StreamX server (embedded or remote).
#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn page_url(server_base: &str, youtube_id: &str) -> String {
    format!(
        "{}/api/trailer/page/{}",
        server_base.trim_end_matches('/'),
        youtube_id
    )
}

/// Open a trailer popup showing only the embedded video player.
/// Returns true when handled; false means the caller should fall back
/// to the default browser.
pub fn open(youtube_id: &str, title: &str, server_base: &str) -> bool {
    // Wrapper page: full-window iframe embed. Loaded with an https base
    // URL so the embed has a real embedding origin (a bare top-level
    // embed URL is rejected with error 153).
    let html = format!(
        "<!doctype html><html><head>\
         <meta name=\"viewport\" content=\"width=device-width\">\
         <style>html,body{{margin:0;height:100%;background:#000;overflow:hidden}}\
         iframe{{width:100%;height:100%;border:0}}</style></head><body>\
         <iframe src=\"https://www.youtube.com/embed/{youtube_id}?autoplay=1&rel=0&playsinline=1\" \
         allow=\"autoplay; fullscreen; encrypted-media\" allowfullscreen></iframe>\
         </body></html>"
    );
    let base_url = "https://play.streamxos.com/";
    #[cfg(target_os = "macos")]
    {
        let _ = server_base;
        imp::open(&html, base_url, title)
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        let _ = (html, base_url, title);
        browser::open_app_window(&page_url(server_base, youtube_id))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (html, base_url, title, server_base);
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn page_url_normalizes_trailing_slash() {
        assert_eq!(
            super::page_url("http://127.0.0.1:8999/", "dQw4w9WgXcQ"),
            "http://127.0.0.1:8999/api/trailer/page/dQw4w9WgXcQ"
        );
        assert_eq!(
            super::page_url("https://play.example.com", "abc-DEF_12"),
            "https://play.example.com/api/trailer/page/abc-DEF_12"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn browser_override_is_first_candidate() {
        let list = super::browser::candidates(|k| {
            (k == "STREAMX_TRAILER_BROWSER").then(|| "/opt/custom/browser".to_string())
        });
        assert_eq!(list[0], std::path::PathBuf::from("/opt/custom/browser"));
        assert!(list.len() > 1, "built-in candidates follow the override");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_candidates_cover_chromium_family() {
        let list = super::browser::candidates(|_| None);
        let names: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
        for expected in ["chromium", "google-chrome", "brave", "microsoft-edge"] {
            assert!(
                names.iter().any(|n| n.contains(expected)),
                "missing {expected} in {names:?}"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_candidates_resolve_edge_from_program_files() {
        let list = super::browser::candidates(|k| match k {
            "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_string()),
            "ProgramFiles" => Some(r"C:\Program Files".to_string()),
            _ => None,
        });
        assert!(list
            .iter()
            .any(|p| p.display().to_string().contains("msedge.exe")));
        assert!(list
            .iter()
            .any(|p| p.display().to_string().contains("chrome.exe")));
    }
}
