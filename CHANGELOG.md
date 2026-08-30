# Changelog

All notable changes to StreamX. The format follows
[Keep a Changelog](https://keepachangelog.com), versions follow
[SemVer](https://semver.org).

## [0.3.5] - 2026-08-30


### Fixed

- Resume interrupted HLS transcodes instead of serving truncated cache (3dcfa678)

## [0.3.4] - 2026-08-28


### Added

- Providers_version in config.toml refreshes default providers (0a7588f6)
- Hourly update check with version badges (d6fe0b77)
- Show the version number in the update popup (72652892)

### Fixed

- Stamp the app version and refresh providers on upgrade (f66ed8e0)
- Notice pills truncate gracefully instead of overflowing (6c9a74c4)

## [0.3.3] - 2026-08-28


### Fixed

- Consistent provider health on category and search; unstick slow pill (e005bf98)

## [0.3.2] - 2026-08-28


### Added

- Provider health surfaces - split timeouts, error chains, slow/error UI on web and desktop (7ca7d9c5)

## [0.3.1] - 2026-08-27


### Other

- Merge branch 'main' into feat/windows (48e2d0f9)

## [0.3.0] - 2026-08-27


### Added

- Windows desktop (60579a25)
- Youtube trailers in native popup player; fix libmpv locale init; match web trailer icon (00d43550)
- Youtube trailer popup on linux and windows (16b55528)

### Fixed

- Fix windows homedir resolution and ci build (93e0fd4e)
- Portable signal handling and test file seeding (fb2392f8)
- Mpv JSON IPC over named pipes (08017b1d)
- Count ffmpeg via CIM in kill tests (2f5ea45d)
- Allowlist bcryptprimitives/combase; tolerate dynamic CRT in dev builds (e8643144)
- Allowlist the Windows ICU DLLs (40f3d7a8)

### Internal

- Placebo preset so the watchdog encode outlasts the idle window (8ecbc691)
- Measure watchdog idle window from start_stream (c4109fb8)
- Fetch libmpv from the master SourceForge mirror with checksum-verified retries (c4af3aef)
- Run the in-process libmpv playback test on the runner desktop (6b6a062f)
- Expect PE format when linkcheck parses its own test binary (6edcc35e)
- Retrigger after dropped workflow event (29fc4101)
- Supply env-derived browser candidates in the override test (73f04b99)
- Manual dispatch trigger for dropped pull_request events (eb1230cf)
- Cargo build cache and pinned-download cache (0fdec8b5)

### Other

- Merge remote-tracking branch 'origin/fix/libmpv' into feat/windows (68ce5be5)
- Merge pull request #4 from andreasbros/feat/windows (2f12673d)

## [0.2.6] - 2026-08-26


### Fixed

- Downloads opens player page like history; mpv edge resize; drop rebuilt movie page (703d8935)
- Classic maximize on macos so edge resize survives; downloads opens player page like history (0681682e)

### Other

- Merge branch 'main' of github.com:andreasbros/streamx (3b91a743)

## [0.2.5] - 2026-08-25


### Added

- Search UX polish - clear button, double-click select, drop stale search responses (d62c3710)

### Other

- Merge branch 'main' of github.com:andreasbros/streamx (7ce9150f)

## [0.2.4] - 2026-08-25


### Added

- Open movie page from downloads; correct app version shown in web and desktop menus (29daa729)

### Other

- Merge branch 'main' of github.com:andreasbros/streamx (b826284b)

## [0.2.3] - 2026-08-25


### Fixed

- Latest yts movie posters (a0c9af48)

### Internal

- Replace deprecated magic-nix-cache with cachix (de0ea694)
- Increase build timeout (0c725494)
- Optimise build and release pipeline (1b2719b1)

### Other

- Merge branch 'main' of github.com:andreasbros/streamx (e32a2466)

## [0.2.2] - 2026-08-25


### Internal

- Staple app before dmg, sign and staple the dmg (c17e35ac)

### Other

- Merge branch 'main' of github.com:andreasbros/streamx (797edc9c)

## [0.2.1] - 2026-08-25


### Internal

- Forward secrets to the called release workflow (309b3c16)

### Other

- Merge branch 'main' of github.com:andreasbros/streamx (ddaa8147)

## [0.2.0] - 2026-08-24


### Added

- Release pipeline (23000b04)
- First-run account creation, admin clean/wipe, embedded player fixes (close freeze, audio window, dock icon), bundled providers and app icon (c3187192)

### Internal

- Build web UI before rust job so rust-embed finds web/dist (5df906d8)
- Apple signatory (239376e6)

## [0.1.0] - 2026-08-24


### Added

- Working torrents download and video stream playback with UI and Admin pages; HLS realtime transcode is buggy; (4e4cb13c)
- Initial implementation of standalone dekstop app (3f50905f)
- Fix GUI build (29522183)
- Fix video playback (afeec2a9)
- Fix macos metal build (91189e6d)
- Update doc with nix and macos troubleshooting (ac2d649e)
- Improve config file creation on fresh startup (cc63b90b)
- Desktop app admin env vars creds (4fbb7b34)
- Feat static linking (867e7a1b)
- Static musl build (cca82403)
- Macos build; release scripts (5e428275)

### Fixed

- Fix music library (4a6f3cbe)
- Fix music all download (c33494b1)
- Fix music playlist playback and GUI posters display (73619030)
- Web app music playlists; desktop ui; desktop ui performance (679b24be)
- Downloads dir setting; macos build and tests (f431f02e)
- Desktop app and web app ui fixes (28ab042a)
- Latest movies category (0ccd6636)
- Mpv embed; transcode tests; (7a76b687)

### Documentation

- Updated readme (11fca76f)

### Internal

- Merge gitignore changes (8f49503e)
- Merge gitignore changes (a8f8fb49)
- Directory restructure; nix flake overhaul; (faa48498)
- Update rust nightly (4a4ce044)
- Release doc (d7f2f053)

### Other

- Initial commit (f1bff95c)
