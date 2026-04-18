use crate::error::Result;
use serde::{Deserialize, Serialize};

const YTS_API_URL: &str = "https://yts.lt/api/v2/list_movies.json";

const TRACKERS: &[&str] = &[
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.openbittorrent.com:80",
    "udp://tracker.coppersurfer.tk:6969",
    "udp://glotorrents.pw:6969/announce",
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://torrent.gresille.org:80/announce",
    "udp://p4p.arenabg.com:1337",
    "udp://tracker.leechers-paradise.org:6969",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub magnet: String,
    pub seeds: u32,
    pub leeches: u32,
    pub size: String,
    pub size_bytes: u64,
    pub quality: Option<String>,
    pub year: Option<u32>,
    pub rating: Option<f64>,
    pub poster: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YtsResponse {
    status: String,
    data: Option<YtsData>,
}

#[derive(Debug, Deserialize)]
struct YtsData {
    movies: Option<Vec<YtsMovie>>,
}

#[derive(Debug, Deserialize)]
struct YtsMovie {
    title: String,
    year: u32,
    rating: f64,
    medium_cover_image: Option<String>,
    torrents: Option<Vec<YtsTorrent>>,
}

#[derive(Debug, Deserialize)]
struct YtsTorrent {
    hash: String,
    quality: String,
    seeds: u32,
    peers: u32,
    size: String,
    size_bytes: u64,
}

pub struct SearchProvider {
    client: reqwest::Client,
}

impl Default for SearchProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) StreamX/0.1")
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        let response = self
            .client
            .get(YTS_API_URL)
            .query(&[("query_term", query), ("sort_by", "seeds"), ("limit", "20")])
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("YTS API request failed: {err}");
                return Ok(Vec::new());
            }
        };

        let yts: YtsResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!("Failed to parse YTS response: {err}");
                return Ok(Vec::new());
            }
        };

        if yts.status != "ok" {
            tracing::warn!("YTS API returned non-ok status: {}", yts.status);
            return Ok(Vec::new());
        }

        let movies = match yts.data.and_then(|d| d.movies) {
            Some(m) => m,
            None => return Ok(Vec::new()),
        };

        let mut results: Vec<SearchResult> = movies
            .into_iter()
            .flat_map(|movie| {
                let torrents = movie.torrents.unwrap_or_default();
                torrents.into_iter().map({
                    let title = movie.title.clone();
                    let year = movie.year;
                    let rating = movie.rating;
                    let poster = movie.medium_cover_image.clone();
                    move |torrent| {
                        let display_title =
                            format!("{title} ({year}) [{quality}]", quality = torrent.quality);
                        let magnet = build_magnet(&torrent.hash, &display_title);

                        SearchResult {
                            title: display_title,
                            magnet,
                            seeds: torrent.seeds,
                            leeches: torrent.peers,
                            size: torrent.size.clone(),
                            size_bytes: torrent.size_bytes,
                            quality: Some(torrent.quality),
                            year: Some(year),
                            rating: Some(rating),
                            poster: poster.clone(),
                        }
                    }
                })
            })
            .collect();

        results.sort_by(|a, b| b.seeds.cmp(&a.seeds));

        Ok(results)
    }
}

fn build_magnet(hash: &str, title: &str) -> String {
    let encoded_title = urlencoding::encode(title);
    let trackers: String = TRACKERS
        .iter()
        .map(|t| format!("&tr={}", urlencoding::encode(t)))
        .collect();
    format!("magnet:?xt=urn:btih:{hash}&dn={encoded_title}{trackers}")
}
