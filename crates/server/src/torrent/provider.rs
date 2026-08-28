use crate::config::ProviderConfig;
use crate::error::Result;
use crate::server::proxy::{self, CINEMETA_PROXY_ID};
use crate::torrent::metadata::CinemetaClient;
use serde::Deserialize;

fn proxy_opt(url: &Option<String>, proxy_id: u32) -> Option<String> {
    url.as_ref().map(|u| proxy::to_proxy_url(u, proxy_id))
}

fn proxy_posters(group: &mut SearchResultGroup, proxy_id: u32) {
    group.poster = proxy_opt(&group.poster, proxy_id);
    group.poster_small = proxy_opt(&group.poster_small, proxy_id);
    group.poster_medium = proxy_opt(&group.poster_medium, proxy_id);
    group.poster_large = proxy_opt(&group.poster_large, proxy_id);
    group.backdrop = proxy_opt(&group.backdrop, proxy_id);
}

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

pub use streamx_api::types::{SearchResult, SearchResultGroup};

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
    runtime: Option<u32>,
    genres: Option<Vec<String>>,
    language: Option<String>,
    mpa_rating: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    imdb_code: Option<String>,
    yt_trailer_code: Option<String>,
    small_cover_image: Option<String>,
    medium_cover_image: Option<String>,
    large_cover_image: Option<String>,
    background_image: Option<String>,
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
    #[serde(rename = "type")]
    source_type: Option<String>,
    video_codec: Option<String>,
    bit_depth: Option<String>,
    audio_channels: Option<String>,
}

pub use streamx_api::types::{
    MusicVideoResult, TvEpisode, TvSearchResultGroup, TvSeason, TvTorrent,
};

#[derive(Debug, Deserialize)]
struct ApibayTorrent {
    name: String,
    info_hash: String,
    seeders: String,
    leechers: String,
    size: String,
    added: String,
    #[allow(dead_code)]
    category: String,
}

#[derive(Debug, Deserialize)]
struct EztvResponse {
    #[allow(dead_code)]
    torrents_count: Option<u32>,
    torrents: Option<Vec<EztvTorrent>>,
}

#[derive(Debug, Deserialize)]
struct EztvTorrent {
    title: String,
    imdb_id: Option<String>,
    season: Option<String>,
    episode: Option<String>,
    magnet_url: String,
    seeds: u32,
    peers: u32,
    size_bytes: Option<String>,
    filename: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TorrentioResponse {
    streams: Option<Vec<TorrentioStream>>,
}

#[derive(Debug, Deserialize)]
struct TorrentioStream {
    #[allow(dead_code)]
    name: Option<String>,
    title: Option<String>,
    #[serde(rename = "infoHash")]
    info_hash: String,
    #[serde(rename = "fileIdx")]
    #[allow(dead_code)]
    file_idx: Option<usize>,
}

pub struct SearchProvider {
    client: reqwest::Client,
    providers: Vec<ProviderConfig>,
    cinemeta: CinemetaClient,
}

impl Default for SearchProvider {
    fn default() -> Self {
        Self::new(Vec::new(), None)
    }
}

/// reqwest errors hide the interesting part ("operation timed out",
/// "connection refused") in their source chain; flatten it for logs
/// and for messages surfaced to the UIs.
pub fn error_chain(err: &dyn std::error::Error) -> String {
    let mut out = err.to_string();
    let mut cur = err.source();
    while let Some(src) = cur {
        out.push_str(": ");
        out.push_str(&src.to_string());
        cur = src.source();
    }
    out
}

impl SearchProvider {
    pub fn new(providers: Vec<ProviderConfig>, socks5: Option<String>) -> Self {
        // Dead hosts fail fast at connect; slow-but-alive origins get
        // room to answer (the YTS mirror has been seen taking ~20s).
        let mut builder = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) StreamX/0.1");
        if let Some(ref url) = socks5 {
            if let Ok(proxy) = reqwest::Proxy::all(url) {
                builder = builder.proxy(proxy);
            }
        }
        let client = builder.build().unwrap_or_default();
        let cinemeta = CinemetaClient::new(client.clone());
        Self {
            client,
            providers,
            cinemeta,
        }
    }

