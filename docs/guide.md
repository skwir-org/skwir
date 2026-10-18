# Using Skwir

This guide is for Skwir 0.1 on Linux. Every action is also a command in
the palette (`Ctrl+Shift+P`), which shows its keys. `F1` lists the keys
that work where the keyboard is.

## The window

- **Sidebar** (`F9` shows or hides it). Places in groups: your folders,
  the places you kept (bookmarks, searches, tags), Recent, the Trash and
  the drives. Click a group's header to fold it, drag it to move it; the
  sidebar's menu shows or hides each group. A place can be renamed
  (`F2`), moved or hidden; renaming a place never renames its folder.
  Drop a folder beside the rows to bookmark it.
- **Sidebar keys.** `F6` goes round the sidebar, the files and the
  preview. In the sidebar the arrows walk it, `Enter` opens a place, `Ctrl+Enter` opens it in a tab,
  `Alt+Up`/`Alt+Down` move it, `Delete` removes or hides it, `Escape`
  goes back to the files.
- **Panes.** Each pane has its own row: Back, Forward, the address, and
  the view buttons. `F3` (or `Ctrl+Shift+D`) splits the window or joins
  it, `Shift+F3` splits the other way, `Tab` switches pane. General says
  which pane closing the split closes.
- **Tabs.** `Ctrl+T` opens one, `Ctrl+W` closes it, `Ctrl+Shift+T`
  reopens the last closed. `Ctrl+Tab` and `Ctrl+Shift+Tab` (or
  `Ctrl+PageDown`, `Ctrl+PageUp`) switch tabs, `Ctrl+Shift+PageDown` and
  `Ctrl+Shift+PageUp` move one, `Alt+1`…`Alt+8` go to a tab and `Alt+9`
  to the last.
- **More** (the sidebar's button, or the pane's row when the sidebar is
  hidden) holds what has no button, and Layout: where the split and the
  preview go, or Automatic, which follows the window's shape. About
  Skwir, at its end, shows the version, system, display and graphics,
  and copies them for a report.
- **Footer.** Each pane's footer shows how many items it lists, what is
  selected and what it weighs. The last pane's footer also shows Undo,
  what waits for a paste, the shelf, plans waiting for review, and the
  player.
- **Toasts** say what was done ("Moved “a.txt” to Trash"), with Undo. A
  long plan shows its progress with Pause and Cancel. Quitting while one
  runs asks first, then undoes what it left half done.

## Moving around

| Keys | Action |
| --- | --- |
| `Enter` | Open |
| `Alt+Up`, `Backspace` | Parent folder |
| `Alt+Left`, `Alt+Right` | Back, forward (hold or right-click Back or Forward for the history) |
| `Alt+Home` | Home folder |
| `Ctrl+L` | Type a path |
| `Ctrl+J` | Jump |
| `Ctrl+Enter`, middle click | Open in a new tab behind this one |
| `F5`, `Ctrl+R` | Refresh |
| `Ctrl+D` | Keep the place shown in the sidebar |

- **The address** shows where you are as a trail. Click a segment to go
  there; each `›` lists the folders inside the segment before it. The
  trail starts at the deepest sidebar place that holds the folder, under
  its sidebar name; Appearance › Show the full path starts it at `/`.
  Click beside the trail, or press `Ctrl+L`, to type.
- **Typing in the address.** Words filter the folder shown as you type,
  and stay after the trail as a pill with a funnel, as a Narrow by choice
  does; `Escape` or its × shows everything again. A search, by contrast,
  takes the trail's place; `Escape` with nothing selected goes back to the
  folder it looks in. The dropdown says what `Enter` does
  and offers the rest: a path goes there, words search the folder shown
  (`Alt+Enter`: the home folder), `>` hands the rest to the palette.
- **Type the first letters of a name** in the file list to go to it.
- **Jump** goes to a folder you used before, from zoxide's list and the
  desktop's recent files, by a few letters of its name.
- **Recent** (in the sidebar, or `recent:`) lists the files the desktop
  says you used recently.

## Views

`Ctrl+1`, `Ctrl+2` and `Ctrl+3` show the folder as a list, a grid or
columns. The View menu, at the end of the pane's row, has every view, the
sort, hidden files (`Ctrl+H` or `Alt+.`) and the zoom (`Ctrl++`,
`Ctrl+-`, `Ctrl+0`, or `Ctrl+wheel`).

