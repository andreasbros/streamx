//! Reusable widgets rendered directly (no child Entities). Matches the
//! nocapsec pattern where each function returns `impl IntoElement`.

use crate::theme::Theme;
use gpui::{
    div, img, px, FontWeight, ImageSource, InteractiveElement, ObjectFit, ParentElement, Resource,
    SharedString, Styled, StyledImage,
};

/// Movie poster tile dimensions. Sized for readability on desktop
/// viewports (~1080p+). Keeps the classic 2:3 movie-poster aspect ratio.
pub const TILE_POSTER_W: f32 = 180.0;
pub const TILE_POSTER_H: f32 = 270.0;
pub const TILE_TOTAL_W: f32 = 180.0;
pub const TILE_TOTAL_H: f32 = 360.0;

/// Responsive tile bounds: tiles never shrink below MIN or stretch past
/// MAX; column count adapts instead, like CSS
/// `repeat(auto-fit, minmax(MIN, 1fr))`.
pub const TILE_MIN_W: f32 = 132.0;
pub const TILE_MAX_W: f32 = 224.0;
pub const TILE_GAP: f32 = 12.0;

/// Resolved tile sizing for a given available width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileLayout {
    pub tile_w: f32,
    pub poster_h: f32,
    pub total_h: f32,
    /// Multiplier applied to tile text sizes so type scales with the
    /// tile instead of drowning in it or overflowing it.
    pub font_scale: f32,
    /// Columns that fit fully at this width.
    pub per_row: usize,
}

/// Fit as many columns as possible at >= the (UI-scaled) minimum width,
/// stretch them evenly up to the maximum, and scale tile text with the
/// tile. Extra space beyond max-width columns stays as margin;
/// fewer-than-min columns never happen (tiles overflow into scrolling
/// instead). The global UI scale grows the bounds themselves so tiles
/// keep pace with the rest of the interface on large displays.
pub fn tile_layout(available_width: f32) -> TileLayout {
    let s = crate::theme::ui_scale();
    let min = TILE_MIN_W * s;
    let max = TILE_MAX_W * s;
    let gap = TILE_GAP * s;
    let avail = available_width.max(min);
    let cols = ((avail + gap) / (min + gap)).floor().max(1.0);
    let tile_w = ((avail - (cols - 1.0) * gap) / cols).clamp(min, max);
    let font_scale = (tile_w / (TILE_POSTER_W * s)).clamp(0.85, 1.2);
    let poster_h = tile_w * 1.5;
    TileLayout {
        tile_w,
        poster_h,
        total_h: poster_h + 64.0 * font_scale * s,
        font_scale,
        per_row: cols as usize,
    }
}
use streamx_api::types::SearchResultGroup;

/// Build a GPUI image source for a poster URL. Server-relative paths
/// ("/proxy/...", "/api/posters/...") go through our AssetSource via the
/// Embedded resource variant; GPUI's default `From<&str>` would route
/// them to the HTTP loader with a relative URI, which fails. Absolute
/// URLs use GPUI's own HTTP loader.
pub fn poster_image_source(url: &str) -> ImageSource {
    if url.starts_with('/') {
        ImageSource::Resource(Resource::Embedded(SharedString::from(url.to_string())))
    } else {
        ImageSource::from(SharedString::from(url.to_string()))
    }
}

/// Flat card container. Call `.child(...)` on the returned div.
pub fn card(theme: &Theme) -> gpui::Div {
    div()
        .p(px(theme.space_4()))
        .rounded(px(theme.radius_lg()))
        .bg(theme.bg_surface())
        .border_1()
        .border_color(theme.border_subtle())
}

/// Frosted-glass card: semi-transparent dark background with a subtle
/// border. Use on top of backdrop images so text stays legible.
/// GPUI has no real backdrop-blur; the dark tint is the best we can do.
pub fn frost_card(theme: &Theme) -> gpui::Div {
    div()
        .p(px(theme.space_4()))
        .rounded(px(theme.radius_lg()))
        .bg(theme.frost())
        .border_1()
        .border_color(theme.border_default())
}

/// Primary button. Click handling is up to the caller via `.on_click(...)`.
pub fn primary_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px(px(theme.space_4()))
        .py(px(theme.space_2()))
        .rounded(px(theme.radius_md()))
        .bg(theme.accent())
        .text_color(theme.fg_on_accent())
        .text_size(px(theme.fs_2()))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| s.bg(theme.accent_hover()))
        .child(div().child(label.into()))
}

pub fn secondary_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px(px(theme.space_4()))
        .py(px(theme.space_2()))
        .rounded(px(theme.radius_md()))
        .bg(theme.bg_elevated())
        .text_color(theme.fg_primary())
        .text_size(px(theme.fs_2()))
        .font_weight(FontWeight::MEDIUM)
        .border_1()
        .border_color(theme.border_default())
        .cursor_pointer()
        .hover(|s| s.bg(theme.bg_panel()).border_color(theme.border_strong()))
        .child(div().child(label.into()))
}

