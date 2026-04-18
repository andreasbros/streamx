//! Global keyboard shortcuts. Translated into high-level `Shortcut` events so
//! `app.rs` can dispatch without duplicating key-parsing logic.

use gpui::KeyDownEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    /// Escape - pop page / close modal
    Back,
    /// Enter / Return - activate focused element
    Activate,
    /// `/` or Ctrl/Cmd+K - focus search input
    FocusSearch,
    /// Tab - move focus forward
    FocusNext,
    /// Shift+Tab - move focus backward
    FocusPrev,
    /// Arrow keys on grids
    Left,
    Right,
    Up,
    Down,
    /// `M` - toggle drawer menu (not wired yet)
    ToggleMenu,
    /// `F` - fullscreen (video - Phase 5)
    Fullscreen,
    /// Space - play/pause in player (Phase 5)
    PlayPause,
    /// Typing input (forwarded to focused text field)
    Char(char),
}

/// Parse a GPUI KeyDownEvent into a high-level Shortcut.
///
/// Returns None for key presses we don't handle (e.g. modifier-only, weird
/// keys). The caller should let the event fall through to child elements.
pub fn translate(ev: &KeyDownEvent) -> Option<Shortcut> {
    let key = ev.keystroke.key.as_str();
    let mods = &ev.keystroke.modifiers;

    // Ctrl+K / Cmd+K
    if (mods.control || mods.platform) && key == "k" {
        return Some(Shortcut::FocusSearch);
    }

    match key {
        "escape" => Some(Shortcut::Back),
        "enter" => Some(Shortcut::Activate),
        "tab" if mods.shift => Some(Shortcut::FocusPrev),
        "tab" => Some(Shortcut::FocusNext),
        "left" => Some(Shortcut::Left),
        "right" => Some(Shortcut::Right),
        "up" => Some(Shortcut::Up),
        "down" => Some(Shortcut::Down),
        "/" => Some(Shortcut::FocusSearch),
        "m" if !mods.control && !mods.platform && !mods.alt => Some(Shortcut::ToggleMenu),
        "f" if !mods.control && !mods.platform && !mods.alt => Some(Shortcut::Fullscreen),
        "space" => Some(Shortcut::PlayPause),
        other if other.chars().count() == 1 => {
            other.chars().next().map(Shortcut::Char)
        }
        _ => None,
    }
}
