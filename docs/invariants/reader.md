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
  `settings.apply` (queued behind a running save) and read when the
  reader for a file is made, never when a later page arrives. Every file
  shown gets its reader from `Shell::let_reader_go` (another file,
  another member of an archive, another view of the file), in that mode
  from the start (`ReaderState::new`): what stands for the first page
  (its thumbnail) and the page itself are drawn that way, and the session
  that arrives only sizes the column (`reader_opened`), never switches.
- A page shown goes with its reader: `let_reader_go` takes it from the
  panel, since the next reader has no column for it. Left there, it was
  drawn again a page at a time while the next file loaded.
- Pages drawn without a reader (`pdftoppm`, where no worker is installed)
  are turned by asking for each one: their reader reads a page at a time,
  whatever the choice.
- Read a page at a time, the page shown is kept with the pages the
  reader keeps (64 MiB), and the next and previous pages are read ahead.
  A turn to a kept page shows it without rendering it; a turn to a page
  being read ahead waits for it; anything else read ahead gives way to
  the page asked. What the panel shows of a kept page stays in the atlas
  until the cache lets it go (`DocumentScroll::holds`).
- A document whose file changes reopens at the same page, zoom and mode
  (`reader_reopen`).
- The column is an item per page, built on the window's thread: every
  count the reader reads of a session is `reader::page_count`, the
  engine's held to `document::MAX_PAGES`, whichever engine or plugin
  says it.
- Pages are virtualized around a logical page/offset anchor. Size and
  fit/zoom changes are resolved in prepaint: they change every row's height,
  so rows reset to a new uniform estimate (old heights would mislead the
  scrollbar) and the anchor is restored in the same prepaint
  (`continuous_element`). The estimate is each row's height hint, which
  gpui keeps through the list's first layout and a width change (patch
  0040): dropped there, the column counted only the pages laid out, and
  the scrollbar ended at the last page visited. After Ctrl and the wheel
  the anchor is the page under the pointer and the share of it there
  (`ZoomAnchor`), landed under the pointer at the new rows' own heights
  (`landing`).
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
  the page, `reader_wheel`; so do Up and Down at its edge, `reader_arrow`,
  waiting while a turn is on its way, so a held key starts the next page
  at its top).
