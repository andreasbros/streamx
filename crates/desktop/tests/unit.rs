//! Unit tests for the pure pieces of the desktop crate - no GPU, no window.
//!
//! These cover:
//!   - Keybindings translation from GPUI KeyDownEvents to our Shortcut enum.
//!   - Playback path resolution (longest common dir, candidate enumeration,
//!     mpv argument building).
//!   - AppState page stack mutations.
//!   - Mode on-disk round-trip.
//!
//! Run with:  nix develop --command cargo test -p streamx-desktop --test unit

use streamx_desktop::{
    app::{fire_debounced, info_hash_from_magnet, DebounceState},
    components::{tile_layout, TILE_MAX_W, TILE_MIN_W},
    keybindings::{translate, Shortcut},
    playback::{candidate_paths, longest_common_dir, PlayTarget},
    router::Page,
    state::{AppState, Mode},
    text_input::TextModel,
    theme::{responsive_scale, set_viewport_width, ui_scale, UI_BASELINE_W},
};

use gpui::{KeyDownEvent, Keystroke, Modifiers};
use std::path::PathBuf;
use streamx_api::types::TorrentFile;

fn key(k: &str) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers::default(),
            key: k.into(),
            key_char: None,
        },
        is_held: false,
        prefer_character_input: false,
    }
}

fn key_shift(k: &str) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                control: false,
                alt: false,
                shift: true,
                platform: false,
                function: false,
            },
            key: k.into(),
            key_char: None,
        },
        is_held: false,
        prefer_character_input: false,
    }
}

fn key_ctrl(k: &str) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                control: true,
                alt: false,
                shift: false,
                platform: false,
                function: false,
            },
            key: k.into(),
            key_char: None,
        },
        is_held: false,
        prefer_character_input: false,
    }
}

// ---------- keybindings ----------

#[test]
fn escape_maps_to_back() {
    assert_eq!(translate(&key("escape")), Some(Shortcut::Back));
}

#[test]
fn enter_maps_to_activate() {
    assert_eq!(translate(&key("enter")), Some(Shortcut::Activate));
}

#[test]
fn slash_focuses_search() {
    assert_eq!(translate(&key("/")), Some(Shortcut::FocusSearch));
}

#[test]
fn ctrl_k_focuses_search() {
    assert_eq!(translate(&key_ctrl("k")), Some(Shortcut::FocusSearch));
}

#[test]
fn tab_forward_backward() {
    assert_eq!(translate(&key("tab")), Some(Shortcut::FocusNext));
    assert_eq!(translate(&key_shift("tab")), Some(Shortcut::FocusPrev));
}

#[test]
fn arrow_keys_all_four() {
    assert_eq!(translate(&key("left")), Some(Shortcut::Left));
    assert_eq!(translate(&key("right")), Some(Shortcut::Right));
    assert_eq!(translate(&key("up")), Some(Shortcut::Up));
    assert_eq!(translate(&key("down")), Some(Shortcut::Down));
}

#[test]
fn m_and_f_bare_only() {
    assert_eq!(translate(&key("m")), Some(Shortcut::ToggleMenu));
    assert_eq!(translate(&key("f")), Some(Shortcut::Fullscreen));
    // Modifier combos should NOT trigger the bare shortcut.
    assert!(!matches!(
        translate(&key_ctrl("m")),
        Some(Shortcut::ToggleMenu)
    ));
}

#[test]
fn unknown_key_returns_none() {
    assert_eq!(translate(&key("pagedown")), None);
    assert_eq!(translate(&key("f13")), None);
}

// ---------- playback ----------

fn file(idx: usize, path: &str, size: u64, video: bool, audio: bool) -> TorrentFile {
    TorrentFile {
        index: idx,
        path: path.into(),
        size,
        is_video: video,
        is_audio: audio,
    }
}

#[test]
fn longest_common_dir_nested() {
    let files = vec![
        file(0, "Album/track1.mp3", 1, false, true),
        file(1, "Album/track2.mp3", 1, false, true),
        file(2, "Album/track3.mp3", 1, false, true),
    ];
    assert_eq!(longest_common_dir(&files).as_deref(), Some("Album"));
}

#[test]
fn longest_common_dir_mixed_returns_none() {
    let files = vec![
        file(0, "a.mp4", 1, true, false),
        file(1, "Other/b.mp4", 1, true, false),
    ];
    assert_eq!(longest_common_dir(&files), None);
}

