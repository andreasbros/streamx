//! Strongly-typed theme backed by design tokens generated from
//! `design-tokens/tokens.json`. Regenerate with: `node scripts/gen-tokens.mjs`.
//!
//! The raw constants in `generated.rs` are machine-emitted. Anything
//! UI-facing should go through [`Theme`] so future extensions (light mode,
//! user overrides) have a single injection point.

mod generated;

pub use generated::*;

use gpui::{Hsla, Rgba};
use once_cell::sync::Lazy;

/// Manual UI scale multiplier from STREAMX_UI_SCALE (e.g. "1.25" for
/// preference; combined with the responsive viewport scale below).
static UI_SCALE_OVERRIDE: Lazy<f32> = Lazy::new(|| {
    std::env::var("STREAMX_UI_SCALE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .map(|v| v.clamp(0.5, 3.0))
        .unwrap_or(1.0)
});

/// Effective global UI scale (responsive × override), stored as f32 bits
/// so the render pass can update it lock-free each frame.
static UI_SCALE_BITS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(f32::to_bits(1.0));

/// Design baseline width: at this window width the UI renders 1:1.
pub const UI_BASELINE_W: f32 = 1100.0;

pub fn ui_scale() -> f32 {
    f32::from_bits(UI_SCALE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Derive the responsive scale for a viewport width. Pure, so it can be
/// unit-tested: sub-baseline windows shrink slightly (never below 0.85),
/// large/4K windows grow up to 1.5×.
pub fn responsive_scale(viewport_w: f32) -> f32 {
    (viewport_w / UI_BASELINE_W).clamp(0.85, 1.5)
}

/// Called once per frame by the root render with the current viewport
/// width. Every Theme size accessor picks the new scale up immediately,
/// so fonts, spacing, menus, inputs and buttons all scale together.
pub fn set_viewport_width(viewport_w: f32) {
    let s = (responsive_scale(viewport_w) * *UI_SCALE_OVERRIDE).clamp(0.5, 3.0);
    UI_SCALE_BITS.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

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
    pub fn bg_app(&self) -> Rgba {
        rgb(COLOR_BG_APP)
    }
    pub fn bg_surface(&self) -> Rgba {
        rgb(COLOR_BG_SURFACE)
    }
    pub fn bg_elevated(&self) -> Rgba {
        rgb(COLOR_BG_ELEVATED)
    }
    pub fn bg_panel(&self) -> Rgba {
        rgb(COLOR_BG_PANEL)
    }
    pub fn bg_overlay(&self) -> Rgba {
        rgba(COLOR_BG_OVERLAY_RGB, COLOR_BG_OVERLAY_ALPHA)
    }

    // --- foregrounds ---
    pub fn fg_primary(&self) -> Rgba {
        rgb(COLOR_FG_PRIMARY)
    }
    pub fn fg_secondary(&self) -> Rgba {
        rgb(COLOR_FG_SECONDARY)
    }
    pub fn fg_muted(&self) -> Rgba {
        rgb(COLOR_FG_MUTED)
    }
    pub fn fg_disabled(&self) -> Rgba {
        rgb(COLOR_FG_DISABLED)
    }
    pub fn fg_on_accent(&self) -> Rgba {
        rgb(COLOR_FG_ON_ACCENT)
    }

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
    pub fn border_focus(&self) -> Rgba {
        rgb(COLOR_BORDER_FOCUS)
    }

    // --- accents ---
    pub fn accent(&self) -> Rgba {
        rgb(COLOR_ACCENT_SOLID)
    }
    pub fn accent_hover(&self) -> Rgba {
        rgb(COLOR_ACCENT_HOVER)
    }
    pub fn accent_text(&self) -> Rgba {
        rgb(COLOR_ACCENT_TEXT)
    }
    pub fn accent_subtle(&self) -> Rgba {
        rgba(COLOR_ACCENT_SUBTLE_RGB, COLOR_ACCENT_SUBTLE_ALPHA)
    }

    // --- status ---
    pub fn success(&self) -> Rgba {
        rgb(COLOR_STATUS_SUCCESS)
    }
    pub fn warning(&self) -> Rgba {
        rgb(COLOR_STATUS_WARNING)
    }
    pub fn error(&self) -> Rgba {
        rgb(COLOR_STATUS_ERROR)
    }
    pub fn critical(&self) -> Rgba {
        rgb(COLOR_STATUS_CRITICAL)
    }

    // --- media semantic ---
    pub fn favourite(&self) -> Rgba {
        rgb(COLOR_MEDIA_FAVOURITE)
    }
    pub fn playing(&self) -> Rgba {
        rgb(COLOR_MEDIA_PLAYING)
    }
    pub fn trailer(&self) -> Rgba {
        rgb(COLOR_MEDIA_TRAILER)
    }

    // --- spacing (px) — scaled by UI_SCALE ---
    pub fn space_1(&self) -> f32 {
        SPACE_1 * ui_scale()
    }
    pub fn space_2(&self) -> f32 {
        SPACE_2 * ui_scale()
    }
    pub fn space_3(&self) -> f32 {
        SPACE_3 * ui_scale()
    }
    pub fn space_4(&self) -> f32 {
        SPACE_4 * ui_scale()
    }
    pub fn space_5(&self) -> f32 {
        SPACE_5 * ui_scale()
    }
    pub fn space_6(&self) -> f32 {
        SPACE_6 * ui_scale()
    }

    // --- radius (unchanged by scale — visual identity) ---
    pub fn radius_sm(&self) -> f32 {
        RADIUS_SM
    }
    pub fn radius_md(&self) -> f32 {
        RADIUS_MD
    }
    pub fn radius_lg(&self) -> f32 {
        RADIUS_LG
    }
    pub fn radius_xl(&self) -> f32 {
        RADIUS_XL
    }

    // --- font size — scaled by UI_SCALE ---
    pub fn fs_1(&self) -> f32 {
        FONT_SIZE_1 * ui_scale()
    }
    pub fn fs_2(&self) -> f32 {
        FONT_SIZE_2 * ui_scale()
    }
    pub fn fs_3(&self) -> f32 {
        FONT_SIZE_3 * ui_scale()
    }
    pub fn fs_5(&self) -> f32 {
        FONT_SIZE_5 * ui_scale()
    }
    pub fn fs_6(&self) -> f32 {
        FONT_SIZE_6 * ui_scale()
    }

    /// Current global UI scale, for the few fixed pixel sizes that sit
    /// outside the fs_*/space_* token system (titlebar height, drawer
    /// width, window controls).
    pub fn scale(&self) -> f32 {
        ui_scale()
    }

    /// Frosted-glass background for text overlays on top of backdrop
    /// images. GPUI lacks real backdrop-blur, so this is a dark
    /// translucent rectangle that keeps text legible over any image.
    pub fn frost(&self) -> Rgba {
        rgba(0x000000, 0.55)
    }

    pub fn frost_subtle(&self) -> Rgba {
        rgba(0x000000, 0.35)
    }
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
        MOTION_DURATION_INSTANT,
        MOTION_DURATION_FAST,
        MOTION_DURATION_MEDIUM,
        MOTION_DURATION_SLOW,
        MOTION_EASING_DEFAULT,
        MOTION_EASING_LINEAR,
        MOTION_EASING_EASE_IN,
        MOTION_EASING_EASE_OUT,
        SHADOW_SM,
        SHADOW_MD,
        SHADOW_LG,
        SHADOW_FOCUS,
        FONT_FAMILY_SANS,
        FONT_FAMILY_MONO,
        FONT_WEIGHT_REGULAR,
        FONT_WEIGHT_MEDIUM,
        FONT_WEIGHT_SEMIBOLD,
        FONT_WEIGHT_BOLD,
        FONT_SIZE_4,
        FONT_SIZE_7,
        FONT_SIZE_8,
        FONT_SIZE_9,
        SPACE_7,
        SPACE_8,
        SPACE_9,
        RADIUS_FULL,
        ZINDEX_BASE,
        ZINDEX_STICKY,
        ZINDEX_AUDIO_PLAYER,
        ZINDEX_OVERLAY,
        ZINDEX_MODAL,
        ZINDEX_TOAST,
        COLOR_STATUS_INFO,
        COLOR_MEDIA_PLAYING,
    );
    let _: Hsla = Hsla::default(); // keep gpui::Hsla import warm even if we don't expose it today
}
