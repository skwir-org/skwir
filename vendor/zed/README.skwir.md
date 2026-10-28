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

`just gpui-check-windows` runs gpui_windows's tests of the mesh pass under
Wine, built for Windows from the same copy (`tools/gpui/check-windows.sh`).
Wine's Direct3D 11 draws on OpenGL, through Mesa's llvmpipe on an Xvfb of
its own.

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
| `0027-manifest-with-a-resource-compiler` | The Windows manifest embedded where a resource compiler is at hand: on Windows, or named (`RC`) | — |
| `0028-gpui-not-published` | gpui unpublished, as Skwir's path dependency | Skwir's only |
| `0029-gpu-context-on-a-thread` | Vulkan starts on a thread of its own while the platform sets up (0014, on wgpu) | — |
| `0030-active-descendant-for` | An element is said as focused while a focus handle beside it holds the focus (the combobox pattern) | — |
| `0031-aria-states` | Live regions, modal dialogs, lists of many selected, disabled and read-only elements | — |
| `0032-held-keys` | An action sees the key event it came from, held or not; X11 tells a held key from a press | — |
| `0033-meshes` | A triangle mesh drawn in 3D (`Window::paint_mesh`), by wgpu, Direct3D 11 and Metal | — |
| `0034-windows-shaders-at-start` | Built for Windows on another system, the shaders are compiled as the renderer starts | — |
| `0035-apple-shaders-at-start` | Built for macOS on another system, the shader bindings are made and the shaders compiled as the renderer starts | — |
| `0036-no-icu` | A jump list's removed entries are read without ICU's `u_strlen`, so gpui needs no icuuc.dll | — |
| `0037-middle-ellipsis-over-clamped-lines` | A clamped text cut in the middle keeps its first lines, and its last ends as the text does | — |
| `0038-press-cancels-a-waiting-tooltip` | A press cancels a tooltip still waiting to show | — |
| `0039-shifted-space-is-space` | The space bar with Shift is Space to key bindings whatever the layout types there (Linux) | — |
| `0040-list-keeps-height-hints-across-a-width-change` | A list keeps each item's height, measured or hinted, as its estimate when its width changes and on its first layout, so a scrollbar measures every item (0004's last hunk) | — |
| `0041-key-binding-for-a-system` | `KeyBinding::new_for_os` binds any string, its `secondary-` the given system's, without a keyboard mapper (`KeyBinding::new` takes a static string, and `load` maps keys by the layout) | — |

The numbers are those of the gpui 0.2.2 series where a patch came from it.

### Gone with gpui 0.2.2

Zed's main branch has its own answer to each of these patches:

- **0002** (taffy's `f32` literals): taffy 0.13.
- **0003** (Blade atlas retirement): wgpu destroys a texture only once the GPU
  is done with it.
- **0004** (list item height hints): `ListItem::Unmeasured`'s `size_hint`, and
  a list remeasures an item whose height changed. Not its last hunk: Zed's
  list drops every hint when its width changes, and on its first layout, so
  the reader's column counted only the pages laid out and its scrollbar
  ended at the last page visited (2026-10-28): 0040.
- **0005** (clamped truncation, middle ellipsis): `TruncateFrom::Middle` and
  `truncate_wrapped_line`, except a middle ellipsis over several lines, which
  Zed cuts to one: 0037.
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

Under another number:

- **0014** (GPU context on a thread): now 0029, on wgpu.
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

### 0027: Windows manifest with a resource compiler

gpui's build script embeds a Windows manifest (per-monitor DPI awareness,
Common Controls 6, the segment heap) with a resource compiler, MinGW's
`windres` unless one is named. It does so where one is at hand: built on
Windows, as 0.2.2 did, or where one is named as embed-resource reads it
(`RC`, `RC_<target>`). Skwir's Windows package names llvm-rc
(`tools/package/windows.sh`, ADR-074); `check-cross` and Wine's tests name
none and build from Linux without it.

### 0028: gpui not published

`publish = false`: `cargo deny` allows a path dependency without a version
only from an unpublished crate.

### 0029: GPU context on a thread

0014 on wgpu. Upstream makes the GPU context when the first window opens,
after the fonts, the connection to the display server and the app's own
setup. Loading the driver takes longer than any of them. On NVIDIA that is
the Vulkan loader, every ICD, NVIDIA's GL core, its shader compiler and its
ray-tracing core, which wgpu-hal's adapter query loads.

- `LinuxPlatform::new` starts Vulkan's instance and the enumeration of its
  adapters on a thread of its own (`WgpuContext::start_vulkan`). It does so
  where the environment names a display server the platform may connect to.
- `WgpuContext::vulkan_alone` takes that answer for the first window. It
  makes one in place where no thread could be made, and when a lost device
  is recovered.
