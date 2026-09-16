# SRNG v0.1 Format Specification

This document is the implementation contract for SRNG v0.1. It defines the accepted source syntax, compiler output, runtime scene model, reference semantics, units, rendering properties, diagnostics, compatibility requirements, and known limitations.

An implementation conforming to this document should be able to parse, compile, resolve, execute, and render SRNG v0.1 without requiring undocumented assumptions.

## 1. Purpose

SRNG is a relationship-first vector graphics format. Objects are declared independently and relationships between them are represented explicitly. Source nesting is not used as an implicit scene graph.

The v0.1 pipeline has three boundaries:

1. `.srng` source text.
2. `SRNG-IR` JSON emitted by the compiler.
3. `SRNG-SCENE` produced by the runtime after geometry/unit/reference resolution.

The renderer consumes `SRNG-SCENE` rather than reparsing source text.

## 2. Source encoding

Source files are UTF-8 text.

Whitespace is insignificant except inside strings. `//` begins a line comment extending to the next newline.

Strings use double quotes. The compiler supports escaped newline, tab, carriage return, quote, and backslash characters.

Identifiers begin with an ASCII letter, `_`, or `%`. Subsequent characters may also contain digits, `-`, `.`, and `/`.

Numbers support integers, decimals, negative values, and scientific notation such as `1e2`, `2.5e-3`, and `-4E6`.

Hexadecimal colors are lexed beginning with `#` followed by hexadecimal digits.

## 3. Grammar

```text
document      := "srng" scalar ";" declaration*
declaration   := file | unit | node | relation | reference | animation
file          := "file" scalar ";"
unit          := "unit" ident "=" number ident ";"
node          := ["node"] ident ident block
relation      := "relation" ident "->" ident block
reference     := "reference" ident "=" scalar (block | ";")
animation     := "animation" ident block
block         := "{" property* "}"
property      := ident ":" value ";"
```

`scalar` is an identifier, number, string, or color token.

Property values are retained by the compiler as normalized strings. Their meaning is interpreted by the runtime or renderer according to the property name.

## 4. Header

Every file begins with:

```srng
srng 0.1;
```

A missing header produces compiler diagnostic `E101`.

The version string is copied into both `SRNG-IR` and `SRNG-SCENE`.

## 5. File identifier

A file may contain:

```srng
file "hero";
```

The file identifier is metadata. It does not replace filesystem paths for cross-file reference resolution.

## 6. Units

### 6.1 Built-in units

SRNG v0.1 defines:

- `px`
- `pt`
- `in`
- `cm`
- `mm`
- `%`
- `vw`
- `vh`

Runtime conversion uses the configured viewport width, viewport height, and DPI.

Conversions are:

```text
1 px = 1 runtime pixel
1 pt = dpi / 72 pixels
1 in = dpi pixels
1 cm = dpi / 2.54 pixels
1 mm = dpi / 25.4 pixels
1 vw = viewport_width / 100
1 vh = viewport_height / 100
```

For `%`, the horizontal viewport dimension is used for horizontal quantities and the vertical viewport dimension for vertical quantities.

### 6.2 Custom units

```srng
unit gu = 8 px;
```

A custom unit recursively resolves through its base unit. Cyclic custom-unit definitions are runtime errors.

The runtime preserves all custom unit definitions in `SRNG-SCENE.unit_context`.

### 6.3 Unitless lengths

A numeric geometry value without an explicit unit is interpreted as pixels by the runtime.

## 7. Nodes

The canonical form is:

```srng
rect card {
    position: 64px 64px;
    size: 520px 320px;
    fill: #ffffff;
}
```

The explicit form is also valid:

```srng
node rect card {
    position: 64px 64px;
}
```

The first identifier is the node kind; the second is the node id.

Node kinds are open-ended at compiler level. Unknown kinds remain valid declarations so later renderer versions or external implementations may define additional categories.

The reference renderer currently understands rectangular geometry for `rect`, `canvas`, `group`, and `shadow`, elliptical geometry for `ellipse` and `circle`, and arbitrary path geometry through the `data` property.

## 8. Required geometry

All executable nodes require an explicit `position` property.

```srng
position: 20px 40px;
```

`position` contains exactly two lengths: x then y.

Nodes lacking position receive compiler warning `W201`; at runtime they become inactive and receive `R201`.

Optional rectangular size is:

```srng
size: 300px 120px;
```

`size` contains width then height. Negative dimensions are invalid.

Runtime-resolved geometry is stored separately from authored property strings.

## 9. Paths

