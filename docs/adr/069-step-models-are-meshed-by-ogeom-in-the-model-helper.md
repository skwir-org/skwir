# ADR-069: STEP models are meshed by ogeom in the model helper

Status: accepted, 2026-10-19. Built and checked on Linux; Windows and
macOS compiled. Amended the same day: IGES files are read alike.

## Context

- A STEP file (ISO 10303-21) holds a model's exact surfaces and the edges
  that trim them, never triangles, so showing one means meshing it with a
  geometry kernel. Skwir left STEP to an installed thumbnailer (f3d). On
  the reference machine nothing drew one: f3d was not installed, and
  nixpkgs' f3d names STEP `application/vnd.step`, a type the desktop's
  database does not know (it says `model/step`).
- The user asked, 2026-10-19, what supporting STEP would take, then
  whether Foxtrot would be "better than nothing". The candidates were
  scored on step-conformance (github.com/dan335/step-conformance: 27 CC0
  parts, each read at 0.0125, 0.05, 0.1 and 0.5 mm, against their exact
  shape or OpenCascade's mesh at 0.001 mm):
  - **OpenCascade**, what f3d, FreeCAD and KiCad use: the reference. C++,
    LGPL-2.1 with an exception; reading and meshing STEP loads 23 of its
    libraries, 55 MB. It would be a helper of its own, packaged for each
    system.
  - **ogeom** 0.9.16 (pure Rust, MIT or Apache-2.0, released 2026-10-09):
    89 of 108 runs, every real part.
  - **Foxtrot** (Formlabs, unchanged since 2021, AP214 only, no units): 68
    of 108. It drops faces of revolution, domes and tori without a word:
    a bowl lost its dish, a panel drew nothing.
  - **truck**: 91 of 108 on its unreleased main branch (the suite's
    published results), 81 for its release from 2024; it ignores the
    file's unit.
- On NIST's public-domain test files (17 AP242 and 11 AP203, 7 KB to
  6 MB, from several CAD programs), ogeom read 27 of 28 with no face
  missing, in 0.04 to 0.8 s and 20 to 140 MB, picture drawn (release
  build). The 28th holds only triangles (AP242's tessellated geometry).

## Decision

1. **ogeom meshes STEP in `skwir-model-worker`** (`--format step`): every
   part where the file's product tree places it, then any body the tree
   does not reach; each face to a chord of a thousandth of the model's
   diagonal and a turn of 20°; lengths in the file's unit, colours from
   its styles (`COLOUR_RGB` on a solid, a shell or a face). The helper
   stays pure Rust and goes on every system with the others. It grows
   from 1.5 to 3.6 MB stripped, 0.6 to 1.3 MB compressed. ogeom brings 19
   crates new to the workspace, all pure Rust.
2. **Each face keeps its own points.** The window's smooth shading
   averages normals at shared positions, so a welded CAD part would round
   its edges; kept apart, a curved face shades smooth and an edge stays
   sharp, as CAD shows it. Whether the model is watertight, and its
   volume, are measured on the mesh ogeom welds of the same faces: an
   exported file's faces meet within its tolerance, not bit for bit.
3. **A face that cannot be meshed is counted and said.** The helper's
   line says `faces` and `faces_missing`; Properties lists the faces, and
   the panel says "N of M faces could not be drawn", so a hole is never
   taken for the shape. A panic in the reader comes back as the reason.
4. **By its type, bounded as a file read whole.** The provider takes
   `model/step`, which the database reads from the name and the file's
   first line. Reading costs the helper ten to fifty times the file's
   size, so a STEP file's bound is `Bounds::binary_input` (ADR-068): 16
   MiB, refused for Full preview past it, 128 MiB for Full preview.
5. **The header is read by the plugin**, without the helper: the protocol
   (AP203, AP214, AP242) and the program that wrote the file.

## Consequences

- STEP is shown in the panel and the grid like a mesh, turned in 3D
  (ADR-067), measured, and in its colours. A desktop thumbnailer that
  claims the type (f3d) still comes first in the grid.
- ogeom's known gaps, from step-conformance: a B-spline face closed all
  round with no seam edge (a cylinder, a dome or a torus written so)
  comes back empty, which the panel says; a hemisphere on a disc lands up
  to 20 mm off; a cone to a point is 0.08 mm off at the finest chords. A
  file of triangles only is refused with ogeom's reason. Updates to ogeom
  are taken as any dependency's, re-scored on step-conformance.
- IGES, amended the same day: ogeom reads it too (`read_iges`), so an
  IGES file (`model/iges`) is meshed as a STEP file is, the global
  section giving the program and the IGES version. Twelve SolidWorks
  2014 exports (SparkFun's published models) read closed, no face
  missing, in 0.02 to 0.18 s. ogeom keeps only *independent* trimmed
  surfaces: a file that puts them in a group (a 402 entity, which makes
  them logically dependent, as two 1998 EUCLID-QUANTUM files in
  OpenCascade's test data do) is refused with ogeom's reason. Marking
  them independent read one of those two right and the other as shards,
  so the input is not rewritten: it is ogeom's to read them.
- OpenCascade stays the way to the files ogeom cannot read, as a helper
  of its own, if real files show the need.
- Checked: the helper's tests (a cylinder in millimetres and in inches,
  red with a blue top face: closed, its volume within 1%, every triangle
  in its face's colour; what is not STEP refused), the plugin's (the
  header, the details, the bound refused for Full preview), the
  step-conformance and NIST files above, their pictures looked at.
  `cargo deny` passes. Windows and macOS compiled.