- **List.** The header's menu chooses the columns; click a header to sort
  by it. Where files from many folders are listed (a search, a tag, the
  shelf, Recent), the Folder column shows where each one is, and Show in
  Folder (`Ctrl+Alt+O`) goes there.
- **Grid.** Thumbnails and names; View › Fill thumbnails crops pictures
  to their tiles.
- **Columns.** The folders above, side by side, and what the folder under
  the cursor holds after them. `→` goes into it, `←` back up; a click on a
  folder shows what it holds. `Shift` and the wheel scroll to the columns
  that do not fit.
- **Tree** opens folders in place. **Board** shows the folders of the one
  shown as columns of cards you can drag between.
- **Usage** (`Ctrl+4`) draws what takes the room in a folder, counted as
  you look; its − and + (or the zoom: `Ctrl` and the wheel, `Ctrl+=`,
  `Ctrl+-`) draw more or fewer levels.
  - Click a block to select it, double-click one inside a folder to go
    there, arrows to move between blocks; `F5` or `Ctrl+R` counts again.
  - Its buttons size blocks by room or by number of files, and color them
    by age or by kind of file.
  - A dashed outline marks a cache or a trash: room you can reclaim
    without losing anything you keep.
  - Where a folder's files also have names elsewhere (hard links), it
    says what deleting the folder would actually free. Moving files to
    the Trash frees nothing until it is emptied.
- **Map** (`Ctrl+5`) places a folder's pictures and videos, and tracks
  and flight plans, where they were taken or go. Its default background
  is drawn offline from data Skwir carries; a background that downloads
  tiles (OpenStreetMap, a server you add) is used only once you choose it
  in Settings › Map backgrounds.
- **Timeline** (`Ctrl+Shift+L`) shows when files changed, one bar per
  period; click a bar to search it.

## Selecting and acting

- **Select.** A click or an arrow selects; `Shift` extends;
  `Ctrl+click` adds or removes one (or `Ctrl+arrows` to move, then
  `Ctrl+Space`); `Ctrl+A` selects all, `Ctrl+Shift+I` inverts, `Escape`
  clears. Drag from an empty spot for a rectangle. The Menu key or
  `Shift+F10` opens the context menu.
- **Copy, cut, paste:** `Ctrl+C`, `Ctrl+X`, `Ctrl+V`. What waits for a
  paste is dimmed and counted in the footer; a click on the count
  forgets it. On Linux the clipboard is the desktop's: files copied or
  cut in Skwir paste in other file managers (GNOME's, Thunar, Dolphin…)
  and as their paths where text goes, and files copied there paste in
  Skwir. Copying anything else forgets a cut. With two panes, `Shift+F5` copies the selection to
  the other pane and `Shift+F6` moves it there; `Shift+F7` asks, as a
  drop does (and offers to extract an archive there).
- **Drag and drop.** Files dropped on a folder ask whether to move, copy
  or link them; hold `Shift` to move, `Ctrl` to copy or `Ctrl+Shift` to
  link without asking (General can turn the question off). An archive
  dropped can be extracted instead. Files dropped on a sidebar place go
  where they would if dropped in that place: into a folder, into the
  Trash, onto a tag. On Wayland, files also drag into other
  applications.
- **Copies.** A copy onto a USB drive runs at the drive's pace, so its
  progress is what is really written, and it ends once the files are on
  the drive. A copy that needs more room than its destination has asks
  before it starts. Hard links stay links; each name of a file is its
  own row, and the footer counts the file once.
- **Names already taken.** A copy, a move or a link onto names already
  in the destination first asks, once, about all of them: keep both (the
  new one is named `name (copy).ext`, a link `name (link).ext`), replace
  (what was there goes to the Trash), replace if newer, or skip; a folder
  onto a folder is merged by default (a link is never merged). Choose
  for all files and all folders, or row by row (`↑` `↓` a row, `←` `→`
  its choice); `Enter` goes on, `Escape` cancels and nothing is done. A
  copy into its own folder makes `name (copy).ext` without asking, and
  Undo takes all of it back.
- **Rename** with `F2`. With several files selected, `F2` (or `Ctrl+F2`)
  renames them together from a template, a regex and case rules, with a
  preview of every new name (`Tab` moves between fields, `Ctrl+K`
  changes case).
- **Edit as text.** `Ctrl+E` opens the folder's names in your editor
  (`$VISUAL` or `$EDITOR`). What you rename, move or delete there comes
  back as a plan.
- **New folder:** `Ctrl+Shift+N`.
- **Open with** another application: `Shift+Enter`. In the list, `Alt+A`
  switches Always use for… to make the choice the default.
- **Copy Path:** `Ctrl+Shift+C` or `Ctrl+Alt+C`, or in the context menu:
  the selected paths as text, one per line; on the folder's background,
  the folder's. **Copy paths for a shell**, in the palette, quotes them
  for a command line (give it a key in Keyboard settings). **Run
  Command…** in the context menu runs a shell command on the selection
  in a terminal.
- **Extract Here**, in an archive's menu, extracts it beside it (its one
  folder or file as it is, several in a folder named after it), asking
  a ZIP's password when it has one. **Compress to
  ZIP** and **Compress…** (the name's ending picks the format: `.7z`,
  `.tar.zst`…) make an archive of the selection beside it.
