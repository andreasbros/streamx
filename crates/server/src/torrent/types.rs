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
    pub is_audio: bool,
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
