use clap::Parser;
use std::net::SocketAddr;
use streamx::cli;
use streamx::config;
use streamx::db;
use streamx::error::{self, Result};
use streamx::server;
use streamx::torrent;
use streamx::transcode;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    let config = config::load_config(&cli)?;

    if let Some(cmd) = &cli.command {
        return run_command(cmd, &config);
    }

    let filter = tracing_subscriber::EnvFilter::try_new(&config.log_level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        data_dir = %config.data_dir.display(),
        "Starting StreamX"
    );

    let db_dir = config.data_dir.join("db");
    std::fs::create_dir_all(&db_dir).map_err(|e| error::Error::Config {
        message: format!("Failed to create database directory: {e}"),
    })?;
    let db_path = db_dir.join("streamx.db");
    let database = db::Database::open(&db_path)?;
    database.init().await?;
    info!("Database initialized");

    if let (Some(admin_user), Some(admin_pass)) = (&config.admin_user, &config.admin_password) {
        match database.find_user_by_username(admin_user).await? {
            Some(_) => {
                info!(username = %admin_user, "Admin user already exists, skipping creation");
            }
            None => {
                let password_hash = server::auth::hash_password(admin_pass)?;
                database.create_user(admin_user, &password_hash).await?;
                info!(username = %admin_user, "Admin user created");
            }
        }
    }

    database.set_downloading_to_paused().await?;
    info!("Reset in-flight downloads to paused state");

    let torrent_engine =
        torrent::TorrentEngine::create(&config.torrent, &config.data_dir, database.clone()).await?;
    let search_provider = torrent::SearchProvider::new();
    let cache_dir = config.data_dir.join("cache");
    let hls_pipeline = transcode::HlsManager::new(&config.transcode, cache_dir).await?;

    let bind_addr = config.server.bind.clone();
    let port = config.server.port;
    let open_browser = config.open_browser;

    let app = server::build_router(
        database,
        config,
        torrent_engine,
        search_provider,
        hls_pipeline,
    );

    let addr: SocketAddr =
        format!("{bind_addr}:{port}")
            .parse()
            .map_err(|_| error::Error::Config {
                message: format!("Invalid bind address: {bind_addr}:{port}"),
            })?;

    info!(%addr, "Server listening");

    if open_browser {
        let url = format!("http://{addr}");
        let _ = open_url(&url);
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|source| error::Error::ServerBind {
            address: addr.to_string(),
            source,
        })?;

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|source| error::Error::ServerBind {
        address: addr.to_string(),
        source,
    })?;

    Ok(())
}

fn run_command(cmd: &cli::Command, config: &config::AppConfig) -> Result<()> {
    let data_dir = &config.data_dir;

    match cmd {
        cli::Command::Clean => {
            let cache_dir = data_dir.join("cache");
            let downloads_dir = data_dir.join("downloads");

            if cache_dir.exists() {
                std::fs::remove_dir_all(&cache_dir).map_err(|e| error::Error::Io { source: e })?;
                println!("Removed cache: {}", cache_dir.display());
            }
            if downloads_dir.exists() {
                std::fs::remove_dir_all(&downloads_dir)
                    .map_err(|e| error::Error::Io { source: e })?;
                println!("Removed downloads: {}", downloads_dir.display());
            }
            println!("Clean complete. Config and database preserved.");
        }
        cli::Command::Wipe => {
            let keep_config = data_dir.join("config.toml");
            let config_backup = if keep_config.exists() {
                Some(std::fs::read_to_string(&keep_config).ok())
            } else {
                None
            };

            let entries: Vec<_> = std::fs::read_dir(data_dir)
                .map_err(|e| error::Error::Io { source: e })?
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
                    std::fs::remove_dir_all(&path).map_err(|e| error::Error::Io { source: e })?;
                    println!("Removed: {}", path.display());
                } else {
                    std::fs::remove_file(&path).map_err(|e| error::Error::Io { source: e })?;
                    println!("Removed: {}", path.display());
                }
            }

            if let Some(Some(content)) = config_backup {
                std::fs::write(&keep_config, content)
                    .map_err(|e| error::Error::Io { source: e })?;
            }

            println!("Wipe complete. Only config.toml preserved.");
        }
    }

    Ok(())
}

fn open_url(url: &str) -> std::result::Result<(), std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn()?;
    }

    Ok(())
}
