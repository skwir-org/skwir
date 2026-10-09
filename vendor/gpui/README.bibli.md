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
  It also fixes which text layouts are reused. Flexbox probes a text without
  a width before it gives it one; 0.2.2 answered the later measure from that
  probe, so a `truncate()`d text in a flex row was never truncated, only
  clipped by its parent (every long name in the list view). A layout is now
  reused only between two unconstrained measures, each measure starts from
  the runs of the whole text, and a text whose shaped width fits is left
  alone even when its summed character advances do not.
  Zed's main branch has since grown the same three things
  (`TextOverflow::TruncateMiddle`, `truncate_wrapped_line`, the cache rule);
  this patch keeps their public names so callers survive an upgrade, and
  should be dropped, not upstreamed, when gpui is updated.

- `0006-text-run-positions.patch`: a run of text can sit off the baseline
  (`RunPosition`: `Superscript`, `Subscript`, or `Reduced` for synthesized
  small capitals), set on `TextRun`, `HighlightStyle` or `TextStyle`. Each
  platform shapes a line at one size, so `LineLayoutCache::layout_line` shapes
  a line whose runs are all on the baseline as before, in one call, and
  otherwise shapes each stretch of one position on its own at its size
  (`scale`) and joins them: glyph positions and indices are offset to the
  line's, `ShapedRun::position` keeps each run's, and the line's ascent and
  descent are its text's at full size, so its height and baseline do not
  move. `paint_line` draws each run's glyphs at its size, raised or lowered
  by its `rise`. `FontRun` and `ShapedRun` gain the field; the platform
  shapers only set `Baseline` in the runs they build. The mac and Windows
  edits are mechanical and not compiled here. A line wrapper measuring
  single characters for truncation still measures them at the line's size.
  `just list-check` runs the layout test with the list and wrapper tests.

- `0007-keyboard-click-needs-its-key-down.patch`: a focused element's
  Enter or Space key-up is a click only when the element was focused as the
  key went down. `Window::keyboard_click_origin` records the focused element
  on the key-down, before any binding or listener can move the focus, and
  the element's key-up listener fires its click listeners only for itself.
  Without it, a dialog that closes on Enter's key-down and gives the focus
  back to the button that opened it (a picker opened from a toolbar button)
  clicked that button with the same key's release, opening it again. The
  record is the window's because bindings run before an element's key-down
  listeners: a button that consumes Enter through its binding never sees
  the key-down of its own keyboard click. A test in `elements::div`
  (`keyboard_click_tests`), run by `just list-check`.

- `0008-clamp-image-sampling-to-its-atlas-tile.patch`: images share the
  polychrome atlas without padding between them (`padding: 0`), and sprites
  are sampled with a linear filter. An image drawn larger than its pixels
  (a map tile at a fractional zoom, 1 to 1.41 times) then filtered its edge
  texels with whatever lay beside it in the atlas, weighted up to
  `0.5 - 0.5 / scale`: another tile, invisibly, but a slot left empty
  showed as a darker line along the image's edge (a tile's outline across
  a map). The polychrome sprite fragment shader clamps its coordinate to
  half a texel inside the sprite's atlas tile, so an image samples only
  itself; inside the tile nothing changes. WGSL (Blade), Metal and HLSL
  alike; only the WGSL is compiled here, the others are mechanical.
  Verified on Xvfb with lavapipe: a map opened on a fresh start and zoomed
  in half a level showed a tile's outline at the same place on every
  start, and does not with the patch.

- `0009-image-formats-gpui-decodes.patch`: gpui asked for `image` with its
  default features, which include `avif`: in `image` 0.25 that is the AVIF
  *encoder*, rav1e, 2.4 MB of machine code nothing calls (decoding AVIF is
  the separate `avif-native`). gpui now names the formats it decodes itself
  (`ImageFormat`: PNG, JPEG, WebP, GIF, BMP, TIFF); bibli's crates name
  theirs, and features add up, so what bibli decodes is unchanged. Also
  gone: Radiance HDR and farbfeld, which nothing reaches, and `image`'s
  `rayon` (parallel OpenEXR decoding; our previews run on workers already).
  In Zed the dependency is declared in the workspace `Cargo.toml`, so the
  same change belongs there upstream; this diff is against the published
  manifest.

