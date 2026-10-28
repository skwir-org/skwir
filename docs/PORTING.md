# What is Linux's: the Phase 5 checklist

What is still Linux-only, by subsystem: a survey of the tree on 2026-10-05
(ADR-035, `docs/PLAN.md` Phase 5). Each port step takes its items from here
and removes them when done, so what remains is what is left. Line numbers
are omitted: search for the function named.

Everything compiles for Windows and macOS (`just check-cross`, part of
`just lint-all`), except the Poppler, MuPDF and DjVuLibre helpers, which
no package off Linux carries, and `skwir-ui` and `skwir` on macOS: gpui's
Metal backend generates its bindings from Apple's SDK. The media helper
is linted against GStreamer's own build for each system
(`tools/cross/clippy-media.sh`). The Windows build's tests run under Wine
(`just test-windows`).

| Mark | Off Linux, the code... |
| --- | --- |
| **(n)** | errors "not available on \<system\> yet" (`skwir_core::system::not_yet`) |
| **(u)** | answers unknown or nothing (a predicate with no error to give) |
| **(c)** | builds, but means something only on Linux |
| **(5b)** | needs the real machine (Phase 5b) |

"Under Wine" means it ran in the Windows build under Wine; "compiled only"
means it never ran.

## Where each system's code goes

- `core/src/fs/dir.rs`: a folder held open, listed and stat-ed by name
  (`Dir`). Unix: a descriptor (getdents on Linux, readdir on macOS).
  Windows: one shared handle, listed through the IDs its records carry
  (`fs/win.rs`, ADR-036).
