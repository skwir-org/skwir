# ADR-062: Files on the system clipboard

Status: accepted, 2026-10-16. Built on Linux (X11 and Wayland); macOS and
Windows keep Skwir's own clipboard until their part is written.

## Context

- Copy and Cut filled a clipboard of Skwir's own (the `ops-basic`
  plugin's: entries by identity, whether they were cut, where from). Other
  applications saw nothing: a file copied in Skwir could not be pasted in
  another file manager, nor one copied there in Skwir.
- Every Linux file manager puts files on the system clipboard, in two
  families that do not read each other (checked on 2026-10-16: Dolphin
  26.08, Thunar 4.20.7 and PCManFM-Qt 2.3 run on a display of their own,
  the clipboard read after each copy and cut; Nautilus, Nemo and Caja
  from their sources):
  - GTK's (Nautilus, Thunar, Nemo): `x-special/gnome-copied-files`,
    `copy` or `cut` then one `file://` URI a line; Caja the same bytes
    as `x-special/mate-copied-files`. Text: the paths, one a line.
  - KDE's (Dolphin): `text/uri-list`, and `application/x-kde-cutselection`
    = `1` for a cut. Text: `file://` URLs.
  - Thunar reads only GNOME's list: it cannot paste Dolphin's files
    (Paste greyed). Dolphin reads only the URI list: Thunar's cut pasted
    in Dolphin was copied, the file left in both folders. PCManFM-Qt
    offers and reads both, and moved Dolphin's cut.
  - A name that is not UTF-8 (`lat\xe9.txt`): Thunar wrote its byte
    (`lat%E9.txt`); Dolphin wrote `lat%EF%BF%BD.txt`, another file.
- Windows Explorer puts `CF_HDROP` and "Preferred DropEffect" (a cut is
  a move), and no text. Finder puts one `public.file-url` per file and
  has no cut (Option-Command-V moves at paste time).
- gpui's clipboard held text and images only, wrote one text, read
  another application's text on the UI thread without a deadline
  (Wayland's pipe) or for up to 4 s per format (X11), and never said that
  the clipboard changed.

## Decision

1. **The system's clipboard is the one clipboard.** Copy and Cut put the
   files on it (`Outcome::Files`, which the window writes); Skwir's own
   remembers what it put there, by identity, while it is still there.
   Once anything else is copied, by another application or in Skwir (a
   path, a name being renamed), Skwir's is forgotten and cut entries are
   no longer dimmed, as in every file manager.
2. **Both families are offered**, so any of them pastes: GNOME's list
   (no trailing newline, which Nautilus 44 refuses), MATE's, the URI list
   (CRLF), KDE's cut flag on a cut, and the paths as text, one a line, as
   Copy Path writes them. A name that is not UTF-8 is percent-encoded byte
   for byte in the URIs; in the text it is written as its URI (Thunar's
   choice), never lossily.
3. **Pasting what another application copied** reads GNOME's list, then
   MATE's, then the URI list with KDE's flag, on a worker, never on the
   UI thread. Only `file://` URIs are files: Skwir speaks no file-system
   protocol (ADR-021), so others are left out and said. The paste is a
   copy or a move as the clipboard says, applied at once and undoable, as
   a drop from another application is (its entries read from the disk on
   a worker). Text is never taken for paths.
4. **The command asks the window for it.** `clipboard_paste` with nothing
   in Skwir's clipboard answers `WantsClipboard` (typed, as
   `NothingSelected`); the window reads the system's and runs it again
   with what it holds as `clipboard` (`ClipboardArg`: entries and whether
   they were cut). An agent or a script passes `clipboard` itself, or is
   told the clipboard holds nothing; it never reads the user's.
5. **Pasting Skwir's own cut empties the system clipboard** once every cut
   entry has moved, so no other application moves them again. Another
   application's cut, once pasted, is left where it is: its files have
   gone from where it names them, and pasting it again says so.
6. **Skwir's terminal types files pasted quoted**, as files dropped on
   it: their text, a path a line, would run line by line in a shell that
   takes no bracketed paste.
7. **Where it is not built** (macOS, Windows: their formats are
   `public.file-url` and `CF_HDROP`), Skwir's own clipboard stays as it
   was, apart from the system's, and nothing is written for files.

## Consequences

- gpui (patch 0024) offers files with a cut flag beside a text, reads
  another application's files off the main thread, empties the clipboard
  of the files it put there, and says when they are no longer there. On
  Wayland a set selection takes the latest input's serial (a Copy from the
  menu used the last key press's, which a compositor may refuse once
  another client set the selection since).
- A clipboard manager that takes the clipboard over with every type
  (wl-clip-persist) re-offers Skwir's own `pid/` type on Wayland, so
  Skwir's files stay its own (read from its documentation, not tried); one
  that keeps text alone takes their place, and Skwir forgets them. When
  Skwir quits, what it put there goes unless a manager kept it; Skwir's
  own clipboard went with it already.
- Copying text anywhere forgets a cut in Skwir: what files managers do,
  and what the system's clipboard holds.
- A Skwir cut is a move wherever it is pasted: GTK's file managers read
  GNOME's `cut`, Dolphin KDE's flag. Between Thunar and Dolphin the
  families still do not meet; Skwir pastes both.
- Images and text pasted as new files (Nautilus, Dolphin) are another
  decision, for later.
- Dimming another application's cut, as Nautilus does, needs the
  clipboard read on each change, which X11 tells only through XFixes: not
  done.
