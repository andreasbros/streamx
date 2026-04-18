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
    ) -> Result<WatchEntry> {
        let id = Uuid::new_v4().to_string();
        let watched_at = Utc::now().to_rfc3339();

        let conn = self.connection().lock().await;
        conn.execute(
            "INSERT INTO watch_history (id, user_id, magnet_uri, title, file_name, watched_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, user_id, magnet_uri, title, file_name, watched_at],
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
            poster_url: None,
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
