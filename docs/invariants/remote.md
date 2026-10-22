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
  token). A lane runs at most `LANE_RUNS` jobs at once, and always one
  fewer than the pool's workers; the rest wait their turn without
  holding a worker (cancelled ones are let go of as more park, and with
  the pool). A remote that stops answering holds its own jobs, never the
  window, local work, or another remote's.
- A worker busy past `STUCK` while work waits is replaced, up to the
  pool's size again, whether or not more work is asked after (a look is
  planned `STUCK` later): a call that never returns costs a thread, not
  the pool.
- A job that serves one request after another (a remote's thumbnails)
  says each begun with `scheduler::progress`: its lane is not taken for
  stalled however long the job runs.
- Work that reads nothing of a remote (closing a folder: its watch, its
  descriptor) runs in the shared lane, never behind a held one.
- A remote whose lane has run a job for `NOT_ANSWERING` (three seconds)
  while its folder is listed is said not to answer in the pane's banner
  (`Scheduler::stalled`, `ui/loading.rs`), looked at again each second
  while it is listed, never once it is.
- No lock shared beyond one remote is held across a call that reaches a
  path (`inotify_add_watch`, `statvfs`, `open`): take what is needed,
  let go, call, then take the lock again. What such a call may race with
  is settled after it, never by holding on to things while it runs (the
  watcher adds again a descriptor let go of during its add; it keeps no
  orphan).

## What differs on a network

- **Thumbnails** follow the remote's rule (`Registry::remotes`,
  `App::shows_thumbnail`): Never, Small files (`Thumbnails::SMALL`),
  Always, or Automatic (Always on the local network, Never otherwise or
  when unsure; Small files on the local network where its program reads
  a file whole before any part of it, kio-fuse over WebDAV, FTP and NFS:
  `Connector::reads_whole`); Never where no plugin knows the remote. Ask it before
  reading anything. A remote's thumbnails are made in its lane, one
  worker each (`thumbs.rs`); preview-ahead reads a remote's files only
  where its thumbnails are Always. A preview asked for reads anyway.
- **Listings** are read again when the window comes back
  (`App::refresh_remote`, at most once in `REFRESHED_WITHIN`) and on F5:
  a server does not say what changed on it. What this machine changes
  there is watched as anywhere. A listing is read again through
  `App::read_again`, never `OpenLocation::refresh` alone: a program that
  keeps what it listed (kio-fuse, 30 s) is told to forget it first
  (`Known::forget`), in the remote's lane.
- **Walks** (searches, counts) never enter a network mount below their
  root (`reach::is_point`); Usage asks before measuring a remote.
- **Delete** on a FUSE remote (sshfs, rclone, GVfs) is permanent and asked
  (the trash command makes a `Delete` there); a kernel share keeps its own
  `.Trash-<uid>`. Nothing is ever copied from a remote into this
  machine's trash to be trashed. The question counts what is deleted;
  what the same command moves to the trash is said beside it. A run of
  deletions in one folder there goes together, as moves' originals do
  (`transfer::remove_ahead`, by `gio remove`, rsync or sftp, a lone
  folder too): through the mount, each name is a round trip under the
  folder's lock. 100 files of 16 KiB from a server 20 ms away: 0.62 s
  by rsync, 1.13 by sftp (2.29–2.33 through the mount); a folder of 100,
  0.48 and 1.37 (2.38–2.43).
- **Undo there** removes what a step made rather than putting it in a
  trash there is none of: a copy (`Uncopy`), an archive extracted or made
  (`archive_unmake`), only while it is as it was made (`op::made_stays`),
  Redo making it again; Replace deletes what is replaced for good, which
  the question about names taken says before anyone chooses it.
