# macOS packaging

SRNG Studio builds natively with Cargo on Apple Silicon.

## Build

```sh
cargo build --release --manifest-path studio/Cargo.toml
```

The executable is `studio/target/release/srng-studio`.

## Create the application bundle

```sh
bash packaging/macos/package-app.sh
```

This creates `dist/SRNG Studio.app` with:

- the native `srng-studio` executable;
- `Info.plist` bundle metadata;
- `.srng` document type / `dev.srng.graphics` UTI registration;
- the native `srng-studio.icns` application icon.

SRNG Studio accepts a `.srng` or `.svg` path as its first process argument, which is the portable command-line/file-launch contract used by the application. LaunchServices document-open behavior remains a target-macOS acceptance test because it is delivered by the desktop event system rather than repository CI.

CI packages the unsigned application as `srng-studio-macos-aarch64.zip`.

Signing, notarization and Gatekeeper validation require private Apple release credentials and are intentionally outside repository-only CI.
