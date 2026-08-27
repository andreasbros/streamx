# Releasing StreamX

One page, in order. Everything runs from the repo root on the Mac.

## One-time setup

`gh` and `git-cliff` ship in the dev shell; nothing to install.

```bash
nix develop
gh auth login          # one-time; stored in ~/.config/gh
grep extra-platforms /etc/nix/nix.conf
#   extra-platforms = x86_64-darwin   (required for Intel builds; needs Rosetta)
```

## 1. Verify the tree

```bash
nix develop
export CARGO_TARGET_DIR=$PWD/target/nix
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All commits on main must use Conventional Commit messages
(`feat: ...`, `fix: ...`, `chore: ...`); they become the changelog.

## 2. Build and smoke-test local artifacts (optional but recommended)

```bash
nix run .#build-all
# -> dist/streamx-desktop-aarch64-macos.dmg  dist/streamx-desktop-x86_64-macos.dmg
```

Mount one dmg, drag StreamX.app somewhere, launch it, play a movie.
First launch of the Intel app on this Mac is slow (Rosetta translation).

## 3. Preview the release

```bash
nix run .#release -- patch --dry-run     # or minor / major / X.Y.Z
```

Read the generated notes. Wrong grouping means a commit message needs
amending before you tag.

## 4. Cut the release

Two equivalent ways; pick one.

**From the GitHub website (no local tooling):** Actions > cut-release >
"Run workflow". Enter a SemVer (`0.2.0`) or a bump level
(`patch`/`minor`/`major`), optionally a commit SHA to release an older
commit, and run. GitHub bumps the version, writes `CHANGELOG.md`,
commits, tags, fast-forwards `main` when possible, creates the release,
and builds every artifact. The GitHub mobile app can trigger it too.

**From this machine:** requires `main` checked out, clean, and pushed.

```bash
nix run .#release -- patch
```

This bumps the workspace version, regenerates `CHANGELOG.md`, commits
`chore(release): vX.Y.Z`, tags, pushes, and creates the GitHub release
with the notes. All binaries come from CI (next step); local `dist/`
artifacts are for your own testing and are never published.

## 5. Watch CI attach the rest

The tag triggers `.github/workflows/release.yml`:

- Three parallel Linux jobs (x86_64 server, aarch64 server on a native
  ARM runner, desktop tarball), each gated by its linkage check
- macOS job: both dmgs, signed and notarized
- Windows jobs: x86_64 and arm64 zips on native runners (static MSVC
  CRT; libmpv-2.dll and ffmpeg bundled, pinned by sha256), each gated
  by the windows-dist linkage check

```bash
gh run watch                 # or: gh run list --workflow release
gh release view vX.Y.Z      # confirm all 7 artifacts are attached
```

## 6. Tell users how to install (release notes template)

macOS: download the dmg for your chip (Apple Silicon = aarch64,
Intel = x86_64), drag StreamX.app to Applications, launch.

Windows: download `streamx-desktop-<arch>-windows.zip` (arm64 for
Windows on ARM, x86_64 otherwise), unzip, run `streamx-desktop.exe`.
Everything the app needs is in the folder.

Linux server: download `streamx-server-<arch>-linux-musl`, `chmod +x`, run.
Works on any distro, fully static.

Linux desktop: download `streamx-desktop-x86_64-linux.tar.gz`, unpack,
run `bin/streamx-desktop` (glibc 2.39+ desktop distros).

## Build cache (one-time setup)

CI substitutes unchanged Nix derivations (cross toolchains, static
FFmpeg, crate deps) from a Cachix binary cache; without it every run
compiles cold (~1.5h Linux, ~1h macOS). Free for open source.

1. https://app.cachix.org > sign in with GitHub > create a cache named
   `streamx` (public).
2. Cache settings > Auth tokens > generate a write token.
3. GitHub secret `CACHIX_AUTH_TOKEN` = that token.

First run after setup is still cold (it populates the cache); later
runs drop to roughly 15-25 minutes, dominated by the code that actually
changed. Without the secret, CI still pulls from the public cache and
just skips pushing.

## If something went wrong

- CI job failed: fix, push to main, then re-run the job from the GitHub
  UI or `gh run rerun <id>`. The release and tag already exist; jobs
  only upload artifacts. In a pinch a locally built dmg can be attached
  with `gh release upload vX.Y.Z dist/streamx-desktop-*-macos.dmg`, replaced by the
  CI build once the job is green.
- Bad release entirely: `gh release delete vX.Y.Z`,
  `git push origin :refs/tags/vX.Y.Z`, `git tag -d vX.Y.Z`, revert the
  release commit, fix, release again with the same version.
- Never rewrite a tag that users may have downloaded from.

## Apple signing + notarization (one-time setup)

Removes the "Open Anyway" step for users. CI does the signing; you only
provision credentials once. All secrets go to GitHub: repo > Settings >
Secrets and variables > Actions > New repository secret.

1. Join the Apple Developer Program ($99/year):
   https://developer.apple.com/programs/enroll - use your Apple ID with
   two-factor auth. Wait for the enrollment email.

2. Create the "Developer ID Application" certificate (on this Mac):
   - Keychain Access > Certificate Assistant > Request a Certificate
     From a Certificate Authority: your email, "Saved to disk". This
     writes a `.certSigningRequest` and puts the private key in your
     keychain.
   - https://developer.apple.com/account/resources/certificates >
     "+" > Developer ID Application > upload the request > download the
     `.cer` > double-click to install it into Keychain Access.

3. Export the certificate for CI:
   - Keychain Access > My Certificates > "Developer ID Application:
     <name> (<TEAMID>)" > right-click > Export > `.p12` with a strong
     password.
   - Secrets:
     - `MACOS_CERT_P12`: `base64 -i cert.p12 | pbcopy` and paste
     - `MACOS_CERT_PASSWORD`: the export password

4. Create an App Store Connect API key (for notarization):
   - https://appstoreconnect.apple.com > Users and Access >
     Integrations > App Store Connect API > Team Keys > "+", role
     Developer. Download the `.p8` (single chance).
   - Secrets:
     - `APP_STORE_CONNECT_KEY`: `base64 -i AuthKey_XXXX.p8 | pbcopy`
     - `APP_STORE_CONNECT_KEY_ID`: the Key ID shown next to the key
     - `APP_STORE_CONNECT_ISSUER_ID`: shown at the top of the page

5. Done. The release workflow detects the secrets automatically: it
   imports the certificate into a throwaway keychain, signs with
   hardened runtime + timestamp, then per architecture notarizes and
   staples the .app itself, signs the .dmg, and notarizes and staples
   the .dmg too - so both the disk image and an app copied out of it
   validate offline. Without the secrets it falls back to ad-hoc
   signing exactly as before.

6. Verify on the next release: download a dmg and run
   `spctl -a -t open --context context:primary-signature -v streamx-desktop-aarch64-macos.dmg`
   (expect "accepted"), or just open the app - no Gatekeeper prompt.

Local signed builds work too:
`CODESIGN_IDENTITY="Developer ID Application: <name> (<TEAMID>)" nix run .#build-all`
(notarization stays a CI concern).
