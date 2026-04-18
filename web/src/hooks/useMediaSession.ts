import { useEffect, useRef } from "react";
import { debugLog } from "../lib/debug-log";

interface MediaSessionOptions {
  title: string;
  artist?: string;
  artwork?: string;
  duration?: number;
  currentTime?: number;
  playing?: boolean;
  onPlay?: () => void;
  onPause?: () => void;
  onSeekForward?: () => void;
  onSeekBackward?: () => void;
  onSeekTo?: (time: number) => void;
}

export function useMediaSession(opts: MediaSessionOptions): void {
  const cbRef = useRef(opts);
  cbRef.current = opts;
  const lastPositionUpdate = useRef(0);

  // Set metadata when title/artwork changes
  useEffect(() => {
    if (!("mediaSession" in navigator)) return;
    if (!opts.title) return;

    const artwork: MediaImage[] = [];
    if (opts.artwork) {
      const src = opts.artwork.startsWith("http")
        ? opts.artwork
        : `${window.location.origin}${opts.artwork}`;
      artwork.push({ src, sizes: "512x512", type: "image/jpeg" });
    }

    navigator.mediaSession.metadata = new MediaMetadata({
      title: opts.title,
      artist: opts.artist || "StreamX",
      album: "StreamX",
      artwork,
    });

    debugLog.info("media-session", `metadata: ${opts.title}`);
  }, [opts.title, opts.artwork, opts.artist]);

  // Register action handlers
  useEffect(() => {
    if (!("mediaSession" in navigator)) return;

    const handlers: [MediaSessionAction, MediaSessionActionHandler][] = [
      [
        "play",
        () => {
          debugLog.info("media-session", "action: play");
          cbRef.current.onPlay?.();
        },
      ],
      [
        "pause",
        () => {
          debugLog.info("media-session", "action: pause");
          cbRef.current.onPause?.();
        },
      ],
      [
        "seekforward",
        () => {
          debugLog.info("media-session", "action: seekforward");
          cbRef.current.onSeekForward?.();
        },
      ],
      [
        "seekbackward",
        () => {
          debugLog.info("media-session", "action: seekbackward");
          cbRef.current.onSeekBackward?.();
        },
      ],
      [
        "seekto",
        (details) => {
          const t = (details as MediaSessionActionDetails).seekTime;
          if (t !== undefined && t !== null) {
            debugLog.info("media-session", `action: seekto ${t.toFixed(1)}s`);
            cbRef.current.onSeekTo?.(t);
          }
        },
      ],
    ];

    for (const [action, handler] of handlers) {
      try {
        navigator.mediaSession.setActionHandler(action, handler);
      } catch {
        debugLog.debug("media-session", `action ${action} not supported`);
      }
    }

    return () => {
      for (const [action] of handlers) {
        try {
          navigator.mediaSession.setActionHandler(action, null);
        } catch {
          // ignore
        }
      }
    };
  }, []);

  // Update playback state
  useEffect(() => {
    if (!("mediaSession" in navigator)) return;
    navigator.mediaSession.playbackState = opts.playing ? "playing" : "paused";
  }, [opts.playing]);

  // Update position state (throttled to avoid thrashing)
  useEffect(() => {
    if (!("mediaSession" in navigator)) return;
    if (!navigator.mediaSession.setPositionState) return;

    const now = Date.now();
    if (now - lastPositionUpdate.current < 1000) return;

    const dur = opts.duration || 0;
    const pos = opts.currentTime || 0;
    if (!dur || !isFinite(dur) || dur <= 0) return;
    if (!isFinite(pos) || pos < 0 || pos > dur) return;

    try {
      navigator.mediaSession.setPositionState({
        duration: dur,
        position: pos,
        playbackRate: 1,
      });
      lastPositionUpdate.current = now;
    } catch {
      // Safari throws on edge cases
    }
  }, [opts.currentTime, opts.duration]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (!("mediaSession" in navigator)) return;
      debugLog.info("media-session", "cleanup");
      navigator.mediaSession.metadata = null;
      navigator.mediaSession.playbackState = "none";
    };
  }, []);
}
