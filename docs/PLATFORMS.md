# Platform support

`srngc` is intentionally implemented using only Rust's standard library in v0.1. There are no native C/C++ dependencies, graphics SDK dependencies, or platform-specific system calls in the compiler core.

## Omarchy / Arch Linux

SRNG supports Omarchy and other Arch-based Linux distributions on both x86_64 and AArch64.

Install the Rust toolchain:

```bash
sudo pacman -S rustup
rustup default stable
```

Build and test:

```bash
cargo build --release
cargo test
```

Install `srngc` for the current user:

```bash
./scripts/install.sh
```

The installed binary is normally available as `~/.cargo/bin/srngc`. Ensure `~/.cargo/bin` is in `PATH`.

For an Apple Silicon Mac running Asahi Linux/Omarchy, the relevant Rust target is `aarch64-unknown-linux-gnu`.

## macOS

SRNG supports Intel and Apple Silicon macOS.

Install Rust with rustup, then build normally:

```bash
cargo build --release
cargo test
./scripts/install.sh
```

Native Apple Silicon builds use `aarch64-apple-darwin`. Intel macOS builds use `x86_64-apple-darwin`.

No Homebrew libraries are required by the compiler itself.

## CI compatibility matrix

GitHub Actions runs native tests on Linux and macOS and type-checks these targets:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

This keeps the compiler portable while the renderer/runtime layers are developed separately.
