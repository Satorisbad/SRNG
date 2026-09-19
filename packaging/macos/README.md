# macOS packaging

SRNG Studio builds natively with Cargo on Apple Silicon.

## Build

```sh
cargo build --release --manifest-path studio/Cargo.toml
```

The executable is `studio/target/release/srng-studio`.

## Application bundle contract

A release bundle should install the executable as:

`SRNG Studio.app/Contents/MacOS/srng-studio`

The bundle must declare `.srng` as an imported document type and pass the opened file path as the first process argument. SRNG Studio already accepts that argument and opens `.srng` directly.

Signing, notarization and Gatekeeper validation require release credentials and therefore are intentionally outside repository-only CI.
