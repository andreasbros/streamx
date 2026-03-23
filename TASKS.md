# StreamX Tasks

## Done

- [x] Backend scaffold (CLI, config, Snafu errors, tracing, Axum)
- [x] SQLite database with migrations, user CRUD
- [x] Auth: bcrypt + JWT, rate limiting, case-insensitive usernames
- [x] YTS torrent search (real API, verified with e2e tests)
- [x] librqbit torrent engine (real BitTorrent with DHT, sequential download)
- [x] Demo video streaming (Big Buck Bunny HLS via Mux)
- [x] React frontend (Radix UI, video.js, dark/light themes)
- [x] Login/Register page with neon laser Three.js background
- [x] Search page with poster images, quality badges, sort controls
- [x] Video player: video.js for Chrome/Firefox, native for Safari
- [x] Direct file streaming with HTTP range requests (no HLS needed for MP4)
- [x] librqbit FileStream with piece prioritization for seeking in partial downloads
- [x] Watch history UI + backend API
- [x] Settings page with theme toggle
- [x] Debug pane (collapsible, log levels, auto-scroll)
- [x] Logo (SX SVG with white outline, overlapping letters)
- [x] Nix dev shell (flake.nix with rust-overlay, pnpm, node)
- [x] Playwright e2e tests (API + browser, against real backend)
- [x] Rust integration tests (stream lifecycle, auth, API)
- [x] FFmpeg transcoding: GPU (VAAPI/NVENC/QSV), CPU fallback, HDR tone mapping
- [x] FFmpeg faststart remux for MP4 files with moov at end
- [x] Torrent pause/resume lifecycle (auto-pause on heartbeat timeout)
- [x] Stream recovery after restart (file_path.txt cache, hash-based IDs)
- [x] Non-blocking stream creation (instant response, background torrent add)
- [x] CLI: clean, wipe commands
- [x] CLI: --admin-user/--admin-password
- [x] SQLite in .streamx/db/ folder
- [x] Cloudflare tunnel support
- [x] README with streaming pipeline docs

## High priority

- [x] Safari HEVC/x265 fallback: auto-detect unsupported codecs via canPlayType + file extension, fall back to HLS CPU transcode
- [ ] HLS seek to any position: when user seeks beyond transcoded range, restart FFmpeg with `-ss {position}`
  - Frontend detects seek beyond available range
  - Requests `/api/stream/{id}/playlist.m3u8?seek={seconds}`
  - Backend kills current FFmpeg, starts new from that position
  - Returns new playlist, frontend switches to it
- [ ] Download management: partial/ and complete/ directories
  - librqbit downloads to `downloads/partial/{torrent_name}/`
  - On completion + hash verification, atomic move to `downloads/complete/{torrent_name}/`
  - SQL `downloads` table tracking state per file:
    ```sql
    CREATE TABLE downloads (
        info_hash TEXT PRIMARY KEY,
        magnet_uri TEXT NOT NULL,
        title TEXT,
        file_name TEXT,
        file_size INTEGER,
        status TEXT NOT NULL,  -- initializing, downloading, verifying, complete, paused, error
        progress REAL DEFAULT 0,
        partial_path TEXT,
        complete_path TEXT,
        peers INTEGER DEFAULT 0,
        download_speed INTEGER DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    ```
  - Lock-free: SQLite WAL for concurrent reads/writes, atomic rename for file move
  - No deadlocks: single writer pattern, no mutex nesting
  - All code paths check download state from DB, not in-memory guesses
  - file_path.txt replaced by DB lookup
- [ ] Move DHT files to .streamx/dht/ folder
- [ ] Nix static build: `nix build` producing musl-linked binary
- [ ] Frontend embedding: production binary serves built UI assets
- [ ] scripts/build-release.sh: cross-compile for all targets

## Medium priority

- [ ] Comprehensive Playwright browser tests for all UI flows
- [ ] Frontend unit tests (vitest): toward 95% coverage
- [ ] Rust unit test coverage: toward 95%
- [ ] Watch history resume: save/restore playback position
- [ ] Search history in UI
- [ ] Subtitle support: extract .srt from torrent, render in player
- [ ] Stream metadata in DB (poster, year, rating, file type)

## Polish

- [ ] Skeleton loading states
- [ ] Toast notifications
- [ ] Error states UI
- [ ] Mobile responsive layout
- [ ] Picture-in-Picture
- [ ] Playback speed selector
- [ ] Quality selector
- [ ] scripts/generate-logo.sh
- [ ] Favicon.ico + apple-touch-icon generation
