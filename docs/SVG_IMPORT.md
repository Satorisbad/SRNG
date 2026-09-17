# SVG import into SRNG

`srng-svg` is the reference SVG-to-SRNG importer. It translates SVG semantics into SRNG source and then uses the normal compiler/runtime/renderer pipeline. The importer never bypasses the SRNG scene contract.

## Conversion

```bash
cargo run --bin srng-svg -- artwork.svg -o artwork.srng
cargo run --bin srng-svg -- artwork.svg --native -o artwork-native.srng
```

`--native` removes `svg-*` provenance after native SRNG properties/resources have been emitted. `--stdout` prints the result instead of writing it. `--strict` treats remaining warnings as a failing conversion, which is useful for fidelity-sensitive jobs.

## Rendering and Studio

```bash
cargo run --manifest-path renderer/Cargo.toml --bin srng-render -- artwork.srng -o artwork.png
cargo run --manifest-path studio/Cargo.toml
```

The renderer derives its viewport from the first active SRNG canvas unless `--viewport WIDTHxHEIGHT` is supplied.

## v0.4 supported mappings

The importer/runtime/renderer stack currently covers:

- `<svg>` canvas geometry plus root `viewBox` and `preserveAspectRatio`
- `<g>` / `<symbol>` hierarchy through explicit `contains` relations
- `<rect>` including rounded corners, `<circle>`, `<ellipse>`, `<path>`, `<line>`, `<polyline>`, `<polygon>`
- transform lists: `matrix`, `translate`, `scale`, `rotate`, `skewX`, `skewY`, including inherited group transforms
- `opacity`, `fill-opacity`, and `stroke-opacity`, including inherited group opacity
- fills/strokes, fill rules, line caps/joins, dash arrays/offsets, and common SVG lengths
- native linear-gradient resources and radial-gradient rendering
- `<clipPath>` vector geometry including `objectBoundingBox` handling for supported shapes
- simple binary masks as clips and general vector alpha/luminance masks through isolated compositing
- `maskContentUnits` coordinate conversion and retained mask region/unit metadata
- repeating SVG patterns, object-bounding-box patterns, nested vector pattern content, and embedded data-image pattern content
- standalone embedded `data:image/*` raster images
- simple local `<use href="#id">` references to supported vector shapes
- imported text position/content/font-family/font-size/font-weight/font-style/text-anchor with a practical renderer fallback when outline path data is not present

The v0.4 renderer bakes viewBox and transform matrices into geometry before normal preparation so hand-authored SRNG and imported SRNG share the same render command model.

## Native resource semantics

Supported pattern and mask resources do not require `svg-*` metadata. The importer emits SRNG-native fields such as:

- `pattern-ref`, `pattern-width`, `pattern-height`, `pattern-data`
- `mask-ref`, `mask-data`, `mask-type`, mask unit/region metadata
- `clip`
- `gradient-kind`, `gradient-units`, gradient coordinates/stops
- `transform`, opacity fields
- `href`, `use-data`, `use-fill`, `use-stroke`
- text/font fields

For features currently implemented through a compatibility raster step (for example radial gradients, text, or embedded raster images), any temporary SVG fragment is reconstructed in memory from SRNG-native properties. It is not required as source-of-truth data in native SRNG files.

`--native` output is regression-tested after all `svg-*` provenance is stripped.

## Masks and clipping

Supported clip shapes become SRNG path geometry. `clipPathUnits="objectBoundingBox"` is resolved against the receiving node before rendering.

Simple opaque-white masks may be reduced to vector clips. General vector masks are rendered into an isolated layer. Luminance masks convert RGB luminance to alpha before the existing alpha compositor is applied, so gray mask values produce partial transparency rather than behaving as fully opaque alpha masks.

## Patterns

Simple vector pattern children are encoded as native vector resource records. More complex nested pattern content can retain a compatibility resource when required, and the CPU renderer rasterizes the tile locally with repeat sampling. `patternUnits="userSpaceOnUse"` and object-bounding-box tile sizing are distinguished.

## Text

Text remains a native SRNG declaration with content, position, and common font properties. When outline `data` is available, the renderer uses the normal vector path. Otherwise the practical v0.4 fallback rasterizes the text resource locally. This is intentionally not claimed to be a complete cross-platform shaping engine; font availability and complex-script shaping can still differ by environment.

## Images and `<use>`

Embedded `data:image/*` image content can render as a standalone image through the local renderer path. Network resources are never fetched. Simple `<use>` references to supported local vector shapes are expanded to native geometry and preserve the referenced fill/stroke. More complex symbol/use inheritance remains diagnostic-first rather than silently approximated.

## Fault tolerance and remaining boundaries

Conversion is diagnostic-first. Unsupported SVG semantics are preserved when possible and reported rather than silently discarded.

The major remaining boundaries after the v0.4 fidelity milestone are intentionally larger subsystems rather than missing basic scene-graph features:

- broad SVG filter graphs (`feGaussianBlur`, morphology, turbulence, lighting, complex filter composition, and similar primitives)
- production-grade font discovery/shaping/outlining with deterministic behavior across all fonts and scripts
- arbitrary external/network resource loading (intentionally disabled by the security model)
- highly advanced SVG paint-server inheritance/compositing combinations outside the tested v0.4 resource model

These boundaries do not prevent ordinary SRNG vector scenes from compiling/rendering; unsupported portions continue to produce diagnostics while unaffected content renders.

## IDs, hierarchy, and security

SVG IDs are sanitized into deterministic legal SRNG identifiers. Parent/child structure is emitted as explicit `relation parent -> child { kind: contains; }` declarations and declaration order remains paint order.

The importer parses supplied XML locally. It does not fetch network resources, external CSS, external fonts, linked SVGs, or remote images. Embedded resources already contained in the input may be rendered locally.

## Pipeline

```text
SVG
  -> srng-svg importer
  -> SRNG source
  -> compiler / SRNG-IR
  -> runtime / SRNG-SCENE
  -> semantic normalization
  -> renderer preparation
  -> CPU PNG or host-owned GPU target
```