Arbitrary vector outlines use `data` containing SVG-compatible path syntax understood by Kurbo/Vello:

```srng
path logo {
    position: 0px 0px;
    data: "M 0 0 L 100 0 L 50 80 Z";
    fill: #ffffff;
}
```

When `data` is present and non-empty, it takes precedence over generated primitive geometry.

Text is not shaped by the SRNG v0.1 renderer. A text node intended for direct rendering must provide pre-shaped outline path data in `data`. Otherwise renderer diagnostic `G301` is emitted.

## 10. Relationships

Relationships are explicit:

```srng
relation card -> title {
    kind: contains;
    gap: 8px;
}
```

A relation contains:

- source id (`from`)
- target id (`to`)
- optional `kind`
- arbitrary additional properties

Compiler validation requires both relation endpoints to be declared locally. Missing endpoints produce `E210` or `E211`.

The runtime additionally marks a relation active only when both endpoints are active. This is important when one endpoint is a broken reference or invalid node. An inactive relation produces `R210`.

Relations describe semantic/spatial meaning. The v0.1 reference renderer does not automatically perform layout from relations.

## 11. References

References link one declaration to another without flattening or copying the referenced source into the compiler IR.

```srng
reference logo = "./brand.srng#logo" {
    position: 460px 104px;
}
```

A target is split at the final `#` into:

```text
source_file = ./brand.srng
target_id   = logo
```

A local reference uses:

```srng
reference copy = ".#original";
```

### 11.1 Resolution

The runtime resolves reference chains recursively.

Targets may be nodes or references. Reference-to-reference chains are followed until a node is reached.

Cross-file source paths are resolved relative to the referencing file.

The local runtime intentionally rejects network targets containing `://` with `R233`.

### 11.2 Cycles

Both local and cross-file reference cycles are invalid.

Example:

```srng
reference a = ".#b";
reference b = ".#a";
```

Cycles produce `R234`, and affected references are inactive.

Resolution depth is also bounded by `RuntimeOptions.max_reference_depth`. Exceeding the limit produces `R232`.

### 11.3 Provenance and overrides

A resolved reference retains:

- its original target string (`provenance`)
- linked source geometry
- linked source properties
- authored instance geometry
- authored instance properties

Renderer preparation merges them. Instance geometry overrides linked geometry only for fields explicitly authored on the instance. Instance properties override properties with the same names from the linked source.

This preserves reference identity instead of destructively flattening the scene.

## 12. Animation declarations

Animation is reference/metadata-only in v0.1:

```srng
animation hover {
    reference: "./motions.srng#hover";
}
```

The compiler and runtime preserve animation declarations. The v0.1 renderer does not execute a timeline or interpolation model.

## 13. Paint order

Renderable nodes and references receive monotonically increasing `paint_order` values according to source declaration order.

The renderer sorts active renderable items by this value before emitting draw commands.

Later declarations therefore paint after earlier declarations unless a future format version defines a different ordering mechanism.

## 14. Fill

Solid fill:

```srng
fill: #ff8800;
```

Supported solid color forms are:

```text
#RGB
#RGBA
#RRGGBB
#RRGGBBAA
```

No fill:

```srng
fill: none;
```

Fill rule:

```srng
fill-rule: nonzero;
```

or:

```srng
fill-rule: evenodd;
```

`nonzero` is the default.

## 15. Stroke

Example:

```srng
stroke: #ffffff;
stroke-width: 2px;
stroke-linecap: round;
stroke-linejoin: bevel;
stroke-miterlimit: 4;
stroke-dasharray: 4 2;
stroke-dashoffset: 1;
```

Supported line caps:

- `butt`
- `round`
- `square`

Supported joins:

- `miter`
- `round`
- `bevel`

Unknown values fall back to the default style rather than changing the compiler grammar.

## 16. Linear gradients

A linear gradient is declared with:

```srng
fill: linear-gradient;
gradient-stops: 0% #ff0000, 100% #0000ff;
gradient-start: 0 0;
gradient-end: 300 0;
```

`gradient-stops` must contain at least two comma-separated stops. Each stop contains an offset followed by a supported hex color.

Offsets may use percentages or normalized values from `0` to `1`.

When start/end are omitted, the renderer derives a diagonal gradient from the resolved geometry bounds.

The same paint syntax may be used for `stroke`.

## 17. Clipping

A renderable declaration may reference another active local node as a clip path:

```srng
clip: mask;
```

The renderer emits a push-clip command before the object's fill/stroke and a matching pop afterward.