    fn provider(&self, kind: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.kind == kind)
    }

    fn providers_by_kind(&self, kind: &str) -> Vec<ProviderConfig> {
        self.providers
            .iter()
            .filter(|p| p.kind == kind)
            .cloned()
            .collect()
    }

    /// Parse "provider_name: query" prefix from search query.
    /// Returns (provider_name, actual_query) or (None, original_query).
    fn parse_provider_prefix(query: &str) -> (Option<&str>, &str) {
        if let Some((prefix, rest)) = query.split_once(':') {
            let prefix = prefix.trim();
            let rest = rest.trim();
            if !prefix.is_empty() && !rest.is_empty() && !prefix.contains(' ') {
                return (Some(prefix), rest);
            }
        }
        (None, query)
    }

    fn providers_by_kind_and_name(&self, kind: &str, name: Option<&str>) -> Vec<ProviderConfig> {
        self.providers
            .iter()
            .filter(|p| {
                p.kind == kind
                    && match name {
                        Some(n) => {
                            let n_lower = n.to_lowercase();
                            p.name
                                .as_deref()
                                .map(|pn| pn.to_lowercase() == n_lower)
                                .unwrap_or(false)
                                || p.format
                                    .as_deref()
                                    .map(|f| f.to_lowercase() == n_lower)
                                    .unwrap_or(false)
                        }
                        None => true,
                    }
            })
            .cloned()
            .collect()
    }

    pub async fn search(
        &self,
        query: &str,
        page: u32,
    ) -> Result<streamx_api::types::SearchResponse> {
        use streamx_api::types::{ProviderError, SearchResponse};
        let (prefix, actual_query) = Self::parse_provider_prefix(query);
        let providers = self.providers_by_kind_and_name("movies", prefix);
        if providers.is_empty() {
            return Ok(SearchResponse::default());
        }

        // Each provider gets a 15s budget so one slow upstream cannot
        // hold everyone else's results hostage; a blown budget becomes
        // a visible provider error instead of a silent stall.
        let futs: Vec<_> = providers
            .iter()
            .map(|p| {
                let p = p.clone();
                let q = actual_query.to_string();
                async move {
                    let fmt = p.format.as_deref().unwrap_or("yts");
                    let url = p.url.clone();
                    let res = tokio::time::timeout(std::time::Duration::from_secs(15), async {
                        match fmt {
                            "torrentio" => self.search_torrentio_movies(&q, &p).await,
                            "apibay" => self.search_movies_apibay(&q, &p).await,
                            _ => self.search_yts(&q, &p, page).await,
                        }
                    })
                    .await;
                    (url, res)
                }
            })
            .collect();

        let all_results = futures::future::join_all(futs).await;

        let mut groups: Vec<SearchResultGroup> = Vec::new();
        let mut provider_errors: Vec<ProviderError> = Vec::new();
        for (url, res) in all_results {
            match res {
                Err(_elapsed) => provider_errors.push(ProviderError {
                    url,
                    message: "no response within 15 seconds".to_string(),
                }),
                Ok(Err(err)) => provider_errors.push(ProviderError {
                    url,
                    message: err.to_string(),
                }),
                Ok(Ok(rows)) => groups.extend(rows),
            }
        }

        merge_movie_groups(&mut groups);
        Ok(SearchResponse {
            results: groups,
            provider_errors,
        })
    }

    async fn search_yts(
        &self,
        query: &str,
        provider: &ProviderConfig,
        page: u32,
    ) -> Result<Vec<SearchResultGroup>> {
        let api_url = provider
            .api_url
            .clone()
            .unwrap_or_else(|| format!("{}/api/v2/list_movies.json", provider.url));
        let page_str = page.to_string();
        let response = self
            .client
            .get(&api_url)
            .query(&[
                ("query_term", query),
                ("sort_by", "seeds"),
                ("limit", "20"),
                ("page", &page_str),
            ])
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(err) => {
                let chain = error_chain(&err);
                tracing::warn!("YTS API request failed: {chain}");
                return Err(crate::error::Error::Provider { message: chain });
            }
        };

        let yts: YtsResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                let chain = error_chain(&err);
                tracing::warn!("Failed to parse YTS response: {chain}");
                return Err(crate::error::Error::Provider {
                    message: format!("invalid response: {chain}"),
                });
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

        let mut groups: Vec<SearchResultGroup> = movies
            .into_iter()
            .map(|movie| {
                let title = movie.title.clone();
                let year = movie.year;
                let poster = movie
                    .large_cover_image
                    .clone()
                    .or_else(|| movie.medium_cover_image.clone());

                let torrents = movie.torrents.unwrap_or_default();
                let mut variants: Vec<SearchResult> = torrents
                    .into_iter()
                    .map(|torrent| {
                        let display_title =
                            format!("{title} ({year}) [{quality}]", quality = torrent.quality);
                        let magnet = build_magnet(&torrent.hash, &display_title);

                        SearchResult {
                            magnet,
                            seeds: torrent.seeds,
                            leeches: torrent.peers,
                            size: torrent.size,
                            size_bytes: torrent.size_bytes,
                            quality: Some(torrent.quality),
                            video_codec: torrent.video_codec,
                            audio_channels: torrent.audio_channels,
                            bit_depth: torrent.bit_depth.map(|b| b.to_string()),
                            source_type: torrent.source_type,
                        }
                    })
                    .collect();

                variants.sort_by_key(|v| std::cmp::Reverse(v.seeds));

                SearchResultGroup {
                    title: movie.title,
                    year: Some(year),
                    rating: Some(movie.rating),
                    runtime: movie.runtime,
                    genres: movie.genres.unwrap_or_default(),
                    language: movie.language,
                    mpa_rating: movie.mpa_rating,
                    summary: movie.summary,
                    imdb_code: movie.imdb_code,
                    trailer_code: movie.yt_trailer_code,
                    poster,
                    poster_small: movie.small_cover_image,
                    poster_medium: movie.medium_cover_image,
                    poster_large: movie.large_cover_image,
                    backdrop: movie.background_image,
                    variants,
                }
            })
            .collect();

        groups.sort_by(|a, b| {
            let best_a = a.variants.iter().map(|v| v.seeds).max().unwrap_or(0);
            let best_b = b.variants.iter().map(|v| v.seeds).max().unwrap_or(0);
            best_b.cmp(&best_a)
        });

        for group in &mut groups {
            proxy_posters(group, provider.id);
        }

        Ok(groups)
    }

    /// Public descriptions of the configured providers (no secrets).
    pub fn provider_infos(&self) -> Vec<streamx_api::types::ProviderInfo> {
        self.providers
            .iter()
            .map(|p| streamx_api::types::ProviderInfo {
                name: p.name.clone().unwrap_or_else(|| p.url.clone()),
                kind: p.kind.clone(),
                url: p.url.clone(),
            })
            .collect()
    }

    pub async fn browse(
        &self,
        sort_by: &str,
        query_term: Option<&str>,
        genre: Option<&str>,
        minimum_rating: Option<u32>,
        limit: u32,
        page: u32,
    ) -> Result<streamx_api::types::BrowseResponse> {
        use streamx_api::types::{BrowseResponse, ProviderError};
        // Browse only works with catalog-based providers (not torrentio)
        let provider = match self
            .providers_by_kind("movies")
            .into_iter()
            .find(|p| p.format.as_deref().unwrap_or("yts") != "torrentio")
        {
            Some(p) => p,
            None => return Ok(BrowseResponse::default()),
        };
        let api_url = provider
            .api_url
            .clone()
            .unwrap_or_else(|| format!("{}/api/v2/list_movies.json", provider.url));
        let limit_str = limit.to_string();
        let page_str = page.to_string();
        let mut params: Vec<(&str, &str)> = vec![
            ("sort_by", sort_by),
            ("limit", &limit_str),
            ("page", &page_str),
            ("order_by", "desc"),
        ];
        let rating_str;
        if let Some(r) = minimum_rating {
            rating_str = r.to_string();
            params.push(("minimum_rating", &rating_str));
        }
        if let Some(q) = query_term {
            params.push(("query_term", q));
        }
        if let Some(g) = genre {
            params.push(("genre", g));
        }

        let provider_url = provider.url.clone();
        let fail = |message: String| {
            Ok(BrowseResponse {
                results: Vec::new(),
                provider_errors: vec![ProviderError {
                    url: provider_url.clone(),
                    message,
                }],
            })
        };
        let response = match self.client.get(&api_url).query(&params).send().await {
            Ok(r) => r,
            Err(err) => {
                let chain = error_chain(&err);
                tracing::warn!("YTS browse request failed: {chain}");
                return fail(chain);
            }
        };

        let yts: YtsResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                let chain = error_chain(&err);
                tracing::warn!("Failed to parse YTS browse response: {chain}");
                return fail(format!("invalid response: {chain}"));
            }
        };

        if yts.status != "ok" {
            return fail(format!("provider status: {}", yts.status));
        }

        let movies = match yts.data.and_then(|d| d.movies) {
            Some(m) => m,
            None => return Ok(BrowseResponse::default()),
        };

        let groups: Vec<SearchResultGroup> = movies
            .into_iter()
            .map(|movie| {
                let title = movie.title.clone();
                let year = movie.year;
                let poster = movie
                    .large_cover_image
                    .clone()
                    .or_else(|| movie.medium_cover_image.clone());
                let torrents = movie.torrents.unwrap_or_default();
                let mut variants: Vec<SearchResult> = torrents
                    .into_iter()
                    .map(|torrent| {
                        let display_title =
                            format!("{title} ({year}) [{quality}]", quality = torrent.quality);
                        let magnet = build_magnet(&torrent.hash, &display_title);
                        SearchResult {
                            magnet,
                            seeds: torrent.seeds,
                            leeches: torrent.peers,
                            size: torrent.size,
                            size_bytes: torrent.size_bytes,
                            quality: Some(torrent.quality),
                            video_codec: torrent.video_codec,
                            audio_channels: torrent.audio_channels,
                            bit_depth: torrent.bit_depth.map(|b| b.to_string()),
                            source_type: torrent.source_type,
                        }
                    })
                    .collect();
                variants.sort_by_key(|v| std::cmp::Reverse(v.seeds));
                SearchResultGroup {
                    title: movie.title,
                    year: Some(year),
                    rating: Some(movie.rating),
                    runtime: movie.runtime,
                    genres: movie.genres.unwrap_or_default(),
                    language: movie.language,
                    mpa_rating: movie.mpa_rating,
                    summary: movie.summary,
                    imdb_code: movie.imdb_code,
                    trailer_code: movie.yt_trailer_code,
                    poster,
                    poster_small: movie.small_cover_image,
                    poster_medium: movie.medium_cover_image,
                    poster_large: movie.large_cover_image,
                    backdrop: movie.background_image,
                    variants,
                }
            })
            .collect();

        let mut groups = groups;
        for group in &mut groups {
            proxy_posters(group, provider.id);
        }

        Ok(BrowseResponse {
            results: groups,
            provider_errors: Vec::new(),
        })
    }

    async fn search_movies_apibay(
        &self,
        query: &str,
        provider: &ProviderConfig,
    ) -> Result<Vec<SearchResultGroup>> {
        let cat = provider.category.as_deref().unwrap_or("207");
        let url = format!(
            "{}/q.php?q={}&cat={}",
            provider.url,
            urlencoding::encode(query),
            cat
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| crate::error::Error::Internal {
                message: e.to_string(),
            })?
            .json::<Vec<serde_json::Value>>()
            .await
            .map_err(|e| crate::error::Error::Internal {
                message: e.to_string(),
            })?;

        let mut groups = Vec::new();

        for item in resp.iter().take(100) {
            let name = item["name"].as_str().unwrap_or("");
            let info_hash = item["info_hash"].as_str().unwrap_or("");
            let size = item["size"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let seeders = item["seeders"]
                .as_str()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);

            if name.is_empty()
                || info_hash.is_empty()
                || info_hash == "0000000000000000000000000000000000000000"
            {
                continue;
            }

            let tracker_params: String = provider
                .trackers
                .iter()
                .map(|t| format!("&tr={}", urlencoding::encode(t)))
                .collect();
            let magnet = format!(
                "magnet:?xt=urn:btih:{info_hash}&dn={}{tracker_params}",
                urlencoding::encode(name)
            );

            let size_str = if size > 1_073_741_824 {
                format!("{:.1} GB", size as f64 / 1_073_741_824.0)
            } else {
                format!("{:.1} MB", size as f64 / 1_048_576.0)
            };

            groups.push(SearchResultGroup {
                title: name.to_string(),
                year: None,
                rating: None,
                runtime: None,
                genres: Vec::new(),
                language: None,
                mpa_rating: None,
                summary: None,
                imdb_code: None,
                trailer_code: None,
                poster: None,
                poster_small: None,
                poster_medium: None,
                poster_large: None,
                backdrop: None,
                variants: vec![SearchResult {
                    magnet,
                    seeds: seeders,
                    leeches: 0,
                    size: size_str,
                    size_bytes: size,
                    quality: None,
                    video_codec: None,
                    audio_channels: None,
                    bit_depth: None,
                    source_type: Some(provider.name.as_deref().unwrap_or("tpb").to_string()),
                }],
            });
        }

        Ok(groups)
    }

    pub async fn search_tv(&self, query: &str) -> Result<Vec<TvSearchResultGroup>> {
        let provider = match self.provider("tv") {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        let fmt = provider.format.as_deref().unwrap_or("eztv");
        match fmt {
            "torrentio" => self.search_tv_torrentio(query, &provider).await,
            "apibay" => self.search_tv_apibay(query, &provider).await,
            _ => self.search_tv_eztv(query, &provider).await,
        }
    }

    pub async fn browse_tv(&self, page: u32, limit: u32) -> Result<Vec<TvSearchResultGroup>> {
        let provider = match self.provider("tv") {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        let fmt = provider.format.as_deref().unwrap_or("eztv");
        match fmt {
            "torrentio" => self.browse_tv_torrentio(page).await,
            "apibay" => self.browse_tv_apibay(page, &provider).await,
            _ => self.browse_tv_eztv(page, limit, &provider).await,
        }
    }

    async fn search_tv_eztv(
        &self,
        query: &str,
        provider: &ProviderConfig,
    ) -> Result<Vec<TvSearchResultGroup>> {
        let api_url = format!("{}/api/get-torrents", provider.url);
        let response = match self
            .client
            .get(&api_url)
            .query(&[("search", query), ("limit", "100")])
            .send()
            .await
        {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("TV search request failed: {err}");
                return Ok(Vec::new());
            }
        };
        let eztv: EztvResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!("Failed to parse TV response: {err}");
                return Ok(Vec::new());
            }
        };
        let torrents = match eztv.torrents {
            Some(t) if !t.is_empty() => t,
            _ => return Ok(Vec::new()),
        };
        Ok(group_eztv_torrents(torrents))
    }

    async fn browse_tv_eztv(
        &self,
        page: u32,
        limit: u32,
        provider: &ProviderConfig,
    ) -> Result<Vec<TvSearchResultGroup>> {
        let api_url = format!("{}/api/get-torrents", provider.url);
        let response = match self
            .client
            .get(&api_url)
            .query(&[
                ("page", page.to_string()),
                ("limit", limit.min(100).to_string()),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("TV browse request failed: {err}");
                return Ok(Vec::new());
            }
        };
        let eztv: EztvResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!("Failed to parse TV browse response: {err}");
                return Ok(Vec::new());
            }
        };
        let torrents = match eztv.torrents {
            Some(t) if !t.is_empty() => t,
            _ => return Ok(Vec::new()),
        };
        Ok(group_eztv_torrents(torrents))
    }

    async fn search_tv_apibay(
        &self,
        query: &str,
        provider: &ProviderConfig,
    ) -> Result<Vec<TvSearchResultGroup>> {
        let cat = provider.category.as_deref().unwrap_or("205");
        let api_url = provider
            .api_url
            .as_deref()
            .map_or_else(|| format!("{}/q.php", provider.url), String::from);
        let results = self.fetch_apibay(&api_url, query, cat).await?;
        Ok(apibay_to_tv_groups(results))
    }

    async fn browse_tv_apibay(
        &self,
        _page: u32,
        provider: &ProviderConfig,
    ) -> Result<Vec<TvSearchResultGroup>> {
        let cat = provider.category.as_deref().unwrap_or("205");
        let api_url = provider
            .api_url
            .as_deref()
            .map_or_else(|| format!("{}/q.php", provider.url), String::from);
        let results = self.fetch_apibay(&api_url, "", cat).await?;
        Ok(apibay_to_tv_groups(results))
    }

    async fn browse_tv_torrentio(&self, page: u32) -> Result<Vec<TvSearchResultGroup>> {
        let skip = (page.saturating_sub(1)) * 50;
        let catalog = self.cinemeta.catalog("series", skip).await?;
        let groups = catalog
            .into_iter()
            .map(|meta| TvSearchResultGroup {
                show_name: meta.title(),
                imdb_id: Some(meta.id),
                seasons: Vec::new(),
            })
            .collect();
        Ok(groups)
    }

    /// Fetch episodes for a specific show by IMDB ID using Torrentio.
    /// If `season` is Some, fetch only that season. Otherwise fetch all.
    pub async fn fetch_show_episodes(
        &self,
        imdb_id: &str,
        season: Option<u32>,
    ) -> Result<Vec<TvSeason>> {
        let provider = match self.provider("tv") {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        let fmt = provider.format.as_deref().unwrap_or("eztv");
        if fmt != "torrentio" {
            return Ok(Vec::new());
        }
        match season {
            Some(s) => {
                let eps = self
                    .fetch_torrentio_season_episodes(&provider.url, imdb_id, s)
                    .await;
                if eps.is_empty() {
                    Ok(Vec::new())
                } else {
                    Ok(vec![TvSeason {
                        season: s,
                        episodes: eps,
                    }])
                }
            }
            None => Ok(self
                .fetch_torrentio_show_seasons(&provider.url, imdb_id)
                .await),
        }
    }

    /// Discover which seasons exist for a show by probing E01 sequentially.
    pub async fn discover_seasons(&self, imdb_id: &str) -> Result<Vec<u32>> {
        let provider = match self.provider("tv") {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        let fmt = provider.format.as_deref().unwrap_or("eztv");
        if fmt != "torrentio" {
            return Ok(Vec::new());
        }
        let mut seasons = Vec::new();
        let mut consecutive_misses = 0;
        for s in 1..=20u32 {
            let id = format!("{imdb_id}:{s}:1");
            let streams = self
                .fetch_torrentio_streams(&provider.url, "series", &id)
                .await
                .unwrap_or_default();
            if !streams.is_empty() {
                seasons.push(s);
                consecutive_misses = 0;
            } else {
                consecutive_misses += 1;
                if consecutive_misses >= 3 {
                    break;
                }
            }
        }
        Ok(seasons)
    }

    async fn fetch_torrentio_season_episodes(
        &self,
        base_url: &str,
        imdb_id: &str,
        season_num: u32,
    ) -> Vec<TvEpisode> {
        const MAX_EPISODES: u32 = 30;
        const BATCH_SIZE: u32 = 10;

        let mut episodes = Vec::new();
        for batch_start in (1..=MAX_EPISODES).step_by(BATCH_SIZE as usize) {
            let batch_end = (batch_start + BATCH_SIZE - 1).min(MAX_EPISODES);
            let futs: Vec<_> = (batch_start..=batch_end)
                .map(|ep_num| {
                    let id = format!("{imdb_id}:{season_num}:{ep_num}");
                    async move {
                        let streams = self
                            .fetch_torrentio_streams(base_url, "series", &id)
                            .await
                            .unwrap_or_default();
                        (ep_num, streams)
                    }
                })
                .collect();
            let batch_results = futures::future::join_all(futs).await;
            let any_found = batch_results.iter().any(|(_, s)| !s.is_empty());
            for (ep_num, streams) in batch_results {
                if streams.is_empty() {
                    continue;
                }
                let mut variants: Vec<TvTorrent> = streams
                    .into_iter()
                    .map(|stream| {
                        let title_text = stream.title.as_deref().unwrap_or("");
                        let seeds = parse_torrentio_seeds(title_text);
                        let size_str = parse_torrentio_size(title_text);
                        let size_bytes = parse_size_to_bytes(&size_str);
                        let quality = extract_quality(title_text);
                        let filename = title_text.lines().next().unwrap_or("").trim().to_string();
                        let display = format!(
                            "S{season_num:02}E{ep_num:02} [{}]",
                            quality.as_deref().unwrap_or("unknown")
                        );
                        let magnet = build_magnet(&stream.info_hash, &display);
                        TvTorrent {
                            magnet,
                            seeds,
                            leeches: 0,
                            size_bytes,
                            quality,
                            filename,
                        }
                    })
                    .collect();
                variants.sort_by_key(|v| std::cmp::Reverse(v.seeds));
                episodes.push(TvEpisode {
                    episode: ep_num,
                    title: None,
                    variants,
                });
            }
            if !any_found {
                break;
            }
        }
        episodes.sort_by_key(|e| e.episode);
        episodes
    }

    async fn fetch_torrentio_streams(
        &self,
        base_url: &str,
        content_type: &str,
        id: &str,
    ) -> Result<Vec<TorrentioStream>> {
        let url = format!(
            "{}/stream/{}/{}.json",
            base_url.trim_end_matches('/'),
            content_type,
            id
        );
        let response = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("Torrentio request failed for {id}: {err}");
                return Ok(Vec::new());
            }
        };
        let body: TorrentioResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!("Failed to parse Torrentio response for {id}: {err}");
                return Ok(Vec::new());
            }
        };
        Ok(body.streams.unwrap_or_default())
    }

    async fn search_torrentio_movies(
        &self,
        query: &str,
        provider: &ProviderConfig,
    ) -> Result<Vec<SearchResultGroup>> {
        let search_results = self.cinemeta.search("movie", query).await?;
        if search_results.is_empty() {
            return Ok(Vec::new());
        }

        let items: Vec<_> = search_results.into_iter().take(10).collect();

        // Fetch Torrentio streams and Cinemeta details concurrently for all movies
        let futs: Vec<_> = items
            .iter()
            .map(|meta| {
                let base_url = provider.url.clone();
                let imdb_id = meta.id.clone();
                async move {
                    let (streams, detail) = futures::future::join(
                        self.fetch_torrentio_streams(&base_url, "movie", &imdb_id),
                        self.cinemeta.get_detail("movie", &imdb_id),
                    )
                    .await;
                    (streams, detail)
                }
            })
            .collect();

        let all_results = futures::future::join_all(futs).await;

        let mut groups = Vec::new();
        for (meta, (streams_result, detail_result)) in items.into_iter().zip(all_results) {
            let streams = streams_result.unwrap_or_default();
            if streams.is_empty() {
                continue;
            }

            let detail = detail_result.ok().flatten();
            let title = meta.title();

            let mut variants: Vec<SearchResult> = streams
                .into_iter()
                .map(|stream| {
                    let title_text = stream.title.as_deref().unwrap_or("");
                    let seeds = parse_torrentio_seeds(title_text);
                    let size_str = parse_torrentio_size(title_text);
                    let size_bytes = parse_size_to_bytes(&size_str);
                    let quality = extract_quality(title_text);
                    let video_codec = extract_codec(title_text);
                    let display_title =
                        format!("{} [{}]", title, quality.as_deref().unwrap_or("unknown"));
                    let magnet = build_magnet(&stream.info_hash, &display_title);

                    SearchResult {
                        magnet,
                        seeds,
                        leeches: 0,
                        size: if size_str.is_empty() {
                            format_size(size_bytes)
                        } else {
                            size_str
                        },
                        size_bytes,
                        quality,
                        video_codec,
                        audio_channels: None,
                        bit_depth: None,
                        source_type: None,
                    }
                })
                .collect();

            variants.sort_by_key(|v| std::cmp::Reverse(v.seeds));

            // Use detail for richer metadata, fall back to search result
            let d = detail.as_ref().unwrap_or(&meta);
            let mut group = SearchResultGroup {
                title: d.title(),
                year: d.parse_year(),
                rating: d.parse_rating(),
                runtime: d.parse_runtime(),
                genres: d.genres_list(),
                language: None,
                mpa_rating: None,
                summary: d.description.clone(),
                imdb_code: Some(meta.id.clone()),
                trailer_code: None,
                poster: d.poster.clone(),
                poster_small: d.poster.clone(),
                poster_medium: d.poster.clone(),
                poster_large: d.poster.clone(),
                backdrop: d.background.clone(),
                variants,
            };
            proxy_posters(&mut group, CINEMETA_PROXY_ID);
            groups.push(group);
        }

        groups.sort_by(|a, b| {
            let best_a = a.variants.iter().map(|v| v.seeds).max().unwrap_or(0);
            let best_b = b.variants.iter().map(|v| v.seeds).max().unwrap_or(0);
            best_b.cmp(&best_a)
        });

        Ok(groups)
    }

    async fn search_tv_torrentio(
        &self,
        query: &str,
        _provider: &ProviderConfig,
    ) -> Result<Vec<TvSearchResultGroup>> {
        let search_results = self.cinemeta.search("series", query).await?;
        let groups = search_results
            .into_iter()
            .take(10)
            .map(|meta| TvSearchResultGroup {
                show_name: meta.title(),
                imdb_id: Some(meta.id),
                seasons: Vec::new(),
            })
            .collect();
        Ok(groups)
    }

    async fn fetch_torrentio_show_seasons(&self, base_url: &str, imdb_id: &str) -> Vec<TvSeason> {
        let season_nums = self.discover_seasons(imdb_id).await.unwrap_or_default();
        let mut seasons = Vec::new();
        for s in season_nums {
            let episodes = self
                .fetch_torrentio_season_episodes(base_url, imdb_id, s)
                .await;
            if !episodes.is_empty() {
                seasons.push(TvSeason {
                    season: s,
                    episodes,
                });
            }
        }
        seasons
    }

    pub async fn search_music_videos(&self, query: &str) -> Result<Vec<MusicVideoResult>> {
        let provider = match self.provider("music-videos") {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        let fmt = provider.format.as_deref().unwrap_or("apibay");
        match fmt {
            "scrape" => {
                let search_url = format!(
                    "{}/category-search/{}/Music-videos/1/",
                    provider.url,
                    urlencoding::encode(query)
                );
                self.scrape_list(&provider.url, &search_url).await
            }
            _ => {
                let cat = provider.category.as_deref().unwrap_or("601");
                let api_url = provider
                    .api_url
                    .as_deref()
                    .map_or_else(|| format!("{}/q.php", provider.url), String::from);
                let results = self.fetch_apibay(&api_url, query, cat).await?;
                Ok(apibay_to_music_results(results))
            }
        }
    }

    pub async fn browse_music_videos(&self, page: u32) -> Result<Vec<MusicVideoResult>> {
        let provider = match self.provider("music-videos") {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        let fmt = provider.format.as_deref().unwrap_or("apibay");
        match fmt {
            "scrape" => {
                let url = format!("{}/cat/Music-videos/{}/", provider.url, page);
                self.scrape_list(&provider.url, &url).await
            }
            _ => {
                let cat = provider.category.as_deref().unwrap_or("601");
                let api_url = provider
                    .api_url
                    .as_deref()
                    .map_or_else(|| format!("{}/q.php", provider.url), String::from);
                let results = self.fetch_apibay(&api_url, "", cat).await?;
                Ok(apibay_to_music_results(results))
            }
        }
    }

    pub async fn search_music(&self, query: &str) -> Result<Vec<MusicVideoResult>> {
        let provider = match self.provider("music") {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        let fmt = provider.format.as_deref().unwrap_or("apibay");
        match fmt {
            "scrape" => {
                let search_url = format!(
                    "{}/category-search/{}/Music/1/",
                    provider.url,
                    urlencoding::encode(query)
                );
                self.scrape_list(&provider.url, &search_url).await
            }
            _ => {
                let cat = provider.category.as_deref().unwrap_or("101");
                let api_url = provider
                    .api_url
                    .as_deref()
                    .map_or_else(|| format!("{}/q.php", provider.url), String::from);
                let results = self.fetch_apibay(&api_url, query, cat).await?;
                Ok(apibay_to_music_results(results))
            }
        }
    }

    pub async fn browse_music(&self, page: u32) -> Result<Vec<MusicVideoResult>> {
        let provider = match self.provider("music") {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        let fmt = provider.format.as_deref().unwrap_or("apibay");
        match fmt {
            "scrape" => {
                let url = format!("{}/cat/Music/{}/", provider.url, page);
                self.scrape_list(&provider.url, &url).await
            }
            _ => {
                let cat = provider.category.as_deref().unwrap_or("101");
                let api_url = provider
                    .api_url
                    .as_deref()
                    .map_or_else(|| format!("{}/q.php", provider.url), String::from);
                // "Top 100" must be CURRENT: merge the live top-100 feed
                // with this-year searches, dedupe by info hash, and rank
                // recent uploads (newest first) ahead of evergreen
                // compilations that dominate pure seeder counts.
                let year = chrono::Utc::now().format("%Y").to_string();
                let q_top = format!("top 100 {year}");
                let q_hits = format!("hits {year}");
                let (top, y1, y2) = tokio::join!(
                    self.fetch_apibay(&api_url, "", cat),
                    self.fetch_apibay(&api_url, &q_top, cat),
                    self.fetch_apibay(&api_url, &q_hits, cat),
                );
                let mut merged: Vec<ApibayTorrent> = Vec::new();
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                for batch in [
                    y1.unwrap_or_default(),
                    y2.unwrap_or_default(),
                    top.unwrap_or_default(),
                ] {
                    for t in batch {
                        if t.seeders.parse::<u32>().unwrap_or(0) == 0 {
                            continue;
                        }
                        if seen.insert(t.info_hash.to_lowercase()) {
                            merged.push(t);
                        }
                    }
                }
                let now = chrono::Utc::now().timestamp();
                let fresh_cutoff = now - 180 * 24 * 3600;
                let added = |t: &ApibayTorrent| t.added.parse::<i64>().unwrap_or(0);
                let seeders = |t: &ApibayTorrent| t.seeders.parse::<u32>().unwrap_or(0);
                merged.sort_by(|a, b| {
                    let fa = added(a) >= fresh_cutoff;
                    let fb = added(b) >= fresh_cutoff;
                    // Fresh uploads first (newest first); stale ones
                    // trail, ordered by popularity.
                    fb.cmp(&fa).then_with(|| {
                        if fa && fb {
                            added(b).cmp(&added(a))
                        } else {
                            seeders(b).cmp(&seeders(a))
                        }
                    })
                });
                merged.truncate(100);
                Ok(apibay_to_music_results(merged))
            }
        }
    }

    async fn fetch_apibay(
        &self,
        api_url: &str,
        query: &str,
        cat: &str,
    ) -> Result<Vec<ApibayTorrent>> {
        // Browsing (no query): use apibay's live top-100 feed, which is
        // continuously refreshed. The old literal `q=top100` search
        // surfaced stale "Top 100 <month>" chart rips instead of what is
        // actually popular right now.
        if query.is_empty() {
            let base = api_url.trim_end_matches("/q.php").trim_end_matches('/');
            let first_cat = cat.split(',').next().unwrap_or(cat).trim();
            let url = format!("{base}/precompiled/data_top100_{first_cat}.json");
            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Vec<ApibayTorrent>>().await {
                        Ok(items) if !items.is_empty() => return Ok(items),
                        Ok(_) => {}
                        Err(err) => {
                            tracing::warn!("Apibay top100 parse failed, falling back: {err}")
                        }
                    }
                }
                Ok(resp) => {
                    tracing::warn!(status = %resp.status(), "Apibay top100 fetch failed, falling back")
                }
                Err(err) => tracing::warn!("Apibay top100 request failed, falling back: {err}"),
            }
        }

        let mut params = vec![("cat", cat.to_string())];
        if query.is_empty() {
            params.push(("q", "top100".to_string()));
        } else {
            params.push(("q", query.to_string()));
        }
        let response = match self.client.get(api_url).query(&params).send().await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("Apibay request failed: {err}");
                return Ok(Vec::new());
            }
        };
        let torrents: Vec<ApibayTorrent> = match response.json().await {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!("Failed to parse apibay response: {err}");
                return Ok(Vec::new());
            }
        };
        // Filter out the "no results" sentinel (id=0, name="No results")
        Ok(torrents
            .into_iter()
            .filter(|t| t.info_hash != "0000000000000000000000000000000000000000")
            .collect())
    }

    async fn scrape_list(&self, base_url: &str, url: &str) -> Result<Vec<MusicVideoResult>> {
        let html = match self.client.get(url).send().await {
            Ok(r) => match r.text().await {
                Ok(t) => t,
                Err(err) => {
                    tracing::warn!("Failed to read 1337x response: {err}");
                    return Ok(Vec::new());
                }
            },
            Err(err) => {
                tracing::warn!("1337x request failed: {err}");
                return Ok(Vec::new());
            }
        };

        let document = scraper::Html::parse_document(&html);
        let row_sel = scraper::Selector::parse("tbody tr")
            .unwrap_or_else(|_| scraper::Selector::parse("tr").unwrap_or_else(|_| unreachable!()));
        let name_sel = scraper::Selector::parse("td.name a:nth-child(2)").unwrap_or_else(|_| {
            scraper::Selector::parse("td a").unwrap_or_else(|_| unreachable!())
        });
        let seeds_sel = scraper::Selector::parse("td.seeds")
            .unwrap_or_else(|_| scraper::Selector::parse("td").unwrap_or_else(|_| unreachable!()));
        let leeches_sel = scraper::Selector::parse("td.leeches")
            .unwrap_or_else(|_| scraper::Selector::parse("td").unwrap_or_else(|_| unreachable!()));
        let size_sel = scraper::Selector::parse("td.size")
            .unwrap_or_else(|_| scraper::Selector::parse("td").unwrap_or_else(|_| unreachable!()));

        let mut results = Vec::new();

        for row in document.select(&row_sel) {
            let name_el = match row.select(&name_sel).next() {
                Some(el) => el,
                None => continue,
            };

            let title = name_el.text().collect::<String>().trim().to_string();
            if title.is_empty() {
                continue;
            }

            let detail_path = name_el.value().attr("href").unwrap_or("");
            let detail_url = if detail_path.starts_with('/') {
                format!("{}{}", base_url, detail_path)
            } else {
                detail_path.to_string()
            };

            let seeds: u32 = row
                .select(&seeds_sel)
                .next()
                .map(|el| el.text().collect::<String>().trim().parse().unwrap_or(0))
                .unwrap_or(0);

            let leeches: u32 = row
                .select(&leeches_sel)
                .next()
                .map(|el| el.text().collect::<String>().trim().parse().unwrap_or(0))
                .unwrap_or(0);

            let size = row
                .select(&size_sel)
                .next()
                .map(|el| {
                    // 1337x puts size in first text node (before the span)
                    el.text().next().unwrap_or("").trim().to_string()
                })
                .unwrap_or_default();

            let date = extract_date_from_title(&title);
            results.push(MusicVideoResult {
                title,
                magnet: None,
                seeds,
                leeches,
                size,
                detail_url,
                date,
            });
        }

        Ok(results)
    }

    pub async fn get_magnet(&self, detail_url: &str) -> Result<Option<String>> {
        // Validate URL to prevent SSRF - must start with a known provider URL
        let is_allowed = self
            .providers
            .iter()
            .any(|p| detail_url.starts_with(&p.url));
        if !is_allowed {
            return Ok(None);
        }

        let html = match self.client.get(detail_url).send().await {
            Ok(r) => match r.text().await {
                Ok(t) => t,
                Err(_) => return Ok(None),
            },
            Err(_) => return Ok(None),
        };

        let document = scraper::Html::parse_document(&html);
        let magnet_sel = scraper::Selector::parse("a[href^='magnet:']")
            .unwrap_or_else(|_| scraper::Selector::parse("a").unwrap_or_else(|_| unreachable!()));

        let magnet = document
            .select(&magnet_sel)
            .next()
            .and_then(|el| el.value().attr("href"))
            .map(String::from);

        Ok(magnet)
    }
}

