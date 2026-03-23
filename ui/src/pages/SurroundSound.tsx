import { useNavigate } from "react-router-dom";
import {
  Flex,
  Text,
  Card,
  Badge,
  Button,
  Separator,
} from "@radix-ui/themes";
import { ArrowLeftIcon, PlayIcon, GlobeIcon } from "@radix-ui/react-icons";
import { api } from "../api/client";

interface DemoTrack {
  title: string;
  format: string;
  formatColor: "purple" | "blue" | "green" | "amber" | "red" | "cyan" | "orange";
  channels: string;
  quality: string;
  size: string;
  magnet: string;
}

const DEMO_TRACKS: DemoTrack[] = [
  // --- Well-seeded Blender open movies (WebTorrent + web seeds) ---
  {
    title: "Big Buck Bunny - Sunflower (AC3 5.1 Surround, 60fps)",
    format: "AC3 5.1",
    formatColor: "blue",
    channels: "5.1",
    quality: "1080p 60fps",
    size: "~355 MB",
    magnet: "magnet:?xt=urn:btih:565DB305A27FFB321FCC7B064AFD7BD73AEDDA2B&dn=bbb_sunflower_1080p_60fps_normal.mp4&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A80%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&ws=http%3A%2F%2Fdistribution.bbb3d.renderfarming.net%2Fvideo%2Fmp4%2Fbbb_sunflower_1080p_60fps_normal.mp4",
  },
  {
    title: "Big Buck Bunny 4K UHD (FLAC 5.1, x265, 60fps)",
    format: "FLAC 5.1",
    formatColor: "orange",
    channels: "5.1",
    quality: "4K 60fps",
    size: "~616 MB",
    magnet: "magnet:?xt=urn:btih:5B8C29A1E13D409422089CF113851DEC9E2F4E97&dn=Big+Buck+Bunny+4K+UHD+HFR+60+fps+FLAC+WEBRip+2160p+X265&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A80%2Fannounce",
  },
  {
    title: "Sintel (AC3 5.1 Surround, 1024p)",
    format: "AC3 5.1",
    formatColor: "blue",
    channels: "5.1",
    quality: "1024p",
    size: "~129 MB",
    magnet: "magnet:?xt=urn:btih:6a9759bffd5c0af65319979fb7832189f4f3c35d&dn=sintel.mp4&tr=wss%3A%2F%2Ftracker.btorrent.xyz&tr=wss%3A%2F%2Ftracker.fastcast.nz&tr=wss%3A%2F%2Ftracker.openwebtorrent.com&ws=https%3A%2F%2Ffastcast.nz%2Fdownloads%2Fsintel-1024-surround.mp4&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2Fsintel-1024-surround.mp4",
  },
  // --- Channel test / speaker check videos ---
  {
    title: "5.1 Surround PCM Channel Test (individual speakers)",
    format: "PCM 5.1",
    formatColor: "green",
    channels: "5.1",
    quality: "1080p",
    size: "~100 MB",
    magnet: "magnet:?xt=urn:btih:59bd2de84ca4c56f5d158974eb01e2a260b36792&dn=Surround+Sound+Test+PCM+5.1&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/surround-sound-test-pcm-5.1/",
  },
  {
    title: "5.1 Surround PCM Demo (speaker placement test)",
    format: "PCM 5.1",
    formatColor: "green",
    channels: "5.1",
    quality: "1080p",
    size: "~80 MB",
    magnet: "magnet:?xt=urn:btih:2d9a7e23848d8a32be8f974ccfbd82223131c711&dn=5.1+Surround+Sound+Test+PCM+Demo&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/5.1-surround-sound-test-pcm-demo/",
  },
  {
    title: "DTS 5.1 Best Quality Channel Check",
    format: "DTS 5.1",
    formatColor: "blue",
    channels: "5.1",
    quality: "1080p",
    size: "~150 MB",
    magnet: "magnet:?xt=urn:btih:52b9bd8592de146ea0069edb0485af274ecdcbd7&dn=DTS+5.1+Surround+Sound+Test&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/best-5.1-surround-sound-test-by-dts/",
  },
  {
    title: "DD/DTS 5.1 & 7.1 Speaker Tests (multi-format)",
    format: "DD/DTS",
    formatColor: "blue",
    channels: "5.1/7.1",
    quality: "1080p",
    size: "~300 MB",
    magnet: "magnet:?xt=urn:btih:d41ce493e980189a4e120ec89cf37f377d1eb1d7&dn=Dolby+Digital+DTS+5.1+7.1+Tests&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/SurroundSound/",
  },
  {
    title: "5.1 Test Files Multi-Format (AAC, AC3, DTS, WAV)",
    format: "Multi 5.1",
    formatColor: "amber",
    channels: "5.1",
    quality: "Various",
    size: "~200 MB",
    magnet: "magnet:?xt=urn:btih:642b1f15b43866e67d5a8a000daf5ba6377239cc&dn=5.1+Surround+Test+Files+AAC+AC3+DTS+WAV&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/5.1SurroundSoundTestFilesVariousFormatsAACAC3MP4DTSWAV/",
  },

  // --- Archive.org surround demos (with web seeds for fast download) ---
  {
    title: "Elephant's Dream (AC3 5.1, HD)",
    format: "AC3 5.1",
    formatColor: "blue",
    channels: "5.1",
    quality: "1080p",
    size: "~700 MB",
    magnet: "magnet:?xt=urn:btih:0a636a566ef52021447ae7b8d6cd23814f3e9407&dn=Elephants+Dream&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/ElephantsDream/",
  },
  {
    title: "Dolby Atmos Unfold Demo",
    format: "Atmos",
    formatColor: "purple",
    channels: "5.1+",
    quality: "1080p",
    size: "~50 MB",
    magnet: "magnet:?xt=urn:btih:e66c28bc97b506c97b307913d78d4358229ccbbe&dn=Dolby+Atmos+Unfold&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/dolby-atmos-unfold/",
  },
  {
    title: "Dolby Atmos 5.1 Fury Test (1080p)",
    format: "AC3 5.1",
    formatColor: "blue",
    channels: "5.1",
    quality: "1080p",
    size: "~56 MB",
    magnet: "magnet:?xt=urn:btih:b84d9682fcf6c2a89ab8bf58ef0df6f40d682943&dn=Universe+Fury+Dolby+Atmos+1080p&tr=http://bt1.archive.org:6969/announce&tr=http://bt2.archive.org:6969/announce&ws=https://archive.org/download/universe-fury-dolby-atmos-1080p/",
  },
];

