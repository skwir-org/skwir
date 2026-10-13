# Operation invariants: plans, copies, volumes, the trash

Read this before changing an operation (`skwir_core::op`), the executor,
copies, volumes or the trash's operations. Binding, like `AGENTS.md`.

## Plans and steps

- Commands that change files return a `Plan`; the caller applies it. A
  user's single action applies at once, undoable; batches and anything
  from a background task or an agent go through `App::propose`.
- Every operation has an inverse, a dry-run `check`, and is idempotent.
- Steps reference entries by `EntryRef` (`AGENTS.md`, "Files and
  entries").
- A step that changes metadata states its words (`op::Words`: what it
  will do, for the review; what it did, for the toast). What a plan left
  out goes in its note (`Plan::with_note`), never in its title.
- A step Undo cannot reverse (`Operation::undoable`: deleting for good)
  makes its plan ask before any gesture applies it
  (`Shell::apply_user_plan`).
- **Room.** A plan the user asked for as one action
  (`Executor::execute_whole`) compares what it will write
  (`Operation::writes`: copies, cross-filesystem moves, a multi-name file
  once) with each volume's free space (`op::room`) and asks before
  starting when one is short. It never refuses unasked (network shares
  may under-report).

- **Access (ADR-058).** `set_access` changes a mode as chmod takes it
  (`op::access::Mode`: octal, a folder's set-ID bits kept unless five
  digits say otherwise; symbolic clauses with `X`, `s`, `t` and copies),
  and a group by number, never the owner. Its step changes what a link
  leads to, and with `inside` what a folder holds: no link inside is
  followed, no other drive mounted inside is reached. It reads the mode
  back and fails, putting back what it changed, when the drive kept
  nothing; a failure partway puts back the same way. Its inverse
  (`access_back`) holds each changed item's mode and group, which the
  journal counts (`Effect::held`). `check` refuses what the user does
  not own (root aside), a group they are not in, a read-only drive and
  the trash's files, in words.

## Running

- Plans run on a thread of their own, never a worker.
- A failed recursive removal may already have removed writable entries.
  Its error names and counts what actually remains; tests compare that
  report with the surviving tree, never assume a directory traversal order.
- A plan of several steps shows a bar with Pause and Cancel
  (`Executor::pause`, `Executor::cancel`). Pausing waits where Cancel
  would be answered (`StepContext::check_cancelled`), so long steps call
  it between units of work. User actions meanwhile wait behind the plan,
  and the bar says so.
- The bar counts items, the current item's bytes moving it between two
  counts; a one-item plan counts bytes (`toast::bar`).
- **Quit** asks while a plan runs (its key or the window's close button,
  `Shell::quit`), then cancels and waits, out of sight, for partial work
  to be taken back (`Executor::wind_down`): a copy cut short leaves
  nothing in its destination.

## Staging

- A step that makes something in a destination stages it
  (`StepContext::stage`, then `Staging::place`): copies, extracted
  archives.
- A copy holds a lock on its staging folder while it runs. Unlocked
  staging folders are leftovers of a crash or kill: swept by a later copy
  into that destination, and at start for the destinations an earlier run
  was copying into (`op::staging`). Only where this machine alone writes.

## Copies

- Build copies with `Copy::from_settings`, so every path (commands,
  shelf, clipboard, drops) captures the tag-preservation preference in
  the plan. Never read live preferences during apply or redo.
- Copy tags before restoring read-only permissions; publish the staged
  copy only after its metadata succeeded.
- Copies keep the hard links among what they carry (ADR-049). Notes and
  companions: [notes.md](notes.md).

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
  (`same_tree`) is skipped by a copy and put in the trash by a move, a
  link already saying the same path is done; a folder into itself is
  refused. A link, even to a folder, is never merged: it takes a file's
  choices. A move out of the trash keeps a restore's rules.
- A dry run foresees what each earlier step takes and makes
  (`StepContext::foresee`): a check asks the context whether a name is
  free (`exists`, `made`, `empty_after`), never the disk alone.
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
  copy began.
- On FUSE (ntfs-3g), the write-out itself is the wait, on its own thread,
  a second of writing apart, since each one rewrites the new files'
  records.
- Test volumes with fake `lsblk`/`udisksctl` scripts first on `PATH`,
  never on a real drive.

## The trash

The trash plugin reads ([locations.md](locations.md)); the core acts.

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
- `.trashinfo` is parsed one way (`op::parse_trash_info`).
- An item is its record: two names of one file trashed apart are two
  items (ADR-049), and Empty Trash removes every record.
