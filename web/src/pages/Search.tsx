import { useState, useEffect, useCallback, useRef } from "react";
import { useNavigate } from "react-router-dom";
import {
  Box,
  Flex,
  Text,
  TextField,
  Button,
  Card,
  Badge,
  Select,
  Skeleton,
  Separator,
  IconButton,
} from "@radix-ui/themes";
import {
  MagnifyingGlassIcon,
  PlayIcon,
  Link2Icon,
  ChevronDownIcon,
  ChevronUpIcon,
  Cross2Icon,
  VideoIcon,
  ArrowLeftIcon,
  ArrowRightIcon,
} from "@radix-ui/react-icons";
import { TrailerModal } from "../components/TrailerModal";
import { FavouriteButton } from "../components/FavouriteButton";
import { useSearch } from "../hooks/useSearch";
import { useServerSettings } from "../hooks/useServerSettings";
import { api } from "../api/client";
import { isMagnetLink, formatBytes, formatRuntime } from "../lib/utils";
import type { SearchResultGroup, SearchResult } from "../api/types";

type SortKey = "seeds" | "size" | "year" | "rating";

function bestSeeds(group: SearchResultGroup): number {
  return Math.max(0, ...group.variants.map((v) => v.seeds));
}

function largestSize(group: SearchResultGroup): number {
  return Math.max(0, ...group.variants.map((v) => v.size_bytes));
}

function sortGroups(
  groups: SearchResultGroup[],
  key: SortKey
): SearchResultGroup[] {
  return [...groups].sort((a, b) => {
    switch (key) {
      case "seeds":
        return bestSeeds(b) - bestSeeds(a);
      case "size":
        return largestSize(b) - largestSize(a);
      case "year":
        return (b.year ?? 0) - (a.year ?? 0);
      case "rating":
        return (b.rating ?? 0) - (a.rating ?? 0);
    }
  });
}

function qualityLabel(quality: string | undefined): string {
  if (!quality) return "?";
  const q = quality.toLowerCase();
  if (q.includes("2160") || q.includes("4k")) return "4K";
  if (q.includes("1080")) return "FHD";
  if (q.includes("720")) return "HD";
  if (q.includes("480") || q.includes("360")) return "SD";
  return quality;
}

function qualityColor(quality: string | undefined): "blue" | "green" | "orange" | "purple" | "gray" {
  if (!quality) return "gray";
  const q = quality.toLowerCase();
  if (q.includes("2160") || q.includes("4k")) return "purple";
  if (q.includes("1080")) return "blue";
  if (q.includes("720")) return "green";
  if (q.includes("480") || q.includes("360")) return "orange";
  return "gray";
}

function formatSourceType(src: string): string {
  if (src === "bluray") return "BluRay";
  if (src === "web") return "WEB";
  return src.toUpperCase();
}

