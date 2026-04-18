import { useEffect, useState } from "react";
import { useParams, useNavigate, useSearchParams } from "react-router-dom";
import { Flex, Text } from "@radix-ui/themes";
import { PauseIcon } from "@radix-ui/react-icons";
import { useAudioPlayer } from "../hooks/useAudioPlayer";
import { api } from "../api/client";
import type { TorrentFileInfo } from "../api/types";
import { DEFAULT_VIDEO_POSTER_URL, LOGO_URL } from "../assets";

function formatTime(seconds: number): string {
  if (!isFinite(seconds) || seconds < 0) return "0:00";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function trackTitleFromPath(path: string): string {
  const name = path.split("/").pop() ?? path;
  return name.replace(/\.[^.]+$/, "").replace(/^\d+[\s._-]+/, "");
}

export function MusicPlayer() {
  const { streamId, fileIndex: fileIndexStr } = useParams<{ streamId: string; fileIndex: string }>();
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const audioPlayer = useAudioPlayer();
  const isGuest = searchParams.has("guest");
  const [error, setError] = useState<string | null>(null);
  const [imgSrc, setImgSrc] = useState(DEFAULT_VIDEO_POSTER_URL);

  const fileIndex = parseInt(fileIndexStr ?? "0", 10);

  // Set artwork URL eagerly
  useEffect(() => {
    if (streamId) {
      setImgSrc(api.getArtworkUrl(streamId, fileIndex));
    }
  }, [streamId, fileIndex]);

  useEffect(() => {
    if (!streamId) return;

    let cancelled = false;

    async function loadAndPlay() {
      try {
        let files: TorrentFileInfo[] = [];
        for (let attempt = 0; attempt < 15; attempt++) {
          const res = await api.getStreamFiles(streamId!) as { files: TorrentFileInfo[]; status?: string };
          if (res.status === "error") {
            if (!cancelled) setError("Stream not available");
            return;
          }
          files = res.files.filter((f) => f.is_audio);
          if (files.length > 0) break;
          await new Promise((r) => setTimeout(r, 1000));
        }

        if (cancelled) return;

        if (files.length === 0) {
          setError("No audio files found");
          return;
        }

        const tracks = files.map((f) => {
          const ext = f.path.split(".").pop()?.toUpperCase() ?? "";
          return {
            title: trackTitleFromPath(f.path),
            streamId: streamId!,
            fileIndex: f.index,
            artworkUrl: api.getArtworkUrl(streamId!, f.index),
            format: ext,
            fileSize: f.size,
          };
        });

        const startIdx = tracks.findIndex((t) => t.fileIndex === fileIndex);
        audioPlayer.playQueue(tracks, startIdx >= 0 ? startIdx : 0);

        if (!isGuest) {
          navigate("/music", { replace: true });
        }
      } catch {
        if (!cancelled) {
          setError("Failed to load track");
        }
      }
    }

    loadAndPlay();
    return () => { cancelled = true; };
  }, [streamId, fileIndex]); // eslint-disable-line react-hooks/exhaustive-deps

  const track = audioPlayer.currentTrack;

  if (error) {
    return (
      <Flex direction="column" align="center" justify="center" gap="3" py="9">
        <Text size="3" color="red">{error}</Text>
      </Flex>
    );
  }

  return (
    <Flex direction="column" align="center" justify="center" gap="4" py="6">
      <div
        onClick={() => {
          if (audioPlayer.isPlaying) audioPlayer.pause();
          else audioPlayer.resume();
        }}
        style={{
          width: 240,
          height: 240,
          borderRadius: 12,
          overflow: "hidden",
          boxShadow: "0 8px 32px rgba(0,0,0,0.4)",
          position: "relative",
          cursor: "pointer",
        }}
      >
        <img
          src={track?.artworkUrl ?? imgSrc}
          alt=""
          onError={() => setImgSrc(DEFAULT_VIDEO_POSTER_URL)}
          style={{
            width: "100%",
            height: "100%",
            objectFit: "cover",
            animation: "albumBreath 16s ease-in-out 0s infinite alternate",
          }}
        />
        <div style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: audioPlayer.isPlaying ? "rgba(0,0,0,0.15)" : "rgba(0,0,0,0.3)",
        }}>
          {audioPlayer.isPlaying ? (
            <PauseIcon width={48} height={48} style={{ color: "white", opacity: 0.7 }} />
          ) : (
            <img src={LOGO_URL} alt="Play" width={160} height={160} style={{ opacity: 0.9 }} />
          )}
        </div>
      </div>
      <Flex direction="column" align="center" gap="1">
        <Text size="4" weight="bold" align="center">
          {track?.title ?? "Loading..."}
        </Text>
        {track?.album && (
          <Text size="2" color="gray">{track.album}</Text>
        )}
        {audioPlayer.duration > 0 && (
          <Text size="1" color="gray">
            {formatTime(audioPlayer.currentTime)} / {formatTime(audioPlayer.duration)}
          </Text>
        )}
      </Flex>
      {!track && (
        <Text size="1" color="gray">Connecting to stream...</Text>
      )}
    </Flex>
  );
}
