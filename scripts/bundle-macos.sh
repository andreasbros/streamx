#!/usr/bin/env bash
# Package streamx-desktop as a self-contained macOS .app.
#
# Copies every non-system dylib the binary (transitively) links, such as
# libmpv and its FFmpeg closure from the Nix store, into
# Contents/Frameworks, rewrites install names to @rpath, ad-hoc signs
# the bundle, and verifies the result with streamx-linkcheck under the
# strict "macos" policy: only Apple system libraries and bundled
# @rpath dylibs may remain.
#
# FFmpeg and ffprobe ship inside the bundle too (Contents/Helpers):
# the server's transcode pipeline resolves them from there before PATH.
# Pass FFMPEG_BIN / FFPROBE_BIN, or run inside `nix develop` where they
# are on PATH.
#
# Signing: ad-hoc by default. Set CODESIGN_IDENTITY="Developer ID
# Application: ..." to sign for distribution (notarization is a
# separate `notarytool submit` step afterwards).
#
#   scripts/bundle-macos.sh [binary] [out-dir]
#   default: target/release/streamx-desktop -> dist/StreamX.app
set -euo pipefail

BIN="${1:-target/release/streamx-desktop}"
OUT="${2:-dist/StreamX.app}"
NAME="StreamX"
LINKCHECK="${LINKCHECK:-cargo run -q -p streamx-linkcheck --}"
FFMPEG_BIN="${FFMPEG_BIN:-$(command -v ffmpeg || true)}"
FFPROBE_BIN="${FFPROBE_BIN:-$(command -v ffprobe || true)}"
IDENTITY="${CODESIGN_IDENTITY:--}"

[ -x "$BIN" ] || { echo "binary not found: $BIN" >&2; exit 1; }
[ -n "$FFMPEG_BIN" ] && [ -x "$FFMPEG_BIN" ] || { echo "ffmpeg not found; set FFMPEG_BIN or run in nix develop" >&2; exit 1; }
[ -n "$FFPROBE_BIN" ] && [ -x "$FFPROBE_BIN" ] || { echo "ffprobe not found; set FFPROBE_BIN" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Frameworks" "$OUT/Contents/Resources" "$OUT/Contents/Helpers"
cp "$BIN" "$OUT/Contents/MacOS/streamx-desktop"
cp "$FFMPEG_BIN" "$OUT/Contents/Helpers/ffmpeg"
cp "$FFPROBE_BIN" "$OUT/Contents/Helpers/ffprobe"
chmod u+w "$OUT/Contents/MacOS/streamx-desktop" "$OUT/Contents/Helpers/ffmpeg" "$OUT/Contents/Helpers/ffprobe"

cat > "$OUT/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>com.streamx.desktop</string>
  <key>CFBundleExecutable</key><string>streamx-desktop</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

is_system() { case "$1" in /System/Library/*|/usr/lib/*|@rpath/*|@executable_path/*|@loader_path/*) return 0;; *) return 1;; esac; }

# Non-system dylibs referenced by a Mach-O, one per line.
foreign_deps() {
  otool -L "$1" | tail -n +2 | awk '{print $1}' | while read -r lib; do
    is_system "$lib" || echo "$lib"
  done
}

# Bundled file name for a source dylib. Distinct libraries can share a
# basename (Apple's and GNU's libiconv.2.dylib both appear in the mpv
# closure), so a collision gets the store hash as a prefix.
declare -A name_for=()   # source path -> bundled basename
declare -A base_owner=() # bundled basename -> source path
# Sets BUNDLE_NAME (no subshell: the bookkeeping arrays must persist).
bundle_name() {
  local src="$1" base
  base="$(basename "$src")"
  if [ -n "${name_for[$src]:-}" ]; then BUNDLE_NAME="${name_for[$src]}"; return; fi
  if [ -n "${base_owner[$base]:-}" ] && [ "${base_owner[$base]}" != "$src" ]; then
    local hash
    hash="$(basename "$(dirname "$(dirname "$src")")" | cut -c1-8)"
    base="${hash}-${base}"
  fi
  name_for[$src]="$base"
  base_owner[$base]="$src"
  BUNDLE_NAME="$base"
}

# Breadth-first copy of the dylib closure, rewriting references.
queue=(
  "$OUT/Contents/MacOS/streamx-desktop"
  "$OUT/Contents/Helpers/ffmpeg"
  "$OUT/Contents/Helpers/ffprobe"
)
declare -A copied=()
while [ ${#queue[@]} -gt 0 ]; do
  obj="${queue[0]}"; queue=("${queue[@]:1}")
  while read -r dep; do
    [ -n "$dep" ] || continue
    bundle_name "$dep"
    name="$BUNDLE_NAME"
    dest="$OUT/Contents/Frameworks/$name"
    if [ -z "${copied[$dep]:-}" ]; then
      copied[$dep]=1
      cp "$dep" "$dest"
      chmod u+w "$dest"
      install_name_tool -id "@rpath/$name" "$dest" 2>/dev/null
      queue+=("$dest")
    fi
    install_name_tool -change "$dep" "@rpath/$name" "$obj" 2>/dev/null
  done < <(foreign_deps "$obj")
done

install_name_tool -add_rpath "@executable_path/../Frameworks" "$OUT/Contents/MacOS/streamx-desktop" 2>/dev/null || true
for h in ffmpeg ffprobe; do
  install_name_tool -add_rpath "@loader_path/../Frameworks" "$OUT/Contents/Helpers/$h" 2>/dev/null || true
done

# Sign inside-out: every dylib and helper, then the bundle. Required on
# Apple Silicon after rewriting load commands; with CODESIGN_IDENTITY
# set this produces a distributable Developer ID signature.
for obj in "$OUT"/Contents/Frameworks/* "$OUT"/Contents/Helpers/*; do
  codesign --force --sign "$IDENTITY" "$obj" >/dev/null 2>&1
done
codesign --force --sign "$IDENTITY" "$OUT" >/dev/null

echo "bundled $(ls "$OUT/Contents/Frameworks" | wc -l | tr -d ' ') dylibs, $(du -sh "$OUT" | cut -f1) total"

# Verify: the executable and every bundled dylib must satisfy the strict policy.
status=0
for obj in "$OUT/Contents/MacOS/streamx-desktop" "$OUT"/Contents/Helpers/* "$OUT"/Contents/Frameworks/*; do
  $LINKCHECK "$obj" --policy macos >/dev/null || { echo "FAIL: $obj"; status=1; }
done
[ $status -eq 0 ] && echo "ok: $OUT is self-contained (policy macos)"
exit $status
