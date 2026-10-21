# ADR-078: Copies to and from a remote go in batches, by an installed tool where one serves

Status: accepted, 2026-10-21, the user's choice ("go ahead with 1-4")
among the levers measured below. Extends ADR-077: Skwir still speaks no
protocol; an installed tool may now move the bytes of a copy as one
mounted the folder.

## Context

- Measured 2026-10-21 against OpenSSH's own server run on this machine,
  through sshfs mounted as the remote plugin mounts it, a round trip of
  20 ms set on the way (`tc netem` in a user namespace), Skwir's copy run
  by its executor:
  - a 256 MiB file up: 3.6 s (`cp` 2.9, `scp` 3.6); down: 23.9 s
    (11 MiB/s) where `scp` took 3.5: the kernel reads a FUSE mount ahead
    128 KiB at a time, a round trip each;
  - 100 files of 16 KiB up: 16.6 s; down: 4.6 s; `rsync` 0.7 and 0.6,
    its login included. Each file costs about eight round trips through
    FUSE and SFTP (made, written, its mode, its time, closed): as many
    for `cp -rp` (16.0 s), so Skwir's copy wastes nothing, the way does;
  - 8 files copied at once into one folder: half the time (18.4 s to
    8.9); more SFTP connections (`max_conns`) nothing more; the kernel
    takes a folder's lock for each name made in it.
- The user chose, of the levers: (1) a large file on a remote read in
  ranges at once; (2) several files copied at once; (3) no mode set again
  where the file was made with it; (4) the installed tool moving the
  bytes. (1) and (3) are inside a file's copy (`op::ranges`,
  `builtin::copy_file`).
- A plan's steps run one after another on its thread, each checked,
  applied and given an inverse; its context is a step's at a time
  (`RefCell`s). Copies stage in one folder per destination the plan
  shares (`StepContext::staging_for`) and are renamed into place whole.

## Decision

1. **A batch.** Before the executor checks a copy (`Copy`, keeping
   attributes), it looks at the run of copies that follow it into the
   same folder. Where the destination or the source is on a remote
   (`fs::reach`) and not on a drive written through (`pace`), the copies
   of that run whose sources share a folder are made at once, in a folder
   of the plan's staging folder there (`op::transfer::batch_ahead`). Each
   step then runs as before, check, apply and inverse, its apply placing
   the copy the batch made (the no-replace rename it makes anyway) rather
   than making it. A copy the batch did not take, or did not finish, is
   made by its step, as any other.
   A file or a link copied under its own name, which the destination's
   listing (read once, before) shows free, the batch places itself:
   never over anything, its bytes written in the staging folder until
   whole, then verified (the name was free; what is there has the
   source's size and time). Its step only says so and gives its inverse:
   placed by its step, each would cost a look and a rename across the
   network, as many round trips as the whole batch. A batch cancelled
   takes back what it placed.

2. **What a batch takes.** An entry of the folder whose name no other of
   the batch has, with no companion beside it (by the folder's names,
   `Companions::owner`), and a tree holding only files, folders and
   links: no file of several names (ADR-049 links them), no special file,
   no name a companion rule claims, no `user.*` attribute (a volume that
   refuses them gets them as companions, file by file). Told by walking
   the tree once, its names and stats, which the progress's total reads
   too.

3. **Who copies it.** A plugin's `Transfer` that serves the pair of
   folders (`App::transfers`, owned and withdrawn as any contribution),
   else Skwir's own: eight files at a time, each made, written, given
   its mode where it differs and its time, folders given theirs once
   full. A batch's bytes are counted as the transfer says them.

4. **rsync, for sshfs.** The remote plugin's transfer serves a mount it
   made with sshfs, to or from this machine: `rsync -rlpt` through ssh,
   multiplexed on the connection sshfs holds (sshfs's ssh is started as
   the master of a control socket in the runtime folder,
   `ControlMaster=auto`), so nothing is asked again and no credential
   passes through Skwir. What it places beside, `--ignore-existing` with
   `--temp-dir` in the staging folder. rsync must be installed here and
   on the server; a server without it is remembered for the mount and
   its copies go through the mount. Its progress is read from
   `--info=progress2`.

5. **Moves and the pause.** A move to another device (one end a remote,
   or two remotes) is a copy, then the original removed: a batch takes
   it as a copy, after reading what its source is, and its step removes
   the original only if it is still that, as an unbatched move does. A
   batch heeds the plan's pause: Skwir's own copy waits between files; a
   tool's process group is stopped (`SIGSTOP`) and continued on resume.
   A plan cut short before the steps whose copies its batch placed takes
   those back: no Undo would hold them.

6. **Placing beside, through FUSE.** Skwir's own copy makes each file in
   a folder of its worker's and links it into place, then removes that
   name (a no-replace rename where links are refused): the kernel takes
   the destination folder's lock for each name made there, and sshfs
   refuses the no-replace flag, so a rename cost about four round trips
   under that lock, a link about three.

## Consequences

- Measured after, the same way (20 ms; load 8 to 61): a 256 MiB file
  down 6.4–6.6 s (23.9 before); 100 files of 16 KiB pasted one by one up
  0.87 s, as one folder 0.81–0.91 s (16.6), down 0.26–0.45 s (4.6), by
  rsync; without it, up 7.3 s as one folder and 15.2 one by one, down
  0.58–0.75 s.
- A paste of many small files onto or from an SFTP server goes at
  rsync's pace where rsync is on both ends, at twice the mount's pace
  elsewhere; a large file comes down in ranges.
- A batch is cancelled as a whole: Cancel stops the tool (its process
  group); Pause holds it where it is (on Unix; on Windows a tool runs
  on), and Skwir's own copy between files. What it made is in the
  staging folder, removed with the plan.
- What a batch did is what its steps say: a step it served is applied
  and undone as before (its copy to the trash); one it did not serve is
  copied as before. Nothing of the batch is kept beyond the plan.
- Measured after 5 and 6 (20 ms, 100 files of 16 KiB): moved one by
  one up by rsync 0.80 s, down 2.75 (the originals removed through the
  mount, a round trip each); without rsync, pasted one by one up 8.4–8.5
  s (15.0–15.4 with one folder aside for all, 10.7 renamed from a folder
  each), moved up 8.5, down 2.9.
- Not yet: rclone's and GVfs's own transfers (`rclone copy`, `gio
  copy`); sshfs mounts Skwir did not make (no connection to share:
  their copies go through the mount, eight at a time); a move's
  originals removed on the server one by one through the mount (`rsync
  --remove-source-files` could remove them in its stream). Without
  rsync, a name made in a folder through FUSE costs a round trip or two
  under its lock, whatever the way.