fn group_eztv_torrents(torrents: Vec<EztvTorrent>) -> Vec<TvSearchResultGroup> {
    use std::collections::HashMap;

    // Group by show name
    let mut shows: HashMap<String, Vec<&EztvTorrent>> = HashMap::new();
    for t in &torrents {
        let show_name = extract_show_name(&t.title);
        shows.entry(show_name).or_default().push(t);
    }

    let mut groups: Vec<TvSearchResultGroup> = shows
        .into_iter()
        .map(|(show_name, show_torrents)| {
            let imdb_id = show_torrents
                .iter()
                .find_map(|t| t.imdb_id.as_ref())
                .cloned();

            // Group by season -> episode
            let mut season_map: HashMap<u32, HashMap<u32, Vec<TvTorrent>>> = HashMap::new();
            for t in show_torrents {
                let season = t
                    .season
                    .as_deref()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(1);
                let episode = t
                    .episode
                    .as_deref()
                    .and_then(|e| e.parse::<u32>().ok())
                    .unwrap_or(0);

                let size_bytes = t
                    .size_bytes
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);

                let quality = extract_quality(&t.title);

                let variant = TvTorrent {
                    magnet: t.magnet_url.clone(),
                    seeds: t.seeds,
                    leeches: t.peers,
                    size_bytes,
                    quality,
                    filename: t.filename.clone().unwrap_or_default(),
                };

                season_map
                    .entry(season)
                    .or_default()
                    .entry(episode)
                    .or_default()
                    .push(variant);
            }

            let mut seasons: Vec<TvSeason> = season_map
                .into_iter()
                .map(|(season_num, episodes_map)| {
                    let mut episodes: Vec<TvEpisode> = episodes_map
                        .into_iter()
                        .map(|(ep_num, mut variants)| {
                            variants.sort_by_key(|v| std::cmp::Reverse(v.seeds));
                            TvEpisode {
                                episode: ep_num,
                                title: None,
                                variants,
                            }
                        })
                        .collect();
                    episodes.sort_by_key(|e| e.episode);
                    TvSeason {
                        season: season_num,
                        episodes,
                    }
                })
                .collect();
            seasons.sort_by_key(|s| s.season);

            TvSearchResultGroup {
                show_name,
                imdb_id,
                seasons,
            }
        })
        .collect();

    // Sort by total seeds across all torrents
    groups.sort_by(|a, b| {
        let seeds_a: u32 = a
            .seasons
            .iter()
            .flat_map(|s| &s.episodes)
            .flat_map(|e| &e.variants)
            .map(|v| v.seeds)
            .sum();
        let seeds_b: u32 = b
            .seasons
            .iter()
            .flat_map(|s| &s.episodes)
            .flat_map(|e| &e.variants)
            .map(|v| v.seeds)
            .sum();
        seeds_b.cmp(&seeds_a)
    });

    groups
}

