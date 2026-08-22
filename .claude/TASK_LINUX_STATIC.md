# Task: self-contained Linux builds (static server, static-linked desktop)

Handover from the Claude session that ran on the macOS dev machine
(Apple Silicon Mac mini) on 2026-08-21/23. You are a Claude agent on a
**Linux host**. Everything below was verified on macOS; your job is the
Linux half, which could not be validated there.

Read `README.md` first (sections "Modes", "Build", "Self-contained
release artifacts") and `CLAUDE.md` (code standards: no `unwrap`/
`expect`/`panic!` outside tests, `snafu` errors, no em-dashes, lean
docs, everything inside `nix develop`, `CARGO_TARGET_DIR=target/nix`).
Never commit; leave changes in the working tree for the user.

## What already exists (do not redo)

- `crates/linkcheck`: library + `streamx-linkcheck` CLI that parses
  ELF/Mach-O and enforces a per-platform linkage policy:
  - `static`: no `PT_INTERP`, no `DT_NEEDED` (musl server)
  - `linux-desktop`: only allowlisted host sonames (libc, libm, libdl,
    libpthread, libgcc_s, ld-linux, libvulkan, libwayland-client,
    libwayland-egl, libxkbcommon, libX11, libX11-xcb, libxcb, libXcursor,
    libXi, libXrandr, libfontconfig, libfreetype, libdbus-1, libva,
    libdrm, libEGL, libGL). Everything else (mpv, FFmpeg, sqlite, ssl,
    libass, ...) must be static. See `linux_desktop_allowlist()`.
  - `macos` / `macos-dev`: not your concern.
  - `policy_for_current_target()` picks `static` on musl and
    `linux-desktop` on glibc.
- Integration tests `crates/server/tests/static_link.rs` and
  `crates/desktop/tests/static_link.rs` check the real built binaries
  (`CARGO_BIN_EXE_*`) against the policy. `STREAMX_LINKCHECK_BIN=<path>`
  makes them check an arbitrary artifact (use it for cross-built
  musl binaries the host cannot execute).
- `rust-toolchain.toml` has `x86_64/aarch64-unknown-linux-musl`.
- `flake.nix` (Linux-only outputs, **never built yet**):
  - `packages.streamx-x86_64-linux-musl`, `packages.streamx-aarch64-linux-musl`
    via `mkServer { crossPkgs = pkgs.pkgsCross.musl64 / aarch64-multiplatform-musl; static = true; }`
    (`CARGO_BUILD_TARGET`, `+crt-static`, cross `cc` as linker).
  - `packages.streamx-desktop` (glibc) via `craneLib.buildPackage`.
  - `checks.linkage-server-x86_64-musl`, `linkage-server-aarch64-musl`,
    `linkage-desktop`, `linkage-server` that run the CLI on each artifact.
  - `srcWithWeb` includes `web/dist` (rust-embed reads it at compile
    time): run `cd web && pnpm install && pnpm build` before `nix build`.
- Desktop playback is **in-process libmpv** (`crates/desktop/src/playback/embedded.rs`,
  crate `libmpv2`). `crates/desktop/build.rs` links it with pkg-config;
  `STREAMX_MPV_STATIC=1` asks pkg-config for static libs
  (`pkg-config --static --libs mpv`). If libmpv cannot initialize at
  runtime the app falls back to spawning an `mpv` executable.
- macOS packaging: `scripts/bundle-macos.sh` (bundles libmpv's dylib
  closure into `StreamX.app`, verified). Not relevant on Linux, but it
  is the model for "everything the binary needs ships with it".

## What you need to implement

### 1. Static `streamx` server on musl (highest value, should be close)

1. `cd web && pnpm install && pnpm build && cd ..`
2. `nix build .#streamx-x86_64-linux-musl` (and `aarch64` via
   `pkgsCross`, or natively if your host is aarch64).
3. Expect the first run to surface cross-compile details: the `cc`
   crate needs `CC_x86_64_unknown_linux_musl`, `aws-lc-sys` (rustls)
   needs cmake + a working cross C compiler, `libsqlite3-sys`
   (bundled) compiles C. Fix env/toolchain wiring in `mkServer`, not in
   the crates.
4. `nix build .#checks.x86_64-linux.linkage-server-x86_64-musl` must pass.
   Also run the binary: `./result/bin/streamx --help` and
   `file result/bin/streamx` must say "statically linked".
5. `STREAMX_LINKCHECK_BIN=result/bin/streamx cargo test -p streamx --test static_link`
   from the dev shell.

### 2. Linux desktop with static libmpv + FFmpeg

Goal: `streamx-desktop` on glibc that passes `--policy linux-desktop`:
dynamic only against the allowlist, with libmpv, FFmpeg, libass,
libplacebo, etc. linked statically.

