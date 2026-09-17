# SRNG — Claude Handoff Context

This archive is a full snapshot of the SRNG repository prepared so another coding agent can inspect the implementation directly without relying on a chat transcript.

## Snapshot identity

- Repository: `Satorisbad/SRNG`
- Handoff branch: `claude-handoff-v0.4`
- Source branch: `svg-fidelity-complete-v0.4`
- Source commit used for this handoff: `0345e9266e81ee75eda9d3261b10503b8342fe48`
- Active development PR: `#9 — feat: complete advanced SVG fidelity milestone`
- PR #9 is stacked on `svg-native-semantic-v0.3` and is intentionally **not merged**.

Treat the code in this repository as the source of truth. This document explains the intended architecture, semantics, current milestone, and known boundaries, but if anything here disagrees with the source or tests, inspect the source and tests before making changes.

## What SRNG is

SRNG is a relationship-first declarative vector graphics language, compiler, runtime, renderer, SVG importer, and desktop Studio. The project is intended as a programmable modern alternative to SVG while remaining fault-tolerant, renderer-neutral at the scene level, and readable by both humans and AI systems.

The current file-format header remains `srng 0.1;`. The implementation has advanced through multiple renderer/importer milestones while preserving that v0.1 language/runtime contract.

The broad pipeline is:

```text
SRNG source
  -> lexer/parser
  -> compiler / SRNG-IR
  -> runtime / resolved scene
  -> semantic normalization
  -> renderer preparation
  -> CPU or GPU renderer

SVG input
  -> SVG importer
  -> native SRNG source
  -> the same compiler/runtime/renderer pipeline
```

The SVG importer is not supposed to bypass the SRNG scene model. A major project goal is that imported artwork can eventually live as genuine SRNG semantics rather than opaque embedded SVG.

## Core language/runtime design

The established v0.1 semantics are:

- Fault-tolerant compilation. Broken portions produce diagnostics while unaffected content remains usable/renderable.
- Diagnostics are data, not visual error overlays.
- Positions are explicit.
- Built-in and custom units are supported, including mixed units.
- Node kinds are intentionally open-ended rather than limited to a fixed SVG-like element enum.
- Relationships are first-class declarations between IDs.
- Cross-SRNG references remain linked attachments with provenance rather than being destructively flattened.
- Local and cross-file reference chains are supported; cycles are invalid and reference depth is capped.
- Authored overrides apply on top of linked reference geometry/properties.
- Animation/state declarations are currently reference/context storage only. There is no v0.1 timeline executor.
- The runtime preserves source/provenance/unit information and paint order.
- Unsupported or broken portions should fail diagnostically rather than silently corrupting unrelated content.

Representative SRNG:

```srng
srng 0.1;
file "hero";
unit gu = 8 px;

rect card {
    position: 64px 64px;
    size: 520px 320px;
    fill: #ffffff;
}

relation card -> title {
    kind: contains;
    gap: 4gu;
}

reference logo = "./brand.srng#logo" {
    position: 460px 104px;
}
```

Important compiler/runtime files are under `src/` and `src/runtime_v2/`.

## Repository map

### Root compiler/runtime

- `src/lexer.rs` — lexer.
- `src/parser.rs` — fault-tolerant parser.
- `src/ast.rs` — language AST.
- `src/ir.rs` — compiler IR.
- `src/diagnostic.rs` — diagnostic model.
- `src/runtime.rs`, `src/runtime_v2.rs`, `src/runtime_v2/*` — scene execution, geometry/unit resolution, references, provenance, diagnostics.
- `src/main.rs` — compiler CLI.
- `src/bin/srngr.rs` — runtime CLI.

### SVG import

- `src/svg_import.rs` — original SVG import layer and preservation logic.
- `src/svg.rs` — native semantic promotion/provenance stripping layer.
- `src/svg_complete.rs`, `src/svg_final.rs`, `src/svg_v4.rs` — later fidelity/import normalization layers used by the current branch.
- `src/bin/srng-svg.rs` — SVG-to-SRNG CLI, including `--native`.
- `docs/SVG_IMPORT.md` — current supported SVG mappings and remaining boundaries.

### Renderer

- `renderer/src/model.rs` — prepared render model/commands.
- `renderer/src/prepare.rs` — core scene-to-render-command preparation.
- `renderer/src/semantic.rs` — native semantic compatibility preparation.
- `renderer/src/semantic_v4.rs` through `semantic_v7.rs` — stacked normalization passes for advanced fidelity work.
- `renderer/src/cpu.rs` — CPU rendering and local raster compatibility paths.
- `renderer/src/gpu.rs` — optional GPU backend/type-checked path.
- `renderer/src/bin/srng-render.rs` — render CLI.

