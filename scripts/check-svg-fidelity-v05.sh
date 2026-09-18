#!/usr/bin/env bash
set -euo pipefail

cargo test --workspace
cargo test --manifest-path renderer/Cargo.toml --all-features
cargo check --manifest-path renderer/Cargo.toml --all-features

# Supported native v0.5 resources must not require semantic SVG reconstruction.
if grep -R --line-number -E 'radialGradient|<text|<image' renderer/src/semantic*.rs; then
  echo 'error: SVG compatibility reconstruction remains in semantic passes' >&2
  exit 1
fi

echo 'SVG fidelity v0.5 checks passed'