- A platform that starts headless after all lets it go.
- Vulkan's instance takes no display: `create_surface` gives a surface its
  window's.

Ten starts of each build, alternated back to back on a quiet machine
(2026-10-18):
- the adapters were found 51 ms after the start instead of 59;
- the first frame came at 119 ms instead of 125 (medians); on gpui 0.2.2 it
  came at 109.

The device, made and tested on the window's surface, stays on the window's
path (ADR-064).

### 0030: Active descendant for a focus handle

Upstream's `aria_active_descendant` holds only under the focused element.
A picker keeps the keyboard in its text field while the highlighted option
is in a list beside it, not under it: the web's combobox, where
`aria-activedescendant` on the field names the option.
`aria_active_descendant_for(&FocusHandle)` says the element is focused
while that handle holds the focus, wherever its element is (ADR-065). Two
tests beside upstream's in `window/a11y.rs`.

### 0031: ARIA states

AccessKit nodes have states gpui's builder does not set:
`aria_live` (a node announced when it appears or its name changes; a
notification), `aria_modal`, `aria_multiselectable`, `aria_disabled` and
`aria_read_only`. `Live` is re-exported beside `Role`. A test beside
upstream's in `elements/div.rs`.

### 0032: Held keys

A key held down repeats as key-down events marked `is_held`, but an action
never saw the event it came from, and the X11 client marked none.
`Window::key_down_event` is the key-down event being dispatched, while the
actions it resolves to and its listeners run (`None` for any other event),
so an action tells a press from a repeat (ADR-066: a switch bound to a key
is not flipped over and over). The X11 client reports a press of the key
last pressed, not released since, as held, the one key the server repeats,
and asks for XKB's detectable auto-repeat, as GTK and Qt do, so no release
comes before each repeat. A test beside upstream's in `key_dispatch.rs`;
X11 checked by hand on Xwayland.

### 0033: Meshes

gpui draws no 3D: its primitives are fixed, and the one that shows an
outside image (`PaintSurface`) exists only on macOS. Skwir turns 3D models
in its preview (ADR-067).

- **`gpui::Mesh`** holds positions and the triangles between them, three
  indices each. `Mesh::new` refuses one without triangles, an index past
  the positions and a coordinate that is not finite. Each mesh has an id,
  which a renderer keeps its buffers by. `with_colors` gives each position
  a colour (sRGB bytes), drawn in place of the view's, blended across a
  face in linear light.
- **`Window::paint_mesh(bounds, mesh, view, cx)`** paints one, a
  primitive of its own (`PaintMesh`) in draw order and clipped by the
  content mask like any other, and says what it did (`MeshPainted`:
  painted, loading, or refused by a window without a 3D pass or a GPU that
  cannot hold it). Clip space spans the bounds. `MeshView` gives the model-view and projection matrices
  (column-major), whether the projection has perspective, the surface's
  colour in linear RGB, the ambient light and two lights fixed to the
  camera.
- **A mesh is uploaded before it is painted, off the main thread.**
  `paint_mesh` asks the platform window (`PlatformWindow::prepare_mesh`,
  `MeshPreparation`): a mesh on the GPU is painted; one that is not starts
  its upload and is not painted, and the window is drawn again once it
  arrives (a task awaiting the upload refreshes it). The renderer uploads
  on a thread of its own: NVIDIA's driver maps the memory uploads go
  through uncached, and 72 MB took a second there.
- **`gpui::MeshStore`** is that bookkeeping, for every renderer: it keeps
  a mesh's buffers by its id, makes them on its thread through the
  renderer's `MeshMaker` (how its GPU holds a mesh), the normals too once
  smooth shading asks, and lets go of what a frame did not draw
  (`end_frame`). Buffers the GPU could not make refuse the mesh for good
  (`MeshPreparation::Never`); normals it could not make leave it flat. Its
  thread ends with it: the store waits for the upload under way and skips
  the rest, so nothing of the GPU's is made or let go of once the renderer
  is gone (under Wine, a device let go of by a thread while the process
  exited had it killed).
- **`Window::draws_meshes`** says whether the window draws them
  (`PlatformWindow::draws_meshes`, false by default). The Wayland, X11,
  Windows and macOS windows say yes.
- **`PaintMesh::framed`** says where a mesh is drawn in a frame, alike for
  every renderer (`MeshFrame`): the part shown, in whole pixels, and the
  target's size, rounded up to 256 pixels; `MeshFrame::params` and
  `composite` are the words the shaders read, in the order and layout the
  WGSL declares them.
