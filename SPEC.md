# StreamX — Torrent Video Streaming Player

## Overview

StreamX is a single static binary torrent-based video streaming player written in Rust. When executed, it starts a web server serving a modern reactive UI where users can search for torrents, paste magnet links, and instantly stream video content in the browser. All dependencies — including FFmpeg libraries — are statically linked into the binary.

**Project name:** `streamx`
**Binary name:** `streamx`
**Logo:** Square logo with "SX" monogram

---

## Architecture

```
streamx (single static binary, ~30-50MB)
├── Axum web server (serves UI + REST API + HLS segments)
├── librqbit (BitTorrent engine — full protocol, DHT, PEX, magnet links)
├── FFmpeg (statically linked — transcoding MKV/x265 → HLS)
├── SQLite (user accounts, search history, watch history)
├── Embedded frontend (rust-embed — React/TypeScript SPA)
└── Config/data directory (~/.streamx/)
```

```
┌──────────────────────────────────────────────────────────┐
│                   streamx binary                         │
│                                                          │
│  ┌────────────┐  ┌──────────────┐  ┌──────────────────┐ │
│  │ Axum       │  │ librqbit     │  │ SQLite (rusqlite)│ │
│  │ Web Server │  │ BitTorrent   │  │                  │ │
│  │            │  │              │  │ - Users/auth     │ │
│  │ - REST API │  │ - Full swarm │  │ - Search history │ │
│  │ - HLS      │  │ - TCP/UDP   │  │ - Watch history  │ │
│  │ - Auth     │  │ - DHT/PEX   │  │ - Settings       │ │
│  │ - Static   │  │ - Magnets   │  │                  │ │
│  └─────┬──────┘  └──────┬──────┘  └──────────────────┘ │
│        │                │                                │
│  ┌─────┴────────────────┴──────────────────────────────┐ │
│  │ FFmpeg (statically linked via ffmpeg-sys-next)      │ │
│  │ Transcode: MKV/x265/x264/AAC → HLS (fMP4 + m3u8)  │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌─────────────────────────────────────────────────────┐ │
│  │ Embedded Frontend (rust-embed)                      │ │
│  │ React + TypeScript + Radix UI + hls.js              │ │
│  └─────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

---

## Project Structure

```
streamx/
├── SPEC.md
├── Cargo.toml                  # Workspace root
├── Cargo.lock
├── flake.nix                   # Nix flake — dev shell + build
├── flake.lock
├── .envrc                      # direnv — auto nix develop
├── rust-toolchain.toml         # Pin Rust nightly/stable
├── clippy.toml                 # Clippy config
├── rustfmt.toml                # Fmt config
├── streamx.default.toml        # Default config reference
├── scripts/
│   ├── generate-logo.sh        # Bash script to generate SX logo icons
│   └── build-release.sh        # Cross-compile release builds
├── backend/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs             # Entry point — CLI, config loading, server start
│   │   ├── cli.rs              # Clap CLI definition
│   │   ├── config.rs           # Config loading (TOML + env vars + CLI args)
│   │   ├── error.rs            # Snafu error types
│   │   ├── server/
│   │   │   ├── mod.rs          # Axum router setup
│   │   │   ├── auth.rs         # Auth middleware — JWT + bcrypt passwords
│   │   │   ├── api.rs          # REST API handlers
│   │   │   ├── stream.rs       # HLS streaming endpoint
│   │   │   └── static_files.rs # Serve embedded frontend
│   │   ├── torrent/
│   │   │   ├── mod.rs          # Torrent engine wrapper around librqbit
│   │   │   ├── engine.rs       # Start/stop/status torrents
│   │   │   ├── provider.rs     # Default torrent search provider
│   │   │   └── types.rs        # Torrent-related types
│   │   ├── transcode/
│   │   │   ├── mod.rs          # FFmpeg transcoding pipeline
│   │   │   ├── hls.rs          # HLS segment generation
│   │   │   └── probe.rs        # Media file probing (codec detection)
│   │   ├── db/
│   │   │   ├── mod.rs          # SQLite connection pool
│   │   │   ├── migrations.rs   # Schema migrations (embedded)
│   │   │   ├── users.rs        # User CRUD
│   │   │   ├── history.rs      # Search + watch history
│   │   │   └── settings.rs     # Per-user settings
│   │   └── embedded.rs         # rust-embed frontend assets
│   └── tests/
│       ├── api_tests.rs        # API integration tests
│       ├── torrent_tests.rs    # Torrent engine unit tests
│       ├── transcode_tests.rs  # Transcoding tests
│       ├── auth_tests.rs       # Auth flow tests
│       └── e2e_tests.rs        # Full end-to-end tests
├── ui/
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── tsconfig.json
│   ├── vite.config.ts
│   ├── playwright.config.ts
│   ├── index.html
│   ├── public/
│   │   └── icons/              # Generated logo icons
│   ├── src/
│   │   ├── main.tsx            # React entry point
│   │   ├── App.tsx             # Root app — router + theme provider
│   │   ├── api/
│   │   │   ├── client.ts       # API client (fetch wrapper with auth)
│   │   │   └── types.ts        # API response types
│   │   ├── hooks/
│   │   │   ├── useAuth.ts      # Auth context + hooks
│   │   │   ├── useStream.ts    # Stream status polling
│   │   │   ├── useSearch.ts    # Search with debounce
│   │   │   └── useTheme.ts     # Dark/light theme toggle
│   │   ├── pages/
│   │   │   ├── Login.tsx       # Login / register page
│   │   │   ├── Search.tsx      # Search + magnet link input
│   │   │   ├── Player.tsx      # Video player page (fullscreen capable)
│   │   │   └── History.tsx     # Watch history page
│   │   ├── components/
│   │   │   ├── VideoPlayer.tsx # hls.js + HTML5 video + custom controls
│   │   │   ├── SearchBar.tsx   # Search input with autocomplete
│   │   │   ├── TorrentCard.tsx # Search result card (seeds, size, quality)
│   │   │   ├── ProgressBar.tsx # Download progress overlay
│   │   │   ├── ThemeToggle.tsx # Dark/light switch
│   │   │   └── Layout.tsx      # App shell / navigation
│   │   ├── styles/
│   │   │   └── global.css      # Radix UI theme tokens + animations
│   │   └── lib/
│   │       ├── auth.ts         # JWT token management
│   │       └── utils.ts        # Shared utilities
│   └── tests/
│       ├── login.spec.ts       # Playwright: auth flow
│       ├── search.spec.ts      # Playwright: search + results
│       ├── player.spec.ts      # Playwright: video playback
│       └── history.spec.ts     # Playwright: watch history
└── README.md
```

---

## Technology Stack

### Backend (Rust)

| Crate | Purpose | Version |
|---|---|---|
| `axum` | Web server + routing | latest |
| `tokio` | Async runtime (full features) | latest |
| `librqbit` | BitTorrent engine | latest |
| `rusqlite` | SQLite with bundled feature | latest |
| `rust-embed` | Embed frontend assets in binary | latest |
| `snafu` | Error handling (no unwrap/panic) | latest |
| `clap` | CLI argument parsing (derive) | latest |
| `serde` / `serde_json` | Serialization | latest |
| `toml` | Config file parsing | latest |
| `bcrypt` | Password hashing | latest |
| `jsonwebtoken` | JWT auth tokens | latest |
| `tower-http` | CORS, compression, tracing middleware | latest |
| `tracing` / `tracing-subscriber` | Structured logging | latest |
| `ffmpeg-sys-next` | Static FFmpeg bindings | latest |
| `uuid` | Unique IDs for streams | latest |
| `tokio-util` | Async utilities | latest |

### Frontend (TypeScript/React)

| Package | Purpose |
|---|---|
| `react` + `react-dom` | UI framework |
| `@radix-ui/themes` | Component library (dark/light themes) |
| `@radix-ui/react-*` | Individual Radix primitives as needed |
| `hls.js` | HLS video playback |
| `react-router-dom` | Client-side routing |
| `vite` | Build tool |
| `typescript` | Type safety |
| `@playwright/test` | E2E browser tests |
| `framer-motion` | Animations (dopamine-inducing transitions) |

### Build / Dev

| Tool | Purpose |
|---|---|
| Nix (flake) | Reproducible dev environment + builds |
| `pnpm` | Frontend package manager |
| `cargo` | Rust build |
| `musl` | Static linking target for Linux |
| Playwright | UI E2E tests |

---

## Detailed Requirements

### 1. CLI & Configuration

**Priority order** (highest wins):
1. CLI arguments
2. Environment variables (prefixed `STREAMX_`)
3. Config file (`~/.streamx/config.toml` or custom path via `--config`)
4. Defaults

**CLI (clap derive):**
```
streamx [OPTIONS]

