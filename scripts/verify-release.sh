#!/usr/bin/env bash
# Full release verification for the Linux artifacts. Run from the repo
# root, inside or outside `nix develop`. Builds every release output,
# enforces the linkage policies, and (when a Docker daemon is
# available) boots the artifacts in stock distro containers, including
# streaming a real torrent.
#
#   scripts/verify-release.sh
#
# macOS artifacts are verified by scripts/bundle-macos.sh instead.
set -euo pipefail
cd "$(dirname "$0")/.."

# A global insteadOf=ssh git rewrite breaks Nix's git fetcher (see
# README); neutralize it for the nix invocations.
export GIT_CONFIG_GLOBAL=/dev/null

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

step "Building release artifacts"
nix build .#streamx-x86_64-linux-musl --out-link result-x86_64-musl
nix build .#streamx-aarch64-linux-musl --out-link result-aarch64-musl
nix build .#streamx-desktop-dist --out-link result-desktop-dist

step "Linkage policies + clippy + fmt (nix flake check)"
nix flake check

step "Workspace test suite"
nix develop --command bash -c \
  'CARGO_TARGET_DIR=$PWD/target/nix cargo test --workspace'

if docker info > /dev/null 2>&1; then
  step "Stock-distro container e2e (Alpine streaming, distro matrix, desktop loader)"
  nix develop --command bash -c \
    'CARGO_TARGET_DIR=$PWD/target/nix cargo test -p streamx --test docker_static_tests -- --ignored'
else
  step "SKIPPED container e2e: no Docker daemon (start Docker Desktop / colima and rerun)"
fi

step "All release checks passed"
