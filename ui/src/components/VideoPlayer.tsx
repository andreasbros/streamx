import { useEffect, useRef, useState } from "react";
import type VideoJsPlayerType from "video.js/dist/types/player";

interface Props {
  src: string;
  type?: string;
  onTimeUpdate?: (time: number) => void;
}

function isSafari(): boolean {
  const ua = navigator.userAgent;
  return /Safari/.test(ua) && !/Chrome/.test(ua);
}

function isHlsUrl(url: string): boolean {
  return url.includes(".m3u8");
}

export function VideoPlayer({ src, type, onTimeUpdate }: Props) {
  const videoRef = useRef<HTMLDivElement>(null);
  const playerRef = useRef<VideoJsPlayerType | null>(null);
  const [initError, setInitError] = useState<string | null>(null);

  useEffect(() => {
    if (!videoRef.current) return;
    const videoEl = videoRef.current.querySelector("video");
    if (!videoEl) return;

    const safari = isSafari();
    const isHls = isHlsUrl(src);
    const isDirectFile = !isHls;

    if (isDirectFile || (safari && isHls)) {
      videoEl.src = src;
      videoEl.load();

      const onTime = () => {
        if (onTimeUpdate && videoEl.currentTime > 0) {
          onTimeUpdate(videoEl.currentTime);
        }
      };
      videoEl.addEventListener("timeupdate", onTime);

      return () => {
        videoEl.removeEventListener("timeupdate", onTime);
        videoEl.src = "";
      };
    }

    let disposed = false;

    import("video.js").then((mod) => {
      if (disposed) return;
      const videojs = mod.default;
      try {
        const player = videojs(videoEl, {
          controls: true,
          responsive: true,
          fluid: true,
          playbackRates: [0.5, 1, 1.25, 1.5, 2],
          html5: {
            vhs: {
              overrideNative: !safari,
            },
            nativeAudioTracks: safari,
            nativeVideoTracks: safari,
          },
          sources: [{
            src,
            type: type || "application/x-mpegURL",
          }],
        });

        playerRef.current = player;

        if (onTimeUpdate) {
          player.on("timeupdate", () => {
            const ct = player.currentTime();
            if (typeof ct === "number") onTimeUpdate(ct);
          });
        }

        player.on("error", () => {
          const err = player.error();
          if (err && safari && isHls) {
            player.dispose();
            playerRef.current = null;
            videoEl.src = src;
            videoEl.load();
            videoEl.play().catch(() => {});
          }
        });
      } catch (err) {
        setInitError(String(err));
      }
    }).catch((err) => {
      setInitError(`Failed to load video.js: ${err}`);
    });

    return () => {
      disposed = true;
      if (playerRef.current) {
        playerRef.current.dispose();
        playerRef.current = null;
      }
    };
  }, [src]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div>
      {initError && <div style={{ color: "red", padding: 8 }}>{initError}</div>}
      <div ref={videoRef} style={{ borderRadius: 8, overflow: "hidden" }}>
        <video
          className="video-js vjs-big-play-centered"
          playsInline
          controls
          style={{ width: "100%", height: "100%" }}
        />
      </div>
    </div>
  );
}