- **Find Similar**, in the context menu, searches for files of the same
  kind, day, size or tags.
- **Delete** moves to the Trash. `Shift+Delete` deletes for good after
  asking, since it cannot be undone (and says what stays on disk when a
  file has other names).
- **Undo and Redo:** `Ctrl+Z`, `Ctrl+Shift+Z` (or `Ctrl+Y`), for every
  change Skwir made, in order. The history lasts while Skwir runs.
- **Plans.** A change to many files, and anything an agent or a plugin
  proposes, is shown as steps to review first. `Enter` applies it,
  `Escape` dismisses it.

## Other places

- **The Trash** lists what every trash holds, with where each file came
  from and when it was deleted. Restore puts a file back where it was; a
  file dragged or pasted out of it is restored where you drop it.
  `Delete` there deletes for good; Empty Trash is in its row.
- **The shelf** (`Ctrl+Shift+B` adds the selection) gathers files from
  anywhere to move, copy or trash them together later. `Delete` there
  takes files off the shelf; its buttons move or copy them to the other
  pane.
- **Drives** are listed mounted or not. Click one to mount it (a locked
  one asks for its passphrase); middle-click to open it in a new tab. A
  drive mounted for you has an eject button, which writes out what is
  pending and unmounts it.

## Searching

`Ctrl+F` starts a search in the folder shown. A search is a place, with
Back, tabs and the sidebar like any folder. Its address shows what it
looks for as pills: click one to change it, **+ Filter** adds one, and
the text can be typed as is:

