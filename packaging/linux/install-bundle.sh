#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
DATA_HOME="${XDG_DATA_HOME:-${HOME}/.local/share}"
APP_DIR="${DATA_HOME}/applications"
MIME_DIR="${DATA_HOME}/mime/packages"
APP_ICON_DIR="${DATA_HOME}/icons/hicolor/scalable/apps"
MIME_ICON_DIR="${DATA_HOME}/icons/hicolor/scalable/mimetypes"

for required in \
  "$ROOT/bin/srng-studio" \
  "$ROOT/share/applications/srng-studio.desktop" \
  "$ROOT/share/mime/packages/srng.xml" \
  "$ROOT/share/icons/hicolor/scalable/apps/srng-studio.svg" \
  "$ROOT/share/icons/hicolor/scalable/mimetypes/application-x-srng.svg"
do
  if [[ ! -f "$required" ]]; then
    printf 'SRNG Studio bundle is incomplete: missing %s\n' "$required" >&2
    exit 1
  fi
done

mkdir -p "$BIN_DIR" "$APP_DIR" "$MIME_DIR" "$APP_ICON_DIR" "$MIME_ICON_DIR"
install -m755 "$ROOT/bin/srng-studio" "$BIN_DIR/srng-studio"
install -m644 "$ROOT/share/applications/srng-studio.desktop" "$APP_DIR/srng-studio.desktop"
install -m644 "$ROOT/share/mime/packages/srng.xml" "$MIME_DIR/srng.xml"
install -m644 "$ROOT/share/icons/hicolor/scalable/apps/srng-studio.svg" "$APP_ICON_DIR/srng-studio.svg"
install -m644 "$ROOT/share/icons/hicolor/scalable/mimetypes/application-x-srng.svg" "$MIME_ICON_DIR/application-x-srng.svg"

if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database "${DATA_HOME}/mime"
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR"
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "${DATA_HOME}/icons/hicolor" >/dev/null 2>&1 || true
fi
if command -v xdg-mime >/dev/null 2>&1; then
  xdg-mime default srng-studio.desktop application/x-srng
fi

printf 'Installed SRNG Studio to %s\n' "$BIN_DIR/srng-studio"
printf 'Registered *.srng as application/x-srng\n'
