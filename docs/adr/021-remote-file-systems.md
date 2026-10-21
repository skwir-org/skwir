# ADR-021: Remote file systems come from installed tools

Status: accepted, 2026-09-27, as a direction; revised by ADR-077
(2026-10-21), which decided and built its steps: mounts Skwir knows are
remote, through sshfs, GVfs, rclone and fstab, and the way to rclone's API
kept open.

## Context

Skwir will open remote files: SFTP, SMB and WebDAV shares, S3 buckets,
cloud drives. Each protocol is a client, an authentication scheme and its
own failures; Dolphin and Nautilus use KIO and GVfs, frameworks larger than
Skwir. Implementing them would put a network layer in the core and make
Skwir hold credentials.

Installed tools already speak them and keep the credentials: GVfs
(`gio mount`) and sshfs mount a remote through FUSE; rclone does so for
over seventy back ends (`rclone mount`) and serves a JSON API on a socket
(`rclone rcd`: server-side copy and move, jobs with progress and
cancellation). Principle 4 says use them.

Today the places plugin lists `fuse*` mounts only under `/mnt`, `/media`
and `/run/media`: GVfs mounts (`/run/user/<uid>/gvfs`), NFS and CIFS are
not listed. `sftp://` lines in GTK bookmarks are kept but cannot be opened.
Plugins can register operation kinds (`op::OpRegistry`) and location
schemes (`query:`, `shelf:`).

## Decision

- **No file-system protocol in Skwir, no network layer in the core.** A
  plugin needing HTTP keeps its own client (`map-tiles`, ADR-020); one two
  plugins share goes to `skwir-platform` as a utility, not a core
  contract.
- **Skwir holds no credentials.** The keyring, GVfs and rclone keep them,
  and prompt for them. A passphrase Skwir must pass on follows the
  locked-volume rule: never stored, logged or on a command line; no `Debug`
  on its argument struct.
- **Step 1: mounts.** A mounted remote is a folder.
  - The places plugin lists GVfs mounts and network file systems (`nfs`,
    `cifs`, `9p`, `fuse.sshfs`, `fuse.rclone`, `fuse.gvfsd-fuse`…). An
    `sftp://`, `smb://` or `dav://` bookmark opens by `gio mount`, as
    `places.mount` uses udisks; rclone remotes (`rclone listremotes`)
    mount the same way. Eject unmounts.
  - A location there is **remote** (read by `skwir-platform` from
    mountinfo's type, carried by the core on the location). Content is read
    only when asked (thumbnails and previews wait; Usage asks before
    walking); the listing is reread on F5 and on focus, since the kernel
    does not report server-side changes; Delete says it is permanent
    without a Trash; tags say they are unavailable without extended
    attributes.
  - Remote locations get a small worker budget of their own: a dead server
    can block a `stat` uninterruptibly. The UI thread never waits on one,
    and a hung remote must not take every worker.
- **Step 2, where mounts fall short: rclone as a worker.** A `remote`
  plugin runs `rclone rcd` on a private socket for the session and provides
  locations for rclone remotes: server-side copies and moves, real progress
  and cancellation (`job/status`, `job/stop`), bounded requests instead of
  kernel waits, no FUSE (awkward on macOS and Windows). Its operations are
  plugin kinds, with inverses where the back end allows.
- **Step 2 needs from the core** that copy, paste, drop and the shelf pick
  an operation by source and target schemes (as `Keeper` does) instead of
  building a path-based `Copy`. Decided in its own ADR.

## Consequences

- Remote access depends on what is installed; without GVfs or rclone the
  feature is absent and says why (`Plugin::requirements`).
- rclone's and some FUSE file systems' inodes are made up and may change
  between mounts: references fall back to their path hint, so stable
  identity (principle 6) is weaker there.
- Server capabilities (Trash, xattrs, mtimes, permissions) vary; Skwir
  reports the difference rather than hiding it.
- A later plugin speaking a protocol itself, if a tool proves unusable,
  would be a new decision, not a core change.
