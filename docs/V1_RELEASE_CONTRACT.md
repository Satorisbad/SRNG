# SRNG v1 release contract

This document freezes the public compatibility contract for the first product release of SRNG.

## Versioning

The product release is **SRNG v1**. The source-language header remains:

```srng
srng 0.1;
```

The `0.1` token is the current source grammar/IR compatibility version and is not the desktop product version. Changing that token requires an explicit source-format migration and is not implied by shipping SRNG Studio v1.

## Frozen source contract

The grammar, lexical rules, diagnostics model, units, node declarations, relationships, references, animations, and runtime boundaries defined by `docs/SPEC.md` are the v1 compatibility baseline.

The following rules are frozen for v1:

- UTF-8 source files.
- `srng 0.1;` is required.
- Valid declarations remain usable when unrelated declarations are broken whenever recovery is possible.
- Broken references/resources produce diagnostics and must not silently corrupt unrelated rendering.
- Relationships remain explicit rather than being inferred from textual nesting.
- Resource/reference provenance is preserved through compilation/runtime.
- Unknown or unsupported properties remain diagnosable and must not be silently reinterpreted as unrelated semantics.
- Renderer-specific state does not become implicit source syntax.

## Compatibility rule

A v1-compatible implementation must not change the meaning of already-valid `srng 0.1` documents without a new grammar compatibility version. Additive behavior may be introduced when old documents keep the same meaning and unsupported new behavior degrades through diagnostics.

## Diagnostics

Diagnostic codes are part of the tooling contract. Existing codes should not be reassigned to unrelated meanings. New diagnostics receive new codes. Error handling should remain bounded: malformed input must not panic, hang, or generate unbounded duplicate diagnostics.

## Desktop file contract

- Extension: `.srng`
- MIME type: `application/x-srng`
- macOS UTI: `dev.srng.graphics`
- Encoding: UTF-8 text

Desktop applications receive the selected SRNG path as a normal file-open argument and render the document directly.

## Tooling contract

The v1 repository provides:

- `srngc --check` for validation;
- `srngc` for compilation to IR;
- `srngfmt` for conservative formatting;
- `srng-svg` for SVG-to-SRNG conversion;
- `srngr` for runtime scene execution;
- the renderer toolchain for raster output;
- SRNG Studio for native desktop viewing/editing.

## Non-contract internals

Internal module layout, GPU resource allocation strategy, cache implementation, test fixture organization, and Studio widget layout may change without a source-format version bump as long as the public source/runtime/rendering semantics remain compatible.
