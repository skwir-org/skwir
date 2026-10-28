# Changes

## Unreleased

Skwir 0.3 reaches servers, phones and cameras: type an address and it mounts
the server with a tool you have, lists the servers your network announces and
the phones you plug in, and copies to and from them in batches, going on from
where a server stopped. The address field is now one field to go somewhere,
filter the folder and search below it as you type. Menus open their submenus
beside them, F1 lists keys by topic, and a tag looks the same everywhere. Each
release now offers builds to try on Windows and macOS.

[Installation instructions](README.md#installing)

- **Servers**: `sftp://`, `smb://`, `dav://`, `ftp://`, `nfs://`, `afp://`,
  `s3://` and rclone's remotes open as folders, mounted by sshfs, GVfs, KIO or
  rclone, whichever you have. Passwords are asked in the window and kept in
  your keyring only if you ask. Servers stand in the sidebar's Network section,
  added and set up from Skwir, and Nearby lists those your network announces
  ([the guide](docs/guide.md#servers-phones-and-cameras)).
- **Phones and cameras** are listed as soon as they are plugged in, and mounted
  once you open them. On KDE's desktop, a phone KDE holds opens through it.
- **Copies** to and from a server go in batches, a large file in parts, and
  within one server on the server. Actions that share nothing run at once, and
  an action an error stopped is retried from where it stopped.
- **The address field**: `Ctrl+Shift+F` filters this folder and `Ctrl+F`
  searches it and its subfolders, both as you type, one switch between them.
  The field offers places, tags, tabs, history and commands; `#tag` finds a
  tag; a Search button ends it.
- **Menus** open their submenus beside them, show a setting as one row of
  choices, and group the file menu by what is done to the file. **F1** lists
  the keys by topic, in columns, and finds a shortcut by what it does or by its
  keys, as Settings › Keyboard does.
- **Tags** are drawn as the same chips everywhere, named on the grid's tiles
  where there is room, each with its menu; a pinned tag is a command you can
  give a key.
- **Tabs** are pills, a split tab named by both its panes, or a list in the
  sidebar.
- **The timeline** counts size as well as files, by the time you choose
  (modified, created, opened, or when a picture was taken), and takes drags and
  the keyboard.
- **Encrypted drives** can remember their passphrase in your keyring.
- **Builds to try on Windows and macOS** (Apple silicon, macOS 12 or later)
  come with the release, opening archives and playing audio and video now. They
  are made on Linux and have never run on either system
  ([how to help](docs/HELP-WANTED.md)).

Changed from 0.2, which you may notice:

- `Ctrl+F` searches the folder and its subfolders as you type, after a short
  pause; a search that reads contents (`content:`), or the subfolders of a
  server, waits for `Enter`. `Ctrl+Shift+F` filters the folder alone, as words
  typed after `Ctrl+L` still do.
- The Jump overlay is gone: `Ctrl+J` opens the address field, your places
  first.
- In a search, a word starting with `#` is a tag: `#print` is `tag:print`.
  Quote a name that starts with `#` (`"#3"`).
- What you paste, drop, make or extract in a folder is selected there.
- The file menu is regrouped: opening it, copying and keeping it, changing it
  (Move to Trash and Delete last), making or finding from it, its views,
  describing it. Compress, Open With and Tags are submenus.
- A toast's Undo takes back the action it reports, and Undo the last thing you
  asked for, waiting for it if it still runs.
- `Ctrl` and the wheel zoom the pane under the pointer, not the one with the
  keys.

Known limits: Linux only, as in 0.2. The Windows and macOS builds are to try,
never run on either system ([how to help](docs/HELP-WANTED.md)). Remote folders
need the tools that mount them; Skwir speaks no network protocol itself. They
were tried against servers run on one machine, a phone and a camera, not every
server and network. Undo history is cleared when Skwir exits.

### All changes

- **Remote folders**: type an address (`sftp://`, `smb://`, `dav://`,
  `ftp://`, `nfs://`, `afp://`…) and Skwir mounts it with what you have
  installed for it (sshfs, GVfs, KIO's kio-fuse, rclone, or your fstab's line),
  in the order Settings › Network › Opening addresses gives, then shows its
  folder. A password, or a server ssh does not know, is asked in the window;
  Remember keeps the password in your desktop's keyring, passwords Nautilus
  remembered are used too, and Forget Password in the server's menu removes
  it. On KDE's desktop, `smb://`, `dav://` and `ftp://` open through KIO, with
  the passwords KDE keeps and KDE's own dialogs asking for a password or about
  a certificate; GVfs is used where it is installed, even before anything
  started it. `s3://bucket/` opens through your own AWS configuration, and any
  remote rclone has opens by its address (`rclone://name/`). The address
  completes the servers Skwir knows and your ssh configuration's hosts.
- **The Network section**: every server stands there, mounted or not: rclone's
  remotes, GVfs's and KDE's, and those you added. A server added by its address
  is kept in Skwir's own list or, on KDE's desktop, in KDE's network folders,
  which Dolphin shows too; Bookmarks stay yours, and a bookmark of a server
  becomes one with Keep as Server. A Network place shows how full its server
  is. Settings › Network lists every server, with the actions of its menu.
- **Servers set up from Skwir**: one Add Server for every kind, a server by its
  address or one of rclone's remotes (a cloud storage, an encrypted remote over
  another, a union of several), in forms made from what rclone asks, a
  sign-in through your browser where a cloud needs one; rclone's configuration
  can be encrypted, its password kept in your keyring. A server's menu edits,
  tests or removes it, and has Mount Options… (read only, rclone's cache,
  bandwidth), Connect at Login (by your system's service manager, for a server
  that asks nothing at login) and, for SFTP, Set Up Key Login…, which puts a
  key of yours on the server over one login.
- **Nearby**, at the end of the Network section, lists the servers your network
  announces (a NAS, a Mac, a Windows PC), found by Avahi and wsdd while you look
  at it, and only then; a server's menu adds it.
- **Phones and cameras**: a phone plugged in for file transfer, a camera and an
  iPhone are listed among the drives as soon as GVfs offers them, and mounted
  once opened, never because they were plugged in. One another program holds
  says which (KDE's kiod6), and on KDE's desktop a phone KDE holds opens
  through KDE itself, by a helper of Skwir's. A phone over MTP takes and gives
  files two to three times faster than before, and a copy
  onto a camera (or a phone in its photos mode) works: it is written in place.
- **A server's folder**: its thumbnails show where it is on your local network,
  and for small files elsewhere once a copy from it proved it fast (its menu
  chooses otherwise). A server that stops answering says so above its files,
  with Go Back and Disconnect, and holds nothing else up; Eject lets go of it
  even then. `Delete` there deletes for good, and asks first: nothing of a
  server is carried to this machine's Trash.
- **Copies to and from a server** go in batches through rsync, sftp or gio
  where those are faster; a large file comes down in several parts at once; a
  copy within one server is made on the server (through rclone's own copy too,
  where its storage copies there); many files are deleted there in one go.
  Copies to and from KDE's remotes go several at once through KIO's own tool,
  three to fourteen times faster than through its folders. Undo of a copy works
  where a server keeps no trash: what was made is removed, while it is
  unchanged. A copy a server refuses as it is finished is said to have failed,
  and a move keeps its original.
- **Actions at once**: a rename, a new folder or a tag no longer waits behind a
  long copy it shares nothing with, and copies between different disks or
  servers run together (four actions at most); two long ones on one disk still
  take turns, which ends sooner than sharing it. Undo takes back the last thing
  you asked for, and waits for it if it still runs; a toast's Undo takes back
  the action it reports.
- **Retry**: an action an error stopped (a server gone, a disk full, a drive
  pulled out) is retried from where it stopped, from its message or
  `skwir cmd retry`. A large file goes on from what got across once its last
  part is checked; a folder keeps the files it had finished, and the file it
  was sending goes on too (by rsync, sftp or Skwir's own copy; `gio` and KIO's
  copies start it again). Undo takes the whole action back. An action a server
  stopped goes on by itself once the server answers again: its message says it
  waits, and its bar says it goes on.
- **Encrypted drives**: the passphrase question offers to remember it in your
  keyring, as for a server's password; the drive then opens without asking,
  until its passphrase changes. Forget Passphrase is in its menu. One Nautilus
  remembered opens it too. A wrong passphrase is asked again at once. On a Mac,
  an encrypted APFS drive that is plugged in and locked is listed, and unlocks
  from the sidebar; on Windows, a drive BitLocker locked opens Windows' own
  unlock dialog (macOS and Windows untried).
- **The address field** offers what the text can do, from every source: a
  path's names, the places you go to (Jump's, now in the field: `Ctrl+J`),
  tags, other tabs, where the pane was, saved and recent searches
  (`Shift+Delete` forgets one), commands with their keys. What it understood
  is tinted as you type, and what it cannot read is underlined. A scope stands
  before the text: the folder shown, or another chosen with a click,
  `Alt+Down` or `@` (the folder above, home, the whole disk, a sidebar folder,
  a tag, Recent).
- **Filter and search, one field**: `Ctrl+Shift+F` looks in the folder alone
  and `Ctrl+F` in its subfolders too, the switch at the top of the list going
  from one to the other with the words kept. The files show what the words keep
  as you type: the folder filtered at once, or the search after a short pause.
  A folder's filter is drawn as a search's pills, right after its trail, and a
  search keeps its folder's trail. `dunes jpg` finds `dunes.jpg`; `#print` is
  `tag:print`, and `#` alone offers the tags. A Search button ends the field
  ("Search this folder and its subfolders", "Edit this search"). Taking a
  search's last pill away shows its place whole.
- **Menus**: a submenu opens beside its row, as elsewhere on the desktop; an
  item's state is a check or a choice, never words in its title; a setting
  with a few values is one row of buttons, swatches or stars. View and More are
  rebuilt that way, and popups that are menus look like the context menu. The
  file menu is grouped by what is done to the file, related actions in a
  submenu named by what they do (Compress, Open With, Tags), Edit Tags… first
  among the changes. The context menu stays within the window, and no tooltip
  shows over it.
- **Keys**: F1 lists the keys that work where you are by topic, nearest first,
  in as many columns as fit, an action with several keys on one row, with a
  scrollbar. F1 and Settings › Keyboard find a shortcut by words of what it
  does (`rename`) or by its keys (`ctrl t`). `Ctrl+Shift+Space` works on
  layouts where Shift and Space type another space (French Ergo-L, bépo), and
  `Enter` and `Space` press a dialog's focused button.
- **Tags** are one strip in every view: each tag a chip with its name, giving
  way to dots and then "+N" as room shrinks; named on the grid's tiles where
  they are large enough, and as captions under the names where chosen. Pinned
  tags come first, a tag named after its color is a dot, and a place leaves out
  the tag it says of all it lists. A tag has its menu wherever a file shows it,
  the columns view and the preview's header show tags, a screen reader hears
  them, pointing at a tag in the sidebar rings its files, and each pinned tag
  is a command to bind to a key. The sidebar, menus and the trail draw a tag
  as the same chip, and Properties' folded Notes heading too.
- **Tabs** are pills, the tab shown a card, the × on it and on the one under
  the pointer. A split tab is named by both its panes, the one without the keys
  dimmer, and its icon draws its layout. General › Tabs lists them in the
  sidebar instead.
- **Your desktop's colors**: on Plasma, Skwir wears your color scheme;
  elsewhere the colors of the GTK theme your desktop names (adw-gtk3, Materia
  and most others), each with your accent, and follows them as you change them.
  Appearance › Desktop colors turns it off. Text in the accent now reads at a
  text's contrast (the address's scope, a selected folder), and the sidebar
  stands apart from the files.
- **Timeline**: the period pressed is the one shown, and each place opens on
  the finest that shows all its dates. Day or week can still be picked over
  years: part of the bars shows, and the wheel or the new arrows move along.
  Bars are labelled with their months and years (or days) where there is room,
  and say their count above them when they are wide enough. Clicking a bar in
  the list a bar opened narrows it further (a year, then a day) instead of
  failing. A file dated far from all the others no longer squeezes the rest
  into a few bars: it is counted aside.
- **Timeline, more to it**: bars of **size** as well as of files; a menu for
  which time they count, modified, created, last opened, or when a picture or
  a video was **taken** (its camera's clock, so photos copied later still fall
  when they were shot); drag across bars to list them all; the files selected
  marked at the foot of their bars, today's bar underlined. The strip takes the
  keyboard (`F6`: arrows, `Shift`, `Enter`), and a screen reader hears each
  bar.
- **Search** by when a picture was taken: `taken:2023-04`,
  `taken:2023-04-15..2023-04-17`, `taken>=last-year`, with a Taken filter;
  Created and Accessed filters too.
- **A search of the whole disk** that plocate answers says how old its answer
  is ("as of Yesterday 03:00", plocate's database at its last update): a file
  made or renamed since is not in it. Agents and scripts are told too
  (`entries` answers `as_of`).
- **View buttons**: choose which views have a button beside the address.
  Right-click the buttons (or press Menu on one, or View › Buttons…) and check
  the views you switch to, the tree, the board, Usage or the map among them;
  uncheck those you never use. The view shown keeps its button. Default buttons
  brings back list, grid and columns.
- **Columns view**: a click on a file in a column to the left goes up to that
  folder with the file selected, as in Finder, so it is previewed; it did
  nothing.
- **Zoom**: `Ctrl` and the wheel zoom the pane under the pointer, and the
  cursor stays in sight through a zoom.
- **What you put in a folder is selected**, as in other file managers, and
  scrolled to: a new folder or file (or the folder of that name that was there
  already), what you paste or drop, a copy of a file beside itself, an archive
  you make or extract. Copied to the other side of the split (Shift+F5), it is
  selected there, as what you restore from the Trash is where the other side
  shows its folder. A selection you make while it is being done is kept.
- **Properties** show when anything about a file last changed (Changed: a new
  name, permissions or tags move it, Modified only its contents), and each time
  says which it is in a tooltip. A folded group says more where its heading has
  room: "Modified 2026-10-25 14:02", "Owned by…", "Downloaded from…", rather
  than the bare value.
- **Deleting permanently**: a file's sidecars (XMP, a Mac's `._` files,
  TagSpaces' notes) show as a small "+ metadata" mark after its name, and where
  they are when you point at it. A long name keeps the room: a sidecar's name
  no longer cuts it to "…", and it is cut in its middle, keeping its extension,
  as in the file views. A plan's review lists every name found taken, scrolled,
  no longer cut at 200, and a long plan shows, and can be cancelled, while it
  is checked.
- **Contents** highlight the part you are in (a PDF's section, a book's
  chapter, a heading) and follow you as you read. They no longer close when a
  book moves to its next chapter, and the new pin at their top keeps them open
  beside the text, which narrows to make room; the pin is remembered. A book's
  sections are indented under their chapters, and its parts are in its contents
  with their chapters under them; a PDF's outline nests as deep as it goes
  (with PDFium, all below its second level stood at the second); a file's
  headings are indented by the headings they fall under, never two steps for a
  level skipped. A book's contents no longer list the chapter's own headings
  again above its chapters: those its contents leave out stand under the
  chapter you are reading.
- **Previews**: a file of several sections (a workbook's sheets, a book's
  chapters, a font's character sheets) keeps both arrows and its tools
  (Contents, the text size, Full preview) as you go from one to the next. Read
  a page at a time, a notch of the wheel turns one page, from a page that fits
  the panel too. Find says that nothing matches in its own popup, under what
  you typed, rather than at the bottom of the window; so does the hex view's
  "Match at 0x…". A protected EPUB says so rather than show its encrypted
  chapters, and a film made to be stretched (anamorphic) shows its poster in
  its own shape.
- **Fixed from 0.2**: a document read in continuous scrolling had a scrollbar
  that ended at the last page visited; it measures every page. A long name in
  the grid wraps on up to three lines again. Going from one PDF to another in
  an archive no longer redraws the first a page at a time while the next one
  opens, nor does turning to a PDF's hex view.
- **Scripts and agents**: a file selected, a folder opened or a sort changed by
  `skwir cmd` or an agent shows in the window at once, not at the next move of
  the mouse. An agent's changes are always reviewed and its private folders
  withheld, whatever its client says, scripts reaching the window by a way of
  their own; no server is reached for an agent unasked.
- **Player**: the rates are said over MPRIS, and a rate set there is taken.
- **About** and `skwir --version` say the commit a build between releases was
  made from (`0.2.0+8ef7a93f6d38`); a release says its version alone.
- **Safer**: a copy skips a file as already there only once it proved both the
  same; a private folder stays private through a link to it; programs run in
  the sandbox get only the environment they need; the archive helper never
  reads the files an archive's mtree names; a page's resource that climbs out
  of its folder by name is refused; a path copied for PowerShell or a shell is
  quoted so that nothing in its name runs (typographic quotes, control
  characters).
- **Builds to try on Windows and macOS**: Windows' as a zip and a per-user
  installer, macOS's as `skwir.app` for Apple silicon in a disk image or a zip,
  for macOS 12 or later. Both open, extract and make archives (libarchive
  linked in) and play audio and video (GStreamer's own build carried). They are
  made on Linux, Windows' tried under Wine, macOS's looked over: neither has
  ever run on its system.
- **Under the hood**: gpui from Zed's main of 2026-10-27, built with Rust
  1.99.1; PDFium chromium/8125 in the packages. tokio, rustls-webpki and
  Wasmtime were updated for security advisories. The packages suggest the
  tools remote folders use (sshfs, rclone, GVfs's backends, kio-fuse, rsync,
  FUSE, Avahi, wsdd).
<!-- The review fixes of 2026-10-28 go above: an item each a user notices. -->

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
