use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentInfo {
    pub name: String,
    pub total_size: u64,
    pub files: Vec<TorrentFile>,
    pub info_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentFile {
    pub index: usize,
    pub path: String,
    pub size: u64,
    pub is_video: bool,
}

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
