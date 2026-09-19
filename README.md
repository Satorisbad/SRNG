# SRNG

SRNG is a relationship-first vector graphics format and native rendering stack designed for precise programmatic and agentic editing without flattening scene semantics.

The repository contains the SRNG compiler/runtime, SVG importer, CPU/GPU renderer, tooling, and SRNG Studio desktop application.

## Current product surface

- Native `.srng` source format with explicit relationships and reusable resources.
- Fault-tolerant compiler and diagnostics.
- Runtime scene resolution.
- CPU renderer and GPU resource path with deterministic fallback.
- SVG import including resource references, images, masks, filters, gradients, and text architecture.
- Font loading and text shaping infrastructure.
- Native SRNG Studio for opening, rendering, editing, reloading, watching, zooming/panning, and exporting SRNG documents.
- Linux desktop/MIME integration and macOS application-bundle metadata.
- Validation, formatting, conversion, runtime, and rendering command-line tools.

## Source compatibility

The first product release is SRNG v1. The current source grammar header remains:

```srng
srng 0.1;
```

The source compatibility contract is defined in [`docs/SPEC.md`](docs/SPEC.md) and frozen for the v1 product release in [`docs/V1_RELEASE_CONTRACT.md`](docs/V1_RELEASE_CONTRACT.md).

## Build

Compiler/runtime/tooling:

```sh
cargo build --release
```

Studio:

```sh
cargo build --release --manifest-path studio/Cargo.toml
```

Renderer:

```sh
cargo build --release --manifest-path renderer/Cargo.toml
```

## Open an SRNG document

```sh
cargo run --manifest-path studio/Cargo.toml -- examples/basic.srng
```

`.srng` files are rendered directly. SVG files can also be opened for import/comparison.

## Linux desktop install

```sh
bash packaging/linux/install-user.sh
```

This installs the native Studio executable into `~/.local/bin`, registers `application/x-srng`, installs the desktop launcher and icon, and associates `.srng` files with SRNG Studio when the desktop provides the standard XDG tools.

## Validate

```sh
cargo run --bin srngc -- examples/basic.srng --check
```

## Compile to IR

```sh
cargo run --bin srngc -- examples/basic.srng -o build/basic.json
```

Use `--stdout` instead of `-o` to print IR.

## Format

```sh
cargo run --bin srngfmt -- examples/basic.srng --check
cargo run --bin srngfmt -- examples/basic.srng --write
```

## Convert SVG

```sh
cargo run --bin srng-svg -- source.svg -o source.srng
```

## Runtime scene

```sh
cargo run --bin srngr -- examples/basic.srng --stdout
```

## Tests

```sh
cargo test --workspace
cargo test --manifest-path studio/Cargo.toml
cargo test --manifest-path renderer/Cargo.toml
```

The productization torture corpus and desktop/release boundary are documented in [`docs/PRODUCTIZATION_V07.md`](docs/PRODUCTIZATION_V07.md). Tool details are in [`docs/TOOLING.md`](docs/TOOLING.md).

## Platform targets

- Omarchy / Arch Linux ARM64 and x86_64.
- macOS Apple Silicon and Intel where dependencies support the target.

Repository CI verifies portable compiler/runtime/renderer behavior and desktop builds. Actual file-manager association, Wayland/Hyprland behavior, thermal behavior, and signed/notarized macOS distribution require target-machine or release-credential validation.
