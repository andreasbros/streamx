import { useEffect, useState, useCallback } from "react";
import { useParams, useNavigate, useSearchParams } from "react-router-dom";
import {
  Box,
  Flex,
  Text,
  Button,
  Card,
  Badge,
  Progress,
} from "@radix-ui/themes";
import { ArrowLeftIcon } from "@radix-ui/react-icons";
import { VideoPlayer } from "../components/VideoPlayer";
import { useStream } from "../hooks/useStream";
import { api } from "../api/client";
import { formatBytes, formatSpeed } from "../lib/utils";

const DEMO_HLS_URL =
  "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";

function StatusBadge({ status }: { status: string }) {
  const colorMap: Record<string, "green" | "blue" | "amber" | "orange" | "red"> = {
    ready: "green",
    complete: "green",
    transcoding: "blue",
    downloading: "amber",
    initializing: "amber",
    paused: "orange",
    error: "red",
  };
  return (
    <Badge size="2" color={colorMap[status] ?? "gray"}>
      {status}
    </Badge>
  );
}

export function Player() {
  const { id: routeId } = useParams<{ id: string }>();
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const [streamId, setStreamId] = useState<string | null>(
    routeId?.startsWith("pending-") ? null : (routeId ?? null)
  );
  const isDemo = routeId === "demo";
  const magnet = searchParams.get("magnet");

  useEffect(() => {
    if (!magnet || isDemo) return;

    let cancelled = false;
    const tryStart = async () => {
      for (let attempt = 0; attempt < 3; attempt++) {
        try {
          const res = await api.startStream({ magnet_uri: magnet });
          if (!cancelled && res.stream_id) {
            setStreamId(res.stream_id);
            navigate(`/player/${res.stream_id}`, { replace: true });
          }
          return;
        } catch (err) {
          console.error(`Start stream attempt ${attempt + 1} failed:`, err);
          if (attempt < 2) await new Promise((r) => setTimeout(r, 3000));
        }
      }
    };
    tryStart();
    return () => { cancelled = true; };
  }, [magnet, isDemo, navigate]);

  const { status, fileUrl, error } = useStream(isDemo ? null : streamId);

  const videoUrl = isDemo ? DEMO_HLS_URL : fileUrl;
  const videoReady = isDemo || fileUrl !== null;

  const handleTimeUpdate = useCallback(
    (time: number) => {
      if (!streamId || isDemo) return;
      if (Math.floor(time) % 10 === 0 && time > 0) {
        api.updateWatchPosition(streamId, time).catch(() => {});
      }
    },
    [streamId, isDemo]
  );

  return (
    <Flex direction="column" gap="4">
      <Flex align="center" gap="3">
        <Button variant="ghost" onClick={() => navigate(-1)}>
          <ArrowLeftIcon />
          Back
        </Button>
        {isDemo && (
          <Badge size="2" color="blue" variant="surface">
            Demo
          </Badge>
        )}
      </Flex>

      <Box style={{ aspectRatio: "16/9", background: "#000", borderRadius: 8, overflow: "hidden" }}>
        {videoReady && videoUrl ? (
          <VideoPlayer
            src={videoUrl}
            onTimeUpdate={handleTimeUpdate}
          />
        ) : (
          <Flex
            align="center"
            justify="center"
            style={{
              aspectRatio: "16/9",
              borderRadius: "var(--radius-3)",
              background: "var(--gray-a3)",
            }}
          >
            <Text size="3" color="gray">
              {error
                ? error
                : status
                  ? "Waiting for file..."
                  : "Connecting..."}
            </Text>
          </Flex>
        )}
      </Box>

      {!isDemo && status && (
        <Card>
          <Flex direction="column" gap="3">
            <Flex align="center" justify="between">
              <Text size="3" weight="medium">
                Stream Status
              </Text>
              <StatusBadge status={status.status} />
            </Flex>

            {status.status !== "ready" && status.status !== "complete" && (
              <Box>
                <Progress
                  value={status.progress ?? 0}
                  max={100}
                  size="2"
                />
                <Text size="1" color="gray" mt="1">
                  {(status.progress ?? 0).toFixed(1)}%
                </Text>
              </Box>
            )}

            <Flex gap="4">
              <Text size="2" color="gray">
                Peers: <Text weight="medium">{status.peers ?? 0}</Text>
              </Text>
              <Text size="2" color="gray">
                Speed: <Text weight="medium">{formatSpeed(status.speed ?? 0)}</Text>
              </Text>
              {status.file_size != null && status.file_size > 0 && (
                <Text size="2" color="gray">
                  Size:{" "}
                  <Text weight="medium">
                    {formatBytes(status.file_size)}
                  </Text>
                </Text>
              )}
              {status.files && status.files.length > 0 && !status.file_size && (
                <Text size="2" color="gray">
                  Size:{" "}
                  <Text weight="medium">
                    {formatBytes(
                      status.files.reduce((sum, f) => sum + f.size, 0)
                    )}
                  </Text>
                </Text>
              )}
            </Flex>
          </Flex>
        </Card>
      )}

      {error && !isDemo && (
        <Text size="2" color="red">
          {error}
        </Text>
      )}
    </Flex>
  );
}
