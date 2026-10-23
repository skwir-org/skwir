# ADR-077: A remote folder is a mount Skwir knows is remote

Status: accepted, 2026-10-21, the user's choice among the alternatives
below. Built and checked on Linux the same night (sshfs, GVfs and rclone
against servers on this machine, and a window on a display of its own);
fstab's connector tested by its parser only; macOS and Windows compiled.
Revises ADR-021: its direction kept, its steps decided.

## Context

- The user, 2026-10-20: "remote file systems / network protocols next. If
  we have a good architecture that can adapt to a wide range of remotes
  (SFTP, SMB, NFS, GVfs, rclone...), adding new protocols after shouldn't
  be too difficult."
- ADR-021 set a direction (installed tools make mounts; Skwir speaks no
  protocol and holds no credentials; rclone's API later) and nothing was
  built. On 2026-10-20 the code had no notion of a remote:
  - the mount table was read in four places, each its own way; the
    places plugin listed `fuse*` under `/mnt`, `/media` and `/run/media`
    only, never NFS, CIFS or GVfs; `sftp://` bookmarks were kept in the
    file and never shown;
  - one worker pool did every read: a dead server blocks `stat`,
    `statvfs` and `inotify_add_watch` uninterruptibly, so each job on it
    took a worker for good; the watcher's lock, which every folder's
    opening shares, was held across `inotify_add_watch`;
  - a file on a volume that refused `.Trash-<uid>` was copied whole into
    the home trash, from the server to this machine;
  - references did not fall back to their path where inodes are made up
    (rclone, GVfs, sshfs), as ADR-021 said they would.
- Every reader and every operation takes a path: previews, thumbnails,
  helpers, operations with their staging, `syncfs`, attributes and the
  trash, Open With, the terminal, drags to other applications.
- Asked (2026-10-20), the user chose:
  1. mounts Skwir knows are remote, over Skwir speaking protocols itself
     (a file-system layer in the core, every reader and operation
     rewritten, credentials held), "but would be good to plan for
     potential move to 2 as an eventual experiment (and
     fallback/switch)";
  2. sshfs first for `sftp://`, GVfs next, rclone last;
  3. thumbnails in a remote folder Never by default, as Nautilus and
     Dolphin have them, chosen per remote (Never, Small files, Always), and
     by default Automatic: "Always by default if on LAN, Small files if
     high bandwidth, Never in other cases", cautious where unsure.

## Decision

1. **A remote folder is a folder.** Its location is its path under a
   mount an installed tool made (GVfs, sshfs, rclone, the kernel's NFS and
   CIFS, Windows' shares, macOS's own mounts). Everything that works in a
   folder works there, other applications included. Skwir speaks no
   file-system protocol and stores no credential (ADR-021).

2. **Reach** (`skwir_core::fs::reach`). A path is local or remote, told
   from one mount table the core keeps (read again when older than a
   second, at once after a mount made or let go of here): Linux's
   mountinfo types (NFS, CIFS/SMB, 9p, Ceph, AFS…, and the FUSE ones that
   reach a network: sshfs, rclone, GVfs, curlftpfs, s3fs…, or any whose
   source names a server), macOS's `MNT_LOCAL`, Windows' UNC paths and
   mapped drives. Each folder of GVfs's FUSE bridge is a remote of its
   own. Asked in memory, by a path's text or a folder's device, from any
   thread, never by reading the remote.

3. **Lanes.** Work that reads a path of a remote runs in its lane
   (`Scheduler::spawn_in`): at most two of its jobs at once (one fewer
   than the pool's workers in a pool of two), the rest waiting their turn
   without holding a worker. A worker held a second while work waits and
   no worker is free is stood in for, up to the pool's size again. A remote that stops answering costs its own jobs
   and some threads, never the window, local work, or another remote's;
   its folder's banner says it is not answering once its lane has run a
   job three seconds.
   No lock shared beyond one remote is held across a call that reaches a
   path.

4. **What differs on a network.**
   - Thumbnails follow the remote's rule: Never, Small files (up to
     4 MB), Always, or Automatic, which is Always on the local network
     (a private, link-local or loopback address, a `.local` name) and
     Never otherwise. (2026-10-23, the user's choice: "≥10 MB/s, this
     session".) Elsewhere, Small files once a copy down this session
     proved the server fast: 10 MB/s or more over 4 MiB or more, read by
     a file's step or a batch onto this machine (`fs::reach::note_read`),
     slow again under 5 MB/s; kept in memory for that mount alone.
     Nothing is read to find out, and previews are not counted (they
     read what they show, not the file). Each remote's thumbnails are made in its own lane,
     one worker each; a preview reads ahead there only where the rule is
     Always. A preview the user asks for is read whatever the rule.
   - A listing is read again when the window comes back (at most once in
     ten seconds) and on F5: a server does not say what changed on it.
     What this machine changes there is seen as anywhere.
   - Searches do not enter a network mount below their root; Usage
     measures a remote when asked (Refresh).
   - Delete of a file on a FUSE remote (sshfs, rclone, GVfs) is
     permanent and asked: the trash command makes a deletion there. A
     kernel share (NFS, SMB) keeps its own `.Trash-<uid>`. Nothing is
     ever copied from a remote into this machine's trash to be trashed.
   - Where inodes are made up for a mount's life (FUSE: sshfs numbers
     files in the order they are looked up, again once the kernel let go
     of one and from the start when mounted again, often on the device
     number the last mount had), a file's inode number is told apart by
     the mount's own number, mixed into it (`fs::stat::file`: the
     kernel's unique one, Linux 6.8, read from the mount table in memory
     and asked of no server), so a remount's files are other files to
     everything kept in memory. What
     is kept beyond the mount (the cache: fields, positions, converted
     previews) knows such a file by its path, as thumbnails are known;
     notes keep no snapshot of it. A reference there resolves at its
     path, and none is followed to another name by its number.
   - A copy onto a remote is done when the server has it: rclone mounts
     with `--vfs-cache-mode minimal` (a file written alone goes straight
     up; checked: on the server once closed).

