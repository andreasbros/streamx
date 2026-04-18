import { useEffect, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import {
  Flex,
  Text,
  Card,
  Grid,
  Button,
  Box,
  Progress,
  Skeleton,
} from "@radix-ui/themes";
import {
  PlayIcon,
  TrashIcon,
  CounterClockwiseClockIcon,
} from "@radix-ui/react-icons";
import { api } from "../api/client";
import { formatDuration } from "../lib/utils";
import type { WatchHistoryItem } from "../api/types";

export function History() {
  const navigate = useNavigate();
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

  const handleDelete = async (id: string) => {
    try {
      await api.deleteHistoryItem(id);
      setItems((prev) => prev.filter((item) => item.id !== id));
    } catch (err) {
      console.error("Failed to delete history item:", err);
    }
  };

  const handleResume = (item: WatchHistoryItem) => {
    navigate(`/player/${item.id}`);
  };

  if (isLoading) {
    return (
      <Flex direction="column" gap="4">
        <Text size="5" weight="bold">
          Watch History
        </Text>
        <Grid columns={{ initial: "1", sm: "2", md: "3" }} gap="4">
          {Array.from({ length: 6 }).map((_, i) => (
            <Card key={i}>
              <Flex direction="column" gap="2">
                <Skeleton style={{ height: 120, borderRadius: "var(--radius-2)" }} />
                <Skeleton style={{ height: 16, width: "70%" }} />
                <Skeleton style={{ height: 14, width: "40%" }} />
              </Flex>
            </Card>
          ))}
        </Grid>
      </Flex>
    );
  }

  return (
    <Flex direction="column" gap="4">
      <Text size="5" weight="bold">
        Watch History
      </Text>

      {error && (
        <Text size="2" color="red">
          {error}
        </Text>
      )}

      {items.length === 0 ? (
        <Flex direction="column" align="center" gap="3" py="9">
          <CounterClockwiseClockIcon width={48} height={48} style={{ opacity: 0.3 }} />
          <Text size="4" color="gray">
            Nothing watched yet
          </Text>
          <Button variant="soft" onClick={() => navigate("/")}>
            Find something to watch
          </Button>
        </Flex>
      ) : (
        <Grid columns={{ initial: "1", sm: "2", md: "3" }} gap="4">
          {items.map((item) => {
            const progress =
              item.duration_seconds && item.watched_seconds
                ? (item.watched_seconds / item.duration_seconds) * 100
                : 0;

            return (
              <Card key={item.id}>
                <Flex direction="column" gap="2">
                  {item.poster_url ? (
                    <Box
                      style={{
                        height: 120,
                        borderRadius: "var(--radius-2)",
                        overflow: "hidden",
                        background: "var(--gray-a3)",
                      }}
                    >
                      <img
                        src={item.poster_url}
                        alt={item.title}
                        loading="lazy"
                        style={{
                          width: "100%",
                          height: "100%",
                          objectFit: "cover",
                        }}
                      />
                    </Box>
                  ) : (
                    <Flex
                      align="center"
                      justify="center"
                      style={{
                        height: 120,
                        borderRadius: "var(--radius-2)",
                        background: "var(--gray-a3)",
                      }}
                    >
                      <PlayIcon width={24} height={24} />
                    </Flex>
                  )}

                  <Text size="2" weight="medium" truncate>
                    {item.title}
                  </Text>

                  {item.duration_seconds != null && (
                    <Box>
                      <Progress value={progress} max={100} size="1" />
                      <Text size="1" color="gray">
                        {item.watched_seconds != null
                          ? formatDuration(item.watched_seconds)
                          : "0:00"}{" "}
                        / {formatDuration(item.duration_seconds)}
                      </Text>
                    </Box>
                  )}

                  <Text size="1" color="gray">
                    {new Date(item.watched_at).toLocaleDateString()}
                  </Text>

                  <Flex gap="2">
                    <Button
                      size="1"
                      variant="soft"
                      onClick={() => handleResume(item)}
                      style={{ flex: 1 }}
                    >
                      <PlayIcon />
                      {progress > 0 ? "Resume" : "Play"}
                    </Button>
                    <Button
                      size="1"
                      variant="ghost"
                      color="red"
                      onClick={() => handleDelete(item.id)}
                    >
                      <TrashIcon />
                    </Button>
                  </Flex>
                </Flex>
              </Card>
            );
          })}
        </Grid>
      )}
    </Flex>
  );
}