- `core/src/fs/stat.rs`: `stat_at` per system (statx on Linux, fstatat on
  macOS, a handle's records on Windows); `id_of`, `identity` and `changed`
  for a path whose metadata is in hand.
- `core/src/fs/win.rs`: Windows' file calls; the one core module allowed
  `unsafe` for them.
- `core/src/fs/xattr.rs`: extended attributes. Windows answers as a volume
  that keeps none (true). macOS uses the Linux names (`user.*`), which is
  wrong (section 5).
- `core/src/op/sys.rs`: what operations ask of the system.
- `core/src/os_path.rs`: names from bytes (`from_bytes`, `from_bytes_ref`),
  exact on Unix and on Windows (WTF-8).
- `core/src/system.rs`: the system's name and `not_yet`.
- `just test-windows` (`tools/cross/test-windows.sh`): the Windows build's
  tests under Wine. A test needing what Wine lacks says so and passes
  (`under_wine`, core's tests).

## 1. Identity and metadata

Done (step B):

- Identity, listing and stat on Windows (ADR-036); `identity` and `changed`
  read by path there.
- macOS birth time.
- Hidden by the system's mark (`Entry::hidden`): Windows' attribute from the
  listing and a stat, a Mac's `UF_HIDDEN` from a stat. `fs::is_hidden` for
  `read_dir` walks reads Windows' attribute only.
- Core's tests compiled for Windows pass under Wine (166, 2026-10-05).

Left:

- Wine has no 128-bit directory records: the 64-bit fallback ran, the
  128-bit path and its folding did not (ADR-036, 5b).
- A folder moved while shown is not followed off Linux (only inotify's
  `IN_MOVE_SELF` reports it): the listing keeps its old path until relisted.
- Windows: a stat opens the file, so hydrating 100,000 rows costs 100,000
  opens. The listing's records already carry size and times, unused.
- A search batches a folder's files per walking thread (`query`
  `Deferred`). Under Wine, where stats are slow, 100 files of one folder
  reached the metadata providers in 12 calls instead of at most 10
  (`tags_are_matched_in_batches_per_folder` skips there). Check on Windows;
  sharing batches between threads would fix it.
- `op/entry_ref.rs` `find_in_parent`: on Windows every entry of the folder
  is a candidate (`std`'s listing has no ID).
- Usage (`usage/worker.rs` `meta`): Unix only; **(n)** "Disk usage"
  elsewhere (`Scanner::list_root`). Windows needs allocation sizes and link
  counts from a handle.
- `platform/src/users.rs` **(c)**: `/etc/passwd`, `/etc/group`,
  `users::mode`; shown in `ui/properties.rs`, loaded in `ui/shell.rs`. Windows
  entries carry no mode or owner (`keys::MODE`, `UID`, `GID` unset), and
  Properties omits those rows (step E: owners by SID).
- `platform/src/access.rs` **(c)**: POSIX ACLs, capabilities, SELinux.
- `core/src/op/access.rs` **(c)**: on Windows `set_access` sets the
  read-only attribute (the owner's write bit) and refuses a group; with
  no mode on Windows entries, Properties offers neither (ADR-058). It
  changes an item by its path, checked to be the item stat-ed and no
  link just before (`op::sys::access::Held`): a link put there between
  the two is followed, where Unix holds the item itself.
  `fs::count` counts each name of a file of several there (`std` gives
  no link count), and `stat::flags` reads compressed, encrypted and
  read-only from the attributes.
- Hidden by name alone **(c)**: `query/plocate.rs`, `tags/index.rs`,
  `tags/count.rs`; on macOS the `read_dir` walks (`fs::is_hidden`).

## 2. Paths

Done (step B):

- Windows' name rules (`op::valid_file_name`).
- `PATHEXT` (`os_path::find_program`).
- Canonical paths without `\\?\` (`os_path::canonical`, every caller but
  Linux's device code).
- The home folder read one way (`os_path::home`; `~\` on Windows).
- `file:///C:/` and `file://server/share` URIs (`platform/uri.rs`, also used
  by the SQLite readers).
- A drive and its root as one trail segment (`ui/address.rs`).

Left:

- `core/src/os_path.rs`: on Windows, bytes that are not WTF-8 are replaced
  (WTF-8 itself is exact). `is_executable` is true for any file off Unix
  **(c)**.
- `"/"` as a fallback: `ui/terminal.rs`, `places/lib.rs`,
  `platform/apps.rs`, `op/builtin.rs` (several), `op/order.rs`.

## 3. Operations (`core/src/op/sys.rs`, `builtin.rs`)

Done (step C):

- Windows: rename without replacing (`MoveFileExW`, under Wine). A swap is
  refused as a conflict, since Windows cannot exchange two names in one
  step; bulk renames park a name instead.
- A copy made by the system where it keeps more than the bytes
  (`copy_whole`):
  - Windows: `CopyFileExW`, with progress, Cancel and alternate streams (a
    download's mark). Under Wine, except the streams, which Wine lacks.
  - macOS: `clonefile` when attributes are kept (compiled only).
- macOS keeps every attribute a copy should, Finder's included, except two
  the system sets (compiled only).
- macOS copies pipes with `mkfifo` (compiled only).

Left:

- macOS rename without replacing and swap (`renameatx_np`): compiled, never
  run.
- Write-through (`Pace`, `sys::write_range`, `Dir::sync_volume`) is **(n)**
  off Linux, never reached because nothing is removable there (section 6).
  - Linux hands the drive each window of bytes (`sync_file_range`), waits
    for it a window later, and writes the volume out (`syncfs`) once per
    plan and where moves wait to remove their originals.
  - Constraint: committing a volume or flushing a drive's cache costs about
    the same however little was written. On Linux, `syncfs` every few MiB
    held a copy onto a USB hard drive to 7 MiB/s where the drive takes 80.
    So neither `F_FULLFSYNC` nor `FlushFileBuffers` may be paid per window
    or per file.
  - Windows plan: `CopyFileExW` with `COPY_FILE_NO_BUFFERING` (bypasses the
    system cache; progress counts what the drive took), then
    `FlushFileBuffers` once per plan on a copied file. To check: whether
    that writes the other files' names out on FAT and exFAT.
  - macOS plan: `fsync` per window (writes to the drive without flushing
    its cache) on its own thread so the copy goes on; `F_FULLFSYNC` once
    per plan.
  - Measure each on a USB hard drive before choosing.
- Links copied (`symlink_like`): Windows makes a folder link as a folder
  link; it needs a privilege. Untested.
- Windows: the staging folder keeps the access it inherits; read-only is
  the only permission opened and restored (`opened_to_owner`).
- The property tests (`apply_then_undo_restores_the_tree`) skip under Wine,
  whose shell does not trash what an undone copy trashes.
- `platform/file.rs` `rewrite` **(c)**.

## 4. Trash

Done (step D):

- One module, `core/src/op/records.rs`, knows where a trash keeps an item
  and what it records: freedesktop's `files/x` and `info/x.trashinfo`;
  Windows' Recycle Bin `$R` item and `$I` record (both record versions,
  parsed on Linux from fixtures).
- `in_trash`, `trash_info_of`, restoring a part, and taking a file out and
  putting it back read either. A plan keeps a record as bytes (a freedesktop
  record serializes as text, as before).
- The platform lists each local drive's bin for the user's SID
  (`platform/windows.rs`, `trash::trashes`), so `trash:` shows it.
- Trashing and restoring a whole item go through the `trash` crate (the
  shell on Windows).

Left:

- None of the Windows side ran: Wine's Recycle Bin is the freedesktop trash
  and its shell does not trash (trash tests skip under Wine). The record
  format and the listing ran on Linux, on fixtures.
- macOS: `trashItemAtURL` trashes an item (`op/sys.rs` `trash_item`); undo
  keeps the resulting path (`Restore::item`, companions in
  `Restore::landed`) and moves them back (compiled only). `~/.Trash` records
  no origin, so the trash crate's listing and restoring stay **(n)**
  (`trash_list`, `trash_restore`) and `trash::trashes` lists none **(u)**:
  `trash:` is empty on a Mac, and only undo restores.
- Windows: the `trash` crate does not say which `$I` record it wrote, so
  undoing a trash restores the latest item from the path, not its own
  (Linux names its record, ADR-054). A replace undone (a name taken,
  "Replace") then brings back the copy it just put in the trash; the
  original stays there. Finding the record after trashing (the newest from
  the path) would fix it.

## 5. Attributes and notes stores

- `core/metadata.rs` `user.xdg.tags`, `tags/xdg.rs` (Dolphin's names): macOS
  attributes have no `user.` namespace **(c)**.
- `tags/finder.rs` **(c)**: Finder tags as Linux sees them; the native name
  is already under `cfg(target_os = "macos")`.
- `tags/appledouble.rs`, `core/companions.rs`: `._` files.
  `platform/filesystem.rs` `statfs` magic is Linux's; **(u)** elsewhere:
  never AppleDouble, change times never trusted.
- Windows refuses every attribute, so by the notes policy an edit goes to
  the store the folder already uses, else AppleDouble (not exercised).
- xmp, tagspaces, name-tags: portable.

## 6. Volumes, places, bookmarks (step F)

Done:

- Windows: devices are the local drives, fixed and removable (never a
  network drive or a disc, which can hang the caller), each with label,
  letter, filesystem, removable flag and free space (`platform/windows.rs`,
  under Wine).
- macOS: the root and the volumes under `/Volumes` the Finder shows
  (`getmntinfo`, compiled only).
- The user's folders are the system's off Linux (`dirs`: Windows' known
  folders, a Mac's), under Wine.
- Encrypted volumes (ADR-088), compiled only. macOS: the APFS volumes
  `diskutil apfs list -plist` says are locked are listed, unlocked with
  the passphrase on `diskutil apfs unlockVolume`'s input, remembered in
  the Keychain (the parser tested everywhere). Windows: a drive BitLocker
  keeps locked is told by `FVE_E_LOCKED_VOLUME` and opened through
  Explorer's Unlock Drive verb (`unlock-bde`), Windows' own dialog, which
  remembers; Wine has no BitLocker (its drives are never locked, tested).

Left:

- Eject and mount off Linux **(n)**: no command (DiskArbitration,
  `IOCTL_STORAGE_EJECT_MEDIA`); `Mount::udisks` is false there. A Mac's
  APFS volume unlocked is mounted by `diskutil`; only locked ones are
  listed unmounted.
- Whether `diskutil apfs unlockVolume` runs from Skwir's window without
  Full Disk Access, and whether `bdeunlock.exe` shows on Windows Home
  **(u)**: untried (ADR-088).
- Write-through off Linux: `fs::volume::is_removable` is false **(u)**.
  Windows calls a USB hard drive fixed (`GetDriveType`), so ask its bus, as
  on Linux; Windows' default policy for external drives (quick removal)
  already keeps their writes out of its cache. On a Mac DiskArbitration
  says. How to write through: section 3.
- Bookmarks are GTK's **(c)**: Finder's favorites and Windows' Quick Access
  are not read.
- `plugins/places` watches `/dev/disk/by-uuid` and `/run/media/$USER`
  **(c)**, absent elsewhere. On Windows a drive plugged in appears at the
  next rescan, not at once (`WM_DEVICECHANGE` not heard).
- Linux's own: `core/fs/volume.rs` (sysfs, mountinfo),
  `platform/places.rs` (udev, `lsblk`, `udisksctl`, `flush`'s `syncfs`).
- `vim/lib.rs`, `jump/xbel.rs` **(c)**.
- Remote folders (ADR-077): `fs::reach` reads `getfsstat`'s `MNT_LOCAL`
  on a Mac and a share's path or a mapped drive (`GetDriveTypeW`) on
  Windows, compiled only. The system's own shares are a connector
  (`remote::system_shares`, first off Linux; `skwir_platform::shares`):
  Windows' SMB and WebDAV (through its WebClient) by their UNC path,
  connected with `WNetAddConnection2W`; macOS's SMB, AFP, NFS and WebDAV
  mounted by NetFS under `/Volumes`; their addresses' parsing tried under
  Wine, the calls compiled only. sshfs needs a FUSE: on macOS macFUSE or
  FUSE-T (looked for; mount points in the user's own temporary folder,
  macOS having no runtime folder); rclone mounts with one where there,
  else through `rclone nfsmount` (its NFS server, the system's mount),
  compiled only. On Windows rclone mounts through WinFsp (ADR-086) in a
  folder of Skwir's local application data (`%LOCALAPPDATA%\skwir\remote`,
  never a drive letter), started with no window and left running, its
  remote control on the loopback behind a login made for it (kept beside
  the folder while it lives), asked to quit to let go of it; the core
  knows its mounts by their being mount points there
  (`fs::reach::mounts_in`). Compiled and its parts unit-tested; WinFsp is
  a driver Wine lacks, so nothing is mounted under Wine. rclone's remote
  control on the loopback and the configuration's forms' session are
  tried under Wine with rclone's own Windows release, pinned
  (`tools/test/rclone-windows.sh`, `just test-windows`): Wine has no Unix
  socket (`WSAEAFNOSUPPORT`). sshfs does not run on Windows. GVfs, KIO's
  kio-fuse (ADR-081) and fstab are Linux's, and so are phones and
  cameras among the devices (ADR-096: GVfs's volume monitors, its MTP,
  gPhoto2 and AFC): Windows reaches a phone through its Portable Devices,
  which are no folders, and macOS has no MTP; an iPhone's files there
  would be a connector of its own. A phone KDE's kmtpd holds is reached
  through Skwir's helper (`skwir-kmtp-worker`, ADR-097), Linux only too:
  elsewhere it answers `--probe` that it cannot work. The askpass is a named pipe
  on Windows (`skwir_platform::askpass`): rclone's own Windows release
  asks an encrypted configuration's password through `skwir.exe` under
  Wine; Windows' own OpenSSH running it as `SSH_ASKPASS` is not tried
  (Wine has none), nor the pipe's access entry refusing another user.
  Eject asks what uses a mount of `/proc` on Linux, of `lsof` on a Mac
  (compiled only). A paused plan holds its tool: `SIGSTOP` to its group
  on Unix, each thread of its job's processes suspended on Windows
  (under Wine).

## 7. Types, icons, applications (step E)

Done:

- Skwir's own folders per system (`core/src/folders.rs`): `~/Library/…` on
  macOS, `%APPDATA%`/`%LOCALAPPDATA%` on Windows, XDG on Linux.
- Keyring: Windows' Credential Manager (under Wine's); macOS's Keychain
  (compiled only).
- The terminal to open: on Windows, Windows Terminal or a console of its
  own, and a program run in a console that ends with it; on macOS `open -a`,
  which runs no program in it yet **(n)**.
- First weekday from the locale: Windows' `GetLocaleInfoEx` (under Wine),
  macOS's `NSCalendar` (compiled only).
- Types (ADR-037): the system's registry first (`platform/mime/uttype.rs`,
  UTType, compiled only; `platform/mime/registry.rs` over
  `windows::classes`, under Wine), shared-mime-info behind it. Its reader is
  vendored to build on Windows; its compiled files ship in the macOS and
  Windows packages, built from `vendor/shared-mime-info`
  (`tools/package/mime-database.sh`).
- Icons (2026-10-06): the system's own off Linux
  (`platform/system_icons.rs`). A file's by extension, or its own where the
  system draws each such file differently (Windows' `.exe`, `.lnk`, `.ico`;
  a Mac's `.app`); a folder's, user folder's and volume's by path; the
  trash and drives as stock icons. Windows: the shell's system image list
  (under Wine, whose stock icons are a stub: no trash icon). macOS:
  `NSWorkspace` and `NSImage` (compiled only).

Left:

- macOS types never ran: the `UTType` calls and the table reading
  conformances as kinds (`uttype::KINDS`) are unverified **(5b)**.
- System icons never ran on the systems **(5b)**: whether AppKit's icons
  and images answer off the main thread, as documented; the small icons of
  Windows' jumbo list; a folder with its own icon (not asked: every folder
  is drawn as a folder).
- `platform/apps.rs` **(c)**: desktop entries, `mimeapps.list`, the locale,
  `Exec` codes. Open With lists nothing off Linux (Launch Services and
  Windows' association handlers are not read).
- Owners: Windows entries carry none (SIDs not read). macOS's are uids named
  through `/etc/passwd` **(c)**, which misses Directory Services' users.

## 8. Thumbnails and isolation

- `platform/thumbnails.rs`: the freedesktop cache **(c)**; private
  elsewhere. `thumb_providers.rs` `.thumbnailer` files **(c)**.
- `platform/sandbox.rs` (ADR-039): bubblewrap on Linux. macOS:
  `sandbox-exec` with Skwir's profile, compiled, never run; whether
  LibreOffice, Pandoc, ImageMagick and the decoders start under it, and what
  the profile must let through, is for a Mac **(5b)**. Windows: nothing; an
  AppContainer is to be built and tried on the machine **(5b)**.
  - Without isolation LibreOffice (`platform/preview.rs`) and ImageMagick
    (`platform/magick.rs`; `convert` is a system tool on Windows) are
    refused. Pandoc (`documents/pandoc.rs`) runs in its own
    `--sandbox` (2.15 or later) on every system, and alone where the system
    isolates nothing.
- System thumbnails (ADR-041, `platform/system_thumbs.rs`), at the Generate
  stage after Skwir's decoder: Quick Look on macOS (compiled, never run
  **(5b)**); the shell's `IShellItemImageFactory` on Windows (under Wine,
  whose shell draws none **(5b)**).
- PDFs (ADR-041): the packages carry PDFium; its helper finds it beside
  itself or in a Mac bundle's `Frameworks` (the Windows helper loaded the
  pinned `pdfium.dll` that way under Wine). PDF thumbnails and converted
  office pages are drawn by the engine (`document::first_page`); `pdftoppm`
  only where there is none.
- DjVu (ADR-061): djvu-rs's helper, in Rust, is in the macOS and Windows
  packages (Windows': its `--probe` answers under Wine; macOS's compiled
  only **(5b)**); DjVuLibre's links a library those packages do not carry,
  so it is Linux's alone.
- The Windows package (ADR-074) is made on Linux, a zip and a per-user
  installer of one folder (`tools/package/windows.sh`), and tried under
  Wine (`tools/package/windows-check.sh`): its programs answer, it installs
  and leaves nothing once removed. Its window is not drawn there (Wine's
  DirectWrite has no font sets) **(5b)**. Built on Linux, gpui compiles its
  shaders as the renderer starts (vendor patch 0034).
- The macOS package (ADR-075) is made on Linux too, for Apple silicon,
  against Apple's SDK (`just macos-sdk`): `skwir.app`, signed ad hoc,
  zipped and in a disk image (`tools/package/macos.sh`), looked over by
  `tools/package/macos-check.sh` (its programs, libraries and seal); never
  run **(5b)**. gpui makes its Metal bindings for the target and compiles
  its shaders as the renderer starts (vendor patch 0035). Everything above
  marked "compiled only" for macOS is now linked as well.
- Archives (ADR-041): the archive helper is in both packages, libarchive
  linked into it statically (`tools/package/libarchive.sh`, nixpkgs'
  sources built with the packages' compilers: Zig for Windows, xmac's
  clang for macOS, which gives its own zlib, bzip2, iconv and Expat). On
  Windows it opens an archive by its path's UTF-16 (libarchive's `_w`
  calls), asks the C library for UTF-8 (`.UTF-8`, `LC_CTYPE` 2), reads a
  tar's and a cpio's names as UTF-8 and writes a ZIP's in UTF-8, flagged
  so; a ZIP's or a RAR's unflagged names are read in the OEM code page, as
  Windows' archivers write them. Under Wine its tests and the archives
  plugin's pass (their trash steps skipped), and the package's check makes
  and lists each format skwir makes. macOS's is linked, never run: which
  of its system libraries' symbols macOS 12 lacks, if any, is unknown
  **(5b)**.
