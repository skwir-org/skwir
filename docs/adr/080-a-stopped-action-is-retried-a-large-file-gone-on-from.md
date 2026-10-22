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
   staging folder (a file alone, not one of a folder copied whole or a
   tool's batch; a file with holes too, its stretches written in order,
   its holes read back as the source's), having moved at least 8 MiB
   (`KEEP_FROM`: below, starting again costs little), what it wrote
   stays: the staging folder keeps its lock held and its notes, so no
   sweep takes it, and goes with the stopped plan, removed once the
   retry places the file or the plan is let go. A process that ends
   meanwhile leaves it to the sweeps, as any staging folder.
5. **Gone on from, checked.** The retry's copy of that source into that
   folder goes on from what the partial holds, asked of where it is
   when the retry runs, when the source has the length and time it had,
   and the partial's last MiB (`CHECKED_TAIL`) reads as the source's at
   the same place; else the MiB before it, four back at most (a mount
   cut short may leave the last bytes it was asked for zeroed), the
   partial cut there. Otherwise it starts again.
6. **A folder cut short** (2026-10-22, the user's "tackle the not done
   points"). A folder copied whole by its step, cut short partway
   through one of its files, having moved 8 MiB, is kept whole as a
   large file is (4), told by the file it was writing, never by looking
   at a server that may not answer. The retry's copy goes on in it: a
   file of its source's length and time (to the second where either
   keeps no more, as SFTP's; given last, after its bytes and attributes;
   exactly elsewhere, so two files of one length changed in one second
   are told apart) was finished and
   stays; the file it was writing goes on as a large file does (5);
   anything else there is made again; its folders are taken as they
   are, opened to their owner where their mode was given already.
7. **A batch cut short.** A batch of copies (ADR-078) that fails after
   moving 8 MiB, its steps failing after it (its server gone), keeps the
   folder it copied into with the staging folder, and rsync keeps the
   file it was writing in a folder of the staging folder
   (`--partial-dir`, absolute, on the receiving side; rsync keeps it on
   a lost connection or a hangup, not when killed outright). The
   retry's batch into that folder goes on in it: what it finished
   stays, as in 6 (`transfer::keep_finished`); rsync passes over what
   is finished (its own quick check, the same length and time) and
   takes the file it kept as the basis of its delta copy, whose
   checksums say what is the same, the file made checked whole; sftp
   and `gio` make the file they were writing again: `sftp`'s `put -a`
   appends to what is there without reading it, and `gio` has no way
   on. A copy's check calls a target already done when it is its
   source to the second where either keeps no more (`same_tree`),
   as a batch placed it on a server keeping seconds.
   (2026-10-23, the user's night list) Skwir's own batch and sftp's go on
   from the file they were writing too, as a large file does (5): each
   file's source is noted as the batch lays it out (`transfer::Layout`,
   kept with the batch), and the retry keeps a file cut short whose
   source is still so, as far as its last part reads as the source's,
   cut there (`transfer::keep_finished`). Skwir's own copy appends to it;
   sftp's by `reput` (`put -a`), what it appends to read back first. A
   transfer that makes each file anew (`gio`, `kioclient`:
   `Transfer::goes_on`) finds none: they are removed before it copies.
8. **Not Windows' restartable copy.** Windows' own copy, which keeps
   what a byte copy loses there, can track its progress in the file it
   makes and start again from it (`COPY_FILE_RESTARTABLE`), but flushes
   that file as it goes, slower for every copy, and cannot be measured
   here nor tried under Wine: a copy cut short there starts again.

## Consequences

- An upload or a download a server cut short goes on from what got
  across: Retry once the server answers again.
- A server keeps, until Retry or until the plan is let go, at most
  eight files cut short, each in a hidden `.skwir-copy-…` folder of
  this machine's, as a copy under way does.
- What is read back is a MiB at its end: a server that wrote out of
  order elsewhere in the file would go unseen. OpenSSH's sftp server
  writes in the order asked.
- A copy that read fewer bytes than its source had when it began fails
  (Retry goes on, or starts again on a source changed): a mount whose
  server stopped answering may answer an end of file, and then say the
  file is that short; a byte copy and a batch's took that end as the
  file's, and placed a copy cut short as whole (found 2026-10-22, once
  in eight runs of the test). A range read short ends what is written
  in order (`op::ranges`): the ranges after it would land too soon.
- A folder copied whole, cut short, keeps what it finished, and its file
  under way goes on (6); a batch cut short too, its file under way gone
  on by rsync, Skwir's own batch and sftp, made again by `gio` (7). A
  batch's folder is kept only when its steps fail after it: where they
  copy what it left (the batch's tool failed alone), the plan ends as
  before.
- Not gone on from: a file a batch by `gio` or `kioclient` was writing,
  or one placed beside what is there (made in a folder of its own each
  run); a file
  alone that `gio` takes (ADR-078, 16); a copy by Windows' system copy
  (8); a folder whose tool kept nothing, killed outright. No retry happens by itself when a server is back:
  the user, or an agent, asks.

## Built

2026-10-22 (STATUS-LOG): Retry in the executor, the toast and the
`retry` command; the partial kept and gone on from in Skwir's own copy.
Checked against OpenSSH's server on this machine, stopped mid-file both
ways. Later that day (6 to 8): a file with holes too; a folder cut
short; a batch cut short, by rsync against that server both ways.
