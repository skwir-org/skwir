# Skwir — Implementation Plan

Skwir is an experimental file manager in Rust: a GPU-accelerated UI
(gpui), a tiny core, everything else a plugin, built to be driven by
humans, scripts and AI agents alike. This file holds the phases with
their goals and **Done when** gates, the status of each, the deviations
from the plan, what is left, and the plans not yet built. Decisions are
in `docs/adr/`. The dated record of what was built, with its
measurements and what was checked, is `docs/STATUS-LOG.md`: work adds
its entry there and says here only what changed in a status and what is
left.

Durations are relative sizes for one developer working with an AI
assistant, not commitments. A phase starts once the previous gate
passes, unless a deviation below says otherwise.

## Principles

Binding, from ADR-000 (AGENTS.md has the working rules):

1. **Always responsive:** no blocking work on the UI thread; everything
   streams, everything cancels.
2. **Light:** idle memory and startup are budgeted and measured
   (ADR-000, revised by ADR-002 and ADR-033).
3. **Stateless and idempotent:** no daemon, no database, one deletable
   cache; re-applying a plan is a no-op.
4. **Harness the environment:** OS thumbnails, icons, the MIME database,
   bookmarks, indexes, installed tools.
5. **Small core:** only what two plugins must agree on; everything else
   is a plugin with a runtime toggle.
6. **Agent-ready:** commands are data, plans before apply, stable
   identity (ADR-049), metadata with provenance.

## Current status (2026-11-05)

