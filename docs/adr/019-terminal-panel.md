# ADR-019: A terminal panel, and which engine draws it

Status: accepted, 2026-09-26.

## Context

Dolphin's F4 opens a terminal under the view that follows the folder shown,
and a `cd` typed there takes the view along. It is the one Dolphin feature
people name when they try another file manager. bibli had only "Open
Terminal Here" (Shift+F4), which hands the folder to `$TERMINAL`: a window
elsewhere that knows nothing of the pane afterwards.

Principle 4 (harness the environment) says to use the user's terminal. An
embedded one cannot be the user's: Wayland gives no way to embed another
application's window, and the protocols that allowed it on X11 (XEmbed)
are gone. So the panel needs a VT engine of its own, and the question is
which, and what it costs (principle 2).

The engines considered:

- **libghostty-vt** (Ghostty's core, through the `libghostty-vt` crates):
  probably the best core in the long run (paged grid, a key encoder, the
  modern protocols), but at 0.2 its API is declared unstable, and building
  it needs Zig 0.16 on the path and, by default, downloads Ghostty's source
  from `build.rs`. For the flake and CI that means pre-fetching the source
  and Zig's packages, for a panel.
- **alacritty_terminal**: pure Rust, what Alacritty and Zed's terminal run
  on, Apache-2.0. It brings the pty (fork, `setsid`, `SIGCHLD`), an event
  loop and selection. Most of its dependencies were already in the tree.
- `vt100`, `wezterm-term`: smaller or larger, less used in this role.

Zed's own terminal crates (gpui on alacritty_terminal) are GPL; nothing is
taken from them.

## Decision

- **A plugin, `terminal`**, owns the pseudo-terminal and the engine
  (`alacritty_terminal` 0.26). It registers a `TerminalProvider` in
  `App::terminals`, three settings (`terminal.program`,
  `terminal.scrollback`, `terminal.follow`) and F4. Disabling it withdraws
  the provider and hangs up on every shell it started, though the UI may
  still hold a handle (`crates/plugins/terminal/tests/session.rs`).
- **The engine does not leave the plugin.** Core's `terminal` module
  defines what the UI draws and sends, toolkit- and engine-free: a
  `Screen` (runs of styled text placed at their column, cursor, selection
  and its text, history position, title, the shell's folder and a count
  of the user's `cd`s) and `Input` (typed text, keys, paste, size, wheel,
  pages, selection, the folder to follow, the palette). The UI never sees
  an alacritty type, so libghostty-vt can replace the engine by rewriting
  the plugin alone, once it is stable and buildable without a network.
  Core holds this because the plugin and the UI must agree on it, as
  `media::MediaSession` does for the player.
- **Nothing waits on the UI thread.** A session is three threads: the
  engine's event loop reads the pty and parses; the session's thread takes
  the UI's inputs and, after output, builds a `Screen`, publishes it
  (`Arc` swap plus a revision) and wakes the app, at most once per 8 ms, so
  a flood of output costs a frame per refresh, not a frame per read; the
  UI's `sync` only compares a revision and clones an `Arc`. Starting the
  shell (a fork, a working directory on a disk that may sleep) runs on a
  worker (`Shell::off_thread`). Idle, every thread sleeps in the kernel.
- **The pane and the shell follow each other** without shell integration.
  The pane going elsewhere types `\x05\x15 cd '<dir>'\r` (Ctrl+E Ctrl+U
  clears the prompt; the leading space keeps it out of most histories;
  quoted for POSIX shells or fish) only while the shell's process group has
  the terminal (`tcgetpgrp`), so an editor never receives it; a folder
  wanted while a program runs is typed once when the shell has it back.
  After output the plugin reads `/proc/<pid>/cwd`; a change it did not
  type is the user's, and the pane goes there. Unknown shells (nu, xonsh)
  are not typed into; the terminal-to-pane direction works with any shell.
- **Keys.** While the terminal has the keyboard, every key the window binds
  at its root is the shell's: `keymap::install` binds each to `NoAction`
  in the `Terminal` context, so Tab completes and Ctrl+L clears. F4 (the
  panel) and Ctrl+Shift+P (the palette, from which everything else runs)
  still reach the window. Ctrl+Shift+C and Ctrl+Shift+V copy and paste,
  Shift+PgUp/PgDn page through the history. Plain text reaches the shell
  through gpui's input handler, so dead keys, the compose key and input
  methods work; keys without text and Ctrl/Alt chords are encoded by the
  plugin (xterm's encoding, cursor-key mode honoured).
- **Drawing.** The grid is an element (`elements/terminal.rs`) that measures
  its cell (the mono font's advance, 1.25 line height) and the columns and
  rows that fit in `prepaint`, and sends a new size only when it changed.
  ASCII runs are shaped at exactly the cell's width with ligatures off, and
  every run is drawn at its own column, so a bold face or a fallback glyph
  cannot drift off the grid. The sixteen ANSI colours are theme tokens
  (`Theme::terminal`); the rest is RGB the programs chose. The cursor does
  not blink (the only animations allowed end by themselves); it is an
  outline when the terminal does not have the keyboard.
- **Clipboard.** Selecting copies to the primary selection and the middle
  button pastes it, as every Linux terminal does. Programs cannot read or
  write the clipboard (OSC 52 off). Paste is bracketed when the program
  asked, and an end marker inside pasted text is dropped.
- **One terminal per window**, under the panes of the tab shown, following
  the active pane. F4 hides it without ending the shell; the shell exiting
  closes it. The window remembers whether it was open and its height
  (`session.rs`); a restored panel starts its shell on a worker at the
  first sync, never before the first frame.

## Consequences

- Cost (measured, below): with the panel never opened nothing is
  allocated and no thread runs; only the engine's code is in the binary.
- The terminal-to-pane direction reads `/proc`: Linux only, as bibli is.
- The engine sets `ALACRITTY_WINDOW_ID` and `WINDOWID` in the shell's
  environment; `TERM` is `xterm-256color`, `COLORTERM` `truecolor`,
  `TERM_PROGRAM` `bibli`.
- F4 in a program that uses it (Midnight Commander's Edit) closes the panel
  instead; everything else is the program's.
- Not yet: mouse reporting to programs (htop, mc take no clicks; the wheel
  on the alternate screen sends arrows), showing an input method's text
  while it is composed, the kitty keyboard protocol, search in the
  history, links, a font size of its own.

## Addendum: measured with the change (2026-09-26)

Release build, on Xvfb with lavapipe (the NVIDIA driver cannot draw there),
isolated XDG directories, the same 5-entry folder.

- Binary, stripped: 44.95 MB before, 45.45 MB after (+0.50 MB, +1.1%).
- Idle PSS with the panel closed, three fresh starts each, alternated:
  89.9 / 90.0 / 90.5 MB before, 90.2 / 90.6 / 90.3 MB after (+0.2 MB
  on average, within noise); 84 threads in both.
- Opening the panel: +2.3 MB PSS (1.9 MB heap, 0.3 MB code) once the
  renderer has drawn mono text before (a text preview). Opened first in
  a fresh process it was +45 MB, of which 18 MB libLLVM and 14 MB GPU
  buffers: lavapipe compiling and allocating for content it had not drawn
  yet, which a text preview costs the same. Two threads per session.
- After 300 000 lines (`seq`, 0.11 s in the shell): PSS +29 MB, the
  history filling (10 000 lines of 100 columns at 24 bytes a cell is
  24 MB). The UI kept drawing; with lavapipe drawing in software the
  process used about 6.4 cores for two seconds, then returned to 0% CPU.
- Idle with the panel open: 0% CPU (three one-second samples).
