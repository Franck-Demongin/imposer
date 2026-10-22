#!/usr/bin/env bash
set -euo pipefail

APP_ID="io.github.Franck-Demongin.Imposer"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN_SOURCE="${1:-$REPO_ROOT/target/release/imposer-gui}"

if [[ ! -x "$BIN_SOURCE" ]]; then
  echo "Binaire introuvable ou non exécutable: $BIN_SOURCE" >&2
  echo "Compile d'abord avec: cargo build --release --features gui --bin imposer-gui" >&2
  exit 1
fi

mkdir -p "$HOME/.local/bin"
mkdir -p "$HOME/.local/share/applications"
for size in 16 32 48 64 128 256 512; do
  mkdir -p "$HOME/.local/share/icons/hicolor/${size}x${size}/apps"
done
mkdir -p "$HOME/.local/share/icons/hicolor/scalable/apps"

install -m 0755 "$BIN_SOURCE" "$HOME/.local/bin/imposer-gui"
install -m 0644 "$REPO_ROOT/packaging/linux/$APP_ID.desktop" "$HOME/.local/share/applications/$APP_ID.desktop"
for size in 16 32 48 64 128 256 512; do
  install -m 0644 "$REPO_ROOT/assets/icons/imposer-${size}.png" "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done
install -m 0644 "$REPO_ROOT/assets/icons/imposer.svg" "$HOME/.local/share/icons/hicolor/scalable/apps/$APP_ID.svg"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$HOME/.local/share/applications" || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f "$HOME/.local/share/icons/hicolor" || true
fi

echo "Imposer installé localement pour $USER."
echo "Vérifie que $HOME/.local/bin est dans PATH."