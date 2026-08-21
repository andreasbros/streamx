import { useState, useEffect, useCallback, useRef } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
  Flex,
  Text,
  Skeleton,
} from "@radix-ui/themes";
import { ArrowLeftIcon, PlayIcon, VideoIcon } from "@radix-ui/react-icons";
import { TrailerModal } from "../components/TrailerModal";
import { FavouriteButton } from "../components/FavouriteButton";
import { Button } from "@radix-ui/themes";
import { api } from "../api/client";
import type { SearchResultGroup } from "../api/types";

const THIS_YEAR = String(new Date().getFullYear());
const CATEGORIES: Record<string, { title: string; sort_by: string; query_term?: string; genre?: string; minimum_rating?: number }> = {
  "this-year": { title: THIS_YEAR, sort_by: "download_count", query_term: THIS_YEAR },
  latest: { title: "Latest", sort_by: "date_added" },
  popular: { title: "Most Popular", sort_by: "download_count" },
  "top-rated": { title: "Top Rated", sort_by: "rating", minimum_rating: 8 },
  action: { title: "Action", sort_by: "download_count", genre: "action" },
  comedy: { title: "Comedy", sort_by: "download_count", genre: "comedy" },
  thriller: { title: "Thriller", sort_by: "download_count", genre: "thriller" },
  horror: { title: "Horror", sort_by: "download_count", genre: "horror" },
  scifi: { title: "Sci-Fi", sort_by: "download_count", genre: "sci-fi" },
  drama: { title: "Drama", sort_by: "download_count", genre: "drama" },
};


function MovieTile({ group, onTrailer }: { group: SearchResultGroup; onTrailer?: (info: { youtubeId?: string; searchQuery?: string }) => void }) {
  const navigate = useNavigate();
  const [imgError, setImgError] = useState(false);
  const poster = group.poster_medium ?? group.poster;

  return (
    <div onClick={() => navigate("/movie", { state: group })} style={{ cursor: "pointer" }}>
      <div style={{ position: "relative" }}>
        {poster && !imgError ? (
          <img
            src={poster}
            alt=""
            loading="lazy"
            onError={() => setImgError(true)}
            style={{ borderRadius: 6, objectFit: "cover", width: "100%", aspectRatio: "2/3", background: "var(--gray-a3)", display: "block" }}
          />
        ) : (
          <Flex align="center" justify="center" style={{ width: "100%", aspectRatio: "2/3", borderRadius: 6, background: "var(--gray-a3)" }}>
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
      <Text size="1" weight="medium" style={{ display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden", marginTop: 4, lineHeight: 1.3 }}>
        {group.title}
      </Text>
      <Flex gap="1" align="center" mt="1">
        {group.year && <Text size="1" color="gray">{group.year}</Text>}
        {group.rating != null && group.rating > 0 && (
          <Text size="1" color="amber">{"\u2605"}{group.rating.toFixed(1)}</Text>
        )}
      </Flex>
    </div>
  );
}

export function Browse() {
  const { category } = useParams<{ category: string }>();
  const navigate = useNavigate();
  const config = CATEGORIES[category ?? ""] ?? { title: "Latest", sort_by: "date_added" };

  const [groups, setGroups] = useState<SearchResultGroup[]>([]);
  const pageRef = useRef(1);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const [trailerInfo, setTrailerInfo] = useState<{ youtubeId?: string; searchQuery?: string } | null>(null);

  const fetchPage = useCallback(async (p: number) => {
    const isFirst = p === 1;
    if (isFirst) setLoading(true); else setLoadingMore(true);
    try {
      const res = await api.browse({
        sort_by: config.sort_by,
        query_term: config.query_term,
        genre: config.genre,
        minimum_rating: config.minimum_rating,
        limit: 20,
        page: p,
      });
      if (res.results.length < 20) setHasMore(false);
      setGroups((prev) => {
        if (isFirst) return res.results;
        const existing = new Set(prev.map((g) => `${g.title}-${g.year}`));
        const newOnes = res.results.filter((g) => !existing.has(`${g.title}-${g.year}`));
        return [...prev, ...newOnes];
      });
    } catch { /* ignore */ }
    if (isFirst) setLoading(false); else setLoadingMore(false);
  }, [config]);

  useEffect(() => {
    setGroups([]);
    pageRef.current = 1;
    setHasMore(true);
    fetchPage(1);
  }, [fetchPage]);

  // Infinite scroll
  useEffect(() => {
    if (!sentinelRef.current || !hasMore) return;
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
  }, [hasMore, loadingMore, fetchPage]);

  return (
    <>
    <Flex direction="column" gap="4">
      <Flex align="center" gap="3">
        <Button variant="ghost" size="1" onClick={() => navigate(-1)}>
          <ArrowLeftIcon width={18} height={18} />
        </Button>
        <Text size="5" weight="bold">{config.title}</Text>
      </Flex>

      <div className="browse-grid" style={{
        display: "grid",
        gap: 12,
      }}>
        {loading
          ? Array.from({ length: 12 }).map((_, i) => (
              <div key={i}>
                <Skeleton width="100%" style={{ aspectRatio: "2/3", borderRadius: 6 }} />
                <Skeleton height="12px" width="80%" style={{ marginTop: 6 }} />
              </div>
            ))
          : groups.map((g, i) => {
              const key = `${g.title}-${g.year ?? i}`;
              return <MovieTile key={key} group={g} onTrailer={setTrailerInfo} />;
            })
        }
      </div>

      {loadingMore && (
        <Flex justify="center" py="2">
          <Text size="2" color="gray">Loading...</Text>
        </Flex>
      )}
      {hasMore && <div ref={sentinelRef} style={{ height: 1 }} />}
      {!hasMore && groups.length > 0 && (
        <Flex justify="center" py="4">
          <Text size="2" color="gray">No more results</Text>
        </Flex>
      )}
    </Flex>
    {trailerInfo && <TrailerModal youtubeId={trailerInfo.youtubeId} searchQuery={trailerInfo.searchQuery} onClose={() => setTrailerInfo(null)} />}
    </>
  );
}