interface HttpTrack {
  title: string;
  format: string;
  formatColor: "purple" | "blue" | "green" | "amber" | "red" | "cyan" | "orange";
  channels: string;
  quality: string;
  url: string;
  needsTranscode: boolean;
}

const HTTP_TRACKS: HttpTrack[] = [
  {
    title: "Dolby Atmos - Leaf (Official Demo)",
    format: "Atmos",
    formatColor: "purple",
    channels: "7.1.4",
    quality: "4K HDR",
    url: "https://archive.org/download/dolby-atmos-5.1-surround-sound-test/Dolby%20Atmos%20Surround%20Sound%20Test.mp4",
    needsTranscode: false,
  },
  {
    title: "DTS 5.1 Channel Check (Archive.org)",
    format: "DTS 5.1",
    formatColor: "blue",
    channels: "5.1",
    quality: "1080p",
    url: "https://archive.org/download/best-5.1-surround-sound-test-by-dts/Best%205.1%20Surround%20Sound%20Test%20By%20DTS.mp4",
    needsTranscode: false,
  },
  {
    title: "Surround Sound PCM 5.1 Test",
    format: "PCM 5.1",
    formatColor: "green",
    channels: "5.1",
    quality: "1080p",
    url: "https://archive.org/download/surround-sound-test-pcm-5.1/Surround%20Sound%20Test%20PCM%205.1.mp4",
    needsTranscode: false,
  },
  {
    title: "Big Buck Bunny (Surround, MP4)",
    format: "AAC 5.1",
    formatColor: "amber",
    channels: "5.1",
    quality: "1080p",
    url: "https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/BigBuckBunny.mp4",
    needsTranscode: false,
  },
  {
    title: "Tears of Steel (Surround, MKV)",
    format: "AC3 5.1",
    formatColor: "blue",
    channels: "5.1",
    quality: "4K",
    url: "https://archive.org/download/tears-of-steel-4k/Tears%20of%20Steel%204K.mkv",
    needsTranscode: true,
  },
  {
    title: "Sintel (Surround, MKV)",
    format: "FLAC 5.1",
    formatColor: "orange",
    channels: "5.1",
    quality: "4K",
    url: "https://archive.org/download/sintel-4k/Sintel%204K.mkv",
    needsTranscode: true,
  },
];

