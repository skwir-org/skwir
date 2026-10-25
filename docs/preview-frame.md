# One preview frame

Status: **the six agreed phases are implemented** (2026-09-21). The
binding rules are in `docs/invariants/previews.md`, "The preview frame".
This record keeps why the frame exists, the decisions behind it, what
was left out or is still not done, and a proposal that is **not
agreed**.

## Why

The PDF reader, the media player and the image viewer were good. The
structured previews (hex, archives, code, tables, Markdown, EPUB, mail,
fonts) and the fallbacks (plain text, folder, no preview) were built
separately, and looking at all fourteen kinds side by side showed it:

- **Grey paper.** Inspections drew on `theme.surface`, greyer than the
  file list, and primary data (hex offsets, `00` bytes, archive sizes)
  was muted.
- **Four toolbars**, each placing More and Open externally its own way,
  with menus that repeated the buttons.
- **One action, several faces.** Hex had two icons, Fit and Fit width
  seven labels, the outline three names and two forms, find three
  implementations.
- **Two tiers of text.** JSON, YAML and TOML had a gutter, folds,
  selection, find and copy; other source files were a bare list, and
  `.html` showed its source although an inert reading mode existed.

So: one frame that every kind fills, instead of four that resemble each
other.

## Decisions

Surfaces, the header card, one text path and the notice strip are as
the invariants state. Decisions that are not visible there, or that
changed during the build:

- **Toolbar.** One builder with fixed slots; a kind declares the slots
  it fills. One icon and one label per action ("Fit", "Fit width",
  "Contents", "View as hex"). The draft order was navigate · view ·
  spacer · find · copy · outline · More. As built, Contents comes first
  (its drawer opens on that side), Copy has no button (Ctrl+C and the
  context menu), hex is in More, and the format's name is the source
  switch.
- **Menu rule.** More holds only what has no button in the chosen width
  variant. The draft kept More always present so it never moved; as
  built, it appears only when it holds something.
- **Copy** takes the selection, else the focused block.
- **Properties placement (revised).** Clicking the header card, or Ctrl+I,
  opens Properties under the card. A bottom bar came first and was dropped:
  it repeated the header. With no preview (folders, several entries
  selected) Properties is always open.
- **Properties groups.** General · Location (path, link target) ·
  Permissions (owner, group, mode in words) · Times (modified, changed,
  created, accessed) · Tags · then what the format knows. The draft had
  preview plugins publish format facts as metadata fields
  (`MetadataProvider`); as built, a provider fills `Inspection::details`
  from its own parse.
- **Archive members are real previews.** A member is written out
  (`PreviewProvider::extract`: `skwir-archive-worker` for an archive, at
  most 16 MiB; the parsed message for an attachment) and previewed like
  any file. Containers never inspect a member themselves: there is no
  second, poorer member view. Nesting is at most two deep; encrypted
  members and links are refused; only an explicit Extract writes a
  member anywhere else (ADR-048 made that a step of its own).
- **Decided:** media and document sessions may run on a member, though
  they keep its temporary file open for their lifetime.

## Left out

- A folder's total size from the Usage cache: the cache belongs to a
  plugin the UI cannot depend on. Properties shows what the listing
  counted.
- A hit counter in the structured previews' find field: their providers
  search as they go and do not know the total. The field says "Enter for
  the next match".
- A text-size slider for fonts: the sheet is drawn larger instead.
- A selection mirrored between the byte view's two columns: the
  selected row is decoded in a strip instead.
- `InspectionKind::Tree`, which nothing produced: removed.
- EXIF was first left out for want of a metadata crate; it was added the
  same day (`skwir_platform::exif`, kamadak-exif) and shows in Properties.

## The planned polish, as of 2026-10-11

All six phases of the original order are done. The polish list, checked
in `crates/ui/src/` and the table plugin:

