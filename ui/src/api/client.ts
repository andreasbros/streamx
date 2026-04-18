import { getToken } from "../lib/auth";
import { debugLog } from "../lib/debug-log";
import type {
  ApiError,
  LoginRequest,
  LoginResponse,
  RegisterRequest,
  SearchRequest,
  SearchResponse,
  SearchHistoryResponse,
  Settings,
  StreamRequest,
  StreamResponse,
  StreamStatus,
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
      body: JSON.stringify({ watched_seconds: watchedSeconds }),
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

  getPlaylistUrl(streamId: string): string {
    const token = getToken();
    const base = `/api/stream/${streamId}/playlist.m3u8`;
    return token ? `${base}?token=${encodeURIComponent(token)}` : base;
  }

  getFileUrl(streamId: string): string {
    return `/api/stream/${streamId}/file`;
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
