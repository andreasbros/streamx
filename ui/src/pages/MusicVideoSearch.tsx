import { useState, useEffect, useCallback, useRef } from "react";
import { useNavigate } from "react-router-dom";
import {
  Box,
  Flex,
  Text,
  TextField,
  Card,
  Skeleton,
  IconButton,
} from "@radix-ui/themes";
import {
  MagnifyingGlassIcon,
  Cross2Icon,
  PlayIcon,
} from "@radix-ui/react-icons";
import { api } from "../api/client";
import type { MusicVideoResult } from "../api/types";

function useSearchMusicVideos() {
  const [results, setResults] = useState<MusicVideoResult[]>([]);
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
        const res = await api.searchMusicVideos({ query: query.trim() });
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

function ResultCard({
  item,
  onPlay,
}: {
  item: MusicVideoResult;
  onPlay: (item: MusicVideoResult) => void;
}) {
  return (
    <Card
      size="1"
      onClick={() => onPlay(item)}
      style={{ cursor: "pointer" }}
    >
      <Flex align="center" gap="3">
        <PlayIcon width={16} height={16} style={{ flexShrink: 0, opacity: 0.5 }} />
        <Flex direction="column" gap="0" style={{ flex: 1, minWidth: 0 }}>
          <Text
            size="2"
            weight="medium"
            style={{
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {item.title}
          </Text>
          <Text size="1" color="gray">
            {item.size}
          </Text>
        </Flex>
        <Flex direction="column" align="end" gap="0" style={{ flexShrink: 0 }}>
          <Text size="1" color="green">
            {item.seeds}
          </Text>
          <Text size="1" color="red">
            {item.leeches}
          </Text>
        </Flex>
      </Flex>
    </Card>
  );
}

export function MusicVideoSearch() {
  const navigate = useNavigate();
  const { results, isLoading, error, search } = useSearchMusicVideos();
  const [query, setQuery] = useState("");
  const [browseResults, setBrowseResults] = useState<MusicVideoResult[]>([]);
  const [browseLoading, setBrowseLoading] = useState(true);
  const [resolving, setResolving] = useState<string | null>(null);

  const fetchBrowse = useCallback(async () => {
    setBrowseLoading(true);
    try {
      const res = await api.browseMusicVideos({ page: 1 });
      setBrowseResults(res.results);
    } catch {
      // ignore
    }
    setBrowseLoading(false);
  }, []);

  useEffect(() => {
    fetchBrowse();
  }, [fetchBrowse]);

  const handlePlay = async (item: MusicVideoResult) => {
    let magnet = item.magnet;
    if (!magnet) {
      setResolving(item.detail_url);
      try {
        const res = await api.resolveMagnet(item.detail_url, "music-videos");
        magnet = res.magnet;
      } catch {
        setResolving(null);
        return;
      }
      setResolving(null);
    }
    const tempId = `pending-${Date.now()}`;
    navigate(`/player/${tempId}`, {
      state: {
        magnet,
        meta: { title: item.title },
      },
    });
  };

  const handleInputChange = (value: string) => {
    setQuery(value);
    search(value);
  };

  const handleClear = () => {
    setQuery("");
    search("");
  };

  const displayResults = query ? results : browseResults;
  const displayLoading = query ? isLoading : browseLoading;

  return (
    <Flex direction="column" gap="4">
      <Text size="5" weight="bold">
        Music Videos
      </Text>

      <Flex gap="2" align="end">
        <Box flexGrow="1">
          <TextField.Root
            size="3"
            placeholder="Search music videos..."
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

      {resolving && (
        <Text size="2" color="gray">
          Resolving magnet link...
        </Text>
      )}

      {!query && (
        <Text size="3" weight="bold">
          Latest
        </Text>
      )}

      {displayLoading ? (
        <Flex direction="column" gap="2">
          {Array.from({ length: 8 }).map((_, i) => (
            <Card size="1" key={i}>
              <Flex gap="3">
                <Skeleton width="16px" height="16px" />
                <Flex direction="column" gap="1" style={{ flex: 1 }}>
                  <Skeleton height="14px" width="70%" />
                  <Skeleton height="12px" width="30%" />
                </Flex>
              </Flex>
            </Card>
          ))}
        </Flex>
      ) : displayResults.length === 0 ? (
        <Flex justify="center" py="6">
          <Text size="2" color="gray">
            No results found
          </Text>
        </Flex>
      ) : (
        <Flex direction="column" gap="2">
          {displayResults.map((item, i) => (
            <ResultCard key={`${item.title}-${i}`} item={item} onPlay={handlePlay} />
          ))}
        </Flex>
      )}
    </Flex>
  );
}
