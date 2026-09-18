# GPU Resource Architecture v0.6

Branch: `gpu-resource-parity-v0.6`

## Scope

This branch closes the GPU resource gap for existing SRNG native renderer commands without changing SVG importer semantics, the SRNG grammar, typography, or the CPU-owned filter graph.

## Resource model

`GpuRenderer` owns a `GpuResourceStore` for the lifetime of a render. The store owns every uploaded `wgpu::Texture` and `TextureView` referenced by `vello_hybrid::TextureId`. `TextureBindings` is rebuilt from the store immediately before rendering, so every external texture referenced by the hybrid scene has a live binding.

Textures use `Rgba8Unorm` with `TEXTURE_BINDING | COPY_DST | RENDER_ATTACHMENT`. Embedded images and rasterized pattern tiles are uploaded once for the render and remain alive until the next render clears the store. This avoids GPU -> CPU readback and avoids rebinding transient borrowed views.

## Embedded images

Native `Command::DrawImage` accepts PNG and SVG data URIs. PNG data is decoded to RGBA8. SVG data is rasterized with `resvg`, then uploaded to a GPU texture. `preserveAspectRatio` placement matches the existing CPU backend: `none`, meet/slice, and xMin/xMid/xMax + YMin/YMid/YMax alignment are retained.

Images are emitted through `vello_hybrid::Scene::draw_texture_rects`, using an affine transform from source pixel coordinates into the requested SRNG rectangle.

## Patterns

Native vector pattern records are rasterized into bounded tiles and uploaded. SVG compatibility pattern tiles already present in `Paint::SvgPattern` are rasterized and uploaded without reconstructing an SVG from SRNG commands.

The pattern texture is sampled through the hybrid external-texture path. The target path remains a native vector clip. Solid, linear-gradient and radial-gradient paths continue through the existing vector paint path.

Texture-backed pattern strokes currently require CPU fallback because `draw_texture_rects` operates on sampled rectangles rather than arbitrary stroked geometry. This is diagnosed instead of silently dropping the stroke.

## Masks and isolated layers

Mask blocks are treated as explicit isolated layers. When the build includes the CPU backend, commands inside the layer and the native mask records are deterministically rasterized, alpha-composited, uploaded once, and then composited by the GPU scene as a texture. No masked content is silently omitted.

A GPU-only build that lacks a native mask shader reports that deterministic CPU fallback is unavailable. This keeps behavior explicit rather than producing an incorrect frame.

## Filter execution boundary

This branch does not implement the CPU-owned native FilterGraph. It introduces `FilterExecutionRequest` and `GpuFilterExecutor` as the reusable GPU execution boundary for a later FilterGraph implementation.

Existing `PushFilter` blocks are preserved as isolated logical regions. Until a native executor is connected, their contents are rendered unfiltered with a diagnostic rather than dropped or translated into a second competing filter representation.

## Clipping and transforms

Existing path clipping remains native in `vello_hybrid`. Images use affine destination transforms. Texture-backed pattern fills use a vector clipping path around the sampled resource. Masked isolated layers are composited through the same scene and therefore remain subject to enclosing clips.

## Lifetime and copies

- CPU decode/rasterize -> GPU upload is one-way.
- No GPU readback is introduced.
- External texture views are owned by `GpuResourceStore` until render completion.
- The store is cleared before the next frame to make lifetime explicit and deterministic.

Future work may add cache keys for immutable images/pattern tiles and native GPU mask/filter passes without changing the public execution boundary introduced here.
