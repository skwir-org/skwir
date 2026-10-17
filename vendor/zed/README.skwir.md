# Vendored gpui (Zed's main branch)

Zed's gpui crates at a commit of its main branch (`UPSTREAM-COMMIT`), plus
Skwir's patches. Skwir depends on `gpui` and `gpui_platform` by path (the root
`Cargo.toml`); ADR-064 records why, and what it cost.

- **The crates** are those `gpui` and `gpui_platform` reach through path
  dependencies, build and target ones included, as Zed has them, and the
  licence and toolchain files beside them.
- **The workspace manifest** is Zed's, trimmed to these crates: its dependency
  table stays as Zed wrote it, with its pins, git revisions included (wgpu
  between releases, Zed's forks of font-kit, xim and scap). Its other members,
  its profiles and its `[patch.crates-io]` are left out. This workspace is
  excluded from Skwir's.
- **Zed's crate patches:** Skwir's root manifest repeats those of Zed's
  `[patch.crates-io]` entries that reach gpui's crates: khronos-egl, async-task,
  calloop and async-process. It leaves out notify, which only Skwir's own
  watcher uses. `deny.toml` names every git source these bring.
- **The patches** are diffs in `../patches/`, in Zed's layout (`crates/gpui/...`).
  Each one's first line says what it does. They apply in order at Zed's root
  with `git apply`.

Tests covering the patches run on a copy, so gpui's dev dependencies stay out
of Skwir's workspace. `just gpui-check` runs, alongside upstream's tests of the
same modules:

- gpui's list, wheel-axis, line wrapper, line layout, style and key dispatch
  tests;
- all of gpui_wgpu's library tests (the atlas, the context, the renderer),
  which need a wgpu adapter (lavapipe will do);
- gpui_linux's clipboard tests.

gpui's library tests embed two of Zed's fonts, which the script fetches once
at the vendored commit.

## Patches

| Patch | What | Upstream |
| --- | --- | --- |
| `0001-wayland-drag-icon` | A file drag handed to the compositor shows the caller's icon | — |
| `0006-text-run-positions` | Superscript, subscript, small-caps runs | — |
| `0008-clamp-image-sampling-to-its-atlas-tile` | No atlas neighbour bleeding into an image's edge | — |
| `0010-key-sequence-timeout` | Configurable (or no) key sequence timeout | — |
| `0011-optional-tooltip` | A tooltip builder may decline | — |
| `0013-renderer-starts-with-its-transparency` | No pipeline rebuild before the first frame | — |
| `0017-wrap-after-an-overflowing-space` | Wrapper and layout agree after an overflowing space | — |
| `0019-font-id-without-fallback` | `TextSystem::font_id` public | — |
| `0020-render-image-into-frames` | `RenderImage::into_frames` returns pixel buffers | — |
| `0022-wheel-scrolls-along-its-axis` | The wheel scrolls along its own axis by default | — |
| `0023-idle-inhibit` | `Window::set_idle_inhibited` keeps the screen awake (Wayland, X11, Windows) | — |
| `0024-clipboard-files` | Files on the Linux clipboard, read off the main thread; X11 sends large data in pieces (0021) | — |
| `0025-vulkan-alone` | GL only where Vulkan offers no GPU, or none that can draw to the window | — |
| `0026-hide-cursor-until-mouse-moves` | `App::hide_cursor_until_mouse_moves` public | — |
| `0027-manifest-built-on-windows` | The Windows manifest embedded only when built on Windows | — |
| `0028-gpui-not-published` | gpui unpublished, as Skwir's path dependency | Skwir's only |

The numbers are those of the gpui 0.2.2 series where a patch came from it.

### Gone with gpui 0.2.2

Zed's main branch has its own answer to each of these patches:

- **0002** (taffy's `f32` literals): taffy 0.13.
- **0003** (Blade atlas retirement): wgpu destroys a texture only once the GPU
  is done with it.
- **0004** (list item height hints): `ListItem::Unmeasured`'s `size_hint`, and
  a list remeasures an item whose height changed.
- **0005** (clamped truncation, middle ellipsis): `TruncateFrom::Middle` and
  `truncate_wrapped_line`.
- **0007** (a keyboard click needs its key-down): an activation key clicks only
  in the focus generation it went down in.
- **0009** (image formats): Zed's workspace takes `image` without its default
  features. It has no AVIF encoder, but it decodes more formats than 0.2.2 did.
- **0012** (Wayland frames on request): the frame loop parks when nothing asks
  for a frame.
- **0015** (stacksafe 1): Zed's workspace has it.
- **0016** (X11 ends when its server is gone): checked on Xvfb. The app ends
  when the server does.
- **0018** (an ellipsis is no place to break): `…` is closing punctuation in
  `is_word_char`.

Not ported:

- **0014** (GPU context on a thread): a few milliseconds of the first frame.
- **0021**: now part of 0024.

### 0001: Wayland drag icon

Upstream hands an in-window drag to the platform as the pointer leaves the
window with the button held (`external_drag_payload`, on Wayland and macOS).
The drag carries no image of its own. `FileDragPaths::with_icon` gives it one:
an `ExternalDragIcon`, RGBA8 with straight alpha, and the pixel under the
pointer.

On Wayland:

- The icon goes into a `wl_shm` buffer (ARGB8888, premultiplied) on a surface
  of its own, passed to `start_drag`.
- The hotspot is set with `wl_surface.offset`, or with the attach offset before
  version 5.
- The surface is destroyed when the drag ends or is replaced.

macOS shows the icons of the files' types. Neither upstream nor 0.2.2 hands a
drag out of the window on X11 or Windows.

Skwir draws the same icon under the pointer inside the window
(`dnd::DragPreview`), so crossing the edge changes nothing. Checked on a
headless sway with a virtual pointer:

- In the window, the preview showed the icon.
- Past the edge, sway drew the same icon at the pointer: the thumbnail once the
  thumbnail service had it, a glyph before.
- Released outside, the drag ended and nothing stayed on screen.

### 0006: Text run positions

A run can sit off the baseline (`RunPosition`: `Superscript`, `Subscript`, or
`Reduced` for synthesized small capitals), set on `TextRun`, `HighlightStyle`
or `TextStyle`.

- **Shaping.** Platforms shape a line at one size, so the line layout's
  `shape` shapes a line with any off-baseline run in stretches of one position.
  Each stretch is shaped at its own size (`scale`), then the stretches are
  joined. Both layout paths go through it.
- **Each run's position.** `ShapedRun::position` keeps it: `FontRun` and
  `ShapedRun` gain the field, and every platform's shaper fills it.
- **Height and baseline.** The line's ascent and descent stay those of its
  full-size text, so neither moves. `paint_line` paints each run at its own
  size, raised or lowered by its `rise`.

Single characters measured for truncation are still measured at the line's
size. The Apple and web shaper edits are not compiled here. The tests are
`text_system::line_layout::run_position_tests`.

### 0008: Clamp image sampling to its atlas tile

Images share the polychrome atlas without padding and are sampled linearly. So
an image drawn larger than its pixels blended its edge with the neighbouring
tile, for example a map tile at a fractional zoom, 1 to 1.41×. The blend was
weighted up to `0.5 - 0.5 / scale`, so an empty slot showed as a dark line:
tile outlines across a map.

The sprite fragment shader now clamps to half a texel inside the sprite's tile.
The fix is in WGSL (wgpu), Metal and HLSL alike; only WGSL is compiled here.

### 0010: Key sequence timeout

A keystroke starting a multi-stroke binding (`g g`) waited one second, then
replayed alone. `App::set_key_sequence_timeout` sets that wait; `None` waits
for the next key, as yazi and modal editors do. Skwir waits (ADR-024) and shows
what may follow (`crates/ui/src/sequence.rs`), so reading the strip is not a
race. A focus change still drops pending keys.

### 0011: Optional tooltip

`optional_tooltip`, on `Interactivity` and `StatefulInteractiveElement`, takes
a builder that may return `None`:

- The element shows nothing, and an enclosing element's tooltip shows instead.
- The builder is asked again on the next move.
- A declining builder requests no redraw.

`TooltipBuilder` returns an `Option`, so `tooltip` and `hoverable_tooltip` wrap
theirs in `Some`. Skwir uses it for text cut to fit
(`crates/ui/src/elements/clip.rs`), known only after layout.

Cost, measured on 0.2.2 (release build, Xvfb, the pointer sweeping):

- 55 such tooltips per draw: not measurable.
- 551 (a 20-column CSV): about 0.3 ms per draw and 0.03 ms per mouse move.

### 0013: Renderer starts with its transparency

The Wayland renderer started transparent. `Window::new` then set server
decorations and an opaque background, which reconfigured the surface. wgpu
builds a pipeline per transparency, so every pipeline was built a second time
before the first frame.

`WindowParams` now carries `window_background` and `window_decorations`, so
the renderer starts as `is_transparent` will report. On lavapipe, the pipeline
phase before the first frame went from 66 to 35 ms. A compositor that answers
with client-side decorations still switches it.

### 0017: Wrap after an overflowing space

When a line wrapped at a candidate, `LineWrapper::wrap_line` did not remember
the character that overflowed. If a space overflowed, the next word did not
follow a space, so the next line broke inside it. The shaped layout never
breaks there. Skwir's grid counts lines with the wrapper, so a three-line name
was cut at both ends.

The character is now remembered when the line wraps at a candidate. Upstream's
`test_extra_columns_overflow_guard` covers the other branch, which is
unchanged. `wrap_line`'s test has the case: "aa bbbb ccccc" breaks at 3 and 8.

### 0019: Font id without fallback

`resolve_font` falls back to its own stack and panics when none of it is
installed. A public `TextSystem::font_id` asks whether a family loads without
fallback. Skwir picks its font that way.

### 0020: Render image into frames

`RenderImage::into_frames` returns an image's frames with their pixel buffers.
Video frames are same-size images made thirty times a second. The window hands
each buffer back to the player once nothing draws it, and the next frame is
read into it instead of into a fresh buffer the kernel maps and zeroes.

### 0022: The wheel scrolls along its axis

Scroll containers take a wheel event together, from the innermost out, and
none stops it. A container that scrolls along one axis took the wheel's other
axis as its own. So a vertical list inside a row that scrolls sideways
(Skwir's board) scrolled with the wheel while the row moved sideways too.

Upstream's `restrict_scroll_to_axis` is now on by default:

- The wheel alone scrolls down.
- Shift and the wheel, which every platform reports sideways, scroll across.
- An element that should take either axis sets it to `false`, such as a strip
  of tabs nothing else scrolls around.

`elements::div::scroll_axis_tests` cover a list in a row that scrolls sideways,
and the opt-out.

### 0023: Idle inhibit

`Window::set_idle_inhibited(bool)` keeps the screen on while a film or a
slideshow plays, with nothing touched for minutes. It is a `PlatformWindow`
method, ignored by default.

- **Wayland:** a `zwp_idle_inhibitor_v1` on the window's surface, which holds
  while the surface is shown.
- **X11:** `ScreenSaverSuspend` (MIT-SCREEN-SAVER 1.1), which Xorg also applies
  to the display's power saving. Suspensions nest per client, one per window
  that asks, and the server ends them when the client goes.
- **Windows:** `SetThreadExecutionState` asks for the display while any window
  holds it. Compiled only.
- **macOS:** not yet.

A window releases what it holds when it closes.

### 0024: Files on the clipboard

This is Skwir's ADR-062. It adds `ClipboardFiles` (the paths, whether they were
cut, and what named no local file) and four `App` methods. Each is a `Platform`
method, with a default for macOS and Windows: the text alone.

- **`write_files_to_clipboard(files, text)`** offers both families of Linux
  file managers, which do not read each other:
  - GTK's `x-special/gnome-copied-files` and MATE's same bytes as
    `x-special/mate-copied-files`;
  - KDE's `text/uri-list`, with `application/x-kde-cutselection`;
  - the text.
- **`read_files_from_clipboard()`** reads another client's list, off the main
  thread.
- **`clear_clipboard_files()`** empties the clipboard if it still holds this
  application's files.
- **`on_clipboard_files_gone(callback)`** says when the files are no longer
  there.

The formats are in `gpui_linux`'s `clipboard_files.rs`, with their tests. The
details are those of the 0.2.2 patch (ADR-062).

**0021 is part of this patch.** An X11 clipboard larger than one request is sent
with INCR (ICCCM 2.7.2), and one request that cannot be answered no longer ends
the thread that serves the clipboard.

### 0025: Vulkan alone

Upstream's instance has both of wgpu's Linux backends, Vulkan and GL. Making
the GL instance loads the GL driver beside Vulkan's: on NVIDIA, EGL and its GL
core, 16 MiB of mapped code and about a second of the first frame on a busy
machine.

The window's first context now uses Vulkan alone where Vulkan offers a GPU (an
adapter that is not a CPU's) that can draw to the window
(`WgpuContext::vulkan_alone`, `first_context`). Elsewhere it uses both
backends, ranked GPU first as upstream ranks them:

- **No Vulkan driver** (an old GPU): GL draws.
- **lavapipe alone beside a GPU only GL drives** (a virtual machine's): the GPU
  draws, not the CPU. Skwir's binary already keeps lavapipe from loading
  beside an NVIDIA, Intel, AMD or virtio GPU (`crates/skwir/src/gpu.rs`).
- **No GPU at all:** lavapipe draws.

Recovering a lost device takes the same path. Checked:

- On Xvfb with no Vulkan driver, llvmpipe drew through GL.
- With lavapipe alone, the session's NVIDIA GL was tried first. It failed
  wgpu's surface test on both Wayland and X11, and lavapipe drew.
- With EGL giving no display (Xvfb under NVIDIA's EGL), GL was left out
  without a panic: Zed's khronos-egl fix.

### 0026: Hide the cursor until the mouse moves

`App::hide_cursor_until_mouse_moves` makes public what upstream's
`CursorHideMode` does while typing. Skwir's fullscreen preview hides the
pointer with its controls (`crates/ui/src/fullscreen.rs`). Elsewhere Skwir
sets `CursorHideMode::OnTyping`: a key that types a character hides the
pointer, and a key that only moves (the arrows through files) leaves it.

### 0027: Windows manifest built on Windows

gpui's build script embeds a Windows manifest with MinGW's `windres`. It now
does so only when built on Windows, as 0.2.2 did, so `check-cross` and Wine's
tests build from Linux without it.

### 0028: gpui not published

`publish = false`: `cargo deny` allows a path dependency without a version
only from an unpublished crate.

## Adding or changing a patch

Make the series, a git repository whose first commit is Zed's crates and whose
next commits are the patches:

    python3 tools/gpui/vendor.py series <zed checkout at UPSTREAM-COMMIT> <dir>

Edit there:

- **A new patch** is a commit named `NNNN name: what it does`.
- **A change to a patch** is a fixup of its commit (`git commit --fixup`, then
  `git rebase -i --autosquash`).

Then `python3 tools/gpui/vendor.py export <dir>` writes the patches and replaces
`vendor/zed` (this file stays). Add a row and a section here.

## Updating gpui

1. Run `series` on a checkout of a newer Zed commit. A patch that no longer
   applies stops it: port it in `<dir>`, commit it under its name, then run
   `series --resume <dir>`.
2. `series` prints Zed's `[patch.crates-io]`. Compare it with Skwir's root
   manifest, and the vendored manifest's git pins with `deny.toml`.
3. Read what moved in the vendored source, not in memory. A patch upstream has
   absorbed goes into "Gone with gpui 0.2.2" or its own list.
4. `export`, then build Skwir and run `just gpui-check`, `just lint-all`, `just
   deny` and the window checks.
5. Measure with `just idle-pss` against the previous build, back to back.
