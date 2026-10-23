# Structured preview invariants

Read this before touching `crates/core/src/inspection.rs`,
`crates/platform/src/{archive,inspection,rich_preview}.rs`,
`crates/archive-worker`, a plugin of the Previews category
(`Category::Previews`: `crates/plugins/archives`, `documents`, `media`…),
or `crates/ui/src/inspector*.rs`, `code_preview.rs` or `markdown.rs`. It
is binding, like `AGENTS.md`.

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

### Full preview (ADR-068)

- **Two tiers.** A bound that keeps a preview quick (text read whole,
  a file read whole, blocks, code lines, archive members, table columns,
  a picture's pixels, a drawing's size) has a larger one for Full
  preview, both in `limits` and read only through
  `limits::Bounds::of(view)` (`InspectRequest::whole`). A bound that
  keeps hostile input out (nesting, a line, a cell, inflation inside a
  container, a decoder's allocation, a model's triangles) is the same
  for both. A provider never reads a quick bound's constant directly
  where the request is at hand.
- **Say what Full preview shows, exactly.** What a provider leaves out by
  its bounds sets `Inspection::whole_available` (`Bounds::cut`); what it
  refuses by them it refuses with `Bounds::refuse`, an
  `inspection::TooLarge` while the bounds are quick, so a refusal that
  Full preview would not lift never offers it. The panel shows a refused
  text file as written with Full preview offered (`source_instead`), any
  other as `PreviewContent::TooLarge`, and puts **Full preview** in the
  bar for both (`preview_bar::whole_tool`).
- **For the file shown only.** `whole` lives in the panel's request,
  reset with the file (`cancel_preview`) and for a member gone down into;
  never in a type's memory (`SourceTypes`, `TurnedTypes`) nor the
  session. Thumbnails, an agent's preview and reading ahead use
  `Bounds::QUICK`.
- **A member is found as it was shown.** `extract`'s `view` is the
  member's: it bounds what is written out. Where the member is listed
  (an archive's listing kept by its bounds, `archive::member`; a
  playlist's tracks; a message's attachments) it is found within Full
  preview's bounds, as the row the user chose may have been.

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
- **A picture is decoded at the side it is shown at** (`decode_image`,
  through `picture::decode_fitted`, as a thumbnail is): a JPEG twice
  that side and more at its least reduced scale still covering it (a
  24-megapixel photo computes and holds a quarter of its pixels), any
  other whole, then shrunk. The preview's own decodes take turns
  (`preview::Turn`): one at a time, and a request whose turn came after
  the panel moved on gives it up.
- **The next picture is read ahead** (`Shell::read_ahead_next`,
  `read_ahead`), `reading.read_ahead` on (the default). On the whole
  screen, the next picture the way the last step went; in the panel and
  in peek, the next entry of a walk (`preview::Walk`): two steps alike
  in the pane's order (Down twice, a grid's row down twice), so a click
  starts none, at the side the panel asks. Only once the one shown has
  arrived: the two never wait for each other's decode, and an arrow held
  down, its steps quicker than a photo's decode, starts none. One at most, `PreviewState::ahead`, kept
  beside the preview since a step replaces the preview. It is keyed as
  the step's preview will be (`shown_entry`); the step takes it in the
  sync that makes the step (`take_ahead`), so the picture is drawn in
  the frame the step is, and one still decoding is adopted as the step's
  request (`AheadSlot::Adopted`), never decoded twice. Its decode has a
  turn of its own (`Turn::Ahead`): a guess the step proves wrong never
  keeps the step waiting, and at most two pictures decode at once.
- Only what skwir decodes itself is read ahead: a type the name
  settles, which no provider accepts (asked on the worker, as `accepts`
  may read), and not written as text (an SVG may open on its source,
  and offers it). A file another program, a player or a reader shows is
  never opened before it is asked for. What was read ahead goes, pixels
  and all, when the whole screen is left, a walk breaks, the panel and
  peek close, the pane shows another location (it is the pane's work,
  `location_lifetime`) or the setting is turned off.
- A picture the panel shows and a document's pictures sit on
  `picture_backdrop` as thumbnails do (`backdrop`, views.md). Their ink is
  read where they are decoded, on the worker (`PreviewContent::Image`'s
  `ink`, `backdrop::Picture::of`), and weighed against what they lie on:
  the panel's `surface` for a picture, its letterbox; `bg` for a
  document's page. A page or a film is opaque (`ink: None`). The backdrop
  lies under the pixels, never the box Fit gives them.

## Models, drawings and diagrams

- A model, drawing or print shows the picture its application stored
  (`models`), read from where the format keeps it and no
  further: a Blender file's blocks before `TEST` (a compressed one
  stream-decoded that far), a package's named members, a G-code's first
  `TEXT_INPUT` and last `TAIL` bytes, a DWG's listed pictures.