export function SurroundSound() {
  const navigate = useNavigate();

  const handlePlay = (track: DemoTrack) => {
    const tempId = `pending-${Date.now()}`;
    navigate(`/player/${tempId}`, {
      state: {
        magnet: track.magnet,
        meta: {
          title: track.title,
          audio_channels: track.channels,
          video_codec: track.quality,
          source_type: track.format,
        },
      },
    });
  };

  const handlePlayUrl = (track: HttpTrack) => {
    if (track.needsTranscode) {
      // Open player with HLS transcode URL
      const hlsUrl = api.getUrlPlaylistUrl(track.url, "source");
      navigate("/player/url-stream", {
        state: {
          hlsUrl,
          meta: {
            title: track.title,
            audio_channels: track.channels,
            video_codec: track.quality,
            source_type: track.format,
          },
        },
      });
    } else {
      // Direct play - browser can handle MP4
      navigate("/player/url-stream", {
        state: {
          directUrl: track.url,
          meta: {
            title: track.title,
            audio_channels: track.channels,
            video_codec: track.quality,
            source_type: track.format,
          },
        },
      });
    }
  };

  return (
    <Flex direction="column" gap="4">
      <Flex align="center" gap="3">
        <Button variant="ghost" size="1" onClick={() => navigate(-1)}>
          <ArrowLeftIcon width={18} height={18} />
        </Button>
        <Text size="5" weight="bold">Surround Sound</Text>
      </Flex>

      <Text size="2" color="gray">
        Demo and reference video content for testing surround sound setups.
      </Text>

      {/* HTTPS Direct Streams */}
      <Text size="4" weight="bold">Instant Play (HTTPS)</Text>
      <Text size="2" color="gray">
        Direct streaming from public URLs. MP4 files play instantly, MKV/FLAC files are transcoded via HLS.
      </Text>
      <Flex direction="column" gap="2">
        {HTTP_TRACKS.map((track, i) => (
          <Card key={i} size="1">
            <Flex
              align="center"
              gap="3"
              onClick={() => handlePlayUrl(track)}
              style={{ cursor: "pointer" }}
            >
              <Flex gap="2" align="center" wrap="wrap" style={{ flex: 1, minWidth: 0 }}>
                <Badge size="1" variant="solid" color={track.formatColor}>
                  {track.format}
                </Badge>
                <Badge size="1" variant="soft" color="gray">
                  {track.channels}
                </Badge>
                <Badge size="1" variant="soft" color="gray">
                  {track.quality}
                </Badge>
                {track.needsTranscode && (
                  <Badge size="1" variant="outline" color="violet">HLS</Badge>
                )}
                <Text size="2" weight="medium" style={{ minWidth: 0 }}>
                  {track.title}
                </Text>
              </Flex>
              <GlobeIcon width={16} height={16} style={{ flexShrink: 0 }} />
            </Flex>
          </Card>
        ))}
      </Flex>

      <Separator size="4" />

      {/* Torrent section */}
      <Text size="4" weight="bold">Torrent Downloads</Text>

      <Flex direction="column" gap="2">
        {DEMO_TRACKS.map((track, i) => (
          <Card key={i} size="1">
            <Flex
              align="center"
              gap="3"
              onClick={() => handlePlay(track)}
              style={{ cursor: "pointer" }}
            >
              <Flex gap="2" align="center" wrap="wrap" style={{ flex: 1, minWidth: 0 }}>
                <Badge size="1" variant="solid" color={track.formatColor}>
                  {track.format}
                </Badge>
                <Badge size="1" variant="soft" color="gray">
                  {track.channels}
                </Badge>
                <Badge size="1" variant="soft" color="gray">
                  {track.quality}
                </Badge>
                <Badge size="1" variant="outline" color="gray">
                  {track.size}
                </Badge>
                <Text size="2" weight="medium" style={{ minWidth: 0 }}>
                  {track.title}
                </Text>
              </Flex>
              <PlayIcon width={16} height={16} style={{ flexShrink: 0 }} />
            </Flex>
          </Card>
        ))}
      </Flex>
    </Flex>
  );
}
