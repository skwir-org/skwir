# ADR-019: A terminal panel, and which engine draws it

Status: accepted, 2026-09-26. Built on Linux; compiled only for macOS and
Windows (on Windows the pane and the shell do not follow each other:
`docs/PORTING.md`, section 9).

## Context

Dolphin's F4 terminal follows the folder shown, and a `cd` typed there
takes the view along; people miss it elsewhere. Skwir only had "Open
Terminal Here" (Shift+F4), handing the folder to `$TERMINAL`, which then
knows nothing of the pane.

Principle 4 says use the user's terminal, but it cannot be embedded (no
way on Wayland; X11's XEmbed is gone). So the panel needs its own VT
engine, within principle 2's budget:

- **libghostty-vt:** likely the best core long-term, but its 0.2 API is
  unstable, and building needs Zig 0.16 and downloads Ghostty's source in
  `build.rs`. Not now.
- **`alacritty_terminal`: chosen.** Pure Rust, Apache-2.0, behind Alacritty
  and Zed's terminal; brings the pty, an event loop and selection; most
  dependencies already in the tree.
- `vt100`, `wezterm-term`: less used in this role.

Zed's terminal crates are GPL; nothing is taken from them.

## Decision

- **The `terminal` plugin** owns the pty and the engine
  (`alacritty_terminal` 0.26). It registers a `TerminalProvider` in
  `App::terminals`, `terminal.program`, `terminal.scrollback`,
  `terminal.follow` and F4. Disabling it hangs up every shell it started,
  even if the UI still holds a handle
  (`crates/plugins/terminal/tests/session.rs`).
- **The engine never leaves the plugin.** Core's `terminal` module defines
  what the UI draws (`Screen`) and sends (`Input`), free of toolkit and
  engine types, so another engine (libghostty-vt, once stable and buildable
  offline) replaces only the plugin. Core holds it because plugin and UI
  must agree, like `media::MediaSession`.
- **Nothing waits on the UI thread.** The engine's thread reads and parses
  the pty; the session thread takes inputs and publishes a `Screen` (`Arc`
  swap plus revision), waking the app at most every 8 ms, so a flood costs
  a frame per refresh. The UI's `sync` compares a revision. The shell
  starts on a worker (`Shell::off_thread`): a fork, and a working directory
  on a disk that may sleep. Idle, every thread sleeps.
- **Pane and shell follow each other** without shell integration:
  - Pane to shell: the plugin types `\x05\x15 cd '<dir>'\r` (clear the
    prompt; a leading space keeps it out of most histories; quoted for the
    shell's family) only while the shell's process group has the terminal
    (`tcgetpgrp`), so an editor never gets it; a folder asked for meanwhile
    is typed when the shell is back. Unknown shells (nu, xonsh) are never
    typed into.
  - Shell to pane: after output the plugin reads the shell's working
    directory; a change it did not type is the user's, and the pane
    follows.
- **Keys.** With the keyboard in the terminal, every root binding is the
  shell's (`NoAction` in the `Terminal` context): Tab completes, Ctrl+L
  clears. Only F4 and Ctrl+Shift+P (the palette) reach the window.
  Ctrl+Shift+C / V copy and paste; Shift+PgUp / PgDn page the history.
  Text goes through gpui's input handler (dead keys, compose, input
  methods); other keys are xterm-encoded by the plugin.
- **Drawing.** The grid element (`elements/terminal.rs`) measures cell and
  grid in `prepaint` and sends a size only when it changes. ASCII runs are
  shaped at exactly the cell width, ligatures off, each run at its own
  column, so bold or fallback glyphs cannot drift. ANSI colours are theme
  tokens (`Theme::terminal`). The cursor does not blink (animations must
  end by themselves); unfocused, it is an outline.
- **Clipboard.** Selection copies to the primary selection; the middle
  button pastes. Programs cannot touch the clipboard (OSC 52 off). Paste
  is bracketed when asked; an end marker inside pasted text is dropped.