Missing clip declarations produce `G211`. Existing clips with unusable geometry produce `G210`.

## 18. Shadows and effects

SRNG v0.1 has no opaque effect-stack object model.

Shadows and similar constructs should be represented as ordinary vector declarations and properties. This keeps their geometry visible to tools and preserves the relationship-first design.

## 19. Compiler fault tolerance

Compilation attempts to preserve valid declarations after malformed input instead of aborting the entire document.

Diagnostics are embedded in `SRNG-IR`. A file with diagnostics may therefore still contain usable declarations.

Diagnostics are metadata and must never be rendered as artwork.

## 20. SRNG-IR JSON

Compiler output has this shape:

```json
{
  "format": "SRNG-IR",
  "version": "0.1",
  "source": "example.srng",
  "file_id": "hero",
  "declarations": [],
  "diagnostics": []
}
```

`file_id` may be `null`.

### 20.1 Unit declaration

```json
{
  "type": "unit",
  "name": "gu",
  "scale": 8,
  "base": "px"
}
```

### 20.2 Node declaration

```json
{
  "type": "node",
  "kind": "rect",
  "id": "card",
  "properties": {
    "position": "64 px 64 px",
    "size": "520 px 320 px",
    "fill": "#ffffff"
  }
}
```

Implementations must not depend on insignificant whitespace inside normalized property strings; they should parse property semantics by tokens/values.

### 20.3 Relation declaration

```json
{
  "type": "relation",
  "from": "card",
  "to": "title",
  "properties": {
    "kind": "contains"
  }
}
```

### 20.4 Reference declaration

```json
{
  "type": "reference",
  "id": "logo",
  "target": "./brand.srng#logo",
  "source_file": "./brand.srng",
  "target_id": "logo",
  "properties": {}
}
```

### 20.5 Animation declaration

```json
{
  "type": "animation",
  "id": "hover",
  "properties": {
    "reference": "\"./motions.srng#hover\""
  }
}
```

## 21. SRNG-SCENE

Runtime output uses:

```json
{
  "format": "SRNG-SCENE",
  "version": "0.1",
  "source": "example.srng",
  "file_id": null,
  "viewport": {
    "width": 1920,
    "height": 1080,
    "dpi": 96
  },
  "unit_context": {},
  "nodes": [],
  "relations": [],
  "references": [],
  "animations": [],
  "diagnostics": []
}
```

### 21.1 Node scene entry

Each node contains:

- `id`
- `kind`
- `source`
- authored `properties`
- resolved `geometry`
- `paint_order`
- `active`

Geometry fields are `x`, `y`, `width`, and `height`; each may be null when not applicable.

### 21.2 Reference scene entry

Each reference contains:

- `id`
- `source_file`
- `target_id`
- `provenance`
- `resolved`
- `active`
- `resolved_kind`
- instance `properties`
- instance `geometry`
- `linked_geometry`
- `linked_properties`
- `paint_order`

### 21.3 Runtime diagnostics

Runtime diagnostics contain:

- severity
- code
- message
- source
- optional declaration
- line
- column

Current runtime-generated semantic diagnostics use line/column zero when no precise source span is available.

## 22. Runtime options

The reference runtime exposes:

```text
viewport_width       default 1920
viewport_height      default 1080
dpi                  default 96
resolve_references   default true
max_reference_depth  default 32
```

Viewport dimensions, DPI, and maximum reference depth must be positive.

## 23. Renderer preparation model

Rendering is deliberately split from runtime execution.

`prepare_scene` converts `SRNG-SCENE` into backend-neutral commands:

```text
PushClip
PopClip
Fill
Stroke
```

Each command contains resolved path geometry and paint/style information.

Both CPU and GPU backends consume this same command sequence. This prevents backend-specific interpretation of SRNG semantics.

## 24. Revision cancellation

Interactive editors can discard stale preparation work through a monotonically increasing revision gate.

A preparation started for an old revision stops emitting new commands once a newer revision becomes current.

This is a rendering optimization and does not alter SRNG file semantics.

## 25. CPU backend

The reference CPU backend uses Vello CPU and produces an RGBA byte buffer.

The output dimensions exactly match the prepared scene dimensions.

CPU rendering is used as the deterministic CI pixel-verification path because it does not require a hardware graphics adapter.

## 26. GPU backend

The reference GPU backend uses Vello Hybrid with wgpu.

The host application owns:

- wgpu instance/adapter/device creation
- queue
- destination texture/view
- command encoder
- command submission
- window/surface presentation

SRNG owns only vector scene preparation and recording rendering commands into the caller-provided target.

