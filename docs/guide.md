# Using bibli

This guide is for bibli 0.1 on Linux. Every action is also a command in
the palette (`Ctrl+Shift+P`), which shows its keys, and `F1` lists the
keys that work where the keyboard is.

## The window

- **The sidebar** (`F9` hides it) lists places in groups: your folders,
  the places you kept (bookmarks, searches, tags), Recent, the Trash and
  the drives, mounted or not. Drag a group by its header to move it,
  click the header to fold it; the sidebar's menu shows or hides each
  group. A place can be renamed (`F2`), moved, or hidden; renaming it
  in the sidebar never renames the folder. `F6` gives the sidebar the
  keyboard: the arrows walk it, `Enter` opens a place.
- **Panes and tabs.** Each pane has its own row: Back and Forward, the
  address, and how the pane shows its folder. `F3` splits the window into
  two panes (`Shift+F3` the other way round), `Tab` goes to the other
  one. `Ctrl+T` opens a tab, `Ctrl+W` closes it, `Ctrl+Shift+T` brings
  back the last one closed, `Alt+1`…`Alt+9` go to a tab.
- **More** (the sidebar's button, or the pane's row when the sidebar is
  hidden) holds what has no button of its own, and Layout: where the
  split and the preview go, or Automatic, which follows the window's
  shape.
- **The footer** of each pane says how many items it lists, what is
  selected and what it weighs. The last pane's footer also shows Undo,
  what waits for a paste, plans waiting for your review, and the player.
- **Toasts** say what was done ("Moved “a.txt” to Trash"), with Undo; a
  long plan shows its progress and a Cancel button.

## Moving around

- **The address** shows where you are as a trail. Each `›` lists the
  folders inside what is before it; a click on a segment goes there.
  Click beside the trail, or press `Ctrl+L`, to type a path. `Alt+Up` (or
  `Backspace`) goes to the parent, `Alt+Left` and `Alt+Right` back and
  forward; a right-click on Back or Forward lists the history.
- **Jump** (`Ctrl+J`) goes to a folder you used before, from zoxide's
  list and the desktop's recent files, by a few letters of its name.
- **Type a name's first letters** in the list to go to it. Typed in the
  address instead (`Ctrl+L`), words filter the folder shown at once;
  `Escape` shows everything again.
- `Ctrl+Enter` (or a middle click) opens a folder in a new tab behind the
  one shown. `Ctrl+D` keeps the place shown in the sidebar.

## Views

`Ctrl+1`, `Ctrl+2` and `Ctrl+3` show the folder as a list, a grid or
columns; the View menu at the end of the pane's row has every view, the
sort, hidden files (`Ctrl+H`) and the zoom (`Ctrl++`, `Ctrl+-`, `Ctrl+0`,
or `Ctrl+wheel`).

- **List**: the header's menu chooses the columns; a click on a header
  sorts by it. Where files from many folders are listed (a search, a
  tag, the shelf) a Folder column says where each is, and Show in Folder
  (`Ctrl+Alt+O`) goes there.
- **Grid**: thumbnails and names; Fill thumbnails crops pictures to
  their tiles.
- **Columns**: the folders above, side by side.
- **Tree** opens folders in place; **Board** shows the folders of the
  one shown as columns of cards you can drag between.
- **Usage** (`Ctrl+4`) draws what takes the room in a folder, counted as
  you look; `Ctrl+R` counts again.
- **Map** (`Ctrl+5`) places pictures, tracks and plans where they were
  taken or go. Its background is drawn from data bibli carries; a
  background that downloads tiles is used only once you choose one.
- **Timeline** (`Ctrl+Shift+L`) shows when the files were changed, a
  bar per period; a click on a bar searches it.

## Selecting and acting

- A click or an arrow selects; `Shift` extends, `Ctrl+click` (or
  `Ctrl+Space`) adds or removes one, `Ctrl+A` selects everything; drag
  from an empty spot for a rectangle. The menu key or `Shift+F10` opens
  the menu, as a right-click does.
- **Copy, cut, paste**: `Ctrl+C`, `Ctrl+X`, `Ctrl+V`. What waits for a
  paste is dimmed, and the footer says how many; `Escape` forgets it.
  Dragging moves or copies as the desktop does, and on Wayland into
  other applications too. `Shift+F5` copies the selection to the other pane, `Shift+F6`
  moves it there.
- **The shelf** (`Ctrl+Shift+B` adds to it) gathers files from anywhere
  to move or copy them together later.
- **Rename** with `F2`; with several files selected, `F2` (or `Ctrl+F2`)
  renames them together, with a preview of every new name. `Ctrl+E`
  opens the folder's names in your editor (`$VISUAL` or `$EDITOR`):
  what you change there, rename, move or delete, comes back as a plan.
- **New folder**: `Ctrl+Shift+N`. **Open with** another application:
  `Shift+Enter`. **Copy paths**: `Ctrl+Shift+C`.
- **Delete** moves to the Trash; `Shift+Delete` deletes for good, after
  asking, since that cannot be undone. The Trash is a place in the
  sidebar: Restore puts a file back where it was, a file dragged out of
  it is restored where you drop it.
- **Undo and Redo**: `Ctrl+Z`, `Ctrl+Shift+Z` (or `Ctrl+Y`), for every
  change bibli made, in order. The journal lasts while bibli runs.
