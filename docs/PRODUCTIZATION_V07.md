# SRNG Productization v0.7

This phase turns the integrated compiler/runtime/renderer into the first complete desktop-facing SRNG product layer.

## Native file behavior

SRNG Studio accepts a path as its first process argument. `.srng` files are read as UTF-8 SRNG source and rendered directly. `.svg` files retain the SVG import/compare workflow. Unknown extensions are rejected with a user-facing status message.

The file dialog and drag/drop surface both formats. A loaded `.srng` can be saved directly, manually reloaded, or watched for disk changes and automatically reloaded. Direct SRNG viewing never requires an SVG conversion step.

The native preview provides fit/reset plus bounded zoom controls and two-axis scrolling for panning large/zoomed documents. Diagnostics remain visible independently of the editable source panel.

## Memory and render safety

Studio rejects source payloads above 32 MiB before conversion/render work and rejects raster targets above an 8192 px side or 16,777,216 total pixels before the renderer allocates the output surface. Oversized documents receive bounded `UI300`/`UI301` diagnostics instead of attempting unbounded desktop allocations.

SVG comparison previews are independently downscaled to a maximum 1600 px side. Repeated-render tests verify deterministic output, and the torture suite exercises large scene graphs and malformed inputs.

These limits are Studio product guardrails; they do not redefine SRNG source semantics.

## Linux desktop integration

`packaging/linux/srng.xml` registers `application/x-srng` for `*.srng`.

`packaging/linux/srng-studio.desktop` declares SRNG Studio as a viewer/editor for that MIME type and launches it with the selected file path.

`packaging/linux/install-user.sh` builds the native release executable, installs it under `~/.local/bin`, installs the desktop/MIME declarations and scalable application icon, refreshes available desktop databases, and sets SRNG Studio as the default SRNG handler when `xdg-mime` is available.

CI also produces a self-contained Linux ARM64 release archive containing the executable and integration metadata.

## macOS integration

`packaging/macos/Info.plist` declares the SRNG document UTI (`dev.srng.graphics`), `.srng` filename extension, `application/x-srng` MIME type, bundle identity, and native application icon.

`packaging/macos/package-app.sh` creates a scripted unsigned `SRNG Studio.app` bundle. CI packages that application as a macOS ARM64 ZIP. Signing and notarization are deliberately separate because they require release credentials.

## Tooling

The repository exposes a complete basic tooling surface:

- `srngc --check` validates SRNG without writing IR;
- `srngc` compiles source to SRNG-IR JSON;
- `srngfmt` formats or checks source conservatively;
- `srng-svg` imports SVG to SRNG;
- `srngr` executes runtime scene resolution;
- renderer tooling performs raster rendering;
- SRNG Studio provides the desktop view/edit workflow.

The v1 compatibility boundary is frozen in `docs/V1_RELEASE_CONTRACT.md`; detailed tool usage is in `docs/TOOLING.md`.

## Torture and resilience testing

The productization integration suite directly tests native SRNG rendering, malformed source recovery, repeated deterministic renders, deep relationship chains, a 100-node scene, complex SVG resource chains, corrupt embedded image input, and the Studio raster safety budget.

The persistent `tests/torture/` corpus adds malformed SRNG, deep relationships, chained clip/mask/filter/reference SVG, Unicode text across multiple scripts, corrupt image data, and cyclic `<use>` references. Studio CI actively executes every fixture in that corpus and enforces bounded diagnostic counts.

Failures are expected to produce bounded diagnostics rather than panic, hang, or silently corrupt unrelated rendering. Existing renderer revision gating remains the cancellation/stale-revision mechanism.

## CI and release artifacts

Studio CI runs compiler/tooling checks and tests, Studio checks and tests, release builds, packaging-script syntax validation, and platform packaging on native Linux ARM64 and macOS ARM64 runners.

Successful runs upload:

- `srng-studio-linux-aarch64.tar.gz`
- `srng-studio-macos-aarch64.zip`

The normal compiler/runtime CI and renderer CI continue to run independently so desktop changes cannot hide lower-layer regressions.

## Repository-side completion boundary

Repository-side productization is complete when compiler/runtime CI, renderer CI, Studio Linux ARM64 CI, and Studio macOS ARM64 CI all pass and both release artifacts are produced.

The following checks inherently require a real target desktop or private release credentials and are therefore outside repository-only completion:

1. Omarchy/Hyprland launch and file-manager `.srng` double-click behavior.
2. Actual desktop MIME/icon presentation after installation.
3. Sustained interactive memory/thermal behavior on the target M2 8 GB machine.
4. macOS LaunchServices document-open behavior, signing/notarization, and Gatekeeper validation on a real Mac with release credentials where required.
