# ADR-102: rclone keeps what it reads; a copy waits until the server has it

Status: accepted 2026-10-28, built 2026-10-29 on Linux; macOS and
Windows compiled (Windows' remote control on the loopback checked on
Linux; WinFsp is not under Wine). Revises ADR-077's mount
(`--vfs-cache-mode minimal`), keeping its rule: a copy onto a remote is
done when the server has it. Amends ADR-086's Mount Options (the File
Cache gains Automatic, and a cache chosen is kept, `minimal` included).

## Context

- **`minimal` keeps nothing read.** Every open of a file on the mount is
  a download, and so is a seek backward or far ahead (ADR-101: 13 for a
  55-byte text file before Skwir read it once). ADR-101 fixes Skwir's
  previews; the programs a file is opened with (LibreOffice, a PDF
  viewer, a picture viewer) read as they read, each open and jump a
  request to the server. And an upload `minimal` sends as the file is
  written cannot be tried again when it fails: nothing of it is kept.
- **`full` keeps what is read and written on this disk**, rclone's own
  cache: the 13 opens were one GET, then none (measured 2026-10-28,
  rclone 1.75.2, WebDAV on this machine). What is written goes up 5 s
  after its `close` (`--vfs-write-back`), and is tried again while it
  fails; a mount stopped 1 s after a `close` left the file in the cache
  alone, and the next mount with that cache sent it.
- **What rclone sends later can be asked.** Its remote control lists the
  uploads waiting, by name, tries and whether each is going
  (`vfs/queue`); sends one now (`vfs/queue-set-expiry`, expiry 0: on the
  server within the second, measured); counts them (`vfs/stats`). Every
  mount Skwir makes serves its remote control on a socket of the runtime
  folder (ADR-086).
- **A copy onto a mount runs no plugin code.** Core writes through it and
  its `close` counts (`builtin::finish`): with `full` the plan would say
  done 5 s before the server has the file, and a move would remove its
  original before then. A drive the user can unplug already waits so:
  the plan ends once the drive has what it wrote, and a move removes its
  originals then (`StepContext::write_out`, `after_write_out`, `pace.rs`).
- **`minimal` was never kept as a choice**: the Mount Options store
  nothing for it (`None` is Skwir's own default), so a user's choice of
  it and no choice look alike.
- The user, 2026-10-28: "Let's go to `full` with safeguards, so other
  apps would benefit and we would get retries. But we should also handle
  the case where the user manually set the rclone config to `minimal`".

## Decision

1. **Automatic: `full`, but where it costs more than it gives.** A mount
   Skwir makes with nothing chosen keeps what it reads (`full`), except:
   - a server on the local network (`lan::said`, `lan::local`), where an
     open costs little and a disk-like server is expected to have what
     was written when `close` returns: `minimal`;
   - a remote that encrypts or wraps others (crypt, alias, union,
     combine, chunker, compress, hasher: its kind, `ON_REMOTES`), whose
     files `full` would keep decrypted on this disk: `minimal`.
2. **A choice is kept.** The File Cache offers Automatic (nothing stored;
   the form says what it chose for this server, and why) beside `off`,
   `minimal`, `writes` and `full`, each stored when chosen and given to
   rclone as chosen, `minimal` included.
3. **rclone's cache, bounded.** rclone's own cache folder, never Skwir's:
   a file waiting there to be sent is not deletable (ADR-000), and any
   later mount of that remote with that cache sends it, Skwir's or the
   user's own. Kept an hour at most (`--vfs-cache-max-age`, rclone's
   default, said), `--vfs-cache-max-size 1G`, and never past what would
   leave the disk under 2 GB free (`--vfs-cache-min-free-space`). A file
   open or waiting to be sent stays whatever the bounds.
4. **A copy is done when the server has it**, as ADR-077 said, on every
   cache. A mount that sends later (`writes`, `full`) says so to core
   (`Known::sends_later`, published with the mount, answered through its
   remote control), and a plan writes onto it as onto a drive the user
   can unplug: at its end, and on the way where moves wait
   (`write_out`, `after_write_out`), it has the mount send now what the
   plan wrote (`vfs/queue-set-expiry`, expiry 0) and waits until the
   queue holds none of it, saying "Sending to the server" with what is
   left. A move removes its original only then. An upload that failed
   three times fails the write-out: the plan says the server refused it,
   the file stays in rclone's cache, which goes on trying, and a move's
   original stays.
5. **Disconnect sends first.** Eject and Disconnect ask the mount's queue
   before letting go: while uploads wait, the place says "Sending 3 files
   to the server…", and Cancel leaves it connected. A forced Disconnect
   says what stays in the cache, to be sent by the next mount.
6. **Seen while it sends.** A plan's progress says how many files the
   server still takes (`op::Transfer::sending`), and a Disconnect's place
   how many it sends first. What other programs left waiting is said on
   the mount's place ("Sending 3 files to the server…"): asked when the
   window comes back (`App::refresh_remote`, `Known::looked_at`: every
   remote, shown or not, at most every 10 s), then every 2 s while
   something waits, on a thread of its own, until nothing does. No timer
   otherwise.
9. **Every system.** A mount's remote control is reached where it
   answers (`uploads::control_of`): a socket of the runtime folder on
   Unix, the loopback behind its login on Windows (`winfsp::rc_of`). The
   copy's wait is a transfer of its own (`uploads::RcloneSends`),
   registered everywhere beside Unix's copies on the server.
7. **Quitting Skwir leaves rclone sending**: its mounts outlive it
   (ADR-077). A session ended with uploads waiting leaves them in the
   cache, sent by the next mount (at login: ADR-087).
8. **A mount Skwir did not make** serves no remote control Skwir knows:
   its `close` counts, as before.

## Consequences

- Programs opening a cloud's files read them from this disk after the
  first time, within the hour; an upload that fails is tried again.
- A copy onto a cloud takes as long as before (the plan waits for the
  server, as it does now during `close`), and its last seconds say what
  is being sent. A move's originals stay until the server has the copy.
- Disk: up to 1 GB of rclone's cache per mount (more while files wait
  to be sent), gone after an hour unread; `rclone`'s own `--cache-dir`
  is the user's to set in its environment.
- What changes: `rclone::mount_arguments` (`full` by the Automatic
  rule, the bounds), `options.rs` (Automatic, `minimal` stored),
  `reach::Known` (`sends_later`), `op/context.rs` (a write-out for
  remotes beside drives' paces), the plugin's Disconnect
  (`begin_disconnect`'s `letting_go`, `unmount_on_windows`) and its
  place's busy line; Windows' test of the logon task's flags
  (`winfsp.rs`) learns the new ones.
- Checked by `just remote-check` against `rclone serve webdav`: a copy's
  plan ends after the server has each file (its log), a move's original
  stays until then, a server refusing uploads (`--max-transfer`) fails
  the write-out and keeps the original, Disconnect waits for the queue,
  a chosen `minimal` mounts `minimal`, a crypt mounts `minimal`.
