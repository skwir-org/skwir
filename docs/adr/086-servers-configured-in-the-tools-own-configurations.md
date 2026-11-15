# ADR-086: Servers configured in the tools' own configurations, through forms

Status: accepted, 2026-10-24. Builds on ADR-077 (a remote folder is a
mount an installed tool makes) and ADR-082 (a server's password in the
keyring).

## Context

Skwir opens what the installed tools know: rclone's remotes, ssh's
hosts, the desktop's bookmarks. Until now a server had to be set up
outside Skwir first (`rclone config` in a terminal, a bookmark made by
another file manager). rclone alone keeps a list of servers of its own,
seventy kinds of storage with their options, and describes them as data
(`config/providers`) and changes them through its remote control
(`config/create`, `config/update`, `config/delete`), its questions
(a browser's sign-in) asked one at a time when asked non-interactively.

A form of several fields, some secret, had no place in Skwir: prompts
asked one line (`ui.ask`, `App::ask_secret`), and a command's arguments,
which agents and scripts write and which are traced, must never carry a
secret.

## Decision

1. **The tools' configurations are the store.** A remote added in Skwir
   is rclone's, in rclone's own configuration file, where `rclone` in a
   terminal finds it too; Skwir keeps no copy, no list, no secret of it
   (but for the cache of what rclone last printed, Amended 2026-11-09,
   below). Its name is one its address holds (`rclone://name/`: ASCII letters,
   digits, `_ - . +`).
2. **Changes go through rclone's remote control on a private socket.**
   A short-lived `rclone rcd` serves it on a socket of the user's
   runtime folder (as a mount's does for copies, ADR-077) for as long as
   a form needs it; a secret travels in a request's body, never on a
   command line, which every user of the machine can read. A session a
   Skwir gone left serving is stopped by the next one.
3. **Forms the plugin describes, the window draws.** `skwir_core::form`
   describes fields (text, numbers, secrets, switches, choices among
   suggestions, a pick from a long list, a link); the plugin waits for
   the answer on a worker (`App::fill`), and the window, which names no
   plugin, shows one form at a time and answers the asker alone. A form
   may wait on its asker (rclone waiting for a browser's sign-in) with
   Cancel alone. rclone's forms are made from what it describes: a new
   kind of storage rclone adds is offered with no work in Skwir. A
   secret is masked, never shown again (an edit says one is kept and
   takes a new one only when typed); options left at rclone's default
   are not written; advanced options fold.
4. **A remote is listed once it is made or changed.** Where rclone
   cannot list it, the user keeps it, goes back and changes it, or drops
   a new one.
5. **rclone's own questions are its own.** They are asked one form each,
   as rclone puts them; a browser's sign-in is opened by rclone itself on
   this machine, the form waiting with the page to open again, Cancel
   stopping it (`config/oauthstop`). Skwir answers two questions itself:
   that the browser is here (`config_is_local`), and whether to sign in
   again (`config_refresh_token`: no for an edit, yes for Sign In Again).
6. **Every surface runs the same commands.** `remote.add_server`,
   `edit_server`, `test_server`, `reconnect`, `remove_server`,
   `encrypt_rclone_config`: from the Network section's heading menu (Add
   Server…, items a group's owners register for it), each remote's menu,
   the palette, the list of rclone's remotes in Settings (records whose
   owner keeps them elsewhere, `RecordSource`, never saved as settings),
   an agent or a script.
7. **An agent proposes; the user fills in the secrets.** A caller may
   start a form with a kind, a provider, a name and options; it is shown
   for the user to review and complete, and nothing is written until they
   accept it. A secret given as an argument is refused: only the user types
   one, in the window.
8. **An encrypted configuration, its password in the keyring.** rclone
   keeps its remotes' secrets only obscured; once a remote is added to a
   configuration that is not encrypted, encrypting it is offered: with a
   password typed twice, kept in the keyring when the user asks
   (ADR-082's keyring, `rclone:configuration`), then given to rclone
   through its password command, never a command line. Skwir opens the
   configuration with the keyring's password (mounts, listings, forms),
   else asks the window; a listing nobody asked for never asks.

9. **What a storage can do is what rclone says.** Each rclone mount
   Skwir made is asked once through its remote control
   (`operations/fsinfo`): no size shown where its storage cannot say one
   (rclone's mount makes one up), Copy Public Link where it makes one,
   and where it empties a trash of its own and its options keep one
   (Google Drive's `use_trash`…), a deletion's question and toast say
   where it goes rather than that it is gone for good. Never decided by
   a kind's name alone.
10. **`s3://` addresses keep nothing.** An S3 bucket opens through the
    user's own AWS configuration (credentials, region, endpoint, a
    profile named in the address), by an rclone remote made of the
    address's parts (`env_auth`).
11. **One Add Server for every kind.** A server added by its address
    (SFTP, SMB, WebDAV, FTP, NFS, AFP) is kept under the name given, its
    password asked by its first connection (ADR-082), never by the form;
    rclone's kinds follow in the same list. (Amended 2026-10-24, below:
    the first version kept it as a bookmark.)
12. **Mount options are Skwir's data, given at connection.** Read only,
    rclone's file cache and bandwidth, sshfs's compression: kept by the
    server's address in the plugin's data, never in a tool's
    configuration; GVfs and KIO take none.
13. **On Windows and macOS.** rclone's remote control is a socket of the
    runtime folder on Unix and, on Windows, a port of the loopback behind
    a login made for it, given to rclone in its environment alone (Wine,
    standing in for Windows here, has no Unix socket). **Amended
    2026-11-14:** the port is rclone's choice, read back from what it says
    as it serves: one Skwir chose and let go of could be another program's
    by then, given the login and the configuration's secrets. rclone mounts
    through WinFsp in a folder of Skwir's local application data, never
    a drive letter, and on macOS without a FUSE through `nfsmount`.

## Consequences

- Skwir stays stateless: removing it leaves rclone's remotes as they
  are, usable from a terminal.
- `rclone` is needed to configure, as it is to mount; the forms follow
  the installed rclone's release.
- Remotes whose names an address cannot hold (spaces, `@`, other
  alphabets) remain rclone's but cannot be opened by an address in Skwir.
- The form is the core's second way to ask the user (beside one line);
  any plugin may describe one.
- Tests run against servers, configurations, keyrings and a browser of
  their own (`RCLONE_CONFIG` and a fake `xdg-open`): nothing is signed in
  to a real provider here, so a sign-in is checked up to the page rclone
  serves. A cloud's trash is named by what rclone and the remote's
  options say, never seen filling here. WinFsp mounts and macOS are
  compiled only; rclone's Windows release is tried under Wine.
- Skwir writes a tool's configuration only through the tool: rclone's
  file never by hand, the desktop's bookmarks through the places plugin's
  commands. KDE's network folders, one desktop entry each, it writes as
  KDE does (amended 2026-10-24).

## Amendments

### 2026-10-24: servers in the Network section, bookmarks the user's

A server added by its address was kept as a bookmark: in the sidebar's
Bookmarks, while a remote of rclone's added from the same list stood in
the Network section. The user asked for the servers in the Network
section, and Bookmarks kept for what they bookmarked, local or remote.

- **Where a server added by its address is kept.** In Skwir's own list
  (`remote.kept_servers`, the plugin's data), or in KDE's network folders
  (a desktop entry of `$XDG_DATA_HOME/remoteview`, Dolphin's `remote:/`,
  written as KDE's Add Network Folder writes one, through the platform's
  `file::rewrite`), as `remote.servers_kept_in` says: Automatic is KDE's
  on KDE's desktop, where KIO opens the kind, Skwir's elsewhere; the
  user may choose either. A kind KIO does not open (FTPS, AFP) is always
  Skwir's. Both are read whatever is chosen, and each server is edited
  and removed where it is kept, KDE's network folders too, so Skwir may
  be the user's file manager on KDE.
- **The Network section is servers; Bookmarks are the user's.** Add
  Server never writes a bookmark. A remote folder the user bookmarks
  (Ctrl+D) stays a bookmark. A remote bookmark (another file manager's,
  or one kept before) is made a server by **Keep as Server** in its
  menu: kept as a server, its bookmark taken out. Dolphin's remote places
  and fstab's network entries still stand in the Network section,
  changed by their programs.
- **Settings lists every server** (`remote.servers`, Network page), each
  where it is kept, with what its menu does: Edit and Remove where Skwir
  changes it (rclone's configuration, its list, KDE's network folders),
  Test for rclone's remotes, Mount Options, Connect at Login, Set Up Key
  Login, Sign In Again.
- **Removing a server takes along what Skwir kept for it**: its mount
  options and its entry connecting it at login, where no other server
  kept shares its top. An address changed in Edit takes its mount
  options along; its entry at login, made for the old address, is taken
  out and said so.
- Not migrated: adding by address was a day old and unreleased, so only
  tests had kept a server as a bookmark.

### 2026-11-09: what rclone listed is cached between starts

Every start ran rclone twice, `config file` and `listremotes --long`:
about half of the user-space CPU a start took where rclone is installed
(measured with `perf` on one machine, on workers, so no later first
frame). What a run knows was kept in memory only.

- What `listremotes --long` printed (names and types, no secret) is
  kept in the deletable cache between starts, with the path rclone gave and the stamps of its configuration file and its
  program. It is used only while both stamps are those it was read with;
  what no longer holds is deleted when found. F5 asks rclone.
- **It is not a store**: rclone's file stays the only place a remote is
  kept. Nothing goes in the cache but what rclone said, and deleting it
  costs only those two runs. A new configuration placed where rclone
  looks first, with the old one unchanged, is seen at F5.
- **Nothing is kept of an encrypted configuration**, whose names the user
  chose not to leave in the clear, nor where the environment names
  remotes of its own (`RCLONE_CONFIG_NAS_TYPE`), which no file's stamp
  tells of. Those are asked at each start, as before.

### 2026-11-15: what the Network section shows is followed, whoever changes it

The user saw the Network section stay as it was while another program
changed what it lists, where Dolphin's sidebar follows. Tried in a
window of its own: a remote added by `rclone config create` from a
terminal, a network folder written as KDE's Add Network Folder writes
one, and a folder mounted with `rclone mount`, none shown after 6 s, and
the mount, unmounted, still shown; all three shown at once on Refresh
remote folders. The section was read at a start, after an action of
Skwir's, on that refresh, and on GVfs's and kmtpd's signals: nothing
followed the files the servers are read from, nor the table of mounts.

- **A reader of servers says where it read them** (`connector::Named`:
  rclone's configuration where rclone placed it, Dolphin's
  `user-places.xbel`, fstab; `servers::sources`: KDE's network folders),
  one never without the other, and the plugin follows that while it is
  enabled (`sources`): the folder holding a file, or the nearest folder
  above that is there while it is not (`rclone config` making its
  folder), and each link on the way there in its own folder (a dotfile
  manager's; NixOS's `/etc/fstab`, through `/etc/static`, which a
  switch re-points), but no folder on a volume mounted read-only (the
  Nix store: nothing there is replaced, what leads there is). A
  change by any program settles (400 ms, at most 2 s while changes go
  on), then the servers are read
  again, which follows what they then name.
- **The table of mounts is followed** (`reach::follow`, ADR-077
  amended): a mount made or let go of by any program is read again.
- **What the system tells is shown only where the section changed**,
  and asks no server: a container's mounts cost a read in memory, an
  unchanged rclone configuration no run of rclone (its stamp,
  2026-11-09). A mount come since is asked whether it answers and how
  full it is only while someone looks, else as the window comes back;
  no bridge (GVfs's, kio-fuse's) is listed, a change of the table never
  changing what one holds. A refresh asked that a told read took the
  place of hands it what it owed (all shown, the mounts known asked).
- **What names no server is never looked up** (`Address::server_name`:
  a device, a remote of rclone's, an S3 bucket). Found by the remote
  tests, run at once in one table of mounts, every test's app now
  following the others' mounts: an S3 bucket's name was resolved as a
  host (a single label, LLMNR and mDNS waited on), holding a worker of
  the system's name service each time, and a user's name took up to ten
  seconds to look up for every program on the machine (`rclone serve`
  starting in ten seconds, a test's question late). The code before had
  it too, each app looking a bucket up once (stalls of seven seconds);
  with the fix, none over 71 ms.
- **What no watch tells is read again as a window comes to the front**
  (`Plugin::on_looked_at`): a folder kio-fuse holds more of, a share
  another program connected, the servers where a file could not be
  watched; one such read at a time, once in ten seconds, as the
  folders shown are read again.
- **KDE is told what Skwir changes in its network folders** as KDE's own
  Add Network Folder (`knetattach`) tells it: `KDirNotify`'s
  `FilesAdded` of `remote:/` on the session bus, after an entry is
  written, changed or removed. KIO's watcher of that folder says the same
  for any change, but runs in `kded6`, which only Plasma starts: outside
  it, an open Dolphin showing Network lists it again on that signal
  alone.
- Not followed: Skwir's own list, the plugin's data, changed through
  Skwir; servers a tool's environment names (`RCLONE_CONFIG_NAS_TYPE`),
  which no file tells of.
