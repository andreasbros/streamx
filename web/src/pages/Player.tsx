import { useEffect, useState, useCallback, useRef } from "react";
import { useParams, useNavigate, useLocation } from "react-router-dom";
import {
  Box,
  Flex,
  Text,
  Button,
  Card,
  Badge,
  Select,
} from "@radix-ui/themes";
import { ArrowLeftIcon, Cross2Icon, DownloadIcon, DrawingPinIcon, TrashIcon, ChevronDownIcon, ChevronUpIcon, Share1Icon, CheckIcon } from "@radix-ui/react-icons";
import { VideoPlayer } from "../components/VideoPlayer";
import type { VideoPlayerHandle } from "../components/VideoPlayer";
import { useStream } from "../hooks/useStream";
import { api } from "../api/client";
import { formatBytes, formatSpeed, formatRuntime } from "../lib/utils";
import { debugLog } from "../lib/debug-log";
import { useAuth } from "../hooks/useAuth";
import { useMediaSession } from "../hooks/useMediaSession";
import { useServerSettings } from "../hooks/useServerSettings";
import { NotWebBadge } from "../components/NotWebBadge";
import { useFavourites } from "../hooks/useFavourites";
import { TrailerModal } from "../components/TrailerModal";
import { LOGO_URL, PAGE_BG_URL, DEFAULT_VIDEO_POSTER_URL } from "../assets";

const DEMO_HLS_URL = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";

function ms(meta: Record<string, unknown> | null, key: string): string | null {
  const v = meta?.[key];
  return typeof v === "string" && v ? v : null;
}
function mn(meta: Record<string, unknown> | null, key: string): number | null {
  const v = meta?.[key];
  return typeof v === "number" ? v : null;
}
const BUFFER_TARGET = 5;

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

// --- Cinematic background ---
function CinematicBg({ poster }: { poster: string | null }) {
  return (
    <div style={{ position: "fixed", inset: 0, zIndex: -1, overflow: "hidden" }}>
      {poster ? (
        <img
          src={poster}
          alt=""
          style={{
            position: "absolute",
            inset: "-20%",
            width: "140%",
            height: "140%",
            objectFit: "cover",
            filter: "blur(40px) brightness(0.15) saturate(1.4)",
          }}
        />
      ) : (
        <img
          src={PAGE_BG_URL}
          alt=""
          style={{
            position: "absolute",
            inset: "-20%",
            width: "140%",
            height: "140%",
            objectFit: "cover",
            filter: "blur(40px) brightness(0.15) saturate(1.4)",
          }}
        />
      )}
      <div style={{ position: "absolute", inset: 0, background: "rgba(0,0,0,0.4)" }} />
    </div>
  );
}

// --- Video box overlay (poster + spinner or play button) ---
function VideoOverlay({
  poster,
  videoReady,
  error,
  onPlay,
  trailerCode,
  trailerSearch,
  onTrailer,
}: {
  poster: string | null;
  videoReady: boolean;
  error: string | null;
  onPlay: () => void;
  trailerCode?: string | null;
  trailerSearch?: string | null;
  onTrailer?: () => void;
}) {
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 2,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 12,
        cursor: videoReady && !error ? "pointer" : "default",
      }}
      onClick={() => {
        if (videoReady && !error) onPlay();
      }}
    >
      <div style={{ position: "absolute", inset: 0, background: "#000", overflow: "hidden" }}>
        <img
          src={poster || DEFAULT_VIDEO_POSTER_URL}
          alt=""
          style={{
            position: "absolute",
            left: 0,
            width: "100%",
            height: "150%",
            objectFit: "cover",
            objectPosition: "center center",
            top: "-25%",
            willChange: "transform",
            transform: "translateY(30px) scale(1.15)",
            animation: "posterPanDown 16s ease-in-out 1s infinite alternate both",
          }}
        />
      </div>
      <div style={{ position: "absolute", inset: 0, background: "rgba(0,0,0,0.35)" }} />

      {/* Content */}
      {error ? (
        <Text size="3" color="red" style={{ position: "relative" }}>{error}</Text>
      ) : videoReady ? (
        <img
          src={LOGO_URL}
          alt="Play"
          style={{
            position: "relative",
            width: 160,
            height: 160,
            opacity: 0.85,
            filter: "drop-shadow(0 0 20px rgba(255,255,255,0.3))",
            transition: "opacity 0.15s, transform 0.15s",
            cursor: "pointer",
          }}
          onMouseEnter={(e) => { e.currentTarget.style.opacity = "1"; e.currentTarget.style.transform = "scale(1.1)"; }}
          onMouseLeave={(e) => { e.currentTarget.style.opacity = "0.85"; e.currentTarget.style.transform = "scale(1)"; }}
        />
      ) : null}

      {/* Watch Trailer - bottom right, only when play button is shown */}
      {!error && (trailerCode || trailerSearch) && onTrailer && (
        <div
          onClick={(e) => { e.stopPropagation(); onTrailer(); }}
          style={{
            position: "absolute",
            bottom: 11,
            right: 19,
            display: "flex",
            alignItems: "center",
            gap: 8,
            cursor: "pointer",
            opacity: 0.85,
            transition: "opacity 0.15s",
            zIndex: 3,
          }}
          onMouseEnter={(e) => { e.currentTarget.style.opacity = "1"; }}
          onMouseLeave={(e) => { e.currentTarget.style.opacity = "0.85"; }}
        >
          <svg className="trailer-icon" viewBox="0 0 24 24" fill={trailerCode ? "#dc2626" : "#888"} xmlns="http://www.w3.org/2000/svg" style={{ width: 28, height: 28 }}>
            <path d="M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.546 12 3.546 12 3.546s-7.505 0-9.377.504A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.504 9.376.504 9.376.504s7.505 0 9.377-.504a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z"/>
          </svg>
          <span className="trailer-text" style={{ color: "white", fontSize: "var(--font-size-5)", fontWeight: 700 }}>Watch Trailer</span>
        </div>
      )}

      {!videoReady && !error && (
        <div
          style={{
            position: "relative",
            width: 43,
            height: 43,
            border: "3.5px solid rgba(255,255,255,0.3)",
            borderTopColor: "rgba(255,255,255,0.8)",
            borderRadius: "50%",
            animation: "spin 0.8s linear infinite",
          }}
        />
      )}
    </div>
  );
}

