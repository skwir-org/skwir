# One preview frame

Status: **design agreed 2026-09-21, not implemented.** Each phase below ships
on its own. When a phase lands, move its rules into
`docs/invariants/previews.md` and say so in `docs/PLAN.md`.

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

- **Placement (decided):** a collapsible bar at the bottom of the panel. It is
  expanded and fills the panel when there is no preview (including folders and
  several entries selected); collapsed to one line when there is one. Its state
  belongs to `session.rs`.
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
holds an `EntryRef`). A member is extracted by `bibli-archive-worker` into a
temporary directory, read back as at most 16 MiB of bytes, and shown by
`archive::member_preview` as an image, 64 KiB of plain text, or hex. Email
attachments share that function.

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
