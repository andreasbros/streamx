use crate::db::Database;
use crate::error::{self, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Download {
    pub info_hash: String,
    pub magnet_uri: String,
    pub title: String,
    pub file_name: String,
    pub file_index: usize,
    pub file_size: u64,
    /// All files in the torrent were selected for download (album).
    /// Persisted so `ensure_active` can restore the full file set
    /// after the torrent leaves the live session.
    pub download_all: bool,
    pub status: String,
    pub progress: f64,
    pub partial_path: Option<String>,
    pub complete_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// JSON-encoded `Vec<ManifestFile>` (stable file list). `None`
    /// until the torrent metadata has been read at least once.
    pub files_json: Option<String>,
}

/// One file in a torrent, with a stable alphabetical `seq_index` used
/// by the streaming API and the `native_index` librqbit needs to
/// stream pieces. Persisted as JSON in `downloads.files_json` so the
/// mapping survives restarts and never shifts when files move between
/// the partial and complete directories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    pub seq_index: usize,
    pub native_index: usize,
    pub path: String,
    pub size: u64,
    pub is_audio: bool,
    pub is_video: bool,
}

impl Download {
    /// Parse the persisted file manifest, if any.
    pub fn manifest(&self) -> Option<Vec<ManifestFile>> {
        let json = self.files_json.as_deref()?;
        serde_json::from_str(json).ok()
    }
}

impl Database {
    pub async fn upsert_download(&self, dl: &Download) -> Result<()> {
        let conn = self.connection().lock().await;
        conn.execute(
            // files_json is intentionally not touched on conflict: it is
            // owned by update_download_files and must survive re-adds.
            "INSERT INTO downloads (info_hash, magnet_uri, title, file_name, file_index, file_size, \
             status, progress, partial_path, complete_path, created_at, updated_at, download_all) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) \
             ON CONFLICT(info_hash) DO UPDATE SET \
             magnet_uri = ?2, title = ?3, file_name = ?4, file_index = ?5, file_size = ?6, \
             status = ?7, progress = ?8, partial_path = ?9, complete_path = ?10, updated_at = ?12, \
             download_all = ?13",
            rusqlite::params![
                dl.info_hash,
                dl.magnet_uri,
                dl.title,
                dl.file_name,
                dl.file_index as i64,
                dl.file_size as i64,
                dl.status,
                dl.progress,
                dl.partial_path,
                dl.complete_path,
                dl.created_at,
                dl.updated_at,
                dl.download_all as i64,
            ],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn get_download(&self, info_hash: &str) -> Result<Option<Download>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT info_hash, magnet_uri, title, file_name, file_index, file_size, \
                 status, progress, partial_path, complete_path, created_at, updated_at, download_all, files_json \
                 FROM downloads WHERE info_hash = ?1",
            )
            .context(error::DatabaseSnafu)?;

        let result = stmt.query_row(rusqlite::params![info_hash], |row| {
            Ok(Download {
                info_hash: row.get(0)?,
                magnet_uri: row.get(1)?,
                title: row.get(2)?,
                file_name: row.get(3)?,
                file_index: row.get::<_, i64>(4)? as usize,
                file_size: row.get::<_, i64>(5)? as u64,
                status: row.get(6)?,
                progress: row.get(7)?,
                partial_path: row.get(8)?,
                complete_path: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                download_all: row.get::<_, i64>(12)? != 0,
                files_json: row.get(13)?,
            })
        });

        match result {
            Ok(dl) => Ok(Some(dl)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context(error::DatabaseSnafu),
        }
    }

    pub async fn update_download_status(&self, info_hash: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE downloads SET status = ?1, updated_at = ?2 WHERE info_hash = ?3",
            rusqlite::params![status, now, info_hash],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn update_download_progress(
        &self,
        info_hash: &str,
        progress: f64,
        file_size: u64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE downloads SET progress = ?1, file_size = ?2, updated_at = ?3 WHERE info_hash = ?4",
            rusqlite::params![progress, file_size as i64, now, info_hash],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn update_download_metadata(
        &self,
        info_hash: &str,
        title: &str,
        file_name: &str,
        file_index: usize,
        file_size: u64,
        partial_path: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE downloads SET title = ?1, file_name = ?2, file_index = ?3, file_size = ?4, \
             partial_path = ?5, status = 'downloading', updated_at = ?6 WHERE info_hash = ?7",
            rusqlite::params![
                title,
                file_name,
                file_index as i64,
                file_size as i64,
                partial_path,
                now,
                info_hash,
            ],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    /// Persist the stable file manifest (JSON `Vec<ManifestFile>`).
    pub async fn update_download_files(&self, info_hash: &str, files_json: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE downloads SET files_json = ?1, updated_at = ?2 WHERE info_hash = ?3",
            rusqlite::params![files_json, now, info_hash],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn update_download_paths(
        &self,
        info_hash: &str,
        partial_path: Option<&str>,
        complete_path: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE downloads SET partial_path = ?1, complete_path = ?2, updated_at = ?3 WHERE info_hash = ?4",
            rusqlite::params![partial_path, complete_path, now, info_hash],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn list_downloads(&self) -> Result<Vec<Download>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT info_hash, magnet_uri, title, file_name, file_index, file_size, \
                 status, progress, partial_path, complete_path, created_at, updated_at, download_all, files_json \
                 FROM downloads ORDER BY updated_at DESC",
            )
            .context(error::DatabaseSnafu)?;

        let entries = stmt
            .query_map([], |row| {
                Ok(Download {
                    info_hash: row.get(0)?,
                    magnet_uri: row.get(1)?,
                    title: row.get(2)?,
                    file_name: row.get(3)?,
                    file_index: row.get::<_, i64>(4)? as usize,
                    file_size: row.get::<_, i64>(5)? as u64,
                    status: row.get(6)?,
                    progress: row.get(7)?,
                    partial_path: row.get(8)?,
                    complete_path: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    download_all: row.get::<_, i64>(12)? != 0,
                    files_json: row.get(13)?,
                })
            })
            .context(error::DatabaseSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::DatabaseSnafu)?;

        Ok(entries)
    }

    pub async fn reset_download(&self, info_hash: &str) -> Result<()> {
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE downloads SET status = 'initializing', progress = 0.0, \
             partial_path = NULL, complete_path = NULL, \
             updated_at = datetime('now') \
             WHERE info_hash = ?1",
            rusqlite::params![info_hash],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn delete_download(&self, info_hash: &str) -> Result<()> {
        let conn = self.connection().lock().await;
        conn.execute(
            "DELETE FROM downloads WHERE info_hash = ?1",
            rusqlite::params![info_hash],
        )
        .context(error::DatabaseSnafu)?;
        conn.execute(
            "DELETE FROM media_metadata WHERE info_hash = ?1",
            rusqlite::params![info_hash],
        )
        .context(error::DatabaseSnafu)?;
        conn.execute(
            "DELETE FROM watch_history WHERE magnet_uri IN (SELECT magnet_uri FROM downloads WHERE info_hash = ?1)",
            rusqlite::params![info_hash],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn set_downloading_to_paused(&self) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE downloads SET status = 'paused', updated_at = ?1 \
             WHERE status IN ('downloading', 'initializing')",
            rusqlite::params![now],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }
}
