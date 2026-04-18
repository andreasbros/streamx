use crate::cli::Cli;
use crate::error::{self, Error, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_server")]
    pub server: ServerConfig,
    #[serde(default = "default_torrent")]
    pub torrent: TorrentConfig,
    #[serde(default = "default_transcode")]
    pub transcode: TranscodeConfig,
    #[serde(default = "default_auth")]
    pub auth: AuthConfig,
    #[serde(default = "default_ui")]
    pub ui: UiConfig,

    #[serde(skip)]
    pub data_dir: PathBuf,
    #[serde(skip)]
    pub log_level: String,
    #[serde(skip)]
    pub open_browser: bool,
    #[serde(skip)]
    pub admin_user: Option<String>,
    #[serde(skip)]
    pub admin_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_open_browser")]
    pub open_browser: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentConfig {
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_true")]
    pub sequential: bool,
    #[serde(default = "default_true")]
    pub seed_after_complete: bool,
    #[serde(default = "default_true")]
    pub dht: bool,
    #[serde(default = "default_true")]
    pub pex: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscodeConfig {
    #[serde(default = "default_segment_duration")]
    pub hls_segment_duration: u32,
    #[serde(default = "default_video_codec")]
    pub video_codec: String,
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,
    #[serde(default = "default_preset")]
    pub preset: String,
    #[serde(default = "default_max_concurrent_transcodes")]
    pub max_concurrent_transcodes: u32,
    #[serde(default = "default_crf")]
    pub crf: u32,
    #[serde(default)]
    pub max_bitrate: Option<String>,
    #[serde(default = "default_audio_bitrate")]
    pub audio_bitrate: String,
    #[serde(default)]
    pub threads: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub jwt_secret: String,
    #[serde(default = "default_session_duration")]
    pub session_duration: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_theme")]
    pub default_theme: String,
}

fn default_server() -> ServerConfig {
    ServerConfig {
        port: default_port(),
        bind: default_bind(),
        open_browser: default_open_browser(),
    }
}

fn default_torrent() -> TorrentConfig {
    TorrentConfig {
        max_connections: default_max_connections(),
        sequential: true,
        seed_after_complete: true,
        dht: true,
        pex: true,
    }
}

fn default_transcode() -> TranscodeConfig {
    TranscodeConfig {
        hls_segment_duration: default_segment_duration(),
        video_codec: default_video_codec(),
        audio_codec: default_audio_codec(),
        preset: default_preset(),
        max_concurrent_transcodes: default_max_concurrent_transcodes(),
        crf: default_crf(),
        max_bitrate: None,
        audio_bitrate: default_audio_bitrate(),
        threads: None,
    }
}

fn default_auth() -> AuthConfig {
    AuthConfig {
        jwt_secret: String::new(),
        session_duration: default_session_duration(),
    }
}

fn default_ui() -> UiConfig {
    UiConfig {
        default_theme: default_theme(),
    }
}

fn default_port() -> u16 {
    8999
}

fn default_bind() -> String {
    "127.0.0.1".to_string()
}

fn default_open_browser() -> bool {
    false
}

fn default_max_connections() -> u32 {
    200
}

fn default_true() -> bool {
    true
}

fn default_segment_duration() -> u32 {
    4
}

fn default_video_codec() -> String {
    "h264".to_string()
}

fn default_audio_codec() -> String {
    "aac".to_string()
}

fn default_preset() -> String {
    "ultrafast".to_string()
}

fn default_max_concurrent_transcodes() -> u32 {
    2
}

fn default_crf() -> u32 {
    23
}

fn default_audio_bitrate() -> String {
    "192k".to_string()
}

fn default_session_duration() -> String {
    "7d".to_string()
}

fn default_theme() -> String {
    "dark".to_string()
}

fn generate_jwt_secret() -> String {
    let mut bytes = [0u8; 64];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn default_data_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::Config {
        message: "HOME environment variable not set".to_string(),
    })?;
    Ok(PathBuf::from(home).join(".streamx"))
}

fn default_config_content() -> String {
    r#"[server]
port = 8999
bind = "127.0.0.1"
open_browser = false

[torrent]
max_connections = 200
sequential = true
seed_after_complete = true
dht = true
pex = true

[transcode]
hls_segment_duration = 4
video_codec = "h264"
audio_codec = "aac"
preset = "ultrafast"
max_concurrent_transcodes = 2
crf = 23
audio_bitrate = "192k"

[auth]
jwt_secret = ""
session_duration = "7d"

[ui]
default_theme = "dark"
"#
    .to_string()
}

pub fn load_config(cli: &Cli) -> Result<AppConfig> {
    let data_dir = match &cli.data_dir {
        Some(d) => PathBuf::from(d),
        None => default_data_dir()?,
    };

    std::fs::create_dir_all(&data_dir).context(error::IoSnafu)?;

    let config_path = match &cli.config {
        Some(p) => PathBuf::from(p),
        None => data_dir.join("config.toml"),
    };

    let mut config = load_from_file(&config_path)?;

    if config.auth.jwt_secret.is_empty() {
        config.auth.jwt_secret = generate_jwt_secret();
        save_config(&config_path, &config)?;
    }

    if let Some(port) = cli.port {
        config.server.port = port;
    }
    if let Some(ref bind) = cli.bind {
        config.server.bind = bind.clone();
    }

    config.data_dir = data_dir;
    config.log_level = cli.log_level.clone().unwrap_or_else(|| "info".to_string());
    config.open_browser = cli.open || config.server.open_browser;
    config.admin_user = cli.admin_user.clone();
    config.admin_password = cli.admin_password.clone();

    ensure_directories(&config)?;

    Ok(config)
}

fn load_from_file(path: &Path) -> Result<AppConfig> {
    if path.exists() {
        let content = std::fs::read_to_string(path).context(error::IoSnafu)?;
        let config: AppConfig = toml::from_str(&content).map_err(|e| Error::Config {
            message: format!("Failed to parse config file: {e}"),
        })?;
        Ok(config)
    } else {
        let content = default_config_content();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context(error::IoSnafu)?;
        }
        std::fs::write(path, &content).context(error::IoSnafu)?;
        let config: AppConfig = toml::from_str(&content).map_err(|e| Error::Config {
            message: format!("Failed to parse default config: {e}"),
        })?;
        Ok(config)
    }
}

fn save_config(path: &Path, config: &AppConfig) -> Result<()> {
    let content = toml::to_string_pretty(config).map_err(|e| Error::Config {
        message: format!("Failed to serialize config: {e}"),
    })?;
    std::fs::write(path, content).context(error::IoSnafu)?;
    Ok(())
}

fn ensure_directories(config: &AppConfig) -> Result<()> {
    let cache_dir = config.data_dir.join("cache");
    std::fs::create_dir_all(&cache_dir).context(error::IoSnafu)?;

    let partial_dir = config.data_dir.join("downloads").join("partial");
    std::fs::create_dir_all(&partial_dir).context(error::IoSnafu)?;

    let complete_dir = config.data_dir.join("downloads").join("complete");
    std::fs::create_dir_all(&complete_dir).context(error::IoSnafu)?;

    let dht_dir = config.data_dir.join("dht");
    std::fs::create_dir_all(&dht_dir).context(error::IoSnafu)?;

    Ok(())
}