- **wgpu** (`wgpu_meshes.rs`, `shaders_mesh.wgsl`) draws each mesh in a
  pass of its own, as paths are drawn:
  - into a target the size of what is shown of it, rounded up to 256
    pixels, with a depth buffer (`Depth32Float`), multisampled as far as
    both formats allow, up to four samples;
  - each face shaded flat from the screen-space derivatives of its
    position, so no normals are sent, and lit on whichever side faces the
    camera; the colour encoded to sRGB as gpui's other colours are;
  - or smoothly (`MeshView::smooth`), by normals the upload thread makes
    when a frame first asks for them (`Mesh::smooth_normals`: each
    position's faces' normals weighted by their areas), oriented to the
    side of the face seen; until they are there the mesh is drawn flat
    and the window drawn again when they arrive (`MeshPreparation::Flat`).
    Without normals the positions' buffer stands in for theirs, unread;
  - then composited into the frame pixel for pixel, premultiplied.
- **Direct3D 11** (`directx_meshes.rs`) and **Metal** (`metal_meshes.rs`)
  draw it the same way, from naga's translations of the same WGSL
  (`gpui_windows/src/shaders_mesh.hlsl`, `gpui_apple/src/shaders_mesh.metal`),
  which a test of gpui_wgpu's writes and keeps current
  (`GPUI_WRITE_MESH_SHADERS=1` writes them again). The composite's
  bindings are apart from the mesh's, so Direct3D gives each a register of
  its own. Direct3D compiles the HLSL at shader model 4.1, as gpui's own
  shaders, once a process, and Metal the MSL once a renderer, when the
  first mesh is asked for: no build tool of either system is needed, so a
  build from Linux has them too. Both multisample four times (what every
  device of theirs has) with a `D32_FLOAT`/`Depth32Float` depth; Metal
  keeps the multisampled target in tile memory on Apple's GPUs. Direct3D
  makes buffers from the store's thread (its device is free-threaded),
  sets a rasterizer state of its own (no culling) and puts back the
  caller's, then the renderer sets back what `pre_draw` set.
- **What it holds:** a mesh's buffers are let go of in the first frame
  that does not draw it; the target goes when no mesh is drawn. An upload
  no one waits for any more (nothing else holds the mesh) is skipped. The
  pipelines are made when the first mesh is drawn (Direct3D's and Metal's
  when the first is asked for, so shaders that cannot be made refuse it),
  never at start. A mesh larger than the device's buffers is never drawn
  (logged once).

