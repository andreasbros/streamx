import { Flex, Text } from "@radix-ui/themes";
import { PlayIcon, PauseIcon, Cross2Icon } from "@radix-ui/react-icons";
import { useAudioPlayer } from "../hooks/useAudioPlayer";

function formatTime(seconds: number): string {
  if (!isFinite(seconds) || seconds < 0) return "0:00";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function AudioPlayerBar() {
  const { currentTrack, isPlaying, duration, currentTime, pause, resume, stop, seek } =
    useAudioPlayer();

  if (!currentTrack) return null;

  const progress = duration > 0 ? (currentTime / duration) * 100 : 0;

  return (
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
      <div
        style={{
          height: 2,
          background: "var(--gray-a4)",
          position: "relative",
        }}
      >
        <div
          style={{
            height: "100%",
            width: `${progress}%`,
            background: "var(--accent-9)",
            transition: "width 0.3s linear",
          }}
        />
        {/* Clickable seek area */}
        <div
          onClick={(e) => {
            const rect = e.currentTarget.getBoundingClientRect();
            const ratio = (e.clientX - rect.left) / rect.width;
            seek(ratio * duration);
          }}
          style={{
            position: "absolute",
            top: -8,
            bottom: -8,
            left: 0,
            right: 0,
            cursor: "pointer",
          }}
        />
      </div>

      <Flex align="center" gap="3" px="3" py="2">
        {/* Artwork placeholder */}
        <Flex
          align="center"
          justify="center"
          style={{
            width: 40,
            height: 40,
            borderRadius: 6,
            background: "var(--accent-a3)",
            flexShrink: 0,
          }}
        >
          {currentTrack.artworkUrl ? (
            <img
              src={currentTrack.artworkUrl}
              alt=""
              style={{ width: 40, height: 40, borderRadius: 6, objectFit: "cover" }}
            />
          ) : (
            <PlayIcon width={16} height={16} />
          )}
        </Flex>

        {/* Title */}
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
            {currentTrack.title}
          </Text>
          <Text size="1" color="gray">
            {formatTime(currentTime)} / {formatTime(duration)}
          </Text>
        </Flex>

        {/* Controls */}
        <Flex gap="2" align="center" style={{ flexShrink: 0 }}>
          <div
            onClick={isPlaying ? pause : resume}
            style={{ cursor: "pointer", padding: 4 }}
          >
            {isPlaying ? (
              <PauseIcon width={18} height={18} />
            ) : (
              <PlayIcon width={18} height={18} />
            )}
          </div>
          <div onClick={stop} style={{ cursor: "pointer", padding: 4 }}>
            <Cross2Icon width={14} height={14} />
          </div>
        </Flex>
      </Flex>
    </div>
  );
}
