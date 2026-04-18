import { useState, useEffect } from "react";
import { Flex, Text } from "@radix-ui/themes";
import {
  PlayIcon,
  PauseIcon,
  Cross2Icon,
  TrackPreviousIcon,
  TrackNextIcon,
  ChevronUpIcon,
} from "@radix-ui/react-icons";
import { useLocation } from "react-router-dom";
import { useAudioPlayer } from "../hooks/useAudioPlayer";
import { useAuth } from "../hooks/useAuth";
import { ExpandedPlayer } from "./ExpandedPlayer";
import { DEFAULT_VIDEO_POSTER_URL } from "../assets";

function formatTime(seconds: number): string {
  if (!isFinite(seconds) || seconds < 0) return "0:00";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function AudioPlayerBar() {
  const {
    currentTrack, isPlaying, duration, currentTime,
    queue, queueIndex,
    pause, resume, stop, seek, next, previous,
  } = useAudioPlayer();

  const [expanded, setExpanded] = useState(false);
  const { isGuest } = useAuth();
  const location = useLocation();
  const hideClose = isGuest && location.pathname.startsWith("/music/play/");
  const [imgSrc, setImgSrc] = useState(DEFAULT_VIDEO_POSTER_URL);

  useEffect(() => {
    setImgSrc(currentTrack?.artworkUrl ?? DEFAULT_VIDEO_POSTER_URL);
  }, [currentTrack?.artworkUrl, currentTrack?.title]);

  if (!currentTrack) return null;

  const progress = duration > 0 ? (currentTime / duration) * 100 : 0;
  const hasNext = queueIndex >= 0 && queueIndex < queue.length - 1;
  const hasPrev = queueIndex > 0 || currentTime > 3;

  return (
    <>
      <div
        style={{
          position: "fixed",
          bottom: 0,
          left: 0,
          right: 0,
          zIndex: 150,
          background: "var(--color-panel-solid)",
          borderTop: "1px solid var(--gray-a5)",
        }}
      >
        {/* Thin progress line */}
        <div style={{ height: 2, background: "var(--gray-a4)", position: "relative" }}>
          <div
            style={{
              height: "100%",
              width: `${progress}%`,
              background: "var(--accent-9)",
              transition: "width 0.3s linear",
            }}
          />
          <div
            onClick={(e) => {
              e.stopPropagation();
              const rect = e.currentTarget.getBoundingClientRect();
              const ratio = (e.clientX - rect.left) / rect.width;
              seek(ratio * duration);
            }}
            style={{ position: "absolute", top: -8, bottom: -8, left: 0, right: 0, cursor: "pointer" }}
          />
        </div>

        <Flex align="center" gap="3" px="3" py="2">
          {/* Clickable area: expand + artwork + title -> expand */}
          <Flex
            align="center"
            gap="2"
            onClick={() => setExpanded(true)}
            style={{ flex: 1, minWidth: 0, cursor: "pointer" }}
          >
            <ChevronUpIcon width={14} height={14} style={{ flexShrink: 0, opacity: 0.5 }} />
            <img
              src={imgSrc}
              alt=""
              onError={() => setImgSrc(DEFAULT_VIDEO_POSTER_URL)}
              style={{
                width: 40,
                height: 40,
                borderRadius: 6,
                objectFit: "cover",
                flexShrink: 0,
                background: "var(--gray-a3)",
              }}
            />
            <Flex direction="column" gap="0" style={{ minWidth: 0 }}>
              <Text
                size="2"
                weight="medium"
                style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
              >
                {currentTrack.title}
              </Text>
              <Flex gap="2" align="center">
                {currentTrack.artist && (
                  <Text size="1" color="gray" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {currentTrack.artist}
                  </Text>
                )}
                <Text size="1" color="gray" style={{ flexShrink: 0 }}>
                  {formatTime(currentTime)} / {formatTime(duration)}
                </Text>
              </Flex>
            </Flex>
          </Flex>

          {/* Controls */}
          <Flex gap="1" align="center" style={{ flexShrink: 0 }}>
            <div
              onClick={previous}
              style={{ cursor: hasPrev ? "pointer" : "default", padding: 4, opacity: hasPrev ? 1 : 0.3 }}
            >
              <TrackPreviousIcon width={16} height={16} />
            </div>
            <div onClick={isPlaying ? pause : resume} style={{ cursor: "pointer", padding: 4 }}>
              {isPlaying ? <PauseIcon width={20} height={20} /> : <PlayIcon width={20} height={20} />}
            </div>
            <div
              onClick={next}
              style={{ cursor: hasNext ? "pointer" : "default", padding: 4, opacity: hasNext ? 1 : 0.3 }}
            >
              <TrackNextIcon width={16} height={16} />
            </div>
            {!hideClose && (
              <div onClick={stop} style={{ cursor: "pointer", padding: 4 }}>
                <Cross2Icon width={14} height={14} />
              </div>
            )}
          </Flex>
        </Flex>
      </div>

      <ExpandedPlayer open={expanded} onClose={() => setExpanded(false)} />
    </>
  );
}
