import { useState, useEffect, useCallback, useRef } from "react";
import { useNavigate } from "react-router-dom";
import {
  Box,
  Flex,
  Text,
  TextField,
  Card,
  Badge,
  Skeleton,
  IconButton,
} from "@radix-ui/themes";
import {
  MagnifyingGlassIcon,
  Cross2Icon,
} from "@radix-ui/react-icons";
import { useSearchTv } from "../hooks/useSearchTv";
import { api } from "../api/client";
import type { TvSearchResultGroup } from "../api/types";

function TvShowCard({
  group,
  onClick,
}: {
  group: TvSearchResultGroup;
  onClick: () => void;
}) {
  const totalEpisodes = group.seasons.reduce((sum, s) => sum + s.episodes.length, 0);
  const totalSeeds = group.seasons
    .flatMap((s) => s.episodes)
    .flatMap((e) => e.variants)
    .reduce((sum, v) => sum + v.seeds, 0);
  const hasEpisodes = totalEpisodes > 0;

  return (
    <Card size="2" onClick={onClick} style={{ cursor: "pointer" }}>
      <Flex direction="column" gap="1">
        <Text size="2" weight="medium">
          {group.show_name}
        </Text>
        <Flex gap="1" wrap="wrap" align="center">
          {hasEpisodes ? (
            <>
              <Badge size="1" variant="soft" color="blue">
                {group.seasons.length} season{group.seasons.length !== 1 ? "s" : ""}
              </Badge>
              <Badge size="1" variant="soft" color="gray">
                {totalEpisodes} ep{totalEpisodes !== 1 ? "s" : ""}
              </Badge>
              <Text size="1" color="green">
                {totalSeeds} seeds
              </Text>
            </>
          ) : (
            <Text size="1" color="gray">
              Tap to browse episodes
            </Text>
          )}
        </Flex>
      </Flex>
    </Card>
  );
}

function SkeletonList() {
  return (
    <Flex direction="column" gap="2">
      {Array.from({ length: 8 }).map((_, i) => (
        <Card size="2" key={i}>
          <Flex direction="column" gap="2">
            <Skeleton height="14px" width="70%" />
            <Skeleton height="12px" width="40%" />
          </Flex>
        </Card>
      ))}
    </Flex>
  );
}

export function TvSearch() {
  const navigate = useNavigate();
  const { results, isLoading, error, search } = useSearchTv();
  const [query, setQuery] = useState("");

  // Browse state with infinite scroll
  const [browseGroups, setBrowseGroups] = useState<TvSearchResultGroup[]>([]);
  const [browseLoading, setBrowseLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const pageRef = useRef(1);
  const sentinelRef = useRef<HTMLDivElement>(null);

  const fetchPage = useCallback(async (p: number) => {
    const isFirst = p === 1;
    if (isFirst) setBrowseLoading(true); else setLoadingMore(true);
    try {
      const res = await api.browseTv({ limit: 30, page: p });
      if (res.results.length === 0) {
        setHasMore(false);
        if (isFirst) setBrowseLoading(false); else setLoadingMore(false);
        return;
      }
      setBrowseGroups((prev) => {
        if (isFirst) return res.results;
        const existing = new Set(prev.map((g) => g.show_name));
        const newOnes = res.results.filter((g) => !existing.has(g.show_name));
        if (newOnes.length === 0) setHasMore(false);
        return [...prev, ...newOnes];
      });
    } catch {
      setHasMore(false);
    }
    if (isFirst) setBrowseLoading(false); else setLoadingMore(false);
  }, []);

  useEffect(() => {
    fetchPage(1);
  }, [fetchPage]);

  // Infinite scroll observer
  useEffect(() => {
    if (!sentinelRef.current || !hasMore || query) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !loadingMore && hasMore) {
          pageRef.current += 1;
          fetchPage(pageRef.current);
        }
      },
      { rootMargin: "200px" }
    );
    observer.observe(sentinelRef.current);
    return () => observer.disconnect();
  }, [hasMore, loadingMore, fetchPage, query]);

  const handleInputChange = (value: string) => {
    setQuery(value);
    search(value);
  };

  const handleClear = () => {
    setQuery("");
    search("");
  };

  const showSearch = query.length > 0;
  const displayGroups = showSearch ? results : browseGroups;
  const displayLoading = showSearch ? isLoading : browseLoading;

  return (
    <Flex direction="column" gap="4">
      <Text size="5" weight="bold">
        TV Shows
      </Text>

      <Flex gap="2" align="end">
        <Box flexGrow="1">
          <TextField.Root
            size="3"
            placeholder="Search TV shows..."
            value={query}
            onChange={(e) => handleInputChange(e.target.value)}
          >
            <TextField.Slot>
              <MagnifyingGlassIcon />
            </TextField.Slot>
            {query && (
              <TextField.Slot>
                <IconButton size="1" variant="ghost" onClick={handleClear}>
                  <Cross2Icon width={14} height={14} />
                </IconButton>
              </TextField.Slot>
            )}
          </TextField.Root>
        </Box>
      </Flex>

      {error && (
        <Text size="2" color="red">
          {error}
        </Text>
      )}

      {!showSearch && (
        <Text size="3" weight="bold">
          Latest
        </Text>
      )}

      {showSearch && !isLoading && results.length > 0 && (
        <Text size="2" color="gray">
          {results.length} show{results.length !== 1 ? "s" : ""}
        </Text>
      )}

      {displayLoading ? (
        <SkeletonList />
      ) : displayGroups.length === 0 ? (
        <Flex justify="center" py="6">
          <Text size="2" color="gray">
            {showSearch ? "No shows found" : "No TV shows available"}
          </Text>
        </Flex>
      ) : (
        <Flex direction="column" gap="2">
          {displayGroups.map((g) => (
            <TvShowCard
              key={g.show_name}
              group={g}
              onClick={() => navigate("/tv/show", { state: g })}
            />
          ))}
        </Flex>
      )}

      {!showSearch && loadingMore && (
        <Flex justify="center" py="2">
          <Text size="2" color="gray">Loading...</Text>
        </Flex>
      )}
      {!showSearch && hasMore && <div ref={sentinelRef} style={{ height: 1 }} />}
      {!showSearch && !hasMore && browseGroups.length > 0 && (
        <Flex justify="center" py="4">
          <Text size="2" color="gray">No more results</Text>
        </Flex>
      )}
    </Flex>
  );
}
