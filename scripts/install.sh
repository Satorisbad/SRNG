#!/bin/sh
set -eu

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: Rust/Cargo is required." >&2
  echo "On Omarchy/Arch: sudo pacman -S rustup && rustup default stable" >&2
  echo "On macOS: install Rust with rustup, then rerun this script." >&2
  exit 1
fi

cargo install --path . --locked

echo "Installed srngc to: $(command -v srngc 2>/dev/null || printf '%s/.cargo/bin/srngc' "$HOME")"
