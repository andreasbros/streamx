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
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub vpn: Option<VpnConfig>,

    #[serde(skip)]
    pub data_dir: PathBuf,
    #[serde(skip)]
    pub log_level: String,
    #[serde(skip)]
    pub log_dir: Option<PathBuf>,
    #[serde(skip)]
    pub open_browser: bool,
    #[serde(skip)]
    pub admin_user: Option<String>,
    #[serde(skip)]
    pub admin_password: Option<String>,
}

impl AppConfig {
    pub fn provider_by_kind(&self, kind: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.kind == kind)
    }

    pub fn provider_by_id(&self, id: u32) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Root of torrent data (`partial/`, `complete/`, `posters/` live under it).
    pub fn downloads_dir(&self) -> PathBuf {
        resolve_downloads_dir(self.torrent.download_dir.as_deref(), &self.data_dir)
    }
}

pub fn resolve_downloads_dir(download_dir: Option<&str>, data_dir: &Path) -> PathBuf {
    match download_dir.map(str::trim) {
        Some(dir) if !dir.is_empty() => expand_tilde(dir),
        _ => data_dir.join("downloads"),
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// Downloads dir for a data dir without a full config load (no directory
/// creation). For processes that resolve paths before or without booting
/// the server, e.g. the desktop app. Honors `STREAMX_CONFIG`.
pub fn downloads_dir_for(data_dir: &Path) -> PathBuf {
    let path = std::env::var("STREAMX_CONFIG")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("config.toml"));
    let download_dir = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
        .and_then(|v| Some(v.get("torrent")?.get("download_dir")?.as_str()?.to_string()));
    resolve_downloads_dir(download_dir.as_deref(), data_dir)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_open_browser")]
    pub open_browser: bool,
    #[serde(default)]
    pub log_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentConfig {
    /// Where torrent data lands. Relative to nothing: absolute path or
    /// `~/...`. Unset means `<data_dir>/downloads`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<String>,
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
    #[serde(default)]
    pub gpu: bool,
    #[serde(default = "default_true")]
    pub hls_downscale: bool,
    #[serde(default = "default_hls_max_height")]
    pub hls_max_height: u32,
    /// Force stereo audio in transcoded HLS tiers (720p/1080p/360p).
    /// Chrome/Firefox MSE cannot decode multi-channel AAC.
    /// Default: true (stereo for browser compatibility).
    /// Set to false to preserve surround in transcoded tiers (Safari/native players only).
    #[serde(default = "default_true")]
    pub hls_force_stereo: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub jwt_secret: String,
    #[serde(default = "default_session_duration")]
    pub session_duration: String,
}

/// A content provider (movies, tv, music, etc).
/// Users supply the URL; we never hardcode external domains.
/// `format`: "yts", "eztv", "apibay", "scrape" (default based on kind)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: u32,
    pub kind: String,
    pub url: String,
    /// Display name for provider-prefixed search (e.g. "yts", "tpb", "1337x")
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    /// Tracker URLs appended to magnets from this provider
    #[serde(default)]
    pub trackers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnConfig {
    pub socks5: String,
}

impl VpnConfig {
    /// Resolve the SOCKS5 URL, expanding `${ENV_VAR}` patterns and
    /// injecting credentials from `STREAMX_SOCKS5_PROXY_USERNAME` /
    /// `STREAMX_SOCKS5_PROXY_PASSWORD` env vars if the URL has no auth.
    pub fn resolved_url(&self) -> String {
        let mut url = expand_env_vars(&self.socks5);

        // If URL has no credentials, inject from env vars
        if !url.contains('@') {
            let user = std::env::var("STREAMX_SOCKS5_PROXY_USERNAME").unwrap_or_default();
            let pass = std::env::var("STREAMX_SOCKS5_PROXY_PASSWORD").unwrap_or_default();
            if !user.is_empty() {
                let auth = if pass.is_empty() {
                    user
                } else {
                    format!("{}:{}", user, pass)
                };
                // socks5://host:port -> socks5://user:pass@host:port
                if let Some(idx) = url.find("://") {
                    url = format!("{}://{}@{}", &url[..idx], auth, &url[idx + 3..]);
                }
            }
        }

        url
    }
}

fn expand_env_vars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            for ch in chars.by_ref() {
                if ch == '}' {
                    break;
                }
                var_name.push(ch);
            }
            result.push_str(&std::env::var(&var_name).unwrap_or_default());
        } else {
            result.push(c);
        }
    }
    result
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
        log_level: None,
    }
}