1. Provide static libmpv and its dependency closure through Nix.
   Candidates: `pkgs.pkgsStatic.mpv-unwrapped` (check it evaluates and
   builds; FFmpeg static is the heavy part), or an overlay that builds
   mpv with `-Ddefault_library=static` and FFmpeg with
   `--enable-static --disable-shared`. Keep it a Nix expression in
   `flake.nix` (a `staticMpv` let-binding), not a script.
2. Wire `packages.streamx-desktop` to build with
   `STREAMX_MPV_STATIC=1` and `PKG_CONFIG_PATH` pointing at the static
   libmpv's `mpv.pc`. `crates/desktop/build.rs` already emits the
   `--static` link lines from pkg-config; if `mpv.pc`'s `Libs.private`
   is incomplete, fix it in the Nix derivation (that is where such
   bugs belong), not with hardcoded `-l` flags in build.rs.
3. `nix build .#checks.x86_64-linux.linkage-desktop` must pass. Inspect
   with `ldd result/bin/streamx-desktop`: only allowlisted sonames.
   If a legitimately required host library is missing from the
   allowlist (for example a Vulkan or Wayland companion lib), add it to
   `linux_desktop_allowlist()` in `crates/linkcheck/src/lib.rs` with a
   one-line reason; do not add mpv/FFmpeg/codec libraries.
4. Run the GPUI probe to prove libmpv creates its window in-process on
   Linux (X11 and, if available, Wayland):
   `STREAMX_TEST_CLIP=/path/clip.mp4 cargo run -p streamx-desktop --example mpv_window_probe`
   must print `vo_configured=true`. Generate a clip with
   `ffmpeg -f lavfi -i testsrc=duration=20:size=640x360:rate=25 -f lavfi -i sine=frequency=440:duration=20 -c:v libx264 -c:a aac clip.mp4`.
5. `STREAMX_TEST_CLIP=... cargo test -p streamx-desktop --test embedded_mpv -- --ignored`
   must pass.
6. `cargo test -p streamx-desktop --test static_link` must pass in the
   dev shell (glibc policy).

### 3. FFmpeg as a runtime dependency of the server (decide, then do)

The server spawns an `ffmpeg` executable for HLS transcoding
(`crates/server/src/transcode/pipeline.rs`). A static `streamx` still
needs ffmpeg on the host. Preferred: embed a static `ffmpeg` binary
(from `pkgsStatic.ffmpeg` or the static closure from step 2) with
`include_bytes!` behind a cargo feature, extracted once to the data
dir's `cache/bin/` with a hash check, and resolved before PATH. Keep
the existing PATH lookup as fallback. Linking libav* directly is a
rewrite of the pipeline; do not start that.

Verify: start the static server with an empty environment
(`env -i HOME=$HOME STREAMX_DATA_DIR=/tmp/x ./result/bin/streamx`),
seed a download with the test helpers in `crates/server/tests/
stream_e2e_tests.rs`, and confirm a playlist request produces segments
(the `hls_pipeline_tests` cover the transcode path).

## Tests you must leave green

```bash
nix develop
export CARGO_TARGET_DIR=$PWD/target/nix
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
nix flake check          # includes the linkage checks for Linux outputs
```

Fixture note: `ffmpeg_kill_tests::watchdog_kills_idle` deliberately runs
a `veryslow` single-thread encode for ~40s; that is expected.

## Known pitfalls from the macOS side

- Flakes only see git-tracked files. If `nix build` cannot find a
  crate, the user has not `git add`ed it; tell them, do not work around it.
- `CARGO_TARGET_DIR` must be absolute (relative paths resolve against
  whatever `cd` happened earlier in the same shell).
- The Nix Rust toolchain can leak store dylibs into binaries through
  its own link search path (on macOS it was `libiconv`). On Linux watch
  for `libgcc_s`/`libstdc++` from the store in `ldd` output; `musl`
  builds must show none at all.
- Two different libraries can share a soname/basename in one closure
  (seen with libiconv). When bundling or static-linking, key on the
  full path, never on the basename.
- Sandbox builds of the whole workspace consume 30+ GB of store space
  per iteration; run `nix-collect-garbage` between attempts if disk is tight.

## Deliverables

- Working `nix build` for the three Linux outputs and green
  `nix flake check` on Linux.
- Any Nix/flake changes, allowlist additions (with reasons), and the
  FFmpeg embedding behind a feature flag.
- Update `README.md` "Self-contained release artifacts" with the Linux
  commands that actually worked and `file`/`ldd` evidence.
- Leave all changes uncommitted and summarize what you changed, what
  you verified, and anything still open.
