# SRNG resource references

`resource-reference-v0.6` keeps reusable content as scene-graph resources rather than converting a referenced group into one synthetic path or one shared paint.

## Import model

Local SVG `<use href="#id">` becomes an SRNG `reference` declaration. Referenced shapes, groups and symbols that originate in SVG resource content are emitted as native SRNG declarations with `resource-only: true`. Resource-only declarations retain source IDs, source element metadata, child containment relations and each child's own paint/style.

Resource-only declarations do not paint by themselves. A resolved reference remains present in `Scene.references`, preserving its authored target and provenance. If the target is a subtree, runtime resolution also exposes resolved child identity in `SceneReference::resolved_nodes` and creates ordinary native scene nodes for rendering. Renderer code therefore does not need SVG-specific `<use>` knowledge.

## Resolution and cycles

References use the existing `(source path, target id)` resolution key. Both direct and indirect recursive references are rejected with `R234`. Reference depth is bounded by `RuntimeOptions::max_reference_depth`. HTTP/network targets remain unsupported (`R233`). A broken top-level reference is marked inactive and produces a diagnostic without aborting unrelated scene content.

Nested references are resolved through the same mechanism as top-level references. A referenced shape stays a first-class reference. A referenced group or symbol resolves a child subtree while retaining the original reference and source child IDs.

## Instance geometry

`x` and `y` are instance offsets. For symbols, `width` and `height` establish the instance viewport when the symbol has a valid `viewBox`. `preserveAspectRatio="none"`, `meet` and `slice` mapping are represented by runtime symbol mapping; the normal alignment keywords determine the viewport offset. The default is `xMidYMid meet`.

Resource child geometry is not flattened together. Each child is mapped independently into the instance coordinate space. Nested reference offsets are additive.

## Style behavior

Each resource child preserves its own explicit paint. Instance `fill` or `stroke` only replaces paint for a child marked as inheriting that property from the instance; an explicit child or resource-ancestor paint remains authoritative. Instance opacity properties are applied to instantiated children.

This distinction avoids the previous failure mode where a multi-style group was reduced to one `use-fill`/`use-stroke` pair.

## Provenance

Resolved instance nodes carry:

- `resource-instance`: the authored reference ID;
- `resource-source-id`: the source resource child ID;
- `resource-provenance`: the original reference target.

`SceneReference` also preserves its `provenance`, target ID and resolved child records.

## Scope

This branch intentionally does not implement fonts, filter graphs, GPU textures or arbitrary network resources. The reference mechanism is source-format neutral: local/cross-file SRNG references continue to use the same runtime resolver, so future SRNG-to-SRNG reusable scene resources do not require an SVG-specific resolver.