- **Plans**: a change of many files, and anything an agent or a plugin
  proposes, is shown as steps to review first. `Enter` applies it,
  `Escape` dismisses it.

## Searching

`Ctrl+F` starts a search in the folder shown; a search is a place, with
Back, tabs and the sidebar like any folder. Its address shows what it
looks for as pills: a click on one changes it, **+ Filter** adds one,
and its text can be typed as is:

| Text | Finds |
| --- | --- |
| `report` | names with "report" |
| `*.pdf`, `ext:pdf` | by extension |
| `kind:image,video`, `mime:application/pdf` | by kind or type |
| `type:dir`, `type:file` | folders or files alone |
| `size>10M`, `size<1.5G` | by size |
| `modified>=2026-01-01`, `modified<7d`, `modified:this-week` | by date |
| `tag:Places/France`, `-tag:draft` | by tag, or without one |
| `rating>=3` | by rating |
| `content:"it's here"` | files that hold the text |
| `in:"~/My Files"` | under another folder |
| `a OR b`, `NOT a`, `(a b)` | either, not, together |

`Alt+Enter` searches the home folder instead. `Ctrl+D` keeps a search in
the sidebar. Over a search with tags, a bar offers what it can narrow to
(the tags within, the files' kinds and years), each with how many files.
A search uses the desktop's indexes where they run (Baloo, plocate) to
go faster, and always checks the files themselves.

## Tags, ratings and comments

`Ctrl+Shift+G` edits the tags of the selection; Details (`Ctrl+I`, or
`Alt+Enter` on a file) shows them with the rating and the comment.

- **Where they are kept.** bibli keeps notes where other applications
  keep them: the file's extended attributes (as Dolphin does), XMP
  sidecars (`photo.jpg.xmp`, as darktable and Lightroom do), a Mac's tags
  and its `._` files on drives that keep no attributes, TagSpaces'
  sidecars, and tags in names (`name[a b].ext`) where you use TagSpaces.
  Settings › Notes says which to read and which to write. When two
  places disagree, the latest change wins and Details says so.
- **Categories.** A tag named `Places/France/Paris` is within
  `Places/France` and `Places`: a search for `Places` finds it. A
  category can hold one tag at a time (Status: Draft, Done).
- **Tags are places.** `tags:/` lists every tag in your home folder, each
  a folder of the files it holds; drop files on a tag to tag them, and
  pin the tags you use in the sidebar. `Alt+G` shows the tags of the
  folder you are in (also first in the menu of the trail's last chevron,
  and All Tags with `Alt`).
- **Copies keep them.** A copy keeps the tags, the rating, the comment
  and where a download came from, onto a drive that keeps no attributes
  too (Settings › General). A move keeps them always.

## Previews

`Space` peeks at a file; `Ctrl+Shift+Space` (or `F11`) opens the preview
panel, which follows the selection. Each kind of file has its tools in
the panel's bar:

- **Text and code**, with its language's colours; **Markdown**, laid
  out, its source a click away; **tables** (CSV, TSV, JSON lines);
  structured files (JSON, YAML, TOML, XML) folded and unfolded.
- **Documents**: PDFs in bibli's reader (continuous or a page at a time,
  zoom, search, contents, text selected and copied); office documents
  through LibreOffice, converted in isolation; EPUB books, comics,
  e-mails, Jupyter notebooks.
- **Pictures, audio and video** with the player (pinned, it keeps
  playing while you move on); fonts with a sample; 3D models; circuit
  boards; maps and tracks; the files inside an archive.
- **Bytes** of any file (View as hex).

A link in a document opens with a click; an address found in plain
text, with `Ctrl+click`. `Ctrl+Shift+O` labels the links in view for the
keyboard. Some kinds need a program bibli does not carry; Settings ›
Plugins says which, and what to install.

## The terminal

`F4` opens a terminal at the bottom, in the folder shown, and follows
you as you move (and the pane follows a `cd`). `Shift+F4` opens your
desktop's terminal there instead. A path printed in the terminal is a
link with `Ctrl+click`.

## Settings

`Ctrl+,` opens them; a change applies at once, with no Save button.
General is what bibli does with files, Appearance how it looks,
Keyboard the keys (record your own over any default, or choose a scheme
such as vim's), Reading the reader shared by documents, books and
Markdown, Notes where notes are kept, Privacy the folders that never
leave this machine, and Plugins every plugin, on or off, with what it
may do and what it needs.

The settings are a file, `~/.config/bibli/settings.toml`. If it cannot
be read (edited by hand into something that is no longer TOML), bibli
starts with its defaults, keeps the file aside as
`settings.toml.unreadable-…`, and says so.

## What bibli keeps, and where

- **Settings**: `~/.config/bibli/settings.toml`, the window's shape among
  them.
- **The cache**: `~/.cache/bibli`. Delete it whenever you like: bibli
  makes again what it needs.
- **Nothing else.** No database, no daemon: tags and notes are on the
  files, bookmarks are the desktop's, thumbnails the desktop's cache.

If bibli stops during a copy, the part already copied stays in a hidden
folder named `.bibli-copy-…` in the destination, never under the file's
own name; delete it, and copy again.

## Agents, scripts and plugins

See the [README](../README.md#scripts-and-agents) for agents and scripts,
and [writing a plugin](plugins/README.md) for plugins from outside bibli.
