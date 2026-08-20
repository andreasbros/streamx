# StreamX UI harness

Playwright-style tests for the GPUI desktop app. The app (built with
`--features ui-test`) exposes a localhost JSON driver; the harness
launches a hermetic backend + the app, injects synthetic keystrokes and
clicks through GPUI's real event dispatch, asserts on app state, and
captures per-scenario screenshots compared against `baselines/` with a
cross-OS tolerance (the UI renders identically on Linux and macOS; only
font rasterization differs, covered by the diff tolerance).

## Run (Linux, inside `nix develop`)

```bash
export CARGO_TARGET_DIR=target/nix
cargo build -p streamx-desktop --features ui-test
cargo build -p streamx
cargo run -p streamx-ui-harness                     # hermetic scenarios
cargo run -p streamx-ui-harness -- --live           # real config: verifies posters render
cargo run -p streamx-ui-harness -- --update-baselines
```

Artifacts (`report.json` + PNGs) land in `$CARGO_TARGET_DIR/ui-test-artifacts`.

## Run (macOS, over SSH)

```bash
scripts/ui-test-macos.sh user@macmini
```

SSH is the recommended transport (scriptable, fast, CI-able). The mac
needs a logged-in GUI session and Screen Recording permission for the
SSH-launched process; use RDP/VNC only for manually watching a run.

## Scenarios

- `01-home` — boots authed to the home page
- `02-search-typing` — `/` focuses search, typed keys run a live search
- `03-search-clear` — escape + backspace restores the browse view
- `04-downloads` — a pinned download appears in the Downloads queue
- `05-back-navigation` — header back walks history
- `06-settings` — settings page renders
- `07-live-posters` (`--live`) — home tiles show real posters: zero
  poster failures and a colorfulness check proving images actually drew

Windows: the capture backend is stubbed (`capture.rs`); the driver and
scenarios are OS-independent and will work once a capture is added.
