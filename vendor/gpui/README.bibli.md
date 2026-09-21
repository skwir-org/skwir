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

- `0004-list-item-height-hints.patch`: `ListItem::Unmeasured` carries an
  optional `size_hint`, the sum tree counts it, and `ListState` gains
  `with_uniform_item_height` / `reset_with_uniform_height`. A list measures
  only what it renders, so `max_offset_for_scrollbar` described the handful
  of items drawn and a scrollbar built from it claimed a long document was
  nearly all visible. A hint gives every item a height until it is measured,
  and real heights replace it as items are drawn, so the estimate converges
  instead of jumping. `ListState::remeasure_items` comes with it: a caller
  whose item changed height wants that item measured again, not replaced,
  and `splice` would drop what it knew — a document that loses a page's
  height every time a page finishes rendering has a scrollbar that jumps
  under the hand dragging it. This mirrors the same two mechanisms in Zed's
  current `list.rs`; `just list-check` runs the list tests, ours included.

- `0005-truncate-last-clamped-line.patch`: a text with `line_clamp(n)` and an
  ellipsis was truncated to `n` line widths *before* wrapping. Wrapping ends
  each line at a word, so the earlier lines never use their whole width and
  the last one overflows by what they left: no ellipsis in sight, and clipped
  at both ends when the text is centred (every two-line name in the grid).
  `LineWrapper::truncate_clamped_line` finds where the last allowed line
  begins and truncates that line to one width; text with hard line breaks
  keeps the old behaviour. The suffix follows the last word, not the space
  after it. The same patch adds `TextOverflow::TruncateMiddle` and
  `Styled::text_ellipsis_middle`: the ellipsis replaces the middle of the
  line (the last allowed line, when clamped) and both ends share what is
  left, so a file name keeps its extension. Runs shrink over the removed
  bytes and the ellipsis joins the run it began in. `just list-check` runs
  the line wrapper tests too.

## Regenerating a patch

    G=$(ls -d ~/.cargo/registry/src/*/gpui-0.2.2)
    (cd / && diff -ruN --exclude=.cargo_vcs_info.json --exclude=Cargo.toml.orig \
        --exclude=.cargo-ok "${G#/}" $PWD/vendor/gpui) > vendor/patches/NNNN-name.patch

then rewrite the path prefixes as above. `0001` excludes `taffy.rs`, which is
`0002` on its own.
