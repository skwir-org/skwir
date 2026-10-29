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
  `Scheduler::spawn_in(reach::lane(path), …)`; in the UI,
  `Shell::off_thread(lane, …)`, `off_thread_until` and
  `latest::Latest::ask` take the lane first, with no default, so every
  caller names one: `reach::lane(path)`, `shell::lane_of(paths)` (the
  first's), `Shell::pane_lane(pane)` (what the pane shows), or
  `Lane::SHARED` for work that reads nothing a remote holds. A lane runs at most `LANE_RUNS` jobs at once, and always one
  fewer than the pool's workers; the rest wait their turn without
  holding a worker (cancelled ones are let go of as more park, and with
  the pool). A remote that stops answering holds its own jobs, never the
  window, local work, or another remote's.
- Work on files from many places (the shelf, the recent files, what a
  plan touched, which the tags index learns) is split by lane
  (`reach::by_lane`), a job each: a server that stops answering holds its own files' job, never
  the others'. What it ranks without reading (Jump's recent files) is
  not read there at all.
- A worker busy past `STUCK` while work waits is replaced, up to the
  pool's size again, whether or not more work is asked after (a look is
  planned `STUCK` later): a call that never returns costs a thread, not
  the pool.
- A job that serves one request after another (a remote's thumbnails)
  says each begun with `scheduler::progress`: its lane is not taken for
  stalled however long the job runs.
- A question that must be answered whether or not the server answers
  (whether an agent may be handed a path: `permissions::Checker`) asks
  it in the remote's lane and waits `reach::NOT_ANSWERING` at most; what
  no answer came for is taken the safe way (private), never as asked.
- Work that reads nothing of a remote (closing a folder: its watch, its
  descriptor) runs in the shared lane, never behind a held one.
- A remote whose lane has run a job for `reach::NOT_ANSWERING` (three seconds)
  while its folder is listed is said not to answer in the pane's banner
  (`Scheduler::stalled`, `ui/loading.rs`), looked at again each second
  while it is listed, never once it is. The banner offers to leave it (Go
  Back, else Go Home) and to let go of the server (Disconnect, its place's
  Eject); an address still connecting, to cancel.
- No lock shared beyond one remote is held across a call that reaches a
  path (`inotify_add_watch`, `statvfs`, `open`): take what is needed,
  let go, call, then take the lock again. What such a call may race with
  is settled after it, never by holding on to things while it runs (the
  watcher adds again a descriptor let go of during its add; it keeps no
  orphan).

## What differs on a network

- Search root opening, traversal, operation rechecks and hydration run in
  the root's remote lane. Keep a lane learned from the opened root's
  device for aliases, and move traversal there before reading descendants.
  A walk of a remote's tree (a search's, a tag count's) runs on its job's
  own thread, a folder at a time as a listing reads one: never threads of
  the walker's own, which would ask the server as many at once as the
  machine has cores, outside the lane, and be held by it unstood-in-for
  (`a_server_that_stops_answering_holds_no_more_than_its_lane`).

- Tool arguments use the receiving program's grammar. Rsync's `-e`
  command doubles an embedded matching quote and treats backslashes
  literally; it is not shell quoting. Test serialization through rsync
  with a recording SSH stand-in, without contacting a server.

- **Thumbnails** follow the remote's rule (`Registry::remotes`,
  `App::shows_thumbnail`): Never, Small files (`Thumbnails::SMALL`),
  Always, or Automatic (Always on the local network, Never otherwise or
  when unsure; Small files on the local network where its program reads
  a file whole before any part of it, kio-fuse over WebDAV, FTP and NFS:
  `Connector::reads_whole`; Small files elsewhere once copies down this
  session proved the server fast: `fs::reach::note_read`, 10 MB/s over
  4 MiB or more, slow again under 5, by the mount, in memory; a preview
  is not counted, as it reads what it shows, not the file);
  Never where no plugin knows the remote. Ask it before
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
- **Walks** (searches, counts, the timeline's subtree) never enter a
  network mount below their root (`reach::is_point`, told before the
  point is stat-ed, which its server answers), nor answer what an index
  learned there (the points below the root, from `reach::all`): a
  search's hints and plocate's answer, the tags index's counts and tag
  locations leave out what a pane shown there taught them. Usage asks
  before measuring a remote.
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
  listed among the sidebar's devices; they keep a lane of their own. A
  camera moves nothing between its folders: copies there are written in
  place, never staged (ADR-083, operations.md).
- **A device has an address** (`mtp://`, `gphoto2://`, `afc://`,
  `Scheme::device`, ADR-096), GVfs's name for it, which GVfs alone opens
  and Eject lets go of. It is no server: its name (a serial) is never
  looked up on a network (`lan`), nothing of it is kept as a server's
  (passwords, mount options, login), and the user sees its volume's name
  (`Remotes::label_of`), never the address's.
- **What GVfs offers is listed before it is mounted** (ADR-096): its
  volume monitors on the session bus, asked and followed
  (`Connector::device_signals`, `devices`: one listener for every tool,
  on the program's one session connection, listening again on the next
  once the session's bus is started again (`bus::keep`), every tool
  listed again then; each tool listed again on a thread of its own, a
  burst asked once more after; what a tool asks to
  list them, on a connection of its own closed once answered, as zbus
  gives up on a call only by its connection's timeout: `remote::bus`),
  each device the same
  place mounted or not (`remote:<address>`). A device is mounted once
  opened, never because it was plugged in: mounted, it is held from
  every other program. When it does not open, the programs holding its
  node are named (`holders`), GVfs's own for that kind left out.
- **A phone KDE's kmtpd holds** (ADR-097) opens through it: the `kmtp`
  connector, tried before GVfs, answers only for a phone kmtpd holds
  (`Connector::opens`), asked only where `kiod6` owns its name and kmtpd
  is loaded (what `/modules` lists, which loads nothing), never loading
  it, except on KDE's desktop. Its helper (`skwir-kmtp-worker`) serves
  kmtpd's interface as a FUSE folder of Skwir's (`fuse.skwir-kmtp`): one
  call at a time, a file fetched whole into an unnamed file of the cache
  and sent whole at its close, a replaced file removed first, a move
  between folders refused (copies written in place, ADR-083), times and
  modes refused. Eject lets go of Skwir's mount alone: the phone stays
  KDE's. The phone keeps GVfs's address and place, its device found by
  Solid's `udi` in sysfs and udev (`Kmtp::of_udi`).
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
  one; of AFP and an iPhone's AFC the same up, within and removed, but
  down through its bridge, sixteen files at once, which was faster (AFP)
  or asked the device fewer requests (AFC: GVfs sends them one at a
  time, a USB round trip each on an iPhone; counted, not timed)
  (`gio_copy::Ways`).
  A file alone goes by `gio` where it was measured faster than the
  bridge, which reads 128 KiB at a time (down from NFS and AFP, both
  ways on MTP, down from AFC in fewer requests), and up to a camera,
  whose bridge sends a file after its `close` returned (a refusal there
  never told); by its step elsewhere. For a server of kio-fuse's of
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
  mount; by `gio copy` within a GVfs mount (its daemon copies); by
  rclone itself within one rclone mount Skwir made, where its backend
  copies on the server (`rclone_copy`: the mount's remote control on a
  socket of the runtime folder, `--rc-addr unix://…`, the user's alone; a
  file by `operations/copyfile`, a folder by `operations/mkdir` and
  per-file copies, then `vfs/refresh` so the mount shows them; whether it
  copies there asked once a mount, by `operations/fsinfo`). Recursive
  children, refresh and placement heed pause and cancellation. A
  synchronous RC connection belongs to the plan's cancellation: cancel
  shuts it down, interrupting a stalled response and the server's request
  context. Only acknowledged copies report bytes. A cancelled capability
  probe says nothing about backend support and remains retryable. A lone
  file within one remote goes in a batch where a transfer serves it,
  which its step would bring here and send back. A server a tool does
  not reach is remembered for its mount. rsync keeps a file it was
  writing when cut short in the staging folder (`--partial-dir`, an
  absolute path on the receiving side, else none), which its retry goes
  on from (ADR-080, 7). Skwir's own batch and sftp's go on from the file
  they were writing (`Transfer::goes_on`, `Aside::goes_on`, sftp's
  `reput`): its source noted as the batch laid it out (`Layout::made`),
  still so, and its last part read back as the source's; a transfer that
  makes each file anew finds none there. sshfs keeps what it read of a
  file cut short (its length, read back and cut through the mount) after
  `reput` appended beside it, and carries it to the name its folder is
  placed at: sftp gives each such file its time through the mount after,
  which makes sshfs let go (`give_times`); read before, a later copy of
  it noted its source at the length cut short.
- A plan a server stopped goes on by itself once that server answers
  again (ADR-080, 9; operations.md): its mount asked in its lane, one
  question at a time (`fs::reach::answers`), never read from the error.
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

- `sftp:`, `smb:`, `dav:`, `davs:`, `ftp:`, `ftps:`, `nfs:`, `afp:`,
  `rclone:` and `s3:` locations are addresses (`remote::address::Address`): opening
  one connects it on a thread of its own and sends
  `ListingEvent::Redirect` with the mounted folder, which replaces the
  address in the pane's history, only while the pane still shows the
  address (`workspace::Redirect::from`). Never a password in an address.
- Connections to one address's top take turns (`Remotes::reach`): the
  second finds the first's mount. A mount at the point Skwir would make
  is let go of only when it answers "not connected"; a live one is never
  unmounted under what uses it.
- An address's top is what one mount of it holds (`Address::top`): the
  server for SFTP, FTP and rclone's remotes, a bucket for S3, a share
  for SMB, a volume for AFP. An NFS export may be of several parts
  (`/srv/photos`), and a WebDAV share is where its server serves it
  (`/remote.php/dav/files/me`, the server's root often no share): only
  the server says, so such an address keeps its whole path, a folder,
  and the connector says the export or share from what it mounted
  (`Connected::address`: GVfs's mount spec, whose WebDAV prefix is the
  collection the server serves the path in). An address's path holds no
  `.` or `..` (`Address::parse`), and what it names below a mount is
  joined to its point name by name, never above it.
- A remote folder is named by its address where the user reads or keeps
  it: its Network place roots its trail; a path said to the user is
  written as its address (`remotes::address_below`, the deletion's
  question); tabs keep it as its address (`App::address_of`), so a later
  start connects again; the folder remembered for the next start is
  never a remote's.
- A remote folder bookmarked is kept by its address in the desktop's
  shared bookmarks (`BookmarkEdit::AddAddress`, `uri::address_uri`), as
  other file managers keep theirs, never by its mount's path. It is one
  of the bookmarks: renamed and moved among the folders' by the same
  commands (`places.rename_bookmark`, `places.move_bookmark`), named by
  `Marked::Address`, in the file's order.
- An `s3://bucket/path` address (`s3://profile@bucket/` for an AWS
  profile) opens through the user's own AWS configuration (credentials,
  region and endpoint, as the AWS tools read them), by an rclone remote
  made of the address's parts (`env_auth`): nothing is kept of it, in
  rclone's configuration or Skwir's (ADR-086). Tested against `rclone
  serve s3` with AWS files of the test's own.
- An agent's call connects only once the user allowed it.
- The address field completes the server being typed (`sftp://me@ho`)
  with those known: mounted, the tools', the remote bookmarks, and for
  SFTP the `Host` entries of the user's ssh configuration. A folder after
  the server is not completed: the server would be asked.

## Servers nearby (ADR-084)

- The Nearby place (`nearby:`, `remote::nearby`) lists the servers
  announced on the local network while a pane shows it, and browses
  nothing otherwise: let go of, Avahi's browsers are freed and its
  connection closed, the wsdd Skwir started stopped. Through Avahi's
  browsers on the system bus and wsdd (its daemon's `/run/wsdd.socket`,
  else `wsdd --no-host --discovery` on a socket of the runtime folder);
  never a discovery protocol of Skwir's own. The wsdd Skwir starts ends
  with Skwir, whatever ends it: the system kills it once the browser's
  thread that started it ends (`process::ends_with_starter`, Linux's
  parent-death signal, which comes when that thread ends, not the
  process: that thread waits on it and stops it before ending). What a
  Skwir gone left (the socket of a wsdd killed so, a wsdd still listening
  as one before that left it, killed: told to end, wsdd takes seconds to
  say goodbye) is swept as the next starts one (`nearby::sweep_left`,
  only once the system says its Skwir ended). Avahi's signals are read on
  a thread of their own into a queue of no bound, each resolved on
  another: zbus reads nothing while a match's queue (64) is full, so the
  browsers' first burst held every answer behind it (a test announces
  more than 64). Each server is a place
  (`EntryId::place`) leading to its address, one row however often it is
  seen, carrying the name it announced (`nearby::ANNOUNCED`). One
  selected, where no server holds its address, its context menu adds it
  (`remote.nearby_menu`: Add as Server…, `remote.add_server` given its
  kind and its form's parts, its name the announced one), as Add Server
  would with or without rclone.
- Tested on a network of the test's own (`tools/test/nearby.sh`): user,
  network, mount and pid namespaces, a veth pair to a namespace standing
  for a PC, a system bus of its own with Avahi on it (its user mapped to
  root through a passwd of the namespace's), wsdd announcing the PC with
  a UUID of its own. Two wsdd on one interface never hear each other.
  Never the user's Avahi, bus or network.

## Servers' configurations (ADR-086)

- The tools' own configurations are the store: a remote added in Skwir
  is rclone's, in rclone's configuration, which Skwir changes only
  through rclone's remote control (`rclone_config::Session`, a `rclone
  rcd` on a socket of the runtime folder, its own for the form's
  while), never by writing the file. Skwir keeps no list or secret of
  it. A session a Skwir gone left serving is stopped by the next one
  (`config-<pid>-<n>.sock`).
- A secret goes to rclone in a request's body, a password command's
  answer or its environment, never a command line. It reaches the plugin
  only from the window's form (`App::fill`): a command's arguments never
  carry one, nor an advanced option, some of which have rclone run a
  program (`rclone_forms::Prefill::refusal`), a form never shows one
  kept (`Kind::Secret { kept }`), and one typed again is the only one
  written; one typed in a form refused is kept for that form asked again,
  in memory, shown as kept.
- Forms are made from what rclone describes (`config/providers`), never
  from a list of kinds in Skwir; what a form leaves at rclone's default
  is not written. rclone's own questions are asked as it puts them; the
  plugin answers only that the browser is here and whether to sign in
  again (`config_is_local`, `config_refresh_token`).
- A remote of rclone's is named as rclone names one
  (`address::rclone_name`: letters and digits of any alphabet, `_ - . +
  @` and spaces, starting with neither `-` nor a space, ending with no
  space; `config create`'s own rule), Skwir's Add Server's too. Its
  address holds the name escaped (`rclone://my%20drive/`,
  `Address::rclone`), reading back to it: an `@` there is the name's, an
  rclone remote having no user. Built from a name, never from text with
  the name in it.
- rclone's remote control is reached through `rc::At`: a socket of the
  runtime folder on Unix (no login: no other user reaches it), the
  loopback on Windows behind a login made for each session or mount,
  given to rclone in its environment alone, never on its command line or
  in a log (`At`'s `Debug` says the port alone); a logon task's rclone,
  given no environment, reads the login's SHA-1 from an htpasswd file
  kept with it (`rc::Login::htpasswd`). On Windows rclone mounts
  through WinFsp in a folder of Skwir's (`winfsp`), its login kept beside
  it while it lives, and lets go when asked to quit.
- Key login (`keys`, `keys_form`, ADR-086): the keys offered are the
  agent's, then ssh's default ones it does not hold, then a new one made
  in the user's `~/.ssh` (never over one there), its passphrase given to
  ssh-keygen through the askpass, never its command line. Only public
  halves leave the machine, over one login (a control master), through
  the server's shell or, where it runs none, sftp. Windows' ssh makes no
  master: there each ssh and sftp logs in alone, the server's password
  the first login was let in with given to the sftp sessions after
  (`Typed`, never asked again), and whether ssh logged in is read from
  how it ended (`ran`: its command's status, 255 its own failure, an
  exec request refused counting as in). Tested on Linux too, each login
  alone (`Ssh::alone`), against rclone's server. Tested against an
  OpenSSH server run as the user (its home and commands the test's, by a
  `ForceCommand`) and rclone's SFTP server (a password, no shell), with
  an ssh agent and keys of the test's own: never the user's agent,
  `~/.ssh`, configuration or known hosts.
- Connect at login (`login`, ADR-087): the service manager's entry runs
  Skwir's own mount command at Skwir's own point; what needs an answer at
  login is refused. Turned off while it carries a mount, its file is gone
  and its manager forgets the unit's own commands (systemd reloads it as
  not found: no clean-up once it ends, no restart): the mount is then as
  one Skwir made, Eject letting go of it and its point, the next mount
  there of what it leaves otherwise (`mounter::point_for`, rclone's
  socket). Nothing relies on a unit's commands once it is turned off.
  On Windows rclone's logon task serves its remote control on a port it
  chooses, read back from its log, behind a login made with the task and
  kept beside the mounts while the task is (`winfsp::keep_at_login`,
  rclone checking its SHA-1 in an htpasswd file); Eject forgets it once
  no task is there.
  Tests read units with `systemd-analyze verify` in
  their own folders, run a unit's command as it would be run, and install
  through a stand-in `systemctl` given a runtime folder of its own: never
  the user's manager, and never a manager of the test's own started from
  a session's cgroup (systemd moves that cgroup's processes into its
  `init.scope`). The one check under the user's own manager is theirs to
  run (`tools/test/login-user-systemd.sh`: a test ignored by default,
  `connect_at_login_under_the_user_s_own_systemd`, that does nothing
  unless the script names the manager's unit and runtime folders); no
  recipe and no agent runs it.
- Mount options (read only, rclone's file cache and bandwidth, sshfs's
  compression) are the plugin's data by the server's address
  (`remote.options`, `options::MountOptions`), never a tool's
  configuration, given when it next connects (`Connector::connect_with`),
  after Skwir's own arguments so they win, its entry at login too, which
  is written again when they change (`login::Manager::renew`: nothing
  started or stopped). GVfs and KIO take none.
- One Add Server for every kind: a server added by its address (SFTP,
  SMB, WebDAV, FTP, NFS, AFP: `servers_form`) is kept under the name
  given (`servers`, ADR-086 amended), then opened; its password is asked
  by that first connection (with the window's Remember, ADR-082), never
  by the form. Without rclone, Add Server offers these alone.
- **The Network section is servers; Bookmarks are the user's.** A server
  added by its address is kept in Skwir's list (`remote.kept_servers`)
  or KDE's network folders (`$XDG_DATA_HOME/remoteview/*.desktop`,
  written as KDE writes them, through `file::rewrite`, and read back
  through KIO's own `remote:/` in `remote-check`:
  `kde_reads_the_network_folders_skwir_writes`), as
  `remote.servers_kept_in` says, and never as a bookmark. Both are read
  whatever is chosen and whatever connector is installed, and each is
  edited and removed where it is kept. Dolphin's remote places and
  fstab's entries stand there too, changed by their programs (`Kept`).
  A remote folder bookmarked stays a bookmark until Keep as Server
  (its place's menu, a `place` menu source) makes it a server and takes
  its bookmark out.
- Settings' list of servers (`remote.servers`) is every server the
  Network section knows, as `remote.server_records` answers from memory:
  each line says where it is kept, and does what its menu does
  (`actions`); Edit and Remove are left out where another program
  changes it (`fixed`). A remote of rclone's mounted is let go of before
  it is removed, and kept while it stays mounted. Removing a server takes
  its mount options and its entry at login along, where no other server
  kept shares its top, and
  the password Skwir keeps for it in the keyring, where no other server
  needs it (the same server, as the same user: the servers kept by
  address read as they are then, never the state a refresh has yet to
  bring), its confirmation saying so. A line forgets that password as
  its menu does (Forget Password). Settings is told when one is kept or
  forgotten (`Remotes::note_remembered`).
- A remote of rclone's Skwir began to set up is noted
  (`rclone_forms::UNFINISHED`) before rclone is asked to make it, saved at
  once, and forgotten once its setup ends: done, cancelled (the remote
  removed), or removed later. One still noted is half made (Skwir ended
  in the middle of its questions): its line in Settings and its place say
  so (`NOT_FINISHED`, the place drawn as not ready), its line and menu
  offer Finish Setting Up… (Edit, rclone's questions again) and Remove;
  Skwir never removes it unasked.
- The configuration's password: the keyring's (`rclone:configuration`)
  where the user kept it, else asked of the window (`ConfigAsk`, through
  rclone's `--password-command`, Skwir's path one word in CSV's quotes,
  as rclone splits it); never asked for a listing nobody asked for. It is
  not encrypted while a remote of it connects at login, which nobody
  would be there to open it for.
- Tested with configurations of the tests' own: nextest gives every test
  `RCLONE_CONFIG`, `SSH_AUTH_SOCK` and AWS's files of the tests' own
  (`tools/test/isolate-xdg.sh`), and a test names its own configuration
  (`Rclone { config }`). A sign-in is tried with a browser of the test's
  own first on the `PATH` (it notes the page, opens nothing): never a real
  provider, never the user's browser.

## Connectors

- One trait (`remote::connector::Connector`): connect an address's top
  to a mount, recognise mounts of its tool, disconnect, list the servers
  its configuration names. The order tried is `remote.connectors`; a
  mount already there is used first. A connector that fails without
  asking anything (GVfs finding no NFS export holding the path, fstab
  naming none) leaves the address to the next one installed; one that
  asked the user speaks for the attempt, which no other asks again.
  Failures are said together, each by its tool. Adding a protocol is a
  scheme a connector maps, or a connector.
- GVfs is the session's: its bridge is `$XDG_RUNTIME_DIR/gvfs` alone
  (`Gvfs::bridge`), never another the mount table lists (another
  user's, or the user's own while a window runs with a runtime folder of
  its own); a mount of another bridge is neither recognised nor
  connected through. It is there where its daemon runs or the session
  bus would start it (`org.gtk.vfs.Daemon`): `gio mount` starts it, which
  mounts the bridge, waited for a moment.
- The system's own shares, off Linux (`system_shares`, first in the
  order there; `skwir_platform::shares`): Windows' SMB and WebDAV by
  their UNC path (`\\server\share`, `\\server@SSL@port\DavWWWRoot`),
  connected for the session with no drive letter; macOS's SMB, AFP, NFS
  and WebDAV mounted by NetFS under `/Volumes`. A password is asked once
  the server refused the login without one (Windows' saved
  credentials, the Keychain, a guest), given in the call alone.
  Compiled only.
- KIO is the session's kio-fuse (ADR-081), asked over the session bus
  (`Kio`, `mountUrl`), there where the bus has it or would start it; its
  mount is a `kio-fuse-…` folder of the session's runtime folder alone.
  Each server is a folder two below it (`webdav/nas:8080`), a remote of
  its own (`reach`'s bridges: GVfs's one deep, kio-fuse's two), its
  address read back from the two folders' names; what it holds of no
  remote (an archive KIO reads) is not shown. It mounts what it is asked:
  an address on a server it holds is asked for again
  (`Connector::mounts_each_path`). It lets go of no server: no Eject.
- Letting go of a mount (Eject) is refused while an action of Skwir's
  is under way there (`Executor::under_way_in`, by its footprint; one
  cancelled is not), naming it, before anything is left and again just
  before the unmount; `force` (an agent's, a script's) cuts it too. It
  first leaves the folders shown there (`App::release`), then waits, ten
  seconds at most, until their listings let go of them (closed a moment
  later on a worker, `fs::wait_let_go`) and the remote's lane runs
  nothing: until then the system refuses the unmount as busy. It stops
  waiting once that lane has run a job for `reach::NOT_ANSWERING`: a
  read waiting for a server that does not answer waits for the server,
  not for time. GVfs refuses too while its backend is at work (a read
  through its bridge, a folder just listed through it, closed after the
  bridge's release returned): `Gvfs::disconnect` asks again while the
  mount is still there, five seconds at most (`BUSY_PATIENCE`). It
  refuses only while a read of its runs: between two, it lets go, and
  what another program was reading there is cut.
- The system refuses to unmount a folder watched as one used (GVfs's
  trash puts a watch on the top of every mount, inotify). Refused as
  busy, `mounter::unmount` asks what other programs use the mount
  (`process::using`: a file open or mapped there, a working folder in
  it; never a watch, never a look under it; never Skwir itself, whose
  actions were asked and whose reads left there were given up on):
  none, it is let go of lazily, which ends the watches; some, Eject
  names them ("“vim” is using it"). Where the system does not say, it
  says another program uses it: Linux's `/proc` says, macOS's `lsof`
  (compiled only); Skwir mounts nothing on Windows yet. A mount fstab
  lets the user mount is let go of so too (`umount -l` where nothing
  uses it).
- A FUSE mount let go of whatever holds it (`mounter::force_unmount`)
  has its connection cut first on Linux (`/sys/fs/fuse/connections/<n>/
  abort`, the user's own for a mount of theirs; the kernel's number for
  the device, `Mounted::kernel_device`), while that number still names
  it, and only a mount at the point itself, never the bridge a folder is
  in (GVfs's holds every mount of the session): a call waiting for a
  server that does not answer then fails at once, rather than hold a
  worker and the lane (keyed by the point, which the next mount there
  takes again). macOS's `umount -f` cuts it itself. Where
  fusectl is not mounted, what waits keeps waiting for its server. The
  rclone of a mount Skwir made is then asked to quit
  (`rclone::quit`, `core/quit`, as on Windows), and waited for a moment:
  cut, it would wait for its call to the server first, minutes on end,
  and, ending, unmount its point and remove its socket's path, a mount
  made there again meanwhile losing both.
- A mount gone by the time the system is asked to let go of it was let
  go of, as asked: its program let go of it (rclone does within
  milliseconds once its connection is cut, often before Skwir's lazy
  unmount runs), or the user did. The system's refusal then ("not found
  in /etc/mtab") is no error: `mounter::unmount` and `force_unmount`
  read the mount table again before saying one (`mounter::mounted`).
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
- A mount Skwir makes is under `$XDG_RUNTIME_DIR/skwir/remote/` (on
  macOS, which has no runtime folder, the user's temporary folder; on
  Windows, Skwir's local application data: never the cache, which a start
  sweeps), rclone's remote control's sockets beside it (`rc`), named by
  `connector::point_name` so its address reads back from it: nothing else
  is kept. It outlives Skwir, as GVfs's do.
- A tool that daemonizes is run until it has mounted
  (`process::run_leaving`): cancelled, it and what it started are stopped;
  mounted, nothing it left running is touched.
- A copy onto a remote is done when the server has it (ADR-102), whatever
  the mount's cache: a copy is done once its close said so (operations.md):
  a FUSE program sends there what it held and tells there what its server
  refused; and a mount that sends a file only after it is closed
  (rclone's `writes` and `full`: `Transfer::sends_later`, asked of its
  remote control once a mount) is written out as a removable drive is:
  before the plan ends, and before a move removes its originals, it is
  asked to send what waits now (`vfs/queue-set-expiry`) and waited for
  (`Transfer::send`, `uploads::send_now`), the plan's progress saying how
  many files the server still takes. An upload tried three times fails
  the write-out: the file stays in rclone's cache, which goes on trying,
  and a move's original stays. Disconnect sends what waits first (a
  refusal keeps it connected); a forced one, or one of a server that
  stopped answering, says what stays in the cache, which the next mount
  with that cache sends. What other programs left waiting is said on the
  mount's place, asked when the window comes back (`Known::looked_at`)
  and every 2 s while some waits, never otherwise. A mount's remote
  control is reached where it answers on each system
  (`uploads::control_of`: Unix's socket, Windows' loopback behind its
  login), every call about uploads given up after 5 s.
- **rclone's cache** is the one the user chose for the server, else
  Automatic's (`options::automatic`, `Remotes::automatic_cache`, resolved
  when connecting and for the entry at login, never stored): `full` (what
  is read is read again from this disk), but `minimal` for a remote that
  encrypts or wraps others (its kind, `rclone_forms::wraps_others`:
  `full` keeps its files on this disk as read) and on the local network
  (`lan`). A cache chosen is kept, `minimal` included. rclone's own cache
  folder holds what it keeps, never Skwir's, which must stay deletable
  without loss: an hour unread, a gigabyte, never the disk's last two
  (`--vfs-cache-max-age`, `--vfs-cache-max-size`,
  `--vfs-cache-min-free-space`).
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
  Each keyring call is made on a thread of its own, the connection
  waiting for its answer or its Cancel (`Keyed::heard`): a keyring that
  asks to be unlocked waits for its own prompt, which the user may leave
  open. Tested against GNOME Keyring unlocked and locked
  (`tools/test/private-keyring.sh`, `locked-keyring.sh`: its prompt,
  GCR's, answered by xdotool on a display of its own), KeePassXC
  (`keepassxc-keyring.sh`: a database made from KeePass XML exposing its
  root group) and Wine's Credential Manager (`just test-windows`).
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
  (`skwir_platform::askpass`): on Windows a named pipe of a name no one
  guesses, made the pipe's first, its one access entry the user's, local
  clients alone, opened by the askpass as a client the server can know
  but never act as; `gio mount`'s questions are read from its
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
- A test lets go of what it mounted before its server stops, once it is
  mounted (an address opened as added connects a moment later), and only
  its own server's (`let_go_of_mounts`, by its port: the tests' runtime
  folder is shared): a mount whose server is gone holds whatever looks
  into it, the next tests included.
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
- GVfs's AFC (an iPhone's) is tested against a stand-in device
  (`tools/test/afc-standin.py`, run by `tools/test/gvfs-afc.sh`): it
  answers usbmuxd's socket (`USBMUXD_SOCKET_ADDRESS`), lockdownd without
  TLS (its `StartSession` asks none), and AFC over a folder, logging each
  request. It sends what an answer carries after a bare header, as a
  device does: libimobiledevice reads the first part in one receive. Its
  pace is no iPhone's: ways are chosen there by the requests counted.
- kio-fuse as Plasma starts it (its bus hands the activation to
  `systemd --user`, `SystemdService=kio-fuse.service`) is tried with a
  manager of the trial's own: `systemd --user --unit=<an empty target>`
  in a scope given its cgroup (`systemd-run --user --scope -p
  Delegate=yes`), every XDG folder and the runtime folder its own, its
  bus its own `dbus.service` (`<listen>systemd:</listen>`,
  `--address=systemd: --systemd-activation`; a manager joins only a bus
  it runs). Every `systemctl --user` there runs with the trial's runtime
  folder alone (`env -i`), or it reaches the user's manager. A whole
  Plasma session runs so too, nested on Xvfb: `startplasma-wayland`
  (`systemdBoot=false` in the trial's `startkderc`), the bus's
  `standard_session_servicedirs` over KDE's packages alone, KWin's X11
  backend on Mesa (`__EGL_VENDOR_LIBRARY_FILENAMES` naming Mesa's: the
  NVIDIA driver draws nothing on Xvfb). Keys typed through the nested KWin
  arrive as others: give Skwir its address as its argument.
- A server that runs as root (unfs3 with rpcbind for NFS, netatalk for
  AFP) runs in namespaces of its own, user (root mapped to the user,
  subordinate ids beside it), network, mount and pid, with a GVfs of its
  own: every process ends with them (`tools/test/gvfs-nfs-afp.sh`, which
  `just remote-check` runs). A copy of `/etc` there drops ownership: kept,
  it lands on a subordinate id the user cannot remove. unfs3 splits a
  reply over 64 KiB into fragments, which GVfs's NFS client never reads:
  a large file down from NFS is timed against NFS-Ganesha's in-memory
  backend (it keeps a file's first 2 MiB), not tested.
- GVfs's AFP backend (1.60.3, and its main branch on 2026-10-23) dies
  now and then. It frees a request whose reply was read before its last
  write called back (SIGSEGV in `write_command_cb`), which a server on the
  same machine answers fast enough to do. It also changes its request
  hash from two threads, one without the lock (SIGABRT in
  `g_hash_table_remove`). Its mount goes with it, so a copy in flight
  fails with an I/O error, or says a folder it has just made is not one.
  The harness runs GVfs built with the fix for both
  (`tools/test/gvfs-afp-request-lifetime.patch`) until upstream has it.
  When a test through GVfs's bridge fails, look first in `coredumpctl` for
  a backend that died.
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