This separation avoids coupling SRNG to a window system and allows the same renderer layer to work on Linux and macOS.

The renderer must be constructed for a target texture format and maximum target dimensions. A prepared scene larger than that target is rejected.

SRNG v0.1 does not expose image textures, so the reference GPU backend supplies an empty external texture-binding table.

## 27. Cross-platform requirements

The v0.1 reference implementation is continuously checked for:

- Linux x86_64
- Linux AArch64
- macOS x86_64
- macOS Apple Silicon/AArch64

Native runtime and CPU-renderer tests run on both Ubuntu and macOS CI hosts.

The GPU backend is type-checked on both platforms without requiring a physical GPU in CI.

Platform-specific window creation or presentation code is outside the SRNG format/runtime contract.

## 28. Color behavior

SRNG colors are currently authored as sRGB-style hexadecimal colors and passed through the Vello/Peniko backend color model.

SRNG v0.1 does **not** guarantee linear-light compositing, color-managed output, ICC profile handling, wide-gamut preservation, or exact colorimetric equivalence between all GPU/CPU/platform combinations.

Implementations must not claim such guarantees unless they add and document an explicit color-management layer.

## 29. Error isolation

A broken declaration should not prevent unrelated valid declarations from compiling or rendering where possible.

Broken references become inactive. Relations connected to inactive/missing endpoints also become inactive. Other nodes continue through the pipeline.

This behavior is central to editor use: one invalid region of a file must not blank the complete scene.

## 30. Diagnostic code groups

Compiler diagnostics currently use `E...` for errors and `W...` for warnings.

Important compiler codes include:

```text
E001 unterminated string
E002 expected hexadecimal color
E003 unexpected character
E101 missing SRNG header
E200 duplicate id
E210 missing relation source
E211 missing relation target
W200 duplicate unit declaration
W201 node lacks explicit position
W202 reference target lacks #id
```

Runtime codes include:

```text
R101 unknown declaration type ignored
R120 invalid unit
R200 invalid position
R201 missing node position
R202 negative node size
R203 invalid node size
R210 inactive/missing relation endpoint
R220 invalid reference position
R221 invalid reference size
R230 missing reference target id
R231 target not found
R232 maximum reference depth exceeded
R233 network reference rejected
R234 cyclic reference
R235 reference target/load/geometry failure
```

Renderer codes include:

```text
G101 no renderable geometry
G210 unusable clip geometry
G211 missing/inactive clip
G220 invalid fill paint
G221 invalid stroke paint
G301 text requires outline path data
G500 CPU backend render error
G600 GPU scene/backend command error
```

Consumers should treat the code as the stable machine-readable field and the message as human-readable context.

## 31. Complete example

```srng
srng 0.1;
file "example";

unit gu = 8 px;

rect clipBox {
    position: 16px 16px;
    size: 400px 220px;
}

rect background {
    position: 16px 16px;
    size: 400px 220px;
    fill: linear-gradient;
    gradient-stops: 0% #202840, 100% #101018;
    gradient-start: 16 16;
    gradient-end: 416 236;
    clip: clipBox;
}

path mark {
    position: 0px 0px;
    data: "M 60 60 L 160 60 L 110 160 Z";
    fill: #ffffff;
    stroke: #000000;
    stroke-width: 2px;
}

relation background -> mark {
    kind: contains;
}

reference secondMark = ".#mark" {
    position: 220px 40px;
}

animation pulse {
    reference: "./animation.srng#pulse";
}
```

## 32. Minimal implementation sequence

A clean-room SRNG v0.1 implementation should proceed in this order:

1. Decode UTF-8 source.
2. Lex identifiers, numbers, strings, colors, punctuation, comments, and scientific notation.
3. Parse the header and declarations while recovering after malformed statements.
4. Validate duplicate ids, relation endpoints, and obvious source-level issues.
5. Emit `SRNG-IR` without flattening references.
6. Load runtime options and resolve custom units.
7. Resolve node geometry.
8. Resolve local/cross-file reference chains with depth and cycle detection.
9. Preserve linked and authored reference state separately.
10. Deactivate invalid references and dependent relations without deleting unrelated content.
11. Assign/preserve source paint order.
12. Emit `SRNG-SCENE`.
13. Convert active scene entries into backend-neutral fill/stroke/clip commands.
14. Render those commands through a CPU or GPU backend.
15. Keep diagnostics as metadata; never render them as visual error objects.

If these rules are followed, the implementation will match the behavioral contract of the SRNG v0.1 reference compiler/runtime/renderer.