Options:
  -p, --port <PORT>          Listen port [default: 8999] [env: STREAMX_PORT]
  -b, --bind <ADDR>          Bind address [default: 127.0.0.1] [env: STREAMX_BIND]
  -d, --data-dir <PATH>      Data directory [default: ~/.streamx] [env: STREAMX_DATA_DIR]
  -c, --config <PATH>        Config file path [env: STREAMX_CONFIG]
      --log-level <LEVEL>    Log level [default: info] [env: STREAMX_LOG_LEVEL]
      --open                 Open browser on start [env: STREAMX_OPEN]
  -V, --version              Print version
  -h, --help                 Print help
```

**Default config file (`~/.streamx/config.toml`):**
```toml
[server]
port = 8999
bind = "127.0.0.1"
open_browser = true

[torrent]
download_dir = "~/.streamx/downloads"
max_connections = 200
sequential = true          # Sequential download for streaming
seed_after_complete = true
dht = true
pex = true

[transcode]
# Auto-detect: if source is browser-compatible (MP4/H264/AAC), stream directly
# Otherwise transcode to HLS
hls_segment_duration = 4   # seconds
video_codec = "h264"
audio_codec = "aac"
preset = "ultrafast"       # Prioritize speed over compression

[auth]
jwt_secret = ""            # Auto-generated on first run if empty
session_duration = "7d"

