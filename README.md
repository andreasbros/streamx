# StreamX

Self-hosted torrent streaming. Single static Rust binary serving a React UI. Search movies, TV, and music, paste a magnet link, and stream directly in the browser with HLS transcoding and adaptive bitrate.

![StreamX home page](docs/og-preview.png)

## Features

- **Multi-source search**: movies (YTS, Torrentio), TV (Torrentio, eztv), music + music videos (apibay, 1337x)
- **Browser streaming**: sequential BitTorrent download + on-the-fly HLS transcoding (fMP4/CMAF). Watch while it downloads.
- **Adaptive bitrate**: multi-variant HLS (360p / 720p / 1080p / source). hls.js handles quality switching automatically.
- **Codec support**: H.264 passthrough, HEVC/H.265 passthrough on capable devices, MKV/AC3/DTS transcoded to browser-safe H.264 + AAC.
- **Surround audio preserved** through transcoding (up to 8 channels).
- **Music player**: album browsing, per-track streaming while downloading, playlists, favourites, MediaSession integration (iOS lock screen controls), AirPlay, Web Audio EQ, shareable track links with OG previews.
- **Shareable links**: guest tokens let you share a specific stream with a single URL, without giving away your account.
- **Multi-user**: bcrypt + JWT auth, per-user search/watch history and favourites.
- **GPU acceleration**: FFmpeg auto-detects VAAPI / NVENC / VideoToolbox, falls back to CPU.
- **Optional SOCKS5 proxy** for torrent traffic.

## Architecture

- **Backend** (Rust): Axum HTTP server, `librqbit` BitTorrent engine, FFmpeg for transcoding, SQLite for state. The release binary embeds the frontend.
- **Frontend** (TypeScript): React + Radix UI, `hls.js` for HLS playback, `framer-motion` for transitions. Built with Vite.
- **Streaming pipeline**: torrent peers → librqbit sequential download → FFmpeg (passthrough or transcode) → HLS master playlist → hls.js / Safari native.

## Build

All tooling is pinned via Nix. Enter the dev shell, then build the frontend and the release backend (the release binary bundles the UI).

```bash
nix develop

# Frontend
cd web && pnpm install && pnpm build && cd ..

# Backend (release build embeds the UI)
cargo build --release --manifest-path crates/server/Cargo.toml

# Run
./target/release/streamx
# Open http://127.0.0.1:8999
```

## Configuration

Config lives at `~/.streamx/config.toml` and is created with defaults on first run.

```toml
[server]
port = 8999
bind = "127.0.0.1"
# log_level = "info"

[torrent]
max_connections = 200
sequential = true

[transcode]
video_codec = "h264"
preset = "ultrafast"
crf = 23
hls_force_stereo = true   # set false to preserve surround in HLS tiers

[auth]
session_duration = "7d"
# jwt_secret auto-generated on first run if empty

# Movies: YTS has browse + search with rich metadata
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

# Music
[[providers]]
id = 4
kind = "music"
url = "https://apibay.org"
format = "apibay"
category = "101"

# Optional: route torrent traffic through a SOCKS5 proxy
# [vpn]
# socks5 = "socks5://user:pass@host:port"
```

Extra providers can be kept out of the main config in `~/.streamx/providers.toml` (same `[[providers]]` format). That file is gitignored by default.

### Provider formats

