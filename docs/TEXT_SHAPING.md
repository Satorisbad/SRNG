# SRNG Text Shaping Architecture

Branch: `text-shaping-v0.6`

## Scope

This subsystem owns Unicode text analysis, run segmentation, bidirectional layout semantics, shaping requests, glyph clusters, advances/offsets, spacing, multiline layout metadata, and diagnostics.

It intentionally does **not** own:

- filesystem or system font discovery;
- TrueType/OpenType parsing;
- glyph outline extraction;
- rasterization;
- renderer paint/filter/image behavior;
- SVG text reconstruction.

Those boundaries allow `font-engine-v0.6` to plug into shaping without creating a second font loader.

## Pipeline

```text
SRNG text + TextStyle
        |
        v
Unicode/script/direction analysis
        |
        v
logical text runs
        |
        v
visual run ordering
        |
        v
FontProvider::select_face
        |
        v
FontProvider::shape_run
        |
        v
FontShapingResult
        |
        v
SRNG ShapedText / GlyphRun / ShapedGlyph
        |
        v
font outline extraction + native SRNG renderer
```

## Public data model

`TextStyle` preserves the text semantics that the renderer/importer already needs to carry:

- font families;
- font size;
- font weight;
- font style;
- text anchor;
- letter spacing;
- word spacing;
- line height;
- language;
- optional explicit direction.

`ShapedText` is renderer-independent. It contains:

- original UTF-8 source text;
- visual glyph runs;
- line source ranges and line/run ranges;
- shaping diagnostics.

`GlyphRun` records:

- source byte range;
- direction;
- script;
- language;
- selected face handle;
- shaped glyph list;
- total advance;
- run-local diagnostics.

`ShapedGlyph` records:

- backend glyph ID;
- UTF-8 cluster byte offset into the original source;
- source byte range represented by the cluster;
- x/y advances;
- x/y offsets.

No API assumes one Unicode scalar maps to one glyph, or that a glyph maps to one character. Multiple glyphs may share a cluster, and one glyph may represent multiple source scalars.

## Font provider contract

`FontProvider` is deliberately narrow:

```rust
fn select_face(&self, request: &FontRequest, text: &str)
    -> Result<FontFaceRef, FontProviderError>;

fn shape_run(&self, request: ShapeRunRequest<'_>)
    -> Result<FontShapingResult, FontProviderError>;
```

A real provider should perform OpenType shaping using its selected font face and return glyph IDs, UTF-8 cluster offsets, advances, and offsets. The shaping layer does not parse font files itself.

This is the intended integration point for the parallel font engine.

## Unicode and bidi behavior

The standalone layer currently performs deterministic script/direction run segmentation for representative Latin, Hebrew, Arabic, and Devanagari ranges, preserves inherited/common characters with neighboring runs, determines paragraph direction from the first strong character unless direction is forced, and performs run-level visual reordering for RTL paragraphs.

The API is structured so the internal analyzer can later be replaced with a full Unicode Script/Bidi implementation without changing renderer-facing types.

Complex shaping itself is delegated to the provider, where OpenType GSUB/GPOS, kerning, ligatures, mark positioning, contextual substitutions, and script/language features belong.

## Spacing and multiline semantics

Letter and word spacing are applied after provider shaping to returned advances. Provider offsets are retained exactly. Line height is preserved per `ShapedLine`, and explicit newlines produce stable source ranges and run ranges.

Text anchoring remains part of `TextStyle`; final anchor-origin adjustment belongs to layout/render integration because it depends on the containing SRNG text object position.

## Fallback and diagnostics

If the provider cannot select a face or cannot shape a run, shaping emits a diagnostic and creates a Unicode-preserving fallback run. This fallback is a continuity mechanism only; it is not a replacement for real OpenType shaping.

Diagnostics currently include:

- `TS001`: no matching font face;
- `TS002`: missing shaping data;
- `TS003`: unsupported script;
- `TS004`: backend error.

## Tests

Unit tests cover:

- Latin multi-character ligature cluster mapping (`office` with a synthetic `ffi` glyph);
- combining-mark zero advance and positional offsets;
- RTL paragraph/run behavior with Hebrew;
- Arabic RTL shaping requests and byte clusters;
- Devanagari complex-script detection;
- multiline source ranges and explicit line height;
- post-shaping letter/word spacing;
- missing-font diagnostics without loss of Unicode source mapping.

The mock provider intentionally simulates backend shaping rather than loading real fonts, keeping this branch independent of `font-engine-v0.6`.