#[test]
fn longest_common_dir_single_file_flat() {
    let files = vec![file(0, "movie.mp4", 1, true, false)];
    // Single flat file has no common directory.
    assert_eq!(longest_common_dir(&files), None);
}

#[test]
fn candidate_paths_single_flat_file() {
    let data = PathBuf::from("/tmp/streamx_test");
    let files = vec![file(0, "movie.mp4", 1_000, true, false)];
    let target = &files[0];
    let paths = candidate_paths(&data, &files, target);
    // Expect: complete/movie.mp4 and partial/movie.mp4 (no nested dir guess).
    assert_eq!(paths.len(), 2);
    assert!(paths[0].ends_with("complete/movie.mp4"));
    assert!(paths[1].ends_with("partial/movie.mp4"));
}

#[test]
fn candidate_paths_nested_album() {
    let data = PathBuf::from("/tmp/streamx_test");
    let files = vec![
        file(0, "Album/01.mp3", 1, false, true),
        file(1, "Album/02.mp3", 1, false, true),
    ];
    let target = &files[0];
    let paths = candidate_paths(&data, &files, target);
    // Expect nested (Album/Album/01.mp3) and flat (Album/01.mp3) per base dir.
    assert_eq!(paths.len(), 4);
    assert!(paths[0].ends_with("complete/Album/Album/01.mp3"));
    assert!(paths[1].ends_with("complete/Album/01.mp3"));
    assert!(paths[2].ends_with("partial/Album/Album/01.mp3"));
    assert!(paths[3].ends_with("partial/Album/01.mp3"));
}

#[test]
fn play_target_local_mpv_args() {
    let t = PlayTarget::LocalFile(PathBuf::from("/data/movie.mp4"));
    assert_eq!(t.mpv_args(), vec!["/data/movie.mp4"]);
}

#[test]
fn play_target_http_without_token() {
    let t = PlayTarget::Http {
        url: "http://example/file".into(),
        token: None,
    };
    assert_eq!(t.mpv_args(), vec!["http://example/file"]);
}

#[test]
fn play_target_http_with_token_adds_header() {
    let t = PlayTarget::Http {
        url: "http://example/file".into(),
        token: Some("jwt.token".into()),
    };
    let args = t.mpv_args();
    assert_eq!(args[0], "http://example/file");
    assert_eq!(
        args[1],
        "--http-header-fields=Authorization: Bearer jwt.token"
    );
}

#[test]
fn play_target_display() {
    let local = PlayTarget::LocalFile(PathBuf::from("/x/y.mkv"));
    assert_eq!(local.display(), "/x/y.mkv");
    let http = PlayTarget::Http {
        url: "http://a/b".into(),
        token: None,
    };
    assert_eq!(http.display(), "http://a/b");
}

// ---------- state ----------

#[test]
fn mode_roundtrip() {
    assert_eq!(Mode::from_str("embedded"), Mode::Embedded);
    assert_eq!(Mode::from_str("thin-client"), Mode::ThinClient);
    assert_eq!(Mode::from_str("unknown"), Mode::Embedded); // default
    assert_eq!(Mode::Embedded.as_str(), "embedded");
    assert_eq!(Mode::ThinClient.as_str(), "thin-client");
}

// cargo test runs each test in its own thread. The config override lives in a
// process-wide env var, so tests that touch it must serialize.
static CONFIG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_temp_config<R>(f: impl FnOnce() -> R) -> R {
    let _guard = CONFIG_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("tempdir");
    let prev = std::env::var("STREAMX_DESKTOP_CONFIG_OVERRIDE").ok();
    unsafe {
        std::env::set_var("STREAMX_DESKTOP_CONFIG_OVERRIDE", dir.path());
    }
    let r = f();
    unsafe {
        match prev {
            Some(v) => std::env::set_var("STREAMX_DESKTOP_CONFIG_OVERRIDE", v),
            None => std::env::remove_var("STREAMX_DESKTOP_CONFIG_OVERRIDE"),
        }
    }
    r
}

#[test]
fn page_stack_navigate_back() {
    with_temp_config(|| {
        let state = AppState::new();
        state.replace_page(Page::Login);

        assert_eq!(state.current_page(), Page::Login);
        state.navigate(Page::Search);
        assert_eq!(state.current_page(), Page::Search);
        state.navigate(Page::Movie);
        assert_eq!(state.current_page(), Page::Movie);

        assert!(state.back());
        assert_eq!(state.current_page(), Page::Search);
        assert!(state.back());
        assert_eq!(state.current_page(), Page::Login);

        // At root, back() is a no-op returning false.
        assert!(!state.back());
        assert_eq!(state.current_page(), Page::Login);
    });
}

