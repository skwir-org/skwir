# ADR-067: A 3D model is turned in the window, drawn by its GPU

Status: accepted, 2026-10-19. Built and checked on Linux (wgpu); Direct3D
11's pass checked under Wine, Metal's compiled only.

## Context

- `skwir-model-worker` reads STL, OBJ, PLY, OFF, glTF and 3MF meshes in a
  process of its own and draws one still on the CPU: 35° right of the
  front, 28° above, without perspective, two lights. The preview shows
  that picture. A model cannot be turned.
- ADR-064 moved Skwir to Zed's gpui partly for this: wgpu on Linux, Metal
  on macOS, Direct3D 11 on Windows. gpui has no hook for drawing of one's
  own: its primitives are fixed, and the one that shows an outside image
  (`PaintSurface`) exists only on macOS.
- Frames drawn on the CPU by a helper (proposed 2026-10-17) measured 12 ms
  a frame at 800 px for 225k triangles on 20 cores, 22 ms on 4, 58 ms for
  a million on 4. Dragging would keep every core busy, a model over a
  million triangles would need a simplified copy while it moves, and a
  screen at scale 2 costs four times the pixels.
- The previews' rule was that nothing is drawn in 3D in-process. What
  that rule protects is the window from the file: parsing stays in the
  helper.

## Decision