- `0010-key-sequence-timeout.patch`: a keystroke that begins a binding of
  several strokes (`g g`) waited one second for the next and was then
  replayed alone, a constant in `Window::dispatch_key_event`.
  `App::set_key_sequence_timeout` sets it; `None` waits until the next key,
  as yazi and a modal editor do. bibli waits (ADR-024) and shows what may
  follow (`crates/ui/src/sequence.rs`), so reading the strip is not a race.
  A change of focus still drops the pending keys, as before.

- `0011-optional-tooltip.patch`: `optional_tooltip`, on `Interactivity`
  and `StatefulInteractiveElement`: a tooltip whose builder may return
  `None`, so nothing shows for that element and a tooltip of an element it
  lies in shows instead; the builder is asked again on the next move.
  A builder that declines no longer asks for a redraw, which gpui did
  after every delayed build whatever it returned.
  `TooltipBuilder` already ended in an `Option` (the tooltip machinery
  takes one, and `InteractiveText` declines through it), so `tooltip` and
  `hoverable_tooltip` now wrap theirs in `Some`. bibli uses it for a text
  cut to fit (`crates/ui/src/elements/clip.rs`): whether it was cut is
  known only after layout, so its tooltip decides when shown.
  `InteractiveText` could decline already, but it notifies its view each
  time the pointer crosses a character, and bibli's shell is one view.
  Measured in a release build on Xvfb, sweeping the pointer: 55 such
  tooltips per draw (a list) cost nothing measurable; 551 (a CSV of 20
  columns, one per cell) about 0.3 ms per draw (median 7.55 to 7.86 ms)
  and 0.03 ms per mouse move.

- `0012-wayland-frames-on-request.patch`: a Wayland window draws when it
  needs a frame, not at the display's rate. 0.2.2 asked for the next
  frame callback in every frame callback and committed the surface each
  time, drawn or not, so a visible idle window woke its main thread every
  refresh (60 times a second, about 1% of a core on the reference
  machine, and the compositor's time too); every platform polls
  `WindowInvalidator::is_dirty` that way. `PlatformWindow::frame_requester`
  (default `None`: a platform that polls, X11 still among them) gives the
  window a callback, which `WindowInvalidator` calls whenever it marks the
  window dirty (`set_dirty`, `invalidate_view`) and `Window::on_next_frame`
  when a callback waits for the next frame. On Wayland it pings a calloop
  source (`FrameSchedule`), and the event loop asks the compositor for a
  callback unless one is on its way: a ping, not an idle, because gpui
  runs its foreground tasks inside calloop idles and 0.13 does not wake
  for an idle queued from one, so the request would wait for the next
  event. `draw` asks for the next callback on the commit that presents
  the frame, so frames that follow one another (an animation, the second
  of presenting after input) come once per refresh, and the first frame
  with nothing to draw lets the window sleep. Dropping the window and its
  requester drops the ping, which removes the source. Before the first
  configure no callback is asked for: the configure draws the first frame
  itself. Measured on Hyprland: the idle main thread went from 60 wakes a
  second to the compositor's liveness ping (`xdg_wm_base.ping`, every
  1.5 s), idle CPU from 1.0 to 0.2 %. Checked in a headless sway
  (lavapipe) on a socket of its own: keys, a file created by another
  program with no input, a file created while the window was on another
  workspace (shown when it came back; 0 CPU while hidden), a resize, and a
  toast's entrance animation, each drawn without input.

