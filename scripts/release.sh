#!/usr/bin/env bash
# Build a distributable StreamX artifact for one target triple.
#
#   scripts/release.sh <triple> <output>
#
#   aarch64-apple-darwin      StreamX.app for Apple Silicon
#   x86_64-apple-darwin       StreamX.app for Intel Macs (built on
#                             Apple Silicon via Rosetta-backed Nix,
#                             `extra-platforms = x86_64-darwin`)
#   x86_64-unknown-linux-musl static `streamx` server (Linux host)
#   aarch64-unknown-linux-musl                 "
#
# macOS: <output> ending in .dmg produces a drag-to-Applications disk
# image, .zip a ditto-zipped app (both preserve signatures); anything
# else is the .app directory. Linux: <output> is the server binary path.
#
# Signing: ad-hoc unless CODESIGN_IDENTITY is set (see bundle-macos.sh).
# Notarization: set NOTARY_KEY_FILE (App Store Connect .p8),
# NOTARY_KEY_ID, and NOTARY_ISSUER_ID and the flow becomes the full
# ceremony: notarize + staple the .app, sign the .dmg, notarize +
# staple the .dmg. Without them those steps are skipped (local builds).
set -euo pipefail
cd "$(dirname "$0")/.."

TRIPLE="${1:?usage: scripts/release.sh <triple> <output>}"
OUT="${2:?usage: scripts/release.sh <triple> <output>}"
TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/nix}"
export CARGO_TARGET_DIR="$TARGET_DIR"

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

case "$TRIPLE" in
  *-apple-darwin) ;;
  *-unknown-linux-musl)
    if [ "$(uname -s)" != "Linux" ]; then
      echo "Linux artifacts build on a Linux host: nix build .#streamx-${TRIPLE%%-*}-linux-musl" >&2
      exit 1
    fi
    step "Static server for $TRIPLE"
    nix build ".#streamx-${TRIPLE%%-*}-linux-musl" --out-link result-release
    install -m 755 result-release/bin/streamx "$OUT"
    nix run .#streamx-linkcheck -- "$OUT" --policy static
    echo "done: $OUT"
    exit 0
    ;;
  *) echo "unsupported triple: $TRIPLE" >&2; exit 1 ;;
esac

ARCH="${TRIPLE%%-*}"
NIX_SYSTEM="${ARCH}-darwin"

step "Web UI (embedded into the binary)"
( cd web && pnpm install --silent && pnpm build > /dev/null )

step "Media dependencies for $NIX_SYSTEM (libmpv + ffmpeg)"
nix build ".#packages.${NIX_SYSTEM}.media-deps" --out-link "result-media-$ARCH"
MEDIA="$(readlink -f "result-media-$ARCH")"

step "Desktop binary for $TRIPLE"
ENV_TRIPLE="$(echo "$TRIPLE" | tr '-' '_')"
export PKG_CONFIG_ALLOW_CROSS=1
export "PKG_CONFIG_PATH_${ENV_TRIPLE}=$MEDIA/lib/pkgconfig"
export PKG_CONFIG_PATH="$MEDIA/lib/pkgconfig"
cargo build --release -p streamx-desktop --target "$TRIPLE"
BIN="$TARGET_DIR/$TRIPLE/release/streamx-desktop"

# lipo names the CPU arm64 where Rust triples say aarch64.
WANT_ARCH="$ARCH"; [ "$WANT_ARCH" = "aarch64" ] && WANT_ARCH="arm64"
GOT_ARCH="$(lipo -archs "$BIN")"
[ "$GOT_ARCH" = "$WANT_ARCH" ] || { echo "built $GOT_ARCH, expected $WANT_ARCH" >&2; exit 1; }

notarize() {
  # notarize <path>: submit and wait; hard-fails the release on
  # rejection so a bad signature can never ship.
  if [ -z "${NOTARY_KEY_FILE:-}" ]; then
    return 0
  fi
  step "Notarize $(basename "$1")"
  xcrun notarytool submit "$1" \
    --key "$NOTARY_KEY_FILE" \
    --key-id "$NOTARY_KEY_ID" \
    --issuer "$NOTARY_ISSUER_ID" \
    --wait
}

step "Bundle + verify"
# Users drag the .app into Applications, so it is always named
# StreamX.app; only the artifact filename carries the architecture.
APP_DIR="$OUT"
ZIP=""
DMG=""
case "$OUT" in
  *.zip)
    ZIP="$OUT"
    APP_DIR="$(dirname "$OUT")/release-$ARCH/StreamX.app"
    ;;
  *.dmg)
    DMG="$OUT"
    APP_DIR="$(dirname "$OUT")/release-$ARCH/StreamX.app"
    ;;
esac
cargo build -q -p streamx-linkcheck
FFMPEG_BIN="$MEDIA/bin/ffmpeg" FFPROBE_BIN="$MEDIA/bin/ffprobe" \
  LINKCHECK="$TARGET_DIR/debug/streamx-linkcheck" \
  scripts/bundle-macos.sh "$BIN" "$APP_DIR"

# Notarize and staple the app itself before it goes into any
# container, so a copied-out app validates offline on first launch.
if [ -n "${NOTARY_KEY_FILE:-}" ]; then
  APP_ZIP="$(dirname "$APP_DIR")/notarize-app.zip"
  ditto -c -k --keepParent "$APP_DIR" "$APP_ZIP"
  notarize "$APP_ZIP"
  rm -f "$APP_ZIP"
  xcrun stapler staple "$APP_DIR"
fi

if [ -n "$DMG" ]; then
  step "Disk image for distribution"
  STAGE="$(dirname "$APP_DIR")"
  ln -sfn /Applications "$STAGE/Applications"
  rm -f "$DMG"
  hdiutil create -volname "StreamX" -srcfolder "$STAGE" -fs HFS+ \
    -format UDZO -ov -quiet "$DMG"
  rm -rf "$STAGE"
  if [ "${CODESIGN_IDENTITY:--}" != "-" ]; then
    codesign --force --sign "$CODESIGN_IDENTITY" --timestamp "$DMG"
  fi
  if [ -n "${NOTARY_KEY_FILE:-}" ]; then
    notarize "$DMG"
    xcrun stapler staple "$DMG"
  fi
  echo "done: $DMG ($(du -h "$DMG" | cut -f1))"
elif [ -n "$ZIP" ]; then
  step "Zip for distribution"
  rm -f "$ZIP"
  ditto -c -k --keepParent "$APP_DIR" "$ZIP"
  rm -rf "$(dirname "$APP_DIR")"
  echo "done: $ZIP ($(du -h "$ZIP" | cut -f1))"
else
  echo "done: $APP_DIR"
fi
