# ADR-050: Names are indexed by skwird

Status: accepted, 2026-10-10. Nothing is built. Amends ADR-026 (an index
that proves its coverage answers) and the last point of ADR-003 (skwir
may start skwird for the session).

## Context

- **A search walks every time.** On the reference home (2.1M files,
  138k folders), a walk that stats every file took 5.6 s, and a
  `content:` scan 34 to 67 s (`skwir-bench trigram`, 2026-10-10). A name
  search of the home folder walks every folder each time it runs.
- **A search of `/` trusts plocate.** When plocate answers, the walk is
  skipped (`plocate::try_locate` in the query plugin's provider).
  plocate's database is only as fresh as the last `updatedb`, usually a
  day, so files made since are missing. This is the one place a search
  breaks ADR-026's rule that the walk answers.
- **fsearch** (macOS, MIT) answers a name query over millions of files
  in about a millisecond, from an index mapped into memory and kept
  current by FSEvents. Its Linux port, fsearch-crossplatform, covers the
  home folder only and watches every folder with inotify. An application
  linking it follows its daemon's index, but installs watches of its
  own: one inotify watch per folder per process, and its README counts
  465k of a 524k limit for one developer's home.
- **Keeping an index current depends on the system and its
  privileges:**
  - Linux: as a user, one inotify watch per folder and no history
    across restarts; as root, fanotify on whole filesystems.
  - macOS: FSEvents, with a history a restart replays.
  - Windows: as administrator, the change journal (USN); otherwise one
    recursive watch and no history.
- **The rules this must keep:** ADR-000 rules out a daemon in skwir.
  ADR-003 lets the system's indexes serve a term, as long as skwir never
  installs or starts them. ADR-026 makes an index a hint and the walk
  the answer.

## Decision

1. **skwird, a project of its own, indexes names.** Its repository's
   `docs/DESIGN.md` holds its design.
   - skwir is one of its clients. It speaks skwird's protocol and never
     links its engine.
   - skwir is complete without skwird: it walks, as now.
   - skwird is a general tool, and skwir asks nothing of it that
     another client could not.
2. **Three lifetimes, one protocol.** skwird runs as a system service,
   as a user service, or on demand: started by its first client, ending
   a minute after its last.
   - skwir never installs or enables a service. Its packages may
     recommend skwird.
   - Where skwird is installed, skwir may start it on demand. That is a
     helper ending with skwir, not a daemon (ADR-000). It amends
     ADR-003's "none is ever started by skwir", for skwird alone.
3. **Coverage, not hints.** Each answer says, for each root and each
   kind of fact (names, attributes), one of:
   - live since a time (nothing dropped, and applied up to a barrier the
     search set);
   - checked at a time;
   - as of a time;
   - not covered, and why.

   ADR-026 is amended to trust a proven coverage (its Consequences).
4. **skwir keeps what a search means.**
   - The query plugin compiles the terms skwird can answer: names,
     kinds, sizes, and times as instants from the user's calendar
     (ADR-017).
   - It considers each file skwird names as it considers any other: a
     stat, the providers, every term.
   - Types stay the MIME database's (ADR-037), and the syntax stays the
     query plugin's.
5. **Identity.** skwird's answers carry each entry's device and inode,
   and its folder's. A result is then an `EntryRef` with its identity,
   and two names of one file are two results (ADR-049).
6. **Privacy.**
   - skwird never indexes private folders: it honours `.private` itself
     (ADR-044).
   - A system service answers each user only what that user could list.
   - skwir still applies its own privacy rules to what skwird names,
     before an agent or a plugin from outside sees it.
7. **A provider in the query plugin.**
   - It speaks the protocol through skwird's client crate, once that is
     published: skwir builds from its lockfile, so no git dependency.
   - The core's hints gain coverage (ADR-026), so no plugin depends on
     another.

## Consequences

- **Under a covered root**, live or just checked, a name search of the
  home folder answers from the index without walking. Elsewhere it
  hints and walks, as now.
- **A search of `/` that plocate answers** still skips the walk, as
  now, but says its answer is as of the database's last update (not
  built). skwird's system service, where it covers `/`, replaces it.
- **The same answers could later serve:**
  - the address field, as one types;
  - Usage of a whole tree at once, from folder totals;
  - "recently changed";
  - the tags index's checks;
  - a file's other names.

  Each is a decision of its own.
- **Nothing is built.** The order:
  1. skwird's phase 0 (measuring on the reference home, warm and cold,
     and fanotify on btrfs as root);
  2. skwird's phase 1 (a Linux user's engine, started on demand);
  3. the hints' coverage and the provider (`docs/PLAN.md`, "Names
     indexed by skwird").

  Release 0.1 ships without it.
