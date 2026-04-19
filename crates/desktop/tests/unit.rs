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
    keybindings::{translate, Shortcut},
    playback::{candidate_paths, longest_common_dir, PlayTarget},
    router::Page,
    state::{AppState, Mode},
    text_input::TextModel,
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
    unsafe { std::env::set_var("STREAMX_DESKTOP_CONFIG_OVERRIDE", dir.path()); }
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
