# StreamX

Torrent-based video streaming player. Single static Rust binary serving a React UI.

## Build

All tools are managed via Nix. Run `nix develop` to enter the dev shell.

```bash
nix develop
cd web && pnpm install && pnpm build && cd ..
cargo build --manifest-path crates/server/Cargo.toml
```

Always set `CARGO_TARGET_DIR=target/nix` for cargo commands: the plain
`target/` dir is shared with the editor's host-toolchain rust-analyzer and
gets poisoned (E0514 stale-rmeta errors).

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

- `crates/server/` - Rust backend (Axum, librqbit, FFmpeg transcoding, SQLite). Cargo workspace member; binary still named `streamx`.
- `crates/api/` - shared API types + client (`Api` trait: HTTP and in-process backends).
- `crates/desktop/` - GPUI desktop app (`streamx-desktop`); `ui-test` feature adds the JSON test driver.
- `crates/ui-harness/` - Playwright-style desktop UI tests (see its README).
- `web/` - React/TypeScript frontend (Vite, Radix UI, hls.js, framer-motion). Built assets (`web/dist/`) are embedded into the server binary via rust-embed.
- `flake.nix` - Nix flake for dev shell and builds

## Testing

- Rust: `cargo test` (unit + integration)
- Frontend: `pnpm test` (vitest) and `pnpm test:e2e` (Playwright)
- Desktop UI: `cargo build -p streamx-desktop --features ui-test && cargo build -p streamx && cargo run -p streamx-ui-harness` (add `--live` for real-poster verification). Screenshots export to `$TEST_SCREENSHOTS_DIR` (repo `.env`) as timestamped JPEG runs; macOS via `scripts/ui-test-macos.sh user@macmini`.
- E2E tests use real backend with mock streaming endpoint
- Release artifacts: `scripts/verify-release.sh` builds all Linux outputs, runs `nix flake check` (linkage policies) and the Docker container e2e (`cargo test -p streamx --test docker_static_tests -- --ignored`)
- All tests must run inside `nix develop`
- Performance metrics tracked in `benchmarks/e2e_perf.json` (git-tracked)
- After running E2E tests, serve the report for review: `python3 -m http.server 8997 -d /tmp/streamx_e2e_artifacts/html-report`
- Report port: 8997 (configurable via `STREAMX_REPORT_PORT`)
