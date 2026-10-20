# ADR-050: Names are indexed by skwird

Status: accepted, 2026-10-10. Built in part: a search that plocate
answers says as of when (2026-10-20, amendment below); skwird, coverage
and its provider are not built. Amends ADR-026 (an index that proves its
coverage answers) and the last point of ADR-003 (Skwir may start skwird
for the session).

## Context

- **Searches walk every time.** On the reference home (2.1M files, 138k
  folders), a walk statting every file took 5.6 s, a `content:` scan
  34-67 s (`skwir-bench trigram`, 2026-10-10).
- **A search of `/` trusts plocate** (`plocate::try_locate`): when it
  answers, the walk is skipped, though its database is as old as the
  last `updatedb` (usually a day). The one place a search breaks
  ADR-026's "the walk answers".
- **fsearch** (macOS, MIT) answers name queries over millions of files
  in about a millisecond from a memory-mapped index kept current by
  FSEvents. Its Linux port covers only home and watches every folder
  with inotify; each linking application adds its own watches (its
  README counts 465k of a 524k limit for one home).
- **Keeping an index current depends on system and privilege:**

  | System | As a user | Privileged |
  |---|---|---|
  | Linux | an inotify watch per folder, no history across restarts | root: fanotify on whole filesystems |
  | macOS | FSEvents, history replayed after restart | — |
  | Windows | one recursive watch, no history | administrator: the USN change journal |

- **Rules to keep:** no daemon in Skwir (ADR-000); system indexes may
  serve a term, but Skwir never installs or starts them (ADR-003); an
  index hints, the walk answers (ADR-026).

## Decision

1. **skwird, a separate project, indexes names** (design: its
   repository's `docs/DESIGN.md`). Skwir is one client: it speaks
   skwird's protocol, never links its engine, asks nothing another
   client could not, and stays complete without it (it walks).
2. **Three lifetimes, one protocol:** system service, user service, or
   on demand (started by its first client, ending a minute after its
   last). Skwir never installs or enables a service; packages may
   recommend skwird. Where installed, Skwir may start it on demand: a
   helper ending with Skwir, not a daemon (ADR-000). This amends
   ADR-003's "none is ever started by Skwir", for skwird alone.
3. **Coverage, not hints.** Each answer states, per root and kind of
   fact (names, attributes): live since a time (nothing dropped,
   applied up to a barrier the search set), checked at a time, as of a
   time, or not covered and why. ADR-026 is amended to trust proven
   coverage.
4. **Skwir keeps what a search means.** The query plugin compiles the
   terms skwird can answer (names, kinds, sizes, times as instants from
   the user's calendar, ADR-017) and checks each file skwird names like
   any other: stat, providers, every term. Types stay the MIME
   database's (ADR-037); syntax stays the query plugin's.
5. **Identity.** Answers carry each entry's and its folder's device and
   inode: a result is an `EntryRef` with identity, and two names of one
   file are two results (ADR-049).
6. **Privacy.** skwird never indexes private folders (honours `.private`
   itself, ADR-044); a system service answers each user only what that
   user could list; Skwir still applies its own privacy rules before an
   agent or outside plugin sees anything.
7. **A provider in the query plugin**, through skwird's client crate
   once published (no git dependency: Skwir builds from its lockfile).
   The core's hints gain coverage (ADR-026), so no plugin depends on
   another.

## Consequences

- Under a covered root (live or just checked), a home name search
  answers from the index without walking; elsewhere it hints and walks.
- A plocate-answered search of `/` still skips the walk but says its
  answer is as of the last update (built 2026-10-20, amendment below).
  skwird's system service, where it covers `/`, replaces plocate.
- Possible later uses, each a decision of its own: the address field as
  one types, Usage of a whole tree from folder totals, "recently
  changed", the tags index's checks, a file's other names.
- **Built:** plocate's answer dated (2026-10-20). The rest, in order:
  1. skwird phase 0: measure on the reference home, warm and cold, and
     fanotify on btrfs as root;
  2. skwird phase 1: a Linux user engine, started on demand;
  3. hints' coverage and the provider (`docs/PLAN.md`, "Names indexed
     by skwird").

  Release 0.1 ships without it.

## Amendments

### 2026-10-20: plocate's answer is dated

The Consequences' "will say its answer is as of the last update", built
(`docs/PLAN.md`, "Names indexed by skwird", step C):

- **In the core,** a listing keeps what its answer rests on:
  `location::AsOf` (what answered, in the user's words, and when it was
  last brought up to date) through `ListingEvent::AsOf(Option<AsOf>)`,
  read with `Listing::as_of`. Each run of a search says it, `None` for a
  walk, so a refresh that walks takes back what plocate said. Point 3's
  coverage (live, checked) is to extend this, not to add another way of
  saying it.
- **The time** is the oldest modification time of the databases plocate
  read: its default, which its help names (`(default is
  /var/lib/plocate/plocate.db)`; NixOS's is `/var/cache/locatedb`), then
  those `LOCATE_PATH` adds, as plocate reads them. `updatedb` writes a
  database whole, so its time is the update's. When none can be stat-ed
  the answer is as of a time that cannot be told, and says so. A missing
  default makes plocate fail, and the search walks.
- **Said:** the pane's footer adds "as of Yesterday 03:00" after the
  count, its tooltip what answered and what that leaves out; the status
  line a screen reader reads says it too; `entries` answers `as_of`
  (`by`, `updated` in seconds), to scripts and agents.
- **Checked:** unit tests (the help's default, the oldest of several
  databases, the listing keeping and taking it back, the footer's words);
  a search through the app with a stand-in plocate on `PATH`, its
  listing and its `entries`, then a walk saying nothing; plocate 1.1.25's
  help and its reading of `LOCATE_PATH` (a missing default fails, exit 1).
  In a window on Xvfb (debug build, a stand-in plocate whose database was
  dated the day before at 03:00): the footer and its tooltip, kept by F5,
  gone in a search of a folder. Not checked: Orca reading the status
  line; a real system database (the reference machine has no plocate).
