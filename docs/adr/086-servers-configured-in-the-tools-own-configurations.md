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
   terminal finds it too; Skwir keeps no copy, no list, no secret of it.
   Its name is one its address holds (`rclone://name/`: ASCII letters,
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
    (SFTP, SMB, WebDAV, FTP, NFS, AFP) is kept as a bookmark in the
    desktop's shared bookmarks under the name given, its password asked
    by its first connection (ADR-082), never by the form; rclone's kinds
    follow in the same list. KDE's network folders are known servers.
12. **Mount options are Skwir's data, given at connection.** Read only,
    rclone's file cache and bandwidth, sshfs's compression: kept by the
    server's address in the plugin's data, never in a tool's
    configuration; GVfs and KIO take none.
13. **On Windows and macOS.** rclone's remote control is a socket of the
    runtime folder on Unix and, on Windows, a port of the loopback behind
    a login made for it, given to rclone in its environment alone (Wine,
    standing in for Windows here, has no Unix socket). rclone mounts
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
  commands.
