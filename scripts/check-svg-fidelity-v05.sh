#!/usr/bin/env bash
set -euo pipefail

cargo test --workspace
cargo test --manifest-path renderer/Cargo.toml --all-features
cargo check --manifest-path renderer/Cargo.toml --all-features

# Supported native v0.5 resources must not be rebuilt into temporary SVG in semantic passes.
if grep -R --line-number -E 'radialGradient|linearGradient|<text|<image|<mask|<pattern|<filter' renderer/src/semantic*.rs; then
  echo 'error: SVG compatibility reconstruction remains in semantic passes' >&2
  exit 1
fi

# The native command model is required for the completed v0.5 resource set.
grep -q 'PushMask' renderer/src/model.rs
grep -q 'PushFilter' renderer/src/model.rs
grep -q 'DrawImage' renderer/src/model.rs
grep -q 'RadialGradient' renderer/src/model.rs
grep -q 'Pattern {' renderer/src/model.rs

# Native importer promotion must exist for the corresponding SVG features.
grep -q 'filter-chain' src/svg_v4.rs
grep -q 'image-data' src/svg_v4.rs
grep -q 'gradient-spread' src/svg_v4.rs
grep -q 'gradient-transform' src/svg_v4.rs

echo 'SVG fidelity v0.5 checks passed'
