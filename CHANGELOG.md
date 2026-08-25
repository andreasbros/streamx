# Changelog

All notable changes to StreamX. The format follows
[Keep a Changelog](https://keepachangelog.com), versions follow
[SemVer](https://semver.org).

## [0.2.3] - 2026-08-25


### Fixed

- Latest yts movie posters (a0c9af48)

### Internal

- Replace deprecated magic-nix-cache with cachix (de0ea694)
- Increase build timeout (0c725494)

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
