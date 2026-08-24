//! Destructive maintenance shared by the server CLI (`streamx clean` /
//! `streamx wipe`) and the desktop app's Admin page. Callers must
//! guarantee no server components are running against the data dir.

use crate::config::AppConfig;
use crate::error::{Error, Result};

fn io(e: std::io::Error) -> Error {
    Error::Io { source: e }
}

/// Remove cache and downloads. Config and database are preserved.
pub fn clean(config: &AppConfig) -> Result<()> {
    let cache_dir = config.data_dir.join("cache");
    let downloads_dir = config.downloads_dir();

    if cache_dir.exists() {
        std::fs::remove_dir_all(&cache_dir).map_err(io)?;
        tracing::info!(path = %cache_dir.display(), "maintenance: removed cache");
    }
    if downloads_dir.exists() {
        std::fs::remove_dir_all(&downloads_dir).map_err(io)?;
        tracing::info!(path = %downloads_dir.display(), "maintenance: removed downloads");
    }
    Ok(())
}

/// Remove everything in the data dir except `config.toml`: database,
/// history, favourites, cache, downloads, DHT state, logs.
pub fn wipe(config: &AppConfig) -> Result<()> {
    let data_dir = &config.data_dir;
    let keep_config = data_dir.join("config.toml");
    let config_backup = keep_config
        .exists()
        .then(|| std::fs::read_to_string(&keep_config).ok())
        .flatten();

    let entries: Vec<_> = std::fs::read_dir(data_dir)
        .map_err(io)?
        .filter_map(|e| e.ok())
        .collect();
    for entry in entries {
        let path = entry.path();
        if path
            .file_name()
            .map(|n| n == "config.toml")
            .unwrap_or(false)
        {
            continue;
        }
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(io)?;
        } else {
            std::fs::remove_file(&path).map_err(io)?;
        }
        tracing::info!(path = %path.display(), "maintenance: removed");
    }
    if let Some(content) = config_backup {
        std::fs::write(&keep_config, content).map_err(io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_for(dir: &std::path::Path) -> AppConfig {
        let mut c: AppConfig = toml::from_str("").expect("empty config");
        c.data_dir = dir.to_path_buf();
        c
    }

    #[test]
    fn clean_keeps_config_and_db() {
        let dir = tempfile::tempdir().expect("tmp");
        for sub in ["cache", "downloads/complete", "db"] {
            std::fs::create_dir_all(dir.path().join(sub)).expect("mkdir");
        }
        std::fs::write(dir.path().join("config.toml"), "x").expect("write");
        std::fs::write(dir.path().join("db/streamx.db"), "x").expect("write");

        clean(&config_for(dir.path())).expect("clean");

        assert!(!dir.path().join("cache").exists());
        assert!(!dir.path().join("downloads").exists());
        assert!(dir.path().join("config.toml").exists());
        assert!(dir.path().join("db/streamx.db").exists());
    }

    #[test]
    fn wipe_keeps_only_config() {
        let dir = tempfile::tempdir().expect("tmp");
        for sub in ["cache", "db", "dht", "logs"] {
            std::fs::create_dir_all(dir.path().join(sub)).expect("mkdir");
        }
        std::fs::write(dir.path().join("config.toml"), "keep me").expect("write");
        std::fs::write(dir.path().join("db/streamx.db"), "x").expect("write");

        wipe(&config_for(dir.path())).expect("wipe");

        let left: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, vec!["config.toml"]);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config.toml")).expect("read"),
            "keep me"
        );
    }
}
