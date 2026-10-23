# Changes

## Unreleased

- **Your desktop's colors**: on Plasma, Skwir wears your color scheme;
  elsewhere the colors of the GTK theme your desktop names (adw-gtk3,
  Materia and most others), each with your accent, and follows them as
  you change them. Appearance › Desktop colors turns it off.
- **Contents** highlight the part you are in (a PDF's section, a book's
  chapter, a heading) and follow you as you read. They no longer close
  when a book moves to its next chapter, and the new pin at their top
  keeps them open beside the text, which narrows to make room; the pin is
  remembered. A book's sections are indented under their chapters, and
  its parts are in its contents with their chapters under them; a PDF's
  outline nests as deep as it goes (with PDFium, all below its second
  level stood at the second); a file's headings are indented by the
  headings they fall under, never two steps for a level skipped. A
  book's contents no longer list the chapter's own headings again above
  its chapters: those its contents leave out stand under the chapter
  you are reading.
- **Scripts and agents**: a file selected, a folder opened or a sort
  changed by `skwir cmd` or an agent shows in the window at once, not at
  the next move of the mouse.
- **Deleting permanently**: a file's sidecars (XMP, a Mac's `._` files,
  TagSpaces' notes) show as a small "+ metadata" mark after its name, and
  where they are when you point at it. A long name keeps the room: a
  sidecar's name no longer cuts it to "…", and it is cut in its middle,
  keeping its extension, as in the file views.
- **A search of the whole disk** that plocate answers says how old its
  answer is ("as of Yesterday 03:00", plocate's database at its last
  update): a file made or renamed since is not in it. Agents and scripts
  are told too (`entries` answers `as_of`).
- **Remote folders**: type an address (`sftp://`, `smb://`, `dav://`,
  `ftp://`, `nfs://`, `afp://`…) and Skwir mounts it with what you have
  installed for it (sshfs, GVfs, rclone, or your fstab's line), then
  shows its folder; a password or a server it does not know is asked in
  the window and never kept. They sit in the sidebar's Network section,
  are bookmarked by their address (renamed and moved among your other
  bookmarks, in the order other file managers show), and Eject lets go
  of them. A server's thumbnails show where it is on your network, and
  for small files elsewhere once a copy from it proved it fast. Nearby,
  at the end of the Network section, lists the servers your network
  announces (a NAS, a Mac, a Windows PC), found by Avahi and wsdd while
  you look at it. Copies to
  and from a server go in batches through rsync, sftp or gio where those
  are faster, a large file comes down in several parts at once, a copy
  within one server is made on the server, and many files are deleted
  there in one go. A phone plugged in over MTP takes and gives files two
  to three times faster than before. Undo of a copy works where a server keeps no trash:
  what was made is removed, while it is unchanged. On KDE's desktop,
  `smb://`, `dav://` and `ftp://` open through KIO (kio-fuse), with the
  passwords KDE keeps, KDE's own dialogs asking a password or about a
  certificate; GVfs is used where it is installed, even before
  anything started it. A copy a server refuses as it is finished is said
  to have failed, and a move keeps its original. Copying onto a camera (or a
  phone in its photos mode) through GVfs works: the copy is written in place,
  as the camera moves nothing between its folders. A server's password can
  be remembered in your desktop's keyring (Remember in the password
  question), and passwords Nautilus remembered are used too: a
  password-protected SMB, WebDAV or FTP server now opens through rclone.
  Forget Password in its menu removes it. Copies to and from KDE's
  remotes go several at once through KIO's own tool, three to fourteen
  times faster than through its folders, and copying onto an FTP server
  through KIO works.
- **Actions at once**: a rename, a new folder or a tag no longer waits
  behind a long copy it shares nothing with, and copies between
  different disks or servers run together (four actions at most); two
  long ones on one disk still take turns, which ends sooner than sharing
  it. Undo takes back the last thing you asked for, and waits for it if
  it still runs.
