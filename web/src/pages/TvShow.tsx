import { useState, useEffect, useCallback } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import {
  Flex,
  Text,
  Card,
  Badge,
  Button,
  Tabs,
  Skeleton,
} from "@radix-ui/themes";
import { ArrowLeftIcon, PlayIcon } from "@radix-ui/react-icons";
import { formatBytes } from "../lib/utils";
import { api } from "../api/client";
import type { TvSearchResultGroup, TvEpisode, TvTorrent } from "../api/types";

function qualityLabel(q: string | null): string {
  if (!q) return "?";
  const v = q.toLowerCase();
  if (v.includes("2160") || v.includes("4k")) return "4K";
  if (v.includes("1080")) return "FHD";
  if (v.includes("720")) return "HD";
  if (v.includes("480") || v.includes("360")) return "SD";
  return q;
}

function qualityColor(q: string | null): "purple" | "blue" | "green" | "orange" | "gray" {
  if (!q) return "gray";
  const v = q.toLowerCase();
  if (v.includes("2160") || v.includes("4k")) return "purple";
  if (v.includes("1080")) return "blue";
  if (v.includes("720")) return "green";
  if (v.includes("480") || v.includes("360")) return "orange";
  return "gray";
}

function parseFilenameInfo(filename: string): { codec: string | null; audio: string | null; source: string | null } {
  const lower = filename.toLowerCase();

  let codec: string | null = null;
  if (lower.includes("hevc") || lower.includes("x265") || lower.includes("h.265") || lower.includes("h265")) {
    codec = "HEVC";
  } else if (lower.includes("x264") || lower.includes("h.264") || lower.includes("h264")) {
    codec = "H.264";
  } else if (lower.includes("av1")) {
    codec = "AV1";
  } else if (lower.includes("vp9")) {
    codec = "VP9";
  }

  let audio: string | null = null;
  if (lower.includes("atmos")) {
    audio = "Atmos";
  } else if (lower.includes("dts-hd") || lower.includes("dts.hd")) {
    audio = "DTS-HD";
  } else if (lower.includes("truehd")) {
    audio = "TrueHD";
  } else if (lower.includes("dts")) {
    audio = "DTS";
  } else if (lower.includes("dd5.1") || lower.includes("dd+5.1") || lower.includes("ddp5.1") || lower.includes("eac3")) {
    audio = "DD+ 5.1";
  } else if (lower.includes("5.1")) {
    audio = "5.1";
  } else if (lower.includes("7.1")) {
    audio = "7.1";
  } else if (lower.includes("aac")) {
    audio = "AAC";
  }

  let source: string | null = null;
  if (lower.includes("bluray") || lower.includes("blu-ray") || lower.includes("bdrip") || lower.includes("brrip")) {
    source = "BluRay";
  } else if (lower.includes("web-dl") || lower.includes("webdl")) {
    source = "WEB-DL";
  } else if (lower.includes("webrip")) {
    source = "WEBRip";
  } else if (lower.includes("hdtv")) {
    source = "HDTV";
  }

  return { codec, audio, source };
}

