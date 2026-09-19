# SRNG Image Pipeline v0.6

The integrated v0.6 renderer validates and decodes embedded images before backend execution. CPU and GPU therefore consume the same normalized image content instead of maintaining independent codec policy.

## Supported embedded formats

- PNG
- JPEG
- WebP
- GIF using static/first-frame semantics
- SVG through the local `resvg/usvg` path

Only `data:` image references are accepted. Network and external file fetching are disabled.

## Native preparation model

Scene preparation detects the payload from its signature/content, validates the declared MIME, decodes it to bounded RGBA8, and normalizes the validated resource to an internal PNG data resource. Existing `EmbeddedImage` placement and `preserveAspectRatio` semantics remain unchanged, while both CPU and GPU receive codec-independent content.

This normalization is an internal renderer resource step; authored SRNG is not rewritten and SVG XML reconstruction is not used for raster formats.

## Validation and diagnostics

Invalid base64, malformed percent escapes, MIME mismatch, corrupt payloads, unsupported data, invalid SVG, prohibited external references, and allocation-limit failures produce renderer diagnostic `G260`. Invalid image nodes are disabled rather than rendered as placeholder group geometry.

## Security limits

- decoded URI payload: 16 MiB maximum
- intrinsic dimension: 16,384 pixels maximum per axis
- decoded pixel count: 16,777,216 pixels maximum
- decoded RGBA8 storage: 64 MiB maximum
- checked dimension multiplication
- no network or external file fetching

## Alpha and intrinsic dimensions

Raster inputs normalize to RGBA8. Alpha-capable formats retain alpha; opaque formats decode fully opaque. Intrinsic dimensions come from decoded content. Authored destination geometry remains authoritative when present, and normal native SRNG geometry is bridged into the renderer image placement path when SVG-specific source geometry is absent.