- Nothing is read in 3D in-process. A mesh without a stored picture
  goes to `skwir-model-worker` (`skwir_platform::model`): it reads within
  `model::BYTES` and `model::TRIANGLES`, follows glTF buffers only below
  the model's folder, and returns a line of measures and a PNG. Without
  the helper the plugin declines the mesh, leaving it to a desktop
  thumbnailer. The sandbox inputs and helper reads share one glTF buffer
  boundary (`platform/src/model/buffer.rs`): resolve the actual model's
  parent and each buffer, and allow only regular files still beneath that
  parent. A lexical relative name alone is insufficient. Internal file
  and directory links work; outside and dangling links grant no access.
  Bind approved files under their original relative names, preserving the
  sandbox's model and buffer aliases on macOS.
- **A model of exact surfaces is meshed in the helper** (STEP and IGES,
  by ogeom, ADR-069):
  - each face keeps its own points: smooth shading averages normals at
    shared positions, which would round a CAD edge;
  - whether it is watertight, and its volume, are measured on the mesh
    the kernel welds of the same faces, never on the faces as drawn,
    whose edges meet only within the file's tolerance;
  - a face that cannot be meshed is counted (`faces_missing`) and the
    preview says so: a hole is never shown as the shape without a word;
  - its bound is a file's (`Bounds::binary_input`), checked before the
    helper runs: what the helper holds grows with the file, not with the
    triangles.
- **A model that keeps meshes of its surfaces** (Rhino) gives those
  meshes, nothing computed:
  - the helper reads it a chunk at a time, what is not a mesh, a material,
    a layer or a block passed over by its length, never read whole;
  - what Rhino hides is not drawn: hidden objects, layers off (with their
    parents), a block's members but where an instance places them;
  - colours are the rendered view's (vertices, B-rep faces, materials),
    never a layer's display colour;
  - an object shown that stores no mesh is counted (`objects_missing`)
    and said; a model with none shows its picture with the reason.
- **A window that draws meshes turns them** (ADR-067). The preview asks
  for the mesh only where the window draws them on a GPU
  (`Shell::draws_meshes`: `Window::draws_meshes`, and no software
  rasterizer, whose frame grows with the mesh while the window waits;
  `InspectRequest::meshes`). Thumbnails, an agent's preview, a window
  without a 3D pass and one drawn by lavapipe get the still.
  - The helper writes the mesh itself (`--mesh`, `Asked::Turned`) up to
    `model::VIEW_TRIANGLES`, else its still, with a notice saying why; up
    to `model::TRIANGLES` when the user asked for this file
    (`InspectRequest::whole`): 3D asks it, so a model needs no Full
    preview of its own.
  - Whether a model is turned is the user's, by type (`TurnedTypes`,
    `InspectRequest::turned`, the bar's 3D button): unchosen, a mesh is
    turned and a model that stores a picture shows it. A model that can be
    turned says so whichever is shown (`Inspection::mesh_available`), so
    the button is offered both ways. Past `VIEW_TRIANGLES` the type's
    choice does not apply; the button turns that file, unremembered.
  - What it wrote is taken only as long and as large as it says
    (`model::mesh_from`), every index naming a position and every
    coordinate finite (`inspection::Mesh::new`), a colour a position or
    none (`with_colours`), checked again on the way to the GPU
    (`gpui::Mesh::new`, `with_colors`).
  - A model's colours are its points' (sRGB bytes): the helper reads
    them, the still and the window draw them alike. A part without one
    beside parts with one is the still's grey (`mesh::grey`); a glTF
    primitive with neither a material nor `COLOR_0` has none.
  - It is an `InspectionKind::Model`, framed once on the worker
    (`Display::model`, `elements::model::Model`), never while painting.
  - It opens as its still is drawn (the angle, no perspective, the
    framing, the lights), so the thumbnail standing in gives way to it
    without a jump. A change to either is made to both
    (`elements/model.rs`, `model-worker/src/draw.rs`).
  - Each input asks for one frame; nothing is drawn otherwise, and no
    timer runs.
  - The window keeps the mesh while it shows it, the GPU while frames
    draw it: gpui lets go of its buffers in the first frame that does not
    (vendored patch 0033).
  - A mesh the GPU will not hold (larger than its buffers, or buffers it
    could not make) says so where it would be drawn
    (`MeshPainted::Refused`), never "Loading…" for good; the 3D button
    shows its still.
- A drawing made from content (`circuits`: KiCad boards,
  footprints, schematics, symbol libraries; `models`: DXF) is SVG
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