| Item | State |
|---|---|
| Tables: header above the scrolling rows, grid lines, numbers right-aligned, horizontal scroll, sort by a header click | done (`inspector.rs`) |
| Tables: a rows × columns caption | not a caption: rows and columns are Properties facts (`tables`) |
| Hex: offsets in normal text, a strip decoding the selected row | done |
| Hex: the selection mirrored between bytes and text | not done (left out) |
| Fonts: specimen on paper, drawn for the panel's width | done |
| Fonts: a text-size slider | not done (left out) |
| Peek (Space) uses the same frame | done (`preview::render_peek` draws `header` and `under_card`) |
| Remove `InspectionKind::Tree` | done |

## Proposed, not agreed: capabilities instead of kinds

**Not agreed.** This came from a misreading of a request about the
panel's width. It is kept as an idea, outside the agreed plan. It
changes the provider contract of ADR-003 and ADR-009, so it needs an ADR
of its own before any phase that touches core.

The problem: adding a format means fitting one of the eleven
`InspectionKind`s or editing core and the UI. `inspector.rs` branches on
the kind throughout (65 `InspectionKind::` matches on 2026-10-11), the
toolbar's actions are a fixed set inside `ui.inspect`, and the only
alternatives to a provider's presentation are the built-in hex view and
source switch. The first provider that accepts a file owns it. Images,
plain text, folders and "no preview" are not providers: they live in
`preview::compute`, and nothing can replace them. Plugins stay free of
GPUI (ADR-001, ADR-007), so the way to open the panel is to let them say
more as data, and have the UI ask what a preview can do, not what it is.

1. **Layouts and features instead of kinds.** An inspection names its
   layout from a small set the UI owns (prose, code, rows, table, bytes,
   image) and the features it supports (find, folds, outline, text size,
   sections, source), which the toolbar and the key context read. A new
   format needs no UI change; a new layout is one registration in
   `skwir-ui`, resolved like `views::named_renderer`, not a match arm in
   five files.
2. **Actions are contributed.** A provider lists actions for the file it
   shows (id, title, icon, slot, a command invocation given the
   preview's entry, target and selection); the builder places them, the
   rest go to More. Examples: Extract here, Install a font, Rotate, Set
   as wallpaper, Copy path to value (JSON). As commands, the palette,
   the CLI and agents get them too.
3. **Presentations chosen by the user.** Each accepting provider offers
   its presentations (SVG: picture, source, bytes; HTML: reading,
   source; JSON: tree, code; CSV: table, source). One "View as" control
   replaces the hex, source and default buttons. The highest priority is
   the default; the last choice is remembered per type.
4. **Providers declare what they handle** as data (types, extensions,
   priority, presentations) beside `accepts`, so Settings can show which
   plugin previews what and let the user settle a conflict. (Typing by
   content when the name says nothing is built: `mime::by_content`.)
5. **A source, not a path.** The request says where the bytes are: a
   file, or a member extracted from a parent with its display name. This
   is the nested preview, stated once for every provider.
6. **The fallbacks become providers.** Image, text, folder and generic
   move out of `preview::compute` into a built-in plugin of the lowest
   priority, so everything takes one path and a plugin can replace the
   folder card (a repository summary, an album's contact sheet).
7. **Anyone can add to Properties:** fields with a group, label, value and
   provenance, produced in batches on workers. Some are on demand
   ("Compute SHA-256"): a field may be an action until it has a value.
8. **Several entries, and virtual ones.** The request carries the
   selection, so a provider can summarize many files; entries of a query
   or the shelf resolve through `OpenLocation::path_of`.
9. **Readable without a window.** `preview.describe` returns the
   inspection as JSON through the same providers and bounds, for the
   CLI, the MCP server and tests. A conformance test in
   `crates/skwir/tests/` runs every provider over fixtures and checks
   bounds, fold nesting, that actions resolve to registered commands,
   and no trace after disable.

Custom GPUI content from a plugin stays out, as ADR-007 decided for
views, until a third party needs it; items 1 and 2 are the seam it would
use.
