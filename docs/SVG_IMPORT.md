# SVG import into SRNG

`srng-svg` is the reference SVG-to-SRNG importer. It translates SVG semantics into SRNG source and then uses the normal compiler/runtime/renderer pipeline. The importer never bypasses the SRNG scene contract.

## Conversion

```bash
cargo run --bin srng-svg -- artwork.svg -o artwork.srng
cargo run --bin srng-svg -- artwork.svg --native -o artwork-native.srng
```

`--native` removes `svg-*` provenance after native SRNG properties/resources have been emitted. `--stdout` prints the result instead of writing it. `--strict` treats remaining warnings as a failing conversion.

## Rendering and Studio

```bash
cargo run --manifest-path renderer/Cargo.toml --bin srng-render -- artwork.srng -o artwork.png
cargo run --manifest-path studio/Cargo.toml
```

The renderer derives its viewport from the first active SRNG canvas unless `--viewport WIDTHxHEIGHT` is supplied.

## Native mappings

The importer/runtime/renderer stack covers:

- `<svg>` canvas geometry, root `viewBox`, and `preserveAspectRatio`
- `<g>` / `<symbol>` hierarchy through explicit `contains` relations
- `<rect>` including rounded corners, `<circle>`, `<ellipse>`, `<path>`, `<line>`, `<polyline>`, `<polygon>`
- transform lists: `matrix`, `translate`, `scale`, `rotate`, `skewX`, `skewY`, including inherited group transforms
- `opacity`, `fill-opacity`, and `stroke-opacity`
- fills/strokes, fill rules, line caps/joins, dash arrays/offsets, and common SVG lengths
- native linear and radial gradients, including `gradientUnits`, `gradientTransform`, `spreadMethod`, focal point/radius, stop opacity, and href inheritance
- `<clipPath>` geometry including `objectBoundingBox` handling for supported shapes
- native vector masks through isolated alpha/luminance compositing
- native repeating vector patterns using SRNG vector records, with legacy v0.4 pattern provenance readable only as backward compatibility
- standalone embedded `data:image/png` and `data:image/svg+xml` images on the CPU renderer, including meet/slice/none aspect-ratio behavior
- local `<use href="#id">` references to supported vector shapes, including nested aliases and cycle-safe local resolution
- native CPU filter graphs for `feGaussianBlur`, `feOffset`, `feBlend`, `feComposite`, `feColorMatrix`, `feFlood`, `feMerge`, `feMorphology`, and practical `feComponentTransfer`
- imported text content and common font metadata, with deterministic vector fallback including anchors, size, weight, italic/oblique, letter spacing, word spacing, tabs, line height, and multiline layout

## First-class resource semantics

Supported features are lowered into normal SRNG fields and renderer command types rather than reconstructing source SVG in semantic passes. Important native fields include:

- `pattern-ref`, `pattern-width`, `pattern-height`, `pattern-data`
- `mask-ref`, `mask-data`, `mask-type`, mask unit/region metadata
- `clip`
- `gradient-kind`, `gradient-units`, `gradient-transform`, `gradient-spread`, gradient coordinates, focal radius, and stops
- `image-data`, image placement, and `image-preserve-aspect-ratio`
- `filter-graph`, `filter-units`, `filter-primitive-units`, and filter region fields
- legacy `filter-chain`, accepted as v0.5 compatibility input and lowered into the same graph model
- `href`, `use-data`, `use-fill`, `use-stroke`
- text/font/spacing fields

`svg-*` properties are provenance. `--native` output removes them. Verification rejects semantic code that reintroduces temporary `<gradient>`, `<text>`, `<image>`, `<mask>`, `<pattern>`, or `<filter>` reconstruction for supported native features.

## Masks and clipping

Supported clip shapes become SRNG path geometry. `clipPathUnits="objectBoundingBox"` is resolved against the receiving node before rendering. Vector masks use native `paint|path` records and isolated compositing. Luminance masks convert RGB luminance to alpha before compositing.

