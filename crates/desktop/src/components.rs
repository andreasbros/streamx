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
use streamx_api::types::SearchResultGroup;

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
) -> gpui::Stateful<gpui::Div> {
    let title: SharedString = group.title.clone().into();
    let id = id.into();
    let year = group.year.map(|y| y.to_string()).unwrap_or_default();
    let rating = group.rating.map(|r| format!("{:.1}", r)).unwrap_or_default();

    // Pick the best available poster URL. GPUI's `img()` accepts URLs
    // and streams them over HTTP with its own cache.
    let poster_url = group
        .poster_medium
        .as_ref()
        .or(group.poster_large.as_ref())
        .or(group.poster_small.as_ref())
        .or(group.poster.as_ref())
        .cloned();

    // Tile sizing: classic 2:3 movie poster ratio, sized for readability
    // on desktop viewports. The container below caps text height so the
    // overall card stays a predictable aspect.
    let poster_w = TILE_POSTER_W;
    let poster_h = TILE_POSTER_H;
    let poster_placeholder = match poster_url {
        Some(url) => {
            let fallback_bg = theme.bg_panel();
            // GPUI's default `From<&str> for ImageSource` treats any
            // hyper-parseable string as a URI (including relative paths
            // like "/proxy/..."), which routes it to the HTTP loader
            // and fails. For /proxy/ we construct the Embedded variant
            // by hand so our AssetSource is consulted instead.
            let source: ImageSource = if url.starts_with("/proxy/") {
                ImageSource::Resource(Resource::Embedded(SharedString::from(url)))
            } else {
                ImageSource::from(SharedString::from(url))
            };
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
        .text_size(px(theme.fs_1()))
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
        .w(px(TILE_TOTAL_W))
        .h(px(TILE_TOTAL_H))
        .flex()
        .flex_col()
        .gap(px(theme.space_1()))
        .flex_shrink_0()
        .cursor_pointer()
        .child(poster_placeholder)
        .child(
            div()
                .max_h(px(48.0))
                .overflow_hidden()
                .text_size(px(theme.fs_1()))
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
    groups: &[SearchResultGroup],
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
            row = row.child(movie_tile(g, theme, format!("bs-{title}-{i}")));
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
pub fn badge(
    label: impl Into<SharedString>,
    color: gpui::Rgba,
    theme: &Theme,
) -> gpui::Div {
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
