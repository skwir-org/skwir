# ADR-021: Remote file systems come from installed tools

Status: accepted, 2026-09-27, as a direction. Nothing is built yet; each
step below is planned and decided further when it starts.

## Context

Bibli will open files that are not on a local disk: SFTP servers, SMB
and WebDAV shares, S3 buckets, cloud drives. Each protocol is a client,
an authentication scheme and a set of failures of its own; Dolphin and
Nautilus reach them through KIO and GVfs, frameworks larger than bibli.
Implementing them in bibli would put a network layer in the core and
make bibli hold credentials.

Other software already speaks these protocols and keeps the
credentials. GVfs (`gio mount`) and sshfs present a remote as a local
folder through FUSE; rclone does the same for more than seventy back
ends (`rclone mount`) and also answers a JSON API over a socket
(`rclone rcd`: listing, copying and moving on the server, asynchronous
jobs that report progress and can be stopped). Principle 4 says to use
them.

Bibli already meets part of this. The places plugin reads mountinfo and
counts `fuse*` file systems as user data, though only under `/mnt`,
`/media` and `/run/media`: GVfs mounts, under `/run/user/<uid>/gvfs`, and
NFS and CIFS are not listed. The GTK bookmarks file keeps `sftp://`
lines, which bibli preserves but cannot open. Plugins register their own
operation kinds (`op::OpRegistry` is owner-tagged), and locations have
schemes with providers of their own (`query:`, `shelf:`).

## Decision

- **Bibli implements no file-system protocol, and the core has no
  network layer.** Remote files are reached through installed tools. A
  plugin that needs HTTP for something else keeps its client
  (`map-tiles`, ADR-020); a client two plugins share goes to
  `bibli-platform` as a utility, not into the core as a contract.
- **Bibli holds no credentials.** The keyring, GVfs and rclone's
  configuration keep them; a prompt they need is theirs. A passphrase
  bibli must pass on follows the rule for locked volumes (never stored,
  logged or put on a command line; its argument struct has no `Debug`).
- **First, mounts.** A remote mounted by any tool is a folder:
  - The places plugin lists GVfs mounts and network file systems
    (`nfs`, `cifs`, `9p`, `fuse.sshfs`, `fuse.rclone`,
    `fuse.gvfsd-fuse`…) beside local volumes, and opens an `sftp://`,
    `smb://` or `dav://` bookmark by mounting it with `gio mount` and
    navigating to its mount point, as `places.mount` does with udisks.
    rclone remotes (`rclone listremotes`) can be mounted the same way.
    Unmounting is the place's eject.
  - A location on such a file system is **remote**: `bibli-platform`
    reads it from mountinfo's type and the core carries it on the
    location, so every plugin can act on it. On a
    remote location, content is read only when asked: thumbnails and
    previews wait for the user, Usage asks before walking a tree. The
    listing is read again on F5 and when bibli regains focus, because
    the kernel does not report changes made on the server. Deleting says
    it is permanent where there is no Trash. Tags say they are
    unavailable where extended attributes are.
  - Remote locations get a small worker budget of their own. A server
    that stops answering can block a `stat` on its mount in the kernel,
    uninterruptibly; the UI thread never waits on one (principle 1), and
    a hung remote must not take every worker with it.
- **Then, where mounts fall short, rclone as a worker.** A `remote`
  plugin runs `rclone rcd` for the session on a private socket, like the
  media and document workers, and registers a location provider for
  rclone remotes. It gives what a mount cannot: copies and moves done on
  the server, real progress and cancellation (`job/status`, `job/stop`),
  bounded requests instead of kernel waits, and no FUSE, which macOS and
  Windows make awkward (Phase 5). Its operations are plugin-registered
  kinds with inverses where the back end allows them.
- **What the rclone step needs from the core** is a way for copy, paste,
  drop and the shelf to pick an operation by the schemes of their source
  and target, as `Keeper` picks by scheme, instead of building a
  path-based `Copy` directly. It is decided in its own ADR when that
  step starts.

## Consequences

Remote access depends on what is installed: without GVfs or rclone, the
feature is absent and says why (`Plugin::requirements` probes).
Behaviour follows the tools: rclone's and some FUSE file systems'
inode numbers are made up and may change between mounts, so entry
references on a remote fall back to their path hint and stable identity
(principle 6) is weaker there. What a server supports (Trash, extended
attributes, modification times, permissions) varies, and bibli reports
the difference rather than hiding it.

The core stays free of protocols, clients and secrets. Nothing prevents
a later plugin from speaking a protocol itself if a tool proves
unusable; that would be a new decision, not a change to the core.
