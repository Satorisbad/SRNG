# SRNG

SRNG is a relationship-first vector format intended as a programmable alternative to SVG.

This repository contains the first reference compiler, `srngc`, written in Rust.

## What v0.1 supports

- Custom SRNG syntax with an explicit `srng 0.1;` header
- Open-ended vector node kinds (`rect`, `path`, `text`, `stroke`, `gap`, `shadow`, components, etc.)
- Explicit positions and arbitrary vector properties
- Built-in and custom units
- First-class relationships between ids
- Cross-file `.srng#id` references with provenance preserved
- Reference-only animation/state declarations
- Fault-tolerant parsing: valid content survives broken content
- Diagnostics stored in emitted IR rather than rendered visually
- JSON IR suitable for future renderers and editors

## Platform support

The compiler is designed to build natively on:

- Omarchy / Arch Linux (`x86_64` and `aarch64`)
- macOS on Apple Silicon (`aarch64`)
- macOS on Intel (`x86_64`)

The v0.1 compiler uses only Rust's standard library, so it has no native library dependencies. See [`docs/PLATFORMS.md`](docs/PLATFORMS.md).

## Build

```bash
cargo build --release
```

The compiler binary is `target/release/srngc`. To install it into your Cargo bin directory, run:

```bash
sh scripts/install.sh
```

## Compile

```bash
cargo run -- examples/basic.srng
```

That produces `examples/basic.srng.json`. To print IR directly:

```bash
cargo run -- examples/basic.srng --stdout
```

Or choose an output path:

```bash
cargo run -- examples/basic.srng -o build/basic.json
```

The compiler still emits IR when syntax or semantic errors are present, but exits with status `1`. This is intentional: editors/renderers can keep using the valid remainder while surfacing the broken section.

## Tests

```bash
cargo test
```

See [`docs/SPEC.md`](docs/SPEC.md) for the v0.1 compiler surface.
