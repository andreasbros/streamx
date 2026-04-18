import { useState } from "react";
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
} from "@radix-ui/themes";
import {
  MagnifyingGlassIcon,
  PlayIcon,
  Link2Icon,
} from "@radix-ui/react-icons";
import { useSearch } from "../hooks/useSearch";
import { isMagnetLink, detectQuality, formatBytes } from "../lib/utils";
import type { SearchResult } from "../api/types";

type SortKey = "seeds" | "size" | "year" | "rating";

const DEMO_STREAM_ID = "demo";

function sortResults(results: SearchResult[], key: SortKey): SearchResult[] {
  return [...results].sort((a, b) => {
    switch (key) {
      case "seeds":
        return b.seeds - a.seeds;
      case "size":
        return b.size_bytes - a.size_bytes;
      case "year":
        return (b.year ?? 0) - (a.year ?? 0);
      case "rating":
        return (b.rating ?? 0) - (a.rating ?? 0);
    }
  });
}

function ResultCard({
  result,
  onSelect,
}: {
  result: SearchResult;
  onSelect: (r: SearchResult) => void;
}) {
  const quality = result.quality ?? detectQuality(result.title);
  const [imgError, setImgError] = useState(false);

  return (
    <Card
      size="1"
      onClick={() => onSelect(result)}
      style={{ cursor: "pointer" }}
    >
      <Flex gap="3" align="start">
        {result.poster && !imgError ? (
          <img
            src={result.poster}
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

        <Flex direction="column" gap="1" style={{ minWidth: 0 }}>
          <Text
            size="2"
            weight="medium"
            style={{
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {result.title}
          </Text>

          <Flex gap="1" wrap="wrap">
            {quality && (
              <Badge size="1" variant="surface">
                {quality}
              </Badge>
            )}
            {result.year && (
              <Badge size="1" variant="soft" color="gray">
                {result.year}
              </Badge>
            )}
            {result.rating != null && result.rating > 0 && (
              <Badge size="1" variant="soft" color="amber">
                {result.rating.toFixed(1)}
              </Badge>
            )}
          </Flex>

          <Flex gap="2" align="center">
            <Text size="1" color="green">
              {result.seeds}
            </Text>
            <Text size="1" color="red">
              {result.leeches}
            </Text>
            <Text size="1" color="gray">
              {result.size || formatBytes(result.size_bytes)}
            </Text>
          </Flex>
        </Flex>
      </Flex>
    </Card>
  );
}

function SkeletonCard() {
  return (
    <Card size="1">
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

export function Search() {
  const navigate = useNavigate();
  const { results, isLoading, error, search } = useSearch();
  const [query, setQuery] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("seeds");
  const [starting] = useState(false);

  const handleInputChange = (value: string) => {
    setQuery(value);
    if (!isMagnetLink(value)) {
      search(value);
    }
  };

  const startAndNavigate = (magnetUri: string) => {
    const tempId = `pending-${Date.now()}`;
    navigate(`/player/${tempId}?magnet=${encodeURIComponent(magnetUri)}`);
  };

  const handleMagnetSubmit = () => {
    if (!isMagnetLink(query)) return;
    startAndNavigate(query.trim());
  };

  const handleResultSelect = (result: SearchResult) => {
    startAndNavigate(result.magnet);
  };

  const sorted = sortResults(results, sortKey);
  const isMagnet = isMagnetLink(query);

  return (
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
        <Flex justify="center" py="9">
          <Flex direction="column" align="center" gap="3">
            <Text size="5" color="gray">
              Search for something to watch
            </Text>
            <Button variant="soft" size="2" onClick={handleDemo}>
              <PlayIcon />
              Watch Demo
            </Button>
          </Flex>
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
            {results.length} result{results.length !== 1 ? "s" : ""}
          </Text>
          <Select.Root
            value={sortKey}
            onValueChange={(v) => setSortKey(v as SortKey)}
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
          {sorted.map((result, i) => (
            <ResultCard
              key={`${result.magnet}-${i}`}
              result={result}
              onSelect={handleResultSelect}
            />
          ))}
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
  );

  function handleDemo() {
    navigate(`/player/${DEMO_STREAM_ID}`);
  }
}
