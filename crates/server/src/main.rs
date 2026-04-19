use clap::Parser;
use std::net::SocketAddr;
use streamx::cli;
use streamx::config;
use streamx::error::{self, Result};
use streamx::logging::BroadcastLayer;
use streamx::runner;
use streamx::server;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    let config = config::load_config(&cli)?;

    if let Some(cmd) = &cli.command {
        return run_command(cmd, &config);
    }

    let filter = tracing_subscriber::EnvFilter::try_new(&config.log_level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let (log_tx, _) = tokio::sync::broadcast::channel::<String>(1000);
    let (broadcast_layer, log_history) = BroadcastLayer::new(log_tx.clone());

    let _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>;
    if let Some(ref log_dir) = config.log_dir {
        std::fs::create_dir_all(log_dir).map_err(|e| error::Error::Config {
            message: format!("Failed to create log directory: {e}"),
        })?;
        let file_appender = tracing_appender::rolling::daily(log_dir, "streamx.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        _log_guard = Some(guard);
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(non_blocking),
            )
            .with(broadcast_layer)
            .init();
    } else {
        _log_guard = None;
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_target(true))
            .with(broadcast_layer)
            .init();
    }

    info!(
        version = env!("CARGO_PKG_VERSION"),
        data_dir = %config.data_dir.display(),
        "Starting StreamX"
    );

    let bind_addr = config.server.bind.clone();
    let port = config.server.port;
    let open_browser = config.open_browser;

    let components = runner::build_components(config, Some(log_tx), Some(log_history)).await?;

    let addr: SocketAddr = format!("{bind_addr}:{port}")
        .parse()
        .map_err(|_| error::Error::Config {
            message: format!("Invalid bind address: {bind_addr}:{port}"),
        })?;

    // Kill orphaned FFmpeg processes from previous server instances
    kill_orphaned_ffmpeg();

    info!(%addr, "Server listening");

    if open_browser {
        let url = format!("http://{addr}");
        let _ = open_url(&url);
    }

    // Graceful shutdown: catch SIGTERM/SIGINT, kill FFmpeg children
    let shutdown = async {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");
        let mut sigint =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .expect("failed to install SIGINT handler");
        tokio::select! {
            _ = sigterm.recv() => info!("Received SIGTERM"),
            _ = sigint.recv() => info!("Received SIGINT"),
        }
        info!("Shutting down, killing FFmpeg children...");
        kill_all_streamx_ffmpeg();
    };

    let config_for_state: streamx::config::AppConfig = (*components.config).clone();
    let state = server::build_state(
        components.database,
        config_for_state,
        components.torrent_engine,
        components.search_provider,
        components.hls_pipeline,
        components.log_tx,
        components.log_history,
    );
    let app = server::build_router_with_state(state);

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
    .with_graceful_shutdown(shutdown)
    .await
    .map_err(|source| error::Error::ServerBind {
        address: addr.to_string(),
        source,
    })?;

    Ok(())
}

fn kill_orphaned_ffmpeg() {
    let our_pid = std::process::id().to_string();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let pid: u32 = match name_str.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let cmdline = match std::fs::read_to_string(entry.path().join("cmdline")) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if !cmdline.contains("ffmpeg") || !cmdline.contains(".streamx/cache") {
                continue;
            }
            // Check if it's our child (skip those)
            if let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) {
                let ppid = stat.split_whitespace().nth(3).unwrap_or("");
                if ppid == our_pid {
                    continue;
                }
            }
            tracing::warn!(pid, "Killing orphaned FFmpeg process");
            unsafe { libc::kill(pid as i32, libc::SIGTERM); }
        }
    }
}

fn kill_all_streamx_ffmpeg() {
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let pid: i32 = match name_str.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let cmdline = match std::fs::read_to_string(entry.path().join("cmdline")) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if cmdline.contains("ffmpeg") && cmdline.contains(".streamx/cache") {
                tracing::info!(pid, "Sending SIGTERM to FFmpeg process");
                unsafe { libc::kill(pid, libc::SIGTERM); }
            }
        }
    }
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
