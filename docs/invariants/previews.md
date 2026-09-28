# Structured preview invariants

Read this before touching `crates/core/src/inspection.rs`, `crates/platform/src/{archive,inspection,rich_preview}.rs`, `crates/archive-worker`, any `crates/plugins/preview-*` crate or `crates/ui/src/inspector*.rs` / `code_preview.rs` / `markdown.rs`. It is binding, like `AGENTS.md`.

- New format plugins publish `inspection::Inspection` from scheduler workers;
  use the existing preview registry and owner lifetimes. Fallback providers have
  lower priority than typed providers, including after disable/re-enable.
- Keep rich text, table/tree rows and images toolkit-independent. The UI owns
  virtualized layout and commands. Bound input, expansion and visible work;
  a short file can still contain one huge field or deeply nested structure.
- A link in a document goes where it says: `http`, `https` and `mailto` to the
  desktop's browser, a heading anchor to that heading, and anything else to a
  place on this machine — resolved next to the document, never canonicalized,
  so the folder shown is the one the link named. The pane navigates there and
  `Shell::reveal_path` selects the file once the listing has it, which is what
  opens it in the preview. A link is a place to show, never a file to run.
- HTML reading mode is inert: no scripts, remote assets or automatic link opens.
  Markdown images, and an HTML file's pictures and stylesheets, must stay within
  the document directory after canonicalizing; EPUB resources (pictures and
  stylesheets) stay inside their container and email images use content IDs.
  A caller hands `rich_preview::html` its `Resources`; the adapter never opens a
  file or a URL itself. An SVG picture is rasterized with every reference outside
  itself refused.
- Markdown parses parser events directly into semantic blocks
  (`crates/plugins/preview-markdown/src/markdown.rs`): heading level, list depth
  and marker, quote and table cells stay in `inspection::Block` instead of being
  flattened to HTML and scraped back. Raw HTML blocks are the exception, and are
  read by `rich_preview::html` where they stand: dropping them loses a README's
  logo and centered headers. Every image in a document, Markdown's and HTML's,
  goes through the one document-local loader.
