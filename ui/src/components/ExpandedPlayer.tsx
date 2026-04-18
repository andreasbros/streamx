import { useState, useEffect, useRef, useCallback, type CSSProperties } from "react";
import { Flex, Text, Badge, DropdownMenu, Select } from "@radix-ui/themes";
import {
  ChevronDownIcon,
  PlayIcon,
  PauseIcon,
  TrackPreviousIcon,
  TrackNextIcon,
  LoopIcon,
  StarIcon,
  StarFilledIcon,
  PlusCircledIcon,
  ListBulletIcon,
  Share1Icon,
  CheckIcon,
} from "@radix-ui/react-icons";
import { useAudioPlayer, pauseTimeUpdates, resumeTimeUpdates } from "../hooks/useAudioPlayer";
import { useAuth } from "../hooks/useAuth";
import { useFavourites } from "../hooks/useFavourites";
import { useVersionCheck } from "../hooks/useVersionCheck";
import { api } from "../api/client";
import { DEFAULT_VIDEO_POSTER_URL } from "../assets";
import type { Playlist } from "../api/types";

function formatTime(seconds: number): string {
  if (!isFinite(seconds) || seconds < 0) return "0:00";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(0)} MB`;
  return `${(bytes / 1e3).toFixed(0)} KB`;
}

function MarqueeText({ text, style }: { text: string; style?: CSSProperties }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLSpanElement>(null);
  const [needsScroll, setNeedsScroll] = useState(false);
  const [animDuration, setAnimDuration] = useState(10);

  useEffect(() => {
    const container = containerRef.current;
    const inner = innerRef.current;
    if (!container || !inner) return;
    // Measure single copy (before duplication is rendered)
    const overflows = inner.scrollWidth > container.clientWidth + 2;
    setNeedsScroll(overflows);
    if (overflows) {
      // Speed: ~40px per second, so longer text scrolls proportionally
      const singleWidth = inner.scrollWidth;
      setAnimDuration(Math.max(6, singleWidth / 40));
    }
  }, [text]);

  const separator = "\u00A0\u00A0\u00A0\u2022\u00A0\u00A0\u00A0"; // 3 spaces + bullet + 3 spaces

  const fadeMask = needsScroll
    ? "linear-gradient(to right, transparent 0%, black 8%, black 92%, transparent 100%)"
    : undefined;

  return (
    <div
      ref={containerRef}
      style={{
        overflow: "hidden",
        maxWidth: "85vw",
        WebkitMaskImage: fadeMask,
        maskImage: fadeMask,
        ...style,
      }}
    >
      <span
        ref={innerRef}
        style={{
          whiteSpace: "nowrap",
          display: "inline-block",
          animation: needsScroll ? `marquee-seamless ${animDuration}s linear 2s infinite` : undefined,
        }}
      >
        {needsScroll ? (
          <>{text}{separator}{text}{separator}</>
        ) : (
          text
        )}
      </span>
    </div>
  );
}

// Web Audio API equalizer presets
const EQ_PRESETS: Record<string, number[]> = {
  Flat: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  Bass: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0],
  Treble: [0, 0, 0, 0, 0, 2, 4, 5, 6, 6],
  "V-Shape": [5, 4, 2, 0, -2, -2, 0, 2, 4, 5],
  Rock: [5, 3, 1, 0, -1, -1, 0, 2, 3, 4],
  Pop: [1, 3, 5, 4, 2, 0, -1, 0, 2, 3],
  Jazz: [3, 2, 1, 2, -1, -1, 0, 1, 2, 3],
  Classical: [4, 3, 2, 1, 0, 0, 0, 1, 2, 3],
  "Bass Boost": [8, 6, 4, 2, 0, 0, 0, 0, 0, 0],
  Vocal: [0, 0, 2, 4, 5, 5, 4, 2, 0, 0],
};

const EQ_BANDS = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

const EQ_STORAGE_KEY = "streamx_eq_preset";

export function ExpandedPlayer({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const {
    currentTrack, isPlaying, duration, currentTime, queue, queueIndex, repeat,
    pause, resume, seek, next, previous, toggleRepeat, audioRef,
  } = useAudioPlayer();
  const { favourites, addFavourite, removeFavourite } = useFavourites();
  const { updateAvailable, reload } = useVersionCheck();

  const [imgSrc, setImgSrc] = useState(DEFAULT_VIDEO_POSTER_URL);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [shareLoading, setShareLoading] = useState(false);
  const [shareCopied, setShareCopied] = useState(false);
  const [isSeeking, setIsSeeking] = useState(false);
  const seekRatioRef = useRef(0);
  const seekBarRef = useRef<HTMLDivElement>(null);
  const seekTimeRef = useRef<HTMLSpanElement>(null);
  const seekTimeEndRef = useRef<HTMLSpanElement>(null);
  const [airplayAvailable, setAirplayAvailable] = useState(false);
  const [airplayActive, setAirplayActive] = useState(false);
  const sliderRef = useRef<HTMLDivElement>(null);
  const [eqPreset, setEqPreset] = useState(() => {
    try { return localStorage.getItem(EQ_STORAGE_KEY) ?? "Flat"; } catch { return "Flat"; }
  });

  // Web Audio EQ nodes
  const audioCtxRef = useRef<AudioContext | null>(null);
  const sourceRef = useRef<MediaElementAudioSourceNode | null>(null);
  const filtersRef = useRef<BiquadFilterNode[]>([]);

  const touchStartY = useRef(0);
  const dragOffsetRef = useRef(0);
  const isDragging = useRef(false);
  const panelRef = useRef<HTMLDivElement>(null);

  // Update artwork when track changes
  useEffect(() => {
    setImgSrc(currentTrack?.artworkUrl ?? DEFAULT_VIDEO_POSTER_URL);
  }, [currentTrack?.artworkUrl, currentTrack?.title]);

  // Lock body scroll when expanded
  useEffect(() => {
    if (open) {
      document.body.style.overflow = "hidden";
      return () => { document.body.style.overflow = ""; };
    }
  }, [open]);

  // iOS detection: createMediaElementSource breaks background playback
  const { isGuest } = useAuth();
  const isIOS = /iPad|iPhone|iPod/.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);

  // Detect AirPlay availability (Safari only)
  useEffect(() => {
    const audio = audioRef.current as HTMLAudioElement & {
      webkitShowPlaybackTargetPicker?: () => void;
    } | null;
    if (!audio) return;

    // Check if WebKit AirPlay API is available
    if ("webkitShowPlaybackTargetPicker" in (audio as object)) {
      setAirplayAvailable(true);
    }

    // Listen for AirPlay connection changes
    const onAvailability = (e: Event) => {
      const ev = e as Event & { availability?: string };
      setAirplayAvailable(ev.availability === "available");
    };
    const onCurrentChange = (e: Event) => {
      const ev = e as Event & { target?: HTMLAudioElement & { webkitCurrentPlaybackTargetIsWireless?: boolean } };
      setAirplayActive(ev.target?.webkitCurrentPlaybackTargetIsWireless ?? false);
    };

    audio.addEventListener("webkitplaybacktargetavailabilitychanged", onAvailability);
    audio.addEventListener("webkitcurrentplaybacktargetiswirelesschanged", onCurrentChange);

    return () => {
      audio.removeEventListener("webkitplaybacktargetavailabilitychanged", onAvailability);
      audio.removeEventListener("webkitcurrentplaybacktargetiswirelesschanged", onCurrentChange);
    };
  }, [audioRef]);

  // Initialize Web Audio EQ only on non-iOS when user selects non-Flat preset
  const initEq = useCallback(() => {
    if (isIOS) return; // Never hijack audio element on iOS
    const audio = audioRef.current;
    if (!audio || audioCtxRef.current) return;

    try {
      const ctx = new AudioContext();
      const source = ctx.createMediaElementSource(audio);
      audioCtxRef.current = ctx;
      sourceRef.current = source;

      const filters = EQ_BANDS.map((freq, i) => {
        const filter = ctx.createBiquadFilter();
        filter.type = i === 0 ? "lowshelf" : i === EQ_BANDS.length - 1 ? "highshelf" : "peaking";
        filter.frequency.value = freq;
        filter.Q.value = 1.4;
        filter.gain.value = 0;
        return filter;
      });

      let lastNode: AudioNode = source;
      for (const f of filters) {
        lastNode.connect(f);
        lastNode = f;
      }
      lastNode.connect(ctx.destination);
      filtersRef.current = filters;
    } catch { /* Web Audio not supported */ }
  }, [audioRef, isIOS]);

  // Apply EQ preset when it changes
  useEffect(() => {
    if (eqPreset !== "Flat" && !audioCtxRef.current && !isIOS) {
      initEq();
    }
    const gains = EQ_PRESETS[eqPreset] ?? EQ_PRESETS["Flat"] ?? [];
    filtersRef.current.forEach((f, i) => {
      f.gain.value = gains[i] !== undefined ? gains[i] : 0;
    });
    try { localStorage.setItem(EQ_STORAGE_KEY, eqPreset); } catch { /* ignore */ }
  }, [eqPreset, initEq, isIOS]);

  // Load playlists when dropdown opens
  const loadPlaylists = useCallback(async () => {
    try {
      const res = await api.getPlaylists();
      setPlaylists(res.playlists);
    } catch { /* ignore */ }
  }, []);

  const isFav = currentTrack
    ? favourites.some((f) => f.info_hash === currentTrack.streamId && f.title === currentTrack.title)
    : false;

  const toggleFav = () => {
    if (!currentTrack) return;
    if (isFav) {
      const fav = favourites.find((f) => f.info_hash === currentTrack.streamId && f.title === currentTrack.title);
      if (fav) removeFavourite(fav.id);
    } else {
      addFavourite({
        content_type: "music",
        title: currentTrack.title,
        year: null,
        rating: null,
        info_hash: currentTrack.streamId,
        poster_url: currentTrack.artworkUrl ?? null,
        metadata_json: JSON.stringify({
          artist: currentTrack.artist,
          album: currentTrack.album,
          format: currentTrack.format,
          fileIndex: currentTrack.fileIndex,
        }),
      });
    }
  };

  const addToPlaylist = async (playlistId: string) => {
    if (!currentTrack) return;
    await api.addPlaylistTrack(playlistId, {
      info_hash: currentTrack.streamId,
      file_index: currentTrack.fileIndex,
      title: currentTrack.title,
      artist: currentTrack.artist,
      album: currentTrack.album,
      artwork_url: currentTrack.artworkUrl,
    });
  };

  const createAndAdd = async () => {
    if (!currentTrack) return;
    const name = window.prompt("Playlist name");
    if (!name?.trim()) return;
    const pl = await api.createPlaylist(name.trim());
    await addToPlaylist(pl.id);
    setPlaylists((prev) => [pl, ...prev]);
  };

  const getSliderRatio = useCallback((clientX: number) => {
    const el = sliderRef.current;
    if (!el) return 0;
    const rect = el.getBoundingClientRect();
    return Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
  }, []);

  const updateSeekVisual = useCallback((ratio: number) => {
    const bar = seekBarRef.current;
    const timeEl = seekTimeRef.current;
    const timeEndEl = seekTimeEndRef.current;
    if (bar) bar.style.width = `${ratio * 100}%`;
    if (timeEl) timeEl.textContent = formatTime(ratio * duration);
    if (timeEndEl) timeEndEl.textContent = `-${formatTime(Math.max(0, duration - ratio * duration))}`;
  }, [duration]);

  const handleSliderStart = useCallback((e: React.MouseEvent | React.TouchEvent) => {
    e.preventDefault();
    const clientX = "touches" in e ? (e.touches[0]?.clientX ?? 0) : e.clientX;
    const ratio = getSliderRatio(clientX);
    seekRatioRef.current = ratio;
    setIsSeeking(true);
    pauseTimeUpdates();
    updateSeekVisual(ratio);

    const onMove = (ev: MouseEvent | TouchEvent) => {
      ev.preventDefault();
      const cx = "touches" in ev ? (ev.touches[0]?.clientX ?? 0) : (ev as MouseEvent).clientX;
      seekRatioRef.current = getSliderRatio(cx);
      updateSeekVisual(seekRatioRef.current);
    };
    const onEnd = (ev: MouseEvent | TouchEvent) => {
      const cx = "changedTouches" in ev ? (ev.changedTouches[0]?.clientX ?? 0) : (ev as MouseEvent).clientX;
      seek(getSliderRatio(cx) * duration);
      setIsSeeking(false);
      resumeTimeUpdates();
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onEnd);
      document.removeEventListener("touchmove", onMove);
      document.removeEventListener("touchend", onEnd);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onEnd);
    document.addEventListener("touchmove", onMove, { passive: false });
    document.addEventListener("touchend", onEnd);
  }, [getSliderRatio, seek, duration, updateSeekVisual]);

  const handleAirPlay = useCallback(() => {
    const audio = audioRef.current as HTMLAudioElement & {
      webkitShowPlaybackTargetPicker?: () => void;
    } | null;
    if (audio?.webkitShowPlaybackTargetPicker) {
      audio.webkitShowPlaybackTargetPicker();
    }
  }, [audioRef]);

  const handleShare = async () => {
    if (!currentTrack) return;
    setShareLoading(true);
    try {
      const result = await api.createShareLink(currentTrack.streamId);
      const fi = currentTrack.fileIndex ?? 0;
      const fullUrl = `${window.location.origin}/music/play/${currentTrack.streamId}/${fi}?guest=${result.token}`;

      if (navigator.share) {
        try {
          await navigator.share({ title: currentTrack.title, url: fullUrl });
          setShareCopied(true);
          setTimeout(() => setShareCopied(false), 3000);
          return;
        } catch { /* cancelled */ }
      }

      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(fullUrl).catch(() => {});
      }
      setShareCopied(true);
      setTimeout(() => setShareCopied(false), 3000);
    } catch { /* ignore */ }
    finally { setShareLoading(false); }
  };

  const progress = duration > 0 ? (currentTime / duration) * 100 : 0;
  const hasNext = queueIndex >= 0 && queueIndex < queue.length - 1;

  // Swipe down to close - GPU composited, rAF batched, scroll-aware
  const dragBlocked = useRef(false);
  const rafId = useRef(0);

  useEffect(() => {
    const el = panelRef.current;
    if (!el || !open) return;

    // Promote to GPU layer
    el.style.willChange = "transform";

    const onStart = (e: TouchEvent) => {
      const tag = (e.target as HTMLElement).closest("button, [role='slider'], select, [data-radix-collection-item], [data-no-drag], svg, input, a, [data-slider]");
      if (tag) { dragBlocked.current = true; return; }
      // Only allow drag when scrolled to top
      if (el.scrollTop > 5) { dragBlocked.current = true; return; }
      dragBlocked.current = false;
      touchStartY.current = e.touches[0]?.clientY ?? 0;
      isDragging.current = false;
      dragOffsetRef.current = 0;
    };

    const onMove = (e: TouchEvent) => {
      if (dragBlocked.current) return;
      const delta = (e.touches[0]?.clientY ?? 0) - touchStartY.current;
      if (delta > 10) {
        if (!isDragging.current) {
          isDragging.current = true;
          pauseTimeUpdates();
          el.style.overflow = "hidden";
          el.style.transition = "none";
        }
        e.preventDefault();
        dragOffsetRef.current = delta - 10;
        cancelAnimationFrame(rafId.current);
        rafId.current = requestAnimationFrame(() => {
          el.style.transform = `translateY(${dragOffsetRef.current}px)`;
        });
      }
    };

    const onEnd = () => {
      if (dragBlocked.current) { dragBlocked.current = false; return; }
      cancelAnimationFrame(rafId.current);
      resumeTimeUpdates();
      if (!isDragging.current) return;
      el.style.transition = "transform 0.3s cubic-bezier(0.2, 0, 0, 1)";
      if (dragOffsetRef.current > window.innerHeight * 0.2) {
        el.style.transform = `translateY(${window.innerHeight}px)`;
        setTimeout(() => {
          onClose();
          el.style.transition = "none";
          el.style.transform = "";
          el.style.overflow = "";
        }, 300);
      } else {
        el.style.transform = "";
        requestAnimationFrame(() => { el.style.overflow = ""; });
      }
      isDragging.current = false;
      dragOffsetRef.current = 0;
    };

    el.addEventListener("touchstart", onStart, { passive: true });
    el.addEventListener("touchmove", onMove, { passive: false });
    el.addEventListener("touchend", onEnd, { passive: true });

    return () => {
      cancelAnimationFrame(rafId.current);
      el.removeEventListener("touchstart", onStart);
      el.removeEventListener("touchmove", onMove);
      el.removeEventListener("touchend", onEnd);
      el.style.willChange = "";
    };
  }, [open, onClose]);

  if (!open || !currentTrack) return null;

  return (
    <div
      ref={panelRef}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 200,
        display: "flex",
        flexDirection: "column",
        background: "#0a0a0a",
        overflowY: "auto",
        overflowX: "hidden",
        overscrollBehavior: "contain",
        touchAction: "pan-y",
      }}
    >
      {/* Blurred background artwork */}
      <img
        src={imgSrc}
        alt=""
        onError={() => setImgSrc(DEFAULT_VIDEO_POSTER_URL)}
        style={{
          position: "fixed",
          inset: "-20%",
          width: "140%",
          height: "140%",
          objectFit: "cover",
          filter: "blur(60px) brightness(0.2) saturate(1.5)",
          zIndex: -1,
        }}
      />

      {/* Top bar */}
      <Flex align="center" justify="between" px="4" py="3">
        <div onClick={onClose} style={{ cursor: "pointer", padding: 4 }}>
          <ChevronDownIcon width={24} height={24} />
        </div>
        <Text size="1" color="gray">
          {queueIndex >= 0 ? `${queueIndex + 1} / ${queue.length}` : ""}
        </Text>
        <div style={{ width: 32 }} />
      </Flex>

      {updateAvailable && (
        <Flex
          align="center"
          justify="center"
          gap="3"
          py="2"
          onClick={reload}
          style={{ background: "var(--accent-9)", cursor: "pointer", flexShrink: 0 }}
        >
          <Text size="2" weight="medium" style={{ color: "white" }}>New version available</Text>
          <Text size="1" style={{ color: "rgba(255,255,255,0.8)", textDecoration: "underline" }}>Refresh</Text>
        </Flex>
      )}

      {/* Center: artwork + info */}
      <Flex direction="column" align="center" gap="4" px="6" style={{ flex: 1, justifyContent: "center" }}>
        <div style={{
          width: "min(280px, 70vw)",
          height: "min(280px, 70vw)",
          borderRadius: 12,
          overflow: "hidden",
          boxShadow: "0 8px 32px rgba(0,0,0,0.5)",
        }}>
          <img
            src={imgSrc}
            alt=""
            onError={() => setImgSrc(DEFAULT_VIDEO_POSTER_URL)}
            style={{
              width: "100%",
              height: "100%",
              objectFit: "cover",
              animation: "albumBreath 16s ease-in-out 0s infinite alternate",
            }}
          />
        </div>

        <Flex direction="column" align="center" gap="1" style={{ maxWidth: "85vw", width: "100%" }}>
          <Text size="5" weight="bold">
            <MarqueeText text={currentTrack.title} />
          </Text>
          {currentTrack.artist && (
            <Text size="3" color="gray">
              <MarqueeText text={currentTrack.artist} />
            </Text>
          )}
          {currentTrack.album && (
            <Text size="1" color="gray">
              <MarqueeText text={currentTrack.album} />
            </Text>
          )}
          <Flex gap="2" mt="1">
            {currentTrack.format && (
              <Badge size="1" variant="soft" color="blue">{currentTrack.format}</Badge>
            )}
            {currentTrack.fileSize && (
              <Badge size="1" variant="soft" color="gray">{formatBytes(currentTrack.fileSize)}</Badge>
            )}
          </Flex>
        </Flex>
      </Flex>

      {/* Progress bar - draggable */}
      <Flex direction="column" gap="1" px="6" pb="2">
        <div
          ref={sliderRef}
          data-slider="true"
          onMouseDown={handleSliderStart}
          onTouchStart={handleSliderStart}
          style={{
            height: 24,
            display: "flex",
            alignItems: "center",
            cursor: "pointer",
            touchAction: "none",
          }}
        >
          <div style={{
            height: 4,
            width: "100%",
            background: "rgba(255,255,255,0.15)",
            borderRadius: 2,
            position: "relative",
          }}>
            <div
              ref={seekBarRef}
              style={{
                height: "100%",
                width: isSeeking ? undefined : `${progress}%`,
                background: "var(--accent-9)",
                borderRadius: 2,
                transition: isSeeking ? "none" : "width 0.3s linear",
                position: "relative",
              }}
            >
              <div style={{
                position: "absolute",
                right: -8,
                top: -6,
                width: 16,
                height: 16,
                borderRadius: "50%",
                background: "white",
                boxShadow: "0 1px 4px rgba(0,0,0,0.3)",
              }} />
            </div>
          </div>
        </div>
        <Flex justify="between">
          <Text size="1" color="gray"><span ref={seekTimeRef}>{isSeeking ? "" : formatTime(currentTime)}</span></Text>
          <Text size="1" color="gray"><span ref={seekTimeEndRef}>{isSeeking ? "" : `-${formatTime(Math.max(0, duration - currentTime))}`}</span></Text>
        </Flex>
      </Flex>

      {/* Playback controls */}
      <Flex align="center" justify="center" gap="5" py="3" data-no-drag>
        <div
          onClick={toggleRepeat}
          style={{
            cursor: "pointer",
            padding: 8,
            opacity: repeat === "none" ? 0.4 : 1,
            color: repeat !== "none" ? "var(--accent-9)" : undefined,
            position: "relative",
          }}
        >
          <LoopIcon width={20} height={20} />
          {repeat === "one" && (
            <span style={{ position: "absolute", top: 2, right: 2, fontSize: 9, fontWeight: 700 }}>1</span>
          )}
        </div>
        <div onClick={previous} style={{ cursor: "pointer", padding: 8 }}>
          <TrackPreviousIcon width={28} height={28} />
        </div>
        <div
          onClick={isPlaying ? pause : resume}
          style={{
            cursor: "pointer",
            width: 64,
            height: 64,
            borderRadius: "50%",
            background: "white",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          {isPlaying ? (
            <PauseIcon width={32} height={32} color="black" />
          ) : (
            <PlayIcon width={32} height={32} color="black" />
          )}
        </div>
        <div
          onClick={next}
          style={{ cursor: "pointer", padding: 8, opacity: hasNext ? 1 : 0.3 }}
        >
          <TrackNextIcon width={28} height={28} />
        </div>
        <div onClick={isGuest ? undefined : toggleFav} style={{ cursor: isGuest ? "default" : "pointer", padding: 8, opacity: isGuest ? 0.3 : 1 }}>
          {isFav ? (
            <StarFilledIcon width={20} height={20} color="var(--yellow-9)" />
          ) : (
            <StarIcon width={20} height={20} />
          )}
        </div>
      </Flex>

      {/* Actions row: EQ + Playlist + Share + AirPlay */}
      <Flex align="center" justify="center" gap="3" px="6" pb="6" mt="3" wrap="wrap" data-no-drag>
        {!isIOS && (
          <Flex align="center" gap="1" style={{ padding: "4px 8px", borderRadius: 6, background: "rgba(255,255,255,0.1)" }}>
            <Text size="1">EQ</Text>
            <Select.Root value={eqPreset} onValueChange={setEqPreset}>
              <Select.Trigger variant="ghost" />
              <Select.Content>
                {Object.keys(EQ_PRESETS).map((name) => (
                  <Select.Item key={name} value={name}>{name}</Select.Item>
                ))}
              </Select.Content>
            </Select.Root>
          </Flex>
        )}

        {isGuest ? (
          <Flex align="center" gap="1" style={{ padding: "4px 8px", borderRadius: 6, background: "rgba(255,255,255,0.1)", opacity: 0.3 }}>
            <ListBulletIcon width={12} height={12} />
            <Text size="1">Playlist</Text>
          </Flex>
        ) : (
          <DropdownMenu.Root onOpenChange={(o) => { if (o) loadPlaylists(); }}>
            <DropdownMenu.Trigger>
              <Flex align="center" gap="1" style={{ cursor: "pointer", padding: "4px 8px", borderRadius: 6, background: "rgba(255,255,255,0.1)" }}>
                <ListBulletIcon width={12} height={12} />
                <Text size="1">Playlist</Text>
              </Flex>
            </DropdownMenu.Trigger>
            <DropdownMenu.Content>
              {playlists.map((pl) => (
                <DropdownMenu.Item key={pl.id} onClick={() => addToPlaylist(pl.id)}>
                  {pl.name} ({pl.track_count})
                </DropdownMenu.Item>
              ))}
              <DropdownMenu.Separator />
              <DropdownMenu.Item onClick={createAndAdd}>
                <PlusCircledIcon width={14} height={14} />
                New Playlist
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        )}

        <Flex
          align="center"
          gap="1"
          onClick={handleShare}
          style={{
            cursor: shareLoading ? "wait" : "pointer",
            padding: "4px 8px",
            borderRadius: 6,
            background: "rgba(255,255,255,0.1)",
            opacity: shareLoading ? 0.5 : 1,
          }}
        >
          {shareCopied ? (
            <><CheckIcon width={12} height={12} /> <Text size="1">Copied</Text></>
          ) : (
            <><Share1Icon width={12} height={12} /> <Text size="1">Share</Text></>
          )}
        </Flex>

        {airplayAvailable && (
          <Flex
            align="center"
            gap="1"
            onClick={handleAirPlay}
            style={{
              cursor: "pointer",
              padding: "4px 8px",
              borderRadius: 6,
              background: airplayActive ? "var(--accent-a4)" : "rgba(255,255,255,0.1)",
              color: airplayActive ? "var(--accent-9)" : undefined,
            }}
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M5 17H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-1" />
              <polygon points="12 15 17 21 7 21 12 15" />
            </svg>
            <Text size="1">AirPlay</Text>
          </Flex>
        )}
      </Flex>
    </div>
  );
}
