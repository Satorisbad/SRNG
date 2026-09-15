# SRNG v0.1 compiler surface

This document describes the syntax accepted by the first `srngc` compiler. It is intentionally small. The goal of v0.1 is to establish a stable, inspectable IR without freezing every future SRNG feature.

## Design rules carried into v0.1

SRNG is relationship-first. Geometry can still contain ordinary properties, but hierarchy and spatial meaning are represented explicitly with `relation` declarations instead of being hidden in source nesting.

All authored geometry should state its position explicitly. The compiler warns when a node has no `position` property instead of inventing one.

Units are explicit. Built-ins include `px`, `pt`, `mm`, `cm`, `in`, `%`, `vw`, and `vh`. Custom units use `unit <name> = <scale> <base>;`.

Cross-file content is reference-only in v0.1. A target such as `"./icons.srng#star"` keeps both its source file and target id in the IR. The compiler does not silently copy or flatten the source object.

Animation is also reference-only in v0.1. This avoids prematurely defining a timeline language while still letting SRNG files point at animation/state data.

Shadows and similar visual constructs are authored as ordinary vector nodes/properties. There is no opaque effect stack in v0.1.

Compilation is fault tolerant. Broken declarations emit diagnostics, but successfully parsed declarations are retained and JSON IR is still produced. Diagnostics are data in the compiled file; they do not become visible artwork.

## Grammar

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

Node kinds are deliberately open-ended. `rect`, `path`, `text`, `canvas`, `group`, `component`, `gap`, `stroke`, `shadow`, and future kinds can use the same syntax without requiring a compiler release just to add a new declarative vector category.

## Example

```srng
srng 0.1;
file "hero";
unit gu = 8 px;

rect card {
    position: 64px 64px;
    size: 520px 320px;
    fill: #ffffff;
}

text title {
    position: 96px 104px;
    content: "Hello";
}

relation card -> title {
    kind: contains;
    gap: 4gu;
}

reference logo = "./brand.srng#logo" {
    mode: reference-only;
    position: 460px 104px;
    fit: clip;
}
```

## Output

`srngc` emits JSON IR containing `format`, `version`, `source`, optional `file_id`, declarations, and diagnostics. This JSON is the first compiler boundary for renderers, editors, importers, and later link/resolution stages.
