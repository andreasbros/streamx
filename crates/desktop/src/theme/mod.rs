//! Strongly-typed theme backed by design tokens generated from
//! `design-tokens/tokens.json`. Regenerate with: `node scripts/gen-tokens.mjs`.
//!
//! The raw constants in `generated.rs` are machine-emitted. Anything
//! UI-facing should go through [`Theme`] so future extensions (light mode,
//! user overrides) have a single injection point.

mod generated;

pub use generated::*;

use gpui::{Hsla, Rgba};

/// Returns an opaque colour from a packed `0xRRGGBB`.
pub fn rgb(value: u32) -> Rgba {
    gpui::rgb(value)
}

/// Returns a translucent colour.
pub fn rgba(value: u32, alpha: f32) -> Rgba {
    gpui::rgba(((value as u64) << 8 | (((alpha.clamp(0.0, 1.0) * 255.0) as u64) & 0xff)) as u32)
}

/// Strongly-typed palette passed around by reference.
#[derive(Clone, Copy, Debug)]
pub struct Theme;

impl Theme {
    pub const fn new() -> Self {
        Self
    }

    // --- backgrounds ---
    pub fn bg_app(&self) -> Rgba { rgb(COLOR_BG_APP) }
    pub fn bg_surface(&self) -> Rgba { rgb(COLOR_BG_SURFACE) }
    pub fn bg_elevated(&self) -> Rgba { rgb(COLOR_BG_ELEVATED) }
    pub fn bg_panel(&self) -> Rgba { rgb(COLOR_BG_PANEL) }
    pub fn bg_overlay(&self) -> Rgba {
        rgba(COLOR_BG_OVERLAY_RGB, COLOR_BG_OVERLAY_ALPHA)
    }

    // --- foregrounds ---
    pub fn fg_primary(&self) -> Rgba { rgb(COLOR_FG_PRIMARY) }
    pub fn fg_secondary(&self) -> Rgba { rgb(COLOR_FG_SECONDARY) }
    pub fn fg_muted(&self) -> Rgba { rgb(COLOR_FG_MUTED) }
    pub fn fg_disabled(&self) -> Rgba { rgb(COLOR_FG_DISABLED) }
    pub fn fg_on_accent(&self) -> Rgba { rgb(COLOR_FG_ON_ACCENT) }

    // --- borders ---
    pub fn border_subtle(&self) -> Rgba {
        rgba(COLOR_BORDER_SUBTLE_RGB, COLOR_BORDER_SUBTLE_ALPHA)
    }
    pub fn border_default(&self) -> Rgba {
        rgba(COLOR_BORDER_DEFAULT_RGB, COLOR_BORDER_DEFAULT_ALPHA)
    }
    pub fn border_strong(&self) -> Rgba {
        rgba(COLOR_BORDER_STRONG_RGB, COLOR_BORDER_STRONG_ALPHA)
    }
    pub fn border_focus(&self) -> Rgba { rgb(COLOR_BORDER_FOCUS) }

    // --- accents ---
    pub fn accent(&self) -> Rgba { rgb(COLOR_ACCENT_SOLID) }
    pub fn accent_hover(&self) -> Rgba { rgb(COLOR_ACCENT_HOVER) }
    pub fn accent_text(&self) -> Rgba { rgb(COLOR_ACCENT_TEXT) }
    pub fn accent_subtle(&self) -> Rgba {
        rgba(COLOR_ACCENT_SUBTLE_RGB, COLOR_ACCENT_SUBTLE_ALPHA)
    }

    // --- status ---
    pub fn success(&self) -> Rgba { rgb(COLOR_STATUS_SUCCESS) }
    pub fn warning(&self) -> Rgba { rgb(COLOR_STATUS_WARNING) }
    pub fn error(&self) -> Rgba { rgb(COLOR_STATUS_ERROR) }
    pub fn critical(&self) -> Rgba { rgb(COLOR_STATUS_CRITICAL) }

    // --- media semantic ---
    pub fn favourite(&self) -> Rgba { rgb(COLOR_MEDIA_FAVOURITE) }
    pub fn playing(&self) -> Rgba { rgb(COLOR_MEDIA_PLAYING) }
    pub fn trailer(&self) -> Rgba { rgb(COLOR_MEDIA_TRAILER) }

    // --- spacing (px) ---
    pub fn space_1(&self) -> f32 { SPACE_1 }
    pub fn space_2(&self) -> f32 { SPACE_2 }
    pub fn space_3(&self) -> f32 { SPACE_3 }
    pub fn space_4(&self) -> f32 { SPACE_4 }
    pub fn space_5(&self) -> f32 { SPACE_5 }
    pub fn space_6(&self) -> f32 { SPACE_6 }

    // --- radius ---
    pub fn radius_sm(&self) -> f32 { RADIUS_SM }
    pub fn radius_md(&self) -> f32 { RADIUS_MD }
    pub fn radius_lg(&self) -> f32 { RADIUS_LG }
    pub fn radius_xl(&self) -> f32 { RADIUS_XL }

    // --- font size ---
    pub fn fs_1(&self) -> f32 { FONT_SIZE_1 }
    pub fn fs_2(&self) -> f32 { FONT_SIZE_2 }
    pub fn fs_3(&self) -> f32 { FONT_SIZE_3 }
    pub fn fs_5(&self) -> f32 { FONT_SIZE_5 }
    pub fn fs_6(&self) -> f32 { FONT_SIZE_6 }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
}

// Suppress "unused" warnings for constants we don't wrap yet (motion, shadow, zindex).
#[allow(dead_code)]
fn _unused_refs() {
    let _ = (
        MOTION_DURATION_INSTANT, MOTION_DURATION_FAST, MOTION_DURATION_MEDIUM, MOTION_DURATION_SLOW,
        MOTION_EASING_DEFAULT, MOTION_EASING_LINEAR, MOTION_EASING_EASE_IN, MOTION_EASING_EASE_OUT,
        SHADOW_SM, SHADOW_MD, SHADOW_LG, SHADOW_FOCUS,
        FONT_FAMILY_SANS, FONT_FAMILY_MONO,
        FONT_WEIGHT_REGULAR, FONT_WEIGHT_MEDIUM, FONT_WEIGHT_SEMIBOLD, FONT_WEIGHT_BOLD,
        FONT_SIZE_4, FONT_SIZE_7, FONT_SIZE_8, FONT_SIZE_9,
        SPACE_7, SPACE_8, SPACE_9,
        RADIUS_FULL,
        ZINDEX_BASE, ZINDEX_STICKY, ZINDEX_AUDIO_PLAYER, ZINDEX_OVERLAY, ZINDEX_MODAL, ZINDEX_TOAST,
        COLOR_STATUS_INFO, COLOR_MEDIA_PLAYING,
    );
    let _: Hsla = Hsla::default(); // keep gpui::Hsla import warm even if we don't expose it today
}
