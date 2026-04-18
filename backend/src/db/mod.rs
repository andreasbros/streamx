pub mod downloads;
pub mod history;
pub mod migrations;
pub mod settings;
pub mod users;

use crate::error::{self, Result};
use rusqlite::Connection;
use snafu::ResultExt;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).context(error::DatabaseSnafu)?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .context(error::DatabaseSnafu)?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        Ok(db)
    }

    pub async fn init(&self) -> Result<()> {
        let conn = self.conn.lock().await;
        migrations::run_migrations(&conn)?;
        Ok(())
    }

    pub fn connection(&self) -> &Arc<Mutex<Connection>> {
        &self.conn
    }
}
