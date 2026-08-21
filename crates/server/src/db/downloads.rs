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
    /// Pinned downloads keep downloading with no client connected and
    /// auto-resume at boot.
    pub pinned: bool,
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
                 status, progress, partial_path, complete_path, created_at, updated_at, download_all, files_json, pinned \
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
                pinned: row.get::<_, i64>(14)? != 0,
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

    /// Terminal transition: status and progress land in one statement so
    /// a finished download can never linger below 100%.
    pub async fn mark_download_complete(&self, info_hash: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE downloads SET status = 'complete', progress = 100.0, updated_at = ?1 \
             WHERE info_hash = ?2",
            rusqlite::params![now, info_hash],
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
                 status, progress, partial_path, complete_path, created_at, updated_at, download_all, files_json, pinned \
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
                    pinned: row.get::<_, i64>(14)? != 0,
                })
            })
            .context(error::DatabaseSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::DatabaseSnafu)?;

        Ok(entries)
    }

    /// Set or clear the pinned (background download) flag.
    pub async fn set_download_pinned(&self, info_hash: &str, pinned: bool) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE downloads SET pinned = ?1, updated_at = ?2 WHERE info_hash = ?3",
            rusqlite::params![pinned as i64, now, info_hash],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    /// Pinned downloads that still need data — resumed at server boot.
    pub async fn get_pinned_incomplete(&self) -> Result<Vec<String>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare("SELECT info_hash FROM downloads WHERE pinned = 1 AND status != 'complete'")
            .context(error::DatabaseSnafu)?;
        let hashes = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .context(error::DatabaseSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::DatabaseSnafu)?;
        Ok(hashes)
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

    /// Remove every DB trace of a download: the downloads row itself plus
    /// dependent rows in watch_history (keyed by magnet), media_metadata,
    /// favourites and playlist_tracks (keyed by info_hash). Runs in a
    /// transaction; watch_history goes first because its subquery needs
    /// the downloads row to still exist.
    pub async fn delete_download(&self, info_hash: &str) -> Result<()> {
        let mut conn = self.connection().lock().await;
        let tx = conn.transaction().context(error::DatabaseSnafu)?;
        tx.execute(
            "DELETE FROM watch_history WHERE magnet_uri IN (SELECT magnet_uri FROM downloads WHERE info_hash = ?1)",
            rusqlite::params![info_hash],
        )
        .context(error::DatabaseSnafu)?;
        tx.execute(
            "DELETE FROM favourites WHERE info_hash = ?1",
            rusqlite::params![info_hash],
        )
        .context(error::DatabaseSnafu)?;
        tx.execute(
            "DELETE FROM playlist_tracks WHERE info_hash = ?1",
            rusqlite::params![info_hash],
        )
        .context(error::DatabaseSnafu)?;
        tx.execute(
            "DELETE FROM media_metadata WHERE info_hash = ?1",
            rusqlite::params![info_hash],
        )
        .context(error::DatabaseSnafu)?;
        tx.execute(
            "DELETE FROM downloads WHERE info_hash = ?1",
            rusqlite::params![info_hash],
        )
        .context(error::DatabaseSnafu)?;
        tx.commit().context(error::DatabaseSnafu)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> (Database, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let db =
            Database::open(&tmp.path().join("test.db")).unwrap_or_else(|e| panic!("open: {e}"));
        db.init().await.unwrap_or_else(|e| panic!("init: {e}"));
        (db, tmp)
    }

    fn sample(hash: &str) -> Download {
        Download {
            info_hash: hash.to_string(),
            magnet_uri: format!("magnet:?xt=urn:btih:{hash}&dn=test"),
            title: "Test Movie".to_string(),
            file_name: "test.mkv".to_string(),
            file_index: 0,
            file_size: 100,
            download_all: false,
            status: "downloading".to_string(),
            progress: 42.0,
            partial_path: None,
            complete_path: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            files_json: None,
            pinned: false,
        }
    }

    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    async fn seed_related_rows(db: &Database, hash: &str, magnet: &str) {
        let conn = db.connection().lock().await;
        conn.execute(
            "INSERT OR IGNORE INTO users (id, username, password_hash, created_at, is_admin) \
             VALUES ('u1', 'tester', 'x', '2026-01-01', 1)",
            [],
        )
        .unwrap_or_else(|e| panic!("seed users: {e}"));
        conn.execute(
            "INSERT INTO watch_history (id, user_id, magnet_uri, title, watched_at) \
             VALUES ('wh1', 'u1', ?1, 'Test Movie', '2026-01-01')",
            rusqlite::params![magnet],
        )
        .unwrap_or_else(|e| panic!("seed watch_history: {e}"));
        conn.execute(
            "INSERT INTO favourites (id, user_id, content_type, title, info_hash, created_at) \
             VALUES ('f1', 'u1', 'movie', 'Test Movie', ?1, '2026-01-01')",
            rusqlite::params![hash],
        )
        .unwrap_or_else(|e| panic!("seed favourites: {e}"));
        conn.execute(
            "INSERT INTO playlists (id, user_id, name, created_at, updated_at) \
             VALUES ('p1', 'u1', 'My list', '2026-01-01', '2026-01-01')",
            [],
        )
        .unwrap_or_else(|e| panic!("seed playlists: {e}"));
        conn.execute(
            "INSERT INTO playlist_tracks (id, playlist_id, info_hash, file_index, title, position, created_at) \
             VALUES ('t1', 'p1', ?1, 0, 'Track', 0, '2026-01-01')",
            rusqlite::params![hash],
        )
        .unwrap_or_else(|e| panic!("seed playlist_tracks: {e}"));
        conn.execute(
            "INSERT INTO media_metadata (info_hash, title, created_at) \
             VALUES (?1, 'Test Movie', '2026-01-01')",
            rusqlite::params![hash],
        )
        .unwrap_or_else(|e| panic!("seed media_metadata: {e}"));
    }

    async fn count(db: &Database, table: &str, col: &str, val: &str) -> i64 {
        let conn = db.connection().lock().await;
        conn.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE {col} = ?1"),
            rusqlite::params![val],
            |r| r.get(0),
        )
        .unwrap_or_else(|e| panic!("count {table}: {e}"))
    }

    #[tokio::test]
    async fn delete_download_removes_all_related_rows() {
        let (db, _tmp) = test_db().await;
        let dl = sample(HASH);
        db.upsert_download(&dl)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        seed_related_rows(&db, HASH, &dl.magnet_uri).await;

        db.delete_download(HASH)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(count(&db, "downloads", "info_hash", HASH).await, 0);
        assert_eq!(count(&db, "media_metadata", "info_hash", HASH).await, 0);
        assert_eq!(
            count(&db, "watch_history", "magnet_uri", &dl.magnet_uri).await,
            0
        );
        assert_eq!(count(&db, "favourites", "info_hash", HASH).await, 0);
        assert_eq!(count(&db, "playlist_tracks", "info_hash", HASH).await, 0);
    }

    #[tokio::test]
    async fn delete_download_leaves_unrelated_rows() {
        let (db, _tmp) = test_db().await;
        let other = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let dl_a = sample(HASH);
        let dl_b = sample(other);
        db.upsert_download(&dl_a)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        db.upsert_download(&dl_b)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        seed_related_rows(&db, other, &dl_b.magnet_uri).await;

        db.delete_download(HASH)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(count(&db, "downloads", "info_hash", other).await, 1);
        assert_eq!(
            count(&db, "watch_history", "magnet_uri", &dl_b.magnet_uri).await,
            1
        );
        assert_eq!(count(&db, "favourites", "info_hash", other).await, 1);
        assert_eq!(count(&db, "playlist_tracks", "info_hash", other).await, 1);
        assert_eq!(count(&db, "media_metadata", "info_hash", other).await, 1);
    }

    #[tokio::test]
    async fn delete_download_missing_hash_is_noop() {
        let (db, _tmp) = test_db().await;
        db.delete_download("ffffffffffffffffffffffffffffffffffffffff")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    }

    #[tokio::test]
    async fn pinned_flag_roundtrip_and_boot_list() {
        let (db, _tmp) = test_db().await;
        let mut dl = sample(HASH);
        db.upsert_download(&dl)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let complete_hash = "cccccccccccccccccccccccccccccccccccccccc";
        dl.info_hash = complete_hash.to_string();
        dl.status = "complete".to_string();
        db.upsert_download(&dl)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        db.set_download_pinned(HASH, true)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        db.set_download_pinned(complete_hash, true)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let got = db
            .get_download(HASH)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(got.map(|d| d.pinned).unwrap_or(false));

        // Only incomplete pinned downloads are resumed at boot.
        let boot = db
            .get_pinned_incomplete()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(boot, vec![HASH.to_string()]);

        db.set_download_pinned(HASH, false)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let boot = db
            .get_pinned_incomplete()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(boot.is_empty());
    }
}
