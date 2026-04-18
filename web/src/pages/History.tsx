import { useEffect, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import {
  Flex,
  Text,
  Card,
  Button,
  Box,
  Badge,
  Skeleton,
} from "@radix-ui/themes";
import {
  PlayIcon,
  TrashIcon,
  CounterClockwiseClockIcon,
} from "@radix-ui/react-icons";
import { api } from "../api/client";
import { useAuth } from "../hooks/useAuth";
import { formatBytes, formatRuntime } from "../lib/utils";
import type { WatchHistoryItem } from "../api/types";

export function History() {
  const navigate = useNavigate();
  const { user } = useAuth();
  const isAdmin = user?.is_admin ?? false;
  const [items, setItems] = useState<WatchHistoryItem[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchHistory = useCallback(async () => {
    try {
      const data = await api.watchHistory();
      setItems(data.items);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load history");
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchHistory();
  }, [fetchHistory]);

  const handleDelete = async (item: WatchHistoryItem, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      if (isAdmin && item.info_hash) {
        await api.deleteStream(item.info_hash);
      }
      await api.deleteHistoryItem(item.id);
      setItems((prev) => prev.filter((i) => i.id !== item.id));
    } catch (err) {
      console.error("Failed to delete:", err);
    }
  };

  const handleDeleteAll = async () => {
    try {
      for (const item of items) {
        if (isAdmin && item.info_hash) {
          await api.deleteStream(item.info_hash).catch(() => {});
        }
        await api.deleteHistoryItem(item.id).catch(() => {});
      }
      setItems([]);
    } catch (err) {
      console.error("Failed to delete all:", err);
    }
  };

  const handlePlay = (item: WatchHistoryItem) => {
    if (item.info_hash) {
      navigate(`/player/${item.info_hash}`);
    } else {
      const tempId = `pending-${Date.now()}`;
      navigate(`/player/${tempId}`, { state: { magnet: item.magnet_uri } });
    }
  };

  if (isLoading) {
    return (
      <Flex direction="column" gap="4">
        <Text size="5" weight="bold">Watch History</Text>
        <Flex direction="column" gap="2">
          {Array.from({ length: 4 }).map((_, i) => (
            <Card key={i} size="1">
              <Flex gap="3">
                <Skeleton width="48px" height="72px" style={{ borderRadius: 4 }} />
                <Flex direction="column" gap="2" flexGrow="1">
                  <Skeleton height="14px" width="60%" />
                  <Skeleton height="12px" width="40%" />
                </Flex>
              </Flex>
            </Card>
          ))}
        </Flex>
      </Flex>
    );
  }

  return (
    <Flex direction="column" gap="4">
      <Flex align="center" justify="between">
        <Text size="5" weight="bold">Watch History</Text>
        {items.length > 0 && (
          <Button variant="ghost" size="1" color="red" onClick={handleDeleteAll}>
            <TrashIcon width={12} height={12} />
            {isAdmin ? "Delete All" : "Clear All"}
          </Button>
        )}
      </Flex>

      {error && <Text size="2" color="red">{error}</Text>}

      {items.length === 0 ? (
        <Flex direction="column" align="center" gap="3" py="9">
          <CounterClockwiseClockIcon width={48} height={48} style={{ opacity: 0.3 }} />
          <Text size="4" color="gray">Nothing watched yet</Text>
          <Button variant="soft" onClick={() => navigate("/")}>
            Find something to watch
          </Button>
        </Flex>
      ) : (
        <Flex direction="column" gap="2">
          {items.map((item) => (
            <Card
              key={item.id}
              size="1"
              onClick={() => handlePlay(item)}
              style={{ cursor: "pointer" }}
            >
              <Flex gap="3" align="start">
                {item.poster_url ? (
                  <img
                    src={item.poster_url}
                    alt=""
                    loading="lazy"
                    width={48}
                    height={72}
                    style={{
                      borderRadius: 4,
                      objectFit: "cover",
                      flexShrink: 0,
                      background: "var(--gray-a3)",
                    }}
                    onError={(e) => { (e.target as HTMLImageElement).style.display = "none"; }}
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
                    <PlayIcon width={20} height={20} />
                  </Flex>
                )}

                <Flex direction="column" gap="1" style={{ minWidth: 0, flex: 1 }}>
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

                  <Flex gap="1" wrap="wrap">
                    <Badge size="1" variant="soft" color="gray">
                      {new Date(item.watched_at).toLocaleDateString()}
                    </Badge>
                    {item.year != null && (
                      <Badge size="1" variant="soft" color="gray">{item.year}</Badge>
                    )}
                    {item.rating != null && item.rating > 0 && (
                      <Badge size="1" variant="soft" color="amber">{item.rating.toFixed(1)}</Badge>
                    )}
                    {item.runtime != null && item.runtime > 0 && (
                      <Badge size="1" variant="soft" color="gray">{formatRuntime(item.runtime)}</Badge>
                    )}
                    {item.file_size != null && item.file_size > 0 && (
                      <Badge size="1" variant="soft" color="gray">{formatBytes(item.file_size)}</Badge>
                    )}
                  </Flex>

                  {item.genres && (
                    <Text size="1" color="gray" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      {item.genres.split(",").slice(0, 3).join(" / ")}
                    </Text>
                  )}
                </Flex>

                <Box style={{ flexShrink: 0 }}>
                  <Button
                    size="1"
                    variant="ghost"
                    color="red"
                    onClick={(e) => handleDelete(item, e)}
                  >
                    <TrashIcon />
                  </Button>
                </Box>
              </Flex>
            </Card>
          ))}
        </Flex>
      )}
    </Flex>
  );
}
