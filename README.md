# StreamX

Torrent-based video streaming player. Single static Rust binary serving a React UI. Search for torrents, paste magnet links, stream video in the browser.

## About

StreamX starts a web server with a modern UI where you can search for torrents, paste magnet links, and stream video content directly in the browser. All dependencies (including FFmpeg) are statically linked into a single binary.

- Rust backend: Axum, librqbit (BitTorrent), FFmpeg transcoding, SQLite
- React frontend: Radix UI, video.js, framer-motion
- Auth: bcrypt + JWT, multi-user with search/watch history
- Streaming: sequential torrent download with on-the-fly HLS transcoding

## How streaming works

A torrent downloads one movie file (e.g. `Movie.2024.720p.mp4` at ~1GB). BitTorrent downloads this file sequentially so the beginning arrives first.

Once enough data is available (~30% or the file is complete), FFmpeg converts the movie into HLS segments. Each segment is a 4-second chunk of video (~50-500KB as `.ts` files). A playlist file (`playlist.m3u8`) lists all segments in order.

The browser loads segments one at a time using video.js (Chrome/Firefox) or Safari's native HLS player. This allows playback to start before the full file is downloaded.

```
Torrent peers --> librqbit (sequential download) --> movie.mp4
                                                        |
                                          FFmpeg (passthrough or transcode)
                                                        |
                                          HLS segments (segment_0000.ts, segment_0001.ts, ...)
                                                        |
                                          playlist.m3u8
                                                        |
                                          video.js / Safari native HLS --> browser playback
```

Segment duration is configurable via `hls_segment_duration` in `config.toml` (default: 4 seconds). FFmpeg uses hardware acceleration (VAAPI, NVENC, QSV, VideoToolbox) when available, falling back to CPU (libx264).

## Local build

All tools are managed via Nix.

```bash
nix develop

# Frontend
cd ui && pnpm install && pnpm build && cd ..

# Backend
cd backend && cargo build && cd ..

# Run
cd backend && cargo run
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

The vite dev server runs on port 8999 and proxies API requests to the backend on port 8998.

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

## Troubleshooting

**Port 8999 in use:** Check with `ss -tlnp | grep 8999`. Kill the process or use `--port` to pick a different port.

**Frontend not showing:** Build the UI first with `cd ui && pnpm install && pnpm build`, then restart the backend.

**Nix flake not found:** Run `git add flake.nix` -- Nix requires tracked files.

**Video not playing on iPhone/Safari:** Safari uses native HLS. If segments return 401, the auth token may be missing from the playlist URL. Check the debug pane (user menu > Debug Mode).

**Transcoding fails on GPU:** If VAAPI/NVENC fails for certain codecs (e.g. x265 10-bit), FFmpeg automatically falls back to CPU encoding (libx264).