fn apibay_to_tv_groups(torrents: Vec<ApibayTorrent>) -> Vec<TvSearchResultGroup> {
    use std::collections::HashMap;

    let mut shows: HashMap<String, Vec<&ApibayTorrent>> = HashMap::new();
    for t in &torrents {
        let show_name = extract_show_name(&t.name);
        shows.entry(show_name).or_default().push(t);
    }

    let mut groups: Vec<TvSearchResultGroup> = shows
        .into_iter()
        .map(|(show_name, show_torrents)| {
            let mut season_map: std::collections::HashMap<u32, Vec<TvTorrent>> = HashMap::new();
            for t in &show_torrents {
                let (season, _episode) = parse_season_episode(&t.name);
                let magnet = build_magnet(&t.info_hash, &t.name);
                let variant = TvTorrent {
                    magnet,
                    seeds: t.seeders.parse().unwrap_or(0),
                    leeches: t.leechers.parse().unwrap_or(0),
                    size_bytes: t.size.parse().unwrap_or(0),
                    quality: extract_quality(&t.name),
                    filename: t.name.clone(),
                };
                season_map.entry(season).or_default().push(variant);
            }

            let mut seasons: Vec<TvSeason> = season_map
                .into_iter()
                .map(|(season_num, variants)| TvSeason {
                    season: season_num,
                    episodes: vec![TvEpisode {
                        episode: 0,
                        title: None,
                        variants,
                    }],
                })
                .collect();
            seasons.sort_by_key(|s| s.season);

            TvSearchResultGroup {
                show_name,
                imdb_id: None,
                seasons,
            }
        })
        .collect();

    groups.sort_by(|a, b| {
        let seeds_a: u32 = a
            .seasons
            .iter()
            .flat_map(|s| &s.episodes)
            .flat_map(|e| &e.variants)
            .map(|v| v.seeds)
            .sum();
        let seeds_b: u32 = b
            .seasons
            .iter()
            .flat_map(|s| &s.episodes)
            .flat_map(|e| &e.variants)
            .map(|v| v.seeds)
            .sum();
        seeds_b.cmp(&seeds_a)
    });

    groups
}

