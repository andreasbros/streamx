import { useState, useEffect, useCallback, useRef } from "react";
import {
  Box,
  Flex,
  Text,
  TextField,
  Card,
  Badge,
  Skeleton,
  IconButton,
  Separator,
} from "@radix-ui/themes";
import {
  MagnifyingGlassIcon,
  Cross2Icon,
  PlayIcon,
  DownloadIcon,
  StarIcon,
  StarFilledIcon,
} from "@radix-ui/react-icons";
import { useAudioPlayer } from "../hooks/useAudioPlayer";
import { useFavourites } from "../hooks/useFavourites";
import { api } from "../api/client";
import type { MusicVideoResult, TorrentFileInfo } from "../api/types";
import type { AudioTrack } from "../hooks/useAudioPlayer";

// v3: file_index is now backed by a stable server-side manifest
// (alphabetical, includes non-audio files). Caches from v1/v2 could
// hold indices computed from a disk scan that omitted files, so they
// must be dropped.
const ALBUM_CACHE_PREFIX = "streamx_album_v3_";

// One-shot housekeeping: drop stale older cache entries the first
// time this module loads in a session.
(() => {
  try {
    const stale: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k && k.startsWith("streamx_album_") && !k.startsWith(ALBUM_CACHE_PREFIX)) {
        stale.push(k);
      }
    }
    for (const k of stale) localStorage.removeItem(k);
  } catch { /* ignore */ }
})();

