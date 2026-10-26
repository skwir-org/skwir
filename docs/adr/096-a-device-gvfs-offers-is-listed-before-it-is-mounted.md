# ADR-096: A device GVfs offers is listed before it is mounted

Status: accepted, 2026-10-26, the user's ask ("Plugging in a MTP phone
for file transfer doesn't show anywhere in skwir (shouldn't it appear in
devices?), though I can see it in Dolphin"), then the five steps proposed
after the investigation ("Go ahead with 1-5"). Amends ADR-077: a device
GVfs reads has an address of its own.

## Context

- A phone over MTP, a camera over PTP (GVfs's gPhoto2) and an iPhone
  over AFC reach Skwir only through GVfs (ADR-077): once mounted, each is
  a folder of GVfs's FUSE bridge, which the remote plugin lists among the
  sidebar's devices (`Recognised::device`). Skwir saw mounts only.
- Nothing mounted a phone plugged in. GNOME Shell mounts a volume GVfs
  says should be (`should_automount`); on another desktop (Hyprland, the
  reference session) nothing does. GVfs's MTP volume monitor listed the
  phone (`gio mount -li`: a volume "Pixel 5", `mtp://<its USB serial>/`,
  `can_mount`), and its bridge held nothing.
- Dolphin shows it another way: KDE lists portable devices from udev
  (Solid) and opens them through KIO's MTP worker, whose daemon (`kiod6`,
  its `kmtpd` module) opens every MTP device it sees and keeps it open.
  Over USB one program at a time uses an MTP device: while `kiod6` held
  the phone, GVfs could not open it ("Impossible d'ouvrir le périphérique
  MTP « 009,010 »", in the user's language), and `kiod6` took it again as
  soon as it was plugged back in. kio-fuse refuses `mtp:` (ADR-081's
  probe): Skwir cannot go KDE's way.
- A device's mount had no address (`Spec::address` named none). Eject
  asks GVfs to let go of a mount by its address, and failed there ("GVfs
  says no address for this mount"); its label was its folder's name, its
  USB serial included.

## Decision

1. **A device GVfs reads has an address**: `mtp://<GVfs's name>/`,
   `gphoto2://[usb:BBB,DDD]/`, `afc://<identifier>[:service]/`
   (`Scheme::Mtp`, `Gphoto2`, `Afc`; `Scheme::device`), which GVfs alone
   opens. It is its mount folder's address (`Spec::address`), what its
   monitor offers it at, and what Eject lets go of (`gio mount -u`). A
   device is no server: its name (a serial) is never looked up on a
   network, nothing of it is kept as a server's (passwords, mount
   options, connecting at login), and no other tool is tried.
2. **What GVfs offers is listed before it is mounted.** GVfs's volume
   monitors of devices (MTP, gPhoto2, AFC) on the session bus are asked
   as GIO's programs ask them (`org.gtk.Private.RemoteVolumeMonitor`,
   `List`; the bus starts one that does not run), and followed: each of
   their signals, and GVfs's daemon's `Mounted` and `Unmounted`, reads
   the lists again (`Connector::follow_devices`, `gvfs_devices`), on a
   thread of the plugin's while it is enabled. Each mountable volume whose
   address names a device is a place among the devices, by its volume's
   name, at its address, drawn as an unmounted disk is ("Not mounted.
   Click to mount and open it."). Mounted, it is the same place (its key
   `remote:<address>`), at its folder, with Eject, by the same name
   (`Remotes::label_of`); the pane's banner and messages name it so too.
   A volume is read by the place of its fields (name, address, whether it
   mounts, its `unix-device`); fields GVfs adds after are not read, and a
   monitor that answers otherwise lists nothing.
3. **Mounted once opened, never because it was plugged in**: a mounted
   device is held from every other program. Opening it is opening its
   address (ADR-077): the pane says "Opening Pixel 5…", Cancel and Try
   again as for a server.
4. **A device another program holds is said to be held, and by whom.**
   When opening it fails, the programs of the user's that hold its node
   open (the volume's `unix-device`) are read from `/proc`, by the names
   they were started as: "kiod6 has it open, and only one program at a
   time can use a phone. Quit kiod6, then try again." GVfs's own program
   for that kind of device (`gvfsd-mtp`), still letting go of the open
   that failed, is not named; another of its programs (`gvfsd-gphoto2`
   holding the phone as a camera) is.

## Consequences

- A phone, a camera or an iPhone plugged in shows among the devices on
  any desktop where GVfs is installed; one GNOME mounted shows mounted.
  Skwir never stops another program holding it, and never mounts one
  unasked.
- The monitors' interface is GVfs's private one, between its monitors and
  GIO, versioned with GVfs: a GVfs that changed it would list no device
  until Skwir reads the change. The stand-in monitor's test fixes what is
  read; `just mtp-check` reads nixpkgs' GVfs with a phone.
- While the plugin is enabled on Linux, a connection to the session bus
  is kept, with a thread waiting on it and another waiting for the
  plugin's end: three threads with zbus's own, idle memory unchanged
  within what a reading varies (measured on the debug build). The bus
  starts the three monitors where they do not run yet, as any GIO
  program does: small processes of GVfs's, not Skwir's.
- Linux only, as GVfs is. Windows reaches phones through its Portable
  Devices, which are no folders; macOS has no MTP (`docs/PORTING.md`).
- An iPhone kio-fuse holds (KDE's `afc:`) is still known without an
  address.
- Tested: the stand-in monitor on a private bus (`remote/tests/
  devices.rs`: listed by name and address, a camera beside, unplugged,
  disabled and enabled again, a volume that does not mount or names no
  device left out, a holder named through `Remotes::reach`); the window
  on a display of its own against the session's GVfs, the phone held by
  `kiod6` (listed, said held by `kiod6`), then free (opened, the same
  place mounted with Eject, let go of, offered again); `just mtp-check`
  with the phone (offered, opened, Eject, offered again, said held by
  `gvfsd-gphoto2` while it held the phone as a camera).
- A connector through KDE's `kmtpd` instead (its D-Bus interface, served
  as a folder by a FUSE helper of Skwir's) was measured against GVfs on
  the same phone and not built: it would let KDE and Skwir use a phone at
  once, but serves one request at a time, reads whole files only, and
  listed no faster (`docs/STATUS-LOG.md`, 2026-10-26).