fn apibay_to_music_results(torrents: Vec<ApibayTorrent>) -> Vec<MusicVideoResult> {
    torrents
        .into_iter()
        .map(|t| {
            let magnet = build_magnet(&t.info_hash, &t.name);
            let size_bytes: u64 = t.size.parse().unwrap_or(0);
            let date = t
                .added
                .parse::<i64>()
                .ok()
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .or_else(|| extract_date_from_title(&t.name));
            MusicVideoResult {
                title: t.name,
                magnet: Some(magnet),
                seeds: t.seeders.parse().unwrap_or(0),
                leeches: t.leechers.parse().unwrap_or(0),
                size: format_size(size_bytes),
                detail_url: String::new(),
                date,
            }
        })
        .collect()
}

/// Best-effort extraction of an ISO date from a torrent title.
/// Tries `YYYY-MM-DD` / `YYYY.MM.DD`, falls back to a four-digit
/// year (`(2024)` or whitespace-bounded `2024`) and assumes Jan 1.
fn extract_date_from_title(title: &str) -> Option<String> {
    let bytes = title.as_bytes();

    // Look for a 4-digit year followed by month and day, separated
    // by `.`, `-`, `/`, or `_`.
    for i in 0..bytes.len().saturating_sub(9) {
        let slice = &bytes[i..i + 10];
        if slice[0..4].iter().all(|b| b.is_ascii_digit())
            && (slice[4] == b'-' || slice[4] == b'.' || slice[4] == b'/' || slice[4] == b'_')
            && slice[5..7].iter().all(|b| b.is_ascii_digit())
            && slice[4] == slice[7]
            && slice[8..10].iter().all(|b| b.is_ascii_digit())
        {
            let year: u32 = std::str::from_utf8(&slice[0..4]).ok()?.parse().ok()?;
            let month: u32 = std::str::from_utf8(&slice[5..7]).ok()?.parse().ok()?;
            let day: u32 = std::str::from_utf8(&slice[8..10]).ok()?.parse().ok()?;
            if (1900..=2100).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day)
            {
                return Some(format!("{year:04}-{month:02}-{day:02}"));
            }
        }
    }

    // Fallback: bare 4-digit year. Anchor on a non-digit boundary
    // so we don't pick the first half of "20241108".
    let mut prev_digit = false;
    for i in 0..bytes.len().saturating_sub(3) {
        let prev_ok = !prev_digit;
        let curr_digit = bytes[i].is_ascii_digit();
        prev_digit = curr_digit;
        if !prev_ok || !curr_digit {
            continue;
        }
        let next_non_digit = i + 4 == bytes.len() || !bytes[i + 4].is_ascii_digit();
        if !next_non_digit {
            continue;
        }
        if let Ok(year) = std::str::from_utf8(&bytes[i..i + 4])
            .unwrap_or("")
            .parse::<u32>()
        {
            if (1950..=2100).contains(&year) {
                return Some(format!("{year:04}-01-01"));
            }
        }
    }
    None
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn parse_season_episode(title: &str) -> (u32, u32) {
    // S01E02 pattern
    let upper = title.to_uppercase();
    for (i, _) in upper.char_indices() {
        if i + 6 <= upper.len() && upper[i..].starts_with('S') {
            let rest = &upper[i + 1..];
            if let Some(e_pos) = rest.find('E') {
                if let (Ok(s), Ok(e)) = (
                    rest[..e_pos].parse::<u32>(),
                    rest[e_pos + 1..]
                        .split(|c: char| !c.is_ascii_digit())
                        .next()
                        .unwrap_or("0")
                        .parse::<u32>(),
                ) {
                    return (s, e);
                }
            }
        }
    }
    (1, 0)
}

