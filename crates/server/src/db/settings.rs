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

/// Server-wide settings, admin-managed. `disable_transcode` defaults to
/// true: non-WEB-compatible movies are not transcoded server-side and the
/// UI hides their Play button. `web_only` restricts search results and
/// new downloads to WEB source releases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettings {
    pub disable_transcode: bool,
    pub web_only: bool,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            disable_transcode: true,
            web_only: false,
        }
    }
}

impl Database {
    async fn get_app_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.connection().lock().await;
        let result = conn.query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context(error::DatabaseSnafu),
        }
    }

    async fn set_app_setting(&self, key: &str, value: &str) -> Result<()> {
        let updated_at = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
            rusqlite::params![key, value, updated_at],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn get_server_settings(&self) -> Result<ServerSettings> {
        let defaults = ServerSettings::default();
        let parse = |v: Option<String>, default: bool| v.map(|s| s == "true").unwrap_or(default);
        Ok(ServerSettings {
            disable_transcode: parse(
                self.get_app_setting("disable_transcode").await?,
                defaults.disable_transcode,
            ),
            web_only: parse(self.get_app_setting("web_only").await?, defaults.web_only),
        })
    }

    pub async fn set_server_settings(&self, settings: &ServerSettings) -> Result<()> {
        self.set_app_setting(
            "disable_transcode",
            if settings.disable_transcode {
                "true"
            } else {
                "false"
            },
        )
        .await?;
        self.set_app_setting("web_only", if settings.web_only { "true" } else { "false" })
            .await?;
        Ok(())
    }

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