- **Retry**: an action an error stopped (a server gone, a disk full, a
  drive pulled out) is retried from where it stopped, from its message
  or `skwir cmd retry`. A large file goes on from what got across once
  its last part is checked; a folder keeps the files it had finished,
  and the file it was sending goes on too (by rsync, sftp or Skwir's own
  copy; `gio` and KIO's copies start it again). Undo takes the whole
  action back. An action a server stopped goes on by itself once the
  server answers again: its message says it waits, and its bar says it
  goes on.

## 0.2.0 (2026-10-20)

Skwir 0.2.0 turns 3D models in the preview, opens DjVu books, takes the
desktop's accent color and fonts, and can be followed with a screen reader. It
now draws with gpui from Zed's main branch, on wgpu.

[Installation instructions](README.md#installing)

- **3D models** turn in the preview, drawn by the GPU: drag to turn them, Shift
  or the middle button to move them, the wheel to zoom. STEP, IGES and the
  meshes Rhino files store join STL, OBJ, PLY, glTF and 3MF, each in its own
  colors. Perspective and smooth shading are settings, and the 3D button shows
  the model's picture instead, remembered for its type.
- **DjVu** books and scans open in the reader, with search, text selection,
  links and contents, through DjVuLibre where it is installed
  (`skwir-djvulibre`), else the built-in djvu-rs reader.
- **The desktop's look**: Skwir wears the accent color and the higher contrast
  your desktop asks for, and writes in its interface, document and monospace
  fonts, following them as they change. Every color is now kept at a readable
  contrast.
- **Fonts**: the interface, reading and code fonts can each be any installed
  family, chosen from lists that narrow as you type and show each font in
  itself.
- **Screen readers** (AT-SPI, such as Orca) follow the file views, the sidebar,
  the path, fields, dialogs, menus, notifications, the previews (a document's
  page, tables, archives, maps, bytes) and the terminal. F6 goes round the
  sidebar, the files and the preview.
- **Fullscreen** for pictures, documents and text, not only films (`F`, or a
  double click), with a slideshow (`Space`) and the arrows walking the folder's
  pictures. The screen stays awake while a slideshow or a film plays.
- **Previews**:
  - Ctrl and the wheel zoom every preview around the pointer, the arrows scroll
    and turn pages, and what is wider than the panel scrolls sideways, with a
    bar.
  - Find marks every match and goes from one to the next.
  - Full preview shows a file past the limits that keep previews quick.
  - Source view is remembered for each type, and a file that fails to render
    shows its text.
  - The next picture is decoded ahead, in the panel, in Peek and on the whole
    screen. Films show their first picture sooner and sharper.
  - Code previews use far less memory, freed a minute after code was last
    shown. PowerShell, Sass, LiveScript, Salt, ARM assembly and Babel's
    JavaScript are colored too.
  - In books converted from a word processor, a note marked by a plain `[1]`
    opens under it.
- **Links in what you read**: paths written in text, code and Markdown are
  links under Ctrl, looked up on this machine only; a link to a heading or a
  line (`#setup`, `#L12`) lands there; Obsidian's `[[links]]` work; links
  inside an archive lead to its own files; Back returns to the document.
- **Address bar** completion: folder and file names as you type, ranked by
  where you go (zoxide); Tab completes, and `~/d/pr/` finds
  `~/Documents/projects/`, as in fish.
- **Clipboard**: files copied or cut in Skwir paste in other file managers
  (GNOME Files, Dolphin, Thunar and others), and theirs paste in Skwir. Copy
  Path (`Ctrl+Shift+C`, `Ctrl+Alt+C`) copies plain paths.
- **Terminal**: `Ctrl+Shift+C` and `Ctrl+Shift+V` copy and paste; a menu with
  Select all and Clear history; files dropped on it, or the selection with
  `Ctrl+Shift+Enter`, are typed at the prompt, quoted.
- **Browsing**:
  - The columns view shows the folder under the cursor as you move, as Finder
    and yazi do, and scrolls sideways when the pane is narrow.
  - Up selects the folder you came from, Escape leaves a search, and after a
    deletion the next file takes the cursor.
  - Several files dragged fan out behind the one taken, as thumbnails or icons
    (Appearance › Dragged files has other layouts).
  - Extract Here gives an archive's single folder or file as it is, whatever its
    name.
  - A key held down repeats only what moves; `Space` held shows Peek until it is
    released. Typing hides the pointer until the mouse moves.
- **Reviewing changes**: deleting permanently lists what goes, with sizes and
  counts; a proposal's review lists every step, scrolled, from the folder they
  share; items of the Trash are named as they came.
- **Settings** are arranged by subject, each page showing the few that matter
  first. Changed by you lists everything that differs from the defaults.
- **Tags in names** (TagSpaces, filetags) can follow tag edits: Settings › Tags
  and notes, Tags in names, Write: Update existing.
- **Properties**: a folded group says its gist (size, dimensions, tags…).
- **Arch**: `skwir-bin` installs the release's own build without compiling;
  `skwir-git` builds the latest commit.
- **Under the hood**: wgpu draws over Vulkan, or OpenGL where no Vulkan driver
  can draw to the window. rustls, Wasmtime and zip were updated for security
  advisories.

Changed from 0.1, which you may notice:

- Skwir takes the desktop's accent color and fonts. Turn Appearance › Desktop
  colors off to keep its own.
- `Ctrl+Shift+C` copies paths as plain text, no longer quoted for a shell: that
  is "Copy paths for a shell" in the palette.
- Escape with nothing selected no longer forgets files cut or copied.
- The wheel scrolls along its own axis only: Shift and the wheel scroll
  sideways.
- The commands `ui.media_fullscreen` and `ui.media_controls` are now
  `ui.preview_fullscreen` and `ui.fullscreen_controls`: a shortcut or a script
  naming the old ones needs the new names.
- Settings moved to pages by subject; their keys and the settings file are
  unchanged.

Known limits: Linux only, as in 0.1 ([how to help](docs/HELP-WANTED.md) with
macOS and Windows). Screen reader support is new and was checked through
AT-SPI, not yet in daily use; a tree's levels are not read out yet. Undo history
is cleared when Skwir exits.

## 0.1.0 (2026-10-13)

Skwir 0.1.0 is the first release of an experimental file manager for Linux.
Folders stream into view, searches can be saved as places, and bulk changes are
reviewed before they run. It supports Wayland and X11 and requires Vulkan.

[Installation instructions](README.md#installing)

- **Browsing**: list, grid, columns, tree and board views; two panes and tabs; a
  sidebar you arrange, with drives mounted and unlocked from it; recent folders;
  a shelf for gathering files; the Trash, with files restored to their original
  location or wherever you drop them.
- **Searching** (`Ctrl+F`): names, types, sizes, dates in your local time zone,
  tags, ratings, contents and geographic coordinates. Search terms appear as
  editable pills. Save searches in the sidebar and filter their results by
  metadata.
- **Tags, ratings and comments** kept on the files, using formats shared with
  Dolphin, darktable, Lightroom, Finder and TagSpaces. Notes from different
  stores are merged, tags can be browsed as places, and copies preserve them.
- **Changes as plans**: copies, moves, renames and edits to tags are undoable.
  Bulk renames, editing a folder as text, and changes proposed by agents or
  plugins are reviewed before they run. Irreversible operations ask for
  confirmation.
- **Copies and moves** with progress, pause and cancellation. Space is checked
  before starting, and copies to USB drives report progress at the drive's pace.
  When destination names are taken, choose once: keep both, replace into the
  Trash, replace if newer, or skip. Folders merge, and hard links are preserved.
- **Archives and drag and drop**: choose to move, copy, link or extract. Create
  ZIP, 7z and tar archives from a selection, including encrypted ZIPs.
  Extraction asks for passwords and handles destination conflicts.
- **Properties** (`Alt+Enter`): folder contents counted on demand, space used on
  disk, editable permissions, application associations, and SHA-256, MD5 and
  BLAKE3 checksums you can compare with a copied value.
- **Previews**: text and code, Markdown, tables, PDFs, office documents, books,
  comics, e-mail, pictures, audio and video, fonts, 3D models, circuit boards,
  maps, archives and raw bytes. The player keeps playing while you browse. Some
  previews require optional packages or external tools.
- **Other ways to explore**: disk usage by size, file count, age or type; a
  timeline; a map; and an integrated terminal that follows the folder.
- **Keys and settings**: editable shortcuts with conflict checks, an optional
  Vim scheme based on yazi, and settings shared across running instances and
  reloaded when their file changes.
- **Agents and scripts**, off by default: `skwir cmd` and `skwir mcp`, with
  permissions and review for agent actions. Folders marked private are withheld
  from agents and external plugins.
- **External plugins** as WebAssembly modules or isolated programs. Their
  protocol is experimental and may change.
- **Packages** for Debian 13, Ubuntu 24.04, Fedora 44 and Arch Linux; an
  AppImage and tarball for x86-64 Linux with glibc 2.35 or later; and a Nix
  flake with a Home Manager module.

Known limits: Linux only for this release. The macOS and Windows code compiles,
and Windows tests run under Wine, but neither port has been tested on its native
system ([how to help](docs/HELP-WANTED.md)). Screen readers are not supported
yet. Undo history is cleared when Skwir exits.