fn extract_show_name(title: &str) -> String {
    // Pattern: "Show Name S01E02 ..." -> "Show Name"
    if let Some(idx) = title.find(" S0") {
        return title[..idx].trim().to_string();
    }
    if let Some(idx) = title.find(" s0") {
        return title[..idx].trim().to_string();
    }
    if let Some(idx) = title.find(" S1") {
        return title[..idx].trim().to_string();
    }
    // Pattern: "Show Name 1x02 ..."
    for (i, _) in title.char_indices() {
        if i > 0 && i + 3 < title.len() {
            let slice = &title[i..];
            if slice.starts_with(' ')
                && slice.len() > 4
                && slice.as_bytes()[1].is_ascii_digit()
                && slice.as_bytes()[2] == b'x'
                && slice.as_bytes()[3].is_ascii_digit()
            {
                return title[..i].trim().to_string();
            }
        }
    }
    title.trim().to_string()
}

fn extract_quality(title: &str) -> Option<String> {
    let lower = title.to_lowercase();
    if lower.contains("2160p") || lower.contains("4k") {
        Some("2160p".to_string())
    } else if lower.contains("1080p") {
        Some("1080p".to_string())
    } else if lower.contains("720p") {
        Some("720p".to_string())
    } else if lower.contains("480p") {
        Some("480p".to_string())
    } else {
        None
    }
}

