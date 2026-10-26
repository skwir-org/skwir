# ADR-089: Derived indexes are harvested in Skwir's own run, never by a daemon; content stays out of skwird

Status: accepted, 2026-10-26, the user's choice, with indexing while
closed turned on in Settings (point 3); from the design review recorded
in `docs/indexing-and-ai.md`. Not built. Amends ADR-050 (skwird's open
question 4 is answered: no content in skwird; its journal feeds Skwir's
derived indexes) and keeps ADR-000's principle 3 as ADR-050 read it (a
helper that ends with Skwir is not a daemon).

## Context

- ADR-000 rules out a daemon and a database; one deletable cache.
  ADR-050 made one exception for skwird, a project of its own with three
  lifetimes (a system service, a user service, on demand), which Skwir
  may start for the session where it is installed.
- skwird's design leaves open (its question 4) whether an index of
  content belongs in skwird or in each client. skwir measured a trigram
  index on 2026-10-10: 11 % of the text, queries 5 to 175 times faster
  than the scan.
- An index of names is already "a list of what is on someone's disk"
  and skwird treats its system service as attack surface from the first
  line, answering each asker only the folders they could list. Content
  is more: a service reading every user's files would have to filter
  every chunk per asker, and would hold what no other user may see.
  Models, their caches and the user's suggestions are per user anyway.
- The user asked which lifetime indexing should have: root or user
  daemons, and how to never slow the host.
- skwird's phase 0 measured the check that proves names current without
  any watch: 52 ms warm and 1.9 s cold for 652,336 folders. That makes a
  reconciliation at the start of a session cheap, and skwird's
  `changes` journal removes even that where it covers a root.

## Decision

1. **skwird indexes names and attributes only.** Content and derived
   facts never enter it. What Skwir's indexes take from skwird is its
   enumeration (a query by kinds, size and time under a root, no walk)
   and its journal (`changes {root, since}`), which keeps them current
   where skwird covers a root live. Its session records question 4
   answered and moves the journal earlier in its plan.
2. **Derived indexes are per user, in the user's deletable cache, built
   by the harvest in Skwir's own run.** `skwir_core::harvest` runs one
   lane at `Priority::Background` in the window's process, and the
   models it needs run in a helper (`skwir-infer-worker`, ADR-093) that
   ends with the window or a minute after its last call. No daemon of
   Skwir's, nothing as root, nothing reading another user's files.
3. **Indexing while Skwir is closed is the system's service manager's,
   turned on in Settings.** A switch on the Index page, "Also while
   Skwir is closed" (`extract.while_closed`, essential: it changes what
   the system runs), writes an entry for the user's own manager, as
   ADR-087's Connect at Login does: a systemd user service and its
   timer, a launch agent, a Task Scheduler task. The extract plugin
   applies the switch on a worker (`on_settings_changed`); where the
   manager fails, the switch goes back off with the reason on its row,
   as ADR-087's entry is taken back. Turned off, the entry is stopped,
   then removed and its manager told (a unit removed while it runs is
   kept running and forgotten, ADR-087's amendment of 2026-10-25).
   Scripts and agents change the same switch through `settings.apply`;
   nothing else writes the entry, and never a package. Turning the
   plugin off removes it; turning the plugin on again with the switch on
   writes it again.
4. **What the entry runs is `skwir index`,** a windowless Skwir that
   reconciles and harvests under the same scope and pace, then exits:
   once a few minutes after login, then hourly (provisional; a run that
   finds nothing new costs the check). Without a window, the pace's sign
   of the user is the session's idle signal: logind's idle hint on
   Linux, where the desktop sets it (which desktops do is checked when
   built), the system's idle time on macOS and Windows. A desktop that
   gives none leaves the run reading nothing, and the Index page says
   why beside the switch. One harvest runs at a time for a user (a lock
   in the cache's `extract` namespace); a windowless run that finds a
   window of Skwir open ends after its batch and leaves the harvest to
   the window and its setting. Tests never touch the user's manager
   (ADR-087's harness: `systemd-analyze verify` in folders of the
   test's own, the command run, a stand-in `systemctl`).
5. **Freshness without skwird is the check, in Skwir.** At each run the
   harvest lists again the folders whose change time moved (read before
   the entries, a second change within a tick read again next time,
   device and inode compared, as skwird's design says), then stats
   every file in scope and re-reads those whose version moved. Between
   runs, Skwir's own plans tell the indexes what they changed
   (`Executor::observe`). The proof words are skwird's four: live since,
   checked at, as of, not covered and why.
6. **Every index is an accelerator** (ADR-003's rule, kept): absent,
   damaged, of another layout, behind or over its room, Skwir walks,
   reads, or says what it has not read. Nothing waits for an index.

## Consequences

- Idle cost is zero with the plugins off and reclaimable with them on:
  the helper's pages go back when it exits; `just idle-pss` with every
  AI plugin on and idle must equal the baseline (ADR-092).
- `skwir index` joins `skwir cmd` and `skwir mcp` in the CLI; its unit
  files take ADR-087's shape and tests (a stand-in manager, the unit's
  command run).
- Without skwird, a session's first reconcile stats the scope while the
  user is idle: a fraction of a second warm for the XDG folders; the
  cold cost needs root to measure and is recorded when it is.
- skwird's `docs/DESIGN.md` and `docs/PLAN.md` take in question 4's
  answer and the journal's new place (its own session's work;
  `docs/indexing-and-ai.md`, last section).
- A content daemon is a door closed; opening it again needs an ADR with
  a per-asker story for content.
