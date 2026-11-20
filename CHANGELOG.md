# Changes

## Unreleased

Skwir 0.6.0 speaks French and Simplified Chinese, as drafts to review, and
writes numbers, dates and the order of names as your region does. On Linux it
can show files and answer Open and Save dialogs for other programs, and a copy
downloaded from skwir.org can tell you of a new version.

[Installation instructions](README.md#installing)

- **French and Simplified Chinese** (#13): the window speaks your system's
  language, these two as drafts not yet reviewed; Settings › General ›
  Language chooses another, and the draft's notice offers English.
- **Your region's ways**: numbers, sizes, dates and times as your locale writes
  them, names in its alphabetical order.
- **Right to left**: Hebrew and Arabic drawn and edited right in names and
  fields, a document's paragraph from its right; printing as shown, a booklet
  bound on the right.
- **Show in Folder from other programs** opens Skwir once it is the default
  file manager (Settings › General).
- **Skwir answers other programs' Open and Save dialogs** (Settings › General).
- **New versions told**: a copy downloaded from skwir.org asks once whether to
  be told; skwir.org's apt and dnf repositories update it with the system.
- **A document or a picture zoomed in moves with a drag**, the middle button's
  anywhere.
- **A copy onto an rclone mount** shows what the server has, no longer 100%
  while the mount still sends.
- **Shortcuts on other keyboard layouts**: Russian, Greek, Arabic or Hebrew,
  AZERTY's digits.
- **A name's hidden direction controls are shown**, so `gnp.js` reversed can no
  longer pass for a picture.

**Changed from 0.5.1**: sizes count a thousand bytes to a kilobyte (kB, MB), as
disks are sold and GNOME and macOS show them, where 0.5.1 counted 1024 and wrote
KB (Settings › Folders and views › Sizes › Units: Binary, KiB and MiB); names
sort by your locale (accents after their letters, then case; a C locale keeps
0.5.1's order); dates and times are your locale's (a US one writes 2:02 PM);
on a French or Simplified Chinese system the window speaks its draft; a double
click in a text field selects a word, a triple click all; Settings › General
shows the default file manager where Ask what dropped files do was, now under
More settings.

**Windows and macOS builds to try** come with this release too. They are made
on Linux and have never run on either system: please [try them and tell us what
happens](docs/HELP-WANTED.md). Skwir itself is for Linux, as in 0.5.1.

### All changes

- **The window in your language** (ADR-139, #13): everything the window says is
  a message, said in the language Settings › General › Language names:
  Automatic takes the system's languages in their order (`LANGUAGE`,
  `LC_MESSAGES`, `LANG` on Linux; the preferred languages on macOS, the
  display languages on Windows), then English. French and Simplified Chinese
  come with this release, every message translated, as drafts a speaker has
  not reviewed yet: Automatic speaks them, the first start in one says so
  once with **Use English** beside it, and About and Settings say it while
  the window speaks one ("Français — draft, 0% reviewed"). Each language is
  named in itself (Français, 简体中文). The desktop entry and the AppStream
  page are in both too, as drafts. Agents, scripts and the command line still
  read English. A translation is welcome: `docs/invariants/words.md` says
  how, and `docs/glossary-fr.md` and `docs/glossary-zh-Hans.md` hold the
  terms.
- **Sizes counted one way, as you choose** (ADR-139): Settings › Folders and
  views › Sizes › Units is Decimal by default, a thousand bytes to a kilobyte
  (kB, MB, GB), as disks are sold and as GNOME and macOS show sizes, or
  Binary, 1024 (KiB, MiB, GiB), as KDE does. 0.5.1 counted files in 1024s
  written KB and MB, volumes in thousands and limits in MiB; every size now
  follows the setting, at once when it changes. A search still reads `10M` as
  1024-based, as saved searches hold it, and now takes `10MB` (a thousand)
  and `10MiB` (1024) too; the size groups and the Size chip write the units
  shown.
- **Numbers, dates and times as your region writes them**: counts, decimals
  and sizes as the system's number locale writes them (1,234; « 1 234 »,
  « 1,5 Mo »), dates and times as its date locale does: a US locale writes
  2:02 PM and Mar 12, 2024, where 0.5.1 wrote 14:02 and 12 Mar 2024 for
  everyone. The desktop's 12- or 24-hour switch outweighs the locale (GNOME's,
  macOS's, Windows' own). Settings › Appearance › Dates' ISO 8601 now reaches
  every date, Git's commits and a printed listing's included, which wrote ISO
  whatever it said. A file's type is named in your language from the type
  database's translations, Open With's applications in your languages
  (`LANGUAGE`'s list read too), and the paper size is your locale's
  (`LC_PAPER` after `LC_ALL`, as POSIX orders them; the region on macOS and
  Windows, which always took A4). Agents read English's formats.
- **Names in your locale's order** (ADR-139): names sort by the system's
  collation (`LC_COLLATE` on Linux, the region on macOS and Windows): the base
  letters first, then the accents, then the case, numbers by value, spaces and
  punctuation before digits and letters, each language's own alphabet
  (Swedish Ä after Z). 0.5.1 folded case and Latin accents and broke ties by
  bytes; a C or POSIX locale keeps that order. Group by Name heads by the same
  letters (é under E, Swedish Ä apart), in other scripts by CLDR's index
  (pinyin for Han, kana rows, Hangul initials). Tags, Open With's
  applications, rclone's remotes and the themes' lists sort the same way,
  where an accented name went after Z.
- **Text in every script** (gpui 0069 to 0084): Hebrew and Arabic names and
  text fields are drawn, wrapped and edited right, the caret, a selection and
  the arrows following the letters drawn. Lines break as Unicode's rules say:
  Chinese and Japanese by theirs, Thai, Lao, Khmer and Burmese between words by
  dictionary; a line may wrap after a hyphen or a slash. Ctrl+arrows,
  Ctrl+Backspace and Ctrl+Delete go a word at a time in every script; a
  double click in a text field selects a word and a triple click all of it,
  where a double click selected all; the settings search finds words typed
  without spaces. Chinese characters take their Japanese forms on a Japanese
  system. Where no installed font draws a script you read, a notice names the
  font to install, and [the guide](docs/guide.md) each system's package. The
  screen reader is told the window's language (Linux's AT-SPI does not pass
  it on yet).
- **Documents read in their own direction**: a Hebrew or Arabic paragraph,
  heading, list item or table cell in a document (Markdown, a web page in
  reading mode, a book, a mail) starts at its right, its marker, quotation
  bar and indent on that side; a page that says it reads right to left (HTML's
  `dir`, a book's page progression) runs its tables from the right. Code
  reads left to right on every line, whatever letter starts it. A click, a
  selection or a link's hover on a centered or right-aligned text now lands
  where its rows are drawn: a centered title page selected the wrong words.
- **Printing right to left** (ADR-111 revised): each block is laid out in its
  own direction, code left to right, a listing as the window runs; the PDF of
  a document that reads right to left says so to its viewer. A booklet can be
  **Bound on the right**, preset from the document: a PDF's or an EPUB's
  declared direction, a manga's `ComicInfo.xml`, else its text's first
  letters. Print… gives a comic its pages and direction from its own menu
  too, as from the preview.
- **A name's hidden controls are shown** (ADR-139): a name holding Unicode's
  direction controls shows each as a tag wherever the name is drawn alone
  (rows, tiles, tabs, the trail, the title, text fields, an archive's members,
  Rename Several's preview, group headings): `Hello_` + U+202E + `gnp.js`
  reads `Hello_⟨RLO⟩gnp.js`, where 0.5.1 drew `Hello_sj.png`, a picture. The
  name's bytes stay as they are.
- **Shortcuts on other keyboard layouts** (gpui 0076): a shortcut on a
  non-Latin layout (Russian, Greek, Arabic, Hebrew, Persian, Devanagari) acts
  as the keymap's Latin layout or the US keys' positions name it, AZERTY's
  and Persian's digit rows give their digits, and the shifted comma and
  period no longer swap. Keys are named in the window's language (Entrée,
  Maj+F4), and a search of the Keyboard page, Settings or F1 for keys finds
  the names shown.
- **Show in Folder for other programs** (ADR-140): on Linux, with Skwir the
  desktop's default for folders (Settings › General › Default file manager,
  Open With's Always use on a folder, or the desktop's own settings), another
  program's Show in Folder opens the folder in Skwir with its files
  selected, or their Properties, even when Skwir is not running: Skwir
  answers `org.freedesktop.FileManager1`, and keeps a file in
  `$XDG_DATA_HOME/dbus-1/services` for the session's bus to start it
  (`skwir --file-manager`, no window until asked). A program that holds the
  name and does not let go (Dolphin's daemon) answers until it ends, which
  the row says. Home Manager's `programs.skwir.defaultFileManager` sets it.
  Choose another default before removing Skwir:
  [the guide](docs/guide.md#skwir-for-other-programs) says why.
- **Other programs' Open and Save dialogs** (ADR-141): on Linux, Settings ›
  General › Open and save dialogs can name Skwir for the programs that ask the
  desktop portal for one. Each dialog is a Skwir window of its own, attached
  to the asking program's, opening where it asks, else where that program
  last chose, with the name, the program's filters and choices, Cancel and
  its answer; only you answer it, never an agent or a script. The row writes
  your portal configuration, applied once the portal starts again (the next
  login), and leaves one a link keeps (Home Manager's, a dotfiles folder's)
  to its keeper. Home Manager's `programs.skwir.fileChooser.enable` sets it.
- **A dialog's window kept to finding and choosing** (ADR-144): it offers
  places, the address, search, tags, views, the preview and Properties, the
  shelf; of what changes files, New Folder, Rename, Move to Trash, the
  clipboard and Paste, a file's tags, rating and comment, and Undo, Redo
  and Retry of what that dialog did alone. Tabs, the split, the terminal,
  the pinned player, Activity, Settings, Usage, Open With, printing, the
  commands of plugins from outside and the Trash place are gone from its
  menus, buttons, palette, F1 and links, their keys doing there what they
  would unbound; a place opened in a tab opens in place; Enter and
  Shift+Enter answer with what they name, never opening another program;
  what the dialog's own action asks (a password, names taken) is asked in
  it; Ctrl+Q cancels the dialog alone. The dialogs keep a layout of their
  own (the first takes your window's view and no panel) and the size you
  leave them at, a grid for a program asking for pictures or films;
  opening files, Recent comes first and is where a program with no folder
  starts while the desktop's list has a file to give, and what you choose
  is added to the desktop's recently used files; Recent folders in the bar
  lists the five folders the program last chose in; the filter changes a
  saved name's extension; choosing a folder, its files show dimmed; a file
  dropped is gone to, never copied; the answer counts ("Open 3 files"). Every
  Skwir window says what it is for, `main window` or `file chooser` (Wayland's
  toplevel tag, X11's window role), so a compositor rule can float a dialog
  where nothing else does ([the guide](docs/guide.md#skwir-for-other-programs)
  gives Hyprland's, sway's, i3's and KDE Plasma's).
- **General's essentials** (ADR-072 amended): the language, Open at startup,
  the startup folder (dimmed unless Open at startup is A chosen folder), the
  default file manager and, on Linux, Open and save dialogs; Ask what dropped
  files do is under More settings.
- **A copy downloaded from skwir.org can tell you of a new version**
  (ADR-142): it asks once, two days after it was first seen, whether to be
  told; said yes, it reads skwir.org at the first look of each day (nothing
  about the copy is sent; on Linux, not on a metered connection, whose cost
  Windows and macOS do not tell it yet) and tells a new version once, About
  and More saying it until you update. **Check for a new version** in the
  palette answers at once; Settings › Privacy › Updates turns the daily
  check on or off. A downloaded `.deb` or `.rpm` also offers **Update with
  your system…**, which opens the repository commands of your system
  (Fedora, or Debian or Ubuntu and the systems like them);
  copies a package manager updates (the AUR's, Nix's, a repository's), and
  one whose settings file cannot be written (one Home Manager keeps: set
  `updates.tell` there), never ask. What's new is said once after each
  update from this version on (Settings › General › Updates turns it off).
  The AppImage carries its update information, for AppImageUpdate and Gear
  Lever, through skwir.org.
- **Skwir's repositories** (ADR-142): Debian 13, Ubuntu 24.04 and Fedora 44
  can install Skwir from skwir.org's apt and dnf repositories, which update
  it with the rest of the system: the
  [download page](https://skwir.org/download#repository) gives the command.
  Their indexes are signed by the key "Skwir repositories" (F6EB 0DC1 7B1A
  3E87 8C48 2DB3 0CF3 A0EA 4800 4329), and so is each `.rpm` from this
  release on: dnf asks once to import the key (0x48004329) for the packages.
  `skwir-repository` 2, which they now bring, keeps rpm's copy of the key
  current on Fedora (an extended key taken, a retired one let go), keeps a
  local edit of its apt source across its updates on Debian and Ubuntu,
  and, added again after it was removed, brings the repository back.
- **Plugins from outside speak your language** (protocol 2, ADR-045, ADR-046
  and ADR-047 amended): a plugin on protocol 2 is told the window's languages
  and may say its words as messages of its own catalogs (`l10n/<language>.ftl`
  beside its manifest), counts, sizes, names, folders, decimals and dates
  written as the window writes them; agents read its English. A plugin on
  protocol 1 runs as before. A plugin that is off is described on the
  Plugins page in your language. The hex view's module writes sizes as the
  size setting says. [The protocol](docs/plugins/protocol.md) has it.
- **A document or a picture zoomed in moves with a drag** (ADR-143): a
  picture larger than the panel, a page without text, a page's margins and
  pictures, and the room around and between pages follow the pointer; the
  middle button's drag moves them anywhere; **Drag to move the page** in the
  reader's bar (`H`, or Settings › Reading › Pages) makes a drag across a
  page's text move it too, rather than select. The pointer shows what a
  press will do as it moves; a press within a line's text (between its
  words, in the space between a paragraph's lines, just before or after
  the line) selects; a click where nothing selects clears the selection,
  whichever page holds it, a drag that moves the pages keeps it, and a link
  is followed only on a click. A flick of a picture then a quick click
  leaves it where it is; a double click still sends it to the whole
  screen.
- **Thumbnails come sooner and keep less**: a picture's thumbnail shows before
  the desktop's cache keeps it; a progressive JPEG's is read from its first
  scans alone, keeping only their DC coefficients (7 MB for a 45-megapixel
  photo, where 0.5.1 held 130 MB); a document's or a drawing's thumbnail
  takes up to half as much room in the cache. A comic's JPEG page or cover
  over 16 megapixels, shown small, is decoded at a reduced scale (a few MB
  rather than the whole picture), and one over 44 megapixels is shown
  rather than refused. A lossless JPEG, whose thumbnail and preview were
  stripes of its first rows, kept so in the desktop's thumbnail cache, has
  none of Skwir's now: its decoder reads none yet.
- **A copy onto an rclone mount says what the server has** (ADR-113
  amended): 0.5.1's strip said 100% and its bar stood full while the mount
  still sent the files, as it counted what went into the mount's cache. The
  bar now draws what was written, lighter, under what the server has; the
  percent, the strip, Activity's header and the desktop's progress follow
  what the server has. Activity's card no longer runs its line under its
  Pause and Cancel: every popup's text wraps.
- **A server kept with a folder stays its own place once connected**: a
  server added with a folder (Add Server's Folder, or Keep as Server on a
  remote folder's address, `sftp://nas/srv/photos`) gave way, once mounted,
  to the mount's own place, at the top of the server, under the mount's
  name. It now keeps its name, its position in the sidebar and whether it
  is hidden, and opens its folder rather than the top of the server, also
  when a lost connection is made again. Two servers of one host stay two
  places; one kept from a phone's or a camera's folder shows beside the
  device. A server at a mount's top is shown as the mount's place, as
  before.
- **What you are on keeps its place more surely as a view changes size**:
  a grid's tile kept at the foot of the view was left cut by its gutter,
  so the next arrow scrolled it again; a list's place given back (a tab
  come back, the top row it keeps as its text grows) was moved again when
  the view changed size in that frame; a sort or files coming in that
  frame could keep another row, or none; on the Board, the column kept
  was put off by as much as the columns widened with the text, only the
  first column holding a file of two tags was tried, and a column showing
  a selected card never.
- **What a copy brings shows with its notes**: a file a copy or a move
  will bring, shown where it lands while the copy waits or runs, could
  stay for good without its tags, rating and dates, read as the copy was
  asked but lost as its run began; a filter or a sort by them left it out
  of place until it landed.
- **A size just under a unit is said in the next one**: 0.5.1 said
  1,048,570 bytes as "1024.0 KB"; rounded before its unit is chosen, it
  reads "1.0 MB" now, or "1.0 MiB" in Binary units.
- **Grouped by Type, a type the system does not name is headed as its
  files are called** ("PNG file", "File"), where its MIME type headed it
  (`image/png`, among the a's).
- **Settings › Keyboard's new shortcut finds its command as the palette
  does**: whatever its accents and the order of its words, in its title or
  its name.
- **On Windows, no program Skwir starts opens a console window** (#12): the
  Git helper's flashed at each Git state read, and so did the other helpers,
  plugins from outside and the `rclone` behind Add Server. Seen to stop for
  the Git helper on Windows, by the pull request's author; the rest is tried
  under Wine, which shows no console.
- **Wasmtime 48.0.10**, past eight advisories of 48.0.8 (RUSTSEC-2026-0566
  to RUSTSEC-2026-0573): it runs plugins from outside built as WebAssembly
  modules (off until you turn one on) and the WebAssembly grammars Skwir
  finds where editors keep theirs.
- **The English, reworded in places**: counts are grouped (1,234), plurals
  agree, a list takes a comma before "and" ("a, b, and c"), a percent is
  "42%" everywhere (Activity and the text sizes wrote "42 %"), the search
  plugin is titled Search, and a command nobody declared shows its name as
  written (`gone.open`, not "Gone open").
- **Smaller fixes**: a folder's tab draws the theme's folder icon, as the
  views do, where only the sidebar's places did; a notice's buttons go under
  its words where both do not fit beside each other; a setting's dropdown
  leaves its description the room the buttons left, where the description
  wrapped in a third of the row; Tab reaches a toast's buttons (Undo, Show,
  Close), and a key meant for one toast's button never presses the button
  of the toast said after it; a state file cut short (a crash, a full
  drive) or edited into something that is no TOML lost, at the next save,
  what the window had kept unchanged (its tabs, its hints): it is written
  again whole; a tile server that gives no tile says why in words (no
  connection, refused, a certificate refused), where the network
  library's English was shown.
- **For packagers**: the Linux packages install
  `share/xdg-desktop-portal/portals/skwir.portal` and
  `share/dbus-1/services/org.freedesktop.impl.portal.desktop.skwir.service`
  (ADR-141), the desktop entry's and the metainfo's translations, merged
  from `tools/package/linux/po` with gettext 0.21 or later, and
  `share/skwir/channel`, which says how the copy came: a package manager
  that repackages a release removes it
  ([how](tools/package/channel/README.md)). The Nix package builds
  against a nixpkgs from November 2026 on, whose Rust hook links the
  system's libraries; on macOS zstd stays bundled.

## 0.5.1 (2026-11-16)

Skwir 0.5.1 plays films on in the AppImage, puts the title bar where you want
it, and has fields say what they take. The player follows the file it plays,
and what changes files is worked out beside the window, which no longer waits
for it.

[Installation instructions](README.md#installing)

- **Films play on in the AppImage and the tarball**, 0.5.0's known issue: no
  longer stopped on one frame after a pause and a seek. Their GStreamer is
  1.28.8 throughout, and they decode video on Intel and AMD graphics.
- **The title bar where you want it** (#11): on GNOME the window has its
  buttons and moves and sizes by its edges; Settings › Appearance › Title bar
  puts it where you like.
- **Fields say what they take**: `?` or `F1` in Rename Several's fields, the
  address, Properties' Access and Settings › Exclusions lists what goes there;
  Rename Several offers what can go on as you type, and Run Command… shows the
  line that will run.
- **The player follows the file it plays**: renamed or moved, it plays on;
  deleted, it ends as at its end; a deletion is no longer refused for a file
  it or a preview holds open.
- **What changes files is worked out beside the window**: Delete, a paste or
  Rename Several on a slow server or 100,000 files no longer hold the window;
  past 0.3 s a notice offers Cancel.
- **What you rename is followed into view**, and what you are on stays where
  it was as the split or a panel opens or closes.
- **The sidebar follows other programs**: a remote added with `rclone config`,
  a place added in Dolphin, an fstab entry, a folder mounted anywhere.
- **Extracting into its folder merges**, as a copy does.
- **The volume is a slider** that opens from the speaker.

**Changed from 0.5.0**: Rename Several starts from the names with their
extensions, its case choices side by side; the next film opens once one has
played to its end, with a short gap; More no longer lists volume steps.

**Windows and macOS builds to try** come with this release too. They are made
on Linux and have never run on either system: please [try them and tell us what
happens](docs/HELP-WANTED.md). Skwir itself is for Linux, as in 0.5.0.

### All changes

- **The title bar where you want it** (#11): on GNOME, whose compositor
  draws no title bar, the window had no buttons and could not be moved
  or sized by its edges. Settings › Appearance › Title bar now chooses:
  Automatic, the desktop's way (its title bar where it draws one, the
  window's buttons in Skwir's top row where it does not, as GNOME's
  applications have them), System, Compact or In the top row. The
  buttons are the desktop's, in its order; the empty parts of the top
  rows move the window, and a double click there maximizes or restores
  it (beside the tabs it opens a tab). A choice that does nothing on this desktop
  is shown dimmed, saying why. Built for macOS and Windows too, not yet
  tried there.
- **A folder of notes files says so**: a folder holding only the files
  other programs keep notes in (TagSpaces' `.ts`, a Mac's `._` files,
  XMP sidecars) said "Only hidden files here", and Show hidden files
  then showed nothing, the folder said empty. It now says "Only
  companion files here", with a button that shows hidden and companion
  files (turning on Show companion files, in Settings › Tags and notes,
  for every folder).
- **The sidebar follows what other programs change**: the Network
  section shows at once a remote added or removed with `rclone config`,
  a network folder or a place added in Dolphin, an entry of fstab
  (NixOS's too, as a switch changes it), and a
  folder mounted or unmounted by any program (`sshfs`, `rclone mount`,
  `mount`), where it waited for a refresh or the next start; the drives
  show a volume mounted anywhere, not only under `/run/media`. A server
  you add to KDE's network folders, or edit or remove there, shows at
  once in a Dolphin showing Network, as one Dolphin adds does.
- **An S3 bucket's name is no longer looked up on the network**: Skwir
  sent it to the name resolvers as a server's to tell whether it was
  near, which waited and held the system's name service meanwhile, for
  every program on the machine.
- **The view follows what you rename**: a file whose new name sorts it
  out of sight comes to the middle of the view, selected, the cursor on
  it; renaming several, the first of them does, unless one is still in
  view, where the view stays. Undo and Redo of a rename follow it back.
  A new name starting with a dot shows hidden files (as for what you
  make), and one the filter leaves out is said, under the Renamed
  message, whose Undo stays. Undo and Redo also select what they put back
  in the folder shown, as what a plan makes is.
- **What you are on stays in view as a view changes size**: opening or
  closing the split, a panel beside or under the files, or the window
  resized kept what was at the top of the view, so the files selected
  could go out of sight (a grid of six columns becoming three). The file
  the cursor is on, or else the first selected one in view, now stays as
  far down the view as it was, whole, in every view, the Board's row of
  columns too; opening the split and closing it again gives the view back
  as it was.
- **A dropdown closes on a second click**: in Settings, clicking an open
  dropdown (or a narrow window's page menu) to close it opened it again.
- **Rename Several's case is in sight**: its five choices stand side by
  side, each written as it leaves a name (Unchanged, lowercase, UPPERCASE,
  Title Case, Sentence case), as the Print dialog's and Settings' choices
  do, where one button went through them unseen. `Tab` reaches it, the
  arrows step it, a screen reader hears it, and `Ctrl+K` still gives the
  next from any field; in a narrow window it is a dropdown.
- **Fields say what they take.** In Rename Several, the `?` inside the
  template, the regex and the replacement, or `F1` in one, lists what it
  takes: each placeholder with what it gives the first file, date
  formats, recipes for common renames (spaces to underscores, what is in
  brackets taken away, two parts swapped) and a pattern's and a
  replacement's signs; a placeholder or a sign chosen is written where
  the cursor is, a recipe fills the regex and the replacement, one
  `Ctrl+Z` taking it back from both. As you
  type `{`, `\` or `$` there, the field lists what can go on from there,
  `Tab` taking the first. The preview tints what the regex matched and what it
  put there. The dialog starts from the names as they are (it started
  from them without their extensions), and `{.ext}` writes an extension
  with its dot, nothing for a file that has none. In the address, `F1`
  or the `?` inside it lists everything a search takes without losing
  what is typed. Properties' Access says, as you type, what a mode comes
  to, refuses one `chmod` would not take before Change, and has its `?`
  too, as do Settings › Exclusions' fields, an example of a whole mode
  or path taking the field's place; Run Command… shows, as you type, the
  very line that will run, the selected paths whole and quoted as your
  shell reads them. What a field takes shows in a list under it, as the
  address's does, the keyboard staying in the field, and a screen reader
  hears it.
- **The player follows the file it plays.** Renamed or moved, by Skwir
  or by another program in a folder a window shows, the track plays on
  under its new name, its mark and the desktop's controls with it.
  Deleted, it ends as if it had played to its end: with Play next or
  Shuffle the next track plays, otherwise the player closes; a track
  further on that another program removed is passed over. Before a
  deletion the player closes the file, as do the document readers, the
  archive and comic previews and the map tiles, so the deletion is never
  refused or left half done for a file in use (Windows, some network
  folders). The preview's player plays on through a rename too.
- **Films play on in the AppImage and the Linux tarball**: after a pause and
  a seek (or peek), a film could stop on one frame for good, as in 0.4.0.
  The PulseAudio client library they carried, Ubuntu 22.04's, did this
  with every sound server; they now carry PulseAudio 17's.
- **The volume control shows what a film is heard at**: the desktop gives a
  player's sound back the volume and mute it last had, and its mixer can
  move them; the control said its own value over them, 100 % over a film
  at 0 %. It now shows the sound's own.
- **Video decoding on Intel and AMD graphics in the AppImage**: it carried
  Ubuntu 22.04's libva, which opens neither a VA-API driver built for a
  later libva, as current ones are, nor one outside Debian's folder; it
  now carries libva 2.24, which looks where each distribution keeps them.
- **The AppImage's GStreamer is of one version**, 1.28.8 throughout, built
  for it: the plugins the player uses, as Windows and macOS have them,
  with Linux's own video decoders (VA-API, NVIDIA's). It no longer carries
  Ubuntu's 1.20 plugins beside it, nor GStreamer's network sources,
  tracker and chiptune music, MIDI, MXF, DV or RealMedia.
- **The volume is a slider**: the speaker in a film's or a song's bar, and
  in the dock, opens it above itself, its level, an upright slider and
  Mute; Up and Down step it, as the wheel over the speaker does, which
  shows the popup while it turns. The speaker's waves say how loud. More
  no longer lists five volume steps, nor the time where the bar shows it.
- **A paused film comes back fitted**: a small film paused in the dock,
  shown again in the panel, was drawn at its own size until it played;
  it fills the panel as before.
- **Extracting into the folder there merges**: an archive extracted again
  where its folder is (a few files removed from it since) was offered only
  a second whole copy, replacing the folder, or skipping. Its folder is
  now merged into as a copy's is: what was removed comes back, what is
  there the same stays, a file changed since is asked about, and Undo
  takes back only what it placed. An archive holding one read-only folder
  is extracted again, which it could not be. The question about names
  taken no longer shows a row of choices for all files when no file is
  asked about, as when only folders are.
- **The player's speed is one row**, as the menus' other settings are: its
  name, then its values, Left and Right walking them. Its headings and
  the slideshow's and the timeline's choices are drawn as every menu's.
- **A model's stored picture fits the panel**: the picture a slicer stores
  in a 3MF, shown before the model is turned in 3D, was as wide as the
  panel, and a tall one ran past its foot. It is shown as pictures are,
  whole in the panel.
- **The questions before a permanent deletion and about names taken show
  each item's icon**, as the views do: its type's from the icon theme, a
  link marked as one (in red where it leads nowhere). They showed a grey
  folder or link glyph, and nothing for a file.
- **The wheel takes a step a notch**, on a touchpad under X11, through
  Windows' wheel messages and with a high-resolution wheel: the tab strip,
  Ctrl+wheel's zoom and the structured previews' section and page turns
  took a step for every part of a notch, a short slide going to the end.
- **The player's volume keys and the desktop's media controls lose no step**
  pressed quickly, and a volume moved unmutes the player, as the desktops'
  own volume controls do.
- **A film played after another starts**: the player stood for good at the
  end of the first when the next had subtitles and they were off, or when
  the first had subtitles and the next none (GStreamer 1.28 waiting on
  itself). A film now plays to its end and the next opens then, with a
  short gap; music still goes on without one. A file gone on with
  gaplessly plays its first audio track, and subtitles only where they
  were shown; it kept the tracks of the file before.
- **Films start at once in the AppImage and the tarball after another
  GStreamer program ran**: their player shared that program's plugin
  registry, and each scanned its plugins again after the other.
- **A field full to its 32,767 characters** no longer fills Undo with keys
  that changed nothing; a completion, or the text an agent or a script
  gives the address or a search, is held to that bound as a paste is.
- **On Windows, Skwir asks rclone's remote control only where rclone says
  it serves**: the port was chosen before rclone started, and a program
  taking it meanwhile would have been given the configuration's passwords.
- **The question about names taken stays quick with thousands of them**: it
  read every name taken again for each row it drew.
- **A large folder renamed beside folders opened in place** no longer costs
  their count times its size on the window's thread.
- **Run Command… on a folder's background runs** (Open Folder With): with
  nothing selected it asked for a selection and never ran; it runs in the
  folder, with no paths after the command.
- **A command that names its arguments is given them once**: `cp "$@"
  /backup`, `"$1"` or fish's `$argv` were given the paths a second time
  after the command; they now go only where it names them.
- **Run Command… is no longer offered on macOS and Windows**, where it
  could not start a terminal; asked by an agent or a script, it says it
  is not available there yet.
- **Rename Several** gives a folder no extension (`v1.2` stays whole under
  `{name}` and a case) and keeps the dot a name ends with; a replacement
  that is a space, or has spaces around it, is taken as typed, no longer
  trimmed to nothing; a counter wider than a name can be (`{n:256}`) is
  refused rather than built for every file; Rename waits while no name
  changes; and its preview of many files no longer holds the window back
  at each key.
- **What changes files is worked out beside the window**: Delete, a
  paste, Rename Several's Rename and every command that changes files
  were worked out on the window's thread, which stood still while a
  plan listed a folder or waited on a server, or named 100,000 files
  anew. They are now worked out aside, on what was selected when you
  asked; past 0.3 s a notice says so, with Cancel (or `Escape`). Moving
  and selecting go on meanwhile, what changes files waits its turn, and
  closing the window never waits.
- **Rename Several's names made one** are said by the first three and
  how many more, where the error listed every one of them.
- **What a paste or a new folder selects** is never in another tab's
  pane, nor selected once its wait is over; the note that hidden files
  were turned on for it joins the paste's message, whose Undo stays,
  where it replaced it. The player, fullscreen and the preview no longer
  move the cursor of a pane in a tab not shown.
- **Undo and Redo answer in the window they were asked from**, where
  their message went to the window in front.
- **The address card's row reached by the arrows shows whole**: Up from
  its first row brought the last into view half under the card's foot.

## 0.5.0 (2026-11-14)

Skwir 0.5.0 groups a folder's files under headings, marks them with their Git
state and shows their history, and shows what a copy brings where it will land.
A folder can keep its own order and view and show its README, formulas are
typeset, and Activity is one floating panel.

[Installation instructions](README.md#installing)

- **Groups** (View › Group by): by kind, type, date, size, rating, tags,
  owner, folder, or a picture's date taken, its dimensions or a film's length,
  each under a heading that selects, folds and filters; the Board gives each
  group its column.
- **Git**: every view marks what a file is to its repository (changed, staged,
  new, in conflict), a folder what it holds below; History lists a file's or a
  folder's commits, its renames followed, each opening as its patch.
- **What a copy brings shows where it will land**, dimmed and tinted to how
  far it came, in every view; a file another program is writing says so.
- **A folder remembers its order and view** when asked (View › Remember for
  this folder).
- **Folders first is a setting** (#10): off, folders sort with the files.
- **Every sort is a column**: Created, Accessed, Taken, Dimensions, Duration,
  Owner and Kind, in the list and under the grid's names.
- **Formulas typeset** in notebooks, Markdown, web pages and books, by an
  optional helper; a notebook's tables and HTML results show as Jupyter shows
  them.
- **A folder's preview shows its README or its cover.**
- **Play from the files' menu**: music to the pinned player, a film in the
  quick preview; what plays is marked in every view.
- **Narrow windows**: Settings lists its pages in a menu, and texts wrap rather
  than cut a word.

**Changed from 0.4.0**: Activity is one floating panel that opens by itself
for a long action (Ctrl+Shift+J), its tray and pin gone; Sort by and Group by
are one row each in the View menu, their direction in the submenu; Source, 3D,
Full preview and View as hex stand at the preview bar's end, beside More; a
first start shows the grid; Git marks and a folder's README are on by default.

**Windows and macOS builds to try** come with this release too. They are made
on Linux and have never run on either system: please [try them and tell us what
happens](docs/HELP-WANTED.md). Skwir itself is for Linux, as in 0.4.0.

### All changes

- **Groups** (ADR-130, ADR-131): View › Group by puts a pane's files under
  headings by any criterion the sort takes (Name, Kind, Type, Extension,
  Size, Modified, Created, Accessed, Deleted, Rating, Tags, Owner, Folder,
  Taken, Dimensions, Duration, Git), dates by your calendar (Today, Yesterday,
  … then a year each), or by the sort itself, each with a direction of its
  own. A heading counts its group and stays pinned at the top while you
  scroll, in the list, the tree and the grid. A click on it selects its
  group; its menu selects it, shows only its files, folds it or the others;
  the keys stop on it, Enter selects, Left and Right fold and unfold. The
  Board gives each group its column, a dozen at most, the last holding the
  groups left. A screen reader hears each heading. Each pane comes back
  sorted and grouped as it was left.
- **Sorting a large folder by what it reads stays smooth**: an order that
  needs every file's dates, sizes or types is made once they are read, on a
  worker and every core, the order before shown meanwhile, and the footer
  says "Reading… N of M". Grouping /nix/store's 336,000 files by Kind took
  0.3 s where it took 1.3 s, and the type of a long name is read twice as
  fast.
- **Sort by and Group by are one row each** in the View menu, saying how they
  are ("Modified, newest"), their direction first in the submenu. Folders
  first stands with them, then what is listed, apart from what is drawn
  with the files.
- **Every sort is a column** (ADR-130): Created, Accessed, Taken, Dimensions,
  Duration, Owner and Kind can be shown in the list and under the grid's
  names (View › Under names), each where it applies. Taken, Dimensions and
  Duration are read from the files on demand: a picture's header, a film's
  movie header, else `ffprobe`. The Type column's header sorts.
- **Folders first is a setting** (#10, Settings › Folders and views ›
  Sorting, and View › Folders first): off, folders sort with the files, so
  the newest item comes first by date. A folder has no size of its own:
  by size it sorts below every file.
- **A folder remembers its order and view** when asked (ADR-132, View ›
  Remember for this folder): every pane that shows it shows it so, sorted,
  grouped and in the list or the grid, the pane's own coming back elsewhere.
  It follows the folder through Skwir's renames and moves. Each column of
  the Columns view is in its own folder's order.
- **Git state** (ADR-134, Settings › Folders and views › Git, on by default):
  every view marks a file in a repository on its picture's corner, each state
  its own shape: a sign for a conflict, a dot for a change, a check for what
  is staged, a plus for what is new; a folder holds the strongest state below
  it, what is ignored is unmarked. It is read as `git status` says it, by
  Skwir's own helper, which runs and writes nothing, and changes made
  elsewhere (a commit, `git add`, a checkout) show on their own. A Git column,
  a caption and groups (In conflict, Changed, Staged, New, Unchanged,
  Ignored) sort and show it; a screen reader hears it with the name.
- **History** (ADR-133): a file's or a folder's commits, in the preview's
  History, the branches and tags at each, the top folder's drawn as a graph
  of its merges. A file's follows its renames, as `git log --follow`. A
  commit opens as its patch below the list, colored as a diff; its menu
  copies its hash or its summary.
- **What a copy brings shows where it will land** (ADR-135): a copy, a move to
  another drive, an extraction, a restore, an undo or a redo shows what it
  brings in the folder shown before it lands, where it will be in the pane's
  order and groups, dimmed, tinted to how far it came, with "42 %",
  "waiting" or "paused". Pointed at, it says where it comes from and how long
  is left; pressed, it opens its card in Activity. It shows in every view,
  and a group no file is in yet gets a dimmed heading of its own. Copies by
  rsync, rclone, `sftp`, `gio` and `kioclient` are followed file by file
  where the system tells it.
- **A file another program is writing says so** with a "writing" pill, until
  it is closed (or quiet for 2 s on macOS and Windows).
- **A folder's preview shows its README or its cover** (Settings › Folders and
  views › Previews, on by default): the first of a list of names it holds
  (`README.md`, `README`, `cover.*`, `folder.*` and others), which you can
  edit; a file nothing previews is passed over for the next.
- **Play from the files' menu**: any number of tracks go to the pinned player
  in the pane's order, a film opens in the quick preview and plays there.
  What plays wears a badge on its picture in every view, a click on it
  pausing or playing on; a screen reader hears "playing" or "paused".
- **Formulas are typeset** (ADR-129) in notebooks, Markdown (`$`, `$$`,
  GitHub's math blocks), web pages that load MathJax or KaTeX, MathML in books
  and pages, and footnotes, by `skwir-math-worker` (KaTeX's TeX and fonts,
  through RaTeX), optional as the Mermaid helper is: Linux's packages ship it
  as `skwir-math`, Nix's under `withMath`, Windows' and macOS's carry it.
  Without it a formula stays the TeX it was written in. Equations are
  numbered by their `\tag` alone, as Jupyter has it; a tall formula grows
  its own line only; copied or read aloud, a formula is its TeX.
- **Notebooks show as Jupyter shows them**: each cell read on its own, a
  pandas table as a table, HTML results and pictures pasted into a cell, a
  result's formulas typeset as JupyterLab sets them.
- **Tables size their columns by their content** and a wide one scrolls
  sideways, rather than every column taking an equal share and its words
  broken anywhere. Quotations, lists and paragraphs are spaced as a page has
  them.
- **Contents is in every document's bar** (Markdown, HTML, notebooks, word
  processors' documents, books, messages), disabled where there is nothing to
  list.
- **The preview's bar keeps its tools in place**: Source, 3D, Full preview
  and View as hex stand at its end, beside More, each lit while on, so a
  tool clicked stays under the pointer, and the bar changes once a click,
  as its answer comes. More has Open With and its submenu, as the file menu
  has; View as hex is last there.
- **A film fills its view**: Fit enlarges a small film as players do, where a
  picture keeps its pixels; a film has no Fit width.
- **Activity is one floating panel** (ADR-114 amended) above the footer's
  strip: it opens by itself for an action that lasts, stays open while you
  work elsewhere, and goes once the work has nothing more to show. Its
  chevron folds it until the next long action (Settings › General › Actions
  › Open Activity by itself turns that off); Ctrl+Shift+J gives it the
  keyboard, Escape gives the keyboard back. Earlier is folded at its foot
  with its count. The tray and its pin are gone.
- **Narrow windows** (ADR-010 amended): where its pages no longer fit beside
  it, Settings lists them in a menu at its title, its search under it, and
  a setting's control goes under its words rather than cut them. Everywhere,
  a text beside an icon or a button wraps at its widest word rather than run
  past its card (vendored gpui 0061); tooltips, drags, the palette and menus
  keep their lines whole.
- **Pickers scroll** under a scrollbar, by the wheel too: the palette,
  Choose, the tag editor and a form's choices. A palette row stays one line.
- **The Plugins page has a filter** and lists plugins by name; the Exclusions
  page lists its categories, groups and rows by name (ADR-125 amended).
- **Entering a folder no longer flashes empty**: the folder left stays drawn,
  its pictures too, until the one gone to is whole, 150 ms at most. A folder
  sorted or grouped by dates or sizes appears in its groups at once.
- **A first start shows the grid**, as most file managers do; a window still
  comes back in the view it was left in.
- **A file hidden in a way its name does not say** (Windows' attribute, a
  Mac's flag, a hidden folder a tag's place found it in) wears a crossed eye
  on its picture, never a fade, which says cut. A screen reader hears "cut"
  and "hidden".
- **One line, one item, one folder** are counted in the singular.
- **A remounted GVfs server opens at once**, once its folder answers, rather
  than failing for a second or so after it was ejected and connected to
  again; rclone's servers are waited for however long rclone takes to list
  them.
- **A one-line field takes 32,767 characters at most** (the address, the
  palette, a rename, a prompt, as Qt's fields hold): the paths of 20,000
  copied files pasted into the address held the window while their 600 KB
  were laid out.
- **Settings finds what you type word by word**, whatever its case or
  accents ("editeur" finds "éditeur"), in its search and in every page's
  filter alike, and finds a plugin by where it comes from too.
- **A player opening a track or a playlist on a server that stopped
  answering no longer holds up other folders' work**: it waits in that
  server's lane alone.
- **Without a session runtime folder** (no `XDG_RUNTIME_DIR`), Skwir keeps
  its sockets and mount points in a folder of your own in the temporary
  folder, made private as the runtime folder is, instead of going without.
- **A long text no longer moves sideways after it shows**: its line numbers
  have room for every line from the first frame, where 0.4.0 widened them a
  moment later for a file of 100,000 lines or more. Its scrollbar is about
  the right size from the first frame too.
- **A large download cut short by its server goes on from what is sure**:
  read from an SFTP, GVfs or rclone mount several parts at once, then
  retried once the server answered, it could keep a few kilobytes of zeroes
  the mount gave as its server went. It now goes on from before the last
  32 MiB it was reading, checked against the server.
- **A film's picture comes back after its subtitles are turned off**: now
  and then, turning a film's subtitles off or choosing another of its audio
  tracks left its picture still for good, as GStreamer had already chosen
  the tracks itself. The tracks are now asked for before it chooses.

## 0.4.0 (2026-11-10)

Skwir 0.4.0 opens several windows, draws the window as cards, and keeps what
tools manage and what holds your secrets out of searches and away from agents.
A filter now shows only the folders that lead to a match, a search can show as
a tree, folders appear whole, and Skwir takes less memory at rest.

[Installation instructions](README.md#installing)

- **Several windows** (Ctrl+N): one Skwir, one queue, one History, each window
  with its own tabs and look; Skwir started again opens a window in the one
  running.
- **Cards**, the new look: the panes, the preview and the terminal on the
  window's frame (Settings › Appearance › More settings › Cards off for lines).
- **Exclusions** (#6): Settings › Exclusions lists about 280 kinds of folders
  tools fill and about 245 kinds of secrets, each kept from searches, from
  agents or both.
- **Filters look below**: a filter by tag, type, date, size or rating shows only
  the folders that lead to a match, and in the Tree view a search shows as a
  tree.
- **Folders appear whole**, read and sorted away from the window, which never
  stalls.
- **See-through backgrounds** for the bars, the files and the preview, from 50
  to 100 %.
- **Save as in one line** in Print…, its folders completed as you type, and
  Print Listing… for a search, a tag or the Trash.
- **Activity unfolds** above the footer for an action still going after 1.5 s.
- **The Nix package in variants** (#7), about 1 GB rather than 3.
- **XML and CSV** (#8, #9): XML under other names colored as XML, CSV separated
  by semicolons, tabs or pipes shown as columns.

**Changed from 0.3.1**: the window is drawn as cards; Preview (F11) and
Properties (Alt+Enter, Ctrl+I) are two switches; Skwir started again opens in
the one running (`skwir --new-instance` starts one of its own); Quick filters
and the timeline are each pane's, in the View menu; the private folders of
0.3.1 are read into Exclusions.

**Windows and macOS builds to try** come with this release too. They are made
on Linux and have never run on either system: please [try them and tell us what
happens](docs/HELP-WANTED.md). Skwir itself is for Linux, as in 0.3.1.

### All changes

- **A text preview opens with its colors**: it no longer flashes uncolored
  first, as long as its colors take under about a tenth of a second.
- **Agents and scripts listen whatever ran first**: once Nearby, an rsync copy
  or a Skwir finishing without its window had made Skwir's runtime folder,
  Agents and scripts refused to listen there ("the channel's folder is not the
  user's alone") until the session ended. Each now makes it the user's alone,
  and one made before is closed again.
- **Keys act in the window you are typing in**: after a script or an agent
  moved Skwir's idea of the active window, a paste or a Delete could land in
  another window. The same goes for a second launch at the same moment, which
  now hands over instead of starting a second program, and for a question held
  by a window that closes, which moves to the next window.
- **Delete no longer takes folders shown only as the way to a search's hits**:
  Invert, the rubber band and a sweep leave them out as Select All did.
- **Renaming a folder under a filter keeps it shown and selected**, F5 asks
  again about every folder, and a change now re-checks only the folders above
  it, so a run of small changes no longer leaves folders hidden.
- **Exclusions keep what the list cannot show** through a change, say when an
  entry cannot be read, and ask the login shell apart from the terminal, so a
  shell's start-up can no longer stop Skwir.
- **Names break only between whole characters**, Devanagari and combining marks
  included, in the grid and under a drag.
- **Print and Save follow one reading of a typed path**: `~` then Enter saves
  in your home folder as the name shown, a folder that did not answer is asked
  again, and one you may not list is no longer called missing.
- **Cards draw with less work**: their outline no longer opens a costly pass in
  each frame, and it stays sharp at fractional scales.
- **Properties no longer blanks while a file grows**, and a retry that finds its
  work already done says so.
- **Less memory at rest on Linux**, about 30 MiB: the code a start runs is
  now laid out together in the program, so the rest of it is no longer
  loaded along with it.
- **Skwir starts lighter where rclone is installed**: it no longer runs rclone
  twice at each start to list your remotes, while its configuration is
  unchanged. An encrypted configuration is still asked each time, its names
  never written down, and F5 in the Network section asks again.
- **A search can show as a tree**: in the Tree view, what `Ctrl+F` finds is laid
  out under its folders, which fold; those only on the way are dimmed, and are
  left out of Select All. The List view stays flat.
- **A filter by tag, type, date, size or rating now shows only the folders that
  lead to a match**: they appear as they are found, dimmed when they do not
  match themselves, and the footer says when it is still looking. A folder
  Skwir cannot look into stays. Names and patterns still look in the folder
  shown alone.
- **The address field's text no longer jumps when it is focused**: it stays
  where it first appears as the suggestions arrive. Text fields in dialogs and
  popups now sit centred in their selection and caret, and text set to be
  centred or right-aligned there is drawn so.
- **XML under another name is colored as XML** (#8): a `.ofx` that declares
  itself XML, a dialect such as `.xliff`, `.xsl`, `.atom` or `.rdf`, or a file
  with no extension. A name a grammar knows keeps its colors.
- **Code colors follow a file's type where its name says nothing**: a script
  with no extension by its `#!` line, and JSON, Python or Lisp files under names
  of their own, such as `.geojson`, `.har` or `.wsgi`.
- **Printing goes by what a file holds where its name says nothing**, as its
  preview does: a PDF with no extension prints as a PDF, and code prints in
  its colors, a selection included.
- **Files in UTF-16 or with a byte order mark** are recognized by what they
  declare: XML, and scripts by their `#!` line.
- **CSV files separated by semicolons** (#9), tabs or pipes show as their
  columns: the separator is read from the file, a `sep=` first line included,
  and the preview's details name it.
- **CSV files on Windows with Excel installed** show as tables: the system
  calls them Excel workbooks, and the preview now goes by what the file holds
  when the two disagree.
- **Exclusions, one list of what Skwir treats apart** (#6): Settings ›
  Exclusions lists the folders tools fill with what they manage themselves, by kind
  (version control, project environments, build output, toolchains and
  packages by language, editors, containers, games, AI models and agents,
  caches, the system's), about 280 on Linux from research across
  ecosystems, with what each holds, and about 245 secrets kept from agents:
  keys and key files wherever they are, tools' sign-ins, browsers' and mail
  clients' profiles, password managers, wallets, messaging apps, shell
  histories. Each kind is kept
  from searches (and tag counts and the timeline), from agents, or both, by a
  switch; each folder by a tick; your own folders go in Skwir's kinds or
  yours, as a name (`node_modules`), a path, a pattern (`*.log`,
  `cmake-build-*`, `~/Projects/*/out`, `regex:…`) of files, folders or both, a name only beside the file of the tool
  that makes it (`target` beside `Cargo.toml`), or a folder holding a marker
  (`CACHEDIR.TAG`, `.private`). What you change is kept as what differs, so
  what a later version adds reaches you; the private folders of 0.3.1 are
  read among them. Only you can make a private folder readable: an agent
  asking is refused. A path may start at a variable (`$CARGO_HOME/registry`,
  `${CARGO_HOME:-~/.cargo}/registry`), and Skwir's follow the variables their
  tools read, those your login shell exports included; keyrings, password
  stores and browser profiles are found by what they hold, wherever they
  are.
- **Hidden files in tag places** (#6): a tagged `~/.config` is listed in its
  tag's place; Show hidden files decides what shows, a file inside a hidden
  folder too, as a search without `hidden:true` finds it. A place or a folder
  with only hidden files says so, with Show hidden files.
- **Searches follow Show hidden files**: with hidden files shown, a search
  finds them too, and runs again as you turn them on or off; `hidden:true`
  always finds and shows them, `hidden:false` never does.
- **Searches say what they walked past**: "· 3 excluded" in the
  footer; the where pill's **Excluded files and folders** (`excluded:true`) searches
  them too, as a search of such a folder itself does. A trash is never
  walked.
- **The Nix package in variants** (#7): the closure is about 1 GB, not 3 GB
  (the Rust toolchain no longer rides along); `minimal` (270 MiB) carries no
  optional helper, `full` every reader; `.override { withMedia = false; }`
  and the other flags pick what you need; PDFs open through nixpkgs' PDFium;
  the helpers are off your PATH.
- **Cards, the new look** (on by default; Settings › Appearance › More settings › Cards off
  gives the window of earlier versions, apart by lines): the panes,
  the preview's parts and the terminal are drawn as cards on the window's frame, a
  shade under the files (in a light look the desktop's own window color, the
  cards' bars between the two), each card outlined by a pixel, apart by a 6 px
  gutter rather than by lines; the sidebar and the tabs lie on the frame. The cards run to the
  window's edges, the top excepted, where the frame's own row is, square where
  they meet them and rounded where they stand apart; on a tiled window the top
  gutter goes too. Split, Properties and Preview, and the sidebar's toggle and
  More while it is put away, stay on the frame: the pane's row ends before
  them with a rounded corner, curving down to the files, and the preview's
  card starts under them. While the window is split, the active pane's row is
  underlined in the accent and the other pane steps down a shade. The gutter
  between two cards is the edge to drag, lit under the pointer. With the window
  see-through, the frame lets the desktop through as the bars do, each ground
  painted once.
- **The colors of Hyprland's desktop shells**: Automatic colors now wear the GTK
  style sheet laid over your theme (`~/.config/gtk-3.0/gtk.css`, else GTK 4's),
  where Noctalia, Caelestia and Dux Shell write their colors, and follow it as
  they write it again (a new wallpaper); the color schemes of qtengine and
  hyprqt6engine beside qt6ct's; and, on Omarchy, its theme. Where the sheet names
  a sidebar color of its own (Noctalia's, adw-gtk3's), the cards lie on it.
- **Selected text reads on a dark selection**: where the desktop's selection is
  dark in a light look (Noctalia's), the text selected in a field, a preview,
  the code view, an inspection's table or bytes and the terminal is written in
  the selection's own text color, as Dolphin writes it, instead of staying dark
  on it; a line pointed at and a drop's target are a fainter band there, so
  their text still reads. A button that is on, under the pointer, turns its
  icon back with its words, instead of leaving a white icon on the hover's
  light ground; a menu button's chevron (View's) takes its icon's color while
  its menu is open; the Tags dialog's check box on the highlighted row is
  drawn in the selection's colors instead of the accent, which may be the
  selection's own. Whatever is chosen draws all it holds in the selection's
  text color, whichever way it came to be chosen: the Undo history's chevron
  while it is open, a menu's row while its submenu is open, a popup's row the
  keyboard is on, Activity's pin, a shortcut being edited, a board's card, the
  sidebar's eject under the pointer, the cursor's ring on a selected file, the
  timeline's bars and counts in the range chosen, and the icons of a desktop's
  color scheme on a selected row; and under the pointer, a search's pill
  turns its × dark with its words instead of leaving it white on the hover's
  light ground.
- **See-through fields**: with the window's background let through, the address
  field, the view buttons and the tab shown let it through as their bar does,
  instead of staying solid.
- **Rounded corners keep what they hold**: a folder's pictures in the grid, the
  board's pictures, the dock's cover and the Settings dialog's content stop at
  their rounded corners instead of showing square ones past them.
- **The terminal is written at the code's size**: the desktop's monospace size
  at 100 %, as the code view and the desktop's own terminal write it, and Code
  text's percentage of it. It was drawn about 7 % smaller.
- **The line between split panes snaps to the middle**: dragged within 12 px
  of it, it goes there and stays until the pointer moves on, so the panes are
  even again without a double click. The line between Properties and the
  preview, and between an archive's view and the member it shows, does the
  same where it started.
- **Print Listing… anywhere**: with nothing selected, a search, a tag, the
  Trash, Recent or the shelf prints its listing as a folder does, as the pane
  shows it, filtered or not, titled by its name and what filters it. Where the
  files come from several folders, each one's folder is printed beside it; in
  the Trash, when each was deleted. A tag listed says how many files it holds.
- **Save as, in one line**: choosing PDF in Print…'s dialog shows where it
  goes, folder and name together (`~/Documents/notes.pdf`): beside the file (or
  in the folder listed, a search's folder, Documents for the Trash), the name
  but `.pdf` selected so typing renames it. Folders are completed as you type,
  as in the address field, in a list under it that scrolls and folds once you
  leave it (Tab fills in or takes a folder, Down goes into the list, Alt+Up
  goes to the folder above, Ctrl+Z takes a folder chosen back), keeping the
  name; the other pane's folder, the folders chosen before (kept from one run
  to the next) and the sidebar's places and bookmarks are a click or Down
  away, and a press elsewhere in the dialog folds the list. Choosing PDF with a
  click puts the keyboard in Save as, its name selected to be typed over. A
  folder left without a name gets it back. The name follows the
  layout until you type one. The dialog says when the folder already holds that
  name, and holds Save back, saying why, while the folder is none, relative, or
  the name one no file can have. The folder chosen is kept for the next PDF.
- **Show**: the notice of a PDF saved, an archive made or anything else made
  in a folder no pane shows says where it went and offers Show, which goes
  there with it selected.
- **A dialog's field that offers choices** has its button inside it, at its
  end (up while its list shows), as a combobox does; Save as's list has a
  scroll bar, and a path it cannot save at reddens the field and its note.
- **A choice clicked in a dialog that shows a field to type in** (Pages…'s
  pages) puts the keyboard there; a press elsewhere in a dialog folds the list
  of a choice unfolded.
- **A field of a dialog that offers choices and takes any text** lists every
  choice when unfolded, rather than only the one it holds; what is typed in it
  still narrows the list, and Enter takes what was typed rather than the first
  choice matching it (`~/` saved in the folder first offered, not the home
  folder): Down goes into the list.
- **About**'s buttons stay on one line, the dialog as wide as they need: in
  0.3.1 Licences went to a line of its own. Licences now comes after Copy
  details, before Website and Source code, which open the browser.
- **About and the shortcuts overview stay open when clicked inside** while
  a text is previewed: a click in either closed it, and About's ✕ closed it
  only to open it again.
- **Settings, the shortcuts overview and About close a menu left open** as
  they appear: drawn over them, it stayed over Settings, and a click inside
  the overview or About closed them instead.
- **A folder's preview has no toolbar**: it said how many items the folder
  holds, which Properties says above it, and its More held nothing the pane's
  menu lacks but View as hex, which did nothing. View as hex is no longer
  offered for a pipe, a socket, a device or a link that leads nowhere either.
- **Preview and Properties are two switches**: F11 and the Preview button show
  or hide the preview, Alt+Enter, Ctrl+I and a new Properties button beside it
  show or hide Properties, and the side panel is there while either is on.
  Each press undoes the one before, on a file, a folder or a pipe, from a
  closed panel or an open one: Alt+Enter on a folder used to open the panel
  and never close it. Properties can fill the panel alone, with no preview
  read. With Preview alone, a folder shows its icon and how many items it
  holds; Properties shows only when asked. The card above the panel is no
  longer a button.
- **Properties no longer jump as they fill in**: rows read a moment later (On
  disk, Opens with, the checksums, the drive) are there at once, labelled, and
  their values fill in without moving anything. General ends with what is
  shown measured in its own unit (a folder's Contains, a picture's Dimensions,
  a document's Pages, a track's Duration); the rest of Content is the last
  group. What is beside a file (its sidecars, whatever their form: `x.jpg.xmp`,
  Adobe's `x.xmp`, a Mac's `._x`, TagSpaces' `.ts`) is read with the rest of
  its metadata, and the card keeps its icon's place while the icon is read.
  Copying a folder of photos with Adobe's `x.xmp` sidecars no longer reads the
  folder once for each photo.
- **A search shows its files' tags**: a file a search found had its size and
  date but never its tags, rating or what is beside it, in its row or in
  Properties. Its rows are now read as a folder's are, as they come into view;
  a sort by rating waits for what it sorts by, a sort by size does not. A
  file whose name several types claim gets its Opens with with the rest, and
  with Properties alone at all.
- **Several windows**: Ctrl+N (or More › New window) opens a window on the
  folder shown, looking as the window it came from; a folder's and a place's
  menus have Open in New Window, a tab's Move to New Window. The windows are one
  Skwir: one queue, so two copies to one disk still take turns; one History, so
  Ctrl+Z in any window undoes the last thing done anywhere; one Activity; one
  plays at a time (a play started in a window pauses another's where it is, to
  go on from there when played again; closing a window stops what it plays);
  one Settings dialog. Each window comes back where it was and as big,
  maximized if it was (where it was only where the system lets a program
  say: not on Wayland, whose compositor places windows), and a new window
  takes the size of the one it comes from, a step from it. Each window has its own tabs, split, panels and
  look; what an action did is shown, and its toast said, in the window that
  asked. Ctrl+Shift+W closes a window, as Ctrl+W does on its last tab (the
  last window too, as its close button does); Ctrl+Shift+T brings it back with its tabs. With At startup
  set to come back, every window comes back as it was, remembered as it
  changes: Quit keeps them all, a window closed alone is forgotten. Agents and
  scripts reach every window (`window.new`, `window.close`, `window.select`,
  `window.list`, `window.open`; `tab.move` to a window or a new one).
- **A window last used with the keyboard takes a drop**: files dragged onto it
  from another application landed on nothing.
- **The folder you came from is selected wherever you go up**: a place of the
  trail, two levels up or more, the sidebar's, the columns view's, as Alt+Up
  already did; a hidden folder left no longer turns hidden files on.
- **The Undo history shows why an Undo waits, whole**: the end of the line
  under an action held by a later one ("first: it touched what this did") was
  hidden under a scrollbar once it wrapped, and a long name ran past the
  History's edge; it now wraps and the History is as tall as it. The address
  card's rows of buttons (Add a term, Search in) are no longer cut when they
  wrap either.
- **A narrow pane's row fits**: in a narrow pane the address field gives up
  its Search, Bookmark and Tags buttons (Ctrl+F, Ctrl+D and Alt+G remain)
  rather than drawing the View button over the pane's corner, and the view
  buttons show wherever they fit, however long the folder's path.
- **Up beside Back and Forward, and the view buttons put away**: the view
  buttons' menu (their right-click, or View › Buttons…) has a switch in its
  View buttons heading; off, one button stands for them, the view shown, and
  opens the View menu, as in a narrow pane, its right-click giving the menu
  back. Under
  Navigation buttons, Up adds an arrow after Back and Forward that goes to the
  folder above (or the place before in a tag's trail), as Alt+Up does,
  dimmed at the top. Both are also in Settings › General › Address; Up is
  off and the view buttons on until you choose otherwise.
- **What was already done is said so**: extracting an archive again whose
  folder holds what it lists, pasting a copy where the same one is, moving a
  file into the folder it is in, trashing a file already gone, tagging a file
  with a tag it has, or undoing what you had put back by hand no longer says
  it did it: the notice says “photos.zip” was already extracted to Downloads,
  with Show and no Undo. An action that did part of it says both: "Copied 3
  items to Docs; 2 were already there" (it said "Copied 5 items", or "Copied
  0 items" when all were). A folder that differs from what it would make is
  asked about as a name taken, as before.
- **Activity unfolds for a long action**: an action you asked that is still
  going after 1.5 s (a slider among General's More settings, from 0.5 to 5 s)
  opens Activity above the footer of its window, with its bar, speed, time
  left, Pause and Cancel. It takes room rather than covering the files and
  leaves the keyboard where it is; quicker actions, and those of other
  windows, stay in the footer's strip. Its chevron folds it into the strip for
  that window, remembered; the pin of Activity opened from the strip unfolds it
  again. Every window, pinned before or not, starts unfolded.
- **The sidebar's free space stays current**: a drive's "free of" and its bar
  were measured at start and when drives came and went, so a Skwir open for
  hours showed the morning's numbers, even after copying gigabytes onto the
  drive. They are now measured again when a window comes back to the front,
  when an action ends, and every minute while a window has the keyboard; a
  server's mount when the window comes back and when an action wrote there.
  Asking does not wake a sleeping drive.
- **Skwir started again opens in the one running**: a launcher, a browser's
  Show in Folder or `skwir ~/Documents` opens a window there, on the folder
  asked, rather than a second Skwir that knows nothing of the first; with
  General › Open folders from other programs in set to A tab, it opens as a tab
  of the window used last, and Skwir started with no folder brings that window
  forward. Only a Skwir of the same build, with the same settings, on the same
  display takes a launch; `skwir --new-instance` starts one of its own.
- **Quick filters and the timeline are each pane's**: both are in the View
  menu, side by side, and act on the pane it hangs from, so the panes of a
  split each have their own, and the timeline stays under its pane rather than
  following the active one. A split or a tab takes them from the pane it comes
  from; the window remembers them as it remembers the view. The timeline left
  More, and Quick filters left Settings (on there, they come on in the panes).
  A timeline bar of a folder's own files now filters the folder as Quick
  filters' year does, the filter kept as you go until taken off with its ×,
  and its bars count the folder's files alone; over its folders inside, or by
  when pictures were taken, a bar still opens a search.
- **What plays stays in sight**: a track kept playing goes under Properties
  when Preview is turned off, where it used to vanish while still playing, and
  to the footer's strip when both are off. A click on it there, or on the
  strip's title, turns Preview on and shows the track as it plays; a film's
  Fullscreen goes there too, Preview off or on.
- **The player's seek bar** has a knob, and a drag that leaves the bar still
  moves it: let go anywhere, it seeks there.
- **Sliders in Settings**: a setting judged as it changes can be a slider
  rather than a number to type, first Tags suggested in the sidebar. Left and
  Right step it, Page Up and Page Down by a tenth, Home and End to its ends.
- **See-through backgrounds**: Settings › Appearance › More settings ›
  Background opacity (the bars, the tabs, the sidebar), Files background
  opacity (the files, the terminal) and Preview background opacity (whatever
  a preview shows: text, tables, music, the room around a picture, a page or
  a film, and Properties beside it), from 50 to 100 %. What is behind the
  window shows through, blurred where the desktop blurs it, while text, icons,
  pictures, menus and dialogs stay whole; a table's rows, headers and code
  blocks let it through as the preview around them does. The window stays
  opaque while the desktop asks for more contrast.
- **A dragged file's name** is written under its icon on up to three lines, as
  the grid writes names, a long one keeping its beginning and its end, rather
  than on one line stretching far beside it.
- **Names on several lines break where they read best**, in the grid and under
  a dragged icon: lines about as long as each other, broken at a space, before
  the extension or after a separator rather than mid-word, never leaving a
  character or two alone on the last line (`photograph_final.jp` over `eg` is
  now `photograph_` over `final.jpeg`).
- **Out of the window, a drag keeps its name** (Wayland): other applications
  see the same icon and name as Skwir, as sharp on a scaled display.
- **A drag brought back into the window** no longer shows its icon twice.
- **Dragging files marks only where they would go**: a folder, a place, a tab
  or a column under the pointer, no longer the pane or the sidebar around it as
  well.
- **A folder appears whole**: opened, it shows its files all at once when it is
  listed, rather than streaming in and moving under the eye, and a large one is
  ready sooner (`/nix/store`'s 390 000 entries in about half a second rather
  than 0.72 s). It is read and sorted away from the window, which never stalls
  meanwhile, and says how many items it has read so far. Searches, the Trash
  and Nearby still show what they find as they find it.
- **Sorting a large folder anew** (a column's header, the direction, hidden
  files) no longer stalls the window: the rows keep their order until the new
  one is ready, a moment later.

## 0.3.1 (2026-11-05)

Skwir 0.3.1 writes at your desktop's text size and lets you choose your own:
the whole window from 80 to 200 %, the files, reading and code each apart. It
follows your desktop's fonts and colors more closely, offers twelve palettes,
and comes with its own fonts.

[Installation instructions](README.md#installing)

- **Text sizes**: Settings › Appearance › Text size for the window, Files text
  size for the views, Settings › Reading for documents and code; bars, rows and
  menus grow with the text while icons and pictures keep their size.
- **Fonts from** and **Colors from** choose whose fonts and colors Skwir wears:
  your desktop's, Skwir's own, GTK's or Qt's.
- **Palettes**: Skwir, Paper, Slate, Black, Solarized, Nord, Gruvbox,
  Catppuccin, Dracula, Rosé Pine, Tokyo Night and Everforest, worn as you go
  through the list, saved only when you take one.
- **Skwir's own fonts**, Inter and JetBrains Mono, come with its packages.
- **Breeze's icons** take your color scheme's accent, as KDE's applications draw
  them, and bold text in variable fonts (Noto Sans, Cantarell, Ubuntu Sans) is
  drawn bold.
- **Automatic** says what it comes to: "Automatic (Breeze)", the theme it
  gives, the engine installed.
- **Usage** selects files and folders inside folders, and keeps up with a
  folder whose files keep changing for a quarter to a half of the work.
- **Grammars** settings say where grammars were looked for and why one cannot
  be used.

**Changed from 0.3.0**: at 100 % text is your desktop's size times its text
scaling (on a desktop at 10 pt, 13.3 px rather than 14); buttons, menus and
dialogs are written at the body size, the smaller size kept for shortcuts,
counts and headers; Colors from replaces Desktop colors, and if you had turned
Desktop colors off, Skwir keeps its own colors and fonts.

**Windows and macOS builds to try** now draw in the system's font and colors,
and Windows' follows its Text size. They are made on Linux and have never run on
either system: please [try them and tell us what happens](docs/HELP-WANTED.md).
Skwir itself is for Linux, as in 0.3.0.

### All changes

- **Text sizes**: the whole window's text has a size, from 80 to 200 %
  (Settings › Appearance › Text size), and the file views' a size of their own
  (Files text size, or "Files text: larger" in the palette). Bars, rows,
  menus, labels and columns grow with the text while icons and pictures keep
  their size. Names in the grid are now as large as in the list, and tab titles
  and Properties' values as large as the sidebar's places. At 100 % Skwir now
  writes at your desktop's font size, times its text scaling, as its other
  applications do: on a desktop at 10 pt, text is 13.3 px rather than 14, a
  little smaller than before, while tooltips, settings' descriptions and
  messages are written at that size rather than smaller. **Fonts from**
  (Settings › Appearance) chooses whose fonts and sizes Skwir follows: your
  desktop's, Skwir's own (Inter and JetBrains Mono, at your desktop's sizes),
  or GTK's or Qt's where they are set up apart. **Colors from** replaces Desktop colors with the same choices for
  the colors, Qt's color schemes (qt5ct, qt6ct, LXQt) now read too; if you
  had turned Desktop colors off, Skwir keeps its own colors and fonts. A font's Automatic says the family it comes to,
  each size the points it comes to, written in its list at that size, and About's Copy details says the
  display's scale and the text's sizes. A change made in qt5ct, qt6ct, LXQt or
  GTK's settings is taken in as it is saved, and on Wayland GTK's fonts are
  read in the order GTK's applications read them.
  Reading and code have sizes of their own too (Settings › Reading): each file
  opens at its size, the preview's − and + changing only the file shown, and
  tables and the byte view, which could not be enlarged, now follow them.
- **Text you read or act on is written at the body size**: buttons, menus'
  choices, the toolbar's labels, the footer, dialogs and settings' choices;
  the smaller size is kept for shortcuts, counts, column headers and what is
  said under a name, and badges such as the grid's tags are no smaller than
  any desktop's smallest text. Every dialog's title is written at the title
  size. Dialogs, tabs and tooltips grow with the text, never wider than the
  window, while switches and tags' dots keep their size, as icons do. An
  empty folder's or place's message no longer runs past a narrow pane.
- **Bold text in variable fonts**: on Linux, bold and semibold text in a variable
  font, such as Noto Sans on Fedora and NixOS, Cantarell or Ubuntu Sans, is now
  drawn bold, where it was drawn regular: headings and bold in previews, titles.
- **Breeze's icons in your colors**: folders and the other icons Breeze draws
  in the color scheme's colors now take the accent and the warning, error and
  success colors as KDE's applications draw them, where large folders were
  drawn flat in the text's color, grey or black.
- **Skwir's own fonts**, Inter and JetBrains Mono for code, now come with its
  packages, so choosing them looks the same everywhere, and macOS and Windows
  write code in JetBrains Mono by default. They are read only when drawn.
- **Automatic says what it gives**: in Settings, a setting left on Automatic
  outlines the choice it comes to now (the theme's Light or Dark, GTK or Qt,
  the document, DjVu and video engine installed, where servers are kept), or
  says it beside, "Automatic (Breeze)", as a language's grammar does.
- **macOS and Windows** draw the window in their own interface font (San
  Francisco, Segoe UI) rather than Inter, and Windows' Text size, in its
  Accessibility settings, enlarges Skwir's text as it changes. They wear their
  colors too: on macOS those it draws its own windows in, Increase contrast's
  when it is on; on Windows the accent, and a contrast theme's colors while one
  is on.
- **Palettes**: Colors from also offers twelve palettes, worn whatever the
  desktop says but its more contrast: Skwir, Paper, Slate and Black, and
  Solarized, Nord, Gruvbox, Catppuccin, Dracula, Rosé Pine, Tokyo Night and
  Everforest as their makers publish them, each with its own accent, terminal
  colors and code colors (Reading's Default). Each is drawn in the list as a
  small window in its colors, and the window wears the one highlighted while
  the list is open; nothing is saved until one is taken.
- **The desktop followed more closely**: GNOME's Large Text shows as it is
  turned on or off, and on X11 it no longer enlarges the text twice; a settings
  portal that stops answering no longer stops Skwir following the desktop; on
  Wayland, where the portal names no font, GTK's settings are read as GTK reads
  them; a print is written in the fonts Fonts from chooses.
- **Select inside folders in Usage**: a click on a block inside a folder
  selects that file or folder, at any depth, to open, copy or act on as any
  other, and the arrows move among the blocks inside folders. What is selected
  there follows a rename, and is read only when it changes.
- **Usage while files keep coming**: a folder whose files keep changing is
  counted again at a pace, the map at most a second behind, for a half to a
  quarter of the work it took.
- **Grammars**: Settings › Reading › Grammars says where grammars were looked
  for and what was nearly found (a repository a folder too deep, a
  configuration that does not read), each with what to do. An editor's
  grammar library is colored with its grammar's own repository's queries or
  the system's, never another editor's, checked before it is offered and
  named by both ("tree-sitter CLI, with Emacs's library"); one that cannot be
  used says whether its library or its queries are refused, and what to do. A
  library in the system's folders is the system's, not Emacs's.
- **Dragging** names what it carries on one line, in the window's font,
  where a name broke at each dot or dash, in a font of gpui's choosing.
- **The board** keeps every column filled after another view and back, where
  all but the place's own showed empty.
- **Print Listing…** is what the folder's menu calls printing its listing, as
  the palette does; Print… stays for files.
- **More settings** points down at a settings page's end, and Fewer settings
  up, where the page goes; More settings pointed right, as a link to another
  page does.

## 0.3.0 (2026-11-03)

Skwir 0.3 reaches servers, phones and cameras: type an address and it mounts
the server with a tool you have, lists the servers your network announces and
the phones you plug in, and copies to and from them in batches, going on from
where a server stopped. What runs stands in the footer, saying how fast it goes
and how long it has left, and closing the window no longer stops it. The
address field is now one field to go somewhere, filter the folder and search
below it as you type, and what you filter by stays as you go. Skwir prints
what it previews, booklets included, makes smaller copies of pictures and
PDFs, and colors code with the tree-sitter grammars your editors installed.
Menus open their submenus beside them, F1 lists keys by topic, and a tag looks
the same everywhere. FB2 and Kindle books open beside EPUB. Each release now
offers builds to try on Windows and macOS.

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
- **What runs** stands at the end of the footer, saying how fast it goes and
  how long it has left, and how far the whole queue has come ("2 of 3 · 47 % ·
  1 waiting"); `Ctrl+Shift+J` opens Activity, with each action, what waits its
  turn and what failed this session. An action that lasts more than a moment
  says as it starts that it gets ready, goes, or waits its turn, on a tint.
  Closing the window stops nothing: Skwir finishes without it, an icon in the
  tray, and the desktop is told when a long action ends.
- **Undo history**: the arrow beside Undo in the footer (or `Ctrl+Alt+Z`, or
  More › Undo history…) lists what you did, each with its own Undo. An action
  can be undone on its own unless a later one touched the same files: then it
  says which, and offers to undo them together.
- **The address field**: `Ctrl+Shift+F` filters this folder and `Ctrl+F`
  searches it and its subfolders, both as you type, one switch between them.
  The field offers places, tags, tabs, history and commands; `#tag` finds a
  tag; a Search button ends it.
- **Filters stay as you go**: a kind, a tag, a date or a size you filter by is
  kept in every folder and search you open, until you take it off; Quick
  filters offer what to narrow to anywhere. Back shows each place as you left
  it, and `Ctrl+D` keeps a filtered folder under Saved with its filter.
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
- **Books** in FB2 and Mobipocket (MOBI, AZW3) open as EPUBs do, and text can
  be read a screen at a time, the wheel turning a page.
- **Printing**: Print… (`Ctrl+P`) prints what a preview shows, or makes a PDF
  of it: the pages you pick, a booklet of two, four or eight pages a side, a
  contact sheet of pictures, a folder's listing, several files as one job
  ([the guide](docs/guide.md#previews)).
- **Smaller copies**: Reduce Size… makes a smaller copy of a picture or a PDF
  beside it; a PDF keeps its text, links and forms, and a scan's black and
  white is written without loss.
- **Code** is colored by the tree-sitter grammars your editors or your system
  installed (Zed, Helix, Neovim, Emacs) as well as by bat's, chosen language by
  language, and folded and outlined as its grammar says.
- **Typing a name** goes to the next one starting so each time, accents and
  case aside, and brings it to the middle of the view.
- **Builds to try on Windows and macOS** (Apple silicon, macOS 12 or later)
  come with the release, opening archives and playing audio and video now. They
  are made on Linux and have never run on either system
  ([how to help](docs/HELP-WANTED.md)).

Changed from 0.2, which you may notice:

- Closing the window stops nothing that runs: Skwir finishes it without the
  window, then ends. Quit asks while something runs: Finish in the background,
  Stop and quit, or Keep the window.
- What runs is in the footer and in Activity (`Ctrl+Shift+J`), no longer in
  cards piled over the last files of the list; toasts say one answer at a time.
- While an action runs the computer does not go to sleep by itself, and one
  that ran 5 s or more and ends while you are in another window is a desktop
  notification (Settings › General › Actions turns either off).
- A notch of the wheel moves a document 120 px, as browsers do, where it moved
  60; a touchpad moves it as before.
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
- Narrow by is now Quick filters (View › Quick filters), over any place, and
  what it or the address sets that the files hold (a kind, a tag, a date, a
  size) stays as you open other folders and searches until you take its pill
  off.
- Back and Forward show a place with the filters it had when you left it.
- `Ctrl+D` on a filtered folder (or a tag, the Trash) keeps it with its filter
  under Saved, the sidebar's Searches section renamed; a folder's own menu
  still bookmarks it alone. A saved or recent search runs as it was, without
  the filters you have on now.
- A letter typed again goes to the next name that starts with it, not the same
  one.
- The EPUB plugin is Books now; what you set for it is kept.

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
  key of yours on the server over one login (on Windows too, where a server
  that offers only SFTP is reached that way).
- **Nearby**, at the end of the Network section, lists the servers your network
  announces (a NAS, a Mac, a Windows PC), found by Avahi and wsdd while you look
  at it, and only then; a server's menu adds it. A Skwir that crashed leaves no
  wsdd of its own searching the network.
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
  take turns, which ends sooner than sharing it. Moves to the trash, and tag
  changes in different folders, run together too. A folder made in the
  folder a long copy goes into is made at once: an action waits only for one
  making the same names. Undo takes back the last
  thing you asked for, and waits for it if it still runs; a toast's Undo takes
  back the action it reports, even after an unrelated move to the trash or tag
  change.
- **Retry**: an action an error stopped (a server gone, a disk full, a drive
  pulled out) is retried from where it stopped, from its message or
  `skwir cmd retry`. A large file goes on from what got across once its last
  part is checked; a folder keeps the files it had finished, and the file it
  was sending goes on too (by rsync, sftp or Skwir's own copy; `gio` and KIO's
  copies start it again). Undo takes the whole action back. An action a server
  stopped goes on by itself once the server answers again: its message says it
  waits, and its bar says it goes on.
- **How fast, how long** (ADR-113): an action's bar says how much of how much,
  and under it how fast it goes and how long it has left (`23.4 MB/s · about 3
  min left`), measured from its first byte over the last few seconds, its
  total known from its start. Onto a mount that sends what is copied after
  (rclone's caches) it says "then sent to the server", then the sending's own
  speed and time left. A long action's answer says how much it moved in how
  long ("30.0 MB in 17 s"), and `skwir cmd running` says what runs. Cancel
  stops a copy within a second or two, in the middle of a large file from a
  slow server too; a server's bytes show as they arrive, and a batch counts
  its files as each comes whole.
- **Activity** (ADR-114): what runs is a strip at the end of the last pane's
  footer, before Undo, in as many words as there is room for ("Copy 3 items
  to Backup · 42 % · about 3 min left"), with Pause and Cancel for one action,
  several summed, with Pause all and Cancel all. A click or `Ctrl+Shift+J`
  opens Activity above it: each action's card, what waits its turn (each with
  Cancel, or Cancel waiting), Cancel all over the whole queue, and
  Earlier, this session's answers the Undo history does not hold (what failed,
  with Retry, and messages). Pinned, it stands above the footer while something
  runs, taking room rather than covering the files, three cards high at most,
  Earlier folded to a line.
  An action an error stopped stays in the strip, in red, until it is retried,
  seen or dismissed. Toasts say one answer at a time, at the last pane's lower
  right.
- **The desktop is told** (ADR-114): an action that ran 5 s or more and ends,
  stops or waits for its server while you are in another window is a
  notification, taken back once you come back; a click brings the window
  forward with Activity open. Plasma's task manager, Windows' taskbar and the
  Dock show the progress, and the computer stays awake while something runs
  and is not paused (a closed lid still sleeps). Settings › General › Actions
  turns the notifications, or keeping awake, off.
- **Without the window** (ADR-115): closing the window stops nothing; Skwir
  finishes what runs, then ends by itself. Meanwhile an icon in the tray, or a
  notification that stays where there is no tray, shows how far it went, with
  Open, Pause all and Stop all and quit; every end is told, and a question
  waits there for you. Starting Skwir again opens its window on what runs.
  Quit asks while something runs: Finish in the background, Stop and quit, or
  Keep the window. Logging out or shutting down is held off, naming the
  action, where the desktop lets a program ask (Plasma 6.8 lets none).
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
  a tag, Recent). Its list stays within the window, scrolling, and `tag:` or
  `#` offers every tag you have, the folder's first, as quickly where each
  file carries dozens.
- **Filters that stay** (ADR-107): what of a filter or a search the files
  hold (a kind, an extension, a tag, a date, a size, a rating) is kept wherever
  you go, a folder below or above, a search, the Trash, a tag, a tab or a split
  opened from the pane, until you take its pill off, Escape shows everything or
  Quick filters change it; the words that find names stay with the folder they
  were typed in. Its pills say they stay, the footer counts what is shown ("12
  of 340 items", "Reading 1,200 files…" while sizes and dates come), and a
  folder it hides everything in says "Hidden by filters", with Show everything.
  **Quick filters** (once Narrow by) show over any place when asked and say why
  they offer nothing; over a search with a tag, or a tag's place, by themselves.
- **Places as you left them** (ADR-109): Back and Forward show each place with
  the filters it had when you left it, a search with its own terms. `Ctrl+D`
  and the bookmark button keep what is shown: a folder as a bookmark, a search
  as a saved search, and a folder, a tag or the Trash shown filtered under
  **Saved** (the Searches section, renamed) with its filter, named after it
  ("Pictures · kind:image"); the button is filled only for it as filtered so. A
  saved one opens as it was kept, a bookmark with the filters you have on. A
  closed tab opened again keeps its filters, as do the last window's tabs when
  Skwir starts. A server's folder is kept by its address, so it connects again;
  with the query plugin off, `Ctrl+D` bookmarks a filtered folder alone and
  says so.
- **A tag's place** is searched and filtered at its own depths (ADR-108): the
  switch reads "This tag | With sub-tags", All Tags' "These tags | Every tagged
  file", the words typed filtering what each lists.
- **Filter and search, one field**: `Ctrl+Shift+F` looks in the folder alone
  and `Ctrl+F` in its subfolders too, the switch at the top of the list going
  from one to the other with the words kept. The files show what the words keep
  as you type: the folder filtered at once, or the search after a short pause.
  A folder's filter is drawn as a search's pills, right after its trail, and a
  search keeps its folder's trail. `dunes jpg` finds `dunes.jpg`; `#print` is
  `tag:print`, and `#` alone offers the tags. A Search button ends the field
  ("Search this folder and its subfolders", "Edit this search"), on a folder,
  a tag or Recent; All Tags, the Trash and the shelf are no place to search in.
  Taking a search's last pill away shows its place whole.
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
  as the same chip, and Properties' folded Notes heading too. The sidebar no
  longer suggests a tag found only in hidden folders or on a server mounted
  under your home, and a tag refused beside a Mac's `._` file Skwir won't
  rewrite says so.
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
- **Typing a name** in the files goes forward, as in Dolphin: a new name, and
  one letter typed again and again, look past the file the cursor is on, so
  each press reaches the next name, round the end after the last; a name typed
  on stays on its file while its name still begins so. Names are matched as
  the order sorts them, case and accents aside ("ecole" reaches "École"), Space
  goes on with a name where one goes on so (else it peeks), the file reached is
  brought to the middle of the view, and a pill at the foot of the pane shows
  what is typed, "No match" once the folder is listed. A key stepping past the
  list's bottom edge moves it one row, not a page.
- **Usage's map** is drawn in soft tiles: rounded blocks apart from each other,
  open folders as pale trays under a header, in tints of one lightness that
  tell neighbours apart, the chosen block ringed in the accent. A block keeps
  its tint as files come and go, and where the desktop asks for more contrast
  its labels read at 7:1.
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
- **Books**: FB2 (and FB2 in a zip) and Mobipocket books (MOBI 6, KF8/AZW3, and
  PalmDOC) open in the books plugin, once EPUB's, with their covers, contents
  and pictures; a book protected by DRM, or in Amazon's KFX, says so. Settings ›
  Reading's book engine is one choice: Automatic reads them with Skwir's own
  engine, MuPDF chosen reads those it reads well.
- **Text read a screen at a time**: with Continuous scrolling off (its button
  after Full width, or Settings › Reading), Markdown and HTML in reading mode,
  word processors' documents, reflowed books and e-mail turn a screen at a
  time, never cutting a line; a notch of the wheel, Down, Page Down or Right
  turn one on, Up, Page Up or Left back. It is a choice of its own beside the
  documents' continuous scrolling.
- **A server's previews** read each file once: a small text on a cloud remote
  opened with up to thirteen downloads, now one, and the file shown before is
  kept a minute for coming back to it. rclone's mounts keep what they read
  (`full` cache), and a copy onto one ends only once the server has it; a file
  another program left to send says so on its place ("Sending 1 file to the
  server…").
- **Previews**: a file of several sections (a workbook's sheets, a book's
  chapters, a font's character sheets) keeps both arrows and its tools
  (Contents, the text size, Full preview) as you go from one to the next. Read
  a page at a time, a notch of the wheel turns one page, from a page that fits
  the panel too; read continuously, it moves the pages 120 px, as browsers do,
  where it moved 60, a touchpad as before. A table's columns (a CSV, a
  spreadsheet, a database) are sized by dragging the edge of their header, a
  double click fitting one to its content again. Find says that nothing matches in
  its own popup, under what you typed, rather than at the bottom of the
  window; so does the hex view's
  "Match at 0x…". A protected EPUB says so rather than show its encrypted
  chapters, and a film made to be stretched (anamorphic) shows its poster in
  its own shape. Mermaid's architecture diagrams are drawn, and a STEP or IGES
  model whose curved faces close on themselves without a seam (a cylinder, a
  dome, a torus) is drawn whole, most parts faster and with fewer triangles.
  Spreadsheets and SQLite databases are recognised by their type, not their
  name: a database named `.sqlite` or `.db3` shows when it holds SQLite, and a
  file of such a name holding something else is no longer opened as one. A
  document whose first page is slow to draw (a scanned cover) shows it at once,
  blurred, then sharp, and its first page is drawn at the size the panel needs
  rather than a fixed large one. The wheel past the end of a font, or of any
  file, opened from an archive turns that file's view, no longer closing it.
- **Printing** (ADR-111): Print… (`Ctrl+P`, the file's menu, the preview's
  More menu) prints a file as its preview shows it: a PDF as it is, an office
  document as its pages, a text or source file numbered and colored, a table
  with its header on every page, a Markdown document or a book as it reads, a
  picture fitted to the page. Skwir's dialog asks first: to a printer or a PDF
  beside the file; all pages, the page shown, what is selected or pages typed
  (`1-3, 7, 10-`); as shown, or a booklet of two, four or eight pages a side,
  each drawn with the pages where they land, its folds marked in the margins
  and how to fold said once it is made; for a PDF, its paper. Your system's
  dialog then chooses the printer, the paper, the copies and the sides.
  Several files selected print as one job, or make one PDF or one each; with
  pictures among them, a contact sheet prints them several to a page; with
  nothing selected, Print… prints the folder's listing as a table. A print
  shows its progress, with Cancel, while its PDF is made, and a PDF saved can
  be made smaller (Smaller file). Scripts save PDFs with or without the window
  open (`skwir cmd --apply print.save_pdf`).
- **Smaller copies** (ADR-112): Reduce Size… (Compress in the menu of JPEG
  and PNG pictures and PDFs, the preview's More menu) makes a smaller copy
  beside each file, `scan (reduced).pdf`, the file itself untouched (Undo puts
  the copy in the trash): Lossless, Print, Screen or Small, in gray if you
  like, without a photo's location and camera details (its date stays), or at
  most a size. A PDF keeps its text, links, forms and outline: only its images
  change (at most 300, 150 or 96 dots per inch as its pages draw them, below
  Lossless), a scan's black and white written without loss (JBIG2), and a
  page that would look different keeps its own. A copy that would not be
  smaller is not made, and a PDF protected by a password or signed is left as
  it is; Skwir says so. It shows its progress with Pause and Cancel, and is
  made by Skwir's own engine, the same on every system; `shrink.reduce` makes
  the plan for scripts and agents.
- **Code colored by grammars** (ADR-110): code is colored with bat's grammars,
  those you added to bat or Sublime Text, or a tree-sitter grammar: one in
  Skwir's grammars folder (`~/.local/share/skwir/grammars`), or one an editor
  or your system installed (Zed's extensions, Helix, Neovim, Emacs, the
  tree-sitter CLI, `/usr/lib/tree_sitter`). Settings › Reading › Grammars lists
  the languages with the files each colors and what colors it: Automatic (bat
  first, unless you choose otherwise), bat, the tree-sitter grammar of each
  place one was found, or Plain text; a language bat does not know, such as
  Svelte, is colored by its tree-sitter grammar. Both wear one theme. A
  tree-sitter grammar reads the file whole, so a string open across a jump is
  colored right; the code view folds where it says, and Contents lists the
  file's definitions, each leading to its line. Code blocks in documents are
  colored the same way. Grammars are read by a helper of their own, sandboxed,
  let go of a minute after its last answer.
- **Settings' lists** (map tile servers, rclone's remotes, the languages and
  their grammars) are drawn as lists inside their card, saying so when empty,
  a long one with a filter and its rows folded past eight.
- **Faster with many files**: selecting all of 100,000 files and acting on
  them, a plan's checks, the tags index, a tag's places, a listing's renames
  and a filter's tests stay in proportion to what changed: finding a whole
  selection's files took 100 s there, now 43 ms. A text field's drag selects
  past its ends, scrolling; the address field's rows hold still as you type,
  and `Enter` or `Tab` waits for the folder typed.
- **Fixed from 0.2**: a document read in continuous scrolling had a scrollbar
  that ended at the last page visited; it measures every page. A long name in
  the grid wraps on up to three lines again. Going from one PDF to another in
  an archive no longer redraws the first a page at a time while the next one
  opens, nor does turning to a PDF's hex view. Files copied (not cut) and
  pasted in the Trash are refused, rather than the files themselves being
  trashed; the same goes for Shift+F5 to a pane showing the Trash. A drag onto
  the Trash still trashes, whatever keys are held. Undoing a move to the trash on
  another drive whose original stayed (it changed during the copy, or could
  not be removed) no longer fails with "is there again": the trash's copy is
  taken back when nothing would be lost. A name whose accents are stored apart
  from their letters (as macOS writes them) sorts beside the same name written
  whole, and typing finds it either way.
- **Scripts and agents**: a file selected, a folder opened or a sort changed by
  `skwir cmd` or an agent shows in the window at once, not at the next move of
  the mouse. An agent's changes are always reviewed and its private folders
  withheld, whatever its client says, scripts reaching the window by a way of
  their own; no server is reached for an agent unasked.
- **Player**: the rates are said over MPRIS, and a rate set there is taken.
  The media keys, the desktop's style and plugged-in phones keep being
  followed after the session's bus restarts.
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
  1.99.1; PDFium chromium/8125 in the packages; every other dependency at the
  newest release the others allow. tokio, rustls-webpki, Wasmtime and
  pulldown-cmark were updated for security advisories (RUSTSEC-2026-0422,
  0423, 0424 and 0433, a Markdown file of nested wikilinks that could hold a
  worker); plugins from outside now run on Wasmtime 48.0.8, a long-term
  release supported until 2028 and the one tree-sitter's grammars run on
  (ADR-110), and each is compiled once more, on its first start after the
  update. Two helpers are new in the `skwir` package:
  `skwir-tree-sitter-worker` reads grammars, `skwir-print-worker` makes PDFs
  to print and smaller copies.
  The 3D model helper is built with ogeom 0.14, under the LGPL 2.1, its source
  published beside every build (ADR-042, amended). The packages suggest the
  tools remote folders use (sshfs, rclone, GVfs's backends, kio-fuse, rsync,
  FUSE, Avahi, wsdd). The AppImage, the tarball and the macOS build carry
  their own SQLite, as Windows' does: the newest rusqlite needs one newer than
  older systems have (ADR-022, amended). Distributions' packages still use the
  system's.

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