- **A drag moves the pages** (ADR-143, the rule in `previews.md`'s
  "One of each"): a page takes every left press on it, and decides by
  where it falls (`document::Page::selects_at`, asked by the element
  and `Shell::reader_pointer` alike). Within a line of its text (on a
  letter, between words, in the leading between the lines of a
  paragraph, `LEADING`, a little before its first letter and after its
  last, `BESIDE`) it selects from the line's nearest letter
  (`Grab::select`: the layer moves nothing and shows no hand
  meanwhile), unless `reader::drags` says the page moves (the user's
  `reading.drag_moves`, `ui.reader_drag`, read when the reader is made;
  or a page with no text). Anywhere else on the page (a margin, between
  paragraphs, a picture), and across its text where it drags, the page
  holds the `pan::Grab` itself (`Grab::hold`, even where nothing can
  move, for its click); it sends no move (nothing to select as it goes).
  A press's page is its own, apart from the selection's
  (`ReaderState::down`, `press`, `release`): only a press that selects
  takes the selection's page, so a drag that moves the pages keeps what
  is selected wherever it is, and a click where nothing selects (a
  margin, between paragraphs, text that drags) lets it go, on any page.
  A click is one test, the grab's, for every press on a page, selecting
  or holding the pages: one whose pointer never went past
  `pan::CLICK_SLOP` (`Grab::dragged`, which a selection's drag marks
  too); where on the page its release falls tells nothing, as the page
  may follow the pointer. It follows the link under the press, on the
  press's page (`ui.reader_link`'s `page`); a script's press, with no
  pointer, clicks where it is let go where it was made. A page's hitbox is the page drawn, not the letterbox
  around it: a press there is the layer's. While a press holds the
  pages, no page sets a cursor (the layer's closed hand shows); while
  one selects, the I-beam shows wherever it goes, over every page and
  the layer around them; else a link's is the pointing hand, a line's
  the I-beam, and the rest of the page the open hand where the pages
  can move. The cursor is asked for where the pointer is, at
  paint and at each move alike (`document::cursor_at`): gpui sets it
  only after a draw or as the pointer goes onto another hitbox, and a
  page is one hitbox, so a move that changes the answer (onto a link or
  text, off them) asks for a frame, once; a move that does not draws
  nothing.
- **A page's lines are found once**, as it arrives, on the worker that
  read it (`document::Page::new`, which the preview's content and the
  pages kept hold): a character goes on the line of the one read before
  it where they share more than half the lower one's height, and a line
  break the engine reads ends one. What a press, a selection's drag and
  the cursor ask (`selects_at`, `nearest`) is a search among the lines
  by their tops and across one line by its letters' starts, never a
  walk of the page's characters, of which a page holds up to
  `document::MAX_PAGE_CHARS`; `nearest` answers what that walk would.
- **A page's press is its own only while the grab holds it**
  (`Grab::held_over`). A page is a row of a virtual list: one the pages
  moved out from under the press (or the wheel took away) no longer
  hears its release, which only the layer, never virtual, does. What
  such a page hears later with no press of its own held (a release
  after a click on the toolbar) lets its press go (`ReaderState::down`),
  never a drag or a click on its link. It is told where the pointer's
  events come in (`reader::surface`), not in `ui.reader_pointer`, which
  a script may send with no pointer.
- **A line of the wheel moves pages `WHEEL_LINE` (40 px)**, read
  continuously or a page at a time, so a notch (three lines on Linux)
  goes 120 px, as Chromium's does there (its PDF viewer's). gpui's list
  counts a line 20 px and a `div` the window's line height (26 px): a
  notch went 60 and 78 px. Read continuously, the column takes the wheel
  before the list does (`wheel_steps`, drawn after the column: bubbling
  runs last drawn first, and what is drawn over the pages still comes
  before it). A touchpad's pixels, a wheel turned sideways and one with a
  modifier (Ctrl's zoom) are left as they come (`wheel_step`).
- **Only a wheel's notches are lines whole** (`notches`): gpui says
  lines for a touchpad too on X11 (all of XInput's smooth scrolling is
  lines) and through Windows' wheel messages, and for a high-resolution
  wheel's steps between notches, each a part of a line; a notch is
  `SCROLL_LINES` whole on Wayland, X11 and Windows. Lines in parts go as
  gpui counts them, and add up to a step only once they make a notch's
  lines, as a touchpad's pixels do at 60 (`Wheeled`, `WheelTurn`): no
  step is more than a notch's worth. Every wheel that counts steps (pages,
  Ctrl's zoom, the tab strip, a player's volume) reads its event through
  `Wheeled`, never by whether gpui said lines. gpui marks no source beyond that (its
  `ScrollSource` tells a touchscreen alone): a touchpad's step that comes
  as whole lines is taken as a notch.
- **The wheel turns a page a notch** a page at a time, past the page's
  edge: a page that fits (Fit) is at both its edges, so every notch turns
  it. The wheel and the arrows ask one question, `at_page_edge`, in
  numbers: gpui orders `Pixels` totally, -0 below 0, so the same test in
  `Pixels` never found a page that fits at its bottom edge, and the wheel
  turned such pages back but never on. A notch that comes while a turn
  is on its way is dropped (`WheelTurn`), so a trackpad's momentum never
  runs through the document.
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
  the shell keeps one side in 512 px steps up to `MAX_PART_SIDE`
  (`reader_scroll::page_side`, `Shell::document_shown`), across documents.
  New pages render at that side; shown pages rendered smaller render again
  in place. A smaller side redraws nothing; later pages come smaller.
