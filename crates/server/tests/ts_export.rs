//! Regenerates TypeScript bindings for shared API types.
//!
//! Only active under the `ts-export` Cargo feature. Run with:
//!   cargo test --features ts-export --test ts_export
//!
//! Outputs land in `web/src/api/generated/` per each type's
//! `#[ts(export_to = ...)]` attribute.

#![cfg(feature = "ts-export")]

use ts_rs::TS;

#[test]
fn export_bindings() {
    use streamx_api::types::*;

    // Auth / user
    User::export_all().expect("export User");

    // Torrent
    TorrentFile::export_all().expect("export TorrentFile");
    TorrentInfo::export_all().expect("export TorrentInfo");

    // Search
    SearchRequest::export_all().expect("export SearchRequest");
    SearchResult::export_all().expect("export SearchResult");
    SearchResultGroup::export_all().expect("export SearchResultGroup");
    SearchResponse::export_all().expect("export SearchResponse");

    // TV
    TvTorrent::export_all().expect("export TvTorrent");
    TvEpisode::export_all().expect("export TvEpisode");
    TvSeason::export_all().expect("export TvSeason");
    TvSearchResultGroup::export_all().expect("export TvSearchResultGroup");
    TvSearchResponse::export_all().expect("export TvSearchResponse");

    // Music
    MusicVideoResult::export_all().expect("export MusicVideoResult");
    MusicVideoSearchResponse::export_all().expect("export MusicVideoSearchResponse");
    ResolveMagnetResponse::export_all().expect("export ResolveMagnetResponse");

    // Streams
    CreateStreamRequest::export_all().expect("export CreateStreamRequest");
    CreateStreamResponse::export_all().expect("export CreateStreamResponse");

    // Playlists
    Playlist::export_all().expect("export Playlist");
    PlaylistTrack::export_all().expect("export PlaylistTrack");

    // History
    WatchHistoryItem::export_all().expect("export WatchHistoryItem");
    WatchHistoryResponse::export_all().expect("export WatchHistoryResponse");
    SearchHistoryItem::export_all().expect("export SearchHistoryItem");
    SearchHistoryResponse::export_all().expect("export SearchHistoryResponse");

    // Favourites
    FavouriteItem::export_all().expect("export FavouriteItem");
    FavouritesResponse::export_all().expect("export FavouritesResponse");

    // Settings / errors
    Settings::export_all().expect("export Settings");
    ApiError::export_all().expect("export ApiError");
}
