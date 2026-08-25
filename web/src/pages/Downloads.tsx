import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Badge,
  Button,
  Card,
  Flex,
  IconButton,
  Text,
} from "@radix-ui/themes";
import {
  Cross2Icon,
  DownloadIcon,
  PlayIcon,
  TrashIcon,
} from "@radix-ui/react-icons";
import { api } from "../api/client";
import { useAuth } from "../hooks/useAuth";
import { formatBytes, formatSpeed } from "../lib/utils";
import type { DownloadItem } from "../api/types";

const POLL_MS = 2500;

function statusColor(status: string): "green" | "blue" | "gray" | "red" | "orange" {
  switch (status) {
    case "complete":
      return "green";
    case "downloading":
    case "initializing":
      return "blue";
    case "paused":
      return "gray";
    case "error":
      return "red";
    default:
      return "orange";
  }
}

function ProgressBar({ value }: { value: number }) {
  return (
    <div
      style={{
        width: "100%",
        height: 6,
        borderRadius: 3,
        background: "var(--gray-a4)",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          width: `${Math.min(100, Math.max(0, value)).toFixed(1)}%`,
          height: "100%",
          borderRadius: 3,
          background: value >= 100 ? "var(--green-9)" : "var(--accent-9)",
          transition: "width 0.4s ease",
        }}
      />
    </div>
  );
}

export function Downloads() {
  const navigate = useNavigate();
  const { user } = useAuth();
  const isAdmin = user?.is_admin === true;
  const [items, setItems] = useState<DownloadItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const refresh = useCallback(async () => {
    try {
      const res = await api.listDownloads();
      setItems(res.downloads);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load downloads");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    timerRef.current = setInterval(refresh, POLL_MS);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [refresh]);

  const withBusy = async (id: string, fn: () => Promise<void>) => {
    setBusy(id);
    try {
      await fn();
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Action failed");
    } finally {
      setBusy(null);
    }
  };

  const handleResume = (dl: DownloadItem) =>
    withBusy(dl.info_hash, () => api.pinDownload(dl.info_hash));

  const handleCancel = (dl: DownloadItem) =>
    withBusy(dl.info_hash, () => api.unpinDownload(dl.info_hash));

  const handleDelete = (dl: DownloadItem) => {
    if (!window.confirm(`Delete "${dl.title || dl.file_name}" and all its files?`)) {
      return;
    }
    return withBusy(dl.info_hash, () => api.deleteStream(dl.info_hash));
  };

  // Open the player page directly, exactly like the History page does.
  // Playback works in any download state (streams while downloading).
  const handleOpen = (dl: DownloadItem) => {
    navigate(`/player/${dl.info_hash}`);
  };

  return (
    <Flex direction="column" gap="4">
      <Flex align="center" gap="2">
        <DownloadIcon width={20} height={20} />
        <Text size="5" weight="bold">
          Downloads
        </Text>
      </Flex>

      {error && (
        <Text size="2" color="red">
          {error}
        </Text>
      )}

      {loading && items.length === 0 && (
        <Text size="2" color="gray">
          Loading downloads…
        </Text>
      )}

      {!loading && items.length === 0 && !error && (
        <Flex justify="center" py="6">
          <Text size="2" color="gray">
            No downloads yet. Use the download button on a movie to queue one.
          </Text>
        </Flex>
      )}

      <Flex direction="column" gap="2">
        {items.map((dl) => {
          const active = dl.status === "downloading" || dl.status === "initializing";
          const complete = dl.status === "complete";
          return (
            <Card
              key={dl.info_hash}
              size="2"
              style={{ cursor: "pointer" }}
              onClick={() => handleOpen(dl)}
            >
              <Flex direction="column" gap="2">
                <Flex align="center" gap="2" wrap="wrap">
                  <Text
                    size="2"
                    weight="medium"
                    style={{
                      flex: 1,
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {dl.title || dl.file_name || dl.info_hash}
                  </Text>
                  <Badge size="1" variant="soft" color={statusColor(dl.status)}>
                    {dl.status}
                  </Badge>
                  {dl.pinned && !complete && (
                    <Badge size="1" variant="soft" color="blue">
                      background
                    </Badge>
                  )}
                </Flex>

                <ProgressBar value={dl.progress} />

                <Flex align="center" gap="3" wrap="wrap">
                  <Text size="1" color="gray">
                    {dl.progress.toFixed(1)}%
                  </Text>
                  {dl.file_size > 0 && (
                    <Text size="1" color="gray">
                      {formatBytes(dl.file_size)}
                    </Text>
                  )}
                  {active && (
                    <>
                      <Text size="1" color="gray">
                        {dl.peers} peers
                      </Text>
                      <Text size="1" color="gray">
                        {formatSpeed(dl.speed)}
                      </Text>
                    </>
                  )}
                  <Flex gap="2" ml="auto" align="center">
                    <Button
                      size="1"
                      variant="soft"
                      color="green"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleOpen(dl);
                      }}
                    >
                      <PlayIcon width={12} height={12} />
                      Watch
                    </Button>
                    {!complete && dl.pinned && (
                      <Button
                        size="1"
                        variant="soft"
                        color="orange"
                        disabled={busy === dl.info_hash}
                        onClick={(e) => {
                          e.stopPropagation();
                          handleCancel(dl);
                        }}
                      >
                        <Cross2Icon width={12} height={12} />
                        Cancel Download
                      </Button>
                    )}
                    {!complete && !dl.pinned && (
                      <Button
                        size="1"
                        variant="soft"
                        disabled={busy === dl.info_hash}
                        onClick={(e) => {
                          e.stopPropagation();
                          handleResume(dl);
                        }}
                      >
                        <DownloadIcon width={12} height={12} />
                        Download
                      </Button>
                    )}
                    {isAdmin && (
                      <IconButton
                        size="1"
                        variant="soft"
                        color="red"
                        disabled={busy === dl.info_hash}
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDelete(dl);
                        }}
                        aria-label="Delete download"
                      >
                        <TrashIcon width={14} height={14} />
                      </IconButton>
                    )}
                  </Flex>
                </Flex>
              </Flex>
            </Card>
          );
        })}
      </Flex>
    </Flex>
  );
}
