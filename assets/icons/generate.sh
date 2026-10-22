#!/usr/bin/env bash
set -euo pipefail

# Regeneration reference for the application icon.
# The committed PNG/ICO files make normal builds independent of these tools.
# Tools: rsvg-convert (librsvg), inkscape, or imagemagick as a fallback.

SOURCE="assets/icons/imposer.svg"
OUTPUT_DIR="assets/icons"

mkdir -p "$OUTPUT_DIR"
for SIZE in 16 32 48 64 128 256 512; do
  rsvg-convert -w "$SIZE" -h "$SIZE" "$SOURCE" -o "$OUTPUT_DIR/imposer-${SIZE}.png"
done

# Windows multi-size ICO (Pillow or ImageMagick can also produce it):
magick "$OUTPUT_DIR/imposer-256.png" \
  \( "$OUTPUT_DIR/imposer-48.png" \) \
  \( "$OUTPUT_DIR/imposer-32.png" \) \
  \( "$OUTPUT_DIR/imposer-16.png" \) \
  "$OUTPUT_DIR/imposer.ico"