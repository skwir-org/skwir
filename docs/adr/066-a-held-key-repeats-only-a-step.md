# ADR-066: A held key repeats only a step; Space held peeks until let go

Status: accepted, 2026-10-19. Built and checked on Linux (Wayland and
X11); compiled for Windows, where gpui reports a held key as upstream
does; on macOS only the core's part is compiled, as for every change to
the window (ADR-064).

## Context

- Space in the files opened peek (`ui.peek`), Space in peek closed it
  (`ui.peek_close`). gpui runs a binding's action for each of the
  system's repeats of a key held down, so a held Space opened and closed
  peek at the repeat rate. Any switch on a key does the same (F for the
  whole screen, F11 for the panel), since every key went through that
  path.
- gpui knows a repeat (`KeyDownEvent::is_held`): its Wayland client makes
  the repeats itself and marks them, Windows says so (`lParam` bit 30),
  macOS too (`isARepeat`). Its X11 client said `false` for every press.
  An action never sees the event it came from.
- Other file managers, checked on 2026-10-18 and 10-19:
  - GNOME Files 50 with Sushi 50, run on a headless compositor of their
    own, Space held 4 to 6 s. On X11 (Xwayland) the preview flashed: open
    0.6 s, closed 0.6 s, five times in 6 s. On Wayland it opened once and
    stayed, also after release, because Sushi's separate window took the
    focus and GTK repeats a key only in the window that has it. Neither
    program checks for repeats (their source), and GTK gives a shortcut
    no way to tell. Nemo with nemo-preview has the same design (source).
  - PowerToys Peek toggles on every key-down of its shortcut (source, not
    run).
  - QuickLook for Windows ignores Space's repeats, and Space held 750 ms
    or more closes the preview when it is let go (`KeystrokeDispatcher.cs`).
  - Files for Windows runs each shortcut once per physical press: a
    release, or losing the focus, arms it again (`MainPage.xaml.cs`).
  - Finder: Apple documents the toggle alone. One 2018 tutorial shows
    Space held showing Quick Look until it is let go, and its author
    answered a reader whose preview "will open but then will quickly
    close on its own": the reader had held Space too long.

## Decision

1. **A key held down runs again only a step.** A command says when, by
   the arguments it runs with (`CommandSpec::repeats`, a rule declared
   with `Declared::repeats`; `command::always` for a command that only
   steps). Without one, a command runs once a press.
   - Steps: the cursor, a list's, a menu's and a toolbar's arrows, pages,
     scrolling, seeking, the volume, zoom, the map, the settings' arrows,
     the next or previous tab (`tab.select` and `tab.move` by `delta`),
     Back, Forward and Up; picking a file and going to the next.
   - Once a press: switches (peek, the whole screen, a panel, the split,
     play and pause, mute) and acts (Enter, Delete, Rename, Undo, a
     menu's group, another track).
   - The window's root drops a held key's command that does not repeat
     before any part of the window takes it (`Shell::capture_run_command`).
     A character typed in a field and the terminal's keys are not
     commands: they repeat as before.
2. **Space held peeks until it is let go.** A tap opens peek and it
   stays, as before. Held until the system repeats it, peek shows while
   the key is held and closes when it is let go, unless peek was put to
   use meanwhile: a command other than walking the files with the arrows
   (`ui.cursor`), or a click in it (`preview::PeekPress`). The wheel does
   not count: it only looks. The same holds for any key bound to
   `ui.peek`.
   - What makes a press a hold is the system's first repeat, not a
     constant of ours. QuickLook's fixed 750 ms closes the preview of
     someone who presses slowly, as Finder's reader found. The repeat
     delay is the user's own setting, longer for whoever holds keys
     longer, and with repeat turned off Space is the toggle alone.
3. **gpui tells.** Vendored patch 0032:
   - `Window::key_down_event` is the key-down event being dispatched,
     while the actions it resolves to and its listeners run.
   - The X11 client reports a press of the key last pressed, not released
     since, as held: the server repeats only that one. It asks for XKB's
     detectable auto-repeat, as GTK and Qt do, so the server no longer
     sends a release before each repeat.

## Alternatives

- **Space never closes peek** (Escape only). That fixes peek alone: F,
  F11 and every other switch still flip over and over, and in a video's
  peek a held Space still plays and pauses at the repeat rate. It also
  drops the Quick Look habit of closing with the key that opened.
- **Everything repeats but switches** (Qt's and GTK's default). Every
  new command would have to remember to opt out. A held Enter or Delete
  acts again with each repeat.
- **Peek only while held.** Reading a document, playing a film or the
  whole screen do not fit under a held key.
- **A fixed hold time,** as QuickLook does: see 2.

## Consequences

- A held Enter opens once, a held Delete trashes once, a held Ctrl+Z
  undoes once.
- A plugin's command runs once a press unless it declares a rule. A
  plugin from outside cannot declare one yet: its commands run once a
  press.
- `keymap::tests::held_keys_repeat_only_steps` holds that every built-in
  key that moves (an arrow, a page, Home, End) runs a command that
  repeats, but for four that act (a menu's group opened or closed, a
  settings page entered, another track), and that no switch repeats.
- Checked on a headless sway, the app on Wayland and on X11 (Xwayland):
  - a tap opened peek and the next closed it;
  - Space held 3 s showed peek throughout, ran `ui.peek` once, and peek
    closed when Space was let go;
  - Right pressed during the hold showed the next file, and the release
    still closed peek;
  - Right held during the hold walked 20 files on Wayland, 24 on X11,
    peek showing each, and Space's release closed peek;
  - a click in peek during the hold kept it open after the release;
  - Down held 2 s moved the cursor 36 times (the repeat delay, then 25 a
    second);
  - F11 held 2 s switched the panel once.
