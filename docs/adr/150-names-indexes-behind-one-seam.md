# ADR-150: Names indexes behind one seam; skwird's a plugin of its own

Status: accepted, 2026-11-23, on the user's answers (asked whether
Skwir was ready to take skwird in, then chose: a plugin of its own
behind a seam core owns; the user's folders and bookmarks kept live,
"fully tweakable": following each source switched, any folder added or
taken off; the fake engine in tests and a check against the real one;
an index's answer shown at once and proven after, "as long as it
doesn't create some confusing flicker or unpredictable reordering"; and,
asked, a context-menu entry beside the command). Built in part the same
day: 1, 2, 4 and 5 (`docs/PLAN.md`, step A), sizes and times not yet
asked of an index (a link's are its target's); then 3, the plugin (step
B), with no setting yet (the plugin's switch turns it off; Kept live's
page comes with 6), checked in a window through a real engine. Amends
ADR-050 (point 7: where the provider lives) and ADR-026's amendment of
2026-10-10 (coverage is a seam of its own, not `HintProvider`'s).

## Context

- **skwird is ready** (ADR-050): its phases 1 to 3 are done on Linux (a
  user's engine started by its first client, a user service with live
  folders, the system service on fanotify), and its client is copied in
  `vendor/` (ADR-050, amended 2026-11-23).
- **ADR-050 point 7** put the provider in the query plugin. The search
  is not the index's only user: Phase 7's harvest is in core
  (`skwir_core::harvest`, ADR-089) and reads the journal; the tags
  index's checks are the tags plugin's; Usage and the address field may
  follow (ADR-050's later uses). Plugins never depend on each other.
- **plocate is a special case** in the query plugin
  (`plocate::try_locate`): a search of `/` whose terms are name
  substrings skips the walk and says as of when (ADR-050's amendment of
  2026-10-20), apart from the hints.
- **A user's engine watches only what clients register,** within a
  quarter of the user's inotify watches, shared by its clients and
  filled first come; the user's home (867,000 folders) does not fit
  524,288 watches. A system service hears everything, and `watch` there
  does nothing. Registering 100,000 folders takes about 5 s.
- **On the user's home**, a sync of a live folder is a barrier (0.02 to
  0.1 s); elsewhere a sync is skwird's check (0.43 s warm, a few seconds
  cold); a walk statting every file took 5.6 s (2026-10-10).

## Decision

1. **One way of saying coverage, in core.** For a root and each kind of
   fact (names and kinds; sizes and times), an index says **live** since
   a time, **checked** at a time, **as of** a time (or a time it cannot
   tell), or **not covered**, and why (skwird's reasons: outside, excluded,
   private, denied, a read failed or cut short, gone, replaced, a mount,
   a network mount, a snapshot, not read yet, a watch limit), with the
   subtrees below it that are not covered. What a listing rests on
   (`location::AsOf`, ADR-050's amendment of 2026-10-20) grows into it,
   as that amendment asked: one way for the footer, the status line and
   `entries` to say it, `entries` gaining fields and renaming none.
2. **A names index is a seam core owns,** registered by owner as the
   hints are: the coverage of a root; a sync (a barrier where live,
   else a check); a query of what an index can answer (name terms, all
   to hold, each matched as the query plugin's `NamePat` matches; kinds;
   a size range; a modification-time range as instants of the user's
   calendar, ADR-017), streamed in batches and stopped by the search's
   cancel token; the journal later, when the harvest takes it. An index
   may name more than match, and fewer only where it says it does not
   cover. The query plugin asks every registered index and takes, for
   the root, the strongest proof (live, then checked, then as of), and
   checks every file it names as any other (ADR-050, point 4).
3. **skwird's provider is a plugin of its own, `skwird`,** holding what
   is not the search's: its connection (made at the first search, never
   at start, ADR-128 and ADR-141; one per search, stopped through the
   client's `Stopper` from the search's cancel token, the engine's
   silence a backstop longer than a first build; none kept while idle,
   so an engine started on demand ends); its registrations (the folders
   of the Secrets entries, ADR-125, through `exclude`; the folders kept
   live, 6); its settings (`skwird.*`); its switch, which leaves every
   search walking. Every error is no engine, and the search walks.
