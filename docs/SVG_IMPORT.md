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

## Current exact mappings

The importer currently maps these SVG structures directly:

- `<svg>` -> `canvas`
- `<g>` and `<symbol>` -> `group` plus `contains` relations
- `<rect>` -> `rect`
- `<circle>` -> `circle`
- `<ellipse>` -> `ellipse`
- `<path>` -> SRNG `path` with the original SVG path data
- `<line>` -> SRNG path data
- `<polyline>` / `<polygon>` -> SRNG path data
- `<text>` / `<tspan>` -> SRNG text declarations with content retained

It resolves common SVG lengths (`px`, `pt`, `mm`, `cm`, `in`, `%`, or unitless SVG user units) to pixel geometry at 96 DPI. Presentation attributes and inline `style` declarations are merged with SVG inheritance for the currently supported paint/stroke fields.

The importer normalizes supported paints including hexadecimal colors, `none`, a small core named-color set, and integer `rgb(r,g,b)` values. The SRNG renderer remains the authority for final paint behavior.

## Loss prevention

Conversion is deliberately diagnostic-first. Unsupported SVG semantics are not silently discarded.

When `preserve_source_attributes` is enabled (the default), original SVG attributes are stored as `svg-attr-*` SRNG properties. Unsupported elements are represented by non-rendering metadata nodes containing `svg-source-xml`. Non-rendering SVG subtrees such as `<defs>` are likewise preserved as metadata instead of accidentally painting their definition children.

The importer emits `Sxxx` diagnostics for fidelity gaps. Important current gaps include:

- root `viewBox` transformations when the viewBox does not directly match the viewport
- element `transform`
- rounded rectangle radii
- opacity / fill-opacity / stroke-opacity
- masks, filters and clip paths
- SVG paint servers such as `url(#gradient)`
- `<image>` raster content
- `<use>` expansion/reference conversion
- text shaping/outlining

The source data for these cases is retained so later importer revisions can upgrade the conversion without requiring the original author to reconstruct lost information.

## Text

SRNG v0.1 stores imported text and its position, but the current renderer requires text to be pre-shaped into outline path data. Therefore text import emits `S301`. This is a known renderer/import boundary, not silent data loss.

## IDs and hierarchy

SVG IDs are sanitized into legal SRNG identifiers and made unique deterministically. SVG parent/child structure is emitted as explicit `relation parent -> child { kind: contains; }` declarations. Rendering order remains declaration order.

## Security model

The importer parses the supplied XML locally. It does not fetch network resources. External images, CSS, fonts and linked SVG resources are not downloaded by the importer. This keeps SVG import deterministic and avoids hidden network access from untrusted input.

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
