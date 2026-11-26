# ADR-050: Names are indexed by skwird

Status: accepted, 2026-10-10. Built in part: a search that plocate
answers says as of when (2026-10-20, amendment below); skwird's client
is copied into `vendor/` (2026-11-23, amendment below); coverage and the
provider are not built. Amends ADR-026 (an index that proves its
coverage answers) and the last point of ADR-003 (Skwir may start skwird
for the session). Amended by ADR-089 (2026-10-26): content stays out of
skwird; its journal feeds Skwir's derived indexes. Point 7 amended
2026-11-23: the client is a copy, published nowhere; and by ADR-150 the
same day: the provider is a plugin of its own, `skwird`, behind a seam
core owns, which plocate's answer joins. Point 2 amended 2026-11-26:
packages may carry skwird beside Skwir's program, found after `PATH`,
and Skwir says where it is missing.

## Context

- **Searches walk every time.** On the reference home (2.1M files, 138k
  folders), a walk statting every file took 5.6 s, a `content:` scan
  34-67 s (`skwir-bench trigram`, 2026-10-10).
- **A search of `/` trusts plocate** (`plocate::try_locate`): when it
  answers, the walk is skipped, though its database is as old as the
  last `updatedb` (usually a day). The one place a search breaks
  ADR-026's "the walk answers".
- **fsearch** (macOS, MIT) answers name queries over millions of files
  in about a millisecond from a memory-mapped index kept current by
  FSEvents. Its Linux port covers only home and watches every folder
  with inotify; each linking application adds its own watches (its
  README counts 465k of a 524k limit for one home).
- **Keeping an index current depends on system and privilege:**

  | System | As a user | Privileged |
  |---|---|---|
  | Linux | an inotify watch per folder, no history across restarts | root: fanotify on whole filesystems |
  | macOS | FSEvents, history replayed after restart | — |
  | Windows | one recursive watch, no history | administrator: the USN change journal |

- **Rules to keep:** no daemon in Skwir (ADR-000); system indexes may
  serve a term, but Skwir never installs or starts them (ADR-003); an
  index hints, the walk answers (ADR-026).

## Decision