[ui]
default_theme = "dark"
```

### 2. Data Directory (`~/.streamx/`)

Created automatically on first run:
```
~/.streamx/
├── config.toml            # User config (created from defaults if missing)
├── streamx.db             # SQLite database
├── downloads/             # Torrent downloads (configurable)
├── cache/                 # HLS segment cache
└── logs/                  # Log files (optional)
```

### 3. Authentication & Users

- First run: no users exist → UI shows registration form to create admin user
- Subsequent runs: login required
- Passwords: bcrypt hashed, stored in SQLite
- Sessions: JWT tokens, configurable expiry
- Multi-user: each user has own search/watch history
- API: all endpoints except `/api/auth/login` and `/api/auth/register` require valid JWT
- **Security:** parameterized SQL queries only (rusqlite), no string interpolation in queries. Input validation on all endpoints. Rate limiting on auth endpoints.

**SQLite schema:**
```sql
CREATE TABLE users (
    id TEXT PRIMARY KEY,          -- UUID
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE search_history (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    query TEXT NOT NULL,
    result_count INTEGER,
    searched_at TEXT NOT NULL
);

CREATE TABLE watch_history (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    magnet_uri TEXT NOT NULL,
    title TEXT NOT NULL,
    file_name TEXT,
    duration_seconds INTEGER,
    watched_seconds INTEGER,     -- Resume position
    poster_url TEXT,
    watched_at TEXT NOT NULL
);

CREATE TABLE active_streams (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    magnet_uri TEXT NOT NULL,
    file_index INTEGER NOT NULL,
    status TEXT NOT NULL,         -- downloading, transcoding, ready, error
    progress REAL,
    peers INTEGER,
    download_speed INTEGER,      -- bytes/sec
    created_at TEXT NOT NULL
);
```

### 4. REST API

All responses are JSON. Auth via `Authorization: Bearer <jwt>` header.

```
POST   /api/auth/register       { username, password } → { token }
POST   /api/auth/login          { username, password } → { token }
GET    /api/auth/me             → { user }

POST   /api/search              { query } → { results: [{ title, magnet, seeds, leeches, size }] }
GET    /api/search/history      → { searches: [...] }

POST   /api/stream              { magnet_uri, file_index? } → { stream_id, status }
GET    /api/stream/:id          → { status, progress, peers, speed, files }
GET    /api/stream/:id/playlist.m3u8  → HLS master playlist
GET    /api/stream/:id/:segment.ts    → HLS segment
DELETE /api/stream/:id          → stop torrent + cleanup

GET    /api/history             → { items: [...] }
PUT    /api/history/:id         { watched_seconds } → update resume position
DELETE /api/history/:id         → remove from history

GET    /api/settings            → { theme, ... }
PUT    /api/settings            { theme, ... } → update
```

### 5. Torrent Engine

- Use `librqbit` as the BitTorrent engine
- **Sequential downloading** enabled by default (critical for streaming — download pieces in order)
- Support magnet links and .torrent files
- Connect to full BitTorrent swarm (TCP/UDP), DHT, PEX
- Default search provider: integrate a provider that returns magnet links (configurable)
- When streaming starts:
  1. Parse magnet / start torrent
  2. List files in torrent
  3. Auto-select largest video file (or user selects)
  4. Begin sequential download
  5. Once enough data buffered (~2-5 seconds), start transcoding/serving

### 6. Transcoding Pipeline

**Decision flow:**
```
File selected → probe with FFmpeg
  → MP4 container + H264 + AAC → stream directly (no transcode)
  → MKV / x265 / other → transcode to HLS on the fly
```

**HLS transcoding:**
- Input: file being downloaded (can start before complete — read available bytes)
- Output: HLS playlist (`.m3u8`) + segments (`.ts`, 4 seconds each)
- FFmpeg flags: `-preset ultrafast -tune zerolatency` for minimum latency
- Segments generated on-demand (don't transcode the whole file upfront)
- Cache segments in `~/.streamx/cache/`
- Clean up cache when stream is stopped

**Static FFmpeg linking:**
- Use `ffmpeg-sys-next` with static feature flags
- All codecs compiled in: H264, H265/HEVC, AAC, AC3, VP9, AV1 decode
- Output codecs: H264 + AAC only (browser-compatible)
- The binary must contain all FFmpeg libraries — no system FFmpeg dependency

### 7. UI Design

**Framework:** React 18+ with TypeScript, Vite build, Radix UI components

**Theme:**
- Radix UI theme provider with dark (default) and light modes
- Dark theme: deep blacks (#0a0a0a), accent color (electric blue #3b82f6 or purple #8b5cf6)
- Smooth transitions between themes
- CSS variables from Radix UI tokens

**Pages:**

**Login/Register:**
- Clean centered card
- Username + password fields (Radix `TextField`)
- Toggle between login and register
- Animated transitions (framer-motion)

**Search (main page):**
- Large search bar at top (Radix `TextField` with search icon)
- Magnet link paste support (detect magnet: prefix, auto-start)
- Search results as cards in a grid:
  - Title, seeds (green), leeches (red), file size, quality badge (4K/1080p/720p)
  - Sort by seeds (default), size, name
  - Click to start streaming
- Smooth card entrance animations (stagger)
- Search history in sidebar or below

**Player:**
- Full-screen capable (Fullscreen API)
- Custom video controls overlay (Radix primitives):
  - Play/pause (spacebar)
  - Seek bar with preview thumbnails (if available)
  - Volume control + mute
  - Playback speed (0.5x, 1x, 1.25x, 1.5x, 2x)
  - Quality selector (if multiple transcoding profiles)
  - Fullscreen toggle (F key)
  - Picture-in-Picture (PiP)
  - Subtitles toggle (if .srt found in torrent)
- Download progress bar at bottom (thin, colored by buffered vs downloaded)
- Overlay stats (toggle with 'i' key): peers, download speed, upload speed, progress
- Auto-hide controls after 3 seconds of no mouse movement
- Keyboard shortcuts:
  - Space: play/pause
  - Left/Right: seek ±10s
  - Up/Down: volume
  - F: fullscreen
  - M: mute
  - Esc: exit fullscreen

**History:**
- Grid of watched content
- Poster image (if available), title, last watched date
- Resume position indicator
- Click to resume playback

**Animations (framer-motion):**
- Page transitions (slide/fade)
- Card hover effects (subtle scale + shadow)
- Loading states (skeleton screens with shimmer)
- Progress bar animations (smooth interpolation)
- Toast notifications (slide in/out)
- Player controls fade in/out

### 8. Logo Generation

**`scripts/generate-logo.sh`:**
- Uses ImageMagick (`convert` / `magick`) to generate the logo
- Square logo with "SX" monogram
- Dark background, modern sans-serif font
- Generate sizes: 16x16, 32x32, 48x48, 64x64, 128x128, 256x256, 512x512
- Output to `ui/public/icons/`
- Also generate `favicon.ico` (multi-size)
- Also generate `apple-touch-icon.png` (180x180)

### 9. Nix Configuration

**`flake.nix` must provide:**

**`nix develop`:**
- Rust toolchain (latest stable via `rust-overlay` or `fenix`)
- `cargo`, `rustfmt`, `clippy`
- `pnpm` (for frontend)
- Node.js LTS
- Playwright browsers
- `pkg-config`, `openssl` (if needed)
- musl cross-compilation toolchains
- ImageMagick (for logo generation)

**`nix build`:**
- Build frontend first (`pnpm build` in `ui/`)
- Copy frontend dist to `backend/ui-dist/` (for `rust-embed`)
- Build Rust binary with musl target for static linking
- Output: single static binary `streamx`

**Build targets:**
```
nix build .#streamx-x86_64-linux    # Linux x86_64 (musl static)
nix build .#streamx-aarch64-linux   # Linux ARM64 (musl static)
nix build .#streamx-x86_64-darwin   # macOS Intel
nix build .#streamx-aarch64-darwin  # macOS Apple Silicon
```

**`scripts/build-release.sh`:**
- Builds all 4 targets
- Creates checksums
- Output to `release/` directory

### 10. Code Quality Requirements

**Rust:**
- `cargo fmt` — zero formatting issues (use `rustfmt.toml` with defaults)
- `cargo clippy` — zero warnings, treat warnings as errors (`-D warnings`)
- `cargo check` — clean compilation
- **No `.unwrap()` or `.expect()` anywhere** — use `snafu` for all error handling
- **No `panic!()` macro** — graceful error propagation everywhere
- All errors use `Snafu` derive with context selectors
- All async code uses `tokio`
- All public functions documented
- Unit tests for all modules
- Integration tests for API endpoints
- E2E tests for full workflows

**Error handling pattern:**
```rust
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to bind server to {address}"))]
    ServerBind { address: String, source: std::io::Error },

    #[snafu(display("Database error"))]
    Database { source: rusqlite::Error },

    #[snafu(display("Torrent engine error: {message}"))]
    TorrentEngine { message: String },

    // ... etc
}

