# StreamX

Self-hosted torrent streaming. Search movies, TV, and music, paste a magnet link, and watch while it downloads.

StreamX ships as two binaries from one Rust workspace:

- **`streamx-desktop`**: a native desktop app with a GPU-rendered UI, built on [GPUI](https://www.gpui.rs/) from the [Zed](https://github.com/zed-industries/zed) editor. Linux and macOS share the same renderer, so the app looks and behaves identically on both. Windows support is coming soon.
- **`streamx`**: a single static server binary with the React web UI embedded. Runs headless on a box or NAS and serves any browser or phone.

![StreamX home page](docs/og-preview.png)

## Modes

| Mode | Binary | UI | Where the media engine runs |
|---|---|---|---|
| **Embedded** (default desktop) | `streamx-desktop` | Native GPUI | Inside the app. No separate server, no browser. Playback reads files straight from disk through mpv. The app also exposes the web UI on `:8999` for phones and other devices on the network. |
| **Thin client** | `streamx-desktop` | Native GPUI | On a remote `streamx` server. The app is a pure client: search, browse, and stream over HTTP from the server's library. |
| **Server** | `streamx` | React web UI | On the server. Browser playback with HLS; every browser and phone is a client. |

Pick Embedded or Thin client on the desktop app's login screen (changeable later in settings). All three modes share the same API, the same SQLite library, and the same torrent and transcoding engine.

## Features

- **Multi-source search**: movies (YTS, Torrentio), TV (Torrentio, eztv), music and music videos (apibay, 1337x).
- **Home page**: current-year releases (most downloaded), latest uploads, popular, top rated, and genre rows. Infinite scroll into any category.
- **Watch while it downloads**: sequential BitTorrent download with on-the-fly HLS transcoding for browsers, direct file playback in the desktop app.
- **Adaptive bitrate**: multi-variant HLS (360p / 720p / 1080p / source) with automatic quality switching.
- **Codec support**: H.264 passthrough, HEVC/H.265 passthrough on capable devices, MKV/AC3/DTS transcoded to browser-safe H.264 + AAC, surround preserved (up to 8 channels).
- **WEB transcode control**: admins choose whether non-WEB releases are transcoded server-side. Off by default, the browser gets the file as-is and non-WEB rows carry a crossed-out WEB badge; the desktop app plays everything natively.
- **Downloads**: pin a movie to keep downloading server-side with no client connected and watch it later from any device, or download the file straight to your device named after the movie. Stop, resume, and delete from the movie page or the Downloads page.
- **Multi-user safe**: a stream being watched by someone else cannot be deleted or paused out from under them.
- **Removable storage aware**: point `download_dir` at an external drive. If the drive is missing the app refuses to start or mutate the library instead of re-downloading or orphaning files.
- **Music player**: album browsing, per-track streaming while downloading, playlists, favourites, MediaSession (lock screen controls), AirPlay, Web Audio EQ, shareable track links with OG previews.
- **Shareable links**: guest tokens share a single stream by URL without handing out your account.
- **Multi-user**: bcrypt + JWT auth, per-user history and favourites, admin monitor with live disk, CPU, transcode, and download stats.
- **GPU acceleration**: FFmpeg auto-detects VAAPI / NVENC / VideoToolbox and falls back to CPU.
- **Optional SOCKS5 proxy** for torrent traffic.

## Architecture

- **`crates/server`**: Rust backend. Axum HTTP server, `librqbit` BitTorrent engine, FFmpeg transcoding, SQLite. Embeds `web/dist` into the `streamx` binary.
- **`crates/api`**: shared API types and the `Api` trait with two backends: HTTP (thin client, web) and in-process (embedded desktop). Every feature is written once against this trait.
- **`crates/desktop`**: the GPUI app. In Embedded mode it boots the server components in-process and talks to them with no network hop.
- **`web`**: React + Radix UI + `hls.js`, built with Vite.
- **Streaming pipeline**: peers → librqbit sequential download → FFmpeg (passthrough or transcode) → HLS master playlist → hls.js / Safari native. The desktop app skips HLS and hands the file to mpv.

## Build

All tooling is pinned via Nix.

```bash
nix develop

# Web UI (embedded into the server binary)
cd web && pnpm install && pnpm build && cd ..

# Server
cargo build --release -p streamx

# Desktop app (Linux, macOS)
cargo build --release -p streamx-desktop
```

Run `./target/release/streamx-desktop` for the app, or `./target/release/streamx` and open `http://127.0.0.1:8999` for the web UI.

The desktop app embeds **libmpv** for playback: video runs in-process, no mpv executable or PATH lookup. The dev shell provides libmpv via pkg-config; if libmpv cannot initialize at runtime the app falls back to an `mpv` executable when one is found. macOS desktop builds need Xcode's Metal shader compiler, see [Troubleshooting](#troubleshooting).

### Self-contained release artifacts

Every shipped binary must satisfy an explicit linkage policy, enforced by `crates/linkcheck` (tests in `cargo test`, a CLI for pipelines, and `nix flake check`):

| Target | Policy |
|---|---|
| Linux, musl (`streamx` server) | Fully static: no interpreter, no shared libraries |
| Linux, glibc (`streamx-desktop`) | Only allowlisted host libraries (libc, Vulkan, Wayland/X11, fontconfig); everything else static |
| macOS | Apple system frameworks plus dylibs bundled inside the `.app` (`@rpath`) |

```bash
# Static servers (run on a Linux host)
nix build .#streamx-x86_64-linux-musl
nix build .#streamx-aarch64-linux-musl

# macOS app: bundles libmpv and its FFmpeg closure into StreamX.app,
# rewrites install names, ad-hoc signs, and verifies the strict policy
scripts/bundle-macos.sh target/release/streamx-desktop dist/StreamX.app

# Check any artifact
cargo run -p streamx-linkcheck -- path/to/binary --policy static|linux-desktop|macos
```

FFmpeg is still an external runtime dependency of the server's HLS transcoding; embedding it is the next step.

## Configuration

Config lives at `~/.streamx/config.toml` and is created with defaults on first run. The desktop app and the server read the same file.

```toml
[server]
port = 8999
bind = "127.0.0.1"
# log_level = "info"

[torrent]
# download_dir = "/Volumes/storage/streamx"   # default: ~/.streamx/downloads
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

Env overrides: `STREAMX_DATA_DIR`, `STREAMX_CONFIG`, `STREAMX_PORT`, `STREAMX_BIND`, `STREAMX_LOG_LEVEL`, `STREAMX_ADMIN_USER`, `STREAMX_ADMIN_PASSWORD`.

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
cargo run -p streamx -- --port 8998
```

### Checks

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cd web && pnpm typecheck
```

### Tests

```bash
# Rust unit + integration tests (server, api, desktop)
cargo test --workspace

# Frontend unit tests
cd web && pnpm test

# Browser end-to-end tests (requires a running backend on port 8999)
cd web && pnpm test:e2e

# Desktop UI tests (JSON test driver + screenshots, see crates/ui-harness/README.md)
cargo build -p streamx-desktop --features ui-test && cargo build -p streamx && cargo run -p streamx-ui-harness
```

## CLI

```
streamx                                             # start the server (port 8999 by default)
streamx --port 9000                                 # custom port
streamx --admin-user <user> --admin-password <pw>   # create an admin user on first boot
streamx clean                                       # remove cache and downloads (keeps config + DB)
streamx wipe                                        # remove everything except config.toml
```

## Project layout

```
crates/
  server/              Rust backend (Axum, librqbit, FFmpeg, SQLite). Builds the `streamx` binary.
    src/
      config.rs          configuration, env overrides, download_dir resolution
      server/            HTTP routes, auth, admin monitor, static asset serving
      torrent/           librqbit engine, provider dispatch, canonical file listing
      transcode/         FFmpeg HLS pipeline (GPU detect, probe, multi-variant)
      db/                SQLite (users, history, downloads, favourites, playlists)
      local_api.rs       in-process Api backend used by the embedded desktop app
  api/                 shared API types + `Api` trait (HTTP and in-process clients)
  desktop/             GPUI desktop app (`streamx-desktop`): embedded and thin-client modes
  ui-harness/          Playwright-style desktop UI tests and screenshot export
web/                   React + TypeScript frontend (Vite, Radix UI, hls.js)
  src/
    pages/             Search, Browse, Movie, Player, Music, Downloads, Admin
    components/        VideoPlayer, AudioPlayerBar, ExpandedPlayer, Layout
    hooks/             useSearch, useStream, useAudioPlayer, useServerSettings
    api/               API client and types
Cargo.toml             Cargo workspace (members = ["crates/*"])
flake.nix              Nix flake (dev shell + builds)
```

## Troubleshooting

- **Port in use:** `ss -tlnp | grep 8999` (Linux) or `lsof -i :8999` (macOS) and kill the process, or pass `--port`.
- **Web UI looks stale after an upgrade:** the server embeds `web/dist` at compile time. Rebuild the UI (`cd web && pnpm build`), then rebuild the server binary. HTML, CSS, and JS are served with no-cache headers, and the client drops its session caches whenever the build hash changes, so a restart is all a browser needs.
- **Downloads directory unavailable at startup:** `download_dir` points at a volume that is not mounted. Mount the drive and restart. The app deliberately refuses to start without it rather than re-download into the wrong place.
- **Safari HLS shows 401:** the stream token is not attached to segment requests. Open the debug pane (user menu → Debug Mode) and check the last `/api/stream/.../playlist.m3u8` response.
- **Transcoding fails on GPU:** FFmpeg automatically falls back to CPU. Pass `--log-level debug` and watch the backend logs to see which acceleration was tried.
- **Experimental Nix feature 'nix-command' is disabled:** the flake file must be tracked by git (`git add flake.nix`).
  ```bash
  mkdir -p ~/.config/nix
  cat >> ~/.config/nix/nix.conf <<'EOF'
  experimental-features = nix-command flakes
  EOF
  ```
- **macOS desktop build fails with `tool 'metal' not found` or `missing Metal Toolchain`:** GPUI compiles its Metal shaders at build time with Apple's `metal` compiler, which ships only with the full Xcode.app (Command Line Tools is not enough, and nixpkgs cannot redistribute it).

  Fix (once per machine):
  1. Install Xcode.app from the Mac App Store, open it once, and accept the licence.
  2. Point the active developer directory at Xcode:
     ```bash
     sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
     sudo xcodebuild -license accept
     ```
  3. On Xcode 26 and later the Metal Toolchain is a separate download, and an Xcode update can drop it:
     ```bash
     xcodebuild -downloadComponent MetalToolchain
     ```
  4. Verify with `xcrun --find metal`, then re-run `nix develop --command cargo check -p streamx-desktop`.

  The server (`streamx`) and the web UI build fine without Xcode. Only `streamx-desktop` needs it.

## Tooling

- [`docs/og-preview.png`](docs/og-preview.png) is produced by `web/tests/screenshot-og.spec.ts`. Regenerate it against a running instance with:
  ```bash
  cd web && npx playwright test tests/screenshot-og.spec.ts --config tests/live.config.ts
  ```

## Credits

- The desktop UI is built on [GPUI](https://www.gpui.rs/), the GPU-accelerated Rust UI framework from the [Zed](https://github.com/zed-industries/zed) team. Their work is what makes a native, fluid, cross-platform player UI in Rust possible.
- [librqbit](https://github.com/ikatson/rqbit) for the BitTorrent engine, [FFmpeg](https://ffmpeg.org/) for transcoding, [mpv](https://mpv.io/) for desktop playback.

## License

[MIT](LICENSE)
