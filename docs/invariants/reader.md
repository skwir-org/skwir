# Document reader invariants

Read this before touching `crates/pdf-worker`, `crates/poppler-worker`,
`crates/mupdf-worker`, `crates/djvu-worker`, `crates/djvulibre-worker`,
`crates/platform/src/document.rs`, `crates/ui/src/reader*.rs` or
`crates/ui/src/elements/document.rs`. It is binding, like `AGENTS.md`.
Measurements: `docs/pdf-comparison.md`, ADR-061 for `DjVu`.

## Processes and isolation

- Native PDF libraries load only in `skwir-pdf-worker`,
  `skwir-poppler-worker` and `skwir-mupdf-worker`, never in the UI process;
  DjVuLibre only in `skwir-djvulibre-worker`. djvu-rs, though Rust, is
  young: it reads documents in `skwir-djvu-worker`, isolated like the
  others (ADR-061). All share one protocol and session manager;
  `tools/pdf/check.py` runs the same fixtures on each PDF engine,
  `tools/djvu/check.py` on each DjVu engine.
- Pages our own Rust code draws (a fixed-layout book, a comic) run in
  process on the scheduler, like an inspection, under the same contract:
  bounded, cancellable, one owned pixel buffer.
- Session creation, render and search are blocking worker entry points. The
  manager thread owns child cleanup and temporary files; dropping a UI
  session only cancels or sends, never waits or deletes.
- Documents are untrusted: every worker runs isolated where the system
  allows (`sandbox::untrusted_reading`, ADR-041). It can read the document
  at `/input`, its scratch folder at `/work`, its own folder (where a
  package carries PDFium) and the library the settings name; nothing else,
  no network. Without isolation
  (Windows, Linux without bubblewrap) it runs as is (`Otherwise::Run`). The
  manager keeps the real scratch path; the worker gets the inside paths.
- A test of a stand-in worker that watches its process asks for no
  isolation (`open_with_worker(.., false)`): isolation renumbers processes.

## Engines and fallback

- MuPDF is used only when chosen, never by `auto` (ADR-014).
- A PDF page drawn elsewhere (a thumbnail, a converted office document's
  first page) uses an engine in `auto`'s order (`document::first_page`,
  `document::AUTO`), and `pdftoppm` only when none is installed, as on
  macOS, Windows and a minimal Linux (ADR-041).