#[test]
fn replace_page_clears_stack() {
    with_temp_config(|| {
        let state = AppState::new();
        state.navigate(Page::Search);
        state.navigate(Page::Movie);
        state.navigate(Page::Player);

        state.replace_page(Page::Login);
        assert_eq!(state.current_page(), Page::Login);
        assert!(!state.back(), "replace_page should reset stack to one page");
    });
}

#[test]
fn set_mode_persists_to_override_config() {
    with_temp_config(|| {
        let state = AppState::new();
        state.set_mode(Mode::ThinClient);

        // Fresh AppState in the same override dir should pick up the mode.
        let state2 = AppState::new();
        assert_eq!(*state2.mode.read(), Mode::ThinClient);
    });
}

// ---------- TextModel ----------

#[test]
fn text_model_insert_moves_cursor() {
    let mut m = TextModel::new();
    m.insert_str("hi");
    assert_eq!(m.value(), "hi");
    assert_eq!(m.cursor(), 2);
}

#[test]
fn text_model_backspace_at_end() {
    let mut m = TextModel::with_value("abc");
    m.backspace();
    assert_eq!(m.value(), "ab");
    assert_eq!(m.cursor(), 2);
}

#[test]
fn text_model_backspace_at_start_noop() {
    let mut m = TextModel::with_value("abc");
    m.move_home(false);
    m.backspace();
    assert_eq!(m.value(), "abc");
    assert_eq!(m.cursor(), 0);
}

#[test]
fn text_model_forward_delete() {
    let mut m = TextModel::with_value("abc");
    m.move_home(false);
    m.forward_delete();
    assert_eq!(m.value(), "bc");
    assert_eq!(m.cursor(), 0);
}

#[test]
fn text_model_arrow_movement() {
    let mut m = TextModel::with_value("abc");
    assert_eq!(m.cursor(), 3);
    m.move_left(false);
    assert_eq!(m.cursor(), 2);
    m.move_home(false);
    assert_eq!(m.cursor(), 0);
    m.move_right(false);
    assert_eq!(m.cursor(), 1);
    m.move_end(false);
    assert_eq!(m.cursor(), 3);
}

#[test]
fn text_model_shift_extends_selection() {
    let mut m = TextModel::with_value("hello");
    m.move_home(false);
    m.move_right(true);
    m.move_right(true);
    assert_eq!(m.selection_range(), Some((0, 2)));
    assert_eq!(m.selected_text().as_deref(), Some("he"));
}

#[test]
fn text_model_selection_collapses_on_bare_arrow() {
    let mut m = TextModel::with_value("hello");
    m.select_all();
    assert_eq!(m.selection_range(), Some((0, 5)));
    m.move_right(false);
    // Bare right from a selection -> cursor at end of selection, no anchor.
    assert_eq!(m.cursor(), 5);
    assert_eq!(m.anchor(), None);
}

#[test]
fn text_model_typing_replaces_selection() {
    let mut m = TextModel::with_value("hello");
    m.select_all();
    m.insert_str("hi");
    assert_eq!(m.value(), "hi");
    assert_eq!(m.cursor(), 2);
    assert_eq!(m.anchor(), None);
}

#[test]
fn text_model_insert_at_middle() {
    let mut m = TextModel::with_value("ac");
    m.move_home(false);
    m.move_right(false);
    m.insert_str("b");
    assert_eq!(m.value(), "abc");
    assert_eq!(m.cursor(), 2);
}

#[test]
fn text_model_strips_control_chars() {
    let mut m = TextModel::new();
    m.insert_str("a\nb\tc");
    assert_eq!(m.value(), "abc");
}

#[test]
fn text_model_unicode_cursor_counts_chars_not_bytes() {
    let mut m = TextModel::with_value("café");
    assert_eq!(m.char_len(), 4);
    assert_eq!(m.cursor(), 4);
    m.backspace();
    assert_eq!(m.value(), "caf");
}

// ---------- live-search debounce ----------

fn collect_fire(
    value: &str,
    submitted: bool,
    st: &mut DebounceState,
    debounce_ms: u64,
) -> Option<String> {
    let mut fired = None;
    fire_debounced(
        value,
        submitted,
        st,
        std::time::Duration::from_millis(debounce_ms),
        |q| fired = Some(q),
    );
    fired
}

