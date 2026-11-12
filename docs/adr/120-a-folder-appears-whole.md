# ADR-120: A folder appears whole

Status: accepted and built, 2026-11-06, on Linux; macOS and Windows
compiled only. Amends ADR-000 (principle 1: listings stream). Built in
three steps the same day: a folder held back, streaming past a second;
its one sort on a worker; then the whole folder filled on a worker, and
the streaming past a second removed (this record says the last).

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
  arrive, the count climbs, the scrollbar shrinks, Select all and the
  arrows act on part of a folder, and work is redone at each slice that
  a whole folder does once.
- **Measured on `/nix/store`** (392 580 entries, a window on Xvfb, three
  runs of each alternated, from the first frame to every row shown):
  streamed, 0.70 to 0.73 s in the release build; held back whole and
  sorted on the window's thread, 0.53 to 0.54 s, but with one sync of
  107 to 158 ms (the sort). With the sort on a worker the longest sync
  was 33 to 45 ms: the listing's tables growing as entries arrived, its
  216-byte entries copied whole at each doubling (24 ms of 31 at 230 400
  entries, in a probe with the binaries' mimalloc; the name index's
  rehash 6.5 ms).
- **Holding the old view** until the new one is ready (as tabs do,
  ADR-018's 150 ms) was refused for navigation: a click that changes
  nothing for longer than that feels unanswered. (Amended 2026-11-12,
  below: the files alone held, 150 ms at most.)
- **Streaming past a second was dropped** (the user, 2026-11-06): while
  a folder streams one cannot really act on it, and keeping it would
  keep building folders on the window's thread, with the tables'
  growth, for the slowest and largest of them.

## Decision

1. **A folder opened shows nothing of it until it is whole.** The pane
   goes to the folder at once (its address, its trail, an empty view),
   and the folder's entries appear together, sorted, when its listing
   completes, however long that takes. (Amended 2026-11-12, the user's
   choice: the empty view drew one frame before a folder that came at
   once, a flicker, with groups the more seen. The files of the folder
   left stay drawn instead, for `tabs::HOLD` at most, while the address
   and the trail change at once; a folder ready within it takes their
   place directly, and any command ends the wait. The view draws them
   from the folder left's listing, its location closed
   (`Pane::drawn_entry`); no command finds them. The same day: a pane
   sorted or grouped by what a stat or the metadata providers read (a
   date, a size, an owner, tags) showed a folder first with nothing
   read, every file in one group, then moved them as rows were read;
   such a pane's folder is now read of every entry before its listing
   completes (`OpenContext::reads_metadata`, the folder provider's
   listing), and appears in its groups at once.) The footer says nothing at
   first, then, with the banner after 200 ms as before, "Loading…" and
   how many entries have been read ("Loading… 120 000 items",
   "Loading location… 120 000 items").
2. **Built on a worker.** A folder's listing is filled on the
   scheduler, not on the window's thread (`Listing::filled_elsewhere`,
   `FillJob`): the listing the pane holds stands for it and holds
   nothing (`is_held`) while short jobs apply what the location sent
   (`Listing::fill_job`, `Pane::fills_wanted`, `PumpReport::fills`, run
   by `App::pump_report` at `Priority::Visible`). Each job applies what
   is queued and returns: no job waits on a slow server, or holds the
   lane of the server it lists. Once the listing is complete, failed or
   sent elsewhere (`ListingEvent::Redirect`), the job sorts it in the
   pane's order and says it is done; the next pump takes it whole,
   under the same id, and applies what came after on the window's
   thread, as any change to a folder shown.
3. **What streams.** A location whose provider says it streams
   (`LocationProvider::streams`) is shown as it comes, on the window's
   thread as before: a search's results (`searches`, the default), and
   what is gathered from several sources, each an answer of its own,
   one slow source not holding back the others: the trash of every
   volume (a disk asleep), servers nearby.
4. **The folders a pane keeps beside its own** (the columns above, those
   opened in place) are filled the same way.
5. **Until taken, a held listing is not complete,** nor failed: the
   banner, the footer, the empty folder's placeholder, a tab shown again
   and `pump_until_complete` wait for the folder shown; what waits for
   an entry to be shown (a reveal, the column scrolled to its child)
   waits while it is held. Its events are its jobs': the pump applies
   none, and they are no backlog to pump again for.
6. **A failure shows what came.** A listing that fails is taken with
   what it read. A server that stops answering shows nothing, with the
   banner's ways out (Go Back, Disconnect), as before.
7. **A folder left is filled no further.** A job finds it is the last
   to hold the fill and stops; the listing is freed on a worker, as
   every listing let go of.
8. **Refreshing keeps what is shown.** F5 reads the folder again into
   the listing it has: it never goes empty.

## Consequences

- The window's thread does no work per entry for a folder until it is
  shown, whatever its size: no sort, no tables growing.
- A folder on a slow disk or server shows its count for as long as it
  takes, and nothing of it until it is whole.
- Nothing is hydrated while a folder is filled: no row is shown to ask
  for it. The rows shown first are asked for once they appear. But for
  a pane whose order reads it (amended 2026-11-12): the folder provider
  reads every entry's before it says the listing is complete.
- What streams (a large search, a large trash) is still built on the
  window's thread, its tables growing there.
- The listing filled on a worker is allocated there and freed on a
  worker; what the window's thread changes in it later is its own.

## Not done

- A sort chosen anew (a column's header clicked) sorted the whole folder
  on the window's thread: made on a worker since, the order before shown
  meanwhile ([ADR-121](121-an-order-asked-anew-is-made-on-a-worker.md)).