1. **The window draws a mesh on its GPU.** Vendored patch 0033:
   - `gpui::Mesh` holds positions and the triangles between them, checked
     when it is made (every index names a position, every coordinate is
     finite). `Window::paint_mesh` draws it in bounds, seen through a
     `MeshView`: the model-view and projection matrices, the surface's
     colour, the ambient light and two lights fixed to the camera.
   - wgpu draws it in a pass of its own, as gpui draws paths: offscreen,
     multisampled, with a depth buffer the size of what is shown, then
     composited in draw order and clipped like any primitive. Direct3D 11
     and Metal draw it alike, from naga's translations of the same WGSL,
     kept beside their renderers and current by a test; the frame's
     geometry and the shaders' parameters are gpui's, one for the three
     (`PaintMesh::framed`). The translations are compiled when the first
     mesh is asked for, so a build from Linux has them too.
   - Faces are shaded flat from the screen-space derivatives of their
     position, so no normals are sent, and lit on either side: a mesh's
     winding is not always kept.
   - A mesh may carry a colour for each position (`Mesh::with_colors`,
     sRGB bytes), drawn in place of the view's and blended across a face
     in linear light.
   - Smooth shading (`MeshView::smooth`) asks for normals, which the
     renderer makes on its upload thread when a frame first asks for them
     (each position's faces' normals, weighted by their areas): the mesh is
     drawn flat meanwhile (`MeshPreparation::Flat`), then again. Flat
     shading makes none.
   - A mesh's buffers are made and filled on a thread of the renderer's
     own, never the window's: `Window::paint_mesh` draws a mesh once it is
     there, starts its upload otherwise, and draws the window again when it
     arrives (`gpui::MeshStore`, every renderer's). The buffers are let go
     of in the first frame that does not draw the mesh; the target goes
     when no mesh is drawn. The pipelines are made when the first mesh is
     drawn or asked for, never at start. The thread ends with the
     renderer.
   - `Window::draws_meshes` says whether the window draws them: every
     window on Linux, Windows and macOS does. `paint_mesh` says whether it
     painted the mesh, is loading it, or will not (`MeshPainted`): a mesh
     larger than the GPU holds, or one whose buffers it could not make,
     says so in the preview.
2. **The file is still read in the helper.** `skwir-model-worker --mesh`
   writes what it measures, then the positions and triangles as
   little-endian numbers, then a colour a position when the file gives
   any: PLY's point colours, MeshLab's in OBJ, a glTF material's base
   colour times its points' (`COLOR_0`), 3MF's base materials and colour
   groups (a point split where its triangles disagree). A part without
   one beside parts with one is the still's grey. The still and the
   thumbnails are drawn in them too. Textures are not read: an image
   decoder in the helper, coordinates, a texture and a sampler for what
   the base colours mostly say already. Skwir checks the counts against what it asked,
   every index against the positions and every coordinate before anything
   reaches the GPU (`inspection::Mesh::new`, then `gpui::Mesh::new`). The
   rule becomes: nothing is read in-process; the window draws what the
   helper read.
3. **The preview asks for what the window can draw.** Where it draws
   meshes on a GPU, the preview asks for the mesh
   (`InspectRequest::meshes`); elsewhere for the still, as before.
   - The user chooses with the bar's 3D button (`ui.inspect` `turn`),
     remembered for the file's type with the window's state
     (`TurnedTypes`), as Source is. Unchosen, a mesh is turned and a model
     that stores a picture (a slicer's 3MF) shows it.
   - A mesh over `model::VIEW_TRIANGLES` (four million) shows its still,
     whatever its type's choice, and says why. The 3D button turns that
     file, up to `model::TRIANGLES` (`InspectRequest::whole`), and that is
     never remembered: the user asked for this file, not for every large
     one.
   - A window drawn by a software rasterizer (lavapipe, where there is no
     GPU) gets the still: its frame grows with the mesh, and the window's
     thread waits for it. Decided by what the adapter is
     (`GpuSpecs::is_software_emulated`), not by a measured threshold.
   - Thumbnails stay stills. An agent's or a script's preview asks for the
     still.
4. **The view opens as the still is drawn:** the same angle, no
   perspective, the same framing. The thumbnail that stands in while the
   mesh is read gives way to it without a jump.
   - Two settings change the look (Reading, 3D models; bar buttons, the
     first to go to More): Perspective (`reading.model_perspective`), the
     camera five times the model's radius from its middle, drawn at the
     still's scale there; Smooth shading (`reading.model_smooth`). Both
     off by default, as the still is drawn.
   - Drag turns the model around its up axis and tilts it; Shift+drag or
     the middle button moves it; the wheel zooms at the pointer.
   - Once clicked, the arrows turn it, Shift and the arrows move it, the
     zoom keys zoom, Ctrl+0 (Fit) shows it as it opened.
   - Nothing is drawn unless asked: each input asks for one frame.
5. **Bounds.** A mesh of up to `model::VIEW_TRIANGLES` triangles is
   turned unasked; up to `model::TRIANGLES` when asked (3).

## Alternatives

- **Frames drawn on the CPU by a helper** (2026-10-17): no GPU code of
  ours, but every core busy while dragging, a simplified copy above a
  million triangles, and four times the work at scale 2.
- **wgpu in a helper:** 4 to 8 MB more, a GPU device per helper (100 to
  300 ms and about 100 MB on NVIDIA), and the frames copied back to the
  window anyway.
- **A viewer of its own (f3d, VTK):** over 100 MB, and its window cannot
  be embedded.
- **Parsing in the window:** a crash or a crafted file in a format reader
  would reach the window.

## Consequences

- **The pass, measured** (`mesh_frame_times`, the headless renderer,
  release; 1000 × 800 pixels at four samples each; two tori turning, of
  225,600, a million and four million triangles; 2026-10-19):

  | Adapter | A frame (median) | The upload |
  | --- | --- | --- |
  | NVIDIA GTX 1060 (Vulkan) | 0.72, 1.08, 2.47 ms | 50 ms, 0.23 s, 0.94 s |
  | Intel Iris Xe, Alder Lake (Vulkan) | 2.23, 2.62, 4.98 ms | 1, 6, 31 ms |
  | lavapipe, 20 threads | 84 ms, 0.56 s, 2.6 s | 2, 15, 50 ms |

  - lavapipe without multisampling still took 80 ms, 0.16 s and 0.52 s:
    hence the still for a software rasterizer (decision 3).
  - NVIDIA's driver maps the memory uploads go through uncached: 72 MB
    took 0.49 s to allocate and 0.46 s to fill (158 MB/s, no page fault),
    each way wgpu offers. Hence the uploads off the window's thread
    (decision 1). The Intel GPU filled the same in 28 ms.
- **The window, measured** (release; headless sway rendering on the Intel
  GPU; the app on its Vulkan driver; the panel 320 pixels wide):
  - a model shows 0.19, 0.49 and 2.0 s after the click; the helper alone
    takes 0.07, 0.37 and 1.47 s to read them;
  - the window's own share grows by its copy of the mesh: 5, 19 and
    72 MB, given back with the preview. The GPU's copy is the driver's;
  - turning the largest for three seconds drew a frame for each of the
    101 pointer moves, gpui's frame time 4.7 ms (median), 8.6 ms (90th
    percentile), at 23% of a core; at rest, not a tick in five seconds.
- **The helper reads faster** (measured alone, release, the same tori):
  2.26 s for four million triangles became 0.48 s, a million 0.6 s
  became 0.13 s. Most of it was hashing: `stl_io` makes an STL's same
  corners one through std's `SipHash`, and the check that a mesh is
  closed made them one again and sorted twelve million edges on one
  thread. A binary STL is now read from its bytes, its corners made one
  through a cheap hash on as many threads as there are cores, each corner
  going to the thread its hash names; the check trusts a reader that made
  them one already, and sorts the edges in groups, a thread each. Taking
  the helper's output back and checking it costs the window about 0.1 s
  at four million triangles.
- **The window keeps the mesh while it shows it,** besides the GPU's
  copy, so a device lost and found again uploads it anew. Giving it back
  once uploaded would halve a large model's cost to the window.
- **Checked** on Wayland: drawn, turned by a drag and the arrows, moved
  by Shift and the arrows, zoomed by the wheel, shown as it opened by
  Ctrl+0; on X11 (Xwayland): drawn and turned by a drag; on lavapipe, the
  still. **Not checked yet:** the window on NVIDIA (the user's session),
  a screen reader.
- **Direct3D 11 and Metal** (2026-10-19): Direct3D's pass drew two
  squares in depth, in their colours, composited where their bounds are,
  under Wine on X, its d3d11 on Mesa's llvmpipe through OpenGL and through
  Vulkan (`just gpui-check-windows`). The test through gpui's renderer,
  a quad under the mesh and one over it, skips under Wine, whose HLSL
  compiler cannot compile gpui's own shaders; it is for a Windows machine,
  as is any measure. Metal's pass is compiled only. On Windows, WARP gets
  the still, as lavapipe does; macOS reports no adapter, and turns.
- **The Wine run found a fault in the shared store:** its upload thread,
  left to end alone, let go of the device while the process exited, and
  Wine killed the process about four times in ten; elsewhere it went
  unseen. The thread now ends with the renderer, which waits for the
  upload under way.
