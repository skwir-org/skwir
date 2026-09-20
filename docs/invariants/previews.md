# Structured preview invariants

Read this before touching `crates/core/src/inspection.rs`, `crates/platform/src/{archive,inspection,rich_preview}.rs`, `crates/archive-worker`, any `crates/plugins/preview-*` crate or `crates/ui/src/inspector*.rs` / `code_preview.rs` / `markdown.rs`. It is binding, like `AGENTS.md`.

- New format plugins publish `inspection::Inspection` from scheduler workers;
  use the existing preview registry and owner lifetimes. Fallback providers have
  lower priority than typed providers, including after disable/re-enable.
- Keep rich text, table/tree rows and images toolkit-independent. The UI owns
  virtualized layout and commands. Bound input, expansion and visible work;
  a short file can still contain one huge field or deeply nested structure.
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
  Reading mode also caps the text column at `MarkdownStyle::measure` and
  centres it: a maximum, applied in layout, never a measured width.
- Only `bibli-archive-worker` links libarchive. Member names are labels/keys,
  never extraction paths. Read only selected regular members, bound the actual
  decompressed bytes and keep temporary cleanup off the UI thread.

## Structured code previews

- Code inspections contain one bounded logical line per block. Fold ranges refer
  to those block indices; do not pass them through `split_blocks` or change the
  line order after deriving folds. Ranges must be nested or disjoint.
- Derive folds from parser spans/events, not indentation or delimiters inside
  strings. Preserve TOML/YAML comments and syntax. Highlight complete documents
  on a worker with parser state retained between lines; rendering only consumes
  prepared runs and visible indices. Copy must retain empty selected lines.
