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

- gpui's list and uniform list, wheel-axis, line breaks, line wrapper, line
  layout, text, style, menu and key dispatch tests;
- all of gpui_wgpu's library tests (the atlas, the context, the renderer,
  the text system), which need a wgpu adapter (lavapipe will do), and the
  text system's again with font-kit choosing the faces, as Skwir builds it;
- gpui_linux's clipboard tests.

`just gpui-check-windows` runs gpui_windows's tests of the mesh pass under
Wine, built for Windows from the same copy (`tools/gpui/check-windows.sh`).
Wine's Direct3D 11 draws on OpenGL, through Mesa's llvmpipe on an Xvfb of
its own.

gpui's library tests embed two of Zed's fonts, which the script fetches once
at the vendored commit, and 0045's Inter's variable font, which Zed does not
carry: the script takes it from the Inter release Skwir's packages pin
(`tools/package/fonts.lock`).

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
| `0042-list-overdraw-set-later` | `ListState::set_overdraw`: a list about to scroll up by more than its overdraw measures that far first, and crosses heights known rather than hinted | — |
| `0043-activate-with-token` | `Window::activate_window_with_token`: the window brought forward with the token the desktop gave (Wayland's xdg-activation, a notification clicked) | — |
| `0044-should-quit` | `Application::on_should_quit`: macOS's `applicationShouldTerminate:` asks the application whether it may quit (Quit from the Dock, a log out, a restart) | — |
| `0045-variable-font-weights` | A variable face is shaped, measured and drawn at the weight asked within its weight axis (Linux), a variable fallback at its run's | — (its tests need Inter's variable font in Zed's assets) |
| `0046-active-drag-of` | `App::has_active_drag_of::<T>()`: whether the drag under way carries a `T` | — |
| `0047-platform-draws-its-drag` | A drag the platform carries, come back to its source window, is not drawn there again | — |
| `0048-glyph-rasterization-public` | `TextSystem::raster_bounds` and `rasterize_glyph` public | — |
| `0049-drag-icon-scale` | A drag icon drawn at a scale is shown at its logical size (Wayland: a viewport, or the buffer's scale) | — |
| `0050-drop-is-the-pointers` | A drag the platform brings into a window counts as the pointer's, so a window last used by keyboard takes its drop | — |
| `0052-inferred-list-measures-at-its-width` | A list sized by its items (`ListSizingBehavior::Infer`) whose width is set measures them at the width they are drawn at, so items that wrap make it as tall as they are: a width it says from its first frame, its parent's (`w_full`) once drawn at it, that first frame asking for another; Zed's measured them at their own width, and a row that wraps was cut under a scrollbar (the Undo history, 2026-11-07) | — |
| `0051-x11-window-place` | X11: a window opens where it is asked (program-specified position, static gravity, no 2 px nudge but at 0, 0), maximized or on the whole screen as asked before it maps (`_NET_WM_STATE` written), and says where it is on the root window, a resize included | — |
| `0053-rounded-content-mask` | An element with rounded corners that hides its overflow keeps what it holds within its corners (`ContentMask::rounded_bounds`, `corner_radii`), on wgpu, Direct3D 11 and Metal | — |
| `0055-paint-around` | `Window::paint_around`: a color outside a rounded rectangle, the complement of a quad there told how opaque it is (`Shadow::under`), so two see-through grounds meet without a seam; `Window::snap_to_device_pixels` | — |
| `0054-window-tiling` | `Window::tiling`: the edges that meet another window or the screen's, whoever draws the decorations (Wayland's tiled states with server-side ones too) | — |
| `0056-ground-ink` | `Styled::ground`: a background whose ink everything inside is drawn in while the element is on it (text, icons, underlines), whatever color each names; an opaque background of its own, a hover's in its place, gives each its own color back (`Style::ink`, `Window::with_element_ink`) | — |
| `0057-deferred-paint-text-style` | A deferred draw paints under the text style of where it stands in the tree, as it prepaints | — |
| `0058-rounded-mask-on-device-pixels` | A rounded content mask lies on whole device pixels, as what it holds is drawn | — |
| `0059-unreached-mask-corners` | A primitive that reaches no rounded corner of its mask is kept by its rectangles alone, so its fragments skip the corners' distance | — |
| `0060-inline-boxes` | A line holds boxes its caller draws (`RunPosition::Inline`): each character of an inline run is one, as wide as it says, never shaped or painted; `TextLayout::inline_boxes` says where each was drawn; a box taller than its text grows the row it is set in, and that row alone (`LineRows`); `StyledText::with_said` gives what such a text says to assistive technology | — |
| `0061-text-narrows-to-its-widest-word` | A text's minimum content width is its widest word (CSS's), so beside others in a row it narrows and wraps rather than running past them; Zed's was its whole line, which no row could shrink. A layout a text keeps answers another width only where it is the same there. A tooltip and what a drag shows are laid out as wide as their words (`AvailableSpace::max_size`) | — |
| `0062-drop-svg` | An SVG given as bytes (`paint_svg`'s `data`) is kept in the atlas at each size it was drawn at until its caller lets it go (`Window::drop_svg`, `App::drop_svg` for every window, as `drop_image`): a page's formulas leave with the page | — |
| `0063-hitbox-is-frontmost` | A hitbox says whether it is the frontmost one under the mouse (`Hitbox::is_frontmost`): the empty parts of a title bar the application draws move the window, a button over them keeping its press | — |
| `0064-rounded-masks-meet-at-corners` | Two rounded content masks with a corner at the same point meet there along the larger arc (`ContentMask::intersect`): a card at a window's corner is rounded as the window is | — |
| `0065-decorations-as-answered` | A Wayland window keeps its decorations until the compositor answers a request, and says while it waits (`Window::decorations_pending`): a compositor that keeps its own (Hyprland's) no longer shows those asked for a frame | — |
| `0066-title-bar-on-windows-and-macos` | An open window's title bar hidden on Windows, made transparent on macOS, as decorations are requested and said (`request_decorations`, `window_decorations`); Windows' `zoom` restores a maximized window; a drag area counts in Windows' hit test where it is frontmost | — |
| `0067-client-decorations-supported` | A window says whether the application's decorations can be had before they are asked for (`Window::client_decorations_supported`): not where X11 runs no compositor or its window manager reads no `_GTK_FRAME_EXTENTS` | — |
| `0068-drag-areas-that-keep-the-double-click` | A drag area whose double click is the application's (`WindowControlArea::DragOnly`): Windows moves the window from it but neither maximizes nor restores it there; a non-client release counts the clicks as its press did | — |
| `0069-right-to-left-text` | A line holding right-to-left text keeps each byte's bidi level (UAX #9) and its glyphs in the order they are drawn: its carets, hit tests and selections answer where it is drawn (`LineLayout::selection_ranges`, `index_left_of`, `index_right_of`), it wraps in its text's order, each row laid out as it reads, and a decoration follows its characters | — |
| `0070-directwrite-right-to-left-runs` | Windows: a run read right to left is placed from its origin leftward, as DirectWrite draws it, and a line that starts right to left reads so | zed#31102 (open) |
| `0071-word-characters` | The letters, marks and digits of every script that puts spaces between its words are word characters by their Unicode properties (a Greek, Hebrew, Arabic or Devanagari word wraps whole); Chinese, Japanese, Korean, Thai and the like break between characters, never before a mark; bidi controls and joiners are glue | — |
| `0072-line-breaks-by-uax-14` | A line wraps where the Unicode line breaking algorithm (UAX #14) lets it, by ICU4X's line segmenter, its data compiled in behind one seam (`text_system::line_breaks`): kinsoku, Thai, Lao, Khmer and Burmese by dictionary, Tibetan, the bidi controls' glue; ASCII by its classes, without the segmenter; no row is spaces alone; 0071's rule goes | — |
| `0073-language-for-assistive-technology` | A window says the language its words are in to assistive technology (`Window::set_a11y_language`, on its tree's root), a div or a text where it is another (`aria_language`) | — |
| `0074-chinese-characters-in-the-language` | Linux: the fallback for Chinese characters takes their forms in the language, Japanese, Korean, Traditional (Taiwan's, Hong Kong's) or Simplified Chinese, by the locale's script and region (ICU4X's likely subtags), at start and as `TextSystem::set_language` asks | — |
| `0075-layout-direction` | A layout right to left, inherited as CSS's `direction` through the text style (`LayoutDirection`, `Styled::layout_direction`, `Window::layout_direction`): taffy mirrors flex rows, start and end alignments, wrapped lines and grid columns; a box that scrolls sideways starts at its right; `TextAlign::Start` and `End` | — |
| `0076-shortcuts-on-layouts-without-latin-letters` | Linux: a shortcut (Control, Alt or Super held) on a layout whose letters are not Latin is named by the keymap's first Latin layout, or a US one where it has none, never by its own ASCII symbols; a shortcut on a digit row that types no digit names its digit; the US positions of the digit row, `-` and `=`, and `<` and `>` no longer swapped | — |
| `0077-no-isolates-for-assistive-technology` | What a node says to assistive technology (its label, description, value, placeholder) has its bidirectional isolates (U+2066 to U+2069) taken out, but a text run's, whose offsets a caret counts | — |
| `0078-kept-breaks-for-line-wrappers` | A text beyond ASCII that a line wrapper wraps (a name a grid counts the lines of) is segmented once: its breaks kept by its text in the text system, two generations of 4,096 texts, so counted again at another width it costs about what it did before 0072 | — |
| `0079-fit-content-lengths` | A length may be CSS's `fit-content(<length-percentage>)` (`Length::FitContent`, `fit_content`): a measured element with such a flex basis or size is measured with the limit as its definite available space, so it chooses its width by the room it is given | — |
| `0080-aligned-hit-tests` | A text centered or aligned at its right answers where its rows are drawn: `TextLayout::index_for_position` and `position_for_index` move by each row's offset as painted (`WrappedLine::row_offset`), so a click, a link's hover and a selection land on the characters under the pointer | — |
| `0081-direction-of-a-text` | `LayoutDirection::of_text`: the direction a text reads in where nothing else says one, its first strong character's (UAX #9's P2 and P3, as HTML's `dir="auto"` reads it), to lay out what holds the text in it | — |
| `0082-lists-follow-the-layout-direction` | A list's items are laid out, prepainted and painted in the list's own text style, its layout direction among it; a uniform list's rows scrolled sideways right to left move by its offset, not twice it | — |
| `0083-window-menu-by-a-flag` | The menu macOS lists the windows in is the one that says it is (`Menu::window_menu`), never the one named "Window" | — |
| `0084-text-direction` | A text's paragraphs read in the direction it says (`TextDirection`, inherited, `Styled::text_direction`; `WindowTextSystem::shape_text_in`), not each its first strong character's: gpui's levels worked out so (UAX #9's HL1) and the platform told (`PlatformTextSystem::layout_line_in`: DirectWrite's reading direction, Core Text's base writing direction, for cosmic-text a mark left out of what it gives back); a line of code that starts with an Arabic comment keeps its `//` at its left | — |
| `0085-foreign-parent` | `WindowOptions::foreign_parent`: a window shown for another program (a dialog it asked for, the desktop portal's `parent_window`) is that program's window's child, `WM_TRANSIENT_FOR` on X11, imported through xdg-foreign (unstable v2) on Wayland, so the window manager keeps it over it; a handle that names nothing leaves a window of its own | — |
| `0086-rows-reorder-within-paragraphs` | A wrapped row holding the end of one paragraph and the start of the next (a separator parts them on one line) lays each paragraph's stretch, and the separator, out on its own in the text's order, as the line is laid out unwrapped, never reordered across them | — |
| `0087-right-to-left-answers-kept` | A line holding right-to-left text works out its carets, hit tests and selections at the first question and keeps them with it (`LineLayout::bidi_row`, a wrapped line's rows), each answer after that a search; its carets' edges sorted once, so a long right-to-left line costs a sort, never a move of every edge for each | — |
| `0088-scroll-handle-direction` | `ScrollHandle::direction`: the direction the element a handle tracks was last laid out in, its own or inherited, which way its offset runs sideways (what `clamp_scroll_position` holds it to) | — |
| `0089-window-tag` | `WindowOptions::tag` (`WindowTag`): what a window is for, named for the user's window rules: Wayland's xdg-toplevel-tag-v1 tag and its translated description, part of the first commit where the compositor offers the protocol; X11's `WM_WINDOW_ROLE`, for an ASCII tag (another takes the role back); said again by `Window::set_tag` (the description in another language), the platform told only what changed | — |
| `0090-image-pieces` | An image larger than the atlas's largest texture is drawn in pieces (`PlatformAtlas::max_texture_side`), each filtered with the pixels beside it, so no seam shows; only the pieces that show are uploaded | — |
| `0091-window-being-resized` | `Window::is_being_resized`: whether the user is resizing the window by dragging (Wayland's `resizing` state), so a size given meanwhile is told apart from one the compositor lays the window out at, whatever its tiled states say (Hyprland says every window is tiled); false where the platform does not say | — |

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

### 0040: A list keeps its height hints across a width change

A book read in continuous scrolling had a scrollbar that ended at the
last page visited, while the bar counted all its pages. The reader gives
every page's row a height estimate before the list lays it out
(`with_uniform_item_height`, or a hint per item); Zed's list, on its
first layout and whenever its width changes, made every row unmeasured
with no estimate, so the list's height, and the scrollbar's, was only the
rows laid out since. 0004's last hunk kept the estimate there; it went
when 0004 was dropped with gpui 0.2.2 (2026-10-18), Zed having hints of
its own, and every document read continuously had the scrollbar of the
pages seen since.

What each row measured, or was hinted at, now stays its estimate until it
is measured again, as `remeasure_items` already does: at another width
the measured heights are stale, but still the best guess. A structured
preview's list takes the same hints and gains the same fix.
`test_uniform_height_hints_survive_the_first_layout_and_a_width_change`
lays out 100 hinted rows of 50 px in a 200 px list, then at another
width, and finds 4800 px to scroll both times (all 5000 less the 200
shown);
`just gpui-check` runs it with upstream's list tests.

### 0041: A key binding for a system

Zed's main of 2026-10-27 parses keystrokes for a system known at run
time, and its two ways to make a `KeyBinding` stopped fitting a keymap
read from the user's settings. `KeyBinding::new` now takes a
`&'static str`, refuses `secondary-` (`Keystroke::parse` knows no system)
and panics on keys that do not parse: Skwir's bindings are strings read
at run time, its text input's own say `secondary-` (Skwir panicked at
start without this patch), and one the user mistyped must be warned
about, never end the program. `KeyBinding::load` takes the `App` and maps
the keys through the platform's keyboard mapper, which on Windows reads
them by the layout: Skwir's keys keep their meaning whatever the layout
(AGENTS.md, Conventions), and its keymap becomes gpui's bindings without
an `App` (`gpui_bindings`, which its tests call without a window).

`KeyBinding::new_for_os(keys, action, context, os)` makes a binding from
any string, its `secondary-` resolved for the system given and its other
modifiers as written, through `load_internal` with the dummy mapper, as
`new` did before; it returns an error, not a panic, when a keystroke or
the context does not parse. Skwir names its system once
(`keymap::HOST_OS`, from the target) and goes through it for every
keystroke parsed or written back and every binding (`keymap::binding`,
which warns of one that does not parse). No test of its own: Skwir's
keymap tests build every default binding through it.

### 0042: A list's overdraw set later

A list measures the items in its overdraw above its scroll top, and only
those: `scroll_by` counts every other item above at its hint. Upstream fixes
the overdraw when the `ListState` is made. Text read a screen at a time
(ADR-103) finds the page that ends where the one shown starts by scrolling
up a page from there; across items not laid out yet, a book's long
paragraphs hinted as short ones, the move went too far, and each frame
measured one more item on the way back, so a chapter's last page was
found a page short (2026-10-29).

`ListState::set_overdraw` changes it from the next layout on. Skwir's
pager makes it a view tall while it finds a page back
(`inspector_pages::found_back`), so the frame laid out from where the page
ends has measured the page above, and puts it back once the page is
found. `test_overdraw_set_later_measures_that_far_above_the_top` scrolls
400 px up from the fifth of ten 300 px items hinted at 50 px: with no
overdraw it reaches the list's start (eight hints); with 400 px set and a
layout between, it crosses item 4 and item 3's last 100 px, 200 px into
item 3. `just gpui-check` runs it with upstream's list tests.

### 0043: Activate with a token

A notification server that follows version 1.2 of the specification sends
an xdg-activation token with a click on a notification (`ActivationToken`,
before `ActionInvoked`). Upstream's `activate` asks the compositor for a
token of its own, with the serial of the last press in the window, which a
compositor refuses when the window was not used last: Mutter and KWin then
only mark the window as asking for attention. Skwir's notices of long
actions (ADR-114) bring the window forward when clicked.

`PlatformWindow::activate_with_token` hands the desktop's token to
`xdg_activation_v1::activate` on Wayland; elsewhere, and on a compositor
without the protocol, it activates as before. `Window::activate_window_with_token`
takes the token if there is one.

### 0044: Should quit

macOS asks a program whether it may quit before it does: Quit from the
Dock's menu or the application menu, a log out, a restart, `App::quit`
too (`applicationShouldTerminate:`). Upstream's delegate does not answer,
so the program quits at once. Skwir finishes what runs without its window
(ADR-115): while something is under way it answers no and asks the user,
and a log out is cancelled, the system saying by which program.

`Application::on_should_quit` registers the handler; macOS's delegate
answers `NSTerminateCancel` when it says no, `NSTerminateNow` otherwise or
without one, or while the application is borrowed. Other platforms never
ask and ignore it (`Platform::on_should_quit` has a default).

### 0045: Variable font weights

A variable font is one face to fontdb, at the weight its file declares
(400), its other weights named instances of its weight axis: Noto Sans as
Fedora and NixOS ship it, Cantarell, Ubuntu Sans, Inter's variable file.
Upstream's Linux text system chose a face by its declared weight, loaded
every face at the normal weight and drew glyphs at the face's default
coordinates, so a semibold or bold run in such a font was shaped, measured
and drawn regular. cosmic-text 0.19 does the rest already: `get_font(id,
weight)` shapes at that weight, its matcher counts a variable face as
holding the weights its axis spans, and its own drawing sets swash's
coordinates.

`LoadedFont` carries the weight it is drawn at, its face's weight axis and
swash's coordinates there (none for a face of one weight, which is drawn
as before). `font_id` chooses the face as before, then for a variable face
returns its instance at the weight asked, clamped to its axis, loaded once
per weight (`LoadedFontKey` names the weight). An axis a font gives
backwards (its least past its greatest, which swash reads as it is) spans
the weights between its ends, as read-fonts takes it, never a panic of
`clamp`. A run is shaped at its font's weight (`font_match_properties`);
its glyphs are measured (`advance`, `typographic_bounds`, `font_metrics`)
and drawn (`render_glyph_image`) with its coordinates. Every glyph drawn
sets the scaler's coordinates, none for a face of one weight: swash's
context keeps them from one builder to the next, so a face drawn after a
variable one would otherwise take its coordinates. A glyph cosmic-text
shaped in a variable fallback at another weight resolves to the
fallback's instance there (`font_id_for_cosmic_id`). Three tests: with
Inter's variable font, a face at four weights (fonts kept per weight,
wider lines, wider advances, a bold H with more ink than a regular one,
where the same before) and a variable fallback of a bold run drawn bold;
and an axis given backwards. Not changed: a static family without the
weight asked is drawn at its nearest face, the fallback beside it at that
face's weight. macOS and Windows have text systems of their own.

To upstream it, Inter's variable font (SIL Open Font License) goes into
`assets/fonts/inter/` beside Zed's other test fonts: its tests embed
`InterVariable.ttf`, which Zed does not carry and `just gpui-check` takes
from the Inter release Skwir's packages pin. Zed carries no variable font
of its own to test with.

### 0046: Active drag of

`App::has_active_drag` says only that something is dragged. A drop target
nested in another (a folder's row in a pane, a place in the sidebar) hides
what is behind it from the pointer while files are dragged, so that only
the destination shows it takes them, not the area holding it as well. It
must not while another kind of drag is under way: an edge resized, a tab or
a place moved, whose release the area behind still hears.
`App::has_active_drag_of::<T>()` tells a drag by the type of what it
carries, as `drag_over::<T>` and `on_drop::<T>` already do inside gpui;
`ExternalPaths` for what the platform drags in.

### 0047: The platform draws its drag

Upstream hands an in-window drag to the platform as the pointer leaves the
window (0001 gives it an icon on Wayland), and takes it back when the pointer
comes back to the window it started in (`restore_platform_drag`): the drag's
view is drawn under the pointer again. The platform still carries the drag
and draws its image there too, so it was seen twice. While the drag back in
its source window is the platform's (`RestoredInSourceWindow`,
`App::platform_draws_drag`), the window draws its view no more; the drag
otherwise goes on as it did (its hover styles, its drop). Checked on a
headless sway: out of the window and back, one image.

### 0048: Glyph rasterization public

`TextSystem::raster_bounds` and `rasterize_glyph` were the crate's: a caller
can now draw text into an image of its own, glyph by glyph, with the fonts
and the fallbacks the window draws with. Skwir writes a dragged file's name
into the image the compositor shows (`dragname.rs`). A glyph comes as one
byte of coverage a pixel, or four (BGRA) for an emoji, placed at its raster
bounds' origin from its position on the baseline; `RenderGlyphParams` was
already public.

### 0049: Drag icon scale

An `ExternalDragIcon` says how many of its pixels make one of the
window's (`scale`): an image drawn at the window's scale. On Wayland its
surface shows it at its logical size, through a viewport where the
compositor has the viewporter (any scale, fractional ones included), else
through the buffer's scale where the scale is whole and divides the image;
else pixel for pixel, as before. The hotspot is given in the surface's
logical coordinates. It extends 0001 (a patch of its own rather than a
change of 0001, made by hand). Checked on a headless sway at 2 and 1.25: the
name under the icon as sharp outside the window as inside, the same size.

### 0050: A drop is the pointer's

A window counts no hitbox as hovered while its last input was the keyboard
(`HitboxId::is_hovered`), and a drag the platform brings in (files from
another application) moves no pointer of the window's own: dropped on a
window last used by keyboard, it landed on nothing. `FileDropEvent::Entered`,
`Pending` and `Submit` now set the window's input modality to the mouse.
Found while a tab was dragged between two of Skwir's windows (a drag later
withdrawn, ADR-122: the platforms do not all carry one); the drop from
another application has the same cause.

Test: `a_drop_lands_in_a_window_last_used_by_keyboard`.

### 0051: A window's place on X11

Skwir opens a window where it was (ADR-122). On X11 gpui could not:

- **Where it opens**: no `WM_NORMAL_HINTS` position was given, so a
  window manager placed it as it liked (openbox put a window asked at
  50, 40 at 350, 157). The hints now give the position as the program's
  with static gravity, the ICCCM's way of saying that the window itself,
  not its frame, goes there: what `bounds` reads.
- **No drift**: every window was created 2 px right of where it was
  asked, a workaround for one opened at the default position (GNOME on
  Ubuntu 22); one opened where it was left moved 2 px at each opening.
  Created where asked; the check below it still moves one at 0, 0.
- **Where it is**: a real `ConfigureNotify` says where the window is in
  its parent, the window manager's frame, and gpui dropped the origin of
  a resize; a synthetic one (the manager's, after a move) says where it
  is on the root window. A real one's origin is now asked of the server
  (`X11WindowStatePtr::root_origin`, `TranslateCoordinates`), a resize's
  included.
- **Maximized as asked**: gpui asks a window opened maximized (or on the
  whole screen) to be so before it maps it, by a client message, which a
  window manager takes only for a window it manages: it opened windowed.
  Asked before mapping, the state is written as the window's
  `_NET_WM_STATE` when it maps (`unmapped_states`), as the EWMH says.

Checked on Xvfb under openbox 3.6 and with no window manager: windows
opened where they were left, to the pixel, across restarts; a window moved
and resized at once was read where it went; one maximized (wmctrl) opened
maximized and went back to its own size and place. Not checked under
GNOME's, KDE's or a tiling window manager.

### 0053: Rounded content masks

Skwir draws its window's areas as rounded cards (ADR-123), whose content
(a terminal's cell backgrounds, a page, a film, a selected row) must stop
at their corners. gpui's content mask was a rectangle ("only rectangular
content masks are supported"): a rounded element that hid its overflow
let its children's corners show past its arcs.

- **The mask holds a rounded rectangle** beside its bounds:
  `ContentMask::rounded_bounds` and `corner_radii`, all zero by default
  (`ContentMask::new`), which draws as before. Masks intersected keep the
  intersection of their bounds and the rounded rectangle of the one with
  rounded corners, the smaller of two (the one inside): a mask holds one,
  so an outer rounded mask's corners are not kept inside an inner one's.
- **Where one comes from**: `Style::overflow_mask` of an element hidden
  along both axes with rounded corners, inside its border as CSS clips
  (each radius less the wider of its two borders, held to half the inner
  size). Hidden along one axis, or square, the mask stays a rectangle.
- **Every primitive keeps to it** (quads, shadows, paths as rasterized,
  underlines, monochrome, subpixel and polychrome sprites, surfaces): the
  vertex stage hands the rounded rectangle on, flat, and the fragment's
  alpha is the rounded rectangle's coverage (`quad_sdf`, antialiased over
  a pixel), skipped while every radius is zero. Meshes (0033) keep their
  rectangle.
- **The records grow by 32 bytes** (a mask is 12 words): the WebGL
  decoders read the new strides (the quad's fixed decoder 12 texels), and
  `webgl_record_sizes_match_shader_word_strides` and the quad layout test
  say so. Direct3D's records carry the mask's fields one by one, laid out
  as gpui's struct.

Checked: `just gpui-check` (naga validates the storage, WebGL and
subpixel variants; a headless render on lavapipe clips a quad's four
corners and keeps a square mask's: `headless_renderer_clips_to_a_rounded_content_mask`;
the mask's tests in `style::tests`). Every HLSL entry point compiles
with Microsoft's DXC (`-T vs_6_0`, `ps_6_0`), as the original did; Wine's
`D3DCompile` (vkd3d) compiles neither the original nor this one ("Struct
store rhs is not HLSL_IR_LOAD"), so the Windows renderer is unchecked
under Wine. Metal is written as the others and compiled nowhere.

### 0054: A window's tiling

A window drawn as cards keeps a gutter around them where it floats, and
draws them to its edges where it meets another window or the screen's
edge (ADR-123). gpui said which edges were tiled only through
`Window::window_decorations`, and only with client-side decorations:
Skwir asks for the server's, and saw none, though Wayland's compositor
sends the tiled states either way (`xdg_toplevel`'s `tiled_*`, version 2).

- `PlatformWindow::tiling`, by default the client decorations' tiling,
  else every edge while the window is maximized or on the whole screen,
  else none; Wayland's is the tiling the compositor configured.
- `Window::tiling` asks it.

Checked on a headless sway 1.12: a tiled window drew its cards to its
top, right and bottom edges (the sidebar held its left). Not checked:
Hyprland, GNOME's and KDE's tiling, X11 (maximized), macOS and Windows.

### 0055: Painting around a rounded rectangle

Skwir's window lets what is behind it through its grounds (ADR-119),
each painted once: a ground with a rounded part in it (a card on the
frame, a field in a bar) is painted around a hole the part's shape. Where
the two antialiased edges met along a curve, a pixel half in each got
the ground at half its opacity, then the part at half over it: drawn one
over the other, they let a quarter of what is behind through at their
middle, a seam (148 between 168 and 175 on Xvfb).

- **`Window::paint_around(bounds, hole, radii, color, under)`** paints
  `color` over `bounds` outside the rounded rectangle `hole`: an inset
  shadow with no blur, the exact complement of a quad of that shape (the
  same `quad_sdf`, the same half pixel), both snapped as quads are.
- **`Shadow::under`**, the inset shadow's former padding: how opaque what
  is drawn in the hole is, before or after. The complement's coverage is
  `(1 − c) / (1 − under · c)` where the hole's is `c`, so the two add up
  to the pixel as one ground would; 0 gives the plain complement, as
  before, which `paint_inset_shadows` keeps.
- **`Window::snap_to_device_pixels`**: a shape laid out on whole device
  pixels as quads are drawn, so the straight edges of a path around them
  meet them exactly.

Checked: `just gpui-check gpui_wgpu`
(`headless_renderer_paints_around_a_rounded_quad_without_a_seam`: a
translucent rounded quad over its complement told its opacity leaves the
arc one tone, within 2 levels; told nothing, more than 8); every HLSL
entry point compiles with DXC; the macOS build type-checks, the shader
bindings carrying `under`. Metal compiled nowhere.

### 0056: A ground's ink

A desktop's selection may be dark in a light look, or the accent itself
(Noctalia's): what is drawn on it must be drawn in its text color. gpui
draws each glyph and icon in the color it names, and an icon names its
own (an `svg` inherits nothing): a selected row's words followed its text
color, its muted shortcut, its icons and its chevron did not. Skwir
chose their colors where it knew the row was chosen, and missed one each
time a row was chosen some other way (a submenu open, the keyboard on a
popup's row, a button whose menu is open, 2026-11-08).

- **`Styled::ground(ground, ink)`** sets the background to `ground` and
  `Style::ink` to the pair: while the element's background is that ground,
  what is painted inside it, its own text included, is drawn in `ink`.
- **`Style::paint`** draws what is inside an element (its continuation)
  in its ground's ink while it is on it; in the colors each names under an
  opaque background of its own (a field's, a tag's solid chip, a hover's
  over the ground: the refined background is no longer the ground); in its
  parent's under none, or under one that lets the ground through (a tint).
  `Style::ink_inside` says which.
- **`Window::with_element_ink` / `element_ink`**: the ink in force, which
  `paint_glyph`, `paint_svg`, underlines and strikethroughs paint in,
  whatever color they are given. A deferred draw (a popover, a tooltip)
  is painted outside the tree, in its own colors. A quad is not ink: a
  mark drawn as a shape (a box's border, a dot, a bar) is chosen where it
  is drawn.

Checked: `just gpui-check gpui -- style::tests`
(`what_is_on_a_ground_is_drawn_in_its_ink`: a ground's ink, an opaque
background of its own, a tint, a hover's in place of the ground, none);
on Xvfb with Noctalia's colors, the footer's Undo history chevron, a
menu's row whose submenu is open, the View button with its menu open
and the rows of that menu, an eject button under the pointer, all white
on the teal; the gpui crate type-checks. Not checked: Windows and macOS
(the change is in gpui's shared code, no platform's).

### 0057: A deferred draw paints in its text style

`prepaint_deferred_draws` restored the text style a deferred element stands
in; `paint_deferred_draws` did not, so what reads the style while painting
got the window's default instead: `window.line_height()` (1 rem × φ, 26 px
where the field's is 19 to 23) and a text's alignment (`TextLayout::paint`).
A field's text is centred in the line height it paints with, so it sat lower
than laid out inside its own selection and caret; the address field, drawn
in place until its card's rows arrive and deferred above the card after,
moved down as they came. Text that says it is centred or right-aligned in a
popover or a dialog was drawn at the left.

`paint_deferred_draws` restores the stack as prepaint does, and clears it
after.

Checked: `just gpui-check gpui`
(`a_deferred_draw_paints_in_the_text_style_it_stands_in`: read 19 px while
prepainting and 26 px while painting before the change, 19 px in both after).
Not checked: Windows and macOS (the change is in gpui's shared code, no
platform's).

### 0058: A rounded mask on device pixels

0053 covered a mask's bounds to device pixels but only scaled its rounded
rectangle, while the quads and sprites inside are snapped. A rounded element
that hides its overflow, its top at 16.5 device pixels, had a child snapped to
16 with its row 16 at half opacity (`saturate(0.5 - 0)`): the straight edge of
a picture went half see-through at 1.5× or on a tile scrolled to a fractional
offset. The rounded rectangle is snapped as a quad is
(`Window::snapped_content_mask`). A card, whose mask is on device pixels
already, is drawn as it was.

Checked: `just gpui-check gpui`
(`a_rounded_mask_lies_on_device_pixels`). Not checked: Windows and macOS (the
change is in gpui's shared code, no platform's).

### 0059: Corners a primitive does not reach

A mask with rounded corners carried its radii into every primitive inside it,
so a glyph in the middle of a card's list evaluated the corners' signed
distance per fragment for arcs it cannot reach (0053's early return applies
only when every radius is zero). `Scene::insert_primitive` zeroes the radii of
a primitive that has no pixel in the square of a rounded corner, and intersects
the mask's bounds with its rounded rectangle, which is then all its straight
edges do. Every renderer gains and no shader changes. Left as they were: a
shadow, which paints past its bounds; a sprite turned, whose bounds do not say
where it is drawn; a mesh, which keeps its rectangle (0033).

Checked: `just gpui-check gpui`
(`a_primitive_away_from_the_corners_is_drawn_without_them`) and `gpui_wgpu`'s
`headless_renderer_clips_to_a_rounded_content_mask`. The fragment cost saved
was not measured.

### 0060: Inline boxes

A run can be boxes a line holds for its caller to draw: a formula in a
sentence (ADR-129). `RunPosition::Inline(InlineBox)` gives the box's width,
the height it stands above the baseline and the depth it reaches below it,
in ems of the text's size, kept in thousandths so a run still compares and
hashes.

- **Shaping.** `shape`, 0006's, takes an inline stretch itself: each
  character of it is one glyph the platform never sees, as wide as the box.
  Two boxes side by side are two characters, so runs merged or split around
  them change nothing. The line's ascent and descent stay its text's; a
  line of boxes alone stands on its font's baseline (a space shaped for
  its metrics).
- **Wrapping and hit tests** read glyph positions, so a box is a character
  a word may break before, selected and pointed at as one.
- **Painting.** `paint_line` draws no glyph for a box; decorations go
  under it as under any glyph, backgrounds fill its row.
- **Where.** `WrappedLine::inline_boxes` walks the glyphs as `paint_line`
  does (each wrap aligned) and gives each box's bounds on its line's
  baseline; `TextLayout::inline_boxes` gives them for the whole text, its
  alignment kept from its layout.
- **Height.** A row is one line high, but a row holding a box that stands
  higher or reaches lower than its text, which grows by what the box needs
  above the text and below it, in whole pixels (`LineLayout::rows`, a
  `LineRows`); its text keeps its place one line high in it (the row's
  band). Every row-wise answer reads them: a line's size, its paint
  bounds, where a glyph, decoration, background or box is painted, which
  index a position is at and where an index is. `TextLayout::row_bounds_for_index`
  gives a row's bounds and `TextLayout::row_edges` every row's top, for
  callers that set something under a row or cut a page between two. A
  text without boxes, or with none taller than its lines, lays out as
  before.
- **What it says.** `StyledText::with_said` gives `InteractiveText` its
  words for assistive technology where the characters drawn do not say
  them.

The tests are `text_system::line_layout::run_position_tests::each_character_of_an_inline_run_is_a_box_of_its_width`,
`text_system::line_layout::run_position_tests::a_row_grows_by_what_its_box_needs_and_the_others_keep_their_height`,
`text_system::line::tests::inline_boxes_are_where_their_glyphs_are_painted`
and `text_system::line::tests::inline_boxes_taller_than_their_text_grow_their_row_alone`.

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

### 0061: A text narrows to its widest word

A text's layout answered taffy's minimum-content probe
(`AvailableSpace::MinContent`) unwrapped: its whole line. A flex item may
not shrink below that, so a sentence beside an icon or a button in a row
too narrow for both never wrapped; it ran past the row (a plugin's
requirement under its card, 2026-11-12). CSS takes the longest word.

- **The width.** `LineLayout::min_wrap_width` is the widest stretch between
  two places the line may wrap, measured as `compute_wrap_boundaries`
  measures a line, so wrapped there no word is cut. The places are one
  rule both ask (`wraps_before`): before a word after a space, and before
  any other character but a space, after the first that is not one. Since
  0072 they are UAX #14's (`LineBreaks`).
- **The probe.** A text that wraps (`WhiteSpace::Normal`, not truncated)
  answers it with its lines' minimum content sizes
  (`WrappedLineLayout::min_content_size`: that width, and the rows it
  wraps into there), its lines unwrapped, from the line layout cache. It
  leaves the text's own layout as it was, so no other probe is answered
  with it. One that does not wrap (`whitespace_nowrap`, `truncate`)
  answers as before, its whole line: what must stay on one line says so.
- **Once a line.** The width and its wraps are worked out once for a
  line and kept with its unwrapped entry in the line layout cache
  (`WrappedLineLayout::min_content`), across frames. That entry is the
  same whatever widths the text is laid out at: asked of the lines laid
  out at a width, as first written, a window being resized worked it out
  again for every text at each new width, each keeping its own copy.
- **A narrow layout answers only its width.** A text keeps the layout
  it was last measured at, and Zed's answered a probe with no width
  (max-content) from it whatever width it was made at. A box measured
  for its minimum content width lays its texts out at their widest word;
  a row then asking one how wide it would be got that width back and kept
  it there (the command palette's titles a word a line, the bulk rename's
  Case chip, 2026-11-12). A kept layout answers a measure at another
  width only where it is the same there: no line of it wrapped, and its
  widest fits (any, for a probe with no width), which also spares laying
  a text out again at a width it fits.
- **What is laid out on its own.** A tooltip and what a drag shows were
  laid out at `AvailableSpace::min_size`, which meant as wide as their
  words while a text's minimum was its line. With the minimum now a word,
  they narrowed to their widest (a toolbar button's tooltip, 2026-11-12). They
  are laid out at `AvailableSpace::max_size` (new, beside `min_size`): a
  text there keeps to its line and wraps only where a width it was given
  (a `max_w`) is too narrow.

Where a row has room, nothing moves: Skwir's screens at 1280 px were drawn
alike before and after it, pixel for pixel (33 screens; 640 and 460 px
too, but for single-line labels that said nothing, a dropdown's, which now
say `whitespace_nowrap`, and dialog titles that wrap now, which ran under
the close button). Its cost, in the debug build on Xvfb: none scrolling a
list or a grid of 2,000 files, about 3 % of a frame scrolling the
settings' Plugins page, all sentences (65.7 against 67.6 ms, median).
A second walk, of what the first left out (53 states at 1280, 640 and
460 px: tooltips, drags, menus and their fly-outs, prompts, the command
palette, a review, previews and their menus, the columns view, a split,
tabs, the terminal), found the two a narrow layout kept, and draws alike
since but for what the fixtures change (a clock, free
space). With the rule for a kept layout, scrolling the Plugins page costs
about 3 % of a frame and 5 % of the window thread's time, the palette
nothing; a first rule, which laid such texts out twice again, cost 6 to
12 %. The tests are
`text_system::line_layout::tests::test_min_wrap_width_is_the_widest_word`,
`elements::text::tests::a_text_in_a_narrow_row_wraps`,
`elements::text::tests::a_text_laid_out_narrow_first_keeps_its_line`
(it fails with Zed's rule for a kept layout),
`elements::text::tests::a_text_s_narrowest_is_kept_with_its_unwrapped_lines`
and
`window::tests::what_is_laid_out_on_its_own_keeps_its_words_on_a_line`
(it fails at `min_size`, for a tooltip and for a drag).

### 0063: A hitbox frontmost under the mouse

A title bar the application draws moves the window from its empty parts
only: a press on a button over it is the button's. `Hitbox::is_hovered`
holds there too, a button not occluding (an occluding one would hide the
root's own listeners under it, as Skwir's press counting).
`Hitbox::is_frontmost`: no hitbox inserted after it contains the mouse,
and no other has captured the pointer. Skwir's `elements::window_drag`
asks it (ADR-138).

Checked: `just gpui-check gpui`
(`a_hitbox_is_frontmost_where_nothing_is_over_it`); in a window on headless
sway, a press on Split split the panes without moving the window, one beside
it moved the window.

### 0064: Rounded masks meet at a corner

0053's intersection of two rounded masks keeps the smaller rounded
rectangle whole: a card on a window's edge, square there (`frame::card`),
inside the window's rounded mask (where Skwir draws the window's frame,
ADR-138), drew its square corner over the window's rounded one. Where the
two rectangles have a corner at the same point (within half a pixel), the
kept one is rounded there by the larger radius: exactly the two shapes'
intersection at that corner.

Checked: `just gpui-check gpui`
(`rounded_masks_meet_at_a_corner_along_the_larger_arc`); on headless Weston,
the files card at the window's bottom-right corner rounded with it. Not
checked: Windows and macOS (the change is in gpui's shared code).

### 0065: Decorations as the compositor answered

`request_decorations` set the window's decorations to those asked and drew
them at once, before the compositor's answer (`xdg-decoration`'s
`configure`, which it sends after `set_mode`). Hyprland keeps its frame
whatever is asked: an application asking for none drew its own title bar
for a frame or two, then took it back. The window keeps the decorations it
has until the answer, which sets them and updates the window (its geometry,
its opaque region, as the request did); `Window::decorations_pending` says a
request differing from them waits (none elsewhere, nor where no decoration
manager answers, where the request still holds at once). The answer is the
surface's `configure` that follows the request, which the protocol
promises; the decoration's own `configure` before it may be missing where
the mode stays (amended: waiting for the decoration's alone, a compositor
keeping its mode left the request waiting for good). A wait that ends
unchanged tells the window as a change does (`appearance_changed`). A
configure sent before the compositor read the request ends the wait early:
the window then draws what it has, never what was asked.

Checked: in a window on headless sway, a window opened asking for none and
kept in sway's frame (`border pixel`), then switched both ways on the open
window; a first version holding the state's borrow across the redraw
panicked at the window's opening, found there. Not checked: Hyprland
itself; no test (gpui's tests have no compositor to answer).

### 0066: The title bar on Windows and macOS

Skwir draws its title bar where the user asks (ADR-138), on an open window
too. gpui set Windows' hidden title bar and macOS's transparent one when a
window opened (`TitlebarOptions::appears_transparent`) and said
`Decorations::Server` whatever they were.

- **Asked and said alike everywhere**: `request_decorations(Client)` hides
  Windows' title bar (`hide_title_bar`, a `Cell` now, the frame recomputed
  by `notify_frame_changed`) or makes macOS's transparent (the content
  under it, the title hidden, AppKit asked on the foreground executor as
  `toggle_fullscreen` is, the traffic lights moved or put back);
  `Server` gives the system's back. `window_decorations` says which.
- **Changed after the update that asks**: Windows' frame is recomputed on
  the foreground executor, as `resize` moves the window: recomputed at
  once, it sent `WM_SIZE` while the window was being updated, whose
  report of the new size could not borrow the app and was lost, the
  window drawn at its old size. `Window::request_decorations` reads the
  window's size again before the next frame, for any platform that
  changes it at once.
- **The traffic lights moved only over a transparent title bar**:
  `move_traffic_light` runs at every resize, title and occlusion change,
  and moved them to the position given even after the system's title bar
  came back, into or below it.
- **Windows' `zoom`** restores a maximized window, as Wayland's and
  macOS's do.
- **Tiled as the system's are**: the application's decorations of a
  maximized or fullscreen window (Windows; fullscreen on macOS) say every
  edge tiled, as the default `tiling` says of the system's: an
  application's frame leaves no room above its top row there, where the
  screen's corner is Close's (amended the same day).
- **A drag area where it is frontmost**: the hit test answered a
  `WindowControlArea::Drag` wherever the mouse was within it, a button
  over it included, which then never got its press. A drag area now
  counts where no hitbox is over it (as 0063's `is_frontmost`); a
  button's area (Close, Max, Min) where the mouse is over it.

Checked: clippy for Windows (`x86_64-pc-windows-gnu`) and macOS
(`aarch64-apple-darwin`, in `tools/package/macos.sh`'s environment) on
Skwir's UI. Not run: a gpui window cannot be drawn under Wine, and no Mac
is at hand.

### 0067: Client decorations supported

Where X11 runs no compositor, or its window manager reads no
`_GTK_FRAME_EXTENTS` (i3 with picom), gpui falls back to the window
manager's frame whatever the application asks (`request_decorations`),
which an application learned only once it asked. gpui knows before: the
compositor's selection and the window manager's supported atoms are read
as the app starts (`client_side_decorations_supported`), once: a
compositor started or stopped later is not seen until the app starts
again (following the selection's owner, XFixes, is left). `PlatformWindow` and `Window`
`client_decorations_supported` say it: X11's flag; true elsewhere (a
Wayland compositor answers only once asked). Skwir offers its title bar's
two values as choices that do nothing there (ADR-138).

Checked: on Xvfb with no window manager, Settings › Appearance › Title
bar offered Compact and In the top row dimmed with why, and a value set
in the file was kept and said to do nothing there.

### 0068: Drag areas that keep the double click

A `WindowControlArea::Drag` is a caption to Windows, which maximizes or
restores the window at its double click. Where a double click means
something of the application's (Skwir's tab strip, where it opens a
tab), the area was told as nothing, and Windows moved the window from no
part of it: a maximized window with its tabs shown had only the gaps
between its buttons to be dragged by. `WindowControlArea::DragOnly` is a
caption too (`HTCAPTION`, where it is frontmost, as 0066 has a drag
area), but its double click (`WM_NCLBUTTONDBLCLK`) goes to the window
alone, never to `DefWindowProc`. The release that follows it counts the
clicks as its press did (it said one), so the application's click handler
sees a double click.

Checked: clippy for Windows (`x86_64-pc-windows-gnu`) on Skwir's UI. Not
run: a gpui window cannot be drawn under Wine.

### 0069: Right-to-left text

Hebrew and Arabic were shaped and drawn right on one line by cosmic-text
and Core Text, but everything gpui worked out from the glyphs took their
order in a run for their text's and the order they are drawn in at once:
a caret in a Hebrew name stood at the wrong end of its letter, a click
landed on the letter beside it, a selection was one rectangle from where
its start's caret was to its end's, a color or underline stopped at the
first glyph whose character came earlier, and a wrapped line was cut in
the order it is drawn (on macOS, a Hebrew paragraph's last words on its
first row; on Linux, where cosmic-text hands a line read right to left
from its right end, never cut at all). `text_system::bidi` says how each
answer is found; in short:

- **Levels.** A line whose text holds a right-to-left letter or control
  (`R`, `AL`, `RLE`, `RLO`, `RLI`; a scan of its characters, none for
  ASCII) keeps each byte's embedding level, UAX #9 run by `unicode-bidi`
  (already in gpui's tree, by usvg) through rule L1, and each paragraph's
  (`LineLayout::bidi_levels`, a `BidiLevels`). A paragraph's direction is
  its first strong character's, as cosmic-text and Core Text take it (and
  DirectWrite, asked so: 0070), unless the text says one (0084). Every
  other line keeps `None`, and every
  answer below is worked out as before, by the same code, behind one test
  of that `Option`: a line all left to right costs no more than it did,
  but the scan when it is shaped (not each frame it is drawn: the layout is
  kept). The scan asks the blocks such characters are in before the class
  table (a test walks every code point): in a release build, on a machine
  under load (15), 12 to 30 ns for a 30-byte ASCII name, 0.19 to 0.35 µs
  for a French one, 0.15 to 0.18 µs for 15 CJK characters (1.2 µs asking
  the table for each); a Hebrew name's levels, 4 to 5 µs.
- **Drawn order.** The glyphs of such a line are put in the order they
  are drawn, left to right, where the platform handed them otherwise
  (cosmic-text a line read right to left from its right end; a run of Core
  Text's from its end, or its runs in their text's order, which it is free
  to): each run turned left to right, the runs set side by side
  (`LineLayout::put_in_drawn_order`). Painting, wrapping and every walk of
  a line's glyphs assume it.
- **Cells and carets.** The glyphs side by side with one index are a cell
  (a letter and its marks, a ligature), from its left edge to the next
  one's. A place in the text stands where the cell after it begins (its
  right edge, read right to left) or where the cell before it ends. Where
  the two differ (the text turns direction), it takes the edge no other
  place stands at, so every edge is reached: both ends of a Hebrew word in
  an English name, the far left of "אבג.txt". Where both are free, the
  edge of the cell at the lower level, the paragraph's own direction
  (where a Hebrew word starts in an English sentence, the caret stands
  after the English before it, where a Latin letter typed there goes). `x_for_index`,
  `closest_index_for_x` and `index_for_x` read them, round trip included;
  `index_left_of` and `index_right_of` walk them as drawn, for arrow
  keys; `selection_ranges` gives a selection's spans of x, one for each
  stretch drawn side by side.
- **Rows.** Such a line wraps in its text's order (the same rule as
  `compute_wrap_boundaries`, its cells measured one after another), each
  row then laid out as it reads: its trailing spaces at their paragraph's
  level (L1), its cells reordered (L2), its glyphs moved. The rows follow
  each other along x in the layout a `WrappedLineLayout` draws from, as a
  wrapped line's rows always have, so painting, alignment, boxes (0060)
  and what reads a row's ends work unchanged; a line that fits keeps its
  layout. Where a row starts in the text is `WrappedLineLayout::row_starts`
  (its first character in the text's order, drawn at its right end in
  Hebrew), which `TextLayout::row_bounds_for_index` and `wrapped_text`
  read; `position_for_index` and `index_for_position` answer row by row as
  above. A row holding two paragraphs reorders each on its own (0086); the
  answers are kept with the line (0087).
- **Decorations.** A glyph whose character comes before its decoration
  run's start (one read right to left, painted from its end) has its run
  looked for from the first again: colors, backgrounds, underlines and
  strikethroughs follow their characters.
- **Tests' text system** (`NoopTextSystem`) lays right-to-left text out as
  it reads, as a platform does, so tests of it see what a window shows.

Not done: a paragraph read right to left is still drawn from its
element's left (`TextAlign`, its start being its right is a mirrored
layout's, not this; what lays a paragraph out in its own direction asks
0081, as Skwir's documents do); cosmic-text's mark offsets, which gpui drops for
every script, are not drawn (a vowel point may sit beside its letter:
upstream's, unchecked in a window); a line whose runs sit off the
baseline (0006) is shaped a stretch at a time, each its own paragraph;
`ShapedLine::split_at` and its cursor cut such a line by its text's order
as before (Skwir uses neither); a text's minimum content size (0061)
counts such a line's rows right but grows a row for a box (0060) by its
cells' places before they are laid out.

The tests are `text_system::bidi::tests` (every right-to-left character
in the blocks asked first, Hebrew, Arabic with digits, a
Latin name with a Hebrew part, a Hebrew word between Latin ones, wrapped
Hebrew and mixed paragraphs, selections, levels, drawn order),
`text_system::line::tests::a_decoration_follows_its_characters_in_right_to_left_text`
(nothing was underlined before), and gpui_wgpu's `cosmic_text_system::tests::a_right_to_left_line_is_taken_from_its_left`,
shaped by cosmic-text.

Checked in a window too (Skwir's debug build on Xvfb, 2026-11-17): Hebrew,
Arabic and mixed names in the grid and in the rename field, the stem
selected where it is drawn, Home and End at the text's two ends, the
arrows walking the letters as drawn (across a Hebrew word in a Latin
name, one edge a press), a click landing between the letters under it, a
drag over the turn of direction selecting two spans; a text preview wraps
a Hebrew paragraph from its first words, a mixed line row by row, Arabic
with its digits, and a selection's background follows its characters
there. Compiled for macOS (Skwir's UI type-checked for
`aarch64-apple-darwin` against the SDK, gpui_apple and gpui_macos with it)
and for Windows; run on neither. On macOS nothing of Core Text's is
changed: the rows of a wrapped Hebrew paragraph came out in the wrong
order there because gpui cut a line in the order it is drawn, which this
patch does in its text's order on every system.

### 0070: DirectWrite's right-to-left runs

DirectWrite hands `DrawGlyphRun` each run with its origin and its bidi
level: a run at an odd level reads right to left, its glyphs in their
text's order and placed leftward from its origin, its right end. gpui
placed every run where the last ended, rightward: Hebrew and Arabic were
drawn backwards (zed#31102). Where the text reads both ways (0069's
`BidiLevels`), each glyph is placed as DirectWrite draws it, from the
run's origin, leftward at an odd level (an offset along the run's
direction too), the run turned left to right, and the line moved so its
left end is at 0. A line whose first strong character reads right to left
is laid out so (`DWRITE_READING_DIRECTION_RIGHT_TO_LEFT`, its box as wide
as the line, no wrapping), as cosmic-text and Core Text lay it out. A line
all left to right is placed as before, by the same code.

The test is `direct_write::tests::right_to_left_runs_are_placed_as_drawn`,
for Windows itself: under Wine 11 (`just gpui-check-windows
right_to_left_runs_are_placed_as_drawn`) gpui's DirectWrite selects no font
at all, Segoe UI, Wine's own Tahoma added from its data folder, or a font
its system collection lists (Inter, Unifont), so it fails there before
laying anything out (as Zed's tests of DirectWrite's separators would).
Checked: it builds for Windows and runs under Wine to that point; Skwir's
UI passes clippy for Windows (`x86_64-pc-windows-gnu`). Not seen drawn: a
gpui window cannot be drawn under Wine.

### 0071: Word characters

The wrapper (`LineWrapper::is_word_char`, which `LineLayout`'s wrapping
asks too) took a word to be made of ASCII, Latin, Cyrillic and Bengali
letters and some punctuation, from a list of blocks: a Greek, Armenian,
Georgian, Hebrew, Arabic or Devanagari word wrapped between any two
letters, and a mark typed apart could be cut from its letter. Beyond
ASCII, which keeps its rules and goes first:

- **Letters, marks and digits** of every script are word characters, by
  their general category (`unicode-properties`, already in gpui's tree by
  usvg's rustybuzz), not by blocks.
- **But not the letters and digits of scripts written without spaces
  between words** (by their script, `unicode-script`, there by rustybuzz
  too): Han, Hiragana, Katakana, Bopomofo, Hangul, Yi, Tangut, Nüshu and
  Khitan break between any two characters, as before; so do Thai, Lao,
  Khmer, Myanmar and the Tai scripts, whose words want a dictionary (UAX
  #14's class SA) that gpui does not have: a line of Thai breaks between
  characters, where a word may end or not, rather than run as one word as
  wide as its sentence. Korean breaks between syllables, as UAX #14 and a
  browser's `word-break: normal` do. A mark of any of them stays with its
  letter: no line starts with a Thai vowel sign or tone mark.
- **Glue** beside the punctuation and no-break spaces of before: the
  joiners (ZWJ, word joiner, zero width no-break space; UAX #14's LB8a and
  LB11) and the bidirectional marks, embeddings, overrides and isolates
  (U+061C, U+200E, U+200F, U+202A to U+202E, U+2066 to U+2069; class CM,
  LB9), so a name isolated inside its quotes (“⁨…⁩”, ADR-139) wraps as its
  words and its quotes do, and a text's narrowest width keeps the quotes
  with the name.
- **What moves:** `×` and `÷`, symbols, are a place to wrap now, as `+`
  is; `ª`, `º`, `µ`, `²`, `½`, letters and numbers below Latin-1's letters,
  are word characters.
- **Speed.** A character of the Basic Multilingual Plane is answered from
  its page of 256, worked out the first time one of it is asked about
  (`WORD_PAGES`, 8 KiB at most): measured on a release build under load,
  6 to 9 ns a character for French, Greek, Hebrew and CJK names, against 9
  to 12 for the list of blocks before and 20 to 100 asking the tables each
  time; ASCII 7 against 12. A page costs its 256 look-ups once.

Tibetan (no break before its tsheg, after it) and Japanese kinsoku beyond
the closing punctuation listed are not done. 0072 replaces this rule by
UAX #14's, and keeps its cases as tests.

The tests are `text_system::line_wrapper::tests::test_is_word_char` (each
script, the marks, the glue, every page against the characters worked
out) and `text_system::line_layout::tests::a_word_of_any_script_is_kept_whole`
(the narrowest width of Greek, Devanagari, Georgian, a name isolated in
its quotes, Chinese and Thai), and the Hebrew paragraph's narrowest width
in 0069's `a_hebrew_paragraph_wraps_from_its_first_row`.

### 0072: Line breaks by UAX #14

0071 told a word by its characters' properties, and a line wrapped by a
rule between two characters (`wraps_before`): before a word after a space,
before any other character but a space. That rule can say nothing of what
needs more than two: a closing bracket or full stop that must not start a
line (Japanese and Chinese kinsoku), a Thai sentence whose words only a
dictionary tells apart (0071 broke it between any two letters), Tibetan's
tsheg, French guillemets kept with their words. `text_system::line_breaks`
answers where a line may wrap by the Unicode line breaking algorithm, UAX
#14, as ICU4X's line segmenter has it (`icu_segmenter` 2.4, the standard's
version 15.1, its strict default, no locale's tailoring), found once for a
line's text (`LineBreaks::of`: a bit a byte, 256 bytes inline):

- **Who asks.** The shaped layout's wraps (`compute_wrap_boundaries`, and
  0069's `wrap_in_text_order`), its narrowest width (`min_wrap_width`,
  0061) and the line wrapper (`wrap_line`, `truncate_wrapped_line`) ask the
  same breaks, so they never disagree; 0071's `is_word_char` and its pages
  are gone, and with them gpui's `unicode-properties` and `unicode-script`.
  A glyph after the first of its character is no place to wrap (0071's
  rule let a CJK character drawn in two glyphs be cut between them).
- **ASCII** is answered without the segmenter (`line_breaks::ascii`): its
  classes (`LineBreak.txt`) and the rules that apply to them, LB4 to LB31,
  in a pass over the bytes. A test compares it with the segmenter on every
  pair of ASCII characters, every string of up to four of its classes and
  3,000 longer ones at random: the same answer for each.
- **Where it parts from 0071's rule**, the standard is followed: a hyphen
  lets a line wrap after it (`non-|English`, `x-|ray`, but `a-1`, `x -1`
  and `file_2026-11-18.txt` whole), a slash and a question mark after them
  rather than before (`a/|b/|c.txt`, `foo?|b=2`); `&`, `*`, `<`, `>`, `+`
  and `=` are letters and numeric prefixes (`a=1&b=2`, `2+3` whole; `C+|+`);
  no break between a word and its bracket (`a(b)c`, `photo(1).jpg`), nor
  before punctuation after a space (`a !`); `×` is a letter (`2×3`); a
  word's marks and the joiners, no-break spaces and bidi controls glue as
  in 0071.
- **Gpui's tailoring.** A line's indent, the spaces it starts with (and
  each line's after a newline), is no place to wrap, nor the place before
  its first other character (an editor's continuation rows keep the indent,
  `IndentAdjustment`). And a row a space overflowed into starts with it: no
  place to wrap comes before its first word, so no row is a space alone.
  0071's wrapper had that only by accident (the character before was not
  updated after a forced wrap, so Zed's `test_extra_columns_overflow_guard`
  passed), its shaped layout not at all; both say it now.
- **Kept while wrapped.** A text's breaks are kept by the text in the line
  layout cache (`LineLayoutCache::breaks_of`), frame to frame as its lines
  are, whatever width or font it is wrapped at: a window resized wraps its
  texts again without segmenting them again. `LineLayout::wrapped` and
  `compute_wrap_boundaries` take the breaks found; `min_wrap_width` and the
  wrapper find their own.
- **An element** of a line (`LineFragment::Element`, Zed's inlays) reads as
  a letter where its line's breaks are found: a line wraps before one after
  a space, never between one and the word it stands in, as before.
- **The data** is compiled in: the rules, and the Thai, Lao, Khmer and
  Burmese dictionaries (`LineSegmenter::new_dictionary`; ICU4X's LSTM model,
  smaller, segments Thai about 100 times slower). It is named in one place,
  `line_breaks::segmenter`, where a data pack read at run time could take
  its place (`try_new_dictionary_with_buffer_provider`).

**What it costs**, measured on release builds of gpui's library tests,
0071's and 0072's, run one after the other ten times on a machine others
used (load 5 to 15 on 20 threads), the fastest batch of each kept; a line
of glyphs 8 px wide (Chinese 16), wrapped at a third of its width; lines of
a name's length and paragraphs four sentences long (`wrap_bench.rs` in the
night's scratchpad, not kept):

| line | chars | wrapped (ns) | narrowest | wrapper | resized |
| --- | --- | --- | --- | --- | --- |
| ASCII | 63 | 342 → 409 | 212 → 224 | 293 → 424 | 614 → 581 |
| French | 71 | 391 → 937 | 252 → 651 | 398 → 921 | 716 → 646 |
| Japanese | 34 | 268 → 914 | 167 → 709 | 279 → 877 | 431 → 455 |
| Thai | 61 | 366 → 2,675 | 260 → 2,452 | 419 → 2,800 | 644 → 666 |
| ASCII | 576 | 2,681 → 3,370 | 1,990 → 1,937 | 2,500 → 3,898 | 3,389 → 2,884 |
| French | 652 | 3,054 → 8,530 | 2,334 → 7,015 | 2,981 → 9,072 | 3,963 → 3,332 |
| Japanese | 312 | 1,595 → 7,248 | 1,409 → 6,133 | 2,252 → 7,680 | 2,321 → 2,465 |
| Thai | 596 | 3,507 → 23,302 | 2,718 → 21,479 | 4,083 → 24,339 | 5,053 → 3,889 |

"Wrapped" is `compute_wrap_boundaries` with the breaks found anew,
"narrowest" `min_wrap_width`, "wrapper" `LineWrapper::wrap_line`, and
"resized" a line wrapped through the line layout cache at another width
each frame, its shaped layout kept: what a window being resized costs, the
breaks kept. Found alone, the breaks cost 2.5 ns a character for ASCII (the
table of steps), 10.5 to 12.5 for French, 23 to 26 for Japanese and 44 to
46 for Thai (the segmenter). Kept, a resized line costs what it did, within
the noise; found anew, ASCII costs 1 to 1.6 times what it did, French 2.3
to 3, Japanese 3 to 4.5, Thai 6 to 9.5. The wrapper keeps nothing (Skwir's
grid counts its names' lines with it as its order or zoom changes,
`GridView::measure`).

**What the data weighs**, stripped: gpui's `hello_world` example, release,
is 1,091,528 bytes larger (26,768,184 → 27,859,712). Alone in a binary, the
line segmenter's rules and code are about 68 KB and the four dictionaries
about 1.05 MB (ICU4X's LSTM models instead: 400 KB in all). Skwir's core
finds word boundaries with ICU4X's word segmenter and the same
dictionaries (`skwir_core::segment`): beside it, the line segmenter adds
about 37.6 KB (a probe binary of each, 3,475,160 → 3,512,760 bytes).

Checked: gpui's library tests (`just gpui-check`: 168 in gpui, gpui_wgpu's
and gpui_linux's), Skwir's UI tests (743), gpui and gpui_windows compiled
for Windows, gpui for macOS. Not seen in a window.

The tests are `text_system::line_breaks::tests` (ASCII against the
segmenter; where ASCII parts from 0071's rule; the indent; 0071's words of
every script, kept; glue, isolated names and guillemets; kinsoku, small
kana; Thai, Lao, Khmer, Burmese and Tibetan),
`text_system::line_wrapper::tests::test_wrap_line_where_the_standard_allows`,
`text_system::line_layout::tests::a_row_a_space_overflowed_into_is_not_left_alone`,
`text_system::line_layout::breaks_kept_tests` and 0071's
`a_word_of_any_script_is_kept_whole`, a Chinese full stop and a Thai
sentence added.

### 0073: The language for assistive technology

A screen reader picks a voice by the language of what it reads, and gpui
gave AccessKit none: Skwir's words in French were read with an English
voice's rules (ADR-139). AccessKit's nodes say a language
(`Node::set_language`, a BCP 47 tag), which holds for the nodes below one
but those that say their own.

- **The window's**: `Window::set_a11y_language(Option<SharedString>)`,
  kept beside the window's title (`A11y::language`) and said by the root
  node of every frame's tree from the next one; `None` says none.
- **An element's**, where it is not the window's (a name in another
  language, a quotation): `aria_language` on a div with an id and a role
  (beside `aria_label`), and on a `StyledText`, said by the
  `InteractiveText` that is its node.

Not done by AccessKit itself on Linux: accesskit_unix 0.22.1 answers
AT-SPI's `Accessible.Locale` with an empty string whatever the nodes say,
and exposes a node's language only among a text's attributes
(accesskit_atspi_common's `text_attributes`), which gpui's nodes have none
of: Orca on Linux reads no language from it yet. What macOS's and Windows'
adapters make of it was not checked.

The tests are `window::a11y::tests::the_window_s_language_is_said_by_its_root`
(none until set, the root's every frame, a node's own kept, unset),
`elements::div::tests::test_write_a11y_info_aria_language_builder` and
`elements::text::tests::a_text_says_its_language_where_it_has_one`.

### 0074: Chinese characters in the language's forms

A Chinese character is drawn in the fallback font when the font asked for
has none (Skwir's own, Inter and JetBrains Mono, have none), and its form
differs by language: 直, 骨 and 誤 are not drawn alike in Japanese,
Korean, Traditional and Simplified Chinese. cosmic-text picks the
fallback by its locale (`han_unification` in its `fallback/unix.rs`), but
by an exact tag, "ja", "ko", "zh-HK" or "zh-TW", Simplified Chinese
otherwise, while the system's locale comes as `ja-JP` or `ko-KR`: a
Japanese or Korean desktop drew Simplified Chinese forms (seen on Xvfb,
`LANG=ja_JP.UTF-8` and `ko_KR`, 2026-11-18). harfrust is given no language,
so a font's `locl` feature does not mend it.

- **The key** (`han_key`, in `gpui_wgpu`): Japanese is "ja", Korean "ko";
  another language's script and region, likely where it says none (ICU4X's
  `LocaleExpander::new_common`), name the forms: Traditional Chinese is
  "zh-HK" in Hong Kong and Macao and for Cantonese, "zh-TW" elsewhere;
  Simplified Chinese "zh-CN". Any other language is given as it is. A POSIX
  locale's encoding and modifier (`ja_JP.UTF-8`) are left out.
- **At start**, `CosmicTextSystem::new` makes its font system again with
  that key (`into_locale_and_db`, `new_with_locale_and_db`: the database
  moves whole).
- **When asked**: `TextSystem::set_language(&str)` (for a language chosen
  in the application) reaches `PlatformTextSystem::set_language`, which
  answers whether the fallbacks changed; cosmic-text's font system is then
  made again from its own database, so the faces loaded keep their ids, and
  the text system lays out again what it had laid out, as after
  `add_fonts` (its font generation moves). macOS and Windows, which pick
  their own fallback, change nothing.

The tests are `cosmic_text_system::tests::han_key_names_the_forms_of_chinese_characters`
(ja-JP, ko-KR, zh-Hant, zh-TW, zh-HK, zh-MO, yue, zh, zh-SG, zh-CN,
zh-Hans-HK, fr-FR, C) and
`cosmic_text_system::tests::a_language_set_picks_its_fallbacks_and_keeps_the_faces`.
The forms themselves were not looked at in a window after the change.

### 0075: Layout direction

gpui handed taffy every box left to right, though taffy 0.15 lays out right
to left too (`Style::direction`): a window in Arabic, Hebrew or Persian
could not be mirrored (docs/i18n.md, level 3). This gives gpui CSS's
`direction`:

- **`LayoutDirection`** (`LeftToRight`, the default, or `RightToLeft`), a
  field of `TextStyle` (`layout_direction`), so inherited as CSS's is:
  `Styled::layout_direction` sets it on an element and what it holds, an
  element inside setting its own back where it must stay left to right
  (a code view, a terminal).
- **To taffy.** `Window::request_layout` and `request_measured_layout`
  hand taffy the direction inherited where they are called (a div pushes
  its own text style first), and taffy mirrors what it mirrors: a flex
  row runs from the right, `justify_start` and `justify_end` (and a grid's
  start and end) change sides, wrapped lines and grid columns go the other
  way. Paddings, margins, borders and insets stay where they are said:
  left is left (Skwir turns those itself).
- **`Window::layout_direction()`**: the inherited direction where it is
  asked (laying out, prepainting, painting), for elements that place what
  they hold by hand. A window keeps how many text style refinements on its
  stack say a direction (`layout_directions_set`), so where none does, as
  in every window left to right, it is answered without looking.
- **Text alignment.** `TextAlign::Start` and `TextAlign::End`, resolved
  against the inherited direction where a text is laid out and painted
  (`TextAlign::resolve`; a line painted with one unresolved reads it as
  left to right). The default stays `Left`; a window right to left sets
  `Start` on its root. A text's own paragraph still reads in the
  direction of its first strong letter (0069), or the one it says (0084).
- **Scrolling sideways.** A box laid out right to left that scrolls
  sideways (`overflow_x_scroll`) starts at its right: what overflows lies
  to its left, and its offset runs from 0, the right end where its content
  starts, up to `max_offset`, as CSS's `scrollLeft` runs down from 0 there
  (`ScrollHandle::offset`'s sign is the other way round from left to
  right's). `ScrollHandle::scroll_to_item` brings an item into view as left
  to right, one wider than the box with its right edge in view. A wheel
  turned on a box that scrolls only sideways moves it toward its end as it
  turns down, leftward there; what the platform reports sideways (a
  trackpad, a tilted wheel, Shift and the wheel) moves it where it points.

Left to right, nothing changes: gpui's 486 library tests and Skwir's UI
tests (744) pass as before.

A scrollbar drawn beside a box that scrolls sideways reads
`ScrollHandle::offset` and turns its sign right to left (Skwir's does).
`uniform_list` and `list`, which lay their items out as roots: 0082.

The tests are `elements::div::layout_direction_tests` (a row left to right
as before, right to left from its right and its end on the left, a
direction inherited but where an island says its own, paddings where
they are said, `Window::layout_direction` in prepaint),
`elements::div::scroll_axis_tests::a_strip_left_to_right_scrolls_as_it_did`
and `a_strip_right_to_left_starts_at_its_right` (offsets, maximum, items'
places, the wheel both ways, `scroll_to_item` to the far end and back),
`style::tests::start_and_end_align_by_the_layout_direction` and
`elements::text::tests::a_text_aligned_to_its_start_follows_the_layout_direction`.
Compiled for Windows and macOS; not seen in a window.

### 0076: Shortcuts on layouts without Latin letters

gpui named a key by what it types (`keystroke_from_xkb`), its US position
(`guess_ascii`) only where that was no ASCII character. On a layout of
Greek, Cyrillic, Hebrew or Arabic letters, the ASCII symbols it types
named its shortcuts: Greek Ctrl+Q was `ctrl-;`, Hebrew Ctrl+W `ctrl-'`,
Russian Ctrl+Shift+/ `ctrl-,` (it opened Skwir's settings), Arabic
Ctrl+Shift+Z `ctrl-~`. A digit row that types none named its shortcuts by
what it types: AZERTY's Ctrl+1 was `ctrl-&`, Persian's `ctrl-farsi_1`. And
`guess_ascii` gave `>` for Shift and the comma, `<` for Shift and the
period. Only while Control, Alt or Super is held, what a key types (its
`key_char`) unchanged:

- **A layout whose letters are not Latin** (none of its keys gives a
  letter from a to z at its first level, as Firefox's `IsLatinGroup` asks;
  kept with the keymap, `latin_layouts`) names a shortcut as the keymap's
  first Latin layout does, at the level the same modifiers give there
  (`ShortcutKey::Latin`: Russian beside a US layout, Ctrl+Shift on the
  slash key is `ctrl-?`), or as a US layout does where the keymap has none
  (`ShortcutKey::UsPosition`, `guess_ascii`). Then the names as before:
  Shift on a symbol left out (`ctrl-?`), kept on a letter (`ctrl-shift-z`).
- **The digit row** (keycodes 10 to 19): a key whose first level gives no
  digit (AZERTY's `&`, `é`; Persian's `۱`) names a shortcut by the digit a
  level of it gives, the lowest first, else its US digit
  (`digit_of_row`): AZERTY's Ctrl+& and Ctrl+Shift+& are both `ctrl-1`.
  Where its first level is a digit, its shifted symbols keep their names:
  a US layout's Ctrl+Shift+1 is still `ctrl-!`, a German one's
  Ctrl+Shift+2 `ctrl-"`.
- **`guess_ascii`** knows the digit row (`1` to `0`, `!` to `)` with
  Shift), `-` and `_`, `=` and `+`, and gives `<` and `>` where they are.
  Its new keys name only what is above: a key of a Latin layout that types
  no ASCII character there keeps its own name (German's `ß`, AZERTY's `é`
  outside a shortcut).

Every key's name, Control, Control and Shift, Alt and none held, was
compared before and after on US, German, AZERTY, French Ergo-L, Greek,
Russian, and US with Russian active (a scratch test, not kept): US,
German and Ergo-L name every key as before (Ergo-L's Ctrl+Shift+1 to 3
stay `ctrl-shift-eurosign`, `-guillemotleft`, `-guillemotright`, no US
key's names), keys held alone as before but for Shift and the comma or
period of a layout without Latin letters (`<` and `>` now, as a US layout
gives them). What changed: AZERTY's Ctrl and Alt with the digit row (to
`ctrl-1` … `ctrl-0`); Greek's Ctrl and Alt with Q (`ctrl-q`,
`ctrl-shift-q`); Russian's and US-with-Russian's shortcuts on symbol keys
to their US positions (`ctrl-/`, `ctrl-@`, `ctrl-?`, `ctrl-|`, `ctrl-{`,
`ctrl-<` on the key beside the left Shift, 49 names in all).

The tests are `linux::platform::tests::a_shortcut_is_named_by_a_latin_layout`
(Greek, Hebrew, Russian, Arabic, US with Russian, AZERTY with Russian,
AZERTY's and Persian's digit row, German, US, keys held alone) and
`linux::platform::tests::a_us_position_is_as_a_us_layout_has_it` (the
table, against a US keymap's own names). X11 and Wayland both name keys
by `keystroke_from_xkb`; checked in tests on xkeyboard-config's layouts,
not in a window.

### 0077: No isolates for assistive technology

Skwir isolates a name inside its sentences (FSI … PDI, ADR-139) so its
letters are drawn in their order whatever direction they read in. gpui
hands what it draws to AccessKit as it is: a text's string is its node's
value (`elements::text`), the window's title its root's label. Speech
needs no isolate, and braille shows each as a cell of its own.
`A11yNodeBuilder::finalize`, once a frame's tree is built (only while a
reader listens), takes U+2066 to U+2069 (LRI, RLI, FSI, PDI) out of each
node's label, description, value and placeholder, writing a string again
only where it holds one (`leave_out_isolates`). A text run (a node of role
`TextRun`, or one with `character_lengths`: a field's, a terminal's, a
reader's) keeps them, the offsets its caret is told counting them.

The test is `window::a11y::tests::isolates_are_left_out_of_what_a_node_says`
(a label "Moved “⁨a⁩” to Trash", a description with LRI and RLI, a value
with FSI, a placeholder without any, a text run's value with FSI kept).

### 0078: Kept breaks for line wrappers

0072 keeps a text's breaks with the window's line layout cache, but a line
wrapper (`LineWrapper::wrap_line`, `truncate_wrapped_line`) found them anew
each call: Skwir's grid counts each name's lines with one
(`name_lines::count`), once for each zoom and text size, and a name beyond
ASCII was segmented again each time. The text system now keeps them
(`TextSystem::line_breaks`, `line_breaks::KeptBreaks`), by the text, for a
wrapper wrapping one text alone (a fragment of a line with elements, an
editor's, finds them as before):

- **Beyond ASCII only.** ASCII's table of steps costs about what a look-up
  does; it is found anew and never kept.
- **Bounded.** Two generations of `KeptBreaks::GENERATION` (4,096) texts:
  a text asked for is kept in the current one; once it is full it becomes
  the previous one and what was there goes. A text asked again from the
  previous generation comes back to the current one. At most 8,192 texts,
  each its name and a few words of bits (some 0.9 MB at the most).
- **One lock**, the text system's, taken once a text: a look-up, and on a
  miss the breaks found and kept under it.

Measured on release builds of gpui's library tests, 0071's, 0077's and
this one's, each run six times in turn on a machine others used (load 2 to
8): 2,000 names, counted once at 96 px (as a folder is opened, new names
each round), then at three other widths (as it is zoomed), the fastest of
each kept, nanoseconds a name, 9.6 px a character:

| names | 0071 | 0072 | 0078, first | 0078, again |
| --- | --- | --- | --- | --- |
| ASCII, `Summer holiday 12 (copy).jpg` | 67 | 135 to 145 | 113 to 116 (not kept) | 113 to 116 |
| French, `Été à la plage 12, copie déjà triée.jpg` | 86 | 346 | 382 | 94 |
| Japanese, `夏休みの写真12（コピー）.jpg` | 81 | 304 | 381 | 101 |
| Thai, `รูปถ่ายวันหยุดพักผ่อน12.jpg` | 109 | 892 | 921 | 130 |

Counted again, a name beyond ASCII costs 1.06 to 1.16 times what it did
before 0072; counted for the first time, 1.02 to 1.25 times what 0072's
does (the text kept). ASCII's wrapper costs about twice 0071's, its table
of steps and the bits it sets.

The tests are `text_system::line_breaks::tests::breaks_are_kept_by_their_text_for_two_generations`
and `text_system::line_wrapper::tests::a_text_beyond_ascii_is_segmented_once`.

### 0079: Fit-content lengths

A row's control that has several versions (segmented buttons, else a
dropdown: Skwir's `elements::adaptive::Fit`) beside a text that wraps
(`elements::beside`) must know the row's width to choose one before the
line is laid out: a flex item's content-based basis is measured under a
max-content constraint (CSS Flexbox 9.2.3 E), whatever room the row has,
and a flex item only grows and shrinks continuously from it, so a row
whose buttons did not fit gave the dropdown the buttons' room and the
text none of it. Taffy 0.15 has CSS Sizing 4's `fit-content()` for a
flex basis and a size, which measures the item with its limit as its
definite available space, a percentage of the container's definite inner
size; gpui's `Length` had no way to say it.

`Length::FitContent(DefiniteLength)` (and `fit_content(limit)`) is one:
written `fit-content(50%)` or `fit-content(12px)`, read back by
`Length::try_from` and in JSON. A fraction is `Dimension::fit_content_percent`,
a length `fit_content_px`, in device pixels as gpui's lengths are; a margin
or an inset given one is automatic. Where a fraction has no definite size
to resolve against (a row laid out as wide as what it holds), the item is
measured as its content, as an automatic basis is.

The tests are `taffy::fit_content_tests` (a measured box of basis
`fit_content(relative(0.5))` in a row 300 px wide is measured under 150 px
and keeps the width it measured; `fit_content(px(100.))` under 100 px; an
automatic basis never under the row's width; a row as wide as what it
holds measures it as its content; the written form read back).

### 0080: Aligned hit tests

A text centered or aligned at its right (`TextAlign::Center` or `Right`,
`Start` and `End` resolved to them) is painted with each row moved by the
room it leaves (`aligned_origin_x`), but `TextLayout::index_for_position`
and `position_for_index` counted every row from the text's left, as if it
were aligned left: a click, a hover over a link, a drag selecting, a note
opened under its reference and a match brought into view landed on the
characters drawn half the row's free room (centered) or all of it (at the
right) further along. A book's centered title page selected the wrong
words. Each now finds the row it reads (`LineRows::at` for a point; for
an index, the row `row_bounds_for_index` places it in, where a row wraps
ending the row before) and moves by that row's offset,
`WrappedLine::row_offset`: what `paint` adds to it, for the alignment the
text was laid out with and its bounds' width. A text aligned left is
answered as before, by an offset of 0.

The test is `elements::text::tests::an_aligned_text_answers_where_its_rows_are_drawn`
(left as before; at the right and centered, a point and an index both
ways; wrapped at the right, each row from where it stands; Hebrew at the
right, read from it).

Checked in a window (Skwir's debug build on Xvfb, 2026-11-18): a drag over
the middle of a centered paragraph (Markdown's `<p align="center">`)
selects the words under the pointer, where the build before selected
words some 90 px to their right; a drag from the right end of a
right-aligned Arabic paragraph (0081) selects its first words.

### 0081: The direction of a text

`LayoutDirection::of_text(&str) -> Option<LayoutDirection>`: the direction
a text reads in where nothing else says one, its first strong character's
(UAX #9's P2 and P3: the first of class L, R or AL, what an isolate holds
passed over, as `unicode_bidi::get_base_direction_full` finds it), not
stopping at a paragraph separator, as HTML's `dir="auto"` reads an
element's text; `None` where it has none (digits, punctuation, nothing),
for the caller to say. Answered without a look-up while the text is
ASCII.

It is for laying out what holds a text in the direction the text reads
(`Styled::layout_direction`), as a page whose paragraphs are marked
`dir="auto"` does (GitHub's Markdown): Skwir's documents lay each block out
so, its lines, its list marker, its quotation's bar and its indent on the
side it starts at. The text's own letters already read in that direction
whatever its box says (0069). An alignment by each line's own direction
(AppKit's natural alignment, beside 0075's `Start` and `End`) was the
other way: it moves a block's lines but nothing beside them, so a bullet
stayed at the left of an Arabic item drawn at the right; a direction lays
the whole block out, and the alignment follows (`TextAlign::Start`).

The test is `style::tests::a_text_reads_in_its_first_strong_character_s_direction`
(Latin, accented Latin, Greek, Chinese, Hebrew and Arabic; digits or
punctuation first; a newline first; an isolated name in an English
sentence; a text all isolated; none).

### 0082: Lists follow the layout direction

0075 made divs follow the layout direction. `list` and `uniform_list` lay
their items out as roots and place them by hand; what that broke, right to
left, and what it did not:

- **Where an item stands** was already right: taffy sets a root laid out
  right to left at the right of the width it is given
  (`compute_in_flow_root_layout`), so an item narrower than its list stands
  at the list's right, in both, the direction the list's own or inherited.
  The tests say so now.
- **A direction a `list` says itself** (`list(..).layout_direction(..)`)
  never reached its items: they were laid out, prepainted and painted
  outside the list's text style (an inferred list measured them inside it,
  in `request_layout`), so in the direction around the list, and every text
  style the list set (a color, a size) reached them while measured but not
  while drawn. They are laid out, prepainted and painted in it now, as a
  div's children are and `uniform_list`'s were (its `Interactivity` pushes
  its own).
- **A uniform list scrolled sideways** (`ListHorizontalSizingBehavior::Unconstrained`)
  moved its rows by twice its offset: each row is given the list's width
  grown by the offset, which taffy sets at that width's right, and was
  placed at the offset too, so at its largest offset a row's left end
  never came into view. Right to left, a row is given its width from the
  list's left: its start moves right by the offset (which runs from 0 up
  there, 0075) and its end stays at the list's left, as left to right its
  start moves left by it and its end stays at the right.
- **What scrolls vertically** (the wheel, `scroll_to_item`, the scrollbar
  API of `ListState`) is the same either way. A scrollbar drawn beside a
  sideways scroll reads `ScrollHandle::offset`, signed as 0075 says.

Left to right, the rows' origins are worked out by the same arithmetic as
before. In Skwir's mirrored window nothing changes either (no list of
Skwir's says its own direction or scrolls sideways): its file list, its
columns, Settings' rows and the address field's suggestions in `ar-XB`
were compared pixel for pixel with the build before, the same.

The tests are `elements::uniform_list::layout_direction_tests` (an item
narrower than the list at its right, right to left, its own direction or
its parent's; one as wide where it was; a row 500 px wide in 300 scrolled
sideways both ways, its offset kept within its range, which failed
before) and `elements::list::layout_direction_tests::an_item_starts_at_the_list_s_start`
(a direction the list says itself reaching its items, which failed
before).

### 0083: The Window menu by a flag

gpui_macos handed AppKit as its Window menu (`setWindowsMenu:`, where it
lists the windows) the top-level menu named "Window": a menu bar in
another language lost the list of its windows. `Menu::window_menu` (a
field, set by `Menu::window_menu(bool)`, kept by `OwnedMenu`) says which
menu it is, and gpui_macos asks it, of a top-level menu or a submenu,
reading no name. Zed's menus would set it on their Window menu. Skwir
makes no menu bar yet (`cx.set_menus` is called nowhere), so it sets none.

The test is `platform::app_menu::tests::the_window_menu_is_the_one_that_says_it_is`.
gpui_macos type-checked for `aarch64-apple-darwin` (Skwir's UI, in
`tools/package/macos.sh`'s environment); not run on a Mac.

### 0084: A text's direction

A paragraph read its direction from its first strong character (0069), as
cosmic-text and Core Text take it: a line of code that starts with a
Hebrew or Arabic letter (`// تعليق`, `"שלום",` a list's item, an Arabic
comment indented) was ordered right to left, its `//`, quotes, brackets and
indent moved to its other end. Code editors (VS Code, the JetBrains ones,
Zed) lay every code line out left to right: an Arabic comment's words still
read right to left inside it, the line's structure stays where it is. The
direction a text's paragraphs read in can now be said:

- **`TextDirection`**: `Auto`, each paragraph its first strong character's
  (UAX #9's P2 and P3, HTML's `dir="auto"`), the default, so nothing that
  says none changes; `LeftToRight` or `RightToLeft`, every paragraph so
  whatever letter starts it (the standard's rule HL1). A field of
  `TextStyle` (`text_direction`), inherited, apart from the layout's
  direction (0075): `Styled::text_direction` on what holds code. A text
  element shapes its lines in it; elsewhere `WindowTextSystem::shape_text_in`,
  `shape_line_in` and `layout_line_in` take it last (`shape_text` and the
  others are `Auto`, so Zed's callers are as they were). The line layout
  cache keeps a line by its direction too.
- **Levels** (`BidiLevels::of_in`): UAX #9 with the paragraph level said.
  Left to right, a line without a right-to-left character keeps none, told
  by the same scan as `Auto`'s: a line of code in English is the line it
  was, by the same code, and costs nothing more. Right to left, every line
  has levels, as it reads from its right.
- **The platform is told** (`PlatformTextSystem::layout_line_in`, asked
  only for a direction said), so what is drawn is what gpui's carets, hit
  tests, selections and rows (0069's answers, from the levels) assume:
  - DirectWrite reads a paragraph in its layout's reading direction, never
    its first strong character's (0070 set it from that): told the one
    said (`SetReadingDirection`).
  - Core Text reads it in the base writing direction of its paragraph
    style, where one says it: the line's string is given one
    (`CTParagraphStyleCreate`, `kCTParagraphStyleSpecifierBaseWritingDirection`).
  - cosmic-text's `ShapeLine` takes it from its first strong character
    alone (`BidiInfo::new(line, None)`) and takes no other. The trait's
    default (`text_system::bidi::lay_out_marked`) sets a mark of the
    direction (LRM, U+200E, or RLM, U+200F) at the start of each paragraph
    that would read the other way: the paragraph's first strong character,
    of the level's own class, as its start of sequence is, so every
    character after it has the level HL1 gives it (a test compares them on
    texts with brackets, digits after Arabic letters, an isolate, a point
    before its letter, several paragraphs). harfrust hides a mark (no
    advance, drawn as nothing); its glyphs are left out, the others' indices
    are the text's, and the caller's runs are its own: a mark is never the
    caller's to see. A bidirectional mark (LRM, RLM, ALM) keeps the font
    span it is in (`pick_covering_slot`): the one set at a paragraph's start
    moves no letter after it to a fallback font found for the mark, and one
    of the text's own no longer splits the span of the word it is in (an
    Arabic word's joining across it included). gpui_web, whose shaping is
    cosmic-text's, takes the default too.
  - The tests' text system lays a line out with the levels said.

Skwir says it on its code views' lines (`code_preview::reads_as_code`: the
code view's rows where a grammar colors the text, a document's code blocks,
the byte view's rows), and on a document's other blocks, each in its own
direction (`markdown::Reads`), so the lines after a paragraph's hard breaks
read in the paragraph's; a plain text's rows keep each line's own
direction.

Not done: what Zed's editor would say (its lines stay `Auto` here); a
paragraph read right to left is still drawn from its element's left, as
0069 has it; `ShapedLine::split_at`, which Skwir does not use, cuts a line
by its text's order as before.

The tests are `text_system::bidi::tests::levels_are_those_of_the_direction_said`,
`a_mark_reads_a_paragraph_as_its_direction_says` (the levels a mark gives,
against those said), `a_line_laid_out_with_marks_is_the_line_said` (the
glyphs, their places and indices, the runs handed),
`elements::text::tests::a_text_reads_in_the_direction_it_inherits` and
`…_on_every_line` (a line after a hard break that starts with a Hebrew
letter, said left to right, reads so, as each line of a paragraph marked
`dir="auto"` reads in the paragraph's direction), gpui_wgpu's
`cosmic_text_system::tests::a_line_reads_in_the_direction_it_is_said_to`
(shaped by cosmic-text: a Hebrew comment after `//` left to right, Latin
before Hebrew right to left, English as it was) and
`a_bidi_mark_keeps_the_span_it_is_in`, and gpui_windows's
`right_to_left_runs_are_placed_as_drawn`, a line said left to right added
(built for Windows, not run: Wine selects no font for it, 0070). Each of
gpui's and gpui_wgpu's fails with the levels, the platform's layout and
the font span left as they were (the API kept).

Checked in a window (Skwir's debug build on Xvfb, 2026-11-18, against a
build of the same tree without 0084): a Python and a Rust file whose lines
start with an Arabic comment, an indented one and a Hebrew string keep
their `#`, `//`, quotes and indent at the left, the Arabic and Hebrew
words read right to left inside the line; a drag from a line's start over
its Arabic comment selects from its `#`; a Markdown code block the same; a
`.txt` of Arabic and Hebrew lines and a Rust file in English drawn pixel
for pixel as before. Compiled for Windows (Skwir's UI with clippy,
gpui_windows's tests) and type-checked for macOS (Skwir's UI against the
SDK, gpui_apple's text system with it); DirectWrite's and Core Text's
paths not run.

### 0085: A foreign parent

A window shown for another program (the dialog the desktop portal asks
for, ADR-141) opened as a window of its own: the window manager could
put it behind the window that asked, and moved neither with the other.
`WindowOptions::foreign_parent` names that program's window as its
display server does (`ForeignParent`, the portal's `parent_window`), and
the window is opened as its child:

- **X11** (`ForeignParent::X11`, a window's id): the window's
  `WM_TRANSIENT_FOR` names it, written before the window maps, as ICCCM
  has a dialog say whose it is. The window manager keeps it over that
  window; an id that names no window is ignored by the window manager,
  and the window is one of its own.
- **Wayland** (`ForeignParent::Wayland`, the handle the asking client
  was given when it exported its toplevel): the handle is imported
  through xdg-foreign, unstable v2 (`zxdg_importer_v2`, bound only where
  the compositor offers it), and the imported toplevel is made the
  parent of the window's surface (`set_parent_of`, sent once the surface
  is a toplevel, as the protocol asks). A handle that names nothing is
  told by the compositor's `destroyed`, which is not a destructor: the
  imported object stays inert, and the window a toplevel of its own. The
  imported object is destroyed before the toplevel. A compositor without
  the protocol leaves a window of its own.

A handle of the other display server (an `x11:` id on Wayland, a Wayland
handle on X11) is ignored. Windows and macOS have no such handle from
another program here, and leave the field unread. Skwir sets it on the
window that answers another program's Open or Save dialog
(`chooser::foreign_parent`, from the request's `parent_window`).

Checked in a window (Skwir on Xvfb and on a headless sway, 2026-11-19):
on Xvfb, a dialog asked with `x11:` and another window's id had
`WM_TRANSIENT_FOR` naming it, one asked with no window and one whose id
named nothing had none; on sway 1.12, a dialog asked by a toplevel that
had exported itself through xdg-foreign v2 floated over it (the asker
tiled), and one asked with no window tiled. Not checked: KWin, mutter and
Hyprland (whose 0.57.1 offers `zxdg_importer_v2`); no test, as gpui's
tests have no display server to answer.

### 0086: Rows reorder within their paragraphs

A line may hold several paragraphs, parted by a separator other than a
newline (UAX #9's class B: U+2029, U+001C to U+001E, U+0085, a lone CR),
which gpui's lines are not split at. cosmic-text lays such a line
out a paragraph at a time, each separator on its own, one after another in
the text's order (`layout_line_with_separators`), and 0069's levels are
each paragraph's. But a wrapped row holding the end of one paragraph and
the start of the next was reordered as one stretch (L2 over the whole
row): two right-to-left paragraphs swapped places when the line wrapped,
the same line drawn in one order unwrapped and in the other wrapped.

Each row is now cut where a paragraph starts and around each separator,
each piece reordered on its own (`drawn_by_paragraph`, L2 within the
piece), the pieces in the text's order, as the platform lays out the line
unwrapped. A place at a paragraph's start whose two edges tie (both free,
or both taken) stands at that paragraph's edge, its first character's,
rather than at the edge of the lower level, which compared levels of two
paragraphs. The tests' text system (`laid_out_as_read`) lays a line's
paragraphs out so too, as the tests' stand-in for a platform does.

The tests are `text_system::bidi::tests::two_paragraphs_on_one_row_keep_their_order`
(two right-to-left paragraphs parted by U+001C, drawn unwrapped and
wrapped with the first on the left, the carets at their ends) and
`a_row_is_drawn_in_the_order_it_reads` (pieces kept apart).

### 0087: Right-to-left answers kept

Every question asked of a line holding right-to-left text (`x_for_index`,
`index_for_x`, `closest_index_for_x`, `selection_ranges`,
`index_left_of`, `index_right_of`, a wrapped line's `position_for_index`,
`index_for_position` and `row_starts`) walked its glyphs, sorted its cells
and worked out its carets again, and the carets' edges were kept sorted by
inserting each where it goes: for right-to-left text, whose edges come
right to left as its text goes on, each at the front. A long unwrapped
right-to-left line of n cells (a text preview with wrapping off) cost
about n²/2 moves per hover, caret or selection asked.

- **Kept.** A line works its row out at the first question and keeps it
  (`LineLayout::bidi_row`, a `KeptBidiRow`, made empty with the line:
  every platform's text system and gpui's own make a line with
  `Default::default()` there), as the line is kept from frame to frame in
  the layout cache. A wrapped line keeps its rows and where each starts
  (`WrappedLineLayout`'s, beside its narrowest width, 0061). A line all
  left to right keeps nothing, as before.
- **Sorted once.** The edges the carets take are gathered and sorted once,
  those the places left open settle kept beside them, as each is settled
  (0069's order of settling kept): a row costs a sort.
- **Searched.** A row keeps its carets by where they stand too: the caret
  nearest a point (of several as near, the first in the text), and the one
  beside a place (the nearest edge that way, of its places the nearest in
  the text), are a binary search, where they were a walk of every caret.

The tests are `text_system::bidi::tests::a_line_s_answers_are_worked_out_once`
(one row kept through every question, a wrapped line's rows too, none for
a line all left to right) and
`a_row_s_carets_are_found_as_a_walk_of_them_finds_them` (the nearest and
the beside answers equal to a walk of every caret, on Hebrew, mixed,
Arabic with digits and two paragraphs, every 2.5 px and every place);
0069's tests of carets, hit tests and selections answer as they did. Not
measured.

### 0088: The scroll handle's direction

A handle kept the direction its element was last laid out in
(`ScrollHandleState::direction`, 0075), which decides how its offset is
held sideways (from 0 down to `-max_offset`, right to left from 0 up), but
said it to nobody: what moves a box by its handle (Skwir's
`elements::pan::Moves`, a drag that moves a picture or pages) read the
direction where it was itself drawn, wrong for a box that says its own.
`ScrollHandle::direction` says it.

The tests are `elements::div::scroll_axis_tests::a_strip_left_to_right_scrolls_as_it_did`
and `a_strip_right_to_left_starts_at_its_right`, a strip laying itself out
right to left in a window laid out left to right.

### 0089: A window's tag

A compositor's or window manager's rules match a window by its program
(the app id, `WM_CLASS`) and its title, which tell apart neither two
windows of one program serving different ends (a program's main window
and the dialog it shows for another program, ADR-141) nor anything
whose title the window does not choose (the dialog's title is the asking
program's, in its language). `WindowOptions::tag` names what the window
is for (`WindowTag`: the tag, untranslated, and what it means in the
user's language):

- **Wayland**: xdg-toplevel-tag-v1 (staging, `xdg_toplevel_tag_manager_v1`,
  bound only where the compositor offers it): `set_toplevel_tag` and
  `set_toplevel_description` are sent once the surface is a toplevel,
  before its first commit, as the protocol asks. A compositor without
  the protocol is told nothing.
- **X11**: `WM_WINDOW_ROLE` (STRING), the role the session management
  spec has a window say, which window managers' rules match (i3's
  `window_role`, Openbox's `role`, KWin's window role); X11 has no place
  for the description. STRING is Latin-1, and window managers read it so:
  a tag beyond ASCII is no role (a warning is logged), where its UTF-8
  bytes would read as another name, and the role said before is taken
  back (the property deleted), so that rules naming the old purpose no
  longer match. `WindowTag::tag` says a tag is ASCII, as configuration
  files name it.

**Said again.** `Window::set_tag` says what the window is for once more:
its description in another language (an application whose language
changes while it runs), or a purpose changed, which the protocol lets a
client say at any time after the first commit. The window keeps what it
was last told (`Window::tag`) and tells the platform only what differs,
so it may be asked each frame: Wayland sends both requests again, X11
writes the role again (`PlatformWindow::set_tag`, which other platforms
leave as nothing).

Windows and macOS leave the field unread. Skwir tags its own windows
`main window` and those answering another program's Open or Save dialog
`file chooser` (`finishing::window_tag`, from the core's window), and
says the description again in the language chosen
(`finishing::tag_again`, asked as the title is said).

Checked in a window, as recorded in Skwir's STATUS-LOG; no test, as
gpui's tests have no display server to answer.

### 0090: An image in pieces

The atlas makes a texture for an image at most as large as the device
allows (wgpu's `max_texture_dimension_2d`, 16 384 on Metal's and Direct3D
11's atlases); `paint_image` failed for a larger one, and nothing was
drawn. Skwir kept every picture under 4096 a side for it, so a tall
picture (2000 × 11 500) was held at 712 × 4096 and blurred when zoomed.
Now a larger frame is drawn in pieces (`assets::image_pieces`), each a
key of its own (`RenderImageParams::piece`, 0 for a frame drawn whole):

- **No seam.** A piece holds the row or column beside its own on each
  side another piece lies, and is drawn over its own pixels alone: its
  sprite covers what it holds, its content mask what it owns. A
  magnified edge is then filtered with its true neighbour, as the whole
  frame would be (0008 clamps sampling to what the piece holds, half a
  texel in).
- **No gap, nothing twice.** The edges of what each piece owns are
  rounded to whole device pixels from one map of the frame onto the
  screen, so two pieces meet on the same edge and every pixel's centre
  lies in one mask.
- **Corners.** The frame's corner radii round only the pieces that hold
  its corners.
- **Uploaded as seen.** A piece outside the clip is neither copied nor
  uploaded; `drop_image` removes every piece, `has_image_atlas_entry`
  finds a frame by any of them.

`PlatformAtlas::max_texture_side` says the atlas's limit: wgpu's the
device's, 16 384 by default (Metal, Direct3D 11, the headless atlas). A
frame within it is drawn as before, as one sprite.

The tests are `assets::tests::a_frame_larger_than_a_texture_is_cut_in_pieces`
(a frame that fits is one piece; one of 10 × 7 in textures of 5 is twelve,
each holding at most 5 a side, their own pixels tiling the frame once)
and `window::tests::an_image_wider_than_a_texture_is_drawn_in_pieces` (a
20 000 × 2 frame in the headless atlas drawn as two sprites, the first
holding 16 383 columns and the second the rest and one, meeting on the
device pixel nearest the seam, dropped with both). Metal's and Direct3D
11's draws are compiled only.