- PDFium's library is the one the settings name (`document::PdfiumChoice`,
  shared by reader, thumbnails and probe), else the one Skwir's package
  carries beside the helper (or in a Mac bundle's `Frameworks`), else the
  system's. Every helper answers `--probe` (`process::probe`).
- `auto` falls back only when a worker or library is absent: the worker is
  not found (`process::Worker`), says `Unavailable`, or exits 127 before its
  greeting (the loader's missing-library code,
  `LineChannel::missing_runtime`).
- Report document errors directly; never hide a malformed file by silently
  trying other engines.
- DjVu's `auto` is DjVuLibre, then djvu-rs (`document::DJVU_AUTO`), under
  the same rule: the next only when one is absent. A DjVu thumbnail takes
  that order whatever the reader's.
- Poppler offers the same reader features as PDFium. The CLI image fallback
  is for `auto` only, when neither of its workers is available.

## Protocol and sessions

- Lines are `Tagged`: the greeting is reply 0; request `n` gets reply `n`,
  once, in order (`skwir_core::document::worker::serve`).
- A cancelled request sends `Cancel`; its answer is drained later. Restart a
  worker only when it stays busy past a short grace, times out or breaks the
  protocol, never for a cancel or a page's `Error`.
- A page whose text cannot be carried is shown without selectable text.
- A session follows both the selected preview and its provider registration.
  Disabling a provider cancels even an idle session; test this without
  native libraries.
- Page and search requests have separate cancellation and generation checks.
  Cancel background search before a visible page request.

## Pixels and the atlas

- Pages move into GPUI as one owned pixel buffer, swizzled in place:
  full-page copies on scheduler threads can keep large allocations in
  allocator arenas after the preview closes.
- `RenderImage` reference counts do not free atlas entries. Call
  `cx.drop_image(image, Some(window))` when replacing or closing a preview,
  as the thumbnail LRU does on eviction.

## Continuous reading

- Documents open in continuous mode unless the reader's button last said
  otherwise. The choice is `reading.continuous`, written through
  `settings.apply` (queued behind a running save) and read when a document
  opens, never when a later page arrives.
- A document whose file changes reopens at the same page, zoom and mode
  (`reader_reopen`).
- Pages are virtualized around a logical page/offset anchor. Size and
  fit/zoom changes are resolved in prepaint: they change every row's height,
  so rows reset to a new uniform estimate (old heights would mislead the
  scrollbar) and the anchor is restored in the same prepaint
  (`continuous_element`).
- An arriving page invalidates only its own height
  (`ListState::remeasure_items`), without resetting the anchor or splicing:
  a spliced row forgets its height, and the scrollbar jumps while dragged.
  A page arriving at its placeholder's size needs no invalidation.
- Render only demanded pages and neighbours, serialize requests, reject
  cancelled generations, and evict offscreen pixel buffers with their atlas
  entries.
- Page interactions carry a page number; never apply an adjacent page's
  selection or links to the opening page.
- A page zoomed wider than the panel scrolls sideways, with its bar along
  the bottom, read continuously or a page at a time; a page at a time has
  its vertical bar too, for the page shown (the wheel past its edge turns
  the page, `reader_wheel`).
- Overdraw measurement varies between frames: keep prefetch neighbours
  explicit so a later layout does not cancel work it just requested.

## Coordinates and page size

- Pixel, text, link and search rectangles share normalized coordinates.
  Fit/zoom transforms belong in `elements/document.rs` prepaint.
- Poppler's text layout has a top-left origin, its search and link
  rectangles a bottom-left one; normalize in the worker.
- DjVu's text is in the page's pixels as stored, its links' areas in the
  page's as shown, both from the bottom left: a page the file turns is
  drawn turned, and its words turned with it (`Rect::turned`), by both
  engines. `tools/djvu/check.py` measures where each engine's ink lands
  against the text that says where it is.
- An engine that knows words, not glyphs (DjVu's, a plugin from outside
  Skwir's pages) makes them characters and searches them with
  `document::words`, so selection, copy and search agree across engines.
  Words lying over one another are one run over the box around them: a
  text layer that places its lines and not their words gives each word
  its line's box, and characters covering each other darkened a
  selection with every word it took.
- djvu-rs draws a page larger than it is at a size whose step through it
  is no whole number of sixteenths of a pixel: its helper draws at the
  largest size below the side asked that it draws in place
  (`exact_size`), never at the side asked regardless.
- Verify rotated pages with `tools/pdf/check.py` for every engine, using
  only its synthetic fixtures.
- A page's size (`PageContent::width`/`height`) is in CSS pixels (1/96 in;
  PDF points times `CSS_PER_POINT`): what 100% zoom shows and layout uses,
  whatever its pixels.
- Pages render at the side they cover on screen, never a fixed one. Page
  elements report their coverage in prepaint (`elements::document::Shown`);
  the shell keeps one side in 512 px steps up to `MAX_SIDE`
  (`reader_scroll::page_side`, `Shell::document_shown`), across documents.
  New pages render at that side; shown pages rendered smaller render again
  in place. A smaller side redraws nothing; later pages come smaller.
- A cached page remembers the side it was asked at, so an engine with no
  more pixels (a small picture) is not asked again.

## Reflowable documents

- A reflowable document (a MuPDF book, `DocumentInfo::reflowable`) is laid
  out by its worker at the preview's text size; size keys relayout instead
  of zooming (`Shell::preview_zoom`, `relayout`).
- After a relayout the reader lands through `reader::Anchor` and
  `DocumentSession::locate`, never the old page number, and takes the
  located page only if it has not moved since.
- Asking a worker where text lies after a layout goes through `Locate`;
  fixed-page engines answer `Located(None)`.
- Missing fonts come from fontconfig inside the worker (ADR-014), never
  from the UI process.

## Page labels

- A document that names its pages (a PDF's `/PageLabels`, a book's page
  list) says so once, on open (`DocumentInfo::labels`, filtered by
  `document::page_labels` for every engine; `tools/pdf/check.py` asserts
  it).
- Counting by printed pages (`reading.printed_pages`, set by
  `ui.printed_pages` from `preview_bar::unit_popup`, shared with books), the
  page field shows and accepts those names; Previous and Next still turn the
  file's pages.
- PDFium reads labels without loading pages (`FPDF_GetPageLabel`); MuPDF
  through its PDF document.

## Measuring

- Separate renderer time from full UI latency, and include child PSS, not
  only Skwir's.
