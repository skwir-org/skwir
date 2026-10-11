# Vendored gpui 0.2.2

The `gpui` 0.2.2 crate exactly as published on crates.io, plus skwir's
patches, wired in through `[patch.crates-io]` in the root `Cargo.toml`.
Remove both once the patches are upstream.

Each patch is a diff in `../patches/`, its paths rewritten to the Zed monorepo
layout (`crates/gpui/...`) so it applies there with `git apply -p1`.

Tests covering the patches run on an isolated copy, so GPUI's dev
dependencies stay out of skwir's workspace:

- `just list-check`: list, line wrapper, line layout and keyboard click
  tests (0004 to 0007, 0017). No GPU needed.
- `just atlas-check`: the Blade atlas tests (0003). Needs a working GPU
  context (Linux, dev shell).

## Patches

"—": no upstream status recorded.

| Patch | What | Upstream |
| --- | --- | --- |
| `0001-wayland-outgoing-drag` | Drag files out of the window on Wayland | — |
| `0002-taffy-f32-literals` | `f32` suffix on two float literals | — |
| `0003-blade-atlas-retirement` | Free atlas textures only once the GPU is done | — |
| `0004-list-item-height-hints` | Height estimates for unmeasured list items | Zed's current `list.rs` has both mechanisms |
| `0005-truncate-last-clamped-line` | Clamped truncation, middle ellipsis, layout reuse | In Zed main: drop on update, do not upstream |
| `0006-text-run-positions` | Superscript, subscript, small-caps runs | — |
| `0007-keyboard-click-needs-its-key-down` | A key-up clicks only the element its key-down focused | — |
| `0008-clamp-image-sampling-to-its-atlas-tile` | No atlas neighbour bleeding into an image's edge | — |
| `0009-image-formats-gpui-decodes` | `image` without its AVIF encoder | Belongs in Zed's workspace `Cargo.toml` |
| `0010-key-sequence-timeout` | Configurable (or no) key sequence timeout | — |
| `0011-optional-tooltip` | A tooltip builder may decline | — |
| `0012-wayland-frames-on-request` | Wayland draws on request, not every refresh | — |
| `0013-renderer-starts-with-its-transparency` | No pipeline rebuild before the first frame | — |
| `0014-gpu-context-on-a-thread` | GPU context created in parallel with setup | — |
| `0015-stacksafe-1` | `stacksafe` 1.0 instead of 0.1 | Belongs in Zed's workspace `Cargo.toml` |
| `0016-x11-ends-when-its-server-is-gone` | X11 app ends when its server is gone | — |
| `0017-wrap-after-an-overflowing-space` | Wrapper and layout agree after an overflowing space | Zed main has the same order; the fix and its `wrap_line` test are for upstream |
| `0018-ellipsis-is-no-place-to-break` | `…` is a word character | — |
| `0019-font-id-without-fallback` | `TextSystem::font_id` public | — |
| `0020-render-image-into-frames` | `RenderImage::into_frames` returns pixel buffers | — |

### 0001: Wayland outgoing drag

Adds `Window::start_external_drag(paths, icon)` (a `PlatformWindow` method,
unsupported by default) and `ExternalDragIcon` (RGBA8 pixels and a hotspot).
The Wayland client offers `text/uri-list` from a `wl_data_source`, draws the
icon on a `wl_shm` surface (premultiplied ARGB8888; hotspot via
`wl_surface.offset`, or the attach offset on older compositors), starts the
drag with the last mouse-press serial, and on `Cancelled` or `DndFinished`
destroys the icon and synthesizes a mouse-up so the in-window drag ends. A drop
back onto skwir arrives as an ordinary `ExternalPaths` drop.

### 0002: taffy f32 literals