pub type Result<T> = std::result::Result<T, Error>;
```

**TypeScript/Frontend:**
- Strict TypeScript (`strict: true`)
- No `any` types
- ESLint + Prettier clean
- All API calls typed
- Playwright tests for all user flows

### 11. Testing

**Rust unit tests (`cargo test`):**
- Config loading (TOML, env vars, CLI args, priority order)
- Auth (register, login, JWT validation, bcrypt)
- Database operations (CRUD for all tables)
- Torrent types and parsing
- Transcoding probe logic
- API handler responses

**Rust E2E tests:**
- Start server → register user → login → search → start stream → verify HLS endpoint → stop stream
- Multi-user isolation
- Auth rejection for invalid tokens

**Playwright UI tests:**
- Login flow (register + login + logout)
- Search flow (enter query → see results → click result)
- Player flow (start stream → verify video loads → controls work)
- History flow (watch → appears in history → resume)
- Theme toggle
- Responsive layout

### 12. Security

- **SQL injection:** impossible — use `rusqlite` parameterized queries only
- **XSS:** React auto-escapes, no `dangerouslySetInnerHTML`
- **Auth:** bcrypt for passwords (cost factor 12), JWT with HMAC-SHA256
- **CORS:** restricted to same-origin by default
- **Rate limiting:** auth endpoints limited to 10 requests/minute per IP
- **Input validation:** all API inputs validated and sanitized
- **Path traversal:** file paths canonicalized and checked against allowed directories
- **No secrets in logs:** mask passwords and tokens in tracing output

---

## Build & Run

```bash
# Development
cd streamx
nix develop              # Enter dev shell
cd ui && pnpm install    # Install frontend deps
cd ui && pnpm dev        # Frontend dev server (hot reload)
cd backend && cargo run  # Backend dev server