- Audio and video (ADR-076): the media helper Linux has, with GStreamer's
  own build of 1.28.8 carried, cut down to what it plays with
  (`tools/package/gstreamer.py`): every decoder of FFmpeg's (gst-libav, 211
  of them) behind the system's (Direct3D 11, VideoToolbox), and GStreamer
  kept apart from any the user installed (`carried_gstreamer`: its plugins,
  scanner and registry). Windows: the helper in `gstreamer\bin` among the
  libraries (`helper_folders`); under Wine the package's check plays H.264
  and AAC, VP9 and Opus through it (FFmpeg's decoders: Wine has no
  Direct3D or Media Foundation decoder). macOS: GStreamer a framework in
  the bundle's Frameworks, the helper's rpath; looked over, never run
  **(5b)**. Turning subtitles off while paused stalled in some runs under
  Wine, never on Linux **(5b)**.
- Left for the machines (ADR-041): LibreOffice on macOS under
  `sandbox-exec` (found where it installs itself, `process::required`).

## 9. Processes, terminal, editors (step H)

Done:

- Windows: a helper and everything it starts are held in a job object and
  ended together on a cancel or a timeout (`platform/windows.rs` `Job`,
  under Wine).
- Whether a process runs is asked of each system (`process::runs`), so a
  running instance's cache folders are never swept off Linux (they were, by
  `/proc`).
