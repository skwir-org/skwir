# ADR-121: An order asked anew is made on a worker

Status: accepted and built, 2026-11-06, on Linux; macOS and Windows
compiled only. Follows ADR-120 (a folder appears whole, built and
sorted on a worker). Amended 2026-11-12: an order that reads what its
entries lack is made once they are read.

## Context

- **Asked for** (2026-11-06), after ADR-120 took a folder's building and
  first sort off the window's thread: the sort that remained there, an
  order asked anew of a folder shown. A column's header clicked, the
  direction turned, hidden files shown or hidden: each sorted the whole
  folder on the window's thread, about 110 ms for `/nix/store`'s 390 000
  entries in the release build.
- **A listing keeps one order**, for one spec (`SortCache`), placing in
  it what arrives, goes or changes. While it is current, the order for
  another spec can be made from the same entries anywhere, if the
  entries stay as they are meanwhile.
- **What acts on a pane's order** (the arrows, Select all, a range
  clicked with Shift, an agent's selection by index) acts on the order
  drawn: an order made elsewhere must not be acted on before it is
  drawn.

## Decision

1. **The order before stays until the new one is made.** A pane's order
   is the one its listing shows (`Listing::order_shown`): its own for
   the spec asked when the listing has it; while one asked anew is made,
   the order the listing had before, still kept current as entries come
   and go. The header says the sort asked at once; the rows follow when
   the order is back. Everything that reads a pane's order reads this
   one, so nothing acts on an order not drawn.
2. **Made on a worker.** The pump asks for the sort
   (`Listing::sort_job`, through `Pane::jobs_wanted` and
   `PumpReport::jobs`, run by `App::pump_report` at `Priority::Visible`);
   the next drain after its wake takes the order (`take_sorted`),
   stamping the listing's version and what it shows, so the views take
   it. The window's thread sorts no folder anew.
3. **Without a copy.** The listing's entries are shared (`Arc`) with the
   sort alone, which lets go of them before sending its order; until the
   order is back the listing takes no events (they wait in its channel,
   and are no backlog to pump again for), so the next change finds the
   entries its own (`Arc::make_mut`) and copies nothing.
4. **The last asked wins.** A sort asked for another order while one
   runs lets the first go (its order is dropped when it comes). A
   listing changed directly under its sort is sorted again; a sort
   never run is asked again.
5. **Sorted on the window's thread still:** a listing that has no order
   to show yet (a search's first results), and one whose order no longer
   holds (an entry renamed or hidden in place, a companion arriving,
   companions shown or hidden): rare, and their order before would be
   wrong, not only another.
6. **Hidden files hidden leave the selection at once**, as before
   (`Pane::retain_shown` asks the spec, not the order): for the moment
   the order before still shows them, they are not selected.

## Consequences

- Sorting a large folder anew no longer stalls the window; the rows
  change once the worker is done (about 110 ms for 390 000 entries in
  the release build), the header at once.
- The listing's events (watcher, metadata) wait for that time.
- A direct sorted order of a listing (`Listing::sorted`: the agent's
  `list`, the bench, tests) is made where it is asked, as before.

## Not done

- The re-sorts of decision 5 stay on the window's thread.

## Amended 2026-11-12: an order that reads waits for its reading

- **Asked for** (2026-11-12): grouping `/nix/store` (385 113 entries)
  was very slow. Grouped by Modified, sorted by Name, its entries were
  stat-ed in 377 batches after the order was made (every entry in the
  group not read yet), and each batch was placed in it on the window's
  thread, a pass over the whole order each, its groups told by another:
  2 s of the window's thread in the release build, 5.5 s in 30 ms
  steps in the debug build, which took 13 s to settle. Reading the
  entries first and sorting once took 0.17 s.
- **Decision**, of four ways put to the user, who chose this one and the
  footer's count:
  7. **An order asked anew that reads what the entries shown lack** (a
     stat: a size, a date, an owner; the providers' pass: tags, a
     rating; `SortSpec::needs_metadata`) **is made once they are read.**
     The listing asks the reading itself, once it is whole
     (`Listing::reading_wanted`, from `Pane::jobs_wanted` for the
     listing shown and the folders opened in place; a `ListingJob::Read`
     asks `OpenLocation::hydrate` from a worker, after what is drawn),
     so the shell asks nothing (`sync_sort_hydration` is gone) and an
     agent's or a script's pane is read too. Until it is read, the order
     before stays (decision 1), and what moves in the order shown is left
     where it was (`SortCache::unplaced`); then the order is made whole
     on a worker (`sort_job`, `order_before`).
  8. **What was asked is told without a list of ids**: the entries are
     kept in the order they came, so it is everything before where the
     reading asked to (`Reading::asked_to`) that waited, and a count of
     those still waiting, kept as each is answered (read, unreadable,
     gone). Nothing is kept per entry. What comes after is asked at the
     next pump; what is read changing (another criterion, hidden files
     shown) or what is shown changing otherwise than by the entry (a
     companion's file came) counts again, what was read staying read.
     A location that never answered an entry it was asked would leave
     the order waiting, said in the footer: every location answers every
     id (AGENTS.md, "Entries arrive before metadata").
  9. **The footer says how far it went** once the reading has lasted
     200 ms, as "Loading…" waits: "385,123 items · Reading… 80,967 of
     385,095" (`loading::State::reading`).
- **Consequences**, measured on `/nix/store` (the machine loaded; same
  run compared): the 377 batches cost the window's thread 91 ms in all
  (1.1 ms the longest) where placing each cost 1.12 s, then one sort on
  a worker (0.2 s release). In the debug build, grouping by Modified
  takes the window's thread 38 ms in 262 syncs (0.5 ms the longest), and
  settles in 11.6 s, nearly all of it the providers reading 385 000
  entries (9.3 s) before one 2.2 s sort.
- **Not done**: what is read on demand (Taken, Dimensions, Duration,
  ADR-131) is still placed as it comes, 64 entries a batch. A sort by a
  stat waits for the providers' pass too (tags, comments), which it
  does not need: a reading of the stat alone would end sooner.