- **A picture is decoded when it is seen** (`inspection::Picture`). A
  provider that can read a picture again names it (`Picture::Later`: its own
  key and the size it will be shown at, read from the file's header by
  `inspection::picture_size`) and decodes it in `PreviewProvider::picture`,
  with the same containment as when it was named. The row holds that box,
  placeholder or picture, so nothing moves when it arrives
  (`markdown::Shown`). `inspector_pictures.rs` asks for what the rows drew,
  nearest first, and the one before and after, one at a time, from the
  provider that answered the inspection (`Display::origin`); it takes a result
  only for its generation, keeps `CACHE_BYTES` of decoded pixels, and drops
  the atlas entry of every picture it lets go, and of all of them when the
  content is replaced. What cannot be read again (a message's parts) or is
  part of the page (an SVG drawing) is `Picture::Ready`, decoded with the
  page and bounded by `EMBEDDED_IMAGES`.
- A picture is decoded in the process when `thumb_providers::decodes` says
  so; any other `image/*` type is the desktop thumbnailer's to draw
  (`thumbnails` plugin, a fallback provider), and so is a 3D model (the
  `model/*` types and what the MIME database draws as a picture, `pictured`), and what it drew is all there
  is (`Pixels::Drawn`: never sharpened, its own size from EXIF when known).
  A decoder added in the process adds its type there, which is what keeps
  the fallback away. Floating-point pictures hold linear light and are
  encoded as sRGB in `picture::decode`, which every shown picture passes.
- A reader never lets a library allocate from what a file claims. Cells
  are streamed and only the page kept (spreadsheets: two cells a million
  rows apart must cost two cells, which is why ODS and XLS, whose calamine
  readers build the grid whole, are not read); what a library reads whole
  is inflated once through a counter first; a table declaring its own
  unpacked size (WOFF) is inflated by our bounded inflater.
- A Mermaid diagram is a drawing in the page: its source goes to
  `bibli-mermaid-worker` (`bibli_platform::diagram`), never to a layout
  engine linked into bibli, and its SVG is decoded with the page like an
  HTML drawing, within `EMBEDDED_IMAGES`. Whatever cannot be drawn, the
  helper missing included, stays the code it was written as.
- A database is opened read-only and immutable (nothing written beside
  it, WAL or not), its schema untrusted, tables only, each cell bounded in
  the query, cancellation wired to SQLite's progress handler.
- A block is text, a rule or a table row: `BlockKind`, not a set of flags that
  can disagree. A table row is text too: its cells are ranges of `Block::text`,
  which holds them joined by tabs, so a cell keeps its emphasis, code and links
  as ordinary runs and search, selection and copy need no special case. The
  view draws each range as a column through `inspector_text::part`, which
  clips runs, links and the selection to a range and reports pointer offsets
  in the block's own coordinates. Only running text is ever split by
  `split_blocks`; a row's ranges would not survive it.
- Headings carry a link target (`Block::anchor`): the explicit `{#id}` when the
  document gives one, otherwise a slug of the heading, numbered when a document
  repeats itself. `[jump](#the-plan)` resolves against it. Front matter is
  parsed as a metadata block and skipped, never read as a rule and a heading.
- Revealing a list item can only measure its way to what the list has already
  laid out (`ListState::scroll_to_reveal_item`). A heading, a search match or
  an outline entry further down needs `scroll_into_view`, which jumps directly
  to anything unmeasured. Reaching for the reveal alone barely moves the view.
- A list marker belongs to the first block of its item: whichever block takes it
  spends it, so a loose item's later paragraphs do not repeat the bullet. An
  image ends the block it sits in, and both halves keep the quote and list they
  were written in.
- Reading typography lives in one place, `crates/ui/src/markdown.rs`
  (`MarkdownStyle`); block layout asks it for sizes, spacing and colors rather
  than hard-coding numbers in the inspector. `InspectionKind::Markdown` selects
  the roomier reading profile; every other kind keeps the compact spacing, which
  must stay legible: it is what EPUB, email and HTML read in. A block's frame
  (spacing, quote border, list indent) is drawn once for every kind of block;
  only the middle differs, so nothing can return early and lose the border.
  Reading mode also caps the text column at `MarkdownStyle::measure`, centres
  it and keeps `page_padding` between the text and the edge of the pane: all
  maxima and paddings applied in layout, never a measured width. The cap is
  the Reading width preference (`reading.width`, in ems of the text, so
  it grows with the text size; `markdown::measure`), Full width being none;
  the Full width button is that same preference (`ui.full_width`, through
  `settings.apply`), after the text size as Fit width is after a
  document's zoom, and the first to go to More. The column is for text
  only.
- Only `bibli-archive-worker` links libarchive. Member names are labels/keys,
  never extraction paths. Read only selected regular members, bound the actual
  decompressed bytes and keep temporary cleanup off the UI thread.

## The preview frame

Every preview fills one frame; none builds its own. The design and what is
still to come are in `docs/preview-frame.md`.

- **A wait is shown only once it has lasted.** The panel has one delayed
  "still loading" signal (`Shell::sync_busy`, `preview_busy`), like the
  pane's (`loading.rs`): a request that takes longer than `BUSY_AFTER`
  (200 ms) shows "Loading…" in the body and the caption and the bar under
  the toolbar; a quicker one shows nothing at all in between, and each
  request (generation) gets a grace of its own. Nothing draws from the
  phase directly. Likewise a preview still on its way is not "nothing to
  preview": Details fills the body only when the phase is idle
  (`details::alone`), never for the instant before a picture replaces it.
- **The desktop's thumbnail is that wait's picture, not a preview.** It is
  sent ahead and kept aside (`PreviewState::placeholder`), shown only once
  the wait has lasted, instead of "Loading…" (`preview::placeholder`), so
  a preview that comes at once never flashes it. It stands where the
  preview will: an image's at the image's own size (read from its
  header), so only its sharpness changes; a document's where the reader
  draws its first page; anything else at its proportions, as large as a
  preview may be. A preview replaces it; a file with no picture of its own
  (no preview for its type, a film until its player has a frame) keeps it
  as its picture. Never show it as content before the wait has lasted.
- **A changed file is read again in place.** A preview is identified by its
  pane, entry, path and the providers (`PreviewKey`); a new one of those is
  another preview and nothing of the last survives. The file's date, size
  and the theme are its `Version`: when they change the same request is
  asked again (`Shell::preview_changed`) and the reader stays where they
  were: page, zoom, find field, folds, selection and place. A live player
  keeps playing, and a theme change re-reads only text, the one thing drawn
  in its colours.
- **Surfaces.** What is read (text, code, tables, bytes, trees, prose, the
  facts about an entry) is on `theme.bg` in `theme.text`, like the file list
  beside it. `theme.surface` is chrome: the header, the toolbar, and the
  letterbox around a picture, a film or a document page (`preview::on_paper`
  decides). `code_bg` sets a code block, an inline span or a table header
  apart on paper; `zebra` tints alternate rows; hover is `theme.hover`. No
  preview derives a colour locally or hard-codes one.
- **One toolbar.** A preview never builds a row: it lists its tools in
  `preview_bar::Bar`, each in a slot, and the bar lays them out. Left to right:
  contents (first, because its drawer opens on that side) · navigate (parent,
  previous, the page or chapter as a field with the count, next:
  `preview_bar::number_tool`, the reader's pages and a book's numbered
  sections alike, `Inspection::sections`) · view (zoom or text size, each
  as smaller, the value as a picker, larger; then fit or width: Fit width
  for a page, Full width for text; then the presentation: the reader's
  Continuous, or the format's own name as the switch between rendered and
  source) · spacer · find · More. Text and a page give way alike: the
  same keeps for the same places. Copy has no button: it is Ctrl+C and the content's
  right-click menu (`Shell::open_preview_menu`), where people look for it. An
  action too rare for a button in any variant, like View as hex, is
  `Item::menu_only`.
  `zoom_tools` is the one zoom group, for a picture and a page alike; text
  has its twin in the inspector (the text size picker, `text_size_panel`,
  a popup of kind `text`). Shared
  actions take their words from `preview_bar::label`. An item says the
  narrowest of the three variants that still shows it (`keep`).
- **The menu rule** is the application toolbar's: More holds the buttons the
  chosen variant dropped (`PopoverAnchor::popup_for`), a kind's settings that
  never have a button (volume, speed, tracks), and what no kind has a button
  for (open externally, copy the path). Nothing is both a button and a menu
  row, and More is always last, so it never moves. A popup of another tool
  (the zoom picker, the find field) is anchored to that tool, or to More when
  the variant dropped it (`Item::opens`, `preview::active_popup`).
- **A map is looked at and moved** (`map_preview.rs`, `elements/map.rs`,
  ADR-020). It is an inspection (`InspectionKind::Map`), so Details, the
  switch to the source and the way out of a container are the frame's; its
  places are projected once on the worker (`Display::map`), never while
  painting. The pointer drags it, the wheel and a double click zoom it around
  the pointer (the wheel needs no Ctrl: there is nothing to scroll), and once
  clicked the arrows move it and the zoom keys zoom it (`MapPreview`);
  Ctrl+0 and Fit fit it to its places at whatever size it is drawn. The view
  belongs to the file: kept when the file is read again, fitted for another.
  Its colours are the `map_*` tokens, on paper. A map has no Find. It is
  drawn on the registered tile source's tiles (`map_tiles.rs`), asked for
  in prepaint, farthest from the middle first (a source serves the newest
  request first), cancelled when they leave the view, a coarser or finer
  tile already here standing in while one comes, with the source's
  attribution at the bottom right whenever its tiles are drawn. Its
  Background button chooses among the backgrounds the source's setting
  lists (`map_tiles::Backgrounds`, read once per source revision) and
  applies the choice through `settings.apply`.
- **One of each.** Find is a field under its button with Enter and F3, in the
  reader and in structured previews alike (`DocumentFind`, `InspectorFind`).
  Contents is one drawer (`outline.rs`) for a document's outline, a file's
  headings and a book's chapters. What a preview says about itself (how it is
  shown, what was left out) is one strip under the toolbar, never a grey
  sentence inside the content. The keys are the same everywhere they mean
  something: Ctrl+F, Ctrl+C, Ctrl++ / Ctrl+- (zoom what is looked at, size what
  is read), Ctrl+0, `[` and `]` for the previous and next section or page; a
  picture takes them, and Ctrl+wheel, once clicked. Which one a key does
  follows from what is shown, never from a per-format switch: text in
  reading mode changes size (the inspector's text size); an image is a
  picture (`preview::picture`, with `zoom_tools` in its bar): whole by
  default (Fit), as wide as the panel (Fit width), or at a share of its own
  size. It is laid out at its own size (what 100% is: an image's size
  before it was shrunk for display), never at its pixels', which only fill
  its box; after layout it compares the device pixels it covers with the
  ones it has and, short, is decoded again at the next step
  (`preview::sharper`, `Shell::sharpen_picture`), never past its own
  pixels, in place: the zoom and the scroll stay. A page zooms in the
  reader, a PDF's, a fixed-layout book's or a comic's alike
  (`docs/invariants/reader.md`); a book an engine laid out into pages is
  laid out again at another text size. A user asks for bigger or smaller
  and never chooses the mechanism. A zoom step from Fit or Fit width starts
  at the scale they show (`Shell::picture_scale`: read from the bounds the
  picture's container had in its last prepaint, or the scale the reader's
  pages reported, when the key is pressed), so Zoom out never enlarges.
- **One text path.** Any text file without a presentation of its own is the
  code view (`inspection::plain`): a block per line, the language from the
  name, a gutter, selection, find and copy. A rendered format's source is the
  same view (`inspection::source`). Both are paged and a page ends on a line;
  text the code view's bounds do not hold (one endless line) stays a single
  block of plain text. There is no second, poorer text renderer in the UI.
  HTML files open in the inert reading mode, like Markdown, with their source
  one toggle away.
- **A name that says nothing is not a binary file.** `mime::by_name` answers
  `None` for a name the database has no glob for (`settings.cfg`, `README`,
  `.env`); the preview worker then asks the bytes (`mime::by_content`: the
  database's magic, else text or binary by `inspection::text_prefix`) before
  any provider is, and the header takes what they said. A request says
  `application/octet-stream` only when the bytes were read and are binary;
  that, and nothing about the name, is what the hexadecimal preview accepts.
  A declared type is trusted only as far as the bytes bear it out, by the
  same rule (`preview::compute`): text is read as text whatever it is called.
  The unknown-file icon (`icons::unknown`) is the one place the desktop's
  octet-stream stands for "unknown"; it is a picture, never a claim.
- **The header is a card** (`preview::header`): the entry's icon, its name
  (a trail inside a container) and one line of type, size and date. It is
  the one place that says what the panel shows, and clicking it opens
  Details. It has no close button: the panel is closed by the Preview button
  that opened it (the peek overlay alone keeps one).
- **Details** (`details.rs`) is a section of the frame, not a kind of
  preview: everything known about what the panel shows that the header does
  not say (no Name or Type row: the header has them). With nothing to
  preview (a folder, an unknown file, an error) it is the whole body; under
  a preview it opens right under the card, above the toolbar, up to half
  the panel (Ctrl+I; with the panel closed, Ctrl+I opens both). Whether it
  is open belongs to the window's shape (`session.rs`).
  Facts come from three places and none is read while painting: the entry's
  metadata with its provenance, collected when the entry, its listing or the
  selection changes (`entry_groups`, pure and tested); what the preview
  already holds in memory (a picture's size, a document's pages, a film's
  length and tags, what the camera wrote: `bibli_platform::exif`, pure Rust,
  read on the worker that decodes the picture); and what the format told us, `Inspection::details`, which
  a provider fills from the parse it already did (`inspection::detail` bounds
  each fact). Any field a plugin or the user attached to the entry shows under
  More with who said it; a model's suggestion says so. Owner and group names
  come from `bibli_platform::users`, loaded once on a worker and never read on
  the UI thread. Details never starts a scan: a folder says what its listing
  already counted. A click copies a value.
- **A member is previewed as the file it is.** Providers take a path, so a
  container writes the member out: `PreviewProvider::extract` puts it in a
  private directory the panel owns, under a fixed name plus an extension
  reduced to letters and digits (`archive::member_file`), because previews
  tell formats by extension and nothing else of a member's name may reach the
  filesystem. The worker pipeline then runs on that file like on any other:
  an archived picture zooms, an attached PDF opens in the reader, a JSON
  member folds. Sessions may hold the file open; it lives as long as the panel
  shows it (`preview::Nested`). The panel follows containers `NESTING` deep;
  the header's trail and the Back button, first in every bar, lead out.
  Directories live in the one deletable cache under the process id, are only
  ever removed on a worker, and what a killed process left is swept at the
  next start. The entry's own commands (open externally, copy the path) stay
  about the container: nothing opens a member from its temporary path. A
  provider whose targets are not files (a book's chapters) returns `None` and
  shows them itself. A container that only names files that exist on their
  own (a playlist) returns them with `own_file`: nothing is written or
  removed, and a location that is not on this machine is refused.
- **Rows.** What the inspector works out about rows is pure and tested
  (`inspector_rows.rs`) and never computed while painting. A table's column
  widths come from its content, as an estimate from character counts made on
  the worker that builds the `Display` (a cell that is wider after all is
  truncated); a column whose filled cells are all numbers is right-aligned.
  A click on a header orders the rows shown by value or by text, blanks last,
  then descending, then back to the file's order; it reorders the index of
  visible rows, never the data, so selection, find and copy keep working. A
  selected row of the byte view is decoded in a strip under it (integers both
  ways round, a float, the text), outside the sideways scroller. A font's
  sheet is drawn for the width the panel shows it at, and its blocks are
  stacked, so a long specimen text pushes the rest down.
- **HTML is read for its structure** (`rich_preview::html`, shared by HTML
  files, EPUB and mail): headings, paragraphs, quotes and code; lists as a
  depth and a marker beside the text, numbered from `start`, the marker on an
  item's first block only; table rows as cells that are ranges of the row's
  text, like Markdown's, an empty cell still a column; rules. XHTML (a book's
  chapters, `.xhtml` files) is read as XML first (`rich_preview/xml.rs`), into
  the same tree the HTML parser builds, so `<title/>` cannot swallow a page.
- **A page's own styling is kept where it says how text reads**
  (`rich_preview/style.rs`, ADR-012): which elements make blocks or are
  hidden, emphasis, underline and strike, alignment (`Block::align`),
  preserved line breaks, size relative to the page's body text
  (`Block::size`, running text only) and left margins as a bounded indent
  (`Block::indent`, never on quotes, lists or tables, which have a shape of
  their own). It is a cascade (built-in sheet, the page's sheets, `style`
  attributes, `!important`), never a second set of hard-coded tag checks:
  what a tag means is stated once in the built-in sheet. Fonts, colours,
  boxes and absolute sizes stay the reader's. A book searches with its
  stylesheets, so hidden text is not a match.
- **Superscripts, subscripts and small capitals are runs**
  (`Run::script`, `Run::small_caps`, from `vertical-align` and
  `font-variant` in the cascade), drawn by gpui at their own size and height
  (`gpui::RunPosition`, vendored patch 0006). Small capitals are the
  capitals of lowercase letters, reduced, and only where the capital has the
  letter's length (`inspector_text::small_capitals`): the drawn text keeps
  every offset of the block's, which selection, links and copy use.
- **A note opens under its reference** (`note_popover.rs`). A link is a
  reference when the book marks it so or sets it raised (`Run::note`); a
  note is every block of the element the book marks as one (`Block::note`:
  a footnote, an endnote, an item of a list of notes), or the one block its
  name lands in. A note in another chapter is read from there on a worker,
  through the provider that read the page; a reference that leads to no note
  goes where it says. The popover is a toolbar popup (`popup_panel`): it
  takes the keyboard and gives it back, and Escape, a click outside,
  scrolling the text or replacing it close it. It opens where the reference
  is drawn, read from its text layout when clicked.
- **Every `id` is a link target** (`Block::anchors`): a section's is its first
  block, a word's the block around it. A footnote (`footnote`, `note`,
  `rearnote`, `doc-footnote`) moves to the end of the page under a rule; a
  printed page number (`pagebreak`) is a target, never read.
- **A book of pages is a document** (ADR-013 addendum): a fixed-layout
  book (`rendition:layout pre-paginated`) with the built-in engine, and a
  comic, open in the document reader through an in-process
  `DocumentSession` (`preview-epub/src/pages.rs`, `preview-comics`),
  continuously or a page at a time like a PDF, never as an inspection:
  `inspect` answers `None` for them. A page is drawn or decoded at the side
  the reader asks, its own size (CSS pixels for a page, pixels for a
  picture) what 100% is; the book's contents are the reader's outline;
  search finds pages, not places, since a drawn page has no text layer.
- **A fixed-layout page is drawn, not read** (`rich_preview/page.rs`,
  ADR-013): one that states its size is laid out from the cascade
  (`PageStyle`) and drawn with the side the reader asks on its longer
  side. Text goes to resvg as
  SVG `<text>` with the book's fonts, renamed to their CSS families;
  pictures are decoded by the `image` crate and composited between the SVG
  fragments in paint order (`Canvas`), never through resvg's decoders. Fonts
  and pictures come only through the caller's `Resources`; an obfuscated
  font is restored by the provider, an encrypted one is not read. In a
  book of pages, a page that states no size and is one picture is that
  picture; any other is drawn at the size the book's first pages state.
- **A book's rows are its contents** (`inspector::listed`): the Contents
  drawer shows them; they are not list items after its text, nor part of
  what find and copy read. They are its table of contents alone
  (`preview-epub::table_of_contents`: the EPUB 3 `toc` nav, else the NCX
  `navMap`), never the landmarks, the printed page list or the NCX's copy
  of the same chapters, which `rbook::Epub::toc` also iterates.
- **A book is counted by chapters or by its printed pages**
  (`Inspection::printed_pages`, from its `page-list` or NCX `pageList`;
  `ui.printed_pages`, a preference, chosen in the count's own picker,
  `unit_panel`). Counted by pages, the field, previous and next, `[` and
  `]` all go by printed page: a page is found by its label as printed, and
  shown at the top of the view from the block holding its mark
  (`inspector::page_starts`, built when a chapter arrives); the page shown
  is the last to start at or before the top of the view, read from the
  list's scroll, never measured. A page in another chapter opens that
  chapter at its mark, and a chapter reached by a link is then only its
  number (`request.target` is cleared on arrival). A book without printed
  pages counts chapters, whatever the preference.
  A book reads in the same typography as a document (the reading profile).
- **Sections follow each other under the wheel.** Any preview with a previous
  and a next section (a book's chapters, pages of text, of bytes, of rows)
  turns when the wheel pushes on past its edge: it must first come to rest
  there, so arriving at the end of a chapter does not leave it
  (`inspector::edge_step`, tested), and a trackpad has to mean it
  (`WheelTurn`). Going back lands at the end of the section before.
- **Muted is for secondary facts** (line numbers, captions, column headers),
  never for the data: offsets, sizes and values are `theme.text`. The only
  dimmed data is what stands for nothing: a zero byte, the dot of an
  unprintable one.

## Structured code previews

- Code inspections contain one bounded logical line per block. Fold ranges refer
  to those block indices; do not pass them through `split_blocks` or change the
  line order after deriving folds. Ranges must be nested or disjoint.
- Derive folds from parser spans/events, not indentation or delimiters inside
  strings. Preserve TOML/YAML comments and syntax. Highlight complete documents
  on a worker with parser state retained between lines; rendering only consumes
  prepared runs and visible indices. Copy must retain empty selected lines.
