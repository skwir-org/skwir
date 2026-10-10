# What is Linux's: the Phase 5 checklist

A survey of the tree on 2026-10-05 (ADR-035, `docs/PLAN.md` Phase 5),
grouped by subsystem. Each step of Phase 5 takes its items from here and
removes them when they are done, so what remains is what is left to
port. Line numbers are left out where step A moved the code; search for
the function named.

Since step A everything compiles for Windows and macOS (`just
check-cross`, part of `just lint-all`). What has no code for a system
yet either errors with the system's name (`skwir_core::system::not_yet`)
or answers as "unknown" where a predicate has no error to give; each
item below says which. Marks: **(n)** errors with "not available on
<system> yet" there; **(u)** answers unknown or nothing there; **(c)**
builds everywhere, means something only on Linux.

## Where each system's code goes

- `core/src/fs/dir.rs`: a folder held open, listed and stat-ed by name
  (`Dir`). Unix: a descriptor (getdents on Linux, readdir on macOS).
  Windows: a handle shared for every use, listed through the IDs its
  records carry (`fs/win.rs`, ADR-036).
- `core/src/fs/stat.rs`: `stat_at` per system (statx on Linux, fstatat
  on macOS, a handle's records on Windows), `id_of`, `identity` and
  `changed` for a path whose metadata is in hand.
- `core/src/fs/win.rs`: Windows' file calls, the one module of core
  allowed `unsafe` for them.
- `core/src/fs/xattr.rs`: extended attributes. Windows answers as a
  volume that keeps none, which is true of it; macOS uses the Linux
  names (`user.*`), which is not (section 5).
- `core/src/op/sys.rs`: what the operations ask of the system.
- `core/src/os_path.rs`: names from bytes (`from_bytes`,
  `from_bytes_ref`): exact on Unix and on Windows (WTF-8).
- `core/src/system.rs`: the system's name and `not_yet`.
- `just test-windows` (`tools/cross/test-windows.sh`): the Windows build's
  tests under Wine; a test needing what Wine lacks says so and passes
  (`under_wine`, core's tests).

## 1. Identity and metadata

Done in step B: identity, listing and stat on Windows (ADR-036),
macOS's birth time, hidden by the system's mark (`Entry::hidden`:
Windows' attribute from the listing and a stat, a Mac's `UF_HIDDEN`
from a stat; `fs::is_hidden` for `read_dir` walks, Windows' attribute
only), `identity` and `changed` read by path on Windows. Core's tests
compiled for Windows pass under Wine (166, 2026-10-05). Left:

- Wine has no 128-bit directory records: the fallback to the 64-bit
  ones ran, the 128-bit path and the folding did not (ADR-036, 5b).
- Off Linux a folder moved while shown is not followed (inotify's
  `IN_MOVE_SELF` says it; Windows' and macOS's watchers do not here):
  the listing keeps its old path until relisted.
- On Windows a stat opens the file: a hydration of 100 000 rows costs
  as many opens. The listing's records carry the size and the times
  already, unused so far.
- A search batches a folder's files per walking thread (`query`
  `Deferred`): under Wine, where stats are slow, 100 files of one folder
  reached the metadata providers in 12 calls, not at most 10
  (`tags_are_matched_in_batches_per_folder` skips there). To see on
  Windows; sharing the batches between threads would fix it.
- `op/entry_ref.rs` `find_in_parent`: on Windows every entry of the
  folder is a candidate (no ID in `std`'s listing).
- Usage (`usage/worker.rs` `meta`): Unix only; **(n)** "Disk usage"
  elsewhere (`Scanner::list_root`). Windows needs allocation sizes and
  link counts from a handle.
- `platform/src/users.rs` (c): `/etc/passwd` and `/etc/group`;
  `users::mode`; shown in `ui/details.rs`, loaded in `ui/shell.rs`.
  Windows entries carry no mode or owner (`keys::MODE`, `UID`, `GID`
  unset): Details leaves those rows out there (step E: owners by SID).
- `platform/src/access.rs` (c): POSIX ACLs, capabilities, SELinux.
- Hidden by name alone (c): `query/plocate.rs`, `tags/index.rs`,
  `tags/count.rs`; on macOS the `read_dir` walks (`fs::is_hidden`).

## 2. Paths

Done in step B: Windows' name rules (`op::valid_file_name`),
`PATHEXT` (`os_path::find_program`), canonical paths without `\\?\`
(`os_path::canonical`, every caller but Linux's device code), the home folder read one way
(`os_path::home`, `~\` on Windows), `file:///C:/` and
`file://server/share` URIs (`platform/uri.rs`, which the SQLite
readers use too), a drive and its root as
one trail segment (`ui/address.rs`). Left:

- `core/src/os_path.rs`: Windows names are UTF-8 only, what is not is
  replaced (WTF-8 exactly); `is_executable` is true for any file off
  Unix (c).
- `"/"` as a fallback: `ui/terminal.rs`, `places/lib.rs`,
  `platform/apps.rs`, `op/builtin.rs` (several), `op/order.rs`.

## 3. Operations (`core/src/op/sys.rs`, `builtin.rs`)

Done in step C: a rename without replacing on Windows (`MoveFileExW`,
run under Wine), a swap refused there as a conflict (Windows cannot
exchange two names in one step; bulk renames park a name instead); a
copy made by the system where it keeps more than the bytes
(`copy_whole`): Windows' `CopyFileExW`, with progress and Cancel and
its alternate streams (a download's mark), run under Wine but for the
streams, which Wine lacks; macOS's `clonefile` when attributes are kept
(compiled only); macOS keeps every attribute a copy should, Finder's
among them, but two the system sets (compiled only); pipes copied on
macOS by `mkfifo` (compiled only). Left:

- macOS's rename without replacing and swap (`renameatx_np`) are
  compiled, never run.
- Write-through (`Pace`, `sys::write_range`, `Dir::sync_volume`):
  Linux hands the drive each window of bytes and waits for it a window
  later (`sync_file_range`), and writes the volume out once per plan,
  and on the way where moves wait to remove their originals (`syncfs`);
  **(n)** elsewhere, never reached since nothing is removable there
  (section 6). A call that commits the volume or
  flushes the drive's own cache costs about the same however little it
  writes: on Linux, `syncfs` every few MiB held a copy onto a USB hard
  drive to 7 MiB/s where the drive takes 80. So neither system's flush
  (`F_FULLFSYNC`, `FlushFileBuffers`) may be paid per window or per
  file. Windows: `CopyFileExW` with `COPY_FILE_NO_BUFFERING` writes
  past the system's cache, its progress counting what the drive took,
  and `FlushFileBuffers` once per plan on a file it copied (whether that
  writes the other files' names out on FAT and exFAT is to check).
  macOS: `fsync` per window, which writes to the drive without
  flushing its cache, on a thread of its own so the copy goes on, and
  `F_FULLFSYNC` once per plan. What each costs on a USB hard drive is
  to measure there before choosing.
- Links copied (`symlink_like`): Windows makes a folder link as one;
  it needs a privilege, untested.
- On Windows the staging folder keeps the access it inherits; read-only
  is the only permission opened and restored (`opened_to_owner`).
- The property tests (`apply_then_undo_restores_the_tree`) skip under
  Wine, whose shell does not trash what an undone copy trashes.
- `platform/file.rs` `rewrite` (c).

## 4. Trash

Done in step D: one module knows where a trash keeps an item and what
it says of it (`core/src/op/records.rs`): freedesktop's `files/x` and
`info/x.trashinfo`, and on Windows the Recycle Bin's `$R` item and `$I`
record (both versions of the record, parsed on Linux from fixtures).
`in_trash`, `trash_info_of`, restoring a part, taking a file out and
putting it back read either; a record is kept as bytes in a plan (a
freedesktop one serializes as the text it always did). The platform
lists each local drive's bin for the user's SID (`platform/windows.rs`,
`trash::trashes`), so `trash:` shows it. Trashing and restoring a whole
item go through the `trash` crate, the shell on Windows. Left:

- None of the Windows side ran: Wine's Recycle Bin is the freedesktop
  trash and its shell does not trash (the tests that trash skip under
  Wine). The record format and the listing ran on Linux, on fixtures.
- macOS: an item is trashed with `trashItemAtURL` (`op/sys.rs`
  `trash_item`), whose resulting path the undo keeps (`Restore::item`,
  its companions in `Restore::landed`), so undoing a trash moves them
  back (compiled only). `~/.Trash` keeps no record of where a file came
  from, so the trash crate's listing and restoring stay **(n)**
  (`trash_list`, `trash_restore`) and `trash::trashes` lists none
  **(u)**: `trash:` is empty on a Mac, and only an undo restores.

## 5. Attributes and notes stores

- `core/metadata.rs` `user.xdg.tags`, `tags/xdg.rs` (Dolphin's
  names): on macOS an attribute has no `user.` namespace (c).
- `tags/finder.rs` (c): Finder tags as Linux sees them; the native
  name already under `cfg(target_os = "macos")`.
- `tags/appledouble.rs`, `core/companions.rs`: `._` files;
  `platform/filesystem.rs` `statfs` magic (Linux; **(u)** elsewhere:
  never AppleDouble, change times never trusted).
- On Windows every attribute is refused, so by the notes policy an
  edit goes to the store the folder already uses, else AppleDouble
  (not exercised).
- xmp, tagspaces, name-tags: portable.

## 6. Volumes, places, bookmarks (step F)

Done: on Windows the devices are the local drives (fixed and
removable, never a network drive or a disc, which can hang whoever
asks), each its label and letter, its filesystem, removable when it
is, its free space (`platform/windows.rs`, run under Wine); on macOS
the root and the volumes under `/Volumes` the Finder shows
(`getmntinfo`, compiled only); the user's folders are the system's off
Linux (`dirs`: Windows' known folders, a Mac's), run under Wine. Left:

- Eject and mount off Linux **(n)**: no command (DiskArbitration,
  `IOCTL_STORAGE_EJECT_MEDIA`); `Mount::udisks` is false there.
- Write-through off Linux: `fs::volume::is_removable` is false there
  **(u)**. Windows says a USB hard drive is fixed (`GetDriveType`), so
  removable is to be asked of its bus, as on Linux; its default policy
  for external drives (quick removal) already keeps their writes out
  of its cache. A Mac's is DiskArbitration's to say. How either is
  written through is section 3's note.
- Bookmarks are GTK's (c): Finder's favorites and Windows' Quick Access
  are not read.
- `plugins/places` watches `/dev/disk/by-uuid` and `/run/media/$USER`
  (c), absent elsewhere; drives plugged in on Windows appear at the next
  rescan, not at once (`WM_DEVICECHANGE` not heard).
- `core/fs/volume.rs` (sysfs, mountinfo), `platform/places.rs` (udev,
  `lsblk`, `udisksctl`, `flush`'s `syncfs`): Linux's.
- `vim/lib.rs`, `jump/xbel.rs` (c).

## 7. Types, icons, applications (step E)

Done: skwir's own folders per system (`core/src/folders.rs`: settings
and cache in `~/Library/…` on macOS, `%APPDATA%`/`%LOCALAPPDATA%` on
Windows, the XDG ones on Linux as before); the keyring (Windows'
Credential Manager, run under Wine's; macOS's Keychain, compiled only);
the terminal to open (Windows Terminal or a console of its own, and a
program run in a console that ends with it; `open -a` on macOS, which
runs no program in it yet **(n)**); the first weekday from the
system's locale (Windows' `GetLocaleInfoEx`, run under Wine; macOS's
`NSCalendar`, compiled only); types (ADR-037): the system's registry
first (`platform/mime/uttype.rs`, UTType, compiled only;
`platform/mime/registry.rs` over `windows::classes`, run under Wine),
shared-mime-info behind it, its reader vendored to build on Windows,
its compiled files carried by the macOS and Windows packages, compiled
from the source vendored in `vendor/shared-mime-info`
(`tools/package/mime-database.sh`); icons (2026-10-06): the system's
own off Linux (`platform/system_icons.rs`): a file's by its extension,
its own where the system draws such files each theirs (Windows' `.exe`,
`.lnk`, `.ico`; a Mac's `.app`), a folder's, a user folder's and a
volume's by path, the trash and drives as stock icons; Windows' shell's
from the system image list (run under Wine, whose stock icons are a
stub: the trash has none there), `NSWorkspace`'s and `NSImage`'s on
macOS (compiled only). Left:

- macOS's types never run: the `UTType` calls, and the table of
  conformances read as kinds (`uttype::KINDS`), are unverified **(5b)**.
- The system's icons never ran on the systems: whether AppKit's icons
  and images answer off the main thread, as documented; the Windows
  jumbo list's small icons; a folder with an icon of its own (not asked:
  every folder is drawn as a folder) **(5b)**.
- `platform/apps.rs` (c): desktop entries, `mimeapps.list`, the
  locale, `Exec` codes; Open With lists nothing off Linux (Launch
  Services, Windows' association handlers not read).
- Owners: Windows entries carry none (SIDs not read); macOS's are its
  uids, named through `/etc/passwd` (c), which misses Directory
  Services' users.

## 8. Thumbnails and isolation

- `platform/thumbnails.rs`: the freedesktop cache (c; private
  elsewhere). `thumb_providers.rs` `.thumbnailer` files (c).
- `platform/sandbox.rs` (ADR-039): bubblewrap on Linux, `sandbox-exec`
  with skwir's profile on macOS (compiled, never run: whether
  LibreOffice, Pandoc, ImageMagick and the decoders start under it,
  and what the profile must let through, is for a Mac **(5b)**);
  nothing on Windows, where an AppContainer is to be built and tried
  on the machine **(5b)**. Without isolation LibreOffice
  (`platform/preview.rs`) and ImageMagick (`platform/magick.rs`;
  `convert` is a system tool on Windows) are refused; Pandoc
  (`preview-documents/pandoc.rs`) runs in its own `--sandbox` (2.15 or
  later) on every system, and alone where the system isolates nothing.
- The system's thumbnails (ADR-041, `platform/system_thumbs.rs`): Quick
  Look's on macOS (compiled, never run **(5b)**), the shell's
  `IShellItemImageFactory` on Windows (run under Wine, whose shell
  draws none **(5b)**), at the Generate stage after skwir's decoder.
- PDFs (ADR-041): the packages carry PDFium, which its helper finds
  beside itself or in a Mac bundle's `Frameworks` (the Windows helper
  loaded the pinned `pdfium.dll` so under Wine); PDF thumbnails and
  converted office pages are drawn by the engine (`document::
  first_page`), `pdftoppm` only where none is.
- Left for the machines (ADR-041): audio and video through AVFoundation
  and Media Foundation (a helper per system, not GStreamer bundled);
  the archive helper with libarchive linked in statically; LibreOffice
  on macOS under `sandbox-exec` (found where it installs itself,
  `process::required`).

## 9. Processes, terminal, editors (step H)

Done: on Windows a helper and everything it starts are held in a job
object, ended together on a cancel or a timeout (`platform/windows.rs`
`Job`, run under Wine); whether a process runs is asked of each system
(`process::runs`), so a running instance's cache folders are never
swept off Linux (they were by `/proc`); where the shell works is read
on macOS too (`process::working_dir`, `proc_pidinfo`, compiled only);
the `cd` line is written for PowerShell and `cmd` too (tested on
Linux). Left:

- On Windows the terminal (ConPTY through alacritty) is compiled,
  never run; who has the terminal and where the shell is are **(u)**
  there, so the pane and the shell do not follow each other (a shell's
  own report, OSC 9;9 or 7, would say where it is).
- Programs off `PATH` (ADR-041): on macOS Homebrew's and MacPorts'
  folders are searched after `PATH` (`os_path::find_program`: an
  application started from the Finder has the system's folders alone),
  and LibreOffice is found where it installs itself on macOS and
  Windows (`process::required`). Compiled only.
- `platform/process.rs`: a child is put in its job just after it
  starts (std cannot start it suspended): what it starts in that
  instant escapes the job.
- `line_channel.rs` exit 127 (c); `platform/media.rs` (Unix).
- `plugins/shell`, `plugins/edit-as-buffer` (c): `/bin/sh` wrappers,
  `$SHELL`, `$EDITOR`, `$XDG_RUNTIME_DIR`.

## 10–15. The rest

- Media keys (step I): `plugins/mpris` is registered on the free
  desktops only now; a Mac's Now Playing and Windows' media controls
  (SMTC) are not written (`skwir_core::playing` is the seam).
- Hints (step I): Spotlight answers a tag search on a Mac (`mdfind`,
  Finder's tags; compiled only); Windows Search is not asked;
  `query/plocate.rs` (c), `tags/hints.rs` Baloo (c).
- The free desktops' shared files read through `core/xdg.rs` (c):
  thumbnails, trash, apps, mime, thumbnailers, icons, places; jump,
  tags, vim, geo, edit-as-buffer. Absent elsewhere, so read as nothing.
- Fonts: `mupdf-worker/fonts.rs` `fc-match` (c).
- `alloc/lib.rs` ELF constructor (Linux); `skwir/gpu.rs` sysfs (c).
- `archive-worker/main.rs`: `LC_CTYPE` value and `C.UTF-8` (c).
- Keys (step J, done): the standard shortcuts are `secondary` (Cmd on
  a Mac, Ctrl elsewhere, the same keys on Linux as before), bindings
  are compared as GPUI reads them (`keymap::normal_keys`), caps say ⌘
  and ⌥ on a Mac, and the pointer's Ctrl gestures (Ctrl+click,
  Ctrl+wheel, a band kept) are Cmd there. Kept Ctrl on a Mac: what it
  reserves or means otherwise (Cmd+H hides an app, Cmd+Space is
  Spotlight, Ctrl+arrows switch Spaces), the terminal's keys, Ctrl+Tab,
  word moves (a Mac's are Option+arrows: not done). Compiled for
  Windows, never for a Mac (skwir-ui).
- Fonts (step J, done): each system's own after Inter (Helvetica Neue
  and Menlo on a Mac, Segoe UI and Cascadia Mono or Consolas on
  Windows), never run.
- `ui/dnd.rs` outgoing drag is patch 0001, Wayland's; a drag's copy
  modifier is Ctrl, a Mac's is Option (step J, not done).
- Agents and scripts (ADR-043): on Windows the window listens on a
  named pipe in the user's name (`plugins/control/src/channel.rs`),
  tried under Wine (`tests/pipe.rs`), which keeps no accounts apart.
  Left for Phase 5b's gate: the pipe has Windows' default
  security, which lets every account open it to read, and a pipe
  another account made first under that name is taken for skwir's
  (the window does not listen, and `skwir mcp` would talk to it). The
  pipe needs a DACL of the user alone, and the client a check of the
  server's account (`GetNamedPipeServerProcessId`). On macOS the
  socket is in the user's own `$TMPDIR`, compiled only.
- Plugins from outside skwir (ADR-045, ADR-046): a module runs the same
  everywhere (Wasmtime, compiled for Windows and macOS, run on Linux
  only); a program is isolated by bubblewrap on Linux, by
  `sandbox-exec` on macOS (`sandbox::plugin`, compiled only), and does
  not run on Windows, which has no isolation yet (ADR-039): it says
  so (`system::not_yet`).
- `bench`: the raw listing is the system's own (readdir and fstatat
  on macOS, `std::fs` on Windows), compiled, never run; its budgets are
  Linux's (step L).

## Tests that run on Linux alone

Gated `#[cfg(unix)]` (links, modes, scripts standing in for programs,
names that are no UTF-8, attributes, Usage's walk) or
`#[cfg(target_os = "linux")]` (pipes, the freedesktop trash, Baloo, the
type database, icon themes) in step A.
Each port step that gives a system the code turns the matching tests on
for it.

## Hardest first

1. Identity, stat and listing (step B).
2. The trash (step D).
3. The type database off Linux (step E, ADR-037: done).
4. The converters that need isolation (step G, ADR-039: macOS and
   Pandoc done, Windows on the machine).
5. Volumes and places (step F).
6. The copy engine's semantics (step C).
7. The terminal (step H).
8. Process trees on Windows and `PATHEXT` (steps B, H).
9. Byte paths in what is stored and serialized (step B).
