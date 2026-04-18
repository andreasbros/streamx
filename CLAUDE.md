# StreamX

Torrent-based video streaming player. Single static Rust binary serving a React UI.

## Build

All tools are managed via Nix. Run `nix develop` to enter the dev shell.

```bash
nix develop
cd ui && pnpm install && pnpm build
cd backend && cargo build
```

## Code standards

- Rust: `cargo fmt --all`, `cargo clippy -- -D warnings`, `cargo check` must all pass with zero warnings
- No `.unwrap()`, `.expect()`, or `panic!()` anywhere in Rust code
- Error handling via `snafu` with context selectors
- All async code uses `tokio`
- TypeScript: strict mode, no `any` types
- No `dangerouslySetInnerHTML` in React
- All SQL queries must use parameterized statements (rusqlite)
- No hardcoded secrets; JWT secret auto-generated on first run
- No em-dashes in docs or comments
- No static obvious code comments
- Lean documentation

## Project structure

- `backend/` - Rust backend (Axum, librqbit, FFmpeg transcoding, SQLite)
- `ui/` - React/TypeScript frontend (Vite, Radix UI, hls.js, framer-motion)
- `flake.nix` - Nix flake for dev shell and builds

## Testing

- Rust: `cargo test` (unit + integration)
- Frontend: `pnpm test` (vitest) and `pnpm test:e2e` (Playwright)
- E2E tests use real backend with mock streaming endpoint
- All tests must run inside `nix develop`
- Performance metrics tracked in `benchmarks/e2e_perf.json` (git-tracked)
- After running E2E tests, serve the report for review: `python3 -m http.server 8997 -d /tmp/streamx_e2e_artifacts/html-report`
- Report port: 8997 (configurable via `STREAMX_REPORT_PORT`)
