# SVG Fidelity v0.5

This milestone removes the remaining basic-SVG compatibility reconstruction from the renderer-facing SRNG model and makes resource behavior explicit and testable.

## Scope

- native radial-gradient paint commands
- native mask resources for vector masks that can be expressed as SRNG paths
- native repeating pattern records without requiring preserved SVG XML for supported vector tiles
- stronger `<use>` reference resolution and diagnostics
- image resource normalization and diagnostics
- deterministic text fallback retained as a last resort, with explicit font metadata and shaping boundaries
- runtime/tooling diagnostics that distinguish unsupported semantics from malformed SRNG

## Compatibility principle

Preserved `svg-*` fields are provenance only. A native SRNG file must not require preserved SVG XML to render any feature marked as supported by this milestone. Unsupported SVG constructs stay diagnostic-first rather than silently degrading.

## Resource model

Gradient resources are represented as SRNG properties and lowered directly into renderer paint commands. Pattern and mask resources use native path/paint records when their contents are supported. Raster image payloads remain data resources and do not rely on rebuilding source SVG markup.

## Text

Text keeps semantic fields (`content`, `font-family`, `font-size`, `font-weight`, `font-style`, `text-anchor`). Exact font shaping/outlining is a separate subsystem; when no pre-shaped outline is present, the renderer uses deterministic built-in vector fallback geometry. This fallback is not advertised as authored-font fidelity.

## Security

External network resource loading remains disabled. Local fragment references and embedded data resources are allowed. Unsupported external references must produce diagnostics instead of implicit I/O.
