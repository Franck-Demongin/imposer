#!/usr/bin/env bash
set -euo pipefail

APP_ID="io.github.Franck-Demongin.Imposer"

rm -f "$HOME/.local/bin/imposer-gui"
rm -f "$HOME/.local/share/applications/$APP_ID.desktop"
for size in 16 32 48 64 128 256 512; do
  rm -f "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done
rm -f "$HOME/.local/share/icons/hicolor/scalable/apps/$APP_ID.svg"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$HOME/.local/share/applications" || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f "$HOME/.local/share/icons/hicolor" || true
fi

echo "Imposer désinstallé localement."