// --- Stream bar: speed before play, buffer health during play ---
function StreamBar({
  bufferedSeconds,
  playing,
  fileReady,
  speed,
  progress,
}: {
  bufferedSeconds: number;
  playing: boolean;
  fileReady: boolean;
  speed: number;
  progress: number;
}) {
  // During playback: show buffer health (seconds buffered ahead, target 5s)
  if (playing || bufferedSeconds > 0) {
    const pct = Math.min(bufferedSeconds / BUFFER_TARGET * 100, 100);
    const full = bufferedSeconds >= BUFFER_TARGET;
    const color = full ? "var(--green-9)" : bufferedSeconds > 2 ? "var(--amber-9)" : "var(--red-9)";
    const textColor = full ? "green" as const : bufferedSeconds > 2 ? "amber" as const : "red" as const;
    return (
      <Flex align="center" gap="2">
        <Text size="1" color="gray" style={{ width: 55, flexShrink: 0 }}>Buffer</Text>
        <Box flexGrow="1">
          <div style={{ height: 6, borderRadius: 3, overflow: "hidden", background: "var(--gray-a3)" }}>
            <div style={{ height: "100%", width: `${pct}%`, background: color, borderRadius: 3, transition: "width 0.3s ease, background 0.5s ease" }} />
          </div>
        </Box>
        <Text size="1" color={textColor} style={{ width: 48, textAlign: "right", flexShrink: 0 }}>
          {full ? `${Math.floor(bufferedSeconds)}s` : `${Math.floor(pct)}%`}
        </Text>
      </Flex>
    );
  }

  // File ready but not playing: show ready or buffering state
  if (fileReady && progress > 0) {
    // Show "Ready" only once enough data has been downloaded to likely start playback
    const enoughData = speed === 0 || progress >= 2;
    return (
      <Flex align="center" gap="2">
        <Text size="1" color="gray" style={{ width: 55, flexShrink: 0 }}>Stream</Text>
        <Box flexGrow="1">
          <div style={{ height: 6, borderRadius: 3, overflow: "hidden", background: "var(--gray-a3)" }}>
            <div style={{
              height: "100%",
              width: enoughData ? "100%" : `${Math.min(progress * 50, 100)}%`,
              background: enoughData ? "var(--green-9)" : "var(--amber-9)",
              borderRadius: 3,
              transition: "width 0.5s ease",
            }} />
          </div>
        </Box>
        <Text size="1" color={enoughData ? "green" : "amber"} style={{ width: 48, textAlign: "right", flexShrink: 0 }}>
          {enoughData ? "Ready" : `${progress.toFixed(1)}%`}
        </Text>
      </Flex>
    );
  }

  // Not ready: shimmer with 0s
  return (
    <Flex align="center" gap="2">
      <Text size="1" color="gray" style={{ width: 55, flexShrink: 0 }}>Stream</Text>
      <Box flexGrow="1">
        <div style={{ height: 6, borderRadius: 3, overflow: "hidden", background: "var(--gray-a3)", position: "relative" }}>
          <div style={{ position: "absolute", inset: 0, borderRadius: 3, background: "linear-gradient(90deg, transparent, var(--gray-a6), transparent)", backgroundSize: "200% 100%", animation: "shimmer-reverse 1.5s ease-in-out infinite" }} />
        </div>
      </Box>
      <Text size="1" color="gray" style={{ width: 48, textAlign: "right", flexShrink: 0 }}>0s</Text>
    </Flex>
  );
}

// --- Total download progress bar ---
function TotalBar({ progress }: { progress: number }) {
  const pct = progress >= 100 ? 100 : progress;
  return (
    <Flex align="center" gap="2">
      <Text size="1" color="gray" style={{ width: 55, flexShrink: 0 }}>Total</Text>
      <Box flexGrow="1">
        <div style={{ height: 6, borderRadius: 3, overflow: "hidden", background: "var(--gray-a3)" }}>
          <div style={{ height: "100%", width: `${pct}%`, background: pct >= 100 ? "var(--green-9)" : "var(--gray-a8)", borderRadius: 3, transition: "width 0.5s ease, background 0.5s ease" }} />
        </div>
      </Box>
      <Text size="1" color={pct >= 100 ? "green" : "gray"} style={{ width: 48, textAlign: "right", flexShrink: 0 }}>
        {pct >= 100 ? "100%" : `${pct.toFixed(1)}%`}
      </Text>
    </Flex>
  );
}

// =============================================================
// Main Player component
// =============================================================
function cleanPath(raw: string): string {
  let s = raw.replace(/^https?:\/\/[^/]+/, "");
  try {
    const u = new URL(raw, window.location.origin);
    u.searchParams.delete("token");
    s = u.pathname + (u.search ? u.search : "");
  } catch { /* use raw */ }
  return s;
}

function UrlActions({ path, downloadName }: { path: string; downloadName?: string }) {
  const full = path.startsWith("http") ? path : `${window.location.origin}${path}`;
  const btnStyle: React.CSSProperties = {
    color: "var(--gray-9)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    width: 24,
    height: 24,
    borderRadius: 4,
    background: "var(--gray-a3)",
  };
  return (
    <Flex gap="2" style={{ flexShrink: 0 }}>
      <a href={full} target="_blank" rel="noopener" title="Open in browser" style={btnStyle}>
        <svg width="14" height="14" viewBox="0 0 15 15" fill="currentColor"><path d="M3 2a1 1 0 00-1 1v9a1 1 0 001 1h9a1 1 0 001-1V8.5a.5.5 0 00-1 0V12H3V3h3.5a.5.5 0 000-1H3zm6.5 0a.5.5 0 000 1H11.3L7.15 7.15a.5.5 0 00.7.7L12 3.71V5.5a.5.5 0 001 0v-3a.5.5 0 00-.5-.5h-3z" /></svg>
      </a>
      <a href={`vlc://${full}`} title="Open in VLC" style={{...btnStyle, color: undefined}}>
        <svg width="14" height="14" viewBox="0 0 48 48">
          <path d="M24 4l-5.5 18h11L24 4z" fill="#FF8800" />
          <path d="M17 26l-3 8h20l-3-8H17z" fill="#FF6600" />
          <path d="M11 37c0 2.2 1.8 4 4 4h18c2.2 0 4-1.8 4-4H11z" fill="#EE5500" />
        </svg>
      </a>
      {downloadName && (
        <a href={full} download={downloadName} title={`Download as ${downloadName}`} style={{...btnStyle, color: "var(--blue-9)"}}>
          <svg width="14" height="14" viewBox="0 0 15 15" fill="currentColor"><path d="M7.5 1a.5.5 0 01.5.5v7.8l2.15-2.15a.5.5 0 01.7.7l-3 3a.5.5 0 01-.7 0l-3-3a.5.5 0 01.7-.7L7 9.3V1.5a.5.5 0 01.5-.5zM2 12.5a.5.5 0 01.5-.5h10a.5.5 0 010 1h-10a.5.5 0 01-.5-.5z" /></svg>
        </a>
      )}
    </Flex>
  );
}

