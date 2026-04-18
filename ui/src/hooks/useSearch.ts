import { useState, useRef, useCallback } from "react";
import { api } from "../api/client";
import type { SearchResultGroup } from "../api/types";

const CACHE_KEY = "streamx_search_results";

function getCachedResults(): SearchResultGroup[] {
  try {
    const cached = sessionStorage.getItem(CACHE_KEY);
    return cached ? JSON.parse(cached) : [];
  } catch {
    return [];
  }
}

export function useSearch() {
  const [results, setResults] = useState<SearchResultGroup[]>(getCachedResults);
  const [isLoading, setIsLoading] = useState(false);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const pageRef = useRef(1);
  const queryRef = useRef("");

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
      setHasMore(false);
      sessionStorage.removeItem(CACHE_KEY);
      return;
    }

    queryRef.current = query.trim();
    pageRef.current = 1;
    setIsLoading(true);

    timerRef.current = setTimeout(async () => {
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        const data = await api.search({ query: query.trim(), page: 1 });
        if (!controller.signal.aborted) {
          setResults(data.results);
          setError(null);
          setHasMore(data.results.length >= 10);
          sessionStorage.setItem(CACHE_KEY, JSON.stringify(data.results));
        }
      } catch (err) {
        if (!controller.signal.aborted) {
          setError(err instanceof Error ? err.message : "Search failed");
          setResults([]);
          setHasMore(false);
        }
      } finally {
        if (!controller.signal.aborted) {
          setIsLoading(false);
        }
      }
    }, 300);
  }, []);

  const loadMore = useCallback(async () => {
    if (isLoadingMore || !queryRef.current || !hasMore) return;

    setIsLoadingMore(true);
    const nextPage = pageRef.current + 1;

    try {
      const data = await api.search({ query: queryRef.current, page: nextPage });
      if (data.results.length > 0) {
        let addedCount = 0;
        setResults((prev) => {
          const existing = new Set(prev.map((r) => r.title));
          const newItems = data.results.filter((r: SearchResultGroup) => !existing.has(r.title));
          addedCount = newItems.length;
          if (newItems.length === 0) return prev;
          const merged = [...prev, ...newItems];
          sessionStorage.setItem(CACHE_KEY, JSON.stringify(merged));
          return merged;
        });
        pageRef.current = nextPage;
        // Stop if no new unique results (provider doesn't support pagination)
        if (addedCount === 0) {
          setHasMore(false);
        }
      } else {
        setHasMore(false);
      }
    } catch {
      setHasMore(false);
    } finally {
      setIsLoadingMore(false);
    }
  }, [isLoadingMore, hasMore]);

  return { results, isLoading, isLoadingMore, error, hasMore, search, loadMore };
}
