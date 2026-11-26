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

Amended 2026-11-26, on the user's question whether both could be had: a
Skwir running shows the files a launch hands it, the launch waiting in
its terminal; where none runs, the program of its own, as before. A
viewer's program neither takes later viewers nor becomes the file
manager (Rejected). Amended the same day: Print… in a viewer asks
Skwir's own dialog first, as the file manager does (5). Amended the
same day, from a review of the handover: a viewing is taken in two
steps, the launch confirming it (1), and what stands for standard input
is removed only as the caller's own spool (7). Amended the same day,
from a review of the window: what a viewer offers is each command's to
say, as for dialogs, and More's rows, keys and buttons ask it (5); what
it says is drawn over its card (3).

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

1. **`skwir view [FILE…]`, and `skw [FILE…]`: a window of the Skwir
   running, else a program of its own** (amended 2026-11-26).
   - `skw` is a link to the program, installed by the packages. Started
     under that name, it is `skwir view`.
   - Where a Skwir of its instance runs (ADR-122: the same build,
     settings and display), the launch hands it the files, as a launch
     hands it a folder, and that Skwir shows them in a viewer's window.
     The launch waits in its terminal until that window closes, as
     viewers do there. The launch going first (its terminal's Ctrl+C,
     the terminal closed) closes the window.
     - Taken in two steps, so that the launch alone decides whether it
       is (amended 2026-11-26, from a review: a Skwir answering just
       after the launch gave up showed the files a second time, and
       removed the launch's spool under its own window). The Skwir
       answers it taken and shows nothing; the launch confirms it, and
       from then on waits whatever comes; only then is it shown. A
       launch that gave up confirms nothing, and the Skwir lets the
       viewing go unseen once that launch goes, or two seconds after it
       answered.
     - Linux: `Finishing.View` on the bus, answered with the viewing's
       number; then `Viewed`, whose first asking is the confirmation,
       answered once its window closes (asked again should the bus give
       up on a call waiting that long; one waiting as that Skwir stops is
       told the window closed where it did, else that the Skwir ended).
       The launch is followed by its name on the bus.
     - Both are asked by the instance's own name, `org.skwir.skwir.I<key>`,
       and `Viewed` is answered for the launch that asked `View` alone
       (amended 2026-11-26, from a second review). The program's one
       connection also holds the file manager's name (ADR-140), which a
       sandboxed program may be allowed to talk to, and zbus dispatches a
       call by its path whatever name it was addressed to: `Finishing`
       (`Open`, `View`, `Viewed`, `Choose`) refuses a call addressed by
       any other name, so no sandbox opens a server's address there, views
       a file, or asks for files to be chosen.
     - Windows: the pipe's message says it views (`VIEW`), answered
       `TAKEN`; the launch confirms (`CONFIRMED`), and its connection
       stays until the window closes (`CLOSED`). The Skwir looks four
       times a second whether the launch is still there: a synchronous
       pipe's read would hold the write that says the window closed.
       The message's version is 4: both ends are one build.
     - Nothing shown after all (confirmed too late, that Skwir no longer
       taking them, its window refused by the system: `NotTaken`, or
       `NOT_TAKEN` on the pipe), the launch views the files itself, as
       where none runs.
     - One that does not take them within two seconds is given up on,
       connecting to the bus included. Where none runs, it costs a stat
       (Linux) or a pipe not found.
     - The program ends with its last viewer (4): it waits, two seconds
       at most, until each launch whose window closed has read so and
       gone (`handover::wait_told`), else that launch would read that
       its Skwir ended.
   - Where none runs, with `--new-instance`, and on macOS (the system
     brings a running application forward rather than hand it anything,
     ADR-122), it is a process of its own beside any Skwir running. It
     hands nothing over and takes nothing over: no launches, no later
     viewer, no FileManager1, no portal's dialogs (ADR-122, 140, 141).
     It ends once its window closes.
   - That program starts only what viewing needs. Of the plugins, those that show
     files or act on the one shown: each says it (`Plugin::for_viewer`),
     and a test refuses one that registers a preview, a thumbnail or map
     tiles without saying it. Plugins from outside are not looked for.
     Of the program's start, not what the file manager starts: the login
     shell's variables (ADR-125), the collation's table for typing names,
     Settings' row for dialogs, the address field's suggestions, the
     sweep of members extracted by a program killed (which another Skwir
     may be using). It reads on a worker what its window shows: the
     tree-sitter grammars installed, as each language's is chosen
     (ADR-110), and owner names, for Properties (amended 2026-11-26, from
     a review: code was colored by bat's grammars alone there).
   - It never becomes the file manager, whatever asks for the user's
     windows there: its presence or a notice clicked brings its viewer
     forward if it is up, and a Mac's Dock Quit or logging out with its
     window gone ends it, what runs stopped, as a signal does (amended
     2026-11-26, from a review: they opened the user's windows in it).
   - It reads the settings and the state the user's Skwir reads. What it
     saves it writes over the files as they stand, its own keys alone
     (`Settings::persist`), and it never records the user's windows
     (`finishing::record_windows`).
   - A relative path is read against the folder it is started in, by
     the launch. A file that is not there is said on standard error with
     status 1, before any window or handover. The Skwir showing them
     ending before their window closed is said there too, with status 1.
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
     answers the user is said over it, at its foot, off the whole screen
     too (amended 2026-11-26, from a review: only the whole screen drew
     it, and Print's answers, Copy path's, an error went unsaid).
   - Its title is the file's name, and its tag `viewer`, for the
     compositor's rules (ADR-141, 9).
   - It has a size of its own, the last one left, kept apart from the
     user's windows and the dialogs' (`ui.viewer_size`), and saved as it
     closes. It is kept as a dialog's is (ADR-144, 4): the size it was
     given while it floated, or the user dragged it to, never a tile's
     (amended 2026-11-26, from a review: a tile's size was kept, and the
     next viewer, floated, opened at it).
   - Its title bar is the main window's, by `ui.title_bar` (ADR-138):
     the system's, skwir's above or compact, none on the whole screen.
     In the top row, the card's header is that row: the window's buttons
     at its ends, as the desktop orders them (the start's before the
     file's icon, the end's last), its empty parts moving the window. The card has a × of its own only where the system frames the
     window, whose frame may have no Close (Hyprland's); where skwir draws
     the window's Close, it is the one (amended as tried, 2026-11-25: the
     buttons had a row of their own above the card, the start's left out,
     and two × stood at the top right).
4. **Its keys.**
   - Peek's: the arrows step, F goes to the whole screen.
   - Escape, Ctrl+W, Quit (Ctrl+Q), the window's Close and the card's ×
     close the window: whatever ends peek there ends the window
     (`Shell::close_peek`; amended as tried, 2026-11-25, the × having
     closed only the preview). On the whole screen, Escape leaves the
     whole screen first. In a viewer's own program, that ends it, once
     what runs (a print being sent) is finished, as a window's closing
     does (ADR-115).
   - In the Skwir running (amended 2026-11-26), a viewer's Quit closes
     that viewer alone. The user's Quit closes their windows and the
     dialogs' and leaves viewers', which launches wait on: what it stops
     is stopped at once (`Executor::cancel_all`), an action waiting for
     its server to go on by itself included, which then waits for Retry
     alone (amended 2026-11-26, from a review: it went on once its server
     answered, and kept the program), and the program ends with the last
     viewer. A signal ends all, viewers included.
   - In a viewer's own program, Quit's question asked there (a Mac's
     Dock Quit or logging out while a print is sent) closes its window
     whichever way it is answered (amended 2026-11-26, from a review: its
     answers closed nothing).
   - Steps go through the folder, in the order that folder is shown in
     (ADR-132), as peek's do. With several files given, they go through
     those alone, in the order given.
5. **What it offers** (ADR-144's one answer, with a third kind of
   window: `App::in_viewer`, which every way to run a command there asks
   through `Shell::in_window`: keys, menus and More's rows, buttons,
   pickers, prompts, Properties' links, toasts' buttons). Each command
   says it where it is registered (`Declared::in_viewer`,
   `not_in_viewer`), else its topic and effect say it
   (`CommandSpec::in_viewer_by_default`): the previews' commands, what
   plays in them and the window's own parts (its menus, prompts,
   pickers, toasts, buttons) are offered, never what changes files
   (`Effect::Plans`) unless it says so; an item of the preview's menu, a
   picker and a prompt only where what they lead to is
   (`Declared::leads_to`): Reduce Size… is no row of a viewer's More. A
   row, a button or a key the window does not run is not drawn nor
   listed, and a key bound to it does what it would with nothing bound.
   No list in core names a plugin's command (amended 2026-11-26, from a
   review: as first built, core listed the plugins' commands it offered
   beside the topics, Open With's application rows were left out of it
   and did nothing, menus drew rows the window then refused, and F6 sent
   the focus into parts never drawn). So it offers:
   - the previews' commands and what plays in them, but the pinned
     player (pinning, a menu's Play, Keep playing), whose dock a viewer
     does not draw: what plays ends with its preview, as in a dialog's
     window;
   - the window's own, its buttons and Quit, the list of its keys (F1),
     but the parts of a window it does not draw (the pane's menu, the
     sidebar and the parts F6 goes round, Activity, Settings' keys);
   - Open With: its rows in More and its picker;
   - Print…;
     - Print… (Ctrl+P, the preview's More) asks Skwir's own dialog
       first, in the viewer's window, as the file manager does (ADR-111):
       a printer or a PDF, the pages, as shown or as a booklet. In a
       viewer's own program too, which shows no window of the user's: it
       asks wherever the window it was pressed in can ask
       (`App::can_ask`). What is printed is the file shown; a listing is
       not offered, the pane being never drawn (amended 2026-11-26, the
       user having found the system's dialog opened at once there).
     - What it asks of what the user did there (a PDF saved over a name
       taken, short of room) is answered there, Enter and Escape
       included, as in a dialog's window (`ui.review_apply`,
       `ui.review_cancel`; amended 2026-11-26, as tried: the window
       refused both keys, and only its buttons answered), and a plan's
       Cancel too. What it saved is said there, as any plan's answer goes
       to the window whose pane asked it (`ExecEvent::Started::asking`),
       without Undo, which a viewer does not offer (amended 2026-11-26, as
       tried: it was said in the user's window behind, or nowhere).
   - Properties of the file shown, its bar's button, as peek's card has
     it, and copying what it says; a document's links, which open in the
     browser (amended 2026-11-26, from a review: both were refused).
   - Show in Skwir (`ui.viewer_reveal`, a row of its More): a window of
     the user's own on the folder, the file selected; Skwir started on
     its own, in a process group of its own, as the program it is, never
     the `skw` link (amended 2026-11-26, from a review: no row ran it).
   - Nothing that changes files, nor tabs, the split, the terminal,
     selecting by hand (Select all, a pane's cursor keys), going places
     or the palette's other commands, nor a plugin from outside's
     command, which the host cannot tell is fit for it. It views. Its own
     steps move its pane's cursor (`select`, offered there): through the
     folder, on the whole screen, in a slideshow, to the next track; each
     step says so (`Declared::leads_to`), so none is offered where that
     move is not (amended 2026-11-26, from a review: the steps were
     refused once the selection was no longer offered).
6. **No file named.**
   - The system's dialog opens (gpui's `prompt_for_paths`): the portal
     on Linux, which may be Skwir's own (ADR-141), the system's panel on
     macOS and Windows. It takes several files.
   - Then the viewer opens on what was chosen.
   - Cancelled, nothing opens, and a Skwir started for it ends.
7. **`-`, standard input.**
   - The launch reads it, before handing over and once every other
     argument is known good, into a file of Skwir's cache: its own
     spool, `view/<pid>/stdin`, named by the type its bytes have
     (`stdin.png`; the extension the type's first), or `stdin` where
     none (`handover::spool_file`, the one rule both ends use).
   - It reads 1 GiB at most, room for a film. Past that, it stops, says
     so with status 1, and opens nothing: a file cut short would be
     shown as whole.
   - The file is removed once its viewing ends, by whoever is left to:
     the launch, once its wait ends however it does; a viewer's own
     program, as its window closes or it ends; the Skwir showing it only
     where its launch went first (its terminal's Ctrl+C), once the
     window let go, or went before confirming and its process ended (one
     that gave up views the file itself). One a program left by ending
     otherwise is removed at a later start, once a day old and its
     process ended.
   - Handed over, the Skwir may remove it (amended 2026-11-26, from two
     reviews: the path was taken as the caller named it, so a program
     allowed to talk to Skwir on the bus, a sandboxed one allowed
     `org.freedesktop.FileManager1` included, could have any file of the
     user's removed). So `piped` is taken only as the caller's own
     spool: one of the files, exactly `<cache>/view/<pid>/stdin` or
     `stdin.<extension>`, `<pid>` the caller's process as the bus says
     it (`GetConnectionUnixProcessID`) or the pipe
     (`GetNamedPipeClientProcessId`), a regular file in a real folder,
     no link.
     - On Linux, both the user's, and held from the check on (off the
       bus's thread): its folder kept open, opened never through a link,
       and the file removed by its name there, only while it is still
       the file found. A link put in its path meanwhile changes nothing
       (`handover::linux::Spool`).
     - On Windows, where the pipe lets the user's own processes alone
       ask, it is removed by its path: a process able to write skwir's
       cache could have a file named `stdin…` removed where a link it
       put there leads.
     - Anything else refuses the viewing whole (`NotTaken`), rather than
       show it and leave the file; refused, the launch views the files
       itself, its spool kept.
     - So too a launch whose cache is not the running Skwir's (an
       `XDG_CACHE_HOME` a shell sets and the session does not): its
       spool is not where that Skwir looks. The cache is kept out of the
       instance (ADR-122): such a shell's other launches, and its views
       of files named, still go to the Skwir running, rather than start
       a second program each.
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
- **A window of the running Skwir alone** (chosen first, then set aside
  the same day): it opens at once and costs a few MiB, but a preview
  that fails takes the user's windows with it, and a viewer started from
  a terminal would wait there or not depending on whether a Skwir ran.
  Taken back where a Skwir runs (2026-11-26): the launch now waits for
  its window, which answers the second reason; the first is the price.
- **A viewer's program taking later viewers** (2026-11-26): it is a child
  of the terminal that started it, so Ctrl+C there, or closing that
  terminal, would close the windows other terminals wait on. Avoiding it
  means starting every viewer detached, its errors and log reaching no
  terminal, for a gain only while two viewers are up.
- **A viewer's program becoming the file manager as skwir starts**
  (2026-11-26):
  - the file manager would live in the viewer's terminal: that terminal
    waiting until Quit, its Ctrl+C ending a copy midway, its log
    printed there;
  - it would carry that terminal's environment (a `nix develop` PATH, a
    `LANG` set for one command) into all it starts;
  - everything a start does once before its first window would need a
    second, late path, tried both ways;
  - for a gain lasting only while that viewer is open: every later
    viewer goes to the Skwir started.
- **Returning to the terminal at once:** the viewer would have to leave
  its terminal, and its errors with it.
- **Space closing it, as peek's does:** Space plays what plays, and the
  window is not over the user's files.

## Consequences

- A viewer shown by the Skwir running costs a window; one in a program
  of its own costs a process, with what a window costs (the GPU, fonts,
  the catalogs). Measured 2026-11-26 on the `dist` build, on a headless
  sway drawing on the Intel GPU, a text file shown, medians of three
  idle starts (PSS, Skwir's own share):
  - skwir alone, 82.6 MiB (its binary's pages 44.2, anonymous 33.8);
  - a viewer's own program, 90.4 MiB (binary 58.0, anonymous 27.7):
    more than skwir, its start reading code ADR-128's order lays out for
    skwir's start;
  - a viewer handed to skwir, 28.8 MiB more, the launch waiting
    included (binary 22.6, anonymous 5.8, the launch's 5.5 of it);
  - from the launch to the window's first frame (five runs): handed,
    22 to 38 ms (26 the median); its own program, 63 to 130 (85).
  - with a viewer's start laid out too (ADR-128, amended the same
    night): skwir 76.5 MiB, a viewer's own program 73.0, one handed to
    skwir 22.8 more.
- A preview that fails in the Skwir running takes the user's windows
  with it, as it would in its preview panel.
- While viewers' windows alone are up, what runs without the user's
  windows shows no presence, as with dialogs' windows (ADR-141); it
  shows once they close.
- Where code asked whether a window answered a dialog to mean "not the
  user's own", it now asks that (`WindowTabs::is_own`), and a viewer's
  window is not.
