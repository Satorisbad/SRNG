#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
BIN_PATH="${BIN_DIR}/srng-studio"
DESKTOP_PATH="${APP_DIR}/srng-studio.desktop"

printf 'Building SRNG Studio release binary...\n'
cargo build --release --manifest-path "$REPO_ROOT/studio/Cargo.toml"

mkdir -p "$BIN_DIR" "$APP_DIR"
install -m 0755 "$REPO_ROOT/studio/target/release/srng-studio" "$BIN_PATH"

cat > "$DESKTOP_PATH" <<EOF
[Desktop Entry]
Type=Application
Name=SRNG Studio
Comment=Convert, inspect and visually compare SVG and SRNG files
Exec=${BIN_PATH} %f
TryExec=${BIN_PATH}
Icon=applications-graphics
Terminal=false
Categories=Graphics;Development;
MimeType=image/svg+xml;
StartupNotify=true
Keywords=SVG;SRNG;Vector;Graphics;
EOF

chmod 0644 "$DESKTOP_PATH"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
fi

printf '\nInstalled SRNG Studio.\n'
printf 'Launch it from your app search as "SRNG Studio".\n'
printf 'Binary: %s\n' "$BIN_PATH"
printf 'Desktop entry: %s\n' "$DESKTOP_PATH"