5. **Addresses.** `sftp:`, `smb:`, `dav:`, `davs:`, `ftp:`, `ftps:`,
   `nfs:`, `afp:` and `rclone:` locations (and `ssh:`, `cifs:`,
   `webdav:`, `webdavs:` as other programs write them) are addresses:
   opening one connects it on a thread of its own and sends
   `ListingEvent::Redirect` with the mounted folder, which takes the
   address's place in the pane's history. Typed, bookmarked (GTK
   bookmarks other file managers write), given to `skwir`, navigated by a
   script: one way. An agent's connection is asked about first: the
   server is told who the user is. The address field completes the
   server being typed with those known (mounted, the tools', the
   bookmarks, the hosts of the user's ssh configuration for SFTP). A remote folder is named by its
   address where the user reads or keeps it: the sidebar's Network place
   of each mount roots its trail (`me@host › docs`), a deletion's question
   says `sftp://me@host/docs`, and tabs keep a remote folder as its
   address, so a later start connects again.

6. **Connectors** (the `remote` plugin). One trait: connect an address's
   top to a mount, recognise its tool's mounts (made by Skwir or not), let
   go of one, list the servers its configuration names. Which connector
   opens an address is a setting, an order (`remote.connectors`): sshfs,
   GVfs, rclone, then fstab. A mount already there is used first. Adding
   a protocol is a scheme a connector maps, or a connector.
   - **sshfs**: `sftp://`, through the user's ssh (configuration, agent,
     known hosts), reconnecting, a server gone told within a minute.
   - **GVfs**: `gio mount`, its questions read from its output and
     answered from the window; its mounts are the desktop's, shared with
     other applications; the session's own bridge
     (`$XDG_RUNTIME_DIR/gvfs`).
   - **rclone**: its configured remotes and SFTP, SMB, WebDAV and FTP
     addresses it opens without a password of Skwir's (SFTP through the
     user's ssh).
   - **fstab**: the network entries the user may mount (`user`, `users`).
   - Mounts Skwir makes are under `$XDG_RUNTIME_DIR/skwir/remote/`, each
     folder named so its address reads back from it: nothing else is
     kept. A mount outlives Skwir, as GVfs's do (principle 3).
   - The sidebar's Network section lists every network mount by its
     name, with Eject where the user may let go of it, and the servers
     the tools know; each place's menu chooses its thumbnails, kept in the
     data file (`remote.chosen`, ADR-057). A phone or a camera GVfs reads
     is a device: among the devices, near for its thumbnails.

7. **Credentials.** Never stored, logged, or put on a command line by
   Skwir. ssh's questions (a password, a key's passphrase, an unknown
   host) reach the window through `SSH_ASKPASS`: the `skwir` executable,
   run so with `SKWIR_ASKPASS` naming a socket of the connection's own
   (private, removed after), asks the window over it. GVfs's go through
   `gio mount`'s standard input; rclone's configuration password through
   its `--password-command`. A password is `App::ask_secret`, an unknown
   host `App::ask_as` with `Asker::Connection` (Connect, Cancel). A
   question refused ends that attempt: ssh would otherwise ask again,
   counting it as a wrong password. (Amended by ADR-081: KIO's questions
   are asked in KDE's own dialog, which Skwir never sees; and by ADR-082:
   a server's password the user asks to remember is kept in the desktop's
   keyring.)

8. **The way to Skwir speaking protocols, kept open.** The user may want
   it as an experiment, a fallback or a switch:
   - first, as a connector: a helper of Skwir's serving a mount itself
     (SFTP through OpenSSH's `sftp` subsystem, so ssh still holds the
     credentials). The core does not change; `remote.connectors` is the
     switch, per scheme;
   - then, only if mounts prove the limit, locations whose entries are
     not on this machine: operations chosen by source and target scheme
     (ADR-021's step 2, rclone's server-side copies), readers given what
     to read rather than a path, and a copy in the cache for other
     applications. Reach, lanes, addresses and connectors are what such a
     provider would reuse.

## Consequences

- Remote folders need a tool installed: sshfs, GVfs, rclone, or the
  system's mounts. Without one, an address says which is missing.
- SFTP on macOS and Windows needs a FUSE (macFUSE, WinFsp) for sshfs and
  rclone; SMB, NFS and WebDAV mount natively there, which no connector
  asks yet.
- A copy within one remote is made by the server only where the kernel
  asks it to (`copy_file_range` on NFS 4.2 and SMB); elsewhere the bytes
  come here and go back. rclone's API (8, second step) would do better.
  (Since: sftp's `cp`, `gio copy` within a GVfs mount, and, 2026-10-23,
  rclone's remote control within one rclone mount Skwir made, where its
  backend copies on the server: `rclone_copy`.)
- The `remote` plugin uses the network, through the tools it starts: it
  declares `network` and `programs` (ADR-044). The core still has no
  network layer; `map-tiles` still makes its own requests (ADR-020).
- What a hung NFS hard mount holds stays held: a plan reading it waits
  on its thread, as the kernel waits; lanes keep the rest going.
- Not built yet: servers found on the local network (GVfs's
  `network:///`); Windows' and macOS's own connectors. (Automatic's
  Small files where reads proved fast, and renaming and moving a remote
  bookmark: built 2026-10-23.)
