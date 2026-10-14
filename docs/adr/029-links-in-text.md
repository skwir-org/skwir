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
