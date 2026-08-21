import { useEffect, useRef, useState, useImperativeHandle, forwardRef } from "react";
import type VideoJsPlayerType from "video.js/dist/types/player";
import { debugLog } from "../lib/debug-log";

export interface QualityLevel {
  height: number;
  enabled: boolean;
}

export interface VideoPlayerHandle {
  play: () => boolean;
  pause: () => void;
  seek: (time: number) => void;
  getQualityLevels: () => QualityLevel[];
  setQualityLevel: (height: number | "auto") => void;
}

interface Props {
  src: string;
  type?: string;
  durationSeconds?: number;
  onTimeUpdate?: (time: number) => void;
  onBufferInfo?: (info: {
    bufferedSeconds: number;
    currentTime: number;
    duration: number;
    readyState: number;
    playing: boolean;
    videoHeight: number;
    currentSrc: string;
  }) => void;
  onPlayError?: (error: string) => void;
  onServerError?: (recovering: boolean) => void;
}

function isSafari(): boolean {
  return /Safari/.test(navigator.userAgent) && !/Chrome/.test(navigator.userAgent);
}

export const VideoPlayer = forwardRef<VideoPlayerHandle, Props>(
  function VideoPlayer({ src, type, durationSeconds, onTimeUpdate, onBufferInfo, onPlayError, onServerError }, ref) {
    const containerRef = useRef<HTMLDivElement>(null);
    const playerRef = useRef<VideoJsPlayerType | null>(null);
    const cbRef = useRef({ onTimeUpdate, onBufferInfo, onPlayError, onServerError });
    cbRef.current = { onTimeUpdate, onBufferInfo, onPlayError, onServerError };
    const [initError, setInitError] = useState<string | null>(null);

    useImperativeHandle(ref, () => ({
      play(): boolean {
        const p = playerRef.current;
        if (!p) return false;
        const promise = p.play();
        if (promise && typeof promise.catch === "function") {
          promise.catch((err: unknown) => {
            const msg = String(err);
            debugLog.error("player", `play() rejected: ${msg}`);
            if (msg.includes("NotSupportedError") || msg.includes("not supported")) {
              cbRef.current.onPlayError?.("not_supported");
            }
          });
        }
        return true;
      },
      pause() {
        const videoEl = containerRef.current?.querySelector("video");
        if (videoEl) videoEl.pause();
      },
      seek(time: number) {
        const videoEl = containerRef.current?.querySelector("video");
        if (videoEl) videoEl.currentTime = time;
      },
      getQualityLevels(): QualityLevel[] {
        try {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          const p = playerRef.current as any;
          const reps = p?.tech?.({ IWillNotUseThisInPlugins: true })?.vhs?.representations?.();
          if (!reps || !Array.isArray(reps)) return [];
          const levels: QualityLevel[] = [];
          const seen = new Set<number>();
          for (const r of reps) {
            const h = r.height;
            if (h && !seen.has(h)) {
              seen.add(h);
              levels.push({ height: h, enabled: r.enabled() });
            }
          }
          levels.sort((a, b) => b.height - a.height);
          return levels;
        } catch { return []; }
      },
      setQualityLevel(height: number | "auto") {
        try {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          const p = playerRef.current as any;
          const reps = p?.tech?.({ IWillNotUseThisInPlugins: true })?.vhs?.representations?.();
          if (!reps || !Array.isArray(reps)) return;
          for (const r of reps) {
            r.enabled(height === "auto" || r.height === height);
          }
          debugLog.info("vjs", `Quality set to ${height}`);
        } catch { /* ignore */ }
      },
    }));

    useEffect(() => {
      const videoEl = containerRef.current?.querySelector("video");
      if (!videoEl) return;

      const safari = isSafari();
      let disposed = false;
      let pollId: ReturnType<typeof setInterval> | null = null;

      const report = () => {
        if (!videoEl || document.fullscreenElement) return;
        const ct = videoEl.currentTime;
        // Use metadata duration if available (HLS playlist only reports transcoded portion)
        const rawDur = videoEl.duration || 0;
        const dur = (durationSeconds && durationSeconds > 0 && (rawDur < durationSeconds * 0.9 || !isFinite(rawDur)))
          ? durationSeconds : rawDur;
        let ahead = 0;
        for (let i = 0; i < videoEl.buffered.length; i++) {
          if (videoEl.buffered.start(i) <= ct && videoEl.buffered.end(i) > ct) {
            ahead = videoEl.buffered.end(i) - ct;
            break;
          }
        }
        // Get current HLS variant playlist + active segment from VHS
        let currentSrc = videoEl.currentSrc || "";
        try {
          const p = playerRef.current;
          if (p) {
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            const tech = (p as any).tech?.({ IWillNotUseThisInPlugins: true });
            const vhs = tech?.vhs;
            const media = vhs?.playlists?.media?.();
            if (media?.uri) {
              let segInfo = media.uri;
              // Try to get current segment index/URI
              const segments = media.segments;
              if (segments && vhs.mediaIndex != null && segments[vhs.mediaIndex]?.uri) {
                segInfo += " | " + segments[vhs.mediaIndex].uri;
              }
              currentSrc = segInfo;
            }
          }
        } catch { /* ignore */ }
        cbRef.current.onBufferInfo?.({
          bufferedSeconds: ahead,
          currentTime: ct,
          duration: dur,
          readyState: videoEl.readyState,
          playing: !videoEl.paused && !videoEl.ended && videoEl.readyState > 2,
          videoHeight: videoEl.videoHeight || 0,
          currentSrc,
        });
      };

      const isHls = src.includes(".m3u8");

      // HLS streams: use hls.js (proven compatible with MPEG-TS segments).
      // video.js VHS has fMP4/MPEG-TS quirks that cause append failures.
      // Non-HLS (direct file): use video.js for controls, range requests, etc.
      if (isHls && !safari) {
        import("hls.js").then((hlsMod) => {
          if (disposed) return;
          const Hls = hlsMod.default;
          if (!Hls.isSupported()) {
            debugLog.error("hls", "hls.js not supported, falling back to video.js");
            loadVideoJs();
            return;
          }
          debugLog.info("hls", `src=${src.substring(0, 80)}`);
          const hls = new Hls({
            enableWorker: true,
            startPosition: 0,
            maxBufferLength: 30,
            maxMaxBufferLength: 60,
          });
          hls.loadSource(src);
          hls.attachMedia(videoEl);

          // Expose a minimal player-like API for the rest of the component
          playerRef.current = {
            play: () => { videoEl.play()?.catch(() => {}); return true; },
            currentTime: (t?: number) => { if (t !== undefined) videoEl.currentTime = t; return videoEl.currentTime; },
            duration: (d?: number) => {
              if (d !== undefined && d > 0) {
                // Can't set duration on native element, but store for UI
                return d;
              }
              return videoEl.duration;
            },
            paused: () => videoEl.paused,
            dispose: () => { hls.destroy(); },
            error: () => videoEl.error ? { code: videoEl.error.code, message: videoEl.error.message } : null,
            on: () => {},
            off: () => {},
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
          } as any;

          hls.on(Hls.Events.MANIFEST_PARSED, () => {
            debugLog.info("hls", "manifest parsed");
            videoEl.currentTime = 0;
            report();
          });
          hls.on(Hls.Events.ERROR, (_evt: string, data: { type: string; details: string; fatal: boolean }) => {
            debugLog.error("hls", `${data.type} ${data.details} fatal=${data.fatal}`);
            if (data.fatal) {
              if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
                hls.startLoad();
                cbRef.current.onServerError?.(true);
              } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
                hls.recoverMediaError();
              } else {
                cbRef.current.onPlayError?.("not_supported");
              }
            }
          });

          videoEl.addEventListener("timeupdate", () => {
            cbRef.current.onTimeUpdate?.(videoEl.currentTime);
            report();
          });
          videoEl.addEventListener("playing", () => {
            cbRef.current.onServerError?.(false);
            report();
          });
          videoEl.addEventListener("error", () => {
            const err = videoEl.error;
            if (err?.code === 4) cbRef.current.onPlayError?.("not_supported");
          });
          videoEl.addEventListener("resize", report);
          // Enable native browser controls (remove video-js class which hides them)
          videoEl.controls = true;
          videoEl.className = "";
          videoEl.style.width = "100%";
          videoEl.style.height = "100%";
          videoEl.style.objectFit = "contain";
          pollId = setInterval(report, 1000);
        });
        return;
      }

      // Safari uses native HLS (no MSE needed), non-HLS uses video.js
      function loadVideoJs() {
      import("video.js").then((mod) => {
        if (disposed) return;
        try {
          const sourceType = type || (isHls ? "application/x-mpegURL" : "video/mp4");
          debugLog.info("vjs", `src=${src.substring(0, 60)} type=${sourceType}`);

          const player = mod.default(videoEl!, {
            controls: true,
            responsive: true,
            // The parent Box is a fixed 16:9 frame with overflow hidden;
            // fluid mode adds its own aspect-ratio height and pushes the
            // control bar past the clip edge. fill sizes to the parent.
            fill: true,
            bigPlayButton: false,
            preload: "auto",
            liveui: isHls,
            playbackRates: [0.5, 1, 1.25, 1.5, 2],
            html5: {
              vhs: {
                overrideNative: !safari,
                experimentalLLHLS: false,
                handleManifestRedirects: true,
                bandwidth: 5000000,
              },
              nativeAudioTracks: safari,
              nativeVideoTracks: safari,
            },
            sources: [{ src, type: sourceType }],
          });

          playerRef.current = player;

          // HLS debug tracing
          if (isHls) {
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            const tech = () => { try { return (player as any).tech?.({ IWillNotUseThisInPlugins: true }); } catch { return null; } };
            player.on("loadstart", () => debugLog.info("vjs", "loadstart"));
            player.on("loadeddata", () => {
              const vhs = tech()?.vhs;
              const media = vhs?.playlists?.media?.();
              debugLog.info("vjs", `loadeddata playlist=${media?.uri || "?"} segments=${media?.segments?.length || 0}`);
              if (media?.segments?.[0]?.map?.uri) {
                debugLog.info("vjs", `init segment: ${media.segments[0].map.uri}`);
              }
            });
            player.on("waiting", () => debugLog.debug("vjs", `waiting ct=${player.currentTime()?.toFixed(1)}`));
            player.on("canplay", () => debugLog.info("vjs", `canplay ct=${player.currentTime()?.toFixed(1)}`));
            player.on("playing", () => debugLog.info("vjs", `playing ct=${player.currentTime()?.toFixed(1)}`));
            player.on("stalled", () => debugLog.warn("vjs", `stalled ct=${player.currentTime()?.toFixed(1)}`));
          }

          player.on("timeupdate", () => {
            const t = player.currentTime();
            if (typeof t === "number") cbRef.current.onTimeUpdate?.(t);
            report();
          });
          if (isHls) {
            player.on("loadedmetadata", () => {
              player.currentTime(0);
              if (durationSeconds && durationSeconds > 0) {
                player.duration(durationSeconds);
              }
            });
            const seekOnce = () => {
              player.currentTime(0);
              if (durationSeconds && durationSeconds > 0) {
                player.duration(durationSeconds);
              }
              player.off("canplay", seekOnce);
            };
            player.on("canplay", seekOnce);

            // VHS overwrites duration every playlist refresh (only transcoded
            // segments so far). Re-override with the known metadata duration so
            // the scrub bar shows the full movie length, not just 5 minutes.
            let settingDur = false;
            player.on("durationchange", () => {
              if (settingDur) return;
              if (durationSeconds && durationSeconds > 0) {
                const cur = player.duration();
                if (typeof cur === "number" && cur < durationSeconds * 0.9) {
                  settingDur = true;
                  player.duration(durationSeconds);
                  settingDur = false;
                }
              }
            });
          }

          // resize fires when decoded video dimensions change (first frame + ABR switches)
          videoEl?.addEventListener("resize", report);
          for (const evt of ["progress", "loadedmetadata", "loadeddata", "canplay", "waiting", "playing", "pause"]) {
            player.on(evt, report);
          }

          // Retry logic for server disconnections
          let retryCount = 0;
          let savedTime = 0;

          const attemptRecovery = () => {
            if (disposed) return;
            retryCount++;
            savedTime = player.currentTime() || savedTime;
            debugLog.warn("vjs", `Recovery attempt ${retryCount}, resuming from ${savedTime.toFixed(1)}s`);
            cbRef.current.onServerError?.(true);

            const delay = retryCount <= 5 ? 1000 : Math.min(1000 + retryCount * 400, 3000);
            setTimeout(() => {
              if (disposed) return;
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              (player as any).error(null);
              player.src({ src, type: sourceType });
              player.ready(() => {
                if (savedTime > 0) player.currentTime(savedTime);
                player.play()?.catch(() => {});
              });
            }, delay);
          };

          player.on("error", () => {
            const err = player.error();
            debugLog.error("vjs", `error: ${err?.code} ${err?.message}`);

            if (err?.code === 4) {
              cbRef.current.onPlayError?.("not_supported");
              return;
            }

            // Network error (2) - attempt recovery
            if (err?.code === 2) {
              attemptRecovery();
              return;
            }

            // Decode error (3) - likely codec incompatibility (e.g. HEVC on Chrome)
            // Retry a few times in case it's transient, then report as unsupported
            if (err?.code === 3) {
              if (retryCount < 3) {
                attemptRecovery();
              } else {
                debugLog.error("vjs", `Decode error persists after ${retryCount} retries, reporting as unsupported`);
                cbRef.current.onPlayError?.("not_supported");
              }
              return;
            }

            if (err && safari && videoEl) {
              player.dispose();
              playerRef.current = null;
              videoEl.src = src;
              videoEl.load();
            }
          });

          // Reset retry count on successful playback
          player.on("playing", () => {
            if (retryCount > 0) {
              debugLog.info("vjs", `Recovered after ${retryCount} retries`);
              cbRef.current.onServerError?.(false);
              retryCount = 0;
            }
          });

          // Also handle stall (buffered data exhausted, server might be down)
          let stallTimer: ReturnType<typeof setTimeout> | null = null;
          player.on("waiting", () => {
            if (stallTimer) clearTimeout(stallTimer);
            stallTimer = setTimeout(() => {
              if (disposed || player.paused() || !videoEl) return;
              const buffered = videoEl.buffered;
              const ct = videoEl.currentTime;
              let ahead = 0;
              for (let i = 0; i < buffered.length; i++) {
                if (buffered.start(i) <= ct && buffered.end(i) > ct) {
                  ahead = buffered.end(i) - ct;
                  break;
                }
              }
              if (ahead < 0.5) {
                debugLog.warn("vjs", "Stall detected with empty buffer, attempting recovery");
                attemptRecovery();
              }
            }, 15000);
          });
          player.on("playing", () => {
            if (stallTimer) { clearTimeout(stallTimer); stallTimer = null; }
          });

          pollId = setInterval(report, 1000);
          player.on("dispose", () => { if (pollId) clearInterval(pollId); });
        } catch (e) {
          setInitError(String(e));
        }
      }).catch((e) => setInitError(`Failed to load video.js: ${e}`));
      } // end loadVideoJs

      // For Safari HLS or non-HLS sources, use video.js
      loadVideoJs();

      return () => {
        disposed = true;
        if (pollId) clearInterval(pollId);
        videoEl.removeEventListener("resize", report);
        if (playerRef.current) { playerRef.current.dispose(); playerRef.current = null; }
      };
    }, [src]);

    return (
      <div style={{ width: "100%", height: "100%", position: "relative" }}>
        {initError && (
          <div style={{ color: "red", padding: 8, position: "absolute", top: 0, zIndex: 10 }}>
            {initError}
          </div>
        )}
        <div ref={containerRef} style={{ width: "100%", height: "100%", position: "relative" }}>
          <video
            className="video-js vjs-big-play-centered"
            playsInline
            style={{ position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "contain" }}
          />
        </div>
      </div>
    );
  }
);
