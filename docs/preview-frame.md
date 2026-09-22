# One preview frame

Status: **the six agreed phases are implemented** (2026-09-21). Their rules
are in `docs/invariants/previews.md`, "The preview frame", which is the binding
text; this document keeps the reasoning, what was left out, and the proposal
at the end, which is still only a proposal.

Left out, deliberately or for want of means:

- EXIF for pictures: no metadata crate is in the dependency tree; Details shows
  a picture's dimensions.
- A folder's total size from the Usage cache: the cache belongs to a plugin the
  UI cannot depend on. Details shows what the listing counted.
- A hit counter in the structured previews' find field: providers search as
  they go and do not know the total. The field, Enter and F3 are the reader's.
- A text-size slider for fonts and a selection mirrored between a byte view's
  two columns. The sheet is drawn larger instead, and the selected row is
  decoded.
- `InspectionKind::Tree`, which nothing produced, is removed.

The PDF reader, the media player and the image viewer are good. The structured
previews (hex, archives, code, tables, Markdown, EPUB, mail, fonts) and the
fallbacks (plain text, folder, no preview) were built separately and show it.
This design makes one frame that every kind fills, instead of four that
resemble each other.

## What is wrong today

Found by looking at all fourteen kinds in a window and reading the code.

- **Grey paper.** Every `Inspection` kind draws on `theme.surface` (#f0f0f0),
  greyer than the file list beside it (`bg`, #fafafa). On top of that, primary
  data is muted: hex offsets and `00` bytes (`inspector.rs`, hex cells), archive
  sizes (`archive_preview.rs`), everything but the name in the no-preview card
  (`preview.rs::generic`). `PreviewContent::Text` hard-codes two base16 colours
  instead of tokens.
- **Four toolbars.** `inspector::toolbar_row`, `reader::reader_row`,
  `media::row`, `preview_toolbar::row`. The reader packs left with no spacer.
  More is always present in two and narrow-only in two. Open externally is a
  bar button in two and menu-only in two.
- **Redundancy.** The inspector's menu repeats every entry of `tools()`.
  Reader: outline, find, copy, open. Image: all of zoom. Media: mute,
  fullscreen.
- **One action, several faces.** Hex is icon `list` in `preview_toolbar` and
  `grid` in the inspector. "Fit whole image" / "Fit whole page" / "Fit image" /
  "Fit page"; "Fit image width" / "Fit page width" / "Fit width". Outline is
  "Document contents", "Contents" and "Headings and contents", a chip row in
  the inspector and a drawer in the reader. Find has three implementations
  (inspector prompt, reader popover, hex provider).
- **Two tiers of text.** JSON, YAML and TOML get a gutter, folds, selection,
  find and copy. Every other source file goes through `PreviewContent::Text`:
  a bare list with none of them. An `.html` file shows its source although an
  inert reading mode exists (`rich_preview::html`, used by EPUB and mail).
- **Smaller.** The folder card says its item count twice in an empty panel and
  shows neither size nor date. A selected archive member keeps the archive's
  header and never names the member. CSV clips columns and leaves a gap under
  its header. Images have no zoom key; Text, Dir, Generic and Error have no key
  context. `InspectionKind::Tree` has no producer.

## The frame

1. **Surfaces.** What you read (text, code, tables, hex, trees, prose) is on
   `bg` in `text`. `surface` is chrome: header, toolbar, and the letterbox
   around images, video and document pages, which stay on it. Muted is for
   secondary facts (line numbers, captions), never for data. New tokens
   `code_bg` and `zebra`; nothing derives a colour locally.
2. **Header.** Name, then type, size and date. A nested target is a trail,
   `bundle.zip › sub › data.json`, and each step goes back to it.
3. **One toolbar builder, fixed slots, in this order:**
   navigate (back/parent, previous/next, page) · view (zoom or text size, fit,
   mode toggles: continuous, source/rendered, hex) · spacer · find · copy ·
   outline · More. A kind declares which slots it fills; it never builds a row.
   One icon and one label per action: "Fit", "Fit width", "Contents",
   "View as hex".
4. **The menu rule** is the application toolbar's: More holds only what has no
   button in the chosen width variant. Open externally, Reveal and Copy path
   live there for every kind, left-aligned. More is always present, so it never
   moves.
5. **Shared behaviour.** One find bar (the reader's popover: hit counter, F3
   and Shift+F3). One outline drawer. One copy rule: the selection, else the
   focused block. One `Preview` key context: Ctrl+F, Ctrl+C, Ctrl++ / Ctrl+-,
   Ctrl+0, Ctrl+wheel, `[` and `]` for previous and next.
6. **One text path.** `PreviewContent::Text` goes away: all text is the
   inspector's Code kind, so every source file gets the gutter, selection, find
   and copy. HTML opens in reading mode with a Source toggle, like Markdown.
7. **States.** Loading, empty, error and "too large, showing the first N" share
   one notice strip under the toolbar, replacing grey sentences in the content.

## Details: what we know about the entry

A section of the frame, not a preview kind.

- **Placement (decided, revised):** the header is a card (icon, name, type ·
  size · date) and clicking it, or Ctrl+I, opens Details right under the
  card, up to half the panel; the bottom bar it first had repeated the
  header. Details fills the body when there is no preview (including folders
  and several entries selected). Its state belongs to `session.rs`.
- **Groups:** General · Location (path, link target) · Permissions (owner,
  group, mode in words) · Times (modified, created, accessed) · Tags · then
  what the format knows.
- Core already carries `mode`, `uid`, `gid`, `nlink`, `created`, `accessed`,
  `link_target` (`fs/stat.rs`, provenance Filesystem) and `tags`; none is
  shown. The MIME shown today is guessed from the name.
- Format facts need preview plugins to publish metadata fields, with
  provenance, in batches on workers (`MetadataProvider`; only `tags` registers
  one today): image dimensions and EXIF, audio tags (now only in the live
  `MediaState`), document properties from the document session, EPUB title and
  author, archive totals, font family and version.
- Folders: item count, breakdown by kind, and the total size only when the
  Usage cache already has it. Details never starts a scan.
- Values can be selected; a click copies one.

## Archive members as real previews

Providers take a path and nothing else (`PreviewProvider`, `PreviewRequest`
holds an `EntryRef`). A member is written out by `PreviewProvider::extract`
(`bibli-archive-worker` for an archive, at most 16 MiB; the parsed message
for an email attachment) and the worker pipeline previews that file like any
other. Containers never inspect a member themselves: there is no second,
poorer member view.

- Keep the extracted file for as long as the preview's generation lives, and
  dispatch the registry again on a nested `PreviewRequest`: the temporary path,
  the member's display name, the MIME from that name, and the parent it came
  from. JSON gets its tree, Markdown its reading mode, a PDF its reader, a
  picture its zoom. The header trail is the way back.
- **Decided:** media and document sessions are allowed on a member, although
  they keep the temporary file open for their lifetime.
- Bounds: the existing size limits; nesting at most two deep; encrypted members
  and links still refused; cleanup off the UI thread; member names stay labels,
  never extraction paths; nothing opens a member externally from the temporary
  path, only an explicit Extract.

## Proposed, not agreed: capabilities instead of kinds

This part was written from a misreading of a request about the panel's width.
It is kept as an idea, is not part of the agreed plan or of the order below,
and needs a decision of its own before anyone builds it.

Adding a format today means fitting one of eleven `InspectionKind`s or editing
core and the UI: `inspector.rs` switches on the kind forty times (which tools
exist, which style, which row estimate), the toolbar's actions are a fixed list
inside `ui.inspect`, and the only alternatives to a provider's presentation are
the hard-coded Hex and Source buttons. The first provider that accepts a file
owns it. Images, plain text, folders and "no preview" are not providers at all:
they live in `preview::compute` and nothing can replace them. Plugins stay free
of GPUI (ADR-001, ADR-007); the way to open the panel up is to let them say more
as data, and to make the UI ask what a preview can do rather than what it is.

1. **Layouts and features instead of kinds.** An inspection names the layout
   that draws it, from the small set the UI owns (prose, code, rows, table,
   bytes, image), and the features it supports: find, folds, outline, text
   size, sections, source. The toolbar builder and the key context read the
   features. A new format picks a layout and needs no UI change; a new layout
   is one registration in `bibli-ui`, resolved like `views::named_renderer`,
   not a match arm in five files.
2. **Actions are contributed.** A provider lists actions for the file it
   shows: id, title, icon, toolbar slot and a command invocation that receives
   the preview's context (entry, target, selection). The frame's builder places
   them; what has no room goes to More. Archives offer "Extract here", fonts
   "Install", images "Rotate" or "Set as wallpaper", JSON "Copy path to value".
   They are commands, so the palette, the CLI and agents get them too.
3. **Presentations, chosen by the user.** Every provider that accepts a file
   offers its presentations (SVG: picture, source, bytes; HTML: reading,
   source; JSON: tree, code; CSV: table, source). One "View as" control in the
   view slot replaces the Hex, Source and Default buttons. The highest priority
   is the default, and the last choice is remembered per type.
4. **Providers declare what they handle** as data (types, extensions,
   priority, presentations) beside `accepts`, so Settings can show which plugin
   previews what and let the user settle a conflict, and the type is sniffed
   from content when the name says nothing.
5. **A source, not a path.** The request says where the bytes are: a file, or
   a member extracted from a parent, with its display name. This is the nested
   preview of the previous section, stated once for every provider.
6. **The fallbacks become providers.** Image, text, folder and generic move
   out of `preview::compute` into a built-in plugin with the lowest priority.
   Everything then takes one path, and a plugin can replace the folder card (a
   repository summary, a contact sheet for an album).
7. **Anyone can add to Details,** not only the plugin that draws the preview:
   fields with a group, a label, a value and their provenance, produced in
   batches on workers. Some are on demand ("Compute SHA-256"): a field may be
   an action until it has a value.
8. **Several entries, and virtual ones.** The request carries the selection,
   so a provider can summarize many files, and entries of a query or a shelf
   resolve through `OpenLocation::path_of`.
9. **Readable without a window.** `preview.describe` returns the inspection as
   JSON through the same providers and bounds, for the CLI, the MCP server and
   tests. A conformance test in `crates/bibli/tests/` runs every provider over
   fixtures and checks the contract: bounds, fold nesting, actions that resolve
   to registered commands, no trace after disable.

Custom GPUI content from a plugin stays out, as ADR-007 decided for views, until
a third party needs it; items 1 and 2 are the seam it would use. This changes
the provider contract of ADR-003 and ADR-009, so it needs an ADR of its own
before the first phase that touches core.

## More

- **Tables:** sticky header, grid lines, numbers right-aligned, horizontal
  scroll, a rows × columns caption, sort by a click on a header.
- **Hex:** offsets in normal text, the selection mirrored between bytes and
  ASCII, a strip decoding the bytes under the cursor (u8/u16/u32, float,
  UTF-8).
- **Fonts:** specimen on paper at a readable size, with a text-size slider.
- **Peek** (Space) uses the same frame and gets all of this.
- Remove `InspectionKind::Tree`, or put JSON on it.

## Order

1. The frame contract in `docs/invariants/previews.md`; surfaces and tokens.
2. The toolbar builder and the menu rule; one find, one outline, one key
   context.
3. One text path; HTML reading mode.
4. Details: core fields first, then plugin-contributed fields.
5. Nested previews for archive members and mail attachments.
6. Tables, hex, fonts, the folder card.
