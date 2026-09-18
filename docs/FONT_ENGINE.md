# SRNG Font Engine v0.6

The font engine is isolated in `renderer/src/font.rs` and owns font discovery, face resolution, metrics, character-to-glyph mapping, and vector outline extraction.

## Scope

Implemented here:

- TrueType/OpenType parsing through `ttf-parser`.
- System font discovery and family/style/weight matching through `fontdb`.
- Explicit font-file loading.
- Deterministic requested-family fallback order.
- Units-per-em, ascent, descent, and line-gap access.
- Glyph horizontal advance and bounding boxes.
- Unicode scalar to glyph-ID mapping.
- Quadratic and cubic outline extraction without rasterization.
- Conversion of font-unit outlines into renderer SVG-path geometry.
- Missing-font and malformed-font diagnostics.
- Preservation of the previous built-in 5x7 vectors as emergency fallback only.

Not implemented here:

- HarfBuzz-class shaping.
- Ligature/substitution/positioning processing.
- Bidi.
- Complex-script shaping.

Those belong to the shaping layer.

## Public API

`srng_renderer::font` exposes four main operations required by the shaping layer:

1. `FontSystem::resolve(&FontRequest)` resolves family fallback plus requested weight/style.
2. `FontFace::metrics()` and `FontFace::glyph_metrics()` expose font and glyph metrics.
3. `FontFace::glyph_for_char()` maps Unicode scalar values to glyph IDs.
4. `FontFace::glyph_outline()` returns native vector segments preserving line, quadratic, and cubic curves.

`GlyphOutline::to_svg_path_scaled()` converts those vector segments into the renderer's path representation while retaining curves.

## Determinism and fallback

Requested families are tested in authored order. A missing requested family emits `F001`; it is not silently renamed to a different authored family. Generic CSS families (`serif`, `sans-serif`, `monospace`, `cursive`, `fantasy`) resolve through the platform font database.

If no requested face can be resolved, or if the simple non-shaping path cannot map a glyph, text rendering falls back to the historical built-in vector glyphs and marks the node with `text-fallback: "builtin-vector-emergency"`.

The fallback is intentionally not the primary rendering path.

## Renderer integration

`semantic_v7` now resolves real font outlines first for text nodes that do not already contain pre-shaped `data`. It performs only direct character-to-glyph mapping and advance placement. It does not claim to shape scripts or implement OpenType layout.

A future shaping agent should consume the font module directly and produce positioned glyph IDs/outlines. No font-loader rewrite is required for that integration.

## Errors

Malformed font bytes/files return `FontError::InvalidFont`. File-system failures return `FontError::Io`. Missing requested families are non-fatal `FontDiagnostic` values so unrelated scene rendering continues.