Rust 1.98 warns that the implicit float fallback in `taffy.rs` becomes a
hard error (`float_literal_f32_fallback`, rust-lang/rust#154024).

### 0003: Blade atlas retirement

Removing a Blade texture's last image cancels its queued initialization
and uploads before its atlas index is recycled; destruction waits for the
renderer's previous GPU submission, and views are destroyed before their
textures. This makes rapid preview replacement between drawing and
Wayland presentation safe, including index reuse by a smaller texture. Two
ignored GPU tests in `platform::blade::blade_atlas::tests` cover removal
before flush and slot reuse.

**Invariant: call `cx.drop_image` only from `update` or `render` code,
never from `prepaint` or `paint`.** `BladeRenderer::draw` submits the
frame, waits for the *previous* submission, then destroys retired
textures. A texture retired before the scene is built belongs at most to
the previous submission; one retired in `paint` may be referenced by the
in-flight scene and freed while the GPU reads it. skwir drops images in
`replace_preview_content`, the reader page cache and the thumbnail LRU.

### 0004: List item height hints

A list measures only what it renders, so `max_offset_for_scrollbar`
described only the items drawn, and a long document's scrollbar claimed
it was nearly all visible. `ListItem::Unmeasured` now carries an optional
`size_hint` that the sum tree counts (`ListState::with_uniform_item_height`,
`reset_with_uniform_height`); real heights replace it as items are drawn,
so the estimate converges instead of jumping. `ListState::remeasure_items`
re-measures an item whose height changed: `splice` would drop the known
height, and a scrollbar would jump under the hand each time a page
finished rendering.

### 0005: Truncate the last clamped line

- **Clamped truncation.** With `line_clamp(n)` and an ellipsis, text was
  truncated to `n` line widths *before* wrapping; lines end at words, so
  the last line overflowed: no ellipsis, and clipped at both ends when
  centred (every two-line grid name).
  `LineWrapper::truncate_clamped_line` truncates the last allowed line to
  one width. Text with hard line breaks keeps the old behaviour; the suffix
  follows the last word, not the space after it.
- **Middle ellipsis.** `TextOverflow::TruncateMiddle` and
  `Styled::text_ellipsis_middle` replace the middle of the (last) line, so
  a file name keeps its extension. Runs shrink over the removed bytes.
- **Layout reuse.** Flexbox probes a text without a width first, and 0.2.2
  answered the real measure from that probe, so a `truncate()`d text in a
  flex row was only clipped (every long name in the list view). A layout is
  now reused only between two unconstrained measures, each measure starts
  from the whole text's runs, and text whose shaped width fits is left
  alone even if its summed advances do not.

Zed main has all three (`TextOverflow::TruncateMiddle`,
`truncate_wrapped_line`, the cache rule); the patch keeps their public names so
callers survive an upgrade.

### 0006: Text run positions

A run can sit off the baseline (`RunPosition`: `Superscript`, `Subscript`, or
`Reduced` for synthesized small capitals), set on `TextRun`, `HighlightStyle`
or `TextStyle`. Platforms shape a line at one size, so
`LineLayoutCache::layout_line` shapes a line with any off-baseline run in
stretches of one position, each at its size (`scale`), and joins them;
`ShapedRun::position` keeps each run's (`FontRun` and `ShapedRun` gain the
field). The line's ascent and descent stay its full-size text's, so height and
baseline do not move; `paint_line` raises or lowers each run by its `rise`. The
mac and Windows shaper edits are mechanical and not compiled here. Single
characters measured for truncation are still measured at the line's size.

### 0007: Keyboard click needs its key-down

Enter or Space key-up clicks the focused element only if it was focused
when the key went down (`Window::keyboard_click_origin`, recorded before
any binding or listener can move focus). Otherwise a dialog closing on
Enter's key-down and refocusing the toolbar button that opened it was
clicked open again by the same key's release. The record is the window's
because bindings run before an element's key-down listeners. Tested in
`elements::div` (`keyboard_click_tests`).

### 0008: Clamp image sampling to its atlas tile

Images share the polychrome atlas without padding and are sampled
linearly, so an image drawn larger than its pixels (a map tile at a
fractional zoom, 1 to 1.41×) blended its edge with the neighbouring tile,
weighted up to `0.5 - 0.5 / scale`: an empty slot showed as a dark line
(tile outlines across a map). The sprite fragment shader clamps to half a
texel inside the sprite's tile. WGSL, Metal and HLSL alike; only WGSL is
compiled here. Verified on Xvfb with lavapipe: the outline that appeared at
the same place on every fresh start, zoomed half a level, is gone.