- **Identity** on a mount whose program makes inode numbers up (FUSE:
  sshfs, GVfs, rclone, a FUSE mount on this machine alike): sshfs
  numbers files in the order they are looked up, again once the kernel
  let go of one and from the start once remounted, often on the device
  number the last mount had. The inode number there is told apart by the
  mount's own number, mixed into it (`fs::stat::file`, `stat::on_mount`,
  `reach::mount_of`), and `FileId` keeps its 16 bytes, which every
  entry, order and selection carries (24 made a sorted listing's work
  12 to 25 % slower). Every stat names a file through `fs::stat::file` (a
  folder held open asks once for its entries and its listing's
  provisional numbers), so a listing, an operation and a field agree;
  whether a device's numbers are made up is `FileId::lasts`, asked of
  the table. The mount's number is the kernel's unique one (Linux 6.8),
  read with the table by a `statx` of the point that asks its program
  nothing (`AT_STATX_DONT_SYNC`); a point below another FUSE mount or a
  network's is not looked up, and keeps the table's number, which a
  later mount may take again.
- One name, one row: a listing removes every id a name it lists belonged
  to before (`path_provider::gone_after`), not only those its refresh
  noted when asked; a file the watcher added meanwhile, numbered anew by
  its FUSE program since (sshfs, once the kernel forgot it), would
  otherwise stay beside the new one. And a folder's workers (its listing,
  the watcher's changes, hydration: two at once in a remote's lane) send
  what they did to its names under the names' lock, as the names hold it
  then (`Shared::send_as_now`): sent after the lock, one worker's events
  reached the pane after another's that changed the names later, and a
  name kept two rows.
- What is kept beyond the mount (the cache: fields, positions, converted
  previews) never knows a file by a made-up identity: by its path
  (`FileId::kept_as`, where `FileId::lasts` says no), as thumbnails are;
  notes keep no snapshot of it, and merge as when none was kept.
- **References** there resolve at their path (`EntryRef::resolve`):
  another number tells neither a remount nor a replacement. They are
  never followed to another name by number.
- **Devices GVfs reads** (a phone over MTP or AFC, a camera over
  gPhoto2: `Recognised::device`) are near, whatever their address, and
  listed among the sidebar's devices; they keep a lane of their own.
- **Staging** on a remote is swept only where this machine noted it
  (operations.md, "Staging"): a folder its copy could not remove as its
  server stopped answering goes once that remote answers again.
