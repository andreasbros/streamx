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

export interface SearchResult {
  title: string;
  magnet: string;
  seeds: number;
  leeches: number;
  size: string;
  size_bytes: number;
  quality?: string;
  year?: number;
  rating?: number;
  poster?: string;
}

export interface SearchResponse {
  results: SearchResult[];
}

export interface StreamRequest {
  magnet_uri: string;
  file_index?: number;
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
  browser_compatible?: boolean;
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