function ShareButton({ streamId }: { streamId: string }) {
  const [copied, setCopied] = useState(false);
  const [loading, setLoading] = useState(false);

  const copyToClipboard = (text: string) => {
    // Clipboard API may fail outside user gesture trust window
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).catch(() => {
        fallbackCopy(text);
      });
    } else {
      fallbackCopy(text);
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 3000);
  };

  const fallbackCopy = (text: string) => {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.left = "-9999px";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  };

  const handleShare = async () => {
    setLoading(true);
    try {
      const result = await api.createShareLink(streamId);
      const fullUrl = `${window.location.origin}${result.url}`;

      // Try native share (mobile)
      if (navigator.share) {
        try {
          await navigator.share({ title: "StreamX", url: fullUrl });
          setCopied(true);
          setTimeout(() => setCopied(false), 3000);
          return;
        } catch {
          // User cancelled or not supported - fall through to copy
        }
      }

      copyToClipboard(fullUrl);
    } catch {
      copyToClipboard(window.location.href);
    } finally {
      setLoading(false);
    }
  };

  return (
    <Button variant="soft" size="1" onClick={handleShare} disabled={loading} style={{ fontSize: "var(--font-size-2)", fontWeight: 700, height: "auto", padding: "7px 12px" }}>
      {copied ? <><CheckIcon width={14} height={14} /> Copied</> : <><Share1Icon width={14} height={14} /> Share</>}
    </Button>
  );
}

function addToken(path: string): string {
  const token = localStorage.getItem("streamx_token");
  if (!token) return path;
  const sep = path.includes("?") ? "&" : "?";
  return `${path}${sep}token=${encodeURIComponent(token)}`;
}

function StreamUrls({ currentSrc, streamId, title }: { currentSrc: string; videoUrl: string | null; streamId: string | null; title: string }) {
  const [expanded, setExpanded] = useState(false);
  const [segmentHistory, setSegmentHistory] = useState<string[]>([]);

  useEffect(() => {
    if (!currentSrc) return;
    const parts = currentSrc.split(" | ");
    const segment = parts[1] ? cleanPath(parts[1]) : null;
    if (segment) {
      setSegmentHistory((prev) => {
        if (prev[0] === segment) return prev;
        return [segment, ...prev.filter((h) => h !== segment)].slice(0, 5);
      });
    }
  }, [currentSrc]);

  const activePlaylisPath = currentSrc ? cleanPath(currentSrc.split(" | ")[0] || "") : null;
  const masterPath = streamId ? `/api/stream/${streamId}/playlist.m3u8` : null;
  const filePath = streamId ? `/api/stream/${streamId}/file` : null;
  const vlcToken = localStorage.getItem("streamx_token") || "";
  const vlcPath = streamId && vlcToken ? `/api/stream/${streamId}/vlc/${vlcToken}` : null;
  const safeTitle = (title || streamId || "stream").replace(/[^a-zA-Z0-9._-]/g, "_");

  return (
    <Card>
      <Flex direction="column" gap="1">
        <Flex
          align="center"
          justify="between"
          onClick={() => setExpanded((v) => !v)}
          style={{ cursor: "pointer" }}
        >
          <Text size="2" weight="medium">Stream URLs</Text>
          {expanded ? <ChevronUpIcon width={14} height={14} /> : <ChevronDownIcon width={14} height={14} />}
        </Flex>
        {expanded && (
          <Flex direction="column" gap="1" style={{ fontFamily: "monospace", fontSize: 10, lineHeight: 1.5 }}>
            {filePath && (
              <Flex align="center" gap="2">
                <Badge size="1" variant="soft" color="gray" style={{ fontSize: 9, flexShrink: 0 }}>file</Badge>
                <Text size="1" color="gray" style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{filePath}</Text>
                <UrlActions path={addToken(filePath)} downloadName={`${safeTitle}.mkv`} />
              </Flex>
            )}
            {vlcPath && (
              <Flex align="center" gap="2">
                <Badge size="1" variant="soft" color="blue" style={{ fontSize: 9, flexShrink: 0 }}>VLC</Badge>
                <Text size="1" color="gray" style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{vlcPath}</Text>
                <UrlActions path={vlcPath} downloadName={`${safeTitle}.mkv`} />
              </Flex>
            )}
            {masterPath && (
              <Flex align="center" gap="2">
                <Badge size="1" variant="soft" color="gray" style={{ fontSize: 9, flexShrink: 0 }}>playlist</Badge>
                <Text size="1" color="gray" style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{masterPath}</Text>
                <UrlActions path={addToken(masterPath)} downloadName={`${safeTitle}_playlist.m3u8`} />
              </Flex>
            )}
            {activePlaylisPath && activePlaylisPath !== masterPath && (
              <Flex align="center" gap="2">
                <Badge size="1" variant="soft" color="gray" style={{ fontSize: 9, flexShrink: 0 }}>active</Badge>
                <Text size="1" color="gray" style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{activePlaylisPath}</Text>
                <UrlActions path={addToken(activePlaylisPath)} downloadName={`${safeTitle}_active.m3u8`} />
              </Flex>
            )}
            {segmentHistory.map((seg, i) => (
              <Flex key={seg} align="center" gap="2" style={{ opacity: i === 0 ? 1 : 0.5 }}>
                <Badge size="1" variant="soft" color="gray" style={{ fontSize: 9, flexShrink: 0 }}>{i === 0 ? "seg" : `seg-${i}`}</Badge>
                <Text size="1" color="gray" style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{seg}</Text>
                <UrlActions path={seg} downloadName={`${safeTitle}_${seg.split("/").pop() || "segment.ts"}`} />
              </Flex>
            ))}
          </Flex>
        )}
      </Flex>
    </Card>
  );
}