- **Copies** (ADR-078): a file of two ranges or more on a FUSE remote is
  read 8 ranges of 4 MiB at a time, written in order (`op::ranges`): the
  kernel reads FUSE ahead 128 KiB at a time, a round trip each. A run of
  copies to or from a remote goes in batches (operations.md, ADR-078).
  For an sshfs mount, by rsync: through the connection sshfs holds for
  one Skwir made (its ssh is the master of a control socket in the
  runtime folder, `ssh_mount::control_path`), the user's own ssh for one
  they made, on the port sshfs was given (`ssh_mount`), asking nothing
  (`BatchMode`) either way; a server that runs none (asked once:
  `rsync --version`) leaves uploads to eight `sftp` sessions (`sftp`),
  downloads to Skwir's own copy, sixteen files at once: sshfs serves
  ten requests at once (libfuse's default, which it offers no option to
  change), a file down through it two round trips, where an `sftp`
  session gets one file after another, four each. For a mount of the
  session's GVfs of SFTP, FTP, WebDAV, SMB, NFS, a phone's MTP or a
  camera's gPhoto2, by
  eight `gio copy` at once (`gio_copy`: `-i`, nothing on its input, so
  nothing is replaced), what a server refused at once copied again by
  one; of AFP the same up, within and removed, but down through its
  bridge, sixteen files at once, which was faster (`gio_copy::Ways`).
  A file alone goes by `gio` where it was measured faster than the
  bridge, which reads 128 KiB at a time (down from NFS and AFP, both
  ways on MTP), and up to a camera, whose bridge sends a file after its
  `close` returned (a refusal there never told); by its step elsewhere. For a server of kio-fuse's of
  WebDAV, SMB or FTP, by eight `kioclient copy` at once (`kio_copy`:
  each its own KIO workers; addresses read from the mount's folders;
  never over anything without `--overwrite`), up, down and within one
  server, measured faster every way; over FTP a file alone too, kio-fuse
  writing no new file there (KIO's FTP worker reads back no empty one).
  kio-fuse is told to forget what it listed after each, the originals of
  moves removed by `kioclient remove`. Else
  through the mount, eight files at a time, sixteen down. Within one
  mount: by `sftp`'s `cp` on an sshfs mount's server (the copy-data
  extension, asked once), each file then given its time through the
  mount; by `gio copy` within a GVfs mount (its daemon copies). A lone
  file within one remote goes in a batch where a transfer serves it,
  which its step would bring here and send back. A server a tool does
  not reach is remembered for its mount. rsync keeps a file it was
  writing when cut short in the staging folder (`--partial-dir`, an
  absolute path on the receiving side, else none), which its retry goes
  on from (ADR-080, 7).
  A connector offers transfers for its mounts (`Connector::transfers`).
  A copy made with its source's mode is not given it again.
- The originals of moves from GVfs are removed together by `gio remove`
  at the plan's end (operations.md); from sshfs, by rsync where the
  server runs it, else sftp sessions (`ssh_mount::remove_beside`). sshfs
  keeps what it read of the server 20 s (`dcache_timeout`), and lets go
  of a path only when something is made, removed or renamed there
  through the mount, of all below a name renamed, or when the server
  refuses to open it: so each file a tool removed is opened through the
  mount after, a folder is renamed through the mount into a folder of
  Skwir's in its own before the tool removes it there, and that folder
  goes last, through the mount. sftp removes only where the mount shows
  links as links (not `follow_symlinks`): it reads the tree through the
  mount. A removal through the mount waits for the server under the
  kernel's lock of its folder, one name at a time; an open does not.
- What a tool writes or removes beside a mount, no watch sees: the
  window reads again the folders its panes show there at the plan's end
  (operations.md). sshfs, which keeps a folder's names 20 s, reads such
  a folder from the server again once a folder of Skwir's is made and
  removed in it (`ssh_mount::shown_again`), at the plan's end just
  before the window reads it (`Transfer::show`): a pane that read the
  folder while a tool wrote there could have had sshfs keep what it
  held then, for 20 s (a `remote-check` run under load once waited for
  the copies in vain).

## Addresses

- `sftp:`, `smb:`, `dav:`, `davs:`, `ftp:`, `ftps:`, `nfs:`, `afp:` and
  `rclone:` locations are addresses (`remote::address::Address`): opening
  one connects it on a thread of its own and sends
  `ListingEvent::Redirect` with the mounted folder, which replaces the
  address in the pane's history, only while the pane still shows the
  address (`workspace::Redirect::from`). Never a password in an address.
- Connections to one address's top take turns (`Remotes::reach`): the
  second finds the first's mount. A mount at the point Skwir would make
  is let go of only when it answers "not connected"; a live one is never
  unmounted under what uses it.
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
- GVfs is the session's: its bridge is `$XDG_RUNTIME_DIR/gvfs` alone
  (`Gvfs::bridge`), never another the mount table lists (another
  user's, or the user's own while a window runs with a runtime folder of
  its own); a mount of another bridge is neither recognised nor
  connected through. It is there where its daemon runs or the session
  bus would start it (`org.gtk.vfs.Daemon`): `gio mount` starts it, which
  mounts the bridge, waited for a moment.
- KIO is the session's kio-fuse (ADR-081), asked over the session bus
  (`Kio`, `mountUrl`), there where the bus has it or would start it; its
  mount is a `kio-fuse-…` folder of the session's runtime folder alone.
  Each server is a folder two below it (`webdav/nas:8080`), a remote of
  its own (`reach`'s bridges: GVfs's one deep, kio-fuse's two), its
  address read back from the two folders' names; what it holds of no
  remote (an archive KIO reads) is not shown. It mounts what it is asked:
  an address on a server it holds is asked for again
  (`Connector::mounts_each_path`). It lets go of no server: no Eject.
- Letting go of a mount (Eject) first leaves the folders shown there
  (`App::release`), then waits, ten seconds at most, until their
  listings let go of them (closed a moment later on a worker,
  `fs::wait_let_go`) and the remote's lane runs nothing: until then the
  system refuses the unmount as busy, and Eject said another program
  used it. GVfs refuses too while its backend is at work (a read through
  its bridge, a folder just listed through it, closed after the bridge's
  release returned): `Gvfs::disconnect` asks again while the mount is
  still there, five seconds at most (`BUSY_PATIENCE`).
- The system refuses to unmount a folder watched as one used (GVfs's
  trash puts a watch on the top of every mount, inotify). Refused as
  busy, `mounter::unmount` asks what uses the mount
  (`process::using`: a file open or mapped there, a working folder in
  it; never a watch, never a look under it): nothing, it is let go of
  lazily, which ends the watches; something, Eject names it ("“vim” is
  using it"). Where the system does not say, it says another program
  uses it: Linux's `/proc` says, macOS's `lsof` (compiled only); Skwir
  mounts nothing on Windows yet. A mount fstab lets the user mount is let
  go of so too (`umount -l` where nothing uses it).
- Whether each mount's program still answers is asked in its lane, one
  question at a time a mount: none is queued behind one a server gone
  holds for good. A refresh publishes what is mounted at once, read from
  memory, and the servers the tools know after (`rclone listremotes`, a
  name to resolve): the mounts never wait on them.
- What is mounted is kept in the plugin's state, the newest read, and
  every publish shows the state as it then is, one at a time: a publish
  never brings back an older list (a slow refresh's, a liveness
  answer's), which took a new mount's `Known` away. A connection made
  publishes before its folder is shown (`Remotes::publish_mounted`), so
  F5 there finds what its program is told (`Known::forget`). Nothing is
  published once the plugin is disabled.
- A mount Skwir makes is under `$XDG_RUNTIME_DIR/skwir/remote/`, named by
  `connector::point_name` so its address reads back from it: nothing else
  is kept. It outlives Skwir, as GVfs's do.
- A tool that daemonizes is run until it has mounted
  (`process::run_leaving`): cancelled, it and what it started are stopped;
  mounted, nothing it left running is touched.
- A copy onto a remote is done when the server has it: rclone mounts with
  `--vfs-cache-mode minimal` (a file written alone goes straight up), and
  a copy is done once its close said so (operations.md): a FUSE program
  sends there what it held and tells there what its server refused.
- A question refused ends that attempt (`mounter::attempt`): ssh counts
  a refused password as a wrong one and asks again otherwise. What its
  asker stopped waiting for leaves the window (`App::withdraw`,
  `withdraw_secret` wake it).
- **Credentials**: never stored by Skwir but in the desktop's keyring,
  never logged or put on a command line. A server's password the user
  asked to remember (the prompt's Remember, ADR-082) is kept there once
  the connection served (`passwords::Keyed`), keyed by the server
  (`passwords::key`), and the plugin's data notes which servers have one
  (`remote.remembered`), never the password; Forget Password removes it.
  A connection's password question is answered from the keyring first:
  Skwir's own, then one GVfs kept (libsecret's network schema, read
  only, never changed), the window asked once one is refused. A server's
  password is `Ask::password`: GVfs's Password question, ssh's prompts
  that name the login (`me@host's password:`); a key's passphrase, a
  code, rclone's configuration's are `Ask::secret`, asked each time.
  rclone takes a server's password obscured, in its environment alone
  (`RCLONE_SMB_PASS`…), after listing the server's top tells whether it
  needs one. ssh asks through `SSH_ASKPASS` = the `skwir` executable, which
  asks the window over the connection's own socket
  (`skwir_platform::askpass`); `gio mount`'s questions are read from its
  output (English, `LC_MESSAGES=C`) and answered on its input; a password
  is `App::ask_secret`, a server Skwir cannot vouch for `App::ask_as`
  with `Asker::Connection`. KIO's are KDE's own (its password dialog,
  KWallet), which Skwir never sees: kio-fuse asks nothing, so where it
  fails `kioclient --interactive ls` lets KDE ask, then kio-fuse is asked
  again; over HTTPS KDE asks first (on a certificate not trusted kio-fuse
  waits for good), twice where the first failed. Never `stat`: its error
  box waits to be dismissed, then the window says it again.
- Test a connector against a server on this machine (`rclone serve`
  with the fixture's XDG folders: it keeps host keys in its cache), never
  a real one. ssh takes its home from the user database, never `$HOME`:
  a test gives it `-F /dev/null` and a `UserKnownHostsFile` of its own
  (`Sshfs`'s test fields), and a window check never accepts a host into
  the user's known hosts (try SFTP's questions there and remove the line
  after, or check through WebDAV and rclone); and never the session's GVfs: a GVfs of the test's own runs
  on a bus whose configuration lists GVfs's services alone, its runtime
  folder short enough for ssh's control socket
  (`crates/skwir/tests/remote_folders.rs`, `just remote-check`). A KIO
  of the test's own likewise (`PrivateKio`), Qt drawing nothing; what
  its bus started runs apart from it once the bus's helper ends, and is
  known by the runtime folder it was given, never by name. A WebDAV
  server of rclone's is changed through WebDAV, as another client would:
  rclone keeps what it listed, and with no cache (`--dir-cache-time 0`)
  its cleanups ran into each other and it stopped answering. Samba runs
  as the user (`OwnSamba`): `smbd` believes it is root (`uid_wrapper`),
  its user `tester` is the user and `nobody` its guest (`nss_wrapper`),
  on a port of its own; a share asking `tester`'s password, one a guest
  opens (KIO's password dialog no test answers).
- KDE's dialogs and KWallet are tried in a window on a display of its
  own, the bus started with that display so `kiod` and `ksecretd` draw
  there. KWallet 6.30 is `ksecretd` (wallets, the Secret Service) with
  `kwalletd6` over it: make `kdewallet` first through the Secret Service
  (`CreateCollection`, alias `default`, then its prompt), as a Plasma
  login leaves it; without one, KDE's password server asks for a wallet
  with no name and then hangs. A server's cut-off upload stays an entry
  in rclone's folder until it reads the folder again: a test that waits
  for what a refused copy left to be swept gives rclone a short
  `--dir-cache-time` (15 s), never 0.
- kio-fuse as Plasma starts it (its bus hands the activation to
  `systemd --user`, `SystemdService=kio-fuse.service`) is tried with a
  manager of the trial's own: `systemd --user --unit=<an empty target>`
  in a scope given its cgroup (`systemd-run --user --scope -p
  Delegate=yes`), every XDG folder and the runtime folder its own, its
  bus its own `dbus.service` (`<listen>systemd:</listen>`,
  `--address=systemd: --systemd-activation`; a manager joins only a bus
  it runs). Every `systemctl --user` there runs with the trial's runtime
  folder alone (`env -i`), or it reaches the user's manager.
- A server that runs as root (unfs3 with rpcbind for NFS, netatalk for
  AFP) runs in namespaces of its own, user (root mapped to the user,
  subordinate ids beside it), network, mount and pid, with a GVfs of its
  own: every process ends with them (`tools/test/gvfs-nfs-afp.sh`, which
  `just remote-check` runs). A copy of `/etc` there drops ownership: kept,
  it lands on a subordinate id the user cannot remove. unfs3 splits a
  reply over 64 KiB into fragments, which GVfs's NFS client never reads:
  a large file down from NFS is timed against NFS-Ganesha's in-memory
  backend (it keeps a file's first 2 MiB), not tested.
- A test that stops its own OpenSSH server mid-copy ends its listener
  before its sessions (sshfs reconnects at once, its new session not
  among those found), finds them in the kernel's lists of children (one
  pass over `/proc` per process let a loopback copy finish meanwhile),
  and hangs them up where rsync must keep its partial
  (`OwnSshd::stop_gently`). A file it reads back down was never sent
  through the mount: what went up may be in its cache, read whole
  whatever the server.
- A phone is written to only when someone runs `just mtp-check` with it
  plugged in (`tools/test/gvfs-mtp.sh`, over MTP, then as a camera over
  gPhoto2): a GVfs of its own in user, mount
  and pid namespaces, a folder of its own on the phone, removed after;
  never on a phone plugged in that no one offered, and never through the
  session's GVfs, which may hold it (one program at a time).
