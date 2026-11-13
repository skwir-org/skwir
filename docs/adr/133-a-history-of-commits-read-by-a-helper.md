# ADR-133: A file's and a folder's history of commits, read by a helper

Status: accepted, 2026-11-13, on the user's answers (asked in the session
that built it: History a way of looking rather than a setting, gix in a
helper of its own, in the main package). Built on Linux. Amended
2026-11-13: a file followed across its renames (2), on the user's ask;
the files' state in the views is ADR-134's.

## Context

- **A folder in a repository** was shown by what it holds (its README
  since 2026-11-12) and a file by its content; what was done to them, by
  whom and when, needed a terminal (`git log`). File managers show at
  most a file's state (Dolphin's and Nautilus's overlays); forges show a
  folder's README and, a click away, its history and the graph of its
  merges.
- **The README and the history would compete** for the folder's
  preview. A setting choosing one hides the other; a way of looking at
  it, as Source and View as hex are for a file, offers both.
- **Reading a repository**: the `git` program, or a library.
  - `git` is installed where repositories are, but a repository's own
    configuration can make it run programs: a text converter or an
    external diff for `log -p`, a file monitor, a pager. A folder from
    a downloaded archive is enough. Each call would have to switch every
    one off, and a version that adds one would run it.
  - **gix 0.90** (gitoxide, MIT or Apache-2.0, pure Rust) reads objects
    and runs nothing. Measured 2026-11-12, stripped, thin LTO, over an
    empty program (0.31 MB): open, walk and read commits and trees
    (`sha1`, no default features) +1.6 MB; with its tree and line
    diffs (`blob-diff`, which brings attributes, filters and the
    command runner) +2.9 MB; with its speed options +3.0 MB; 58 crates
    skwir did not have, gitoxide's own but ten. On 100,000 commits with
    10,000 merges: the whole walk 21 ms (`git rev-list`: 32 ms), a
    screenful under 1 ms, a folder's commits by comparing its tree entry
    with the parents' 1.4 s, naively (git: 0.2 s); 18 and 51 MiB at
    most. On skwir's own 1,600 commits, `crates/ui`'s 793, as git says.
  - ADR-128 measured that each MiB of code in skwir costs about 0.73 MiB
    idle, used or not: gix in the program would cost every user some
    2 MiB for good.

## Decision

