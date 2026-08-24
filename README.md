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

All tooling is pinned via Nix. Builds are one command each: Nix builds
the web UI in the sandbox, embeds it, and compiles the Rust binaries.

```bash
nix build .#streamx           # server, native
nix build .#streamx-desktop   # desktop app (Linux)
```

Binaries land in `./result/bin/`.

### Development

For iterative work, enter the dev shell and drive the toolchains
directly. Cargo builds embed `web/dist` from the working tree, so build
the web UI once first (and again after changing `web/`):

```bash
nix develop

# Web UI (embedded into the server binary by cargo builds)
cd web && pnpm install && pnpm build && cd ..

# Server
cargo build --release -p streamx

# Desktop app (Linux, macOS)
cargo build --release -p streamx-desktop
```

Run `./target/release/streamx-desktop` for the app, or `./target/release/streamx` and open `http://127.0.0.1:8999` for the web UI.

The desktop app embeds **libmpv** for playback: video runs in-process, no mpv executable or PATH lookup. The dev shell provides libmpv via pkg-config; if libmpv cannot initialize at runtime the app falls back to an `mpv` executable when one is found. macOS desktop builds need Xcode's Metal shader compiler, see [Troubleshooting](#troubleshooting).

### Self-contained release artifacts

`scripts/verify-release.sh` runs the whole release gate in one command:
builds every Linux artifact, enforces the linkage policies via
`nix flake check`, runs the workspace test suite, and boots the
artifacts in stock distro containers (skipped when no Docker daemon is
running). The individual pieces are described below.

Supported platforms:

| Artifact | Platforms | Requirements on the host |
|---|---|---|
| `streamx` server (musl, x86_64 + aarch64) | Any Linux distribution, any libc, containers | None: fully static, FFmpeg embedded |
| `streamx-desktop` (Linux, x86_64) | Desktop distros with glibc >= 2.39 (Ubuntu 24.04+, Fedora 40+, Debian 13+) | Base desktop system only (glibc, X11/Wayland, Vulkan, ALSA) |
| `StreamX.app` (macOS, Apple Silicon + Intel) | macOS | None: libmpv/FFmpeg bundled in the .app |
| Windows | planned | |

The server is truly static: musl libc is compiled into the binary, there
is no dynamic loader and no shared libraries, so it runs identically on
every distro and in empty containers. A **GUI** binary cannot be fully
static on Linux: video needs Vulkan, whose loader `dlopen`s the host's
GPU driver (NVIDIA, Mesa) at runtime, and audio reaches
PipeWire/PulseAudio through plugins `dlopen`ed by the host's ALSA.
The desktop app therefore follows the shape every shipped Linux GUI app
uses (Steam, OBS, AppImages): everything we own is linked statically
(libmpv, FFmpeg, libass, libplacebo, shaderc), and only the OS interface
layer is dynamic, restricted to an explicit allowlist of libraries every
desktop installation already has. The glibc >= 2.39 floor comes from the
build toolchain's symbol versioning; supporting older LTS releases means
building against an older glibc sysroot (roadmap).

Every shipped binary must satisfy an explicit linkage policy, enforced by `crates/linkcheck` (tests in `cargo test`, a CLI for pipelines, and `nix flake check`):

| Target | Policy |
|---|---|
| Linux, musl (`streamx` server) | `static`: no interpreter, no shared libraries |
| Linux, glibc (`streamx-desktop`) | `linux-dist`: standard system loader, no store RUNPATH, only allowlisted host sonames; everything else static |
| macOS | Apple system frameworks plus dylibs bundled inside the `.app` (`@rpath`) |