- The shell's working folder is read on macOS too (`process::working_dir`,
  `proc_pidinfo`, compiled only).
- The `cd` line is written for PowerShell and `cmd` too (tested on Linux).

Left:

- Windows' terminal (ConPTY through alacritty): compiled, never run. Who
  has the terminal and where the shell is are **(u)** there, so the pane and
  the shell do not follow each other (a shell's own report, OSC 9;9 or 7,
  would say where it is).
- Programs off `PATH` (ADR-041), compiled only: on macOS Homebrew's and
  MacPorts' folders are searched after `PATH` (`os_path::find_program`; an
  application started from the Finder has only the system's folders);
  LibreOffice is found where it installs itself on macOS and Windows
  (`process::required`).
- `platform/process.rs`: a child joins its job just after it starts (std
  cannot start it suspended); whatever it starts in that instant escapes
  the job.
- `line_channel.rs` exit 127 **(c)**; `platform/media.rs` (Unix).
- `plugins/shell`, `plugins/edit-as-buffer` **(c)**: `/bin/sh` wrappers,
  `$SHELL`, `$EDITOR`, `$XDG_RUNTIME_DIR`.

## 10–15. The rest

- **Media keys** (step I): `plugins/mpris` is registered on the free
  desktops only. A Mac's Now Playing and Windows' media controls (SMTC) are
  not written (`skwir_core::playing` is the seam).
