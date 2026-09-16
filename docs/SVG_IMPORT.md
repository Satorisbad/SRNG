# SVG import into SRNG

`srng-svg` is the reference SVG-to-SRNG importer. It is intentionally separate from the SRNG compiler: the importer translates SVG semantics into SRNG source, then the normal compiler/runtime/renderer pipeline handles that SRNG exactly like hand-authored SRNG.

## Basic conversion

```bash
cargo run --bin srng-svg -- artwork.svg -o artwork.srng
```

To print generated SRNG instead of writing a file:

```bash
cargo run --bin srng-svg -- artwork.svg --stdout
```

`--strict` makes warnings fail the command. This is useful for fidelity-sensitive conversion jobs because unsupported SVG features can never be missed silently.

```bash
cargo run --bin srng-svg -- artwork.svg --strict
```

## Visual validation

The CPU renderer includes a PNG output command. This gives a simple end-to-end check on macOS or Linux:

```bash
cargo run --bin srng-svg -- artwork.svg -o artwork.srng
cargo run --manifest-path renderer/Cargo.toml --bin srng-render -- artwork.srng -o artwork-srng.png
```

Open `artwork.svg` and `artwork-srng.png` side by side. The PNG command automatically uses the first active SRNG `canvas` size as its viewport unless `--viewport WIDTHxHEIGHT` is supplied.

The native Studio application provides the same pipeline visually:

```bash
cargo run --manifest-path studio/Cargo.toml
```

## Current mappings

The importer currently maps these SVG structures directly:

- `<svg>` -> `canvas`
- `<g>` and `<symbol>` -> `group` plus `contains` relations
- `<rect>` -> `rect`; `rx`/`ry` rectangles are baked into curved path geometry
- `<circle>` -> `circle`
- `<ellipse>` -> `ellipse`
- `<path>` -> SRNG `path` with the original SVG path data
- `<line>` -> SRNG path data
- `<polyline>` / `<polygon>` -> SRNG path data
- `<text>` / `<tspan>` -> SRNG text declarations with content retained
- `<clipPath>` -> SRNG clip geometry for supported vector children
- simple opaque-white `<mask>` geometry -> SRNG clip geometry
- general SVG masks -> preserved SVG mask resources composited by the CPU renderer
- SVG `<pattern>` fills -> repeating SVG tile paint, including Figma-style embedded data-image patterns

Pattern rendering distinguishes `patternUnits="userSpaceOnUse"` from SVG's default `objectBoundingBox` units. Object-bounding-box tile sizes are resolved against the geometry receiving the paint instead of being treated as one-pixel user-space tiles.

It resolves common SVG lengths (`px`, `pt`, `mm`, `cm`, `in`, `%`, or unitless SVG user units) to pixel geometry at 96 DPI. Presentation attributes and inline `style` declarations are merged with SVG inheritance for the currently supported paint/stroke fields.

The importer normalizes supported paints including hexadecimal colors, `none`, a small core named-color set, integer `rgb(r,g,b)` values, and the supported pattern paint-server path. The SRNG renderer remains the authority for final paint behavior.

## Masks and clipping

Supported clip paths are converted into SRNG clip nodes and applied by the renderer. Simple opaque masks use the same vector clipping path when that conversion is lossless.

Masks that need alpha/luminance semantics are retained through `<defs>` and evaluated by the CPU renderer. Masked content is first rendered into an isolated offscreen RGBA layer, the SVG mask is rasterized, its alpha is applied to the isolated layer, and the result is composited back into the scene. This avoids relying on backend-specific native mask-layer behavior.

## Pattern fills

Pattern definitions remain preserved in SRNG metadata for provenance. During CPU preparation they are reconstructed as an SVG tile, rasterized locally, and painted with repeat sampling. This supports vector tiles and embedded `data:` raster images referenced from `<defs>`.

The implementation currently handles the common Figma export shape where a pattern defaults to `objectBoundingBox`, has `width="1" height="1"`, and references an embedded image through `<use>`.

## Loss prevention

Conversion is deliberately diagnostic-first. Unsupported SVG semantics are not silently discarded.

When `preserve_source_attributes` is enabled (the default), original SVG attributes are stored as `svg-attr-*` SRNG properties. Unsupported elements are represented by non-rendering metadata nodes containing `svg-source-xml`. Non-rendering SVG subtrees such as `<defs>` are likewise preserved as metadata instead of accidentally painting their definition children.

The importer emits `Sxxx` diagnostics for fidelity gaps. Important remaining gaps include:

- root `viewBox` transformations when the viewBox does not directly match the viewport
- general element `transform`
- opacity / fill-opacity / stroke-opacity outside the SVG mask/pattern resource path
- filters
- SVG gradient paint servers imported through `url(#gradient)`
- standalone `<image>` raster content
- `<use>` expansion/reference conversion outside preserved SVG resources
- advanced clip-path unit/transform combinations
- text shaping/outlining

Pattern and complex-mask capability diagnostics are informational in the current public SVG facade rather than stale unsupported-feature warnings.

The source data for remaining unsupported cases is retained so later importer revisions can upgrade the conversion without requiring the original author to reconstruct lost information.

## Text

SRNG v0.1 stores imported text and its position, but the current renderer requires text to be pre-shaped into outline path data. Therefore text import emits `S301`. This is a known renderer/import boundary, not silent data loss.

## IDs and hierarchy

SVG IDs are sanitized into legal SRNG identifiers and made unique deterministically. SVG parent/child structure is emitted as explicit `relation parent -> child { kind: contains; }` declarations. Rendering order remains declaration order.

## Security model

The importer parses the supplied XML locally. It does not fetch network resources. External images, CSS, fonts and linked SVG resources are not downloaded by the importer. Embedded data images already present in the SVG may be rendered as part of a preserved pattern resource. This keeps import deterministic and avoids hidden network access from untrusted input.

## Intended pipeline

```text
SVG
  -> srng-svg importer
  -> SRNG source
  -> srng compiler / SRNG-IR
  -> runtime / SRNG-SCENE
  -> renderer preparation
  -> CPU PNG or host-owned GPU target
```

The importer is an input adapter. It does not bypass or duplicate the SRNG compiler/runtime contract.