### 0009: Image formats gpui decodes

`image`'s default `avif` feature is the AVIF *encoder* (rav1e): 2.4 MB of
machine code nothing calls (decoding is `avif-native`). gpui now names the
formats it decodes (`ImageFormat`: PNG, JPEG, WebP, GIF, BMP, TIFF); skwir's
crates name theirs, and features add up, so what skwir decodes is unchanged.
Also gone: Radiance HDR, farbfeld (unreachable) and `rayon` (previews already
run on workers). The diff is against the published manifest.

### 0010: Key sequence timeout

A keystroke starting a multi-stroke binding (`g g`) waited one second (a
constant in `Window::dispatch_key_event`), then replayed alone.
`App::set_key_sequence_timeout` sets it; `None` waits for the next key, as yazi
and modal editors do. skwir waits (ADR-024) and shows what may follow
(`crates/ui/src/sequence.rs`), so reading the strip is not a race. A focus
change still drops pending keys.

### 0011: Optional tooltip

`optional_tooltip` (on `Interactivity` and `StatefulInteractiveElement`) takes
a builder that may return `None`: the element shows nothing, an enclosing
element's tooltip shows instead, and the builder is asked again on the next
move. A declining builder no longer requests a redraw. `TooltipBuilder` already
returned an `Option`, so `tooltip` and `hoverable_tooltip` now wrap theirs in
`Some`. skwir uses it for text cut to fit (`crates/ui/src/elements/clip.rs`),
known only after layout. `InteractiveText` could already decline, but it
notifies its view whenever the pointer crosses a character, and skwir's shell
is one view.

Cost (release build, Xvfb, pointer sweeping): 55 such tooltips per draw,
not measurable; 551 (a 20-column CSV), about 0.3 ms per draw (median 7.55
to 7.86 ms) and 0.03 ms per mouse move.

### 0012: Wayland frames on request

0.2.2 requested a frame callback in every frame callback and committed the
surface each time, so an idle visible window woke its main thread every
refresh (60/s, about 1% of a core on the reference machine, plus
compositor time).

`PlatformWindow::frame_requester` (default `None`: a polling platform, X11
among them) gives the window a callback that `WindowInvalidator` calls when it
marks the window dirty (`set_dirty`, `invalidate_view`), as does
`Window::on_next_frame`. On Wayland it pings a calloop source
(`FrameSchedule`), which asks the compositor for a callback unless one is on
its way. A ping, not an idle: gpui runs foreground tasks inside calloop idles,
and calloop 0.13 does not wake for an idle queued from one. `draw` requests the
next callback on the presenting commit, so continuous frames come once per
refresh and the first frame with nothing to draw lets the window sleep. No
callback is requested before the first configure, which draws the first frame.
Dropping the window drops the ping, which removes the source.

Measured on Hyprland: idle wakes went from 60/s to the compositor's
liveness ping (`xdg_wm_base.ping`, every 1.5 s); idle CPU from 1.0 to
0.2%. Checked in headless sway (lavapipe), each drawn without input: keys,
a file created by another program, a file created while the window was on
another workspace (shown on return, 0 CPU while hidden), a resize, a
toast's entrance animation.

### 0013: Renderer starts with its transparency