- **Hints** (step I): Spotlight answers a tag search on a Mac (`mdfind`,
  Finder's tags; compiled only). Windows Search is not asked.
  `query/plocate.rs` **(c)**, `tags/hints.rs` Baloo **(c)**.
- **Free-desktop shared files**, read through `core/xdg.rs` **(c)**:
  thumbnails, trash, apps, mime, thumbnailers, icons, places; jump, tags,
  vim, geo, edit-as-buffer. Absent elsewhere, so read as nothing.
- **Fonts**: `mupdf-worker/fonts.rs` `fc-match` **(c)**. Each system's UI
  fonts after Inter (step J, done, never run): Helvetica Neue and Menlo on a
  Mac; Segoe UI and Cascadia Mono or Consolas on Windows. The font
  settings (ADR-073) offer every family the text system lists there:
  `platform/fonts.rs` asks fontconfig which write Latin and which are
  monospaced, and resolves its aliases **(c)**; a Mac (CoreText's traits)
  and Windows (DirectWrite's `IsMonospacedFont`) say nothing yet, so a
  family that writes no Latin is offered and drawn as Automatic, and code
  is offered every family. Neither names a font of the desktop's
  (ADR-071).
- `alloc/lib.rs` ELF constructor (Linux); `skwir/gpu.rs` sysfs **(c)**.
- **Keys** (step J, done): the standard shortcuts are `secondary` (Cmd on a
  Mac, Ctrl elsewhere; Linux unchanged); bindings are compared as GPUI reads
  them (`keymap::normal_keys`); key caps say ⌘ and ⌥ on a Mac; the
  pointer's Ctrl gestures (Ctrl+click, Ctrl+wheel, a band kept) are Cmd
  there. Ctrl is kept on a Mac where macOS reserves Cmd or gives it another
  meaning (Cmd+H hides an app, Cmd+Space is Spotlight, Ctrl+arrows switch
  Spaces), for the terminal's keys, Ctrl+Tab, and word moves (a Mac's are
  Option+arrows: not done). Compiled for Windows, never for a Mac
  (skwir-ui).
- **The screen kept awake** during a slideshow or a film is gpui patch
  0023: Wayland's idle inhibitor and X11's screen saver, tried on Linux;
  Windows' display requirement, compiled only; nothing on a Mac yet (an
  `IOPMAssertion`).
- **Files on the clipboard** (ADR-062) are gpui patch 0024, Wayland's and
  X11's (`ui/clipboard.rs` behind `cfg(target_os = "linux")`). On macOS
  (`public.file-url` items; no cut, Finder moves at paste time) and Windows
  (`CF_HDROP` and "Preferred DropEffect"; gpui's `CF_HDROP` read joins the
  names with no separator) Skwir's own clipboard stays apart from the
  system's, and a paste with nothing in it says there is nothing to paste.
- **Drag and drop**: a drag leaves the window as gpui hands it to the
  platform, on Wayland and macOS; on Wayland under Skwir's icon (gpui
  patch 0001), on macOS under the icons of the files' types. X11 and
  Windows hand no drag out yet. A drag's copy modifier is Ctrl; a Mac's
  is Option (step J, not done).
