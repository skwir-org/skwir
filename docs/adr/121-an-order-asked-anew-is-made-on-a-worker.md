# ADR-121: An order asked anew is made on a worker

Status: accepted and built, 2026-11-06, on Linux; macOS and Windows
compiled only. Follows ADR-120 (a folder appears whole, built and
sorted on a worker).

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
