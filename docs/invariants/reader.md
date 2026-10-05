# Document reader invariants

Read this before touching `crates/pdf-worker`, `crates/poppler-worker`, `crates/mupdf-worker`, `crates/platform/src/document.rs`, `crates/ui/src/reader*.rs` or `crates/ui/src/elements/document.rs`. It is binding, like `AGENTS.md`.

- Native PDF libraries load only in `bibli-pdf-worker` / `bibli-poppler-worker`
  / `bibli-mupdf-worker`, never in the UI process. Every engine uses the same
  protocol/session manager, and `tools/pdf/check.py` runs the same fixtures on
  each. MuPDF is chosen explicitly, never by `auto` (ADR-014). A session
  whose pages are drawn by our own Rust code (a fixed-layout book's, a
  comic's pictures) runs in process on the scheduler, like an inspection,
  with the same contract: bounded, cancellable, one owned pixel buffer.
  Session creation/render/search are blocking worker entry points. The manager
  thread owns child cleanup and temporary files; dropping a UI session must
  only cancel/send, never wait for a process or delete files.
- Protocol lines are `Tagged`: the greeting is reply 0 and request `n` gets
  reply `n`, once, in order (`bibli_core::document::worker::serve`, shared by
  both workers). A cancelled request sends `Cancel` and its answer is drained
  later; a worker is restarted only when it stays busy past a short grace,
  times out or breaks the protocol, never for a cancel or a page's `Error`.
  A page whose text cannot be carried is shown without selectable text.
- A session follows both the selected preview and its provider registration.
  Page/search requests have separate cancellation and generation checks.
  Cancel background search before a visible page request. Disabling a provider
  must cancel even an idle session; test this without requiring native libraries.
- Document pages transfer one owned pixel buffer into GPUI; swizzle it in
  place. Full-page copies on scheduler threads can retain large allocations
  across allocator arenas even after the preview is closed.
- `RenderImage` reference counts do not release uploaded GPUI atlas entries.
  Explicitly call `cx.drop_image(image, Some(window))` when replacing/closing
  a preview, as the thumbnail LRU does on eviction.
- Continuous reading is how a document opens, unless the reader's own button
  was last used to say otherwise: the choice is the user's and outlives the
  document, so it is the `reading.continuous` preference, written through
  `settings.apply` like any other (queued while another save runs), and
  read when a document opens, never on a later page's arrival. A document
  whose file changes is opened again where it was read (`reader_reopen`):
  same page, zoom and mode.
- Continuous reading virtualizes pages with a logical page/offset anchor. Size
  changes and fit/zoom are resolved in prepaint. They change every row's
  height at once, so the rows are reset to a new uniform estimate and the
  anchor is restored in the same prepaint (`continuous_element`): the old
  heights would only mislead the scrollbar. A page that arrives invalidates
  its own measured height (`ListState::remeasure_items`) without resetting
  that anchor and without splicing: a spliced row forgets its height, and a
  document that loses a page every time one finishes rendering has a
  scrollbar that jumps while it is being dragged. A page that arrives at the size its placeholder assumed needs
  no invalidation at all. Render only demanded pages and neighbours,
  serialize requests, reject cancelled generations, and evict offscreen pixel
  buffers and their atlas entries together. Page interactions carry a page
  number; never apply an adjacent page's selection or links to the opening page.
  Overdraw measurement is not stable between frames: keep prefetch neighbours
  explicit so a later layout does not cancel the work it just requested.
- PDF pixel, text, link and search rectangles share normalized coordinates.
  Fit/zoom transforms belong in `elements/document.rs` prepaint. Verify rotated
  pages with `tools/pdf/check.py` for every engine; use only its synthetic
  fixture generator. Poppler text layout has a top-left origin, whereas its
  search/link rectangles have a bottom-left origin; normalize at the worker.
- A reflowable document (a book through MuPDF, `DocumentInfo::reflowable`)
  is laid out by its worker at the preview's text size; its size keys lay it
  out again instead of zooming (`Shell::preview_zoom`, `relayout`). A new
  layout has other pages: the reader lands through `reader::Anchor` and
  `DocumentSession::locate`, never by the old page number, and accepts the
  located page only if the reader has not moved since. Anything that asks a
  worker where text lies after a layout goes through `Locate`; a fixed-page
  engine answers `Located(None)`. A document's missing fonts come from
  fontconfig inside the worker (ADR-014 addendum), never from the UI process.
- A page's size (`PageContent::width`/`height`) is its own, in CSS pixels
  (1/96 in; a PDF's points times `CSS_PER_POINT`): what 100% zoom shows and
  what layout uses, whatever its pixels. Pages are rendered at the side
  they cover on screen, never at a fixed one: each page element reports
  what it covers in prepaint (`elements::document::Shown`), the shell keeps
  one side in 512 px steps up to `MAX_SIDE` (`reader_scroll::page_side`,
  `Shell::document_shown`, kept from one document to the next), new pages
  are rendered at it and shown ones asked at less are rendered again,
  replacing their pixels in place. A smaller side redraws nothing: the next
  pages come smaller. A cached page remembers the side it was asked at, so
  an engine that has no more pixels (a small picture) is not asked again.
- A document that names its pages otherwise than by their number (a PDF's
  `/PageLabels`, a fixed-layout book's page list) says so once, when it
  opens (`DocumentInfo::labels`, filtered by `document::page_labels`, the
  same for every engine: `tools/pdf/check.py` asserts it). Counted by
  printed pages (`reading.printed_pages`, set by `ui.printed_pages`, the count's picker shared with books,
  `preview_bar::unit_popup`), the page field shows and takes those names;
  previous and next still turn the file's pages. PDFium reads labels
  without loading pages (`FPDF_GetPageLabel`); `MuPDF` reads them through
  its PDF document.
- `auto` may fall back when the worker/library is absent: the worker not
  found where it can run (`process::Worker`), saying `Unavailable`, or
  exiting 127 before its greeting, the dynamic loader's word for a missing
  library (`LineChannel::missing_runtime`, as the player's). Report document
  errors directly; do not conceal malformed files by silently trying engines.
  Poppler provides the same reader features. CLI image fallback is for `auto`
  only when neither reader worker is available. Measurements must distinguish
  renderer time from full UI latency and include child PSS, not only bibli PSS.
