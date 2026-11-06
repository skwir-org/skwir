# ADR-120: A folder appears whole

Status: accepted and built, 2026-11-06, on Linux; macOS and Windows
compiled only. Amends ADR-000 (principle 1: listings stream). The whole
sort moved to a worker the same day (decision 5).

## Context

- **Asked for:** a folder shown whole, as other file managers show one,
  rather than streamed in (2026-11-06). hyprfm lists `/nix/store`
  (about 390 000 entries) in under a second where Skwir took several:
  it reads and sorts the folder on a worker (`QtConcurrent`), then hands
  the view the whole of it at once.
- **A folder streamed in moves under the eye.** Each slice of entries
  pumped (`PUMP_BUDGET`, 4 ms) is placed in the order kept while the
  listing streams (a quarter of it at a time, `Listing::sorted`), the
  view drawn after each: rows shift as names that sort before them
  arrive, the count climbs, the scrollbar shrinks, and work is redone
  at each slice that a whole folder does once.
- **Measured on `/nix/store`** (392 580 entries, a window on Xvfb, three
  runs of each alternated, from the first frame to every row shown):
  streamed, 0.70 to 0.73 s in the release build, 2.64 to 2.72 s in the
  debug build; held back whole, 0.53 to 0.54 s and 2.50 to 2.52 s (the
  debug build past a second, so it streams from then). The UI thread
  spends less in all (294 ms of syncs against 367 ms, in the release
  build), but its longest sync grows from 36 ms to 110 ms (from 125 ms
  to 315 ms in the debug build): the one sort of the whole folder, on
  the UI thread.
- **Then asked for** (2026-11-06): that one sort on a worker, as hyprfm
  does it.
- **Holding the old view** until the new one is ready (as tabs do,
  ADR-018's 150 ms) was refused for navigation: a click that changes
  nothing for longer than that feels unanswered.

## Decision

1. **A folder opened shows nothing of it until it is whole.** The pane
   goes to the folder at once (its address, its trail, an empty view),
   and the folder's entries appear together when its listing completes,
   sorted once. The footer says nothing at first, then "Loading…" with
   the banner, after 200 ms as before.
2. **Past a second, it streams.** A folder not listed within
   `location::WHOLE_WITHIN` (1 s) shows what has arrived, and the rest
   streams in as before. A listing that fails shows what came.
3. **A search streams from the start** (`LocationProvider::searches`):
   its results are answers as they are found, not a folder's contents.
4. **The folders a pane keeps beside its own** (the columns above, those
   opened in place) appear whole the same way.
5. **Sorted on a worker.** Once a held folder is whole, failed or held
   long enough, the pump asks for its sort (`Listing::sort_whole`,
   `Pane::sorts_wanted`), run on the scheduler at `Priority::Visible`
   (`SortJob::run`); the window's thread never sorts a whole folder.
   The worker reads the listing's entries without a copy: they are
   shared (`Arc`) with the sort alone, which lets go of them before it
   sends its order, and the listing takes no events until the order is
   back (they wait in its channel, and are not a backlog to pump for):
   the next mutation finds the entries its own again
   (`Arc::make_mut`) and copies nothing. A listing changed directly
   under its sort is sorted again; a sort never run is asked again.
6. **The hold is a state of the listing** (`Listing::whole_within`,
   `is_held`), not a look at the clock: it ends when the drain after
   the worker's wake takes the order (`drain`), which stamps the
   versions what is shown moves, so every reader of a frame agrees and
   every cache keyed on them is made again. A held listing is not
   complete until then (`is_complete`): the banner, the footer, the
   empty folder's placeholder and a tab shown again wait for the folder
   shown. The window asks for a pump when the hold's time comes
   (`Pane::held_for`, `loading.rs`); what waits for an entry to be shown
   (a reveal, the column scrolled to its child) waits while it is held.
7. **Refreshing keeps what is shown.** F5 reads the folder again into
   the listing it has: it never goes empty.

## Consequences

- The UI thread no longer re-places a streaming folder at each slice,
  nor sorts it whole: opening `/nix/store`, its longest sync went from
  75 to 158 ms (the sort, three runs) to 33 to 45 ms in the release
  build, and from 310 to 322 ms to 116 to 129 ms in the debug build
  (where the folder streams past its second, and arrivals are placed as
  they always were). What is left in the release build is the listing's
  own tables growing as entries arrive (rehashed whole at each doubling,
  about 42 ms at 262 144 entries), as when the folder streamed. The
  sort's time on its worker is in the wait.
- Nothing is hydrated while a folder is held: no row is shown to ask
  for it. The rows shown first are asked for once they appear.
- A folder on a slow server shows nothing for its first second, then
  streams as before.
- While its sort runs, a folder's events wait: a change made in it
  then shows once the order is back.

## Not done

- **Sorting again on a worker.** A sort chosen anew (a column's header
  clicked) still sorts the whole folder on the window's thread: showing
  the order before until the new one is back needs the view to know the
  order it shows is not the one asked for.
