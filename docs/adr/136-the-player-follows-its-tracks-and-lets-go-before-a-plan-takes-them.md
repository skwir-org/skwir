# ADR-136: The player follows its tracks, and lets go before a plan takes them

Status: accepted, 2026-11-14, on the user's answer (asked what the player
should do when the file it plays is deleted or moved, here and on a
remote; they chose the recommendation below, the next track following
one deleted where Continue says one does). Built on Linux; the Windows
part compiled, not tried there. Amended the same day, the user asking
to close the two gaps left: a queued track found gone is passed (7),
and the other holders of a file shown let go too (8).

## Context

- **The player knew its tracks by path alone.** Nothing told it of a
  move or a deletion: the dock, the playing mark in the views, Reveal,
  the queue and the desktop's media controls kept the old path; a
  track deleted played on to its end on a local disk (the helper keeps
  the file open), and the queue failed on the tracks gone. A rename of
  the file the panel plays was another preview: the panel stopped it,
  or showed a second player beside the pin's.
- **Playing on after a deletion is a local disk's behaviour.** On
  Windows a file open for reading cannot be removed, moved or replaced:
  GStreamer's `filesrc` opens through GLib's `g_open`, the C runtime's
  `_wopen`, without `FILE_SHARE_DELETE` (read in GLib 2.88's and
  GStreamer 1.28's sources). Through an sshfs mount, a file removed
  while open is kept on the server as `.fuse_hidden…` until it is
  closed, and its folder cannot be removed meanwhile.
- **Plans told what they changed only once they ended**
  (`Executor::observe`), and only when something applied.
- The shelf and bookmarks already follow every plan's changes
  (`op::Follow`).

## Decision

1. **What this process keeps open says so** (`fs::kept_open`): a
   holder registers the file with how to let go of it, and drops its
   guard once the file is closed. The media session's manager keeps the
   file it plays, and the one queued after it once its worker has opened
   it (`Reply::Handing`, as the first nears its end), and drops them once
   its worker is reaped. The file queued and not opened yet is let go of
   by being un-queued (`Request::Next` with none): the track plays on,
   and the pin tells the queue again once the plan has ended; one handed
   over as a plan takes it ends the session for that plan
   (`kept_open::taken_by`). Like the folders listings hold
   (`fs::wait_let_go`), it is the process's.
2. **A plan lets go of them before its first step**
   (`Executor::apply_plan`, `let_go_of_kept`): those at or below what
   its steps remove (trash, delete, remove) or move to another volume
   (copied, then removed: told by the mount table in memory, never a
   stat), and on Windows also what they move, rename, swap or put
   another file in the place of (below a folder copied there, only what
   it replaces). It
   waits for each to be closed, two seconds at most; a holder slower
   than that is left to it, its step failing as it would. Looked at only
   while something is kept open.
3. **A plan that let go of one tells its observers even if it changed
   nothing** (a cancel, a failure), by the id they are told
   (`observe_plans`), so the holder takes it up again.
4. **The session says a plan took its file** (`media::Update::taken`)
   rather than failing: no error is shown.
5. **The player follows its tracks through each plan that ends**,
   whatever asked it (the files, an agent, an undo). An observer per
   window follows the pin's track and queue through the plan's changes
   on the executor's thread (`op::Follow`) and sends what moved or went.
   - **Moved or renamed:** the pin's path and its queue follow, the
     track playing on; the mark, Reveal and the desktop's controls with
     them. The track queued after it is told again by its new path.
   - **Taken away:** tracks gone leave the queue, a shuffle keeping its
     order of those left. The track played ends as if it had played to
     its end: with Continue on Play next or Shuffle, the next of those
     left plays; on Stop, or with none left, the player closes. Undo
     brings the file back, not the player.
   - **Taken by a plan that left it** (cancelled, failed), or moved it
     (Windows): taken up again where it stood, playing or paused as it
     was.
6. **The panel's player plays on through a rename** of the file it
   plays: the same file (`EntryId::file`, the same content) renamed is
   the same preview, its key moved to the new name. Its own player (not
   the pin's) whose file a plan took is taken up again as the pin is
   (`PanelTaken`): the plan left the file, the panel asks for it again;
   moved it (on Windows, where a rename takes the file), the panel follows
   it to its new name; either way its next player starts where it stood,
   playing or paused as it was (`Shell::panel_resume`). Taken away, the
   panel shows what follows.
7. **What another program does** is followed where a pane of the
   window shows the pin's folder: a pane keeps the renames it followed
   (`Pane::renames_since`), and a rename of the pin's name, known by the
   entry it was last listed as (never another name of its file), moves
   the pin; its name gone from the folder with no plan of Skwir's under
   way there (`Executor::under_way_in`, asked before the plans' ends are
   read, then again only once a plan has started or ended) takes it
   away. Elsewhere no watch says (a folder not shown, a
   server's own changes): the track plays on, and a queued track found
   gone is passed: the media helper does not hand over a file no longer
   there (the track ends), and the pin, opening a track that is not
   there, drops it from the queue and goes on past it the way it went
   (`Pinned::pass`, `Shell::pass_gone`), as past a track taken away.
8. **Every holder of a file shown lets go of it** (amended): the
   document engines (PDFium, Poppler, `MuPDF`, `DjVuLibre`) and the
   archive helper (archives, comics), through their manager
   (`fs::kept_open::WhileOpen`: the request in flight cancelled, the
   helper ended); an `MBTiles` map's SQLite connection and a reader of
   what a preview holds (`Held::reader`, a PMTiles map's), in Skwir's
   process, closing what they keep. Each opens the file again for the
   next request once the plan that took it has ended
   (`kept_open::wait_while_taken`, `Taking`, held by the plan to its
   end), finding it gone, moved or left; a held reader, which no request
   of its own could stop waiting (a map's tiles share one), and a map's
   `MBTiles` connection, whose tiles would each hold a worker for the
   plan's length, fail a read while the plan runs instead
   (`kept_open::Taken`): the map keeps such a tile as asked again once a
   plan has ended (`kept_open::ended`), never as a failure, nor says the
   file's tiles cannot be read. Not the terminal: its shell
   stands in the folder shown, and only ending the user's shell would
   take it out.

## Consequences

- On a remote, what Skwir does is followed as on a local disk: the plan
  says it, not a watch. What the server does from elsewhere is seen
  only when its folder is read again (ADR-077).
- The music stops at the start of a plan that removes the file playing,
  even where that step is not its first.
- The terminal's shell keeps the folder it stands in from being removed
  or moved on Windows; nothing lets go of it.
- A pane's rename record keeps the latest 1024; a reader that looks
  less often loses the oldest, and a rename lost reads as the file gone.
