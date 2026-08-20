//! Page renderers. Each is a free function returning `impl IntoElement`.
//! State lives on `Arc<AppState>`, so pages take `&AppState` and read what
//! they need to build the view tree.

use crate::components::*;
use crate::state::{AppState, BrowseData};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;
use gpui::{div, px, FontWeight, IntoElement, ParentElement, SharedString, Styled};
use streamx_api::types::SearchResultGroup;

pub fn login_page(state: &AppState, theme: &Theme) -> impl IntoElement {
    let err = state.connection_error.read().clone();
    let server = state.server_url.read().clone();
    let instructions = "Set STREAMX_USERNAME and STREAMX_PASSWORD, then relaunch. Real in-app inputs land in Phase 4b.";

    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.bg_app())
        .child(
            card(theme)
                .w(px(440.0))
                .flex()
                .flex_col()
                .gap(px(theme.space_4()))
                .child(
                    div()
                        .text_size(px(theme.fs_6()))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.fg_primary())
                        .child("StreamX Desktop"),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_2()))
                        .text_color(theme.fg_secondary())
                        .child(SharedString::from(format!("Server: {}", server))),
                )
                .child(
                    div()
                        .text_size(px(theme.fs_2()))
                        .text_color(theme.fg_muted())
                        .child(SharedString::from(instructions)),
                )
                .when_some(err.clone(), |el, e: String| {
                    el.child(
                        div()
                            .p(px(theme.space_3()))
                            .rounded(px(theme.radius_md()))
                            .bg(theme.bg_elevated())
                            .border_1()
                            .border_color(theme.error())
                            .text_color(theme.error())
                            .text_size(px(theme.fs_1()))
                            .child(SharedString::from(e)),
                    )
                }),
        )
}

pub fn search_page(state: &AppState, theme: &Theme) -> impl IntoElement {
    let browse = state.browse.read().clone();
    let query = state.query.read().clone();
    let loading = *state.browse_loading.read();

    // Header: title + hint
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .mb(px(theme.space_4()))
        .child(
            div()
                .text_size(px(theme.fs_6()))
                .font_weight(FontWeight::BOLD)
                .text_color(theme.fg_primary())
                .child(SharedString::from(if query.is_empty() {
                    "Search & Browse".to_string()
                } else {
                    format!("Results for: {}", query)
                })),
        )
        .child(
            div()
                .text_size(px(theme.fs_1()))
                .text_color(theme.fg_muted())
                .child(SharedString::from(if loading {
                    "loading…"
                } else {
                    "press / to search (Phase 4b) · Esc back · M menu"
                })),
        );

    // Browse sections (8 rows)
    let sections = browse_sections_view(&browse, theme);

    div()
        .size_full()
        .overflow_hidden()
        .p(px(theme.space_5()))
        .bg(theme.bg_app())
        .flex()
        .flex_col()
        .child(header)
        .child(sections)
}

fn browse_sections_view(browse: &BrowseData, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(browse_section("Latest", theme, &browse.latest))
        .child(browse_section("Most Popular", theme, &browse.popular))
        .child(browse_section("Top Rated", theme, &browse.top_rated))
        .child(browse_section("Action", theme, &browse.action))
        .child(browse_section("Comedy", theme, &browse.comedy))
        .child(browse_section("Thriller", theme, &browse.thriller))
        .child(browse_section("Sci-Fi", theme, &browse.scifi))
        .child(browse_section("Horror", theme, &browse.horror))
}

