#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:-$ROOT/dist}"
APP="$OUT/SRNG Studio.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"

cargo build --release --manifest-path "$ROOT/studio/Cargo.toml"
rm -rf "$APP"
mkdir -p "$MACOS" "$RESOURCES"
install -m755 "$ROOT/studio/target/release/srng-studio" "$MACOS/srng-studio"
install -m644 "$ROOT/packaging/macos/Info.plist" "$CONTENTS/Info.plist"
install -m644 "$ROOT/packaging/macos/srng-studio.icns" "$RESOURCES/srng-studio.icns"

printf 'Created %s\n' "$APP"
printf 'Signing/notarization intentionally not performed.\n'
