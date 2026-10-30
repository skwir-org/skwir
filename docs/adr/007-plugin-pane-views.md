# ADR-007: Plugin-controlled pane views and cached disk usage

Status: accepted, 2026-09-19. Amended 2026-09-27 (view roles), 2026-09-29
(alternate views), 2026-09-30 (hard links), 2026-10-09 (nested map, map
modes).

## Context

Usage was a global overlay backed by one scan: entering a folder closed it,
reopening rescanned the subtree, and split panes could not browse it
independently. The view registry was read only at pane creation, and the
toolbar and palette hard-coded List, Grid and Columns.

## Decision

### Views from plugins

- Plugins contribute owner-tagged `ViewSpec` descriptors (renderer name,
  title, icon, order, atomic data revision). `ViewSpec::named` has no toolkit
  dependency; bundled renderers implement `PaneView` in `skwir-ui` and resolve
  through `views::named_renderer`. Opaque UI factories remain for base views.
- The toolbar and palette list the registry's renderers and omit unknown
  ones; a narrow toolbar offers the others in a menu.
- Panes reconcile registrations by factory identity: a removed view falls
  back to List, and a disable/re-enable between frames still replaces its
  state. The chosen view survives navigation.
- Producers publish, advance the revision, then wake the app; the UI queries
  data only when the location or revision changes.
- GPUI stays out of core and plugins. A public toolkit-facing plugin SDK
  waits for a real external view.

### Usage: cache and scan

- A lazy worker with a disposable in-memory cache; no files, no database.
- Each directory's breakdown is cached on its own, at any depth: its 64
  largest children plus an aggregate of the rest. LRU eviction at 4096
  directories or 16 MiB of estimated payload (not a process memory limit).
  Missing or evicted details are computed on demand; unchanged siblings are
  reused when ancestors update.
- On a miss, Usage lists the current directory's files and unknown folders
  first, then streams its children through the recursive scanner. The second
  listing avoids an unbounded path queue and uses entry types to stat files
  once; it is skipped when a folder holds only files. Temporary child lists
  are bounded, and cancellation is checked while reading.
- Snapshots have `incomplete` totals and striped blocks, labelled "counted so
  far", never implying unknown folders are empty. Progress needs only the
  visible breakdown and the running branch's counters.
- **Only completed subtrees enter the cache.** Partial or unreadable
  breakdowns never do; a finished result can still be incomplete (read
  errors), tracked apart.
- Snapshots are throttled to 200 ms and used only until a location's first
  scan finishes. Later refreshes keep the finished chart and swap it
  atomically; automatic ones publish no busy state, an explicit one may show
  delayed progress over the kept chart. Hover identity and block colors
  survive reordering.

### Usage: watching

- Coverage is non-recursive watches, not following symlinks. **The scanner
  installs each directory's watch as it visits it, before reading it; never
  ask the watcher backend to walk a subtree** (that cannot be cancelled).
  Directory replacement drops obsolete registrations.
- A cold root gets a shallow snapshot before its watch is registered; it is
  never cached, and the directory is read again after registration. Events
  queued during a scan reconcile the two listing passes.
- Coalesced events invalidate affected subtrees and ancestor totals; only
  panes whose paths intersect them refresh, keeping their display meanwhile.
  Cache eviction alone never refreshes an unrelated pane.
- At most four roots are watched; losing coverage invalidates its data. Watch
  cost also depends on directory count and OS limits.
- A watch failure marks results stale and falls back to revalidation every
  two seconds, with a deadline per active view that unrelated events cannot
  postpone; healthy panes never poll. Lost events invalidate data and
  coverage; active roots register again before scanning.
- Explicitly navigated directory aliases use that fallback too: an inode
  watch cannot see a symlink replaced in an ancestor. Interior symlinks are
  leaves. Event accumulation is bounded; overflow invalidates the whole cache.

### Usage: cancellation

- Requests carry a pane, a generation and a token tied to the pane's location
  lifetime: navigation, Back and pane close cancel at once, without waiting
  for the UI.
- Cancellation is checked around each watch registration and throughout
  traversal (an OS call already issued must return first). Cancelled or
  replaced requests cannot publish. Completed child caches stay usable.
- Status tells queued or running work (`scanning`) from visible progress
  (`updating`), published only after 200 ms, so cache hits and short
  refreshes do not flash.
- Closing a view releases its subscription. Disabling the plugin cancels the
  worker and drops its cache and watches there, without joining it on the UI
  thread.
- Ctrl+R or the refresh button bypasses the cache.

## Consequences

- Usage is a normal pane view: shared selection, opening, previews, context
  actions, history. The chart shows the folder's immediate children; smaller
  items are reached through List, the address bar or selection.
- Totals include hidden files; the hidden-files preference decides which
  blocks show.
- Allocated bytes keep stat's semantics: shared extents count for each file,
  so they are not exclusive physical ownership. Unreadable entries raise an
  incomplete-total warning.
- A Map or Gallery view needs only a descriptor and a renderer, no change to
  the view selector or palette.

## Amendments

### 2026-09-27: view roles

