use serde::{Deserialize, Serialize};

// Re-exports from the shared api crate so existing
// `crate::torrent::types::TorrentFile` paths keep working.
pub use streamx_api::types::{TorrentFile, TorrentInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TorrentStatus {
    Initializing,
    Downloading,
    Ready,
    Seeding,
    Paused,
    Error,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub id: String,
    pub magnet_uri: String,
    pub file_index: usize,
    pub status: TorrentStatus,
    pub progress: f64,
    pub peers: u32,
    pub download_speed: u64,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub error_message: Option<String>,
}
