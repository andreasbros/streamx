#!/usr/bin/env bash
# Run the desktop UI harness on a macOS host over SSH (recommended over
# RDP/VNC: scriptable, faster, CI-able; keep RDP for eyeballing only).
#
# Requirements on the mac:
#   - nix installed, repo checkout synced by this script
#   - a user logged into the GUI session (the app needs a display)
#   - Screen Recording permission granted to the SSH-launched terminal
#     (System Settings > Privacy & Security > Screen Recording), or the
#     screenshots come out black
#
# Usage: scripts/ui-test-macos.sh user@macmini [-- harness args...]
set -euo pipefail

HOST="${1:?usage: ui-test-macos.sh user@host [-- harness args...]}"
shift
if [[ "${1:-}" == "--" ]]; then shift; fi
REMOTE_DIR="streamx-ui-test"

echo "==> syncing working tree to $HOST:$REMOTE_DIR"
rsync -az --delete \
  --exclude target --exclude target-nix --exclude node_modules \
  --exclude web/dist --exclude .git \
  ./ "$HOST:$REMOTE_DIR/"

echo "==> building + running harness on $HOST"
ssh "$HOST" "cd $REMOTE_DIR && nix develop --command bash -c '
  set -e
  export CARGO_TARGET_DIR=target/nix
  cargo build -p streamx-desktop --features ui-test
  cargo build -p streamx
  cargo run -p streamx-ui-harness -- $*
'"

echo "==> fetching artifacts"
mkdir -p target/macos-ui-artifacts
rsync -az "$HOST:$REMOTE_DIR/target/nix/ui-test-artifacts/" target/macos-ui-artifacts/
echo "macOS artifacts in target/macos-ui-artifacts"