- `0013-renderer-starts-with-its-transparency.patch`: the Wayland window
  created its renderer transparent whatever it would be, and `Window::new`
  then asked for server decorations and set an opaque background, which
  reconfigured the surface and built every pipeline again before the first
  frame (about 10 ms on the reference machine's NVIDIA driver).
  `WindowParams` now carries `window_background` and `window_decorations`,
  and the renderer starts with the transparency `is_transparent` will give
  once the window has set them. A compositor that answers with client-side
  decorations still switches it, as before. The mac edit is mechanical and
  not compiled here.

- `0014-gpu-context-on-a-thread.patch`: `BladeContext::spawn` creates the
  GPU context on a thread of its own and `PendingBladeContext::join`
  waits for it; the Wayland and X11 clients start it first, so loading the
  drivers (the longest step of their setup) overlaps the fonts and the
  display connection instead of following them. Without a thread it is
  created in place. On the reference machine, with a warm page cache, the
  fonts take about 7 ms, which is what it saves there; after a boot both
  come from disk.

With 0012 to 0014, bibli's first frame went from a median of 134.6 ms to
114.5 ms on the reference machine (ten warm starts of each build,
alternated back to back on a quiet machine); 0013 and 0014 are the ones on
that path.

- `0015-stacksafe-1.patch`: gpui asked for `stacksafe` 0.1, whose
  attribute macro is built on `proc-macro-error2`, code a future Rust
  refuses (cargo's future-incompatibility warning on every build).
  `stacksafe` 1.0 is the same crate with its macro on `syn` alone: the
  same `StackSafe` and `#[stacksafe]`, expanded to the same
  `stacker::maybe_grow` with the same sizes, and the same check in debug
  builds that a `StackSafe` is reached only inside such a function; gpui's
  code is unchanged. In Zed the dependency is declared in the workspace
  `Cargo.toml`; this diff is against the manifest as `0009` left it.

- `0016-x11-ends-when-its-server-is-gone.patch`: when the X server went
  away (it ended, its socket closed), the X11 client never noticed. libxcb
  never recovers a connection in error, and its socket stays readable, so
  calloop called the event source again at once, forever: each poll failed,
  logged `error while polling for X11 events` (a backtrace with it where
  one is captured) and returned as if nothing had happened, the refresh
  timers the same. A bibli left on an Xvfb that was stopped spun a core and
  wrote 131 GB of log in four hours, until the disk was full; on the
  reference machine the log grew 27 MB a second. Now a poll that fails
  on a connection in error (`XCBConnection::has_error`; a single event that
  does not parse is not one, and still only warns) says so once and stops
  the event loop (`X11Client::connection_lost`), so the application ends
  as when it quits: `LinuxPlatform::run` runs the quit callbacks (bibli's
  winds its plans down). Windows dropped then are not destroyed on a
  server that is gone, which logged a failure with a backtrace for each.
  Checked on Xvfb, stopping the server under a window: the app ended
  within a second, its log two lines longer (the clipboard's worker says
  it once too). Wayland's client already ends its loop on a lost
  connection: its source returns the error.

- `0017-wrap-after-an-overflowing-space.patch`: `LineWrapper::wrap_line`
  remembered the character before the next one (`prev_c`) only when that
  character did not wrap the line. When a space overflowed it, the word
  after it did not follow a space, so it was no place to break, and the
  next line broke wherever its width ran out, inside a word. The shaped
  layout (`LineLayout::compute_wrap_boundaries`) remembers every
  character and breaks before the word, so the two disagreed after every
  such space. What asks the wrapper where lines break and lets the layout
  draw them went wrong: `0005`'s clamped truncation took the last line to
  start inside a word, kept the rest of that word before the ellipsis,
  and the layout, starting the line a word earlier, drew it wider than the
  box, cut at both ends (a grid name of three lines); bibli's grid counts
  each name's lines with it too. Now the character is remembered before
  the line is checked. Zed's main branch has the same order: the fix and
  the `wrap_line` test are for upstream, the clamped test is `0005`'s.
  `just list-check` runs both.

- `0018-ellipsis-is-no-place-to-break.patch`: `…`, the ellipsis GPUI
  truncates with, is a word character (`LineWrapper::is_word_char`, which
  both wrappers ask), as Zed's `⋯` already was. As any character neither
  a letter nor a space, it was a place to break, and the latest one before
  the width ran out: when a clamped text's last line, cut in its middle,
  had its ellipsis where the line above overflowed, the layout broke
  before it and not where the truncation started the line ("portrait,
  anothe" over "…ere.jpg"). Nothing is lost on other text: a word after
  an ellipsis still follows its space.

- `0019-font-id-without-fallback.patch`: `TextSystem::font_id` is
  public, so a caller can ask whether a family loads without
  `resolve_font`, which falls back to its own stack and panics when none
  of that is installed either. bibli picked its font by resolving a
  family that does not exist and comparing: on a minimal system with
  only Liberation or Inter, that panicked before the window opened.

## Regenerating a patch

    G=$(ls -d ~/.cargo/registry/src/*/gpui-0.2.2)
    (cd / && diff -ruN --exclude=.cargo_vcs_info.json --exclude=Cargo.toml.orig \
        --exclude=.cargo-ok "${G#/}" $PWD/vendor/gpui) > vendor/patches/NNNN-name.patch

then rewrite the path prefixes as above and delete the `diff -ruN` lines,
which name the folders it was made in (a home folder among them): `git
apply` reads the `---` and `+++` lines alone. `0001` excludes `taffy.rs`, which is
`0002` on its own. A patch over files an earlier one changed (`0006` touches
the text system `0005` did) is the diff against the committed vendored tree:
`git diff HEAD -- vendor/gpui`, with the same prefixes rewritten.
