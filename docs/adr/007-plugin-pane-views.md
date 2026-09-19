# ADR-007: Plugin-controlled pane views and cached disk usage

Status: accepted, 2026-09-19

## Context

Usage was a global overlay backed by one scan. Entering a folder closed it,
opening it again rescanned the subtree, and split panes could not browse Usage
independently. The existing view registry was populated only at pane creation;
its toolbar and palette were hard-coded to List, Grid and Columns.

## Decision

Plugins contribute owner-tagged `ViewSpec` descriptors with a renderer name,
title, icon, order and atomic data revision. `ViewSpec::named` has no toolkit
dependency. Bundled renderers implement `PaneView` in `bibli-ui` and are resolved
by `views::named_renderer`. Opaque UI factories remain supported for base views.
The toolbar and palette discover available renderers through the registry;
unknown renderers are omitted. Narrow toolbars expose other views in More.

Panes reconcile registrations by factory identity, so removal drops the view
and falls back to List, and even a disable/re-enable between frames replaces
its state. View selection survives directory navigation. Producers publish
results, advance the revision, and wake the app; the UI queries data only when
the location or revision changes. GPUI stays outside core and plugin crates.
A public toolkit-facing plugin SDK remains deferred until there is an actual
external view consumer.

Usage owns a lazy worker and a disposable in-memory cache. Directory breakdowns
are stored independently, keeping the largest 64 immediate children and an
aggregate for the rest, at any scan depth. The cache evicts least-recently-used
breakdowns at 4096 directories or 16 MiB of estimated node/path payload. This
is not a total process-memory limit. The scan bounds temporary child lists and
checks cancellation while reading directories. Missing/evicted details are
computed on demand; unchanged siblings are reused during ancestor updates.

On a cache miss, Usage first lists the current directory's files and unknown
folders, then streams its child directories through the recursive scanner.
A second directory listing avoids retaining an unbounded queue of paths and
uses entry types to avoid stat-ing ordinary files twice. It is skipped for
folders containing only files. Only the visible breakdown and the running
branch's counters are needed for progress, including within a large subtree.
Snapshots have `incomplete` totals and striped blocks; the UI labels them as
counted so far, rather than implying that unknown folders are empty. Completed
subtrees enter the cache; partial or unreadable breakdowns do not. Filesystem
events queued during a scan reconcile changes between the two listing passes.
For a cold watch root, an initial shallow snapshot precedes root watch
registration. This snapshot is never cached; the scanner rereads the current
directory after registration to cover changes made during setup. Descendant
watches are installed as the cancellable scanner visits each directory, before
reading it, rather than asking the watcher backend to walk the whole subtree.
This retains the additional shallow pass on first open, without an uninterruptible
recursive registration phase. Subsequent progress snapshots are
throttled to the existing 200 ms interval. Progressive rendering is only used
until a location's first scan finishes. Later refreshes retain the finished
chart and replace it atomically when fresh totals are ready; automatic refreshes
publish no intermediate busy state. Explicit refresh may show delayed progress
over the retained chart. Snapshot completion is separate from total completeness,
since read errors can leave a finished result incomplete. Hover identity and
block colors survive size-based reordering.

Recursive coverage uses individual non-recursive watches, without following
symlinks. The scanner also registers newly discovered subdirectories before
reading them; directory replacement removes obsolete watch registrations.
Coalesced events invalidate affected subtrees and ancestor totals; only panes
whose paths intersect those events refresh, retaining their previous display.
Cache eviction alone is not a reason to refresh an unrelated pane. At most four
independent roots are watched; evicting coverage invalidates its cached data.
Recursive watch resource use also depends on the number of directories and OS
limits. Watch failures mark affected results stale and use a two-second
background revalidation fallback with a separate deadline for each active view;
unrelated events cannot postpone it or make healthy panes poll. Lost events
invalidate cached data and watch coverage; active roots attempt registration
again before scanning. Explicitly navigated directory aliases also use this
fallback, since an inode watch cannot
detect replacement of a symlink in an ancestor path. Interior symlinks remain
leaves. Local event accumulation is bounded; overflowing it invalidates the
whole cache.

Requests carry a pane, generation and cancellation token linked to that pane's
location lifetime. Navigation, Back and pane close cancel old work immediately,
without waiting for the UI to notice a different path. Completed child caches
remain usable; partial results never become completed cache entries. Cancellation
is checked before and after each watch registration and throughout traversal;
an already-issued operating-system I/O call must still return before that worker
can stop. Cancelled or replaced requests cannot publish results. The command
status distinguishes queued/running work (`scanning`) from visible progress
(`updating`): scan checkpoints publish
progress only after 200 ms, so cache hits and brief refreshes do not flash an
updating indicator. Closing a view releases its subscription;
disabling the plugin cancels the worker and drops all cached state and watches
on that worker, without joining it on the UI thread. No files or database are
created for the cache. Ctrl+R or the refresh button explicitly bypasses the cache.

## Consequences

Usage is a normal pane view with shared selection, file opening, previews,
context actions and navigation history. Its chart shows the current folder's
immediate children; smaller items can be reached through List, the address bar,
or normal selection commands. Totals include hidden files; the hidden-files
preference controls which individual blocks are shown. Allocated bytes retain
stat's semantics, including shared extents counted for each file; they are not
exclusive physical disk ownership. Unreadable entries produce an explicit
incomplete-total warning.

Adding a bundled Map or Gallery view requires a plugin descriptor and a UI
renderer, without changing the shell's view selector or command palette.
