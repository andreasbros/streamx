import { useState } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import {
  Flex,
  Text,
  Card,
  Badge,
  Button,
  Separator,
} from "@radix-ui/themes";
import { ArrowLeftIcon, PlayIcon, VideoIcon } from "@radix-ui/react-icons";
import { TrailerModal } from "../components/TrailerModal";
import { FavouriteButton } from "../components/FavouriteButton";
import { formatBytes, formatRuntime } from "../lib/utils";
import type { SearchResultGroup, SearchResult } from "../api/types";

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
  const [imgError, setImgError] = useState(false);
  const [summaryExpanded, setSummaryExpanded] = useState(false);
  const [showTrailer, setShowTrailer] = useState(false);

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

  return (
    <Flex direction="column" gap="4">
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
        {group.variants.map((variant) => (
          <Card
            key={variant.magnet}
            size="1"
            onClick={() => handlePlay(variant)}
            style={{ cursor: "pointer" }}
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

              <PlayIcon width={16} height={16} style={{ flexShrink: 0 }} />
            </Flex>
          </Card>
        ))}
      </Flex>
    </Flex>
  );
}
