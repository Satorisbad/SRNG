# SRNG native filter graph v0.6

SRNG v0.6 represents supported SVG filters as a native graph. Filter execution does not reconstruct SVG XML.

## Representation

A renderable declaration may contain `filter-graph` plus filter-region metadata:

```srng
filter-graph: "feGaussianBlur(in=SourceAlpha sigma_x=2 sigma_y=2)->blur; feOffset(in=blur dx=3 dy=2)->offset; feMerge(offset SourceGraphic)->final";
filter-units: "userSpaceOnUse";
filter-x: "0";
filter-y: "0";
filter-width: "128";
filter-height: "128";
```

Each statement creates one `FilterNode`. `->name` is the native equivalent of SVG `result="name"`. `in=name` and `in2=name` reference earlier results. If the first input is omitted, the first primitive reads `SourceGraphic`; later primitives read the immediately previous result.

`SourceGraphic` is the isolated RGBA rendering of the filtered element. `SourceAlpha` contains the same alpha channel with RGB cleared to zero.

## CPU primitives

The CPU reference backend supports:

- `feGaussianBlur`
- `feOffset`
- `feBlend`: normal, multiply, screen, darken, lighten
- `feComposite`: over, in, out, atop, xor, arithmetic
- `feColorMatrix`: 4x5 matrices; SVG matrix, saturate, hueRotate, and luminanceToAlpha are lowered to this form by the importer
- `feFlood`
- `feMerge`
- `feMorphology`: erode and dilate
- `feComponentTransfer`: identity, table, discrete, linear, and gamma channel functions

The graph model is backend-neutral. CPU evaluation lives in `renderer/src/filter_cpu.rs`, separate from generic scene command execution, so a GPU backend can consume the same `FilterGraph` later.

## Regions and clipping

The importer retains `filterUnits`, `x`, `y`, `width`, and `height` as native SRNG properties. Preparation resolves these values to a `FilterRegion`; the CPU evaluator clears pixels outside that region before compositing the filtered layer back into the scene.

The default SVG filter region is `-10% -10% 120% 120%` with `filterUnits="objectBoundingBox"`.

## Unsupported primitives

Unsupported primitives remain explicit graph nodes. Import emits diagnostic `S360`; renderer preparation emits `G241`. CPU execution bypasses that node by forwarding its declared input to its named result. This is deliberate fault isolation, not a silent approximation: downstream graph references remain valid and unrelated scene rendering is not discarded.

## Compatibility

The v0.5 native `filter-chain` syntax remains readable. A chain such as `blur(2 3); offset(4 -1)` lowers into the same `FilterGraph` representation used by v0.6.

GPU filter execution is not required by v0.6; CPU is the reference backend for this subsystem.
