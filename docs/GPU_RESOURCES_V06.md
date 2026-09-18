# GPU Resource Architecture v0.6

Branch: `gpu-resource-parity-v0.6`

## Scope

This branch closes the GPU resource gap for existing SRNG native renderer commands without changing SVG importer semantics, the SRNG grammar, typography, or the CPU-owned filter graph.

## Resource model

`GpuRenderer` owns a `GpuResourceStore` for the lifetime of a render. The store owns every uploaded `wgpu::Texture` and `TextureView` referenced by `vello_hybrid::TextureId`. `TextureBindings` is rebuilt from the store immediately before rendering, so every external texture referenced by the hybrid scene has a live binding.

Textures use `Rgba8Unorm` with `TEXTURE_BINDING | COPY_DST | RENDER_ATTACHMENT`. Embedded images and rasterized pattern tiles are uploaded once for the render and remain alive until the next render clears the store. The external-texture path uses premultiplied RGBA as required by `vello_hybrid`. No GPU readback is introduced.

The `gpu` feature includes the deterministic CPU renderer as a resource fallback. This is intentional: Vello Hybrid 0.2 does not expose native mask layers, so unsupported hardware/backend operations can still preserve SRNG semantics instead of dropping content.

## Embedded images

Native `Command::DrawImage` accepts PNG and SVG data URIs. PNG data is decoded to RGBA8 and premultiplied before upload. SVG data is rasterized with `resvg`, whose tiny-skia pixmap data is already premultiplied, then uploaded to a GPU texture. `preserveAspectRatio` placement matches the existing CPU backend: `none`, meet/slice, and xMin/xMid/xMax + YMin/YMid/YMax alignment are retained.

Images are emitted through `vello_hybrid::Scene::draw_texture_rects`, using an affine transform from source pixel coordinates into the requested SRNG rectangle.

## Patterns

Native vector pattern records are deterministically rasterized into bounded tiles and uploaded. SVG compatibility pattern tiles already present in `Paint::SvgPattern` are rasterized and uploaded without reconstructing SVG from SRNG commands.

Each uploaded tile is repeated with source rectangles constrained to the actual texture dimensions, then clipped by the native vector target path. This satisfies Vello Hybrid's external-texture source-region requirements while preserving repeat semantics. Solid, linear-gradient and radial-gradient paths continue through the existing vector paint path.

Texture-backed pattern strokes are diagnosed rather than silently dropped because the current external-texture rectangle API cannot directly paint arbitrary stroked geometry. The CPU resource path remains available for deterministic fallback integration.

## Masks and isolated layers

Mask blocks are treated as explicit isolated layers. Commands inside the layer and the native mask records are deterministically rasterized through the bundled CPU fallback, alpha-composited, uploaded once, and then composited by the GPU scene as a premultiplied texture. No masked command is silently omitted.

This avoids GPU-to-CPU readback: CPU fallback produces the isolated resource before upload, and the resulting layer moves one-way to the GPU.

## Offscreen and filter execution boundary

`GpuTexture` is the reusable texture/offscreen resource abstraction and is created with render-target usage so later native offscreen passes can target the same resource type.

This branch does not implement the CPU-owned native FilterGraph. It introduces `FilterExecutionRequest` and `GpuFilterExecutor` as the reusable GPU execution boundary for a later FilterGraph implementation.

Existing `PushFilter` blocks are preserved as isolated logical regions. Until a native executor is connected, their contents are rendered unfiltered with a diagnostic rather than dropped or translated into a second competing filter representation.

## Clipping, transforms and compositing

Existing path clipping remains native in `vello_hybrid`. Images use affine destination transforms. Texture-backed pattern fills use native vector clipping around repeated sampled resources. Masked isolated layers are composited through the same scene and therefore remain subject to enclosing clips.

Alpha-bearing external resources are premultiplied before sampling so source-over compositing follows Vello Hybrid's texture contract.

## Lifetime and copies

- Decode/rasterize -> GPU upload is one-way.
- No GPU readback is introduced.
- External texture views are owned by `GpuResourceStore` until render completion.
- Texture dimensions travel with `TextureHandle`, preventing out-of-bounds source regions.
- The store is cleared before the next frame to make lifetime explicit and deterministic.

Future work may add cache keys for immutable images/pattern tiles and native GPU mask/filter passes without changing the public execution boundary introduced here.
