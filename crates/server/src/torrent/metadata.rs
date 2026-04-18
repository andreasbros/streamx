use dashmap::DashMap;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::Result;

const CACHE_TTL: Duration = Duration::from_secs(3600);
const BASE_URL: &str = "https://v3-cinemeta.strem.io";

#[derive(Debug, Deserialize)]
struct CatalogResponse {
    metas: Option<Vec<CinemetaMeta>>,
}

#[derive(Debug, Deserialize)]
struct DetailResponse {
    meta: Option<CinemetaMeta>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CinemetaMeta {
    pub id: String,
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    pub name: Option<String>,
    pub poster: Option<String>,
    pub background: Option<String>,
    #[serde(rename = "releaseInfo")]
    pub release_info: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "imdbRating")]
    pub imdb_rating: Option<String>,
    pub genres: Option<Vec<String>>,
    pub runtime: Option<String>,
}

impl CinemetaMeta {
    pub fn parse_year(&self) -> Option<u32> {
        self.release_info
            .as_deref()
            .and_then(|r| r.chars().take(4).collect::<String>().parse().ok())
    }

    pub fn parse_rating(&self) -> Option<f64> {
        self.imdb_rating
            .as_deref()
            .filter(|r| !r.is_empty())
            .and_then(|r| r.parse().ok())
    }

    pub fn parse_runtime(&self) -> Option<u32> {
        self.runtime
            .as_deref()
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse().ok())
    }

    pub fn genres_list(&self) -> Vec<String> {
        self.genres.clone().unwrap_or_default()
    }

    pub fn title(&self) -> String {
        self.name.clone().unwrap_or_default()
    }
}

pub struct CinemetaClient {
    client: reqwest::Client,
    search_cache: Arc<DashMap<String, (Instant, Vec<CinemetaMeta>)>>,
    detail_cache: Arc<DashMap<String, (Instant, CinemetaMeta)>>,
    catalog_cache: Arc<DashMap<String, (Instant, Vec<CinemetaMeta>)>>,
}

impl CinemetaClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            search_cache: Arc::new(DashMap::new()),
            detail_cache: Arc::new(DashMap::new()),
            catalog_cache: Arc::new(DashMap::new()),
        }
    }

    /// Browse top/popular content from Cinemeta catalog.
    /// `media_type` should be "movie" or "series".
    /// `skip` is the offset for pagination (0, 50, 100, ...).
    pub async fn catalog(&self, media_type: &str, skip: u32) -> Result<Vec<CinemetaMeta>> {
        let cache_key = format!("catalog:{media_type}:{skip}");
        if let Some(entry) = self.catalog_cache.get(&cache_key) {
            if entry.0.elapsed() < CACHE_TTL {
                return Ok(entry.1.clone());
            }
        }

        let url = if skip == 0 {
            format!("{BASE_URL}/catalog/{media_type}/top.json")
        } else {
            format!("{BASE_URL}/catalog/{media_type}/top/skip={skip}.json")
        };

        let response = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("Cinemeta catalog request failed: {err}");
                return Ok(Vec::new());
            }
        };

        let catalog: CatalogResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!("Failed to parse Cinemeta catalog response: {err}");
                return Ok(Vec::new());
            }
        };

        let results = catalog.metas.unwrap_or_default();
        self.catalog_cache
            .insert(cache_key, (Instant::now(), results.clone()));
        Ok(results)
    }

    /// Search for movies or series by text query.
    /// `media_type` should be "movie" or "series".
    pub async fn search(&self, media_type: &str, query: &str) -> Result<Vec<CinemetaMeta>> {
        let cache_key = format!("{media_type}:{}", query.to_lowercase());
        if let Some(entry) = self.search_cache.get(&cache_key) {
            if entry.0.elapsed() < CACHE_TTL {
                return Ok(entry.1.clone());
            }
        }

        let encoded = urlencoding::encode(query);
        let url = format!("{BASE_URL}/catalog/{media_type}/top/search={encoded}.json");

        let response = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("Cinemeta search failed: {err}");
                return Ok(Vec::new());
            }
        };

        let catalog: CatalogResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!("Failed to parse Cinemeta search response: {err}");
                return Ok(Vec::new());
            }
        };

        let results = catalog.metas.unwrap_or_default();
        self.search_cache
            .insert(cache_key, (Instant::now(), results.clone()));
        Ok(results)
    }

    /// Get detailed metadata for a specific IMDB ID.
    /// `media_type` should be "movie" or "series".
    pub async fn get_detail(
        &self,
        media_type: &str,
        imdb_id: &str,
    ) -> Result<Option<CinemetaMeta>> {
        let cache_key = format!("{media_type}:{imdb_id}");
        if let Some(entry) = self.detail_cache.get(&cache_key) {
            if entry.0.elapsed() < CACHE_TTL {
                return Ok(Some(entry.1.clone()));
            }
        }

        let url = format!("{BASE_URL}/meta/{media_type}/{imdb_id}.json");

        let response = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("Cinemeta detail request failed: {err}");
                return Ok(None);
            }
        };

        let detail: DetailResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!("Failed to parse Cinemeta detail response: {err}");
                return Ok(None);
            }
        };

        if let Some(meta) = detail.meta {
            self.detail_cache
                .insert(cache_key, (Instant::now(), meta.clone()));
            Ok(Some(meta))
        } else {
            Ok(None)
        }
    }
}
