# SRNG tooling

The repository exposes separate tools for validation, compilation, formatting, SVG import, runtime execution, rendering, and desktop viewing.

## Validate SRNG

```sh
cargo run --bin srngc -- drawing.srng --check
```

Exit status is `0` when no error-severity diagnostics are produced and `1` when validation fails. Diagnostics include file, line, column, severity, code, and message.

## Compile SRNG to IR

```sh
cargo run --bin srngc -- drawing.srng -o drawing.srng.json
```

Use `--stdout` to emit IR to standard output.

## Format SRNG

Print formatted source:

```sh
cargo run --bin srngfmt -- drawing.srng
```

Rewrite the file:

```sh
cargo run --bin srngfmt -- drawing.srng --write
```

Check formatting without changing the file:

```sh
cargo run --bin srngfmt -- drawing.srng --check
```

`srngfmt` is deliberately conservative. It normalizes leading indentation and blank-line runs while preserving declaration/property text, strings, comments, and property order.

## Convert SVG to SRNG

```sh
cargo run --bin srng-svg -- source.svg -o source.srng
```

The SVG importer preserves unsupported or broken portions as diagnostics where possible instead of discarding unrelated valid content.

## Resolve runtime scene

```sh
cargo run --bin srngr -- drawing.srng --stdout
```

This compiles and executes the document into SRNG-SCENE semantics.

## Render

The renderer crate provides the native raster path and `srng-render` tooling. Studio uses the same renderer pipeline rather than a separate preview implementation.

## Desktop Studio

```sh
cargo run --manifest-path studio/Cargo.toml -- drawing.srng
```

Studio also accepts SVG files. SRNG documents open and render directly; they do not require SVG conversion.