# Production build
nix build .#streamx-x86_64-linux

# Run
./streamx                           # Defaults: http://127.0.0.1:8999
./streamx --port 9000 --open        # Custom port, open browser
STREAMX_PORT=9000 ./streamx         # Via env var
```

---

## Deployment — Cloudflare Tunnel

StreamX is exposed to the internet via a Cloudflare Tunnel on the developer's machine. The tunnel is already configured and running as a user-level systemd service.

**Public URL:** `https://streamx.cbdemo.net/`
**Local URL:** `http://localhost:8999`

The tunnel config at `~/.cloudflared/config.yml` contains an ingress rule:
```yaml
- hostname: streamx.cbdemo.net
  service: http://localhost:8999
```

The tunnel is managed with:
```bash
systemctl --user restart cloudflared   # Restart after config changes
systemctl --user status cloudflared    # Check status
```

The UI must handle being served behind a reverse proxy / tunnel:
- Use relative URLs for all API calls (no hardcoded `localhost`)
- Respect `X-Forwarded-*` headers
- WebSocket support for live progress updates (Cloudflare tunnels support WebSockets)

---

## Implementation Order

1. **Backend scaffold:** Cargo workspace, CLI (clap), config loading, Snafu errors, tracing
2. **Database:** SQLite setup, migrations, user model, auth (bcrypt + JWT)
3. **Web server:** Axum router, auth middleware, static file serving
4. **Frontend scaffold:** Vite + React + Radix UI + routing, login/register pages
5. **Torrent engine:** librqbit integration, magnet link handling, sequential download
6. **Search:** torrent search provider integration, search API, search UI
7. **Transcoding:** FFmpeg static linking, probe, HLS pipeline
8. **Streaming:** HLS endpoint, connect torrent download → transcode → serve
9. **Player UI:** hls.js integration, custom controls, fullscreen, keyboard shortcuts
10. **History:** watch history, resume position, history UI
11. **Polish:** animations, loading states, error states, themes
12. **Testing:** Rust unit tests, API integration tests, Playwright UI tests
13. **Nix build:** musl static builds for all 4 targets
14. **Logo:** generate-logo.sh script
15. **README:** usage docs, screenshots, build instructions