- **3D models turned in the window** (ADR-067) are gpui patch 0033: wgpu's
  pass on Linux, Direct3D 11's and Metal's from naga's translation of the
  same shader. Direct3D's pass ran under Wine (its d3d11 on Mesa's
  llvmpipe, `just gpui-check-windows`), but not through gpui's renderer,
  whose own shaders Wine cannot compile; whether a model turns in the
  window on a real GPU, and how fast, is for the machine **(5b)**. Metal's
  is compiled only (gpui_apple, with an empty shader library standing in
  for gpui's own) **(5b)**. WARP, Windows' software rasterizer, gets the
  still, as lavapipe does.
- **Agents and scripts** (ADR-043): on Windows the window listens on a named
  pipe in the user's name (`plugins/control/src/channel.rs`), tried under
  Wine (`tests/pipe.rs`), which keeps no accounts apart. Left for Phase 5b's
  gate: the pipe has Windows' default security, which lets every account
  open it to read, and a pipe another account created first under that name
  is taken for Skwir's (the window does not listen, and `skwir mcp` would
  talk to it). The pipe needs a DACL of the user alone, and the client a
  check of the server's account (`GetNamedPipeServerProcessId`). On macOS
  the socket is in the user's own `$TMPDIR`, compiled only.
- **Plugins from outside Skwir** (ADR-045, ADR-046): a module runs the same
  everywhere (Wasmtime; compiled for Windows and macOS, run on Linux only).
  A program is isolated by bubblewrap on Linux and `sandbox-exec` on macOS
  (`sandbox::plugin`, compiled only), and does not run on Windows, which has
  no isolation yet (ADR-039): it says so (`system::not_yet`).
- **Bench**: the raw listing is the system's own (readdir and fstatat on
  macOS, `std::fs` on Windows), compiled, never run; its budgets are
  Linux's (step L).

## Tests that run on Linux alone

Step A gated them `#[cfg(unix)]` (links, modes, scripts standing in for
programs, names that are not UTF-8, attributes, Usage's walk) or
`#[cfg(target_os = "linux")]` (pipes, the freedesktop trash, Baloo, the type
database, icon themes). Each port step that gives a system the code turns
the matching tests on for it.

## Hardest first

1. Identity, stat and listing (step B).
2. The trash (step D).
3. The type database off Linux (step E, ADR-037: done).
4. The converters that need isolation (step G, ADR-039: macOS and Pandoc
   done, Windows on the machine).
5. Volumes and places (step F).
6. The copy engine's semantics (step C).
7. The terminal (step H).
8. Process trees on Windows and `PATHEXT` (steps B, H).
9. Byte paths in what is stored and serialized (step B).
