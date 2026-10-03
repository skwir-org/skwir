# ADR-029: Links found in text, and in the terminal

Status: accepted, 2026-10-03.

## Context

A document's links were already followed: Markdown, HTML reading mode,
EPUB and HTML mail carry the links their authors wrote, and a click goes
where each says (`docs/invariants/previews.md`). Everything else showed
addresses and paths as dead text: source code and logs in the code view,
a README's bare `https://…` (pulldown-cmark has no GFM autolinks), a
plain-text mail, and the terminal panel (ADR-019), where `ls`, compilers,
`grep` and `git` print the very files a file manager could show. Programs
also write links of their own into a terminal (OSC 8: `ls --hyperlink`,
`rg --hyperlink-format`, gcc, systemd), which the engine kept and the
screen dropped.

Two questions closed doors: what gesture follows a link that nobody wrote
as one, and what following a path does when the terminal under the panes
follows the pane (ADR-019).

## Decision

### What is found

Addresses are found by their scheme, and only the schemes bibli follows:
`http`, `https`, `mailto`, `file` (`bibli_platform::links::urls`). A name
with a dot is never taken for a domain: `main.rs`, `setup.py` and
`README.md` are files, though `.rs`, `.py` and `.md` are countries'
domains. In the terminal the words of a line may also be paths, with the
line and column a message points at (`src/main.rs:12:5`); only the
filesystem says whether one names a file (`links::resolve`, on a worker).

### The gesture: written links answer a click, found ones Ctrl+click

In a document read as a page (Markdown, a plain-text mail) a found address
is a link like the written ones, as GitHub and mail readers make them. In
raw text (the code view, the terminal) a click and a double click select:
a link found there is drawn as one, and followed, only while Ctrl (Cmd on
a Mac, gpui's `secondary`) is held over it (`Run::found`), as GNOME
Terminal, Konsole, Windows Terminal, iTerm2, VS Code and Zed do. Shift
added shows a file's link in a tab of its own, in front. The keyboard has
the same reach: Ctrl+Shift+O labels the links in view, in the terminal
and in a preview, and typing a label follows its link (a capital for a
tab), as foot's and kitty's hints do.

### What following does

An address goes to the desktop's browser or mail client through one gate
(`ui.open_url`, `open::open_address`, on a worker): nothing else a file
names reaches the desktop's handlers. bibli fetches nothing (ADR-020).
A file or a folder is shown in bibli (`ui.reveal`): a folder is entered,
a file is selected in its folder, and a line number shows that line in
the preview, opening the panel if it is closed. A link is never opened
with an application: printing a hostile file to the terminal cannot make
a click run anything.

### The terminal follows the pane, so a path is read where it was printed

Following a path moves the pane, and the terminal follows the pane, so
the shell changes directory too: one rule, no exception, no hidden state
where they disagree. The cost is that the next relative path of the same
compiler message would be read from the wrong folder. So the plugin reads
a path from the folder the shell was in when its line was first shown,
and stamps the answer into the grid as a link of the engine's own
(`Cell::set_hyperlink`, an id no program can write): the grid keeps it
through scrolling, the history and reflow, and loses it with its text.
A path with a folder in it is also looked for above that folder, as cargo
writes paths from its workspace's root. Lines that scroll past between
two screens are never shown, so never stamped then; shown later, they are
read from the folder of that later moment. Paths are found outside
full-screen programs only.

## Consequences

- `Screen` carries `links` (core data, no engine type); the UI hit-tests
  them with the frame's geometry and draws the one under the pointer.
- The terminal plugin depends on `bibli-platform` for the finder. Its
  stamps are bounded (4096), and the paths waiting for the filesystem too
  (1024): in a flood of output, what does not fit waits for a later screen.
  A stamp is one allocation its cells share, as a program's link is; a
  word that names nothing wears one marker shared by all such words, so
  a long history costs no more than its links.
- A link a program wrote shows where it leads in a tooltip while Ctrl is
  held over it, since its text may say otherwise.
- Not covered: table cells and Details comments find no addresses; a line
  beyond the first page of a long text file is not shown; links in a
  full-screen program's paths; a program that changes directory itself
  (`make -C`) prints paths read from the shell's folder.
