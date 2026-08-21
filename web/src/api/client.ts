import { getToken } from "../lib/auth";
import { debugLog } from "../lib/debug-log";
import type {
  ApiError,
  DownloadsResponse,
  FavouriteItem,
  FavouritesResponse,
  LoginRequest,
  LoginResponse,
  MusicVideoSearchResponse,
  RegisterRequest,
  ResolveMagnetResponse,
  SearchRequest,
  SearchResponse,
  SearchHistoryResponse,
  ServerSettings,
  Settings,
  StreamRequest,
  StreamResponse,
  StreamStatus,
  TvSeason,
  TvSearchResponse,
  User,
  WatchHistoryResponse,
} from "./types";

class ApiClient {
  private async request<T>(
    path: string,
    options: RequestInit = {}
  ): Promise<T> {
    const headers = new Headers(options.headers);

    if (!headers.has("Content-Type") && options.body) {
      headers.set("Content-Type", "application/json");
    }

    const token = getToken();
    if (token) {
      headers.set("Authorization", `Bearer ${token}`);
    }

    debugLog.debug("API", "Request", {
      method: options.method ?? "GET",
      url: path,
    });

    const response = await fetch(path, {
      ...options,
      headers,
    });

    if (!response.ok) {
      let errorData: ApiError;
      try {
        errorData = (await response.json()) as ApiError;
      } catch {
        errorData = {
          error: "request_failed",
          message: `Request failed with status ${response.status}`,
        };
      }
      debugLog.error("API", "Request failed", {
        url: path,
        status: response.status,
        error: errorData,
      });
      throw new ApiRequestError(
        errorData.message || errorData.error,
        response.status,
        errorData
      );
    }

    debugLog.debug("API", "Response", {
      status: response.status,
      url: path,
    });

    if (response.status === 204) {
      return undefined as T;
    }

    return response.json() as Promise<T>;
  }

  async login(data: LoginRequest): Promise<LoginResponse> {
    return this.request<LoginResponse>("/api/auth/login", {
      method: "POST",
      body: JSON.stringify(data),
    });
  }

  async register(data: RegisterRequest): Promise<LoginResponse> {
    return this.request<LoginResponse>("/api/auth/register", {
      method: "POST",
      body: JSON.stringify(data),
    });
  }

  async me(): Promise<User> {
    return this.request<User>("/api/auth/me");
  }

  async search(data: SearchRequest): Promise<SearchResponse> {
    return this.request<SearchResponse>("/api/search", {
      method: "POST",
      body: JSON.stringify(data),
    });
  }

  async browse(params: { sort_by?: string; query_term?: string; genre?: string; minimum_rating?: number; limit?: number; page?: number }): Promise<SearchResponse> {
    const q = new URLSearchParams();
    if (params.sort_by) q.set("sort_by", params.sort_by);
    if (params.query_term) q.set("query_term", params.query_term);
    if (params.genre) q.set("genre", params.genre);
    if (params.minimum_rating) q.set("minimum_rating", String(params.minimum_rating));
    if (params.limit) q.set("limit", String(params.limit));
    if (params.page) q.set("page", String(params.page));
    return this.request<SearchResponse>(`/api/search/browse?${q.toString()}`);
  }

  async searchHistory(): Promise<SearchHistoryResponse> {
    return this.request<SearchHistoryResponse>("/api/search/history");
  }

  async startStream(data: StreamRequest): Promise<StreamResponse> {
    return this.request<StreamResponse>("/api/stream", {
      method: "POST",
      body: JSON.stringify(data),
    });
  }

  async streamStatus(streamId: string): Promise<StreamStatus> {
    return this.request<StreamStatus>(`/api/stream/${streamId}`);
  }

  async stopStream(streamId: string): Promise<void> {
    return this.request<void>(`/api/stream/${streamId}`, {
      method: "DELETE",
    });
  }

  async pauseStream(streamId: string): Promise<void> {
    await this.request(`/api/stream/${streamId}/pause`, { method: "PUT" });
  }

  async serverSettings(): Promise<ServerSettings> {
    return this.request<ServerSettings>("/api/settings/server");
  }

