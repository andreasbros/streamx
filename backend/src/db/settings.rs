use crate::db::Database;
use crate::error::{self, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    pub theme: String,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
        }
    }
}

impl Database {
    pub async fn get_settings(&self, user_id: &str) -> Result<UserSettings> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare("SELECT theme FROM user_settings WHERE user_id = ?1")
            .context(error::DatabaseSnafu)?;

        let result = stmt.query_row(rusqlite::params![user_id], |row| {
            Ok(UserSettings { theme: row.get(0)? })
        });

        match result {
            Ok(settings) => Ok(settings),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(UserSettings::default()),
            Err(e) => Err(e).context(error::DatabaseSnafu),
        }
    }

    pub async fn update_settings(&self, user_id: &str, settings: &UserSettings) -> Result<()> {
        let updated_at = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "INSERT INTO user_settings (user_id, theme, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(user_id) DO UPDATE SET theme = ?2, updated_at = ?3",
            rusqlite::params![user_id, settings.theme, updated_at],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }
}
