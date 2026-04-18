#!/bin/bash
# Generate app icons from logo-white-transparent.svg for all platforms
# Usage: ./scripts/generate-icons.sh

set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
SRC="$ROOT/assets/logo-white-transparent.svg"
OUT="$ROOT/public/icons"

mkdir -p "$OUT"

echo "Generating icons from $SRC..."

# Favicon SVG (copy as-is)
cp "$ROOT/assets/logo-white-transparent.svg" "$OUT/favicon.svg"

# PNG favicons
convert -background none -density 300 "$SRC" -resize 16x16 "$OUT/icon-16.png"
convert -background none -density 300 "$SRC" -resize 32x32 "$OUT/icon-32.png"
convert -background none -density 300 "$SRC" -resize 48x48 "$OUT/icon-48.png"

# Apple touch icon (180x180 with padding on dark bg for visibility)
convert -background "#0a0a0a" -density 300 "$SRC" -resize 140x140 \
  -gravity center -extent 180x180 "$OUT/apple-touch-icon.png"

# Android Chrome icons
convert -background "#0a0a0a" -density 300 "$SRC" -resize 152x152 \
  -gravity center -extent 192x192 "$OUT/android-chrome-192x192.png"
convert -background "#0a0a0a" -density 300 "$SRC" -resize 384x384 \
  -gravity center -extent 512x512 "$OUT/android-chrome-512x512.png"

# iPad icons
convert -background "#0a0a0a" -density 300 "$SRC" -resize 120x120 \
  -gravity center -extent 152x152 "$OUT/icon-152.png"
convert -background "#0a0a0a" -density 300 "$SRC" -resize 140x140 \
  -gravity center -extent 167x167 "$OUT/icon-167.png"

# MS Tile
convert -background "#0a0a0a" -density 300 "$SRC" -resize 108x108 \
  -gravity center -extent 144x144 "$OUT/mstile-144x144.png"

# ICO (multi-size)
convert "$OUT/icon-16.png" "$OUT/icon-32.png" "$OUT/icon-48.png" "$OUT/favicon.ico"

echo "Generated icons:"
ls -la "$OUT"
