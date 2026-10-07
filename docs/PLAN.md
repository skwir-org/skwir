# bibli — Implementation Plan

Experimental file manager in Rust. GPU-accelerated UI, tiny core, everything else a plugin,
designed from the first commit to be driven by humans, scripts, and AI agents alike.

Principles (non-negotiable, every phase is checked against them):

1. Always responsive: no blocking work on the UI thread, ever. Everything streams, everything cancels.
2. Light: idle RSS and startup time are budgeted and measured in CI.
3. Stateless and idempotent: no daemon, no database. One throwaway cache. Re-applying a plan is a no-op.
4. Harness the environment: reuse OS thumbnails, icons, MIME db, bookmarks, indexes, installed tools.
5. Core = only what two plugins must agree on. Everything else is a plugin with a runtime toggle.
6. Commands are data. Plans before apply. Stable entry identity. Metadata carries provenance.

Durations are relative sizes for one developer working with an AI assistant, not commitments.
Each phase has a **Done when** gate. Do not start the next phase before the gate passes.

Status (2026-10-04): Phases 0 to 4 are built and their gates pass. Phase 2's gate, one
week of daily use as the only file manager, passed, followed by two weeks of improvements
from it. Phase 4's gate holds: every plugin (now 36, 14 of them previews) is proven by
`crates/bibli/tests/plugins_toggle.rs` to disable at runtime with no trace and to
re-enable. Beyond the plan, the tree now has: a settings dialog
(ADR-010: category column, search, controls chosen by the schema, changes applied
immediately through the atomic save, plugin toggles with tool requirements, a read-only
shortcut list); two native PDF readers (PDFium and
Poppler, ADR-004/005) with continuous scrolling; embedded video and audio playback through
a GStreamer worker (ADR-008) with a vendored atlas fix (patch 0003); ten structured preview
plugins (ADR-009); tag-preserving copies (ADR-006); and Usage as a persistent plugin pane
view with cached, watched, progressive sizes (ADR-007). Recorded deviations: `search` is
folded into `query`; `view-dual` is per-pane shell state rather than a crate; the review
modal, prompts, treemap and timeline strip live in `bibli-ui`; `edit-as-buffer` uses
`$VISUAL`/`$EDITOR` in a terminal; AccessKit is unavailable in gpui 0.2.2 (ADR-001); the native
workers are workspace members outside `default-members`, so the default build needs no
Poppler, GStreamer or libarchive. The idle memory gate, open since ADR-002's addendum at
118 to 120 MB of PSS against 100, is closed by ADR-033 (2026-10-04): the budget counts
bibli's own share of PSS, without the GPU driver's code, whose share depends on the GPU
vendor and on the other programs mapping it. Measured by `just idle-pss`: 89.2 MiB own
share against 100 (133.7 MiB in all, 44.6 of them the driver's). Two findings of that
measurement, fixed the same day: an idle Wayland window woke at the display's rate (gpui
asked for a frame callback in every one; vendored patch 0012: it draws when asked, idle CPU
1.0 to 0.2 %), and the first frame read a median of 146 ms against 150 (patches 0013 and
0014: pipelines built once, the GPU context created beside the fonts; median 134.6 to
114.5 ms back to back, ADR-002's addendum). Found with them: the places plugin rescanned and
woke the window about twice a second for as long as the GTK bookmarks file existed, once
anything had opened it (notify reports opens; it now drops access events, with a test).
`bibli-platform`'s `the_bytes_decide_for_a_name_that_says_nothing` expected shell scripts'
old MIME name; shared-mime-info 2.5.1 names them `text/x-shellscript`, so it now asks the
database for the name (`unalias`). The per-change validation history that used to sit here
is in `docs/STATUS-LOG.md`.

Phase 5 begins without the machines (2026-10-05, ADR-035): no Mac and
no Windows machine is available, so the phase is split. 5a, on Linux:
every system's code, compiled and linted from here (`just
check-cross`: the GNU Windows target and macOS, Zig compiling their
C), tested where Linux or Wine can. 5b, later, on the machines: the
original gate. Measured the same day: for Windows every C dependency
and gpui's backend build, and only bibli's code fails; for macOS
gpui's backend needs Apple's SDK, so `bibli-ui` and `bibli` are not
checked there, and core fails first on its manifest (ordinary
dependencies listed as Linux-only). The dev shell gained the two
targets, Zig and cargo-zigbuild; `nix develop .#windows` adds Wine.

Phase 5, step A: it compiles everywhere (2026-10-05). `just
check-cross` is green and part of `just lint-all`: clippy with warnings
as errors over the default members and their tests for
`x86_64-pc-windows-gnu`, and over the workspace but `bibli-ui`, `bibli`
and the native workers for `aarch64-apple-darwin`. Compiled only:
nothing of it ran on Windows or macOS. The system is reached through
seams core owns: `fs::Dir` (a folder held open, listed and stat-ed:
getdents and statx on Linux as before, readdir and fstatat on macOS),
`fs::stat::{identity, changed}` for metadata in hand, `fs::xattr` (the
`xattr` crate is core's alone now; Windows answers as a volume keeping
none), `op/sys.rs` (rename without replacing, swap, reflink, kernel
copy, pipes, links, permissions, the trash crate's listing),
`os_path::{from_bytes, from_bytes_ref}` (four plugins' copies of it
removed), `EntryKind::from(FileType)`. What has no code for a system
says so (`system::not_yet`): on Windows listing a folder, a stat, a
rename without replacing, a swap, Usage; on macOS copying a pipe, the
trash's listing and restore; off Linux writing through to a drive. A
few predicates answer "unknown" instead (no drive removable, no space,
no trashes, no change time on Windows), listed in `docs/PORTING.md`
with the rest. Off Linux the terminal leaves out the primary
selection, never types a `cd` (who has the terminal is not known), and
does not follow the shell's folder; Windows starts ConPTY, never run.
Tests needing Unix (links, modes, scripts) or Linux (pipes made with
`mknodat`, the freedesktop trash, Baloo, the type database) are gated
so. Linux: `just lint-all` and `just test-all` pass; its behaviour
did not change (one rename fallback reads `ENOTSUP` on macOS only).
The rule is in AGENTS.md ("Every system").

Phase 5, step B: identity, paths and listing (2026-10-05). Windows
lists and stats (ADR-036): a folder is a handle shared for every use,
listed through the IDs its records carry (no file opened per row), its
entries stat-ed by a handle each; a 128-bit ID is folded into
`EntryId`'s 64 bits, NTFS's kept exactly. Entries carry the system's
hidden mark (`Entry::hidden`: Windows' attribute, a Mac's `UF_HIDDEN`
from a stat). Paths: Windows' name rules in `valid_file_name`,
`PATHEXT` in `find_program`, `os_path::canonical` without `\\?\` for
every caller, WTF-8 names exact, the home folder read one way, `~\`,
`file:///C:/` URIs, a drive as one trail segment. Also done early from
step C, since every test needed it: a rename without replacing on
Windows (`MoveFileExW`), and a swap refused there as a conflict.
`just test-windows` runs the Windows build's tests under Wine (nextest
through cargo-zigbuild, a Wine prefix of its own, no display): core's
166 pass; four skip their trash part, which Wine's shell lacks
(`under_wine`). Wine has no 128-bit directory records, so the fallback
ran and the 128-bit path did not. Off Linux a folder moved while shown
is not followed; Usage stays Unix's. SQLite is built in on Windows
(ADR-022 amended). Nothing of this ran on Windows or macOS themselves.

Phase 5, step C: operations (2026-10-05). A copy is the system's where
it keeps more than the bytes: Windows' `CopyFileExW` (progress and
Cancel through its routine, the file's alternate streams kept, the mark
a download carries among them), macOS's `clonefile` when attributes
are kept; elsewhere the copy is as it was. macOS keeps every attribute
a copy should (Finder's tags are no `user.*` there) but the two the
system sets, and copies a pipe with `mkfifo`. With step B's rename
without replacing and swap refused, that is the step but write-through
off Linux, which waits for step F's removable drives. Run under Wine:
the Windows copy (every copy test of core), not its streams (Wine has
none; the test skips there). Compiled only: everything macOS.

Phase 5, step D: the trash (2026-10-05), Windows' half. Where a trash
keeps an item and what it says of it is one module
(`op::records`): freedesktop's pairs as before, and the Recycle Bin's
`$R` item and `$I` record (versions 1 and 2), so restoring a part,
taking a file out of the trash and putting it back work on either;
records are kept as bytes in plans, a freedesktop one serialized as the
text it was. `trash:` lists the user's folder (by their SID) in each
local drive's bin. The record format and the listing ran on Linux from
fixtures; nothing of the Windows trash ran (Wine's bin is the
freedesktop trash, and its shell trashes nothing). On macOS an item is
trashed with `trashItemAtURL`, whose resulting path the undo keeps
(with the companions'), so an undo moves it back; the Mac's trash keeps
no record of where a file came from, so `trash:` lists nothing there
and only an undo restores. Compiled only.

Phase 5, step E: the environment (2026-10-05), in part. bibli's own
settings and cache go where each system keeps an application's
(`core::folders`; Linux's XDG folders unchanged). The keyring is the
Credential Manager on Windows (run under Wine's, in its scratch
prefix) and the Keychain on macOS (compiled only). A terminal opens on
Windows (Windows Terminal, else a console of its own; a program to run
gets a console that ends with it, which is what waiting for it waits
for) and on macOS (`open -a`, no program run in it yet). The first
weekday is the system locale's (Windows' run under Wine). Types
(ADR-037, accepted 2026-10-06 as the user chose): the system's own
registry first, shared-mime-info behind it. macOS asks Uniform Type
Identifiers (compiled only), Windows the registry's classes (run under
Wine, in its scratch prefix); the database only says a system's answer
more precisely, reads the bytes and holds aliases and hierarchy. Its
reader, `xdg-mime`, is vendored with its Unix-only gate lifted, and
reads on Windows (run under Wine, from beside the test programs); the
macOS and Windows packages carry its compiled files beside bibli,
with its licence and a note of where the source is, compiled from the
source vendored in `vendor/shared-mime-info` (2.5.1) by
`tools/package/mime-database.sh` (run on Linux; the package scripts
never run on a Mac or Windows). Not
done: icons from the system, Open With from Launch Services or
Windows' handlers, owners by SID.

PDFium carried, and what each system brings (2026-10-06, ADR-041, as
the user chose). Every package carries PDFium, a pinned release of
bblanchon/pdfium-binaries (`tools/package/pdfium.lock`, fetched and
checked by `pdfium.sh`), which its helper finds beside itself or in a
Mac bundle's `Frameworks`: PDFs open everywhere. PDF thumbnails and
converted office pages are drawn by the engine (`document::first_page`;
tested with no Poppler tool to be found), `pdftoppm` only where none is.
Linux's packages split: on Debian and Fedora `bibli` (the app, PDFium,
archives, 3D models), `bibli-media` recommended, `bibli-poppler`,
`bibli-mermaid` and `bibli-mupdf` suggested, every dependency found by
the tools; on Arch one `bibli` whose Poppler and GStreamer libraries are
optional, and `bibli-mermaid` and `bibli-mupdf`. The main Debian package
went from 26 MB (85 MB installed) to 20 MB (67 MB), depending on what
bibli links alone. `try.sh` checks each system's packages twice in a
fresh root: `bibli` alone (PDFium working, the rest absent or saying it
lacks its libraries), then with the others and what they recommend
(every helper working). macOS and Windows: their packages carry PDFium
and leave Mermaid and MuPDF out; the system's thumbnails
(`system_thumbs`: Quick Look, compiled only; the Windows shell, run
under Wine, whose shell draws none); Homebrew's and MacPorts' folders
searched on macOS, LibreOffice found where it installs itself (compiled
only); the Windows PDF helper loaded the pinned `pdfium.dll` beside it
under Wine. Left for the machines: audio and video through AVFoundation
and Media Foundation, libarchive linked in statically, everything run.

