# SRNG Image Pipeline v0.6

`image-pipeline-v0.6` moves embedded-image decoding into shared scene preparation so CPU and future GPU backends consume the same validated native image resource.

## Supported embedded formats

- PNG
- JPEG
- WebP
- GIF (static/first-frame semantics via the Rust `image` decoder)
- SVG through the existing local `resvg/usvg` path

Only `data:` image references are accepted. Network or external file fetching is disabled.

## Native resource model

`EmbeddedImage` stores:

- original data URI for provenance/debugging
- detected codec
- intrinsic width and height
- decoded RGBA8 pixels
- destination x/y/width/height
- SVG `preserveAspectRatio` string

Raster decoding is backend-independent. The CPU renderer uploads the prepared RGBA8 buffer into its pixmap representation. GPU texture upload remains owned by the GPU-resource branch.

## MIME and data URI validation

Declared MIME is not trusted. The payload is decoded first and its format is identified from file signatures/content. A mismatched MIME declaration is rejected. Invalid base64, malformed percent escapes, unknown data, corrupt codec payloads and invalid SVG all produce image diagnostic `G260` during preparation instead of a generic render failure.

Accepted JPEG declarations are `image/jpeg` and the legacy alias `image/jpg` when the payload is actually JPEG.

## Security and allocation limits

- encoded payload after URI decoding: 16 MiB maximum
- intrinsic dimension: 16,384 pixels maximum on either axis
- decoded pixel count: 16,777,216 pixels maximum
- decoded RGBA8 storage: 64 MiB maximum
- dimensions are checked before full raster decode where supported
- allocation arithmetic is checked
- external/network fetching is never attempted

These limits are intentionally conservative and deterministic. They prevent unbounded allocations and common decompression-bomb patterns while still allowing large embedded artwork.

## Aspect ratio and intrinsic size

Intrinsic dimensions come from the decoded resource rather than MIME metadata. If authored destination dimensions are absent, preparation falls back to the intrinsic dimensions. Existing `preserveAspectRatio` handling remains in the renderer, including `none`, `meet`, `slice`, and x/y alignment tokens.

## Alpha

All raster codecs are normalized to RGBA8. Alpha-capable sources preserve their alpha channel. Formats without alpha decode as fully opaque RGBA8.

## GIF semantics

GIF is treated as a static embedded image. The decoded first frame is used; animation/timing is outside this branch's ownership.

## Diagnostics

`G260` covers invalid or unsafe image resources, including unsupported content, MIME mismatch, malformed URI encoding, corrupt image data, size-limit violations, invalid placement and prohibited external references.
