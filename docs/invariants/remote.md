# Remote invariants: remote folders, lanes, connectors, addresses

Read this before touching what reads a path that may be across a network
(listing, stat, thumbnails, previews, walks, the sidebar's measures), the
`remote` plugin, or a connector. Binding, like `AGENTS.md`. Reasoning:
ADR-077 (which revises ADR-021).

## A remote folder is a folder

- A remote is a mount an installed tool made (sshfs, GVfs, rclone, the
  kernel's NFS and CIFS, a Windows share). Its folders are path
  locations: everything that works in a folder works there. Skwir speaks
  no file-system protocol and stores no credential.
- Whether a path is remote is `skwir_core::fs::reach` (`of`, `lane`,
  `of_device`): the system's mount table, read again when older than a
  second and at once after a mount made or let go of here
  (`reach::changed`). Never decide it by reading the path: a lookup is in
  memory, from any thread. Never `canonicalize` a path to classify it.
- A path reached through a link into a remote is told by its folder's
  device once opened (`reach::of_device`): the folder listing takes its
  lane from it.

## Lanes

- Work that reads a path that may be remote runs in its lane:
  `Scheduler::spawn_in(reach::lane(path), …)`, the UI's
  `Shell::off_thread_in(lane, …)` (and `off_thread_through` with a
  token). A lane runs at most `LANE_RUNS` jobs at once; the rest wait
  their turn without holding a worker. A remote that
  stops answering holds its own jobs, never the window, local work, or
  another remote's.
- A worker busy past `STUCK` while work waits is replaced, up to the
  pool's size again: a call that never returns costs a thread, not the
  pool.
- A remote whose lane has run a job for `NOT_ANSWERING` (three seconds)
  while its folder is listed is said not to answer in the pane's banner
  (`Scheduler::stalled`, `ui/loading.rs`), looked at again each second
  while it is listed, never once it is.
- No lock shared beyond one remote is held across a call that reaches a
  path (`inotify_add_watch`, `statvfs`, `open`): take what is needed,
  let go, call, then take the lock again.

## What differs on a network

- **Thumbnails** follow the remote's rule (`Registry::remotes`,
  `App::shows_thumbnail`): Never, Small files (`Thumbnails::SMALL`),
  Always, or Automatic (Always on the local network, Never otherwise or
  when unsure); Never where no plugin knows the remote. Ask it before
  reading anything. A remote's thumbnails are made in its lane, one
  worker each (`thumbs.rs`); preview-ahead reads a remote's files only
  where its thumbnails are Always. A preview asked for reads anyway.
- **Listings** are read again when the window comes back
  (`App::refresh_remote`, at most once in `REFRESHED_WITHIN`) and on F5:
  a server does not say what changed on it. What this machine changes
  there is watched as anywhere.
- **Walks** (searches, counts) never enter a network mount below their
  root (`reach::is_point`); Usage asks before measuring a remote.
- **Delete** on a FUSE remote (sshfs, rclone, GVfs) is permanent and asked
  (the trash command makes a `Delete` there); a kernel share keeps its own
  `.Trash-<uid>`. Nothing is ever copied from a remote into this
  machine's trash to be trashed.
- **References** on a FUSE remote whose device changed (mounted again,
  inodes made up anew) resolve at their path (`EntryRef::resolve`).
- **Staging** is never swept on a remote (`volume::only_here`).

## Addresses

- `sftp:`, `smb:`, `dav:`, `davs:`, `ftp:`, `ftps:`, `nfs:`, `afp:` and
  `rclone:` locations are addresses (`remote::address::Address`): opening
  one connects it on a thread of its own and sends
  `ListingEvent::Redirect` with the mounted folder, which replaces the
  address in the pane's history. Never a password in an address.
- A remote folder is named by its address where the user reads or keeps
  it: its Network place roots its trail; a path said to the user is
  written as its address (`remotes::address_below`, the deletion's
  question); tabs keep it as its address (`App::address_of`), so a later
  start connects again; the folder remembered for the next start is
  never a remote's.
- A remote folder bookmarked is kept by its address in the desktop's
  shared bookmarks (`BookmarkEdit::AddAddress`, `uri::address_uri`), as
  other file managers keep theirs, never by its mount's path.
- An agent's call connects only once the user allowed it.
- The address field completes the server being typed (`sftp://me@ho`)
  with those known: mounted, the tools', the remote bookmarks, and for
  SFTP the `Host` entries of the user's ssh configuration. A folder after
  the server is not completed: the server would be asked.

## Connectors

- One trait (`remote::connector::Connector`): connect an address's top
  to a mount, recognise mounts of its tool, disconnect, list the servers
  its configuration names. The order tried is `remote.connectors`; a
  mount already there is used first. Adding a protocol is a scheme a
  connector maps, or a connector.
- A mount Skwir makes is under `$XDG_RUNTIME_DIR/skwir/remote/`, named by
  `connector::point_name` so its address reads back from it: nothing else
  is kept. It outlives Skwir, as GVfs's do.
- A tool that daemonizes is run until it has mounted
  (`process::run_leaving`): cancelled, it and what it started are stopped;
  mounted, nothing it left running is touched.
- A copy onto a remote is done when the server has it: rclone mounts with
  `--vfs-cache-mode minimal` (a file written alone goes straight up).
- A question refused ends that attempt (`mounter::attempt`): ssh counts
  a refused password as a wrong one and asks again otherwise. What its
  asker stopped waiting for leaves the window (`App::withdraw`,
  `withdraw_secret` wake it).
- **Credentials**: never stored, logged, or put on a command line by
  Skwir. ssh asks through `SSH_ASKPASS` = the `skwir` executable, which
  asks the window over the connection's own socket
  (`skwir_platform::askpass`); `gio mount`'s questions are read from its
  output (English, `LC_MESSAGES=C`) and answered on its input; a password
  is `App::ask_secret`, a server Skwir cannot vouch for `App::ask_as`
  with `Asker::Connection`.
- Test a connector against a server on this machine (`rclone serve`
  with the fixture's XDG folders: it keeps host keys in its cache), never
  a real one. ssh takes its home from the user database, never `$HOME`:
  a test gives it `-F /dev/null` and a `UserKnownHostsFile` of its own
  (`Sshfs`'s test fields), and a window check never accepts a host into
  the user's known hosts (try SFTP's questions there and remove the line
  after, or check through WebDAV and rclone); and never the session's GVfs: a GVfs of the test's own runs
  on a bus whose configuration lists GVfs's services alone, its runtime
  folder short enough for ssh's control socket
  (`crates/skwir/tests/remote_folders.rs`, `just remote-check`).
