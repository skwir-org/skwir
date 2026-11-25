# ADR-151: A file is viewed in a window of its own

Status: accepted 2026-11-25, on the user's answers. They asked for "a
small skeleton app shipped with skwir but independent from it": given a
file, or with none the file chooser, it shows the file's preview as
Skwir does, for people previewing from a terminal. They asked whether a
separate program could load only what viewing needs. The two were
measured. They first chose a window of the running Skwir, then, the
same day, a program of its own that starts only what viewing needs:
`skwir view` and a link `skw`. One file steps through its folder. With
no argument it opens the system's dialog. An Open With entry and
standard input come with it. Built on Linux the same day and checked
in a window on Xvfb; macOS and Windows compiled only.

## Context

- **The preview is the window's.** What draws a file is about 25
  modules written as the window's own methods (`preview.rs` alone
  10,800 lines: the reader, the inspector, the player, the whole
  screen, code, archives, maps, 3D). They use its menus, toasts, keys,
  commands and pane. What the preview shows is a pane's cursor. Peek
  (Space, ADR-066) already shows it over the window, a card holding the
  file's bar and its preview. Its arrows step through the pane's order,
  and F goes to the whole screen.
- **A separate program, measured** (2026-11-25, `target/dist/skwir`
  after 0.7.0):
  - The binary is 167 MB, 38 MB of it symbols: about 129 MB stripped.
  - By its symbols, 15 to 25 % is the file manager's alone: the sidebar,
    Settings, the remote, search, tags and terminal plugins, the agents'
    channel, printing and shrinking.
  - The rest is what any window showing previews needs: gpui and wgpu,
    the session bus (accessibility, the desktop's colors, the portal's
    dialog), the picture codecs, fonts, every language's catalog, core,
    the preview and its plugins.
  - A viewer of its own would therefore be about 100 MB beside skwir,
    sharing nothing on disk: Rust links each program whole. Rust's
    dylibs have no stable interface, ship libstd as a library of its
    own, and lose LTO and ADR-128's order.
  - Memory follows what runs, not the file's size (ADR-128). Code a
    viewer never runs is never read, whichever binary holds it.
  - Pulling the preview out of the window first would be a refactor of
    most of the UI.
- **Elsewhere:**
  - macOS has Quick Look (`qlmanage -p`).
  - GNOME has Sushi, Nautilus's Space, behind a D-Bus name.
  - KDE has Dolphin's panel and no viewer of its own.
  - Windows has PowerToys' Peek.
  - Terminal users have a viewer per kind: imv and feh, zathura, mpv.
    They wait in the terminal while their window is open.

## Decision

1. **`skwir view [FILE…]`, and `skw [FILE…]`: a program of its own.**
   - `skw` is a link to the program, installed by the packages. Started
     under that name, it is `skwir view`.
   - It is always a process of its own, beside any Skwir running: it
     hands nothing over and takes nothing over (no launches, no
     FileManager1, no portal's dialogs: ADR-122, 140, 141). It waits in
     its terminal while its window is open, as viewers do there, and
     ends once that window closes.
   - It starts only what viewing needs. Of the plugins, those that show
     files or act on the one shown: each says it (`Plugin::for_viewer`),
     and a test refuses one that registers a preview, a thumbnail or map
     tiles without saying it. Plugins from outside are not looked for.
     Of the program's start, not what the file manager starts: the login
     shell's variables (ADR-125), the collation's table for typing names,
     Settings' row for dialogs, the address field's suggestions, owner
     names, the sweep of members extracted by a program killed (which
     another Skwir may be using).
   - It reads the settings and the state the user's Skwir reads. What it
     saves it writes over the files as they stand, its own keys alone
     (`Settings::persist`), and it never records the user's windows
     (`finishing::record_windows`).
   - A relative path is read against the folder it is started in. A file
     that is not there is said on standard error with status 1, before
     any window.
2. **A viewer's window is the program's, not the user's own.**
   - Its kind is a window's kind in the core (`WindowKind::Viewer`), as
     a dialog's is (`WindowKind::Chooser`).
   - It is never kept for the next start, nor reopened, nor where a
     launch or an agent's command goes. It is not listed as a window of
     the user's (`window.list`).
   - It has one tab, with one pane on the file's folder that is never
     drawn, its cursor on the file. So the preview is what it is
     everywhere: the provider the type finds, the folder's watch (a file
     rewritten is shown again, so a note beside its editor follows each
     save), what is read ahead.