  async updateServerSettings(settings: ServerSettings): Promise<ServerSettings> {
    return this.request<ServerSettings>("/api/admin/settings", {
      method: "PUT",
      body: JSON.stringify(settings),
    });
  }

  async listDownloads(): Promise<DownloadsResponse> {
    return this.request<DownloadsResponse>("/api/downloads");
  }

  async pinDownload(streamId: string): Promise<void> {
    await this.request(`/api/stream/${streamId}/download`, { method: "POST" });
  }

  async unpinDownload(streamId: string): Promise<void> {
    await this.request(`/api/stream/${streamId}/download`, { method: "DELETE" });
  }

  async resumeStream(streamId: string): Promise<void> {
    await this.request(`/api/stream/${streamId}/resume`, { method: "PUT" });
  }

  async watchHistory(): Promise<WatchHistoryResponse> {
    return this.request<WatchHistoryResponse>("/api/history");
  }

  async updateWatchPosition(
    id: string,
    watchedSeconds: number
  ): Promise<void> {
    return this.request<void>(`/api/history/${id}`, {
      method: "PUT",
      body: JSON.stringify({ watched_seconds: Math.floor(watchedSeconds) }),
    });
  }

  async deleteHistoryItem(id: string): Promise<void> {
    return this.request<void>(`/api/history/${id}`, {
      method: "DELETE",
    });
  }

  async getSettings(): Promise<Settings> {
    return this.request<Settings>("/api/settings");
  }

  async updateSettings(data: Settings): Promise<Settings> {
    return this.request<Settings>("/api/settings", {
      method: "PUT",
      body: JSON.stringify(data),
    });
  }

  async searchTv(data: SearchRequest): Promise<TvSearchResponse> {
    return this.request<TvSearchResponse>("/api/tv/search", {
      method: "POST",
      body: JSON.stringify(data),
    });
  }

  async browseTv(params: { page?: number; limit?: number }): Promise<TvSearchResponse> {
    const q = new URLSearchParams();
    if (params.page) q.set("page", String(params.page));
    if (params.limit) q.set("limit", String(params.limit));
    return this.request<TvSearchResponse>(`/api/tv/browse?${q.toString()}`);
  }

  async getTvShowSeasons(imdbId: string): Promise<{ seasons: number[] }> {
    return this.request<{ seasons: number[] }>(`/api/tv/show/${imdbId}`);
  }

  async getTvShowEpisodes(imdbId: string, season: number): Promise<{ seasons: TvSeason[] }> {
    return this.request<{ seasons: TvSeason[] }>(`/api/tv/show/${imdbId}?season=${season}`);
  }

  async searchMusicVideos(data: SearchRequest): Promise<MusicVideoSearchResponse> {
    return this.request<MusicVideoSearchResponse>("/api/music-videos/search", {
      method: "POST",
      body: JSON.stringify(data),
    });
  }

  async browseMusicVideos(params: { page?: number }): Promise<MusicVideoSearchResponse> {
    const q = new URLSearchParams();
    if (params.page) q.set("page", String(params.page));
    return this.request<MusicVideoSearchResponse>(`/api/music-videos/browse?${q.toString()}`);
  }

  async searchMusic(data: SearchRequest): Promise<MusicVideoSearchResponse> {
    return this.request<MusicVideoSearchResponse>("/api/music/search", {
      method: "POST",
      body: JSON.stringify(data),
    });
  }

  async browseMusic(params: { page?: number }): Promise<MusicVideoSearchResponse> {
    const q = new URLSearchParams();
    if (params.page) q.set("page", String(params.page));
    return this.request<MusicVideoSearchResponse>(`/api/music/browse?${q.toString()}`);
  }

  async resolveMagnet(detailUrl: string, apiBase: string): Promise<ResolveMagnetResponse> {
    return this.request<ResolveMagnetResponse>(`/api/${apiBase}/resolve-magnet`, {
      method: "POST",
      body: JSON.stringify({ detail_url: detailUrl }),
    });
  }