```bash
# Static servers (run on a Linux host); web UI built and embedded
# automatically like every nix output.
nix build .#streamx-x86_64-linux-musl
nix build .#streamx-aarch64-linux-musl

# Verify
file result/bin/streamx
#   ELF 64-bit LSB pie executable, x86-64 ... static-pie linked
nix build .#checks.x86_64-linux.linkage-server-x86_64-musl
#   ok: .../bin/streamx satisfies fully-static (0 shared libraries)
STREAMX_LINKCHECK_BIN=$PWD/result/bin/streamx cargo test -p streamx --test static_link

# Linux desktop, distributable artifact: the plain .#streamx-desktop
# output only starts on Nix systems (its ELF interpreter lives in
# /nix/store); the -dist variant rewrites it to the standard system
# loader. Verified to run on stock Ubuntu 24.04.
nix build .#streamx-desktop-dist
nix build .#checks.x86_64-linux.linkage-desktop-dist
#   ok: .../bin/streamx-desktop satisfies linux-dist (16 shared libraries)
# ldd shows only allowlisted host sonames (libc, libstdc++, Vulkan,
# X11/Wayland, xkbcommon, ALSA); no mpv/FFmpeg/codec libraries.

# macOS releases: one command per architecture, run on a Mac.
# Produces a drag-to-Applications disk image containing StreamX.app
# with libmpv, its FFmpeg closure, and the ffmpeg/ffprobe executables
# bundled (Contents/Helpers), ad-hoc signed and policy-verified.
# Intel builds work on Apple Silicon via Rosetta-backed Nix
# (`extra-platforms = x86_64-darwin` in nix.conf).
scripts/release.sh aarch64-apple-darwin dist/StreamX-aarch64.dmg
scripts/release.sh x86_64-apple-darwin  dist/StreamX-x86_64.dmg
# .zip instead of .dmg produces a ditto zip; a bare path produces the .app.
# Set CODESIGN_IDENTITY="Developer ID Application: ..." to sign for
# distribution; without it users approve the first launch once via
# System Settings > Privacy & Security > "Open Anyway".

# Lower-level: bundle an already built binary into an .app
scripts/bundle-macos.sh target/release/streamx-desktop dist/StreamX.app
```

### Releases

Versioning is [SemVer](https://semver.org) with the workspace
`Cargo.toml` as the single source of truth; `CHANGELOG.md` is generated
from [Conventional Commit](https://www.conventionalcommits.org)
messages by git-cliff (`cliff.toml`).

```bash
# Everything this platform can build (macOS: both dmgs; Linux: the
# full verify-release gate)
nix run .#build-all

# Cut a release from a clean, pushed main: bumps the version, writes
# CHANGELOG.md, commits, tags vX.Y.Z, pushes, and creates the GitHub
# release. The tag triggers .github/workflows/release.yml, which builds
# and attaches the Linux and macOS artifacts.
nix run .#release -- patch          # or minor / major / X.Y.Z
nix run .#release -- patch --dry-run   # preview the notes first
```

Step-by-step instructions: [docs/RELEASING.md](docs/RELEASING.md).

# Check any artifact
cargo run -p streamx-linkcheck -- path/to/binary --policy static|linux-desktop|linux-dist|linux-dev|macos|macos-dev
```

The musl servers are built with the `embed-ffmpeg` cargo feature: static
`ffmpeg`/`ffprobe` executables are embedded in the binary and extracted
to `<data>/cache/bin` on first start (hash-checked), so HLS transcoding
works on a host with nothing installed. Builds without the feature keep
resolving both tools from `PATH`. The embedded FFmpeg carries a curated
feature set (native decoders plus dav1d, libx264 + native aac encoding,
plain-http IO); the static in-process libmpv shares it, so the desktop
player streams `http://` in-process and falls back to a system `mpv`
for `https://` remote servers.

If a `nix build` fails fetching a git crate dependency (for example
`Cannot find Git revision ... in ref 'master' of repository .../blade`),
a global `url."git@github.com:".insteadOf = "https://github.com/"` git
rewrite is the cause: Nix's evaluator shells out to git to vendor
`Cargo.lock` git dependencies over anonymous HTTPS, the rewrite forces
those fetches onto SSH, and without your SSH agent in the evaluator's
environment the fetch fails (Nix then misreports it as a missing
revision). Fix it by scoping the rewrite to pushes only, which keeps
SSH pushes working while fetches stay on HTTPS:

```bash
git config --global --unset url.git@github.com:.insteadof
git config --global url."git@github.com:".pushInsteadOf "https://github.com/"
```

One-off alternative: prefix the build with `GIT_CONFIG_GLOBAL=/dev/null`.

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

# Release artifacts in stock distro containers (testcontainers; needs a
# Docker daemon, Docker Desktop or colima on macOS, and network):
# - boots the static server in vanilla Alpine, checks the linkage
#   policy, embedded web UI and ffmpeg, then streams a real torrent
# - boots the same binary across Ubuntu/Debian/Rocky/Fedora images
# - loads the desktop dist binary on stock Ubuntu 24.04 with only
#   distro packages installed
nix build .#streamx-x86_64-linux-musl --out-link result-x86_64-musl   # aarch64 on Apple Silicon
nix build .#streamx-desktop-dist --out-link result-desktop-dist
cargo test -p streamx --test docker_static_tests -- --ignored
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