1. **History is a way of looking** at a folder or a file in a
   repository, as Source is (ADR-068's frame, `Slot::Look`): a provider
   offers itself by role (`PreviewProvider::way`, `preview::Way`:
   its label and icon, and whether it looks at folders), never chosen by
   type; the bar shows its button where it accepts the entry, lit while
   it is shown. A click shows the history; another, the folder's README
   or count, or the file, again.
   - **Chosen for a folder, kept for folders** (`PreviewState::folder_way`,
     part of the window's shape): every folder in a repository shows its
     history until it is turned off; a folder in none shows what it
     holds. A file's is chosen for that file.
   - Asked of the providers on a worker beside the preview, never before
     it: a folder on a slow server shows its README while its repository
     is looked for. A folder's repository is looked for once a minute at
     most (`git::Repo::find`, kept by folder).
2. **What it shows** (`InspectionKind::History`): the commits that
   reached `HEAD`, newest first, parents after their children; each row
   its summary, the branches and tags at it, its short id (as git
   shortens it: `Id::shorten`, `core.abbrev` or the repository's size,
   longer where it would be ambiguous), its author, its date (the
   user's calendar). A commit's menu copies its id whole (`Ctrl+C`
   too), its short id or its summary.
   - **The names at a commit are chips at the text's size**
     (`inspection::RefKind`): a branch in its lane's color, its tint
     under the name, its line around it, the one checked out in
     semibold; a remote-tracking one outlined alone; a tag in the
     chrome's tint, its glyph before its name in semibold.
   - The bar's title names the source and the branch checked out
     (`Git history · main`); the button's tooltip, Git.
   - **Its Contents are its tags** (asked by the user, 2026-11-13), in
     the drawer documents have: newest first, the release the commits at
     the top went into highlighted, a tag chosen choosing its commit;
     disabled where none. One pass over the rows on the worker, 3.8 ms
     for 100,000 commits and 1,000 tags in an optimized build; the part
     shown found by binary search, the drawer virtualized.
   - The whole repository's (its top folder's) draws **the graph** of
     its merges down the rows' start (`inspection::GraphRow`: each row's
     node in a lane, the lines through it, each in its lane's color, one
     of the theme's `lanes`), laid out by the plugin as the rows come
     (`lanes::lay`), linear in the rows times the lanes.
   - A folder's or a file's lists **the commits that changed it**: its
     entry in the commit's tree differs from every parent's (git's
     history simplified as `git log -- path` keeps it, a merge shown
     where it changed it against each side); without a graph, as
     `git log` draws none.
   - **A file is followed across its renames**, as `git log --follow`
     follows it, along each line of history apart (`rename.rs`): each
     commit knows the file's name in it, `HEAD`'s the one asked, a
     parent's given by its newest child. Where a child has the file and
     its first parent nothing of that name, where it came from is looked
     for among the files the child took out: the same file first (its blob),
     else the regular file most alike, 50 % at least, by git's estimate
     (its spans hashed); one whose size rules it out never read, 1,000
     compared at most, none past 8 MiB, within the deadline (the tree
     diff too, 256 folders deep at most); once a rename. A merge's other
     parents are not looked in, as `--follow` looks along first parents:
     a side that lacks the file is a line where it was not made yet, and
     its diff against the merge can be the whole repository's. A commit from before a rename names the file as it was then
     (the log's `path`, a row's target `inspection::commit_target`), and
     its patch is read under that name; the rename's shows it as
     `git show -M` does (its similarity, both names, then what changed).
     A folder is not followed, nor a copy (`--follow`'s), and a merge
     joining two names is kept, as a path's history keeps it.
   - The first 500, quickly; Full preview the rest, to 100,000. The
     helper stops at 10 s (50 s asked whole), and says it did.
   - In `--date-order`: newest first, never a parent before all its
     children (the helper's walk, then Kahn's order, n log n).
3. **A commit opens as its patch** below the list, which stays above it
   as an archive's does (`kept_rows`, `members.rs`): the helper writes
   `<id>.patch` (what `git show` prints: its header, its message, a
   unified diff with three lines of context of each file changed, under
   the folder or the file whose history it is), previewed as the text it
   is, colored as a diff, found in, copied. A merge's against its first
   parent. A file larger than 8 MiB or holding a NUL in its first 8,000
   bytes is said to differ, as binary. 2 MiB of patch at most, Full
   preview 64 MiB.
4. **A helper reads the repository** (`skwir-git-worker`, gix with
   `sha1` alone: no attributes, no filters built; its command runner
   comes in with its protocol, called only by a network client, which
   is not built), in
   the main package of every system, as the archive and model helpers
   are: pure Rust, about 2 MB, no library of the system's. Its own tree
   walk and line diff (`imara-diff`) stand where gix's would bring the
   filters.
   - JSON on standard input, JSON on standard output (`log`, `patch`).
   - Isolated as every helper reading what a file holds (`sandbox`):
     the repository's git folder, its common folder and its alternates
     readable, each only where it is what its pointer says (a folder of
     objects and references, a store of objects: `Repo::of`), so a
     repository made to name another folder opens nothing more; the
     patch's folder writable, nothing else; opened
     isolated from the user's and the system's configuration
     (`gix::open::Options::isolated`).
   - Nothing it reads is run, fetched or written to the repository: no
     hook, no filter, no lock taken, no index refreshed.
5. **A plugin of its own** (`git`, Previews), off without a trace; it
   requires the helper, as the reader plugins do theirs.
6. **A member is written out by the provider that showed its container**
   (`Previews::extract`, as `inspect` chooses: the one the view names,
   else the type's): a commit is the history's, never the folder's or
   the file's type's.

## Consequences

- A folder's preview has a bar wherever a way of looking accepts it,
  its count's too: History alone, before More.
- A history is read again when the folder or file shown changes, not
  when the repository does (a commit made in a terminal): shown again,
  it is read anew. Where a folder's repository is, is kept a minute: a
  repository made meanwhile shows its History after that.
- The History button can come a moment after the preview, as the
  repository is looked for beside it; a folder's bar comes with it.
- On skwir's own 1,601 commits, following a file costs what its plain
  history does (0.1 s), the same commits as git's.
- Later, as said with the user: History as a place in the pane, the
  commits as rows at full width, a commit opened as a folder of what it
  changed, its tree browsed as an archive's. The files' state in the
  views is ADR-134.
