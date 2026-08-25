//! Serde types shared between server, desktop client, and web (via ts-rs).
//!
//! When the `ts-export` feature is enabled, each type also derives `ts_rs::TS`
//! and is written to `web/src/api/generated/` by the ts_export test in the
//! server crate.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

// Tiny macro to cut down on the boilerplate of `#[cfg_attr(ts-export, ...)]`
// on every type. Emits the derive + the export path (`web/src/api/generated/`).
macro_rules! ts {
    ($item:item) => {
        #[cfg_attr(feature = "ts-export", derive(TS))]
        #[cfg_attr(
            feature = "ts-export",
            ts(export, export_to = "../../../web/src/api/generated/")
        )]
        $item
    };
}

// ===================== Torrent =====================

ts! {
/// Individual file inside a torrent. Used by the multi-file album flow and
/// the desktop track picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentFile {
    pub index: usize,
    pub path: String,
    pub size: u64,
    pub is_video: bool,
    pub is_audio: bool,
}
}

impl TorrentFile {
    pub fn detect_video(path: &str) -> bool {
        let lower = path.to_lowercase();
        lower.ends_with(".mp4")
            || lower.ends_with(".mkv")
            || lower.ends_with(".avi")
            || lower.ends_with(".webm")
            || lower.ends_with(".mov")
            || lower.ends_with(".m4v")
            || lower.ends_with(".wmv")
            || lower.ends_with(".flv")
            || lower.ends_with(".ts")
    }
    pub fn detect_audio(path: &str) -> bool {
        let lower = path.to_lowercase();
        lower.ends_with(".mp3")
            || lower.ends_with(".flac")
            || lower.ends_with(".m4a")
            || lower.ends_with(".aac")
            || lower.ends_with(".ogg")
            || lower.ends_with(".oga")
            || lower.ends_with(".opus")
            || lower.ends_with(".wav")
            || lower.ends_with(".wma")
            || lower.ends_with(".alac")
    }
}

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentInfo {
    pub name: String,
    pub total_size: u64,
    pub files: Vec<TorrentFile>,
    pub info_hash: String,
}
}

// ===================== Auth =====================

ts! {
#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(default, skip_serializing)]
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub password_hash: String,
    pub created_at: String,
    pub is_admin: bool,
}
}

// Manual Deserialize so `password_hash` just defaults to empty on the client.
impl<'de> serde::Deserialize<'de> for User {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            username: String,
            created_at: String,
            is_admin: bool,
            #[serde(default)]
            password_hash: Option<String>,
        }
        let r = Raw::deserialize(d)?;
        Ok(User {
            id: r.id,
            username: r.username,
            created_at: r.created_at,
            is_admin: r.is_admin,
            password_hash: r.password_hash.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub token: String,
}

// ===================== Version =====================

#[derive(Debug, Deserialize)]
pub struct VersionResponse {
    pub version: String,
    pub hash: String,
}

// ===================== Search =====================

ts! {
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_page")]
    pub page: u32,
}
}

fn default_page() -> u32 {
    1
}

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub magnet: String,
    pub seeds: u32,
    pub leeches: u32,
    pub size: String,
    pub size_bytes: u64,
    pub quality: Option<String>,
    pub video_codec: Option<String>,
    pub audio_channels: Option<String>,
    pub bit_depth: Option<String>,
    pub source_type: Option<String>,
}
}

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultGroup {
    pub title: String,
    pub year: Option<u32>,
    pub rating: Option<f64>,
    pub runtime: Option<u32>,
    #[serde(default)]
    pub genres: Vec<String>,
    pub language: Option<String>,
    pub mpa_rating: Option<String>,
    pub summary: Option<String>,
    pub imdb_code: Option<String>,
    pub trailer_code: Option<String>,
    pub poster: Option<String>,
    pub poster_small: Option<String>,
    pub poster_medium: Option<String>,
    pub poster_large: Option<String>,
    pub backdrop: Option<String>,
    #[serde(default)]
    pub variants: Vec<SearchResult>,
}
}

ts! {
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultGroup>,
}
}

// ===================== TV =====================

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvTorrent {
    pub magnet: String,
    pub seeds: u32,
    pub leeches: u32,
    pub size_bytes: u64,
    pub quality: Option<String>,
    pub filename: String,
}
}

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvEpisode {
    pub episode: u32,
    pub title: Option<String>,
    #[serde(default)]
    pub variants: Vec<TvTorrent>,
}
}

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvSeason {
    pub season: u32,
    #[serde(default)]
    pub episodes: Vec<TvEpisode>,
}
}

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvSearchResultGroup {
    pub show_name: String,
    pub imdb_id: Option<String>,
    #[serde(default)]
    pub seasons: Vec<TvSeason>,
}
}

ts! {
#[derive(Debug, Serialize, Deserialize)]
pub struct TvSearchResponse {
    pub results: Vec<TvSearchResultGroup>,
}
}

