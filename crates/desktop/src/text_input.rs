//! Single-line text input with cursor, selection, and clipboard.
//!
//! The cursor/selection state lives in [`TextModel`] (pure, unit-testable).
//! [`TextInput`] wraps it into a GPUI entity and handles focus/rendering.

use crate::theme::Theme;
use gpui::{
    div, px, App, AppContext, ClipboardItem, Context, CursorStyle, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render,
    SharedString, Styled, Window,
};

/// Pure text-editing model. Cursor is a char index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextModel {
    value: String,
    cursor: usize,
    anchor: Option<usize>,
}

impl TextModel {
    pub fn new() -> Self {
        Self { value: String::new(), cursor: 0, anchor: None }
    }

    pub fn with_value(v: impl Into<String>) -> Self {
        let value: String = v.into();
        let cursor = value.chars().count();
        Self { value, cursor, anchor: None }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    pub fn set_value(&mut self, v: impl Into<String>) {
        self.value = v.into();
        self.cursor = self.value.chars().count();
        self.anchor = None;
    }

    pub fn char_len(&self) -> usize {
        self.value.chars().count()
    }

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        let (lo, hi) = if a < self.cursor { (a, self.cursor) } else { (self.cursor, a) };
        if lo == hi { None } else { Some((lo, hi)) }
    }

    pub fn selected_text(&self) -> Option<String> {
        let (lo, hi) = self.selection_range()?;
        Some(self.value.chars().skip(lo).take(hi - lo).collect())
    }

