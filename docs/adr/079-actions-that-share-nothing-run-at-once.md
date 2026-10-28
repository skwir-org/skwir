# ADR-079: Actions that share nothing run at once, one heavy action a disk

Status: accepted, 2026-10-21, the user's choice ("Yes, I confirm 1-5":
Undo of an action still running waits for it; four actions at most; a
solid-state disk no exception; all local volumes one channel where the
disks cannot be told; more than 1 000 steps heavy), after asking for it
("we need to be smart about this though: hammering a local disk with
different operations will slow everything down"). Amends the executor's
one lane (`op::executor`) and operations.md's "Running".

## Context

- Every plan, undo and redo runs on one first-in-first-out lane, so the
  undo journal's order is the order things happened on disk. A rename
  asked during a 20-minute copy to a USB drive waits the 20 minutes; a
  download from a server waits for a copy between two local disks. The
  bar says "1 action waiting", nothing more.
- A plan is checked when its turn comes, not when asked (a user's
  action as a whole: unfolded, names taken settled, every step dry-run,
  room): what an earlier plan did is seen. Its entries are named by
  `EntryRef`: a file renamed in its folder is followed, one moved
  elsewhere or replaced is not (the step conflicts; a delete of what is
  gone is already done).
- The journal is two stacks of inverses, pushed when a plan ends;
  Undo pops the top when its turn comes on the lane.
- Within one plan, a batch already uses what a device does at once
  (ADR-078: eight to sixteen files, eight `gio`, eight `sftp`
  sessions). A second bulk copy on the same disk shares its bandwidth
  without ending sooner; on a rotating disk the two streams make it
  seek between them and both end later.
- The core finds a volume's disks without reading the volume
  (`fs::volume`: `/sys/dev/block`, a partition's disk, a mapped or RAID
  device's members, btrfs by the device mounted), and a remote by its
  mount (`fs::reach`, its lane).

## Decision

### What an action touches

1. **A footprint.** Each step says what it touches, as claims on paths
   and on shared things, derived for the core's own steps from what
   they are (`Change`, the resolved `EntryRef`, the destination):
   - `read(T)`: the tree at `T` read (a copy's source);
   - `write(T)`: the tree at `T` changed, made or removed (a copy's
     target, a move's source and target, a deletion, a tag edit's file);
   - `names(F)`: a name made, removed or renamed in the folder `F` (any
     step that makes or removes an entry claims its parent; a step
     claims its companions' places as it claims its file's,
     `Footprint::made_with`; amended 2026-10-24: "they live beside their
     file, so they are covered" missed a write in place and a companions'
     folder);
   - `store(S)`: something several actions keep: a volume's trash (its
     `files` and `info`), the tags index, a plugin's store. Amended
     2026-10-28: a claim is on the store whole (it reads, takes or
     empties what it holds: a restore, an item deleted for good) or only
     on a record of its own there (`Footprint::record_in`: a file
     trashed, its record made new; a notes edit, its file's record,
     which its paths name). Two claims meet on a store when one is on
     it whole; two records of their own meet only where their paths
     do. Two trashings conflicted before, so a toast's Undo of one was
     refused after the other.

   Two claims conflict when a `write(A)` meets any claim on a path
   within `A` or above it, a `names(F)` meets a `read` or `write` of a
   tree holding `F` or another `names(F)`, or two claims name the same
   store. Reads never conflict with reads. Paths are compared by their
   components, a folder made canonical on this machine's volumes (a
   link to a folder is the folder), as named on a remote (no round
   trip).
2. **A step that cannot say** (a plugin's step without a footprint, a
   step from a plugin outside Skwir, ADR-045) conflicts with every
   plan: its plan starts alone, after every plan asked before it, and
   every plan asked after it waits for it. That is the lane of today.
   A plugin's built-in steps (an archive extracted, a picture's
   position written) say their footprint.

### Heavy and light

3. **A heavy action** moves bytes or walks a tree: a copy, a move to
   another volume, an archive extracted or made, a deletion of a folder
   for good, a change of modes inside a folder, a trash emptied; or any
   plan of more than 1 000 steps (renaming ten thousand photos writes a
   disk as a copy does). Every other action is light: a rename, a new
   folder or file, a link, a move or a trash within one volume (a
   rename), a restore within one, a tag, a rating, a comment, a mode of
   a few files.
4. **Channels.** A heavy action uses the channel of each place its
   steps read or write bytes in: on this machine a physical disk (each
   disk under a mapped, encrypted or RAID volume; one disk for all its
   partitions); a remote, by its mount (its lane). Where the disks
   behind a volume cannot be told (macOS and Windows until their own
   lookup is written), all of this machine's volumes are one channel:
   never two heavy local actions at once there, as today. A solid-state
   disk is no exception: its own action's batch already keeps it busy,
   and no measurement on this machine decides for others.

### When an action starts