Linux packages (2026-10-06, ADR-040, the formats the user chose:
Debian's, RPMs and the AUR; no Flatpak). Each is built on the
distribution it is for: a `.deb` (cargo-deb, `tools/package/deb.sh`),
an `.rpm` (cargo-generate-rpm, `rpm.sh`), and a PKGBUILD with its
`.SRCINFO` (`tools/package/aur`), from one list of the programs
shipped (`tools/package/programs`, which `just dist` and the flake
read too). `bibli` goes to `/usr/bin`, its helpers to `/usr/lib/bibli`
(`/usr/libexec/bibli` on Fedora), where bibli now looks
(`process::helper_folders`); a desktop entry and AppStream metainfo
(both pass `desktop-file-validate` and `appstreamcli validate`) and
the licence texts, now in the repository, go with them. Tried from
here without containers (`tools/package/try.sh`: a distribution's root
filesystem entered as its root user through bubblewrap): each package
built on Debian 13, Fedora 44 and Arch, installed in a fresh root by
apt, dnf or pacman with what it depends on and nothing it recommends,
every program found with its libraries there (`linux/check.sh`); on
Debian and Fedora bibli then drew its window on a virtual display
through the distribution's own Mesa (software Vulkan). namcap finds
nothing in the PKGBUILD. CI builds the three in their containers
(`linux-packages`), never run (no remote). Not done: publishing (the
AUR and a repository are the user's), Ubuntu's own build, an icon.
Settings now says a helper is installed only where it works: every
helper answers `--probe` (`process::probe`), the PDFium reader opening
the library the settings name, the player finding GStreamer's playback
plugins, and one whose linked libraries are missing is refused by the
system before it starts (tested on a Poppler reader whose library was
out of reach), so a package may leave a helper's libraries optional.

bibli is under the Apache License 2.0 alone (2026-10-06, ADR-042, as
the user chose), no longer MIT or Apache-2.0: `LICENSE`, every crate's
field, and every package's. Every dependency built into bibli allows it
(checked over the lockfile). The Poppler reader, linking Poppler whose
core is under the GPL version 2 or 3, is distributed under the GPL
version 3 (it said version 2 or later); the MuPDF reader stays under
the AGPL. Arch's `bibli`, which holds the Poppler reader, now says so.

Phase 5, step G: isolation (2026-10-06), in part (ADR-039, as the
user chose). macOS isolates converters and decoders with
`sandbox-exec` under bibli's profile, the contract bubblewrap has on
Linux (compiled, never run). `sandbox::command` returns the paths a
program is given, which only bubblewrap moves (`/input`, `/work`), so
no caller assumes them (ImageMagick, LibreOffice). Pandoc runs in its
own `--sandbox` (2.15 or later) on every system, and alone where the
system isolates nothing (Windows, a Linux without bubblewrap), writing
relative to its folder; tested on Linux both inside bubblewrap and
alone. LibreOffice and ImageMagick stay refused without isolation.
Windows' AppContainer is decided and left for the machine (5b), as
are Quick Look's and Windows' own thumbnails.

Phase 5, step F: places and volumes (2026-10-05), in part. Windows
lists its local drives as devices (label and letter, filesystem,
removable, free space) and the user's own folders (run under Wine);
macOS its root and the volumes under `/Volumes` the Finder shows
(`getmntinfo`, compiled only) and its user folders. Not done: eject
and mount off Linux, Finder's favorites and Quick Access, hearing a
drive plugged in on Windows. Windows needs no write-through of bibli's
(it writes removable drives through itself by default); a Mac's does.

Phase 5, steps G and H (2026-10-05), in part. A running instance's
cache folders were swept off Linux, where `/proc/<pid>` is not:
whether a process runs is now asked of each system. Isolation was
chosen per system the next day (step G above, ADR-039); QuickLook and
the Windows thumbnail cache are not read. On Windows a helper and what it starts are held in a job
object, ended together (run under Wine). Where the shell works is read
on macOS too (`proc_pidinfo`, compiled only); the `cd` typed into a
shell is written for PowerShell and `cmd` as well (tested on Linux),
though on Windows nothing types it yet: who has the terminal is not
known there.

Phase 5, step J: keys and the window (2026-10-05), but the drag. The
standard shortcuts are written `secondary`, which GPUI reads as Cmd on
a Mac and Ctrl elsewhere, so Linux's keys did not change; bindings are
compared as GPUI reads them, not as written (`keymap::normal_keys`:
`ctrl-c` and `secondary-c` are one key on Linux, for the layers and
for a scheme's `keys.like`); caps say ⌘ and ⌥ on a Mac; Ctrl+click,
Ctrl+wheel and a kept band are Cmd there. What a Mac reserves (Cmd+H,
Cmd+Space, Ctrl+arrows) keeps Ctrl. The fonts tried are each system's
own after Inter. Not done: an outgoing drag on macOS and Windows (a
gpui patch each), Option as a Mac's copy modifier in a drag, Option+
arrows for words. Nothing of it ran on a Mac, where bibli-ui is not
even compiled here.

Phase 5, step I (2026-10-05), in part: MPRIS is registered on the
free desktops alone (it is D-Bus's); Spotlight answers a tag search on
a Mac (`mdfind` over Finder's tags, compiled only). Not written: a
Mac's Now Playing, Windows' media controls, Windows Search.

Phase 5, steps K and L (2026-10-05). Written: a Nix package of bibli
and its helpers (`packages.default`, `just package-nix`), which
evaluates and is not built yet (a flake builds from git's tracked
files, and Phase 5's new files are not tracked until committed); an
AppImage of it (`just package-appimage`, nix-appimage), not built
either; the macOS `.app` and `.dmg` (`tools/package/macos.sh`) and the
Windows `.msi` (`tools/package/windows/bibli.wxs`,
`tools/package/windows.ps1`), never run, unsigned, without the native
workers' libraries; a CI matrix for macOS and Windows (`ci.yml`'s
`native` job, natively built, not blocking until 5b), never run (no
remote). ADR-038 says what the budgets measure on macOS
(`phys_footprint`) and Windows (the private working set), 100 MiB each
until 5b measures; `just idle-pss` says it measures nothing off Linux.

Phase 5a's checks on Linux (2026-10-06): `just lint-all` green (the
workspace, then Windows and macOS); `just test-all` 1176 passed.
Under Wine (`just test-windows`, all but bibli-ui, bibli and the
native workers, whose gpui imports Windows' own ICU, which Wine lacks):
766 passed, 15 ignored, those needing what Wine lacks passing with a
word on why. The tests that run Pandoc, ImageMagick and LibreOffice
(ignored by default) pass on Linux, but for the last's PDF reader
step, which needs the native workers built.
`just bench-check` read three of the listing stream's timings over
budget (`flat100k` complete, `images10k` first batch and complete) on a
machine loaded by other programs (load average 10 to 15); the last
commit's bench, built in a worktree and run back to back with this
one, read the same budgets over by as much, and five paired rounds on
`images10k` showed no consistent difference (3.7 to 13.9 ms either
way). To run again on a quiet machine before committing; `just
idle-pss` was not run (it opens a window in the session).

Phase 5a's gate passes (2026-10-06). `just check-cross` is part of
`just lint-all` and green. Every step above has its code or says what
is not done (G, I and K brought up to date). The Linux packages build
and install on Debian 13, Fedora 44 and Arch, `bibli` alone and then
with the others: every program found and working, and the licences in
place (a new check of `linux/check.sh`, which found Fedora's dropped on
a system installed without documents; fixed). Linux's tests pass
(`just test-all`, 1176), and so do poppler-check and mupdf-check.
Budgets, against the commit before Phase 5a (`e4c6322`): `just
bench-check` meets all 36 on an idle machine (the listing stream's
three timings read over only while the machine compiled or was loaded,
as in the runs above); `just idle-pss`, the two builds alternated back
to back, reads an own share of 95.8 MiB against 95.4 (the binary's
0.4 MiB) and a first frame no slower. The first frame itself could not
be measured that day: the build that read a median of 114.5 ms on
2026-10-04 (`23e6e59`), alternated with the two, read 180 to 851 ms,
the machine loaded by other programs (load average 4 to 10). ADR-002's
150 ms is to be measured again on a quiet machine. Found with it: the
own share grew 3.9 MiB between 2026-10-04 and Phase 5a (91.5 to 95.4
MiB in the same runs, 3.2 of it anonymous memory at idle), by the work
of 10-04 and 10-05, before Phase 5; 4.2 MiB of the budget is left.
5b, on the machines, starts from this plan's steps and
`docs/PORTING.md`.

The idle memory found with Phase 5a's gate (2026-10-06). Bisected, most
of the 3.9 MiB came with the Trash's place (ADR-034), and measured in its
parts, from its icon, not the trash plugin's work: when a trash holds
something the place shows `user-trash-full`, which Adwaita has as a
128 px SVG with filters, and `img()` drew every full-color SVG icon at
twice the size its file declares, 256 by 256 for a 16 px row. Icons are
now rasterized for the pixels they cover, rounded up to a few sides and
at most 256 (`icons::element`, `bibli_platform::icons::rasterize`). With
them: a scheduler worker idle a second after its jobs gives back what
they freed (ADR-011's addendum), and a name is typed without an
allocation per pattern of the type database (the vendored xdg-mime's
patch 0002: 1,411 allocations to type `notes.txt`, 6 now; about 13 µs a
name). `just idle-pss`, alternated back to back: an own share of 95.3 MiB
before, 92.8 to 92.9 with the icons (anonymous memory 30.1 to 27.2
MiB), 91.5 to 91.6 with the idle release, what the commit of 2026-10-04
reads the same day (91.5). The icons draw as before in a window (Xvfb,
scale 1 and 2: the list, the sidebar, the grid zoomed in).

Icons are the system's own off Linux (2026-10-06, ADR-041's point 4,
`bibli_platform::system_icons`): a file's by its extension, its own
where the system draws such files each theirs (Windows' `.exe`, `.lnk`,
`.ico`; a Mac's `.app`), a folder's, a user folder's and a volume's by
path, the trash and drives as the system's stock icons; `NSWorkspace`
and `NSImage` on macOS (compiled only), the shell's system image list on
Windows (run under Wine: a type's, a folder's and a program's icon; its
stock icons are a stub, so the trash has none there). The UI's icons are
a `Pic`, a theme's file or the system's picture, drawn at the size they
cover. On Linux the theme is read where the desktop keeps it: GNOME and
those built on it in GSettings (which writes no `settings.ini`),
Cinnamon's and MATE's in theirs, KDE's `kdeglobals`, Xfce's xfconf, then
GTK's `settings.ini`. Found with it: a theme counted as installed by its
display name (`Breeze`) while settings and lookups name its folder
(`breeze`), so any theme whose two names differ fell back to Adwaita;
themes are now known by their folders. Breeze's small icons, drawn in the
color scheme's colors (KDE's `current-color-scheme` stylesheet), are
shapes drawn in the text color, as symbolic ones are, so they read on a
dark theme. Checked in a window (Xvfb): with `XDG_CURRENT_DESKTOP=GNOME`
and a `gsettings` answering `breeze`, Breeze's icons, in the light and
the dark theme.

The document readers run isolated (2026-10-06, ADR-041's next
hardening): PDFium's, Poppler's and MuPDF's helpers, which parse
documents nobody vouched for, start through the sandbox as the
converters do (bubblewrap on Linux, `sandbox-exec` on macOS, compiled
only; as before where nothing isolates), the document read-only at
`/input`, the scratch folder at `/work`, the helper's own folder and the
library the settings name readable (`sandbox::untrusted_reading`). A
stand-in worker under it sees its document and nothing beside it. The
three engines through the reader's session, sandboxed against not
(debug helpers): pages and search the same, a document's opening 10 to
20 ms longer (PDFium 6-8 to 25-27 ms, Poppler 18-19 to 26, MuPDF 4 to
13), its pages no slower; a page of text in a font it does not embed
drawn to the same pixels.

The icon theme is a setting of bibli's own (2026-10-07, as the user
asked): Appearance's Icon theme, Automatic (the desktop's, as before)
or any icon theme installed, listed by its name when the settings open
(on a worker: it reads the icon folders), stored by its folder's
(`ui.icon_theme`); one no longer installed follows the desktop and is
said missing. A change applies at once: the theme's lookups are
forgotten and asked again (`IconCache::set_theme`, the platform's cache
by generation, so a lookup begun before a change keeps its answer to
itself). The free desktops' only: macOS and Windows draw the system's
icons. Checked in a window (Xvfb): with GNOME's setting saying Breeze
and bibli's Adwaita, Adwaita's icons; the list of themes by name, no
cursor theme nor hicolor; Papirus chosen, every icon redrawn in it.

Calendar periods, one reading (2026-10-05): the deviation "the query language's date
parser and the timeline's calendar math are duplicated" is closed. Weeks, months and years
are `bibli_platform::calendar::Period` (the first day of the one holding a day, the next,
the one before, how many between), which the query's dates (`2026-05`) and named periods
(`last-month`) and the timeline's bars all read; a date typed is `civil_day`, which refuses
`2026-02-31`; the year of an instant (the Narrow by bar, a tag location's `year`) is
`Calendar::year_of`. The period tests moved from the timeline to the calendar.

Tile archives previewed (2026-10-05), the first step of ADR-022's part 4: a PMTiles
(version 3) or MBTiles file previews as the map it holds, its facts in Details, its raster
tiles drawn over the background where it has them (a `tiles::Layer` the inspection
carries; `SourceInfo` says a source's zooms and area, `NoTile` a tile it lacks), a vector
archive outlined with a word on why it is not drawn. A map placing nothing but an area is
now fitted to it (`Shapes::new` counts the file's bounds). Checked by the plugin's tests,
two of them on files the reference `pmtiles` tool wrote (each tile where it was put, told
by its colour), and on Xvfb in the debug build: a raster PMTiles and its MBTiles copy drawn
fitted, out to the tile of their shallowest zoom and in past their deepest, their
attribution above the background's; a vector PMTiles outlined with its layers; a plain
SQLite file still in the tables preview; idle CPU 0 to 1.5 %, as with no map shown. Not
checked: a real offline map of either format (none may be copied here), Brotli or
Zstandard archives (refused with words, untested against a real one), the dark theme.
Backgrounds from a file remain proposed.

Sidebar tags as pills (2026-10-01): a place with a color is drawn as a
pill of it, and a group's pills wrap together left-aligned (a justified
line spreads its last pills to both edges), so twenty tags fit in ten
short lines where they took twenty rows; the sidebar lists 20 by default
(was 10). A place may carry a `drop` command (`Place::drop`), run with a
pane's dragged selection or another application's files: a tag's is
`tags.add` with itself. Checked in a window: twenty tags wrapping, a long
one truncated with its name in a tooltip, the current search's pill
marked, a file dragged from the list onto a pill tagged (with Undo).

Notes stores, step G (2026-09-30), the last of "Notes everywhere": the
name-tags plugin. Tags in names are two stores (`Kept::Name`): `TagSpaces`'
`name[a b].ext`, read only in folders a `TagSpaces` location covers (a
`.ts` folder there or above: yt-dlp names downloads `title [id].ext`), and
filetags' `name -- a b.ext`, read only once turned on (a store now says
whether it is read by default, `NotesStore::default_read`). An edit never
writes a name and a name is never said to differ, so the Notes page has
no Write row for them: "Tags into Name" (`names.tag`) is a plan of
renames holding the tags the name can (the others named in its note), and
"Tags out of Name" (`names.untag`) first has the other stores keep the
name's tags (an edit reading the file without its name), then renames it,
undone as one. Checked by the plugin's tests (both conventions split and
joined, a `TagSpaces` name read inside a location and a download's
`[id]` outside it, tags out of a name and back, into a name with one left
out). Of "Done when": a round trip through an exFAT stick was checked on a
FUSE mount that refuses attributes, not on exFAT with a Mac reading it;
another application's later edit winning, undo byte for byte, and
`tags_fill_us` within its budget are checked (the bench's app now runs
every store: `flat100k` 227 ms against 550, `images10k` 41.5 against
120, `tag:` over a million files 2.66 s, under a load average of 15). Windows and macOS's own
stores wait for Phase 5. Three settings tests timed out once under a load
average of 30 from other programs and passed when run again.

Notes stores, step F (2026-09-30): the tagspaces plugin. `TagSpaces`'
sidecars are a store and companions: `.ts/x.json` (and its thumbnail,
`.ts/x.ext.jpg`, which follows it) and a folder's `.ts/tsm.json`, inside
the folder (stores now know a batch's folders, `Probe::folder`); the
`.ts` folder is hidden like companions. A tag is read with the palette
color nearest its hex color (none for a gray), and a tag kept keeps its
object, its exact color with it; a new tag given a color is written with
the palette's hex. The description is the comment; `TagSpaces` has no
rating. Every other key of a sidecar stays. A location's index
(`.ts/tsi.json`, found at the search's root or above it) is a search hint
(ADR-026), its paths absolute or relative to the location. Updated where
present, created only when asked. Checked by the plugin's tests (the
documented sidecar edited in place and undone byte for byte, a folder's
own notes, a rename carrying sidecar and thumbnail, the index hinting)
and a cross-plugin test (one edit reaching the desktop's attributes, an
XMP sidecar and a `TagSpaces` one, none differing after, undone).
Deviation: `tsi.json`'s layout was read from its documentation's
description, not from an index `TagSpaces` wrote; entries are read
tolerantly (an array, or an object's `entries`).

Notes stores, step E (2026-09-30): the xmp plugin. XMP sidecars are a
store and a companion: `x.jpg.xmp` (darktable, digiKam) and Adobe's
`x.xmp`, which follows its file only while no other file shares its stem
and is read for a file unless `photoshop:SidecarForExtension` says it is
another's. Keywords are `dc:subject` (Lightroom's and digiKam's nested
keywords read for their last part when it is empty, and kept in step
when present), the rating `xmp:Rating` in whole stars (the store's `fit`
rounds a half star, so it is not said to differ), the comment the
default of `dc:description`, and Lightroom's `xmp:Label` a tag of its
name and color, written back there while kept. An edit splices the
changed properties into the text as it is (attribute or element,
declaring a namespace where needed), so darktable's history and every
property bibli does not know stay; a text it cannot edit is left alone.
Sidecars are updated where present and created only when asked (Notes
page). XMP inside pictures, videos and PDFs is read on demand for
Details ("XMP in the file"), never written. Checked by the plugin's tests
(a darktable-like sidecar edited and read back, a new one, a packet
inside a JPEG, both names following renames) and a cross-plugin test
(the desktop's tags and a sidecar edited as one, a photo application's
later edit winning, the sidecar hidden beside its file). Deviations: the
nested keywords are read for their last part; the model has no nesting
yet, so a new tag is written at the top of the hierarchy.

Notes stores, step D (2026-09-30): the Notes page. A plugin registers a
store with `App::register_notes_store`, which declares
`notes.<store>.read` (on by default) and `notes.<store>.write` (Off,
Update existing, Where attributes are missing for a store beside the
file, Create; the store's default) under the store's title, and withdraws
them with the plugin; a save rebuilds the policy an edit captures
(`App::apply_notes_settings`). Found in a window and fixed: turning
companions on or off left the list as it was until something else changed
it (the views key on the listing's versions: a change of the setting now
moves them, `Listing::companions_changed`). The copy setting's
description no longer says a copy fails where it now keeps attributes
beside the file, and making the stores agree says so in its toast.
Checked in a window on an isolated display: the Notes page and its two
rows per store, the orphan listed and the companion hidden with hidden
files, the View menu's Companion files both ways, Details' "Kept in",
"Differs in" with Make them agree (the desktop's attribute rewritten, the
row gone), "Beside it" and an orphan's "Beside … which is gone"; idle CPU
0–1%. Deviation: the Notes page lists the stores by title, not the
platform's own first.

Notes stores, step C (2026-09-30, ADR-028): files beside a file are
declared by name rules (`bibli_core::companions`), the tags plugin's `._`
file the first, which also holds attributes. Listings mark them and hide
one whose file is listed unless hidden files and `core.show_companions`
(General, and the View menu under Hidden files) are both on; an orphan
is listed as any file; a change to a companion re-reads its file (the
watcher's `._` special case is now the rule). Rename, move, copy, swap,
trash, restore and delete carry companions, their inverses bring them
back, and a companion's place taken is a conflict (the operations'
property test now runs with companions in its trees). A copy takes a
`._` file's attributes in onto a volume keeping attributes and keeps
the file's attributes beside it onto one refusing them, the resource
fork and Finder info as `user.com.apple.*`; ADR-006 is amended. A
no-replace rename refused by the filesystem (bindfs, sshfs, many FUSE
ones: until now every copy onto them failed) looks, then renames.
Details lists a file's companions (Location, "Beside it") and what an
orphan belonged to. A batch of a folder whose listing is complete gets
its names (`MetadataProvider::fill_listed`), so the `._` store asks the
listing instead of reading the folder. Checked: the core's and the tags
plugin's tests, and an ignored test run against a `bindfs --xattr-none`
mount (`BIBLI_NO_XATTR_DIR`): a copy onto it keeps tags and rating in a
`._` file, a copy back makes them attributes, and tagging a file there
starts its `._` file, which undo removes. Deviations: a batch hydrated
before its folder's listing completes still reads the folder once for
`._` names; the View menu item and the hiding were checked by tests,
not in a window yet.

Notes stores, step B (2026-09-30): when the stores disagree about a
file, the notes are merged against what bibli last wrote, kept in the
cache (`notes` namespace, by the file's identity) with what each store
read back: a store that changed since was edited by another application
and its edit wins (tags added and taken away, a rating or a comment
changed; the first store's over a later one's); a store that holds
nothing now is not an edit; without a snapshot, the union. Each store
says what it can hold of some notes (`NotesStore::fit`: the desktop's
attributes hold no tag with a comma, the Finder no rating), so a store
is said to differ only when it could say the same and does not. The
provider fills where the notes are kept and the stores that differ
(`keys::NOTES_KEPT`, `keys::NOTES_DIFFER`); Details shows them under
Notes, the second with "Make them agree" (`tags.agree`: a `set_notes`
with `sync`, written to every store even where the merged notes already
hold). Undo forgets the snapshot. Deviation: the Details rows were
checked by tests, not yet in a window.

Notes stores, step A (2026-09-30, ADR-027): the core holds the model
of a file's notes (`bibli_core::notes`: tags with the color a file gives
them, rating, comment, origin), the `NotesStore` trait and its registry
(`App::notes`, owner-tagged), and the edit, a `set_notes` step that
captures the stores' policy when the plan is made, writes every store
the policy allows, the file's own attributes first and, where they are
refused, the store its folder already uses (else AppleDouble, now
created there), and whose inverse puts back each store's bytes exactly
(`restore_notes`), refusing when another application changed them
since. The tags plugin's code became three stores (the desktop's
attributes, the Finder's, `._` files, which now also keep the desktop's
attributes as `xdg.tags`), read in one batch per folder with one
`listxattr` per file; `set_tags`, `set_rating` and `set_comment` data
still load, as the edit they were. The index learns from applied
changes (`Executor::observe`) instead of a hook of the plugin's. A test
that counted tags right after an undo raced the folder's watcher under
load and now waits for the listing to show them. Deviation: merging is
still the union (step B brings the snapshot); the policy is each
store's default until the Notes page (step D).

Tags everywhere (2026-09-30): the sidebar has Tags, the tags on the most
files the tags plugin's index knows under the home folder (`tags.sidebar`,
10 by default), each a search of the home folder built by the query
plugin (`query.location`, so the tags plugin writes no query syntax), a
dot of the color it wears in place of an icon (`Place::color`: the
user's choice, else the one a file gave it, which the vocabulary now
remembers, else its name's), published when the index changes and only
when what it shows did. The index keeps tags as spelled and learns what
the plugin's operations write. A tag's row renames it or takes it from
every file (`tags.rename`, merging with a tag of the new name;
`tags.delete`): the files come from the search, walk included, never
from the index, and the plan is proposed for review, then undone as
one. Open searches now follow what bibli changes: after each plan the
query plugin checks again the files it touched under each search's root
and adds, updates or removes rows (before, a search stayed as walked
until refreshed). A Mac's notes on a volume without attributes are read
from the `._` files beside the files (`appledouble.rs`, the layout of
Apple's `copyfile.c`), on FAT, exFAT, NTFS, network and FUSE volumes
(`bibli_platform::filesystem`): tags with their colors, the Finder
comment and where a download came from (also read from a Mac's
attributes under `user.`/`osx.`), a folder's `._` names read once per
listing (a per-file lookup measured at a third of a `listxattr`). A tag
edit rewrites the `._` file whole, keeping every other attribute, the
resource fork and its mode; one laid out otherwise is never written,
and a file with neither says its volume keeps no tags. A change to `._x`
makes the folder listing read `x` again. Checked in a window on an
isolated display: the Tags group and its colors, a search from a row,
rename through the review, the search emptying as it applied and
filling again on Undo, and on a FUSE folder that refuses attributes
(`bindfs --xattr-none`) a Mac's tags, comment and addresses read from
`._` files, a tag added (the row refreshed from the `._` change) and a
file without one refused; idle CPU 0–1%. `bench-check` passed on a
quiet machine, the listing budgets that failed under load included
(`flat100k` streams in 49.7 ms against 100, `images10k` in 10.7 against
12; `tag:` over a million files 2.1 s). Deviations: no `._` file
written by a Mac was read (the tests build Apple's layout by hand, from
its source, not with our encoder); a `._` file is never created, so a
file on a stick without one cannot be tagged (the sidecar decision is
next); a tag's color the user chose does not follow a rename; the
sidebar lists only tags the index has seen, not Baloo's; renaming
leaves hidden files as they are, as the search does.

Tags, continued (2026-09-30): the grid shows a tile's tags as dots over
its picture's corner (three, then "+N", the names in a tooltip), so rows
keep the height their names give them; a list row's chips are placed in
prepaint (`elements/chips.rs`) and "+N" says how many dropped. A Mac's
Finder tags (`com.apple.metadata:_kMDItemUserTags`, a binary property
list, under `user.`, `osx.` on an HFS+ volume, or as a Samba stream) are
read with ours, wear the color the Mac gave them (`keys::TAG_COLORS`),
and are rewritten with ours on an edit, an undo giving their colors back;
a tag named after a color wears it. `tags.toggle` gives or takes one tag,
to bind to a key. Details lists what the system adds to the mode (an
access list, a default one, file capabilities, an SELinux label) under
Permissions, read with the fields on a worker. `ui.open_url` goes through
the platform's one link filter (`open::is_web_uri`), which refuses what
is not an `http(s)` link with a host and says so. Measured
(`bibli-bench`, new `tags_fill_us`, `tag_search_us` and a `search`
command that prints each run): a search stat-ed every file by opening
its folder again; through a folder kept open (`fs::Statter`) a stat-only
search of a million files went from 2.2–2.8 s to 0.64–0.9 s. `tag:`
stays bound by reading attributes (btrfs: about 1.6 s per million, not
scaling past eight threads), so ADR-026 adds hints: a `tag:` search asks
Baloo (when it indexes) and an index the tags plugin learns as it reads
files, considers those files first, then walks; the first of 1 000 hits
in a million files arrive after 0.10 s instead of seconds. Checked in a
window on an isolated display (dots and their tooltip, "+N" at two
widths, Finder colors, the access list, Open with a stand-in `xdg-open`)
and idle CPU back to about 0%; Baloo against a private instance (an
ignored test that starts `baloo_file` on its own D-Bus session).
Deviations: no Finder attribute written by a Mac was read (the tests
write Apple's format with the same encoder; an HFS+ image is the next
check); the cold `tag:` time needs root to drop caches and is not
measured; `bench-check` was run while other programs loaded the machine
(load average 28 to 56), and the listing budgets of `flat100k` and
`images10k` failed then, though `images10k` alone lists in 3.9–7.0 ms
against 12.

Tags and notes (2026-09-30): the tags plugin reads everything it knows of
a file with one `listxattr` (then only the attributes present): tags, a
rating (`user.baloo.rating`, half stars, as Dolphin) and a comment
(`user.xdg.comment`), which it writes with inverses (`set_rating`,
`set_comment`), and where a download came from (`user.xdg.origin.url`,
`user.xdg.referrer.url`, read only). A link's are its target's. The tag
prompt, which set the union of the selection's tags on every file (and
could drop tags it had not read yet), became a tag editor: it reads the
selected files' tags on a worker, shows each as on all, on some ("on 1 of
2") or none, then the tags used lately (a vocabulary in the cache, bounded,
written on a worker), and applies only what moved (`tags.edit`, one
undoable plan). A tag wears the color its name gives it or one the user
chose from the editor (`tags.colors`), in every view. Details has a Notes
group (tags as chips that search for the tag, stars to click, the comment
with Edit) and a Source group (Open in the browser, Copy); the context
menu a Notes group (ratings, Comment… holding the comment there); the list
a Rating column, off by default, and a sort by rating, best first. Search
has `rating>=4` (stars, halves allowed; `rating:0` is unrated) and
`comment:`. The tag chips of a row take only what its name leaves and drop
whole. Copies keep every `user.*` attribute (ADR-006 amendment). Checked
in a window on an isolated display (the editor on two files tagged
differently, a new tag, a color, stars, a comment from the menu, the
links, the column and its sort, a search) and idle CPU back to about 0%.
Deviations: the Finder's tag attribute (Phase 4, item 8) is still not
read; tags have no index (a later ADR, once `tag:` over a home folder is
measured); the grid shows no tags yet.

Window row and pane rows (2026-09-29): the application toolbar became the
window row, 38 px: over the sidebar its toggle and More, over the panes the
tabs (ADR-018 addendum: shown even when there is one, so a second tab no
longer adds a strip), then the Split and Preview toggles. The views and the
View menu moved to each pane's row, so a split's panes show and change
their own; a narrow pane folds them into one button (two `AdaptiveRow`
variants, the address counted at its minimum). The bookmark button ends the
address field; Up and New folder lost their buttons (the trail's parent
segment and Alt+Up; the folder's menu and Ctrl+Shift+N). Chrome above the
first row: a folder 108 to 104 px; split with two tabs and a search 217 to
177 px. Checked in a window on an isolated display (folder, split, narrow
split, the View menu from the right pane, More, a search, the sidebar
hidden) and idle CPU back to 0%. Then, still A: a right-click on Back or
Forward lists the history that way (`back`/`forward` take `steps`), and
the trail's chevrons list the folders inside the segment before them (a
last one after the folder shown), read on a worker; a second click on a
menu's button now closes it, the View and More menus included, which
used to open again. This is step A of the toolbar overhaul. Step B: a
search is drawn in its address field as pills (where it looks, each facet
set, "+ Filter" for the rest) instead of a row of chips, and the field is
where one types: Ctrl+F starts a search there with where it looks as a
pill, a dropdown offers searching here or at home, files containing the
words, Jump's places, a path to go to, and `>` for the palette; the search
prompt is gone. A search's tab and history rows read what it looks for
and where (`LocationProvider::title`). Words typed over a folder filter
it as they are typed (the core's `filter`, applied by `Pane::order`, so
select all takes only what is shown), with the count as "1 of 7 items",
a pill on the trail once the field lets go, Escape to show everything,
and a state of its own when nothing matches; Enter then searches the
folders inside. The dropdown hangs from the field's right end so the
filtered names stay in sight. Checked in a window on an isolated display
(pills, Filter, a facet chosen from it, the scope menu, a pill removed,
Ctrl+F, Down and Enter, a path, `>`, filtering, the pill, Escape, no
match) and idle CPU back to 0%. Then the application toolbar went: each
area owns its top (the sidebar's header holds its toggle and More; Split
and Preview mirror them in the preview panel's own bar while it is open
at the right, else at the end of the tab strip, else of the last pane's
row; a sidebar not drawn leaves its buttons in the first pane's row), and the tab strip above the panes
shows with two tabs or more, or one with "Show a lone tab"
(`ui.single_tab`, off by default). A window of one tab has one row of
bars above its lists, split side by side or stacked: 72 px above a
folder's first row. With nothing selected, the preview panel shows where
the pane is: the folder, as a selected one, with a Volume group in
Details (device, filesystem, free space), or a search's or the shelf's
name and count. The status bar went the way of the toolbar: each pane has
its own footer (what it lists, what is selected there, its size), the
window's state (Undo, what waits for a paste, the shelf, proposals, the
player's strip) ends the last pane's footer, and the sidebar and the
preview panel, which need none, run to the window's foot. With nothing
selected, a pane's footer says what its files weigh ("5,000 items ·
488.3 KB"), summed by a worker since the listing knows only the sizes of
the rows drawn. Step C, a selection's action bar built from the context
menu's items, was tried (a pill over the foot of a pane while its
selection was picked, commit 37204aa) and reverted: not convinced it is
needed beside the context menu and the keys.

Busy volumes (2026-09-29): a volume being unlocked, mounted or unmounted
says so on its sidebar row (`Place::busy`: a still hourglass and
"Unlocking…" in place of its details, since nothing animates) from the
moment the passphrase is given until the attempt ends, and offers nothing
to click again meanwhile. When it ends the sidebar is right at once: the
quick rescan used to borrow the last `lsblk` answer, so a volume just
unlocked was offered for unlocking again until `lsblk` ran, behind the
listing of the volume it had opened. Between the unlock and the mount,
udev's announcement of the new device rescans, and `lsblk` lists the
container as its cleartext volume, which showed "Not mounted" until the
next `lsblk` after the mount; that volume now carries its container's
busy state and goes with it (`Volume::container`). A volume can be unmounted while bibli
shows it: `App::release` sends the panes showing it home and closes the
folders kept there (an open listing holds its folder), and the unmount
waits up to 3 s for helpers to let go before saying another program uses
it. Checked with fake `lsblk` and `udisksctl` first on PATH, in tests and
in a window on an isolated display; the release by what the process
holds open (`/proc/self/fd`). Not checked on a real drive.

Tree view (2026-09-29, ADR-025): a Tree view beside the List in the View
menu (`ViewRole::Alternate`: a toolbar button only while shown), whose folders open in place under a triangle (Right
and Left open, enter, close and go to the parent; Collapse all in the View
menu). The folders opened are core pane data (`Pane::expanded`, the
`expand` command) listed as kept locations, and their entries are the
pane's: `Pane::order`, `Pane::entry` and `Pane::path_of` span them, so
ranges, select all, every command, drags and the preview act on nested
entries unchanged. Only the tree keeps them open; another view closes
them. Deviations: no Ctrl+digit for the view (Usage and the map hold 4
and 5); a nested selection is not restored after a tab sleeps. 876
default tests pass; checked on an isolated X display: opening by the
triangle and by keys, a rename of a nested file with its toast and the
watcher's update, and switching back to the List closing the folders.

Keys (2026-09-28, ADR-024): a `vim` plugin, off by default, binds yazi's
keymap in the file list (hjkl, `g g`/`G`, Ctrl+U/D, Space to pick, `v`
and `V` for a range, `y x p d D a r`, `/ ? n N`, `, m` and the other
sorts, `g h`/`g c`/`g d`, `t t`, 1 to 9, `[ ] { }`, `~`), each key a
command anyone can run. The selection now knows whether it was picked
(`Selection::picked`): a scheme's moves `follow` (a picked selection
stays while the cursor moves) and a sweep holds a range while it moves,
shown in the status bar; the standard keys are unchanged. Keys come in
layers (plugins, shell, schemes, the user's), so a scheme wins whatever
order plugins were toggled in, and a binding a later one hides is no
longer named by menus or tooltips. The user's shortcuts are a setting
(`keyboard.bindings`) edited on the Keyboard page, which ADR-010 had
left read-only; an empty command frees a key. Type-ahead yields while
letters are bound in the file list. A key that begins a sequence waits
for the next (vendored patch 0010) under a strip listing what can
follow. New for everyone: `new_file` (New File in the folder's menu; a
name ending in `/` is a folder), `ui.find`. Deviations from yazi: no line
modes, links, tasks or trash place; Left, Right and Tab keep bibli's
meaning; `f` and `s` both search, as bibli has no filter apart from
search; `u` undoes (vim's, not yazi's). 874 default tests pass; checked
on an isolated X display: picking, sweeping, `g` sequences, find, new
file and folder, undo, a user shortcut saved, restarted and run, and
type-ahead back once the plugin is off.
More previews (2026-09-28): SVG drawings are drawn by resvg (inert, sharper
as they are zoomed; `Pixels` says how far a picture may sharpen); OpenEXR and
DDS decode in the process, floating-point light encoded as sRGB in
`picture::decode`; a picture bibli does not decode (HEIC, AVIF, JPEG XL, raw)
is drawn by the desktop's thumbnailer at 1024 px (`thumbnails` plugin,
`PreviewInfo::Picture`). Code is highlighted with bat's 213 grammars
(two-face), about 1.2 MB more once code is first shown. Fonts read WOFF and
WOFF2; JSON with comments, JSONC and JSON5 show as written; JSON Lines, Excel
workbooks (calamine, cells streamed: ODS and XLS stay with LibreOffice, whose
readers there allocate the whole grid) and SQLite databases (the system's
libsqlite3, read-only and immutable) are tables; Jupyter notebooks read as
documents. Radiance HDR and farbfeld stay out: the MIME database names
neither.
Mermaid (2026-09-28): fenced `mermaid` blocks in Markdown and notebooks are
drawn by merman, Zed's Rust Mermaid, in `bibli-mermaid-worker`: linked into
bibli it added 14.7 MB of code (51.9 to 66.6 MB of text), so it is a helper
of its own (16.3 MB), a default member since it needs no native library.
Its SVG is made for resvg and drawn on white like a drawing in HTML
(decoded with the page, `EMBEDDED_IMAGES` of them); without the helper a
diagram stays code.
3D models (2026-09-28): the desktop's thumbnailers (f3d, Blender's) now
draw 3D models for the preview panel as they did for the grid (the
`model/*` types and what the MIME database draws as a picture). The new
`preview-models` plugin shows Blender files, 3MF packages, FreeCAD
documents, DWG and DXF drawings and G-code (text or Prusa's binary) by the
picture stored in them, read only as far as it (a compressed Blender file
is decoded as a stream that far), with a print's time, filament and
printer, a Blender file's version and scenes, in Details; about 120 KB of
bibli's code. Meshes that store no picture (STL, OBJ, PLY, OFF, glTF, a
3MF left unpictured) are drawn by `bibli-model-worker`, pure Rust (1 MB
of code), a default member: a z-buffered view from the front, right and
above, drawn at twice the size and averaged down, and measured (size,
volume when watertight, triangles). 3DBenchy (225 706 triangles) takes
140 ms, its volume 15.55 cm³ as published. Without the helper the plugin
leaves meshes to an installed thumbnailer. STEP and IGES stay with f3d
(its nixpkgs build names STEP `application/vnd.step`, which the MIME
database here does not, so those need f3d's own MIME types); OpenCASCADE
would be a helper of its own if ever wanted. `SketchUp` models (the
PNG a 2021 model keeps in the zip after its header, or among an older
one's first bytes), `LayOut` documents and Rhino models (the preview
bitmap, zlib-compressed, in the properties table) show their stored
picture too; checked on two `SketchUp` 2021 models, a `LayOut` 2021
document and two Rhino 5 models; older `SketchUp` and newer Rhino files
only on synthetic ones.
Circuits (2026-09-28): the new `preview-circuits` plugin draws KiCad
boards, footprints, schematics and symbol libraries (KiCad 6 onwards;
boards since 4) from their S-expressions, written as SVG for the inert
rasterizer, with a board's size, layers, footprints, nets, tracks and
vias, a schematic's symbols and sheets in Details. Checked against
`kicad-cli`'s own SVG on KiCad's demo projects (KiCad 9 and 10 files):
pins land on wires, turned and mirrored symbols match, multi-unit
references are lettered. Release build, 2048 px: 50 to 600 ms a file;
peak about 25 MB beyond the file's own size (an 84 MB board: 3.3 s,
111 MB, so files over 32 MiB are not drawn). Text is set in a system
font: KiCad's stroke font is GPL.
DXF drawings (2026-09-28) are drawn from their entities rather than only
by the thumbnail few of them store: model space, blocks and dimensions
expanded within budgets, `AutoCAD`'s colour index, hatches filled, text
set in a system font, fitted and aligned text stretched between its
points. Compared with ezdxf's own renderer on 18 files (ezdxf's
examples, KiCad's exports and one other): the same geometry;
3 to 790 ms each in release.
G-code without a picture (2026-09-28) is drawn from its moves: prints
from the front, right and above in their features' colours or by
height, CNC and laser programs from above by depth, arcs in any plane
included. Streamed twice, nothing held but the picture: 16 real
programs (cncjs's examples, LinuxCNC's, a PrusaSlicer print stripped
of its thumbnails) in 140 ms at most in release, at 1024 pixels as at
128; the three that compute their moves with parameters are left as
text.
Gerber and drill files (2026-09-28): the circuits plugin draws a
Gerber layer (RS-274X: macros with their arithmetic, regions, arcs in
both quadrant modes, clear polarity, step and repeat, aperture blocks;
the deprecated knockout is not) in its layer's colour, and an Excellon
drill file's holes, slots and routes. Compared with gerbv on KiCad's
exports (KiCad 10, with and without X2 attributes), a demo project's
own and gerbv's test files: the same pictures, where gerbv's older
build does not itself draw a macro's rotation or a block; 60 ms at
most in release.
Audio and video (2026-09-25, ADR-016): `preview-audio` is folded into
`preview-media` ("Preview: audio and video"), which declares every playback
preference, the shell player's included (keep playing while you browse, on by
default; play on selection; stop, play next or shuffle when a track ends).
Settings pages follow their subject (ADR-010 addendum): the reader's
preferences, shared by several preview plugins, moved from Appearance to a
Reading page (`reading.*`), and a moved key keeps its value
(`Settings::rename`). The Plugins page lists plugins under the category
each declares (`Plugin::category`: browsing, files, previews, desktop), which
`plugins.list` also reports.
Huge folders (2026-09-25): the UI applied a listing in one drain that lasted
as long as the worker kept sending, holding the UI thread and the workspace
lock (0.9 s for the 366 000 entries of `/nix/store`, warm cache, release; longer
on a slow disk), so no frame, banner included, was drawn. The drain is now a
slice per sync (`PUMP_BUDGET`) and arrivals are placed in the sorted order
instead of sorting again: the same folder draws its first rows at once, shows
"Loading location…" with a growing count, and is complete in 0.75 s.
The columns view no longer lists its folders a second time: the pane keeps
the folders above open beside the one shown, and going up or down adopts the
listing already there (`App::keep_locations`), so `/nix/store` comes back at
once, complete, and its column is whole as soon as a child is entered.
Bookmarks (2026-09-25): beyond adding, the sidebar's bookmarks can be
renamed, removed and put in order (a right-click menu that each place fills
through `Place::menu`, and dragging one bookmark onto another), every edit
answered by a toast with Undo and then Redo. Ctrl+D and the background menu
bookmark the folder shown, and a button in each pane's row shows whether it
is bookmarked and toggles it. The GTK file is watched, so edits made in other
file managers appear, and a bookmark whose folder is gone is dimmed with the
reason. Edits keep the lines they do not touch, remote bookmarks included.
Open With (2026-09-25): the `open-with` plugin lets the user choose the
application that opens the selection (Open With…, Shift+Enter) or the folder
shown (Open Folder With… on the background). The applications come from the
desktop's entries and `mimeapps.list` files (`bibli_platform::apps`): the
default first, then those that declare the type, then those that open a
parent type (plain text for a C source), then every other application, read
on a worker and read again when an install or a changed default moves the
stamp. The list is the shell's `ui.choose`, a filterable picker a plugin
fills with choices or with a command that lists them, as `ui.ask` asks for a
line of text. The picker's switch, "Always use for PNG image files" (Alt+A,
offered when the selection is one type), also makes the chosen application
the type's default: `bibli_platform::apps::AssociationEdit` rewrites the
user's `mimeapps.list` (the one that already decides, a desktop's own list
first), keeping the lines others wrote, and returns the edit that reverts it,
which the toast offers as Undo, then Redo, as bookmark edits do; both files
go through `file::rewrite`, which follows links and refuses a read-only one.
Saved searches (2026-09-25): kept like bookmarks, under their own Searches
heading below Bookmarks. They stay in bibli's settings (`query.saved`), not
in the GTK file, which every GTK file chooser reads and would show as broken
rows; one list mixing both would need an order file of our own over two
stores. What is shared is the gesture: Ctrl+D and the pane's button run the
core `keep` command, which hands the location shown to the plugin that keeps
its scheme (`registry::Keeper`: the places plugin for folders, the query
plugin for searches), and the button is filled while a place with a
`forget` command is the location shown. A search is saved under a name made
from it (`ext:pdf in Documents`) and, from its right-click menu, renamed,
edited (its name and place stay) or removed; any place with a `reorder`
moves by dragging within its group. Each edit is a `saved::Edit` applied on
a worker that returns its inverse, offered as Undo, then Redo. A saved
search is known by its expression, as a bookmark by its folder, so
`query.save` and `query.forget` changed: saving no longer replaces a search
of the same name, and forgetting takes the expression (or, as before, the
name). A theme icon that exists only as a `-symbolic` variant, as
`folder-saved-search` does in current Adwaita and Breeze, is used as GTK
does, drawn in the text color (`icons::themed`; the worker that finds it
reads it, so drawing it never reads the disk). The background menu of a
search or the shelf no longer offers what needs a folder (New Folder,
Bookmark This Folder, Open Folder With, Edit as Text, Usage, Timeline,
Search here): those items say `empty folder`, a token that holds while the
pane shows a folder. The text input scrolls a line longer than its field
to keep the cursor in view and clips it to the field, so a long search's
address no longer runs under the pane's buttons; unfocused, it shows its
start. A `when` token may list alternatives (`selection|folder`, a GPUI
`(selection || folder)` for keys): Open Terminal Here and Run Command say
so, and work in `App::work_dir`, the folder shown or, in a search, the
nearest folder holding the selection; Paste, always into the folder
shown, says `folder`. Run Command asked for no command and failed
everywhere; it now asks through `ui.ask`.
Query words for people (2026-09-25), the first step toward a query designer
that needs no syntax: `kind:` (`kind:image,video`, `kind:documents`,
`kind:music`) groups files as the desktop's MIME database does, from each
type's generic icon (`x-office-document` covers PDF, EPUB, Markdown and word
processor files; `package-x-generic` archives) and from descent from
`text/plain` for text and code (`bibli_platform::mime::Kind`, cached per
type); and named periods (`modified:today`, `this-week`, `last-month`,
`this-year`, with `<`, `>=` and ranges as for dates) are calendar periods in
UTC like the dates, weeks starting on Monday. The search prompt's example
uses both.
Dates are the user's (2026-09-25, ADR-017): the query text records no zone;
dates, named periods and the timeline's bars are days of
`bibli_platform::calendar::Calendar`, the system's time zone with its clock
changes (a day of 23 or 25 hours, a midnight that does not exist) and the
locale's first weekday, asked of `locale` once on a worker. Durations are
counted on the calendar (`7d` across a clock change is seven days; `1mo`
works). A search showing a named period lists again when it moves on,
checking the wall clock at least every ten minutes so a sleep past midnight
is not missed. Saved searches with dates shift by at most the zone's offset;
the timeline's bars, labels and the queries they become now name the same
day.
Search chips (2026-09-25), a query designer that needs no syntax: a pane
showing a search has a row of chips under its address row (Name, Kind,
Modified, Size, Tag, Contains, In <folder>), each opening its choices in a
popover anchored to it (the kinds, periods from Today to Older than a year,
sizes, Custom… for a value to type). A choice refines the search and moves
the pane, so results follow every click and Back undoes it; a set chip
reads its value and has a × and a Remove item. An empty search (Enter with
nothing typed) is every entry below the folder, which the chips then
narrow. The query plugin owns the meaning (`query.describe`,
`query.refine`, `facets.rs`), so the CLI and agents get the same chips as
data; a part no facet holds (an OR across facets, a NOT, `ext:`) is a chip
of its own showing its text, so nothing typed is lost. The Tag chip picks
from the tags on the files the search found (`query.tag_choices` through
`ui.choose`, reading them with `tags.list { read }` on a worker; there is no
index of tags to ask, by design), most used first, or one typed; with the
tags plugin off it asks for one. Find Similar, on a file's menu, searches
where the pane searches for files of the same kind, modified the same day,
of about its size (half to twice) or sharing a tag, built on a worker from
the file's own properties. A keyboard click fires only on the element its
key went down on (vendored patch 0007): a picker closed with Enter gave the
focus back to the chip that opened it and the key's release reopened it.
Tabs (2026-09-26, ADR-018): a tab is an arrangement of panes, one or two
when that tab is split, and the core workspace owns them (`Workspace::tabs`,
`bibli_core::tabs`, the `tab.*` commands, so scripts and agents list, open,
show, move and close them). A tab not shown is dormant: its panes keep
history, sort and selection and let go of listing, watchers and kept
columns; showing it lists again and gives the selection back once the
listing is complete, scrolled to. The strip appears above the panes with
two tabs, names each after its place or folder, takes drops like a sidebar
row, reorders by dragging, and closes with its ×, a middle click or
Ctrl+W, answered by a toast with Undo (Ctrl+Shift+T reopens with the
history). Middle-click on a folder, a place or a breadcrumb, or "Open in
New Tab", opens it behind the current tab. With `core.startup = last` the
tabs come back at startup, only the shown one listed. The shell no longer
makes or drops a pane's UI itself: `Shell::sync_tabs` follows the
workspace's revision, which also gave closing a split one release path.
Two keys moved (the user's choice): Open Terminal Here from Ctrl+T to
Shift+F4, Tags… from Ctrl+Shift+T to Ctrl+Shift+G. Measured: ten dormant
tabs cost no PSS beyond noise over one (ADR-018 addendum, now a budget).
Switching tabs no longer draws a blank or half-listed pane (ADR-018
addendum): the tab left stays drawn until the tab shown is listed, has its
selection, scroll and first rows' metadata back, for at most 150 ms; every
measured switch (up to 100 000 entries, 89 ms) was ready within it.
A tab shown again is scrolled to the pixel it was. "Free the memory of
hidden tabs" (`core.sleep_hidden_tabs`, on by default) can keep every tab
listed instead, for instant switches at the cost of memory.
Open: `listing::pending_opens_report_errors_asynchronously_and_can_be_replaced`
is flaky ("cancelled open must not publish late events"): it failed eight
runs in a row at the parent commit and on this work, then passed in later
full runs; not investigated here.
Terminal (2026-09-26, ADR-019): F4 opens a terminal under the panes, as in
Dolphin: the new `terminal` plugin runs the shell on a pseudo-terminal with
`alacritty_terminal` and publishes core's `terminal::Screen`, which the UI
draws with an element of its own; the engine never leaves the plugin, so
libghostty-vt can replace it there once it is stable and builds without
fetching sources. The pane and the shell follow each other: the plugin
types a `cd` when the pane goes elsewhere, only while the shell has the
terminal, and a `cd` typed by the user takes the pane there (read from
`/proc`, any shell). While the terminal has the keyboard every window key
is the shell's except F4 and the palette. Measured (ADR-019 addendum):
+0.5 MB stripped binary, no idle cost with the panel closed, +2.3 MB when
opened. Not yet: mouse reporting to programs, an input method's text while
composing, search in the history. Found on the way: no part of the
window's shape was restored at startup (at the parent commit too), because
presenting the first tab wrote the default shape over the saved one before
it was read; `remember_session` now writes nothing until
`restore_session` has run. Open: on Xvfb a restored panel (preview or
terminal) appears only at the first input after startup; not investigated
on a real compositor.

The context menu follows the file (2026-09-28): a menu item's `when` can say
what the selected entries are, `kind:image` (every one) or `has:.gpx` (one at
least), by kind, MIME type with its parents, or extension, from their names
only; Geotag from Track now needs pictures and a track, Remove Location
pictures, and both left the More group. A plugin can add items made when the
menu opens (`registry::MenuSource`, a command answering from memory):
`open-with` names the default application in Open ("Open With Image
Viewer"), from a catalog it reads on a worker; the others stay behind
Open With…. The default is the desktop's rule (XDG MIME Applications, as
gio reads it): the default a list names for the type or a parent, else the
first application a list added or an entry declares. Open starts that
application itself: `xdg-open` on Hyprland reads only named defaults and
`mimeinfo.cache`, so a picture the user had KDE remember for Gwenview
opened in a browser. `xdg-open` remains for what the catalog opens nothing
with. Found on the way: the MIME database
takes 55 to 85 µs to type one name, whatever its shape (release build), so
the menu types a selection with `mime::by_names`, once per suffix beyond 64
names and at most 256 questions. Open: the list view still asks it for each
row it draws, a few milliseconds a frame.

---

Video playback follow-up (ADR-008): `preview-media` now opens an optional
GStreamer worker session, initially paused, with play/pause, seek dragging,
volume/mute, speed, audio/subtitle tracks, fullscreen and keyboard controls in
the shared preview frame. `auto` retains FFmpeg still previews when playback is
unavailable; explicit GStreamer selection reports missing dependencies. The Nix
shell includes playback plugins and `just run` builds the worker. Frame delivery
uses one reusable RAM-backed slot and a latest-frame mailbox; provider disable,
selection changes and closing cancel the session without waiting on the UI.
The worker owns A/V synchronization, and events wake the UI. Output is capped
at a 1920-pixel side; native-resolution 4K output, GPU buffer sharing, external
subtitle-file selection and a second backend are deferred. The synthetic check
in `tools/media/check.py` covers paused seeking, two audio tracks, embedded
subtitles, volume, rate, end-of-stream and replay. Validation and measurements
are recorded in `docs/video-playback.md`. Lint, 148 default tests and the
filesystem/thumbnail benchmark gate pass. Native checks cover both resolutions
and the real audio backend with muted output; audible lip sync is unverified.
The historical UI memory gate remains open. Native checks also compare paused
pixels with subtitles on/off. Track changes re-preroll and restore position and
play state, avoiding paused output-reconfiguration races.

Video-switching crash follow-up: rapid selection changes exposed a Blade atlas
lifetime bug in vendored GPUI. An evicted texture could still have queued
initialization/uploads, and its index could be reused before those uploads ran.
Patch 0003 cancels queued work before recycling the index and retires native
textures after the previous GPU submission completes. Two explicit GPU
regressions cover removal before presentation and reuse by a smaller texture;
`just atlas-check` runs them without adding GPUI dev dependencies to the workspace.
Lint, 148 default tests, both GPU tests and the benchmark gate pass. The release
app survived 320 rapid video selections and reaped its workers on preview close;
the unpatched build reproduced the reported panic after 49 switches. Scrolling
50 pages each in the 10k-image grid and 100k-entry list also passed; the final
five-second idle CPU sample was 1.4% of one core.

Video interaction follow-up: seek intent remains visible across mouse release;
both status and frame messages reject older command generations. Flushing seeks
retain the target until the entire pipeline finishes preroll. Initial paused
previews retain the shared thumbnail service's image until Play or Seek.
Fullscreen uses a black, aspect-preserving canvas that upscales small sources,
with an overlaid toolbar collapsed by its button or C. Real-window checks verified
320×180 upscaling to a 2560×1440 display, black side bars for 320×240 video,
controls, restoration of window size, and a stable seek bar across mouse release.
Five repeated native seek/track tests plus a 1080p run passed. Lint, 148 default
tests and all microbenchmark budgets pass; the optional native-tool tests remain
skipped in the default suite.

Video poster/controls follow-up: fullscreen controls use a cancellable one-second
inactivity timer; pointer movement reveals them, while hover, scrubbing, open
menus and keyboard focus prevent hiding. A cached native-resolution FFmpeg
poster replaces the thumbnail asynchronously, bounded to 8192 pixels per side
without upscaling. Starting playback or seeking cancels extraction and rejects
late results. Posters sample a representative timestamp independently of the
desktop thumbnail, whose timestamp is unavailable. Native checks verify exact
3840×2160 output and cache reuse, no upscaling of small input, and cancellation.
The separate Hide/Show button is removed. Real-window checks cover idle and
playing video, movement resetting the deadline, hover protection, menu focus
and dismissal, and fullscreen exit. Lint, 148 default tests, two explicit native
video tests and the benchmark gate pass.

Reading fidelity (2026-09-24, ADR-012): reading mode keeps the part of a
document's CSS that says how text reads, through a cascade over the parsed tree
with the selector engine scraper already uses: a book's class-borne italics,
centred titles, verse indents and hidden text now show, in the reader's theme.
XHTML chapters are read as XML (a self-closed `<title/>` emptied the chapter);
every `id` is a link target, footnotes gather at the end of their chapter and
page numbers leave the text; SVG cover pages and drawings show; a fixed-layout
book of pictures shows page by page. A book's or a document's pictures are
decoded as they come into view (ADR-012 addendum), so a chapter shows every
illustration it has. Superscripts, subscripts and small capitals are drawn
(vendored gpui patch 0006: a run's own size and height in its line), and a
note reference opens its note in a popover, from another chapter too.
Fixed-layout pages are drawn (ADR-013): a converted comic's lettering, set
over its picture in the book's fonts, comes out in its balloons. MuPDF is a
third, optional reader engine (ADR-014, AGPL worker), chosen explicitly and
compared with PDFium and Poppler in `docs/pdf-comparison.md`. Lint and the
default tests pass; a synthetic book was checked in a window on an isolated
display, including following a footnote and its way back. MuPDF is also a
book engine (ADR-014 addendum, `preview-epub.backend`, built-in by default):
it lays books out into pages for the reader, with the desktop's fonts
(fontconfig) for named, generic and uncovered-script text. Ctrl++ / Ctrl+-
work on every book: reading mode sizes its text, a drawn page or a comic
is shown as a picture (Fit by default, Fit width, zoom, as images are,
never in the reading column), and a MuPDF book is laid out again at the new size and reopened on
the page holding what was being read (`Locate`). Deviation: the crate's
`system-fonts` feature was dropped for a loader of our own, because its
`font-kit` broke the whole-workspace build. Verified in a window on
synthetic books and a comic; `just mupdf-check` covers layout and locate.
The reading column's width is the Reading width preference (Narrow,
Comfortable, Wide, Full width), shared by books and Markdown, with a Full
width button beside the text size, where a document has Fit width; zoom steps on any picture start
from the scale Fit shows, so Zoom out never enlarges. A picture zoomed
past its pixels (a drawn page, a comic's page, an image) is drawn or
decoded again at the side it covers, in 512 px steps up to 4096 px, where
it is; 100% is its own size, not its pixels (an image's Details now give
its real dimensions). Verified in a window on synthetic pages: a drawn
page at 400% redrawn at 3584 px, a 3000 × 4500 comic page at 100% at
4096 px, a 5000 × 3000 image at 48% at 2560 px. Then (ADR-015) books of
pages and comics became documents: a fixed-layout EPUB and a CBZ open in
the reader, continuously or a page at a time, through in-process sessions,
and the inspector's page view went. The reader renders every page at the
side it covers (512 px steps up to 4096 px, for every engine) instead of a
fixed 2048 px, and a page's size is its own (CSS pixels), so 100% is real
size. Verified in a window on synthetic files: a fixed EPUB and a comic in
both modes, a zoom from Fit redrawing pages at 1024 to 2560 px, a PDF at
100% at its size in points; all three PDF engines pass `tools/pdf/check.py`. The
reading toolbar now mirrors the reader's: a chapter field between previous
and next (the reader's page field, shared), a text size picker between
smaller and larger, Full width where Fit width is, and the rendered/source
switch last in the view group, where Continuous is. The desktop's
thumbnail no longer flashes before a preview: it waits aside and stands
in only once a load has lasted 200 ms, where the preview will be (an
image's at its own size, a document's as its first page), and stays as
the picture of a file with no preview of its own. A book's Contents are its
table of contents alone (no landmarks, no printed page list, no second
NCX copy), and a book that lists its printed pages can be counted by them
in the toolbar (`ui.printed_pages`); the reader does the same with a PDF's
page labels and a fixed-layout book's page list, read by all three
engines.

Music (2026-09-22): the audio preview became a small player. Sound: a
perceptual volume, `scaletempo` so speed keeps the pitch, a spectrum folded
into logarithmic bands. Tracks: Previous and Next through the folder's audio
files with hand-over at the end of a track; playlists (M3U, PLS, XSPF, CUE) as
a plugin whose tracks open in the player, which then walks the playlist. Keep
playing: a pin that lets playback outlive the selection, steered from the
status bar. What plays is core data (`playing::NowPlaying`, `PlayerRequest`),
which an `mpris` plugin mirrors onto the session bus: media keys, the
desktop's widget, `playerctl`. ReplayGain is a
setting, and a pinned player hands over without a gap: one worker carries the
queue, told the next file ahead of time. Verified in a window on
an isolated display and, for MPRIS, on a private session bus with `playerctl`.

One preview frame (2026-09-21, `docs/preview-frame.md`): the structured
previews were built apart from the reader, the player and the picture viewer,
and it showed. What is read is now on paper rather than the panel's grey; one
toolbar builder places every kind's tools in the same slots, with a More menu
that holds only what has no button; find, the contents drawer and the keys are
one each; every text file is the code view and HTML opens in reading mode;
Details lists everything known about an entry, filling the panel when there is
nothing to preview; a member of an archive or an attachment opens as the file
it is, two containers deep, which lifts the "nested sessions in containers"
limit noted below; tables have a grid, sized columns and sorting, the byte view
decodes the selected row. Deviations: no EXIF (no metadata crate in the tree),
no folder total from the Usage cache (a plugin the UI cannot depend on), no hit
count in the structured find field (providers search as they go). 283 default
tests and the bench budgets pass; every kind was checked in a window on an
isolated display.

Extended previews: ten independent runtime plugins add audio,
hexadecimal, Markdown reading mode, archives, EPUB reading mode, CSV/TSV tables,
JSON/YAML/TOML trees, font specimens, email and comics. Audio reuses GStreamer;
archives/comics use the optional libarchive helper. Semantic content is rendered
by a shared virtualized GPUI viewer. EPUB publisher CSS/fixed layouts, encrypted
archives, full-file audio waveforms and nested PDF/video sessions in containers
remain outside this initial implementation. See ADR-009 for dependencies, bounds
and behavior; `docs/previews.md` describes controls and installation. Strict lint,
163 default tests (six optional tests skipped), and all microbenchmark budgets
pass. Native checks exercise archive member reads, output limits and symlink
rejection, plus audio tags, spectrum, playback, seek and pause with a fake audio
sink; the paused audio worker measured 0.0% CPU and 12.6 MiB PSS. The existing
native video regression also passes. Real-window checks cover all ten formats,
text selection, search focus, EPUB links, section navigation, archive members,
worker cleanup and scrolling images10k/grid and flat100k/list. A settled debug
UI sample after the latter measured 2.2% CPU; this does not establish the release
UI idle/startup budgets or audible playback quality on a real device.

Archive presentation follow-up: contents use the file list's row density,
themed icons, aligned human-readable sizes, folder-first ordering and a summary
footer, with expand/collapse-all controls. Lint, 164 tests and isolated X11 UI
checks pass, including narrow-panel truncation and member preview/back navigation.
An initial benchmark run exceeded images10k first-batch (2664 us / 2500 us budget)
and completion (20363 us / 12000 us) timings; the cause was not established.
The subsequent ASF validation run passed all budgets (1663 us and 11753 us).

ASF preview follow-up: the media provider recognizes application/vnd.ms-asf,
application/x-ms-asf and the .asf extension. Provider ownership controls startup,
including when a generic binary MIME would otherwise select the hex fallback.
The GStreamer worker reports audio-only streams and enables their spectrum;
video streams keep the existing player. Lint and 166 tests pass, plus synthetic
WMV/WMA ASF playback/seek/pause checks and the existing native video regression.


Structured preview follow-up: JSON/YAML/TOML use syntax-highlighted code lines,
line numbers and parser-derived folds, with expand/collapse-all controls. JSON
gets two-space formatting; TOML/YAML retain syntax, comments, order and indentation.
Source mode, search and selection/copy remain available, including blank lines.
Parsing/highlighting stay on workers; the viewer virtualizes visible lines and
preserves logical indices when folded. Invalid or oversized input falls back to
paginated source. Lint and 174 tests pass (six optional native tests skipped);
isolated X11 checks exercise all three formats, folding, source and search.
The TOML grammar loads separately from precompiled default syntaxes to avoid
rebuilding them on first preview. The first benchmark run exceeded images10k
completion (12966 us / 12000 us); a rerun after concurrent builds finished passed
all unchanged budgets (9897 us). UI startup/idle budgets remain unestablished by
these functional checks.

Links (2026-10-03, ADR-029): addresses found in raw text (the code view, long
text) are followed with Ctrl+click and drawn as links only while Ctrl is held
over them; Markdown's bare addresses and plain-text mail are links like written
ones. The terminal shows the links programs write (OSC 8) and finds addresses
and paths (`src/main.rs:12:5`) in their output: Ctrl+click reveals a path in the
pane (the shell follows), Ctrl+Shift+click in a new tab, a line number shows that
line in the preview. Paths are read from the folder their line was printed in,
stamped into the grid by the plugin. Ctrl+Shift+O labels the links in view, in
the terminal and in a preview, for the keyboard. Every address goes through one
gate (`ui.open_url`, on a worker; the inspector used to call the desktop itself).
Not covered: table cells, Details comments, lines past a long file's first page.
Lint and 1033 of 1034 tests pass; the failure (`mime::tests::the_bytes_decide_for_a_name_that_says_nothing`)
compares with the system's MIME database, which now names a shell script
`text/x-shellscript`. Checked on an isolated X11 display: Ctrl hover and click in the
code view, Markdown and the terminal, labels in both, reveal with line in the pane
and in a new tab, idle CPU back to 0–1 %.

Text documents (2026-10-03, ADR-030): Word (`.docx`, `.docm`, `.dotx`,
`.dotm`), OpenDocument text (`.odt`, `.ott`) and RTF follow a Text
documents setting: Reading (the default), Pages (LibreOffice, as before)
or Pandoc, each giving way to the next when its program is missing.
bibli reads Word and OpenDocument itself into reading-mode blocks
(`preview-documents`: headings, emphasis, lists with the numbers their
document gives them, tables with merged cells, footnotes and endnotes
under their reference, links, pictures named for later, text boxes,
fields, accepted changes, Details from the package's properties), a
long document in parts of `PART_BLOCKS` with numbers counted from its
start. Pandoc reads RTF, or everything when chosen, in the converters'
sandbox (`bibli_platform::sandbox`, extracted from the LibreOffice
path), its extracted pictures kept in the plugin's cache. The grid shows
the picture of its first page an Office or OpenDocument file keeps
(`stored.rs`, a fallback before converted pages). Along the way the
reading mode counts `<ol type>` lists as their type says, ignores white
space between table cells, and leaves a gap after a list marker wider
than its column. Measured once on the reference machine, a small
synthetic Word file: LibreOffice 1.2–2.7 s and up to 370 MB, Pandoc
0.03 s and 120 MB. Not covered: `.fodt`, presentations and spreadsheets
as text, find beyond the part shown, links to bookmarks in another part.
Checked: lint; bench-check; 1050 of 1051 tests (the failure is the
system MIME database's `text/x-shellscript`, as before); the ignored Pandoc smoke
test, and LibreOffice's (nine formats through the extracted sandbox,
with the worker paths set); on an isolated X11 display, Word and ODT
files written by LibreOffice and by Pandoc, an RTF through Pandoc, a
3200-paragraph document's second part, a footnote's popover, a kept
first page in the grid, and Pages as the default.
Pictures by ImageMagick (2026-10-03, ADR-031): a picture no desktop
thumbnailer draws (AVIF, HEIC, JPEG XL, JPEG 2000, raw files, PSD, XCF,
DjVu, as the installed build reads them) has a thumbnail and a preview
from ImageMagick, the last thumbnail provider of all (`Stage::LastResort`,
new in the core) and the preview's after the thumbnailer: the format
named from the MIME database's globs (`mime::extensions`), never one of
`magick::REFUSED`, run only in the sandbox under a policy written per
run (that format read, PAM written, no delegate, 256 MiB, one thread),
two at a time (`thumbnails.imagemagick`, on by default). The desktop's
thumbnailers now run in the same sandbox where bubblewrap can isolate
(`sandbox::available` asks it once), as before where it cannot; the
sandbox names its file from a plain extension (`sandbox::inside_name`)
and binds `/etc/alternatives`, the fontconfig cache and a program kept
outside the system's folders. Declined: AVIF decoded in the process
(rav1d: 2.7 MB of code with its assembly) and KDE's thumbnailers (a
Qt helper for one desktop). Measured on the reference machine, a
synthetic 12-megapixel AVIF to 256 px: ImageMagick 0.72 s and about
213 MB, `heif-thumbnailer` 0.57 s; ffmpegthumbnailer isolated about
5 ms slower than alone. Checked: lint; bench-check; 1059 of 1060 tests
(the system MIME database's `text/x-shellscript`, as before); the
ignored ImageMagick test (an AVIF with transparency drawn isolated, a
file that is none refused); a fake ImageMagick behind a failing
thumbnailer, isolated; on an isolated X11 display, the grid's
thumbnails of AVIF (one transparent, one named with brackets and an
apostrophe), HEIC, JPEG XL, PSD and JPEG 2000 by ImageMagick and an MP4
by the sandboxed ffmpegthumbnailer, all written to the shared cache, and
an AVIF's preview.

Tags as places (2026-10-03, ADR-032): a location that is no folder can list
places, entries that are locations of their own (`EntryId::place`), read as
a trail of places that Up walks back, and say what gestures mean there
(`Verbs`: a drop, Delete through `take_out`, New Folder's key, F2 on a
place). The tags plugin's `tags:` location is the tags of a folder browsed
as folders: a tag lists its tags as places and the files tagged with it
exactly (Include Sub-Tags lists everything within), what narrows it (also
or not tagged, a kind, a year) stays while its trail is walked, files
dropped or pasted on a tag are given it, moved from another tag they lose
that one, Delete takes the tag off while Move to Trash still trashes, F2
renames a tag everywhere, Ctrl+Shift+N makes a tag. It works in every view:
the tree opens tags in place (what a pane opens in place is now a location
at an entry), the columns walk the tags above. The sidebar's tags and All
Tags open tag folders; the Tags view and `PaneView::command` are gone. The
shelf takes the same verbs: Delete unshelves. Lists of files from many
folders gain a Folder column and Show in Folder (Ctrl+Alt+O). The extras
of the design came with it: a tag dropped or pasted in a folder is
exported as a folder of its files; a tag's grid tile shows a few of its
pictures; the board (an alternate view) lays out a place's places or a
folder's folders as columns of cards, a card dragged across moving there;
and any folder can be narrowed by the Narrow by bar (the View menu's
Narrow by, `query.narrow`), kept below the folder it was set in. Checked
in a window on Xvfb: browsing, Up, the menus, Delete and its Undo, F2,
New Tag, drags onto tags and across the board, the columns and tree over
tags, mosaics, and a folder narrowed, kept below and dropped above it.
Not checked in a window: an export by a real drag (tested through the
clipboard), the board's keys, and the Narrow by bar over a very large
folder, which reads every file's metadata as the timeline does. The bench
check of tags now goes through a tag location (1.5 s against its 4 s
budget).

The sidebar's places and the trash (2026-10-04, ADR-034): the places a
plugin offers (Home, the desktop's folders, Recent, the Shelf, the Trash,
a volume) are the user's to move, rename and hide in the sidebar
(`ui.sidebar_place`, kept with the window's shape), those it suggests to
hide, those the user keeps their owners' as before; a group's header
lists its places with a check while shown. A volume is named by its
filesystem, so its arrangement holds mounted or not, and one udisks says
not to show (an EFI partition, a recovery one) is offered hidden. F6
gives the sidebar the keyboard (arrows, Enter, Menu, Alt+arrows, F2,
Delete). The trash plugin's `trash:` lists the home trash and the
volumes', each file under its old name from its old folder: Restore,
Delete for good (the core's Delete takes the `.trashinfo` along), Empty
Trash, files dropped on it or its place trashed; Move to Trash, Rename,
Cut and Show in Folder are not offered there, and rename, move and swap
refuse a trashed file. Any user plan Undo cannot take back now asks
first. Checked in a window on Xvfb with a synthetic home: hiding from
the header's checklist and the place's menu, Undo, renaming, a drag
reordering Places, the arrangement after a restart, the keys (F6, the
arrows, folding, Enter, Menu, Alt+Up), F1's list, the EFI partition
hidden, the Trash listing, Restore, Delete with its question, and a file
dropped on the Trash. Not checked in a window: Empty Trash (in tests
only, as the machine's own volume trash was listed beside the fixture),
a drag between groups refused. Not done: a Deleted column or sort, and
restoring a file by dragging it out of the trash (refused: restore it
first).

Every location that is no folder reads as a trail (2026-10-04, ADR-032
amended): the shelf and Recent showed their text (`shelf:`, `recent:`)
in the address and now read as one place, as the trash did; a provider
that gives no trail gets one place, its name (`App::trail`). Its provider
says how it is shown (`LocationProvider::title`, `icon`, `empty_state`,
`Trail::holds_places`): the shell's choices by scheme are gone (a tab's
and the history's glyph, the empty pane's words, the search's "Nothing
found" read from the last text typed), and a chevron after the last
place is drawn only where it holds places, so the trash's no longer
opens an empty menu. Recent has a glyph and words of its own. Checked in
a window on Xvfb with a synthetic home: Recent, the Shelf and the Trash
in the address, the empty Shelf and Recent, the history's glyphs, a tag
location's trail and words, and an empty search. Left by scheme in the
shell: the search's pills (the query plugin's `query.describe`).

A location's buttons are its own (2026-10-04, `Verbs::tools`): the
shelf's Move and Copy to the other pane, which the shell drew by scheme,
are the shelf's tools, and the trash has one, Empty Trash, in its pane's
row. A tool says the pane it runs with (`ToolPane`: the one shown, or
the other pane's folder) and can be pressed only while its location
lists something and, for the other pane's folder, while the other pane
shows a folder: not when both panes show the shelf, where they did
nothing but fail. A button names the pane it acts on when it has to
(`toolbar::invoke` adds its own only when none is named). Checked in a
window on Xvfb with a synthetic home: the shelf's buttons without a
split, with both panes on the Shelf and with a folder beside it, Copy
into that folder, and the trash's button and its words. Not pressed in a
window: Empty Trash, as the machine's own volume trash is listed beside
the fixture (its plan, run with a pane, in tests).

In the trash, and from the sidebar's places (2026-10-04, ADR-034
amended): a trashed folder opens in the trash (`trash:` and its path,
`Trash › old project › sub`) instead of at its path under
`.local/share/Trash/files`, where the pane offered New Folder, paste and
Move to Trash, and where a file could be put in the trash a second time;
a location may now say where its folders lead (`location_of`, asked of
folders too), so opening, a tab, the tree and a drop on the row go
there. A file of a trashed folder is restored alone, its folders made
again as they are in the trash (`RestorePart`, undone by `ReturnPart`),
and a folder restored into one there again merges into it (`Unmerge`
undoes it); a drop or a paste there trashes, and Trash refuses a file of
the trash at any depth. A folder's trail starts at the deepest folder
the sidebar shows that holds it, under the sidebar's name
(`sidebar::roots`): Documents renamed Papers reads `Papers › 2026`, and
its tab and the history say Papers too. Checked in a window on Xvfb
with a synthetic home: a trashed folder and one inside it opened in the
trash, its menu (Restore, Delete Permanently, no Move to Trash or
Rename), a file restored alone with its folders made, Undo, Alt+Up back
to the Trash, a place renamed in the sidebar in the trail and in a tab.
Not checked in a window: a merge (in tests), a drop on a trashed folder,
a drive's trail (in tests: the machine's drives are real ones). Left: a
folder restored by merging leaves the companions trashed beside it in
the trash; the CLI and agents do not see the sidebar's names.

Up the tree from a trail (2026-10-05): with no Up button, the folders a
trail leaves out above its first place were reached only by Alt+Up. A
trail that starts at a folder of the sidebar (Home, a bookmark, a drive:
anything but `/`) now leads with a chevron whose menu lists every folder
from the root down to it, as a tree, each under its own name as the
other chevrons list folders (`/ › home › example`, not the sidebar's
Home; `ui.parents_menu`). A drive's shows where it is mounted
(`/ › run › media › …`); leaving drives out, as first built, needed the
mount table read on a worker and hid the chevron from a drive mounted
inside the home folder. The chevron is the first segment's own, so a
narrow pane wraps it out of sight with that segment. Checked on Xvfb
with a synthetic home: the chevron, its menu and a row opening its
folder, none at `/`, a second click closing it, and the narrow pane.
Left: the segments a narrow pane wraps out of sight are not in that
menu, as they are in Dolphin's.

A review for consistency, and its first fixes (2026-10-05): nine
read-only reviews looked for the kind of exception the trail's drive
root was (a second piece of code saying what another already knows) and
found about 170 places, grouped in ten patterns; they are fixed in that
order. First the bugs that stood alone: F1 and the strip of a key
sequence left out every binding with a `!` condition (Delete, Cut,
Rename), and F1 now lists only the binding a key runs where it is
pressed; Shift+F3 in a structured preview split the window (it finds the
previous match now, with a button, and Escape closes its find); the
middle button and Ctrl+Enter opened nothing on a tag and a trashed
folder at its raw path (`tab.open` with `ids`, through
`Pane::leads_to`); "Tags out of Name" could rename two files to one
name and fail halfway (`order_renames`); a changed Documents or Books
preference closed the document open in the reader (only the provider it
concerns starts again); the table reader and geo shared a priority, so
a Litchi mission's `.csv` never showed as a map (geo outranks it now,
and the test that keeps providers apart asks every preview plugin the
binary wires, `Previews::accepting`); vim's `p`, `d`, `x`, `r` and `D`
had copied the standard keys' conditions, which had moved on (`keys.like`
takes a standard key's bindings instead); Enter in the address before
any edit took a search's or a filter's words for a path; the address's
places were stat-ed in render; Open With typed a file by its name, so a
pipe named `x.png` went to the image viewer (`apps::type_of` takes the
entry, and a pipe, a socket or a device is never handed to the
desktop); bibli's own `ffprobe`, `pdfinfo`, `pdftoppm`, `ffmpeg` and the
archive and model helpers read files outside the sandbox
(`sandbox::untrusted`, the one way, with `Otherwise::Refuse` or `Run`);
files dropped from another application applied in part; `bibli trash:`
opened `./trash:`; the View menu offered Zoom where nothing zooms
(`PaneView::zooms`); a preview's menu opened again on its own button's
second click, and a folder read late could replace the history menu.
Checked on Xvfb with a synthetic home: `bibli trash:`, F1 with a file
selected, the View menu in Board, Shift+F3 and the previous match in a
JSON preview, Enter on a filter's pill and on a search unedited, a
preview menu's button clicked twice. Not checked in a window: the
middle button on a tag (core path in tests), vim's keys, a drop from
another application.

The sidebar from its places' own answers (2026-10-05): a sidebar row
said again what its location says, and the copies had drifted. A place
had a `drop` of its own beside its location's `Verbs::drop`: the Shelf's
was missing, so a folder dropped on the Shelf, Recent, a saved search or
All Tags fell through to the column and was bookmarked, and a tag's pill
added the tag where its folder in a pane moves files out of the tag they
came from. `Place::drop` is gone: a row takes what its location takes
(a folder's row is a folder; any other runs `Verbs::drop`), one that
takes nothing swallows the drop, and a pill dragged onto files gives
what its location takes. Owners listed Rename… and their own Remove item
in a place's menu beside `rename` and `forget`, in three wordings; the
sidebar now builds Rename… and Remove From Sidebar from those fields for
every place, and F2 renames a suggested tag as its menu does. The
trash's top said it held no places although its chevron's menu listed
them inside a trashed folder; it holds them now. The names the user
gives places in the sidebar were the UI's alone, so a renamed Trash
kept its name in the close toast and `tab.list`; the sidebar publishes
them to the core (`Registry::set_place_titles`), and the tab, the
history, the trail, the address's search pill, the preview and every
message name a place as the sidebar does (`App::location_title`, the
UI's own `location_name` gone). A tag's search was kept as the tag's pin
by two branches in the shell, around the keepers; they are gone, and a
keeper says which of its locations it keeps (`Keeper::keeps`: a tag
folder of the home folder, not narrowed), so the pane's button is not
offered at All Tags, where Ctrl+D refused. Checked on Xvfb with a
synthetic home: a folder dropped on the Shelf shelved, a file dropped
on Recent doing nothing, a bookmark's menu, the Trash renamed Bin in its
trail (with its chevron), its tabs and the close toast.

One surface for the shell's commands and one base for its lists
(2026-10-05): the shell's own commands (`ui.*`) were names matched in a
150-arm switch, invisible to `commands`, agents and scripts, with their
arguments read by hand and defaults guessed for what they did not
understand (`ui.sort {key: "type"}` sorted by name). They are declared in
the registry like every command (`commands.rs`: a title, what it does,
the schema of its arguments), checked before they run
(`CommandRegistry::check`), and one asked from elsewhere waits for the
window (`App::request_front`); a user's shortcut naming a command none is
called, or giving it arguments it does not take, is refused when saved.
What repeated another went: 22 cursor commands (`ui.cursor` with `to` and
`step`), `ui.confirm_delete` (`delete`, which asks as any plan Undo cannot
take back does), `ui.open`, the shell's Rename, New folder and New file
prompts (`ui.ask`, with a `select` range; `new_file` makes a folder of a
name ending in `/`), the reader's find, copy, select all, contents, page
field and match next to the structured preview's (`ui.preview_find`,
`_copy`, `_select_all`, `_contents`, `_goto`, `_match`, one key each in a
`Preview` context set while the preview shows a document or a structured
preview, both find fields one `PreviewFind`), the pinned player's mute
(`PlayerRequest::Mute`), commands nothing ran. Key places are one table
(`keymap::PLACES`: token, context, what F1 calls it), bindings name them
by token, and one evaluator reads a binding's condition. Popups: opening
any closes the one open and the press that closed a popup leaves its own
button's click alone (`Shell::toggle_popup`), so two never hold the
keyboard and the hand lists of which commands keep which popup open are
gone; the arrows walk a toolbar popup's rows; the palette, a choice,
Jump, the tag editor and the address's suggestions move by one command
(`ui.list_move`, the page keys too) and filter by one function; a pill's ×
is one element with a tooltip, and the narrowing pills became buttons the
keyboard reaches; a tag's menu is one (`tags.tag_menu`, a source of the
places' and the tags' menus), the same on its pill, in Details and in a
tag location. Checked on Xvfb with a synthetic home: the arrows, Shift and
Ctrl+Space in the list, F2 with the stem selected, New folder, Shift+Delete
asking, the arrows in the View menu and the palette, a tag's menu on its
pill and in Details, Ctrl+F, F3, Shift+F3 and Escape in a JSON preview.
Not done: the current item is still marked two ways (a check for an
option, a highlight for where one is); Home and End do not reach a
toolbar popup's ends.

An entry drawn one way, in every view (2026-10-05): the board's cards
and the columns' rows above the folder shown were built by hand beside
the list's rows and the grid's tiles, and four collectors read an entry
four ways; a card had no link or tag mark, no dimming while cut, no
middle ellipsis or tooltip for a long name, no "+N" for its tags, no
menu marker, and was not dragged as the selection nor took a tag
dragged onto it. Each view now reads an entry once (`Shell::look`) and
draws its own shape around one set of pieces (`views::entry`: the
picture and its mark, the highlight, the name, the tags, and `behave`
for what an entry answers); a card dragged to another column is a drop
like any other, told the tag it leaves. The list's detail columns were
five parallel arrays and magic indices (the menu hid Rating as
"index 3"); they are one table (`views::list::COLUMNS`) the header, the
rows, the header's menu, the View menu's sorts and Settings read, so the
header and the menu say Modified alike, every cut column says itself
whole when pointed at, and Rating is offered where ratings are kept.
Their dragged widths were a setting rewritten on each drag and read on
every settings change; they are the window's shape now (`Session`, by
column, the old setting carried over). The seven edges that size an area
(the sidebar, the preview, the split, an archive's view, the dock, the
terminal, a list column) are one element (`elements::edge`): the list's
had no hover and no double click back to its width, and stood over its
column's label (it is in the gap now). The board's keys stepped through
the pane's order onto the columns' own entries and never scrolled to the
cursor; they move along a column and across to the same height, with
Shift extending, and the row of columns has a scrollbar along its
bottom (`scrollbar::horizontal`). A narrow pane in the Columns view cut
the folder shown first; it keeps it and cuts the folders above.
Smaller: counts said "12345 files" and "1 items" (`format::count`),
dates were formatted three ways (`format::time`, `exact_time`, `day`),
the rubber band's colors were derived locally (`Theme::band`). Checked
on Xvfb with a synthetic home: the header's menu, Rating shown, a column
dragged narrower with its cut types' tooltips, the width kept across a
restart and reset by a double click, sorting by Modified from the View
menu, a tag category as a board (cards with their chips, the arrows,
Shift+Left, a row of seven columns scrolled by the keys with its bar),
the Columns view in a split, the split's edge dragged and reset.

Safe reads, and one helper where there were several (2026-10-05): files
another application keeps beside a file were read whole, as they were:
a pipe named `x.jpg.xmp`, `.ts/x.json` or a volume trash's `x.trashinfo`
held the worker that read it, and a large one was read into memory.
They go through one read (`fs::file::read_regular`, the core's, which
the platform re-exports with `open_regular`): a regular file only,
within what is expected, refused rather than cut (a `._` file is
written back whole, so one cut was worse than none). A PDF's links to
mail, to a file beside it or to a folder were refused, and the map's
attribution opened its address itself: every document's link now goes
where it says through one follower (`Shell::follow_document_link`:
addresses through `ui.open_url`, a place on this machine shown in its
folder, in a tab with Shift). A worker named by its variable was taken
on faith, so a probe said "installed" and the engine chosen
automatically failed instead of trying the next; a worker is now found
where it runs (`process::Worker`, one per helper, which also gives its
requirement and its "Install … to …" message), and one that exits 127
before greeting (its library missing) gives way to the next engine, as
the player already did (`LineChannel::missing_runtime`). `plocate` ran
without a timeout, a bound or the search's cancellation. The workspace
called a location's `path_of` under its lock in some places and outside
it in others, with a release choreography to close panes after it; the
rule is now written on `OpenLocation` (those calls answer from memory,
so they are made under the lock) and the choreography is gone.
Then the helpers: a disabled plugin's worker could still publish a
count, a cut or a provider (the gate now covers every store a plugin
publishes in); a location given as a command's argument four ways, two
of them losing a name that is not UTF-8 (`Location::arg`,
`Invocation::with`, `Invocation::navigate`); an operation's kind said
twice (`op::Kind`); two `Exec` tokenizers and three key-file readers
(`keyfile`); two mountinfo parsers and three ideas of "removable" (one
of each: a USB drive mounted under `/mnt` is drawn as removable, and
Eject stays for what udisks mounted); three hand-written "latest answer
wins" (`latest::Latest`); 18-field place and menu-item literals
(`Place::new`, `MenuItem::new`); five resolvers of "which files"
(`App::targets`, and "nothing selected" typed everywhere); three
sidecar-name caches (`companions::FolderNames`); five "toast with Undo"
copies (`App::notify_undoable`); two settled watchers (`fs::Settled`);
five zip readers, one without a bound on its members
(`platform::package`); the JSON-lines framing three times (`jsonl`).
Checked on Xvfb with a synthetic home: an XMP sidecar's tag and rating,
a pipe named like a sidecar not holding the listing, a Markdown link to
a file beside it selecting it, the Trash listing following a trash and
its Undo. Not checked in a window: a PDF's mail link, a missing engine
library, plocate.

No plugin's name in the shell, a folder as a location, and types from
the MIME database (2026-10-05), the review's last three patterns. The
shell asked plugins by name what any location or provider could say:
whether a location is a search (`LocationProvider::searches`, for its
"Searching…", its count and its address), whether ratings are kept
(`NoteStores::held`, for the Rating column; rating, comment and making
stores agree are the core's `notes.*` commands, as the notes are), what
a tag pill does with Shift or Alt (`Place::clicks`, carried by the
place), what Copy Path puts on the clipboard (`Outcome::Clipboard`),
how many files wait on the shelf (`tally::Tally`, a count any plugin
publishes in the footer, opening what it counts), which preview shows a
file's bytes (`PreviewProvider::shows_bytes`), what F5 means in Usage
and who draws its empty state (`PaneView::refresh`, `draws_empty`), and
what kind a place is in the Type column (`keys::PLACE_KIND`). A folder
was a case beside every location: its trail is now a `Trail` like the
others, its chevrons list folders and places through one call
(`App::places_in`), Shift+F5 and Shift+F6 give the selection to what the
other pane shows as a drop there does (onto a tag: the tag), `App::release`
lets go of whatever holds a folder under a mount point, whatever its
scheme (`OpenLocation::holds`), and the map and the timeline count what
the pane shows (`Pane::order`: its filter too). Then types: a provider
took files by extension lists that disagreed with the MIME database and
with each other, so a `.mkd` had no Markdown, a `.jar` no archive, a
comic no TIFF page, a `.ppsx` a thumbnail but no preview, an `.xlsb`
with Spreadsheets as Pages no preview at all, and a type named under
another name was missed (`audio/matroska` covers, `image/vnd.ms-dds`).
A provider now takes a request by its type (`mime::of_request`, from the
bytes where the name is ambiguous) against a kind or a `mime::Types`,
matched through the database's aliases and without case; office
documents are one list (`preview::is_office`) and "shown as pages" one
question that the tables and the documents ask (`shown_as_pages`);
playlists one table the player refuses and the playlist preview reads.
`LibreOffice`'s desktop entries were tried as the list and dropped: they
claim plain text, CSV and PDF. The XDG directories are read in the core
(`bibli_core::xdg`), the cache and settings paths included, and the
thumbnailers' NixOS folders beside `XDG_DATA_DIRS` are gone. The
settings dialog's pages are one table (`ui.settings` with `page: core`
reaches General). Checked on Xvfb with a synthetic home: a `.mkd` as
Markdown, a `.jar` as an archive, a text file as text and View as hex,
both kinds of trail chevron, Shift+click on a tag pill, Shift+F5 onto a
tag pane (the tag given, then undone), the shelf's footer count opening
the shelf, the timeline with a name filter (14 files, then 4). Not
checked in a window: F5 in Usage (its watcher refreshes it as well),
Alt+click on a pill, an office document converted.

Out of the trash, and what was left open (2026-10-05): a file of the
trash could leave it only by Restore; dragged to a folder it was
refused. A move of a file of the trash now takes it out there, as a
restore does (`Move` is a `RestorePart` to its place), so a drop, a
paste after Cut (offered in the trash now), Shift+F5 and Shift+F6 all
do it: an item under the name it had (the trash's `draft.txt.2` is
`draft.txt` again), with its `.trashinfo` and the companions trashed
with it, all back as the trash had them undone (`Untrashed`). A folder
restored into one there again brings its companions too
(`Unmerge::companions`), and folders a restore made read-only go when it
is undone. The trash has a Deleted column and sort (`SortKey::Deleted`,
newest first). A search names the folder it looks in as the sidebar
does (its pill and its title, read again when a place is renamed). The
first segment a narrow pane leaves in sight lists what is above it,
the segments wrapped out included (`PaneUi::trail_shown`, the layout's
answer read when its chevron is pressed), and Home and End reach a
toolbar popup's ends. One `.trashinfo` reader (`op::parse_trash_info`).
Checked on Xvfb with a synthetic home: the Deleted column and its sort
from the header and the View menu; Cut in the trash and Paste on
Desktop of the trash's `draft.txt.2`, landing as `draft.txt`, and its
undo; Shift+F6 out of the trash; a drag out of the trash and a drop on a
trashed folder (which trashes); a folder restored into one made again,
its `._` file with it, and the undo putting both back as they were; a
place renamed twice while a search in it was shown (its pill and both
tabs following); a narrow pane's chevron over a folder trail and a
trash folder's trail, a row of its menu, Home and End in that menu and
the View menu. Not checked in a window: a move out of the trash across
volumes (tests cover the same path for any move), Empty Trash (real
volume trashes are listed). Left: a row the keyboard reaches beyond a
popup's visible height is not scrolled into view (the arrows had that
already); "Add to Bookmarks" is offered on a trashed folder.

What that left (2026-10-05): a row the keyboard reaches in a popup
taller than the window scrolls into view (the arrows, Tab, Home, End:
`toolbar::RevealRow`, the row scrolling its popup in prepaint and asking
for the next frame, since a refresh asked while drawing is dropped), and
a folder of the trash is not offered to the bookmarks, nor bookmarked
(`places.bookmark` refuses it: restored or deleted, the bookmark would
lead nowhere). Checked on Xvfb with a synthetic home: End, Up and Home in
a menu of sixty folders, the padding kept at both ends, and a trashed
folder's menu.

The Map view chooses its background (2026-10-05): only the map preview
had a Background button. The Map view's row ends with one too, naming
the background shown and opening the same rows (`map_preview::
background_rows`: each background, Manage backgrounds…) in the pane's
toolbar menu (`ui.toolbar_menu`, `map_backgrounds`), so one popup at a
time, Escape and the keys work as everywhere. Checked on Xvfb with a
synthetic GPX track: the button and its popup, a switch to a local test
server (refused, its toast) and back to Outline from the keyboard, and
the preview's own popup unchanged. Not chosen in a window:
OpenStreetMap, which downloads.

## Phase 0 — Foundations and toolkit spike (~2 weeks)

Goal: a repo that can measure itself, and a toolkit decision backed by numbers.

- Cargo workspace: `crates/core`, `crates/platform`, `crates/ui`, `crates/plugins/*`, `crates/bench`.
- Nix flake dev shell (this is a NixOS machine), `just` recipes, CI (lint, test, bench on Linux).
- Benchmark harness generating synthetic trees on demand (never committed):
  flat 100k-entry dir, 1M-file deep tree, dir of 10k large images.
- Written performance budgets, enforced by `bench` as CI regression checks:
  - cold start to first frame < 100 ms
  - 100k-entry directory: first rows painted < 16 ms after listing starts, scroll at display refresh rate
  - idle RSS < 100 MB with default plugins, identical with AI plugins disabled vs not compiled
- Toolkit spike: the same demo in **GPUI** and **Floem**: a virtualized 100k-row list with lazy
  thumbnails, on Wayland (Hyprland), on the Intel iGPU and the NVIDIA eGPU. Measure first paint,
  frame time while scrolling, RSS, and test external drag-and-drop and IME once.
- ADR-001 records the toolkit choice and the numbers. ADR-000 records the principles above.

**Done when:** budgets are documented, `bench` runs in CI, the ADR is written, and the winning
spike renders 100k rows at refresh rate on both GPUs.

---

## Phase 1 — Core kernel (~4 weeks)

Goal: `crates/core` is complete enough that plugins can be written against it, with no UI.

- **Entry model.** `EntryId` = (device, inode) on Unix, file id on Windows. Streaming listing via
  `getdents` batches (rustix), `d_type` used to avoid stat, lazy stat on demand. Entries carry a
  metadata bag: typed fields with **provenance** (Filesystem, User, Plugin(name), Model(name, confidence)).
- **Watching.** `notify`-based, per open location, coalesced, feeds the same entry stream.
- **Locations.** `LocationProvider` trait: string → entry stream. Only the path provider ships in core.
- **Commands as data.** `Command` = serde struct with typed args, registered by name, dispatched by
  the core. Keymap and palette are consumers of the registry. Nothing in a command captures UI state.
- **Operations and plans.** `Operation` trait with `apply` and `inverse`. `Plan` = ordered list of
  operations referencing `EntryId`s, with a dry-run `diff()`. Executor runs plans in the background
  with progress and cancellation and writes an undo journal. Re-applying an applied plan is a no-op.
  Built-in ops: copy (reflink when possible), move, rename (atomic exchange for swaps), trash, delete, mkdir.
- **Registries.** Views, panels, sidebar contributions, context-menu contributions, settings schema.
- **Plugin lifecycle.** `Plugin` trait: `register(&mut Registry)`, `enable`, `disable`. Compile-time
  crates, runtime toggle in settings. Plugins depend only on core, never on each other.
- **Cache and settings.** Content-addressed cache dir, deletable at any time. Plugins get a namespaced
  slot in cache and in settings, nothing else.
- **Work scheduler.** One priority queue: viewport-visible work first, everything cancellable.
  Provider calls are batched, async, timed; slow providers are surfaced.
- Tests: property tests that `inverse(apply(op))` restores the tree; idempotency of plans; listing
  benchmarks against the synthetic trees.

**Done when:** a headless test drives a plan (list, select, rename, trash, undo) through commands only,
and listing the 1M-file tree streams without ever blocking the caller.

---

## Phase 2 — Minimal daily-driver (~4 weeks)

Goal: replace the file manager you use every day, even if ugly.

- `crates/ui` on the chosen toolkit: window, layout with panes, theme tokens, command palette,
  keymap, address bar (paths only for now), status/progress area. **AccessKit wired in from the start**
  (blocked: gpui 0.2.2 has no AccessKit support, see ADR-001; re-check at every gpui upgrade).
- Plugins: `view-list`, `places` (XDG user dirs, GTK bookmarks, udisks2 volumes over D-Bus),
  `ops-basic` (the built-in operations exposed as commands with keybindings), `open-with` (xdg-mime),
  `undo` (journal UI).
- `crates/platform` Linux: shared-mime-info via the `xdg-mime` crate, XDG icon theme lookup,
  freedesktop trash, **read-only** freedesktop thumbnail cache lookup.
- Keyboard-first: every action reachable from the palette; mouse works but is secondary.
- Dogfooding starts here: use it daily, file issues.

**Done when:** you have used it as your only file manager for one week, budgets still pass,
and every action taken during that week went through a `Command`.

---

## Phase 3 — Visual richness and performance hardening (~4 weeks)

Goal: it looks and feels like the fast, GPU-native app it is.

- **Thumbnail pipeline.** `ThumbnailProvider` trait keyed by MIME. Freedesktop cache read first,
  generation on miss written back with spec-compliant naming (other apps benefit). Discovery of
  installed `*.thumbnailer` files. Decode on a rayon pool, viewport-priority, cancellable, GPU atlas upload.
- Plugins: `view-grid` (gallery), `view-columns` (Miller columns), `preview` (image, syntax-highlighted
  text, PDF via pdfium, video first frame; spacebar peek).
- Drag-and-drop: internal first, then external on Wayland; measured against the spike findings.
- Perf pass: profile with the synthetic trees, fix every budget regression, add flame-graph recipe.

**Done when:** the 10k-image directory scrolls at refresh rate with thumbnails appearing in
visible-first order, and all three views pass the budgets.

---

## Phase 4 — Power features, each as a plugin (~6 weeks)

Goal: the UX bets. Order is by leverage; each is independently shippable and toggleable.

1. `query` — query language (`*.pdf modified<7d size>10M`), a `LocationProvider`, so search results
   are a location. Saved searches are kept in the sidebar like bookmarks.
2. `search` — parallel subtree walk (`ignore` crate walker), results cached by directory mtime;
   uses `plocate` when present. Content search via the ripgrep crates.
3. `shelf` — collect entries from anywhere, apply one plan to all of them.
4. `rename-bulk` — patterns, regex, live preview; produces a plan.
5. `edit-as-buffer` — the directory listing as an editable text buffer, diff → plan (oil.nvim model).
6. `jump` — frecency jump reusing zoxide's db and recently-used.xbel when present; `nucleo` matching.
7. `shell` — open terminal here, mirror cwd, drop selection as arguments.
8. `tags` — xattrs using the XDG and Finder keys; tags are metadata with User provenance.
9. `view-dual` — orthodox two-pane mode over the same model.
10. `usage` — GPU treemap, btrfs-aware.
11. `timeline` — files by mtime as a scrubbable histogram.

By the end there are 10+ internal plugins. That count is the signal that the traits are ready to freeze.

**Done when:** every plugin above can be disabled at runtime with no trace in memory or UI, and
the trait surface has not changed for the last three plugins written.

---

## Maps (planned, ADR-020)

Goal: GPS tracks and routes preview on a map, and a Map pane view shows the files
that carry a position (photos first) where they were taken. Two plugins: `geo`
(formats, preview, thumbnails, the Map view) and `map-tiles` (backgrounds, the
only network code). Each step below ships on its own.

- **A. Track previews, offline.** Core `geo` types (`LatLon`, `Bounds` aware of
  the ±180° meridian, `GeoDoc`: tracks, routes, waypoints, polygons, bounded;
  a waypoint has a kind (fix, airport, takeoff, home, rally) and an altitude
  with its reference (above sea level, above ground, above takeoff), so the
  plan formats of step D fit without changing a core type) and
  `InspectionKind::Map` with `Inspection::geo`. `bibli-ui`: `elements/map.rs`
  (Web Mercator, pan and zoom, placement in prepaint, lines through
  `PathBuilder`) and a map preview kind in the shared frame with its slots (Fit,
  Zoom, Tracks, Open in Maps through a `geo:` URI). `geo` parses GPX (tracks,
  routes, waypoints), KML/KMZ (placemarks, `LineString`, `gx:Track`) and
  GeoJSON on a worker, thinned per zoom level, and fills Details: distance,
  moving and total time, climb and descent, highest and lowest points, time span
  in the user's calendar. The background is a themed graticule.
  *Built 2026-09-27.* The formats stream (quick-xml; a GeoJSON file is parsed
  whole, up to 16 MiB): a 6 MB, 100 000-point GPX is read in about 80 ms
  (release, reference machine). Thinning differs from the plan: no
  Douglas–Peucker levels; a file past `MAX_POINTS` (200 000) keeps every other
  point, evenly, while its facts count them all, and a line is thinned again to
  the pixels it covers when drawn (`elements::map::runs`), with the parts
  outside the view cut. A line without times in KML or GeoJSON is a route (only
  a recording has times; GeoJSON's `coordTimes` makes a track). The map also
  draws a scale bar and degree labels on the graticule, marks where the tracks
  start and end, and labels places while at most 40 are in view. Not built:
  the Tracks menu (every line is drawn); a notch of the wheel zooms half a
  level without Ctrl, as maps do. Not verified: the dark theme's map tokens
  were not looked at in a running window.
- **B. Backgrounds.** Core `tiles` contract; `map-tiles` with the outline
  (Natural Earth, the default), OpenStreetMap once chosen, and a custom URL; the
  tile cache, attribution, and the link to the setting on a map drawn on the
  outline.
  *Built 2026-09-27.* As planned, with these choices: a source answers
  through a callback (downloads never hold a scheduler worker); the UI's
  `map_tiles::TileStore` asks in the map's prepaint for the tiles it
  covers, cancels those that left the view, keeps 48 MB of decoded tiles
  while a map is shown and lets them go with their atlas entries when none
  is; a coarser tile already here stands in while a finer one comes. The
  graticule is drawn only over a background without streets. A failed
  background is said once per source revision; zoomed to street level on
  the outline, a hint (not an error: `Shell::set_hint_about`) links to the
  setting. Verified in the real app against a local tile server (never
  OpenStreetMap's): each tile asked once, from the cache after a restart,
  conditionally (`If-None-Match`, answered 304) once expired, the
  User-Agent naming bibli and its repository; a stale kept-alive connection
  is retried once. The outline was looked at in both themes. Not verified:
  OpenStreetMap's own server.
- **B′. Tile servers.** *Built 2026-09-27* (ADR-020 and ADR-010 addenda): a
  list of servers in Settings (a new records kind of setting, drawn by the
  dialog without knowing what the records are), each with an address in the
  common forms (`{s}`, `{-y}`, `{q}`, `{r}`, `{key}`), headers, and an API
  key kept in the desktop's keyring; a Test per server; the background
  chosen from the map's own toolbar. Verified in the real app inside a
  private keyring and against a local tile server that refuses requests
  without the key: adding, testing, choosing, editing, forgetting the key
  and removing a server, each checked in the settings file (never a secret)
  and in the keyring (the item written, then deleted). Not verified live:
  the `{s}`, `{-y}`, `{q}` and `{r}` forms (tested as pure functions), and
  a keyring that asks to be unlocked.
- **B″. Loading as map libraries do.** *Built 2026-09-27* (ADR-020 addendum):
  the middle of the view first, every downloaded tile kept, finer tiles
  standing in when zooming out, and HTTP/2 through hyper in place of `ureq`.
  At 150 ms a tile, opening a map now shows its middle in 175 ms (it came
  last) and the whole view in 431 ms (was 626), and zooming out two levels
  completes in about 1 s (was 1.9). Found after step C: a tile's outline
  showed as a darker line at some fractional zooms, gpui filtering a
  magnified image's edge with the empty atlas slot beside it; fixed in
  the vendored gpui (patch 0008, `vendor/gpui/README.bibli.md`).
- **C. The Map view.** A `ViewRole::Lens` in the View menu. `geo` scans the
  location shown (a folder, or with "Include subfolders" its subtree; a search or
  a timeline bucket as it is) for EXIF positions and track files, visible
  entries first, cached by identity. Markers carry thumbnails and merge into
  clusters when they are close; selection, Enter, the preview and the rubber
  band work as in other views. The status bar counts what has a position and
  what has not; an empty result is a placeholder offering subfolders. Commands:
  `geo.points`, `geo.status`, `geo.track`, `geo.open_external`,
  `geo.copy_coordinates`.
  *Built 2026-09-27* (ADR-020 addendum). Commands: `geo.scan`, `geo.points`
  (with the counts `geo.status` was to give), `geo.release`,
  `geo.search_subfolders`; Ctrl+5 shows the view. Deviations: the counts
  are in a row at the top of the view, with the Subfolders and Fit buttons,
  not in the status bar; "Include subfolders" is a search
  (`query.search`), not a walk of the plugin's own; the scan goes in
  listing order, not visible entries first (before the positions are
  known, nothing says what is in view); `geo.track`, `geo.open_external`
  and `geo.copy_coordinates` are not built (the preview of a selected track
  or picture has Open in Maps). A scan ends complete only when its listing
  was, and a finished one stays shown while the files are looked at again
  after a change, replaced only if a point moved: found in the real window,
  where a picture geotagged in place was not placed again, and the first
  scan, made before the folder arrived, ended empty and "complete".
  Fixed with it in core: `App::waker_handle` now calls the hook installed
  when it is called (a plugin's handle, taken at registration before the
  UI installs one, woke nothing), and so does a location's `OpenContext`.
  Verified in the real app on Xvfb with synthetic geotagged JPEGs and a
  GPX (invented coordinates): 21 placed and 3 without a position; the
  clusters; a cluster's click zooming in to thumbnails; a click, Shift and
  a drag selecting, the preview following; the Subfolders search (23
  placed); the arrows, the zoom keys and Home; a picture geotagged in
  place while shown (22 placed, without input). Headless: the command
  path in `crates/plugins/geo/tests/map_view.rs`. Not verified: the double
  click that opens (it starts the desktop's viewer); a folder of thousands
  of pictures (no geotagged bench data yet); the dark theme.
  Then, spreading (ADR-020 addendum): a bubble of files at one place opens
  in place as a spiral in the order they were taken, and Spread does it
  for every place. Verified on Xvfb with 24 synthetic pictures at one
  position whose hue follows their shuffled dates (the spiral runs from
  red at its centre to violet at its rim, the two undated last), 5 more
  1 km away (their spiral joins the first when zoomed out and parts when
  zoomed in), a click selecting, a click beside folding, a band holding
  what it covers, and 307 pictures at one spot drawn as a spiral of dots
  at 0.5% idle CPU. The planned "+N" tile past the budget became dots in
  their places, and Escape does not fold (it is the pane's `select_none`;
  a click beside does).
- **D. More formats and polish.** TCX, FIT, IGC and NMEA; track thumbnails in the
  grid; an elevation profile under the preview (a plan's planned altitudes);
  positions of videos (the MP4 location atom); a "Show on map" link on the
  Details position row.
  *Built 2026-09-27* (ADR-020 addendum). TCX, FIT (a bounded decoder of
  its own), IGC and NMEA read; a track's thumbnail is its lines drawn to
  scale (tiny-skia, a fallback after the desktop's thumbnailers); the
  profile sits under the map, its pointer marking the place on the map;
  videos give their position from `moov/udta/©xyz` or QuickTime's keys
  (`bibli_platform::mp4`), placed in the Map view and in Details beside
  the player; a Details row can carry a command (`Detail::action`, core),
  and a picture's or a video's position runs `ui.view` to the Map view,
  which opens on the one file selected. Found with it: the Map view, hidden
  and shown again, showed its old places without scanning again.
- **D′. Flight and drone plans.** Flight plans: Garmin `.fpl`, MSFS/FSX `.pln`,
  X-Plane `.fms`, Little Navmap `.lnmpln`. Drone missions: QGroundControl `.plan`
  (waypoints, survey polygons, geofence, rally points), MAVLink `.waypoints`
  (`QGC WPL 110`), Litchi CSV, DJI WPML `.kmz`. None is in the MIME database and
  several extensions are generic (`.plan`, `.fms`), so each is claimed by its
  extension and confirmed by its content. Details: departure and destination,
  legs, distance (nautical miles beside kilometres for a flight), cruising or
  highest altitude with its reference, waypoint count.
  *Built 2026-09-27* (ADR-020 addendum), MAVLink also as `.mission` and
  `.txt`. Every format of step D and D′ is confirmed by its first 4 KiB
  (the MIME database calls a `.fit` a FITS image, a `.pln` a PlanPerfect
  spreadsheet, a `.plan` a Calligra project), and the provider comes
  before the readers of tables and text, so a Litchi `.csv` is a mission
  and any other `.csv` a table. The Map view does not look inside `.txt`
  and `.csv` files: a folder's notes and spreadsheets are not places.
  Written by three agents in parallel, one crate file each, integrated with
  their shared code moved to `text.rs` (lines, with a log's and a plan's
  policy), `flight.rs` and `mission.rs`; the collector counts a line only
  once it holds points. Verified: 88 geo tests (71 of the parsers'), and
  in the release build on Xvfb one synthetic file of each format
  (invented places): each previews with its facts and profile, a
  `.txt` and a `.csv` that are not missions keep their text and table
  previews, the grid draws every line's thumbnail, the Map view places the
  fourteen that say where they are, "Show on map" opens the Map view on a
  picture, and the profile's pointer marks the track. Not verified: a real
  file of any of these formats (none may be copied here; each was written
  from its documentation), a video's position in Details (the synthetic
  video has no streams, so no poster carries its facts), the dark theme.
  An altitude of 0 in a flight plan is taken as not given: the formats
  write 0 for a fix without one, so a plan's field at sea level has none.
- **Later, each with its own decision** (ADR-022): search by place,
  geotagging and removing locations, and offline place names are
  *built 2026-09-27* (ADR-022's "Decided" paragraphs, ADR-023 for the
  fields read on demand). Verified in the real app on Xvfb: three pictures
  matched along a track through the user's time zone and applied with the
  modification time kept to the nanosecond, a location removed and undone,
  `near:` finding the one picture within 500 m, "This area" the four in
  view, and a picture near a town named in Details. Written by two agents
  in parallel for geotagging and place names, integrated with the fields
  contract. Local PMTiles/MBTiles files preview as their maps (built
  2026-10-05, ADR-022's part 4); backgrounds from them remain proposed.

Tests and fixtures use invented coordinates only. `bibli-bench gen` gains
geotagged pictures and a 100 000-point GPX; `budgets.toml` gains the GPX parse
and the EXIF position scan per 1 000 files.

**Done when:** a GPX, a KML and a GeoJSON file, a flight plan and a drone mission
preview with their facts; a folder
of geotagged pictures shows on the Map view with shared selection; both plugins
disable with no trace (`plugins_toggle.rs`), cancelling tile requests in flight;
no request leaves bibli until a network source is chosen; idle CPU returns to
0% after panning on `images10k`.

---

## Notes everywhere (planned, ADR-027, ADR-028)

Goal: a file's tags, rating and comment on any system (Linux, macOS,
Windows, USB drives) with nothing lost going back and forth; bibli reads
every store it knows by default and writes the ones the user chooses. Each
step ships on its own.

- **A. Stores, no change in behavior.** Core `notes`: the model (tags with
  a given color and a parent, rating, comment, origin), the `NotesStore`
  registry (owner-tagged), `set_notes` with the replaced bytes of every
  store as its inverse (`set_tags`, `set_rating`, `set_comment` still
  read). The tags plugin's XDG, Finder and AppleDouble code becomes three
  stores; the provider, index, vocabulary, sidebar and editor read the
  merged notes. AppleDouble also keeps the XDG attributes.
- **B. Merge against what bibli wrote.** The cache's snapshot per file;
  another application's edit in one store wins; union without one. Details
  lists where the notes are and offers to make disagreeing stores agree.
- **C. Companions.** The core registry of name rules; the listing pairs,
  hides (`core.show_companions`, off, also in the View menu under Hidden
  Files) and re-reads; providers get the
  folder's names (`AsideNames` and the `._` rule of `path_provider.rs`
  go); rename, move, copy, swap, trash, restore and delete carry them;
  a copy converts notes to what the destination keeps (ADR-006 amended).
- **D. Settings.** A Notes page (`notes.*`, declared by the core with
  the registry): each store read or not, written Off,
  Update existing or Create; defaults per platform; on a volume without
  attributes, the folder's convention, else AppleDouble.
- **E. XMP.** Sidecars (`x.ext.xmp`, `x.xmp` with
  `photoshop:SidecarForExtension`): keywords (`dc:subject`, the nested
  forms), `xmp:Rating`, `dc:description`, `xmp:Label` as a tag; read,
  updated where present, created on request. XMP inside files read on
  demand for Details.
- **F. TagSpaces.** `.ts/x.json` sidecars and `tsm.json` for folders
  (hex colors to the nearest palette color, the exact value kept); its
  `tsi.json` as a search hint (ADR-026).
- **G. Names.** TagSpaces (`x[a b].ext`, where a `.ts` folder shows it is
  used) and filetags (`x -- a b.ext`, on request); writing them is a
  rename plan of its own.

Tests use synthetic files built from each format's specification (not our
own encoders) and, where an application can run headless, its output.
Property tests of the operations cover trees with companions.

**Done when:** a file tagged in bibli keeps its notes copied to an exFAT
stick, read by a Mac's Finder there (checked on a Mac or with a `._` file a
Mac wrote), and copied back to a Linux disk; an edit made in another
application's store wins over bibli's older one; undo restores every
store byte for byte; `tags_fill_us` holds its budget.

---

## Phase 5 — Cross-platform (~6 weeks)

Goal: macOS and Windows builds that feel native in the ways that matter.

- `crates/platform` macOS: QuickLook thumbnail service, NSWorkspace icons and open-with, Trash,
  DiskArbitration volumes, Spotlight as a search provider, Finder tag xattr compatibility.
- `crates/platform` Windows: shell thumbnail cache, SHGetFileInfo icons, Recycle Bin, WM_DEVICECHANGE
  volumes, Windows Search as a provider, NTFS USN journal for fast subtree walks.
- File identity, path handling, and reflink/clone (APFS clonefile, ReFS block clone) abstracted in core.
- CI on all three OSes; budgets measured on each.
- Packaging: Nix flake and AppImage/Flatpak, dmg, msi.

Built on Linux first, run on each system later (ADR-035): 5a below is
done without a Mac or a Windows machine, 5b is the gate on them.
`docs/PORTING.md` lists what is Linux's (surveyed 2026-10-05); each
step removes its items and ends with `just check-cross` green for
what it touched.

- **A. It compiles everywhere.** Done 2026-10-05 (status above). The targets and Zig in the dev shell,
  `just check-cross` (Windows: the default members; macOS: all but
  `bibli-ui` and `bibli`, whose gpui backend needs Apple's SDK). Core's
  manifest lists its ordinary dependencies for every system (only
  `inotify` is Linux's); `xattr`, rustix's `fs` and `std::os::unix`
  move behind `cfg(unix)`; what has no implementation yet on a system
  errors with its name. Tests compile too. Then `check-cross` joins
  `lint-all`, so Linux work cannot add new Linux-only code unnoticed.
- **B. Identity, paths and listing.** Done 2026-10-05 (status above), but Usage. `EntryId` per system: macOS
  device and inode (stat, `st_birthtime`), Windows the volume serial
  and the file ID, read with the listing
  (`GetFileInformationByHandleEx`, `FileIdExtdDirectoryInfo`: no open
  per file), a 128-bit ReFS ID folded into the 64 bits (an ADR).
  Listing and stat behind one interface core owns (statx and getdents
  stay Linux's). Hidden by attribute (Windows `HIDDEN`, macOS
  `UF_HIDDEN`) beside the leading dot. Paths: no `\\?\` from
  canonicalization, `valid_file_name` per system, `PATHEXT` in
  `find_program`, Windows file names as WTF-8 bytes in `os_path`, the
  home folder from `os_path::home` everywhere, drive letters in the
  trail and in `file_uri`.
- **C. Operations.** Done 2026-10-05 (status above), but write-through off Linux (with F). No-replace rename and swap (macOS `renamex_np`
  `RENAME_EXCL`/`RENAME_SWAP`; Windows `MoveFileExW` without replace,
  no swap: refused with words), copy (macOS `clonefile`/`fcopyfile`,
  Windows `CopyFileExW`; ReFS block clone later), write-through
  (`F_FULLFSYNC`, `FlushFileBuffers`), attributes (macOS keeps every
  attribute a copy should, not only `user.*`; Windows has none),
  permissions (mode on Unix, read-only on Windows), special files.
  The property tests run on each system's code where Wine can.
- **D. Trash.** Done 2026-10-05 (status above), but listing a Mac's trash. A backend per system behind what core does now:
  freedesktop as it is; the Windows Recycle Bin by its `$I`/`$R` pairs
  (a documented record, parsed and written as `.trashinfo` is, tested
  on fixtures); macOS `~/.Trash` and each volume's `.Trashes/<uid>`
  through `trashItemAtURL` (the resulting path kept for undo), where
  the place a file came from is known only for what bibli trashed.
- **E. The environment.** In part 2026-10-05 (status above): types and the system's icons done 2026-10-06 (ADR-037, `system_icons`); Open With and owners not done. Folders (macOS `~/Library/…`, Windows
  `%APPDATA%`/`%LOCALAPPDATA%`) behind `bibli_core::xdg`'s callers; the
  MIME database off Linux (an ADR: shared-mime-info is GPL data and
  bibli is Apache-2.0, so it is installed beside bibli or replaced by
  the system's types, UTType and the registry); icons, opening and
  Open With (NSWorkspace and Launch Services; `ShellExecuteExW` and the
  association handlers); the terminal to open (Terminal or iTerm,
  Windows Terminal); the keyring (Keychain, Credential Manager); the
  first weekday (the system's locale); owners (Directory Services,
  SIDs).
- **F. Places and volumes.** In part 2026-10-05 (status above): devices and user folders, not eject, favorites or plug events. macOS: `getmntinfo`, DiskArbitration
  (mount, unmount, eject, removable), Finder's favorites read. Windows:
  drives and their kinds, removable media, eject, Quick Access read.
  Bookmarks stay `BookmarkEdit`s with inverses wherever they are
  written.
- **G. Thumbnails and isolation.** In part 2026-10-06 (status above): isolation chosen per system (ADR-039), macOS's built, Windows' AppContainer for the machine; the system's thumbnails written 2026-10-06 (ADR-041: Quick Look's compiled only, the Windows shell's run under Wine). QuickLook (`QLThumbnailGenerator`),
  the Windows thumbnail cache (`IThumbnailCache`); the converters that
  need isolation (LibreOffice, Pandoc, ImageMagick) stay off where no
  sandbox exists, until one is chosen per system (an ADR).
- **H. Processes and the terminal.** In part 2026-10-05 (status above): job objects, quoting, macOS's shell folder; not ConPTY run nor Windows' shell folder. Job objects for a helper's tree
  on Windows; ConPTY through `alacritty_terminal`; following the
  shell's folder (macOS `proc_pidinfo`, Windows through the shell's
  own report); PowerShell and `cmd` quoting beside POSIX and fish.
- **I. Media keys and search hints.** In part 2026-10-05 (status above): Spotlight's tag hints; Now Playing, the system media controls and Windows Search not written. Now Playing
  (`MPNowPlayingInfoCenter`) and the system media controls (SMTC) as
  plugins beside MPRIS, each registered only on its system; Spotlight
  and Windows Search as hint providers beside Baloo and plocate.
- **J. Keys and the window.** Done 2026-10-05 (status above), but the outgoing drag. Cmd for the standard shortcuts on macOS
  (`secondary`, which core already parses); the font family per
  system; an outgoing drag on macOS and Windows (patch 0001 is
  Wayland's).
- **K. Packages and CI.** Linux's built and tried 2026-10-06 (ADR-040, ADR-041: Debian's, RPMs and the AUR, as the user chose, no Flatpak nor AppImage); the `.app`, the `.dmg` and the `.msi` written, never run; CI written, waiting for a remote. On Linux, built and tried here: the Nix
  package and an AppImage or Flatpak. Written, not run: the `.app` and
  `.dmg`, the `.msi`, the native workers' libraries per system, a CI
  matrix (`macos-latest`, `windows-latest`) waiting for a remote.
- **L. Budgets per system.** Decided 2026-10-05 (ADR-038); measured in 5b. What "own share" and "first frame" mean
  on macOS (`phys_footprint`) and Windows (private working set), and
  how each is measured (an ADR); measured in 5b.

**Done when (5a, on Linux):** passed 2026-10-06 (status above). `just check-cross` is part of
`lint-all` and green; every step above has its code or is recorded as
not done, with what ran under Wine and what no machine ran; the Linux
packages build and start; Linux's tests and budgets are unchanged.

**Done when (5b, the original gate):** the Phase 2 dogfood week is repeated on each OS, and the budgets pass on all three.

---

## Phase 6 — The extension boundary (~4 weeks)

Goal: open the plugin system beyond the repo, safely. Only now, because the traits are stable.

Begun 2026-10-07, before Phase 5b's gate, as the user chose: 5b needs a
Mac and a Windows machine, which are not here, and this phase needs
neither. Its first step is the command line and the MCP server
(ADR-043): agents and scripts drive the running window through the
`control` plugin, off by default until the permission layer.

That step is built (2026-10-07). The window listens, while Control is
on, on a Unix socket in a folder of the user's alone (a named pipe on
Windows); `bibli mcp` carries an agent's MCP there through `rmcp`, and
`bibli cmd` runs one command and prints JSON, in a bibli of its own
without a window when none listens. An agent's plan is proposed and
its tool answers once the user has applied or discarded it
(`App::propose_awaiting`, from the executor's `Finished`). Added on
the way: `entries`, a pane's entries as an agent reads them;
`apply_plan` now gives its plan back to its caller instead of applying
it, so it cannot skip a review. The gate's second half was played on
Xvfb with the release binary and a scripted MCP client: it read the
window (`tab.list`, `entries`), proposed a rename and then a move of
two files, each applied with Enter in the review, and a trash
discarded with Escape, each answered as the user did; `bibli cmd`
printed a plan, applied one with `--apply`, opened Settings in the
window and turned Control off through itself. Then every command that
acted on the selection alone takes `entries` (`App::targets`): files
named by absolute path, so a script runs them with no window and an
agent's call cannot act on what the user selected meanwhile; a
relative path, an entry's or a destination's, is refused. Not done: an agent
driven by a model (Claude Code over `bibli mcp`) has not run it; the
Windows pipe is tried under Wine only and keeps the system's default
security (`docs/PORTING.md`); a command answering a value does
nothing from the palette (`entries`, `tab.list`, `control.status`
alike), and which commands an agent may run waits for the permission
layer.

The rest of the phase is built (2026-10-07), in three steps.

The permission layer (ADR-044). Every plugin says what it may do
(`Plugin::capabilities`: read what files hold, write, network,
programs); the Plugins page says it on its line, and where a plugin
from outside comes from. Private folders (the Privacy page: `~/.ssh`,
`~/.gnupg` and their like from the start, and any folder holding a
`.private` file) never reach an agent nor a plugin from outside. Every
command says what it does (`command::Effect`: reads, shows, plans,
acts, user only); an agent's call that acts waits for the user to allow
it, in the review's queue, one that answers the user's own dialogs is
refused (an agent could otherwise apply its own proposal), one that
names a private folder is refused, and what it is answered loses every
private path. Checked on Xvfb with the release binary and a second
scripted agent: the tool list leaves out the user's own commands and
says which ask; `ui.copy_text` asked, was allowed with Enter and ran,
then asked again and was refused with Escape; a `.ssh` and a folder
marked `.private` were refused by name; the home folder an agent listed
had `files` alone where the user saw `.ssh`, `diary` and `files`.

Plugins from outside (ADR-045, `bibli-plugin-api`, `bibli-extend`): a
folder with a `plugin.toml`, found in the data folders' `bibli/plugins`
or `BIBLI_PLUGIN_PATH`, listed on the Plugins page, off until turned
on. It provides fields, thumbnails, previews and commands through one
protocol of JSON methods; files go to it by handle, read through bibli
for the call's own files only and with `read` only; a plan it answers
needs `write` and is reviewed when run from the window. Level 2 runs it
as a program speaking JSON-RPC lines, isolated by bubblewrap (no
network unless declared; on Windows it does not run), started when
first asked, ended idle. Checked by `crates/extend/tests/programs.rs`:
the template as a program answers a field and a command through bibli,
is refused its reads without `read`, never gets a private file; and
the first thumbnailer from outside (`crates/plugins-outside/netpbm`,
PPM and PGM pictures, its PNG written by the module itself) draws
through the thumbnail service as the built-in ones do.

Level 1 runs it as a WebAssembly module (ADR-046): Wasmtime 38, no
WASI, bibli's two imports alone, bounded work and memory, in a helper
of bibli's (`bibli-module-worker`) that bibli starts and speaks to as
it does a program plugin, so Wasmtime never enters bibli's process:
linked in, it cost 1.6 MiB of idle memory (`just idle-pss` back to
back, own share 94.8 against 93.2 MiB, within the 100 MiB budget). The gate's
first half: the hexadecimal preview again as a module
(`crates/plugins-outside/preview-hex`), shown page for page, match for
match and error for error the same as the built-in one by
`crates/extend/tests/modules.rs` (nine views of four synthetic files),
and in the window: the built-in one off and the module on, then the
other way round, the preview panel of the same file differs by no
pixel. The template (`crates/plugins-outside/template`, a word count as
a field and a command) builds as either; `docs/plugins/README.md` and
`protocol.md` say how to write one; `just plugins` builds the modules
into `target/plugins`. Not done: the packages carry no module (the
built-in preview stays theirs); a typed WIT world (payloads are JSON);
previews from outside draw text, code, hex and tables, not the richer
kinds; a module cannot use the network; macOS isolation of a program
is compiled only. Control stays off by default: turning it on is the
user's choice, now with the permission layer between an agent and
their files.

- **Permission layer.** Capabilities per plugin (read contents, write, network, spawn processes) and
  per-directory policies ("never leaves this machine" for `~/.ssh`, anything under a `.private` marker).
  Required before any model provider exists.
- **Level 2, external processes.** Small line-delimited protocol (LSP-style) for provider processes.
  Used first for thumbnailers and external tools we already call.
- **Level 1, WASM plugins.** wasmtime + component model for providers, commands, metadata. Not views.
- **CLI and MCP server.** Both are thin consumers of the command registry: `bibli cmd rename ...`,
  and an MCP server exposing commands and plans so an external agent (e.g. Claude Code) can drive the app.
- Plugin authoring docs and a template crate.

**Done when:** one existing internal plugin is re-implemented as WASM with no user-visible difference,
and an external agent completes a rename-and-move task over MCP that ends in a reviewable plan.

---

## Phase 7 — AI (~10 weeks)

Goal: agents that organize, rename, clean, tag, and summarize, always ending in a plan the user reviews.

1. `extract` — content extraction provider: text from PDFs/docs, OCR, EXIF, audio/video metadata.
   Cached by content hash. Uses installed tools where present. Benefits plain search immediately.
2. `cleaners` — deterministic: exact duplicates (hash), near-duplicate images (perceptual hash),
   empty dirs, stale downloads. No model. Each produces a plan.
3. `fulltext` — keyword search index on Tantivy (ADR-003), embedded, no service. One index per
   subtree, built on demand and only when asked, refreshed incrementally by mtime and content hash,
   stored in the throwaway cache, pausable and deletable. It feeds the query language's `content:`
   term when an index covers the query root; otherwise the existing scan runs, so the index is only
   an accelerator. Text comes from the `extract` provider. Existing system indexes (Tracker, Baloo,
   Recoll, Spotlight, Windows Search) and a running Meilisearch are optional providers for the same
   term, used when present, never required.
4. `models` — `ModelProvider` trait: chat with tool calling, embeddings, image understanding,
   transcription. Implementations: local runtime (lazy-loaded), auto-detected Ollama, remote APIs
   behind explicit opt-in with a persistent "leaves your machine" indicator. Idle cost when disabled: zero.
5. `models-small` — embeddings, CLIP-style image tagging/search, OCR, document classifier.
   Run on CPU or iGPU. These do the volume; the LLM orchestrates.
6. `semantic` — scoped, on-demand, incremental subtree index in the throwaway cache; visible and
   pausable. Natural language becomes a `LocationProvider` in the same address bar.
7. `agent` — the LLM plugin. Tools = the command registry. Output = a `Plan`. Never executes.
   Prompts treat file names and contents as untrusted input. Flows: organize this folder,
   rename these consistently, what is safe to delete here, tag the selection, summarize in preview.
8. Metadata from models carries `Model(name, confidence)` provenance and renders as suggestions
   until confirmed, at which point it is promoted to User provenance and written to xattrs.
9. MCP client, so the agent can use external tools the user configures.

**Done when:** "organize this folder" on a messy Downloads produces a correct, undoable plan using
only local models, and disabling every AI plugin returns idle RSS to the Phase 2 baseline.

---

## Phase 8 — Polish and release (ongoing)

- Accessibility audit (screen reader on each OS), IME, high-DPI, themes.
- Docs, plugin gallery, first public release with an opinionated default plugin set enabled.

---

## Cross-cutting rules

- Every phase ends with a benchmark run and an ADR for any decision that closes a door.
- Features enter as plugins by default. Promotion into core requires the "two plugins must agree" argument.
- No plugin hook may run per frame or per row. Batches and streams only.
- Any AI-derived data is a suggestion until a human confirms it.

## Immediate next steps (first week)

1. Init the workspace, flake, CI, and `bench` with the synthetic-tree generator.
2. Write ADR-000 (principles and budgets).
3. Build the GPUI spike, then the Floem spike, run both on both GPUs, record numbers.
4. Write ADR-001 (toolkit decision).
