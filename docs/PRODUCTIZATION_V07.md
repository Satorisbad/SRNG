# SRNG Productization v0.7

This phase turns the integrated compiler/runtime/renderer into a desktop product.

## Native file behavior

SRNG Studio accepts a path as its first process argument. `.srng` files are read as UTF-8 SRNG source and rendered directly. `.svg` files retain the SVG import/compare workflow. Unknown extensions are rejected with a user-facing status message.

The file dialog and drag/drop surface both formats. A loaded `.srng` can be reloaded from disk and saved without an SVG conversion step.

## Linux desktop integration

`packaging/linux/srng.xml` registers `application/x-srng` for `*.srng`.

`packaging/linux/srng-studio.desktop` declares SRNG Studio as a viewer for that MIME type and launches it with the selected file path.

`packaging/linux/install-user.sh` builds the native release executable, installs it under `~/.local/bin`, installs the desktop and MIME declarations, refreshes databases when their tools are present, and sets SRNG Studio as the default SRNG handler when `xdg-mime` is available.

The final double-click behavior must still be smoke-tested on the target Omarchy desktop because repository CI cannot reproduce the user's file manager/session configuration.

## CI and release artifacts

Studio CI checks and tests the workspace and builds release binaries on native Linux ARM64 and macOS ARM64 runners. Successful runs upload the native `srng-studio` executable as an artifact.

## Torture testing

Repository torture fixtures live under `tests/torture/`. They are intended to grow into a corpus covering:

- deep relationship/reference graphs;
- malformed source and malformed resources;
- large scenes and images;
- Unicode and font shaping;
- image -> mask -> filter -> clip -> reference combinations;
- repeated render/reload cycles;
- cancellation and stale-revision rejection.

Failures must produce bounded diagnostics rather than panic, hang, or silently corrupt unrelated rendering.

## Release boundary

Repository-only work can verify compilation, automated tests, artifact generation and package metadata. These remain target-machine acceptance tests:

1. Wayland/Hyprland launch.
2. File-manager `.srng` double-click association.
3. MIME/icon presentation.
4. Real interactive zoom/pan/reload behavior.
5. M2 8 GB sustained memory/thermal behavior.
6. macOS signing/notarization when release credentials exist.