`renderer/src/lib2.rs` currently wires the semantic preparation chain into the public renderer API. Do not assume the numbered semantic files are dead just because they look incremental: inspect their call chain before consolidating or deleting them.

### Studio

- `studio/src/main.rs` — egui/eframe desktop UI.
- `studio/src/pipeline.rs` — SVG conversion + SRNG render pipeline used by Studio/tests.
- `studio/install-linux.sh` — Linux desktop installation/launcher script.
- `studio/tests/native_semantics.rs` — provenance-stripped/native semantics regression.
- `studio/tests/svg_fidelity_v4.rs` — advanced SVG fidelity regression suite.

### Tests and docs

- `tests/compiler.rs`
- `tests/runtime.rs`
- `tests/runtime_regressions.rs`
- `tests/svg_import.rs`
- `renderer/tests/render_cli.rs`
- `.github/workflows/ci.yml`
- `.github/workflows/renderer.yml`
- `.github/workflows/studio.yml`
- `docs/SPEC.md`
- `docs/RUNTIME.md`
- `docs/IMPLEMENTATION.md`
- `docs/PLATFORMS.md`
- `docs/SVG_STUDIO.md`

## Native SVG-to-SRNG direction

A key recent goal was proving that imported SRNG is understandable and renderable as SRNG itself, not merely as an SVG wrapper.

`srng-svg --native` strips `svg-*` provenance after native SRNG semantics/resources are emitted where supported.

Important native properties/resources include concepts such as:

- `pattern-ref`, `pattern-width`, `pattern-height`, `pattern-data`
- `pattern-units`, `pattern-content-units`
- `mask-ref`, `mask-data`, `mask-type`
- mask unit/region metadata
- `clip`
- `gradient-kind`, gradient units/coordinates/stops
- `transform`
- opacity properties
- `href`, `use-data`, referenced fill/stroke information
- text/font/content properties

For supported vector patterns and masks, native payloads are represented as SRNG vector records instead of retaining raw `<pattern>`/`<mask>` XML in the emitted native file. Some compatibility paths temporarily reconstruct local SVG fragments in memory so the existing renderer can rasterize a resource. Those temporary fragments are implementation details; native SRNG remains the source of truth for supported cases.

Do not silently strip fallback provenance for an advanced resource unless the native representation is complete enough to preserve rendering semantics.

## v0.4 fidelity milestone completed in PR #9

The current source branch completed the following advanced fidelity targets:

- root SVG `viewBox` mapping
- `preserveAspectRatio`
- transform lists: `matrix`, `translate`, `scale`, `rotate`, `skewX`, `skewY`
- transform propagation through containment
- `opacity`, `fill-opacity`, `stroke-opacity`
- inherited/group opacity handling
- native linear gradients
- radial-gradient rendering support
- `clipPathUnits="objectBoundingBox"`
- mask content units
- luminance-mask behavior converted to alpha rather than treating RGB-gray as fully opaque source alpha
- embedded `data:image/*` rendering
- simple local `<use>` resolution
- nested pattern rendering
- deterministic built-in vector fallback text for basic Latin letters, digits, common punctuation, and a visible fallback glyph
- provenance-stripped/native SRNG regression coverage

The deterministic text fallback intentionally avoids depending on host-installed fonts. It is not a replacement for a production text shaping system.

## Renderer design notes

The renderer is split into a renderer-neutral scene plus preparation/normalization and backend execution.

The CPU path uses Vello CPU plus local `resvg`/raster compatibility in cases where a resource is still most safely represented by a reconstructed temporary SVG fragment.

The GPU path is optional and is primarily protected by compile/type-check CI in this phase.

The project does **not** currently promise exact cross-backend colorimetry, ICC/wide-gamut support, or perfect linear-light parity.

Text in v0.1 can also be represented as pre-shaped path geometry. The new fallback only supplies deterministic simple vector geometry when actual outline data is absent.

## Current verification state

At source commit `0345e9266e81ee75eda9d3261b10503b8342fe48`, the following workflows were green:

- main CI on Ubuntu and macOS
- cross-target checks for x86_64/aarch64 Linux/macOS targets covered by the workflow
- renderer CPU tests on Ubuntu and macOS
- runtime regression tests on Ubuntu and macOS
- renderer GPU type-check on Ubuntu and macOS
- Studio conversion/render pipeline on Ubuntu and macOS

