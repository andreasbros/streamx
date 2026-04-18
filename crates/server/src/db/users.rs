use crate::db::Database;
use crate::error::{self, Result};
use chrono::Utc;
use snafu::ResultExt;
use uuid::Uuid;

pub use streamx_api::types::User;

impl Database {
    pub async fn create_user(&self, username: &str, password_hash: &str) -> Result<User> {
        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        let is_admin = self.user_count().await? == 0;
        let username_lower = username.to_lowercase();

        let user = User {
            id: id.clone(),
            username: username_lower.clone(),
            password_hash: password_hash.to_string(),
            created_at: created_at.clone(),
            is_admin,
        };

        let conn = self.connection().lock().await;
        conn.execute(
            "INSERT INTO users (id, username, password_hash, created_at, is_admin) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, username_lower, password_hash, created_at, is_admin as i32],
        )
        .context(error::DatabaseSnafu)?;

        Ok(user)
    }

    pub async fn find_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare("SELECT id, username, password_hash, created_at, is_admin FROM users WHERE username = ?1")
            .context(error::DatabaseSnafu)?;

        let result = stmt.query_row(rusqlite::params![username], |row| {
            Ok(User {
                id: row.get(0)?,
                username: row.get(1)?,
                password_hash: row.get(2)?,
                created_at: row.get(3)?,
                is_admin: row.get::<_, i32>(4)? != 0,
            })
        });

        match result {
            Ok(user) => Ok(Some(user)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context(error::DatabaseSnafu),
        }
    }

    pub async fn find_user_by_id(&self, id: &str) -> Result<Option<User>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, username, password_hash, created_at, is_admin FROM users WHERE id = ?1",
            )
            .context(error::DatabaseSnafu)?;

        let result = stmt.query_row(rusqlite::params![id], |row| {
            Ok(User {
                id: row.get(0)?,
                username: row.get(1)?,
                password_hash: row.get(2)?,
                created_at: row.get(3)?,
                is_admin: row.get::<_, i32>(4)? != 0,
            })
        });

        match result {
            Ok(user) => Ok(Some(user)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context(error::DatabaseSnafu),
        }
    }

    pub async fn user_count(&self) -> Result<i64> {
        let conn = self.connection().lock().await;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
            .context(error::DatabaseSnafu)?;
        Ok(count)
    }
}
