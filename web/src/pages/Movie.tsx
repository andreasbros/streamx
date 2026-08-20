import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import {
  Flex,
  Text,
  Card,
  Badge,
  Button,
  IconButton,
  Separator,
} from "@radix-ui/themes";
import {
  ArrowLeftIcon,
  Cross2Icon,
  DownloadIcon,
  PlayIcon,
  TrashIcon,
  VideoIcon,
} from "@radix-ui/react-icons";
import { TrailerModal } from "../components/TrailerModal";
import { FavouriteButton } from "../components/FavouriteButton";
import { api } from "../api/client";
import { useAuth } from "../hooks/useAuth";
import { useServerSettings } from "../hooks/useServerSettings";
import { formatBytes, formatRuntime, infoHashFromMagnet } from "../lib/utils";
import type { DownloadItem, SearchResultGroup, SearchResult } from "../api/types";

function qualityLabel(q: string | undefined): string {
  if (!q) return "?";
  const v = q.toLowerCase();
  if (v.includes("2160") || v.includes("4k")) return "4K";
  if (v.includes("1080")) return "FHD";
  if (v.includes("720")) return "HD";
  if (v.includes("480") || v.includes("360")) return "SD";
  return q;
}

function qualityColor(q: string | undefined): "purple" | "blue" | "green" | "orange" | "gray" {
  if (!q) return "gray";
  const v = q.toLowerCase();
  if (v.includes("2160") || v.includes("4k")) return "purple";
  if (v.includes("1080")) return "blue";
  if (v.includes("720")) return "green";
  if (v.includes("480") || v.includes("360")) return "orange";
  return "gray";
}

function formatSourceType(src: string): string {
  if (src === "bluray") return "BluRay";
  if (src === "web") return "WEB";
  return src.toUpperCase();
}

