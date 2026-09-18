# SVG Fidelity v0.5

This milestone removes the remaining basic-SVG compatibility reconstruction from the supported renderer-facing SRNG model and makes resources explicit, typed, and testable.

## Implemented in v0.5

- native linear and radial gradient paint commands
- radial focal radius (`fr`), `spreadMethod` (`pad`, `repeat`, `reflect`), `gradientTransform`, gradient inheritance, units, stops, and opacity
- native vector mask resources using `paint|path` records
- native repeating vector pattern resources using `paint|path` records
- local `<use href="#id">` resolution for supported shape targets, including x/y placement and transforms
- embedded `data:image/png` and `data:image/svg+xml` image commands with `preserveAspectRatio` meet/slice/none behavior on the CPU backend
- native filter blocks for `feGaussianBlur` and `feOffset` on the CPU backend
- deterministic vector text fallback with common Latin letters, digits, punctuation, text anchor, font size, weight, style, and letter spacing
- explicit runtime diagnostics for unsupported or malformed resources
- CI and a dedicated SVG fidelity verification script

## Compatibility principle

Preserved `svg-*` fields are provenance only. A native SRNG file does not require preserved SVG XML to render any feature listed above when imported into the native v0.5 representation. The importer promotes supported SVG semantics into normal SRNG fields such as `pattern-data`, `mask-data`, `image-data`, `filter-chain`, and gradient properties.

Legacy v0.4 pattern provenance remains readable as an explicit backward-compatibility path. New native output does not require that reconstruction. Unsupported semantics are diagnostic-first rather than silently approximated.

## Resource model

Gradients are renderer paint values. Pattern and mask resources are typed vector records. Embedded images are image commands with data payload plus placement metadata. Filters are typed offscreen-layer operations. `<use>` references are resolved into normal vector geometry before command preparation.

## Text and fonts

Text retains `content`, `font-family`, `font-size`, `font-weight`, `font-style`, `text-anchor`, and spacing metadata. If outline `data` is present, it is rendered normally. Otherwise the deterministic built-in vector fallback is used. Weight and italic/oblique affect fallback geometry and letter spacing affects layout.

The fallback is not a replacement for a production shaping engine. Exact authored fonts, OpenType shaping, ligatures, kerning, bidi layout, complex scripts, variable fonts, and full Unicode typography remain a separate text subsystem.

## Filters

`feGaussianBlur` and `feOffset` are represented as native filter operations and execute without reconstructing SVG. Broader SVG filter graphs such as morphology, turbulence, displacement, lighting, component transfer, blend/composite graphs, and arbitrary intermediate-result routing are intentionally reported as unsupported until the filter graph model is extended.

## CPU/GPU behavior

The CPU backend is the v0.5 reference implementation for native masks, patterns, embedded images, and filters. The GPU backend renders solid/linear/radial vector paint directly, including advanced gradient spread and radial focal radius. Operations requiring texture upload or offscreen compositing currently produce explicit GPU diagnostics rather than silently rendering an incorrect approximation.

## Images and security

External network resource loading remains disabled. Local fragment references and embedded data resources are allowed. Native v0.5 directly decodes PNG data images and renders embedded SVG image payloads. Other raster codecs remain diagnostic until a dedicated image codec layer is added.

## Fidelity boundary

v0.5 completes the requested basic/native resource pass. Remaining work is deliberately subsystem-scale rather than hidden compatibility reconstruction: production font shaping/outlining, the full SVG filter graph, GPU texture/offscreen parity for masks/patterns/images/filters, additional raster codecs, and the most advanced SVG paint/compositing combinations.
