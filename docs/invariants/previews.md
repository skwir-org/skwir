# Structured preview invariants

Read this before touching `crates/core/src/inspection.rs`,
`crates/platform/src/{archive,inspection,rich_preview}.rs`,
`crates/archive-worker`, any `crates/plugins/preview-*` crate, or
`crates/ui/src/inspector*.rs`, `code_preview.rs` or `markdown.rs`. It is
binding, like `AGENTS.md`.

## Providers

- Format plugins publish `inspection::Inspection` from scheduler
  workers, via the preview registry and owner lifetimes. Fallbacks rank
  below typed providers, also after disable and re-enable.
- Providers of one priority never accept the same file (registration
  order would decide). A provider that settles a shared name by content
  (geo's Litchi `.csv`) outranks one that takes the name whole (the
  table reader). `format_providers_never_compete_for_the_same_file`
  checks every wired preview plugin (`Previews::accepting`).
- **Whoever accepts a file owns it** (`Previews::dispatch`). An `inspect`
  returning `None` does not pass the file on: the panel says the
  provider has no preview.
- `accepts` settles a shared name, on a worker: by the bytes' type when
  the MIME database knows both formats (a GIMP brush's `.gbr`), else by
  the file's start (`.nc`: NetCDF or a CNC program; a Gerber `.gbr`).
- A thumbnail provider's `accepts` runs on the UI thread, on names only;
  its `thumbnail` returns `None` for a file not its own.
- A changed preference re-registers only its provider
  (`Previews::unregister`), so a document open in the reader stays open.
- Rich text, rows and images stay toolkit-independent; the UI owns
  virtualized layout and commands.

## Bounds

- Bound input, expansion and visible work: a short file can hold a huge
  field or deep nesting.
- No library allocates from what a file claims. Cells are streamed and
  only the page kept (two cells a million rows apart cost two cells), so
  ODS and XLS, whose calamine readers build the whole grid, are not read.
  What a library reads whole is first inflated through a counter. A
  self-declared unpacked size (WOFF) goes through our bounded inflater.
- A database opens read-only and immutable (nothing written beside it,
  WAL or not), schema untrusted, tables only, each cell bounded in the
  query, cancellation wired to SQLite's progress handler.

## File types

A name that says nothing is not a binary file.

- `mime::by_name` returns `None` for a name without a glob
  (`settings.cfg`, `README`, `.env`). The preview worker then types the
  bytes (`mime::by_content`: magic, else `inspection::is_text`) before
  any provider runs, and the header shows that type.
- `application/octet-stream` means the bytes were read and are binary,
  never a guess from the name. It is all the hex preview accepts.
- A declared type counts only as far as the bytes agree
  (`preview::compute`): text is read as text whatever its name.
- Text is what `inspection::decode` reads: UTF-8 without NUL, else UTF-16
  by its byte order mark or Windows-1252, if free of control characters
  (an old zip's `.txt` is Latin-1).
- `icons::unknown` is the only place the octet-stream icon means
  "unknown": a picture, never a type claim.

## Programs run on files

- Every program run on a file that is not ours goes through
  `sandbox::untrusted` (`skwir_platform::sandbox`, bubblewrap). Where
  nothing isolates, converters (LibreOffice, Pandoc) and ImageMagick
  refuse (`Otherwise::Refuse`); the desktop's thumbnailers, Skwir's
  decoders (`ffprobe`, `ffmpeg`, Poppler's `pdfinfo` and `pdftoppm`) and
  helpers (the archive and model workers) run unisolated
  (`Otherwise::Run`).
- A sandbox that started and failed is never retried outside it.
- Inside, the file is at `sandbox::inside_name`, never its own name; what
  a program would read from the name (a compressed file's one member) is
  passed with `--stem`.
- The PDF readers and the player are sessions of their own, outside the
  sandbox (ADR-004, ADR-005, ADR-016).

## Pictures

- Skwir decodes in-process what `thumb_providers::decodes` lists, and
  `ImageDecoder` thumbnails all of it; a new in-process decoder adds its
  type there, which keeps the fallback away. A drawing
  (`thumb_providers::is_drawing`: SVG, plain or compressed) is drawn to
  the thumbnail's side, enlarged too, by the preview's inert path.
  Other `image/*` types and 3D models (`model/*`, and types the MIME
  database draws as pictures, `pictured`) go to the desktop thumbnailer
  (`thumbnails` plugin, a fallback). Its drawing is final
  (`Pixels::Drawn`): never sharpened, its own size read from EXIF when
  known.
- ImageMagick (`skwir_platform::magick`, ADR-031) draws what no
  thumbnailer does: the last thumbnail provider (`Stage::LastResort`,
  after every plugin's fallback), and the preview's after the
  thumbnailer. It reads only the format the MIME globs name, passed
  explicitly, never one in `magick::REFUSED`; only in the sandbox; under
  a policy written per run (that format in, PAM out, no delegate, bounded
  memory, one thread); at most `magick::RUNNING` at once.
- **A picture is decoded when it is seen** (`inspection::Picture`). A
  provider that can read it again names it (`Picture::Later`: key and
  display size, via `inspection::picture_size`) and decodes it in
  `PreviewProvider::picture` under the same containment. The row holds
  the box, so nothing moves when it arrives (`markdown::Shown`).
- `inspector_pictures.rs` asks for what the rows drew, nearest first plus
  one before and after, one at a time, from the provider that answered
  (`Display::origin`). It takes only its generation's results, keeps
  `CACHE_BYTES` of pixels, and drops the atlas entry of each picture it
  lets go, and of all of them when the content is replaced.
- What cannot be read again (a message's parts) or is part of the page
  (an SVG drawing) is `Picture::Ready`, decoded with the page within
  `EMBEDDED_IMAGES`.
- Floating-point pictures hold linear light and are encoded as sRGB in
  `picture::decode`, which every shown picture passes.
- A picture the panel shows and a document's pictures sit on
  `picture_backdrop` as thumbnails do (`backdrop`, views.md). Their ink is
  read where they are decoded, on the worker (`PreviewContent::Image`'s
  `ink`, `backdrop::Picture::of`), and weighed against what they lie on:
  the panel's `surface` for a picture, its letterbox; `bg` for a
  document's page. A page or a film is opaque (`ink: None`). The backdrop
  lies under the pixels, never the box Fit gives them.

## Models, drawings and diagrams

- A model, drawing or print shows the picture its application stored
  (`preview-models`), read from where the format keeps it and no
  further: a Blender file's blocks before `TEST` (a compressed one
  stream-decoded that far), a package's named members, a G-code's first
  `TEXT_INPUT` and last `TAIL` bytes, a DWG's listed pictures.
- Nothing is drawn in 3D in-process. A mesh without a stored picture
  goes to `skwir-model-worker` (`skwir_platform::model`): it reads within
  `model::BYTES` and `model::TRIANGLES`, follows glTF buffers only below
  the model's folder, and returns a line of measures and a PNG. Without
  the helper the plugin declines the mesh, leaving it to a desktop
  thumbnailer.
- A drawing made from content (`preview-circuits`: KiCad boards,
  footprints, schematics, symbol libraries; `preview-models`: DXF) is SVG
  rasterized by `inspection::drawing`, the inert path of every drawing:
  nothing outside it is read, and text uses the system's fonts (KiCad's
  stroke font is GPL, not ours to carry).
- A KiCad board is read item by item (`sexpr::Reader`), never as a whole
  tree. Its text is held, so files over `INPUT` are not drawn and say
  so, their source one toggle away.
- DXF blocks expand within budgets of points, texts, inserts and depth
  (`dxf::POINTS`, `INSERTS`, `DEPTH`), since nested array inserts
  multiply. What is left out is said.
- G-code without a picture is drawn from its moves (`toolpath`) straight
  into pixels (tiny-skia), streamed twice (bounds, then drawing): the
  picture is held, never the moves. Segments under a pixel are merged.
- A Mermaid diagram's source goes to `skwir-mermaid-worker`
  (`skwir_platform::diagram`), never to a layout engine linked into
  Skwir. Its SVG is decoded with the page like an HTML drawing, within
  `EMBEDDED_IMAGES`. What cannot be drawn, the helper missing included,
  stays code.

## Text documents (ADR-030)

A text document is read as its structure, or laid out as pages.

- Word and OpenDocument text become blocks in `preview-documents`
  (`docx.rs`, `odt.rs`, `blocks::Builder`), never via HTML, so lists keep
  their document's markers and headings their numbers. Pandoc's HTML
  (RTF, or any format when chosen) goes through `rich_preview::html`.
- The reading provider's `accepts` picks the presentation from the Text
  documents setting, the file's first bytes and what is installed. A
  choice whose program is missing gives way to the next that can show
  the file.
- A package is read by normalized member name, each member within
  `BINARY_INPUT` of inflated bytes. Pictures are named for later by
  member; Pandoc's come from its conversion's cache (extracted again if
  evicted).
- A long document is cut into parts between paragraphs or table rows
  (`blocks::PART_BLOCKS`). List and note numbers count from the start;
  each part's notes follow it under a rule.
- Pandoc runs only in `skwir_platform::sandbox`, as LibreOffice does.
- An Office or OpenDocument file's grid picture is its stored picture of
  the first page (`stored.rs`), ahead of a conversion's first page.

## Links

- A link in a document goes where it says: `http`, `https` and `mailto`
  to the browser; an anchor to its heading; anything else to a place on
  this machine, resolved next to the document and never canonicalized,
  so the folder shown is the one named. The pane goes there and
  `Shell::reveal_path` selects the file once listed, which previews it.
- A link is a place to show, never a file to run.
- Every kind follows links through `Shell::follow_document_link`, a
  PDF's too (beyond links to its own pages).
- An address found in text (ADR-029) is a link: in a document read as a
  page (Markdown's bare addresses, a plain-text mail) like a written one;
  in raw text (the code view) only under Ctrl, since a click there
  selects (`skwir_platform::links::link_block`, on the conversion
  worker).
- Every address, written or found, and the map's attribution reach the
  desktop through `ui.open_url`.

## HTML, EPUB and mail

### Containment

- HTML reading mode is inert: no scripts, no remote assets, no automatic
  link opens.
- Markdown images, and an HTML file's pictures and stylesheets, stay
  inside the document's directory after canonicalizing; EPUB resources
  stay inside their container; email images use content IDs.
- A caller hands `rich_preview::html` its `Resources`; the adapter never
  opens a file or a URL itself.
- An SVG picture is rasterized with every outside reference refused.

### Structure and style

- **HTML is read for its structure** (`rich_preview::html`, for HTML
  files, EPUB and mail): headings, paragraphs, quotes, code, rules.
  Lists are a depth and a marker, numbered from `start` as `type` says
  (`inspection::Numbering`, shared with word processors), the marker on
  an item's first block only. Table rows are cells that are ranges of
  the row's text, as in Markdown: an empty cell is still a column, and
  white space between cells belongs to none.
- XHTML (book chapters, `.xhtml`) is parsed as XML first
  (`rich_preview/xml.rs`) into the same tree, so `<title/>` cannot
  swallow a page.
- **A page's own styling is kept where it says how text reads**
  (`rich_preview/style.rs`, ADR-012): which elements make blocks or are
  hidden; emphasis, underline, strike; alignment (`Block::align`);
  preserved line breaks; size relative to the body text (`Block::size`,
  running text only); left margins as a bounded indent (`Block::indent`,
  never on quotes, lists or tables).
- Styling is a cascade (built-in sheet, the page's sheets, `style`
  attributes, `!important`), never hard-coded tag checks: what a tag
  means is stated once, in the built-in sheet. Fonts, colours, boxes and
  absolute sizes stay the reader's. A book searches with its
  stylesheets, so hidden text never matches.
- Superscripts, subscripts and small capitals are runs (`Run::script`,
  `Run::small_caps`, from `vertical-align` and `font-variant`), drawn by
  gpui at their own size and height (`gpui::RunPosition`, vendored patch
  0006). Small capitals are reduced capitals of lowercase letters, only
  where the capital has the letter's length
  (`inspector_text::small_capitals`), so the drawn text keeps every
  offset that selection, links and copy use.

### Notes and anchors

- Every `id` is a link target (`Block::anchors`): a section's is its
  first block, a word's the block around it.
- A footnote (`footnote`, `note`, `rearnote`, `doc-footnote`) moves to
  the end of the page under a rule. A printed page number (`pagebreak`)
  is a target, never read.
- **A note opens under its reference** (`note_popover.rs`). A reference
  is a link the book marks as one or sets raised (`Run::note`). A note is
  every block of the element marked as one (`Block::note`: a footnote,
  an endnote, an item of a list of notes), else the one block its target
  lands in. A note in another chapter is read there on a worker, through
  the provider that read the page; a reference to no note goes where it
  says.
- The popover is a toolbar popup (`popup_panel`): it takes the keyboard
  and gives it back. Escape, a click outside, scrolling or replacing the
  text closes it. It opens where the reference is drawn, read from its
  text layout on click.

### Books

- **A book's rows are its contents** (`inspector::listed`), shown in the
  Contents drawer, never as list items after the text nor read by find
  and copy. They are the table of contents alone
  (`preview-epub::table_of_contents`: the EPUB 3 `toc` nav, else the NCX
  `navMap`), never the landmarks, the printed page list or the NCX's
  copy of the chapters, which `rbook::Epub::toc` also iterates.
- **A book counts chapters or printed pages** (`Inspection::printed_pages`,
  from `page-list` or NCX `pageList`; the `ui.printed_pages` preference,
  chosen in the count's own picker, `preview_bar::unit_popup`).
  - By pages, the field, previous, next, `[` and `]` go by printed page.
    A page is found by its printed label and shown at the top from the
    block holding its mark (`inspector::page_starts`, built when a
    chapter arrives).
  - The page shown is the last to start at or above the top of the view,
    read from the list's scroll, never measured.
  - A page in another chapter opens that chapter at its mark; a chapter
    reached by a link then shows only its number (`request.target` is
    cleared on arrival).
  - A book without printed pages counts chapters, whatever the
    preference.
- A book reads in the document's typography (the reading profile).
- **A book of pages is a document** (ADR-013 addendum). A fixed-layout
  book (`rendition:layout pre-paginated`) with the built-in engine, or a
  comic, opens in the document reader through an in-process
  `DocumentSession` (`preview-epub/src/pages.rs`, `preview-comics`),
  continuous or a page at a time like a PDF, never as an inspection
  (`inspect` returns `None`). A page is drawn or decoded at the side
  length the reader asks, 100% being its own size (CSS pixels for a
  page, pixels for a picture). The book's contents are the reader's
  outline. Search finds pages, not places: a drawn page has no text
  layer.
- **A fixed-layout page is drawn, not read** (`rich_preview/page.rs`,
  ADR-013). A page that states its size is laid out from the cascade
  (`PageStyle`) and drawn with the requested size on its longer side.
  - Text goes to resvg as SVG `<text>`, the book's fonts renamed to
    their CSS families. Pictures are decoded by the `image` crate and
    composited between SVG fragments in paint order (`Canvas`), never by
    resvg's decoders.
  - Fonts and pictures come only through the caller's `Resources`. The
    provider restores an obfuscated font; an encrypted one is not read.
  - In a book of pages, a page that states no size and is one picture is
    that picture; any other takes the size the first pages state.

## Markdown and blocks

- Markdown events become semantic blocks directly
  (`crates/plugins/preview-markdown/src/markdown.rs`): heading level,
  list depth and marker, quotes and table cells stay in
  `inspection::Block`, never flattened to HTML and scraped back. Raw HTML
  blocks are read in place by `rich_preview::html`: dropping them loses a
  README's logo and centered headers.
- Every image in a document, Markdown's or HTML's, goes through the one
  document-local loader.
- A block is text, a rule or a table row (`BlockKind`), not flags that
  can disagree.
- A table row is text: its cells are tab-joined ranges of `Block::text`,
  so they keep emphasis, code and links as ordinary runs, and search,
  selection and copy need no special case. `inspector_text::part` draws
  each range as a column, clipping runs, links and selection to it and
  reporting pointer offsets in the block's coordinates. `split_blocks`
  splits only running text: a row's ranges would not survive it.
- Headings carry a link target (`Block::anchor`): the explicit `{#id}`,
  else a slug, numbered when it repeats. `[jump](#the-plan)` resolves
  against it.
- Front matter is parsed as a metadata block and skipped, never read as
  a rule and a heading.
- A list marker belongs to its item's first block: the block that takes
  it spends it, so a loose item's later paragraphs repeat no bullet.
- An image ends its block; both halves keep their quote and list. The
  picture is the block's middle (`markdown::block`): in its item's column,
  with the item's marker at its foot when the item begins with it.
- A picture's description (an image's `alt`, a drawing's title) stands in
  for it, as a browser draws it (`Block::set_picture`): never beside a
  picture shown, in its box when it cannot be decoded, and as the block's
  text when there is no picture to show. A Markdown image's description is
  the plain content of its brackets, as CommonMark reads it: code and
  inline HTML as written, an image inside by its own description, a line
  break as a space.

### Reading typography

- Reading typography lives in one place, `crates/ui/src/markdown.rs`
  (`MarkdownStyle`). Block layout asks it for sizes, spacing and colors
  and never hard-codes them in the inspector.
- `InspectionKind::Markdown` selects the roomier reading profile. Other
  kinds keep the compact spacing, which must stay legible: EPUB, email
  and HTML read in it.
- A block's frame (spacing, quote border, list indent) is drawn once for
  every kind; only the middle differs, so nothing can return early and
  lose the border.
- Reading mode caps the column at `MarkdownStyle::measure`, centres it
  and keeps `page_padding` to the pane's edge, all in layout, never from
  a measured width. The column is for text only.
- The cap is the Reading width preference (`reading.width`, in ems, so it
  grows with the text size; `markdown::measure`); Full width is no cap.
  The Full width button is that preference (`ui.full_width`, through
  `settings.apply`), placed after the text size as Fit width is after a
  document's zoom, and the first to go to More.

### Scrolling to a target

- `ListState::scroll_to_reveal_item` measures only through what the list
  has laid out, and alone barely moves. A heading, a search match or an
  outline entry further down needs `scroll_into_view`, which jumps
  straight to anything unmeasured.

## Archives

- Only `skwir-archive-worker` links libarchive.
- For a preview, member names are labels and keys, never paths: read
  only the selected regular members, bound the actual decompressed
  bytes, and keep temporary cleanup off the UI thread.
- Names reach the filesystem in one place, an archive extracted
  (`--extract`, ADR-048): by libarchive's disk writer and its guards (no
  `..`, no absolute path, never through a link it made, no owner or
  set-id bits), in the sandbox writing only where it extracts, into a
  staging folder put in place whole.

## The preview frame

Every preview fills one frame; none builds its own. The design and what
is still to come are in `docs/preview-frame.md`.

### Location and waiting

- **Nothing selected shows where the pane is.** A folder is previewed as
  a selected one would be (its items, Details), named as the sidebar
  names it, with a Volume group in Details (the volume's sidebar name,
  device, filesystem, free space). The folder is stat-ed and the volume
  measured on a worker (`Shell::here`, `VolumeFacts`), again when the
  pane shows another location or its listing completes a change; what
  was shown stays until the answer.
- A location that is not a folder (a search, the shelf, Recent) shows
  its name and count in the header and nothing below: no tools, no empty
  Details.
- **A wait shows only once it has lasted.** The panel has one delayed
  loading signal (`Shell::sync_busy`, `preview_busy`), like the pane's
  (`loading.rs`): a request slower than `BUSY_AFTER` (200 ms) shows
  "Loading…" in the body and caption, and the bar under the toolbar; a
  quicker one shows nothing in between. Each request (generation) has
  its own grace. Nothing draws from the phase directly.
- A preview on its way is not "nothing to preview": Details fills the
  body only when the phase is idle (`details::alone`), never for the
  instant before a picture arrives.
- **The desktop's thumbnail is the wait's picture, not a preview.** Sent
  ahead and kept aside (`PreviewState::placeholder`), it replaces
  "Loading…" once the wait has lasted (`preview::placeholder`) and is
  never shown before, so a quick preview never flashes it. It stands
  where the preview will: an image's at the image's own size (from its
  header), so only sharpness changes; a document's where the reader
  draws page one; anything else at its proportions, as large as a
  preview may be. A preview replaces it; a file with no picture of its
  own (no preview for its type, a film until its player has a frame)
  keeps it.
- **A changed file is read again in place.** Pane, entry, path and
  providers identify a preview (`PreviewKey`); a change in any is another
  preview, and nothing of the last survives. The file's date, size and
  code colors (`code_preview::CodeTheme`: the look code is in and its
  chosen theme) are its `Version`. A new version asks the same request
  again (`Shell::preview_changed`); the reader keeps its page, zoom, find
  field, folds, selection and place, and a live player keeps playing. A
  theme change re-reads only text, the one thing drawn in its colours.

### Surfaces and colours

- What is read (text, code, tables, bytes, trees, prose, the facts about
  an entry) is on `theme.bg` in `theme.text`, like the file list.
  `theme.surface` is chrome: the header, the toolbar, and the letterbox
  around a picture, a film or a page (`preview::on_paper` decides).
- `code_bg` sets code blocks, inline code and table headers apart on
  paper; `zebra` tints alternate rows; hover is `theme.hover`. No preview
  derives or hard-codes a colour.
- The one other background is a code theme's: one chosen on the Reading
  page brings its own, under the code view and code blocks
  (`Display::code_bg`). A look lists only themes of its own lightness, so
  the tokens over them still read.
- Code kept in the look Skwir is not in (`reading.code_appearance`) uses
  that look's tokens (`Display::code_dark`, `Theme::in_look`): the whole
  code view with its gutter, selection and Default's background, and a
  document's code blocks (`MarkdownStyle::code`). Never Skwir's tokens
  over colors meant for the other look.
- Muted is for secondary facts (line numbers, captions, column headers),
  never for data: offsets, sizes and values are `theme.text`. The only
  dimmed data is what stands for nothing: a zero byte, the dot of an
  unprintable one.

### Header and Details

- **The header is a card** (`preview::header`): the entry's icon, its
  name (a trail inside a container) and one line of type, size and date.
  It names what the panel shows: under a container's view kept above a
  member, the container (the member's bar names the member).
- Clicking the card opens Details. So does the (i) button beside it,
  since what the card does is not seen until tried; the button is lit
  while Details is open, as Preview's is while its panel is.
- The card has no close button: the Preview button closes the panel.
  Only the peek overlay keeps one.
- **Details** (`details.rs`) is a section of the frame, not a kind of
  preview: everything known that the header does not say (no Name or
  Type row).
  - With nothing to preview (a folder, an unknown file, an error) it is
    always open, as tall as its content, the toolbar and the content's
    message (why the preview failed) right under it. Not while a
    container's view is kept above the member (`details::alone`): that
    view is content, so Details stays as the card left it.
  - Under a preview it opens on demand in the same place: under the
    card, above the toolbar, up to half the panel (Ctrl+I; with the
    panel closed, Ctrl+I opens both).
  - Whether it is open belongs to the window's shape (`session.rs`).
  - Only `preview::under_card` places it. The toolbar and the content
    (`preview::body`, what a container's split holds under its view)
    never draw it, so neither a kind nor the split can come between the
    card and Details.
- Its facts come from three places, none read while painting: the
  entry's metadata with its provenance, collected when the entry, its
  listing or the selection changes (`entry_groups`, pure and tested);
  what the preview holds in memory (a picture's size, a document's
  pages, a film's length and tags, what the camera wrote:
  `skwir_platform::exif`, pure Rust, read on the worker that decodes the
  picture); and what the format said (`Inspection::details`, from the
  provider's own parse, each fact bounded by `inspection::detail`).
- A field a plugin or the user attached shows under More with who said
  it; a model's suggestion says so.
- Owner and group names come from `skwir_platform::users`, loaded once
  on a worker, never read on the UI thread.
- Details never starts a scan: a folder says what its listing already
  counted. A click copies a value.

### The toolbar

- **One toolbar.** A preview never builds a row: it lists its tools in
  slots of `preview_bar::Bar`, which lays them out. Left to right:
  1. a member's name under its container's view (`Slot::Title`);
  2. contents, first because its drawer opens on that side;
  3. navigate: previous, the page or chapter as a field with the count,
     next (`preview_bar::number_tool`, for the reader's pages and a
     book's numbered sections alike, `Inspection::sections`);
  4. view: zoom or text size (smaller, the value as a picker, larger);
     Fit width for a page or Full width for text; the presentation (the
     reader's Continuous, or the format's name as the switch between
     rendered and source);
  5. spacer, find, More;
  6. the × that leaves a member (`preview_bar::close_tool`), the same for
     every kind, also for a member that could not be written out, whose
     failure stands in its place (`PreviewState::inside`).
- An item states the narrowest of the three variants that still shows
  it (`keep`). Text and a page give way alike: a tool in the same place
  has the same `keep`.
- Copy has no button: it is Ctrl+C and the content's right-click menu
  (`Shell::open_preview_menu`), where people look for it. An action too
  rare for a button in any variant (View as hex) is `Item::menu_only`.
- `zoom_tools` is the one zoom group, for a picture and a page alike;
  text's twin is the inspector's text size picker (`text_size_panel`, a
  popup of kind `text`).
- Shared actions take their words from `preview_bar::label`.
- **The menu rule** is every responsive toolbar's. More holds the
  buttons the chosen variant dropped (`PopoverAnchor::popup_for`), a
  kind's settings that never have a button (volume, speed, tracks), and
  what no kind has a button for (open externally, copy the path).
  Nothing is both a button and a menu row. More is there whenever it
  holds something, last but for the ×.
- Open externally and copy the path belong to the entry: under a
  container's view they are in its More, and a member's More holds only
  the member's own (`Bar::always`).
- Another tool's popup (the zoom picker, the find field) anchors to that
  tool, or to More when the variant dropped it (`Item::opens`,
  `preview::active_popup`), in the bar it was opened from
  (`PreviewState::menu_parked`).

### One of each

- **Find** is a field under its button with Enter and F3, the same in the
  reader and in structured previews (`preview_bar::find_popup`, key
  context `PreviewFind`).
- **Contents** is one drawer (`outline.rs`) for a document's outline, a
  file's headings and a book's chapters.
- What a preview says about itself (how it is shown, what was left out)
  is one strip under the toolbar, never a grey sentence in the content.
- **Keys** mean the same wherever they apply: Ctrl+F, Ctrl+C, Ctrl++ and
  Ctrl+- (zoom what is looked at, size what is read), Ctrl+0, `[` and
  `]` for the previous and next section or page. A picture takes them,
  and Ctrl+wheel, once clicked.
- What a zoom key does follows from what is shown, never from a
  per-format switch; the user asks for bigger or smaller and never
  chooses the mechanism. Text in reading mode changes size (the
  inspector's text size). An image is a picture (`preview::picture`,
  `zoom_tools` in its bar): whole by default (Fit), as wide as the panel
  (Fit width), or at a share of its own size. A page zooms in the reader,
  a PDF's, a fixed-layout book's or a comic's alike
  (`docs/invariants/reader.md`). A book an engine laid out into pages is
  laid out again at the new text size.
- A picture is laid out at its own size (100%: its size before it was
  shrunk for display), never at its pixels', which only fill its box.
  After layout it compares the device pixels it covers with those it
  has; when short, it is decoded again at the next step
  (`preview::sharper`, `Shell::sharpen_picture`), never past its own
  pixels, in place: zoom and scroll stay.
- A zoom step from Fit or Fit width starts at the scale they show
  (`Shell::picture_scale`, read on the key press from the bounds the
  picture's container had in its last prepaint, or the scale the
  reader's pages reported), so Zoom out never enlarges.
- **Sections follow each other under the wheel.** Where there is a
  previous and a next section (a book's chapters, pages of text, bytes or
  rows), the wheel pushing on past an edge turns the section, but only
  after coming to rest there, so reaching the end of a chapter does not
  leave it (`inspector::edge_step`, tested); a trackpad has to mean it
  (`WheelTurn`). Going back lands at the end of the section before.

### Maps

- A map (`map_preview.rs`, `elements/map.rs`, ADR-020) is an inspection
  (`InspectionKind::Map`), so Details, the switch to the source and the
  way out of a container are the frame's. Its places are projected once
  on the worker (`Display::map`), never while painting.
- The pointer drags it; the wheel (no Ctrl: nothing scrolls) and a
  double click zoom around the pointer. Once clicked, the arrows move it
  and the zoom keys zoom it (`MapPreview`). Ctrl+0 and Fit fit it to its
  places at whatever size it is drawn.
- The view belongs to the file: kept when it is read again, fitted for
  another. Colours are the `map_*` tokens, on paper. No Find.
- Tiles from the registered tile source (`map_tiles.rs`) are asked for
  in prepaint, farthest from the middle first (a source serves the
  newest request first), and cancelled when they leave the view; a
  coarser or finer tile already here stands in meanwhile. The source's
  attribution is at the bottom right whenever its tiles are drawn.
- The Background button chooses among the backgrounds the source's
  setting lists (`map_tiles::Backgrounds`, read once per source
  revision), applied through `settings.apply`.

### Rows

- What the inspector works out about rows is pure and tested
  (`inspector_rows.rs`), never computed while painting.
- A table's column widths are estimated from character counts on the
  worker that builds the `Display` (a cell wider after all is
  truncated). A column whose filled cells are all numbers is
  right-aligned.
- A click on a header sorts the rows shown by value or by text, blanks
  last; the next sorts descending, the third restores the file's order.
  It reorders the index of visible rows, never the data, so selection,
  find and copy keep working.
- A selected row of the byte view is decoded in a strip under it
  (integers in both byte orders, a float, the text), outside the
  sideways scroller.
- A font's sheet is drawn for the panel's width, its blocks stacked, so
  a long specimen pushes the rest down.

### Members of containers

- **A member is previewed as the file it is.** Providers take a path, so
  the container writes the member out (`PreviewProvider::extract`) into a
  private directory the panel owns, under a fixed name plus an extension
  reduced to letters and digits (`archive::member_file`): previews tell
  formats by extension, and nothing else of a member's name may reach
  the filesystem.
- The worker pipeline then treats it like any file: an archived picture
  zooms, an attached PDF opens in the reader, a JSON member folds.
  Sessions may hold it open; it lives as long as the panel shows it
  (`preview::Nested`).
- The panel follows containers `NESTING` deep. The header's trail and
  the × at the end of every bar lead out. While a container's view is
  kept above the member, the trail is only words: the card opens
  Details, and that click must not leave the member.
- These directories live in the one deletable cache under the process
  id and are removed only on a worker; what a killed process left is
  swept at the next start.
- The entry's commands (open externally, copy the path) stay about the
  container: nothing opens a member from its temporary path.
- A provider whose targets are not files (a book's chapters) returns
  `None` and shows them itself. A container that only names files that
  exist on their own (a playlist) returns them with `own_file`: nothing
  is written or removed, and a location not on this machine is refused.
- **A container's view stays above its member** (`members.rs`). A member
  of an archive or a message (`inspector::kept_rows`) is previewed under
  the container's view, left as it was shown: its bar with Find and
  More, strip, header, folds and place, the member's row highlighted.
  - The view is parked whole when the member opens (`members::Parked`:
    the `Display` and its `inspector::State`), drawn by the inspector's
    own functions (`inspector::View`), never read again.
  - The member's × or Escape (`PreviewMember`) gives the panel back to
    the view as it was (`Shell::close_member`); so does the trail, to a
    depth where one is kept.
  - The member goes under the view or to its right, as More's Layout
    says (`layout::Member`, in `ui.layout`). Automatic follows the room:
    under it in a panel at the right edge, to its right at the bottom and
    in peek.
  - Their edge is dragged like a split's and stores the view's share,
    one per direction (`members::Share`), in the window's shape. Each
    side keeps a minimum.
  - Details stays under the card, above both (`preview::under_card`):
    it is about what the card names.
- The highlight leads: a click on a file, the arrows (`PreviewMembers`:
  Up and Down walk the rows, Left and Right fold a folder or go up to
  it, `inspector::State::walk`) or a find that lands on a member
  previews it. A folder, or a file that cannot be opened, is only
  highlighted.
- Each side tracks a focus handle, so a press in one gives it the
  keyboard, and `ui.inspect` acts on that side
  (`Shell::container_has_keyboard`). What changes the view alone
  (`inspector::VIEW_ONLY`) is done on it in the panel's place
  (`Shell::with_parked`, through `inspect_here`, never a command);
  another file opens in the member's place (`sibling`); anything else
  first shows the container again.
- Views are kept per member (`PreviewState::containers`, beside
  `nested`) and go with it when the panel goes up, their pictures
  evicted. While a member is on its way, its view is the one it was
  asked from (`container_asked`).
- A playlist keeps the player's Previous and Next instead.
- The view is drawn only where the panel is interactive (peek's copy
  while peek is open): one element holds each focus handle.

## Text and code

- **One text path.** Any text file without a presentation of its own is
  the code view (`inspection::plain`): a block per line, the language
  from the name, a gutter, selection, find and copy. A rendered format's
  source is the same view (`inspection::source`). Both are paged, a page
  ending on a line; text the view's bounds do not hold (one endless
  line) stays one block of plain text. There is no second, poorer text
  renderer in the UI.
- HTML files open in the inert reading mode, like Markdown, their source
  one toggle away.
- Code inspections hold one bounded logical line per block. Fold ranges
  index those blocks: never pass them through `split_blocks` or reorder
  lines after deriving folds. Ranges are nested or disjoint.
- Derive folds from parser spans and events, never from indentation or
  delimiters inside strings. Preserve TOML and YAML comments and syntax.
- Highlight whole documents on a worker, keeping parser state between
  lines; rendering only consumes prepared runs and visible indices.
- Copy keeps empty selected lines.
- Code is colored with bat's grammars and themes (two-face), through the
  one `code_preview::Highlighter`, for the code view and every
  document's code blocks, light or dark as Skwir is unless
  `reading.code_appearance` keeps it in one. Default is Skwir's own
  colors (base16 Ocean's, on that look's background, no font styles). A
  chosen theme (`reading.code_light`, `reading.code_dark`) is one of
  bat's or one the user added to bat, read from bat's folder
  (`bat::themes_dir`, as `bat cache --build` finds them) on a worker at
  start and when the settings open; it replaces bat's theme of the same
  name, as in bat.