- Word and OpenDocument text become blocks in `documents`
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
- A link's fragment is a place in the file it leads to (`links::Mark`):
  once that preview arrives it shows the block holding the mark (an
  `id`, a heading's slug), else the line a code host's fragment names
  (`#L12`, `#L12-L20`); a mark the file does not hold leaves it at its
  start, and says so. A link to the document itself, named, scrolls in
  it as `#name` does. A line asked of a text read where it is shown
  waits for the rows counted, not the first rows shown.
- A link is a place to show, never a file to run.
- Every kind follows links through `Shell::follow_document_link`, a
  PDF's too (beyond links to its own pages).
- An address found in text (ADR-029) is a link: in a document read as a
  page (Markdown's bare addresses, a plain-text mail) like a written one;
  in raw text (the code view) only under Ctrl, since a click there
  selects (`skwir_platform::links::link_block`, on the conversion
  worker).
- A path a text read where it is shown writes (a string, a Markdown or
  HTML link's target, `url(…)`, a word with a slash after a name: never a
  bare word) is a link under Ctrl too, to the file it names from the
  file's folder (`links::WrittenPaths`). It is looked up only as a chunk
  is colored, after its text showed; each word once for the file, a
  bounded number of new ones per chunk, and only on this machine
  (`Here`). A member written out of its container finds none.
- A document's link is followed only where that stays on this machine
  from its folder (`RevealAt::from`, checked with the stat on a worker):
  `//host/share` on Windows connects as the user.
- In a document written out of its container (an archive's file, a
  message's attachment), a relative link names a file of the container,
  from where the document is in it: its rows' tree (depth, first cell)
  gives each row's path (`links::container_link`), and that file is
  previewed in the document's place (`sibling`), at the mark the link
  names. Never the disk around the copy written out. A link that leaves
  the container, or names a file or folder it does not preview, says so.
  A track of a playlist is a file of its own: its links are read next to
  it.
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
  absolute sizes stay the reader's: the reading font (ADR-073). A book searches with its
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
  is a link the book marks as one or sets raised (`Run::note`), or one
  whose target links back to the link's own name (`Run::name`: its `id`,
  an `a`'s `name`), as a book that sets its references as plain `[1]`
  writes its notes; a heading is never such a note. A note is
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
  (`epub::table_of_contents`: the EPUB 3 `toc` nav, else the NCX
  `navMap`), never the landmarks, the printed page list or the NCX's
  copy of the chapters, which `rbook::Epub::toc` also iterates. An
  entry that leads nowhere but is named (an EPUB 3 heading over a group
  of chapters: a part) is a row without a target, its entries nested
  under it; one with neither only wraps them, and adds no depth.
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
  `DocumentSession` (`epub/src/pages.rs`, `comics`),
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
  (`crates/plugins/markdown/src/markdown.rs`): heading level,
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
- Obsidian's links (`[[Note]]`, `[[Note#Heading|label]]`) become
  ordinary relative links when the note is read (`markdown::Wiki`, on
  the conversion worker): `Note.md`, then the name as written, beside the
  note, then from the vault's top (the folder holding `.obsidian`); each
  name looked up once, a name that climbs or is absolute nowhere, and
  only on this machine (`Here`). One found nowhere leads to `Note.md`
  beside the note. The heading is the slug the headings get; a block's
  `^id` stays as written. No search of the vault: that waits for an
  index of names that makes it free.
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
- What is read is written in the reading font (`Theme::reading_font`,
  ADR-073): the blocks of Markdown and HTML, word processors' documents,
  books and mail (`inspector::reads`, `MarkdownStyle::font`), and a note
  opened under its reference; code in them, the code view, text files
  and bytes in the code font (`Theme::code_font`); the rest of an
  inspection (tables, archives, a file's properties) in the interface's.
  A font that changes measures the rows again where they stand
  (`inspector::State::remeasure`), as a text size does.
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
  set-id bits; of attributes, only those a copy keeps), in the sandbox
  writing only where it extracts, into a staging folder put in place
  whole.
- A member's bytes that are not those stored (a CRC or an AES
  authentication that does not match, a size not the one declared) stop
  an extraction and a preview alike, a libarchive warning as well as a
  failure; encrypted, it says "wrong password or damaged archive". A
  warning on a name or a header is not a failure; one putting a member's
  metadata back is, unless the drive refuses what it does not keep.
- An archive is made (`--create`, ADR-059) of what skwir hands the
  helper on its standard input: skwir walks and reads the files
  (`compress::walk`, `archive::create`), the helper opens none, and
  writes only the archive in its staging folder. A pipe, a socket or a
  device is left out. Each file handed is the one walked (identity, size,
  time), read to its size and no further: one changed fails the step.
- A password reaches the helper through its standard input, never its
  arguments, a plan or a log: asked by the step (`StepContext::ask_secret`)
  each time it is needed, Redo included; an empty answer is asked again,
  and each member's attempts count from the first.
- Made or extracted, an archive is done already as a copy is
  (`same_tree`): the listing's names, kinds, sizes and times
  (`archive::Member::mtime`, `link`, `hardlink`), nothing more on the disk.

## The preview frame

Every preview fills one frame; none builds its own. The design and what
is still to come are in `docs/preview-frame.md`.

### Location and waiting

- **Nothing selected shows where the pane is.** A folder is previewed as
  a selected one would be (its items, Properties), named as the sidebar
  names it, with a Volume group in Properties (the volume's sidebar name,
  device, filesystem, free space). The folder is stat-ed and the volume
  measured on a worker (`Shell::here`, `VolumeFacts`), again when the
  pane shows another location or its listing completes a change; what
  was shown stays until the answer.
- A location that is not a folder (a search, the shelf, Recent) shows
  its name and count in the header and nothing below: no tools, no empty
  Properties.
- **A wait shows only once it has lasted.** The panel has one delayed
  loading signal (`Shell::sync_busy`, `preview_busy`), like the pane's
  (`loading.rs`): a request slower than `BUSY_AFTER` (200 ms) shows
  "Loading…" in the body and caption, and the bar under the toolbar; a
  quicker one shows nothing in between. Each request (generation) has
  its own grace. Nothing draws from the phase directly.
- A preview on its way is not "nothing to preview": Properties fills the
  body only when the phase is idle (`properties::alone`), never for the
  instant before a picture arrives.
- **The desktop's thumbnail is the wait's picture, not a preview.** Sent
  ahead and kept aside (`PreviewState::placeholder`), it replaces
  "Loading…" once the wait has lasted (`preview::placeholder`) and is
  never shown before, so a quick preview never flashes it. It stands
  where the preview will: an image's at the image's own size (from its
  header), so only sharpness changes; a document's where the reader
  draws page one; anything else at its proportions, as large as a
  preview may be. A preview replaces it; a file with no picture of its
  own (no preview for its type) keeps it. A film's never stands in: it
  is another frame than the one the panel shows, so the wait would end
  on a different picture. It is the film's picture only when no frame
  comes (media.md, Posters).
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

### Header and Properties

- **The header is a card** (`preview::header`): the entry's icon, its
  name (a trail inside a container) and one line of type, size and date.
  It names what the panel shows: under a container's view kept above a
  member, the container (the member's bar names the member).
- Clicking the card opens Properties. So does the (i) button beside it,
  since what the card does is not seen until tried; the button is lit
  while Properties is open, as Preview's is while its panel is. So do
  Alt+Enter, Ctrl+I and the context menu's last item: one command
  (`ui.properties`), which hides them again where they cover a preview,
  the menu's item then saying Hide Properties (ADR-058). The item's
  words are read at each frame (`menu::properties_title`), never kept
  from when the menu opened: a right-click selects the entry as the menu
  opens, before the panel shows it, and a folder on its way counts as
  shown (`Shell::properties_hide`).
- The card has no close button: the Preview button closes the panel.
  Only the peek overlay keeps one.
- **Properties** (`properties.rs`) is a section of the frame, not a kind of
  preview: everything known that the header does not say (no Name or
  Type row).
  - With nothing to preview (a folder, an unknown file, an error) it is
    always open, as tall as its content, the toolbar and the content's
    message (why the preview failed) right under it. Not while a
    container's view is kept above the member (`properties::alone`): that
    view is content, so Properties stays as the card left it.
  - Over a preview it opens on demand, under the card, with at most its
    share of the room (`properties::Room`): above the toolbar, half the
    room until dragged, less when it says less; in a room wider than
    tall (the panel at the bottom, peek), a column of that share at the
    preview's left. It scrolls past it. The edge between them is
    `elements::edge`, at the start of the preview (`properties::edged`),
    dragged each way apart and back to half on a double click. With the
    panel closed, `ui.properties` opens both. No dialog repeats it.
  - Whether it is open, its share each way and the groups folded belong
    to the window's shape (`session.rs`).
  - Each group's heading folds it (`ui.properties_fold`), remembered by
    title for every file (`properties::Folds`). How a group ships is the
    group's to say (`Group::folded`): the shell's Volume ships folded, and
    a source's group when its rows say so (`Property::folded`: the
    checksums'); the shell names no plugin's group. A folded group says
    its gist on its heading: what its rows say in short, each row's to
    give (`Row::gist`, a source's `Property::gist`), joined in their
    order once `render` has added its rows, so a count says how far it
    went. The size without the bytes, the first fact of the content,
    the notes, the URL's host, the path (cut at its start, `Gist::Path`:
    its end names the file; in the trash, where it was), the owner and
    the mode, when it changed, what More holds, the drive and its free
    room, a digest's first digits. A row that says nothing new folded
    (Device, On disk) has no gist.
  - A row's links (Change…, Edit…, Copy) show while the pointer is on
    it, over the end of its last line: hidden, they take no room, and
    nothing moves when they show. Those that are all a row says (Count,
    Compute, Add… to no tags or no comment) or that stop or start what
    runs (Stop, Count again; `Row::actions_shown`, a source's
    `Property::actions_shown`) always show, after the value on its line
    or under it when the value takes the line, which wraps: nothing is
    measured. The value grows into its line, so its text is laid out at
    the width it is given.
  - Only `preview::under_card` places it. The toolbar and the content
    (`preview::body`, what a container's split holds under its view)
    never draw it, so neither a kind nor the split can come between the
    card and Properties.
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
- Properties never starts a scan by itself: a folder says what its
  listing already counted. What it holds in all is counted when asked
  (Count, or `ui.properties`, which is the asking): `fs::count` on a
  worker, reporting as it goes into `PreviewState::count`. It is kept by
  what it counts (`Counted`: the entry shown, or the entries selected as
  a set), never by the selection's revision, so a selection made again of
  the same entries (a click on the row selected, on the background with
  nothing selected) keeps it; another entry, selection or pane lets it go,
  as does a place that shows no Properties (a search, Recent) and the
  panel closing. It is the pane's location's work
  (`Pane::location_lifetime`): going elsewhere stops it at once. An
  `ui.properties` waiting for the folder shown is dropped where no folder
  is shown, so selecting something there later is not asking. A count
  row is drawn from that state (`Show::Count`), never collected with the
  entry; a count stopped or done counts again from its link. What could
  not be read is said apart: folders that could not be listed, entries
  whose metadata could not be read (`Count::unknown`). With several
  selected, the folders among them count as folders. A click copies a
  value.
- What the file takes on its drive, its flags (`stat::flags`) and the
  drive it is on (the Volume group, kept a few seconds per device) are
  read on the fields worker with the rest, for the one entry shown.
- **Who may use it** (`properties::access_rows`, one builder for an
  entry and for a selection): owner, group, each class's level in words
  (a file's read and write, a folder's List items only, Open items,
  Create and delete items), who can run a file, the mode as chmod writes
  it (`users::mode`, `ls`'s letters) and what its special bits do; for a
  selection, in a group of their own after Selection's
  (`properties::SHARED_PERMISSIONS`, titled and folded apart from the
  Permissions of the entry under the cursor). Each
  change is a `set_access` through `ui.choose` (a folder's switch adds
  `inside`; its levels use `X`) or `ui.ask` (the mode), on the entry by
  `EntryRef` or on the pane's selection by `pane`. Nothing is offered to
  a user who does not own them all (root aside, `users::me`) or for the
  trash's files, and a row says why; a read-only drive, read on the
  worker, takes the links away (`Row::edits_access`): the entry's own
  drive for its Permissions, any of the selected entries' drives for
  theirs (`PreviewState::selection_read_only`, one look per drive,
  asked again when the drives change). The group is one
  of the user's (`users::my_groups`).
- **Plugins' rows** come from property sources
  (`Registry::property_sources`): each source's command is dispatched on
  a worker with the entry shown (`properties::plugin_rows`), again when
  the file changes or a source's revision moves, never while painting; a
  row joins the group it names, or a group of its own before the drive's.
  The shell names no plugin: Opens with is Open with's answer, the
  checksums the checksums plugin's, which says its group ships folded.
- **A source that reads a whole file** (the checksums) reads what the
  window asks in a lane of its own: one job at a time holds a worker,
  reading the file asked last for everything asked of it at once, so
  folders are still listed while a large file is read. The window names
  the file by its `EntryRef` and nothing is stat-ed on its thread; the
  job stats it, and stats it again once read: a file that changed
  meanwhile gives no checksum. Turning the plugin off cancels what runs
  and what waits for a worker alike (one token of the plugin's).

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
     reader's Continuous, or Source, lit while the file's own text is
     shown), then what is shown says itself (`Inspection::title`: the
     format's name, a page's lines);
  5. spacer, find, More;
  6. the × that leaves a member (`preview_bar::close_tool`), the same for
     every kind, also for a member that could not be written out, whose
     failure stands in its place (`PreviewState::inside`).
- An item states the narrowest of the three variants that still shows
  it (`keep`). Text and a page give way alike: a tool in the same place
  has the same `keep`.
- Copy has no button: it is Ctrl+C and the content's right-click menu
  (`Shell::open_preview_menu`), where people look for it. An action too
  rare for a button in any variant (View as hex) is `Item::menu_only`,
  but where nothing else is shown of a file that can be read (a failure,
  a file without a preview: `preview_toolbar::shows_nothing`), View as
  hex is a button: the one way left to look. A link that leads nowhere
  and a special file have none.
- `zoom_tools` is the one zoom group, for a picture and a page alike;
  text's twin is the inspector's text size picker (`text_size_panel`, a
  popup of kind `text`).
- Shared actions take their words from `preview_bar::label`.
- A button's label names what it does or shows, never the state it
  leaves: Source is Source in both states, lit while on. A provider
  offers it (`Inspection::source_available`) only where the file's own
  text differs from what is shown: YAML, TOML and JSON shown as written
  have none. A picture written as text (an SVG: its type descends from
  text and so do its bytes, `preview::picture_written`, decided on the
  worker, `PreviewResult::picture_source`) offers it in the picture's
  bar, kept by type as any other.
- **A presentation that fails on text shows its source**
  (`preview::source_instead`): the file's own text, read where it is
  shown, its notice "Shown as written: " and why, and no Source, since
  there is nothing to switch back to. Every failure of the worker for
  the file itself goes there: a provider's inspection
  (`inspect_shown`), a conversion by a plugin from outside, the player's
  or a reader's opening, a document's first page, a picture drawn or
  decoded. Not a file whose bytes are not text (it says its failure,
  with View as hex), a way of looking chosen (bytes), a member a
  provider shows itself (`InspectRequest::target`), a page turned to
  (the reader keeps the last), nor a cancel. A provider fails rather
  than building a fallback of its own; the source, never a converted
  file's, is read from the file's own path.
- **Source is kept by type** (`preview::SourceTypes`, part of the
  window's shape): turned on or off for one file, it is so for every file
  of that type, by its database name (`mime::unalias`), never for all
  text at once (an email's source is its raw message, a book's its
  chapter's markup). The worker decides it, where the type is settled
  and before a plugin from outside converts the file, and the panel
  takes what it decided (`State::arrived`); what another way of looking
  chose (bytes) stands. The step is `preview::inspect_shown`, which the
  tests call too: a provider that leaves the source to the shared code
  view gets it only for a file that is text, so a file of the type that
  is not is asked again as it shows.
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
- **A find marks what it finds** in a structured preview's text, while
  its field is open: every match of what it looks for in the text drawn
  (`Theme::find_match`), the one it is on more strongly (`find_current`),
  drawn with the text (`inspector_text::Sought`), never by measuring it
  in `render`. Finding and marking match alike, case ignored a character
  at a time, at the text's own offsets (`text_rows::matches`). A find
  goes from the match it is on, past it, unless the query is new
  (`State::find_from`): from match to match, several in a block or a row
  included.
- **Contents** is one drawer (`outline.rs`) for a document's outline, a
  file's headings and a book's chapters, offered where there are some.
  - **What it lists**: a book's own contents, never the headings of the
    chapter shown above them, which named its chapters twice. The
    headings of the section shown that its rows do not name stand under
    the row they fall under (`inspector::book_entries`), the one the
    highlight would mark there; a heading is named by a row when it
    stands where the row points or its words hold the row's, or are held
    by them (`named_headings`, within the row's part of the section). A
    file without contents of its own lists its headings. Headings are
    indented by the headings before them they fall under
    (`inspector::heading_depths`), never by their level, so a level
    skipped is one step in. An entry without a target is a label.
  - **Open until closed** (its button, Contents, `ui.preview_contents`):
    a new section of the file (a book's next chapter) keeps it open; a
    new file closes it, but while pinned.
  - **Pinned** (`reading.contents_pinned`, its pin, `ui.contents_pin`):
    a column beside the content, which narrows to make room
    (`outline::around`), open from file to file where there are contents
    (`Shell::contents_kept`); choosing an entry keeps it open. Unpinned,
    a drawer over the content's left that choosing closes.
  - **The part shown is highlighted** in the selection's colors, the
    keys' place a ring apart, and brought into view as it changes, never
    against the user's own scrolling of the list (`outline::Follow`). It
    is read from what is shown, never measured: a document's, the last
    entry at or before the page the reader counts (`outline::at_page`);
    a book's, its row for the section shown, the last whose fragment is
    at or above the top of the view (`Row::section`, set by the book's
    provider), else the last row before the section; else the last
    heading at or above the top of the view.
- What a preview says about itself (how it is shown, what was left out)
  is one strip under the toolbar, never a grey sentence in the content.
- **Keys** mean the same wherever they apply: Ctrl+F, Ctrl+C, Ctrl++ and
  Ctrl+- (zoom what is looked at, size what is read), Ctrl+0, `[` and
  `]` for the previous and next section or page. A picture takes them
  once clicked.
- **The arrows** move what is shown, as a document reader's
  (`ui.preview_scroll`, bound on the frame, `preview`, and on a picture,
  `image`): Up and Down a tenth of the view (`preview::arrow_step`; rows
  of text and bytes a row), and a page read a page at a time turns at
  its edge, as under the wheel, never while a turn is on its way. Left
  and Right go sideways where what is shown is wider than its box (an
  edge reached stops them), else step as on the whole screen
  (`fullscreen_step`), but a picture in the panel: its step moves the
  pane's selection, which gives the keys back to the pane
  (`cancel_preview`). What has arrows of its own keeps them by being
  deeper: an archive's files, a map, a player, rows of text up and down.
  Page Up, Page Down, Ctrl+Home and Ctrl+End scroll every structured
  preview (`ui.inspect`'s `scroll`, on the side with the keyboard); in
  the reader the page keys turn the page.
- **Ctrl and the wheel** take the zoom keys' step over what is under the
  pointer, unclicked: a picture's or a page's zoom, an inspection's text
  size (the container's view kept above a member too), the pane's view
  (`elements::zoom_wheel`, `Zoomed`). One overlay over what scrolls,
  listening in the capture phase: GPUI's scrolling reads no modifier, so
  a listener on an ancestor heard the wheel after the picture had
  scrolled. A trackpad must mean it (`WheelTurn`). A map has none, its
  wheel zooming around the pointer with Ctrl or without; a film has no
  zoom keys, so no zoom.
- **A picture or a page zooms around the pointer** under Ctrl and the
  wheel (`Shell::preview_zoom_toward`): what is under it stays there, as
  far as what is shown can scroll (a picture shorter than its view sits
  in its middle, a page at a zoom at its corner). Their layout is known
  (`preview::Lay`), so the new offset is worked out on the wheel's turn
  from the view's bounds of the last prepaint, which a zoom does not
  change. Read continuously, row heights change with the zoom: the page
  under the pointer and the share of it there wait for the layout at the
  new zoom (`reader_scroll::ZoomAnchor`, `landing`, in prepaint). The
  zoom keys keep their place.
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
- A picture larger than the panel scrolls both ways from its top-left
  corner, a bar on each edge; smaller, it sits in the middle. Its wrapper
  starts at the left rather than stretching across the panel's column: a
  stretched one keeps the panel's width, and the picture spilled out of it
  on both sides, out of reach of the scroll.
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

### The whole screen

- **One fullscreen for every preview** (`fullscreen.rs`,
  `ui.preview_fullscreen`): a picture, a document's pages, text, a film,
  music (`Shell::fullscreen_shows`); a folder, or a file with no preview,
  has none. It takes peek's place (`render_peek`, `peek` set): the panel
  is then only a picture of the preview. It stays while the file shown
  changes (a step, a slideshow, the next track) and leaves when the
  selection goes or reaches a folder (`sync_preview`). Its state is the
  preview's (`PreviewState::fullscreen`), not a kind's: resetting a
  player or a reader never ends it.
- **Drawn from the frame's own pieces** (`preview::frame`, `controls`,
  `content_area`, `content`), so every kind is there with its keys and
  tools and nothing is drawn twice. Only a film has a picture of its
  own there (`media::film`: enlarged to the screen, a player's clicks).
  What is looked at lies on `stage` (black in both looks), what is read on
  paper; a picture or a film on its way waits on its letterbox
  (`on_paper`), so stepping through them never flashes paper.
- **The bar** is the kind's own, at the top, starting with the name of
  what is shown. Every kind's bar ends with Fullscreen
  (`preview_bar::fullscreen_tool`), lit while on and there while the
  next file loads. Above pages and text, which are read, it stays, and
  what is shown starts under it: it never covers a first line or a
  notice. Over what is looked at, a picture, a film or music, which keep
  the whole screen, it lies over the top and hides
  (`Shell::fullscreen_bar_floats`; a file on its way counts as what its
  type shows, so a step or the next track moves nothing): on one
  cancellable inactivity timer (`arm_fullscreen_hide`), never a loop; a
  stationary pointer event from layout does not reset it; hover, an open
  popup, a knob dragged and the keyboard in the bar keep it. A floating
  bar hidden takes the pointer with it: a layer over the canvas asks for
  no cursor (`CursorStyle::None`), over whatever asks for another (a
  film's hand), taking no click, wheel or drag; a cursor style is the
  window's, so a pointer on another screen keeps its own. C hides or
  shows either bar; the pointer moving brings it back, and the pointer.
- **Keys.** Entering gives the keyboard to what is shown (the picture's,
  the reader's, the inspection's, the player's own handle,
  `shown_focus`), so its keys work at once, and a preview arriving while
  there takes it from the canvas (`fullscreen_arrived`); leaving gives
  it back to what had it before. F toggles wherever a preview has the
  keyboard (`preview`, `image`, `media`, `peek`, `fullscreen`), Escape
  and F leave, C hides the bar, ←/→ step: what is shown takes the
  arrows first (`ui.preview_scroll`, deeper), moves sideways where it is
  wider than the screen and steps where it fits; `ui.fullscreen_step`,
  bound on the canvas, answers where nothing shown holds the keyboard.
- **Pictures walk the pane's pictures**, by type
  (`fullscreen::is_picture`, `mime::Kind::Image`, told once with the type,
  `Info::picture`), found around the entry as a track's neighbours are
  (`Shell::neighbours`, when the listing or the entry changes); a step
  moves the pane's selection and the preview follows. Other files are
  skipped, and so is a file whose name several types claim
  (`mime::ambiguous`): the walk reads no bytes, and a `.djvu` may be a
  book, whose pages would then keep the arrows.
- **The next picture is read ahead** the way the last step went, at the
  side the screen covers (`screen_side`), as in the panel (Pictures,
  above).
- A picture shown as text (its source, its bytes) is still one of the
  pane's pictures: the inspection's bar keeps the walk's tools
  (`preview_toolbar::picture_tools`), and the slideshow goes on.
- **The slideshow** (`ui.slideshow`, Space on a picture, More ›
  Slideshow in the panel) goes from the picture shown to the last, then
  stops there, says so and shows its bar. One timer per picture, armed
  once the picture has arrived or failed (`arm_slideshow`), so a slow
  decode is shown its whole time; a step, Pause or leaving cancels it,
  and the timer checks the entry it was armed for. Its pace is
  `reading.slideshow`, chosen on the Reading page or from its bar
  (`ui.slideshow_pace`).
- What answers the user (`set_status`'s toasts) is drawn over the canvas
  while it shows (`Shell::render`).
- **The screen stays awake** while a slideshow or a film plays, the
  panel's or the pinned one (`Shell::sync_awake`, told on each wake,
  which the window ignores when nothing changed;
  `Window::set_idle_inhibited`, vendored patch 0023). Music, a paused
  film and anything at rest let it sleep.

### Maps

- A map (`map_preview.rs`, `elements/map.rs`, ADR-020) is an inspection
  (`InspectionKind::Map`), so Properties, the switch to the source and the
  way out of a container are the frame's. Its places are projected once
  on the worker (`Display::map`), never while painting.
- The pointer drags it; the wheel (no Ctrl: nothing scrolls) and a
  double click zoom around the pointer. Once clicked (or reached with
  F6), the arrows move it and the zoom keys zoom it (`MapPreview`).
  Ctrl+0 and Fit fit it to its places at whatever size it is drawn.
- PageDown and PageUp go from place to place round the ends
  (`ui.map_point`, `Handles::point`, `Shapes::step`), the map centred on
  each at its zoom and the place ringed; a screen reader reads the
  places as a list and hears each as it is reached
  (`Shapes::read_points`). The Map view goes the same way through its
  files' places (`PaneView::nav`), selecting the file as a click on it
  does: Enter opens it, Space peeks at it.
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

### Models

- A 3D model (`model_preview.rs`, `elements/model.rs`, ADR-067) is an
  inspection (`InspectionKind::Model`), so Properties and the way out of a
  container are the frame's. No Find, no Source.
- A drag turns it around its up axis and tilts it (to straight above or
  below, no further); Shift and a drag, or the middle button, move it;
  the wheel zooms around the pointer. Once clicked (or reached with F6),
  the arrows go round it by 15°, Shift and the arrows move the view, the
  zoom keys zoom it (`ModelPreview`). Ctrl+0 and Fit show it as it
  opened.
- The view belongs to the file: kept when it is read again, as it opened
  for another.
- How it is drawn is the user's, for every model: Perspective and Smooth
  shading (`reading.model_*`, `ui.model_perspective`, `ui.model_smooth`,
  the bar's last buttons). Perspective draws the plane through the
  model's middle at the scale it has without, so switching keeps its size.
  Smooth shading asks gpui for normals, made once a frame asks for them;
  it draws flat meanwhile.

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
  sideways scroller; so are the bars of what scrolls sideways (a table,
  bytes), kept in view: the vertical one on the right edge, the
  horizontal one along the bottom of the rows.
- **Bytes are read where they are shown** (`bytes_view.rs`, ADR-051). A
  row is `HEX_ROW` bytes, so row `r` is at `r × HEX_ROW` and the file's
  length (`Display::bytes`, read on the worker) says how many there are.
  The rows stay the bytes view's (`PreviewProvider::shows_bytes`, a
  plugin from outside too): each page around the rows laid out is asked
  of it by name at its offset, one at a time, the one the view left
  cancelled; a page holds as many rows as its first answer. Find asks it
  to search from a byte and goes to the row it lands on, showing what it
  says; Jump goes to a row. No page is turned.
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
  Properties, and that click must not leave the member.
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
  - Properties stays under the card, above both (`preview::under_card`):
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
  the code view (`inspection::plain`): the language from the name, a
  gutter, selection, find and copy. A rendered format's source is the
  same view (`inspection::source`). There is no second, poorer text
  renderer in the UI: the code view's rows (`code_preview::CodeRow`)
  draw both it and structured code.
- **Text is read where it is shown** (ADR-051). `plain` and `source`
  carry the file (`Inspection::text`) and its first rows, never the
  whole text, and no page:
  - Rows (`platform::text_rows`): a row ends with its line or after
    `LINE_BYTES` at a character boundary. Counting and reading cut rows
    with one cutter, so they agree. The encoding is decided once, from
    the file's first bytes; UTF-16 is cut in its own units.
  - A worker counts the rows into an index of every `STRIDE`th
    (`text_rows::count`), published a block at a time; the view shows
    what is counted, and the bar the lines so far ("…" until done).
  - The view (`text_view.rs`) asks for the chunks around the rows laid
    out, nearest first, one at a time (`Latest`), never per row or per
    frame; a bounded cache keeps those near the view. A chunk is read
    plain first, then colored: from the state the one before it stopped
    in, else fresh, and again once that state is known (`Chunk::exact`).
    Nothing waits on colors to show text.
  - The element (`elements/rows.rs`) scrolls by row and lays out only the
    rows on screen, in prepaint; it keeps nothing per row. After
    Ctrl+End it follows the end until moved up, a file written to
    included. Lines wrap or not (`reading.wrap_lines`); unwrapped, a row
    moves its text and keeps its gutter (`CodeRow::shift`): moved by
    position, never by a margin, which would lay the row out shorter the
    further it is scrolled, and the rows would stop short of its end.
  - Find, copy and a line a link names (`file:12`) read the file on a
    worker (`text_rows::find`, `text`, `row_of_line`): never only what is
    loaded. Copy stops at `text_view::COPY_BYTES` and says so.
  - Find lands on a match, a row and where it is in it. The rows bring
    it into view where its text says it drew it once placed
    (`RowScroll::reveal_spot`, `Rows::seen`): along a line that does not
    wrap, down a row that wraps taller than the view, out from under the
    find field (`Rows::covered`, `Field::drawn`), in the next frame.
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
- **Grammars are matched by Ferroni and let go of** (ADR-060). syntect
  and two-face are vendored for it; Oniguruma's grammar set, all of
  bat's. Take the set from `code_preview::syntaxes` when coloring
  starts and hold it only while coloring (`Highlighter`) or while a
  state may be resumed (`Resume`, whose parser state indexes into it):
  never in a static or a long-lived struct, which would keep every
  grammar compiled for good. Unused for `GRAMMARS_KEPT`, it is let go
  of and the workers that compiled it release.
