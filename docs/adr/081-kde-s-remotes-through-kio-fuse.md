# ADR-081: KDE's remotes through kio-fuse

Status: accepted, 2026-10-22, the user's choice after an exploration
("Do you think we could support KIO as well, as an alternative backend
when GVFS is not installed and KIO is, or that the desktop env is KDE?",
then "go ahead with 1, 2 and 3": a copy is done once its close says so,
GVfs started where installed but not running, a KIO connector). Builds on
ADR-077 (a remote folder is a mount Skwir knows is remote), amends its
seventh decision for KIO alone.

## Context

- KIO is KDE's library of remotes (SMB, WebDAV, FTP, SFTP, NFS, an
  iPhone's AFC, a phone's MTP), C++ and Qt, used from inside a program:
  reaching it otherwise is a helper linked to Qt and KDE Frameworks,
  which ADR-031 refused for thumbnails, or Skwir speaking protocols,
  which ADR-077 put off.
- kio-fuse (KDE, 5.1.1) is KIO's FUSE bridge, as gvfsd-fuse is GVfs's:
  a daemon of the session, started on the session bus when asked
  (`org.kde.KIOFuse`, `mountUrl`), mounting once in the runtime folder
  (`kio-fuse-XXXXXX`) and each address asked below it by its scheme and
  server (`smb/nas/photos`). Plasma has it: Arch's `plasma-workspace`
  requires it, Fedora's Dolphin pulls it in; Debian makes it optional.
  KDE's own programs use it to hand a remote file to a program that does
  not speak KIO.