The tests are the renderer's, on the headless renderer (lavapipe will do):
`headless_renderer_draws_a_mesh_lit_and_in_depth` (nothing drawn before
the upload, the light's values, the nearer face over the farther drawn
after it, the buffers and target let go of),
`headless_renderer_shades_smoothly_once_normals_are_made` (flat while they
are made, then a roof's ridge lit as its normal says),
`headless_renderer_draws_a_mesh_in_its_colors`, gpui's
`smooth_normals_average_the_faces_around`, `headless_renderer_clips_a_mesh_and_keeps_its_order` (what a mask
shows of a mesh is where the whole is drawn, under a quad painted after),
`the_mesh_shader_reads_what_is_written` (the shader validates, each
parameter where `MeshFrame` writes it) and
`the_mesh_shader_s_translations_are_current`. gpui's
`a_mesh_is_framed_by_what_shows_of_it`,
`what_the_gpu_cannot_make_is_asked_for_once` and
`a_store_let_go_of_makes_nothing_more` test the shared parts.
`mesh_frame_times` (ignored) times the pass on the adapter the headless
renderer takes.

Direct3D 11's tests run under Wine, its d3d11 on OpenGL or Vulkan (Mesa's
llvmpipe or lavapipe) on an X display of its own:
`draws_a_mesh_in_depth_where_it_is` (the pass: the nearer square over the
one drawn after it, in its colours, composited where its bounds are, let
go of) and `the_renderer_draws_a_mesh_among_other_primitives` (a quad
under it and one over it, through `DirectXRenderer`; it skips under Wine,
whose HLSL compiler cannot compile gpui's own shaders). Metal is compiled
only.

### 0034: Windows shaders at start

gpui compiles its HLSL shaders ahead with the Windows SDK's `fxc.exe`, in
its build script, and only when the build runs on Windows: built elsewhere,
a release build lacks the bytes it includes, and a debug build compiles
them from the source file's path on the build machine. Built for Windows on
another system (`cfg(gpui_shaders_at_start)`, set by the build script), the
renderer compiles them as it starts, from the source the program carries
(`alpha_correction.hlsl` put in place of its `#include`), with the system's
own compiler, `D3DCompile` in d3dcompiler_47.dll (Windows 10 and 11),
optimized as fxc's `/O3` (debugging flags in a debug build). Built on
Windows, nothing changes.

It costs each start the compiling of eighteen shaders: about 0.28 s under
Wine with Microsoft's d3dcompiler_47 (winetricks), not yet measured on
Windows. Wine's own compiler, vkd3d's, stops on the shadow shader ("struct
store rhs is not HLSL_IR_LOAD"), as it does for the renderer's test above.

It acts only where gpui is built on another system: built on Windows, the
build script compiles the shaders ahead and the patch's code is not
compiled. Kept for the Windows builds made on Linux (ADR-074).

### 0035: Apple shaders at start

gpui's Apple build script makes the shaders' C bindings with cbindgen, then
compiles them ahead with `xcrun metal`; it was compiled, and its cbindgen
taken, only when the build ran on a Mac, so a build for macOS from Linux
had neither. It now runs for the target (`CARGO_CFG_TARGET_OS`), cbindgen
an unconditional build dependency. Built on another system
(`cfg(gpui_shaders_at_start)`), the shaders are stitched to their bindings
and compiled as the renderer starts by macOS's own Metal compiler, the
`runtime_shaders` path. Built on a Mac, nothing changes.

Like 0034, it acts only where gpui is built on another system: built on a
Mac, the shaders are compiled ahead and the patch's code is not compiled.
Kept for the macOS builds made on Linux (ADR-075).

### 0036: No ICU

gpui read the description of a jump list's entry the user removed up to its
NUL with ICU's `u_strlen`, its one call into ICU: so every Windows program
built on gpui imported icuuc.dll. Windows has it from Windows 10 1703 on;
Wine has none, so no test program of Skwir's window started under Wine
(exit 53 as nextest listed them). The NUL is now looked for in the buffer
itself, which also never reads past it. Checked: `just test-windows` lists
and runs the tests of `skwir` and `skwir-ui` under Wine.

### 0037: Middle ellipsis over clamped lines

`truncate_wrapped_line` cut a text with a middle ellipsis to the width of one
line, whatever its `line_clamp`, so a clamped label showed a single line.
Skwir's grid clamps a name to the lines it counted (up to three): every long
name took one line and an ellipsis, as with 0.2.2 before 0005.

`truncate_wrapped_line_middle` keeps the lines before the last as they wrap,
and cuts the middle of the last one: the front takes up to half of what the
affix leaves, the end the rest, as the earlier lines already show how the text
begins. A newline ends a line, as in `shape_text`: the last line then ends as
the text's last line does, and newlines followed by whitespace alone leave
nothing to show (as end truncation has it). Whitespace beside the affix is
kept, as upstream keeps it on one line: a space seen there is the text's own,
and none is ever added (unlike 0005, which trimmed it; chosen 2026-10-25). One
line, and truncation from the start or the end, are upstream's.
`test_multiline_truncation_middle` covers one word, two and three lines, runs
and newlines.

### 0038: A press cancels a waiting tooltip

A press cleared a tooltip already shown, but not one still waiting for its
delay (`ActiveTooltip::WaitingForShow`). A right press on a tag's chip
opens a menu at once; half a second later the chip's tooltip showed over
the menu, and stayed until the pointer moved: after it is shown, the
tooltip's hover is checked in prepaint by the element's bounds alone, which
know nothing of the menu drawn over it (upstream's own TODO in
`handle_tooltip_mouse_move`). The mouse-down handler that clears a shown
tooltip now also drops a waiting one (`cancel_waiting_tooltip`), as other
toolkits do; the next move over the element waits again.
`tooltip_waiting_to_show_is_cancelled_by_a_press` covers it, and
`just gpui-check` runs upstream's tooltip tests beside it.

### 0039: The space bar with Shift is Space

On layouts where Shift+Space types a character of its own, the key arrived
named after that keysym, so no binding on `shift-space` or `ctrl-shift-space`
could fire: Skwir's Ctrl+Shift+Space for the preview, which its tooltip
names, did nothing there. In xkeyboard-config: French Ergo-L and bépo-AFNOR
(a narrow no-break space), bépo, a Swiss variant and others (a no-break
space), and the options `nbsp:level2` (a no-break space) and `nbsp:zwnj2…`
(a zero-width non-joiner, as Persian typists set it).

`keystroke_from_xkb` now takes a key whose unshifted symbol is `space`, at
the level Shift alone gives it, as `space`: Qt matches a shortcut held with
Shift by the key's unshifted symbol the same way, and Windows names the
space bar by its virtual key whatever it types. What AltGr gives (level
three and above: AFNOR's no-break space) stays its own, as in Qt. What the
key types stays the keystroke's `key_char`, so a text field still gets it.
Only the space bar: a shifted punctuation key keeps its symbol, which
gpui's bindings name (`ctrl-?`). GTK 4 matches a shortcut against the
keysym typed, at the same level, and seems to miss this case.

`the_space_bar_with_shift_is_space_whatever_it_types` builds the keymaps
from xkeyboard-config: Ergo-L's and `ir` with `nbsp:zwnj2` give `space`
with Shift, and keep what they type; AFNOR's AltGr+Space stays its own.

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
