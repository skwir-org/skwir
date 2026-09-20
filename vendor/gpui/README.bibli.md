# Vendored gpui 0.2.2

This directory is the `gpui` 0.2.2 crate exactly as published on crates.io,
plus bibli's patches, wired in through `[patch.crates-io]` in the root
`Cargo.toml`. Remove both once the patches are upstream.

## Patches

Kept as diffs under `../patches/`, with paths rewritten to the Zed monorepo
layout (`crates/gpui/...`) so they apply there with `git apply -p1`.

- `0001-wayland-outgoing-drag.patch`: outgoing drag and drop on Wayland.
  Adds `Window::start_external_drag(paths, icon)` (a `PlatformWindow` method
  with an unsupported default) and `ExternalDragIcon` (RGBA8 pixels plus a
  hotspot). The Wayland client creates a `wl_data_source` offering
  `text/uri-list`, uploads the icon into a `wl_shm` buffer on a fresh surface
  (premultiplied ARGB8888, hotspot via `wl_surface.offset` or the attach
  offset on older compositors), calls `wl_data_device::start_drag` with the
  last mouse-press serial, answers `Send` with the URI list, and on
  `Cancelled` or `DndFinished` destroys the icon and synthesizes a mouse-up
  into the originating window so its in-window drag ends. A drop back onto
  the same application arrives as an `ExternalPaths` drop like any other.

- `0002-taffy-f32-literals.patch`: two float literals in `taffy.rs` given an
  explicit `f32` suffix. Rust 1.98 warns that the implicit fallback becomes a
  hard error (`float_literal_f32_fallback`, rust-lang/rust#154024).

- `0003-blade-atlas-retirement.patch`: removing the last image from a Blade
  texture cancels queued initialization/uploads before recycling its atlas
  index. Physical destruction waits for the renderer's previous GPU submission
  to complete; texture views are destroyed before their textures. This fixes
  rapid preview replacement between UI drawing and Wayland presentation,
  including index reuse by a smaller replacement texture. Two ignored GPU
  regression tests cover removal before flush and slot reuse. They live in
  `platform::blade::blade_atlas::tests` and require a working GPU context.
  On Linux, run `just atlas-check` in the development shell. The runner uses an
  isolated copy so GPUI's dev dependencies do not enter bibli's workspace.

  The patch relies on one invariant of the callers. `BladeRenderer::draw`
  submits the frame, waits for the *previous* submission, then destroys the
  retired textures. That is sufficient only because every `cx.drop_image`
  call in bibli runs from an update or render handler, before the current
  frame's scene is built: a texture retired there can belong to the previous
  submission (waited for) but never to the scene being drawn. Calling
  `drop_image` from inside an element's `paint` would retire a texture that
  primitives already recorded in the in-flight scene still reference, and the
  renderer would free it while the GPU may still read it. Keep image drops in
  `update`/`render` code such as `replace_preview_content`, the reader page
  cache and the thumbnail LRU eviction; never in `prepaint` or `paint`.

## Regenerating a patch

    G=$(ls -d ~/.cargo/registry/src/*/gpui-0.2.2)
    (cd / && diff -ruN --exclude=.cargo_vcs_info.json --exclude=Cargo.toml.orig \
        --exclude=.cargo-ok "${G#/}" $PWD/vendor/gpui) > vendor/patches/NNNN-name.patch

then rewrite the path prefixes as above. `0001` excludes `taffy.rs`, which is
`0002` on its own.
