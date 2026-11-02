# ADR-113: How fast an action goes, and how long it has left

Status: accepted 2026-11-02 with the user; built the same day, as
`docs/PLAN.md` ("Speed and time left") records: checked on Linux, against
a throttled rclone server in a window of its own; compiled for Windows
and macOS, not run there.

## Context

- **The user asked** for a copy's or a move's speed, and its time left,
  "especially useful for remote operations that take a long time", done
  properly.
- **A plan's card showed** how far it went: bytes of a one-item plan,
  items for several (`toast::bar`), its bytes known only as each step
  began (`Meter::total` grew with each `add_total`).
- **A user's plan was sized already**: before it began, the room check
  asked every step what it writes (`Operation::writes`, `room::short`),
  walking every tree it copies, and each copy walked its tree again as
  it began (`tree_size`): two walks of a server's folder.
- **Some bytes move none now**: a clone shares blocks (btrfs, APFS); a
  copy going on from where a run cut short counts what the earlier run
  copied.
- **A large file on a FUSE remote** is read as eight ranges at once and
  written in order (`ranges`): it is counted in bursts of 4 MiB.
- **The copy loop asked 8 MiB at a time**: from a server at 2 MB/s, one
  count, and one look at Cancel, every four seconds.
- **rclone's `writes` and `full` caches** (ADR-102, the cache Skwir's
  rclone mounts keep) take a copy at disk speed, then send it: the plan's
  end waits for that (`Transfer::send`), saying the files left.
- **A batch of a folder and a file** (ADR-078) ran its staged and its
  placed copies through one count of what they said: the second run's
  first bytes, under the first's mark, were never counted, unseen while
  nothing compared the count with a total.

## Decision

1. **Measured in the core** (`op::speed`, the plan's `Meter`): what it
   moved, sampled at most every quarter second; its **speed** the bytes
   of the last 5 s, its **time left** what is left over the speed of the
   last 20 s, steadier, so that it counts down rather than jumps. Read
   with its progress (`op::Transfer`'s `planned`, `speed`, `left`), by
   the window and by the `running` command, a script's and an agent's.
2. **Measured from the first byte**, never from the plan's start: the
   wait for a server's first answer, a tree sized, says nothing of how
   fast bytes go. Nothing is said before 2 s of bytes (speed) or 4 s
   (time left), while paused (it starts again after), nor when nothing
   moved for 5 s (a server waited on, a tree walked): no speed falling
   slowly, no time left anyone could believe.
3. **A plan is sized before it begins**: the room check's sizing kept
   step by step (`room::sized`), each step's bytes as it counts them
   (`Operation::work`: what it writes, unless it counts other bytes, an
   archive extracted its archive's, a smaller copy its file's). Its total
   is known from its start, and its bar moves by its bytes, its items
   counted beside. A copy takes the size it was sized at
   (`StepContext::take_walked`) instead of walking its tree again: one
   walk of a server's folder where there were two. An undo, a redo, a
   plan not asked as one action is sized as it begins, its heavy steps
   alone (a rename moves no byte, and is not read for it). A batch that
   counts several steps' bytes takes them out of what waits.
4. **Honest edges.** A clone's bytes, and a copy's gone on from, are
   done, never speed (`advance_unmeasured`; macOS's `clonefile` says it
   cloned). A large remote file's ranges are measured as they arrive,
   done as they are written. The copy asks for about a quarter second's
   worth at a time (`Chunk`: 256 KiB to 8 MiB): its count, its speed and
   Cancel come that often from a slow server or onto a slow drive. A plan
   that moves no byte (50,000 files put in the trash on a server) is
   reckoned by its steps: a time left, no speed.
5. **Into a mount that sends after** (ADR-102), two phases: while the
   plan writes there, its speed, and "then sent to the server" rather
   than a time it could not keep; while the mount sends, the sending's
   speed and time left, by what the mount says it has left
   (`transfer::Left`: rclone's files' sizes less what its `core/stats`
   says it sent of those under way).
6. **Said where the plan is**: under the card's bar, quieter than what
   it is and how far, `23.4 MB/s · about 3 min left`, rounded by steps
   that grow with it (fives of seconds, minutes, tens of minutes past an
   hour). A plan that ran 5 s or more and moved bytes says them in its
   toast: "Copied “film.mkv” to Videos (4.8 GB in 3 min 20 s)", its
   pauses left out (`Finished`'s `moved` and `ran`).
7. **A batch's runs count each from its own start**, and what a run made
   is counted whole once it ran, whatever its transfer said as it went.

## Rejected

- **A walk of its own to size a plan**, beside the copy: the room check
  walks already; a third walk of a server.
- **Measuring from the plan's start**: on the throttled server, 429 KB/s
  said for a copy going at 2 MB/s, the seconds before its first byte
  counted as slowness.
- **Smaller reads of a large remote file's ranges**, for smoother
  counts: each range read whole is what made them 4.4 times faster
  (`ranges`); counting them as they arrive is enough for the speed.
- **A time left while writing into a mount that sends after**: the
  cache takes the bytes at disk speed; the time would be the cache's.

## Consequences

- A user's plan copying from a server walks its folder once, not twice.
- The bar of a plan of several items moves by bytes where it was
  sized, and reaches its total: a batch's lost bytes are counted.
- Each step that counts bytes it does not write says them (`work`), or
  its plan's time left is told only once it begins.
- The copy loop's calls are smaller from a slow server (256 KiB at
  least): more calls, each answering Cancel.
