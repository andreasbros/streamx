# StreamX

Torrent-based streaming player. Single static Rust binary serving a React UI. Search for torrents, paste magnet links, stream video in the browser.

## About

StreamX starts a web server with a modern UI where you can search for torrents, paste magnet links, and stream video content directly in the browser.

- Rust backend: Axum, librqbit (BitTorrent), FFmpeg transcoding, SQLite
- React frontend: Radix UI, hls.js, framer-motion
- Auth: bcrypt + JWT, multi-user with search/watch history
- Streaming: sequential torrent download with on-the-fly HLS transcoding

## How it works

### Provider system

All torrent sources are configured as **providers** in `config.toml`. Each provider has a `kind` (movies, tv, music, music-videos), a `url`, and a `format` that controls how queries are made.

| Format | Supports | How it works |
|---|---|---|
| `yts` | Movies (browse + search) | Queries YTS JSON API. Returns rich metadata (posters, ratings, trailers). |
| `torrentio` | Movies, TV (search only) | Resolves text queries to IMDB IDs via [Cinemeta](https://v3-cinemeta.strem.io), then fetches streams from [Torrentio](https://torrentio.strem.fun). No API key needed. |
| `apibay` | TV, Music, Music Videos (browse + search) | Queries The Pirate Bay API. |
| `eztv` | TV (browse + search) | Queries EZTV API. Returns structured season/episode data. |
| `scrape` | Music, Music Videos (browse + search) | Scrapes 1337x HTML pages. |

### Home page (Browse)

The home page shows movie categories (Latest, Popular, Top Rated, by genre). The UI calls `GET /api/search/browse` with sort/filter/page params. This requires a format with catalog support -- **YTS** for movies, **apibay/eztv** for TV.

Torrentio has no catalog (it's IMDB-ID based), so browse returns empty with `format = "torrentio"`. Keep YTS as your movies provider to populate the home page.

### Search

- **Movies** (`POST /api/search`): With YTS, queries the API directly. With Torrentio, searches Cinemeta for IMDB matches, then fetches Torrentio streams and Cinemeta detail concurrently for each result.
- **TV** (`POST /api/tv/search`): With Torrentio, searches Cinemeta for series, then probes Torrentio for episodes across seasons (up to 15 seasons x 30 episodes, fetched concurrently) to build structured season/episode results. With eztv/apibay, queries their APIs directly.
- **Music / Music Videos** (`POST /api/music/search`, `/api/music-videos/search`): Uses apibay or 1337x scraping. Torrentio does not support these.

### Streaming pipeline

```
Torrent peers --> librqbit (sequential download) --> movie.mp4
                                                        |
                                          FFmpeg (passthrough or multi-variant transcode)
                                                        |
                                     master.m3u8 (adaptive bitrate)
                                      /        |        \
                                 360p/      720p/     source/
                              playlist    playlist    playlist
                              segments    segments    segments
                                      \        |        /
                                  hls.js / Safari native HLS --> browser playback
```

1. User selects a torrent variant (quality/source)
2. Backend downloads via librqbit in sequential mode so the beginning arrives first
3. For browser-compatible formats (H.264/AAC in MP4): **direct HTTP range requests** on the torrent stream. librqbit blocks until pieces arrive, the browser buffers naturally.
4. For incompatible formats (MKV, HEVC/x265, AC3): **multi-variant HLS transcoding**. Multiple FFmpeg processes run in parallel, one per quality tier, producing a master playlist with adaptive bitrate streaming. The player picks the best tier for the connection speed.
5. Browser plays via hls.js (Chrome/Firefox) or Safari's native HLS player

### Adaptive bitrate

Transcodes produce multiple quality tiers filtered by source resolution:

| Tier   | Height | Video Bitrate | Audio Bitrate |
|--------|--------|---------------|---------------|
| 360p   | 360    | 800k          | 128k          |
| 720p   | 720    | 2500k         | 192k          |
| 1080p  | 1080   | 5000k         | 256k          |
| source | native | 8000k         | 320k          |

Tiers with height >= source are skipped (except "source" which is always included). A 480p source produces 360p + source. A 4K source produces all four tiers. hls.js handles automatic quality switching based on bandwidth -- mobile clients get 360p, desktop gets source quality.

Passthrough (browser-compatible source) skips all of this and uses a single flat playlist.

### Audio preservation

Surround audio (5.1, 7.1) is preserved through transcoding. FFmpeg keeps the original channel layout when encoding to AAC, with a safety cap at 8 channels.

### Piped transcoding

For active downloads (not yet complete), the torrent stream is written to a temp file. Once 1MB arrives, FFmpeg processes start reading from it. Each FFmpeg instance blocks on EOF when the file hasn't grown enough, providing natural backpressure that matches the download pace.

## Configuration

`~/.streamx/config.toml` (created on first run):

```toml
[server]
port = 8999
bind = "127.0.0.1"

[torrent]
max_connections = 200
sequential = true

[transcode]
video_codec = "h264"
preset = "ultrafast"
crf = 23

[auth]
session_duration = "7d"

# Movies: YTS for browse + search
[[providers]]
id = 1
kind = "movies"
url = "https://yts.bz"
api_url = "https://yts.bz/api/v2/list_movies.json"

# TV: Torrentio for structured season/episode search
[[providers]]
id = 2
kind = "tv"
url = "https://torrentio.strem.fun/providers=eztv,1337x,thepiratebay"
format = "torrentio"

# Music videos
[[providers]]
id = 3
kind = "music-videos"
url = "https://apibay.org"
format = "apibay"
category = "601"

# Music
[[providers]]
id = 4
kind = "music"
url = "https://apibay.org"
format = "apibay"
category = "101"

# Optional: route torrent traffic through SOCKS5 proxy
# [vpn]
# socks5 = "socks5://user:pass@host:port"
```

### Torrentio provider selection

The Torrentio URL path controls which upstream sources it aggregates:

```
https://torrentio.strem.fun/providers=eztv,1337x,thepiratebay
```

Available: `yts`, `eztv`, `1337x`, `thepiratebay`, `torrentgalaxy`, `nyaasi`, and others. See [torrentio.strem.fun](https://torrentio.strem.fun) for the full list.

## Local build

All tools are managed via Nix.

```bash
nix develop

# Frontend
cd ui && pnpm install && pnpm build && cd ..

# Backend (release build embeds the UI)
cd backend && cargo build --release && cd ..

# Run
./target/release/streamx
# Open http://127.0.0.1:8999
```

### Development (hot reload)

```bash
nix develop

# Terminal 1: frontend dev server
cd ui && pnpm dev

# Terminal 2: backend
cd backend && cargo run -- --port 8998
```

The vite dev server runs on port 8999 and proxies API requests to the backend on 8998.

### Checks

```bash
cargo fmt --all -- --check
cargo clippy -- -D warnings
cargo check
cd ui && pnpm typecheck
```

### CLI commands

```bash
streamx                    # Start the server (default port 8999)
streamx --port 9000        # Custom port
streamx --admin-user admin --admin-password password  # Create admin on startup
streamx clean              # Remove cache and downloads (keeps config + database)
streamx wipe               # Remove everything except config.toml
```

## Project structure

```
backend/
  src/
    config.rs           Configuration loading, env var expansion
    server/             HTTP routes, auth, image proxy
    torrent/
      engine.rs         librqbit torrent management
      provider.rs       Search/browse dispatch, format-specific clients
      metadata.rs       Cinemeta client (IMDB ID resolution, no API key)
    transcode/          FFmpeg HLS pipeline (GPU detection, probe, segmenting)
    db/                 SQLite (users, history, downloads, favourites)
ui/
  src/
    pages/              Browse, Search, TvSearch, Player, etc.
    hooks/              useSearch, useStream, useAudioPlayer, etc.
    api/                API client and types
    components/         VideoPlayer, Layout, DebugPane, etc.
flake.nix               Nix flake for dev shell and builds
```

## Troubleshooting

**Port in use:** `ss -tlnp | grep 8999`. Kill the process or use `--port`.

**Frontend not showing:** Build the UI first (`cd ui && pnpm install && pnpm build`), then restart the backend.

**Nix flake not found:** Run `git add flake.nix` -- Nix requires tracked files.

**Video not playing on Safari:** Safari uses native HLS. If segments return 401, the auth token may be missing. Check the debug pane (user menu > Debug Mode).

**Transcoding fails on GPU:** FFmpeg automatically falls back to CPU encoding if hardware acceleration fails.