4. **plocate is the seam's second index,** registered by the query
   plugin: it covers `/` as of its databases' update, for name
   substrings, as today; the search no longer asks it apart. Where
   skwird covers `/` live or checked, its proof is the stronger.
5. **A search under a covered root:**
   - **live:** a sync (the barrier), the query, what it names
     considered, only the holes walked;
   - **checked, or as of where a sync can prove it:** the query at once,
     what it names considered and shown as any search's results are;
     then the sync, the query again, what is new considered, the holes
     walked; no walk of the rest;
   - **as of only** (plocate): as today, the walk skipped and the answer
     dated;
   - **sizes and times** are asked of an index only where it says them
     live; elsewhere the index answers by name and kind, and the stat
     decides (ADR-026's amendment: a check proves names and kinds only);
   - **holes** are walked as ADR-026 says, except a snapshot below the
     folder searched, passed over as ADR-125's `searches` role passes
     over a tool store, and walked when searched in or with
     `excluded:yes`.

   **Nothing shown is taken back:** each file an index names is checked
   before it is shown, so a proof can only add rows; those it adds
   arrive in the listing's order, as a streaming search's do. The footer
   says the search is under way until the proof, then the proof, once.
6. **Kept live,** for a user's engine (the system service hears every
   folder): a list on the `skwird` plugin's settings page, in three
   groups, as the Exclusions page has them (ADR-125, 9): **Your folders**
   (the desktop's, `places::user_dirs`) and **Bookmarks**
   (`places::bookmarks`), each heading's switch following its source
   (on by default: one added is ticked, one gone leaves); **Added by
   you**, with Add folder… and removal. Rows are ticked; a row of a
   followed source unticked is remembered by its path. Each row says
   what it is: live, not live and why (too many folders for the share,
   on a network), or inside another folder kept live (not registered
   twice); the share's use below, once skwird says it (asked of its
   session, 2026-11-23). When the share is tight, the order is added,
   then your folders, then bookmarks in the sidebar's order, as the page
   says. Changes are sent as differences; at each connection Skwir takes
   back what skwird lists for it and the settings no longer name. With
   the system service, the list is dimmed and says that every local
   folder is live. A command (palette, scripts, agents) and the folder's
   context menu (`Keep live for searches`, in the 1xx group beside the
   bookmarks, ADR-099) add or take off a folder.
7. **Tests:** the client's fake engine (`skwird_client::fake`) in the
   ordinary tests, on a socket of each test's own (`Find.socket`: the
   system service's socket is asked first otherwise); `just
   skwird-check` runs the provider's tests against a real `skwird` (on
   the `PATH`, or named by `SKWIRD`), failing where there is none, and
   joins `release-check` once skwird is packaged. No flake input: skwird
   is published nowhere.

## Considered

- **The provider in the query plugin** (ADR-050 point 7): one crate
  less, but the harvest and the tags index could not reach it, and its
  connection, registrations and settings would be the search's.
- **Keeping live what the panes show** (skwird's advice): no plugin hears
  navigation, a pane's subtree may be the whole home or `/`, and every
  move would register and take back folders by the hundred thousand.
- **The first answer held until the sync:** 0.4 s with nothing shown in
  the warm case, seconds cold, for an answer the checks already make
  right.
- **skwird in the development shell:** a flake input on a path of this
  machine, which breaks the flake for anyone else and for CI.

## Consequences

- Core gains the coverage type and the seam; `location::AsOf` and
  `entries`' `as_of` grow; the query plugin loses its plocate special
  case.
- A plugin crate, `skwird`, its entry in `plugins_toggle`, its settings
  page, its command and menu entry.
- The footer and the status line say live and checked as they say as
  of (ADR-139's messages).
- Steps: `docs/PLAN.md`, "Names indexed by skwird".
