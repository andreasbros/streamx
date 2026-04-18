import { useEffect, useState } from "react";
import { Cross2Icon } from "@radix-ui/react-icons";

interface Props {
  youtubeId?: string;
  searchQuery?: string;
  onClose: () => void;
}

export function TrailerModal({ youtubeId, searchQuery, onClose }: Props) {
  const [resolvedId, setResolvedId] = useState<string | null>(youtubeId ?? null);
  const [loading, setLoading] = useState(!youtubeId);

  useEffect(() => {
    if (youtubeId || !searchQuery) return;
    let cancelled = false;
    fetch(`/api/trailer/search?q=${encodeURIComponent(searchQuery)}`)
      .then(async (resp) => {
        if (cancelled) return;
        if (!resp.ok) {
          window.open(
            `https://www.youtube.com/results?search_query=${encodeURIComponent(searchQuery)}`,
            "_blank",
            "noopener"
          );
          onClose();
          return;
        }
        const data = await resp.json();
        if (data.youtube_id) {
          setResolvedId(data.youtube_id);
        } else {
          window.open(
            `https://www.youtube.com/results?search_query=${encodeURIComponent(searchQuery)}`,
            "_blank",
            "noopener"
          );
          onClose();
          return;
        }
        setLoading(false);
      })
      .catch(() => {
        if (!cancelled) {
          window.open(
            `https://www.youtube.com/results?search_query=${encodeURIComponent(searchQuery)}`,
            "_blank",
            "noopener"
          );
          onClose();
        }
      });
    return () => { cancelled = true; };
  }, [youtubeId, searchQuery]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div
      onClick={onClose}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 1000,
        background: "rgba(0,0,0,0.85)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: 16,
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          position: "relative",
          width: "100%",
          maxWidth: 900,
          aspectRatio: "16/9",
          borderRadius: 12,
          overflow: "hidden",
          background: "#000",
        }}
      >
        {resolvedId && (
          <iframe
            src={`https://www.youtube.com/embed/${resolvedId}?autoplay=1&rel=0&modestbranding=1`}
            title="Trailer"
            allow="autoplay; encrypted-media; fullscreen"
            allowFullScreen
            style={{ position: "absolute", inset: 0, width: "100%", height: "100%", border: "none" }}
          />
        )}
        {loading && (
          <div style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center" }}>
            <div style={{
              width: 40, height: 40,
              border: "3px solid rgba(255,255,255,0.3)",
              borderTopColor: "rgba(255,255,255,0.8)",
              borderRadius: "50%",
              animation: "spin 0.8s linear infinite",
            }} />
          </div>
        )}
        <button
          onClick={onClose}
          style={{
            position: "absolute",
            top: 8,
            right: 8,
            zIndex: 2,
            width: 32,
            height: 32,
            borderRadius: "50%",
            border: "none",
            background: "rgba(0,0,0,0.6)",
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Cross2Icon width={16} height={16} color="white" />
        </button>
      </div>
    </div>
  );
}
