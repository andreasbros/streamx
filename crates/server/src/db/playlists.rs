use crate::db::Database;
use crate::error::{self, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub track_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTrack {
    pub id: String,
    pub playlist_id: String,
    pub info_hash: String,
    pub file_index: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_seconds: Option<i64>,
    pub artwork_url: Option<String>,
    pub position: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct AddTrackRequest {
    pub info_hash: String,
    pub file_index: Option<i64>,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_seconds: Option<i64>,
    pub artwork_url: Option<String>,
}

impl Database {
    pub async fn create_playlist(&self, user_id: &str, name: &str) -> Result<Playlist> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let conn = self.connection().lock().await;
        conn.execute(
            "INSERT INTO playlists (id, user_id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, user_id, name, now, now],
        )
        .context(error::DatabaseSnafu)?;

        Ok(Playlist {
            id,
            user_id: user_id.to_string(),
            name: name.to_string(),
            track_count: 0,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub async fn get_playlists(&self, user_id: &str) -> Result<Vec<Playlist>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.user_id, p.name, p.created_at, p.updated_at, \
                 (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = p.id) as track_count \
                 FROM playlists p WHERE p.user_id = ?1 ORDER BY p.updated_at DESC",
            )
            .context(error::DatabaseSnafu)?;

        let rows = stmt
            .query_map(rusqlite::params![user_id], |row| {
                Ok(Playlist {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    track_count: row.get(5)?,
                })
            })
            .context(error::DatabaseSnafu)?;

        let mut playlists = Vec::new();
        for row in rows {
            playlists.push(row.context(error::DatabaseSnafu)?);
        }
        Ok(playlists)
    }

    pub async fn rename_playlist(&self, id: &str, user_id: &str, name: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3 AND user_id = ?4",
            rusqlite::params![name, now, id, user_id],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn delete_playlist(&self, id: &str, user_id: &str) -> Result<()> {
        let conn = self.connection().lock().await;
        conn.execute(
            "DELETE FROM playlists WHERE id = ?1 AND user_id = ?2",
            rusqlite::params![id, user_id],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn get_playlist_tracks(&self, playlist_id: &str) -> Result<Vec<PlaylistTrack>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, playlist_id, info_hash, file_index, title, artist, album, \
                 duration_seconds, artwork_url, position, created_at \
                 FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position ASC",
            )
            .context(error::DatabaseSnafu)?;

        let rows = stmt
            .query_map(rusqlite::params![playlist_id], |row| {
                Ok(PlaylistTrack {
                    id: row.get(0)?,
                    playlist_id: row.get(1)?,
                    info_hash: row.get(2)?,
                    file_index: row.get(3)?,
                    title: row.get(4)?,
                    artist: row.get(5)?,
                    album: row.get(6)?,
                    duration_seconds: row.get(7)?,
                    artwork_url: row.get(8)?,
                    position: row.get(9)?,
                    created_at: row.get(10)?,
                })
            })
            .context(error::DatabaseSnafu)?;

        let mut tracks = Vec::new();
        for row in rows {
            tracks.push(row.context(error::DatabaseSnafu)?);
        }
        Ok(tracks)
    }

    pub async fn add_playlist_track(
        &self,
        playlist_id: &str,
        req: &AddTrackRequest,
    ) -> Result<PlaylistTrack> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let conn = self.connection().lock().await;

        // Get the next position
        let max_pos: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) FROM playlist_tracks WHERE playlist_id = ?1",
                rusqlite::params![playlist_id],
                |row| row.get(0),
            )
            .context(error::DatabaseSnafu)?;
        let position = max_pos + 1;

        conn.execute(
            "INSERT INTO playlist_tracks (id, playlist_id, info_hash, file_index, title, artist, album, \
             duration_seconds, artwork_url, position, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                id,
                playlist_id,
                req.info_hash,
                req.file_index.unwrap_or(0),
                req.title,
                req.artist,
                req.album,
                req.duration_seconds,
                req.artwork_url,
                position,
                now,
            ],
        )
        .context(error::DatabaseSnafu)?;

        // Touch playlist updated_at
        let _ = conn.execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, playlist_id],
        );

        Ok(PlaylistTrack {
            id,
            playlist_id: playlist_id.to_string(),
            info_hash: req.info_hash.clone(),
            file_index: req.file_index.unwrap_or(0),
            title: req.title.clone(),
            artist: req.artist.clone(),
            album: req.album.clone(),
            duration_seconds: req.duration_seconds,
            artwork_url: req.artwork_url.clone(),
            position,
            created_at: now,
        })
    }

    pub async fn remove_playlist_track(&self, track_id: &str, user_id: &str) -> Result<()> {
        let conn = self.connection().lock().await;
        conn.execute(
            "DELETE FROM playlist_tracks WHERE id = ?1 AND playlist_id IN \
             (SELECT id FROM playlists WHERE user_id = ?2)",
            rusqlite::params![track_id, user_id],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }
}
