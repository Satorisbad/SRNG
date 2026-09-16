# SRNG SVG Studio

`srng-studio` is the native visual front-end for the SVG importer and SRNG renderer.

## Purpose

The app makes SVG -> SRNG conversion inspectable instead of requiring terminal commands. It shows:

1. the original SVG rendered with `resvg`,
2. the generated SRNG source,
3. the SRNG renderer output,
4. importer/compiler/runtime/renderer diagnostics.

The left preview is intentionally rendered by an independent SVG renderer. The right preview is produced by SRNG's own compiler/runtime/CPU renderer. This makes visible differences meaningful during fidelity testing.

## Run

From the repository root:

```bash
cargo run --manifest-path studio/Cargo.toml
```

You can optionally open an SVG immediately:

```bash
cargo run --manifest-path studio/Cargo.toml -- /path/to/artwork.svg
```

On Omarchy/Arch, the application uses the normal Wayland/X11 desktop stack. No browser is required.

## Workflow

- **Open SVG** opens the native file picker.
- You can also drag an SVG file onto the window.
- **Convert** reruns SVG -> SRNG and refreshes both previews.
- The generated SRNG text is editable.
- **Render SRNG** renders the edited SRNG text without re-importing the SVG.
- **Save SRNG** writes the current text to a `.srng` file.
- **Save PNG** writes the current SRNG render to PNG.
- The diagnostics panel reports fidelity warnings and hard errors.

## Verification

The GUI window itself is not opened in headless CI. Instead, CI verifies the same library pipeline used by the window and separately compiles the native application on Linux and macOS.

Tests cover:

- SVG parsing and independent original preview rasterization,
- SVG -> SRNG conversion,
- SRNG compiler/runtime execution,
- CPU rendering,
- expected output dimensions,
- non-empty pixel output,
- PNG encoding and signature validation.

The dedicated `.github/workflows/studio.yml` workflow runs `cargo check` and `cargo test` for the studio crate on both `ubuntu-latest` and `macos-latest`.

## Fidelity model

The studio does not hide SVG features the importer cannot yet reproduce. Unsupported or partially supported SVG semantics remain visible in diagnostics. The original SVG preview therefore serves as the reference image while the SRNG preview shows what the current SRNG implementation actually reproduces.
