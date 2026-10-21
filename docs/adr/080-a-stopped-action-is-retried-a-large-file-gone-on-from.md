# ADR-080: A stopped action is retried, a large file gone on from

Status: accepted, 2026-10-22, the user's choice: two of the points
listed with ADR-079 and confirmed with it ("Tackle all the points"):
"Retry on a stopped action: re-runs the same action; finished steps
count as already done, and it stays one entry in the history", and
"Resuming a large file: after a lost connection, Retry continues from
the partial copy, after re-checking its last part". Builds on ADR-078
(staging on a remote, its notes) and ADR-079 (the history in the order
asked).

## Context

- A plan stops at the step whose error ends it: a server that stopped
  answering (sshfs fails what waits on it once ssh's keep-alives give
  up, about 45 s; at once when the connection closes), a drive pulled
  out, a disk full. What it applied stays applied, an entry in the
  history; the window says "…, then stopped: <reason>" with Undo.
- Asking for the action again by hand ran it whole: what it had done
  counts as already done (a target of the same size and time, the same
  tree), but the file it was in the middle of started over, and its
  staging folder went with the plan (on a server gone, noted and swept
  once reached again, ADR-078).
- Byte by byte, Skwir's own copy writes a file in order (a remote's
  ranges too, `op::ranges`), so what a copy cut short holds is the start
  of its source; but a server may keep less than was sent (sshfs sends
  writes ahead and hears of a failure later).

## Decision

1. **Retry.** A plan a step's error stopped keeps the steps it had
   left, from the one that failed, as they were unfolded and settled
   (names found taken stay answered): the toast's Retry, the `retry`
   command (the last plan stopped unless told which, for an agent or a
   script as for the window), `Executor::retry`. The eight latest are
   kept; older ones are let go. A plan cancelled, or whose end failed
   (a drive's write-out), is not retried.
2. **Asked now.** A retry is an action asked when Retry is pressed: its
   footprint, its turn and its whole check as any (ADR-079), so what
   changed since is seen; a step done meanwhile counts as done. Refused
   before it applies anything (the server still not answering fails its
   check), it stays to be retried.
3. **One action.** What a retry applies joins the stopped plan's entry
   in the history, taken back first: Undo takes the whole action back.
   Unless an action asked since put an entry after it, which may have
   touched what the retry then did: the retry's entry is then its own,
   in its place in the order asked, so undoing in that order still
   puts back each state.
4. **A large file cut short is kept.** When a step fails partway
   through a file it copies byte by byte from its start into its
   staging folder (a file alone, not one of a folder copied whole, a
   tool's batch, or a file with holes), having moved at least 8 MiB
   (`KEEP_FROM`: below, starting again costs little), what it wrote
   stays: the staging folder keeps its lock held and its notes, so no
   sweep takes it, and goes with the stopped plan, removed once the
   retry places the file or the plan is let go. A process that ends
   meanwhile leaves it to the sweeps, as any staging folder.
5. **Gone on from, checked.** The retry's copy of that source into that
   folder goes on from what the partial holds, asked of where it is
   when the retry runs, when the source has the length and time it had,
   and the partial's last MiB (`CHECKED_TAIL`) reads as the source's at
   the same place. Otherwise it starts again.

## Consequences

- An upload or a download a server cut short goes on from what got
  across: Retry once the server answers again.
- A server keeps, until Retry or until the plan is let go, at most
  eight files cut short, each in a hidden `.skwir-copy-…` folder of
  this machine's, as a copy under way does.
- What is read back is the last MiB alone: a server that wrote out of
  order elsewhere in the file would go unseen. OpenSSH's sftp server
  writes in the order asked.
- Not gone on from: a file of a folder copied whole (its tree is staged
  together), a file a tool's batch was making (rsync, sftp, gio: it
  starts again), a file with holes, Windows' system copy (it keeps no
  partial of ours). No retry happens by itself when a server is back:
  the user, or an agent, asks.

## Built

2026-10-22 (STATUS-LOG): Retry in the executor, the toast and the
`retry` command; the partial kept and gone on from in Skwir's own copy.
Checked against OpenSSH's server on this machine, stopped mid-file both
ways.
