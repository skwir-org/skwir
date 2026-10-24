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
| [001](001-ui-toolkit.md) | gpui is the UI toolkit | built (vendored, patched); pin revised by 064 |
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
| [021](021-remote-file-systems.md) | Remote file systems come from installed tools | direction; revised by 077 |
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
| [050](050-names-are-indexed-by-skwird.md) | Names are indexed by skwird | plocate's answer dated (2026-10-20); skwird and coverage not built |
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
| [063](063-the-notices-are-kept-with-the-source.md) | The packages' third-party notices made once and kept with the source; no package build runs cargo-about | built for the Linux packages |
| [064](064-gpui-from-zeds-main-branch-drawn-by-wgpu.md) | gpui from a commit of Zed's main branch, drawn by wgpu, with Zed's pins; Vulkan alone where it has a GPU | built on Linux; Windows compiled, macOS compiled in skwir.app (Metal, ADR-075) |
| [065](065-what-a-screen-reader-hears.md) | What a screen reader hears: AccessKit through gpui, the window named by its location, the files as list boxes with the cursor's entry as the active descendant | the window, the files and the places around them, the previews and the terminal, checked over AT-SPI on Linux |
| [066](066-a-held-key-repeats-only-a-step.md) | A key held down runs again only a step, as its command declares; Space held peeks until let go, by the system's repeat delay | built and checked on Linux (Wayland, X11); Windows compiled, macOS's window not compiled |
| [067](067-a-3d-model-is-turned-in-the-window-drawn-by-its-gpu.md) | A 3D model is turned in the window, drawn by its GPU (gpui patch 0033); the file still read in the helper | built on Linux (wgpu); Direct3D 11 under Wine, Metal compiled only |
| [068](068-full-preview-reads-past-the-quick-bounds.md) | Full preview reads one file past the bounds that keep a preview quick, to larger ones; unremembered, no setting | built and checked on Linux |
| [069](069-step-models-are-meshed-by-ogeom-in-the-model-helper.md) | STEP (and IGES) models meshed by ogeom (pure Rust) in the model helper, each face keeping its points, faces it cannot mesh counted and said | built and checked on Linux; Windows and macOS compiled |
| [070](070-the-address-field-completes-what-is-typed.md) | The address completes what is typed a name at a time: the folder typed read once, its names matched in memory, Enter going where the first row says; Tab, abbreviated paths, places through their provider | built and checked on Linux; Windows and macOS compiled |
| [071](071-the-desktop-s-colors-and-font-skwir-s-own-controls.md) | Skwir draws its own controls on every system and takes from the desktop what the user chose there: the accent, more contrast, a color scheme (Plasma's, a GTK3 theme's, a Windows contrast theme), its fonts' families; contrast guarded, Skwir's palettes included | the derivation, the guard, Linux's portal (accent, contrast, fonts), Plasma's scheme and GTK3 themes built; macOS and Windows not yet |
| [072](072-settings-by-subject-the-few-first.md) | Settings shown by subject, whichever plugin owns the key; each page shows its few essentials and folds the rest; Changed by you; sections alike as one table; a row says one sentence | built and checked on Linux |
| [073](073-fonts-by-role-chosen-among-the-system-s.md) | Three fonts by role (interface, reading, code), each Automatic (the desktop's while Desktop colors is on, else Skwir's) or a family the text system draws; tokens of the theme, so a change shows at once; a long list of choices narrows as you type | built and checked on Linux; macOS and Windows compiled, their font registries not asked |
| [074](074-windows-builds-to-try-are-made-on-linux.md) | Windows builds to try are made on Linux with cargo-zigbuild: gpui's shaders compiled as it starts, resources by a named resource compiler, no console behind the window; a zip and a per-user installer (wixl) of one folder, notices for Windows included; tried under Wine | the zip and installer built and tried under Wine; the window never drawn there, nothing run on Windows |
| [075](075-macos-builds-to-try-are-made-on-linux.md) | macOS builds to try are made on Linux for Apple silicon: Apple's SDK by xmac, clang and ld64.lld, gpui's shaders compiled as it starts; skwir.app signed ad hoc and zipped, no folder of the building machine in it; looked over on Linux | built and looked over on Linux; nothing run on a Mac |
| [076](076-audio-and-video-off-linux-through-a-trimmed-gstreamer.md) | Audio and video on Windows and macOS through the media helper Linux has, with GStreamer's own build carried, cut down and aligned with the AppImage: FFmpeg's decoders (gst-libav) behind the system's, no GPL plugin; kept apart from any GStreamer installed; amends ADR-041 | built; Windows' plays clips under Wine, macOS's looked over, never run |
| [077](077-a-remote-folder-is-a-mount-skwir-knows-is-remote.md) | A remote folder is a mount Skwir knows is remote: reach and lanes in the core, addresses that connect then show the mounted folder, connectors (sshfs, GVfs, rclone, fstab) in the remote plugin, credentials asked by the window through ssh's askpass and never kept; the way to Skwir speaking protocols kept open | built and checked on Linux; macOS and Windows compiled |
| [078](078-copies-to-and-from-a-remote-go-in-batches.md) | Copies to and from a remote go in batches made before their steps run, each step placing its own: by a plugin's transfer (rsync through the connection an sshfs mount holds) or eight files at a time; a large file on a FUSE remote read in ranges at once | built and checked on Linux, against OpenSSH's server on this machine |
| [079](079-actions-that-share-nothing-run-at-once.md) | Actions that touch nothing in common run at once: each step's footprint (trees read, trees written, folders' names, shared stores), one heavy action a disk or server, at most four at once; the undo history in the order actions were asked, Undo taking the last thing asked | built and checked on Linux (light beside heavy, heavy apart by disk and server); macOS and Windows one channel |
| [080](080-a-stopped-action-is-retried-a-large-file-gone-on-from.md) | A plan a step's error stopped keeps what it had left for Retry (the toast, the `retry` command), asked anew and checked whole, joining the stopped plan's history entry unless an action asked since put one after it; a large file cut short stays in its staging folder, locked, and the retry goes on from it once its last MiB reads as the source's | built and checked on Linux, against OpenSSH's server stopped mid-file |
| [081](081-kde-s-remotes-through-kio-fuse.md) | KDE's remotes (SMB, WebDAV, FTP) through kio-fuse, a connector over the session bus, each server a folder of its mount, its passwords KDE's, told to forget before a folder is read again, before GVfs on KDE's desktop; a copy done once its close says so; GVfs there where it can be started | built and checked on Linux against rclone's WebDAV and FTP servers and Samba |
| [082](082-a-remote-s-password-kept-in-the-desktop-s-keyring.md) | A server's password kept in the desktop's keyring when the user asks (the prompt's Remember), once the connection served; the keyring answers first (Skwir's own, then GVfs's, read only); rclone given it obscured in its environment after a refused login; Forget Password; amends ADR-077's seventh decision | built and checked on Linux (rclone, sshfs, GVfs against servers on this machine; a private GNOME Keyring); macOS and Windows compiled |
| [083](083-a-copy-onto-a-camera-is-written-in-place.md) | A copy onto a destination that moves nothing between its folders (a camera through GVfs's gPhoto2) is written in place, never staged and moved; a batch there too, by a transfer that writes in place or Skwir's own; staging refused there; amends the staging rule | built and checked on Linux, in core against a pretended camera and on a phone as a camera |
| [084](084-servers-nearby-are-found-while-the-nearby-place-is-shown.md) | Servers announced on the local network listed in a Nearby place of the Network section while it is shown: Avahi's browsers over the system bus (DNS-SD) and wsdd (WS-Discovery: the system's daemon, else one started in discovery mode), each server a place leading to its address; nothing browsed otherwise | built and checked on Linux, on a network of the test's own (namespaces, a veth pair) |
| [085](085-agent-and-script-connections-have-separate-authority.md) | Agent connections always enforce review and privacy; script execution is selected by a separate endpoint, never caller-controlled MCP metadata | implemented; validation recorded in STATUS-LOG |
| [086](086-servers-configured-in-the-tools-own-configurations.md) | Servers configured from Skwir in the tools' own configurations: rclone's remotes added, edited, signed in, tested and removed through rclone's remote control on a private socket, in forms the plugin describes and the window draws (`App::fill`); secrets typed by the user alone, never an argument; the configuration's encryption offered, its password in the keyring | built and checked on Linux against servers, configurations, keyrings and a browser of the tests' own |
| [087](087-connect-at-login-by-the-system-s-service-manager.md) | A remote connects at login by the system's own service manager, never a daemon of Skwir's: a systemd user unit, a launch agent, a logon task running Skwir's own mount command at Skwir's own mount point; only what asks nothing at login; tests never touch the user's manager | built and checked on Linux (systemd-analyze, the unit's command run, a stand-in systemctl); macOS and Windows compiled, their files tested |
