use crate::db::Database;
use crate::error::{self, Result};
use chrono::Utc;
use serde::Serialize;
use snafu::ResultExt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct SearchEntry {
    pub id: String,
    pub user_id: String,
    pub query: String,
    pub result_count: Option<i32>,
    pub searched_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchEntry {
    pub id: String,
    pub user_id: String,
    pub magnet_uri: String,
    pub title: String,
    pub file_name: Option<String>,
    pub duration_seconds: Option<i64>,
    pub watched_seconds: Option<i64>,
    pub poster_url: Option<String>,
    pub watched_at: String,
}

/// Enriched watch entry that includes info_hash and title from the downloads table.
#[derive(Debug, Clone, Serialize)]
pub struct EnrichedWatchEntry {
    pub id: String,
    pub magnet_uri: String,
    pub title: String,
    pub file_name: Option<String>,
    pub duration_seconds: Option<i64>,
    pub watched_seconds: Option<i64>,
    pub poster_url: Option<String>,
    pub watched_at: String,
    pub info_hash: Option<String>,
    pub file_size: Option<i64>,
    pub year: Option<i32>,
    pub rating: Option<f64>,
    pub runtime: Option<i32>,
    pub genres: Option<String>,
    pub summary: Option<String>,
    pub imdb_code: Option<String>,
}

impl Database {
    pub async fn add_search(
        &self,
        user_id: &str,
        query: &str,
        result_count: i32,
    ) -> Result<SearchEntry> {
        let id = Uuid::new_v4().to_string();
        let searched_at = Utc::now().to_rfc3339();

        let conn = self.connection().lock().await;
        conn.execute(
            "INSERT INTO search_history (id, user_id, query, result_count, searched_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, user_id, query, result_count, searched_at],
        )
        .context(error::DatabaseSnafu)?;

        Ok(SearchEntry {
            id,
            user_id: user_id.to_string(),
            query: query.to_string(),
            result_count: Some(result_count),
            searched_at,
        })
    }

    pub async fn get_search_history(&self, user_id: &str) -> Result<Vec<SearchEntry>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, query, result_count, searched_at \
                 FROM search_history WHERE user_id = ?1 \
                 ORDER BY searched_at DESC LIMIT 100",
            )
            .context(error::DatabaseSnafu)?;

        let entries = stmt
            .query_map(rusqlite::params![user_id], |row| {
                Ok(SearchEntry {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    query: row.get(2)?,
                    result_count: row.get(3)?,
                    searched_at: row.get(4)?,
                })
            })
            .context(error::DatabaseSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::DatabaseSnafu)?;

        Ok(entries)
    }

    pub async fn add_watch(
        &self,
        user_id: &str,
        magnet_uri: &str,
        title: &str,
        file_name: Option<&str>,
        poster_url: Option<&str>,
    ) -> Result<WatchEntry> {
        let id = Uuid::new_v4().to_string();
        let watched_at = Utc::now().to_rfc3339();

        let conn = self.connection().lock().await;
        conn.execute(
            "INSERT INTO watch_history (id, user_id, magnet_uri, title, file_name, poster_url, watched_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, user_id, magnet_uri, title, file_name, poster_url, watched_at],
        )
        .context(error::DatabaseSnafu)?;

        Ok(WatchEntry {
            id,
            user_id: user_id.to_string(),
            magnet_uri: magnet_uri.to_string(),
            title: title.to_string(),
            file_name: file_name.map(String::from),
            duration_seconds: None,
            watched_seconds: None,
            poster_url: poster_url.map(String::from),
            watched_at,
        })
    }

    pub async fn update_watch_position(&self, id: &str, watched_seconds: i64) -> Result<()> {
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE watch_history SET watched_seconds = ?1 WHERE id = ?2",
            rusqlite::params![watched_seconds, id],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn get_watch_history(&self, user_id: &str) -> Result<Vec<WatchEntry>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, magnet_uri, title, file_name, \
                 duration_seconds, watched_seconds, poster_url, watched_at \
                 FROM watch_history WHERE user_id = ?1 \
                 ORDER BY watched_at DESC LIMIT 100",
            )
            .context(error::DatabaseSnafu)?;

        let entries = stmt
            .query_map(rusqlite::params![user_id], |row| {
                Ok(WatchEntry {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    magnet_uri: row.get(2)?,
                    title: row.get(3)?,
                    file_name: row.get(4)?,
                    duration_seconds: row.get(5)?,
                    watched_seconds: row.get(6)?,
                    poster_url: row.get(7)?,
                    watched_at: row.get(8)?,
                })
            })
            .context(error::DatabaseSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::DatabaseSnafu)?;

        Ok(entries)
    }

    pub async fn get_watch_history_enriched(
        &self,
        user_id: &str,
    ) -> Result<Vec<EnrichedWatchEntry>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT w.id, w.magnet_uri, w.title, w.file_name, \
                 w.duration_seconds, w.watched_seconds, w.poster_url, w.watched_at, \
                 d.info_hash, d.title AS dl_title, d.file_name AS dl_file_name, d.file_size, \
                 m.local_poster, m.year, m.rating, m.runtime, m.genres, m.summary, m.imdb_code \
                 FROM ( \
                   SELECT *, ROW_NUMBER() OVER (PARTITION BY magnet_uri ORDER BY watched_at DESC) AS rn \
                   FROM watch_history WHERE user_id = ?1 \
                 ) w \
                 LEFT JOIN downloads d ON w.magnet_uri = d.magnet_uri \
                 LEFT JOIN media_metadata m ON d.info_hash = m.info_hash \
                 WHERE w.rn = 1 \
                 ORDER BY w.watched_at DESC LIMIT 100",
            )
            .context(error::DatabaseSnafu)?;

        let entries = stmt
            .query_map(rusqlite::params![user_id], |row| {
                let watch_title: String = row.get(2)?;
                let dl_title: Option<String> = row.get(9)?;
                let watch_file_name: Option<String> = row.get(3)?;
                let dl_file_name: Option<String> = row.get(10)?;
                let info_hash: Option<String> = row.get(8)?;

                // Prefer download title over watch_history title (which may be the info_hash)
                let title = match &dl_title {
                    Some(t) if !t.is_empty() => t.clone(),
                    _ => watch_title,
                };

                // Prefer download file_name over watch_history file_name
                let file_name = match &dl_file_name {
                    Some(f) if !f.is_empty() => Some(f.clone()),
                    _ => watch_file_name,
                };

                // Prefer local poster over remote poster URL
                let watch_poster: Option<String> = row.get(6)?;
                let local_poster: Option<String> = row.get(12)?;
                let poster_url = local_poster.or(watch_poster);

                Ok(EnrichedWatchEntry {
                    id: row.get(0)?,
                    magnet_uri: row.get(1)?,
                    title,
                    file_name,
                    duration_seconds: row.get(4)?,
                    watched_seconds: row.get(5)?,
                    poster_url,
                    watched_at: row.get(7)?,
                    info_hash,
                    file_size: row.get(11)?,
                    year: row.get(13)?,
                    rating: row.get(14)?,
                    runtime: row.get(15)?,
                    genres: row.get(16)?,
                    summary: row.get(17)?,
                    imdb_code: row.get(18)?,
                })
            })
            .context(error::DatabaseSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::DatabaseSnafu)?;

        Ok(entries)
    }

    pub async fn delete_watch(&self, id: &str) -> Result<()> {
        let conn = self.connection().lock().await;
        conn.execute(
            "DELETE FROM watch_history WHERE id = ?1",
            rusqlite::params![id],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }
}