// ===================== Music =====================

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicVideoResult {
    pub title: String,
    pub magnet: Option<String>,
    pub seeds: u32,
    pub leeches: u32,
    pub size: String,
    pub detail_url: String,
    /// Upload date in ISO `YYYY-MM-DD` form. apibay returns a unix
    /// timestamp; the 1337x scraper falls back to a regex over the
    /// torrent title (e.g. "... 2024-08-12 ...", "(2024)").
    #[serde(default)]
    pub date: Option<String>,
}
}

ts! {
#[derive(Debug, Serialize, Deserialize)]
pub struct MusicVideoSearchResponse {
    pub results: Vec<MusicVideoResult>,
}
}

ts! {
#[derive(Debug, Deserialize)]
pub struct ResolveMagnetResponse {
    pub magnet: String,
}
}

ts! {
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadItem {
    pub info_hash: String,
    pub magnet_uri: String,
    pub title: String,
    pub file_name: String,
    pub file_size: u64,
    pub status: String,
    pub progress: f64,
    pub pinned: bool,
    pub download_all: bool,
    pub created_at: String,
    pub updated_at: String,
    pub peers: u32,
    pub speed: f64,
}
}

ts! {
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamStatus {
    pub id: String,
    pub status: String,
    pub progress: f32,
    pub title: String,
    pub file_name: String,
    pub file_size: u64,
    pub peers: u32,
    pub speed_bps: f64,
}
}

// ===================== Streams =====================

ts! {
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateStreamRequest {
    pub magnet_uri: String,
    pub file_index: Option<usize>,
    pub poster_url: Option<String>,
    pub title: Option<String>,
    pub year: Option<u32>,
    pub rating: Option<f64>,
    pub runtime: Option<u32>,
    #[serde(default)]
    pub genres: Option<Vec<String>>,
    pub language: Option<String>,
    pub video_codec: Option<String>,
    pub audio_channels: Option<String>,
    pub source_type: Option<String>,
    pub summary: Option<String>,
    pub imdb_code: Option<String>,
    pub mpa_rating: Option<String>,
    pub bit_depth: Option<String>,
    pub trailer_code: Option<String>,
    pub poster_small: Option<String>,
    pub poster_medium: Option<String>,
    pub poster_large: Option<String>,
    pub backdrop: Option<String>,
}
}

ts! {
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStreamResponse {
    pub stream_id: String,
    pub status: String,
    pub title: String,
    pub file_name: Option<String>,
}
}

// ===================== Playlists =====================

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub track_count: i64,
    pub created_at: String,
    pub updated_at: String,
}
}

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTrack {
    pub id: String,
    pub playlist_id: String,
    pub info_hash: String,
    pub file_index: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_seconds: Option<i64>,
    pub artwork_url: Option<String>,
    pub position: i64,
    pub created_at: String,
}
}

#[derive(Debug, Deserialize)]
pub struct AddTrackRequest {
    pub info_hash: String,
    pub file_index: Option<i64>,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_seconds: Option<i64>,
    pub artwork_url: Option<String>,
}

// ===================== History =====================

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchHistoryItem {
    pub id: String,
    pub magnet_uri: String,
    pub title: String,
    pub file_name: Option<String>,
    pub duration_seconds: Option<i64>,
    pub watched_seconds: Option<i64>,
    pub poster_url: Option<String>,
    pub watched_at: String,
    pub info_hash: Option<String>,
    pub file_size: Option<i64>,
    pub year: Option<i32>,
    pub rating: Option<f64>,
    pub runtime: Option<i32>,
    pub genres: Option<String>,
    pub summary: Option<String>,
    pub imdb_code: Option<String>,
}
}

ts! {
#[derive(Debug, Serialize, Deserialize)]
pub struct WatchHistoryResponse {
    pub items: Vec<WatchHistoryItem>,
}
}

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHistoryItem {
    pub id: String,
    pub query: String,
    pub result_count: Option<i32>,
    pub searched_at: String,
}
}

ts! {
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchHistoryResponse {
    pub searches: Vec<SearchHistoryItem>,
}
}

// ===================== Favourites =====================

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavouriteItem {
    pub id: String,
    pub user_id: String,
    pub content_type: String,
    pub title: String,
    pub year: Option<i32>,
    pub rating: Option<f64>,
    pub poster_url: Option<String>,
    pub info_hash: Option<String>,
    pub metadata_json: Option<String>,
    pub created_at: String,
}
}

ts! {
#[derive(Debug, Serialize, Deserialize)]
pub struct FavouritesResponse {
    pub items: Vec<FavouriteItem>,
}
}

#[derive(Debug, Deserialize)]
pub struct AddFavouriteRequest {
    pub content_type: Option<String>,
    pub title: String,
    pub year: Option<i32>,
    pub rating: Option<f64>,
    pub poster_url: Option<String>,
    pub info_hash: Option<String>,
    pub metadata_json: Option<String>,
}

// ===================== Settings / errors =====================

ts! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String, // "dark" | "light"
}
}

ts! {
#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub message: String,
}
}