export function TvShow() {
  const navigate = useNavigate();
  const location = useLocation();
  const group = location.state as TvSearchResultGroup | null;

  // Season numbers available for this show
  const [seasonNumbers, setSeasonNumbers] = useState<number[]>(
    () => group?.seasons.map((s) => s.season) ?? []
  );
  // Episodes loaded for the active season
  const [episodes, setEpisodes] = useState<TvEpisode[]>(
    () => group?.seasons[0]?.episodes ?? []
  );
  const [loadingSeasons, setLoadingSeasons] = useState(false);
  const [loadingEpisodes, setLoadingEpisodes] = useState(false);
  const [activeSeason, setActiveSeason] = useState<string>(
    () => String(group?.seasons[0]?.season ?? 1)
  );

  // If we came from browse (empty seasons), discover available seasons
  useEffect(() => {
    if (!group?.imdb_id || seasonNumbers.length > 0) return;
    let cancelled = false;
    setLoadingSeasons(true);
    api.getTvShowSeasons(group.imdb_id).then((res) => {
      if (cancelled) return;
      setSeasonNumbers(res.seasons);
      setLoadingSeasons(false);
      if (res.seasons.length > 0) {
        setActiveSeason(String(res.seasons[0]));
      }
    }).catch(() => {
      if (!cancelled) setLoadingSeasons(false);
    });
    return () => { cancelled = true; };
  }, [group?.imdb_id, seasonNumbers.length]);

  // Load episodes when active season changes (and we don't already have them from search)
  const loadEpisodes = useCallback((seasonNum: number) => {
    if (!group?.imdb_id) return;
    // Check if we already have episodes from the initial search data
    const existing = group.seasons.find((s) => s.season === seasonNum);
    if (existing && existing.episodes.length > 0) {
      setEpisodes(existing.episodes);
      return;
    }
    setLoadingEpisodes(true);
    setEpisodes([]);
    api.getTvShowEpisodes(group.imdb_id, seasonNum).then((res) => {
      const seasonData = res.seasons[0];
      setEpisodes(seasonData?.episodes ?? []);
      setLoadingEpisodes(false);
    }).catch(() => {
      setLoadingEpisodes(false);
    });
  }, [group]);

  useEffect(() => {
    const num = parseInt(activeSeason, 10);
    if (!isNaN(num) && seasonNumbers.includes(num)) {
      loadEpisodes(num);
    }
  }, [activeSeason, seasonNumbers, loadEpisodes]);

  if (!group) {
    return (
      <Flex direction="column" align="center" gap="4" py="9">
        <Text size="4" color="gray">Show not found</Text>
        <Button variant="soft" onClick={() => navigate("/tv")}>Go Back</Button>
      </Flex>
    );
  }

  const handlePlay = (variant: TvTorrent) => {
    const tempId = `pending-${Date.now()}`;
    navigate(`/player/${tempId}`, {
      state: {
        magnet: variant.magnet,
        meta: {
          title: group.show_name,
        },
      },
    });
  };

  const handleSeasonChange = (value: string) => {
    setActiveSeason(value);
  };

  return (
    <Flex direction="column" gap="4">
      <Button variant="ghost" size="1" onClick={() => navigate(-1)} style={{ alignSelf: "flex-start" }}>
        <ArrowLeftIcon width={18} height={18} />
      </Button>

      <Text size="5" weight="bold">
        {group.show_name}
      </Text>

      {loadingSeasons && (
        <Flex direction="column" gap="2">
          <Skeleton height="32px" width="60%" />
          <Skeleton height="14px" width="40%" />
        </Flex>
      )}

      {seasonNumbers.length > 1 && (
        <Tabs.Root value={activeSeason} onValueChange={handleSeasonChange}>
          <Tabs.List>
            {seasonNumbers.map((s) => (
              <Tabs.Trigger key={s} value={String(s)}>
                Season {s}
              </Tabs.Trigger>
            ))}
          </Tabs.List>
        </Tabs.Root>
      )}

      {loadingEpisodes && (
        <Flex direction="column" gap="2">
          {Array.from({ length: 6 }).map((_, i) => (
            <Card size="1" key={i}>
              <Flex direction="column" gap="2">
                <Skeleton height="14px" width="50%" />
                <Skeleton height="12px" width="80%" />
              </Flex>
            </Card>
          ))}
        </Flex>
      )}

      {!loadingEpisodes && episodes.length > 0 && (
        <Flex direction="column" gap="2">
          {episodes.map((ep) => (
            <Card key={ep.episode} size="1">
              <Flex direction="column" gap="2">
                <Text size="2" weight="medium">
                  Episode {ep.episode}
                  {ep.title ? ` - ${ep.title}` : ""}
                </Text>

                <Flex direction="column" gap="1">
                  {ep.variants.map((variant, idx) => {
                    const info = parseFilenameInfo(variant.filename);
                    return (
                      <Flex
                        key={idx}
                        align="center"
                        gap="2"
                        onClick={() => handlePlay(variant)}
                        style={{
                          cursor: "pointer",
                          padding: "4px 8px",
                          borderRadius: 4,
                        }}
                      >
                        <Badge size="1" variant="solid" color={qualityColor(variant.quality)}>
                          {qualityLabel(variant.quality)}
                        </Badge>
                        {info.codec && (
                          <Badge size="1" variant="soft" color="gray">{info.codec}</Badge>
                        )}
                        {info.audio && (
                          <Badge size="1" variant="soft" color="gray">{info.audio}</Badge>
                        )}
                        {info.source && (
                          <Badge size="1" variant="soft" color="gray">{info.source}</Badge>
                        )}
                        <Text size="1" color="gray" style={{ flex: 1, minWidth: 0 }}>
                          {formatBytes(variant.size_bytes)}
                        </Text>
                        <Text size="1" color="green">
                          {variant.seeds}
                        </Text>
                        <Text size="1" color="red">
                          {variant.leeches}
                        </Text>
                        <PlayIcon width={14} height={14} />
                      </Flex>
                    );
                  })}
                </Flex>
              </Flex>
            </Card>
          ))}
        </Flex>
      )}

      {!loadingSeasons && !loadingEpisodes && episodes.length === 0 && seasonNumbers.length > 0 && (
        <Flex justify="center" py="6">
          <Text size="2" color="gray">
            No episodes found for this season
          </Text>
        </Flex>
      )}

      {!loadingSeasons && seasonNumbers.length === 0 && (
        <Flex justify="center" py="6">
          <Text size="2" color="gray">
            No seasons found
          </Text>
        </Flex>
      )}
    </Flex>
  );
}