The Wayland renderer started transparent, then `Window::new` set server
decorations and an opaque background, reconfiguring the surface and
rebuilding every pipeline before the first frame (about 10 ms on the
reference machine's NVIDIA driver). `WindowParams` now carries
`window_background` and `window_decorations`, so the renderer starts as
`is_transparent` will report. A compositor that answers with client-side
decorations still switches it. The mac edit is not compiled here.

### 0014: GPU context on a thread

`BladeContext::spawn` creates the GPU context on its own thread
(`PendingBladeContext::join` waits), started first by the Wayland and X11
clients, so loading drivers overlaps loading fonts and connecting. Without a
thread it is created in place. With a warm page cache the fonts take about 7 ms
on the reference machine, which is the saving; after a boot both come from
disk.

**0012 to 0014:** skwir's first frame went from a median of 134.6 ms to
114.5 ms on the reference machine (ten warm starts per build, alternated on
a quiet machine); 0013 and 0014 are on that path.

### 0015: stacksafe 1

`stacksafe` 0.1's macro uses `proc-macro-error2`, which a future Rust
refuses (a future-incompatibility warning on every build). 1.0 is the same
crate on `syn` alone, expanding identically (`stacker::maybe_grow`, same
sizes, same debug check); gpui's code is unchanged. The diff is against the
manifest as 0009 left it.

### 0016: X11 ends when its server is gone

libxcb never recovers a connection in error and its socket stays readable, so
when the X server went away calloop polled forever, logging `error while
polling for X11 events` each time. A skwir left on a stopped Xvfb spun a core
and wrote 131 GB of log in four hours (27 MB/s on the reference machine). Now a
poll failing on a connection in error (`XCBConnection::has_error`; one
unparsable event still only warns) stops the event loop
(`X11Client::connection_lost`), and the app ends as if it quit:
`LinuxPlatform::run` runs the quit callbacks (skwir winds its plans down).
Windows are not destroyed on the dead server (each logged a failure with a
backtrace). Checked on Xvfb: the app ended within a second, its log two lines
longer. Wayland's client already ended on a lost connection.

### 0017: Wrap after an overflowing space

`LineWrapper::wrap_line` remembered the previous character only when it did not
wrap. After a space overflowed a line, the next word did not follow a space, so
the next line broke mid-word; the shaped layout
(`LineLayout::compute_wrap_boundaries`) broke before the word. With 0005, a
clamped last line started inside a word and was drawn wider than its box, cut
at both ends (three-line grid names); skwir's grid counts lines with the
wrapper too. The character is now remembered first. The clamped test is 0005's.

### 0018: Ellipsis is no place to break

`…`, GPUI's truncation ellipsis, is now a word character
(`LineWrapper::is_word_char`, asked by both wrappers), as Zed's `⋯` was. As a
break point, a middle-truncated clamped line whose ellipsis fell where the line
above overflowed broke before it ("portrait, anothe" over "…ere.jpg"). A word
after an ellipsis still follows its space.

### 0019: Font id without fallback

`resolve_font` falls back to its own stack and panics when none of it is
installed. skwir picked its font by resolving a nonexistent family and
comparing, which panicked before the window opened on a system with only
Liberation or Inter. A public `TextSystem::font_id` asks whether a family
loads without fallback.

### 0020: Render image into frames

`RenderImage::into_frames` returns an image's frames with their pixel
buffers. Video frames are same-size images made thirty times a second: the
window hands each buffer back to the player once nothing draws it (texture
dropped first), and the next frame is read into it instead of a fresh 8 MB
buffer at 1080p, mapped and zeroed by the kernel.

## Adding or regenerating a patch

Edit `vendor/gpui`, then diff it against the published crate:

    G=$(ls -d ~/.cargo/registry/src/*/gpui-0.2.2)
    (cd / && diff -ruN --exclude=.cargo_vcs_info.json --exclude=Cargo.toml.orig \
        --exclude=.cargo-ok "${G#/}" $PWD/vendor/gpui) > vendor/patches/NNNN-name.patch

- Rewrite the path prefixes to `crates/gpui/...`.
- Delete the `diff -ruN` lines: they name the folders it was made in (a
  home folder among them), and `git apply` reads only `---` and `+++`.
- `0001` excludes `taffy.rs`, which is `0002`.
- A patch over files an earlier one changed (`0006` over `0005`'s text
  system) is the diff against the committed tree,
  `git diff HEAD -- vendor/gpui`, prefixes rewritten.
- Add a row and a section here.

## Updating gpui

gpui's API moves between releases: read the vendored source, not memory. On an
update, drop 0005 (Zed main has its changes under the same names); 0004 and
0017 have equivalents in Zed main; 0009 and 0015 move to Zed's workspace
`Cargo.toml`. Run `just list-check` and `just atlas-check`.

Check whether the new gpui supports accessibility (AccessKit): skwir wires it
in once it does (ADR-001).

Once every patch is upstream, remove `vendor/gpui` and its `[patch.crates-io]`
entry.
