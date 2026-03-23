import { useState, useRef, useCallback } from "react";
import { api } from "../api/client";
import type { TvSearchResultGroup } from "../api/types";

export function useSearchTv() {
  const [results, setResults] = useState<TvSearchResultGroup[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const search = useCallback((query: string) => {
    if (timerRef.current) clearTimeout(timerRef.current);

    if (!query.trim()) {
      setResults([]);
      setIsLoading(false);
      setError(null);
      return;
    }

    setIsLoading(true);
    setError(null);

    timerRef.current = setTimeout(async () => {
      try {
        const res = await api.searchTv({ query: query.trim() });
        setResults(res.results);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Search failed");
        setResults([]);
      } finally {
        setIsLoading(false);
      }
    }, 400);
  }, []);

  return { results, isLoading, error, search };
}