1. **skwird, a separate project, indexes names** (design: its
   repository's `docs/DESIGN.md`). Skwir is one client: it speaks
   skwird's protocol, never links its engine, asks nothing another
   client could not, and stays complete without it (it walks).
2. **Three lifetimes, one protocol:** system service, user service, or
   on demand (started by its first client, ending a minute after its
   last). Skwir never installs or enables a service; packages may
   recommend skwird. Where installed, Skwir may start it on demand: a
   helper ending with Skwir, not a daemon (ADR-000). This amends
   ADR-003's "none is ever started by Skwir", for skwird alone.
3. **Coverage, not hints.** Each answer states, per root and kind of
   fact (names, attributes): live since a time (nothing dropped,
   applied up to a barrier the search set), checked at a time, as of a
   time, or not covered and why. ADR-026 is amended to trust proven
   coverage.
4. **Skwir keeps what a search means.** The query plugin compiles the
   terms skwird can answer (names, kinds, sizes, times as instants from
   the user's calendar, ADR-017) and checks each file skwird names like
   any other: stat, providers, every term. Types stay the MIME
   database's (ADR-037); syntax stays the query plugin's.
5. **Identity.** Answers carry each entry's and its folder's device and
   inode: a result is an `EntryRef` with identity, and two names of one
   file are two results (ADR-049).
6. **Privacy.** skwird never indexes private folders (honours `.private`
   itself, ADR-044); a system service answers each user only what that
   user could list; Skwir still applies its own privacy rules before an
   agent or outside plugin sees anything.
7. **A provider in the query plugin**, through skwird's client crate
   once published (no git dependency: Skwir builds from its lockfile).
   The core's hints gain coverage (ADR-026), so no plugin depends on
   another.

## Consequences

- Under a covered root (live or just checked), a home name search
  answers from the index without walking; elsewhere it hints and walks.
- A plocate-answered search of `/` still skips the walk but says its
  answer is as of the last update (built 2026-10-20, amendment below).
  skwird's system service, where it covers `/`, replaces plocate.
- Possible later uses, each a decision of its own: the address field as
  one types, Usage of a whole tree from folder totals, "recently
  changed", the tags index's checks, a file's other names.
- **Built:** plocate's answer dated (2026-10-20). The rest, in order:
  1. skwird phase 0: measure on the reference home, warm and cold, and
     fanotify on btrfs as root;
  2. skwird phase 1: a Linux user engine, started on demand;
  3. hints' coverage and the provider (`docs/PLAN.md`, "Names indexed
     by skwird").

  Release 0.1 ships without it.

## Amendments

### 2026-10-20: plocate's answer is dated

The Consequences' "will say its answer is as of the last update", built
(`docs/PLAN.md`, "Names indexed by skwird", step C):

- **In the core,** a listing keeps what its answer rests on:
  `location::AsOf` (what answered, in the user's words, and when it was
  last brought up to date) through `ListingEvent::AsOf(Option<AsOf>)`,
  read with `Listing::as_of` (grown by ADR-150 into `location::RestsOn`,
  its proof live, checked or as of: `ListingEvent::RestsOn`,
  `Listing::rests_on`). Each run of a search says it, `None` for a
  walk, so a refresh that walks takes back what plocate said. Point 3's
  coverage (live, checked) is to extend this, not to add another way of
  saying it.
- **The time** is the oldest modification time of the databases plocate
  read: its default, which its help names (`(default is
  /var/lib/plocate/plocate.db)`; NixOS's is `/var/cache/locatedb`), then
  those `LOCATE_PATH` adds, as plocate reads them. `updatedb` writes a
  database whole, so its time is the update's. When none can be stat-ed
  the answer is as of a time that cannot be told, and says so. A missing
  default makes plocate fail, and the search walks.
- **Said:** the pane's footer adds "as of Yesterday 03:00" after the
  count, its tooltip what answered and what that leaves out; the status
  line a screen reader reads says it too; `entries` answers `as_of`
  (`by`, `updated` in seconds), to scripts and agents.
- **Checked:** unit tests (the help's default, the oldest of several
  databases, the listing keeping and taking it back, the footer's words);
  a search through the app with a stand-in plocate on `PATH`, its
  listing and its `entries`, then a walk saying nothing; plocate 1.1.25's
  help and its reading of `LOCATE_PATH` (a missing default fails, exit 1).
  In a window on Xvfb (debug build, a stand-in plocate whose database was
  dated the day before at 03:00): the footer and its tooltip, kept by F5,
  gone in a search of a folder. Not checked: Orca reading the status
  line; a real system database (the reference machine has no plocate).

### 2026-11-23: skwird's client is a copy, published nowhere

Point 7 waited for skwird's client crate "once published" on crates.io.
None will be: skwird publishes no crate (the user's choice, 2026-11-11),
and Skwir still takes no git dependency. So:

- **Skwir keeps copies** of skwird's two crates, `vendor/skwird-protocol`
  (the messages) and `vendor/skwird-client` (finding or starting an
  engine, asking it, and a fake engine for tests), each with a
  `README.skwir.md` and a `VENDORED` naming the skwird commit copied.
  skwird's `tools/vendor-client` writes them, their manifests standing
  alone; Skwir copies again to update and never patches them here.
- **Outside the workspace,** as `vendor/zed` is: taken by path
  (`skwird-client` in `[workspace.dependencies]`), never linted or
  tested as Skwir's own. Their only dependencies, serde and serde_json,
  come from crates.io through Skwir's lockfile and are held to the
  advisories as Skwir's. `tools/deny/vendored.py` holds only copies that
  stand for a release, and these stand for none.
- **Which crate takes the client** was decided the same day (ADR-150):
  a plugin of its own, `skwird`, not the query plugin.
- **Checked:** the copies of skwird 40f7f87 built alone and offline with
  Skwir's toolchain (Rust 1.99.1) and the serde and serde_json versions
  its lockfile holds; their tests passed (the protocol's transcripts, the
  fake engine); they compiled for Windows (`x86_64-pc-windows-gnu`) and
  macOS (`aarch64-apple-darwin`).

### 2026-11-26: packages may carry skwird, and Skwir says when it is missing

Point 2 let packages recommend skwird, and Skwir's ignored it: most
people never had it, every search walked, and nothing said why. So:

- **Packages may carry skwird beside Skwir's own program**, the release
  `tools/package/skwird.lock` names: Windows' installer and zip
  (`skwird.exe`, and `skwirdw.exe`, which its logon task runs), the
  Mac's app (in `Contents/MacOS`), and the Linux tarball (in its `bin`).
  The `.deb` and the `.rpm` recommend skwird's own package, and the AUR's
  lists it among its optional dependencies.
- **The AppImage carries none.** Its files are a FUSE mount that ends
  with Skwir, while an engine started on demand outlives its last client
  by a minute (point 2): its program would go from under it.
- **Found after `PATH`**, in one place (the plugin's `program::find`,
  which starting an engine and the Plugins page's requirement both ask):
  on `PATH` first, with Homebrew's folders on macOS, so that an installed
  package takes over and a machine keeps one engine and one index (an
  engine meeting an index format it does not read rebuilds it whole);
  else beside the program that runs, its links followed
  (`folders::program`), as Skwir's helpers are found. Only there: the
  packages that keep helpers in `lib/skwir` or `libexec/skwir` carry no
  skwird. On Windows the file is `skwird.exe`.
- **Started as from `PATH`.** A carried copy is started on demand, by a
  search or a drive's index, exactly as an installed one, and Skwir
  registers it to start neither at login nor at boot. A user who runs its
  `logon add` or `service add` registers that copy, and takes it back
  with that copy before removing Skwir (the guide says how). Windows'
  installer, per user, runs `skwird logon remove` as it uninstalls; the
  zip and the Mac's app have no uninstaller (ADR-075, amended). skwird's
  system service is never to be registered from the carried copy: it
  sits in a folder the user's own programs can change, under a service
  that runs as the system.
- **Said where it is missing**, through core's seam, the shell naming no
  index (`names::Absent`, `NameIndex::absent`): where no skwird is found
  and no system service serves (its socket), Settings › Index shows
  the index's words above its settings. Searches go through the folders
  one by one, and skwird answers name searches across whole disks at once
  from an index it keeps current. Beside the words, Get skwird opens the
  website's download page at skwird's section
  (`https://skwir.org/download#skwird`). A search that walked says
  `no index` in its footer, with those words as its tooltip; a click
  opens Settings › Index. A note, never a question or a toast. Kept
  live no longer promises that the next search starts an engine.
- **From memory.** Whether skwird is missing is looked at off the
  window's thread, as a window comes to the front (`Plugin::on_looked_at`)
  and as a search finds no engine; an engine answering clears it. The
  window asks it as it draws, and is woken only when the answer changes.
  Until a first look, nothing is said.
- **Checked:** the lookup's tests (a fake program folder with an
  executable `skwird` found, one on `PATH` taken over it, one without its
  execute bit none; on Windows `skwird.exe` alone, compiled only), core's
  (no index absent by default, those that are in their order, withdrawn
  with their owner) and the plugin's (said missing once a search finds
  neither skwird nor an engine, never before, nor where skwird is found or
  an engine answers; the app's indexes after a look, gone with the plugin,
  never in a headless test's app). In a window on a headless sway, `PATH`
  without skwird: the Index page's words and button, Get skwird handing
  `https://skwir.org/download#skwird` to a stand-in `xdg-open`, a search's
  `no index`, its tooltip and its click to Settings › Index. With a
  stand-in `skwird` beside the program, then on `PATH` alone: no words on
  the Index page, none in a search's footer, and a refreshed search
  starting that copy (`skwird serve`). A stand-in put on `PATH` while
  Skwir ran: the next search said nothing more. Not checked: Windows and
  macOS packages carrying it (other tracks), a real engine started from a
  carried copy.