#[test]
fn debounce_fires_once_after_value_stabilizes() {
    let mut st = DebounceState::default();
    // Zero debounce: fires as soon as the value is seen.
    assert_eq!(
        collect_fire("batman", false, &mut st, 0).as_deref(),
        Some("batman")
    );
    // Same value on the next tick must NOT refire.
    assert_eq!(collect_fire("batman", false, &mut st, 0), None);
    assert_eq!(collect_fire("batman", false, &mut st, 0), None);
}

#[test]
fn debounce_timer_survives_repeated_ticks() {
    // Regression: the old implementation reset the timer on every tick
    // while the value differed from the last search, so a pending search
    // never fired. Repeated ticks with an unchanged value must fire once
    // the debounce elapses.
    let mut st = DebounceState::default();
    assert_eq!(collect_fire("dune", false, &mut st, 40), None);
    assert_eq!(collect_fire("dune", false, &mut st, 40), None);
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert_eq!(
        collect_fire("dune", false, &mut st, 40).as_deref(),
        Some("dune")
    );
    assert_eq!(collect_fire("dune", false, &mut st, 40), None);
}

#[test]
fn debounce_enter_fires_immediately() {
    let mut st = DebounceState::default();
    assert_eq!(
        collect_fire("alien", true, &mut st, 10_000).as_deref(),
        Some("alien")
    );
}

#[test]
fn debounce_emptied_field_fires_reset() {
    let mut st = DebounceState::default();
    assert_eq!(
        collect_fire("abc", false, &mut st, 0).as_deref(),
        Some("abc")
    );
    // Clearing the field fires an empty query so the browse view returns.
    assert_eq!(collect_fire("", false, &mut st, 0).as_deref(), Some(""));
    // But only once.
    assert_eq!(collect_fire("", false, &mut st, 0), None);
}

#[test]
fn debounce_single_char_queries_fire() {
    let mut st = DebounceState::default();
    assert_eq!(collect_fire("a", false, &mut st, 0).as_deref(), Some("a"));
}

// ---------- navigation ----------

#[test]
fn navigate_dedupes_current_page() {
    with_temp_config(|| {
        let state = AppState::new();
        state.replace_page(Page::Search);
        state.navigate(Page::Search);
        assert!(
            !state.back(),
            "navigating to the current page must not push"
        );
        state.navigate(Page::Movie);
        state.navigate(Page::Movie);
        assert!(state.back());
        assert_eq!(state.current_page(), Page::Search);
        assert!(!state.back());
    });
}

#[test]
fn drawer_style_navigation_keeps_history() {
    with_temp_config(|| {
        let state = AppState::new();
        state.replace_page(Page::Search);
        state.navigate(Page::Downloads);
        state.navigate(Page::Settings);
        assert!(state.back());
        assert_eq!(state.current_page(), Page::Downloads);
        assert!(state.back());
        assert_eq!(state.current_page(), Page::Search);
    });
}

// ---------- magnet parsing ----------

#[test]
fn info_hash_extraction() {
    assert_eq!(
        info_hash_from_magnet(
            "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=x&tr=udp"
        )
        .as_deref(),
        Some("abcdef0123456789abcdef0123456789abcdef01")
    );
    assert_eq!(
        info_hash_from_magnet("magnet:?xt=urn:btih:ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00")
            .as_deref(),
        Some("ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00")
    );
    assert_eq!(info_hash_from_magnet("magnet:?dn=nohash"), None);
    assert_eq!(info_hash_from_magnet("magnet:?xt=urn:btih:"), None);
}

// ---------- poster retry registry ----------

#[test]
fn poster_failure_backoff_and_clear() {
    with_temp_config(|| {
        let state = AppState::new();
        state.mark_poster_failure("/proxy/0/a.jpg");
        // Just-failed loads are not retried immediately (2s backoff).
        assert!(state.due_poster_retries().is_empty());
        // Clearing removes the entry entirely.
        state.clear_poster_failure("/proxy/0/a.jpg");
        assert!(state.poster_failures.lock().is_empty());
    });
}

// ---------- responsive scaling ----------

// The global scale lives in an atomic shared by every test thread, so
// tests that change it serialize and always restore the baseline.
static SCALE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_viewport<R>(width: f32, f: impl FnOnce() -> R) -> R {
    let _guard = SCALE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_viewport_width(width);
    let r = f();
    set_viewport_width(UI_BASELINE_W);
    r
}

