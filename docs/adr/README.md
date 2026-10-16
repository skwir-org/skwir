# Architecture decisions

One record per decision that closed a door: its context, what was
decided, what follows, and dated amendments. A record is amended in
place or revised by a later one, which its status line names. Write a
new one when a change closes a door (`AGENTS.md`, "Engineering rules");
copy the shape of a recent record (`# ADR-NNN: Title`, `Status:`,
Context, Decision, Consequences, Amendments).

Two records share the number 003; both keep it, since code and docs cite
them by topic.

| ADR | Decision | State |
| --- | --- | --- |
| [000](000-principles-and-budgets.md) | Principles and performance budgets | binding; revised by 002, 033, 049 |
| [001](001-ui-toolkit.md) | gpui is the UI toolkit | built (vendored, patched) |
| [002](002-ui-budgets-revisited.md) | UI budgets revisited after Phase 2 | in force; amended by 033 |
| [003](003-fulltext-engine.md) | An embedded full-text search engine | not built (Phase 7); amended by 050 |
| [003](003-preview-providers.md) | Preview providers and external document renderers | built; PDF engine superseded by 004, 005 |
| [004](004-pdfium-reader.md) | PDFium as the document reader | built; amended by 014, 015, 041 |
| [005](005-poppler-reader.md) | Poppler behind the same reader protocol | built |
| [006](006-copy-tag-preservation.md) | Copies keep tags (now every `user.*` attribute) | built; amended by 028 |
| [007](007-plugin-pane-views.md) | Plugins provide pane views; Usage is cached and watched | built |
| [008](008-video-playback.md) | Video through a GStreamer helper | built; amended by 016 |
| [009](009-structured-previews.md) | Content previews as optional plugins, bounded | built; amended by 012, 016 |
| [010](010-settings-dialog.md) | Settings apply at once, from a schema | built |
| [011](011-global-allocator.md) | mimalloc, tuned, is the allocator | built |
| [012](012-reading-mode-styles.md) | Reading mode keeps the CSS that says how text reads | built |
| [013](013-fixed-layout-pages.md) | Fixed-layout book pages are drawn through SVG | built |
| [014](014-mupdf-reader.md) | MuPDF as an optional third reader | built |
| [015](015-pages-at-the-size-shown.md) | Pages rendered at the size shown | built |
| [016](016-one-plugin-for-audio-and-video.md) | One plugin for audio and video | built |
| [017](017-dates-are-the-users.md) | Dates are the user's calendar | built |
| [018](018-tabs.md) | Tabs, dormant when hidden | built |
| [019](019-terminal-panel.md) | A terminal panel on `alacritty_terminal` | built on Linux; compiled only elsewhere |
| [020](020-maps.md) | Maps, their backgrounds, the first network requests | built |
| [021](021-remote-file-systems.md) | Remote file systems come from installed tools | direction only; not built |
| [022](022-maps-later.md) | Search by place, writing locations, offline names and backgrounds | parts 1–3 built; part 4 partly |
| [023](023-fields-on-demand.md) | Fields read on demand | built |
| [024](024-key-schemes-and-the-users-keys.md) | Key schemes and the user's keys, in layers | built |
| [025](025-folders-opened-in-place.md) | Folders opened in place | built; revised by 032 |
| [026](026-search-hints.md) | Indexes hint, the walk answers | built; amended by 050 |
| [027](027-notes-stores.md) | Notes read from every store, written where the user says | built |
| [028](028-companion-files.md) | Files beside a file go where it goes | built, but for orphan matching |
| [029](029-links-in-text.md) | Links found in text and in the terminal | built |
| [030](030-text-documents-read-as-text.md) | Text documents read as text, by Skwir or Pandoc | built |
| [031](031-pictures-by-imagemagick-thumbnailers-isolated.md) | ImageMagick pictures; thumbnailers isolated | built |
| [032](032-a-tag-is-a-place.md) | A tag is a place | built |
| [033](033-idle-memory-own-share.md) | The idle budget counts Skwir's own share | built (`just idle-pss`) |
| [034](034-the-sidebar-is-the-users-and-the-trash-a-place.md) | The sidebar is the user's; the trash is a place | built |
| [035](035-phase-5-built-on-linux.md) | Phase 5 built on Linux, run on its systems later | in force |
| [036](036-identity-on-windows.md) | A file's identity on Windows | under Wine only; point 1 revised by 049 |
| [037](037-the-type-database-off-linux.md) | Each system's types first, shared-mime-info behind | Linux and Wine; macOS compiled only |
| [038](038-budgets-on-macos-and-windows.md) | What the budgets measure on macOS and Windows | waits for Phase 5b |
| [039](039-isolation-on-macos-and-windows.md) | Isolating converters on macOS and Windows | macOS compiled only; Windows not built |
| [040](040-linux-packages.md) | Linux packages: Debian, RPM, AUR | built |
| [041](041-pdfium-carried-and-what-each-system-brings.md) | PDFium carried; what each system brings | built on Linux |
| [042](042-apache-2.0.md) | Skwir is under the Apache License 2.0 | in force |
| [043](043-agents-and-scripts-drive-the-window.md) | Agents and scripts drive the running window | built |
| [044](044-the-permission-layer.md) | The permission layer | built |
| [045](045-plugins-from-outside-as-programs.md) | Plugins from outside, run as programs | built on Linux |
| [046](046-plugins-as-webassembly-modules.md) | Plugins as WebAssembly modules | built |
| [047](047-previews-from-outside-are-files-skwir-shows.md) | A preview from outside is a file Skwir shows | built |
| [048](048-archives-are-extracted-by-libarchive-in-its-sandbox.md) | Archives extracted by libarchive, sandboxed, as a step | built |
| [049](049-a-file-and-its-names.md) | A file and its names are two identities | built |
| [050](050-names-are-indexed-by-skwird.md) | Names are indexed by skwird | not built |
| [051](051-text-is-read-where-it-is-shown.md) | Text is read where it is shown | built |
| [052](052-portable-appimage.md) | A portable AppImage built in a distribution root, and its tree as a tarball | built and tried on x86-64 Linux |
| [053](053-local-releases-minimal-ci.md) | Local Linux releases; minimal routine GitHub CI | implemented; upload tested with a fake GitHub CLI |
| [054](054-a-name-already-taken.md) | A name already taken: keep both, replace, skip or merge, asked once | built; on Windows a replace undone restores the latest item |
| [055](055-the-settings-file-is-shared.md) | The settings file is shared: saves merge, outside changes are taken in, Home Manager merges | built |
| [056](056-the-window-s-state-is-not-a-setting.md) | The window's state is not a setting: a file of its own, read-only settings, settings checked by skwir | built |
| [057](057-what-you-make-is-not-a-setting.md) | What you make in skwir is not a setting: saved searches and tags kept in a data file | built |
| [058](058-properties.md) | Properties: the panel, not a dialog; access changed by plans; no administrator rights | built |
| [059](059-archives-are-made-of-what-skwir-hands-the-helper.md) | Archives made by libarchive of what skwir hands it; passwords asked, names taken settled | built; macOS and Windows compiled only |
| [060](060-code-is-colored-by-ferroni-and-its-grammars-are-let-go-of.md) | Code colored by Ferroni (vendored syntect and two-face); compiled grammars let go of after a minute unused | built |
| [061](061-djvu-read-by-djvulibre-or-djvu-rs.md) | DjVu read by DjVuLibre or djvu-rs, Automatic preferring DjVuLibre | built; DjVuLibre's helper on Linux, djvu-rs's compiled only on macOS and Windows |
| [062](062-files-on-the-system-clipboard.md) | Files on the system clipboard, as both families of Linux file managers put them | built on Linux (X11, Wayland); macOS and Windows keep Skwir's own |
