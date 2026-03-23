use crate::db::Database;
use crate::error::{self, Result};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub info_hash: String,
    pub title: String,
    pub year: Option<i32>,
    pub rating: Option<f64>,
    pub runtime: Option<i32>,
    pub genres: Option<String>,
    pub language: Option<String>,
    pub mpa_rating: Option<String>,
    pub summary: Option<String>,
    pub imdb_code: Option<String>,
    pub trailer_code: Option<String>,
    pub video_codec: Option<String>,
    pub audio_channels: Option<String>,
    pub bit_depth: Option<String>,
    pub source_type: Option<String>,
    pub poster_small: Option<String>,
    pub poster_medium: Option<String>,
    pub poster_large: Option<String>,
    pub backdrop: Option<String>,
    pub local_poster: Option<String>,
    pub created_at: String,
}

impl Database {
    pub async fn upsert_metadata(&self, meta: &MediaMetadata) -> Result<()> {
        let conn = self.connection().lock().await;
        conn.execute(
            "INSERT INTO media_metadata (info_hash, title, year, rating, runtime, genres, \
             language, mpa_rating, summary, imdb_code, trailer_code, video_codec, \
             audio_channels, bit_depth, source_type, poster_small, poster_medium, \
             poster_large, backdrop, local_poster, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21) \
             ON CONFLICT(info_hash) DO UPDATE SET \
             title = ?2, year = ?3, rating = ?4, runtime = ?5, genres = ?6, \
             language = ?7, mpa_rating = ?8, summary = ?9, imdb_code = ?10, \
             trailer_code = ?11, video_codec = ?12, audio_channels = ?13, \
             bit_depth = ?14, source_type = ?15, poster_small = ?16, \
             poster_medium = ?17, poster_large = ?18, backdrop = ?19, \
             local_poster = COALESCE(?20, local_poster)",
            rusqlite::params![
                meta.info_hash,
                meta.title,
                meta.year,
                meta.rating,
                meta.runtime,
                meta.genres,
                meta.language,
                meta.mpa_rating,
                meta.summary,
                meta.imdb_code,
                meta.trailer_code,
                meta.video_codec,
                meta.audio_channels,
                meta.bit_depth,
                meta.source_type,
                meta.poster_small,
                meta.poster_medium,
                meta.poster_large,
                meta.backdrop,
                meta.local_poster,
                meta.created_at,
            ],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }

    pub async fn get_metadata(&self, info_hash: &str) -> Result<Option<MediaMetadata>> {
        let conn = self.connection().lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT info_hash, title, year, rating, runtime, genres, language, \
                 mpa_rating, summary, imdb_code, trailer_code, video_codec, \
                 audio_channels, bit_depth, source_type, poster_small, poster_medium, \
                 poster_large, backdrop, local_poster, created_at \
                 FROM media_metadata WHERE info_hash = ?1",
            )
            .context(error::DatabaseSnafu)?;

        let result = stmt.query_row(rusqlite::params![info_hash], |row| {
            Ok(MediaMetadata {
                info_hash: row.get(0)?,
                title: row.get(1)?,
                year: row.get(2)?,
                rating: row.get(3)?,
                runtime: row.get(4)?,
                genres: row.get(5)?,
                language: row.get(6)?,
                mpa_rating: row.get(7)?,
                summary: row.get(8)?,
                imdb_code: row.get(9)?,
                trailer_code: row.get(10)?,
                video_codec: row.get(11)?,
                audio_channels: row.get(12)?,
                bit_depth: row.get(13)?,
                source_type: row.get(14)?,
                poster_small: row.get(15)?,
                poster_medium: row.get(16)?,
                poster_large: row.get(17)?,
                backdrop: row.get(18)?,
                local_poster: row.get(19)?,
                created_at: row.get(20)?,
            })
        });

        match result {
            Ok(m) => Ok(Some(m)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context(error::DatabaseSnafu),
        }
    }

    pub async fn update_local_poster(&self, info_hash: &str, local_poster: &str) -> Result<()> {
        let conn = self.connection().lock().await;
        conn.execute(
            "UPDATE media_metadata SET local_poster = ?1 WHERE info_hash = ?2",
            rusqlite::params![local_poster, info_hash],
        )
        .context(error::DatabaseSnafu)?;
        Ok(())
    }
}
