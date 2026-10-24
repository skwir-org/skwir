# Operation invariants: plans, copies, volumes, the trash

Read this before changing an operation (`skwir_core::op`), the executor,
copies, volumes or the trash's operations. Binding, like `AGENTS.md`.

## Plans and steps

- Commands that change files return a `Plan`; the caller applies it. A
  user's single action applies at once, undoable; batches and anything
  from a background task or an agent go through `App::propose`. A
  stopped action going on by itself once its server answers again is
  that action, asked already, going on (ADR-080, 9), not a new one.
- Every operation has an inverse, a dry-run `check`, and is idempotent.
- Steps reference entries by `EntryRef` (`AGENTS.md`, "Files and
  entries").
- A step that changes metadata states its words (`op::Words`: what it
  will do, for the review; what it did, for the toast). What a plan left
  out goes in its note (`Plan::with_note`), never in its title.
- **Unfolding.** A step whose steps depend on what it reads says them
  (`Operation::unfolds`: a notes edit and the rename of a name holding
  the file's tags). They replace it before anything is settled or
  checked, on the plan's thread, and in the review's dry run
  (`op::unfold`, `Plan::diff`, `Plan::review`): what is reviewed is what
  is applied, each step with its own inverse. A step it returns never
  unfolds again. The plan's items stay the steps it was asked as
  (`Plan::items`), which the toast counts.
- A step Undo cannot reverse (`Operation::undoable`: deleting for good)
  makes its plan ask before any gesture applies it
  (`Shell::apply_user_plan`), listing what it deletes
  ([shell.md](shell.md), "Asking the user").
- **Room.** A plan the user asked for as one action
  (`Executor::execute_whole`) compares what it will write
  (`Operation::writes`: copies, cross-filesystem moves, a multi-name file
  once) with each volume's free space (`op::room`) and asks before
  starting when one is short. It never refuses unasked (network shares
  may under-report).

- **Access (ADR-058).** `set_access` changes a mode as chmod takes it
  (`op::access::Mode`: octal, a folder's set-ID bits kept unless five
  digits say otherwise; symbolic clauses with `X`, `s`, `t` and copies;
  a clause naming no class leaves out what the umask does, read from
  `/proc/self/status` on Linux and never by setting it, and where the
  system does not say it (macOS) one that would give something is
  refused), and a group by number, never the owner.
  - It changes what a link given leads to, and with `inside` what a
    folder holds: no link inside is followed, no other drive mounted
    inside is reached, no trash (`.Trash-<uid>`, a home's `Trash`,
    `$Recycle.Bin`) is changed, no folder met above is walked again.
  - Nothing is reached by a path that a link put in place mid-walk
    could redirect: each folder is listed through its descriptor
    (`Dir::open_folder`, no link followed, checked to be the folder
    stat-ed), and each item is changed through a hold on it
    (`op::sys::access::Held`: `O_PATH` checked against the stat, the
    mode set through `/proc/self/fd` on Linux; `AT_SYMLINK_NOFOLLOW`
    on macOS). Windows changes by path ([PORTING.md](../PORTING.md)).
  - A folder whose owner can still list and open it once changed is
    changed before what it holds, any other after it, so `u=r` and `u=`
    reach every item. Putting back orders by the modes put back the
    same way (`back_order`), so Undo and Redo reach what the change did.
  - A new group is given first and the mode worked out from what the
    system left: the set-ID bits a group change clears stay cleared
    unless the mode asked sets them (chgrp, then chmod).
  - It reads the root's mode back and fails, putting back what it
    changed, when the drive kept nothing; a failure partway puts back the
    same way, and says what it could not put back. Its inverse
    (`access_back`) holds each changed item's mode and group, which the
    journal counts (`Effect::held`); it puts back what it can, leaves
    what is gone since, and fails naming what it could not.
  - `check` walks what it would change (stopping when the plan is):
    nothing to change is already done, so a recursive plan applied again
    is too. It refuses what the user does not own (root aside), an item
    inside of someone else's that would change (named), a group they
    are not in, a read-only drive and the trash's files, in words; a
    group given to an item whose group the user is not in is pending
    but not undoable.

## Running

- Plans run on a thread of their own, never a worker.
- A failed recursive removal may already have removed writable entries.
  Its error names and counts what actually remains; tests compare that
  report with the surviving tree, never assume a directory traversal order.
- A plan of several steps shows a bar with Pause and Cancel
  (`Executor::pause`, `Executor::cancel`). Pausing waits where Cancel
  would be answered (`StepContext::check_cancelled`), so long steps call
  it between units of work.
- A plan is told started (`ExecEvent::Started`, as asked) before its
  checks (unfolding, names taken, the dry run, room), which may read
  whole trees: Pause and Cancel reach it there (`Executor::show`), and
  cancelled it applies nothing. What it did is counted as it ends
  (`ExecEvent::Finished`'s `items`, settled). A tree compared with what
  is there under its name (`builtin::same_tree`) is read once a plan,
  its settling, dry run and run sharing the verdict while nothing of
  either, nor of their companions, changed (`StepContext::compared`).
- **At once (ADR-079).** Plans, undos and redos start in the order asked,
  except that one starts beside those running when nothing asked before
  it that has not ended conflicts with it (`op::footprint`: what each
  step touches, `Operation::footprint`), when it is heavy no other heavy
  one on one of its channels runs or waits before it, and fewer than
  four run
  (`executor::AT_ONCE`). A step that cannot say what it touches (the
  default, a plugin's) makes its plan run alone. A heavy plan's
  channels (`Footprint::with_channels`) are the disks under what it
  reads and writes (`fs::volume::disks_of`, through LUKS, LVM and RAID,
  a partition its disk), a remote by its mount, a volume kept in memory
  by its own; a volume whose disks cannot be told (all of them on macOS
  and Windows, a ZFS pool) meets every disk. Two plans making names
  in one folder meet. Each running plan has its bar; what waits is
  listed under them, each saying what it waits for (`Executor::waiting`:
  an action asked before it that it meets, by its title; the disk; the
  server; one of the four to end); a paused plan keeps what it holds. The journal keeps
  the order asked (`JournalEntry::seq`): Undo takes the last plan asked,
  waiting while it runs (`Executor::undo_target`, which the status bar
  names), and a plan asked after an undo drops its redo. A toast's Undo
  names the plan it reports (`undo`'s `plan`, `Executor::undo_of`):
  taken back alone when no plan asked after it, its entry still in the
  history, running or waiting, touched what it did
  (`JournalEntry::touched`, the footprints its runs had), else refused,
  saying which did, and nothing taken back. A step's
  footprint names what its `describe` and its companions touch: a new
  step kind says its own, or runs alone.
- **Retry (ADR-080).** A plan a step's error stopped keeps the steps it
  had left, unfolded and settled (`Executor::retry`, the toast's Retry,
  the `retry` command; the eight latest, `STOPPED_KEPT`). A retry is a
  plan asked now, checked whole; refused before it applies anything, it
  stays to be retried. What it applies joins the stopped plan's journal
  entry, its inverses first, unless an entry asked since sits after
  that one: then it has its own. Cancel and a failed write-out are not
  retried.
- **Going on by itself (ADR-080, 9).** A plan stopped by a step that
  reaches a remote (its footprint's paths, `fs::reach`) notes the
  server's mount; that mount is asked whether it answers
  (`fs::reach::answers`: still mounted, how full, its top read), in its
  lane, one question at a time, at once and then every 15 s
  (`Executor::look_for`). Answered at once, the plan was stopped by
  something else: Retry alone. Not answered, or slowly, it waits
  (`ExecEvent::WaitsForServer`, said in its toast); once answered, it is
  retried as Retry would (`ExecEvent::GoesOn`, its card says "going
  on"), three times at most a server. Nothing is read to classify an
  error: its words are the system's, translated.
- The bar counts items, the current item's bytes moving it between two
  counts; a one-item plan counts bytes (`toast::bar`).
- **Quit** asks while a plan runs (its key or the window's close button,
  `Shell::quit`), then cancels and waits, out of sight, for partial work
  to be taken back (`Executor::wind_down`): a copy cut short leaves
  nothing in its destination.

## Staging

- A step that makes something in a destination stages it
  (`StepContext::stage`, then `Staging::place`): copies, extracted
  archives. Except where the destination moves nothing between its
  folders (`fs::reach::writes_in_place`: a camera through GVfs's
  gPhoto2, ADR-083): a copy there is written in place under its name,
  never over what is there, and taken back whole when it fails, what it
  made and nothing else (`builtin::copy_in_place`: a sidecar there before
  it stays); a batch there writes in place too, by a
  transfer that does (`Transfer::writes_in_place`) or Skwir's own
  (`transfer::batch_in_place`); anything else staged is refused there.
- A copy holds a lock on its staging folder while it runs. Unlocked
  staging folders are leftovers of a crash or kill: swept by a later copy
  into that destination, and at start for the destinations an earlier run
  was copying into (`op::staging`). Only where this machine alone writes.
- On a remote, where other machines' copies stage too and their locks
  are not seen here, only what this machine noted is swept: a staging
  folder made there is noted itself (`UnderWay::mark`), the note kept
  when the folder cannot be removed (its server stopped answering), and
  what such notes name is removed once this machine reaches that remote
  again (`UnderWay::sweep_noted`: before a copy stages in that folder,
  and when its mount is found answering, `App::sweep_left_on`, in its
  lane). A run's start never looks at a remote.
- **A large file cut short (ADR-080).** A copy step that fails partway
  through a file it copies byte by byte from its start
  (`StepContext::copying`; a file with holes, written in order, too),
  alone or in a folder copied whole, having moved `KEEP_FROM` bytes,
  keeps what it staged (`StepContext::keep_partial`, told by the file it
  was writing, never by looking): the plan's staging folder there leaves
  its context with its lock and notes (`staging::Partial`, held by the
  stopped plan), so no sweep takes it, and is removed with it when let
  go. The retry's context takes it back (`StepContext::with_partial`):
  its copy of that source goes on from the partial when the source's
  length and time are unchanged and the partial's last MiB reads as the
  source's (`builtin::goes_on_from`; else one of the three MiB before,
  the partial cut there), else starts again; in a folder, a
  file of its source's length and time (to the second where either
  keeps no more) was finished and stays (`builtin::finished_as`),
  anything else is made again
  (`StepContext::go_on_from`, `going_on_in`). A batch that failed after
  moving `KEEP_FROM` bytes keeps its folder and the folder its tool kept
  a file in (`Aside::partial`, rsync's `--partial-dir`), should its
  steps fail after it (`StepContext::keep_batch`); its retry goes on in
  them (`transfer::keep_finished`, `Layout::made` taking what is there).
  A byte copy
  that may be gone on from never trusts the kernel's first 0, and its
  pace counts from where it went on (`Pace::writing(file, from)`).

## Copies

- Build copies with `Copy::from_settings`, so every path (commands,
  shelf, clipboard, drops) captures the tag-preservation preference in
  the plan. Never read live preferences during apply or redo.
- Copy tags before restoring read-only permissions; publish the staged
  copy only after its metadata succeeded.
- Copies keep the hard links among what they carry (ADR-049). Notes and
  companions: [notes.md](notes.md).
- A copy is whole only when it read as many bytes as its source held
  when it began (`builtin::copy_file`, `transfer::copy_one`): a mount
  whose server stopped answering may answer an end of file, and then say
  the file is that short. A range of a large file read short ends what
  is written in order (`op::ranges`); what follows is read on in order.
- What a move compares of its original before removing it
  (`builtin::state_of`: names, kinds, sizes, change and modification
  times) leaves the change time out where the mount makes it up
  (`Remote::makes_up_change_times`: kio-fuse gives the moment it read the
  attributes, anew after 30 s or told to forget), or every original read
  again would stay as "changed".
- A copy is written once closing it says so (`sys::close`, through
  `builtin::finish`), never dropped: a FUSE program (sshfs, rclone,
  kio-fuse) sends there what it held and tells there what its server
  refused, and `syncfs` is not passed to it. Its time and mode are given
  after, by its name (kio-fuse sends a file up as it is closed, which
  would take a time given before), the time before the mode (Windows
  opens the file to give it). A refusal there is said as one, never as a
  source gone (kio-fuse calls it "no such file"). A volume that keeps no
  modes says so with `ENOTSUP`, or kio-fuse's `ENOTTY` (`sys::mode_kept`).
- **Batches (ADR-078).** Before a copy or a move to another device is
  checked, the run of such steps that follow into the same folder is
  looked at once (`op::transfer::batch_ahead`): where one end is a
  remote and the destination is not written through, the entries of one
  source folder a batch takes are copied at once into a folder of the
  plan's staging folder there, by a plugin's `Transfer` that serves the
  two folders, else by Skwir's own copy eight files at a time, sixteen
  from a remote onto this machine (reading takes no lock of the
  folder's; sshfs serves ten requests at once). One file alone is left
  to its step, unless both folders are on one remote and a transfer
  serves them: it may copy there, where the step would bring the bytes
  here and send them back; or a transfer takes a file alone faster than
  the mount does (`Transfer::serves_alone`, measured: ADR-078, 16). A
  batch takes only what
  its step would copy alike: a name of its own in the batch, no
  companion beside it, a tree of files of one name, folders
  and links, no kept attribute; never a move out of the trash or into
  it. Each step still checks, applies (placing the copy made:
  `StepContext::take_batched`) and gives its inverse; a copy a batch did
  not take or finish is made by its step. A file or link under its own
  name, free in the destination's listing read once before, the batch
  places itself beside what is there (never over anything, partial
  bytes in the staging folder) and verifies: what the transfer says it
  placed (`transfer::Told::placed`), else what has the source's size and
  time, as taken or as it is now (a volume keeping no times, GVfs's
  WebDAV, keeps none of the copy's), looked at eight at a time; its
  step's check and apply only say so (`StepContext::placed_by_batch`). Skwir's own copy makes each in a
  folder of its worker's and links it into place, then removes that name
  (`transfer::place`; renamed where links are refused): the kernel takes
  the destination folder's lock for each name made there, so the
  fewer round trips under it, the better. A move's source is read
  (`builtin::state_of`) before its batch copies it, and its step removes
  the original only if it is still that
  (`StepContext::take_batched_state`). Where a transfer removes in the
  original's folder so that its mount sees it (`Transfer::removes`:
  GVfs's `gio remove`; for sshfs, rsync or sftp, then what sshfs kept of
  each let go of, remote.md), the step leaves the original to the
  plan's end, which looks at all of them again, eight at a time, and has
  those still as copied removed together (`transfer::remove_deferred`):
  through a mount the kernel removes a folder's names one at a time.
  The same way, before a removal is checked (a `Delete`, an `Uncopy`),
  the run of removals from it in a remote's folder is removed at once
  (`transfer::remove_ahead`): those whose checks pass then, answered
  from the folder's names listed once (`StepContext::knowing`), with
  the companions those names show beside them (a copy's undo, those it
  placed); each step then says it removed its own
  (`StepContext::take_removed_ahead`). A
  removal is not taken back: a plan that stops before steps whose
  removal was made ahead counts them done, with their inverses.
  What a transfer writes or removes beside a mount, no watch sees: it
  records the folder (`StepContext::changed_beside`), and at the plan's
  end the executor tells the window (`Executor::set_changed_beside`),
  which reads again the folders its panes show there
  (`App::refresh_showing`), a transfer whose mount keeps what it read
  making it read each anew just before (`Transfer::show`: sshfs), never
  earlier: a look during the plan may have kept what it held then. A step whose source a batch took finds it
  where the batch did (`StepContext::resolve`) and looks for no
  companion beside it (the batch read its folder). A batch's bytes are
  counted once, by the batch. A cancel during a batch ends the plan and
  takes back what it placed; a failed batch is logged and left to the
  steps; a plan
  that ends before the steps of copies its batch placed takes them back
  (`transfer::take_back_unclaimed`): no Undo holds them. A transfer that
  finds it cannot serve after all, having made nothing
  (`transfer::NotServed`: a server without rsync), leaves the batch to
  the next that serves.
- A plan's dry runs before it runs as one action (names taken, every
  check, room) read a remote's folder's names once
  (`StepContext::listing_remotes`) and find its sources ahead, eight at
  a time (`transfer::resolve_ahead`): a look at each would be a round
  trip to the server.
- A mode a volume keeps none of (`ENOTSUP`: GVfs's WebDAV, FTP, SMB and
  MTP through its bridge) is no failure of a copy (`sys::mode_kept`), nor
  of a staging folder's removal, which changes no mode it need not; its
  times are set as they can be. A volume of no size at all says nothing
  of its room (`fs::volume::free`), and room is asked only of a window
  shown: with none, nothing is copied.
- A step's own threads (a batch's, `op::ranges`'s) are given the plan's
  `transfer::Control` (its cancel and its pause) and a progress handle
  (`StepContext::progress`), never the context, which is the plan
  thread's alone. Paused, Skwir's own copy waits between files and a
  tool is held where it is (`process::run_watching`'s `held`: `SIGSTOP`
  to its process group; on Windows, which has no such signal, each
  thread of its job's processes suspended).

## Names taken (ADR-054)

- A copy, a move or a link onto a name taken is settled before anything
  runs (`op::clash::settle`, from `Executor::execute_whole`): as the plan
  says beforehand (`Plan::on_conflict`, the commands' `on_conflict`), else
  as the window answers, every name asked at once
  (`Executor::set_clash_asker`, `ui/src/clashes.rs`), else nothing applies
  and the plan says what is taken.
- The answer is the plan: a step under a new name (`Copy::new_name`,
  `Move::new_name`, `Link::new_name`), a `Trash` before what replaces, a
  merge's steps, an `Rmdir` for each folder a move-merge emptied. No step overwrites: a name
  taken in between still fails its step (`rename_noreplace`, staging).
- Never asked: a copy or a link into its own folder and two items of one
  name keep both (`clash::copy_name`, GNOME's `name (copy).ext`; a link's
  `name (link).ext`, `clash::kept_name`); an item there whole
  (`same_tree`: matching kinds, names, link targets, permissions,
  requested attributes and companions, with regular-file bytes compared
  in bounded, cancellable reads) is skipped by a copy and put in the
  trash by a move. Matching size and time alone never prove unrelated
  files equal; unavailable evidence leaves the normal conflict policy in
  charge. Attribute containers and native attributes are compared through
  the same conversion rules as copies. The metadata-only `finished_as`
  hint is reserved for resuming output owned by an interrupted plan.
  Attribute-name equality uses native bytes, never display text; the legacy
  text-only copy/companion interface refuses names it cannot preserve.
  A link already saying the same path is done; a folder into itself is
  refused. A link, even to a folder, is never merged: it takes a file's
  choices. A move out of the trash keeps a restore's rules.
- **Transfer locations are entries, not path spellings or file identities.**
  Resolve parent folders through dot components and symlink aliases before
  deciding that a transfer meets itself (`builtin::transfer_target`); keep
  the leaf unfollowed, so a symlink stays an entry. Parent identity also
  recognizes bind aliases; distinct hard-link names listed in one folder
  stay distinct. An alternate-case spelling of the same file is self only
  when the directory proves one matching entry; ambiguous hard-link
  spellings are refused. Moving to the same entry is already done; copying
  or linking beside it keeps both. Compare a directory's real location with
  its destination before applying any collision policy: a copy or move
  into its own descendant is refused, even through an alias. Ancestor
  identity detects directory aliases too; on Linux the mount table's
  device and filesystem-root paths detect bind mounts starting below the
  source, outside its visible ancestry. Other systems use canonical paths
  and ancestor identities. The same rule guards batches before they stage
  and steps before they mutate, and dry runs resolve destinations made
  earlier in the plan through their existing ancestors.
- Removing an empty directory captures its permissions and kept user
  attributes before removal (`Rmdir`); a failed metadata read leaves it
  intact. Its serialized inverse, `restore_directory`, restores those
  attributes before restricting permissions, publishing the new directory
  only when complete. On Unix, stage the empty directory itself beside its
  final name: a directory made read-only cannot be renamed between parents
  because updating `..` requires write access. A mount that cannot rename
  between folders restores in place and removes its own still-empty
  directory on failure. Existing directories with different metadata
  conflict rather than being changed.
  Every redo removes through `Rmdir` again and captures a fresh inverse;
  nested merge undo restores source directory metadata without changing
  destination directory metadata. Ordinary `mkdir`/`rmdir` plan formats
  keep their existing meanings; native permission snapshots name their
  system explicitly. Native attribute names remain encoded bytes through
  serialization, and saved names and values count toward the journal's
  memory bound. A foreign-owned Unix folder whose owner bits would prevent
  refilling it is refused before removal: recreating it under this user's
  ownership would otherwise strand nested undo. A partial plan keeps that
  original folder, so earlier child moves can still be undone through its
  existing group or ACL access.
- A step that makes something new under a name (an archive extracted
  or made, ADR-059) says so (`Operation::makes`, `made_as`) and is
  settled as a copy is, with a file's choices: kept beside under a number
  (`photos (2).zip`, `clash::kept_name` with no word), what is there
  replaced through the trash, or skipped. It is never merged. A plan
  with such a step is settled even with no copy in it
  (`clash::takes_names`). What is there is never trashed when it is, or
  holds, what the step reads (`Made::inputs`): the step's check refuses
  instead. Replace if newer compares the latest change among the inputs,
  deep in a folder. The dialog says what comes as what the step makes, a
  folder or a file (of the size an archive lists for its one file; an
  archive being made has none yet), as new as that latest change; never
  what it reads (the archive extracted, the first file compressed).
- What such a step makes may depend on what it reads: an archive of one
  folder or file gives it under its own name, a one-file archive kept
  beside keeps its extension (`report (2).pdf`). `makes` reads it, given
  the plan's context, on the plan's thread; settling fixes the name found
  in the step (`made_as`), so what the dry run checks and the dialog asks
  about is the name it is placed under. `describe` lists nothing: it
  says what a listing already read says, else what the archive's name
  says.
- A dry run foresees what each earlier step takes and makes
  (`StepContext::foresee`): a check asks the context whether a name is
  free (`exists`, `made`, `empty_after`), never the disk alone.
- What only the user knows (an archive's password) is asked by the step
  that needs it, as it runs (`StepContext::ask_secret`, the window's
  `PromptKind::Secret`), and never kept in the plan. With no window, no
  one is asked (`Unanswered::NoOne`) and the step says so; Cancel cancels
  the plan.
- Undoing a trash restores its own item, by its record (`Restore::item`,
  named on Linux and macOS): a replace undone puts the copy in the trash
  and brings back what it replaced. Windows' trash does not say where an
  item went: there the latest from the path comes back
  ([PORTING.md](../PORTING.md)).

## Volumes

- The places plugin lists mounted volumes from mountinfo
  (`fs::volume::mountinfo`, the one reader) and unmounted ones from `lsblk`
  on a worker.
- A volume is named by its filesystem (`places::identity`: UUID from
  udev, the container's for a cleartext volume, plus the subvolume), so
  the user's arrangement holds mounted or not. Volumes udisks hides
  (`UDISKS_IGNORE`: EFI, recovery) are offered hidden; the size guess
  applies only where udev says nothing.
- An unlocked container is listed as its cleartext volume
  (`Volume::container`).
- **Mount and unlock.** A place with `activate` runs it instead of
  navigating: `places.mount`, or `ui.unlock_prompt`, which asks for the
  passphrase in a masked input and dispatches `places.unlock` directly,
  never through `Shell::run` (it traces arguments). Opened in a tab
  (`sidebar::open_in_tab`), the command gets `tab`, and the volume opens
  in a background tab.
- **The passphrase** reaches udisks on standard input; it is never
  stored, logged or put on a command line, and its argument struct has no
  `Debug`.
- **Unmount.** First `App::release` lets go of everything under the mount
  point (panes there go home; kept or opened folders close), since the
  kernel refuses to unmount a busy volume. Then `places::flush` (`syncfs`)
  writes it out while still mounted: the kernel drops the mount from
  mountinfo as the unmount begins, before writing, so the row would look
  unmounted while the drive is written. The row stays busy under the
  device too (`Busy::device`). Unmounting locks the container.
- A mount or unmount ending corrects what the last `lsblk` saw
  (`end_busy`).
- Eject is offered only for what udisks mounted for the user
  (`Mount::udisks`).
- **Removable** (`fs::volume::is_removable`): removable, or on USB,
  FireWire or SD, through LUKS or RAID; drawn as removable wherever
  mounted (`lsblk`'s HOTPLUG when unmounted).
- **Write-through onto removable drives** (`op::pace`): each window of
  bytes is handed to the drive as written, and the copy waits only while
  the drive holds the next (`sys::write_range`). Progress is the drive's
  within half a second (two over FUSE), and the drive never idles. A plan's
  copies onto one volume share one pace.
- Names and folders are written out once, at the plan's end
  (`StepContext::write_out`, `syncfs`): a journaled or copy-on-write
  volume on a hard drive takes a third of a second per commit, so a
  commit is never a unit of progress.
- A move removes originals only once a write-out has their copies on the
  drive (`StepContext::after_write_out`, batched so write-outs stay a
  tenth of the plan's time), and keeps an original that changed after its
  copy began. A move whose original's removal waits (here, or for the
  plan's end after a batch) is undone by an `Unmove`: where the original
  stayed (the same file there; on a mount that makes numbers up,
  anything), the move left a copy, taken back as a copy's undo takes it
  (`CopyUndo`: to the trash, or for good only while the original is whole
  as it was copied); else it moves back.
- On FUSE (ntfs-3g), the write-out itself is the wait, on its own thread,
  a second of writing apart, since each one rewrites the new files'
  records.
- Test volumes with fake `lsblk`/`udisksctl` scripts first on `PATH`,
  never on a real drive.

## The trash

The trash plugin reads ([locations.md](locations.md)); the core acts.

- On a remote ([remote.md](remote.md)): a folder a program mounts across
  a network keeps no trash (`op::keeps_no_trash`, from the mount table),
  so Move to Trash makes a deletion, asked; a kernel share keeps its own
  `.Trash-<uid>`; nothing is ever copied from a remote into this
  machine's trash to be trashed. A `Trash` step there conflicts in its
  check. What would put something in a trash there does without: a
  copy's undo is an `Uncopy` (removed while it is as it was made, its
  state kept as `op::made_state`, with the companions the copy placed
  beside it, `op::MadeBeside`, each as it was placed: never one it did
  not place, and one there it did not place, written since or there
  before, keeps the copy as a change does; Redo copies again), a
  plugin's step that makes something does the same (`op::made_stays`,
  `op::remove_made`: the archives' `archive_unmake`), Replace deletes
  for good (`clash::replaces_for_good`, said by the question), and a
  move whose item is there whole removes its original by an `Uncopy`.
- What a plan reads of the trash (an Undo of a trash, a restore, the
  companions trashed with a file) is listed on Linux by the core itself
  (`op::trash_list`): the home trash and the trashes of this machine's
  own volumes, a network mount's only where a step names a path there,
  never an automount's. The `trash` crate's listing looks at the top of
  every mount, and a mount whose server stopped answering held that look
  for good, SIGKILL included.

- **Restore** names the item (`Restore::item`), merges a folder into one
  already there, and brings back the companions trashed with it.
- A file inside a trashed folder is restored alone (`RestorePart`, undone
  by `ReturnPart`): its folders are recreated as in the trash.
- A trashed file moved out (drop, paste after Cut, Shift+F5, `move`) is
  taken out there the same way (`Move` is a `RestorePart` to its place): original name (`draft.txt.2` is
  `draft.txt` again), `.trashinfo` taken, companions beside it
  (`take_trashed_with`); Undo puts it all back (`Untrashed`).
- Delete removes the `.trashinfo` too, wherever a trashed file is deleted
  from.
- Trash refuses files already in a trash at any depth (`op::in_trash`);
  moves within the trash apply only to a trashed folder's files.
- A trash onto another volume (the home trash for a volume that refuses
  one of its own) copies, then removes the original; failing there, it
  leaves nothing the trash does not list (`builtin::failed_across`):
  while the original is whole the copy goes with its record, once part
  of it went the copy stays, listed (`OriginalStays`).
- `.trashinfo` is parsed one way (`op::parse_trash_info`).
- An item is its record: two names of one file trashed apart are two
  items (ADR-049), and Empty Trash removes every record.