export function Movie() {
  const navigate = useNavigate();
  const location = useLocation();
  const group = location.state as SearchResultGroup | null;
  const { user } = useAuth();
  const isAdmin = user?.is_admin === true;
  const serverSettings = useServerSettings();
  const transcodeDisabled = serverSettings?.disable_transcode ?? true;
  const [imgError, setImgError] = useState(false);
  const [summaryExpanded, setSummaryExpanded] = useState(false);
  const [showTrailer, setShowTrailer] = useState(false);
  const [downloads, setDownloads] = useState<Record<string, DownloadItem>>({});
  const [busyHash, setBusyHash] = useState<string | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Track download state for each variant by info hash so the buttons
  // reflect active/complete downloads, and keep polling while any is live.
  const refreshDownloads = useCallback(async () => {
    if (!group) return;
    const hashes = new Set(
      group.variants
        .map((v) => infoHashFromMagnet(v.magnet))
        .filter((h): h is string => h !== null)
    );
    if (hashes.size === 0) return;
    try {
      const res = await api.listDownloads();
      const map: Record<string, DownloadItem> = {};
      for (const dl of res.downloads) {
        if (hashes.has(dl.info_hash.toLowerCase())) {
          map[dl.info_hash.toLowerCase()] = dl;
        }
      }
      setDownloads(map);
    } catch {
      // Non-fatal: buttons fall back to the plain download state.
    }
  }, [group]);

  useEffect(() => {
    refreshDownloads();
    pollRef.current = setInterval(refreshDownloads, 3000);
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [refreshDownloads]);

  if (!group) {
    return (
      <Flex direction="column" align="center" gap="4" py="9">
        <Text size="4" color="gray">Movie not found</Text>
        <Button variant="soft" onClick={() => navigate("/")}>Go Home</Button>
      </Flex>
    );
  }

  const poster = group.poster_large ?? group.poster;
  const hasDirectTrailer = !!group.trailer_code;

  const handlePlay = (variant: SearchResult) => {
    const tempId = `pending-${Date.now()}`;
    navigate(`/player/${tempId}`, {
      state: {
        magnet: variant.magnet,
        poster: group.poster_large || group.poster || group.poster_medium || null,
        meta: {
          title: group.title,
          year: group.year,
          rating: group.rating,
          runtime: group.runtime,
          genres: group.genres,
          language: group.language,
          mpa_rating: group.mpa_rating,
          summary: group.summary,
          imdb_code: group.imdb_code,
          trailer_code: group.trailer_code,
          poster: group.poster,
          poster_small: group.poster_small,
          poster_medium: group.poster_medium,
          poster_large: group.poster_large,
          backdrop: group.backdrop,
          video_codec: variant.video_codec,
          audio_channels: variant.audio_channels,
          bit_depth: variant.bit_depth,
          source_type: variant.source_type,
        },
      },
    });
  };

  // Start (or resume) a background download for a variant: create the
  // stream server-side with full metadata, then pin it so it keeps
  // downloading after the user leaves.
  const handleDownload = async (variant: SearchResult) => {
    const hash = infoHashFromMagnet(variant.magnet);
    setBusyHash(hash);
    try {
      const res = await api.startStream({
        magnet_uri: variant.magnet,
        poster_url: group.poster_large || group.poster || group.poster_medium || undefined,
        title: group.title,
        year: group.year ?? undefined,
        rating: group.rating ?? undefined,
        runtime: group.runtime ?? undefined,
        genres: group.genres ?? undefined,
        language: group.language ?? undefined,
        video_codec: variant.video_codec ?? undefined,
        audio_channels: variant.audio_channels ?? undefined,
        source_type: variant.source_type ?? undefined,
        summary: group.summary ?? undefined,
        imdb_code: group.imdb_code ?? undefined,
        mpa_rating: group.mpa_rating ?? undefined,
        bit_depth: variant.bit_depth ?? undefined,
        trailer_code: group.trailer_code ?? undefined,
        poster_small: group.poster_small ?? undefined,
        poster_medium: group.poster_medium ?? undefined,
        poster_large: group.poster_large ?? undefined,
        backdrop: group.backdrop ?? undefined,
      });
      await api.pinDownload(res.stream_id);
      await refreshDownloads();
    } catch (err) {
      console.error("Download start failed:", err);
    } finally {
      setBusyHash(null);
    }
  };

  const handleCancelDownload = async (hash: string) => {
    setBusyHash(hash);
    try {
      await api.unpinDownload(hash);
      await refreshDownloads();
    } catch (err) {
      console.error("Cancel download failed:", err);
    } finally {
      setBusyHash(null);
    }
  };

  const handleDeleteDownload = async (hash: string, name: string) => {
    if (!window.confirm(`Delete "${name}" and all its files?`)) return;
    setBusyHash(hash);
    try {
      await api.deleteStream(hash);
      await refreshDownloads();
    } catch (err) {
      console.error("Delete download failed:", err);
    } finally {
      setBusyHash(null);
    }
  };

  const bgImage = group.backdrop || group.poster_large || group.poster || null;

  return (
    <Flex direction="column" gap="4">
      {bgImage && (
        <div style={{ position: "fixed", inset: 0, zIndex: -1, overflow: "hidden" }}>
          <img
            src={bgImage}
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
          <div style={{ position: "absolute", inset: 0, background: "rgba(0,0,0,0.4)" }} />
        </div>
      )}
      <Button variant="ghost" size="1" onClick={() => navigate(-1)} style={{ alignSelf: "flex-start" }}>
        <ArrowLeftIcon width={18} height={18} />
      </Button>

      {showTrailer && (
        <TrailerModal
          youtubeId={hasDirectTrailer ? group.trailer_code : undefined}
          searchQuery={hasDirectTrailer ? undefined : `${group.title} ${group.year ?? ""} official trailer`}
          onClose={() => setShowTrailer(false)}
        />
      )}

      <Flex gap="4" wrap="wrap">
        {/* Poster with trailer play overlay */}
        {poster && !imgError ? (
          <div
            style={{ position: "relative", flexShrink: 0, cursor: "pointer" }}
            onClick={() => setShowTrailer(true)}
          >
            <img
              src={poster}
              alt=""
              onError={() => setImgError(true)}
              style={{
                width: 180,
                borderRadius: 8,
                objectFit: "cover",
                background: "var(--gray-a3)",
                maxHeight: 270,
                display: "block",
              }}
            />
            <div style={{ position: "absolute", bottom: 8, right: 8, width: 36, height: 36, borderRadius: "50%", background: hasDirectTrailer ? "rgba(220,38,38,0.85)" : "rgba(120,120,120,0.7)", display: "flex", alignItems: "center", justifyContent: "center" }}>
              <VideoIcon width={18} height={18} color="white" />
            </div>
          </div>
        ) : null}

        <Flex direction="column" gap="2" style={{ flex: 1, minWidth: 200 }}>
          <Flex align="baseline" gap="2" wrap="wrap">
            <Text size="5" weight="bold">{group.title}</Text>
            {group.year && <Text size="3" color="gray">({group.year})</Text>}
            <FavouriteButton group={group} size={30} />
          </Flex>

          <Flex gap="2" align="center" wrap="wrap">
            {group.rating != null && group.rating > 0 && (
              <Badge size="2" variant="soft" color="amber">{"\u2605"} {group.rating.toFixed(1)}</Badge>
            )}
            {group.runtime != null && group.runtime > 0 && (
              <Badge size="2" variant="soft" color="gray">{formatRuntime(group.runtime)}</Badge>
            )}
            {group.mpa_rating && group.mpa_rating !== "" && (
              <Badge size="2" variant="outline">{group.mpa_rating}</Badge>
            )}
            {group.language && group.language !== "en" && (
              <Badge size="2" variant="soft">{group.language.toUpperCase()}</Badge>
            )}
          </Flex>

          {group.genres && group.genres.length > 0 && (
            <Flex gap="1" wrap="wrap">
              {group.genres.map((g) => (
                <Badge key={g} size="1" variant="soft" color="blue">{g}</Badge>
              ))}
            </Flex>
          )}

          <Button variant="soft" size="2" color={hasDirectTrailer ? "red" : "gray"} onClick={() => setShowTrailer(true)} style={{ alignSelf: "flex-start" }}>
            <VideoIcon width={14} height={14} />
            Watch Trailer
          </Button>

          {group.summary && (
            <Text
              size="2"
              color="gray"
              onClick={() => setSummaryExpanded((v) => !v)}
              style={summaryExpanded ? { cursor: "pointer" } : {
                cursor: "pointer",
                display: "-webkit-box",
                WebkitLineClamp: 3,
                WebkitBoxOrient: "vertical",
                overflow: "hidden",
              }}
            >
              {group.summary}
            </Text>
          )}
        </Flex>
      </Flex>

      <Separator size="4" />

      <Text size="3" weight="bold">Available Qualities</Text>
      <Flex direction="column" gap="2">
        {group.variants.map((variant) => {
          const hash = infoHashFromMagnet(variant.magnet);
          const dl = hash ? downloads[hash] : undefined;
          const complete = dl?.status === "complete";
          const activeBackground = !!dl && dl.pinned && !complete;
          const busy = busyHash === hash;
          // With transcode disabled, only WEB source releases are
          // playable in the browser. Others stay downloadable; a
          // double-click still attempts direct playback.
          const notWebCompatible =
            transcodeDisabled && variant.source_type !== "web";
          return (
            <Card
              key={variant.magnet}
              size="1"
              onClick={() => {
                if (!notWebCompatible) handlePlay(variant);
              }}
              onDoubleClick={() => {
                if (notWebCompatible) handlePlay(variant);
              }}
              style={{ cursor: notWebCompatible ? "default" : "pointer" }}
            >
              <Flex align="center" gap="3">
                <Badge size="2" variant="solid" color={qualityColor(variant.quality)}>
                  {qualityLabel(variant.quality)}
                </Badge>

                <Flex direction="column" gap="0" style={{ flex: 1, minWidth: 0 }}>
                  <Flex gap="2" align="center" wrap="wrap">
                    {variant.source_type && (
                      <Text size="1" color="gray">{formatSourceType(variant.source_type)}</Text>
                    )}
                    {variant.video_codec && (
                      <Text size="1" color="gray">{variant.video_codec}</Text>
                    )}
                    {variant.audio_channels && (
                      <Text size="1" color="gray">{variant.audio_channels}ch</Text>
                    )}
                    {variant.bit_depth && (
                      <Text size="1" color="gray">{variant.bit_depth}bit</Text>
                    )}
                  </Flex>
                  <Text size="2" color="gray">
                    {variant.size || formatBytes(variant.size_bytes)}
                  </Text>
                </Flex>

                <Flex direction="column" align="end" gap="0" style={{ flexShrink: 0 }}>
                  <Text size="1" color="green" weight="medium">{variant.seeds} seeds</Text>
                  <Text size="1" color="red">{variant.leeches} peers</Text>
                </Flex>

                {hash && (
                  <Flex
                    align="center"
                    gap="2"
                    style={{ flexShrink: 0 }}
                    onClick={(e) => e.stopPropagation()}
                  >
                    {complete && (
                      <Badge size="1" variant="soft" color="green">
                        Downloaded
                      </Badge>
                    )}
                    {activeBackground && (
                      <Button
                        size="1"
                        variant="soft"
                        color="orange"
                        disabled={busy}
                        onClick={() => handleCancelDownload(hash)}
                      >
                        <Cross2Icon width={12} height={12} />
                        {dl ? `${dl.progress.toFixed(0)}% · Cancel Download` : "Cancel Download"}
                      </Button>
                    )}
                    {!complete && !activeBackground && (
                      <IconButton
                        size="1"
                        variant="soft"
                        disabled={busy}
                        onClick={() => handleDownload(variant)}
                        aria-label="Download"
                        title="Download in background"
                      >
                        <DownloadIcon width={14} height={14} />
                      </IconButton>
                    )}
                    {dl && isAdmin && (
                      <IconButton
                        size="1"
                        variant="soft"
                        color="red"
                        disabled={busy}
                        onClick={() => handleDeleteDownload(hash, group.title)}
                        aria-label="Delete download"
                        title="Delete files and records"
                      >
                        <TrashIcon width={14} height={14} />
                      </IconButton>
                    )}
                  </Flex>
                )}

                {notWebCompatible ? (
                  <Badge
                    size="1"
                    variant="soft"
                    color="gray"
                    style={{ flexShrink: 0 }}
                    title="Server transcoding is disabled; double-click to try direct playback"
                  >
                    Not WEB compatible
                  </Badge>
                ) : (
                  <PlayIcon width={16} height={16} style={{ flexShrink: 0 }} />
                )}
              </Flex>
            </Card>
          );
        })}
      </Flex>
    </Flex>
  );
}