A descriptor states its `ViewRole`. A `Listing` view (list, grid, columns, a
future gallery) shows the folder's entries and has a toolbar button. A `Lens`
(Usage, a future map) answers a question with data it gathers; it is chosen
from the View menu, the palette or its key, with a button only while shown.
The toolbar keeps its width as lenses are added.

### 2026-09-29: alternate views (ADR-025)

An `Alternate` view (the tree) shows the entries in a shape chosen now and
then. It is offered like a lens, so the toolbar keeps only the shapes
switched between all the time.

### 2026-09-30: hard links

- A file counts once in each folder holding any of its links, as `du`
  counts; two sibling folders sharing it each hold it, so a folder's blocks
  may sum to more than the folder.
- Each folder keeps its linked files whose other links may lie outside it
  (dropped once all are found), so a total depends only on its subtree and a
  cached folder composes into any parent.
- Progress snapshots subtract what the branch shares with the tree before
  merging, and stay lower bounds.
- Reflinked or deduplicated extents still count per file: stat cannot see
  them.

### 2026-10-09: folders inside folders

- A status carries, inside the shown folder's entries, what their folders
  hold, up to three more levels, built only from cached breakdowns
  (`worker::nest`): no folder is read for it.
- A folder opens only at 1/256 of the shown folder's weight; entries under
  1/1024 join their folder's other items; at most 1024 entries, heaviest
  folders first. The map cannot draw smaller, and a status is copied and
  parsed whole on every change.
- A folder without a cached breakdown (being counted, unreadable, evicted, or
  scanned without monitoring, which caches nothing) is drawn closed.
- The view's zoom picks the levels drawn (one to four, default three; the
  window's, remembered with its shape). The treemap element decides in
  prepaint whether a block has room to draw open.
- Only the shown folder's entries are the pane's: a click inside one selects
  it and opens its menu, so the selection ring shows what the menu acts on.
  A double click (middle button: in a tab) shows the block itself
  (`ui.reveal`).
- Each level draws its whole folder (hidden entries while hidden files are
  off, and the other items, one block each), so no block exceeds its share.
- Arrows move to the neighbouring block (`treemap::neighbour`, from the last
  prepaint). The footer weighs folders as the map does (`PaneView::weighs`).

### 2026-10-09: map modes

- `usage::Mode` (`ui.usage_mode`; the window's, remembered with its shape):
  size by room (default) or by file count, or size by room and color by
  newest change or by kind; each mode has a legend.
- The scan gathers it all while sizing, and every kept breakdown keeps it:
  each file's kind by extension, as the desktop's MIME database types it,
  asked once per extension (13 µs a name against 2 µs a file for the walk; no
  extension, no kind); a folder's bytes by kind; each entry's newest change
  (a folder's is its newest file's, its own only when it holds none); the
  other items' file count and newest change.
- Cost, alternating builds: about 110 ns of CPU per file, some 5 % of a scan;
  the wall-time change is lost in noise.
- Folders of many small files are kept and opened by file count too: `trim`
  keeps the 16 holding the most files beyond the 64 heaviest; `nest` opens a
  folder holding 1/256 of the files.
- Reclaimable room is a dashed line inside the block in every mode: a cache
  (a folder with a signed `CACHEDIR.TAG`, the user's cache folder) or a trash
  (the home trash, or a volume's recognized by name, `trash::named_as_trash`,
  because asking volumes for their trashes reads each one's device). Each
  folder says how much it holds of these; the header sums it.
- With a selection, the line under the map says what Delete does to its
  room: the Trash keeps it on disk until emptied.

### 2026-10-30: soft tiles

- The user, shown mockups of the map (the current one, pastel colors
  alone, pastel tiles, neutral tiles with a dot of color): "let's go with
  B", the pastel tiles.
- Each block is a tile with rounded corners (6, 5 and 4 px by depth), a
  3 px gap of the ground between tiles. An open block is a tray paler
  than what lies on it, its name (semibold) and size in a 22 px header;
  its size only where its whole name leaves room, a name being worth
  more. A name cut to fewer than six characters (four in a header) is not
  drawn: "gar…g" said nothing. A size is drawn in the name's color, never
  fainter: it is the data.
- Fills are Oklch, one lightness for every hue (`theme::Blocks`): pale
  tints under dark text in the light look, deep muted ones under light
  text in the dark; a test holds the text at 4.5:1 on every fill of
  both. Size and Files spread eight hues around the wheel (the accent's
  shades, before, were 86° wide: neighbours differed by a shade); Age is
  one warm hue, strongest when new, fading to the ground when old; Types
  keeps a hue per kind.
- A hue follows its entry, never its rank by size: each of the folder
  shown's entries takes its file number's, mixed (`usage::hue_of`), or
  the next one free, served in the order of their numbers
  (`usage::hues`), so up to eight entries wear one each, and keep it as
  the map reflows.
- The block under the pointer is tinted with a line inside its edge; a
  chosen one is ringed in the accent, outside it, a band of the ground
  between. The pointer between two tiles is on one of them: a block is
  hit by its tile and half the gaps around it.