#[test]
fn responsive_scale_baseline_and_clamps() {
    assert!((responsive_scale(UI_BASELINE_W) - 1.0).abs() < 1e-6);
    // Small windows shrink, but never below the floor.
    assert!(responsive_scale(900.0) < 1.0);
    assert!((responsive_scale(300.0) - 0.85).abs() < 1e-6);
    // Big/4K windows grow, capped.
    assert!(responsive_scale(1600.0) > 1.2);
    assert!((responsive_scale(2560.0) - 1.5).abs() < 1e-6);
    assert!((responsive_scale(3840.0) - 1.5).abs() < 1e-6);
    // Monotonic across the useful range.
    let mut prev = 0.0;
    for w in [300.0, 800.0, 1100.0, 1440.0, 1650.0, 2000.0] {
        let s = responsive_scale(w);
        assert!(s >= prev, "scale must not shrink as the window grows");
        prev = s;
    }
}

#[test]
fn tile_layout_respects_min_and_max_at_all_widths() {
    with_viewport(UI_BASELINE_W, || {
        for avail in [200.0, 400.0, 800.0, 1060.0, 1440.0, 1920.0, 2560.0, 3840.0] {
            let l = tile_layout(avail);
            assert!(
                l.tile_w >= TILE_MIN_W - 0.01 && l.tile_w <= TILE_MAX_W + 0.01,
                "tile width {} out of bounds at avail {avail}",
                l.tile_w
            );
            assert!((l.poster_h - l.tile_w * 1.5).abs() < 0.01, "2:3 aspect");
            assert!(l.total_h > l.poster_h, "text block below the poster");
            assert!((0.85..=1.2).contains(&l.font_scale));
            assert!(l.per_row >= 1);
        }
    });
}

#[test]
fn tile_layout_adds_columns_as_width_grows() {
    with_viewport(UI_BASELINE_W, || {
        let narrow = tile_layout(700.0);
        let medium = tile_layout(1060.0);
        let wide = tile_layout(1880.0);
        let ultra = tile_layout(2500.0);
        assert!(narrow.per_row < medium.per_row);
        assert!(medium.per_row < wide.per_row);
        assert!(wide.per_row < ultra.per_row);
        // Columns actually fit in the available space.
        for (avail, l) in [
            (700.0, narrow),
            (1060.0, medium),
            (1880.0, wide),
            (2500.0, ultra),
        ] {
            let used = l.per_row as f32 * l.tile_w + (l.per_row as f32 - 1.0) * 12.0;
            assert!(
                used <= avail + 1.0,
                "{} columns of {} overflow {avail}",
                l.per_row,
                l.tile_w
            );
        }
    });
}

#[test]
fn tile_layout_fills_leftover_space_up_to_max() {
    with_viewport(UI_BASELINE_W, || {
        // Between exact column fits, tiles stretch instead of leaving a
        // ragged gap: width must exceed the minimum whenever there is
        // spare room for the chosen column count.
        let l = tile_layout(1000.0);
        let min_used = l.per_row as f32 * (TILE_MIN_W + 12.0) - 12.0;
        if 1000.0 - min_used > l.per_row as f32 {
            assert!(l.tile_w > TILE_MIN_W);
        }
    });
}

#[test]
fn global_scale_grows_ui_and_tiles_together() {
    // Large window: every theme size and the tile bounds grow with it.
    with_viewport(2560.0, || {
        let s = ui_scale();
        assert!((s - 1.5).abs() < 1e-6, "2560px viewport is capped at 1.5x");
        let theme = streamx_desktop::theme::Theme::new();
        assert!(theme.fs_2() > 1.4 * 13.0 * 0.9, "fonts scale up");
        assert!(theme.space_4() > streamx_desktop::theme::SPACE_4);
        let l = tile_layout(2500.0);
        assert!(
            l.tile_w >= TILE_MIN_W * 1.5 - 0.01,
            "tile min grows with the UI scale"
        );
    });
    // Small window: modest shrink, never below the floor.
    with_viewport(800.0, || {
        let s = ui_scale();
        assert!(s < 1.0 && s >= 0.85);
        let l = tile_layout(760.0);
        assert!(l.tile_w >= TILE_MIN_W * s - 0.01);
    });
    // Restored baseline for the rest of the suite.
    assert!((ui_scale() - 1.0).abs() < 1e-6);
}

#[test]
fn set_server_url_persists_and_rebuilds_client() {
    with_temp_config(|| {
        let state = AppState::new();
        state.set_server_url("http://example:1234".to_string());

        let state2 = AppState::new();
        assert_eq!(&*state2.server_url.read(), "http://example:1234");
        assert_eq!(state2.client.read().base_url(), "http://example:1234");
    });
}