/// Section header with title and optional subtitle row.
pub fn section_title(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .text_size(px(theme.fs_5()))
        .font_weight(FontWeight::BOLD)
        .text_color(theme.fg_primary())
        .child(text.into())
}

/// Movie tile (120x240). Renders a placeholder poster box + title + year/rating.
/// The caller provides a globally unique id so identical (section, index)
/// pairs across rows don't collide in GPUI's interaction routing.
pub fn movie_tile(
    group: &SearchResultGroup,
    theme: &Theme,
    id: impl Into<SharedString>,
    layout: TileLayout,
) -> gpui::Stateful<gpui::Div> {
    let title: SharedString = group.title.clone().into();
    let id = id.into();
    let year = group.year.map(|y| y.to_string()).unwrap_or_default();
    let rating = group
        .rating
        .map(|r| format!("{:.1}", r))
        .unwrap_or_default();
    let fs = layout.font_scale;

    // Pick the best available poster URL. GPUI's `img()` accepts URLs
    // and streams them over HTTP with its own cache.
    let poster_url = group
        .poster_medium
        .as_ref()
        .or(group.poster_large.as_ref())
        .or(group.poster_small.as_ref())
        .or(group.poster.as_ref())
        .cloned();

    // Responsive 2:3 poster sized by the caller's TileLayout; text below
    // scales with the tile.
    let poster_w = layout.tile_w;
    let poster_h = layout.poster_h;
    let poster_placeholder = match poster_url {
        Some(url) => {
            let fallback_bg = theme.bg_panel();
            let source: ImageSource = poster_image_source(&url);
            div()
                .w(px(poster_w))
                .h(px(poster_h))
                .rounded(px(theme.radius_md()))
                .overflow_hidden()
                .bg(fallback_bg)
                .border_1()
                .border_color(theme.border_subtle())
                .child(
                    img(source)
                        .w(px(poster_w))
                        .h(px(poster_h))
                        .object_fit(ObjectFit::Cover),
                )
        }
        None => div()
            .w(px(poster_w))
            .h(px(poster_h))
            .rounded(px(theme.radius_md()))
            .bg(theme.bg_panel())
            .border_1()
            .border_color(theme.border_subtle())
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(theme.fs_6()))
            .text_color(theme.fg_muted())
            .child(div().child("▶")),
    };

    let meta_row = div()
        .flex()
        .gap(px(theme.space_2()))
        .items_center()
        .text_size(px(theme.fs_1() * fs))
        .child(
            div()
                .text_color(theme.fg_muted())
                .child(SharedString::from(year)),
        )
        .child(
            div()
                .text_color(theme.favourite())
                .child(SharedString::from(if rating.is_empty() {
                    "".to_string()
                } else {
                    format!("★ {}", rating)
                })),
        );

    div()
        .id(id)
        .w(px(layout.tile_w))
        .h(px(layout.total_h))
        .flex()
        .flex_col()
        .gap(px(theme.space_1()))
        .flex_shrink_0()
        .cursor_pointer()
        .child(poster_placeholder)
        .child(
            div()
                .max_h(px(40.0 * fs))
                .overflow_hidden()
                .text_size(px(theme.fs_1() * fs))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.fg_primary())
                .child(title),
        )
        .child(meta_row)
}

/// A horizontal row: title on top, then a clipping tile row. GPUI lacks a
/// plain "overflow-x: scroll" on Div - scrolling rows land in Phase 4b via
/// `uniform_list`. For now we clip on overflow so layout stays stable.
pub fn browse_section(
    title: impl Into<SharedString>,
    theme: &Theme,
    groups: &[std::sync::Arc<SearchResultGroup>],
) -> gpui::Div {
    let title: SharedString = title.into();

    let mut row = div()
        .flex()
        .gap(px(theme.space_3()))
        .overflow_hidden()
        .pb(px(theme.space_2()))
        .min_h(px(260.0));

    if groups.is_empty() {
        for i in 0..8u32 {
            row = row.child(
                div()
                    .id(SharedString::from(format!("skel-{}-{}", title, i)))
                    .w(px(120.0))
                    .h(px(180.0))
                    .rounded(px(theme.radius_md()))
                    .bg(theme.bg_panel())
                    .flex_shrink_0(),
            );
        }
    } else {
        for (i, g) in groups.iter().enumerate() {
            row = row.child(movie_tile(
                g.as_ref(),
                theme,
                format!("bs-{title}-{i}"),
                tile_layout(1060.0),
            ));
        }
    }

    div()
        .flex()
        .flex_col()
        .gap(px(theme.space_2()))
        .mb(px(theme.space_4()))
        .child(section_title(title.clone(), theme))
        .child(row)
}

/// Badge (small pill for quality/codec labels).
pub fn badge(label: impl Into<SharedString>, color: gpui::Rgba, theme: &Theme) -> gpui::Div {
    div()
        .px(px(theme.space_2()))
        .py(px(2.0))
        .rounded(px(theme.radius_sm()))
        .text_size(px(theme.fs_1()))
        .text_color(color)
        .border_1()
        .border_color(color)
        .child(label.into())
}
