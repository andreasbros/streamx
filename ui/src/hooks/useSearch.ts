import { useState, useRef, useCallback } from "react";
import { api } from "../api/client";
import type { SearchResult } from "../api/types";

export function useSearch() {
  const [results, setResults] = useState<SearchResult[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const search = useCallback((query: string) => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }
    if (abortRef.current) {
      abortRef.current.abort();
    }

    if (!query.trim()) {
      setResults([]);
      setError(null);
      setIsLoading(false);
      return;
    }

    setIsLoading(true);

    timerRef.current = setTimeout(async () => {
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        const data = await api.search({ query: query.trim() });
        if (!controller.signal.aborted) {
          setResults(data.results);
          setError(null);
        }
      } catch (err) {
        if (!controller.signal.aborted) {
          setError(err instanceof Error ? err.message : "Search failed");
          setResults([]);
        }
      } finally {
        if (!controller.signal.aborted) {
          setIsLoading(false);
        }
      }
    }, 300);
  }, []);

  return { results, isLoading, error, search };
}
