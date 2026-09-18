# Vendored gpui 0.2.2

This directory is the `gpui` 0.2.2 crate exactly as published on crates.io,
plus bibli's patches, wired in through `[patch.crates-io]` in the root
`Cargo.toml`. Remove both once the patches are upstream.

## Patches

Kept as diffs under `../patches/`, with paths rewritten to the Zed monorepo
layout (`crates/gpui/...`) so they apply there with `git apply -p1`.

- `0001-wayland-outgoing-drag.patch`: outgoing drag and drop on Wayland.
  Adds `Window::start_external_drag(paths)` (a `PlatformWindow` method with
  an unsupported default). The Wayland client creates a `wl_data_source`
  offering `text/uri-list`, calls `wl_data_device::start_drag` with the last
  mouse-press serial, answers `Send` with the URI list, and on `Cancelled`
  or `DndFinished` synthesizes a mouse-up into the originating window so its
  in-window drag ends. A drop back onto the same application arrives as an
  `ExternalPaths` drop like any other. No drag icon surface yet.

## Regenerating a patch

    G=$(ls -d ~/.cargo/registry/src/*/gpui-0.2.2)
    (cd / && diff -ruN --exclude=.cargo_vcs_info.json --exclude=Cargo.toml.orig \
        --exclude=.cargo-ok "${G#/}" $PWD/vendor/gpui) > vendor/patches/NNNN-name.patch

then rewrite the path prefixes as above.
