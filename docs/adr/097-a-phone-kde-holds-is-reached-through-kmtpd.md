# ADR-097: A phone KDE holds is reached through kmtpd

Status: accepted, 2026-10-26, the user's choice ("Accept"), after the
measurements on a Pixel 5 (`docs/STATUS-LOG.md`, "Devices before they
are mounted") and their agreeing to its four points. Not built yet.
Amends ADR-077 (a mount of Skwir's own helper) and ADR-096 (a second way
to a device).

## Context

- Skwir reaches a phone through GVfs (ADR-096). Over USB one program at
  a time uses an MTP device, and KDE's `kmtpd` (a module of `kiod6`,
  kio-extras) opens every MTP device present when it loads and each one
  plugged in after (`kmtpd.cpp`: `checkDevice` at start, Solid's
  `deviceAdded`), Dolphin open or not, until `kiod6` ends, which it never
  does by itself (`kiod_main.cpp`). `kiod6` loads a module when a call is
  addressed to it (`/modules/kmtpd`), so once a KDE program has asked
  for a phone, KDE holds every phone: on Plasma, always. GVfs then
  cannot open the phone, and Skwir can only say who holds it (ADR-096,
  fourth decision).
- KDE's own way to a folder, kio-fuse, refuses `mtp:` on purpose: its
  list of refused schemes (`kiofuseservice.cpp`) has held `mtp` since
  its first release (commit `8e3c3820`, 2019-12-13, "Blacklist certain
  protocols", for issue #2 "MTP corruption on read", open since
  2019-11-02, last updated 2022-09-11). kio-fuse is maintained (2026:
  mounting on lookup, sign-in prompts), MTP untouched. KIO's MTP worker
  reads no part of a file either.
- `kmtpd` answers on the session bus (`org.kde.kmtp.Daemon`:
  `listDevices`, `devicesChanged`; `org.kde.kmtp.Device`: `friendlyName`,
  `udi`, `listStorages`; `org.kde.kmtp.Storage`: `description`,
  `maxCapacity`, `freeSpaceInBytes`, `getFilesAndFolders`,
  `getFileMetadata` (`(uuustxs)`: object, parent, storage, name, size,
  modified, type), `getFileToFileDescriptor`,
  `sendFileFromFileDescriptor`, `setFileName`, `createFolder`,
  `deleteObject`, the copies ended by `copyFinished`). A call to
  `/modules` loads nothing and lists the modules loaded.
- Measured on the phone against GVfs (the same 40 folders): `kmtpd`
  listed the largest (2287 entries) in 28.7 s (GVfs 21.5 s), read a
  12 MB file whole at 31 MiB/s (GVfs's bridge 7.2 MiB/s, `gio copy`
  about 27), reads no part of a file (GVfs read 64 KiB of 159 MiB in
  0.49 s), and serves one call at a time on `kiod6`'s one thread (a
  listing waited for a read). Locked, the phone kept a listing 60 s and
  gave nothing. Its cache maps paths to the phone's objects for 60 s,
  renewed on use; each listing asks the phone.

## Decision

1. **A connector `kmtp`** (Linux) opens `mtp://` addresses, tried before
   GVfs. It answers only for a phone `kmtpd` holds: asked only of a
   `kiod6` that owns its name on the bus (never started for it), whether
   `kmtpd` is loaded read from `/modules` (never loaded for it), except
   on KDE's desktop (`XDG_CURRENT_DESKTOP`, as the connectors' order
   reads it), where it asks `kmtpd`, loading it, as Dolphin would. Where
   it does not answer, without asking anything, the address goes to
   GVfs as it does now (`Remotes::reach`: the next tool).