- **Where:** named Skwir (skwir.org, application id `org.skwir.skwir`),
  version 0.3.1, released 2026-11-05 on GitHub and the AUR's three
  bases; 0.3.0 was released 2026-11-03, 0.2.0 2026-10-20, 0.1.0
  [released](#release-01-linux) 2026-10-13. Phases 0 to 4, 5a and 6 pass their gates. Maps and Notes
  everywhere are built, their gates not closed, as is much that no phase
  planned ([Built beyond the plan](#built-beyond-the-plan)).
- **Next:** skwird's steps (ADR-050) and Phase 7, which the user put
  after the release (2026-10-07). [Remote folders](#remote-folders)
  were built on 2026-10-21 (ADR-077): what they leave is in their
  section. [What is left](#what-is-left) gathers
  the rest. From 0.3 a release's page is its changelog section above
  `### All changes` (docs/RELEASING.md), kept as short as 0.2.0's page
  (0.3.0's ran to 8 KB and its draft was shortened by hand).
- **Blocked:** Phase 5b needs a Mac and a Windows machine
  (`docs/HELP-WANTED.md`). A Windows build to try is made on Linux
  (ADR-074, `just package-windows`, 2026-10-20): a zip and a per-user
  installer, tried under Wine, its window never drawn there. A macOS
  build to try is made on Linux too (ADR-075, `just package-macos`):
  skwir.app for Apple silicon, looked over, never run. Both carry the
  archive helper, libarchive built on Linux and linked in statically
  (2026-10-20, `tools/package/libarchive.sh`), and the media helper with
  GStreamer's own build beside it (ADR-076, `tools/package/gstreamer.py`). Both were added
  to 0.2.0's release page (built from 5a0bb2b); 0.3.0's `release-build`
  made them with the Linux packages. The user reports
  hosted CI is too slow; routine CI is reduced to essentials, with full
  checks and package builds kept as manual fallbacks. The manual full
  workflow, whose `native` job tests on macOS and Windows runners, has
  never run (no run on 2026-10-20).
- **Budgets:** idle own share **103.8 MiB of 100, over**: 0.3.1's
  tarball on the session's display (quiet, 2026-11-05), where 0.3.0's
  read 103.2 the same evening (103.7 on 11-03) and 0.2.0's 91.3. 0.3.0's
  code grew by a quarter and most of it is resident at idle; the user
  released 0.3.0 and 0.3.1 so, and the work is under [What is
  left](#what-is-left). First frame of 150 ms: the least 147 ms for
  0.3.1's tarball and 116 for 0.3.0's (nine starts each, alternated),
  medians 159 and 154. `just bench-check` meets all 51 budgets
  (2026-11-05, quiet).
- **Since 0.3.1 (2026-11-06):** About's buttons on one line; a shared
  slider (the seek bar, Settings' sliders); the window's grounds each
  painted once, then see-through at the opacity chosen (ADR-119); a
  drag's name under its icon, inside the window and out; names broken
  where they read best; a folder appears whole, streaming only past a
  second (ADR-120, amends ADR-000's principle 1).
- **Work (2026-11-04 to 11-05) in 0.3.1:** text sizes by part
  from the desktop's (ADR-117), Fonts from and Colors from, twelve
  palettes and the desktop's colors on macOS and Windows (ADR-071
  amended), Skwir's own fonts carried in every package (ADR-118),
  Breeze's icons in the color scheme's colors, a variable font's weights
  (gpui patch 0045), Usage selecting inside folders (ADR-116), what a
  grammar search found and refused (ADR-110); the review since 0.3.0
  fixed (78 findings, [below](#review-fixes-2026-11-05)).
- **Work in 0.3.0 (2026-10-21 to 11-03):** remote folders
  (ADR-077 to ADR-088): servers mounted by sshfs, GVfs, KIO or rclone,
  configured from Skwir, found nearby, copies in batches retried from
  where they stopped; phones and cameras listed before they are mounted,
  KDE's reached through kmtpd (ADR-096, ADR-097); the address field as
  one field (ADR-098); menus with submenus (ADR-099); F1 by topic
  (ADR-100); a preview reading its file once (ADR-101, ADR-102); text a
  screen at a time (ADR-103); FB2 and Mobipocket books (ADR-106);
  filters kept until taken off, and places with their filters (ADR-107
  to ADR-109); tree-sitter grammars (ADR-110); printing (ADR-111);
  smaller copies (ADR-112); speed and time left, the footer's strip and
  Activity, finishing without the window (ADR-113 to ADR-115); the
  Undo history (ADR-079 amended). gpui from Zed's main of 2026-10-28,
  Rust 1.99.1, PDFium chromium/8125, every dependency at its newest on
  2026-11-02. The reviews of 10-23, 10-28, 10-30, 11-01, 11-02 and 11-03
  are fixed; what they leave is under [What is left](#what-is-left).
  Earlier work, day by day: `docs/STATUS-LOG.md`.

| Phase or plan | State | Planned size |
|---|---|---|
| [Phase 0: Foundations and toolkit spike](#phase-0-foundations-and-toolkit-spike) | gate passed 2026-09-18 | ~2 weeks |
| [Phase 1: Core kernel](#phase-1-core-kernel) | gate passed by 2026-09-18 | ~4 weeks |
| [Phase 2: Minimal daily-driver](#phase-2-minimal-daily-driver) | gate passed (a week of daily use; recorded 2026-10-04) | ~4 weeks |
| [Phase 3: Visual richness and performance hardening](#phase-3-visual-richness-and-performance-hardening) | gate passed by 2026-09-18 | ~4 weeks |
| [Phase 4: Power features, each as a plugin](#phase-4-power-features-each-as-a-plugin) | gate passed 2026-09-18, holds 2026-10-04 | ~6 weeks |
| [Built beyond the plan](#built-beyond-the-plan) | built 2026-09-18 to 2026-10-19 | — |
| [Maps](#maps) | steps built 2026-09-27; gate not declared | — |
| [Notes everywhere](#notes-everywhere) | steps built 2026-09-30; gate in part (no Mac) | — |
| [Phase 5: Cross-platform](#phase-5-cross-platform) | 5a gate passed 2026-10-06; 5b waits for machines | ~6 weeks |
| [Phase 6: The extension boundary](#phase-6-the-extension-boundary) | gate passed 2026-10-07 | ~4 weeks |
| [Release 0.1: Linux](#release-01-linux) | released 2026-10-13 (0.1.0) | — |
| [Names indexed by skwird](#names-indexed-by-skwird) | C built 2026-10-20; A, B and D wait for skwird (ADR-050) | — |
| [Remote folders](#remote-folders) | A to F built and checked on Linux 2026-10-21 (ADR-077); G's list for the night of 2026-10-23 built (ADR-080 amended, ADR-084); devices listed before they are mounted 2026-10-26 (ADR-096); what is left in G | — |
| [Colors and font from the desktop](#colors-and-font-from-the-desktop) | A to D and the fonts by role built 2026-10-19 and 20; E (macOS, Windows) and Skwir's palettes (F) built 2026-11-05 (ADR-071, ADR-073) | — |
| [Indexing and AI (Phase 7)](#indexing-and-ai-phase-7) | designed and accepted 2026-10-26 (ADR-089 to ADR-095, `docs/indexing-and-ai.md`); nothing built | ~10 weeks |
| [Phase 8: Polish and release](#phase-8-polish-and-release) | planned, ongoing | — |

---

## Review follow-up (2026-10-23)

The project review's fourteen findings are fixed in separate local
commits. The completed set passes the full release gate; regressions,
additional checks and platform limits are recorded in `docs/STATUS-LOG.md`.

| Issue | Work | State |
| --- | --- | --- |
| 1 | Content and metadata proof before skipping a copy or removing a moved source | fixed; full release gate passes |
| 2 | File-operation self checks through path aliases | fixed; full release gate passes |
| 3 | Merged-move undo restores directory attributes | fixed; full release gate passes |
| 4 | MCP authority belongs to its endpoint | fixed; full release gate passes |
| 5 | Privacy follows symlink targets | fixed; full release gate passes |
| 6 | Headless settings save completes before exit | fixed; full release gate passes |
| 7 | Explicit open targets are read off the window thread | fixed; full release gate passes |
| 8 | Remote search work uses its server lane | fixed; full release gate passes |
| 9 | Rclone tree copies honor pause and cancellation | fixed; full release gate passes |
| 10 | Search updates discover arriving directory descendants | fixed; full release gate passes |
| 11 | Replaced field providers cannot repopulate current memory | fixed; full release gate passes |
| 12 | glTF buffers stay within the model directory | fixed; full release gate passes |
| 13 | glTF buffer URIs are decoded as URIs | fixed; full release gate passes |
| 14 | Rsync arguments use its own grammar | fixed; full release gate passes |

## Review fixes (2026-10-24)

The whole-project review of 2026-10-23 (at `b7643e46`: thirteen areas,
182 findings with the reviewers' side notes, 6 high and about 50
medium) was fixed overnight in local commits, one per fix or small
group, none pushed. Each fix was linted and tested in its crate as it
was made; the full checks ran at the end (`docs/STATUS-LOG.md`,
2026-10-24). A finding judged wrong or not worth a change says why;
what is left is under "What is left", "From the review of 2026-10-23".

| Area | Fixed | In part | No change | Left |
| --- | --- | --- | --- | --- |
| Commands, tooling, docs | 20 | | 1 | 1 |
| Core: file operations | 16 | | 1 | |
| Core: kernel, settings, the binary | 30 | 1 | 1 | 4 |
| Platform, plugins from outside, helpers | 17 | 1 | 1 | 1 |
| Other plugins | 13 | 1 | | |
| Search, tags, notes, remote | 19 | 1 | 1 | 1 |
| Window: shell | 20 | | 2 | |
| Window: views, previews, maps | 20 | | | 9 |

The six highs are fixed: typed commands refuse arguments they do not
take (a misspelled `entries` acted on the selection); PowerShell's
typographic quotes are doubled (checked against pwsh 7.6); Undo of a
copy where no trash is kept removes only what the copy placed; CSS,
SVG and STEP nesting is bounded (their tests abort with a stack
overflow without the bounds); the Wine harnesses leave the desktop
alone (the test prefixes' menu entries and types were removed from the
user's folders, by their choice). Decided on the night: a toast's Undo
takes back its own plan (ADR-079 amended); `player.request` stays
unasked for agents (commands.md); the PKGBUILDs' Maintainer lines stay.

## Review fixes (2026-10-28)

Before 0.3.0, nine reviewers read what changed since the review of
2026-10-23 (`b7643e46..6248aa2a`) and the release's consistency: about
105 findings, 6 high. Five fixers, each owning its files, fixed them in
one tree; the integrator checked and committed them by area
(`docs/STATUS-LOG.md`, 2026-10-28). What is left is under "What is
left", "From the review of 2026-10-28".

| Area | Fixed | In part | Not a bug | Left |
| --- | --- | --- | --- | --- |
| Window: address field, menus, forms | 15 | 1 | | 1 (to the integrator) |
| Views, tags, plugins | 25 | 3 | 2 | 2 |
| Core, operations, query, helpers' host | 19 | 1 | 2 | 1 |
| Remote folders and the phone helper | 28 | 1 | | 1 |
| Release: changelog, docs, packages | 12 | | | 1 (the screenshots) |

The six highs are fixed: rows offered before typing run; drops and
pastes onto the Trash and the Shelf pass the argument check; what an
agent's call opens is checked once resolved, not as written; Windows'
logon task's rclone says its port; the package checks accept the phone
helper's honest "no FUSE here"; the changelog covers 0.3.

## Previews read once; rclone keeps what it reads (2026-10-29)

A Dropbox remote mounted by rclone previewed a tiny text file slowly: a
preview opened its file at every step (13 opens for a 55-byte text, 4 for
a `.docx`, 5 for a JPEG), each a download on a mount that keeps nothing
it reads. ADR-101: a preview reads its file once into what the request
holds (`preview::Held`), every provider and step reads that, each
provider's `accepts` is asked once, the file shown before is kept a
minute, a remote's large text is counted as far as it is shown. ADR-102:
rclone mounts keep what they read (`full`, by an Automatic rule that
keeps `minimal` on the local network and for wrapping remotes), and a
copy onto a mount that sends later waits for the server, as onto a
removable drive. Measured in a window over rclone's `minimal`: one GET
per preview of each kind, none flipping back to the file shown before
(a JSON file took two at first: Properties' Opens with typed it from its
bytes again, a name several types claim; it is now given the type the
preview settled, `PropertySource::typed`). Detail in `docs/STATUS-LOG.md`
(2026-10-29); what is left under "What is left".

## Text read a screen at a time (2026-10-29)

A book, a document, a page in reading mode or a message can turn a
screen at a time, as the document reader turns pages (ADR-103): its own
choice (`reading.text_continuous`, the bar's Continuous scrolling); each
screen ends at a whole line and the next starts with the line after it;
Back retraces the screens turned; a notch of the wheel, the arrows and
Page Down turn, and past a chapter's last screen comes the next chapter.
Screens are not counted, so nothing is laid out ahead. Checked in a
window on Xvfb (`docs/STATUS-LOG.md`, 2026-10-29); what is left under
"What is left".

## Performance review and fixes (2026-10-29)

Six reviewers read the whole code (about 430,000 lines) for what grows
faster than its input and what costs CPU, memory, disk, syscalls,
network or wakeups: 59 findings, ten quadratic at sizes Skwir meets (a
folder of 100,000 entries and a selection of all of it, a tag on 100,000
files, a solid comic, a CSV of millions of rows), none exponential. Six
fixers, each owning its files, fixed 53 of them (one in part), then four
of the six first judged not worth it, in one tree; the integrator committed them fix by
fix (`a3d100ec..`; detail in `docs/STATUS-LOG.md`, 2026-10-29). Three
bench cases measure what was quadratic. Two decisions closed doors:
ADR-104 (a copy across a network is found whole by length and time) and
ADR-105 (an archive shown keeps its helper, amending ADR-101). AGENTS.md
gained a Scale rule.

| Area | Fixed | Dropped |
| --- | --- | --- |
| Core: listings, selection, commands, watcher | 12 | |
| Operations and platform | 6, and ADR-104 | 2 |
| Window and views | 8 | 1 |
| Previews and their helpers | 10 (one reduced), and ADR-105 | |
| Search, tags and the shelf | 11, and a race found | 2 |
| Remote folders, maps, tables | 6 | 1 |

| Bench case (flat100k, release) | Before | After |
| --- | --- | --- |
| `entry_at_selection_us`: a whole selection's entries found | 100.3 s | 43 ms |
| `delete_tail_us`: 10,000 of 20,000 files deleted for good | 15.2 s | 0.20 s |
| `tags_forget_us`: a folder of 100,000 tagged files moved away | 4.6 s | 1 to 6 ms |

## Books beyond EPUB: FB2 and MOBI (2026-10-30)

The books plugin (id `books`, once `epub`) reads FictionBook 2 and
Mobipocket books as it reads EPUB (ADR-106): one reading path for every
book, one engine choice. An FB2, plain or zipped, is translated chapter
by chapter into XHTML for reading mode, its notes opening under their
references; MOBI 6, KF8 (AZW3, and the KF8 part of a file holding both)
and PalmDOC texts are read by Skwir's own code (PalmDOC's and the
Huffman decompression, the contents, skeleton and fragment indexes),
with their contents, pictures and covers. Automatic is the built-in
engine; MuPDF, chosen, reads the kinds it reads well, told from the
file before it opens, and leaves the others to the built-in engine.
Encrypted books and KFX say what they are. Fixtures are synthetic,
made by `tools/books/fixtures.py` with calibre. Detail in
`docs/STATUS-LOG.md` (2026-10-30); what is left under "What is left".

## A lens stays until it is taken off (2026-10-30)

What of a pane's filter or search the listing can decide of a file (a
kind, an extension, a tag, a date, a size, a rating) is its lens
(ADR-107): kept wherever the pane goes until a pill's × or Escape takes
it off, a folder, the trash and a tag's place filtered by it, a search
holding it in its own terms, a tab or a split opened from the pane
taking it; the words that find names go with the location. One filter
state in the core (`Pane::lens`); the folder narrowing of 2026-10-03 is
gone, and a tag's place is its tag and depth alone. Narrow by became
Quick filters, shown over any location when asked for and saying why
when it offers nothing; filter pills wear their facet's icons. Built on
a branch beside the FB2 and MOBI work of the same night; checks in
`docs/STATUS-LOG.md` (2026-10-30), what is left under "What is left".

## A tag's place is filtered at its depths (2026-10-30)

Every tag's place is a scope, the top too (ADR-108): the address field's
switch reads its depths ("This tag | With sub-tags", "These tags | Every
tagged file", Include Sub-Tags off and on), words filter it at both,
and what only a search tells searches the place as it is shown on
Enter. The field asks the location its depths (`LocationProvider::depths`);
a folder has none. A search within a place now completes once each entry
was tested. Checks in `docs/STATUS-LOG.md` (2026-10-30).

## Review fixes (2026-10-30)

A review of the day's work (books, the lens, a tag's depths, from
ca394b1a) made 39 findings, fixed the same day with an older defect the
property tests found on the way: a crafted FB2 or MOBI could abort skwir
or take gigabytes (entities, an index count, the Huffman dictionary), the
grid read every `.pdb` on the window's thread, Enter over a search shown
as typed opened it again, a lens came back where it was taken off, a
search within a place could never complete, and an undo after a swap
renamed the wrong file. ADR-107 and ADR-108 are amended; the rules are in
the invariants and AGENTS.md (a hydration answers every id). Checks in
`docs/STATUS-LOG.md` (2026-10-30, "Review fixes").
The same evening four gaps it left were closed: tests that failed under
load wait for what they mean (two product defects found on the way: a
map scan's count, rclone names looked up as hosts), the field's step is
known by its place in the history, a relative date's filter rolls over
at midnight, and a batch's answer holds for its step alone
(`docs/STATUS-LOG.md`, "Known gaps closed"). Then what those test fixes
found: a plan's last files written through rclone waited for until
sent, a failed listing keeping the servers, rclone's SIGTERM exit a
unit's stop, the tests' trash pruned ("The test fixes' leftovers
fixed").

## Review fixes (2026-11-01)

A review of what came after the last round (c4c7ca0c..a56c120d:
tree-sitter grammars, printing, places, the UI's smaller pieces) made 63
findings, fixed the same day by six fixers at once
(29d23c58..ddcee80f), one skipped as unable to happen (a search that
does not read never holds a lens). Three were high: printed marks and accents mirrored across the
baseline, phone photos printed sideways, and Settings reading grammars
on the window's thread. Among the others: a grammar whose highlights
fail gives way to bat, the helper's paths are checked and its compiled
grammars kept in the cache (ADR-110), the print dialog's scale is
applied and only the pages picked are made (ADR-111 revised), a remote
folder's saved place is kept by its address, the last window's tabs come
back filtered and Keep without the query plugin bookmarks plain
(ADR-109 amended), and a refusal of attributes is remembered for a FUSE
mount only. Checks in `docs/STATUS-LOG.md` (2026-11-01, "Review fixes").

## The address card fits the window (2026-11-02)

The user found that `tag:` in the address field, over a folder of many
tags, ran its card off the window, with nothing to scroll. The card now
never leaves the window: no wider than it, no taller than what it leaves
under the field, its rows scrolling there under a scrollbar between the
depth line and the footer, which stay in sight. Its rows are a list that
builds only those in view, so two thousand tags cost what a screenful
does, and the row the keys reach is brought into view with its section's
title (`docs/invariants/shell.md`). Every long list of the card gains it:
a kind's values, the tabs before anything is typed, any list in a short
window or the lower of two panes. Checks in `docs/STATUS-LOG.md`
(2026-11-02, "The address card fits the window").

## Every tag offered, strips as wide as they draw (2026-11-02)

Two things left by the address card's work, done the same day. `tag:`
and `#` offer the folder's tags first, then every other tag the user
has, from the tags plugin's memory (`tags.known`), in every scope; no
file is read while a key is typed (`docs/invariants/locations.md`). A
strip of tags builds and measures only the chips its width may draw
and the one "+N" that fits: a file's seventy tags once laid out seventy
chips and sixty counts on every row at each frame, and a row now costs
what it shows, whatever the number of tags
(`docs/invariants/views.md`). Checks in `docs/STATUS-LOG.md`
(2026-11-02, "Every tag offered").

## Cancel within a file, progress as it comes (2026-11-02)

Reported by the user: Cancel during a copy of large files between two
SFTP servers said "Stopping…" while the copy went on to the end, the
button doing nothing; a copy from the same server just after stood
still at its start, at 80 MiB and at a file's end. Skwir's own batch
copy answers Pause and Cancel between a file's chunks, a large remote
file's readers between their pieces (a quarter second's worth each), so
Cancel lands within a second or two on a slow server
(`docs/invariants/operations.md`, "Running"). What a large file's
ranges bring shows as moved as it comes, not once written in order: the
stalls were the bar's, the bytes coming throughout (ADR-113, amended).
rsync's progress is read as it comes, not a second late; a plan says it
finished once its staging folders are gone; a card stopping says so,
its Cancel inert (`docs/invariants/shell.md`). Checks in
`docs/STATUS-LOG.md` (2026-11-02, "Cancel within a file"). The same day,
asked by the user: a batch's items count as each comes whole, before
their steps place them at its end ("Items counted as they come").
Left: sftp's sessions, gio, kio and rclone tell none as they go, so
theirs count when their run returns.

## Activity, docked (2026-11-02)

Asked by the user: the toasts piled up over the files and hid the last
rows. Brainstormed on a page of mock-ups (three places, what other
programs do, the desktop's notifications); the user chose each point
(ADR-114). What runs is a strip at the end of the last pane's footer,
as the player's is: how far, how long, Pause and Cancel for one action,
in as many words as the footer has room for. A click or Ctrl+Shift+J
opens Activity above it: each action's card, what waits, and Earlier,
this session's answers with Undo or Retry. Pinned, Activity stands
above the footer while something runs, taking room rather than
covering the files. An action an error stopped stays in the strip,
red, until retried, seen or dismissed. One answer at a time, at the
last pane's lower right. A long action that ends, stops or waits for its
server while the window is in the background is a notification, taken
back once the window has the focus; a click brings the window forward
(gpui patch 0043, the desktop's activation token) with Activity open.
The taskbar's entry shows how far what runs has gone, and the machine is
kept awake meanwhile (`skwir_platform::desktop`). Rules in
`docs/invariants/shell.md` ("Feedback"); checks in `docs/STATUS-LOG.md`
(2026-11-02, "Activity, docked"). Left: a real desktop (GNOME's and
Plasma's notification servers and taskbar, a Wayland compositor taking
the token), Windows' toasts and taskbar, macOS at all; KDE's job tracker
(`org.kde.JobViewServerV2`, Dolphin's way to Plasma's tray) set aside in
ADR-114.

## What runs outlives the window (2026-11-02)

Asked by the user: what happens to actions under way when Skwir is
closed mid-way, what other file managers and browsers do, and whether a
taskbar icon should show them finishing. Compared in the terminal
(Nautilus, Nemo, Dolphin, Explorer, Finder hold their work; browsers ask
and cancel); the user took points 1 to 6 (ADR-115). Closing the window
stops nothing: Skwir finishes without it, its panes asleep, then ends.
Quit asks while something is under way: Finish in the background (the
default), Stop and quit, Keep the window. Meanwhile an icon in the tray
or notification area (a notice that stays where there is no tray, the
Dock on macOS) shows it, its badge filling, with a menu (Open, Pause all
or Go on, Stop all and quit). A later launch is handed to it and its
window opens again with what runs (`skwir_platform::handover`). Paused,
stopped by an error, waiting for a server or asking a question, it waits
for the user. Log out and shutdown are held off with the action's words;
a signal to end stops cleanly; macOS asks first (gpui patch 0044). Rules
in `docs/invariants/shell.md` ("Feedback") and `operations.md`; checks
in `docs/STATUS-LOG.md` (2026-11-02, "What runs outlives the window").
Left: real tray hosts (Plasma, waybar, GNOME with AppIndicator), GNOME's
own notification and session dialogs, the portal's inhibitor on a real
desktop, a Wayland compositor taking the token on reopening, Windows'
notification area and shutdown screen on real Windows, macOS at all
(the Dock's click, `applicationShouldTerminate:`), a plan waiting for its
server without a window, two graphical sessions sharing one user bus
(a launch in one handed to a Skwir finishing in the other); going on
after a restart set aside in ADR-115.

## Beside a long copy, and Cancel for what waits (2026-11-02)

Asked by the user: a folder made beside a multi-gigabyte file copied
from a server onto a USB disk, in the folder it was copied into, waited
for the whole copy; and actions waiting their turn had no Cancel. A
folder made costs that kind of disk nothing, measured while it was
written at full speed. Two actions now meet in one folder only on one
name, its case aside; a copy, a move, a link or what a plugin makes
claims its own name, the names it may be kept as beside one taken
(`name (copy).ext`…) and its staging folders, whether it runs or waits
(ADR-079, amended). In Activity each action waiting has its Cancel, and
Cancel all stands over several (`Executor::cancel_waiting`): it ends at
once, having done nothing. Rules in `docs/invariants/operations.md`
("At once", "Running"); checks in `docs/STATUS-LOG.md` (2026-11-02,
"Beside a long copy"). Left: the measure was of one USB disk (btrfs in
LUKS, nearly full); an archive extracted still claims the whole folder
it extracts into (the name it makes is read from the archive); a row of
what waits moves up under the pointer as what is ahead of it starts.

## Review fixes (2026-11-02)

A review before 0.3.0 of what came after the last round
(d14e4213..899c3622: smaller copies, progress and time left, Activity
docked, what runs outliving the window, booklets of four and eight, the
address card, every tag in the field) made 60 findings, fixed the same
evening by seven fixers at once (b943a6b2..b91b1206; pulldown-cmark and
async-executor updated for an advisory and a yank). Four were not
changed, with their reasons: a clone's bytes stay in a toast's total
(ADR-113 says what the copy amounts to), Plasma's logout cannot be held
off by anything Skwir asks (ADR-115), a notice type that duplicates
another is a cleanup for later, and a tag strip's copy of its row's tags
adds nothing to a look built each frame. Three were high: a PDF
protected by an owner password alone was made smaller without its
protection, a window closing could take the core's events and questions
with it (what runs is the program's for its life now), and the thread
telling the desktop could spin once the bus came back while the tray
was gone. Among the others: a smaller copy is checked on its first and
last pages and in small squares of each page, and hostile PDFs are read
within bounds (ADR-112); a size asked tries the qualities below the one
chosen first; a copy's bytes, items and time left stay right across
batches, failures, holes and pauses (ADR-113); a print the portal did
not confirm is said to be handed on, only KDE's 2 taken as sent
(ADR-111); sleep is held off through the portal where a session manager
runs (ADR-114); a later launch is handed only to the user's own Skwir
on Windows (ADR-115); `tag:` keeps its list and offers 200 at a time.
The window checks found two more, fixed with them: a program finishing
without its window did not end after its last action, and the strip
drew its buttons over Undo in a narrow footer; the full test runs one
more: a login unit's clean-up left a mount its program had just left,
asked by a stat FUSE answers from memory (statfs now). Checks in
`docs/STATUS-LOG.md` (2026-11-02, "Review fixes").

## Queue progress and the History (2026-11-03)

Asked by the user: the strip's bar counted only what ran, so it went
back to nothing between two copies waiting their turn; and Undo took
only the last action, or a toast's, refused when pressed when it could
not be. Proposed on a page and chosen whole (the recommended option of
each of nine). Three steps:

1. **The history's rule answered ahead** (ADR-079, 13). Built: each
   journal entry keeps the entries after it that touched what it did,
   worked out once as it joins its stack, out of the journal's lock;
   what an entry keeps of its footprint is bounded to 1,000 claims, only
   ever wider, with the folder its paths are in and the names right
   below it, which tell two apart at once (`footprint::Kept`). Undo and
   Redo of an entry by its place, Undo with what came after (newest
   first, each once the one before ended), and `history` for agents.
   Since the review of 2026-11-03, what joins, runs or waits is compared
   by its footprint whole with what the entries before it keep, so a
   bounded footprint holds only its own entry's Undo, and each plan under
   way keeps the entries it meets, so the History reads sets and compares
   nothing. Rules in `docs/invariants/operations.md` ("At once").
2. **The History panel** (`crates/ui/src/history.rs`). Built: the
   footer's Undo split in two (Undo of the latest; the chevron, a right
   click and Ctrl+Alt+Z open the History), each row's Undo or Redo, the
   reason it waits or cannot on its second line, the action that holds
   it a link to its row, "Undo both"; Earlier's Undo answers the same
   way. The Menu key on the footer's Undo was left out: the footer is
   no stop of the Tab order. Rules in `docs/invariants/shell.md` ("The
   History").
3. **The queue's progress** (ADR-114, amended). Built: the strip, the
   taskbar and the presence count the spell of work, each action one
   (ended whole, running by its share, waiting nothing), "2 of 3 · 47 %
   · … · 1 waiting"; Activity's header draws a segment an action.
   Counted in the window's `Work`, where cards are decided, not in the
   core as proposed (`running` for agents unchanged). Since the review
   of 2026-11-03: a task that ends counts whole, an action cancelled
   before it did anything leaves the count, an action going on by itself
   or retried counts once, and whether an action counts is one test, the
   core's (`counts` on `Started` and `Waiting`).

Checks in `docs/STATUS-LOG.md` (2026-11-03, "Queue progress and the
History").

## The start made plain, Undo and Quit in More (2026-11-03)

Asked by the user: they kept wondering whether an action asked had
started. A page compared the strip alone with a sign where copied files
arrive (ghost rows in place, or an "Arriving" band); the user chose the
strip alone, and Undo, Redo, the History and Quit in More.

- **Never silent** (ADR-114, amended again): an action still under way
  0.3 s after it joined is shown and counted whatever its steps; before
  its first step or byte it says "getting ready…"; while only something
  waits, the strip says what for.
- **Announced**: once, its own sentence on the selection's tint for 2 s
  ("… waits its turn" where the whole does not fit), heard once by a
  screen reader; one timer at a time wakes the window for it.
- **More**: Undo and Redo named by what they take back (off with
  nothing), Undo history…, and Quit last, two icons drawn for them.

Left: the tint is the selection's, pale on some looks; a local copy's
"getting ready…" lasts less than the 0.3 s before anything shows (seen
only on large trees and servers, not timed on a server here). Checks in
`docs/STATUS-LOG.md` (2026-11-03, "The start made plain").

## The History's scroll, Earlier trimmed (2026-11-03)

Asked by the user: scrolling in the History did not seem to work, and
was Activity's Earlier not redundant with it, and too tall pinned?

- **The History opens at its newest**, its first time too: it opened at
  its oldest, so a wheel turned down did nothing. The footer's chevron
  points up, where the History opens (down while open).
- **Activity scrolls beside a scrollbar**, its heading staying.
- **Earlier keeps what the History does not hold** (ADR-114, amended):
  what failed (with Retry), was cancelled or cannot be undone, and
  messages. Pinned, Activity is 260 px high at most, about three cards,
  Earlier folded to a line at its foot (`ui.activity_earlier`). A toast
  said while Activity is open is drawn once it closes.

Checks in `docs/STATUS-LOG.md` (2026-11-03, "The History's scroll").

## Cancel all (2026-11-03)

Asked by the user: a Cancel all on Activity's progress over the queue,
and should the strip's x cancel the one running or all? They chose the
whole queue (ADR-114, amended): the strip's buttons act on what it
says, one action alone or, with several under way or waiting, Pause all
and Cancel all ("Cancel all 3 actions"), what waits cancelled first;
Activity's header holds Cancel all, Waiting's own says "Cancel waiting"
(`ui.activity_cancel_all`, `ui.activity_pause_all`).

Checks in `docs/STATUS-LOG.md` (2026-11-03, "Cancel all").

## Table columns sized by hand (2026-11-03)

Asked by the user: resizable columns in CSV previews and the like. A
table preview's header cell (CSV, TSV, a workbook, a database, JSON
Lines, a playlist) has an edge at its right: dragged, the column is
40 to 1600 px; double-clicked, it is its content's width again. The
widths are the file's, kept while its header stays (the next rows) and
forgotten with another sheet or file (`inspector_rows::Dragged`).
Markdown's tables, which share the reading width and wrap, are not
sized by hand. Edges sized by the pointer's travel (the list's columns
too) now measure from the press, not from gpui's first reported move.

Checks in `docs/STATUS-LOG.md` (2026-11-03, "Table columns").

## Emacs's grammars say where their colors come from (2026-11-03)

A user's report: Emacs's tree-sitter grammars offered only Plain text.
Emacs keeps a library without queries (its modes color in Lisp), and
none was found to lend for those languages. The greyed grammar now says
what to do, by where it was found: Emacs's "Emacs keeps no colors with
its grammars: put the grammar's repository in Skwir's grammars folder
(it need not be built)", an editor's or the system's the same advice,
a repository's own that it has none. Reading's Languages description
names Skwir's grammars folder where it is, and a picker's popup is at
most 440 px wide, a long reason wrapping. Asking Emacs for its own
rules was proposed and left (in "What is left").

Checks in `docs/STATUS-LOG.md` (2026-11-03, "Emacs's grammars").

## Grammars whole or combined (2026-11-04)

The same report, followed further by an agent on the user's machine: a
grammar's row named its library's source though its colors were a
repository's, a nested repository was not found and nothing said so, a
library in `/usr/lib` was called Emacs's. ADR-110's decision 3 amended
with the user, from a page of samples: a grammar found whole or combined
by one rule, said and checked. Built in this order:

- **W1. Sources named by their folder:** Emacs's are its own folders; a
  `libtree-sitter-<lang>` library in the system's library folders is the
  system's, with the system's queries of it.
- **W2. A combined grammar named by both parts,** its queries' source
  first ("tree-sitter CLI, with Emacs's library"), in the picker and in
  Automatic's label.
- **W3. The folders looked in,** folded under Languages, its header
  counting what was nearly found, each near-miss saying what to do.
- **W4. One rule of combining, checked:** a library with its own
  repository's queries (Skwir's folder, the CLI's) or the system's, a
  grammar known by its symbol (`-` and `_` alike), an editor's queries
  never lent; a combined grammar checked by the helper when Languages is
  first shown, offered as being checked until answered.

**Status:** accepted and built 2026-11-04, W1 to W4, checked on Linux
(tests, and a window on a display of its own with real libraries);
Windows and macOS compiled only. Deviations: the folders looked in fold
as every long list does (its first eight rows, what was nearly found
first) rather than shut whole; the check's helper is told its grammars
afresh at each check (`Helper::set_grammars_afresh`), since one told the
same answered from what it had loaded before a query was written again,
found in the window; the helper's check answer says apart why a grammar
colors nothing (`Reply::Checked::colorless`); queries alone of an editor
are no longer listed at all (Neovim's of parsers not installed were
greyed rows). Checks in `docs/STATUS-LOG.md` (2026-11-04, "Grammars
whole or combined"); what it leaves under "What is left".

## Usage selects at any depth (2026-11-04)

Asked by the user: select files in subfolders in Usage, as the Board
does. Opening the folders in place, as the Board does, was weighed and
left with the user (ADR-116): a click would have listed a whole folder
for one entry, and its entries would have joined the pane's order, under
Select all, the counts and the filter. Instead:

- **A view lends the pane what it draws** (`Pane::lend`): shown, so
  selected and acted on as any entry, but in no order; what is no longer
  drawn, another view or location, leaves the selection; read by its
  path when selected (`hydrate`, `App::read_lent`).
- **Usage lends every entry drawn inside the folder's own**, with its
  kind from the scan (`Node::entry_kind`): a click selects the block
  under the pointer at any depth, Ctrl adds, the arrows move among the
  blocks beside the cursor's and out of its folder at its edge, the
  footer weighs a selection at any depth once.

**Status:** accepted and built 2026-11-04, checked on Linux (tests, and
a window on a display of its own); Windows and macOS not built in this
change (nothing in it is one system's). Deviation: a plain click on an
entry of a selection of several in Usage now makes the selection that
entry, at the top level too (the map drags nothing, and the release
that does it in a list never came). Checks in `docs/STATUS-LOG.md`
(2026-11-04, "Usage selects at any depth"); what it leaves under "What
is left".

## Usage paces its recounts (2026-11-04)

Measured with the user after ADR-116 (`docs/STATUS-LOG.md`, 2026-11-04,
"Usage: a reading loop, and how fast changes show"): a change shows in
40 to 240 ms, so the map is not drawn ahead of its count; but a folder
written into without pause was recounted back to back. A change is
still counted at once; changes that keep coming now wait four times the
last recount a change asked for, as revalidations do (`REVALIDATE_DUTY`,
`Active::counts_changes_at`), a request's first scan holding nothing
back. Decided with the user: nothing is selected after an entry inside a
folder goes (ADR-116).

**Status:** built 2026-11-04, checked on Linux (the plugin's tests, and
the release build on a display of its own: the worker's CPU over a copy
halved below the folder shown, a quarter inside a folder of 100,000
files, the map a second behind at most, its last count half a second
after the copy); Windows and macOS not built.

## Text sizes (2026-11-04)

GitHub issue #5: the grid's names were drawn a size smaller than any
other view's (12 px against 14), and no setting changed a size. ADR-117,
taken with the user: four parts with a size each (interface, files,
reading, code), percentages in steps, 100 % the system's body text (the
desktop's size times its text scaling, else 10 pt on Linux, 13 px on
macOS, 14 px on Windows), each part drawn in a rem of its own and what
holds text growing with it, sizes named in the theme, "Fonts from"
Automatic, GTK or Qt. Four steps, a commit each, for 0.3.1.

**Status:** step 1, the files' size (`ui.files_text_size`, `ui.text_size`,
grid names as large as the list's), step 2, reading and code
(`reading.text_size`, `ui.code_text_size`; tables and bytes follow the
preview's size), step 3, the interface (`ui.text_size`, named sizes,
heights in rems), and step 4, the desktop's sizes (Qt's and LXQt's
settings read, `ui.fonts_from`, Copy details), built 2026-11-04 and
checked on Linux (`docs/STATUS-LOG.md`). Reviewed beside 0.3.0 on
2026-11-05: sentences at the body size, Small nine tenths of it, Fonts
from with Skwir's own apart from Desktop colors (since then Colors from,
the same four choices for the colors, ADR-071), the labels saying what
Automatic and each size come to, outlined or in words for every setting
that says it; and Skwir's own fonts, Inter and
JetBrains Mono, carried by every package built here (ADR-118; the
packages' scripts run at the next release). On macOS and Windows,
Automatic is the system's interface font and Windows' Text size is
followed (2026-11-05; compiled, Windows' tried under Wine,
`docs/PORTING.md`).

## Review fixes (2026-11-05)

A review of what came after 0.3.0 (v0.3.0..7527c641: the grammars
combined and checked, Usage lending what it draws, the text sizes and
their sweep, the desktop's fonts and Automatic's answers, Skwir's own
fonts carried, gpui patch 0045, Breeze's icons in the theme's colors,
the palettes and macOS's and Windows' colors) made 78 findings, fixed
the same day by six fixers at once (f18fb034..77034be9, 26 commits, not
pushed). One was high: a release stopped at Arch, whose package check
asked the AUR's source package for fonts it leaves to Arch's packages.
Among the others: Usage read what was selected inside folders again at
each scan anywhere (now only when it changes, and it follows renames);
a palette taken from Colors from's list could stay worn after the
setting changed; each palette crossed in that list blanked every
Breeze icon and inspected the open preview again; all ten carried font
files were read on the window's thread before the first frame (now
those of the families drawn, on a thread at launch, the first theme
waiting for it, as the user chose); a helper's failure was remembered
as a grammar that does not fit; a hung settings portal stopped Skwir
following the desktop; GNOME's Large Text was not followed, and counted
twice on X11. Decided with the user: switches are 36 × 20 px at every
size, as icons; every dialog's title is at Title; lent entries follow
renames; the GSettings fallback is built. The integrator's choices: a
whole grammar colors with its own queries alone, as ADR-110 says
"whole"; the toolbar's icon buttons stay as wide as their icon and
padding (square, the bars fell back to narrower variants sooner); one
step through Colors from's list measured 0.3 to 1.1 ms of the window's
time, left as it is. Checks in `docs/STATUS-LOG.md` (2026-11-05, "Review
fixes").

## What is left

Gathered on 2026-10-20 from the notes of this plan and of
`docs/STATUS-LOG.md`, each as recorded on the date it gives. Later work
may have covered one: check the source before starting it. Work that
closes an item removes its line; work that leaves one adds it here.

**Planned** (in the sections below): Indexing and AI (Phase 7, designed
2026-10-26: ADR-089 to ADR-095); [Grammars and
printing](#grammars-and-printing) (2026-10-31); skwird's steps (ADR-050); the desktop's colors on macOS and Windows (step E); Phase 5b
on the machines (`docs/HELP-WANTED.md`, `docs/PORTING.md`, which lists
what is still Linux's); Phase 8; what [Remote folders](#remote-folders)
leave (ADR-077).

**From the releases of 0.3.0 and 0.3.1** (2026-11-03 and 11-05,
`docs/STATUS-LOG.md`):
- Idle memory misses ADR-000's budget: 103.7 MiB of own share for 0.3.0
  against 0.2.0's 91.3, on the session's display, and 103.8 for 0.3.1. Most of it is the binary: its
  code grew from 54 to 68 MiB and in both releases about 86 % of it is
  resident at idle, so idle memory follows code size. Find why so much
  of it is resident (more than an idle program runs), or make it
  smaller, until `just idle-pss` is green again.
- `just idle-pss` measures the dev build (`target/release`), whose first
  frame read 177 to 184 ms while the shipped tarballs read 137 to 170
  the same evening: why it starts slower is not known.

**From a folder appearing whole** (ADR-120, 2026-11-06):
- The whole folder is sorted once on the UI thread when it appears:
  110 ms for `/nix/store`'s 390 000 entries in the release build, 315 ms
  in the debug build. Sorting it on a worker before it is shown would
  remove that hitch.
- Checked on X11 (Xvfb) only, with `/nix/store` and the unit tests; not
  on a remote folder past its second, nor on macOS or Windows.

**From the see-through grounds** (ADR-119, 2026-11-06):
- Checked on X11 (Xvfb, no compositor) and on a headless sway (no
  blur) only: KWin's blur, Hyprland's, macOS's and Windows' are not
  checked.
- A preview's notice and the column headers drawn on its paper stay
  opaque: making them grounds of their own would take them out of the
  paper's area (`preview::content`, `inspector::content`).

**From the review since 0.3.0** (2026-11-05):
- On X11 GNOME's Large Text is counted once by asking `xrdb -query` for
  `Xft.dpi`: without xrdb it is counted twice, as before. Reading the
  resource through `x11rb` (in the lock through gpui) would need no
  program. Large Text changed while Skwir runs on X11 shows fully at the
  next start (gpui takes the window's scale once).
- A first run, with nothing kept of the desktop's style, reads Skwir's
  own fonts for that run (ADR-118, point 4).
- Usage: a file rewritten in the same second at the same size is seen as
  before and not read again until it changes again; leaving the view
  letting go of what it lent has no UI test (no gpui test harness), nor
  does closing both lists of a setting (DS-13).
- Not tried in a window: a book's size changed while it is laid out,
  the members' and Properties' splits dragged to their least, tooltips
  at 200 % in a narrow window, the tab strip at 200 %, the pill's × at
  80 %, Colors from's palettes hovered over Breeze icons and an open
  code preview, `ui.languages` from an agent.
- Not tried on real systems: a hung portal, the portal starting late on
  a real bus, Plasma with `kdedefaults`, any Mac or Windows machine.
- The text-size test reads sizes given directly; one passed through a
  helper of one's own is not seen.
- At 200 % a settings row keeps its texts at the 35 % share, leaving a
  wide blank before its control (70ea5c92's design).

**From the review of 2026-11-03** (the History and the queue's
progress, fixed the same night):
- Not seen in a window: a toast's Undo held by an action under way (unit
  tests only), the strip saying "N waiting" while nothing runs (a folder
  copy counts only once measured, so it never showed), and none of the
  History with a screen reader (`just a11y-check` not run).
- Tab in the History goes round within it, as a dialog's does, rather
  than leaving the popup; focus coming back to the files when it closes
  has no test (skwir-ui has no gpui test harness).
- A frame drawn between the next action's `Started` being sent and the
  window taking it in can still show the strip empty once.
- An entry put back among later ones (a redo by entry, a retry joining
  its first run) meets only what those later entries keep (ADR-079, 13).
- An entry's `asked` runs grow by one with each undo or redo
  (`HistoryRow::plans`).
- The fixes' tests were not run against the code before them to see
  them fail first.

**From the review of 2026-11-02**:
- Smaller copies: the page check's squares (8 px, 40 levels) are
  reasoned, not measured on real scans, so a text-heavy scan at the
  smallest steps may have images put back it did not need; a crash
  leaves its `copy-*` work folder in the cache until the cache is
  cleared; nothing was measured on large real PDFs or pictures (memory,
  time); the JBIG2 coder and CCITT Group 3 2-D were checked by round
  trips only; a JPEG's JFIF or EXIF density is not scaled when it is
  resized, and IPTC is dropped when a JPEG is written again.
- Plans: `admit()` stays quadratic in the queue. Merging the footprints
  ahead keeps ADR-079 exact, but it is linear only once
  `Footprint::conflicts` and `Names::meets` look each item of the
  smaller footprint up in the larger. Windows' `CopyFileEx` progress may
  count alternate streams beyond a file's length. rsync's order under
  incremental recursion was not run against a real rsync, and the wake
  of a copy that stalls was seen in core tests only.
- The desktop: nothing ran on a real GNOME, Plasma or Windows session
  (GNOME's and PowerDevil's answer to the portal's SUSPEND, the taskbar
  told again on real Explorer, mako or swaync broadcasting another
  program's clicks). Plasma 6.8 holds nothing off at logout:
  xdg-desktop-portal-kde keeps the portal's LOGOUT flag, but nothing in
  plasma-workspace asks it, and a logind block would leave the machine
  on after the logout (ADR-115). `Asked::Notice` does what `Asked::Open`
  does (a cleanup).
- What runs without the window: the race of a window leaving before
  gpui removes it is closed by structure, not reproduced; a window that
  fails to open again, macOS's Quit question and Windows' window were
  not run; another user's pipe cannot be made in a test, so its refusal
  is tested only through `user_of`; Quit's words for a print being sent
  or a plan waiting for its server were not seen in a window. The `ui.*`
  requests and notifications that pile up while no window is open run,
  or show as toasts, when it opens again, possibly stale; a marker a
  crash left costs each later launch one bus connection.
- Activity: an Activity popup open while a plan starts without a press,
  and the scroll to a button among many answers, were not seen in a
  window.
- Printing and the UI: no real GNOME or KDE portal gave the answers now
  read apart; the wheel on X11 and on a real touchpad was not measured;
  `tag:` with 50,000 tags was not timed in a window, and the pane's own
  tags are still asked at each key; the preview's More popup builds its
  items three times a frame while open; a row's tag look is built again
  each frame (`views/entry.rs`).

**From Cancel within a file (2026-11-02)**:
- The 80 MiB stall the user saw was not reproduced as such (their build
  predates arrivals counted at all); sshfs, rsync and Skwir's own copy
  were measured through a relay on this machine, not a real network,
  and GVfs, kio-fuse and rclone mounts not throttled.

**From the address card (2026-11-02)**:
- The lower of two panes split one above the other was not tried: its
  card has what the window leaves under its field, at least 120 px.

**From the review of 2026-11-01**:
- A grammar that loads but whose queries fail is known only once it
  colors: Reading's Grammars page does not check each grammar (the
  helper's protocol has no check of one; a cheap way is to keep a
  failure seen while coloring as the grammar's `Found::problem`).
- The tree-sitter helper's inputs are bounded, its memory is not: a cap
  waits for an ADR (on Linux `RLIMIT_DATA` near 2 GiB, set by the helper
  as it starts, which spares the address space Wasmtime reserves where
  `RLIMIT_AS` would not; a Job Object on Windows; none on macOS).
- Printing was checked by tests and stand-ins only: no real portal or
  printer printed a page scaled, sides chosen or a booklet's turned
  backs; the portal's wait ending when it leaves the bus has no test;
  Windows' quarter turn (clockwise) and macOS's CUPS queue and paper
  (`PMPrinterGetID`, `-o media=`) are compiled only; printing from a
  server that stopped answering was not run. When every page asked is
  past a text's end, the helper's message is shown, not the plugin's;
  a book read by MuPDF may count its pages from another layout than the
  one printed (as before).
- A saved search whose `in:` names a remote folder's mount keeps the
  path, not the address (a saved place keeps the address): its text's
  `in:` would need writing again. A remote bookmark is kept as
  `Location::parse` spells its address and a saved place through
  `address_below`; whether the keep button compares the two alike where
  they percent-encode differently was not checked (as before ADR-109).
- Refused attributes are not remembered on macOS (macFUSE's mount type
  does not start with `fuse`, so it is asked as a kernel's share: only
  the saving is lost). A folder of GVfs's or kio-fuse's bridge shares
  the bridge's mount number: a server unmounted and mounted again within
  one bridge stays refused until the bridge goes.
- Places were checked by commands and tests, with pretended mounts: not
  in a window (the keep button filled, the sidebar's row of a saved
  place at an address, the toasts drawn), not against a real server
  (`just remote-check` has no such case).
- The print helper: a line a page's edge cuts inside a table row taller
  than a page is drawn on both pages; a long code line's tab stops start
  again at each 4 KiB piece; fallback fonts are looked for 4,096
  characters at most (more scripts than that draw only as the faces
  found hold them); a face of a font collection is still copied out of
  it (fontique registers no single face), now within bounds; a role
  face or fallback whose licence forbids embedding is passed over
  silently, only a specimen says so. Its gains in memory and time (long
  lines, tall rows, fallback scans) were not measured, and its PDFs
  were read back with lopdf and pdftotext only.
- A comic's first page no longer drawn twice and a JPEG decoded at a
  reduced scale were not timed on a real comic; a page past about 16.7
  million pixels is decoded whole, as before.
- Not looked at in a window: Reading's grammar picker after the fixes,
  folds kept through a refresh, bat taking over from a grammar that
  fails. The startup that no longer loads bat's grammars was not
  measured with `just idle-pss`.

**From Usage selecting at any depth (2026-11-04, ADR-116)**:
- A file written into a folder Usage counted still costs the worker its
  events (about 3.9 s over a copy of 40,000 files in 21 s, with recounts
  paced): what the pacing leaves.
- A lent link has no target until it is read, so no preview for the
  moment that takes, and the preview is not asked again when it is read.
- Not tried: a link or a pipe selected inside a folder in a window (unit
  tests only), a remote folder, a screen reader (`just a11y-check` not
  run), `just check-cross`, Windows and macOS.

**From grammars and printing (2026-10-31, ADR-110, ADR-111)**:
- The text view, where code files open, does not fold: a grammar's folds
  reach only a structured-data code view without folds of its own.
- Code of another language inside a file a tree-sitter grammar colors is
  colored only by a grammar found for that language: with Tree-sitter
  first and no JavaScript or CSS grammar, an HTML file's script and style
  are plain where bat colors them.
- A grammar's file types taken from its name can name no real file
  (Neovim's `markdown_inline`, `query`, `vimdoc`).
- From grammars whole or combined (2026-11-04, ADR-110 amended): a
  combined grammar that fails while coloring (before Languages was shown)
  falls back to bat as before, but the failure is not kept for the
  Reading page, which checks it itself; "Checking…" was not seen in a
  window (the check answered before a picker opened; unit tests only);
  an injected language named the other way (`c-sharp` for `c_sharp`) is
  matched by the helper's own names, not by `grammars::key_of`; the new
  list was not heard with a screen reader (`just a11y-check` not run).
- Emacs's grammars come without queries: its modes color in Lisp
  (`treesit-font-lock-rules`, often built from `defconst`s, `rx` and
  `regexp-opt`, some captures naming a function). Since 2026-11-03 the
  greyed grammar says to put the grammar's repository in Skwir's
  folder. Proposed, not decided (a user's report): ask Emacs itself,
  `emacs --batch` recording the rules as its `*-ts-mode` files load
  them, expanded with `treesit-query-expand`, then faces mapped to
  capture names, Emacs regexps to `#match?`'s, `:pred` and function
  captures left out; kept in the cache. It runs the user's packages'
  code unasked and finds no package an init file loads (straight,
  elpaca, Doom): an ADR first.
- The first parse of a WebAssembly grammar can take seconds, its colors
  waiting; code blocks are colored as a document is built, so a helper
  starting delays its first paint. Folds a user closed are forgotten
  when the file is read again.
- Printing: nothing asks before printing hundreds of pages; the portal's
  dialog is not given the window as its parent; a listing and several
  files were not printed through a real dialog (a contact sheet and a
  booklet were saved from Skwir's dialog, a booklet printed to a file
  through GTK's); KDE's portal, the user's, was not tried with P5. The
  pages of a file that is not a PDF (a text, a Markdown document) are
  not counted before the dialog asks, so its booklet pictures say `n`.
  The preview's menu shows no key beside Print… (Ctrl+P there).
- macOS's print panel and Windows' print dialog are compiled only;
  Windows' printing and the two new helpers were not run under Wine.

**From books beyond EPUB (2026-10-30, ADR-106)**:
- A KF8 book laid out in fixed pages (a comic) reads as chapters of
  pictures, not as pages in the document reader: calibre 9.17 marks no
  book fixed (no EXTH 122) and kindlegen is withdrawn, so no tool makes
  one to test with.
- MOBI 6 headings written as `<font size>` (calibre's way) read as bold
  paragraphs: reading mode reads `align` but no `<font size>`.
- With MuPDF a MOBI book shows no contents (it reads no NCX index).
  calibre's MOBI 6 contents keep one entry for each place: an entry at
  the same place as its parent is not in the file.
- `.azw`, Kindle's older name, has no type in the database: not read.
- Tried on calibre's output and on books built in the tests only: no
  book made by Amazon's tools, no FB2 from another writer than calibre
  and the hand-written one, nothing near the 64 MiB text bound.
- Windows and macOS compiled only (`just lint-all`, with the review
  fixes of 2026-10-30), not run.

**From places (2026-10-31, ADR-109)**:
- A pane filtered over a search's results (the `filter` command run over
  one) is shown on Back without that filter: a search's step is its
  terms.
- Not checked: Wayland and the user's session (Xvfb only); Windows and
  macOS not compiled for it (`just lint-all` not run).

**From the lens (2026-10-30, ADR-107)**:
- What a lens of tags, dates or sizes asks of a remote (counted:
  `docs/STATUS-LOG.md`, 2026-10-30): nothing beyond the listing on sshfs,
  rclone over WebDAV and kio-fuse over SFTP, but on GVfs over AFC a stat
  and a tag read a file of each folder shown once GVfs's own cache of its
  listing lapsed (about a second), one USB round trip each on an iPhone.
  The tag reads are gone (2026-10-31, `reach::refuses_attributes`: a
  remote that refused extended attributes is not asked again this
  session); the stats are what a sort by date asks too, kept as they
  are. Not counted: GVfs over MTP (an Android phone, through
  `just mtp-check` when one is offered) and GVfs over SFTP and SMB (its
  SFTP backend refused the connection inside the test's namespaces while
  ssh by hand reached the server, and said nothing more with
  `GVFS_DEBUG=1`; counted in a session of its own, outside namespaces, it
  would close this).
- On those mounts no tag is read (sshfs and rclone keep no extended
  attributes): a tag lens shows a server's folders, and the files whose
  tags are kept beside them.
- Not checked: Wayland and the user's session (Xvfb only); a lens over a
  remote folder in a window; Windows and macOS compiled only.

**From the review fixes (2026-10-30)**:
- Two odd spellings of a place's two depths are not known as one place
  (a provider would have to say a location's one spelling).
- `in:trash:` (no scope offers it) tests trashed files with their stat,
  without the trash's own fields; no query term reads them.
- The folder provider's `Unreadable` answer (a stat refused in a folder
  it lists) has no test: only an I/O error makes it.
- Disconnect still sends only what waits when asked, not what a plan
  just wrote and rclone has not queued yet. launchd and Windows' logon
  tasks have no counterpart of `SuccessExitStatus`: a refused stop there
  is as before.

**From a tag's place filtered at its depths (2026-10-30, ADR-108)**:
- The sidebar's All Tags is the top's place of the home folder: its
  "Every tagged file" lists every tagged file under home, which the tags
  index answers; not tried on a large home.
- Not checked: Wayland and the user's session (Xvfb only); Windows and
  macOS compiled only (`just lint-all`, 2026-10-30).

**From the performance review (2026-10-29, ADR-104, ADR-105)**:
- Dropped as not worth it: a folder copy walks its source three times
  (warm caches; ops#6); the tags index's folder list rewritten every 2 s
  while a count learns (tags#8: split by piece, it would trade those
  rare writes for up to 256 reads at every load); the remote places
  republished only when changed (remote#3, part: their closures cannot
  be compared). The tag editor's scan, the XMP packet's search, a cached
  thumbnail's two opens and an EPUB's lookups by scan were done after
  all, the same day.
- Many single-file moves out of one large folder in one plan still cost
  the folder's size each in the tags index (tags#2 left them).
- The Map view's scan walks the listing again after a change; it could
  visit only what `Listing::updated_since` names.
- Compiled only for macOS and Windows: the watcher's written window
  (Windows reports any change, attributes too, as a write, so they wait
  up to a second there; macOS and Windows report no close, so a written
  file is told within the second), files' own icons let go of (ui#5),
  the archive helper's delete sharing on Windows. Whether the document
  engines' opens let another program replace a file on Windows is not
  checked (ADR-105).

**From text read a screen at a time (2026-10-29, ADR-103)**:
- Not tried: a trackpad (a swipe turns one screen, then waits for a
  pause), a high-resolution wheel (each part of a notch turns, as in the
  document reader), the session's Wayland; Windows compiled only, macOS
  not compiled (check-cross leaves `skwir-ui` out of its macOS pass).
- What a screen reader hears still holds the lines laid out under the
  page's cover.

**From previews read once and rclone's cache (2026-10-29, ADR-101,
ADR-102)**:
- An upload refused three times fails a copy's write-out; not tested
  against a server that refuses (rclone retries each upload itself
  before counting a try): only the queue's reading is.
- Windows and macOS: `Held`, the cache bounds and the copy's wait are
  compiled there, not run: WinFsp is not under Wine, so Windows' mounts
  are never made here; their remote control on the loopback is checked
  on Linux (`what_a_mount_sends_later_is_sent_through_a_control_on_the_loopback`).

**From the review of 2026-10-28** (fixed the same day, "Review fixes
(2026-10-28)" below; each with its fixer's proposed design in
`docs/STATUS-LOG.md`):
- scraper 0.29 (selectors 0.41, cssparser 0.38, html5ever 0.40) waits
  for lol_html: it needs servo_arc 0.5, while lol_html 3.0.2 (merman's)
  keeps selectors 0.37 on servo_arc 0.4, so taking it adds a duplicate;
  scraper 0.27's set is in (2026-10-28).
- rusqlite 0.41 (libsqlite3-sys 0.39) supports SQLite 3.45.3 or newer,
  and on Linux Skwir links the system's: the AppImage builds in an
  Ubuntu 22.04 root (3.37.2) and Ubuntu 24.04 has 3.45.1, so 0.40 stays
  (3.34.1 or newer). It waits for those roots to move, or for a decision
  to bundle SQLite on Linux. 0.41 also fixed a use-after-free when a
  progress handler is replaced while it runs; Skwir replaces one only
  between queries, under the connection's lock (map-tiles).
- The PDF helper reads page labels through a second binding to PDFium:
  pdfium-render 0.9 keeps its raw bindings private (its issue 279); drop
  the second binding once that is fixed.
- A trash across filesystems whose original's removal waits is tested
  by unit tests only (`fs::volume` has no stand-in for a removable
  drive); a part restored whose item's removal never came still refuses
  its undo ("in its place in the trash").
- The packages were not built after the crate updates of 2026-10-28
  (`just package-try`, `package-appimage`, `package-windows`,
  `package-macos`): no system library's binding changed (pdfium-render,
  loaded at run time, is the one native crate updated; libsqlite3-sys
  stays 0.38), but a package build before tagging says so for sure.
- Windows' key setup without a control master is compiled only there:
  its logic is tested on Linux (`Ssh::alone`, against rclone's SFTP
  server); Wine has neither AF_UNIX for the askpass nor an sshd.

**From F1 by topic** (ADR-100, 2026-10-27):
- F1 is not walked by `just a11y-check`: what a screen reader hears of it
  is unchecked.

**From the menus' overhaul** (ADR-099, 2026-10-27):
- A submenu's row is not said to open one on Linux: AccessKit's AT-SPI
  adapter (0.19) maps no expanded state, for any element.
- A server's Thumbnails submenu not seen in a window; Windows and macOS
  not compiled for the menus' changes yet.
- The Open With submenu lists applications without their icons: menu
  items take bundled icons only.

**From the address field's overhaul** (ADR-098, 2026-10-26):
- The model half of `query.ask` (Phase G): the field's "Did you mean"
  reads words by rules only (`query.interpret`).
- A plugin from outside cannot offer rows: sources are Rust
  (`SuggestSource`); a command-shaped source would be its seam.

**From the review of 2026-10-23** (fixed 2026-10-24, "Review fixes"
above; each with the design its fixer proposed):
- A cut file dims another folder's name of it: `Pending` keeps each
  entry's folder, its readers pass the row's (ops-basic, the views).
- `hydrate_paths` keeps a path's old identity when another file took
  it: re-keying needs every location of many folders to follow.
- Programs Skwir starts inherit `VK_LOADER_DRIVERS_DISABLE`: no one
  place starts them; a platform constructor would drop what Skwir set
  for itself.
- Extract Here of files dropped from another application types them
  on the window's thread.
- A network plugin's sandbox shares the host's abstract sockets: a
  seccomp filter or a user-mode network, an ADR-045 amendment.
- `platform::filesystem` calls `statfs` directly: no core seam.
- The NFS/AFP harness (`tools/test/gvfs-nfs-afp.sh`) fails (seen
  2026-10-26, the same at 162dc59d): once
  `gvfs_opens_an_nfs_address_below_its_export` has mounted the export
  through Skwir's GVfs connector and let go of it, `gio mount` of it
  again leaves GVfs's bridge answering an I/O error, so
  `gvfs_nfs_and_afp_take_by_gio_what_it_does_better` does not run. With
  Skwir's test left out the harness passes. Not looked into.
- Types by extension: the documents plugin's engine and Pandoc's
  format, the XMP field provider; JSON Lines and JSON with comments,
  which the database does not name. JSON Lines table cells print
  numbers through `f64`, as the formatted JSON did.
- The tags index knows hidden files by a leading dot only (Windows'
  mark).
- The window: a records save waiting on the keyring brings an old list
  back; the switch knob and the treemap's hatch take no theme token; a
  place's mosaic types its pictures by name; link fragments compared
  undecoded; a press in a parked container view; `link` in
  `VIEW_ONLY`; map markers in `f32`; accessibility nodes for every
  marker; picture decodes holding workers while they wait their turn.
- Seen on the night: a move across volumes whose immediate removal
  fails keeps its copy unjournaled; `restore_listed`, `RestorePart`
  and `Untrashed::take` copy and remove across volumes without 1.3's
  handling; `tags.export` and the tags-everywhere commands propose
  after answering, so `skwir cmd` exits first; libarchive's
  lrzip, grzip and lzop filters run programs from `PATH` (on Windows
  unsandboxed); a tag of a name Windows refuses fails its export.
- Not exercised in a window: the drops, bulk rename, front requests
  while a dialog is open, the queued toggles, the zoom wheel's pane,
  the rubber band's pane, the map's clamp, the printed-page steps.

**Gates not closed:**
- Maps: idle CPU after panning on geotagged pictures (`images10k` has
  none). `skwir-bench gen` makes no geotagged pictures and no
  100,000-point GPX, and `budgets.toml` has no budget for the GPX parse
  or the EXIF position scan.
- Notes everywhere: the round trip through an exFAT stick read by a
  Mac's Finder.
- Phase 5b: everything run on macOS and Windows, and the budgets
  measured there (ADR-038).

**Previews and readers:**
- Media (2026-09-19, ADR-008): audible lip sync not assessed; native 4K
  output, GPU buffer sharing, external subtitle files, a second backend;
  full-file waveforms.
- Audio and video on Windows and macOS (2026-10-20, ADR-076): under
  Wine, turning subtitles off while paused stalled in some runs; nothing
  played on the machines; lighter later: applemedia without Vulkan
  (MoltenVK, 7.4 MB), dav1d linked into the helper (5.1 MB), GStreamer
  built by us.
- Archives: 7z and RAR decryption (libarchive has none), passwords for
  agents, Merge for an extraction onto a folder (2026-10-13); a Mac's
  `__MACOSX` beside an archive's one folder still makes a folder around
  them (2026-10-15).
- Text documents (2026-10-03, ADR-030): `.fodt`, presentations and
  spreadsheets as text, find beyond the part shown, links to bookmarks in
  another part.
- DjVu (2026-10-15, ADR-061): indirect documents, annotations other than
  links, djvu-rs's page titles; DjVuLibre's helper in the macOS and
  Windows packages.
- Code (2026-10-14, ADR-060): Vue Component's two patterns Ferroni
  refuses (ferroni#227); the patches offered to syntect and two-face;
  the 7 MiB or so the code view keeps beyond plain text, not looked into.
- Links (ADR-029): table cells and Properties' comments (2026-10-03);
  the structured-data views made whole, bare names (2026-10-14); an
  Obsidian note elsewhere in the vault (waits for skwird's index),
  notes embedded with `![[…]]`.
- 3D models (2026-10-19, ADR-067): textures; the window on NVIDIA (the
  user's session); a screen reader.
- The screen kept awake while playing (2026-10-15): macOS; a desktop
  that idles by its own rules (its D-Bus inhibitor).
- Not exercised in a window: a reader's or a converter's failure shown
  as source (2026-10-15); after a step read ahead, a HiDPI panel asking a
  sharper picture, a slow disk (2026-10-19).
- The wheel glided over pages (2026-11-02, weighed, left for later): a
  notch moves the readers' pages 120 px at once (`WHEEL_LINE`), where a
  browser glides it over 100 to 150 ms so the eye keeps its line; not
  in lists, where one looks for a row. A notch during a glide extends
  its target; any other scroll (a touchpad, keys, the scrollbar, zoom, a
  page's height arriving) ends it; a page turned at a notch still turns
  at once. The animation rule (`docs/invariants/shell.md`) is amended
  first, and frames and CPU are measured against a frame a notch (a
  glide draws the whole window at the display's rate while it lasts;
  PDFium renders no more). It comes with the desktop's reduced motion
  (the portal's `org.freedesktop.appearance` `reduced-motion`; macOS's
  and Windows' own), which keeps today's steps. Not by gpui's
  `App::set_reduce_motion` alone: it holds every `with_animation` at its
  end, fades too, so the player's play/pause flash (whose end is
  transparent) would never show; reduced motion asks for no movement,
  not for no feedback.

**The window:**
- Terminal (ADR-019): mouse reporting, an input method's text while
  composing, search in the history (2026-09-26); a menu item for Follow
  the folder shown (2026-10-19, the shell would name a plugin's key). On
  Xvfb a restored panel appears only at the first input after startup,
  not checked on a compositor.
- Keys (2026-10-13, ADR-024): a key scheme chosen from the Keyboard
  page; "Set shortcut…" from F1 or the palette. A broad default moved
  frees narrower keys of other actions, which the page says rather than
  binds again.
- Menus (2026-10-05): the current item is marked two ways, a check for
  an option, a highlight for where one is.
- Accessibility (ADR-065): never listened to with Orca speaking;
  AccessKit's Linux adapter passes no tree level or expanded state; a
  scanned page's text (`docs/invariants/accessibility.md`).
- Properties (2026-10-13, ADR-058), not checked in a window: a read-only
  drive's links taken away, a drive keeping no permissions (FAT).
- Elements known by what they show (2026-10-21, `elements::known_by`):
  the views' rows by their entry, the trail's crumbs by their place, the
  pickers' options by their item, Properties' rows by what they say, the
  preview's header and frame by the file shown, and the rest of the
  lists that change under the pointer. Left by position, as nothing moves
  them while shown: menus, the deletion question's rows, a plan's steps,
  the About facts, a key sequence's next keys, the tag colors, the
  settings' key actions (by their place in the whole list).
- A flaky test, `pending_opens_report_errors_asynchronously_and_can_be_replaced`
  (`crates/core/tests/listing.rs`), failed eight runs in a row on
  2026-09-26, then passed; not investigated. Since 2026-10-24 it holds
  its worker without stand-ins (`AppConfig::stand_ins`), which removes
  a later timing dependence but cannot explain that run, which came
  before stand-ins.

**Search, tags and places:**
- Never read: Finder attributes or `._` files a Mac wrote (the tests
  write Apple's format from its source); a cold `tag:` not measured
  (2026-09-30, dropping caches needs root).
- The sidebar suggests only the tags Skwir's index has seen, not
  Baloo's; renaming a tag leaves hidden files alone, as the search does
  (2026-09-30).
- Companion files (ADR-028): an orphan offered back to its file.
- Encrypted volumes (ADR-088, 2026-10-25): no real LUKS drive unlocked
  with a kept passphrase (stand-in `udisksctl` only); KWallet's volume
  passphrases (KDE's device notifier) and crypttab's key files not read;
  a Mac's and Windows' unlocking compiled only (`docs/PORTING.md`).
- Not checked in a window: a tag exported by a real drag, a drag between
  sidebar groups refused, a busy volume on a real drive (fake `lsblk` and
  `udisksctl` only), F5 in Usage, Alt+click on a tag's pill, an office
  document converted, a missing engine library, a restore that merges
  (Release 0.1, step 6).
- Tag display (2026-10-26): checked on Xvfb only, not on a compositor
  nor at a HiDPI scale of the session; the hydration of the columns
  view's side columns (what they draw) not timed on a remote folder;
  the grid's captions have no menu on the grid itself (Settings and the
  View menu's Under names…).

**Maps** (2026-09-27 to 10-05): not built, the Tracks menu, `geo.track`,
`geo.open_external` and `geo.copy_coordinates`, backgrounds from a file.
Not verified: the dark look's map tokens, OpenStreetMap's own server,
the `{s}`, `{-y}`, `{q}` and `{r}` server forms live, a keyring that asks
to be unlocked, the double click that opens, a folder of thousands of
pictures, a real file of each flight and drone format, a video's
position in Properties, a real offline map, a background chosen in a
window that downloads.

**The extension boundary** (Phase 6): the packages carry no module; no
typed WIT world (payloads are JSON); a module has no network; program
plugins are isolated on Linux alone (compiled only on macOS, absent on
Windows, where none runs); the Windows control pipe keeps the system's
default security.

**Other systems,** beside `docs/PORTING.md`: files on the system's
clipboard on macOS and Windows (ADR-062); Windows changes access by path,
so a link swapped in between its check and its change is followed
(2026-10-13); macOS's quarantine passed to what is extracted, compiled
only; on Windows, undoing a trash restores the latest item from its path
(2026-10-11).

---

## Phase 0: Foundations and toolkit spike

**Goal:** a repo that can measure itself, and a toolkit decision backed
by numbers.

**Scope:**
- Cargo workspace: `crates/core`, `crates/platform`, `crates/ui`,
  `crates/plugins/*`, `crates/bench`.
- Nix flake dev shell, `just` recipes, CI (lint, test, bench on Linux).
- A benchmark harness generating synthetic trees on demand (never
  committed): a flat 100k-entry folder, a 1M-file deep tree, a folder of
  10k large images.
- Performance budgets, enforced by `bench` as CI regression checks:
  first frame < 100 ms cold; a 100k-entry folder's first rows painted
  < 16 ms after listing starts, scrolling at display refresh rate; idle
  RSS < 100 MB with default plugins, the same with AI plugins disabled
  as not compiled.
- Toolkit spike: the same demo in GPUI and Floem (a virtualized 100k-row
  list with lazy thumbnails) on Wayland (Hyprland), on the Intel iGPU
  and the NVIDIA eGPU: first paint, frame time while scrolling, RSS,
  external drag-and-drop and IME once.
- ADR-001 records the toolkit choice and its numbers; ADR-000 the
  principles.

**Done when:** budgets are documented, `bench` runs in CI, the ADR is
written, and the winning spike renders 100k rows at refresh rate on
both GPUs.

**Status:** gate passed 2026-09-18. GPUI chosen (ADR-001: its p99 frame
interval stays within vsync jitter on both GPUs; Floem's is a dropped
frame). `just bench-check` enforces `crates/bench/budgets.toml` (42
budgets on 2026-10-10). Routine CI is now minimal; the budget check is
part of the local release gate and manual full workflow (ADR-053).

**Deviations:**
- The UI budgets were revised after Phase 2 (ADR-002): idle memory is
  PSS, not RSS; the first frame may take 150 ms until gpui loads fonts
  lazily, our own share under 50 ms. ADR-033 (2026-10-04) counts Skwir's
  own share of PSS, without the GPU driver's code.
- First frame and idle memory are measured by `just idle-pss`, not by
  `bench` (ADR-002, until an automated startup harness in Phase 8).

---

## Phase 1: Core kernel

**Goal:** `crates/core` is complete enough that plugins can be written
against it, with no UI.

**Scope:**
- **Entries.** `EntryId` = device and inode on Unix, the file ID on
  Windows. Streaming listing via `getdents` batches (rustix), `d_type`
  to avoid a stat, lazy stat on demand. A metadata bag of typed fields
  with provenance (Filesystem, User, Plugin(name), Model(name,
  confidence)).
- **Watching:** `notify`, per open location, coalesced, feeding the same
  entry stream.
- **Locations:** `LocationProvider` (string → entry stream). Only the
  path provider ships in core.
- **Commands as data:** a serde struct with typed arguments, registered
  by name, dispatched by the core. Keymap and palette consume the
  registry. No command captures UI state.
- **Operations and plans:** `Operation` with `apply` and `inverse`; a
  `Plan` is an ordered list of operations referencing `EntryId`s, with a
  dry-run `diff()`. The executor runs plans in the background with
  progress, pause and cancellation, and writes an undo journal.
  Re-applying an applied plan is a no-op. Built-in: copy (reflink when
  possible), move, rename (atomic exchange for swaps), trash, delete,
  mkdir.
- **Registries:** views, panels, sidebar and context-menu contributions,
  the settings schema.
- **Plugins:** compile-time crates, a runtime toggle in settings,
  depending only on core, never on each other.
- **Cache and settings:** a content-addressed cache folder, deletable at
  any time; plugins get a namespaced slot in cache and settings,
  nothing else.
- **Scheduler:** one priority queue, visible work first, everything
  cancellable; provider calls batched, async, timed; slow providers
  surfaced.
- Tests: property tests that `inverse(apply(op))` restores the tree;
  idempotent plans; listing benchmarks over the synthetic trees.

**Done when:** a headless test drives a plan (list, select, rename,
trash, undo) through commands only, and listing the 1M-file tree
streams without ever blocking the caller.

**Status:** gate passed; built by 2026-09-18.

**Deviations:**
- Identity was split (ADR-049, 2026-10-10): `FileId` (device and inode)
  is the file, `EntryId` the file under one of its names, so hard links
  are two entries. On Windows a 128-bit file ID is folded into 64 bits
  (ADR-036).

---

## Phase 2: Minimal daily-driver

**Goal:** replace the file manager you use every day, even if ugly.

**Scope:**
- `crates/ui` on the chosen toolkit: window, panes, theme tokens,
  command palette, keymap, address bar (paths only), status and
  progress. AccessKit from the start.
- Plugins: `view-list`, `places` (XDG user folders, GTK bookmarks,
  udisks2 volumes), `ops-basic` (the built-in operations as commands
  with keys), `open-with` (xdg-mime), `undo` (journal UI).
- `crates/platform` on Linux: shared-mime-info through `xdg-mime`, XDG
  icon theme lookup, the freedesktop trash, a read-only freedesktop
  thumbnail cache lookup.
- Keyboard first: every action in the palette.
- Dogfooding starts here.

**Done when:** you have used it as your only file manager for one week,
budgets still pass, and every action taken that week went through a
`Command`.

**Status:** gate passed: a week as the only file manager, then two
weeks of improvements from it (recorded 2026-10-04). Budgets:

- ADR-002 (2026-09-18) revised the two UI budgets Phase 2 missed (first
  frame 143 ms against 100; idle RSS 143 MB against 100).
- The idle memory gate stayed open from 2026-09-19 (118 to 120 MB PSS
  against 100, ADR-002's addendum) until ADR-033 (2026-10-04) budgeted
  the own share: 89.2 MiB against 100 (133.7 MiB in all, 44.6 of it the
  driver's), by `just idle-pss`.
- Found with that measurement and fixed the same day: an idle Wayland
  window woke at the display's rate (vendored patch 0012: it draws when
  asked; idle CPU 1.0 → 0.2 %); the first frame read a median of 146 ms
  (patches 0013 and 0014: pipelines built once, the GPU context created
  beside the fonts; 134.6 → 114.5 ms back to back); the places plugin
  rescanned about twice a second once anything had opened the GTK
  bookmarks file (notify reports opens; access events are dropped).

**Deviations:**
- AccessKit: gpui 0.2.2 has none (ADR-001). Accessibility waits for
  gpui's support, checked at every gpui update. Zed's main branch has it
  (ADR-064); exposing Skwir's places started with ADR-065.

---

## Phase 3: Visual richness and performance hardening

**Goal:** it looks and feels like the fast, GPU-native app it is.

**Scope:**
- **Thumbnails:** `ThumbnailProvider` keyed by MIME type; the
  freedesktop cache read first, generation on a miss written back with
  spec-compliant names; installed `*.thumbnailer` files discovered;
  decoding on a pool, visible first, cancellable, uploaded to the GPU
  atlas.
- Plugins: `view-grid`, `view-columns` (Miller columns), `preview`
  (image, highlighted text, PDF via PDFium, a video's first frame;
  Space to peek).
- Drag and drop: internal first, then external on Wayland.
- A performance pass over the synthetic trees; a flame-graph recipe.

**Done when:** the 10k-image folder scrolls at refresh rate with
thumbnails appearing in visible-first order, and all three views pass
the budgets.

**Status:** gate passed; built by 2026-09-18.

**Deviations:**
- The views are built into `skwir-ui` (`crates/ui/src/views/`), not
  plugin crates: gpui stays in `skwir-ui`, and a `ui-api` crate waits
  for a third-party view.
- The preview became many plugins: PDF through PDFium, Poppler or MuPDF
  readers in helper processes (ADR-003, 004, 005, 014), video and audio
  through GStreamer (ADR-008), structured previews (ADR-009). See
  [Built beyond the plan](#built-beyond-the-plan).

---

## Phase 4: Power features, each as a plugin

**Goal:** the UX bets. Ordered by leverage; each ships and toggles on
its own.

**Scope:**
1. `query`: a query language (`*.pdf modified<7d size>10M`) as a
   `LocationProvider`, so search results are a location. Saved searches
   kept in the sidebar like bookmarks.
2. `search`: a parallel subtree walk (`ignore`), results cached by
   folder mtime; `plocate` when present; content search with the
   ripgrep crates.
3. `shelf`: collect entries from anywhere, apply one plan to all.
4. `rename-bulk`: patterns, regex, live preview; produces a plan.
5. `edit-as-buffer`: the listing as an editable text buffer, diff →
   plan (oil.nvim's model).
6. `jump`: frecency jump from zoxide's database and
   `recently-used.xbel`; `nucleo` matching.
7. `shell`: open a terminal here, mirror the folder, drop the selection
   as arguments.
8. `tags`: xattrs with the XDG and Finder keys; tags are metadata with
   User provenance.
9. `view-dual`: orthodox two-pane mode over the same model.
10. `usage`: GPU treemap, btrfs-aware.
11. `timeline`: files by mtime as a scrubbable histogram.

By the end there are 10+ internal plugins: the signal that the traits
are ready to freeze.

**Done when:** every plugin above can be disabled at runtime with no
trace in memory or UI, and the trait surface has not changed for the
last three plugins written.

**Status:** gate passed 2026-09-18: thirteen plugins, each proven by
`crates/skwir/tests/plugins_toggle.rs` to disable with no trace in any
registry and to re-enable; the trait surface did not change for the
last three written (jump, tags, shell). The test still holds on
2026-10-04 with 36 plugins, 14 of them previews.

**Deviations:**
- `search` is folded into `query`; results are not cached across opens
  (the walk is fast; content search dominates).
- `view-dual` is per-pane state in the shell, not a crate.
- The review modal, prompts, treemap and timeline strip live in
  `skwir-ui`.
- `edit-as-buffer` opens `$VISUAL`/`$EDITOR` in a terminal, not an
  in-app editor.
- The native workers are workspace members outside `default-members`,
  so the default build needs no Poppler, MuPDF, GStreamer or libarchive.
- The settings dialog (2026-09-18) added one defaulted method to the
  plugin trait (its settings hook).
- The Finder tag key was read only from 2026-09-30.
- Closed 2026-10-05: the query's date parser and the timeline's calendar
  math were duplicated; both now read `skwir_platform::calendar::Period`
  (`civil_day` refuses `2026-02-31`; `Calendar::year_of`).

---

## Built beyond the plan

What was built between and beside the phases. The rules these features
follow are in AGENTS.md and the ADRs named; the record of each (dates,
measurements, what was checked) is in `docs/STATUS-LOG.md`, under "Built
beyond the plan, by area" up to 2026-10-10 and under each day after; what
it left is in [What is left](#what-is-left).

- **Previews and readers:** PDF and Office pages and video stills through
  installed tools (ADR-003); PDFium, Poppler and MuPDF readers in helpers
  (ADR-004, 005, 014); video and audio through GStreamer, one plugin
  (ADR-008, 016); structured previews (ADR-009) in one frame
  (`docs/preview-frame.md`); music, playlists and MPRIS; reading mode and
  fixed-layout pages (ADR-012, 013); pages and pictures at the size shown
  (ADR-015); more formats (SVG, EXR, code, tables, notebooks, Mermaid,
  circuits, DXF, G-code); text documents (ADR-030); ImageMagick's
  pictures, thumbnailers isolated (ADR-031); text read where it is shown
  (ADR-051); archives extracted (ADR-048) and made (ADR-059); code
  colored by Ferroni (ADR-060); DjVu (ADR-061); links in text (ADR-029);
  fullscreen, slideshow, find marks and zoom around the pointer
  (2026-10-15 and 16); a film's one picture, posters at the size shown
  (2026-10-16 and 17); 3D models turned on the GPU, STEP, IGES and Rhino
  (ADR-067, 069); Full preview (ADR-068); pictures read ahead
  (2026-10-19); contents that highlight the part shown and pin beside it
  (2026-10-20).
- **The window:** settings (ADR-010), by subject (ADR-072); Usage
  (ADR-007); huge folders drawn as they stream (2026-09-25); tabs
  (ADR-018); the terminal (ADR-019); keys and key schemes (ADR-024), a
  held key's repeats (ADR-066); the context menu by file; rows and
  toolbars (2026-09-29); the tree view (ADR-025); trails (ADR-032); icon
  and code themes; names already taken (ADR-054); the settings file
  shared, the window's state and what the user makes kept apart (ADR-055
  to 057); Properties (ADR-058); files on the desktop's clipboard
  (ADR-062); gpui from Zed's main branch (ADR-064); screen readers
  (ADR-065); address completion (ADR-070); the desktop's colors and
  fonts (ADR-071, 073); the address field as an omnibar, a scope before
  its text and rows from sources, Jump folded into it (ADR-098,
  2026-10-26).
- **Search:** words for people and named periods; dates the user's
  (ADR-017); facets, then pills in the address; saved searches (ADR-057);
  searches following changes; hints from indexes (ADR-026).
- **Tags, places and the trash:** tag-preserving copies (ADR-006); notes
  (ratings, comments) and Finder tags; tags as places (ADR-032); the
  sidebar the user's and the trash a place (ADR-034); busy volumes;
  bookmarks; Open With; an encrypted volume's passphrase remembered in
  the keyring, a Mac's APFS volumes unlocked, Windows' BitLocker drives
  through its own dialog (ADR-088, 2026-10-25; macOS and Windows compiled
  only); tags drawn as one strip in every view, captions under the
  grid's names, a tag's menu on its chips and in the context menu, the
  files of a tag pointed at ringed, the place's own tag left out, each
  pinned tag a command (2026-10-26).
- **Upkeep:** the consistency review (2026-10-05); drops that ask, plans
  that pause, copies at a drive's pace, hard links as entries (ADR-049,
  2026-10-07 to 10); what woke while nothing changed (2026-10-10); the
  review before tagging 0.1.0 (2026-10-13).

---

## Maps

ADR-020; ADR-022 for what came later. Two plugins: `geo` (formats,
preview, thumbnails, the Map view) and `map-tiles` (backgrounds, the
only network code). Each step shipped on its own.

**Goal:** GPS tracks and routes preview on a map, and a Map pane view
shows the files that carry a position (photos first) where they were
taken.

**Steps:**

- **A. Track previews, offline.** Planned: core `geo` types (`LatLon`,
  `Bounds` aware of the ±180° meridian, a bounded `GeoDoc`; a waypoint
  has a kind and an altitude with its reference, so step D′ needs no
  core change), `InspectionKind::Map`; `elements/map.rs` (Web Mercator,
  pan, zoom, placement in prepaint); a map preview kind (Fit, Zoom,
  Tracks, Open in Maps through a `geo:` URI); GPX, KML/KMZ and GeoJSON
  parsed on a worker, thinned per zoom; distance, times, climb and
  extremes in Details; a themed graticule.
  *Built 2026-09-27.* A 6 MB, 100,000-point GPX reads in about 80 ms
  (release). Added: a scale bar, degree labels, start and end marks,
  place labels while at most 40 are in view. Deviations: no
  Douglas–Peucker levels (past `MAX_POINTS`, 200,000, every other point
  is kept, the facts counting all; a line is thinned to the pixels it
  covers when drawn, `elements::map::runs`); a line without times in KML
  or GeoJSON is a route; a wheel notch zooms half a level without Ctrl.
  Not built: the Tracks menu (every line is drawn). Not verified: the
  dark theme's map tokens.
- **B. Backgrounds.** Planned: a core `tiles` contract; `map-tiles` with
  the outline (Natural Earth, the default), OpenStreetMap once chosen,
  a custom URL; the tile cache, attribution, a link to the setting.
  *Built 2026-09-27.* Sources answer through a callback (downloads never
  hold a scheduler worker); `map_tiles::TileStore` asks in prepaint,
  cancels tiles that left the view, keeps 48 MB of decoded tiles while a
  map shows; a coarser tile stands in for a finer one. Verified against
  a local tile server: each tile asked once, from the cache after a
  restart, revalidated with `If-None-Match` (304), the User-Agent naming
  Skwir. Not verified: OpenStreetMap's own server.
- **B′. Tile servers.** *Built 2026-09-27* (ADR-020 and ADR-010
  addenda): servers as a records setting, addresses in the common forms
  (`{s}`, `{-y}`, `{q}`, `{r}`, `{key}`), headers, an API key in the
  keyring, a Test button, the background chosen from the map's toolbar.
  Verified in a private keyring against a local server that requires the
  key. Not verified live: the `{s}`, `{-y}`, `{q}` and `{r}` forms
  (tested as pure functions), a keyring that asks to be unlocked.
- **B″. Loading as map libraries do.** *Built 2026-09-27*: the middle of
  the view first, every tile kept, finer tiles standing in when zooming
  out, HTTP/2 through hyper instead of `ureq`. At 150 ms a tile: the
  middle shows in 175 ms (it came last), the whole view in 431 ms (was
  626), zooming out two levels about 1 s (was 1.9). Vendored patch 0008
  fixes a darker tile edge at fractional zooms.
- **C. The Map view.** Planned: a `ViewRole::Lens`; `geo` scans the
  location for EXIF positions and track files, visible first, cached by
  identity; markers with thumbnails, merged into clusters; selection,
  Enter, the preview and the rubber band as in other views; counts in
  the status bar; a placeholder offering subfolders; commands
  `geo.points`, `geo.status`, `geo.track`, `geo.open_external`,
  `geo.copy_coordinates`.
  *Built 2026-09-27*: `geo.scan`, `geo.points` (with the counts),
  `geo.release`, `geo.search_subfolders`; Ctrl+5. Deviations: the counts
  sit in a row at the top of the view with Subfolders and Fit;
  "Include subfolders" is a search (`query.search`); the scan goes in
  listing order (nothing says what is in view before positions are
  known); `geo.track`, `geo.open_external` and `geo.copy_coordinates`
  are not built (the preview has Open in Maps). Fixed with it in core:
  `App::waker_handle` and `OpenContext` call the hook installed when
  they are called. Verified on Xvfb with synthetic geotagged JPEGs and
  a GPX. Not verified: the double click that opens; a folder of
  thousands of pictures; the dark theme.
  Spreading (ADR-020 addendum): files at one place open as a spiral in
  the order they were taken; 307 pictures at one spot drawn at 0.5 %
  idle CPU. Deviations: the planned "+N" tile became dots; Escape does
  not fold (a click beside does).
  A Background button ends the Map view's row (2026-10-05). Not chosen
  in a window: OpenStreetMap, which downloads.
- **D. More formats and polish.** Planned: TCX, FIT, IGC, NMEA; track
  thumbnails; an elevation profile; positions of videos; "Show on map"
  on Details' position row. *Built 2026-09-27*: FIT by a bounded decoder
  of its own; thumbnails drawn to scale (tiny-skia, after the desktop's
  thumbnailers); video positions from `moov/udta/©xyz` or QuickTime's
  keys (`skwir_platform::mp4`); `Detail::action` in core.
- **D′. Flight and drone plans.** Planned: Garmin `.fpl`, MSFS/FSX
  `.pln`, X-Plane `.fms`, Little Navmap `.lnmpln`; QGroundControl
  `.plan`, MAVLink `.waypoints`, Litchi CSV, DJI WPML `.kmz`; claimed by
  extension, confirmed by content; departure, destination, legs,
  distance, altitude and waypoint count in Details.
  *Built 2026-09-27*, MAVLink also as `.mission` and `.txt`. Every
  format of D and D′ is confirmed by its first 4 KiB (the MIME database
  calls a `.fit` a FITS image, a `.pln` a PlanPerfect spreadsheet, a
  `.plan` a Calligra project), and the provider comes before tables and
  text, so a Litchi `.csv` is a mission. The Map view does not look
  inside `.txt` and `.csv`. An altitude of 0 in a flight plan means none
  given (the formats write 0 for a fix without one). Not verified: a
  real file of any of these formats (each was written from its
  documentation), a video's position in Details, the dark theme.
- **Later** (ADR-022): search by place (`near:`, "This area"),
  geotagging from a track and removing locations, offline place names:
  *built 2026-09-27* (ADR-023 for fields read on demand). Verified on
  Xvfb: pictures matched along a track through the user's time zone,
  their modification time kept to the nanosecond; a location removed
  and undone; `near:` within 500 m. Local PMTiles and MBTiles files
  preview as their maps (2026-10-05, ADR-022's part 4; checked on files
  the reference `pmtiles` tool wrote); not checked: a real offline map,
  Brotli or Zstandard archives (refused with words), the dark theme.
  Backgrounds from a file remain proposed.

Tests and fixtures use invented coordinates only. Planned and not
built: geotagged pictures and a 100,000-point GPX from `skwir-bench gen`,
and budgets for the GPX parse and the EXIF position scan per 1,000 files
(`crates/bench/budgets.toml` has none).

**Done when:** a GPX, a KML and a GeoJSON file, a flight plan and a
drone mission preview with their facts; a folder of geotagged pictures
shows on the Map view with shared selection; both plugins disable with
no trace (`plugins_toggle.rs`), cancelling tile requests in flight; no
request leaves Skwir until a network source is chosen; idle CPU returns
to 0 % after panning on `images10k`.

**Status:** every step built 2026-09-27; the gate is not declared. Met:
the previews and the Map view (synthetic files), both plugins in
`plugins_toggle.rs` (`map-tiles` cancels its downloads on disable), no
request before a network source is chosen. Not measured: idle CPU after
panning on `images10k`, which has no geotagged pictures.

---

## Notes everywhere

ADR-027, ADR-028.

**Goal:** a file's tags, rating and comment on any system (Linux, macOS,
Windows, USB drives) with nothing lost going back and forth; Skwir reads
every store it knows by default and writes the ones the user chooses.

**Steps** (each built 2026-09-30):

- **A. Stores, no change in behavior.** Core `notes`: the model, the
  owner-tagged `NotesStore` registry, `set_notes` whose inverse puts
  back every store's bytes. The tags plugin's XDG, Finder and
  AppleDouble code becomes three stores; AppleDouble also keeps the XDG
  attributes.
  *Built:* an edit captures the policy when the plan is made, writes the
  file's attributes first and, where refused, the store its folder
  already uses, else AppleDouble (now created); `restore_notes` refuses
  when another application changed a store since. One batch per folder,
  one `listxattr` per file. The index learns from `Executor::observe`.
- **B. Merge against what Skwir wrote.** A snapshot per file in the
  cache; another application's edit in one store wins; the union
  without a snapshot. Details lists where the notes are and offers to
  make the stores agree.
  *Built:* an emptied store is not an edit; the first store's edit wins
  over a later one's; each store says what it can hold
  (`NotesStore::fit`: no comma in a desktop tag, no rating in the
  Finder), so it differs only when it could agree; `notes.agree`. Undo
  forgets the snapshot.
- **C. Companions.** Name rules in a core registry; the listing pairs
  and hides them (`core.show_companions`); rename, move, copy, swap,
  trash, restore and delete carry them; a copy converts notes to what
  the destination keeps (ADR-006 amended).
  *Built:* as planned, the resource fork and Finder info kept as
  `user.com.apple.*`. A no-replace rename the filesystem refuses
  (bindfs, sshfs, many FUSE ones, where every copy used to fail) now
  looks, then renames. Tested against a `bindfs --xattr-none` mount
  (`SKWIR_NO_XATTR_DIR`). Deviation: a batch hydrated before its
  folder's listing completes still reads the folder once for `._` names.
- **D. Settings.** A Notes page (`notes.*`): each store read or not,
  written Off, Update existing or Create; on a volume without
  attributes, the folder's convention, else AppleDouble.
  *Built* (`App::register_notes_store`). Deviation: stores are listed by
  title, not the platform's own first.
- **E. XMP.** Sidecars (`x.ext.xmp`, `x.xmp` with
  `photoshop:SidecarForExtension`): `dc:subject`, `xmp:Rating`,
  `dc:description`, `xmp:Label` as a tag; updated where present, created
  on request; XMP inside files read on demand for Details.
  *Built:* `x.xmp` follows its file only while no other file shares its
  stem; ratings in whole stars (`fit` rounds a half); edits spliced into
  the text as it is, so darktable's history and unknown properties stay.
  Deviation: nested keywords are read for their last part, and a new tag
  is written at the top of the hierarchy.
- **F. TagSpaces.** `.ts/x.json` sidecars and a folder's `tsm.json`
  (hex colors to the nearest palette color, the exact value kept);
  `tsi.json` as a search hint (ADR-026).
  *Built:* the sidecar's thumbnail (`.ts/x.ext.jpg`) follows it; the
  description is the comment (TagSpaces has no rating); every other key
  stays. Deviation: `tsi.json`'s layout comes from its documentation,
  not from an index TagSpaces wrote; it is read tolerantly.
- **G. Names.** TagSpaces' `x[a b].ext` (where a `.ts` folder shows it
  is used) and filetags' `x -- a b.ext` (on request); writing them is a
  rename plan of its own.
  *Built:* the `.ts` condition matters because yt-dlp names downloads
  `title [id].ext`; filetags is off until turned on
  (`NotesStore::default_read`); `names.tag` and `names.untag`.
  Deviation, found 2026-10-18: ADR-027 made a name's writing opt-in, but
  the Notes page had no Write setting for one, so an edit never reached
  a name. Amended that day: Write is Off or Update for a name, and
  Update has an edit rename a file whose name holds tags, a `Rename`
  step unfolded beside it on the plan's thread (`Operation::unfolds`).

Tests use synthetic files built from each format's specification (not
our own encoders) and, where an application runs headless, its output.
The operations' property tests cover trees with companions.

**Done when:** a file tagged in Skwir keeps its notes copied to an exFAT
stick, read by a Mac's Finder there (checked on a Mac or with a `._` file
a Mac wrote), and copied back to a Linux disk; an edit made in another
application's store wins over Skwir's older one; undo restores every
store byte for byte; `tags_fill_us` holds its budget.

**Status:** the gate passes in part. The round trip was checked on a
FUSE mount refusing attributes, not on exFAT with a Mac reading it.
Checked: another application's later edit wins; undo is byte for byte;
`tags_fill_us` within budget with every store running (`flat100k`
227 ms of 550, `images10k` 41.5 ms of 120; `tag:` over a million files
2.66 s; load average 15). Windows' and macOS's own stores wait for
Phase 5b.

---

## Phase 5: Cross-platform

**Goal:** macOS and Windows builds that feel native in the ways that
matter.

**Scope:**
- macOS: QuickLook thumbnails, NSWorkspace icons and Open With, Trash,
  DiskArbitration volumes, Spotlight as a search provider, Finder tag
  xattr compatibility.
- Windows: the shell thumbnail cache, SHGetFileInfo icons, the Recycle
  Bin, WM_DEVICECHANGE volumes, Windows Search as a provider, the NTFS
  USN journal for fast subtree walks.
- File identity, paths and reflink/clone (APFS clonefile, ReFS block
  clone) abstracted in core.
- CI on all three systems; budgets measured on each.
- Packaging: Nix flake and AppImage/Flatpak, dmg, msi.

**Split (ADR-035, 2026-10-05):** no Mac or Windows machine is
available. 5a is done on Linux: every system's code compiled and linted
here (`just check-cross`: the GNU Windows target and macOS, Zig
compiling their C), tested where Linux or Wine can. 5b, on the machines,
is the original gate. `docs/PORTING.md` lists what is still Linux's;
each step removes its items. The dev shell has both targets, Zig and
cargo-zigbuild; `nix develop .#windows` adds Wine. At the start, every
C dependency and gpui's backend built for Windows and only Skwir's code
failed; for macOS, gpui's backend needs Apple's SDK.

"Compiled only" below means nothing of it ran; "under Wine" means it
ran there, in a prefix of its own.

**Steps:**

- **A. It compiles everywhere.** *Done 2026-10-05.* `just check-cross`
  is part of `just lint-all`: clippy with warnings as errors over the
  default members and their tests for `x86_64-pc-windows-gnu`, and over
  the workspace but `skwir-ui`, `skwir` and the native workers for
  `aarch64-apple-darwin`. Core reaches the system through its own seams
  (`fs::Dir`, `fs::stat`, `fs::xattr`, `op/sys.rs`, `os_path`); what has
  no code for a system says so (`system::not_yet`), and predicates that
  cannot know answer "unknown" (listed in `docs/PORTING.md`). Off Linux
  the terminal has no primary selection, types no `cd` and does not
  follow the shell's folder; ConPTY starts on Windows, never run. Linux
  behaviour unchanged. Nothing ran on Windows or macOS; `skwir-ui` is
  not compiled for macOS here.
- **B. Identity, paths and listing.** *Done 2026-10-05, but Usage*
  (ADR-036). Windows lists through the IDs its directory records carry
  (no open per row) and folds a 128-bit ID into `EntryId`'s 64 bits,
  NTFS's exactly; `Entry::hidden` carries Windows' attribute and a
  Mac's `UF_HIDDEN`. Paths: Windows' name rules, `PATHEXT`, no `\\?\`
  from canonicalization, WTF-8 names, `~\`, `file:///C:/`, a drive as
  one trail segment. `just test-windows`: core's 166 tests pass under
  Wine, four skipping their trash part. Gaps: Wine has no 128-bit
  directory records, so only the fallback ran; off Linux a folder moved
  while shown is not followed; Usage stays Unix's. SQLite is built in on
  Windows (ADR-022 amended).
- **C. Operations.** *Done 2026-10-05, but write-through off Linux*
  (waits for F). Planned: no-replace rename and swap (`renamex_np`;
  Windows `MoveFileExW`, no swap), system copies, write-through
  (unbuffered `CopyFileExW` or `fsync` per window, `FlushFileBuffers` or
  `F_FULLFSYNC` once per plan: a flush per file costs what a commit does
  on Linux), attributes, permissions, special files. Built: Windows'
  `CopyFileExW` (progress and Cancel, alternate streams kept, a
  download's mark among them) and `MoveFileExW`, a swap refused as a
  conflict; macOS `clonefile` when attributes are kept, every attribute
  kept but the two the system sets, pipes copied with `mkfifo`. Under
  Wine: every core copy test, not the streams (Wine has none). macOS
  compiled only.
- **D. Trash.** *Done 2026-10-05, but listing a Mac's trash.*
  `op::records` reads and writes freedesktop's pairs and the Recycle
  Bin's `$R` item and `$I` record (versions 1 and 2), kept as bytes in
  plans; `trash:` lists the user's folder (by SID) in each local drive's
  bin. The records ran on Linux from fixtures; the Windows trash never
  ran (Wine's bin is the freedesktop trash). macOS trashes with
  `trashItemAtURL`, keeping the resulting path for undo; the Mac's trash
  records no origin, so `trash:` lists nothing there and only an undo
  restores. Compiled only.
- **E. The environment.** *In part.* Done: settings and cache folders
  per system (`core::folders`); the keyring (Credential Manager under
  Wine; Keychain compiled only); a terminal (Windows Terminal, else a
  console of its own; macOS `open -a`, no program run in it yet); the
  first weekday (under Wine). Types (ADR-037, 2026-10-06): the system's
  registry first, shared-mime-info behind it; macOS's UTType compiled
  only, Windows' registry classes under Wine; the vendored `xdg-mime`
  reads on Windows (under Wine); the macOS and Windows packages carry
  the database compiled from `vendor/shared-mime-info` (2.5.1) by
  `tools/package/mime-database.sh`. Icons (2026-10-06, ADR-041 point 4,
  `system_icons`): NSWorkspace compiled only; Windows' system image list
  under Wine, whose stock icons are a stub (no trash icon there). Not
  done: Open With (Launch Services, Windows' handlers), owners by SID.
- **F. Places and volumes.** *In part, 2026-10-05.* Windows lists local
  drives as devices and the user's folders (under Wine); macOS its root,
  `/Volumes` as the Finder shows it (`getmntinfo`) and the user's
  folders (compiled only). Not done: eject and mount off Linux, Finder's
  favorites, Quick Access, hearing a drive plugged in on Windows.
  Windows writes removable drives through by default (quick removal);
  Skwir's write-through there and on a Mac flushes once per plan
  (`docs/PORTING.md`, section 3).
- **G. Thumbnails and isolation.** *In part, 2026-10-06* (ADR-039,
  ADR-041). macOS isolates converters, decoders and the document readers
  with `sandbox-exec` under Skwir's profile (compiled, never run);
  callers ask `sandbox::command` for the paths a program is given. Pandoc
  runs in its own `--sandbox` (2.15 or later) everywhere, and alone
  where nothing isolates (Windows, Linux without bubblewrap); tested on
  Linux both ways. LibreOffice and ImageMagick stay refused without
  isolation. Windows' AppContainer is decided and left for 5b. The
  system's thumbnails: Quick Look compiled only; the Windows shell
  under Wine, whose shell draws none. On every system the PDFium,
  Poppler and MuPDF helpers now run sandboxed
  (`sandbox::untrusted_reading`): a document opens 10 to 20 ms later
  (PDFium 6–8 → 25–27 ms, Poppler 18–19 → 26, MuPDF 4 → 13), pages no
  slower.
- **H. Processes and the terminal.** *In part, 2026-10-05.* Whether a
  process runs is asked of each system (cache sweeps used `/proc`); a
  helper and its children share a job object on Windows (under Wine);
  the shell's folder is read on macOS (`proc_pidinfo`, compiled only);
  the `cd` is quoted for PowerShell and `cmd` too (tested on Linux).
  Not done: ConPTY run; on Windows nothing types the `cd` (who has the
  terminal is not known there).
- **I. Media keys and search hints.** *In part, 2026-10-05.* MPRIS is
  registered on the free desktops alone; Spotlight answers a tag search
  (`mdfind`, compiled only). Not written: a Mac's Now Playing, Windows'
  media controls, Windows Search.
- **J. Keys and the window.** *Done 2026-10-05, but the outgoing drag.*
  Standard shortcuts are written `secondary` (Cmd on a Mac, Ctrl
  elsewhere) and compared as GPUI reads them (`keymap::normal_keys`);
  caps say ⌘ and ⌥; Ctrl+click, Ctrl+wheel and a kept band are Cmd on a
  Mac; what a Mac reserves (Cmd+H, Cmd+Space, Ctrl+arrows) keeps Ctrl;
  each system's fonts follow Inter. Not done: an outgoing drag on macOS
  and Windows (a gpui patch each; patch 0001 is Wayland's), Option as a
  Mac's copy modifier, Option+arrows for words. Nothing ran on a Mac.
- **K. Packages and CI.** *Linux's built and tried 2026-10-06; the
  others written, never run.* Linux: Debian's, RPMs and the AUR
  (ADR-040; [Release 0.1](#release-01-linux) step 10), each built on its
  distribution from one list of programs (`tools/package/programs`);
  helpers in `/usr/lib/skwir` (`/usr/libexec/skwir` on Fedora), found by
  `process::helper_folders`; a desktop entry and AppStream metainfo that
  validate. Every package carries PDFium (ADR-041,
  `tools/package/pdfium.lock`), so PDFs open everywhere; PDF thumbnails
  and office pages are drawn by the engine, `pdftoppm` only where none
  is. Debian and Fedora split `skwir` (the app, PDFium, archives, 3D
  models) from `skwir-media` (recommended) and `skwir-poppler`,
  `skwir-mermaid`, `skwir-mupdf` (suggested); Arch has one `skwir`
  whose Poppler and GStreamer libraries are optional, plus
  `skwir-mermaid` and `skwir-mupdf`. The main Debian package went from
  26 MB (85 MB installed) to 20 MB (67 MB). `tools/package/try.sh`
  builds each in a fresh Debian 13, Fedora 44 or Arch root (bubblewrap)
  and installs `skwir` alone, then with the rest; on Debian and Fedora
  the window drew through the distribution's Mesa. Every helper answers
  `--probe` (`process::probe`), so Settings says a helper is installed
  only where it works and a package may leave its libraries optional.
  The Nix-based AppImage recipe was retained but never verified. ADR-052
  replaces it with an x86-64 AppImage built from this Nix host in a
  rootless Ubuntu 22.04 build root (`just package-appimage`), with all
  programs from the shared list, pinned PDFium, a pinned GStreamer
  core/base built against the same glibc baseline, codecs and
  licence notices. Relative library paths keep host programs' environment
  intact; bundled helpers retain their private runtime inside the sandbox.
  A separate runtime-only root checks the actual image, every helper,
  synthetic PDFs and playback, the window's commands, sandboxed archive
  extraction and undo, and the host terminal's environment. *Built and
  tried 2026-10-11:* a 156 MB image, all 573 ELFs checked against glibc
  2.35, passed in runtime-only Ubuntu 22.04, Ubuntu 24.04 and Fedora 44
  roots. On this NixOS host, `appimage-run` passed the window's command checks on both
  Xvfb/lavapipe and Wayland/NVIDIA. The packaged player's synthetic
  video checks cover seeking, audio and subtitle tracks, rate, end and
  restart. Local workspace lint, cross checks, 1,474 nextest tests,
  dependency checks, reader checks and performance budgets pass; tests
  and budgets were rerun after the heavy build load. CI's same-artifact
  checks on Ubuntu 24.04 and Fedora 44 passed locally, not run remotely;
  other hardware and actual audio output have not been tried.
  Also written: the Nix package (`just package-nix`);
  the `.app` and `.dmg` (`tools/package/macos.sh`) and the `.msi`
  (`tools/package/windows/`), unsigned, without the native workers'
  libraries, never run. Those leave Mermaid and MuPDF out; there Skwir
  finds LibreOffice where it installs itself and searches Homebrew's
  and MacPorts' folders (compiled only); the Windows PDF helper loaded
  the pinned `pdfium.dll` under Wine. The manual full workflow's `native`
  job for macOS and Windows (not blocking until 5b) and the Linux package
  containers remain fallbacks; their hosted results have not been verified
  here. The archive helper is in both packages since 2026-10-20,
  libarchive built on Linux and linked statically (ADR-041's amendment).
  The media helper plays on both since 2026-10-20, GStreamer's own build
  carried (ADR-076). Left for the machines: everything run.
- **L. Budgets per system.** *Decided 2026-10-05* (ADR-038): the own
  share is `phys_footprint` on macOS and the private working set on
  Windows, 100 MiB each until 5b measures. `just idle-pss` says it
  measures nothing off Linux.

**Done when (5a, on Linux):** `just check-cross` is part of `lint-all`
and green; every step has its code or is recorded as not done, with
what ran under Wine and what no machine ran; the Linux packages build
and start; Linux's tests and budgets are unchanged.

**Done when (5b, the original gate):** the Phase 2 dogfood week is
repeated on each system, and the budgets pass on all three.

**Status:** 5a passed 2026-10-06; 5b waits for the machines and starts
from these steps, `docs/PORTING.md` and `docs/HELP-WANTED.md`. Evidence
for 5a:
- `just check-cross` green inside `just lint-all`; `just test-all` 1,176
  passed, poppler-check and mupdf-check pass; under Wine 766 passed and
  15 ignored (all but `skwir-ui`, `skwir` and the native workers, whose
  gpui imports Windows' ICU, which Wine lacks).
- The Linux packages build and install alone and together, every
  program found and working, the licences in place (`linux/check.sh`
  found Fedora's dropped on a system installed without documents;
  fixed).
- Budgets against the commit before 5a (`b52dfcb`): `bench-check` meets
  all 36 on an idle machine (the listing stream's three timings read
  over only while the machine compiled or was loaded); `idle-pss`,
  alternated back to back: own share 95.8 MiB against 95.4 (the
  binary's 0.4), first frame no slower. The first frame itself could
  not be measured that day: under load average 4 to 10, the build that
  read 114.5 ms on 2026-10-04 read 180 to 851 ms.
- The own share had grown 3.9 MiB since 2026-10-04 (91.5 → 95.4 MiB),
  before Phase 5. Bisected and fixed the same day: `img()` drew
  full-color SVG icons at twice their declared size (the Trash's
  `user-trash-full`, a 128 px SVG with filters in Adwaita, became 256 ×
  256 for a 16 px row), so icons are rasterized for the pixels they
  cover (`icons::element`); an idle worker gives back what its jobs
  freed (ADR-011's addendum); a name is typed without an allocation per
  pattern (vendored xdg-mime patch 0002: 1,411 allocations to type
  `notes.txt`, now 6; about 13 µs a name). Own share 95.3 → 92.9 (icons)
  → 91.5 MiB, what the 2026-10-04 commit read that day.

**Deviations:**
- Split into 5a and 5b (ADR-035); Phase 6 and Release 0.1 came before
  5b's gate (the user's choice).
- Packaging: no Flatpak (its sandbox hides the programs Skwir runs, and
  bubblewrap cannot run inside it). ADR-040 chose Debian's, RPMs and the
  AUR over an AppImage carrying Nix's C library, which cannot load other
  systems' graphics drivers. ADR-052 adds a conventional AppImage built
  against Ubuntu's compatibility baseline instead.
- No step yet covers the NTFS USN journal.

---

## Phase 6: The extension boundary

**Goal:** open the plugin system beyond the repo, safely. Only now,
because the traits are stable.

**Scope:**
- **Permission layer.** Capabilities per plugin (read contents, write,
  network, spawn processes) and per-folder policies ("never leaves this
  machine" for `~/.ssh`, anything under a `.private` marker). Required
  before any model provider exists.
- **Level 2, external processes.** A small line-delimited protocol
  (LSP-style) for provider processes; first for thumbnailers and tools
  we already call.
- **Level 1, WASM plugins.** wasmtime and the component model for
  providers, commands, metadata. Not views.
- **CLI and MCP server.** Thin consumers of the command registry:
  `skwir cmd rename ...`, and an MCP server exposing commands and plans
  so an external agent (e.g. Claude Code) can drive the app.
- Plugin authoring docs and a template crate.

**Done when:** one existing internal plugin is re-implemented as WASM
with no user-visible difference, and an external agent completes a
rename-and-move task over MCP that ends in a reviewable plan.

**Status:** gate passed 2026-10-07. Built that day, in this order:

- **CLI and MCP** (ADR-043): the `control` plugin (off by default)
  listens on a Unix socket in a folder of the user's alone (a named pipe
  on Windows); `skwir mcp` carries an agent's MCP there (`rmcp`);
  `skwir cmd` runs one command and prints JSON, in a windowless Skwir
  when none listens. An agent's plan is proposed and its tool answers
  with the verdict (`App::propose_awaiting`); `apply_plan` hands its
  plan back, so nothing skips review; commands take `entries` by
  absolute path (`App::targets`), so an agent's call never acts on what
  the user selected meanwhile.
- **Permission layer** (ADR-044): `Plugin::capabilities`, shown on the
  Plugins page; private folders (the Privacy page) never reach an agent
  or a plugin from outside; `command::Effect` says what each command
  does; an agent's acting call waits for the user in the review's queue,
  a user-only one is refused, answers lose private paths. Checked on
  Xvfb with a scripted agent: a `.ssh` and a `.private` folder refused
  by name; the home folder it listed held `files` where the user saw
  `.ssh`, `diary` and `files`.
- **Plugins from outside** (ADR-045): a folder with a `plugin.toml` in
  the data folders' `skwir/plugins` or `SKWIR_PLUGIN_PATH`, off until
  turned on; fields, thumbnails, previews and commands through one JSON
  protocol; files by handle, read only with `read`; plans need `write`
  and are reviewed. Level 2 runs it as a program under bubblewrap (no
  network unless declared; not on Windows). Checked by
  `crates/extend/tests/programs.rs`; the first thumbnailer from outside
  is `crates/plugins-outside/netpbm`.
- **Level 1** (ADR-046): Wasmtime (50.0.0 since 2026-10-28, minimum
  Rust 1.97; release step 8 says why), no WASI, Skwir's two imports,
  bounded work and memory, in `skwir-module-worker`, so Wasmtime never
  enters Skwir's process. Measured 2026-10-07 with Wasmtime 38: linked
  in, it cost 1.6 MiB of idle memory (94.8 against 93.2 MiB own share);
  a module is compiled once and kept in the plugin's cache folder, so
  the hex module answers in 4.0 ms against 188 ms compiling (release,
  median of 7), and a sandboxed helper restart took 23 ms against
  231 ms the first time.
- **Gate, first half:** the hexadecimal preview as a module
  (`crates/plugins-outside/hex-view`), the same page for page, match
  for match and error for error (`crates/extend/tests/modules.rs`, nine
  views of four synthetic files), and in the window the preview panel
  differs by no pixel.
- **Gate, second half:** first a scripted MCP client on Xvfb with the
  release binary (a rename, then a move, applied with Enter in the
  review; a trash discarded with Escape). Then Claude Code, given
  `skwir mcp`, against its own Skwir on Xvfb with synthetic files, from
  the tools' descriptions alone. It found what scripted agents had not,
  fixed: a listing's `total` still counted redacted private files, so a
  content search told whether a private file held a word (ADR-044's
  fifth point, `App::withheld`); `navigate` answered before listing
  (`entries` waits); reading a folder moved the user's view (`entries`
  takes `location`); `mkdir` only in the folder shown (`in`); reviews
  did not say an agent proposed (`Plan::by`); `undo` did not say what
  it took back; search syntax and some descriptions were written for
  Skwir's developers.
- **Previews from outside** (ADR-047): a plugin may answer with a file
  Skwir shows (`convert`), or a document a page at a time (`page`), read
  by Skwir's reader with text, links and outline; links open only if
  the plugin may use the network. Checked on Xvfb: a PPM shown as
  Netpbm's PNG; a `.log` made Markdown by a Python program plugin; a
  log shown in pages, its text selected and searched.

Left: the packages carry no module (the built-in hex preview stays
theirs); no typed WIT world (payloads are JSON); a module has no
network; program isolation is compiled only on macOS and absent on
Windows, where no program plugin runs; the Windows pipe keeps the
system's default security (`docs/PORTING.md`). Control stays off by
default. A command that only answers (`entries`, `tab.list`,
`control.status`) is data, which the palette leaves out
(`Declared::data`, 2026-10-13).

**Deviations:**
- Begun before 5b's gate, as the user chose: 5b needs machines this
  phase does not.
- The smoke test planned for Release 0.1 drives `skwir cmd`, not
  `skwir mcp`.

---

## Release 0.1: Linux

**Goal:** a first public release people can install and trust with
their files, and a repository others can help with. It comes before
Phase 7.

**Decided by the user (2026-10-07):**
- **Linux alone.** macOS and Windows need machines that are not here
  (5b); what each needs is written as steps others can take
  (`docs/HELP-WANTED.md`).
- **Plugins stay on or off by default as they are.**
- **The plugin protocol is experimental**, and its three unpublished
  versions are one, version 1 (ADR-047, its sixth point revised). Done
  2026-10-07.
- **The repository is published and CI set up last**, by the user.

**Steps** (how each was done, with what was checked: `docs/STATUS-LOG.md`,
"Release 0.1's steps"):

1. **Privacy of the repository.** Done 2026-10-11: every blob and commit
   message scanned for this machine's identifiers, two strings fixed,
   the history rewritten once before the first push; the commits made
   since are searched before each push.
2. **Panics.** Done 2026-10-07: counted by clippy's restriction lints,
   each `expect` stating what the code above made true; a command that
   panics fails its call, not its caller.
3. **Fuzzing** what reads files Skwir did not write. Done 2026-10-07 as
   property tests over any input (each crate's `any_input`,
   `crates/skwir/tests/any_file.rs`). Not covered: what runs in a helper,
   where a crash ends the helper, not Skwir.
4. **Files under failure.** Done 2026-10-07
   (`crates/core/tests/failures.rs`), with the hard links' fixes of
   2026-10-10 (ADR-049).
5. **What users keep from 0.1 on.** Done 2026-10-07: settings read with
   defaults for what is missing, a file that is no TOML set aside, the
   cache in a folder its layout names.
6. **In the window.** Done in part: `just smoke`, checks in the window, a
   run of 109 minutes, the budgets. Left: what was not checked in a
   window ([What is left](#what-is-left)).
7. **Clean.** Done 2026-10-07 but splitting files (no change needed one).
8. **Licences.** Apache-2.0 alone (ADR-042); every dependency allows it;
   the notices in every package, kept with the source (ADR-063), the
   Windows and macOS builds' included (ADR-074, ADR-075); `just deny`.
9. **Documents.** Done 2026-10-07: the README, `CONTRIBUTING.md`,
   `SECURITY.md`, `docs/HELP-WANTED.md`, the issue templates,
   `CHANGELOG.md`, `skwir --version` and `--help`.
10. **The release.** Named Skwir (2026-10-10); local releases and a
    minimal routine CI (ADR-053, `docs/RELEASING.md`); the AUR's tools;
    the app icon; Ubuntu's own packages; the tarball (ADR-052); released
    2026-10-13 as `v0.1.0`; the AUR's `skwir-bin` and `skwir-git`
    (2026-10-17).

**Done when:** every step above is done or says why not, the packages
built by the local gate (or manual CI) install and run on Debian, Ubuntu,
Fedora and Arch, and the history scan finds nothing personal.

**Status:** released 2026-10-13 as 0.1.0. Steps 1 to 10 are done or say
what is left.

---

## Names indexed by skwird

ADR-050. Step C built 2026-10-20; A, B and D wait for skwird's phase 1
(its repository; its phase 0, the measurements, done 2026-10-11).

**Goal:** name searches that answer from an index where it proves it is
current, and walk only where it does not. skwird is a project of its
own (its `docs/DESIGN.md` and `docs/PLAN.md`); these are Skwir's steps,
after Release 0.1 and after skwird's phase 1.

- **A. Coverage in the hints** (ADR-026, amended). `HintProvider` says,
  for a root and the facts a search needs, whether it covers them (live,
  checked, as of a time, not covered and why). A search under a covered
  root considers what the provider names and does not walk there.
  Tested with a fake provider that covers some roots and not others: the
  answer equals the walk's.
- **B. A skwird provider** in the query plugin, speaking protocol 0
  through skwird's client crate. It finds a system service, then a
  user's, then starts skwird on demand where it is installed. Tests use
  a fake skwird on a socket of the test's own, plus the real engine
  where it is installed.
- **C. plocate as of its time.** A search of `/` that plocate answers
  says when its database was last updated. **Built 2026-10-20**
  (ADR-050's amendment): a listing keeps what its answer rests on
  (`ListingEvent::AsOf`), the footer says "as of Yesterday 03:00" with a
  tooltip, `entries` answers `as_of`. Not checked: Orca reading the
  footer; a real system database (none on the reference machine).
- **D. Then, each decided on its own:** the address field searching as
  one types, Usage of a tree from skwird's folder totals, "recently
  changed", the tags index's checks; the harvest's enumeration and its
  journal (ADR-089, "Indexing and AI" below), which step A's proof
  words serve too.

---

## Colors and font from the desktop

ADR-071, ADR-073. Steps A and B built 2026-10-19 (Linux's portal), C and
D on 2026-10-20 (Plasma's scheme, GTK3 themes), E (macOS, Windows) and F
(Skwir's palettes) on 2026-11-05. The fonts by role (ADR-073) built 2026-10-19. How each step was built and
checked: `docs/STATUS-LOG.md`, "Colors and font from the desktop".

**Goal:** Skwir wears what the user chose on their desktop (the accent,
more contrast, a color scheme, the font's family) on controls that stay
its own, every color readable on what it is drawn on.

- **A. One derivation, guarded.** `theme.rs` derives every token from a
  few base colors; Skwir's light and dark palettes are that derivation
  over its own. The contrast guard (4.5:1 for text, 3:1 for the accent;
  7:1 and 4.5:1 when more contrast is asked) moves a foreground's
  lightness until it passes. **Built 2026-10-19.** Not checked in a
  window: the warning's color in Settings, the drag's count, link labels.
- **B. The portal on Linux.** Accent, contrast and fonts, read on a
  thread of its own and followed through `SettingChanged`; the last
  answers kept in the cache for the first frame; the `ui.desktop_colors`
  switch. **Built 2026-10-19.** Not checked: Xfce, Cinnamon, MATE and
  Plasma reading the font (their tests read the files and programs
  alone).
- **C. Plasma's color scheme** from `kdeglobals`, watched. **Built
  2026-10-20** (ADR-071's amendment): the user's `kdeglobals` over the
  system's, the scheme it names filling in, Breeze's when it names none;
  followed as Plasma replaces the file, an unchanged rewrite waking
  nothing.
- **D. GTK3 themes**, found as the icon theme is. **Built 2026-10-20**:
  the theme `GTK_THEME`, the desktop or the portal names, its
  `@define-color`s after its `@import`s within the theme, GTK's color
  functions evaluated; read again when the portal names another. A
  compiled theme (Arc) names none. Read from synthetic themes, then
  adw-gtk3, Materia and Breeze's schemes. Automatic takes a scheme's look
  when it has only one look's.
- Left of C and D: a real Plasma, Xfce, Cinnamon or MATE session; changes
  made in GSettings or xfconf themselves, which are not followed
  (`settings.ini`, qt5ct's and qt6ct's files are, since 2026-11-05).
- **E. macOS and Windows:** **Built 2026-11-05.** macOS: the colors
  AppKit draws its own windows in, each look's (ADR-071's amendment of
  point 2), Increase contrast's appearances when it is on; read again
  when the window's look changes or it is activated. Windows: the accent
  (`UISettings`) and a contrast theme's colors, followed in the registry.
  Windows tried under Wine for what it answers (its accent, no theme);
  macOS compiled only. Left: both on real machines
  (`docs/HELP-WANTED.md`).
- **F. Skwir's palettes** (ADR-071's amendment of 2026-11-05): twelve
  values of Colors from beside Automatic, GTK and Qt (Skwir, Paper,
  Slate, Black; Solarized, Nord, Gruvbox, Catppuccin, Dracula, Rosé Pine,
  Tokyo Night, Everforest), each a `Base` a look through the guard, with
  their makers' terminal colors and code themes (three carried in
  `skwir-platform`, beside bat's), each drawn in miniature in the
  dialog's list;
  declared on every system. **Built 2026-11-05.** Checked in a window on
  Xvfb (`docs/STATUS-LOG.md`, 2026-11-05). Not checked: a screen reader
  on the gallery (its cards are mouse targets; the dropdown is
  announced), macOS and Windows (compiled only), the terminal panel in
  each palette.
- **Fonts by role (ADR-073).** Three choices, Automatic or a family:
  `ui.font` (Appearance), `reading.font` (Reading › Text), `ui.code_font`
  (Reading › Code); Automatic is the desktop's font while Desktop colors
  is on, else Skwir's. The fonts are tokens of the theme, so a change
  shows at once; a long list of choices narrows as you type. **Built
  2026-10-19.** Not checked: Plasma, Xfce, Cinnamon and MATE reading
  their fonts, a screen reader on the long list, the session's own
  display.

---

## Remote folders

ADR-077 (revising ADR-021), docs/invariants/remote.md. Built 2026-10-21
on Linux: how each step was built and checked is in `docs/STATUS-LOG.md`,
"Remote folders".

**Goal:** an SFTP, SMB, WebDAV, FTP, NFS or rclone address opens as a
folder where everything works, through a tool the user has installed;
a server that stops answering holds nothing but its own work; adding a
protocol is a scheme a connector maps, or a connector.

- **A. Reach and lanes (core).** `fs::reach`: the mount table, a path
  remote or not, each GVfs mount apart; the scheduler's lanes (two jobs
  of a remote at once) and its stand-ins for held workers; the folder
  listing in its remote's lane; the watcher's lock no longer held across
  `inotify_add_watch`. **Built.**
- **B. Addresses and what is known of a remote (core, UI).**
  `ListingEvent::Redirect`; `Registry::remotes` (address, name,
  thumbnails rule); thumbnails and read-ahead by rule, in lanes; tabs
  kept by address; remote folders read again when the window comes back;
  the preview's and Properties' reads in lanes; `Asker::Connection`.
  **Built.**
- **C. Operations.** Move to Trash on a FUSE remote deletes, asked;
  nothing copied from a remote into the home trash; references across a
  remount by path; staging notes not looked at on a remote. **Built.**
- **D. Credentials.** `skwir_platform::askpass` and the `skwir`
  executable as `SSH_ASKPASS`; `process::run_leaving` for tools that
  daemonize. **Built.**
- **E. The remote plugin.** sshfs, GVfs, rclone and fstab connectors in
  the user's order; the Network section with Eject and the servers the
  tools know; thumbnails per remote (Automatic, Never, Small Files,
  Always); remote GTK bookmarks; searches and Usage leaving remotes be.
  **Built**, tested against servers on this machine (`just
  remote-check`: rclone, sshfs, and a GVfs of the test's own).
- **F. The window.** Checked on Xvfb: an `sftp://` address typed, the
  host and the password asked in the window, the folder, its thumbnails,
  a preview, Delete, Eject, a restart reconnecting its tab, a refused
  password ending the attempt; the address field completing known
  servers; a WebDAV server frozen (SIGSTOP) said not to answer while the
  rest of the window went on. **Built and checked.**
- **Reviewed** the same night: options that could reach ssh through an
  address, the deletion of a folder holding a mount, two connections to
  one server, the pool's and the watcher's behaviour behind a held
  server, GVfs's phones and cameras, references on FUSE remotes; each
  fixed where it would happen (STATUS-LOG).
- **Identity across a remount** (2026-10-21, after the user's go): on a
  mount whose inodes are made up, a file's inode number is told apart by
  the mount's own number, mixed into it (`fs::stat::file`: the kernel's
  unique one on Linux 6.8 and later), `FileId` keeping its 16 bytes; the
  cache knows such a file by its path; references there resolve at their
  path. A listing's maps hash with foldhash. **Built**, checked against a
  real rclone mount let go of and mounted again (`just remote-check`);
  listings measured as fast as before. Before Linux 6.8 the mount's
  number is the table's, which a later mount may take again.
- **Copies to and from a remote** (2026-10-21, ADR-078, the user's
  choice of the levers measured): a large file on a FUSE remote read in
  ranges at once; a mode made right not set again; copies in batches
  before their steps, by rsync through the connection an sshfs mount
  holds, else eight files at a time, plain files placed by the batch
  itself. **Built**, checked against OpenSSH's own server on this
  machine (`just remote-check`) and timed with a 20 ms round trip
  (STATUS-LOG). Then (the user's "tackle the not done yet points"):
  moves to another device in batches; Pause during a batch (a tool
  stopped, Skwir's own copy between files); copies a batch placed for
  steps that never ran taken back; files placed through FUSE linked from
  a folder of each worker's; eight `gio copy` for GVfs's SFTP mounts.
  rclone's own copy measured slower than its mount: not built. Eject
  lets go of a mount only watched (GVfs's trash watches every mount).
  **Built** and timed the same way. Then (the user's "fix points 1-7"):
  rsync for sshfs mounts the user made, by their own ssh; sftp sessions
  for uploads to SFTP-only servers; gio for GVfs's FTP, WebDAV and SMB;
  moves' originals on GVfs removed together; no per-step look across the
  network where a batch or a listing already knows; a plan's trash read
  from no network mount unasked (a dead server had held Undo for good);
  the remote plugin's refresh publishing mounts before servers; Eject of
  fstab mounts as of Skwir's; Pause on Windows; what uses a mount on
  macOS. Found on the way and fixed: copies onto WebDAV failed on modes
  it keeps none of, and asked about room it does not report; the room
  question waited with no window. **Built**, timed whole as the window
  runs a plan (STATUS-LOG). Then moves' originals on sshfs removed
  together by rsync or sftp, sshfs shown what went; and, found on the
  way, a pane showing a folder a tool wrote or removed in beside its
  mount reads it again at the plan's end (it never did). **Built**.
  Downloads through the mount sixteen files at once, filling the ten
  requests sshfs serves at once, the floor for an SFTP-only server.
  Then (the user's "tackle all the points"): deletions in a remote's
  folder together; Undo of a copy, an extraction or an archive made on
  a network folder a program mounts (which keeps no trash) works,
  removing what was made while unchanged; Replace there deletes for good,
  said first. What a copy cut short by a server that stopped answering
  left there is swept once this machine reaches it again. Copies within
  one server are made there (sftp's `cp`; `gio copy` within a GVfs
  mount). ADR-079 step 1: a light action starts beside a heavy one it
  shares nothing with, the history kept in the order asked; step 2:
  heavy actions on different disks or servers at once, one at a time on
  each. A plan an error stopped is retried, one action in the history,
  a large file going on from what got across (ADR-080). GVfs's NFS
  mounts go through `gio` every way, AFP's but down (measured). Then
  (the user's "tackle the not done points"): a phone's MTP through `gio`
  every way, measured on a phone (`just mtp-check`); a file alone by
  `gio` where it is faster than the bridge (down from NFS and AFP, both
  ways on MTP: ADR-078, 16), a large file down from NFS measured against
  NFS-Ganesha; a file with holes gone on from as any; a folder copied
  whole and a batch cut short gone on in, what they finished kept, the
  file under way gone on from (by rsync in a batch; ADR-080, 6 to 8).
  Windows' restartable copy declined: slower for every copy, untestable
  here. Then (2026-10-22, ADR-081, after an exploration of KIO): a copy
  is done once its close says so (a FUSE program tells there what its
  server refused; a move kept its original only by chance before), its
  time and mode given after; GVfs there where its daemon can be started,
  not only once running; KDE's remotes through kio-fuse (SMB, WebDAV,
  FTP; its SFTP, NFS and an iPhone's folders known), each server a
  folder of its mount, its passwords KDE's, no Eject, told to forget
  what it listed before a folder is read again, before GVfs on KDE's
  desktop. Tested against rclone's WebDAV and FTP servers with a KIO of
  the test's own. Then (ADR-082, the user's "implement the secrets related
  points"): a server's password kept in the desktop's keyring when the
  user asks (the prompt's Remember), the keyring answering first, GVfs's
  kept passwords read, rclone given one obscured in its environment after
  a refused login, Forget Password. SMB tested against Samba run as the
  user (KIO, rclone with a password, GVfs), KDE's remote places listed as
  servers, the order on KDE's desktop tested. KIO's copies in batches by
  `kioclient` (measured faster every way, FTP's writes through kio-fuse
  failing), a move's check leaving kio-fuse's made-up change times out,
  kio-fuse told to forget before what a cut-short copy left is swept.
  kio-fuse asks nothing (found in a window): KDE is let ask where it
  fails (`kioclient --interactive ls`), first over HTTPS (on a
  certificate not trusted kio-fuse waits for good); a mount's `Known`
  published before its folder is shown (an F5 there was flaky). Then
  (2026-10-23, the list the user validated for the night): a remote
  folder's bookmark renamed and moved among the folders', in the file's
  order; a batch cut short goes on from the file it was writing, by
  Skwir's own copy and sftp's `reput`, its source unchanged and its last
  part read back (`gio` and `kioclient` make it again); a plan a server
  stopped goes on by itself once the server answers again (ADR-080, 9);
  Automatic's Small files for a server off the local network once a
  copy down proved it fast this session (10 MB/s); servers nearby, found
  by Avahi and wsdd while the Nearby place is shown (ADR-084); keyrings
  tried (locked, KeePassXC, Wine's Credential Manager), a connection no
  longer held by a keyring's prompt; copies within one rclone mount Skwir
  made, by rclone on the server through its remote control.
- **Servers configured from Skwir** (2026-10-24, ADR-086, the goal the
  user set for the night): rclone's remotes added, edited, tested,
  signed in again and removed through rclone's remote control on a
  private socket, in forms the plugin describes and the window draws
  (`App::fill`, `skwir_core::form`), from what rclone describes of its
  kinds; secrets typed in the window alone, never an argument; a
  browser's sign-in rclone's own, waited for in a form with its page;
  remotes on remotes (crypt, alias, union, combine) with a remote
  picker; encrypting rclone's configuration offered, its password in
  the keyring; what rclone says a storage can do (no made-up size, Copy
  Public Link, a cloud's trash named in a deletion's question and toast);
  `s3://` addresses through the user's AWS configuration, nothing kept;
  one Add Server for every kind (an address kept with its name, its
  password asked by its first connection), KDE's network folders read;
  mount options per server; key login set up from Skwir (the agent's
  keys, the default one, a new one); Connect at Login by the system's
  service manager (ADR-087: a systemd user unit, a launch agent, a logon
  task, running Skwir's own mount command); rclone on Windows through
  WinFsp in a folder
  and on macOS through `nfsmount` (compiled only); rclone's Windows
  release tried under Wine (its remote control on the loopback behind a
  login). The Network section holds the servers, Bookmarks what the user
  bookmarked (ADR-086 amended 2026-10-24): a server added by its address
  is kept in Skwir's list or KDE's network folders (as chosen; KDE's on
  KDE's desktop by default), edited and removed where it is kept; a
  remote bookmark is made a server by Keep as Server. Settings lists
  every server on a Network page of its own (ADR-072 amended), each with
  what its menu does; the Network section's menu holds Add Server…. How
  each was checked is in `docs/STATUS-LOG.md`, "Servers configured from
  Skwir".
- **Devices before they are mounted** (2026-10-26, ADR-096, the user's
  report that a phone plugged in showed only in Dolphin): a phone, a
  camera or an iPhone GVfs reads has an address of its own (`mtp://`,
  `gphoto2://`, `afc://`), never a server's; what GVfs's volume monitors
  offer is listed among the devices by its volume's name, followed over
  the session bus, mounted once opened, the same place mounted, Eject
  through its address; one another program holds (KDE's `kiod6`) is said
  held, by whom. How it was checked is in `docs/STATUS-LOG.md`, "Devices
  before they are mounted".
- **A phone KDE holds** (2026-10-26, ADR-097, the user's choice for
  Plasma): the `kmtp` connector, before GVfs, opens a phone KDE's kmtpd
  holds through Skwir's helper `skwir-kmtp-worker` (FUSE, kmtpd's D-Bus
  interface), never loading kmtpd off KDE's desktop; the same address and
  place as GVfs's. How it was checked is in `docs/STATUS-LOG.md`, "A
  phone KDE holds, through kmtpd".
- **G. Left** (each its own step):
  - a device another program holds is named, never taken from it: KDE's
    `kiod6` keeps every phone it sees while it runs, so on a desktop with
    both, Dolphin and Skwir take turns (ADR-096), unless Skwir reaches
    it through `kmtpd` (ADR-097, built 2026-10-26); where its helper
    cannot work, the message says why before saying to quit `kiod6`. A
    Quit kiod6 button is not to be built (the user, 2026-10-26: Skwir
    reaches such a phone through kmtpd, and ending another desktop's
    daemon is no way to choose GVfs). The session bus is one connection
    for the program since (`skwir_platform::bus`: the desktop's style,
    the media controls, every tool's devices);
  - an iPhone's (AFC) ways chosen by requests counted against a
    stand-in, an iPhone's own pace not timed (no iPhone at hand);
  - on the machines (Phase 5b): Windows' and macOS's own shares (the
    `system` connector, compiled only), sshfs and rclone over macFUSE or
    FUSE-T; rclone through WinFsp on Windows and `nfsmount` on macOS,
    built 2026-10-24 and compiled only (WinFsp is a driver Wine lacks);
    NFS through Windows' own client, not built;
  - a browser's sign-in (Google Drive's, Dropbox's…) checked up to the
    page rclone serves, never signed in to a real provider; a cloud's
    trash named by what rclone and its options say, never seen filling;
  - on Windows, ssh's questions and rclone's reach the window through a
    named pipe (2026-10-25): rclone's own Windows release asked through
    it under Wine; Windows' own OpenSSH running Skwir as its askpass not
    tried (Wine has none), nor the pipe refusing another user;
  - a remote half made when Skwir ends mid-question is said so where it
    is listed (Finish Setting Up…, Remove…), never removed unasked; one
    half made by another program than Skwir is not known to be;
  - GNOME Files' recent servers (`~/.config/gtk-4.0/servers`, GTK 3's
    `gtk-3.0/servers`) are not read (the user's choice, 2026-10-24): a
    history GNOME fills on each connection, not a list the user keeps,
    which would fill the Network section with every server ever opened;
  - a server added by its address has no Test (rclone's lists its top
    through rclone's remote control; Skwir speaks no protocol itself, and
    connecting is opening it);
  - Keep as Server takes the bookmark out of the desktop's shared file,
    which other file managers read too: theirs goes as well;
  - Connect at Login never run by a real systemd, launchd or Task
    Scheduler here (systemd-analyze, the unit's command run as it would
    be, a stand-in systemctl; Wine's Task Scheduler does not start): a
    check under the user's own systemd manager is the user's to run
    (`tools/test/login-user-systemd.sh`): its first run, 2026-10-25,
    started, mounted and restarted the unit, and showed systemd forgets a
    unit turned off while mounted (ADR-087 amended); run again after the
    change, it passed whole; launchd and the Task Scheduler still never;
    on Windows rclone's remotes connect at login by a logon task since
    2026-10-25 (ADR-087 amended), its file and its remote control tried
    under Wine, never run by the Task Scheduler nor mounted by WinFsp
    (whether WinFsp takes its folder away after a crash is its
    documentation's word);
  - a GVfs mount made by another program is used, but its questions are
    the desktop's (Skwir's own connections offer Remember, ADR-082);
  - ADR-077's point 8, if asked: a helper of Skwir's serving SFTP
    through OpenSSH as a connector (the user did not ask, 2026-10-23);
  - GVfs's AFP backend dies now and then (a request freed before its
    last write called back, its request hash changed from two threads):
    the fix (`tools/test/gvfs-afp-request-lifetime.patch`) runs in the
    tests' GVfs and is to be offered upstream. With a desktop's GVfs, a
    copy over AFP may still meet it, and then says the folder it made is
    not one (the mount gone), not that the connection was lost.

---

## Indexing and AI (Phase 7)

ADR-089 to ADR-095, accepted 2026-10-26 after a review with the user:
the design is `docs/indexing-and-ai.md` (contracts, names, formats,
reasons), the binding rules `docs/invariants/indexes.md` and
`docs/invariants/models.md`. Nothing is built. It waits for Release 0.1
as before (the user's choice, 2026-10-07); skwird's phase 1 is not a
prerequisite: without it the harvest reconciles by the check.

**Goal:** searches that answer from an index where it proves it is
current and say what they have not read; files enriched, organized and
found with models that stay local and light by default; every change a
plan the user reviews; the window as light as before with every AI
plugin off.

**Steps**, each useful alone and each ending with its measurements in
`docs/STATUS-LOG.md`:

- **A. The extract provider, and `content:` over documents** (ADR-091;
  ADR-090's second point). The `extract` plugin as a `FieldProvider` for
  `extract.text`, `extract.hash`, `extract.phash` and `extract.facts`,
  read by the previews' readers under their bounds; the scan extracts a
  document on the fly where nothing covers it; `Fields::teach` from the
  previews and the thumbnail service; Properties' "In the index" row.
  No index, no model. Tests: a generated PDF and an ODT found by a word
  inside them, the scan and the preview reading alike; a special file,
  a link's own entry, a private file and a file past `extract.max_mb`
  never read; a failed read retried, never kept as empty; a file copied
  reuses its facts by hash. *Done when:* the tests pass, `tags_fill_us`
  and `just idle-pss` hold, and `extract_text_us` is measured and set.
- **B. The harvest and the trigram index** (ADR-089, ADR-090, ADR-092).
  Core `harvest` (scope, pace, the check, the proof words, indexers),
  `App::last_input`, the Index page and its settings,
  `extract.status`, `extract.build`, `extract.pause`, `extract.resume`,
  `extract.forget`, `skwir index` and the switch that writes its
  service manager's entry (`extract.while_closed`, ADR-087's shape); the `fulltext` plugin; hints with a proof (ADR-050's step A,
  which skwird's provider will use too); the footer's words. Tests:
  each way coverage is lost (`docs/invariants/indexes.md`, "Tests");
  the bench's queries through the index and the scan compared; the
  harvest pausing within one batch of a simulated input; a private
  folder added mid-harvest not read; an index deleted mid-search
  answered by the scan; the switch writing the entry with a
  stand-in manager, stopping it before removing it, and going back off
  when the manager fails; the entry's command run; a windowless run
  reading nothing without an idle signal and ending when a window
  opens. *Done when:* a `content:` search under a checked root answers
  without scanning and says as of when and how many files are not
  read; `just idle-pss` with `fulltext` on and idle equals the
  baseline; `harvest_yield_ms`, `trigram_segment_us` and
  `trigram_query_us` are set; `skwir-bench harvest` reports.
- **C. Cleaners, and steps that can be left out** (ADR-095, points 3 and
  7). `StepMark` in the core, the review's reasons, bars and checkboxes,
  a plan applied as its kept steps; `cleaners.duplicates`,
  `cleaners.similar_pictures`, `cleaners.empty_folders`,
  `cleaners.stale_downloads`. Tests: hard links are not duplicates; a
  step left out is not applied and not in the undo; undo restores the
  tree. *Done when:* a synthetic Downloads with duplicates, near-dupes
  and empties yields a correct, undoable plan whose steps say why.
- **D. Vectors over names, measured first** (ADR-094). `skwir-bench
  embed` on a bilingual synthetic set decides the static model; the
  `semantic` plugin's static tier over names and paths, `about:` and
  `like:`, Find Similar by meaning, `keys::SCORE`, `hints::scored` and
  `hints::coverage`; then text chunks of `extract.text`. *Done when:* a
  name search by meaning across French and English answers from a scope
  of 100,000 names under its budget and says as of when and what is not
  read; `static_embed_us` and `vector_search_us` are set.
- **E. Models and the helper** (ADR-093). Core `models`; the `models`
  plugin with the `ollama`, `installed`, `helper` and `remote` providers
  (`system` compiled only); `skwir-infer-worker` with `--probe` and
  `--build`, packaged as an optional helper; model packs, the catalogue,
  `models.fetch`; the indicator, `models.log`, the Models page; the
  dense tier of D through it. Tests: a stand-in provider on a socket of
  the test's own; an agent's call that would leave asked first; a
  private path refused; the helper ending after idle; a prompt built
  from a file named like an instruction changing nothing. *Done when:*
  the dense rerank improves the bench's recall and stays under a second
  for a hundred chunks on the reference machine, or is kept for what
  was embedded, and the record says which; `just idle-pss` with `models`
  on and idle equals the baseline; `just deny` and the notices are
  green.
- **F. Pictures** (ADR-094, point 3; ADR-091, point 6). Thumbnails
  embedded by the image tower, `about:` finding pictures by words,
  `like:` by looks, `semantic.labels`, `extract.ocr` through the OCR
  provider, off by default; `skwir-bench images`. *Done when:* a
  synthetic set of shapes, colours and rendered words is found by words
  and labelled at a measured precision, and `idle-pss` holds.
- **G. Suggestions, organizing, the address** (ADR-095). `suggest`
  (pills, `suggest.tags`, `suggest.describe`, `suggest.peek`,
  `suggested:`), `organize` (`organize.into`, `organize.propose` with
  the Groups facet, `organize.tags`), `query.ask` (its rules built as
  `query.interpret`, the address field's "Did you mean", ADR-098; rules without a
  provider, the model with one, always the parser's pills). *Done when:*
  "organize this folder" on a synthetic messy Downloads produces a
  correct, undoable plan with no language model, each step saying why;
  with one, the groups are named; a plan of suggestions undone restores
  every store's bytes.
- **H. The agent plugin, and speech** (ADR-095, point 6; ADR-091,
  point 6): last, once the flows above stand.

**Done when:** A to G built and checked on Linux; "organize this folder"
on a messy Downloads produces a correct, undoable plan using local
models only; disabling every AI plugin returns idle memory to the
baseline; the macOS and Windows providers compiled (`docs/PORTING.md`).

**Status:** designed 2026-10-26; nothing built. Measured ahead: the
trigram index (`skwir-bench trigram`, 2026-10-10; ADR-050's context has
the numbers). Deviations from the Phase 7 plan of 2026-09-18, each
recorded in its ADR: Tantivy dropped for trigrams (ADR-090);
`models-small` folded into the `semantic` plugin and the helper
(ADR-093, ADR-094); natural language in the address is `query.ask` over
the query language, not a `LocationProvider` of its own (ADR-095); the
permission layer it needs exists (Phase 6, ADR-044).

---

## Grammars and printing

Planned 2026-10-31 with the user from GitHub #3 (tree-sitter grammars)
and #2 (print from the preview, a booklet mode). Two tracks, a step a
commit; each step's checks and measurements go in `docs/STATUS-LOG.md`,
what was only compiled said so there and here.

**Goal:** code in any language a grammar exists for is colored, from
grammars the user added to bat, Sublime Text or Skwir, or that an editor
or the system installed (tree-sitter's included); whatever a preview
shows can be printed, as it is or as a booklet, or saved as a PDF that a
screen reader reads, from the window or a script.

**Grammars** (#3):

- **G0. The highlighter in `skwir-platform`.** The coloring engine
  (bat's grammars and themes, `Highlighter`, `Resume`, `Palette` as plain
  colors) moves out of `skwir-ui`'s `code_preview`, which keeps the
  drawing, so a plugin colors code as the window does (printing, P3).
  *Done when:* the code view and code blocks color as before and their
  tests pass where they moved.
- **G1. Grammars added to bat and Sublime Text** (`bat::syntaxes_dir`,
  Sublime Text's `Packages` folders and zipped packages): their
  `.sublime-syntax` files join bat's set, one of a language's name
  taking its place as in bat. The set built with them is kept in
  Skwir's cache, keyed by the versions and the files, so reading it
  again stays as quick as bat's own (ADR-060); one that does not read is
  named on the Reading page. *Done when:* a made-up language is colored,
  a broken file is named and the rest work, and a changed file builds
  the set again.
- **G2. ADR-110**, and the module helper on Wasmtime 48 (an LTS release,
  supported 24 months), the version tree-sitter 0.27's WebAssembly
  grammars run on: one Wasmtime for both helpers.
- **G3. Grammars found:** Skwir's folder (`<data>/grammars`, a grammar
  repository as `tree-sitter build` leaves it), Zed's installed
  extensions, Helix's runtime, Neovim's parsers, Emacs's
  (`libtree-sitter-LANG`), the system's packages (`/usr/lib/tree_sitter`,
  `/usr/share/tree-sitter/queries`) and the tree-sitter CLI's own
  (`parser-directories`, its library cache). A grammar without queries
  or file types borrows them from another source naming its language;
  `languages.toml` in Skwir's folder maps the rest. A name without an
  extension is told by its first line. The Reading page lists each
  grammar, where it was found and what is wrong with one.
- **G4. `skwir-tree-sitter-worker`:** native and WebAssembly grammars,
  isolated like the readers, the grammar folders readable; the text the
  preview holds (ADR-101), never read again; colors by line, injections
  from the other grammars found, cancellable, up to what a preview holds
  whole; one helper, let go of a minute after its last use; `--probe`,
  `--build`, the packages' lists.
- **G5. The second engine:** `Highlighter` colors with a tree-sitter
  grammar where the setting says (`reading.grammars`, bat first by
  default); capture names map to the scopes bat's themes color, so both
  engines wear one theme. *Done when:* a fixture grammar colors the code
  view and a fenced block, as text is read where it is shown.
- **G6. Folds and outline:** folds from a grammar's `folds.scm`, a code
  file's outline (functions, types) from `tags.scm` or Zed's
  `outline.scm`, in the preview's outline panel.
- **G7. Checked by hand** with Neovim's parsers and Zed's grammars on
  the reference machine; the guide and the invariants say it.
- **G8. A grammar for each language** (2026-11-01, ADR-110 revised after
  the user found the two lists of grammars read as settings): a Grammars
  section on the Reading page, "Automatic uses" then one list of
  languages, each with a dropdown (Automatic saying what it picks, bat,
  each source's tree-sitter grammar, Plain text), every source's grammar
  kept to choose from; record lists drawn inset everywhere, with a
  filter and folding when long.

**Printing** (#2):

- **P1. ADR-111, the `print` plugin and the system's dialog.** Print…
  (Ctrl+P), Print as Booklet…, Save as PDF and Save as Booklet in a 3xx
  submenu, the preview's More menu and the reader's bar; an agent's
  print through the review queue. The dialog is the system's: the print
  portal on Linux, the print panel on macOS (compiled only), the print
  dialog on Windows with the PDF helper drawing into the printer (tried
  under Wine). It offers All, Current page, Selection and typed pages;
  Skwir prints only those itself (`print::Pages`), the dialog's range
  reset. A PDF, an office document's and a plugin's PDF print as they
  are. Tests: a stand-in portal on a private bus keeps what it is given.
- **P2. Booklet:** a small prompt (signature, white margins trimmed,
  pages), the folding order in core, the PDF helper laying the pages out
  two to a sheet with PDFium, on the paper and duplex the dialog chose;
  pages picked before they are laid out; even and odd sides passed on.
- **P3. PDFs Skwir makes** (`skwir-print-worker`, krilla and parley),
  tagged for screen readers with bookmarks, title and author, in the
  light look, code colored and numbered, the name above and the page
  below: pictures and contact sheets; pages without a PDF (DjVu,
  comics, fixed-layout books, plugins' pages); text and code; tables and
  a folder's listing; rich text (Markdown, HTML, notebooks, documents
  and books in reading mode, email); drawings kept vector, font
  specimens. Not printed: audio, video, archives, maps, hex.
- **P4. Several files in one job, and PDFs from scripts:** `skwir cmd
  print.save_pdf` without a window.
- **P5. One Print…, Skwir's dialog first** (2026-11-01, ADR-111
  revised after the user tried P1 to P4): one Print… in the file's menu
  and the preview's, a form of Skwir's (to a printer or a PDF, the
  pages, as shown, a booklet with its folds drawn, a contact sheet), the
  system's dialog asked only for the printer, the paper and the sides;
  `print.print` and `print.save_pdf` the only commands; the preview's
  menu applying the plans its items answer.
- **P6. Four and eight pages a side, and the paper of a PDF**
  (2026-11-01, asked after P5, ADR-111 revised): booklets of 8 and 16
  pages a sheet, the order found by folding a sheet in simulation, the
  top rows upside down, the folds marked; the "Pages a side" row with
  pictures, how to fold said in the plan's toast; A4, Letter, A5, A3 or
  Legal chosen when saving, the system's first (`per_side`, `paper`).

**Status:** planned 2026-10-31. Built: G0 to G7, P1 to P4 (2026-10-31); `just lint-all` and `just test-all` pass on main after it. Checked on Linux; Windows and macOS compiled only. Deviation: P3, the print helper, came before P1, whose print path builds on it. Revised 2026-11-01 (G8): a grammar chosen for each language, the lists in Settings drawn as lists. Revised 2026-11-01 (P5): the four print items and the booklet prompt became one Print… and Skwir's own dialog, pages no longer left to the desktop's dialog (KDE's hides them); checked on Linux with GTK's portal on a display of its own, KDE's portal not tried; macOS and Windows compiled only. Built 2026-11-01 (P6): 4 and 8 a side, the paper of a PDF; checked on Linux by tests (the signatures, PDFium's placing, A5 upright) and in a window of its own (4 a side on A4, 8 a side on A3, saved, drawn and looked at; the folding toast); not printed on paper, nor folded; macOS and Windows compiled only. Left: a one-sheet zine (offered, not asked); a printer's own paper sizes beyond the five named when saving.

---

## Smaller copies

Planned 2026-11-01 with the user, after a trial of BentoPDF's Hyper
Compress against MuPDF, Ghostscript and qpdf (ADR-112 has its numbers).
A step a commit's worth; checks and measurements in `docs/STATUS-LOG.md`.

**Goal:** a smaller copy of a picture or a PDF beside it, from the
Compress submenu or the preview's menu, as small as asked, its text,
links, forms and look kept, by Skwir's own engine on every system; the
print path's own PDFs no larger than they need be.

- **S1. Pages a reader drew, printed as small as they read:** a DjVu's or
  a comic's pages, written as gray PNG or JPEG 90 by the engine instead
  of four-byte PNG (a PDF's pages were already composed vector by PDFium,
  ADR-111).
- **S2. The engine** (`skwir-shrink`, pure Rust, in the print helper's
  `--shrink` mode) and **Reduce Size… for pictures** (JPEG, PNG): four
  qualities, gray, location and camera details left out, at most a size;
  upright, colour profile and date kept; `op::Unneeded` for a copy that
  would not be smaller.
- **S3. Reduce Size… for PDFs:** images replaced where they are stored,
  in their own colour space, by the dpi they are drawn at; streams stored
  twice kept once, object streams; checked with the reader the settings
  choose, images put back where a page looks different; encrypted and
  signed PDFs left.
- **S4. ADR-112**, the guide and the invariants.
- **S5. What the user asked beyond:** JBIG2 for black-and-white scans
  (lossless, generic region), at most a size, gray, and Save as PDF's
  "Smaller file".

**Status:** planned and built 2026-11-01 (S1 to S5), with the user's
"go for 1-5". Checked on Linux in a window of its own (both menus, the
dialog, a copy's progress and toast, a photo's location removed, Save as
PDF's Smaller file) and against the trial's files; Windows and macOS
compiled only. Deviation: S1 shrank, a PDF's pages being composed vector
already; krilla's `pdf` feature, first planned for it, was not needed.
Left: fonts subset or merged, JPEG 2000 and CMYK JPEGs written, MRC; a
quicker Lossless over large pictures stored without loss; `just test-all`
and `release-check` not run.
- **S6. Progress for long work** (2026-11-01, asked after S5): a step
  whose work runs in a helper announces its size from its lane and counts
  it as the helper says how far it went (`progress` lines, `run_helper`),
  held with what it started while paused: Reduce Size and Save as PDF
  show a moving bar with Pause and Cancel; Print… to a printer shows a
  task's card (`App::tasks`) with Cancel while its PDF is made and sent.
  Left: what Skwir prepares before a helper starts (a long text read and
  colored) says nothing; a picture's copy moves in one step.
  Revised the same evening: what a copy's step says (not made, not under
  the size asked) follows what was made in the plan's own toast
  (`StepContext::say`), no longer a notification 1.5 s later that covered
  it and its Undo; a toast stays at least as long as its words take to
  read.

---

## Speed and time left

Asked 2026-11-02 by the user: a copy's or a move's speed and time left,
"especially useful for remote operations that take a long time", done
properly (ADR-113). Checks and measurements in `docs/STATUS-LOG.md`.

- **T1. Measured in the core** (`op::speed`, the plan's meter): the
  last 5 s for the speed, the last 20 s for the time left, from the
  first byte, nothing said while paused or when nothing moved of late.
- **T2. A plan's total from its start**: the room check's sizing kept
  step by step (`room::sized`, `Operation::work`), each copy taking its
  size instead of walking its tree again; undos and redos sized as they
  begin, their heavy steps alone; a batch's steps counted once.
- **T3. Honest edges**: clones and a copy gone on from never speed; a
  large remote file's ranges measured as they arrive; the copy asking
  about a quarter second's worth at a time; a plan of no bytes reckoned
  by its steps.
- **T4. Mounts that send after**: "then sent to the server" while
  writing, the sending's own speed and time left from rclone's queue.
- **T5. Said**: under the card's bar, in a long plan's toast ("4.8 GB in
  3 min 20 s"), and by the `running` command.

**Status:** planned and built 2026-11-02 (T1 to T5), with the user's "do
all five". Checked on Linux in a window of its own against a WebDAV
server throttled to 2 MiB/s through Skwir's own rclone mount (a download,
an upload with its sending phase, Pause and Resume while sending).
Found on the way and fixed: a batch of a folder and a file lost the
second run's first bytes (its count shared with the first), unseen
until a total was shown. Deviation: a large remote file's ranges are
still read 4 MiB at a time (the measured gain of `ranges`), so its bar
moves by 4 MiB while its speed follows what arrives; amended the same
day (ADR-113, "Cancel within a file" above): read a quarter second's
worth at a time, no slower, and shown as they arrive. Windows and macOS
compiled only. Left: sshfs, GVfs and kio-fuse not checked throttled in
a window (their bytes go as they are written, measured as any copy's);
the card's line not yet said to a screen reader.

---

## Phase 8: Polish and release

**Scope** (ongoing):
- Accessibility audit (a screen reader on each system) once gpui
  supports accessibility (ADR-001; it does since ADR-064, and ADR-065
  started exposing Skwir's places on 2026-10-18); IME, high-DPI, themes.
- Docs, a plugin gallery, a public release with an opinionated default
  plugin set.

**Status:** planned. Deviation: the first public release is Release 0.1,
before Phase 7.

---

## Cross-cutting rules

AGENTS.md and ADR-000 hold the rules. Per phase: end with a benchmark
run, update its status here (deviations included, honestly), and write
an ADR for any decision that closes a door.
