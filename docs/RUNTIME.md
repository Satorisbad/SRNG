# SRNG runtime v0.1

The runtime is the boundary between compiler IR and a renderer. It consumes `SRNG-IR` JSON and emits a renderer-neutral `SRNG-SCENE` document.

It is responsible for:

- converting built-in and custom units to device-independent pixels;
- resolving positions and sizes against a viewport;
- retaining first-class relationships and marking broken relationships inactive;
- resolving local cross-file references without flattening or copying referenced artwork;
- preserving reference provenance;
- carrying compiler diagnostics forward and isolating runtime failures to individual declarations;
- preserving animation references for a later animation system.

It does not rasterize or draw. A renderer consumes `SRNG-SCENE` after the runtime has completed these tasks.

## CLI

Run a source file directly:

```bash
cargo run --bin srngr -- examples/basic.srng --stdout
```

Run previously emitted compiler IR:

```bash
cargo run --bin srngr -- examples/basic.srng.json -o build/basic.scene.json
```

Set the viewport and DPI used for `%`, `vw`, `vh`, physical units, and custom units:

```bash
cargo run --bin srngr -- scene.srng --viewport 1920x1080 --dpi 96
```

Use `--no-resolve` to preserve references without accessing linked files.

## Error behavior

The runtime always returns the scene it could build. A declaration with invalid geometry is retained with `active: false`; unrelated valid nodes remain active. Broken relations are retained with `active: false`. Diagnostics stay in scene metadata and never become visible vector content.

The CLI writes the scene but exits with status `1` when compiler or runtime errors are present. Invalid CLI usage or an unreadable root input exits with status `2`.
