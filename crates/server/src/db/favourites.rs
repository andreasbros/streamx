use crate::db::Database;
use crate::error::{self, Result};
use chrono::Utc;
use snafu::ResultExt;
use uuid::Uuid;

pub use streamx_api::types::{AddFavouriteRequest, FavouriteItem};

impl Database {
    pub async fn add_favourite(
        &self,
        user_id: &str,
        req: &AddFavouriteRequest,
    ) -> Result<FavouriteItem> {
        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        let content_type = req.content_type.as_deref().unwrap_or("movie");

        let conn = self.connection().lock().await;
        conn.execute(
            "INSERT INTO favourites (id, user_id, content_type, title, year, rating, poster_url, info_hash, metadata_json, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                user_id,
                content_type,
                req.title,
                req.year,
                req.rating,
                req.poster_url,
                req.info_hash,
                req.metadata_json,
                created_at,
            ],
        )
        .context(error::DatabaseSnafu)?;

        Ok(FavouriteItem {
            id,
            user_id: user_id.to_string(),
            content_type: content_type.to_string(),
            title: req.title.clone(),
            year: req.year,
            rating: req.rating,
            poster_url: req.poster_url.clone(),
            info_hash: req.info_hash.clone(),
            metadata_json: req.metadata_json.clone(),
            created_at,
        })
    }

    pub async fn get_favourites(
        &self,
        user_id: &str,
        content_type: Option<&str>,
    ) -> Result<Vec<FavouriteItem>> {
        let conn = self.connection().lock().await;

        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match content_type {
            Some(ct) => (
                "SELECT id, user_id, content_type, title, year, rating, poster_url, info_hash, metadata_json, created_at \
                 FROM favourites WHERE user_id = ?1 AND content_type = ?2 \
                 ORDER BY created_at DESC LIMIT 500",
                vec![
                    Box::new(user_id.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(ct.to_string()),
                ],
            ),
            None => (
                "SELECT id, user_id, content_type, title, year, rating, poster_url, info_hash, metadata_json, created_at \
                 FROM favourites WHERE user_id = ?1 \
                 ORDER BY created_at DESC LIMIT 500",
                vec![Box::new(user_id.to_string()) as Box<dyn rusqlite::types::ToSql>],
            ),
        };

        let mut stmt = conn.prepare(sql).context(error::DatabaseSnafu)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let entries = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(FavouriteItem {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    content_type: row.get(2)?,
                    title: row.get(3)?,
                    year: row.get(4)?,
                    rating: row.get(5)?,
                    poster_url: row.get(6)?,
                    info_hash: row.get(7)?,
                    metadata_json: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })
            .context(error::DatabaseSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::DatabaseSnafu)?;

        Ok(entries)
    }

    pub async fn delete_favourite(&self, id: &str, user_id: &str) -> Result<bool> {
        let conn = self.connection().lock().await;
        let rows = conn
            .execute(
                "DELETE FROM favourites WHERE id = ?1 AND user_id = ?2",
                rusqlite::params![id, user_id],
            )
            .context(error::DatabaseSnafu)?;
        Ok(rows > 0)
    }
}
