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
  Markdown images must stay within the document directory after canonicalizing;
  EPUB resources stay inside their container and email images use content IDs.
- Markdown parses parser events directly into semantic blocks
  (`crates/plugins/preview-markdown/src/markdown.rs`): heading level, list depth
  and marker, quote and table cells stay in `inspection::Block` instead of being
  flattened to HTML and scraped back. Raw HTML blocks are the exception, and are
  read by `rich_preview::html` where they stand: dropping them loses a README's
  logo and centered headers. Every image in a document, Markdown's and HTML's,
  shares the one `EMBEDDED_IMAGES` budget and the one document-local loader.
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
  maxima and paddings applied in layout, never a measured width.
- Only `bibli-archive-worker` links libarchive. Member names are labels/keys,
  never extraction paths. Read only selected regular members, bound the actual
  decompressed bytes and keep temporary cleanup off the UI thread.

## The preview frame

Every preview fills one frame; none builds its own. The design and what is
still to come are in `docs/preview-frame.md`.

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
  previous and next, page) · view (how the content is shown: the format's own
  name is the switch between rendered and source, then zoom or text size, fit)
  · spacer · find · More. Copy has no button: it is Ctrl+C and the content's
  right-click menu (`Shell::open_preview_menu`), where people look for it. An
  action too rare for a button in any variant, like View as hex, is
  `Item::menu_only`.
  `zoom_tools` is the one zoom group, for a picture and a page alike. Shared
  actions take their words from `preview_bar::label`. An item says the
  narrowest of the three variants that still shows it (`keep`).
- **The menu rule** is the application toolbar's: More holds the buttons the
  chosen variant dropped (`PopoverAnchor::popup_for`), a kind's settings that
  never have a button (volume, speed, tracks), and what no kind has a button
  for (open externally, copy the path). Nothing is both a button and a menu
  row, and More is always last, so it never moves. A popup of another tool
  (the zoom picker, the find field) is anchored to that tool, or to More when
  the variant dropped it (`Item::opens`, `preview::active_popup`).
- **One of each.** Find is a field under its button with Enter and F3, in the
  reader and in structured previews alike (`DocumentFind`, `InspectorFind`).
  Contents is one drawer (`outline.rs`) for a document's outline, a file's
  headings and a book's chapters. What a preview says about itself (how it is
  shown, what was left out) is one strip under the toolbar, never a grey
  sentence inside the content. The keys are the same everywhere they mean
  something: Ctrl+F, Ctrl+C, Ctrl++ / Ctrl+- (zoom what is looked at, size what
  is read), Ctrl+0, `[` and `]` for the previous and next section or page; a
  picture takes them, and Ctrl+wheel, once clicked.
- **One text path.** Any text file without a presentation of its own is the
  code view (`inspection::plain`): a block per line, the language from the
  name, a gutter, selection, find and copy. A rendered format's source is the
  same view (`inspection::source`). Both are paged and a page ends on a line;
  text the code view's bounds do not hold (one endless line) stays a single
  block of plain text. There is no second, poorer text renderer in the UI.
  HTML files open in the inert reading mode, like Markdown, with their source
  one toggle away.
- **Details** (`details.rs`) is a section of the frame, not a kind of
  preview: everything known about what the panel shows. With nothing to
  preview (a folder, an unknown file, an error) it fills the panel under the
  entry's icon; under a preview it is one bar at the bottom that opens on
  demand, and whether it is open belongs to the window's shape (`session.rs`).
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
  shows them itself.
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
  text, like Markdown's; rules; and the one piece of a page's own styling that
  is structure enough to keep, what is centred (`Block::centered`: a title
  page, a logo), plus bold and italic from an element's inline `style`. A
  publisher's stylesheet is not applied and nothing is executed or fetched. A
  book reads in the same typography as a document (the reading profile).
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