5. Actions start in the order they were asked, except that one may
   start before an earlier one that waits when:
   - it conflicts with no action asked before it that has not ended
     (running or waiting);
   - heavy, none of its channels is used by a heavy action running, and
     no heavy action asked before it waits for one of them (on a
     channel, heavy actions keep their order: none waits for ever);
   - fewer than four actions run.

   Its footprint is worked out on a thread of its own once asked
   (resolving its entries reads: never on the window's thread), and
   again when it is about to start, since what ran before may have
   moved what it names. It is then checked whole, as today, against
   what the earlier actions left.
6. **Pause** keeps an action's claims and channels: what waits behind
   it says it waits for a paused action, so the user resumes or
   cancels it. Cancel and the end free them.
7. **Questions** an action asks (names taken, room, a password) are
   asked one at a time, in the order the actions were asked; an action
   waiting for its answer has applied nothing and holds its claims.

### The undo history

8. **In the order asked.** An action's entry takes its place in the
   history when it is asked and holds its inverse once it ends (what
   applied, cut short too). Two actions that ran at once touched
   nothing in common (5), so their effects, and their inverses, give the
   same result in either order; two that conflict ran in the order
   asked. So undoing in the reverse of the order asked puts back every
   state, and Undo always takes back the last thing the user asked for.
9. **Undo takes the latest entry**, ended or not. Ended, its inverse
   is an action like any (5): it waits only for an earlier action it
   conflicts with, so undoing a rename does not wait for a copy
   elsewhere. Still running, Undo waits for it to end, then takes it
   back, as today; its bar says so, and Cancel stays the way to stop
   it at once. Two Undos take the two latest entries, and run at once
   when those share nothing.

   A toast's Undo takes back the action it reports, not the latest
   (amended 2026-10-24, the user's choice): when no action asked after
   it, still in the history, running or waiting, touched anything it
   touched (their footprints, kept with each entry), it is taken back
   alone: the two share nothing (5), so their inverses give the same
   result in either order (8). Otherwise nothing is taken back, and
   Undo says which action touched it, to be undone first. An action
   that has not ended is waited for, as Undo does. The footer's Undo
   still takes the latest.
10. **Redo** offers what was undone since the last action asked: an
    action asked after an Undo drops that undo's redo, even when the
    undo ends after it.
11. The history's bound (`JOURNAL_ENTRIES`, `JOURNAL_BYTES`) is
    unchanged.

### What the window shows

12. One card for each running action (four at most), each with its own
    bar, Pause and Cancel; under them, the actions waiting, each saying
    what it waits for ("waits for the copy to “Backup”", "waits for the
    disk"). Quit names every action running and waiting. Undo names the
    latest entry, "(once it ends)" while it runs.

## Which actions run at once

| Running | Asked meanwhile | At once? | Why |
| --- | --- | --- | --- |
| a copy from the SSD to a USB drive | a rename on the SSD, elsewhere | yes | light, nothing shared |
| the same copy | a rename in the folder being copied | no | `names` within the tree read |
| the same copy | a copy from an SD card to a server | yes | heavy, channels {SD card, server} and {SSD, USB drive} apart |
| the same copy | a copy between two SSD folders | no, waits | both heavy on the SSD |
| an upload to server A | a download from server A | no, waits | same channel |
| an upload to server A | an upload to server B | yes | channels apart |
| a folder put in the trash (same volume) | a copy into another folder of that volume | yes | the trash is a rename, light; nothing shared |
| the trash emptied | a file put in that volume's trash | no | same store |
| tags written on 500 photos | a copy of those photos | no | the copy reads what is written |
| a copy to a USB drive | Undo of a rename elsewhere | yes | light, nothing shared |
| a step of a plugin from outside | anything | no | its footprint cannot be told |
| two light actions in different folders | | yes | light, nothing shared (four at most) |

## Consequences

- A quick action no longer waits behind a long one it shares nothing
  with, and two long ones on different disks or servers run together;
  two long ones on one disk still take turns, which is faster than
  sharing it.
- The history reads in the order actions were asked; "Undo" is always
  the last thing asked for. Undoing an action never needs another one
  undone first unless they touched something in common, and then the
  order asked is the order they ran.
- An action's footprint is a new duty of every step: a missing claim
  could let two actions meet. A step that cannot say runs alone, and
  tests check each built-in step's claims against what it touches.
- More than one action may be applying at once: four threads at most,
  each with its own context; the executor's state (what runs, what
  waits, the history) is shared under one lock, never held across a
  step.
- Checked, before it is built further: two copies on one rotating disk
  (the USB disk set aside for timings), at once and one after the
  other; a rename during a long copy, how long it waits before and
  after; two copies on two channels, together and one after the other.

## Built in two steps

1. Footprints, conflicts, the history in the order asked, and light
   actions starting beside a heavy one; heavy actions one at a time as
   today. **Built** 2026-10-21 (STATUS-LOG): the core's steps, the notes
   edits and the archives' say their footprints; other plugins' steps
   run alone. Footprints are worked out on a thread of their own, never
   the window's. Deviation: questions from plans running at once are
   asked one at a time in the order they come, not the order the plans
   were asked. What waits was a count on the first bar until 2026-10-22,
   when the actions waiting were listed under the cards, each with what
   it waits for (12), as decided.
2. Channels: heavy actions on different disks and servers at once.
   **Built** 2026-10-22 (STATUS-LOG). On Linux a volume's disks come
   from sysfs (its device, or for btrfs the device mounted); a volume
   kept in memory (tmpfs) is a channel of its own; one whose disks
   cannot be told (ZFS, and every volume on macOS and Windows) meets
   every disk. Measured on the USB disk set aside for timings: two
   copies reading it end sooner in turns than at once (8.70–9.24 s
   against 13.47–14.20 s), two writing it about even; two copies on two
   channels at once end about as soon as the longer alone.
