# ADR-084: Servers nearby are found while the Nearby place is shown

Status: accepted, 2026-10-23, the user's choice for the night ("A place,
browsed while shown", over listing them in the sidebar for as long as
Skwir runs, or not now). Built and checked on Linux the same night, on a
network of the test's own. A new use of the network (AGENTS.md,
"Network"), through what the system runs for it.

## Context

- ADR-077 left "servers found on the local network (GVfs's
  `network:///`)" for later: the Network section lists the mounts and the
  servers the tools know (rclone's remotes, KDE's places, the user's ssh
  hosts), never one that only announces itself.
- Servers announce themselves two ways. DNS-SD over multicast DNS
  (Avahi on Linux): a NAS, a Mac, a Linux server announcing SFTP, SMB,
  WebDAV, FTP, NFS or AFP. WS-Discovery (wsdd): Windows PCs, which no
  longer announce SMB otherwise. Browsing for them sends queries on the
  local network.
- Nautilus lists them under Other Locations, Dolphin under Network: a
  place the user opens, browsed while it is shown. GVfs's `network:///`
  merges both (its `dnssd` backend over Avahi, its `wsdd` backend over
  wsdd's socket, or a wsdd of its own).
- Tried on a network of the test's own (user, network, mount and pid
  namespaces; a veth pair to a second namespace standing for a PC):
  Avahi there found and resolved what it published, and GVfs listed it;
  wsdd found the PC across the pair (two on one interface never hear each
  other: wsdd turns multicast loopback off), but GVfs's `wsdd:///` listed
  nothing, though the wsdd it reads had found the PC.

## Decision

1. **A place, browsed while shown.** The Network section ends with
   Nearby (`nearby:`, the `remote` plugin, Linux): a location whose
   entries are the servers found, each a place (`EntryId::place`, no
   file) named with what it speaks ("NAS (SMB)"), leading to its address
   (`OpenLocation::location_of`): opening one connects as typing it does,
   its questions asked as theirs are. Nothing is browsed until a pane
   shows it, and browsing stops once it is let go of.
2. **Through what the system runs.** Skwir speaks no discovery
   protocol:
   - Avahi over the system bus (`org.freedesktop.Avahi`): a browser per
     service type Skwir opens (`_sftp-ssh._tcp`, `_ssh._tcp`, `_smb._tcp`,
     `_webdav._tcp`, `_webdavs._tcp`, `_ftp._tcp`, `_nfs._tcp`,
     `_afpovertcp._tcp`), each service resolved as it comes, its IPv4
     address first (a `.local` name resolves only where nss-mdns is
     installed; an IPv6 address of the link alone opens nowhere as
     written), on its port, in the folder its TXT names (`path=`). Let go
     of, the browsers are freed and the connection closed.
   - wsdd for Windows PCs (SMB by their IPv4 address): the system's
     daemon where it answers (`/run/wsdd.socket`, which GVfs reads too),
     else wsdd started here in discovery mode (`--no-host`, announcing
     nothing), on a socket of the runtime folder, stopped with the
     place. Asked what it found every 3 s.
   GVfs's `network:///` is not read (a deviation from the plan, which
   named it for WS-Discovery): it adds nothing to these two where it
   works, and its WS-Discovery listed nothing on the test's network
   where wsdd itself found the PC; read directly, the same discovery
   works on every desktop, GVfs or not.
3. **One server, one row.** A server seen several times (on two
   interfaces, two protocols, SFTP announced as `_ssh` and `_sftp-ssh`)
   is one row, by its address, gone with the last. The listing is
   complete once both sources said what they had, three seconds at
   most; what comes later is added as it comes. Neither answering, the
   place says why.

## Consequences

- While Nearby is shown, Avahi sends mDNS queries and wsdd WS-Discovery
  probes on the local network, as Nautilus and Dolphin make them; nothing
  is sent otherwise, and nothing leaves the local network.
- A server found is not remembered: it is listed while it announces
  itself. Bookmarked, it is kept by its address (ADR-077).
- SMB browsing of old (NetBIOS, GVfs's `smb-root`) is not read: SMB1,
  which Windows and Samba turned off.
- Not on macOS and Windows yet (Bonjour's and Windows' own discovery):
  the place is not offered there.
- Tested on a network of the test's own (`tools/test/nearby.sh`, run by
  `just remote-check`): an SMB and a WebDAV server Avahi announced and a
  PC wsdd announced across the veth pair listed, each leading to its
  address; the wsdd Skwir started ended once the place was let go of.
