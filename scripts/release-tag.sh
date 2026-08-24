#!/usr/bin/env bash
# Cut a release: bump the SemVer version, regenerate CHANGELOG.md from
# Conventional Commits (git-cliff), commit, tag, push, and create the
# GitHub release. Binaries are attached by the tag-triggered GitHub
# Actions workflow (.github/workflows/release.yml); any local artifacts
# in dist/ are uploaded too.
#
#   nix run .#release -- patch|minor|major|X.Y.Z [--dry-run]
#
# --dry-run: compute everything and print what would happen; no writes
# to git or GitHub.
set -euo pipefail

[ -f Cargo.toml ] && grep -q '^\[workspace\.package\]' Cargo.toml \
  || { echo "run from the streamx repo root" >&2; exit 1; }

LEVEL="${1:?usage: release-tag.sh patch|minor|major|X.Y.Z [--dry-run]}"
DRY=0
[ "${2:-}" = "--dry-run" ] && DRY=1

CURRENT="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
IFS=. read -r MAJ MIN PAT <<< "$CURRENT"
case "$LEVEL" in
  major) NEXT="$((MAJ + 1)).0.0" ;;
  minor) NEXT="$MAJ.$((MIN + 1)).0" ;;
  patch) NEXT="$MAJ.$MIN.$((PAT + 1))" ;;
  [0-9]*.[0-9]*.[0-9]*) NEXT="$LEVEL" ;;
  *) echo "unknown level: $LEVEL" >&2; exit 1 ;;
esac
TAG="v$NEXT"

echo "release: $CURRENT -> $NEXT ($TAG)"

git rev-parse "$TAG" >/dev/null 2>&1 && { echo "tag $TAG already exists" >&2; exit 1; }

if [ "$DRY" = 1 ]; then
  echo "-- dry run: release notes preview --"
  git-cliff --unreleased --tag "$TAG" --strip all
  echo "-- dry run: no checks enforced, nothing written --"
  exit 0
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
[ "$BRANCH" = "main" ] || { echo "releases are cut from main (on $BRANCH)" >&2; exit 1; }
git diff --quiet && git diff --cached --quiet \
  || { echo "working tree not clean; commit or stash first" >&2; exit 1; }
git fetch origin main --quiet
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] \
  || { echo "main is not in sync with origin/main; push or pull first" >&2; exit 1; }

gh auth status >/dev/null

# Version bump: workspace source of truth + lockfile entries.
sed -i '' -e "0,/^version = \"$CURRENT\"/s//version = \"$NEXT\"/" Cargo.toml 2>/dev/null \
  || sed -i -e "0,/^version = \"$CURRENT\"/s//version = \"$NEXT\"/" Cargo.toml
cargo update --workspace --quiet

git-cliff --tag "$TAG" -o CHANGELOG.md

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore(release): $TAG"
git tag -a "$TAG" -m "StreamX $TAG"
git push origin main "$TAG"

# The release with notes; CI attaches the platform binaries on the tag
# event. Local artifacts (macOS dmgs) are uploaded when present.
git-cliff --latest --strip all > /tmp/streamx-release-notes.md
gh release create "$TAG" --title "StreamX $NEXT" --notes-file /tmp/streamx-release-notes.md
if ls dist/*.dmg >/dev/null 2>&1; then
  gh release upload "$TAG" dist/*.dmg --clobber
fi

echo "released $TAG"