2. **One address, one place.** A device `kmtpd` holds is the address
   GVfs gives it (`mtp://<ID_SERIAL>/`, ADR-096), its `udi` read as the
   device's folder of sysfs and its serial from udev, so it is the same
   place whichever holds it. `kmtp` offers the devices `kmtpd` holds
   (`Connector::follow_devices`: `listDevices` again at
   `devicesChanged`, and whenever GVfs's monitors say a device came or
   went), named by `friendlyName`; a device two connectors offer is one
   place.
3. **A helper of Skwir's mounts it** (`skwir-kmtp-worker`, FUSE through
   `fusermount3`), started by the connector, in Skwir's mounts folder
   (`connector::mounts_folder`) as its sshfs and rclone mounts are, its
   type `fuse.skwir-kmtp`: a device's mount, near, in a lane of its own
   (ADR-077, ADR-096). It serves `kmtpd`'s interface as a folder:
   - each storage a folder, by its description; a folder listed by
     `getFilesAndFolders`, its entries' sizes and times from it, inode
     numbers made up (kept by path, ADR-077);
   - a file opened for reading is fetched whole
     (`getFileToFileDescriptor`) into an unnamed file of Skwir's cache
     folder, read from there, gone at its last close; the mount says it
     reads files whole (`Connector::reads_whole`: Automatic shows small
     files' thumbnails, ADR-081);
   - a file written is kept in such a file and sent at its close
     (`sendFileFromFileDescriptor`), a refusal told by that close; a
     file replaced is removed first, MTP replacing nothing;
   - a folder made, an entry removed, a name changed within its folder
     by `createFolder`, `deleteObject`, `setFileName`; a move between
     folders refused (`EXDEV`): copies are written in place there
     (`Remote::moves_between_folders`, ADR-083); times and modes refused
     (`ENOTSUP`): the phone stamps what is sent;
   - one call to `kmtpd` at a time, as it serves them; the helper ends,
     letting go of its mount, when its device leaves `kmtpd`'s list or
     `kiod6` the bus.
4. **KDE's phone stays KDE's.** Eject lets go of Skwir's helper and its
   mount; `kmtpd` keeps the phone, and Skwir never unloads it nor ends
   `kiod6`. Dolphin and Skwir read the phone at once, one call after the
   other.
5. **The helper is a helper like the others**: listed with the
   packages' programs, answering `--probe` and `--build`, its notices
   made (`fuser`, `zbus`); pure Rust, built by default, Linux only.

## Consequences

- On Plasma, and wherever a KDE program has asked for a phone, Skwir
  opens the phone KDE holds, Dolphin still using it. Elsewhere nothing
  changes: GVfs as in ADR-096.
- Slower where `kmtpd` is: a large folder listed in tens of seconds, a
  file's thumbnail or preview fetching the whole file, a copy holding
  every other call to the phone (Dolphin's too) until done. Skwir's
  lanes keep the rest of the window going; the pane says the phone does
  not answer while it waits.
- A file replaced is lost if the new one is then refused; nothing sent
  keeps its time.
- `kmtpd`'s interface is kio-extras' own, not promised: the stand-in's
  test fixes what is used, and a phone check runs it against nixpkgs'
  kio-extras.
- Skwir starts a FUSE program of its own for the first time (ADR-077
  mounted through installed tools only); the way to Skwir speaking a
  protocol (ADR-077, point 8) stays closed: the helper speaks `kmtpd`'s
  D-Bus interface, not MTP.
- Not chosen: waiting for kio-fuse (refused since 2019, and through the
  same `kmtpd`); speaking MTP in Skwir through libmtp (it could not open
  a phone `kmtpd` holds either); ending `kiod6` from Skwir (a "Quit
  kiod6" button stays possible, and ends what else `kiod6` serves, KIO's
  passwords for the session among them).
- To test: a stand-in `kmtpd` on a private bus (listing, reading,
  writing, refusals, a device gone, a `kiod6` that does not run, one
  where `kmtpd` is not loaded and must not be); the helper's mount in a
  test's FUSE; a phone held by a private `kiod6` in `just mtp-check`
  beside GVfs's part.
