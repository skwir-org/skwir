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
  - It asks nothing but what KDE's password dialog asks (kpasswdserver,
    KWallet): a host key never seen and a certificate not trusted are
    refused, as "Input/output error". It refuses `mtp:` and lets go of no
    server: its folders stay until it ends with the session.
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
4. **Its questions are KDE's.** A password is asked in KDE's own dialog
   and kept in KWallet if the user says so; Skwir never sees it (ADR-077's
   seventh decision, for KIO). What KIO cannot ask through kio-fuse (a
   host key, a certificate) is accepted once in Dolphin.
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
  server (kio-fuse will not remove what it still means to send): noted,
  and swept once removable (ADR-078), which was not seen.
- Copies to and from KIO's remotes go through the mount, eight files at
  a time; `kioclient copy` (which takes many sources, and refuses a name
  taken unless told to replace it) is not used yet: no measurement says
  where it is faster.
- Tested against servers on this machine (`just remote-check`): WebDAV
  and FTP by rclone, on a session bus of the test's own with KIO's
  services alone; SMB and KDE's dialogs not.
