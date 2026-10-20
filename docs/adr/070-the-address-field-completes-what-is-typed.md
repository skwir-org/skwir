# ADR-070: The address field completes what is typed, a name at a time

Status: accepted, 2026-10-19. Built and checked on Linux; Windows and
macOS compiled.

## Context

- A path typed in the address offered one row, "Go to …": nothing listed
  a folder. Words already filtered the folder shown at once and asked
  Jump for places.
- The user, 2026-10-19: "when typing a path in the search bar, there is
  no suggestion for autocompletion, could we list possibilities related
  to the current depth, similar (or better if you have ideas) than what
  Dolphin has". Dolphin's location bar completes against the folder
  typed so far: a popup of the names, and, as its Text Completion menu
  chooses, the rest of a name written inline.
- Asked, the user chose both the list and the rest drawn after the text;
  files as well as folders, folders first; and Enter on a file shows it
  selected in its folder rather than opening it (opening launches an
  application, which cannot be undone; showing can, with Back).

## Decision

1. **One depth at a time.** A path splits at its last separator: the
   folder typed and the name being typed. The folder is read once, on a
   worker (a disk may be asleep), the one read before stopped; every key
   after matches its names in memory. A folder a pane lists gives its
   listing's names, current, read by nothing again (a folder is listed
   once). `./` and `../` start from the folder shown, never the
   process's working folder, and `..` is taken as a shell's `cd` takes it.
2. **Forgiving matching** (`complete.rs`): case ignored unless a capital
   is typed; the whole name, then its start, the start of one of its
   words, anywhere in it. Hidden names only when a dot is typed or the
   pane shows them. Folders, then files; among each, where the user goes
   most (Jump's new `jump.scores`: zoxide's score of a folder and of the
   folders within it, a recent file's recency), then by name. Eight rows.
3. **Enter goes where the dropdown says.** Once a name is typed and one
   matches, the first row is that name, not Go, and what it adds is drawn
   muted after the text (`TextInput::set_hint`): what is seen is what
   Enter does. Go is first while no name is typed (`~/wip/`), for `.` and
   `..`, and when nothing matches. A path gone to is stat-ed on a worker:
   a folder is shown, a file shown selected in its folder, one that is not
   there said. A hidden file revealed turns hidden files on in that pane,
   and says so (`Pane::select_revealed`). Shift+Enter opens a file
   (`open`), as Enter on it in the files would.
4. **Tab completes** (`ui.omnibox_complete`), bound in the address while
   something can be completed (the `completes` key context), so that
   elsewhere it walks on as it did: a row chosen with the arrows is
   taken; else words over a folder that leave one name shown become its
   path; else the part the names typed share is filled in, as a shell
   does; else the highlighted row is taken; with Go highlighted, the
   first name is highlighted, for the next Tab to take. A folder taken ends with its separator and is listed in
   turn. Taking is an edit Ctrl+Z takes back. Right or End at the end of
   the text takes what is drawn after it.
5. **A folder part that names no folder is abbreviated**, as fish takes
   it: `~/w/sk/` stands for the folders its names start (`~/wip/skwir/`,
   `~/wip/skwird/`), each level matched by start or a word's start and
   ranked, four folders at most. With nothing typed after it, the rows
   are those folders; else their names, each saying which folder. Nothing
   is drawn after the text, which they do not continue.
6. **A place's text completes through its provider**
   (`LocationProvider::complete`, on a worker): the name being typed and
   every place at that depth, matched as names are. The tags provider
   answers from the places it lists (`tags:/Places/Fr` with France), what
   narrows the location kept. Words that start a scheme's name (`tr`) add
   its place under Go to; never a search's.
7. **A row keeps its exact path.** A name that is not UTF-8 is written
   approximately in the field and still goes where it says; the folder
   taken stays exact for the names typed after it.
8. **Heard by a screen reader.** The rows are a list box beside the field
   (`modal::options`), as the palette's and Jump's are; the row the arrows
   reach is its active descendant. While a path is typed, the field keeps
   reading back its text and caret, which the palette's typing does not
   need.

## Consequences

- Core: `LocationProvider::complete` with `Completions` and `Completion`,
  `App::schemes`, `Pane::select_revealed` (a jump to a hidden file through
  `jump.to` shows hidden files too).
- Jump: `jump.scores`. Tags: `complete`.
- UI: `complete.rs`; the address's rows are computed when the text or an
  answer changes, never per frame; the input draws a hint and replaces
  its text as an edit (`replace_all`), and says when the hint is taken
  (`HintTaken`).
- Matching a folder's names runs on the UI thread at each key, as the
  filter typed over a folder already does: one scan of names lowercased
  once when read, the best eight kept as they come. Measured on the
  reference machine for 100,000 names: 1.8 to 4.4 ms a key in release
  (every name matching is the slowest), 6 to 65 ms in a debug build.
- Proposed and changed while building: a second Tab was to move through
  the rows; that left no key to take a row and stay in the field (Enter
  leaves it), so Tab takes the highlighted row once nothing more is
  shared, and the arrows move.
- Not done: completing a place in a search's text (its words are the
  query's); a provider other than tags (`trash:` and `recent:` hold no
  places to type).
- Checked: see `docs/STATUS-LOG.md`, 2026-10-19, "Address completion".