| Text | Finds |
| --- | --- |
| `report` | names containing "report" |
| `*.pdf`, `ext:pdf` | by extension |
| `kind:image,video`, `mime:application/pdf` | by kind or type |
| `type:dir`, `type:file` | folders or files only |
| `size>10M`, `size<1.5G` | by size |
| `modified>=2026-01-01`, `modified<7d`, `modified:this-week` | by date |
| `tag:Places/France`, `-tag:draft` | with a tag, or without |
| `rating>=3`, `comment:holiday` | by rating or comment |
| `content:"it's here"`, `content:/re+gex/i` | files containing text or a regex |
| `near:45.0,6.0~5km` | files made near a point (a picture's position) |
| `in:"~/My Files"`, `depth:2`, `hidden:true` | where and how deep to look |
| `a OR b`, `NOT a`, `(a b)` | either, not, together |

- `Alt+Enter` searches the home folder instead. `Ctrl+D` keeps a search
  in the sidebar.
- Over a search with tags, a bar offers what it can narrow to (the tags
  within, the files' kinds and years), each with a count. Over a folder,
  View › Narrow by shows the same bar.
- A search uses the desktop's indexes where they run (Baloo, plocate) to
  go faster, and always checks the files themselves.

## Tags, ratings and comments

`Ctrl+Shift+G` edits the tags of the selection (`Ctrl+Enter` applies).
Properties (`Ctrl+I`, or `Alt+Enter` on a file) shows them with the rating
and the comment.

- **Where they are kept.** Where other applications keep them: the
  file's extended attributes (as Dolphin does), XMP sidecars
  (`photo.jpg.xmp`, as darktable and Lightroom do), a Mac's tags and its
  `._` files on drives without attributes, TagSpaces' sidecars, and tags
  in names (TagSpaces' `name[a b].ext` where you use TagSpaces;
  filetags' `name -- a b.ext` once turned on). Settings › Notes says
  which to read and which to write. When two disagree, the latest change
  wins and Properties says so.
- **Tags in names.** Notes › Tags into Name and Tags out of Name, in the
  menu of a selection, rename files to hold their tags or to drop them.
  With a name's Write set to Update existing, adding or taking a tag
  also renames a file whose name already holds tags (`IMG[vacation].jpg`
  becomes `IMG[vacation alps].jpg`), and Undo takes back both. A name
  holds no spaces or categories; those stay in the other stores.
- **Categories.** A tag named `Places/France/Paris` is within
  `Places/France` and `Places`: a search for `Places` finds it. A
  category can hold one tag at a time (Status: Draft, Done).
- **Tags are places.** `tags:/` lists every tag in your home folder, each
  a folder of its files. `Alt+G`, or the Tags button at the end of the
  address, shows the tags of the folder you are in (and back).
  - Drop files on a tag to tag them; `Delete` takes the tag off the
    selected files.
  - `Ctrl+Shift+N` makes a new tag inside the one shown; `F2` on a tag
    renames it everywhere, as a plan to review.
  - Pin the tags you use to the sidebar. `Shift+click` a pinned tag to
    narrow what is shown to files that have it; `Alt+click` to show its
    files in the folder shown.
- **Copies keep them**: tags, rating, comment and where a download came
  from, even onto a drive without attributes (Settings › General). A move
  always keeps them.

## Previews

`Space` peeks at a file (the arrows walk the files, `Space` or `Escape`
closes). `F11` (or `Ctrl+Shift+Space`) opens the preview panel, which
follows the selection. Each kind of file has its tools in the panel's
bar. Skwir previews text and code, Markdown, tables, structured data,
PDFs and office documents, books, comics, e-mail, notebooks, pictures,
audio and video, fonts, 3D models, circuit boards, maps and tracks,
archives, and the bytes of any file.

- A file shown other than as written (Markdown, a table, a map, JSON
  formatted) has **Source** in the bar: lit, it shows the file's own
  text; pressed again, the rendered view.
- A text file of any size opens whole, numbered from its first line;
  Find and Copy see all of it. More › Wrap lines (`Alt+Z`) lets long
  lines go on sideways.
- A link in a document opens with a click; an address found in plain
  text with `Ctrl+click`.
- Once clicked, a preview takes the arrows, as a PDF reader does: Up and
  Down scroll, Left and Right scroll sideways when it is wider than the
  panel, or else turn the page or the chapter.
- `F` shows a picture, a document, text or a film on the whole screen;
  the arrows go to the next picture or page, and `Space` plays the
  folder's pictures as a slideshow.
- Music and video can keep playing while you browse, in a player at the
  foot of the panel, steered by the desktop's media keys.
- Some kinds need a program Skwir does not carry; Settings › Plugins says
  which, and what to install.

[Previews](previews.md) lists every kind of file with its tools, keys
and requirements.

## Properties

`Alt+Enter` (or `Ctrl+I`, or Properties at the end of the context menu)
shows everything known about the selection in the panel, under the
file's name: its exact size and the room it takes on the drive, where it
is, who owns it and who may use it, its times, your notes, and the drive
it is on. On the folder's background it describes the folder shown. A
click on a row copies what it says (its Path, its Folder).

- **What a folder holds** is counted when you ask: Properties counts it
  at once, or click Count. The total grows as it goes; Stop keeps what
  it has. A file with several names counts once; a drive mounted inside
  is said, not counted.
- **Several selected**: how many of each kind, their total (Count when
  folders are among them), and the owner, group and access they share.
- **Who may use it**, in words: what its owner, its group and others
  can do, who can run it, and the mode as `chmod` writes it. Change…
  beside each changes it; on a folder, Also everything inside (`Alt+A`)
  reaches what it holds, giving folders inside what they need to be
  opened and leaving files as runnable as they were. Allow running makes
  a script or an AppImage a program. The group goes to one of yours; the
  owner stays (giving a file away needs an administrator). Undo takes a
  change back. Files that are not yours, in the trash or on a read-only
  drive say why they cannot change.
- Flags the system keeps say why a file resists: Immutable (no one can
  delete it), Append only, Compressed, Encrypted.
- **Opens with** names the application a file's type opens with; Change…
  makes another the default for every file of that type (Undo in the
  message takes it back).
- **Checksums** (folded until you open them): SHA-256, MD5 and BLAKE3,
  each computed when you click Compute. Copy puts one on the clipboard;
  Compare with copied says whether a checksum you copied (from a
  download page, or `sha256sum`'s line) is the same.
- A click on a group's title folds it, for every file from then on;
  Checksums and the drive start folded, the drive's title still saying
  which and how much room is free.
- What a row can change (Change…, Edit…, Copy) shows when the pointer is
  on it; Count, Compute and Add… are always there.
- Over a preview, Properties takes at most half the panel (beside the
  preview when the panel is at the bottom) and scrolls; drag the line
  between them to give either more room, double-click it to share
  evenly again. `Alt+Enter` again gives the room back to the preview.

## The terminal

`F4` opens a terminal at the bottom, in the folder shown. It follows you
as you move, and the pane follows a `cd`. `Shift+F4` opens your desktop's
terminal there instead. `Ctrl+click` a path printed in the terminal to go
to it; `Ctrl+Shift+O` labels the paths for the keyboard;
`Shift+PageUp`/`Shift+PageDown` scroll. `Ctrl+Shift+C` and
`Ctrl+Shift+V` copy and paste (`Ctrl+C` stays the program's), and
`Ctrl+Shift+A` selects everything. Selected text is also the primary
selection, pasted by the middle button and `Shift+Insert`. The right
button or the `Menu` key opens the terminal's menu: copy, paste, a
link's own items, clearing the history. Drop files on the terminal, or
press `Ctrl+Shift+Enter`, to type the selected files' paths at its
prompt, quoted for your shell.

## Keys

Settings › Keyboard lists every action with its keys, by where they
work. Click a key to change or free it, or **+** to add one, then press
the keys (or type them, for those your desktop keeps); before you save,
it says what those keys do now there. **New shortcut…** binds a command
that needs a value, such as a folder to go to; **Changed by you** lists
what you changed, each with a way back to Skwir's keys. A shortcut of
a plugin you turn off waits for it. Settings › Plugins › **Vim keys** (off by default) gives the
file list yazi's keys: `hjkl` to move, `Space` to pick, `v` for a range,
`y`, `x`, `p` to copy, cut and paste, `/` to find, and `g` and `,`
sequences. While it is on, typed letters no longer jump to names. `F1`
lists what applies where you are, and after the first key of a sequence
a hint shows what can follow.

## Settings

`Ctrl+,` opens them. A change applies at once; there is no Save button.

| Page | What it sets |
| --- | --- |
| General | What Skwir does: at startup, with files (copies, drops, companion files), with the split and tabs |
| Appearance | How it looks: theme, icons, dates, tabs, the path, the Narrow by bar, the grid, the list's columns |
| Reading | The reader shared by documents, books, comics, Markdown and code: pages, line length, code's colors |
| Notes | Where tags, ratings and comments are read and written |
| Privacy | Folders that never leave the machine |
| Plugins | Every plugin, on or off, what it may do and what it needs |
| Keyboard | Every action and its keys, changed where they are listed |

A plugin with options of its own (an engine, the terminal's shell) has a
page of its own, listed under the others.

If the settings file cannot be read (edited by hand into something that
is no longer TOML), Skwir starts with its defaults, keeps the file aside
as `settings.toml.unreadable-…`, and says so. Edited while Skwir runs (by
hand, by another Skwir window, by Home Manager), it is taken in at once.
A settings file that cannot be written (Home Manager links it read-only)
is said above every page: a change there is not kept. What you make in
the window (its layout, saved searches, pinned tags, tags' colours) is
kept in files of its own, so it is kept still.
`skwir cmd settings.check '{"file": "/path/to/settings.toml"}'` says what
in a file Skwir would not take.

## What Skwir keeps, and where

| What | Where |
| --- | --- |
| Settings | `~/.config/skwir/settings.toml` |
| The window as you left it: its shape, tabs, the hints shown | `~/.local/state/skwir/state.toml` |
| What you made in Skwir: saved searches, pinned tags, tags' colours and exclusive categories | `~/.local/share/skwir/data.toml`, kept even when the settings file is read-only |
| The cache | `~/.cache/skwir`: delete it whenever you like, Skwir rebuilds what it needs |
| Thumbnails it makes | The desktop's cache (`~/.cache/thumbnails`), shared unless Thumbnails › Share generated thumbnails is off |
| Tags, ratings, comments | On the files (see above) |
| Bookmarks, default applications | The desktop's own files |
| A tile server's key | The desktop's keyring |
| Plugins from outside Skwir | `~/.local/share/skwir/plugins/` |

There is no database and no daemon.

If Skwir stops during a copy (a crash, a power cut), what it had copied
waits in a hidden folder named `.skwir-copy-…` in the destination, never
under the file's own name. Skwir removes it at its next start or next
copy there; on a network share, delete it yourself.

## Agents, scripts and plugins

See the [README](../README.md#scripts-and-agents) for agents and scripts,
and [writing a plugin](plugins/README.md) for plugins from outside Skwir.
