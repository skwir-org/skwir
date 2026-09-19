# bibli

An experimental file manager. Fast no matter what, light on the machine,
stateless, and built to be driven by people, scripts, and AI agents alike.

bibli is written in Rust on a GPU-accelerated UI (gpui, the toolkit behind
Zed). It runs on Linux today; macOS and Windows are planned. It is early: the
plan is in `docs/PLAN.md`, and the status paragraph there says exactly what
works and what was left out.

## What it is like

- **A query is a place.** Type `*.pdf modified<7d size>10M` and the results
  are a location you can navigate to, come back to, and save as a smart folder.
- **Everything is a command.** Every action is a named command with a JSON
  schema, reachable from the keyboard, the palette, and, later, a CLI and an
  MCP server. An agent uses the same surface you do.
- **Changes are plans.** Batch operations, the external-editor flow, and
  anything an agent proposes appear as a reviewable list of steps before they
  run. Every operation has an inverse; undo works across the board.
- **Nothing is indexed behind your back.** No daemon, no database. bibli
  reuses the thumbnails, icons, MIME database, bookmarks, recent files, and
  tools already on your system, and keeps one cache you can delete at any
  time.
- **Views over one model.** List, grid gallery, Miller columns, dual pane,
  a disk-usage treemap, and a modification-time timeline are all views of the
  same listing.

Features today: streaming directory listings that stay responsive on 100k
entries; lazy metadata; thumbnails from the freedesktop cache with visible-first
generation; preview panel and spacebar peek; drag and drop in and out, on
Wayland; search with content matching; fuzzy jump from zoxide and recent
files; a shelf to collect items from anywhere; bulk rename with live preview;
edit the listing as text in your editor; tags stored as xattrs; run a shell
command on the selection; disk usage; timeline; two panes.

## Running it

You need a Rust toolchain and, on Linux, Wayland or X11 with Vulkan. The
repository ships a Nix flake with everything:

```
direnv allow        # or: nix develop
just run ~/Documents
```

Without Nix: `cargo run --release -p bibli -- <dir>`. The native libraries
needed are listed in `flake.nix`.

Useful keys: `ctrl-shift-p` command palette, `ctrl-l` address bar, `ctrl-f`
search, `ctrl-j` jump, `ctrl-1/2/3` list, grid, columns, `F3` preview, space
peek, `ctrl-shift-d` split, `ctrl-shift-u` disk usage, `ctrl-shift-l`
timeline, `ctrl-shift-b` add to shelf, `ctrl-f2` bulk rename, `ctrl-e` edit
as text, `ctrl-z` undo, `ctrl-,` settings. Every command and its key is in the palette.

Open **Settings** in the toolbar (or press `ctrl-,`) to edit preferences for
currently enabled plugins. Tab/Shift+Tab move between fields; Space cycles a
focused switch or choice. **Default** resets a field in the draft, **Save**
(`ctrl-s`) persists and applies changes, and **Cancel** (`Esc`) discards unsaved
edits. PDF engine, thumbnail and shell preferences apply without a restart.
Plugin enable/disable flags and saved-query collections are not edited here.

## Usage view

Choose **Usage** beside List, Grid and Columns, or press `ctrl-shift-u`.
Click a block to select it; double-click or press Enter to open it. Folders,
Back/Up and address navigation keep the Usage view, independently in each pane.
Selection supports preview, context menus and normal file operations.

Sizes are cached in memory and updated when files change. Unchanged cached
folders need no new scan; **Ctrl+R** or the refresh button forces recalculation.
The previous chart remains visible during updates. If live monitoring is
unavailable, a warning appears and active folders are rechecked periodically.

The chart shows the largest 64 children; the smaller-items block opens List on
a double-click. Totals include hidden files, while `ctrl-h` controls their
individual blocks. Cache entries may be evicted and are discarded on restart
or when the Usage plugin is disabled. Enabling/disabling a view plugin adds or
removes its views from the selector and palette.

## Tags and copying