- **A page drawn whole is counted in pixels** (ADR-147): at that side, at
  most `PAGE_PIXELS` (an A4 page at `MAX_SIDE`), never fewer than
  `MAX_SIDE` on its longer side (`document::held_side`): a tall page (a
  webtoon's) whole at its own width. The reader asks each page at that
  (`DocumentScroll::whole_side`), the page shown by the preview's job
  too, the first before any was shown included (as an upright page of
  ISO's shape, nothing known yet), never the side shown however large (a
  plugin from outside Skwir was asked 8192 pixels and its picture
  refused): its shape as its engine said it, else as the nearest page
  known (the page shown's from the start, `keep_shown_page`), never
  ISO's once one is known (a webtoon's every page was drawn twice, first
  273 pixels across). Read a page at a time, the page shown is the
  preview's to draw: a shape learned that asks more of it whole has it
  drawn again where it is (`keep_shown_page`, as a zoom does), never
  into the cache alone, which the panel never showed. Every
  helper bounds a whole page by the same function; the manager takes at
  most a square page's pixels at `MAX_SIDE`.
- **Past it, what shows is drawn apart** (ADR-147). Each page element
  says in prepaint what of it shows within its view, what it covers and
  what its drawing whole holds (`document::Seen`), read once laid out
  (`report_seen`, both modes). A page covering more than its whole holds
  has the part that shows, with a margin of half as much each way where
  `PAGE_PIXELS` allows, drawn at the side it covers
  (`reader_scroll::part_wanted`, `Shell::document_parts`,
  `request_document_part`), over the page in its place
  (`DocumentElement::part`). One part a page (`DocumentScroll::parts`),
  one asked at a time beside the whole pages, not asked again while what
  is drawn or asked holds what shows, to half a pixel at its side (an
  engine rounds a part to its whole pixels: held exactly, one that missed
  a hair would be asked again as it came, forever); a page that leaves
  the view, or needs no part at the zoom it shows at, lets its part go,
  atlas entry and all, as the cache does pages, and one drawn at more
  than twice the side wanted is drawn again.
- **What is drawn apart is state, chosen again whenever it may change**
  (`DocumentScroll::choose_part`). Whatever may change it says so where
  it changes (`part_changed`: what shows, a part back or failed, a page
  whole come, a session come or gone, parts refused or forgotten,
  everything given up), and it is chosen at the next turn that may
  (`Shell::choose_part_when_due`: as what shows is said and at every
  sync), with no search running and a session there to ask; never by a
  call at each place that changes it. Chosen only as the view moved, the
  second page in view never had its part, nor the view a part asked for
  one it had left; chosen only where it was called for, a document read
  again (its session arriving, nothing in view moving) and a refresh
  asked none. A part let go of asks a frame. A part being
  drawn that is no longer what would be asked gives way; only the
  answer of the one last asked is taken (`PendingPart::serial`). A part
  has its own cancel, apart from the whole pages' (`DocumentScroll::
  cancel` gives up pages read ahead, not what shows). A part that fails
  is not asked again until the page is visited or the pages refreshed;
  a part waits while a search runs, never cancelling a find the user
  started. What a page said twice in a frame (gpui's list lays out
  again after an autoscroll) counts as its last.
- **A part is asked in shares, never pixels** (`document::Part`,
  `WorkerRequest::Render`'s `part`): each engine lays its own pixels on the
  page at the side, draws the whole pixels round the part, and says the
  part it drew exactly; a part carries no text (the whole did). Natively in
  PDFium (`set_origin`), Poppler (cairo's translation), MuPDF (its matrix),
  DjVuLibre (its render rectangle), djvu-rs (`RenderRequest::region`, in
  the page as shown) and a comic (`inspection::picture_part`), each saying
  so (`DocumentSession::draws_parts`); a session that does not (a book
  laid out, a plugin from outside Skwir) is asked no part, its whole cut
  (`render_part`'s default) holding no more than the whole shown. A page
  whose whole came back at its own pixels, short of the side asked (a
  picture, never enlarged), is asked no part: it has no more
  (`Shell::held_back`, told by what came back, never by decoding a
  part). A part that comes back no denser than the whole is held (a
  density, never a count: a page peeking in by a few pixels is as dense
  as a page in view) is not kept, and its page is not asked again
  (`reader_scroll::sharper`). A part asked and none said is an error,
  never the page whole squeezed into the part's place (a helper older
  than the reader, packaged apart).
  `tools/pdf/check.py` and `tools/djvu/check.py` compare each engine's
  part with the whole's at the same side.
- **A part is counted alike by the reader and every engine**: the reader
  sizes it by `Part::most_pixels` (never fewer than any engine draws it
  at, however it lays the page's pixels and rounds the part's) to
  `PAGE_PIXELS`; every engine draws `Part::drawn`, refused only past
  `MAX_PAGE_PIXELS`, the manager's own bound. Counted by its shares alone,
  a part sized to the bound was refused by all five once its whole pixels
  rounded it out: on a screen of two device pixels a pixel, what shows in
  the middle of an A4 page at 400 %. Both checks ask a part at the bound.
- Before any page was shown, the first renders at the side an upright page
  of ISO's shape covers in the content's area (`reader_scroll::area_side`),
  never a fixed side: the area says it as it is laid out for that preview
  (`preview::watch_area`, `reader_scroll::Area`, by generation), and the
  preview's job reads it once the document is open, waiting a frame at
  most: its own preview's measure or a later one's (the same area, laid
  out for the preview that followed at once), never past its cancel. The
  page comes back with the side it was asked at
  (`PreviewResult::page_side`), which is the reader's from then on. A
  fixed 2048 px drew a cover of two JPEG 2000 scans in 3 s in a panel
  that needed 512 px (0.3 s).
- A document opening (nothing of it shown yet, never a turn) on a page
  asked larger than the smallest step has that page drawn at the step
  first (`reader_scroll::draft_side`), sent as the wait's picture
  (previews.md, the desktop's thumbnail) marked as a page (its
  `page_side`), so it stands where the page will whatever the file's
  type (`PreviewState::placeholder_page`). A page slow to draw shows
  blurred once the wait has lasted, then sharp in place; one quick to
  draw never shows it. It costs that small render: 2 ms on a page of
  text, 0.3 s for the scans above, before their 3 s. Only a session
  that draws small much more quickly says so
  (`DocumentSession::draws_small_quickly`: the helpers, which draw at the
  scale asked); a comic's or a fixed-layout book's page, a picture
  decoded, is not drawn twice, and a JPEG page larger than asked is
  decoded at a reduced scale (`inspection::picture`, through
  `picture::jpeg_fitted`), where that holds what an embedded picture
  may.
- A cached page remembers the side it was asked at, so an engine with no
  more pixels (a small picture) is not asked again.

## Reflowable documents

- A reflowable document (a MuPDF book, `DocumentInfo::reflowable`) is laid
  out by its worker at the preview's text size; size keys relayout instead
  of zooming (`Shell::preview_zoom`, `relayout`). The size chosen for it
  is a percentage of Reading's 100 %, as an inspection's
  (`preview.text_percent`, `None` again on reset: Reading's setting), so a
  desktop's size carries it (`PreviewState::book_text_size`). A book is
  laid out again, where the reader is, whenever the size it should be at
  is not the one it was asked at (`laid_out_at`): as a size changes
  (`Shell::follow_book_size`, from `resolve_theme` and the preview's
  steps) and as its session comes, so a change made while it was being
  laid out, when there was no session to lay out again, is not lost.
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

## Outlines

- A document's outline is its entries in reading order, each at the depth
  its tree gives it (`DocumentInfo::outlines`), walked from child to
  sibling by every engine; an entry that leads nowhere is kept, without a
  page. PDFium's is never counted through a bookmark's `parent`, which
  pdfium-render knows one level up only: the third level read as the
  second. `tools/pdf/check.py` asserts three levels and an entry without
  a destination for each engine.
- The contents' entries and the pages they lead to (`outline::Pages`) are
  built once per session (`reader::Outline`, in `reader_opened`), never
  at a frame; the part shown is the entry leading to the highest page at
  or before the one shown, the last of those, found by binary search.

## Measuring

- Separate renderer time from full UI latency, and include child PSS, not
  only Skwir's.
