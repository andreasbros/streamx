import { useCallback, useEffect, useRef, useState } from "react";
import { NotWebBadge } from "../components/NotWebBadge";
import { useNavigate, useLocation } from "react-router-dom";
import {
  Box,
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
  DrawingPinIcon,
  PlayIcon,
  TrashIcon,
  VideoIcon,
} from "@radix-ui/react-icons";
import { TrailerModal } from "../components/TrailerModal";
import { FavouriteButton } from "../components/FavouriteButton";
import { api } from "../api/client";
import { useAuth } from "../hooks/useAuth";
import { useServerSettings } from "../hooks/useServerSettings";
import { displayNameFromMagnet, formatBytes, formatRuntime, infoHashFromMagnet } from "../lib/utils";
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

  // Save the file to the user's device: make sure the stream exists
  // server-side, then hand the browser a same-origin file URL with an
  // attachment hint. The download runs at torrent speed while the
  // server fetches pieces sequentially.
  const handleClientDownload = async (variant: SearchResult) => {
    const hash = infoHashFromMagnet(variant.magnet);
    setBusyHash(hash);
    try {
      const res = await api.startStream({
        magnet_uri: variant.magnet,
        title: group.title,
      });
      const token = localStorage.getItem("streamx_token") || "";
      // Save under the movie's name, keeping the real file's extension.
      const movieName = `${group.title}`.replace(/[\\/:*?"<>|]/g, "-").trim();
      let fileName = movieName;
      let filePart = "file";
      try {
        const { files } = await api.getStreamFiles(res.stream_id);
        const pick =
          files.filter((f) => f.is_video).sort((a, b) => b.size - a.size)[0] ?? files[0];
        if (pick) {
          filePart = `file/${pick.index}`;
          const ext = pick.path.includes(".") ? pick.path.slice(pick.path.lastIndexOf(".")) : "";
          fileName = `${movieName}${ext}`;
        }
      } catch {
        // Metadata not ready yet; the bare /file endpoint still works.
      }
      const a = document.createElement("a");
      a.href = `/api/stream/${res.stream_id}/${filePart}?token=${encodeURIComponent(token)}`;
      a.download = fileName;
      document.body.appendChild(a);
      a.click();
      a.remove();
    } catch (err) {
      console.error("Client download failed:", err);
    } finally {
      setBusyHash(null);
    }
  };

  // Pin: a server-side background download. Create the stream with full
  // metadata, then pin it so it keeps downloading with no client
  // connected and shows on the Downloads page for later viewing.
  const handlePin = async (variant: SearchResult) => {
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
          <Flex align="baseline" gap="2" style={{ minWidth: 0 }}>
            <Text
              size="5"
              weight="bold"
              title={group.title}
              style={{
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
                minWidth: 0,
              }}
            >
              {group.title}
            </Text>
            {group.year && (
              <Text size="3" color="gray" style={{ flexShrink: 0 }}>
                ({group.year})
              </Text>
            )}
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
          // With transcode off, non-WEB releases are served as-is: the
          // player still opens and playback depends on the browser's
          // codec support. The crossed-WEB badge signals no transcode.
          const notWebCompatible =
            transcodeDisabled && variant.source_type !== "web";
          return (
            <Card
              key={variant.magnet}
              size="1"
              onClick={() => handlePlay(variant)}
              style={{ cursor: "pointer" }}
            >
              <Flex direction="column" gap="1">
                {/* Row 1: quality + release name (like a torrent site) + seeds. */}
                <Flex align="center" gap="2" style={{ minHeight: 26 }}>
                  <Badge
                    size="2"
                    variant="solid"
                    color={qualityColor(variant.quality)}
                    style={{ minWidth: 44, justifyContent: "center", flexShrink: 0 }}
                  >
                    {qualityLabel(variant.quality)}
                  </Badge>
                  <Text
                    size="1"
                    title={displayNameFromMagnet(variant.magnet) ?? undefined}
                    style={{
                      flex: 1,
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {displayNameFromMagnet(variant.magnet) || group.title}
                  </Text>
                  <Text
                    size="1"
                    color="green"
                    weight="medium"
                    style={{ flexShrink: 0, minWidth: 64, textAlign: "right" }}
                  >
                    {variant.seeds} seeds
                  </Text>
                </Flex>

                {/* Row 2: aligned spec cells + peers. */}
                <Flex align="center" gap="2" style={{ minHeight: 26 }}>
                  <div
                    style={{
                      display: "grid",
                      gridTemplateColumns: "repeat(4, minmax(0, 1fr))",
                      gap: 8,
                      flex: 1,
                      minWidth: 0,
                      alignItems: "center",
                    }}
                  >
                    {[
                      variant.source_type ? formatSourceType(variant.source_type) : null,
                      variant.video_codec || null,
                      variant.audio_channels ? `${variant.audio_channels}ch` : null,
                      variant.bit_depth ? `${variant.bit_depth}bit` : null,
                    ].map((cell, ci) => (
                      <Text
                        key={ci}
                        size="1"
                        color="gray"
                        style={{
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          textAlign: "center",
                        }}
                      >
                        {cell ?? "\u2013"}
                      </Text>
                    ))}
                  </div>
                  <Text size="1" color="gray" weight="medium" style={{ flexShrink: 0 }}>
                    {variant.size || formatBytes(variant.size_bytes)}
                  </Text>
                  <Text
                    size="1"
                    color="red"
                    style={{ flexShrink: 0, minWidth: 64, textAlign: "right" }}
                  >
                    {variant.leeches} peers
                  </Text>
                </Flex>

                {/* Row 3: play/no-web indicator + actions. */}
                <Flex align="center" gap="2" style={{ minHeight: 26 }}>
                  {notWebCompatible ? (
                    <NotWebBadge />
                  ) : (
                    <PlayIcon width={16} height={16} style={{ flexShrink: 0 }} />
                  )}
                  {hash && (
                    <Flex
                      align="center"
                      gap="2"
                      style={{ flex: 1, minWidth: 0 }}
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
                          title="Unpin: stop the server-side background download"
                        >
                          <Cross2Icon width={12} height={12} />
                          {dl ? `${dl.progress.toFixed(0)}%` : ""}
                          <Box as="span" display={{ initial: "none", sm: "inline" }}>
                            · Unpin
                          </Box>
                        </Button>
                      )}
                      {!complete && !activeBackground && (
                        <Button
                          size="1"
                          variant="soft"
                          disabled={busy}
                          onClick={() => handlePin(variant)}
                          title="Pin: download on the server and watch later from any device"
                        >
                          <DrawingPinIcon width={12} height={12} />
                          <Box as="span" display={{ initial: "none", sm: "inline" }}>Pin</Box>
                        </Button>
                      )}
                      {complete && (
                        <Button
                          size="1"
                          variant="soft"
                          disabled={busy}
                          onClick={() => handleClientDownload(variant)}
                          title="Download the file to this device"
                        >
                          <DownloadIcon width={12} height={12} />
                          <Box as="span" display={{ initial: "none", sm: "inline" }}>Download</Box>
                        </Button>
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
                </Flex>
              </Flex>
            </Card>
          );
        })}
      </Flex>
    </Flex>
  );
}
