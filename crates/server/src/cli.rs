use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "streamx",
    about = "StreamX - Torrent Video Streaming Player",
    version,
    author
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(short = 'p', long, help = "Listen port", env = "STREAMX_PORT")]
    pub port: Option<u16>,

    #[arg(short = 'b', long, help = "Bind address", env = "STREAMX_BIND")]
    pub bind: Option<String>,

    #[arg(short = 'd', long, help = "Data directory", env = "STREAMX_DATA_DIR")]
    pub data_dir: Option<String>,

    #[arg(short = 'c', long, help = "Config file path", env = "STREAMX_CONFIG")]
    pub config: Option<String>,

    #[arg(
        long,
        help = "Log level (trace, debug, info, warn, error)",
        env = "STREAMX_LOG_LEVEL"
    )]
    pub log_level: Option<String>,

    #[arg(
        long,
        help = "Log directory (enables file logging with daily rotation)",
        env = "STREAMX_LOG_DIR"
    )]
    pub log_dir: Option<String>,

    #[arg(long, help = "Open browser on start", env = "STREAMX_OPEN")]
    pub open: bool,

    #[arg(
        long,
        help = "Create admin user on startup (not recommended for production)",
        env = "STREAMX_ADMIN_USER"
    )]
    pub admin_user: Option<String>,

    #[arg(
        long,
        help = "Admin password (not recommended for production, use interactive setup instead)",
        env = "STREAMX_ADMIN_PASSWORD"
    )]
    pub admin_password: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Clear HLS cache and torrent downloads (keeps config and database)
    Clean,
    /// Wipe everything except config file (cache, downloads, database, logs, DHT)
    Wipe,
}
