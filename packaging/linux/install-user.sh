#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
MIME_DIR="${HOME}/.local/share/mime/packages"

mkdir -p "$BIN_DIR" "$APP_DIR" "$MIME_DIR"

cargo build --release --manifest-path "$ROOT/studio/Cargo.toml"
install -m755 "$ROOT/studio/target/release/srng-studio" "$BIN_DIR/srng-studio"
install -m644 "$ROOT/packaging/linux/srng-studio.desktop" "$APP_DIR/srng-studio.desktop"
install -m644 "$ROOT/packaging/linux/srng.xml" "$MIME_DIR/srng.xml"

if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database "${HOME}/.local/share/mime"
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR"
fi
if command -v xdg-mime >/dev/null 2>&1; then
  xdg-mime default srng-studio.desktop application/x-srng
fi

printf 'Installed SRNG Studio to %s\n' "$BIN_DIR/srng-studio"
printf 'Registered *.srng as application/x-srng\n'