Tags live in the `user.xdg.tags` extended attribute on each file or folder.
They require writable user extended attributes: ext4, Btrfs and XFS support
them; support on network mounts depends on the server and mount options.
See [Linux xattr documentation](https://man7.org/linux/man-pages/man7/xattr.7.html).
FAT32 and exFAT do not provide extended attributes
([filesystem comparison](https://learn.microsoft.com/en-us/windows/win32/fileio/filesystem-functionality-comparison)).

**Settings → core → preserve tags on copy** is enabled by default
(`core.preserve_tags_on_copy = true`). It applies to the copy command,
clipboard paste, shelf copies and drag-and-drop, including nested folders.
Disable it to copy without tags. The preference is captured in the copy plan,
so pending plans and redo retain their original choice.

If existing tags cannot be read or written, a preserving copy fails with an
error instead of publishing a copy without tags. Untagged sources need no
destination xattr support. Copies are staged before publication; symbolic
links are copied as links, without following their targets. This option
preserves tags only, not comments, ACLs or other extended attributes, and
does not change move behavior.

## File previews

F3 opens the preview panel; Space opens peek. PDF pages and Office documents
have a single top toolbar: previous/next (`alt-pageup` / `alt-pagedown`),
an editable page number, zoom presets, Fit page/width, document search,
contents, copy and open externally. Narrow panels move secondary controls
into the overflow menu. Search opens below the toolbar; contents opens in
a collapsible drawer. Native reader features work with either PDF engine.
Search and zoom popovers open beneath their toolbar button and stay within
the window; compact toolbars anchor hidden controls to More.
Images and video frames use the same top toolbar, zoom presets, Fit image/width
and overflow menus. Video previews include five-second seek controls; in peek,
`ctrl-left` / `ctrl-right` seek while plain arrows select another file.
Text, folders and unsupported files share the preview header and Open externally
action. Every preview has a Close button; closing peek returns to the side panel
if it was open. Loading feedback stays beneath the toolbar.
All controls are also available in the command palette.

The application toolbar groups views, pane controls and analysis tools, with
Search and Settings on the right. Each pane has its own Back/Up buttons beside
the address. Icons have tooltips; Tab/Shift+Tab navigate toolbar buttons and
Enter/Space activate them (Tab in split panes still switches panes).

Install optional tools on `PATH` to enable these formats:

- PDFium reader: `bibli-pdf-worker` plus the PDFium shared library. Use
  `nix develop .#pdf` for the optional runtime, or set `BIBLI_PDFIUM_LIBRARY`
  to your installed library.
- Poppler reader: `bibli-poppler-worker` plus `poppler-glib`, GLib and Cairo.
  The default Nix shell provides these libraries. `just build` and `just run`
  build both workers; building only `bibli` does not require Poppler headers.
- PDF image preview fallback: Poppler utilities (`pdfinfo`, `pdftoppm`).
- Video stills: `ffmpeg` and `ffprobe`.
- DOC/DOCX, XLS/XLSX, PPT/PPTX, ODT/ODS/ODP (and related formats): LibreOffice,
  either PDF engine, and `bubblewrap` (`bwrap`) on Linux. Both Nix development
  shells include Bubblewrap on Linux. Documents convert in isolation to cached
  PDF pages; spreadsheets show their print layout.

The `preview-media` and `preview-documents` plugins can be disabled independently
using `plugins.<id>.enabled` in settings. Missing tools are reported in the
panel. With either reader engine, drag to select text, use Copy or `ctrl-c`, Find or
`ctrl-f`, and Contents to navigate the outline. Click the page first to focus
reader shortcuts: `ctrl-a` selects page text, Page Up/Down turn pages, and
F3/Shift-F3 step through search matches. Search runs on Enter. Internal links
navigate within the document; HTTP(S) links open in the default browser.
Page and zoom state last while that file remains selected.

The default backend is `auto`: prefer PDFium when available, otherwise use
the Poppler reader. If neither worker is installed, `auto` retains the basic
Poppler utilities image preview. Explicit engine choices require their reader
worker and report an error if unavailable. In **Settings → Preview: documents → backend**, choose `pdfium` or
`poppler` and Save to switch immediately. You can also set this in
`bibli/settings.toml` under your platform's config directory, then restart bibli:

```toml
[preview-documents]
backend = "pdfium" # "auto" or "poppler"
# pdfium_library = "/opt/pdfium/lib/libpdfium.so"
```

An explicit library setting takes precedence over `BIBLI_PDFIUM_LIBRARY`.
Distributors should install workers beside bibli, or on `PATH`;
`BIBLI_PDF_WORKER` and `BIBLI_POPPLER_WORKER` select alternate executables
for development. Native PDF libraries are loaded only in workers while a
document is open. PDFium uses the
upstream BSD-style/Apache-2.0 terms plus third-party notices; the Rust binding
is MIT/Apache-2.0. See [ADR-004](docs/adr/004-pdfium-reader.md). The Poppler worker links GPL
Poppler; its distribution must respect that license. See
[ADR-005](docs/adr/005-poppler-reader.md).

Text selection is currently limited to one page; scanned images need an
existing text layer for search/selection. Password entry, editing, annotations,
continuous page scrolling, full video playback, and spreadsheet cell navigation
remain future work.

To exercise installed renderers with generated fixtures:

```
cargo build --release -p bibli-pdf-worker -p bibli-poppler-worker
BIBLI_PDF_WORKER="$PWD/target/release/bibli-pdf-worker" BIBLI_POPPLER_WORKER="$PWD/target/release/bibli-poppler-worker" cargo nextest run --release -p bibli-platform --run-ignored only -E 'test(preview::tests::real_)'
python3 tools/pdf/check.py --library "$BIBLI_PDFIUM_LIBRARY"
python3 tools/pdf/check.py --engine poppler --worker target/release/bibli-poppler-worker
python3 tools/pdf/compare.py --library "$BIBLI_PDFIUM_LIBRARY"
```

The comparison reports rendering time, CPU, and worker PSS for synthetic text,
vector, and scanned documents. Its scope and recorded results are in
[the PDF comparison](docs/pdf-comparison.md).

## Design in one paragraph

A small core (`crates/core`) owns what two plugins must agree on: entries
with stable identity and provenance-tagged metadata, locations and streaming
listings, commands, operations and plans with an undo journal, registries,
and a priority scheduler. Everything else is a plugin (`crates/plugins/*`)
that can be switched off at runtime. Platform adapters (`crates/platform`)
harness the OS. The UI (`crates/ui`) is a thin shell over the core and the
only crate that knows the toolkit. Performance budgets are measured by
`crates/bench` and enforced in CI.

## Contributing

Read `AGENTS.md` first: it holds the principles, the engineering rules, the
build and verification workflow, and the conventions, for humans and agents.
Decisions that closed a door are recorded in `docs/adr/`.

```
just lint && just test && just bench-check
```

## License

MIT or Apache-2.0, at your option.