pub fn movie_page(state: &AppState, theme: &Theme) -> impl IntoElement {
    let movie = state.selected_movie.read().clone();

    let Some(m) = movie else {
        return fallback(theme, "No movie selected").into_any_element();
    };

    // Header: back hint + title + year
    let header = div()
        .flex()
        .flex_col()
        .gap(px(theme.space_1()))
        .mb(px(theme.space_4()))
        .child(
            div()
                .text_size(px(theme.fs_1()))
                .text_color(theme.fg_muted())
                .child("← Esc to go back"),
        )
        .child(
            div()
                .text_size(px(theme.fs_6()))
                .font_weight(FontWeight::BOLD)
                .text_color(theme.fg_primary())
                .child(SharedString::from(m.title.clone())),
        )
        .child(
            div()
                .flex()
                .gap(px(theme.space_2()))
                .items_center()
                .text_size(px(theme.fs_2()))
                .text_color(theme.fg_secondary())
                .child(SharedString::from(
                    m.year.map(|y| y.to_string()).unwrap_or_default(),
                ))
                .child(SharedString::from(
                    m.rating.map(|r| format!("★ {:.1}", r)).unwrap_or_default(),
                ))
                .child(SharedString::from(
                    m.runtime.map(|r| format!("{} min", r)).unwrap_or_default(),
                ))
                .child(SharedString::from(if m.genres.is_empty() {
                    "".to_string()
                } else {
                    m.genres.join(" · ")
                })),
        );

    // Summary
    let summary: SharedString = m.summary.clone().unwrap_or_default().into();

    // Variants list
    let mut variants_col = div()
        .flex()
        .flex_col()
        .gap(px(theme.space_2()))
        .mt(px(theme.space_3()));

    for (i, v) in m.variants.iter().enumerate() {
        let quality = v.quality.clone().unwrap_or_default();
        let codec = v.video_codec.clone().unwrap_or_default();
        let audio = v.audio_channels.clone().unwrap_or_default();
        let row = div()
            .flex()
            .items_center()
            .gap(px(theme.space_3()))
            .p(px(theme.space_3()))
            .rounded(px(theme.radius_md()))
            .bg(theme.bg_surface())
            .border_1()
            .border_color(theme.border_subtle())
            .child(badge(SharedString::from(quality), theme.accent(), theme))
            .child(
                div()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_muted())
                    .child(SharedString::from(codec)),
            )
            .child(
                div()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_muted())
                    .child(SharedString::from(audio)),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.fg_primary())
                    .child(SharedString::from(v.size.clone())),
            )
            .child(
                div()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.success())
                    .child(SharedString::from(format!("↑{}", v.seeds))),
            )
            .child(
                div()
                    .text_size(px(theme.fs_1()))
                    .text_color(theme.error())
                    .child(SharedString::from(format!("↓{}", v.leeches))),
            )
            .child(primary_button(
                SharedString::from(format!("play-variant-{}", i)),
                "Play",
                theme,
            ));
        variants_col = variants_col.child(row);
    }

    div()
        .size_full()
        .overflow_hidden()
        .p(px(theme.space_5()))
        .bg(theme.bg_app())
        .flex()
        .flex_col()
        .child(header)
        .child(
            div()
                .text_size(px(theme.fs_2()))
                .text_color(theme.fg_secondary())
                .max_w(px(680.0))
                .child(summary),
        )
        .child(section_title("Variants", theme).mt(px(theme.space_5())))
        .child(variants_col)
        .into_any_element()
}

pub fn stub_page(theme: &Theme, title: &'static str, note: &'static str) -> impl IntoElement {
    div()
        .size_full()
        .p(px(theme.space_5()))
        .bg(theme.bg_app())
        .flex()
        .flex_col()
        .gap(px(theme.space_2()))
        .child(
            div()
                .text_size(px(theme.fs_1()))
                .text_color(theme.fg_muted())
                .child("← Esc to go back"),
        )
        .child(
            div()
                .text_size(px(theme.fs_6()))
                .font_weight(FontWeight::BOLD)
                .text_color(theme.fg_primary())
                .child(title),
        )
        .child(
            div()
                .text_size(px(theme.fs_2()))
                .text_color(theme.fg_secondary())
                .child(note),
        )
}

pub fn loading_page(theme: &Theme, text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.bg_app())
        .text_color(theme.fg_secondary())
        .text_size(px(theme.fs_3()))
        .child(text.into())
}

fn fallback(theme: &Theme, text: &'static str) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.bg_app())
        .text_color(theme.fg_muted())
        .text_size(px(theme.fs_3()))
        .child(text)
}

// Picking a tile by index from the browse grid: returns the group if found.
pub fn tile_at(
    browse: &BrowseData,
    section: BrowseSection,
    idx: usize,
) -> Option<std::sync::Arc<SearchResultGroup>> {
    let row: &[std::sync::Arc<SearchResultGroup>] = match section {
        BrowseSection::Latest => &browse.latest,
        BrowseSection::Popular => &browse.popular,
        BrowseSection::TopRated => &browse.top_rated,
        BrowseSection::Action => &browse.action,
        BrowseSection::Comedy => &browse.comedy,
        BrowseSection::Thriller => &browse.thriller,
        BrowseSection::SciFi => &browse.scifi,
        BrowseSection::Horror => &browse.horror,
    };
    row.get(idx).cloned()
}

#[derive(Debug, Clone, Copy)]
pub enum BrowseSection {
    Latest,
    Popular,
    TopRated,
    Action,
    Comedy,
    Thriller,
    SciFi,
    Horror,
}
