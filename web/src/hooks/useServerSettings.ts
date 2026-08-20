import { useEffect, useState } from "react";
import { api } from "../api/client";
import type { ServerSettings } from "../api/types";

let cached: ServerSettings | null = null;
const listeners = new Set<(s: ServerSettings) => void>();

export function invalidateServerSettings(next?: ServerSettings) {
  cached = next ?? null;
  if (next) listeners.forEach((l) => l(next));
}

/**
 * Server-wide settings (transcode gating, WEB-only mode). Fetched once
 * and shared across components; null until the first load resolves.
 */
export function useServerSettings(): ServerSettings | null {
  const [settings, setSettings] = useState<ServerSettings | null>(cached);

  useEffect(() => {
    listeners.add(setSettings);
    if (!cached) {
      api
        .serverSettings()
        .then((s) => {
          cached = s;
          listeners.forEach((l) => l(s));
        })
        .catch(() => {
          // Leave null; callers fall back to conservative defaults.
        });
    }
    return () => {
      listeners.delete(setSettings);
    };
  }, []);

  return settings;
}