3. **What it draws.**
   - Peek's card, filling the window: the file's bar (its name, its ways
     of looking, Full preview, More) and the preview under it.
   - No sidebar, toolbar, tabs, panes, terminal or Activity panel. What
     answers the user is said over it.
   - Its title is the file's name, and its tag `viewer`, for the
     compositor's rules (ADR-141, 9).
   - It has a size of its own, the last one left, kept apart from the
     user's windows and the dialogs' (`ui.viewer_session`).
4. **Its keys.**
   - Peek's: the arrows step, F goes to the whole screen.
   - Escape, Ctrl+W, Quit (Ctrl+Q) and the card's × close the window,
     ending the program: whatever ends peek there ends the window
     (`Shell::close_peek`; amended as tried, 2026-11-25, the × having
     closed only the preview). On the whole screen, Escape leaves the
     whole screen first. Quit asks first while something runs (a print
     being sent), as anywhere (ADR-115).
   - Steps go through the folder, in the order that folder is shown in
     (ADR-132), as peek's do. With several files given, they go through
     those alone, in the order given.
5. **What it offers** (ADR-144's one answer, with a third kind of
   window):
   - the previews' commands and what plays in them;
   - the window's own, its buttons and Quit;
   - Open With, Print…;
   - Show in Skwir (`ui.viewer_reveal`): a window of the user's own on
     the folder, the file selected.
   - Nothing that changes files, nor tabs, the split, the terminal or
     the palette's other commands. It views.
6. **No file named.**
   - The system's dialog opens (gpui's `prompt_for_paths`): the portal
     on Linux, which may be Skwir's own (ADR-141), the system's panel on
     macOS and Windows. It takes several files.
   - Then the viewer opens on what was chosen.
   - Cancelled, nothing opens, and a Skwir started for it ends.
7. **`-`, standard input.**
   - The launch reads it, before handing over, into a file of Skwir's
     cache (`view/`), named by the type its bytes have (`stdin.png`; the
     extension the type's first), or `stdin` where none.
   - It reads 1 GiB at most, room for a film. Past that, it stops, says
     so with status 1, and opens nothing: a file cut short would be
     shown as whole.
   - The file is removed once the window showing it closes. One a
     program left by ending otherwise is removed at a later start, once
     a day old.
8. **Open With.**
   - A desktop entry of its own, "Skwir Preview", runs `skwir view %F`.
   - Its types are the kinds Skwir previews, as the desktop's database
     groups them (`mime::Kind`). They are listed from the database and
     checked against it, never written by hand.
   - It is translated as the program's entry is, and kept out of the
     launcher's list (`NoDisplay`): it opens what it is given.

## Rejected

- **A separate binary:** about 100 MB beside skwir for a few MiB less
  of code read, and the preview pulled out of the window first.
- **A window of the running Skwir** (chosen first, then set aside the
  same day): it opens at once and costs a few MiB, but a preview that
  fails takes the user's windows with it, and a viewer started from a
  terminal would wait there or not depending on whether a Skwir ran.
- **Returning to the terminal at once:** the viewer would have to leave
  its terminal, and its errors with it.
- **Space closing it, as peek's does:** Space plays what plays, and the
  window is not over the user's files.

## Consequences

- Each viewer is a process, with what a window costs (the GPU, fonts,
  the catalogs): measured once built, against the user's Skwir.
- Where code asked whether a window answered a dialog to mean "not the
  user's own", it now asks that (`WindowTabs::is_own`), and a viewer's
  window is not.