function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(0)} MB`;
  return `${(bytes / 1e3).toFixed(0)} KB`;
}

function trackTitleFromPath(path: string): string {
  const name = path.split("/").pop() ?? path;
  return name.replace(/\.[^.]+$/, "").replace(/^\d+[\s._-]+/, "");
}

function formatBrowseDate(iso: string): string {
  // Server hands us "YYYY-MM-DD" or "YYYY-01-01" when only year is known.
  if (/^\d{4}-01-01$/.test(iso)) return iso.slice(0, 4);
  return iso;
}

function cacheAlbumFiles(streamId: string, files: TorrentFileInfo[]) {
  try {
    localStorage.setItem(ALBUM_CACHE_PREFIX + streamId, JSON.stringify(files));
  } catch { /* quota */ }
}

function getCachedAlbumFiles(streamId: string): TorrentFileInfo[] | null {
  try {
    const cached = localStorage.getItem(ALBUM_CACHE_PREFIX + streamId);
    if (cached) return JSON.parse(cached);
  } catch { /* ignore */ }
  return null;
}

function useSearchMusic() {
  const [results, setResults] = useState<MusicVideoResult[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const search = useCallback((query: string) => {
    if (timerRef.current) clearTimeout(timerRef.current);
    if (!query.trim()) {
      setResults([]);
      setIsLoading(false);
      setError(null);
      return;
    }
    setIsLoading(true);
    setError(null);
    timerRef.current = setTimeout(async () => {
      try {
        const res = await api.searchMusic({ query: query.trim() });
        setResults(res.results);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Search failed");
        setResults([]);
      } finally {
        setIsLoading(false);
      }
    }, 400);
  }, []);

  return { results, isLoading, error, search };
}

interface AlbumState {
  streamId: string;
  files: TorrentFileInfo[];
  loading: boolean;
  title: string;
}

function AlbumTrackList({
  album,
  onPlayTrack,
  onPlayAll,
  currentTrack,
  isPlaying,
}: {
  album: AlbumState;
  onPlayTrack: (file: TorrentFileInfo, album: AlbumState) => void;
  onPlayAll: (album: AlbumState) => void;
  currentTrack: AudioTrack | null;
  isPlaying: boolean;
}) {
  const audioFiles = album.files.filter((f) => f.is_audio);

  if (album.loading) {
    return (
      <Card>
        <Flex direction="column" gap="2" p="2">
          <Flex align="center" gap="2">
            <Skeleton width="40px" height="40px" style={{ borderRadius: 6 }} />
            <Flex direction="column" gap="1" style={{ flex: 1 }}>
              <Skeleton height="14px" width="60%" />
              <Skeleton height="12px" width="30%" />
            </Flex>
          </Flex>
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} height="36px" width="100%" />
          ))}
        </Flex>
      </Card>
    );
  }

  if (audioFiles.length === 0) {
    return (
      <Card>
        <Flex align="center" gap="2" p="2">
          <Text size="2" color="gray">No audio files found in this torrent</Text>
        </Flex>
      </Card>
    );
  }

  return (
    <Card>
      <Flex direction="column" gap="1">
        <Flex align="center" justify="between" px="2" py="1">
          <Text size="2" weight="bold">{album.title}</Text>
          <Flex gap="2" align="center">
            <Badge size="1" color="gray">{audioFiles.length} tracks</Badge>
            <div
              onClick={() => onPlayAll(album)}
              style={{ cursor: "pointer", display: "flex", alignItems: "center", gap: 4 }}
            >
              <PlayIcon width={14} height={14} />
              <Text size="1" weight="medium">Play All</Text>
            </div>
          </Flex>
        </Flex>

        {audioFiles.map((file, idx) => {
          const isActive =
            currentTrack?.streamId === album.streamId &&
            currentTrack?.fileIndex === file.index;
          return (
            <Flex
              key={file.index}
              align="center"
              gap="2"
              px="2"
              py="1"
              onClick={() => onPlayTrack(file, album)}
              style={{
                cursor: "pointer",
                borderRadius: 4,
                background: isActive ? "var(--accent-a3)" : undefined,
              }}
            >
              <Text size="1" color="gray" style={{ width: 20, textAlign: "right", flexShrink: 0 }}>
                {idx + 1}
              </Text>
              {isActive && isPlaying ? (
                <span style={{ width: 14, flexShrink: 0, textAlign: "center", color: "var(--accent-9)" }}>
                  &#9654;
                </span>
              ) : (
                <PlayIcon width={14} height={14} style={{ flexShrink: 0, opacity: 0.5 }} />
              )}
              <Text
                size="2"
                weight={isActive ? "bold" : "regular"}
                style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
              >
                {trackTitleFromPath(file.path)}
              </Text>
              <Text size="1" color="gray" style={{ flexShrink: 0 }}>
                {formatBytes(file.size)}
              </Text>
            </Flex>
          );
        })}
      </Flex>
    </Card>
  );
}

export function MusicSearch() {
  const audioPlayer = useAudioPlayer();
  const { favourites, addFavourite, removeFavourite } = useFavourites();
  const { results, isLoading, error, search } = useSearchMusic();
  const [query, setQuery] = useState("");
  const [browseResults, setBrowseResults] = useState<MusicVideoResult[]>([]);
  const [browseLoading, setBrowseLoading] = useState(true);
  const [resolving, setResolving] = useState<string | null>(null);
  const [expandedAlbum, setExpandedAlbum] = useState<AlbumState | null>(null);

  const musicFavourites = favourites.filter((f) => f.content_type === "music");

  const defaultQuery = (() => {
    const now = new Date();
    const year = now.getMonth() < 2 ? now.getFullYear() - 1 : now.getFullYear();
    return `top ${year}`;
  })();

  const fetchBrowse = useCallback(async () => {
    setBrowseLoading(true);
    try {
      const res = await api.searchMusic({ query: defaultQuery });
      setBrowseResults(res.results);
    } catch { /* ignore */ }
    setBrowseLoading(false);
  }, [defaultQuery]);

  useEffect(() => { fetchBrowse(); }, [fetchBrowse]);

  // Pre-cache: fetch file lists for favourited albums in background
  useEffect(() => {
    for (const fav of musicFavourites) {
      if (!fav.info_hash) continue;
      const cached = getCachedAlbumFiles(fav.info_hash);
      if (cached) continue;
      // Fetch in background
      api.getStreamFiles(fav.info_hash).then((res) => {
        const files = (res as { files: TorrentFileInfo[] }).files;
        if (files.length > 0) cacheAlbumFiles(fav.info_hash!, files);
      }).catch(() => {});
    }
  }, [musicFavourites.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleExpand = async (item: MusicVideoResult) => {
    let magnet = item.magnet;
    if (!magnet) {
      setResolving(item.detail_url);
      try {
        const res = await api.resolveMagnet(item.detail_url, "music");
        magnet = res.magnet;
      } catch {
        setResolving(null);
        return;
      }
      setResolving(null);
    }

    const album: AlbumState = {
      streamId: "",
      files: [],
      loading: true,
      title: item.title,
    };
    setExpandedAlbum(album);

    try {
      const streamRes = await api.startMusicStream(magnet);
      album.streamId = streamRes.stream_id;

      // Check localStorage cache first - instant expand
      const cached = getCachedAlbumFiles(streamRes.stream_id);
      if (cached && cached.filter((f) => f.is_audio).length > 0) {
        setExpandedAlbum({ streamId: streamRes.stream_id, files: cached, loading: false, title: item.title });
        return;
      }

      setExpandedAlbum({ ...album });

      // Poll for files
      for (let attempt = 0; attempt < 30; attempt++) {
        await new Promise((r) => setTimeout(r, 1000));
        try {
          const filesRes = await api.getStreamFiles(streamRes.stream_id) as { files: TorrentFileInfo[]; status?: string };
          if (filesRes.status === "error") {
            setExpandedAlbum({ streamId: streamRes.stream_id, files: [], loading: false, title: item.title + " (failed to connect)" });
            return;
          }
          const audioFiles = filesRes.files.filter((f) => f.is_audio);
          if (audioFiles.length > 0) {
            cacheAlbumFiles(streamRes.stream_id, filesRes.files);
            setExpandedAlbum({ streamId: streamRes.stream_id, files: filesRes.files, loading: false, title: item.title });
            return;
          }
          if (filesRes.files.length > 0 && attempt > 5) {
            cacheAlbumFiles(streamRes.stream_id, filesRes.files);
            setExpandedAlbum({ streamId: streamRes.stream_id, files: filesRes.files, loading: false, title: item.title });
            return;
          }
        } catch { /* metadata not ready */ }
      }

      setExpandedAlbum({ streamId: streamRes.stream_id, files: [], loading: false, title: item.title + " (timed out)" });
    } catch {
      setExpandedAlbum(null);
    }
  };

  const handleExpandFav = async (fav: { info_hash: string; title: string }) => {
    // Check cache first
    const cached = getCachedAlbumFiles(fav.info_hash);
    if (cached && cached.filter((f) => f.is_audio).length > 0) {
      setExpandedAlbum({ streamId: fav.info_hash, files: cached, loading: false, title: fav.title });
      return;
    }

    // Not cached - try fetching
    setExpandedAlbum({ streamId: fav.info_hash, files: [], loading: true, title: fav.title });
    try {
      const filesRes = await api.getStreamFiles(fav.info_hash) as { files: TorrentFileInfo[]; status?: string };
      const files = filesRes.files;
      if (files.length > 0) cacheAlbumFiles(fav.info_hash, files);
      setExpandedAlbum({ streamId: fav.info_hash, files, loading: false, title: fav.title });
    } catch {
      setExpandedAlbum({ streamId: fav.info_hash, files: [], loading: false, title: fav.title + " (not available)" });
    }
  };

  const buildTracks = (album: AlbumState): AudioTrack[] => {
    const audioFiles = album.files.filter((f) => f.is_audio);
    return audioFiles.map((f) => {
      const ext = f.path.split(".").pop()?.toUpperCase() ?? "";
      return {
        title: trackTitleFromPath(f.path),
        album: album.title,
        streamId: album.streamId,
        fileIndex: f.index,
        artworkUrl: api.getArtworkUrl(album.streamId, f.index),
        format: ext,
        fileSize: f.size,
      };
    });
  };

  const handlePlayTrack = (file: TorrentFileInfo, album: AlbumState) => {
    const tracks = buildTracks(album);
    const startIdx = tracks.findIndex((t) => t.fileIndex === file.index);
    audioPlayer.playQueue(tracks, startIdx >= 0 ? startIdx : 0);
  };

  const handlePlayAll = (album: AlbumState) => {
    audioPlayer.playQueue(buildTracks(album), 0);
  };

  const handleInputChange = (value: string) => {
    setQuery(value);
    search(value);
  };

  const handleClear = () => {
    setQuery("");
    search("");
    setExpandedAlbum(null);
  };

  // Browse view: sort newest-first by date when available.
  const sortedBrowse = browseResults.slice().sort((a, b) => {
    const da = a.date ?? "";
    const db = b.date ?? "";
    if (da && db) return db.localeCompare(da);
    if (db) return 1;
    if (da) return -1;
    return 0;
  });

  const displayResults = query ? results : sortedBrowse;
  const displayLoading = query ? isLoading : browseLoading;

  const renderAlbumCard = (item: MusicVideoResult, i: number) => {
    const isFav = favourites.some((f) => f.content_type === "music" && f.title === item.title);
    return (
      <Card
        key={`${item.title}-${i}`}
        size="1"
        onClick={() => handleExpand(item)}
        style={{
          cursor: "pointer",
          outline: expandedAlbum?.title === item.title ? "2px solid var(--accent-9)" : undefined,
        }}
      >
        <Flex align="center" gap="3">
          <Flex
            align="center"
            justify="center"
            style={{ width: 40, height: 40, borderRadius: 6, background: "var(--accent-a3)", flexShrink: 0 }}
          >
            <DownloadIcon width={16} height={16} />
          </Flex>
          <Flex direction="column" gap="0" style={{ flex: 1, minWidth: 0 }}>
            <Text size="2" weight="medium" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              {item.title}
            </Text>
            <Flex gap="2" align="center">
              <Text size="1" color="gray">{item.size}</Text>
              {item.date && (
                <Badge size="1" variant="soft" color="gray">{formatBrowseDate(item.date)}</Badge>
              )}
            </Flex>
          </Flex>
          <Flex direction="column" align="end" gap="0" style={{ flexShrink: 0 }}>
            <Text size="1" color="green">{item.seeds}</Text>
            <Text size="1" color="red">{item.leeches}</Text>
          </Flex>
          <div
            onClick={(e) => {
              e.stopPropagation();
              if (isFav) {
                const fav = favourites.find((f) => f.content_type === "music" && f.title === item.title);
                if (fav) removeFavourite(fav.id);
              } else {
                addFavourite({
                  content_type: "music",
                  title: item.title,
                  year: null,
                  rating: null,
                  poster_url: null,
                  info_hash: null,
                  metadata_json: JSON.stringify({ magnet: item.magnet, size: item.size, seeds: item.seeds }),
                });
              }
            }}
            style={{ cursor: "pointer", padding: 4, flexShrink: 0 }}
          >
            {isFav ? (
              <StarFilledIcon width={16} height={16} color="var(--yellow-9)" />
            ) : (
              <StarIcon width={16} height={16} style={{ opacity: 0.5 }} />
            )}
          </div>
        </Flex>
      </Card>
    );
  };

  return (
    <Flex direction="column" gap="4">
      <Text size="5" weight="bold">Music</Text>

      <Flex gap="2" align="end">
        <Box flexGrow="1">
          <TextField.Root
            size="3"
            placeholder="Search music albums..."
            value={query}
            onChange={(e) => handleInputChange(e.target.value)}
          >
            <TextField.Slot>
              <MagnifyingGlassIcon />
            </TextField.Slot>
            {query && (
              <TextField.Slot>
                <IconButton size="1" variant="ghost" onClick={handleClear}>
                  <Cross2Icon width={14} height={14} />
                </IconButton>
              </TextField.Slot>
            )}
          </TextField.Root>
        </Box>
      </Flex>

      {error && <Text size="2" color="red">{error}</Text>}
      {resolving && <Text size="2" color="gray">Resolving...</Text>}

      {expandedAlbum && (
        <AlbumTrackList
          album={expandedAlbum}
          onPlayTrack={handlePlayTrack}
          onPlayAll={handlePlayAll}
          currentTrack={audioPlayer.currentTrack}
          isPlaying={audioPlayer.isPlaying}
        />
      )}

      {/* Favourited albums - shown when not searching */}
      {!query && musicFavourites.length > 0 && (
        <>
          <Text size="3" weight="bold">
            <StarFilledIcon width={14} height={14} color="var(--yellow-9)" style={{ verticalAlign: "middle", marginRight: 4 }} />
            Favourites
          </Text>
          <Flex direction="column" gap="2">
            {musicFavourites.map((fav) => {
              const meta = fav.metadata_json ? JSON.parse(fav.metadata_json) : {};
              const hasCached = fav.info_hash ? getCachedAlbumFiles(fav.info_hash) !== null : false;
              return (
                <Card
                  key={fav.id}
                  size="1"
                  onClick={() => {
                    if (fav.info_hash) {
                      handleExpandFav({ info_hash: fav.info_hash, title: fav.title });
                    } else if (meta.magnet) {
                      handleExpand({ title: fav.title, magnet: meta.magnet, seeds: meta.seeds ?? 0, leeches: 0, size: meta.size ?? "", detail_url: "" });
                    }
                  }}
                  style={{
                    cursor: "pointer",
                    outline: expandedAlbum?.title === fav.title ? "2px solid var(--accent-9)" : undefined,
                  }}
                >
                  <Flex align="center" gap="3">
                    <Flex
                      align="center"
                      justify="center"
                      style={{ width: 40, height: 40, borderRadius: 6, background: "var(--yellow-a3)", flexShrink: 0 }}
                    >
                      <StarFilledIcon width={16} height={16} color="var(--yellow-9)" />
                    </Flex>
                    <Flex direction="column" gap="0" style={{ flex: 1, minWidth: 0 }}>
                      <Text size="2" weight="medium" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                        {fav.title}
                      </Text>
                      <Flex gap="1" align="center">
                        {meta.size && <Text size="1" color="gray">{meta.size}</Text>}
                        {hasCached && <Badge size="1" variant="soft" color="green">cached</Badge>}
                      </Flex>
                    </Flex>
                    <div
                      onClick={(e) => { e.stopPropagation(); removeFavourite(fav.id); }}
                      style={{ cursor: "pointer", padding: 4, flexShrink: 0 }}
                    >
                      <StarFilledIcon width={16} height={16} color="var(--yellow-9)" />
                    </div>
                  </Flex>
                </Card>
              );
            })}
          </Flex>
          <Separator size="4" />
        </>
      )}

      {!query && <Text size="3" weight="bold">Top {defaultQuery.split(" ")[1]}</Text>}

      {displayLoading ? (
        <Flex direction="column" gap="2">
          {Array.from({ length: 8 }).map((_, i) => (
            <Card size="1" key={i}>
              <Flex gap="3" align="center">
                <Skeleton width="40px" height="40px" style={{ borderRadius: 6 }} />
                <Flex direction="column" gap="1" style={{ flex: 1 }}>
                  <Skeleton height="14px" width="70%" />
                  <Skeleton height="12px" width="30%" />
                </Flex>
              </Flex>
            </Card>
          ))}
        </Flex>
      ) : displayResults.length === 0 ? (
        <Flex justify="center" py="6">
          <Text size="2" color="gray">No results found</Text>
        </Flex>
      ) : (
        <Flex direction="column" gap="2">
          {displayResults.map((item, i) => renderAlbumCard(item, i))}
        </Flex>
      )}
    </Flex>
  );
}