  async getFavourites(type?: string): Promise<FavouritesResponse> {
    const q = new URLSearchParams();
    if (type) q.set("type", type);
    const qs = q.toString();
    return this.request<FavouritesResponse>(`/api/favourites${qs ? `?${qs}` : ""}`);
  }

  async addFavourite(data: Omit<FavouriteItem, "id" | "user_id" | "created_at">): Promise<FavouriteItem> {
    return this.request<FavouriteItem>("/api/favourites", {
      method: "POST",
      body: JSON.stringify(data),
    });
  }

  async deleteFavourite(id: string): Promise<void> {
    return this.request<void>(`/api/favourites/${id}`, {
      method: "DELETE",
    });
  }

  getPlaylistUrl(streamId: string, quality?: string): string {
    const token = getToken();
    const params = new URLSearchParams();
    if (token) params.set("token", token);
    if (quality) params.set("quality", quality);
    const qs = params.toString();
    return `/api/stream/${streamId}/playlist.m3u8${qs ? `?${qs}` : ""}`;
  }

  getUrlPlaylistUrl(url: string, quality?: string): string {
    const token = getToken();
    const params = new URLSearchParams();
    params.set("url", url);
    if (token) params.set("token", token);
    if (quality) params.set("quality", quality);
    return `/api/stream/url/playlist.m3u8?${params.toString()}`;
  }

  getFileUrl(streamId: string): string {
    return `/api/stream/${streamId}/file`;
  }

  getFileByIndexUrl(streamId: string, fileIndex: number): string {
    return `/api/stream/${streamId}/file/${fileIndex}`;
  }

  getArtworkUrl(streamId: string, fileIndex: number): string {
    return `/api/stream/${streamId}/artwork/${fileIndex}`;
  }

  async getStreamFiles(streamId: string): Promise<{ files: import("./types").TorrentFileInfo[] }> {
    return this.request(`/api/stream/${streamId}/files`);
  }

  async startMusicStream(magnetUri: string): Promise<StreamResponse> {
    return this.request("/api/stream/music", {
      method: "POST",
      body: JSON.stringify({ magnet_uri: magnetUri }),
    });
  }

  async deleteStream(streamId: string): Promise<void> {
    await this.request(`/api/stream/${streamId}`, { method: "DELETE" });
  }

  async createShareLink(streamId: string, durationHours = 24 * 30): Promise<{ token: string; url: string }> {
    return this.request(`/api/stream/${streamId}/share`, {
      method: "POST",
      body: JSON.stringify({ duration_hours: durationHours }),
    });
  }

  // Playlists
  async getPlaylists(): Promise<{ playlists: import("./types").Playlist[] }> {
    return this.request("/api/playlists");
  }

  async createPlaylist(name: string): Promise<import("./types").Playlist> {
    return this.request("/api/playlists", {
      method: "POST",
      body: JSON.stringify({ name }),
    });
  }

  async renamePlaylist(id: string, name: string): Promise<void> {
    await this.request(`/api/playlists/${id}`, {
      method: "PUT",
      body: JSON.stringify({ name }),
    });
  }

  async deletePlaylist(id: string): Promise<void> {
    await this.request(`/api/playlists/${id}`, { method: "DELETE" });
  }

  async getPlaylistTracks(id: string): Promise<{ tracks: import("./types").PlaylistTrack[] }> {
    return this.request(`/api/playlists/${id}/tracks`);
  }

  async addPlaylistTrack(playlistId: string, track: {
    info_hash: string;
    file_index?: number;
    title: string;
    artist?: string;
    album?: string;
    duration_seconds?: number;
    artwork_url?: string;
  }): Promise<import("./types").PlaylistTrack> {
    return this.request(`/api/playlists/${playlistId}/tracks`, {
      method: "POST",
      body: JSON.stringify(track),
    });
  }

  async removePlaylistTrack(playlistId: string, trackId: string): Promise<void> {
    await this.request(`/api/playlists/${playlistId}/tracks/${trackId}`, { method: "DELETE" });
  }
}

export class ApiRequestError extends Error {
  constructor(
    message: string,
    public readonly status: number,
    public readonly data: ApiError
  ) {
    super(message);
    this.name = "ApiRequestError";
  }
}

export const api = new ApiClient();