export function Player() {
  const { id: routeId } = useParams<{ id: string }>();
  const location = useLocation();
  const navigate = useNavigate();
  const locState = location.state as { magnet?: string; poster?: string; meta?: Record<string, unknown>; directUrl?: string; hlsUrl?: string } | null;

  const isDemo = routeId === "demo";
  const magnet = locState?.magnet || null;

  const [streamId, setStreamId] = useState<string | null>(
    routeId?.startsWith("pending-") ? null : (routeId ?? null)
  );

  const [poster, setPoster] = useState<string | null>(() => {
    const fromNav = locState?.poster || null;
    if (fromNav) return fromNav;
    if (routeId && !routeId.startsWith("pending-")) {
      return localStorage.getItem(`streamx_poster_${routeId}`);
    }
    return null;
  });

  // --- Start stream from magnet ---
  useEffect(() => {
    if (!magnet || isDemo) return;
    let cancelled = false;
    const meta = locState?.meta || null;

    const tryStart = async () => {
      for (let attempt = 0; attempt < 3; attempt++) {
        try {
          const res = await api.startStream({
            magnet_uri: magnet,
            poster_url: poster || (meta?.poster as string) || undefined,
            title: (meta?.title as string) || undefined,
            year: (meta?.year as number) || undefined,
            rating: (meta?.rating as number) || undefined,
            runtime: (meta?.runtime as number) || undefined,
            genres: (meta?.genres as string[]) || undefined,
            language: (meta?.language as string) || undefined,
            video_codec: (meta?.video_codec as string) || undefined,
            audio_channels: (meta?.audio_channels as string) || undefined,
            source_type: (meta?.source_type as string) || undefined,
            summary: (meta?.summary as string) || undefined,
            imdb_code: (meta?.imdb_code as string) || undefined,
            mpa_rating: (meta?.mpa_rating as string) || undefined,
            bit_depth: (meta?.bit_depth as string) || undefined,
            trailer_code: (meta?.trailer_code as string) || undefined,
            poster_small: (meta?.poster_small as string) || undefined,
            poster_medium: (meta?.poster_medium as string) || undefined,
            poster_large: (meta?.poster_large as string) || undefined,
            backdrop: (meta?.backdrop as string) || undefined,
          });
          if (!cancelled && res.stream_id) {
            if (poster) localStorage.setItem(`streamx_poster_${res.stream_id}`, poster);
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
  }, [magnet, isDemo, navigate]); // eslint-disable-line react-hooks/exhaustive-deps

  // --- Persist poster ---
  useEffect(() => {
    if (poster && streamId) localStorage.setItem(`streamx_poster_${streamId}`, poster);
  }, [poster, streamId]);

  // --- Stream state ---
  const { user, isGuest } = useAuth();
  const isAdmin = user?.is_admin === true;
  const { isFavourite, addFavourite, removeFavouriteByTitle } = useFavourites();
  const playerRef = useRef<VideoPlayerHandle>(null);
  const { status, fileUrl, error, metadata: wsMeta } = useStream(isDemo ? null : streamId);
  const meta: Record<string, unknown> | null = wsMeta ? (wsMeta as unknown as Record<string, unknown>) : (locState?.meta ?? null);

  // Pick up poster from WS metadata if not already set
  useEffect(() => {
    if (poster) return;
    if (!wsMeta) return;
    const m = wsMeta as unknown as Record<string, unknown>;
    const wsPoster = (m.poster_large as string) || (m.local_poster as string) || null;
    if (wsPoster) {
      setPoster(wsPoster);
      if (streamId) localStorage.setItem(`streamx_poster_${streamId}`, wsPoster);
    }
  }, [wsMeta, poster, streamId]);

  const [useHls, setUseHls] = useState(false);
  // Server error / reconnection state
  const [serverRecovering, setServerRecovering] = useState(false);
  const [showOfflineBanner, setShowOfflineBanner] = useState(false);
  const offlineTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleServerError = useCallback((recovering: boolean) => {
    setServerRecovering(recovering);
    if (recovering) {
      if (!offlineTimerRef.current) {
        offlineTimerRef.current = setTimeout(() => {
          setShowOfflineBanner(true);
        }, 60000);
      }
    } else {
      if (offlineTimerRef.current) {
        clearTimeout(offlineTimerRef.current);
        offlineTimerRef.current = null;
      }
      setShowOfflineBanner(false);
    }
  }, []);

  const QUALITY_STORAGE_KEY = "streamx_preferred_quality";
  const serverSettings = useServerSettings();

  // Pin / client-download controls in the stream header. Pinned state
  // comes from the downloads queue; toggling updates it optimistically.
  const [pinnedState, setPinnedState] = useState<boolean | null>(null);
  useEffect(() => {
    let live = true;
    if (!streamId) return;
    (async () => {
      try {
        const res = await api.listDownloads();
        const dl = res.downloads.find(
          (d) => d.info_hash.toLowerCase() === streamId.toLowerCase()
        );
        if (live) setPinnedState(dl?.pinned ?? false);
      } catch {
        // Downloads list unavailable; leave controls in the default state.
      }
    })();
    return () => {
      live = false;
    };
  }, [streamId]);

  const togglePin = useCallback(async () => {
    if (!streamId) return;
    try {
      if (pinnedState) {
        await api.unpinDownload(streamId);
        setPinnedState(false);
      } else {
        await api.pinDownload(streamId);
        setPinnedState(true);
      }
    } catch (err) {
      debugLog.warn("player", `pin toggle failed: ${err}`);
    }
  }, [streamId, pinnedState]);
  // Conservative default: treat transcode as disabled until settings load.
  const transcodeDisabled = serverSettings?.disable_transcode ?? true;
  const [selectedQuality, setSelectedQuality] = useState<string>(() =>
    localStorage.getItem(QUALITY_STORAGE_KEY) || "source"
  );
  const qualityOptions = ["source", "1080p", "720p", "360p"];

  // Detect HEVC from codec metadata OR filename (metadata may be unavailable for new streams)
  const codecStr = status?.video_codec?.toLowerCase() ?? "";
  const fileStr = status?.file_name?.toLowerCase() ?? "";
  const sourceIsHevc = /hevc|h265|hev1|hvc1/.test(codecStr)
    || /h[\s._-]?265|hevc/i.test(fileStr);

  const handleQualityChange = (q: string) => {
    setSelectedQuality(q);
    localStorage.setItem(QUALITY_STORAGE_KEY, q);
  };

  const [bufferInfo, setBufferInfo] = useState({
    bufferedSeconds: 0, currentTime: 0, duration: 0, readyState: 0, playing: false, videoHeight: 0, currentSrc: "",
  });

  // Media Session API: lock screen controls, Dynamic Island, background audio
  const videoTitle = String(ms(meta, "title") || status?.title || "");
  const runtimeMin = mn(meta, "runtime");
  debugLog.debug("player", `media-session: title=${videoTitle} poster=${poster?.substring(0, 40) || "none"}`);
  useMediaSession({
    title: videoTitle || "StreamX",
    artist: [mn(meta, "year") ? String(mn(meta, "year")) : null, ms(meta, "language")?.toUpperCase()].filter(Boolean).join(" - ") || "StreamX",
    artwork: poster ?? DEFAULT_VIDEO_POSTER_URL,
    duration: runtimeMin ? runtimeMin * 60 : bufferInfo.duration,
    currentTime: bufferInfo.currentTime,
    playing: bufferInfo.playing,
    onPlay: () => playerRef.current?.play(),
    onPause: () => playerRef.current?.pause(),
    onSeekForward: () => playerRef.current?.seek(bufferInfo.currentTime + 10),
    onSeekBackward: () => playerRef.current?.seek(Math.max(0, bufferInfo.currentTime - 10)),
    onSeekTo: (t) => playerRef.current?.seek(t),
  });

  // Detect codec/container support - only switch to HLS when enough data to transcode
  const needsHlsRef = useRef(false);
  useEffect(() => {
    if (useHls || isDemo) return;
    if (transcodeDisabled) {
      // Server-side transcoding is off: stay on direct playback and let
      // the browser attempt whatever the file is.
      return;
    }

    let unsupported = false;

    // Check codec if available
    const codec = status?.video_codec?.toLowerCase();
    if (codec) {
      const v = document.createElement("video");
      if (codec === "x265" || codec === "hevc" || codec === "hev1" || codec === "hvc1") {
        unsupported = v.canPlayType('video/mp4; codecs="hvc1"') === "" && v.canPlayType('video/mp4; codecs="hev1"') === "";
      } else if (codec === "vp9") {
        unsupported = v.canPlayType('video/webm; codecs="vp9"') === "";
      } else if (codec === "av1") {
        unsupported = v.canPlayType('video/mp4; codecs="av01.0.01M.08"') === "";
      }
    }

    // Check container from filename - MKV is never browser-compatible
    const fileName = status?.file_name?.toLowerCase() ?? "";
    if (!unsupported && (fileName.endsWith(".mkv") || fileName.endsWith(".avi"))) {
      unsupported = true;
    }

    // Detect HEVC from filename when codec metadata is missing (common for new streams)
    if (!codec && /h[\s._-]?265|hevc/i.test(fileName)) {
      unsupported = true;
    }

    needsHlsRef.current = unsupported;
    if (unsupported) {
      const progress = status?.progress ?? 0;
      const ready = status?.status === "complete" || progress >= 5;
      if (ready) {
        debugLog.warn("player", `Unsupported format (codec=${codec} file=${fileName}), switching to HLS`);
        setUseHls(true);

        // If source is HEVC and browser can't decode HEVC natively,
        // auto-select a transcoded tier instead of "source" (which would copy HEVC)
        const isHevc = codec === "x265" || codec === "hevc" || codec === "hev1" || codec === "hvc1";
        const v = document.createElement("video");
        const browserSupportsHevc = v.canPlayType('video/mp4; codecs="hvc1"') !== "" || v.canPlayType('video/mp4; codecs="hev1"') !== "";
        if (isHevc && !browserSupportsHevc && selectedQuality === "source") {
          debugLog.warn("player", "Browser lacks HEVC support, selecting 1080p instead of source");
          setSelectedQuality("1080p");
        }
      } else {
        debugLog.info("player", `Unsupported format, waiting for data (${progress.toFixed(1)}%)`);
      }
    }
  }, [status?.video_codec, status?.file_name, status?.progress, status?.status, useHls, isDemo, selectedQuality, transcodeDisabled]);

  const hlsUrl = streamId ? api.getPlaylistUrl(streamId, selectedQuality) : null;
  // URL-based streams (direct HTTPS or HLS-transcoded HTTPS)
  const urlDirect = locState?.directUrl || null;
  const urlHls = locState?.hlsUrl || null;
  const isUrlStream = !!(urlDirect || urlHls);
  const [urlFallbackHls, setUrlFallbackHls] = useState<string | null>(null);

  const videoUrl = isDemo
    ? DEMO_HLS_URL
    : isUrlStream
      ? (urlFallbackHls || urlHls || urlDirect)
      : (useHls && hlsUrl ? hlsUrl : fileUrl);
  const videoReady = isDemo || isUrlStream || (needsHlsRef.current ? useHls : fileUrl !== null);
  const displayProgress = status?.status === "complete" ? 100 : (status?.progress ?? 0);

  // --- Overlay: visible when video is not playing ---
  // userPlaying = user clicked play (hides overlay immediately)
  // Reset on visibility change so overlay comes back if video died
  const [userPlaying, setUserPlaying] = useState(false);
  const [hasPlayed, setHasPlayed] = useState(false);
  const [summaryExpanded, setSummaryExpanded] = useState(false);
  const [showTrailer, setShowTrailer] = useState(false);
  // Once video has played, never show the poster overlay again
  const overlayVisible = !hasPlayed && !userPlaying && !bufferInfo.playing;



  useEffect(() => {
    const handler = () => {
      if (document.visibilityState === "hidden") {
        // Background: audio continues natively on iOS with Media Session + SW active
        debugLog.info("player", "Entering background, audio continues");
      } else {
        // Foreground: video rendering resumes automatically
        debugLog.info("player", "Returning to foreground");
        // Only reset overlay if video hasn't played yet (don't show poster over playing video)
        setTimeout(() => {
          if (!hasPlayed) setUserPlaying(false);
        }, 500);
      }
    };
    document.addEventListener("visibilitychange", handler);
    return () => document.removeEventListener("visibilitychange", handler);
  }, [hasPlayed]);

  // If bufferInfo.playing becomes true, mark as played and cancel timeout
  useEffect(() => {
    if (bufferInfo.playing) {
      setUserPlaying(true);
      setHasPlayed(true);
      if (playTimeoutRef.current) {
        clearTimeout(playTimeoutRef.current);
        playTimeoutRef.current = null;
      }
    }
  }, [bufferInfo.playing]);

  // Auto-fallback: if HEVC source stalls (currentTime stuck near 0 for 8s), switch to 1080p
  const hevcStallRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (!useHls || !sourceIsHevc || selectedQuality !== "source") {
      if (hevcStallRef.current) { clearTimeout(hevcStallRef.current); hevcStallRef.current = null; }
      return;
    }
    if (bufferInfo.currentTime > 1) {
      // Playing fine, cancel stall timer
      if (hevcStallRef.current) { clearTimeout(hevcStallRef.current); hevcStallRef.current = null; }
      return;
    }
    if (!hevcStallRef.current && userPlaying) {
      hevcStallRef.current = setTimeout(() => {
        hevcStallRef.current = null;
        if (bufferInfo.currentTime < 1) {
          debugLog.warn("player", "HEVC source stalled for 8s, falling back to 1080p");
          setSelectedQuality("1080p");
          localStorage.setItem(QUALITY_STORAGE_KEY, "1080p");
        }
      }, 8000);
    }
  }, [useHls, sourceIsHevc, selectedQuality, bufferInfo.currentTime, userPlaying]);

  const handlePlayError = useCallback((err: string) => {
    if (err === "not_supported") {
      if (playRetryRef.current) { clearInterval(playRetryRef.current); playRetryRef.current = null; }
      if (playTimeoutRef.current) { clearTimeout(playTimeoutRef.current); playTimeoutRef.current = null; }

      if (urlDirect && !urlFallbackHls) {
        debugLog.warn("player", "Direct URL not supported, switching to HLS transcode");
        setUrlFallbackHls(api.getUrlPlaylistUrl(urlDirect, selectedQuality));
      } else if (!useHls && hlsUrl) {
        debugLog.warn("player", "Direct playback not supported, switching to HLS transcode");
        setUseHls(true);
      } else if (useHls && selectedQuality === "source") {
        // Source quality failed - likely HEVC or codec browser can't handle
        // Auto-downgrade to 1080p which always transcodes to H.264
        debugLog.warn("player", "Source quality failed, falling back to 1080p");
        setSelectedQuality("1080p");
        localStorage.setItem(QUALITY_STORAGE_KEY, "1080p");
      }
      setUserPlaying(false);
    }
  }, [useHls, hlsUrl, streamId, urlDirect, urlFallbackHls, selectedQuality, sourceIsHevc]);

  const bufferInfoRef = useRef(bufferInfo);
  const handleBufferInfo = useCallback(
    (info: { bufferedSeconds: number; currentTime: number; duration: number; readyState: number; playing: boolean; videoHeight: number; currentSrc: string }) => {
      if (document.fullscreenElement) return;
      const prev = bufferInfoRef.current;
      const changed =
        Math.abs(prev.bufferedSeconds - info.bufferedSeconds) > 0.5 ||
        prev.playing !== info.playing ||
        prev.videoHeight !== info.videoHeight ||
        prev.readyState !== info.readyState ||
        prev.currentSrc !== info.currentSrc;
      if (changed) {
        bufferInfoRef.current = info;
        setBufferInfo(info);
      }
    },
    []
  );

  const playTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const playRetryRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const statusRef = useRef(status);
  statusRef.current = status;

  const tryPlay = useCallback(() => {
    if (playRetryRef.current) { clearInterval(playRetryRef.current); playRetryRef.current = null; }
    const result = playerRef.current?.play();
    debugLog.info("player", `tryPlay ref=${!!playerRef.current} result=${result}`);
    if (result) {
      setUserPlaying(true);
      // Only set fallback timeout for direct file playback (not HLS).
      // Once in HLS mode, playback stalls are handled by video.js retry logic.
      if (playTimeoutRef.current) clearTimeout(playTimeoutRef.current);
      if (!useHls) {
        const fileName = statusRef.current?.file_name?.toLowerCase() ?? "";
        const knownIncompatible = fileName.endsWith(".mkv") || fileName.endsWith(".avi") || fileName.endsWith(".wmv") || fileName.endsWith(".flv");
        if (knownIncompatible) {
          playTimeoutRef.current = setTimeout(() => {
            debugLog.warn("player", `play timeout 10s, incompatible file: ${fileName}`);
            handlePlayError("not_supported");
          }, 10000);
        }
      }
    } else {
      playRetryRef.current = setInterval(() => {
        const r = playerRef.current?.play();
        if (r) {
          if (playRetryRef.current) { clearInterval(playRetryRef.current); playRetryRef.current = null; }
          setUserPlaying(true);
        }
      }, 500);
    }
  }, [handlePlayError, useHls]);

  const handlePlay = useCallback(() => {
    tryPlay();
  }, [tryPlay]);


  const lastHistoryUpdate = useRef(0);
  const handleTimeUpdate = useCallback(
    (time: number) => {
      if (!streamId || isDemo || time < 5) return;
      const now = Date.now();
      if (now - lastHistoryUpdate.current > 10000) {
        lastHistoryUpdate.current = now;
        api.updateWatchPosition(streamId, time).catch(() => {});
      }
    },
    [streamId, isDemo]
  );

  return (
    <div style={{ position: "relative", minHeight: "100%" }}>
      <CinematicBg poster={ms(meta, "backdrop") || poster} />

      <Flex direction="column" gap="4">
        <Flex align="center" gap="3">
          <Button variant="ghost" size="1" onClick={() => navigate(-1)}>
            <ArrowLeftIcon width={18} height={18} />
          </Button>
          {isDemo && (
            <Badge size="2" color="blue" variant="surface">
              Demo
            </Badge>
          )}
        </Flex>

        {/* Video box */}
        <Box style={{ width: "100%", aspectRatio: "16/9", background: "#000", borderRadius: 8, overflow: "hidden", position: "relative", border: "1px solid var(--gray-a5)" }}>
          {videoReady && videoUrl && (
            <VideoPlayer
              key={videoUrl}
              ref={playerRef}
              src={videoUrl}
              durationSeconds={mn(meta, "runtime") ? (mn(meta, "runtime") ?? 0) * 60 : undefined}
              onTimeUpdate={handleTimeUpdate}
              onBufferInfo={handleBufferInfo}
              onPlayError={handlePlayError}
              onServerError={handleServerError}
            />
          )}
          {(overlayVisible || (!videoReady && !hasPlayed)) && (
            <VideoOverlay
              poster={poster}
              videoReady={videoReady}
              error={error}
              onPlay={handlePlay}
              trailerCode={ms(meta, "trailer_code") || null}
              trailerSearch={meta ? `${ms(meta, "title") || ""} ${mn(meta, "year") || ""} official trailer` : null}
              onTrailer={() => setShowTrailer(true)}
            />
          )}
        </Box>

        {/* Media info + stream status */}
        {!isDemo && status && (
          <Flex direction="column" gap="3">
            {/* Title + metadata row */}
            {meta && (
              <Card>
                <Flex direction="column" gap="2">
                  <Flex align="baseline" gap="2" wrap="wrap">
                    <Text size="4" weight="bold">
                      {ms(meta, "title") || status.title || "Untitled"}
                    </Text>
                    {mn(meta, "year") && (
                      <Text size="3" color="gray">({mn(meta, "year")})</Text>
                    )}
                    {mn(meta, "rating") != null && (mn(meta, "rating") ?? 0) > 0 && (
                      <Text size="2" color="amber">{"\u2605"} {mn(meta, "rating")?.toFixed(1)}</Text>
                    )}
                  </Flex>

                  <Flex gap="2" align="center" wrap="wrap">
                    {mn(meta, "runtime") != null && (mn(meta, "runtime") ?? 0) > 0 && (
                      <Badge size="1" variant="soft" color="gray">{formatRuntime(mn(meta, "runtime") ?? 0)}</Badge>
                    )}
                    {ms(meta, "mpa_rating") && (
                      <Badge size="1" variant="outline">{ms(meta, "mpa_rating")}</Badge>
                    )}
                    {ms(meta, "language") && ms(meta, "language") !== "en" && (
                      <Badge size="1" variant="soft">{ms(meta, "language")?.toUpperCase()}</Badge>
                    )}
                    {(() => {
                      const g = meta.genres;
                      const genreList: string[] = typeof g === "string" ? g.split(",").map((s: string) => s.trim()).filter(Boolean) : Array.isArray(g) ? g as string[] : [];
                      return genreList.slice(0, 3).map((genre: string) => (
                        <Badge key={genre} size="1" variant="soft" color="blue">{genre}</Badge>
                      ));
                    })()}
                  </Flex>

                  {ms(meta, "summary") && (
                    <Flex
                      direction="column"
                      gap="1"
                      onClick={(e) => { e.stopPropagation(); setSummaryExpanded((v) => !v); }}
                      style={{ cursor: "pointer" }}
                    >
                      <Text
                        size="2"
                        color="gray"
                        style={summaryExpanded ? {} : { display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}
                      >
                        {ms(meta, "summary")}
                      </Text>
                      <Flex justify="center">
                        {summaryExpanded
                          ? <ChevronUpIcon width={14} height={14} color="var(--gray-a9)" />
                          : <ChevronDownIcon width={14} height={14} color="var(--gray-a9)" />}
                      </Flex>
                    </Flex>
                  )}
                </Flex>
              </Card>
            )}

            {/* Action buttons */}
            <Flex gap="2" justify="end" wrap="wrap" align="center">
              {!isGuest && meta && (() => {
                const title = String(ms(meta, "title") || status?.title || "");
                const year = mn(meta, "year") ?? undefined;
                const active = isFavourite(title, year);
                return (
                  <button
                    style={{ background: "var(--color-surface)", border: "none", borderRadius: 6, cursor: "pointer", padding: "7px 10px", display: "flex", alignItems: "center", transition: "transform 0.15s" }}
                    onClick={async () => {
                      if (active) {
                        await removeFavouriteByTitle(title, year);
                      } else {
                        await addFavourite({
                          content_type: "movie",
                          title,
                          year: year ?? null,
                          rating: (mn(meta, "rating") as number) ?? null,
                          poster_url: (ms(meta, "poster_medium") || ms(meta, "poster_large") || poster) ?? null,
                          info_hash: streamId ?? null,
                          metadata_json: JSON.stringify({
                            genres: meta?.genres,
                            summary: ms(meta, "summary"),
                            imdb_code: ms(meta, "imdb_code"),
                            poster_large: ms(meta, "poster_large"),
                            backdrop: ms(meta, "backdrop"),
                          }),
                        });
                      }
                    }}
                    title={active ? "Remove from favourites" : "Add to favourites"}
                  >
                    <svg width={22} height={22} viewBox="0 0 24 24" fill={active ? "#facc15" : "none"} stroke="#facc15" strokeWidth={2}>
                      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
                    </svg>
                  </button>
                );
              })()}
              {status.status === "complete" && fileUrl && (
                <a href={fileUrl} download style={{ textDecoration: "none" }}>
                  <Button variant="soft" size="1" style={{ fontSize: "var(--font-size-2)", fontWeight: 700, height: "auto", padding: "7px 12px" }}>
                    <DownloadIcon width={14} height={14} /> Download
                  </Button>
                </a>
              )}
              {!isGuest && streamId && (
                <ShareButton streamId={streamId} />
              )}
              {isAdmin && streamId && (
                <Button
                  variant="soft"
                  size="1"
                  color="red"
                  style={{ fontSize: "var(--font-size-2)", fontWeight: 700, height: "auto", padding: "7px 12px" }}
                  onClick={() => api.deleteStream(streamId).catch(() => {})}
                >
                  <TrashIcon width={14} height={14} /> Delete
                </Button>
              )}
            </Flex>

            {/* Technical info + stream status */}
            <Card>
              <Flex direction="column" gap="3">
                <Flex align="center" justify="between">
                  <Flex gap="2" align="center" wrap="wrap">
                    {status.status !== "downloading" && <StatusBadge status={status.status} />}
                    {useHls && <Badge size="1" variant="surface" color="violet">HLS</Badge>}
                    {!useHls && fileUrl && <Badge size="1" variant="surface" color="blue">Direct</Badge>}
                    {(ms(meta, "video_codec") || status.video_codec) && (
                      <Badge size="1" variant="soft" color="gray">
                        {(ms(meta, "video_codec") || status.video_codec || "").toUpperCase()}
                      </Badge>
                    )}
                    {ms(meta, "audio_channels") && (
                      <Badge size="1" variant="soft" color="gray">
                        {ms(meta, "audio_channels")?.includes("5.1") || ms(meta, "audio_channels") === "6" ? "5.1" : ms(meta, "audio_channels")}ch
                      </Badge>
                    )}
                    {ms(meta, "source_type") && (
                      <Badge size="1" variant="soft" color="gray">
                        {ms(meta, "source_type") === "bluray" ? "BluRay" : ms(meta, "source_type") === "web" ? "WEB" : ms(meta, "source_type")?.toUpperCase()}
                      </Badge>
                    )}
                    {ms(meta, "bit_depth") && (
                      <Badge size="1" variant="soft" color="gray">{ms(meta, "bit_depth")}bit</Badge>
                    )}
                    {status.file_size != null && status.file_size > 0 && (
                      <Badge size="1" variant="soft" color="gray">{formatBytes(status.file_size)}</Badge>
                    )}
                    {(serverSettings?.disable_transcode ?? true) &&
                      ms(meta, "source_type") &&
                      ms(meta, "source_type") !== "web" && <NotWebBadge />}
                  </Flex>
                  <Flex gap="2" align="center" style={{ flexShrink: 0 }}>
                    {status.status === "complete" ? (
                      <Button
                        size="1"
                        variant="soft"
                        title="Download the file to this device"
                        onClick={() => {
                          if (!streamId) return;
                          const token = localStorage.getItem("streamx_token") || "";
                          const rawTitle = String(ms(meta, "title") || status.title || "movie");
                          const movieName = rawTitle.replace(/[\\/:*?"<>|]/g, "-").trim();
                          const fname = status.file_name || "";
                          const ext = fname.includes(".") ? fname.slice(fname.lastIndexOf(".")) : "";
                          const a = document.createElement("a");
                          a.href = `/api/stream/${streamId}/file?token=${encodeURIComponent(token)}`;
                          a.download = `${movieName}${ext}`;
                          document.body.appendChild(a);
                          a.click();
                          a.remove();
                        }}
                      >
                        <DownloadIcon width={12} height={12} />
                        <Box as="span" display={{ initial: "none", sm: "inline" }}>Download</Box>
                      </Button>
                    ) : pinnedState ? (
                      <Button
                        size="1"
                        variant="soft"
                        color="orange"
                        onClick={togglePin}
                        title="Unpin: stop the server-side background download"
                      >
                        <Cross2Icon width={12} height={12} />
                        <Box as="span" display={{ initial: "none", sm: "inline" }}>Unpin</Box>
                      </Button>
                    ) : (
                      <Button
                        size="1"
                        variant="soft"
                        onClick={togglePin}
                        title="Pin: keep downloading on the server and watch later"
                      >
                        <DrawingPinIcon width={12} height={12} />
                        <Box as="span" display={{ initial: "none", sm: "inline" }}>Pin</Box>
                      </Button>
                    )}
                  </Flex>
                </Flex>

                <Flex direction="column" gap="2">
                  <StreamBar
                    bufferedSeconds={bufferInfo.bufferedSeconds}
                    playing={bufferInfo.playing}
                    fileReady={videoReady}
                    speed={status?.speed ?? 0}
                    progress={status?.progress ?? 0}
                  />
                  <TotalBar progress={displayProgress} />
                </Flex>

                <Flex align="center" justify="between" wrap="wrap" gap="2">
                  <Flex gap="4" wrap="wrap">
                    <Text size="1" color="gray" style={{ minWidth: 55 }}>
                      Peers <Text weight="medium" size="1">{status.peers ?? 0}</Text>
                    </Text>
                    <Text size="1" color="gray" style={{ minWidth: 80 }}>
                      <Text weight="medium" size="1">{formatSpeed(status.speed ?? 0)}</Text>
                    </Text>
                    {useHls && !transcodeDisabled && (
                      <Flex gap="2" align="center">
                        <Text size="1" color="violet">
                          {bufferInfo.videoHeight > 0
                            ? `HLS ${bufferInfo.videoHeight}p`
                            : "Transcoding"}
                        </Text>
                        <Select.Root size="1" value={selectedQuality} onValueChange={handleQualityChange}>
                          <Select.Trigger variant="ghost" />
                          <Select.Content variant="soft">
                            {qualityOptions.map((q) => (
                              <Select.Item key={q} value={q}>
                                {q === "source"
                                  ? `Original${bufferInfo.videoHeight > 0 ? ` (${bufferInfo.videoHeight}p)` : ""}${sourceIsHevc ? " (HEVC)" : ""}`
                                  : q}
                              </Select.Item>
                            ))}
                          </Select.Content>
                        </Select.Root>
                      </Flex>
                    )}
                  </Flex>
                  <Flex gap="1" />
                </Flex>
              </Flex>
            </Card>

            <StreamUrls currentSrc={bufferInfo.currentSrc} videoUrl={videoUrl} streamId={streamId} title={String(ms(meta, "title") || status?.title || "")} />
          </Flex>
        )}

        {error && !isDemo && (
          <Text size="2" color="red">{error}</Text>
        )}

        {serverRecovering && !showOfflineBanner && (
          <Card>
            <Flex align="center" gap="2">
              <div style={{ width: 14, height: 14, border: "2px solid var(--amber-9)", borderTopColor: "transparent", borderRadius: "50%", animation: "spin 0.8s linear infinite", flexShrink: 0 }} />
              <Text size="2" color="amber">Reconnecting to server...</Text>
            </Flex>
          </Card>
        )}

        {showOfflineBanner && (
          <Card style={{ border: "1px solid var(--red-a6)" }}>
            <Flex direction="column" gap="2">
              <Text size="2" weight="medium" color="red">Server appears offline</Text>
              <Text size="2" color="gray">Check your internet connection. Playback will resume automatically when the server is back.</Text>
              <Flex gap="2">
                <Button size="1" variant="solid" color="blue" onClick={() => window.location.reload()}>
                  Refresh page
                </Button>
                <Button size="1" variant="ghost" onClick={() => setShowOfflineBanner(false)}>
                  Dismiss
                </Button>
              </Flex>
            </Flex>
          </Card>
        )}
      </Flex>
      {showTrailer && (
        <TrailerModal
          youtubeId={ms(meta, "trailer_code") || undefined}
          searchQuery={meta ? `${ms(meta, "title") || ""} ${mn(meta, "year") || ""} official trailer` : undefined}
          onClose={() => setShowTrailer(false)}
        />
      )}
    </div>
  );
}