    /// Returns true if a selection was present and removed.
    pub fn delete_selection(&mut self) -> bool {
        if let Some((lo, hi)) = self.selection_range() {
            let kept: String = self
                .value
                .chars()
                .enumerate()
                .filter_map(|(i, c)| if i < lo || i >= hi { Some(c) } else { None })
                .collect();
            self.value = kept;
            self.cursor = lo;
            self.anchor = None;
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub fn insert_str(&mut self, s: &str) {
        self.delete_selection();
        let inserted: String = s.chars().filter(|c| !c.is_control()).collect();
        if inserted.is_empty() {
            return;
        }
        let mut out = String::with_capacity(self.value.len() + inserted.len());
        let mut placed = false;
        for (i, c) in self.value.chars().enumerate() {
            if i == self.cursor && !placed {
                out.push_str(&inserted);
                placed = true;
            }
            out.push(c);
        }
        if !placed {
            out.push_str(&inserted);
        }
        self.value = out;
        self.cursor += inserted.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor > 0 {
            let pos = self.cursor - 1;
            let kept: String = self
                .value
                .chars()
                .enumerate()
                .filter_map(|(i, c)| if i == pos { None } else { Some(c) })
                .collect();
            self.value = kept;
            self.cursor = pos;
        }
    }

    pub fn forward_delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor < self.char_len() {
            let pos = self.cursor;
            let kept: String = self
                .value
                .chars()
                .enumerate()
                .filter_map(|(i, c)| if i == pos { None } else { Some(c) })
                .collect();
            self.value = kept;
        }
    }

    pub fn move_cursor(&mut self, new_pos: usize, keep_selection: bool) {
        if keep_selection {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = new_pos.min(self.char_len());
    }

    pub fn move_left(&mut self, shift: bool) {
        let target = if shift || self.anchor.is_none() {
            self.cursor.saturating_sub(1)
        } else {
            self.selection_range().map(|(lo, _)| lo).unwrap_or(self.cursor)
        };
        self.move_cursor(target, shift);
    }

    pub fn move_right(&mut self, shift: bool) {
        let target = if shift || self.anchor.is_none() {
            (self.cursor + 1).min(self.char_len())
        } else {
            self.selection_range().map(|(_, hi)| hi).unwrap_or(self.cursor)
        };
        self.move_cursor(target, shift);
    }

    pub fn move_home(&mut self, shift: bool) {
        self.move_cursor(0, shift);
    }

    pub fn move_end(&mut self, shift: bool) {
        self.move_cursor(self.char_len(), shift);
    }

    pub fn select_all(&mut self) {
        let len = self.char_len();
        if len > 0 {
            self.anchor = Some(0);
            self.cursor = len;
        }
    }
}

impl Default for TextModel {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TextInput {
    model: TextModel,
    placeholder: SharedString,
    is_password: bool,
    focus_handle: FocusHandle,
    pub submitted: bool,
    /// True once the user has clicked inside the field. Hides the
    /// placeholder so the cursor lives on an empty canvas. Reset when
    /// the field loses focus AND is empty.
    clicked_into: bool,
    /// Tracks focus so we can reset `clicked_into` on blur.
    was_focused: bool,
}

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            model: TextModel::new(),
            placeholder: SharedString::default(),
            is_password: false,
            focus_handle: cx.focus_handle(),
            submitted: false,
            clicked_into: false,
            was_focused: false,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn password(mut self) -> Self {
        self.is_password = true;
        self
    }

    pub fn initial(mut self, value: impl Into<String>) -> Self {
        self.model = TextModel::with_value(value);
        self
    }

    pub fn value(&self) -> &str {
        self.model.value()
    }

    pub fn set_value(&mut self, v: impl Into<String>) {
        self.model.set_value(v);
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus_handle.focus(window, cx);
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        let mods = &ev.keystroke.modifiers;
        let cmd = mods.control || mods.platform;
        let shift = mods.shift;

        if cmd && !mods.alt {
            match key {
                "a" => {
                    self.model.select_all();
                    cx.notify();
                }
                "c" => {
                    if let Some(sel) = self.model.selected_text() {
                        if !self.is_password {
                            cx.write_to_clipboard(ClipboardItem::new_string(sel));
                        }
                    }
                }
                "x" => {
                    if let Some(sel) = self.model.selected_text() {
                        if !self.is_password {
                            cx.write_to_clipboard(ClipboardItem::new_string(sel));
                        }
                        self.model.delete_selection();
                        cx.notify();
                    }
                }
                "v" => {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(text) = item.text() {
                            self.model.insert_str(&text);
                            cx.notify();
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        match key {
            "backspace" => { self.model.backspace(); cx.notify(); }
            "delete" => { self.model.forward_delete(); cx.notify(); }
            "left" => { self.model.move_left(shift); cx.notify(); }
            "right" => { self.model.move_right(shift); cx.notify(); }
            "home" => { self.model.move_home(shift); cx.notify(); }
            "end" => { self.model.move_end(shift); cx.notify(); }
            "enter" => { self.submitted = true; cx.notify(); }
            "escape" => { /* parent handles */ }
            "space" => { self.model.insert_str(" "); cx.notify(); }
            _ => {
                if let Some(ime) = ev.keystroke.key_char.as_ref() {
                    self.model.insert_str(ime);
                    cx.notify();
                } else if key.chars().count() == 1 {
                    let ch = key.chars().next().unwrap_or('\0');
                    if !ch.is_control() {
                        let s: String = if shift {
                            ch.to_uppercase().collect()
                        } else {
                            ch.to_string()
                        };
                        self.model.insert_str(&s);
                        cx.notify();
                    }
                }
            }
        }
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::new();
        let focused = self.focus_handle.is_focused(window);
        // Reset `clicked_into` when the field loses focus AND is empty so
        // the placeholder reappears next time it's unfocused + empty.
        if !focused && self.was_focused && self.model.value().is_empty() {
            self.clicked_into = false;
        }
        self.was_focused = focused;

        let display: String = if self.is_password {
            "•".repeat(self.model.char_len())
        } else {
            self.model.value().to_string()
        };

        let chars: Vec<char> = display.chars().collect();
        let cursor_pos = self.model.cursor().min(chars.len());
        let (lo, hi) = self.model.selection_range().unwrap_or((cursor_pos, cursor_pos));

        let before: String = chars[..lo].iter().collect();
        let selected: String = chars[lo..hi].iter().collect();
        let after: String = chars[hi..].iter().collect();

        let is_empty = self.model.value().is_empty();
        // Show placeholder only when nothing's typed AND user hasn't
        // clicked in yet. Keyboard/Tab focus still shows placeholder —
        // just highlights the border.
        let show_placeholder = is_empty && !self.clicked_into;
        let text_color = if is_empty { theme.fg_muted() } else { theme.fg_primary() };

        let mut text_row = div()
            .flex()
            .items_center()
            .h(px(22.0))
            .text_size(px(theme.fs_2()))
            .text_color(text_color);

        if show_placeholder {
            text_row = text_row.child(self.placeholder.clone());
            if focused {
                text_row = text_row.child(cursor_bar(&theme));
            }
        } else if is_empty {
            if focused {
                text_row = text_row.child(cursor_bar(&theme));
            }
        } else {
            text_row = text_row.child(SharedString::from(before));
            if !selected.is_empty() {
                text_row = text_row.child(
                    div()
                        .bg(theme.accent())
                        .text_color(theme.fg_on_accent())
                        .child(SharedString::from(selected)),
                );
            } else if focused {
                text_row = text_row.child(cursor_bar(&theme));
            }
            text_row = text_row.child(SharedString::from(after));
        }

        div()
            .id("text-input-root")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| {
                this.on_key_down(ev, cx);
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _ev, window, cx| {
                this.clicked_into = true;
                this.focus_handle.focus(window, cx);
                cx.notify();
            }))
            .px(px(theme.space_3()))
            .py(px(theme.space_2()))
            .rounded(px(theme.radius_md()))
            .bg(theme.bg_elevated())
            .border_1()
            .border_color(if focused {
                theme.border_focus()
            } else {
                theme.border_default()
            })
            .cursor(CursorStyle::IBeam)
            .child(text_row)
    }
}

fn cursor_bar(theme: &Theme) -> gpui::Div {
    div().w(px(2.0)).h(px(18.0)).bg(theme.accent()).mx(px(1.0))
}

pub fn text_input(cx: &mut App, placeholder: impl Into<SharedString>) -> Entity<TextInput> {
    cx.new(|cx| TextInput::new(cx).with_placeholder(placeholder))
}

pub fn password_input(cx: &mut App, placeholder: impl Into<SharedString>) -> Entity<TextInput> {
    cx.new(|cx| TextInput::new(cx).with_placeholder(placeholder).password())
}
