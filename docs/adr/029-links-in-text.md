# ADR-029: Links found in text, and in the terminal

Status: accepted, 2026-10-03. Built.

## Context

Links written in documents (Markdown, HTML, EPUB, HTML mail) were
followed (`docs/invariants/previews.md`). Elsewhere addresses and paths
were dead text: the code view, a README's bare `https://…`
(pulldown-cmark has no GFM autolinks), plain-text mail, and the
terminal (ADR-019), where `ls`, compilers, `grep` and `git` print the
files Skwir could show. Programs' own terminal links (OSC 8: `ls
--hyperlink`, `rg`, gcc, systemd) were kept by the engine and dropped
by the screen.

Two questions closed doors: which gesture follows a link nobody wrote,
and what following a path does when the terminal follows the pane.

## Decision

### What is found

- **Addresses by scheme**, only those Skwir follows: `http`, `https`,
  `mailto`, `file` (`skwir_platform::links::urls`). A dotted name is
  never a domain: `main.rs` and `README.md` are files.
- **Terminal paths**, with line and column (`src/main.rs:12:5`); only
  the filesystem decides (`links::resolve`, on a worker).

### Written links answer a click, found ones Ctrl+click

- In a document read as a page (Markdown, plain-text mail), a found
  address is a link like a written one.
- In raw text (code view, terminal), clicks select. A found link is
  drawn and followed only while Ctrl (Cmd on a Mac) is held over it
  (`Run::found`), as in GNOME Terminal, Konsole, Windows Terminal,
  iTerm2, VS Code and Zed. Shift added opens a file in a new tab.
- Ctrl+Shift+O labels the links in view; typing a label follows it (a
  capital for a tab), as foot's and kitty's hints do.

### What following does

- An address goes to the desktop's browser or mail client through one
  gate (`ui.open_url`, `open::open_address`, on a worker). Skwir
  fetches nothing (ADR-020).
- A file or folder is shown in Skwir (`ui.reveal`): a folder entered, a
  file selected, a line number shown in the preview.
- A link is never opened with an application: printing a hostile file
  to the terminal cannot make a click run anything.

### A terminal path is read where it was printed

Following a path moves the pane and the terminal follows (one rule, no
hidden disagreement), so the next relative path of the same compiler
message would resolve from the wrong folder. The plugin therefore
resolves a path from the shell's folder when its line was first shown
and stamps the result into the grid as an engine link
(`Cell::set_hyperlink`, an id no program can write), kept through
scrolling, history and reflow.

- A path with a folder in it is also looked for above that folder
  (cargo prints paths from the workspace root).
- Lines that scroll past unseen are read from the folder of when they
  are first shown.
- No paths are found inside full-screen programs.

## Consequences

- `Screen` carries `links` (core data, no engine type); the UI
  hit-tests them.
- The terminal plugin depends on `skwir-platform`. Stamps are bounded
  (4096), and paths awaiting the filesystem (1024); a flood waits for a
  later screen. Stamps share allocations, so a long history costs no
  more than its links.
- A program-written link shows its target in a tooltip under Ctrl,
  since its text may say otherwise.
- Not covered: addresses in table cells and Details comments; lines
  beyond the first page of a long text file; paths in full-screen
  programs; programs that change directory themselves (`make -C`).

## Amendments

### 2026-10-14: a path is looked up only where that stays here

Looking a path up can reach another machine: on Windows `\\host\share`
connects to that host and offers it the user's credentials; below an
automount point (macOS's `/net` and `/home` by default, Linux's autofs
`-hosts` maps) a lookup mounts what the name says. A file printed in the
terminal, or previewed, would make Skwir reach a host it names, which
ADR-020 rules out.

- `links::resolve` looks a path up only where `fs::volume::Here::holds`
  allows: no server or device named by the path (a Windows prefix other
  than a drive), but the share the base folder is on; nothing below an
  automount point that nothing is mounted on yet. The point itself is
  read: `stat` mounts nothing for the name it ends with.
- The mount table is read, never a path: `/proc/self/mountinfo` on
  Linux, `getfsstat` on macOS (compiled only). A table that cannot be
  read leaves only what is below the base folder.
- The terminal reads it once for what a screen asks at once.
- Mounts the user made (sshfs, a mapped drive, `/net/host` once
  mounted) are looked up as listing them is, and so is a link on the
  disk: only the path as written is read.

### 2026-10-14: a link lands where its fragment points, and Back returns

A link to `guide.md#setup` showed the guide from its start, and Back
after it went to the same folder with nothing selected: the document the
link was followed from was lost.

- A fragment is a mark in the file it leads to (`links::Mark`): the
  block holding that `id` or heading slug once its preview arrives, else
  the line code hosts write (`#L12`). A mark the file does not hold is
  said, and the file shown from its start. A link to the document itself
  scrolls in it.
- Each step of a pane's history keeps the entry the cursor was on when
  the pane left it, and Back and Forward select it again: Back after a
  link is the document again. A link to a file beside the one shown adds
  a step without listing the folder again (`App::step_here`).
- The document comes back from its start, not where it was scrolled.

### 2026-10-14: paths a file's text writes

In a file's source (HTML, Markdown, code) and in plain text, the paths
the text writes were dead text: only addresses by scheme were found.

- What source and markup write as a path is looked for
  (`links::written`): what quotes hold, a Markdown link's target, what
  `url(…)` holds, and a word with a slash after a name (`docs/guide.md`,
  `src/main.rs:12`). A bare word is not: in code most are names, and each
  would be a lookup.
- Only the filesystem confirms one, read from the file's folder as a
  terminal path is from the shell's (`resolve`: the folders above for a
  path with a folder in it), on this machine only. A word escaped as a
  URL (`my%20file.md`) is looked for decoded too.
- A found path is a link under Ctrl, to its file's `file://` address
  with the line or the mark it names (`#L12`, `#setup`), followed as a
  document's links are.
- The text never waits for the lookups: they run as a chunk of a text
  read where it is shown is colored, after its plain rows showed. Each
  word is looked up once for the file, and at most 256 new words per
  chunk of 256 rows: a list of thousands of names links what those found.
  Measured here, a lookup took 1 to 2 µs on a warm local disk.
- A member written out of its container finds none: what is beside it
  on the disk is not what was beside it there.
- Not covered: the code views made whole (structured data with folds,
  a book's chapter source); bare names (`see guide.md`).

A document's own links are followed only where that stays on this
machine from its folder too (`RevealAt::from`): on Windows a link to
`//host/share` connected on a click. `file:///` links are read as the
system writes them (`file:///C:/a%20b` on Windows was `/C:/a b`).
