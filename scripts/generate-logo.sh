#!/usr/bin/env bash
#
# generate-logo.sh - Generate PNG icons and favicon.ico from the SVG logo.
# Requires ImageMagick (convert / magick).
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ICONS_DIR="$PROJECT_ROOT/ui/public/icons"
SVG_LOGO="$ICONS_DIR/logo.svg"
SVG_FAVICON="$ICONS_DIR/favicon.svg"

# Detect ImageMagick command (v7 uses "magick", v6 uses "convert")
if command -v magick &>/dev/null; then
  CONVERT="magick"
elif command -v convert &>/dev/null; then
  CONVERT="convert"
else
  echo "Error: ImageMagick is not installed. Please install it first." >&2
  exit 1
fi

echo "Using ImageMagick command: $CONVERT"
echo "Output directory: $ICONS_DIR"
echo ""

# Generate PNG icons at various sizes
SIZES=(16 32 48 64 128 256 512)
for size in "${SIZES[@]}"; do
  echo "Generating icon-${size}.png ..."
  $CONVERT -background none -density 384 "$SVG_LOGO" -resize "${size}x${size}" \
    "$ICONS_DIR/icon-${size}.png"
done

# Generate favicon.ico (multi-size: 16, 32, 48)
echo "Generating favicon.ico ..."
$CONVERT "$ICONS_DIR/icon-16.png" "$ICONS_DIR/icon-32.png" "$ICONS_DIR/icon-48.png" \
  "$ICONS_DIR/favicon.ico"

# Generate apple-touch-icon.png (180x180 with opaque dark background)
echo "Generating apple-touch-icon.png (180x180 with #0a0a0a background) ..."
$CONVERT -background "#0a0a0a" -density 384 "$SVG_LOGO" -resize 180x180 \
  -gravity center -extent 180x180 \
  "$ICONS_DIR/apple-touch-icon.png"

echo ""
echo "Done! Generated files:"
ls -lh "$ICONS_DIR"
