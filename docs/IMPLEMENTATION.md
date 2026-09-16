# SRNG v0.1 Reference Implementation Guide

This document explains how the repository implements the SRNG v0.1 format and how to reproduce the same pipeline in another language or codebase.

## Repository layers

```text
src/lexer.rs                source tokenization
src/parser.rs               fault-tolerant source parser and source validation
src/ir.rs                   SRNG-IR JSON emitter
src/runtime_v2.rs           public runtime assembly
src/runtime_v2/model.rs     SRNG-SCENE structures and runtime options
src/runtime_v2/execute.rs   IR-to-scene execution
src/runtime_v2/geometry.rs  units and geometry resolution
src/runtime_v2/resolve.rs   local/cross-file reference resolution
renderer/src/model.rs       backend-neutral render command model
renderer/src/prepare.rs     SRNG-SCENE to render-command preparation
renderer/src/cpu.rs         Vello CPU implementation
renderer/src/gpu.rs         Vello Hybrid/wgpu implementation
```

## Compiler flow

The compiler performs:

```text
UTF-8 source
    -> lexer
    -> token stream + lexical diagnostics
    -> parser
    -> AST + parser/validation diagnostics
    -> IR emitter
    -> SRNG-IR JSON
```

Compilation intentionally does not resolve cross-file references. Reference provenance must remain inspectable in IR.

The parser recovers after malformed declarations where practical so later valid declarations remain available.

## Runtime flow

The runtime performs:

```text
SRNG-IR
    -> validate runtime options
    -> collect units
    -> resolve local node geometry
    -> construct unresolved references
    -> recursively resolve reference chains
    -> detect cycles/depth failures
    -> mark relation activity
    -> SRNG-SCENE
```

Use `execute_json` when IR is already available in memory.

Use `execute_file` for `.srng` or compiled JSON files. `.srng` inputs are compiled before runtime execution. Cross-file `.srng` references follow the same behavior.

## Reference resolver invariants

A resolver implementation must maintain a stack keyed by `(normalized source path, target id)`.

Before descending into a target, test whether that key already exists in the active stack. If so, report a cycle.

Do not use only target ids for cycle detection because two files may legitimately use the same local id.

Do not globally mark a target as cyclic merely because it has been resolved before. The relevant condition is whether it appears in the current recursive chain.

Reference failures must leave the reference inactive. A failed reference must not be counted as a valid relation endpoint.

## Geometry invariants

Authored property strings remain preserved. Runtime geometry is additional derived state.

This separation matters because editors and serializers need the original authored form while renderers need resolved numeric geometry.

A resolved reference preserves both its linked geometry and its own instance geometry. The renderer overlays instance fields over linked fields rather than replacing the entire geometry structure.

The same overlay rule applies to properties.

## Paint order

Paint order is assigned while iterating source declarations. Nodes and references share the same monotonic counter.

The preparation stage merges both categories and sorts by `paint_order`. Do not render all nodes first and all references second, because doing so changes visual stacking relative to source order.

## Renderer architecture

The preparation stage is backend-neutral.

```text
SRNG-SCENE
    -> prepare_scene
    -> PreparedScene
       - dimensions
       - revision
       - Fill commands
       - Stroke commands
       - PushClip commands
       - PopClip commands
       - renderer diagnostics
    -> CPU or GPU backend
```

This shared preparation layer is required for behavioral parity between software and hardware rendering.

## Revision gate

`RevisionGate` is intended for editors and live-preview systems.

A caller increments the gate before beginning a new preparation. Preparation receives the revision number it belongs to. If a later revision becomes current, stale preparation stops producing new commands.

The gate is not part of the file format and must not affect deterministic static rendering.

## CPU backend

The CPU feature is enabled by default.

```bash
cargo check --manifest-path renderer/Cargo.toml
cargo test --manifest-path renderer/Cargo.toml
```

The CPU renderer returns raw RGBA bytes and renderer diagnostics. CI verifies that a rendered primitive produces a correctly sized non-empty pixmap.

## GPU backend

The GPU backend is optional:

```bash
cargo check --manifest-path renderer/Cargo.toml --no-default-features --features gpu
```

`GpuRenderer` owns persistent Vello Hybrid renderer/resources but does not own the host wgpu device or presentation system.

Host-side flow is:

```text
create wgpu Instance/Adapter/Device/Queue
create destination Texture + TextureView
create CommandEncoder
create GpuRenderer for target format and max dimensions
prepare SRNG scene
GpuRenderer::render_to_view(...)
finish encoder
queue.submit(...)
present if applicable
```

This design works for offscreen rendering, GUI applications, game engines, compositors, and other hosts without making SRNG depend on a specific window toolkit.

## Linux and macOS compatibility

The compiler/runtime avoids platform APIs. Filesystem behavior uses Rust standard-library paths and canonicalization.

The renderer uses Vello CPU for software execution and Vello Hybrid/wgpu for GPU execution.

CI verifies:

```text
native Ubuntu compiler/runtime tests
native macOS compiler/runtime tests
native Ubuntu CPU renderer tests
native macOS CPU renderer tests
Ubuntu GPU backend type-check
macOS GPU backend type-check
x86_64 Linux compiler type-check
AArch64 Linux compiler type-check
x86_64 macOS compiler type-check
AArch64/Apple Silicon macOS compiler type-check
```

Actual hardware GPU rendering is host-dependent and is not asserted by headless CI runners.

## Dependency policy

The root compiler/runtime crate remains lightweight and does not depend on Vello/wgpu.

The renderer is a separate crate. CPU and GPU dependencies are feature-gated.

Current renderer dependencies require Rust 1.90 or newer because of transitive dependency MSRV changes. Do not assume Vello's originally published minimum Rust version is still sufficient after unconstrained transitive updates.

## Color limitation

The current renderer parses sRGB-style hexadecimal colors and forwards them to Peniko/Vello.

It does not add a dedicated linear-light compositing or ICC/color-management layer. Therefore exact colorimetric parity is not part of the v0.1 contract.

## Testing expectations

A conforming implementation should at minimum test:

```text
valid basic compilation
fault-tolerant parsing
scientific notation
custom-unit conversion
missing explicit positions
local references
cross-file references
local reference cycles
cross-file reference cycles
reference-depth limits
broken-reference relation deactivation
reference provenance preservation
linked + instance geometry merging
paint-order preservation
solid fills
none paint
strokes
linear gradients
clipping
stale revision cancellation
CPU pixel-buffer generation
GPU backend compile/type compatibility
```

## Known v0.1 non-goals

The following are intentionally not completed format semantics:

- automatic relational layout
- native text shaping from font/content declarations
- timeline animation execution
- network reference loading
- raster image resource bindings
- opaque filter/effect stacks
- guaranteed linear-light compositing
- ICC or wide-gamut color management
- platform window creation/presentation

These features may be added by later SRNG versions without changing the core v0.1 rule that compilation, runtime resolution, and rendering are separate stages.