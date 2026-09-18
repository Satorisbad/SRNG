#!/usr/bin/env bash
set -euo pipefail

cargo test --workspace
cargo test --manifest-path renderer/Cargo.toml
cargo check --manifest-path renderer/Cargo.toml

# The native graph must remain renderer-native and CPU execution must stay separated.
grep -q 'pub struct FilterGraph' renderer/src/filter.rs
grep -q 'SourceGraphic' renderer/src/filter.rs
grep -q 'SourceAlpha' renderer/src/filter.rs
grep -q 'execute_filter_graph' renderer/src/filter_cpu.rs
grep -q 'PushFilter { graph:' renderer/src/model.rs

# Supported v0.6 primitives must be represented explicitly.
for primitive in GaussianBlur Offset Blend Composite ColorMatrix Flood Merge Morphology ComponentTransfer; do
  grep -q "$primitive" renderer/src/filter.rs
  grep -q "$primitive" renderer/src/filter_cpu.rs
done

# Import must produce the native graph and retain region semantics.
grep -q 'filter-graph' src/svg_filter_v06.rs
grep -q 'filter-units' src/svg_filter_v06.rs
grep -q 'filter-primitive-units' src/svg_filter_v06.rs

# Filter-specific code must not reconstruct SVG XML.
if grep -R --line-number -E '<filter|<fe[A-Z]' renderer/src/filter.rs renderer/src/filter_cpu.rs src/svg_filter_v06.rs; then
  echo 'error: SVG XML reconstruction found in v0.6 filter-specific code' >&2
  exit 1
fi

echo 'Filter graph v0.6 checks passed'
