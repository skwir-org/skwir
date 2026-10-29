# ADR-104: A copy across a network is found whole by length and time

Status: accepted and built 2026-10-29 on Linux (checked by the operations
tests, a remote stood in by `fs::reach::pretend_remote`). Amends the rule
of operations.md that matching size and time never prove files equal.

## Context

- **Merge proves what it skips.** A folder pasted onto one of its name,
  merged, leaves alone what is there whole (`builtin::same_tree`): kinds,
  names, link targets, permissions, attributes, companions, and each
  regular file's bytes read on both sides and compared. A copy then skips
  it; a move puts the original in the trash. The same proof makes a plan
  applied again do nothing (ADR-000's third principle).
- **Across a network the proof is the copy, backwards.** Refreshing a
  backup folder on a NAS or a cloud mount read every unchanged file back
  from the server to avoid sending it: 50 GB read over gigabit is about
  seven minutes, where comparing length and time takes seconds. The
  review of 2026-10-29 (ops#4) found it while making each byte read once
  per settling, which that fix does not remove.
- **What others do.** rsync and rclone decide by length and modification
  time unless asked for checksums; Nautilus and Dolphin skip nothing and
  ask. Length and time agree while the bytes differ when a program edits
  a file and puts its old time back, when two edits fall in one second on
  a volume that keeps whole seconds (SFTP, FAT), or when a copy was
  damaged where it lies.

## Decision

1. **A copy from or to a remote** (`fs::reach` names either side) is found
   whole by kind, permissions, attributes, companions, length and time
   (`finished_as`), its bytes unread. Its verdict only leaves a copy
   undone; it removes nothing.
2. **A move proves the bytes**, wherever it goes: its verdict puts the
   original in the trash, or deletes it where no trash is kept.
3. **A copy between this machine's folders proves the bytes**: reading is
   cheap there, and it catches a copy damaged on a drive.
4. **One rule, one place**: `builtin::proves_bytes(src, copy, moving)`.
   Verdicts made with and without the bytes are kept apart in `Compared`,
   so a copy's is never taken for a move's.

## Consequences

- A merge onto or from a server settles in a walk of each side; a copy
  across a network whose source changed without its length or time is
  left as it was there, as rsync leaves it.
- A move, and a copy on this machine, behave as before.
- What changes: `builtin::same_tree` takes `moving`; clashes pass
  `transfer.moving()`; operations.md's clashes paragraph.
- Checked by `only_a_remote_copy_trusts_length_and_time`: a remote copy
  of files of one length and time but other bytes reads none and finds
  it whole; a move to the same remote, and a local copy, read it and find
  it different.