- Read in its source and tried on this machine (2026-10-22), against an
  OpenSSH server and rclone's WebDAV and FTP servers, on a session bus
  of the test's own:
  - It mounts in 130 to 210 ms; 64 MiB read through it on loopback in
    395–466 ms by one reader, 204–249 by four to eight (an `sftp` get,
    259–286). Over SFTP, SMB and AFC each kernel read is a whole open,
    seek, read and close of the server's file (four round trips per
    256 KiB, five workers per server at most).
  - Over WebDAV, FTP and NFS it reads a file whole into a file of its
    own before any part of it is read, and sends one up whole as it is
    closed: a server's refusal is told only by `fsync` and `close`
    (`EPERM` over SFTP, `ENOENT` over rclone's WebDAV), and the server
    keeps its old file. kio-fuse keeps the file refused to send it
    again, and refuses to remove it meanwhile.
  - It keeps a listing and a file's attributes 30 seconds: a file made on
    the server is not listed until then, or until it is told to forget
    (`org.kde.KIOFuse.Private.forceNodeTimeout`).
  - It asks nothing: its KIO jobs have no window, so KIO tells its
    workers not to ask (`no-auth-prompt`). A server that wants a
    password it has not been given (KWallet, this session's cache of
    KDE's dialog) fails, as "No such file or directory" over WebDAV; on a
    certificate not trusted it waits for good (no answer in three
    minutes). Once it holds a server, it gives any path there without
    looking. It refuses `mtp:` and lets go of no server: its folders stay
    until it ends with the session.
  - `kioclient --interactive` gives its job a window: KDE asks (the
    password in kpasswdserver's dialog, a certificate in kioclient's),
    the answers kept for the session, where kio-fuse finds them. Its
    `stat` shows a progress window, and on failing an error box that
    waits to be dismissed; its `ls` hides progress and ends without a
    word. In the job a certificate was accepted in, KIO's WebDAV takes
    the server's refusal of a login as "does not exist" rather than ask
    the password; the next job asks it.
  - Over WebDAV it keeps no mode and no time: setting either is refused
    (`ENOTTY`, its word for what a protocol does not do).
- Skwir's copies dropped the copy written without asking its close
  what it said, and `syncfs` is not passed to a FUSE program: over
  kio-fuse's WebDAV a copy its server refused counted as done, and a move
  then deleted its original.
- The GVfs connector was unavailable until GVfs's bridge was mounted,
  though `gio mount` starts GVfs's daemon, which mounts it: on a desktop
  other than GNOME's, GVfs installed but not yet running was said absent.

## Decision

1. **A copy is done once its close says so** (`op::sys::close`): a FUSE
   program sends there what it held (sshfs its writes, rclone and
   kio-fuse the file) and tells there what its server refused, as NFS
   does. Its time and mode are given after, by its name (an upload at
   close would take a time given before), the time first (Windows opens
   the file to give it). A copy its close refused is not done, and says
   it was refused as it was closed, never that its source is gone; a
   move keeps its original. The trash's record is written the same way.
   Before, over kio-fuse's WebDAV, only kio-fuse's own second try at
   placing the copy failed it.
2. **GVfs where it can be started.** The GVfs connector is available
   where its daemon runs or the session bus would start it
   (`org.gtk.vfs.Daemon` activatable); `gio mount` starts it, and the
   bridge it mounts is waited for a moment.
3. **The `kio` connector.** It opens SMB, WebDAV (over HTTP and HTTPS)
   and FTP addresses through this session's kio-fuse, asked over the
   session bus; SFTP stays sshfs's (its questions in the window, faster)
   and NFS the kernel's. Each server is a folder of kio-fuse's mount, two
   deep (`webdav/nas:8080`), a remote of its own (`reach`'s bridges,
   GVfs's one deep); its SMB shares are folders below it. What kio-fuse
   holds of SFTP, NFS and an iPhone (AFC: a device) is known all the
   same, mounted by KDE's programs; an archive it reads is no remote.
   KDE's remote places (`user-places.xbel`, Dolphin's sidebar) are its
   servers, listed in the Network section as rclone's remotes are.
4. **Its questions are KDE's.** Where kio-fuse fails to mount, KDE is
   let ask (`kioclient --interactive ls`): its own dialogs, in the user's
   language, the password kept in KWallet if the user says so; Skwir
   never sees it (ADR-077's seventh decision, for KIO). kio-fuse is then
   asked again, and finds what KDE kept. Where KDE could not list it
   either, kio-fuse's reason is told. Over HTTPS, KDE asks first, unless
   kio-fuse holds the server (on a certificate not trusted kio-fuse
   would wait for good), and a second time where the first failed (the
   password, after a certificate accepted).
5. **What differs.** No Eject: kio-fuse lets go of no server. A server
   kio-fuse holds is asked again for each address opened on it (it mounts
   what it is asked). Before a folder there is read again (F5, the window
   back, a transfer beside the mount), kio-fuse is told to forget what it
   listed (`Known::forget`, `App::read_again`, in the remote's lane).
   Where it reads a file whole (WebDAV, FTP, NFS), Automatic thumbnails
   are Small files near, never Always.
6. **The order.** `remote.connectors` defaults to `sshfs gvfs kio rclone
   fstab`, and to `sshfs kio gvfs rclone fstab` on KDE's desktop
   (`XDG_CURRENT_DESKTOP`), whose remotes and passwords are KIO's. An
   order the user saved before KIO gets it last.

## Consequences

- KIO's remotes need kio-fuse (KDE) and a session bus: Linux only, as
  GVfs.
- Over KIO's WebDAV and FTP a copy keeps no time and no mode, and a large
  file read there is first downloaded whole into a file of kio-fuse's
  (in `/tmp`, often memory).
- A copy refused as it was closed leaves its staging folder on the
  server: noted, and swept once removable (ADR-078), kio-fuse told to
  forget first. Against rclone's WebDAV it is removable once the server
  reads the folder again (5 minutes by default): until then it keeps an
  entry of the upload it cut off. Tested so, the server's cache at 15 s.
- Copies to and from KIO's remotes go in batches by eight `kioclient
  copy` at once (ADR-078), measured against copies through the mount
  (2026-10-22, 100 files of 16 KiB, a server 20 ms away): WebDAV up 11.2 s
  through kio-fuse, 0.8–1.1 by `kioclient`, down 2.4 and 0.9, within 11.3
  and 1.2; SMB up 17.0, 5.6, down 6.6, 3.5, within 17.4, 6.2; FTP up 46,
  3.2, down 8.6, 2.6–5.6. Over FTP kio-fuse writes no new file (it makes
  it empty, then reads it back before the first write, which KIO's FTP
  worker fails): every copy up goes by `kioclient`, a file alone too.
- kio-fuse makes a file's change time up (the moment it read its
  attributes): a move's check of its original leaves it out there.
- Tested against servers on this machine (`just remote-check`): WebDAV
  and FTP by rclone, SMB by Samba (a share a guest opens), on a session
  bus of the test's own with KIO's services alone. KDE's dialogs were
  tried in a window on a display of its own, not by the tests: a
  password over WebDAV (asked, cancelled, asked again), a certificate not
  trusted over HTTPS with and without a password, accepted and refused.
  KWallet (6.30, `ksecretd` with `kwalletd6` beside it) was tried so too:
  a password KDE was told to remember was found by kio-fuse alone once
  KIO's session had ended, and Skwir's keyring tests (ADR-082) pass
  against KWallet's Secret Service. Its wallet `kdewallet` was made
  first, as a Plasma login does: on a profile without one, KWallet
  asked to make a wallet with no name, and KDE's password server then
  hung (a matter of KWallet's, not Skwir's). kio-fuse started as Plasma
  starts it was tried too: its bus handing the activation to a `systemd
  --user` of the trial's own (`kio-fuse.service`), Skwir opened a folder
  there and F5 read it again.
