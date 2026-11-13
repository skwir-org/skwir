# ADR-134: A folder's files marked with their Git state

Status: accepted, 2026-11-13, on the user's answers (asked in the session
that built it: a mark on the picture and a Git column in the list, a
folder summing what is below it, what is ignored not marked, on by
default with a setting). Built on Linux.

## Context

- **What a file is to its repository** (changed, staged, new, in
  conflict) was asked of a terminal (`git status`). Dolphin's Git plugin
  and VS Code mark it on the file, and a folder by what it holds.
- **Reading it** compares the index with `HEAD` and with the files on
  the disk, hashing a file whose stat differs, and walks what the index
  does not track, matching it against the ignore patterns.
  - `git status` runs what a repository's configuration names: its clean
    filters (Git LFS) when it hashes a file, its file monitor. A folder
    from a downloaded archive is enough (ADR-133's case).
  - gix's own status (`status`) brings `dirwalk` and `attributes`, which
    bring its filters and its command runner (`command`): what ADR-133
    left out of the helper.
- **Freshness.** A folder shown is watched (its names, what is written
  in it), not what is below it; a commit, a `git add` or a checkout made
  elsewhere writes only the git folder.
- A repository on a remote mount: each file's stat is a request to its
  server (ADR-077).

## Decision

1. **The helper tells the state** (`skwir-git-worker`'s `status`,
   `status.rs`), with gix's index and ignore patterns alone (`excludes`:
   no attributes, no filters, no command runner built), as `git status`
   says it of the files under a folder:
   - the index against `HEAD`, each folder whose cache tree the index
     keeps as `HEAD` has it skipped (the work is the folders that
     differ);
   - each file in the index against the disk: a stat as the index kept
     it, not racy, says it is as it was; a size that differs says it
     changed; else its bytes are hashed as they are (no filter, no end
     of line converted); stat-ed on up to eight threads, as git's
     `core.preloadIndex` does;
   - what the index does not track, walked where the index tracks files
     below: new, or ignored (the repository's `.gitignore` files, its
     `info/exclude`, and the user's file of patterns, which skwir finds
     in their configuration, `git::excludes_file`, the helper being kept
     from it); a folder tracking nothing said whole, new at its first
     file not ignored, ignored when all it holds is, nothing when empty;
   - each path at its strongest (conflict, changed, staged, new,
     ignored); a submodule, a file kept out of the work tree
     (`skip-worktree`) or said unchanged (`assume-unchanged`) as git
     takes them; nothing written, no lock taken, the index not
     refreshed; 20,000 paths at most, 10 s at most.
   - As the repository's configuration says files are compared and what
     is told, else the user's, else git's defaults: `core.fileMode` (an
     executable bit not as the index has it is a change only where it is
     on: a drive that keeps no such bit, exFAT or NTFS, says every file
     executable), `core.symlinks`, `core.ignoreCase`, `core.trustCTime`,
     `core.checkStat`, and `status.showUntrackedFiles` (`no`: nothing new
     or ignored is said, as a repository of one's home files asks). The
     user's are read by skwir with the user's patterns file
     (`git::User`) and told the helper, which takes each where the
     repository says nothing, as git lets the repository's word win.
   - Isolated as for a history (ADR-133, 4), the work tree and the
     user's patterns file readable too.
2. **Core keeps the state, not the plugin** (`vcs`): a provider answers
   for a folder (`vcs::StatesProvider`), its paths below it and their
   states (`vcs::Answer`), from that folder or from the highest of the
   folders above it the pane shows too (a columns view's) in the same
   repository (`Answer::root`), so one answer marks every column; the
   pane writes each entry's into its metadata (`keys::VCS`: the state's
   word; `Bool(false)` where a state it held is no longer said; nothing
   for an entry nobody said anything of), a folder's the strongest below
   it, what is ignored not summed (`vcs::States`), in its listing, in the
   folders it opened in place and in the columns above it.
   - **Made ready on the worker** (`vcs::States`, with the names said in
     each folder): the pane finds the entries an answer names by their
     names, and only what differs reaches the listing; the rest of a
     listing is walked only for a folder new or ignored whole, or to let
     go of a state no longer said.
   - **Whole or not** (`Answer::whole`): an answer that stopped at the
     provider's limits says what it found and leaves the rest as it was,
     never said as committed.
   - **Asked** on a worker in the folder's lane, once the listing is
     whole, again when its names change or a column's above, it opens a
     folder in place, the providers change, or the provider's answer moves
     (`StatesProvider::generation`, from memory); when a file whose time
     was known is written in it, in a folder it opened or in a column
     above, at once after a quiet while and then every 5 s at most, not
     at each write (a log written each second); never for a place, a
     search or a remote mount. Only the latest asking's answer is taken.
     What an answer said stays across a file's update until the next.
   - Gone with the provider (its plugin off, its setting off): every
     entry said nothing of.
3. **The Git plugin answers** (`state.rs`), for a folder in a
   repository's checked-out files, and follows each repository it was
   asked of, 32 at most, by its git folder, its `logs` and its common
   folder (`fs::Settled`, 300 ms): the index, `HEAD`, the packed
   references, a merge or a pick under way. What changes below a folder
   shown has no watch: it is asked again when a window comes to the front
   (`Plugin::on_looked_at`), and by F5. The setting `git.states` (Folders
   and views › Git, on by default).
4. **Every view marks it** on the picture's top-right corner
   (`icons::vcs_mark`, `Marks::vcs`), each state its own shape, so none is
   told by its color alone: a sign in the error color for a conflict, a
   dot in the warning color for a change, a check for what is staged and
   a plus for what is new, in the success color; what is ignored is not
   marked (the user's choice: build folders would carry it everywhere). A
   screen reader hears it with the name ("changed", a folder's "holds
   changes").
5. **What sorts, shows** (ADR-130, amended 2026-11-12): `SortKey::Vcs`
   ("Git"), the most pressing first, what is as committed after, the
   ignored last; a list column (`Detail::Vcs`, off until shown, offered
   wherever a provider is, so no column comes and goes as a repository
   answers), a grid caption, and groups (In conflict, Changed, Staged,
   New, Unchanged, Ignored).

## Consequences

- The helper grows from 2.51 to 2.89 MB, stripped, in the release
  profile, renames (ADR-133, amended) included: gix's index, its ignore
  patterns and their stack (`gix-index`, `gix-ignore`, `gix-worktree`,
  `gix-bitmap`, `filetime`), run only while it is asked.
- On a repository of 100,000 files, the whole work tree's state takes
  0.07 s and 23 MiB in the helper, optimized (`git status`: 0.04 s); a
  folder below, 0.01 s.
- A file with a clean filter (Git LFS) or converted ends of line, whose
  stat changed while its content did not, is said changed: git would
  filter it before hashing. A file not stat-ed yet when it is written (a
  row not drawn yet in a large folder) is told when a window comes to
  the front.
- A submodule's own state is not said; a copy, not a move, is not
  followed (ADR-133).
- `core.excludesFile` and the settings above are read from the user's
  two configuration files as text, their includes not followed; the
  system's configuration is not read.