- **One terminal per window,** under the panes of the tab shown, following
  the active pane. F4 hides it without ending the shell; the shell exiting
  closes it. Open state and height are remembered (`session.rs`); a
  restored panel starts its shell at the first sync, never before the
  first frame.

## Consequences

- Never opened, the panel allocates nothing and runs no thread.
- The shell's folder comes from `process::working_dir`: `/proc/<pid>/cwd`
  on Linux, `proc_pidinfo` on macOS (compiled only), nothing on Windows
  (`docs/PORTING.md`, section 9).
- Environment: `TERM=xterm-256color`, `COLORTERM=truecolor`,
  `TERM_PROGRAM=skwir`; the engine adds `ALACRITTY_WINDOW_ID`, `WINDOWID`.
- F4 inside a program that uses it (mc's Edit) closes the panel instead.
- Not yet: mouse reporting (htop, mc take no clicks; the wheel on the
  alternate screen sends arrows), showing input-method text while
  composed, the kitty keyboard protocol, history search, a font size of
  its own. Links came with ADR-029.

## Amendments

### 2026-09-26: measured

Release build, Xvfb with lavapipe, isolated XDG directories, a 5-entry
folder.

| Measure                                | Result                                                       |
| -------------------------------------- | ------------------------------------------------------------ |
| Stripped binary                        | 44.95 → 45.45 MB (+1.1 %)                                     |
| Idle PSS, panel closed (3 starts each) | 89.9/90.0/90.5 → 90.2/90.6/90.3 MB (noise); 84 threads       |
| Opening the panel                      | +2.3 MB PSS (1.9 MB heap); 2 threads per session             |
| Opening it first in a fresh process    | +45 MB, mostly lavapipe's first mono text (18 MB libLLVM, 14 MB GPU buffers), as for a text preview |
| 300 000 lines of `seq`                 | +29 MB PSS (history: 10 000 × 100 cells × 24 B); ~6.4 cores for 2 s under lavapipe, then 0 % |
| Idle, panel open                       | 0 % CPU                                                      |

### 2026-10-14: the copy and paste keys

The decision named Ctrl+Shift+C / V, but nothing bound them: only the
palette ran `ui.terminal_copy` and `ui.terminal_paste`. Ctrl+Shift+C
reached the program as Ctrl+C (`0x03`): copying interrupted it.
Ctrl+Shift+V arrived as Ctrl+V.

- Both are bound in the `Terminal` context. `keymap::install` binds the
  window's shadowed keys before every other binding, so a key bound in
  the terminal wins over the same keys bound to the window in any layer
  (a user's Ctrl+Shift+C for the whole window had silenced it again). A
  test reads the bindings as GPUI does.
- Shift+Insert pastes what the middle button pastes, the primary
  selection, on Linux (`ui.terminal_paste` with `from: primary`), as
  xterm, Konsole, foot, kitty and Alacritty do. On Windows Ctrl+Insert
  and Shift+Insert copy and paste the clipboard, as Windows Terminal
  does; on macOS Cmd+C and V. Both are compiled only.

### 2026-10-14: the terminal's menu

The right button (or the Menu key) opens a menu: Copy (when something
is selected), Paste, Select all (Ctrl+Shift+A), and Clear the history
when there is one. On a link, the items a Ctrl+click and its tooltip
gave: Follow link (saying where it leads when a program wrote it), in a
new tab for a path, and Copy link address or Copy path.

- Select all and Clear the history are the session's
  (`Input::SelectAll`, `Input::ClearHistory`). Everything selected ends
  at the last line written or the cursor, not at the blank rows under
  the prompt. The history is cleared as a program's `CSI 3 J` clears it:
  the screen stays, a selection in the history goes.
- From the keyboard the menu stands above the cursor's cell, which the
  grid marks in its prepaint, or above the panel when no cursor is
  drawn: the terminal is at the window's foot.
- Shift+F10, the other key of a menu, stays the program's, as the other
  F-keys are.
- An agent may open the menu; selecting everything (the desktop's
  primary selection on Linux) and clearing the history ask first, as
  copying does.