function GroupCard({
  group,
  isExpanded,
  onToggle,
  onPlayVariant,
}: {
  group: SearchResultGroup;
  isExpanded: boolean;
  onToggle: () => void;
  onPlayVariant: (variant: SearchResult, group: SearchResultGroup) => void;
}) {
  const [imgError, setImgError] = useState(false);
  const poster = group.poster_small ?? group.poster;
  const serverSettings = useServerSettings();
  const transcodeDisabled = serverSettings?.disable_transcode ?? true;

  return (
    <Card size="2">
      {/* Collapsed summary row */}
      <Flex
        gap="3"
        align="start"
        onClick={onToggle}
        style={{ cursor: "pointer" }}
      >
        {poster && !imgError ? (
          <img
            src={poster}
            alt=""
            loading="lazy"
            width={48}
            height={72}
            onError={() => setImgError(true)}
            style={{
              borderRadius: 4,
              objectFit: "cover",
              flexShrink: 0,
              background: "var(--gray-a3)",
            }}
          />
        ) : (
          <Flex
            align="center"
            justify="center"
            style={{
              width: 48,
              height: 72,
              borderRadius: 4,
              background: "var(--gray-a3)",
              flexShrink: 0,
            }}
          >
            <PlayIcon />
          </Flex>
        )}

        <Flex
          direction="column"
          gap="1"
          style={{ minWidth: 0, flex: 1 }}
        >
          <Flex align="center" gap="2">
            <Text
              size="2"
              weight="medium"
              style={{
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {group.title}
            </Text>
            {group.year && (
              <Text size="2" color="gray" style={{ flexShrink: 0 }}>
                ({group.year})
              </Text>
            )}
          </Flex>

          <Flex gap="1" wrap="wrap" align="center">
            {[...new Set(group.variants.map((v) => qualityLabel(v.quality)))].map((label) => (
              <Badge
                key={label}
                size="1"
                variant="surface"
                color={qualityColor(
                  group.variants.find((v) => qualityLabel(v.quality) === label)?.quality
                )}
              >
                {label}
              </Badge>
            ))}
            {group.rating != null && group.rating > 0 && (
              <Badge size="1" variant="soft" color="amber">
                {group.rating.toFixed(1)}
              </Badge>
            )}
            {group.runtime != null && group.runtime > 0 && (
              <Badge size="1" variant="soft" color="gray">
                {formatRuntime(group.runtime)}
              </Badge>
            )}
          </Flex>

          <Flex align="center" gap="2">
            <Text size="1" color="green">
              {bestSeeds(group)}
            </Text>
            <Text size="1" color="gray">
              best seeds
            </Text>
            {isExpanded ? (
              <ChevronUpIcon style={{ marginLeft: "auto" }} />
            ) : (
              <ChevronDownIcon style={{ marginLeft: "auto" }} />
            )}
          </Flex>
        </Flex>
      </Flex>

      {/* Expanded detail view */}
      {isExpanded && (
        <Box mt="3">
          <Separator size="4" mb="3" />
          <Flex gap="4" wrap="wrap">
            {/* Big poster on left */}
            {(group.poster_large ?? group.poster) && !imgError ? (
              <img
                src={group.poster_large ?? group.poster}
                alt=""
                loading="lazy"
                width={180}
                style={{
                  borderRadius: 6,
                  objectFit: "cover",
                  flexShrink: 0,
                  background: "var(--gray-a3)",
                  maxHeight: 270,
                }}
              />
            ) : null}

            {/* Details on right */}
            <Flex
              direction="column"
              gap="2"
              style={{ flex: 1, minWidth: 200 }}
            >
              <Flex align="baseline" gap="2" wrap="wrap">
                <Text size="4" weight="bold">
                  {group.title}
                </Text>
                {group.year && (
                  <Text size="3" color="gray">
                    ({group.year})
                  </Text>
                )}
                {group.rating != null && group.rating > 0 && (
                  <Text size="3" color="amber">
                    {"\u2605"} {group.rating.toFixed(1)}
                  </Text>
                )}
              </Flex>

              <Flex gap="2" align="center" wrap="wrap">
                {group.runtime != null && group.runtime > 0 && (
                  <Text size="2" color="gray">
                    {formatRuntime(group.runtime)}
                  </Text>
                )}
                {group.genres && group.genres.length > 0 && (
                  <Text size="2" color="gray">
                    {group.runtime != null && group.runtime > 0
                      ? "\u00B7 "
                      : ""}
                    {group.genres.slice(0, 4).join(" / ")}
                  </Text>
                )}
                {group.language &&
                  group.language !== "en" && (
                    <Badge size="1" variant="soft">
                      {group.language.toUpperCase()}
                    </Badge>
                  )}
                {group.mpa_rating &&
                  group.mpa_rating !== "" && (
                    <Badge size="1" variant="outline">
                      {group.mpa_rating}
                    </Badge>
                  )}
              </Flex>

              {group.summary && (
                <Text
                  size="2"
                  color="gray"
                  style={{
                    maxHeight: 60,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    display: "-webkit-box",
                    WebkitLineClamp: 3,
                    WebkitBoxOrient: "vertical",
                  }}
                >
                  {group.summary}
                </Text>
              )}

              {/* Variant table */}
              <Box
                mt="2"
                style={{
                  border: "1px solid var(--gray-a5)",
                  borderRadius: 6,
                  overflow: "hidden",
                }}
              >
                {group.variants.map((variant, idx) => {
                  const notWebCompatible =
                    transcodeDisabled && variant.source_type !== "web";
                  return (
                  <Flex
                    key={variant.magnet}
                    align="center"
                    gap="2"
                    px="3"
                    py="2"
                    onClick={(e) => {
                      e.stopPropagation();
                      if (!notWebCompatible) onPlayVariant(variant, group);
                    }}
                    onDoubleClick={(e) => {
                      e.stopPropagation();
                      if (notWebCompatible) onPlayVariant(variant, group);
                    }}
                    style={{
                      cursor: notWebCompatible ? "default" : "pointer",
                      borderTop:
                        idx > 0
                          ? "1px solid var(--gray-a4)"
                          : undefined,
                    }}
                  >
                    <Badge size="1" variant="solid" color={qualityColor(variant.quality)} style={{ flexShrink: 0 }}>
                      {qualityLabel(variant.quality)}
                    </Badge>
                    {variant.source_type && (
                      <Text size="1" color="gray" style={{ flexShrink: 0 }}>
                        {formatSourceType(variant.source_type)}
                      </Text>
                    )}
                    {variant.video_codec && (
                      <Text size="1" color="gray" style={{ flexShrink: 0 }}>
                        {variant.video_codec}
                      </Text>
                    )}
                    {variant.audio_channels && (
                      <Text size="1" color="gray" style={{ flexShrink: 0 }}>
                        {variant.audio_channels}
                      </Text>
                    )}
                    <Text size="1" color="gray" style={{ flex: 1, minWidth: 0 }}>
                      {variant.size || formatBytes(variant.size_bytes)}
                    </Text>
                    <Text size="1" color="green" style={{ flexShrink: 0, textAlign: "right" }}>
                      {variant.seeds}
                    </Text>
                    {variant.leeches > 0 && (
                      <Text size="1" color="red" style={{ flexShrink: 0, textAlign: "right" }}>
                        {variant.leeches}
                      </Text>
                    )}
                    {notWebCompatible ? (
                      <Badge
                        size="1"
                        variant="soft"
                        color="gray"
                        style={{ flexShrink: 0 }}
                        title="Server transcoding is disabled; double-click to try direct playback"
                      >
                        Not WEB compatible
                      </Badge>
                    ) : (
                      <PlayIcon width={14} height={14} style={{ flexShrink: 0 }} />
                    )}
                  </Flex>
                  );
                })}
              </Box>
            </Flex>
          </Flex>
        </Box>
      )}
    </Card>
  );
}

function SkeletonCard() {
  return (
    <Card size="2">
      <Flex gap="3">
        <Skeleton width="48px" height="72px" style={{ borderRadius: 4 }} />
        <Flex direction="column" gap="2" flexGrow="1">
          <Skeleton height="14px" width="80%" />
          <Skeleton height="12px" width="50%" />
          <Skeleton height="12px" width="40%" />
        </Flex>
      </Flex>
    </Card>
  );
}

// --- Movie tile for browse sections ---
function MovieTile({ group, onTrailer }: { group: SearchResultGroup; onTrailer?: (info: { youtubeId?: string; searchQuery?: string }) => void }) {
  const navigate = useNavigate();
  const [imgError, setImgError] = useState(false);
  const poster = group.poster_medium ?? group.poster;

  return (
    <div
      onClick={() => navigate("/movie", { state: group })}
      style={{ cursor: "pointer", width: 120, flexShrink: 0 }}
    >
      <div style={{ position: "relative" }}>
        {poster && !imgError ? (
          <img
            src={poster}
            alt=""
            loading="lazy"
            width={120}
            height={180}
            onError={() => setImgError(true)}
            style={{ borderRadius: 6, objectFit: "cover", width: 120, height: 180, background: "var(--gray-a3)", display: "block" }}
          />
        ) : (
          <Flex align="center" justify="center" style={{ width: 120, height: 180, borderRadius: 6, background: "var(--gray-a3)" }}>
            <PlayIcon width={24} height={24} />
          </Flex>
        )}
        <div
          onClick={(e) => {
            e.stopPropagation();
            onTrailer?.(group.trailer_code
              ? { youtubeId: group.trailer_code }
              : { searchQuery: `${group.title} ${group.year ?? ""} official trailer` });
          }}
          style={{ position: "absolute", bottom: 6, right: 6, width: 30, height: 30, borderRadius: "50%", background: group.trailer_code ? "rgba(220,38,38,0.85)" : "rgba(120,120,120,0.7)", display: "flex", alignItems: "center", justifyContent: "center" }}
        >
          <VideoIcon width={16} height={16} color="white" />
        </div>
        <FavouriteButton
          group={group}
          size={24}
          style={{ position: "absolute", top: 6, right: 6 }}
        />
      </div>
      <div style={{ height: 48, marginTop: 4, overflow: "hidden" }}>
        <Text size="1" weight="medium" style={{ display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden", lineHeight: 1.3 }}>
          {group.title}
        </Text>
        <Flex gap="1" align="center" mt="1">
          {group.year && <Text size="1" color="gray">{group.year}</Text>}
          {group.rating != null && group.rating > 0 && (
            <Text size="1" color="amber">{"\u2605"}{group.rating.toFixed(1)}</Text>
          )}
        </Flex>
      </div>
    </div>
  );
}

// --- Browse section: horizontal row with nav arrows and infinite scroll ---
function BrowseSection({
  title,
  category,
  groups: initialGroups,
  isLoading,
  onTrailer,
  browseParams,
}: {
  title: string;
  category: string;
  groups: SearchResultGroup[];
  isLoading: boolean;
  onPlay: (variant: SearchResult, group: SearchResultGroup) => void;
  onTrailer: (info: { youtubeId?: string; searchQuery?: string }) => void;
  browseParams: Record<string, unknown>;
}) {
  const navigate = useNavigate();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [groups, setGroups] = useState<SearchResultGroup[]>(initialGroups);
  const [page, setPage] = useState(1);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const sentinelRef = useRef<HTMLDivElement>(null);

  // Sync initial groups from parent
  useEffect(() => {
    if (initialGroups.length > 0) {
      setGroups(initialGroups);
      setPage(1);
      setHasMore(true);
    }
  }, [initialGroups]);

  // Infinite scroll: observe sentinel at end of row
  useEffect(() => {
    const sentinel = sentinelRef.current;
    if (!sentinel || !hasMore || groups.length === 0) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !loadingMore && hasMore) {
          setLoadingMore(true);
          const nextPage = page + 1;
          api.browse({ ...(browseParams as Record<string, string | number>), limit: 10, page: nextPage })
            .then((res) => {
              if (res.results.length === 0) {
                setHasMore(false);
              } else {
                setGroups((prev) => {
                  const existingTitles = new Set(prev.map((g) => `${g.title}-${g.year}`));
                  const newGroups = res.results.filter((g) => !existingTitles.has(`${g.title}-${g.year}`));
                  return [...prev, ...newGroups];
                });
                setPage(nextPage);
              }
            })
            .catch(() => setHasMore(false))
            .finally(() => setLoadingMore(false));
        }
      },
      { root: scrollRef.current, rootMargin: "0px 200px 0px 0px", threshold: 0 }
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [groups.length, page, loadingMore, hasMore, browseParams]);

  const scroll = (direction: "left" | "right") => {
    const el = scrollRef.current;
    if (!el) return;
    const amount = el.clientWidth * 0.75;
    el.scrollBy({ left: direction === "left" ? -amount : amount, behavior: "smooth" });
  };

  return (
    <Flex direction="column" gap="2">
      <Flex align="center" justify="between">
        <Text
          size="3"
          weight="bold"
          onClick={() => navigate(`/browse/${category}`)}
          style={{ cursor: "pointer" }}
        >
          {title} <ChevronDownIcon width={14} height={14} style={{ verticalAlign: "middle", opacity: 0.5 }} />
        </Text>
        <Flex gap="1">
          <button
            onClick={() => scroll("left")}
            style={{ background: "var(--gray-a3)", border: "1px solid var(--gray-a5)", borderRadius: 4, width: 28, height: 28, cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center" }}
          >
            <ArrowLeftIcon width={14} height={14} />
          </button>
          <button
            onClick={() => scroll("right")}
            style={{ background: "var(--gray-a3)", border: "1px solid var(--gray-a5)", borderRadius: 4, width: 28, height: 28, cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center" }}
          >
            <ArrowRightIcon width={14} height={14} />
          </button>
        </Flex>
      </Flex>
      <div
        ref={scrollRef}
        style={{
          display: "flex",
          gap: 12,
          overflowX: "auto",
          paddingBottom: 8,
          minHeight: 230,
          WebkitOverflowScrolling: "touch",
          scrollbarWidth: "none",
        }}
      >
        {isLoading
          ? Array.from({ length: 6 }).map((_, i) => (
              <div key={i} style={{ width: 120, flexShrink: 0 }}>
                <Skeleton width="120px" height="180px" style={{ borderRadius: 6 }} />
                <Skeleton height="12px" width="80px" style={{ marginTop: 6 }} />
              </div>
            ))
          : groups.map((g, i) => (
              <MovieTile key={`${g.title}-${g.year ?? i}`} group={g} onTrailer={onTrailer} />
            ))
        }
        {loadingMore && (
          <div style={{ width: 120, flexShrink: 0, display: "flex", alignItems: "center", justifyContent: "center" }}>
            <Skeleton width="120px" height="180px" style={{ borderRadius: 6 }} />
          </div>
        )}
        {hasMore && <div ref={sentinelRef} style={{ width: 1, flexShrink: 0 }} />}
      </div>
    </Flex>
  );
}

const SEARCH_QUERY_KEY = "streamx_search_query";

export function Search() {
  const navigate = useNavigate();
  const { results, isLoading, isLoadingMore, error, hasMore, search, loadMore } = useSearch();
  const [query, setQuery] = useState(() => {
    return sessionStorage.getItem(SEARCH_QUERY_KEY) || "";
  });
  const [sortKey, setSortKey] = useState<SortKey>("year");
  const [starting] = useState(false);
  const [expandedIndex, setExpandedIndex] = useState<number | null>(() => {
    const saved = sessionStorage.getItem("streamx_expanded_group");
    return saved ? parseInt(saved, 10) : null;
  });

  // Browse sections
  const browseSections = [
    { title: "Latest", category: "latest", params: { sort_by: "date_added" } },
    { title: "Most Popular", category: "popular", params: { sort_by: "download_count" } },
    { title: "Top Rated", category: "top-rated", params: { sort_by: "rating", minimum_rating: 8 } },
    { title: "Action", category: "action", params: { sort_by: "download_count", genre: "action" } },
    { title: "Comedy", category: "comedy", params: { sort_by: "download_count", genre: "comedy" } },
    { title: "Thriller", category: "thriller", params: { sort_by: "download_count", genre: "thriller" } },
    { title: "Sci-Fi", category: "scifi", params: { sort_by: "download_count", genre: "sci-fi" } },
    { title: "Horror", category: "horror", params: { sort_by: "download_count", genre: "horror" } },
  ];
  const BROWSE_CACHE_KEY = "streamx_browse_cache";
  const BROWSE_CACHE_TTL = 3600000; // 1 hour

  const [browseData, setBrowseData] = useState<Record<string, SearchResultGroup[]>>(() => {
    try {
      const cached = sessionStorage.getItem(BROWSE_CACHE_KEY);
      if (cached) {
        const { data, ts } = JSON.parse(cached);
        const hasContent = Object.values(data).some((arr: unknown) => Array.isArray(arr) && (arr as unknown[]).length > 0);
        if (Date.now() - ts < BROWSE_CACHE_TTL && hasContent) return data;
      }
    } catch { /* ignore */ }
    return {};
  });
  const [trailerInfo, setTrailerInfo] = useState<{ youtubeId?: string; searchQuery?: string } | null>(null);
  const [browseLoading, setBrowseLoading] = useState(() => Object.keys(browseData).length === 0);

  const fetchBrowse = useCallback(async () => {
    setBrowseLoading(true);
    const results = await Promise.all(
      browseSections.map((s) => api.browse({ ...s.params, limit: 10 }).catch(() => ({ results: [] as SearchResultGroup[] })))
    );
    const data: Record<string, SearchResultGroup[]> = {};
    browseSections.forEach((s, i) => { data[s.category] = results[i]?.results ?? []; });
    setBrowseData(data);
    setBrowseLoading(false);
    try {
      sessionStorage.setItem(BROWSE_CACHE_KEY, JSON.stringify({ data, ts: Date.now() }));
    } catch { /* ignore quota errors */ }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const saved = sessionStorage.getItem(SEARCH_QUERY_KEY);
    if (saved && saved.trim() && !isMagnetLink(saved) && results.length === 0) {
      search(saved);
    }
    const hasContent = Object.values(browseData).some((arr) => arr.length > 0);
    if (!hasContent) fetchBrowse();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleClear = () => {
    setQuery("");
    sessionStorage.removeItem(SEARCH_QUERY_KEY);
    setExpandedIndex(null);
    search("");
  };

  const handleInputChange = (value: string) => {
    setQuery(value);
    sessionStorage.setItem(SEARCH_QUERY_KEY, value);
    setExpandedIndex(null);
    if (!isMagnetLink(value)) {
      search(value);
    }
  };

  const startAndNavigate = (
    variant: SearchResult,
    group: SearchResultGroup
  ) => {
    const tempId = `pending-${Date.now()}`;
    navigate(`/player/${tempId}`, {
      state: {
        magnet: variant.magnet,
        poster: group.poster_large || group.poster || group.poster_medium || null,
        meta: {
          title: group.title,
          year: group.year,
          rating: group.rating,
          runtime: group.runtime,
          genres: group.genres,
          language: group.language,
          mpa_rating: group.mpa_rating,
          summary: group.summary,
          imdb_code: group.imdb_code,
          trailer_code: group.trailer_code,
          poster: group.poster,
          poster_small: group.poster_small,
          poster_medium: group.poster_medium,
          poster_large: group.poster_large,
          backdrop: group.backdrop,
          video_codec: variant.video_codec,
          audio_channels: variant.audio_channels,
          bit_depth: variant.bit_depth,
          source_type: variant.source_type,
        },
      },
    });
  };

  const handleMagnetSubmit = () => {
    if (!isMagnetLink(query)) return;
    const tempId = `pending-${Date.now()}`;
    navigate(`/player/${tempId}`, {
      state: { magnet: query.trim() },
    });
  };

  const handlePlayVariant = (
    variant: SearchResult,
    group: SearchResultGroup
  ) => {
    startAndNavigate(variant, group);
  };

  const sorted = sortGroups(results, sortKey);
  const isMagnet = isMagnetLink(query);

  return (
    <>
    <Flex direction="column" gap="4">
      <Flex gap="2" align="end">
        <Box flexGrow="1">
          <TextField.Root
            size="3"
            placeholder="Search movies or paste a magnet link..."
            value={query}
            onChange={(e) => handleInputChange(e.target.value)}
          >
            <TextField.Slot>
              {isMagnet ? <Link2Icon /> : <MagnifyingGlassIcon />}
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

        {isMagnet && (
          <Button size="3" onClick={handleMagnetSubmit} disabled={starting}>
            <PlayIcon />
            Stream
          </Button>
        )}
      </Flex>

      {!query && (
        <Flex direction="column" gap="3">
          {browseSections.map((s) => (
            <BrowseSection
              key={s.category}
              title={s.title}
              category={s.category}
              groups={browseData[s.category] ?? []}
              isLoading={browseLoading}
              onPlay={handlePlayVariant}
              onTrailer={setTrailerInfo}
              browseParams={s.params}
            />
          ))}
        </Flex>
      )}

      {error && (
        <Text size="2" color="red">
          {error}
        </Text>
      )}

      {query && !isMagnet && results.length > 0 && (
        <Flex justify="between" align="center">
          <Text size="2" color="gray">
            {results.length} title{results.length !== 1 ? "s" : ""}
          </Text>
          <Select.Root
            value={sortKey}
            onValueChange={(v) => {
              setSortKey(v as SortKey);
              setExpandedIndex(null);
            }}
          >
            <Select.Trigger variant="ghost" />
            <Select.Content>
              <Select.Item value="seeds">Most Seeds</Select.Item>
              <Select.Item value="size">Largest</Select.Item>
              <Select.Item value="year">Newest</Select.Item>
              <Select.Item value="rating">Best Rated</Select.Item>
            </Select.Content>
          </Select.Root>
        </Flex>
      )}

      {isLoading && !isMagnet && (
        <Flex direction="column" gap="2">
          {Array.from({ length: 6 }).map((_, i) => (
            <SkeletonCard key={i} />
          ))}
        </Flex>
      )}

      {!isLoading && sorted.length > 0 && (
        <Flex direction="column" gap="2">
          {sorted.map((group, i) => (
            <GroupCard
              key={`${group.title}-${group.year ?? i}`}
              group={group}
              isExpanded={expandedIndex === i}
              onToggle={() =>
                {
                  const next = expandedIndex === i ? null : i;
                  setExpandedIndex(next);
                  if (next !== null) {
                    sessionStorage.setItem("streamx_expanded_group", String(next));
                  } else {
                    sessionStorage.removeItem("streamx_expanded_group");
                  }
                }
              }
              onPlayVariant={handlePlayVariant}
            />
          ))}
          {hasMore && (
            <Flex justify="center" py="3">
              <Button variant="soft" size="2" onClick={loadMore} disabled={isLoadingMore}>
                {isLoadingMore ? "Loading..." : "Load more"}
              </Button>
            </Flex>
          )}
        </Flex>
      )}

      {query && !isMagnet && !isLoading && results.length === 0 && (
        <Flex justify="center" py="6">
          <Text size="2" color="gray">
            No results found
          </Text>
        </Flex>
      )}

      {starting && (
        <Flex justify="center" py="4">
          <Text size="2" color="gray">
            Starting stream...
          </Text>
        </Flex>
      )}
    </Flex>
    {trailerInfo && <TrailerModal youtubeId={trailerInfo.youtubeId} searchQuery={trailerInfo.searchQuery} onClose={() => setTrailerInfo(null)} />}
    </>
  );

}