fn merge_movie_groups(groups: &mut Vec<SearchResultGroup>) {
    use std::collections::HashMap;
    let mut by_imdb: HashMap<String, usize> = HashMap::new();
    let mut merged: Vec<SearchResultGroup> = Vec::new();

    for group in groups.drain(..) {
        if let Some(ref imdb) = group.imdb_code {
            if let Some(&idx) = by_imdb.get(imdb) {
                merged[idx].variants.extend(group.variants);
                // Deduplicate by info_hash (magnet contains the hash)
                let mut seen = std::collections::HashSet::new();
                merged[idx].variants.retain(|v| {
                    let hash = extract_magnet_hash(&v.magnet);
                    seen.insert(hash)
                });
                merged[idx]
                    .variants
                    .sort_by_key(|v| std::cmp::Reverse(v.seeds));
                continue;
            }
            by_imdb.insert(imdb.clone(), merged.len());
        }
        merged.push(group);
    }

    // Sort by best seeds
    merged.sort_by(|a, b| {
        let best_a = a.variants.iter().map(|v| v.seeds).max().unwrap_or(0);
        let best_b = b.variants.iter().map(|v| v.seeds).max().unwrap_or(0);
        best_b.cmp(&best_a)
    });

    *groups = merged;
}

fn extract_magnet_hash(magnet: &str) -> String {
    magnet
        .strip_prefix("magnet:?xt=urn:btih:")
        .unwrap_or(magnet)
        .split('&')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

fn build_magnet(hash: &str, title: &str) -> String {
    let encoded_title = urlencoding::encode(title);
    let trackers: String = TRACKERS
        .iter()
        .map(|t| format!("&tr={}", urlencoding::encode(t)))
        .collect();
    format!("magnet:?xt=urn:btih:{hash}&dn={encoded_title}{trackers}")
}

fn parse_torrentio_seeds(title: &str) -> u32 {
    if let Some(idx) = title.find('\u{1F464}') {
        let after = &title[idx + '\u{1F464}'.len_utf8()..];
        let trimmed = after.trim_start();
        let num_str: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        num_str.parse().unwrap_or(0)
    } else {
        0
    }
}

fn parse_torrentio_size(title: &str) -> String {
    if let Some(idx) = title.find('\u{1F4BE}') {
        let after = &title[idx + '\u{1F4BE}'.len_utf8()..];
        let trimmed = after.trim_start();
        let size: String = trimmed.chars().take_while(|c| *c != '\n').collect();
        size.trim().to_string()
    } else {
        String::new()
    }
}

fn parse_size_to_bytes(size_str: &str) -> u64 {
    let parts: Vec<&str> = size_str.split_whitespace().collect();
    if parts.len() != 2 {
        return 0;
    }
    let num: f64 = match parts[0].parse() {
        Ok(n) => n,
        Err(_) => return 0,
    };
    match parts[1].to_uppercase().as_str() {
        "TB" => (num * 1_099_511_627_776.0) as u64,
        "GB" => (num * 1_073_741_824.0) as u64,
        "MB" => (num * 1_048_576.0) as u64,
        "KB" => (num * 1024.0) as u64,
        _ => 0,
    }
}

fn extract_codec(title: &str) -> Option<String> {
    let lower = title.to_lowercase();
    if lower.contains("h.265") || lower.contains("hevc") || lower.contains("x265") {
        Some("H.265".to_string())
    } else if lower.contains("h.264") || lower.contains("x264") {
        Some("H.264".to_string())
    } else {
        None
    }
}
