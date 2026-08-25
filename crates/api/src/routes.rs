//! Route path constants. Keep these in sync between server registration and
//! client calls so a typo fails at compile time on either side.

/// Server version + build hash.
pub const VERSION: &str = "/api/version";

/// `POST { username, password } -> { token }`
pub const AUTH_LOGIN: &str = "/api/auth/login";

/// `POST { username, password } -> { token }`
pub const AUTH_REGISTER: &str = "/api/auth/register";

/// `GET -> User` (requires auth)
pub const AUTH_ME: &str = "/api/auth/me";

pub const LOGIN: &str = AUTH_LOGIN;
pub const REGISTER: &str = AUTH_REGISTER;
pub const ME: &str = AUTH_ME;
pub const SEARCH: &str = "/api/search";
pub const BROWSE: &str = "/api/search/browse";
pub const CREATE_STREAM: &str = "/api/stream";

/// `GET /api/downloads/{info_hash}/movie` -> `SearchResultGroup` rebuilt
/// from the stored download + metadata, for opening the movie page.
pub fn download_movie(info_hash: &str) -> String {
    format!("/api/downloads/{info_hash}/movie")
}

/// `GET /api/stream/{id}/files` -> `{ files: [TorrentFile], status }`
pub fn stream_files(id: &str) -> String {
    format!("/api/stream/{id}/files")
}

/// `GET /api/stream/{id}/file/{index}` binary stream.
pub fn stream_file(id: &str, file_index: usize) -> String {
    format!("/api/stream/{id}/file/{file_index}")
}

/// `GET /api/stream/{id}/artwork/{index}` embedded artwork for an audio track.
pub fn stream_artwork(id: &str, file_index: usize) -> String {
    format!("/api/stream/{id}/artwork/{file_index}")
}

/// Playlist CRUD.
pub const PLAYLISTS: &str = "/api/playlists";
pub fn playlist(id: &str) -> String {
    format!("/api/playlists/{id}")
}
pub fn playlist_tracks(id: &str) -> String {
    format!("/api/playlists/{id}/tracks")
}