Legacy masks that exist only as preserved v0.4 SVG provenance are diagnosed rather than silently executed through hidden SVG reconstruction.

## Patterns

Supported vector pattern children are native vector records and repeat through the renderer's image sampling path. `patternUnits="userSpaceOnUse"` and object-bounding-box tile sizing are distinct. Complex legacy v0.4 patterns may still use the explicitly marked compatibility reader; new native output does not require it.

## Text and fonts

Text remains semantic SRNG content. Pre-shaped outline `data` is rendered exactly as normal vector geometry. When outlines are absent, v0.5 uses a deterministic built-in vector fallback for Latin letters, digits, common punctuation, and a visible unsupported-character glyph.

The fallback handles common layout metadata but is not a production font shaper. Exact authored fonts, OpenType shaping, kerning, ligatures, bidi layout, variable fonts, and full complex-script Unicode typography require the separate text subsystem.

## Images and `<use>`

Embedded PNG and SVG data images render through native image commands on the CPU renderer. Network resources are never fetched. Unsupported image codecs produce diagnostics.

Local `<use>` references to supported vector shapes are expanded into native geometry. Nested local aliases are resolved recursively with cycle protection. Multi-style group/symbol expansion remains diagnostic-first because flattening those into one paint would be incorrect.

## Filters

v0.6 uses a backend-neutral `FilterGraph`. Every primitive has an explicit typed operation and a named output. SVG `result="name"`, `in`, and `in2` become native graph references. `SourceGraphic` is the isolated RGBA rendering of the filtered element; `SourceAlpha` preserves only that source alpha.

The CPU backend executes Gaussian blur, offset, normal/multiply/screen/darken/lighten blend modes, Porter-Duff composite modes plus arithmetic composite, 4x5 color matrices, flood, merge, morphology, and identity/table/discrete/linear/gamma component-transfer functions. SVG color-matrix `saturate`, `hueRotate`, and `luminanceToAlpha` are lowered into native matrices by the importer.

Filter `x`, `y`, `width`, `height`, and `filterUnits` are retained and resolved to a clipping region. Unsupported primitives remain explicit nodes and produce `S360`/`G241` diagnostics. CPU execution explicitly bypasses the unsupported node by forwarding its declared input, preserving named-result routing and unrelated scene rendering rather than silently inventing an effect.

See `docs/FILTER_GRAPH_V06.md` for the native graph grammar and execution contract.

## CPU/GPU behavior

CPU is the reference backend for the v0.6 filter graph. The graph representation is deliberately independent of CPU rendering so a GPU implementation can consume the same nodes later. GPU filter parity is not required for the v0.6 filter milestone.

## Fault tolerance and remaining boundaries

Conversion is diagnostic-first. Unsupported SVG semantics are preserved when possible and reported rather than discarded.

Remaining boundaries include:

- production-grade font discovery/shaping/outlining across fonts and scripts
- GPU texture/offscreen parity for masks, patterns, images, and filters
- filter primitives not yet implemented, including turbulence, displacement, convolve-matrix, lighting, tile, and image-based filter inputs
- additional embedded raster codecs beyond PNG
- arbitrary external/network resource loading, intentionally disabled by the security model
- the most advanced SVG compositing and paint-server combinations outside the tested native resource model

These boundaries do not prevent unaffected SRNG content from compiling and rendering.

## IDs, hierarchy, and security

SVG IDs are sanitized into deterministic legal SRNG identifiers. Parent/child structure is emitted as explicit `relation parent -> child { kind: contains; }` declarations and declaration order remains paint order.

The importer parses supplied XML locally. It does not fetch network resources, external CSS, external fonts, linked SVGs, or remote images. Embedded resources already contained in the input may be rendered locally.

## Pipeline

```text
SVG
  -> srng-svg importer
  -> native SRNG source/resources
  -> compiler / SRNG-IR
  -> runtime / SRNG-SCENE
  -> semantic normalization
  -> renderer preparation
  -> CPU PNG or host-owned GPU target
```
