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
# -> dist/StreamX-aarch64.dmg  dist/StreamX-x86_64.dmg
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

- Linux job: `streamx-x86_64-linux-musl`, `streamx-aarch64-linux-musl`,
  `streamx-desktop-x86_64-linux.tar.gz`, plus `nix flake check`
- macOS job: both dmgs (rebuilt on the runner)

```bash
gh run watch                 # or: gh run list --workflow release
gh release view vX.Y.Z      # confirm all 5 artifacts are attached
```

## 6. Tell users how to install (release notes template)

macOS: download the dmg for your chip (Apple Silicon = aarch64,
Intel = x86_64), drag StreamX.app to Applications. First launch:
approve once under System Settings > Privacy & Security > "Open
Anyway" (the app is not yet notarized).

Linux server: download `streamx-<arch>-linux-musl`, `chmod +x`, run.
Works on any distro, fully static.

## If something went wrong

- CI job failed: fix, push to main, then re-run the job from the GitHub
  UI or `gh run rerun <id>`. The release and tag already exist; jobs
  only upload artifacts. In a pinch a locally built dmg can be attached
  with `gh release upload vX.Y.Z dist/StreamX-*.dmg`, replaced by the
  CI build once the job is green.
- Bad release entirely: `gh release delete vX.Y.Z`,
  `git push origin :refs/tags/vX.Y.Z`, `git tag -d vX.Y.Z`, revert the
  release commit, fix, release again with the same version.
- Never rewrite a tag that users may have downloaded from.

## Later (when the Apple Developer cert exists)

Set `CODESIGN_IDENTITY="Developer ID Application: ..."` for the macOS
builds and add notarization (`xcrun notarytool submit` + staple) after
bundling; the "Open Anyway" step then disappears for users.
