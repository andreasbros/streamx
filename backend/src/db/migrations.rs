use crate::error::{self, Result};
use rusqlite::Connection;
use snafu::ResultExt;

const MIGRATIONS: &[&str] = &[
    // Migration 0: Create users table
    "CREATE TABLE IF NOT EXISTS users (
        id TEXT PRIMARY KEY,
        username TEXT UNIQUE NOT NULL,
        password_hash TEXT NOT NULL,
        created_at TEXT NOT NULL,
        is_admin INTEGER NOT NULL DEFAULT 0
    );",
    // Migration 1: Create search_history table
    "CREATE TABLE IF NOT EXISTS search_history (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL REFERENCES users(id),
        query TEXT NOT NULL,
        result_count INTEGER,
        searched_at TEXT NOT NULL
    );",
    // Migration 2: Create watch_history table
    "CREATE TABLE IF NOT EXISTS watch_history (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL REFERENCES users(id),
        magnet_uri TEXT NOT NULL,
        title TEXT NOT NULL,
        file_name TEXT,
        duration_seconds INTEGER,
        watched_seconds INTEGER,
        poster_url TEXT,
        watched_at TEXT NOT NULL
    );",
    // Migration 3: Create active_streams table
    "CREATE TABLE IF NOT EXISTS active_streams (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL REFERENCES users(id),
        magnet_uri TEXT NOT NULL,
        file_index INTEGER NOT NULL,
        status TEXT NOT NULL,
        progress REAL,
        peers INTEGER,
        download_speed INTEGER,
        created_at TEXT NOT NULL
    );",
    // Migration 4: Create user_settings table
    "CREATE TABLE IF NOT EXISTS user_settings (
        user_id TEXT PRIMARY KEY REFERENCES users(id),
        theme TEXT NOT NULL DEFAULT 'dark',
        updated_at TEXT NOT NULL
    );",
    // Migration 5: Create schema_version tracking
    "CREATE TABLE IF NOT EXISTS schema_version (
        version INTEGER PRIMARY KEY
    );",
    // Migration 6: Create downloads table for torrent state tracking
    "CREATE TABLE IF NOT EXISTS downloads (
        info_hash TEXT PRIMARY KEY,
        magnet_uri TEXT NOT NULL,
        title TEXT NOT NULL DEFAULT '',
        file_name TEXT NOT NULL DEFAULT '',
        file_index INTEGER NOT NULL DEFAULT 0,
        file_size INTEGER NOT NULL DEFAULT 0,
        status TEXT NOT NULL DEFAULT 'initializing',
        progress REAL NOT NULL DEFAULT 0,
        partial_path TEXT,
        complete_path TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );",
];

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY);")
        .context(error::DatabaseSnafu)?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), -1) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .context(error::DatabaseSnafu)?;

    for (i, migration) in MIGRATIONS.iter().enumerate() {
        let version = i as i64;
        if version > current_version {
            conn.execute_batch(migration)
                .context(error::DatabaseSnafu)?;
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                rusqlite::params![version],
            )
            .context(error::DatabaseSnafu)?;
            tracing::info!(version = version, "Applied migration");
        }
    }

    Ok(())
}
