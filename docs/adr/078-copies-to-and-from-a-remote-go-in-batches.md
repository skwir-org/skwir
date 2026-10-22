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

7. **gio, for GVfs's SFTP; not rclone's own.** Measured the same way
   (below), a tool's own copy beat its mount for GVfs alone: eight
   `gio copy` at once, which GVfs's daemon serves together, where its
   FUSE bridge makes a folder's names one at a time. The remote plugin's
   transfer for an SFTP mount of the session's GVfs runs them, `-i`
   with nothing on its input so that nothing is replaced (without it
   `gio` replaces), folders made through the bridge first and finished
   last (`transfer::Layout`); beside what is there, it copies into the
   staging folder and `gio move -i` places. `rclone copy` was slower
   than eight copies through rclone's mount, and the mount does not see
   what it copied until its folder cache expires: none is built.

8. **Every mount a tool reaches** (2026-10-21, the user's "fix points
   1-7"). rsync reaches an sshfs mount the user made with their own ssh,
   as their configuration says, on the port sshfs was given, asking
   nothing (`ssh_mount`). A server that runs no rsync (an SFTP-only
   account, asked once) takes uploads by eight `sftp` sessions; down,
   Skwir's own copy through the mount was faster. gio serves GVfs's FTP,
   WebDAV and SMB mounts too, measured as SFTP's. A transfer that finds
   it cannot serve, having made nothing, leaves the batch to the next.

9. **What a transfer placed, it says** (`transfer::Told::placed`):
   Skwir's own copy, gio by what left its temporary folder, rsync by its
   `--out-format`. Size and time verify what is not said: a volume that
   keeps no times (WebDAV) left its copies unclaimed, their steps then
   finding their names taken.

10. **Moves' originals together where the mount sees it.** Where a
    transfer removes so that the mount sees the removal (GVfs's daemon
    serves its bridge and `gio remove` alike), a move's step leaves its
    original to the plan's end, which looks at all of them again at once
    and has those unchanged removed together. sshfs keeps what it read
    of the server 20 s and lets go of a path only when something is
    done there through the mount, or the server refuses to open it: its
    originals are removed by rsync (one run from an empty folder, each
    name a deletion asked) or sftp sessions, then each file opened
    through the mount, which the server refuses; a folder is renamed
    through the mount into a folder of Skwir's first (sshfs lets go of
    all below a name renamed), removed there, and that folder goes last
    through the mount, sshfs reading the folder's names again. The opens
    go together and a folder's rename is one round trip, where a removal
    through the mount was a round trip a name, one at a time under the
    kernel's lock of its folder.

11. **No look a round trip each.** A step whose source a batch took
    finds it where the batch did and looks for no companion beside it
    (the batch read its folder). The dry runs before a plan runs as one
    action read a remote's folder's names once and find its sources
    ahead, eight at a time: each step looked at each name across the
    network, 5.35 s of 8.41 for 100 files up through GVfs.

12. **What a tool changed is shown.** No watch of this machine's sees
    what a tool writes or removes beside a mount (a batch placed, an
    original removed): the plan says which folders, and at its end the
    window reads again those its panes show. sshfs reads such a folder
    from the server again once a folder of Skwir's is made and removed
    in it.

13. **Deletions together, and no trash where none is kept**
    (2026-10-21, night). A run of deletions in a remote's folder is
    removed at once, as moves' originals are (10), before its steps are
    checked, those whose checks pass then; a lone folder too. A network
    folder a program mounts keeps no trash: a `Trash` step there now
    says so in its check, where it failed once applied, and Undo of a
    copy, an extraction or an archive made there failed so; it now
    removes what was made while it is as it was made, Redo making it
    again, and Replace there deletes for good, as the question says.

14. **Within one server.** A copy between two folders of one sshfs
    mount is made there by `sftp`'s `cp` (the copy-data extension,
    OpenSSH's server from 9.0, asked once), each file then given its
    source's time through the mount, which `cp` does not keep, once its
    folder was read (sshfs looks up a name it does not know one at a
    time: 2.24 s for 100 names, 0.43 once known). Within a GVfs mount,
    `gio copy` (its daemon copies). A lone file within one remote is
    batched where a transfer serves it. Not rsync: its two ends cannot
    both be remote, and a command run on the server goes through a shell
    whose quoting Skwir cannot know.