The v0.4 Studio regression suite covers viewBox/transforms, inherited opacity, linear/radial gradients, luminance masks, object-bounding-box clipping, embedded images, `<use>`, text, nested patterns, and provenance-stripped native SRNG.

Run the tests again before trusting any subsequent modification.

## Useful commands

Build/test the root compiler/runtime:

```bash
cargo check
cargo test --all-targets --locked
```

Convert SVG to SRNG:

```bash
cargo run --bin srng-svg -- artwork.svg -o artwork.srng
cargo run --bin srng-svg -- artwork.svg --native -o artwork-native.srng
```

Run the runtime:

```bash
cargo run --bin srngr -- examples/basic.srng --stdout
```

Renderer:

```bash
cargo check --manifest-path renderer/Cargo.toml
cargo test --manifest-path renderer/Cargo.toml
cargo run --manifest-path renderer/Cargo.toml --bin srng-render -- artwork.srng -o artwork.png
```

GPU type-check:

```bash
cargo check --manifest-path renderer/Cargo.toml --no-default-features --features gpu
```

Studio:

```bash
cargo check --manifest-path studio/Cargo.toml
cargo test --manifest-path studio/Cargo.toml
cargo run --manifest-path studio/Cargo.toml
```

Linux Studio install:

```bash
bash studio/install-linux.sh
```

## Known larger deferred systems

The current branch deliberately does not claim complete SVG or typography support. Major later subsystems include:

- broad SVG filter graphs such as Gaussian blur, morphology, turbulence, lighting, and complex filter composition
- production-grade exact-font discovery, shaping, kerning, ligatures, outlining, and full complex-script/Unicode typography
- arbitrary external/network resource loading; network fetching is intentionally disabled by the current security model
- highly advanced paint-server inheritance/compositing combinations outside tested v0.4 cases
- deeper cleanup/refactoring of compatibility bridges after native first-class resource representations become mature enough

When extending fidelity, prefer explicit diagnostics/fallback preservation over silently approximating unsupported input.

## Branch/PR history relevant to this snapshot

The recent stacked development sequence is:

1. `runtime-v0.1`
2. `renderer-v0.1` — PR #3
3. `svg-import-v0.1` — PR #4
4. `svg-studio-v0.1` — PR #5
5. `svg-fidelity-v0.2` — PR #6
6. `studio-ux-v0.2` — PR #7
7. `svg-native-semantic-v0.3` — PR #8
8. `svg-fidelity-complete-v0.4` — PR #9
9. `claude-handoff-v0.4` — packaging branch created from the completed PR #9 head, with only this handoff document added

Do not merge any PR or branch merely because this archive exists. Merging is a separate user decision.

## Coding expectations for the next agent

If asked to continue implementation:

1. Inspect the current source and relevant tests before changing behavior.
2. Preserve fault tolerance and diagnostic-first semantics.
3. Do not replace native SRNG semantics with opaque SVG payloads as a shortcut.
4. Keep unsupported imported features honest: preserve enough source/provenance to avoid silent visual corruption where native conversion is incomplete.
5. Keep memory/resource usage reasonable; the primary development machine is an 8 GB Apple Silicon MacBook Air running Asahi/Omarchy Linux as well as macOS.
6. Prefer free/open-source dependencies.
7. Add regression tests for behavior changes and keep Linux/macOS portability.
8. Do not merge PRs unless explicitly instructed by the user.

## Suggested first reading order

For fast orientation, read in this order:

1. `README.md`
2. `CLAUDE_CONTEXT.md`
3. `docs/SPEC.md`
4. `docs/RUNTIME.md`
5. `docs/SVG_IMPORT.md`
6. `src/lib.rs`, `src/parser.rs`, `src/runtime_v2/*`
7. `src/svg.rs`, `src/svg_v4.rs`, `src/svg_complete.rs`, `src/svg_final.rs`, `src/svg_import.rs`
8. `renderer/src/lib2.rs`
9. `renderer/src/semantic.rs`, `semantic_v4.rs` ... `semantic_v7.rs`
10. `renderer/src/prepare.rs`, `renderer/src/cpu.rs`, `renderer/src/gpu.rs`
11. `studio/src/pipeline.rs`, `studio/src/main.rs`
12. all tests, especially `studio/tests/svg_fidelity_v4.rs` and `studio/tests/native_semantics.rs`

After reading those, use the tests to verify your mental model instead of relying only on this handoff note.
