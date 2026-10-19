# ADR-068: Full preview reads one file past the bounds that keep a preview quick

Status: accepted, 2026-10-19. Built and checked on Linux; Windows and
macOS compiled.

## Context

- Every preview reads within bounds (ADR-009, `skwir_platform::inspection::
  limits`). Some keep hostile input out: nesting, a line, a cell, what a
  container's member inflates to, a decoder's allocation, a model's
  triangles. Others only keep a preview quick: 2 MiB of Markdown parsed,
  16 MiB of a font read, 2,000 blocks laid out, 64 columns, 4,000 archive
  members, 64 megapixels decoded.
- Past a quick bound a preview showed less and said so ("First 2000
  blocks", "Only the first 64 columns shown"), showed the file's text
  ("Shown as written: Text exceeds 2 MiB preview limit"), or nothing ("Font
  exceeds 16 MiB preview limit"); a DXF past 32 MiB fell back to its stored
  picture without a word. The user had no way to see the rest.
- ADR-067 gave 3D models the first way past one: a mesh over four million
  triangles is shown as a picture, and 3D turns that file, unremembered
  (`InspectRequest::whole`).
- The user, 2026-10-19: "we can generalize to files not previewed because
  of size: no preview or simple preview by default, and full preview when
  clicking a button; though in this case, not remembered, preview size
  limit can be set in settings, maybe?"

## Decision

1. **Two tiers of bounds.** Each quick bound has a larger one beside it in
   `limits`: text read whole 32 MiB, a file read whole 128 MiB, 100,000
   blocks and code lines, 64,000 archive members, 256 columns, 128
   megapixels, 128 MiB of a drawing. They are what a worker can still hold
   of one file the user asked for, a few hundred megabytes at most, and
   what the view still draws smoothly (a table row lays out every column).
   Bounds that keep hostile input out have one tier.
2. **Full preview asks for the larger ones, for the file shown.** The bar's
   Full preview (`ui.inspect` `whole`, a palette command) sets
   `InspectRequest::whole` and asks again. It is the panel's request, gone
   with the file; never remembered for a type or across a restart. A
   thumbnail, an agent's preview and a picture read ahead stay quick.
3. **Offered exactly where it shows more.** A provider takes its bounds
   from the request (`limits::Bounds::of`) and reads every quick bound
   through them. What it cut by them sets `Inspection::whole_available`;
   what it refused by them it refuses with `Bounds::refuse`, an
   `inspection::TooLarge`. The panel offers Full preview for those and no
   other: a text file refused is shown as written with the button, any
   other file says why with the button (`PreviewContent::TooLarge`), and a
   file past the larger bound says so with none.
4. **No setting.** The bounds are not the user's to set: one number would
   cut across bounds each preview chose for its own cost, and a setting
   raised once makes every preview slow for the sake of one file. The
   button reaches the rare large file where it is.

## Consequences

- Covered: Markdown, HTML and notebooks (text, blocks, output lines),
  structured data (text, lines, values), CSV, TSV, JSON Lines,
  spreadsheets and SQLite (columns), playlists (text, tracks), messages
  (the file, blocks), EPUB chapters (blocks), documents read by Pandoc
  (text, blocks), fonts, GeoJSON and flight plans, archives (members
  listed, a member written out), circuit files and DXF drawings, pictures
  and SVG drawings the panel decodes. Word and OpenDocument files are read
  a part at a time already and keep their bounds.
- `PreviewProvider::extract` takes the member's `InspectRequest`: it bounds
  what is written out. A container finds the member within Full preview's
  bounds (an archive in any listing kept of it, `archive::member`), as the
  row chosen may have been listed so.
- An archive's listings are kept by the bounds they were read within; one
  that lists every member serves both. Full preview's is kept as the quick
  ones are (four listings at most): one of 64,000 members holds about
  11 MB until four others were read.
- The helper's output cap (`HELPER_OUTPUT`) follows Full preview's listing,
  the largest.
- A model keeps its own way past its bound: 3D asks for the whole file.
- Checked: the providers' tests read past each quick bound and are refused
  past the larger one. In a window on Linux (headless sway, a debug build):
  a changelog of 3,000 items, a 2 MiB HTML page, a CSV of 100 columns, JSON
  of 30,000 values, a picture of 67 megapixels, an archive of 4,500 members
  (one past the 4,000th opened) and a 20 MB member of an archive were each
  shown whole by Full preview, and quick again when selected anew; the
  memory each took came back once another file was shown. Not checked in a
  window: fonts, messages, books, notebooks, playlists, maps, circuits and
  DXF drawings (their tests only); Windows and macOS (compiled). The button
  is named for a screen reader as every bar button is; not checked over
  AT-SPI.