| Format | Supports | How it works |
|---|---|---|
| `yts` | Movies (browse + search) | YTS JSON API. Rich metadata (posters, ratings, trailers). |
| `torrentio` | Movies, TV (search only) | Resolves text to IMDB IDs via [Cinemeta](https://v3-cinemeta.strem.io), then fetches streams from [Torrentio](https://torrentio.strem.fun). No API key. |
| `apibay` | TV, Music (browse + search) | Pirate Bay JSON API. |
| `eztv` | TV (browse + search) | EZTV API. Structured season/episode data. |
| `scrape` | Music (browse + search) | Scrapes 1337x HTML. |

Torrentio has no catalog (it is IMDB-ID based), so keep YTS as the movies provider if you want the home page populated.

## Development

Hot reload with the Vite dev server proxying to a `cargo run` backend:

```bash
nix develop

# Terminal 1: frontend dev server (vite on :9000, proxies /api to :8998)
cd web && pnpm dev

# Terminal 2: backend
cargo run --manifest-path crates/server/Cargo.toml -- --port 8998
```

### Checks

```bash
cargo fmt --all --manifest-path crates/server/Cargo.toml
cargo clippy --manifest-path crates/server/Cargo.toml -- -D warnings
cargo check --manifest-path crates/server/Cargo.toml
cd web && pnpm typecheck
```

### Tests

```bash
# Rust unit + integration tests
cargo test --manifest-path crates/server/Cargo.toml

# Frontend component tests
cd web && pnpm test

# End-to-end browser tests (requires a running backend on port 8999)
cd web && pnpm test:e2e
```

## CLI

```
streamx                                             # start the server (port 8999 by default)
streamx --port 9000                                 # custom port
streamx --admin-user <user> --admin-password <pw>   # create an admin user on first boot
streamx clean                                       # remove cache and downloads (keeps config + DB)
streamx wipe                                        # remove everything except config.toml
```

Credentials can also be passed via `STREAMX_ADMIN_USER` and `STREAMX_ADMIN_PASSWORD` env vars.

## Project layout

```
crates/
  server/              Rust backend (Axum, librqbit, FFmpeg, SQLite). Builds the `streamx` binary.
    src/
      config.rs          configuration + env var expansion
      server/            HTTP routes, auth, image proxy, static asset serving
      torrent/           librqbit engine, provider dispatch, search formats
      transcode/         FFmpeg HLS pipeline (GPU detect, probe, multi-variant)
      db/                SQLite (users, history, downloads, favourites, playlists)
web/                   React + TypeScript frontend (Vite, Radix UI, hls.js)
  src/
    pages/             Search, Browse, Player, Music, Favourites, etc.
    components/        VideoPlayer, AudioPlayerBar, ExpandedPlayer, Layout
    hooks/             useSearch, useStream, useAudioPlayer, useMediaSession
    api/               API client and types
Cargo.toml             Cargo workspace (members = ["crates/*"])
flake.nix              Nix flake (dev shell + builds)
```

## Troubleshooting

- **Port in use:** `ss -tlnp | grep 8999` and kill the process, or pass `--port`.
- **Frontend not showing:** build the UI first (`cd web && pnpm install && pnpm build`), then restart the backend.
- **Safari HLS shows 401:** the stream token is not attached to segment requests. Open the debug pane (user menu → Debug Mode) and check the last `/api/stream/.../playlist.m3u8` response.
- **Transcoding fails on GPU:** FFmpeg automatically falls back to CPU. Pass `--log-level debug` and watch the backend logs to see which acceleration was tried.
- **Experimental Nix feature 'nix-command' is disabled:  add '--extra-experimental-features nix-command' to enable it:** the flake file must be tracked by git (`git add flake.nix`).
  ```bash
  mkdir -p ~/.config/nix
  cat >> ~/.config/nix/nix.conf <<'EOF'                                                                                                                                                                                                                                                                                                                                                                                                                                             
  experimental-features = nix-command flakes                                                                                                                                                                                                                                                                                                                                                                                                                                        
  EOF
  ```
- **macOS desktop build fails with `tool 'metal' not found`:** GPUI's macOS renderer
  compiles `shaders.metal` into a `.metallib` at build time via Apple's `metal` shader
  compiler. That compiler ships only with the **full Xcode.app** — Command Line Tools
  alone is not enough, and nixpkgs cannot redistribute it.

  Verify:
  ```bash
  xcrun --find metal
  # → xcrun: error: unable to find utility "metal"...
  ```

  Fix (once per machine, ~10 GB download):
  1. Install Xcode.app from the Mac App Store.
  2. Open it once so it installs additional components and accept the licence.
  3. Point the active developer directory at Xcode instead of CLT:
     ```bash
     sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
     sudo xcodebuild -license accept
     ```
  4. Verify:
     ```bash
     xcrun --find metal
     # → /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/metal
     ```
  5. From a fresh shell, re-run `nix develop --command cargo check -p streamx-desktop`.

  The server (`streamx`) and the web UI build fine without Xcode — only the
  `streamx-desktop` GPUI crate needs it.

## Tooling

- [`docs/og-preview.png`](docs/og-preview.png) is produced by `web/tests/screenshot-og.spec.ts`. Regenerate it against a running instance with:
  ```bash
  cd web && npx playwright test tests/screenshot-og.spec.ts --config tests/live.config.ts
  ```

## License

[MIT](LICENSE)
