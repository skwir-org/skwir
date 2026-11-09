# Changes

## Unreleased

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
