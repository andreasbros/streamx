export interface User {
  id: string;
  username: string;
  is_admin: boolean;
  created_at: string;
}

export interface LoginRequest {
  username: string;
  password: string;
}

export interface LoginResponse {
  token: string;
}

export interface RegisterRequest {
  username: string;
  password: string;
}

export interface SearchRequest {
  query: string;
}

export interface SearchResultGroup {
  title: string;
  year?: number;
  rating?: number;
  runtime?: number;
  genres: string[];
  language?: string;
  mpa_rating?: string;
  summary?: string;
  imdb_code?: string;
  trailer_code?: string;
  poster?: string;
  poster_small?: string;
  poster_medium?: string;
  poster_large?: string;
  backdrop?: string;
  variants: SearchResult[];
}

export interface SearchResult {
  magnet: string;
  seeds: number;
  leeches: number;
  size: string;
  size_bytes: number;
  quality?: string;
  video_codec?: string;
  audio_channels?: string;
  bit_depth?: string;
  source_type?: string;
}

export interface SearchResponse {
  results: SearchResultGroup[];
}

export interface StreamRequest {
  magnet_uri: string;
  file_index?: number;
  poster_url?: string;
  // Rich metadata
  title?: string;
  year?: number;
  rating?: number;
  runtime?: number;
  genres?: string[];
  language?: string;
  video_codec?: string;
  audio_channels?: string;
  source_type?: string;
  summary?: string;
  imdb_code?: string;
  mpa_rating?: string;
  bit_depth?: string;
  trailer_code?: string;
  poster_small?: string;
  poster_medium?: string;
  poster_large?: string;
  backdrop?: string;
}

export interface StreamFile {
  index: number;
  name: string;
  size: number;
}

export interface StreamResponse {
  stream_id: string;
  status: StreamStatusType;
}

export type StreamStatusType =
  | "initializing"
  | "downloading"
  | "transcoding"
  | "ready"
  | "complete"
  | "paused"
  | "error";

export interface StreamStatus {
  status: StreamStatusType;
  progress: number;
  peers?: number;
  speed?: number;
  title?: string;
  file_name?: string;
  file_size?: number;
  files?: StreamFile[];
  video_codec?: string;
}

export interface WatchHistoryItem {
  id: string;
  magnet_uri: string;
  title: string;
  file_name: string | null;
  duration_seconds: number | null;
  watched_seconds: number | null;
  poster_url: string | null;
  watched_at: string;
  info_hash: string | null;
  file_size: number | null;
  year: number | null;
  rating: number | null;
  runtime: number | null;
  genres: string | null;
  summary: string | null;
  imdb_code: string | null;
}

export interface StreamMetadata {
  title?: string;
  year?: number;
  rating?: number;
  runtime?: number;
  genres?: string;
  language?: string;
  mpa_rating?: string;
  summary?: string;
  imdb_code?: string;
  video_codec?: string;
  audio_channels?: string;
  bit_depth?: string;
  source_type?: string;
  poster_large?: string;
  local_poster?: string;
}

export interface Settings {
  theme: "dark" | "light";
}

export interface ApiError {
  error: string;
  message: string;
}

export interface SearchHistoryItem {
  id: string;
  query: string;
  result_count: number | null;
  searched_at: string;
}

export interface SearchHistoryResponse {
  searches: SearchHistoryItem[];
}

export interface WatchHistoryResponse {
  items: WatchHistoryItem[];
}

export interface TvTorrent {
  magnet: string;
  seeds: number;
  leeches: number;
  size_bytes: number;
  quality: string | null;
  filename: string;
}

export interface TvEpisode {
  episode: number;
  title: string | null;
  variants: TvTorrent[];
}

export interface TvSeason {
  season: number;
  episodes: TvEpisode[];
}

export interface TvSearchResultGroup {
  show_name: string;
  imdb_id: string | null;
  seasons: TvSeason[];
}

export interface TvSearchResponse {
  results: TvSearchResultGroup[];
}

export interface MusicVideoResult {
  title: string;
  magnet: string | null;
  seeds: number;
  leeches: number;
  size: string;
  detail_url: string;
}

export interface MusicVideoSearchResponse {
  results: MusicVideoResult[];
}

export interface ResolveMagnetResponse {
  magnet: string;
}

export interface FavouriteItem {
  id: string;
  user_id: string;
  content_type: string;
  title: string;
  year: number | null;
  rating: number | null;
  poster_url: string | null;
  info_hash: string | null;
  metadata_json: string | null;
  created_at: string;
}

export interface FavouritesResponse {
  items: FavouriteItem[];
}