fn default_torrent() -> TorrentConfig {
    TorrentConfig {
        download_dir: None,
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
        gpu: false,
        hls_downscale: true,
        hls_max_height: default_hls_max_height(),
        hls_force_stereo: true,
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
    4
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

fn default_hls_max_height() -> u32 {
    1080
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

// std::env::home_dir resolves $HOME on unix and the user profile on
// Windows (un-deprecated with corrected behavior in modern Rust).
fn default_data_dir() -> Result<PathBuf> {
    let home = std::env::home_dir().ok_or_else(|| Error::Config {
        message: "cannot determine the user home directory".to_string(),
    })?;
    Ok(home.join(".streamx"))
}

fn default_config_content() -> String {
    r#"[server]
port = 8999
bind = "127.0.0.1"
open_browser = false

[torrent]
# download_dir = "~/.streamx/downloads"
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
gpu = false
hls_downscale = true
hls_max_height = 1080

[auth]
jwt_secret = ""
session_duration = "7d"

[ui]
default_theme = "dark"

# Default content sources. Extra or replacement providers can live in
# providers.toml next to this file (same [[providers]] format).
[[providers]]
id = 1
name = "yts"
kind = "movies"
url = "https://yts.bz"
api_url = "https://movies-api.accel.li/api/v2/list_movies.json"

[[providers]]
id = 5
name = "torrentio"
kind = "movies"
url = "https://torrentio.strem.fun/providers=yts,1337x,thepiratebay"
format = "torrentio"

[[providers]]
id = 2
name = "torrentio"
kind = "tv"
url = "https://torrentio.strem.fun/providers=eztv,1337x,thepiratebay"
format = "torrentio"

[[providers]]
id = 3
name = "tpb"
kind = "music-videos"
url = "https://apibay.org"
format = "apibay"
category = "601"

[[providers]]
id = 4
name = "tpb"
kind = "music"
url = "https://apibay.org"
format = "apibay"
category = "101"
"#
    .to_string()
}

/// Provider entry in providers.toml (no id required)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderEntry {
    pub kind: String,
    #[serde(default)]
    pub name: Option<String>,
    pub url: String,
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub trackers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProvidersFile {
    #[serde(default, rename = "provider")]
    providers: Vec<ProviderEntry>,
}

fn load_providers_file(data_dir: &Path) -> Vec<ProviderConfig> {
    let path = data_dir.join("providers.toml");
    if !path.exists() {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read providers.toml: {e}");
            return Vec::new();
        }
    };
    let file: ProvidersFile = match toml::from_str(&content) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Failed to parse providers.toml: {e}");
            return Vec::new();
        }
    };

    // Assign IDs starting from 1000 to avoid collision with config.toml providers
    file.providers
        .into_iter()
        .enumerate()
        .map(|(i, entry)| ProviderConfig {
            id: 1000 + i as u32,
            kind: entry.kind,
            name: entry.name,
            url: entry.url,
            api_url: entry.api_url,
            format: entry.format,
            category: entry.category,
            trackers: entry.trackers,
        })
        .collect()
}

pub fn load_config(cli: &Cli) -> Result<AppConfig> {
    // CLI flags win; STREAMX_DATA_DIR / STREAMX_CONFIG env vars cover
    // processes that can't take flags (desktop app, test harnesses), so
    // tests can redirect every on-disk path (db, torrents, cache,
    // posters, dht, logs all live under the data dir).
    let data_dir = match cli.data_dir.clone().or_else(|| {
        std::env::var("STREAMX_DATA_DIR")
            .ok()
            .filter(|s| !s.is_empty())
    }) {
        Some(d) => PathBuf::from(d),
        None => default_data_dir()?,
    };

    if data_dir.exists() && !data_dir.is_dir() {
        return Err(Error::Config {
            message: format!(
                "{} exists but is not a directory. Move or remove it \
                 (e.g. `mv {0} {0}.bak`) and restart.",
                data_dir.display()
            ),
        });
    }

    std::fs::create_dir_all(&data_dir).context(error::IoSnafu)?;

    let config_path = match cli.config.clone().or_else(|| {
        std::env::var("STREAMX_CONFIG")
            .ok()
            .filter(|s| !s.is_empty())
    }) {
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

    // Load additional providers from providers.toml
    let extra_providers = load_providers_file(&data_dir);
    if !extra_providers.is_empty() {
        tracing::info!(
            "Loaded {} providers from providers.toml",
            extra_providers.len()
        );
        config.providers.extend(extra_providers);
    }

    config.data_dir = data_dir.clone();
    config.log_level = cli
        .log_level
        .clone()
        .or_else(|| config.server.log_level.clone())
        .unwrap_or_else(|| "info".to_string());
    config.log_dir = cli
        .log_dir
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| Some(data_dir.join("logs")));
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

    let downloads_dir = config.downloads_dir();
    for sub in ["partial", "complete", "posters"] {
        if let Err(e) = std::fs::create_dir_all(downloads_dir.join(sub)) {
            return Err(Error::Config {
                message: format!(
                    "Downloads directory {} is unavailable ({e}). If it lives \
                     on an external volume, make sure the drive is mounted, \
                     then restart.",
                    downloads_dir.display()
                ),
            });
        }
    }

    let dht_dir = config.data_dir.join("dht");
    std::fs::create_dir_all(&dht_dir).context(error::IoSnafu)?;

    Ok(())
}