15. **NFS and AFP through gio, measured** (2026-10-22, the user's
    "tackle all the points"). Against unfs3 and netatalk run on this
    machine in namespaces of their own, mounted through a GVfs of its
    own, 20 ms, two rounds each, 100 files of 16 KiB: NFS up 23.7–23.8 s
    through the bridge eight at a time, 11.6 by eight `gio`; down 9.2
    sixteen at a time, 3.7; within the mount 23.9–24.0, 10.3; removed
    23.0 one by one, 0.74–0.77. AFP up 15.5, 4.1; within 15.5, 1.0 (its
    server copies); removed 4.3, 0.45–0.50; down 0.98 through the
    bridge, 1.87 by `gio`. A 64 MiB file went up alike either way (NFS
    6.3 and 6.1–6.2 s, AFP 5.85 and 5.83); down from AFP 11.5 s through
    the bridge, 11.3–13.1 read in eight ranges at once (its backend
    serves one read at a time), 5.6 by `gio`. So `gio` serves NFS every
    way and AFP all but copies down. Not measured: a large file down
    from NFS (unfs3 fails to send large replies to libnfs, GVfs's NFS
    client, which then hangs), a phone's MTP.

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
  group); Pause holds it where it is (its process group stopped on Unix,
  the threads of its job's processes suspended on Windows), and Skwir's
  own copy between files. What it made is in the staging folder, removed
  with the plan.
- What a batch did is what its steps say: a step it served is applied
  and undone as before (its copy to the trash); one it did not serve is
  copied as before. Nothing of the batch is kept beyond the plan.
- Measured after 5 to 7 (20 ms, 100 files of 16 KiB): moved one by one
  up by rsync 0.80 s, down 2.75 (the originals removed through the
  mount, a round trip each); without rsync, pasted one by one up 8.4–8.5
  s (15.0–15.4 with one folder aside for all, 10.7 renamed from a folder
  each), moved up 8.5, down 2.9. GVfs (eight `gio`): pasted one by one up
  2.84 s, a folder up 2.78, down 4.26, where through the bridge 13.17,
  10.59 and 4.92; `gio` alone: 1.23 s to copy, 0.63 to place. rclone (a
  shell's timing): through its mount eight at a time 5.9–7.4 s,
  `rclone copy --transfers 8` 9.4 (7.1 without its hash check).
- Measured after 8 to 11 (20 ms, 100 files of 16 KiB, each plan run
  whole as the window runs it): Skwir's sshfs mount by rsync up 0.98 s,
  down 0.76, moved down 2.65; a mount the user made by rsync up 1.42 s
  (8.4 through it), down 0.87; an SFTP-only server up 3.04 s by sftp,
  down 0.85 by Skwir's own copy, moved down 2.89. GVfs, SFTP up 2.97 s
  (8.41 before), down 1.29 (4.09), moved down 2.18 (11.38); WebDAV up
  4.2–5.4 s, down 1.69 (3.69), moved down 2.67 (15.78). Shell timings
  of eight `gio copy` against the bridge eight at a time: WebDAV 1.5 s
  and 25.6–26.1, FTP 9.2–9.4 and 139.8, SMB (Samba) 15.4 and 28.1–28.2.
- Measured after 14 (20 ms; plans built first; two rounds against
  Skwir's own copy through the mount; load 25 to 40): within Skwir's
  sshfs mount, 100 files of 16 KiB 4.17–4.21 s (8.93–9.02), a file of
  64 MiB 1.40–1.79 s (2.60–4.98), the latter whatever its size. GVfs
  within a mount checked, not timed.
- Measured after 13 (20 ms; plans built first; two rounds, load 40 to
  65): 100 files of 16 KiB deleted from Skwir's mount 0.62–0.63 s by
  rsync, 1.12–1.13 by sftp (2.29–2.33 through the mount); a folder of
  100, 0.48 and 1.37 (2.38–2.43).
- Measured after 10 and 12 for sshfs (20 ms, plans built first, run
  whole; load 30 to 40, two rounds each against the removal through the
  mount): 100 files of 16 KiB moved down from Skwir's mount 1.09–1.22 s
  (2.77–3.03), a folder of 100 0.90–0.92 s (2.66–2.72); from an
  SFTP-only server 1.84–1.87 s (2.98–3.00) and 2.00–2.04 s (2.97–2.99).
- Not yet: GVfs's devices (a phone's MTP), not measured, nor a large
  file down from NFS; a lone large file down from AFP goes by its step
  through the bridge, where `gio` took half the time (15);
  rclone's mounts keep their own pace. Without a tool, a name made in a
  folder through FUSE costs a round trip or two under its lock, whatever
  the way. An SFTP-only server's downloads go through sshfs, sixteen
  files at once from 2026-10-21 (eight before): sshfs serves ten
  requests at once, a file two round trips, and no installed tool
  reads faster from such a server (`sftp` sessions get one file after
  another, four round trips each, ten sessions at most on a
  connection). 100 files of 16 KiB from 20 ms away: 0.63–0.67 s (0.72).
