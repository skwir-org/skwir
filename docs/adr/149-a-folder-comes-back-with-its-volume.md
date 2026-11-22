# ADR-149: A folder comes back with its volume

Status: accepted, 2026-11-22, on the user's answers (asked that a pane
showing a folder that is not there because its volume is not mounted
show it again once the volume is mounted, a middle click on the volume
in the sidebar for instance, and only when the folder is under the volume
mounted; chose the general way proposed, the clearer message and the
button). Built on Linux the same day. Amended that evening, on the
user's answers once other file managers were read (decisions 4, 7 to
12): Skwir's own Eject and Disconnect leave the panes where they are;
a folder on a volume the user may let go of is held by nothing between
reads; what other programs announce they unmount is let go of first,
Skwir's own work there named with Cancel; Eject names that work rather
than blaming another program; a server typed in the address field
offers Connect too; a GVfs folder let go of is told.

## Context

- **A pane whose folder could not be shown stayed so** until F5 or its
  button: a folder lost while shown ("This location could not be
  opened", Try again), one not there when opened (a tab kept on a drive
  not plugged in, Back to a folder of a server disconnected: "This
  folder no longer exists", Go to the parent folder). Mounting its
  volume, from the sidebar or anywhere, changed nothing in it.
- **Try again listed what was deleted.** A folder's listing kept the
  directory it held open once it was lost: a refresh listed that
  descriptor, a folder deleted showing as empty, never the folder made
  again at its path.
- **A folder held open kept its filesystem.** A drive pulled out, or a
  server's mount let go of lazily (udisks after a drive is pulled out,
  `umount -l`), leaves its filesystem alive while a descriptor holds it:
  the folder's watch never says it went, the pane showing its files as
  they were, and the filesystem staying until the pane goes elsewhere.
- **A mount made on a folder shown** (an empty mount point) was not seen
  either: the watch is on the folder underneath.
- **The table of mounts is followed already** (`reach::follow`, ADR-077:
  the kernel's word, settled 250 ms), by the places and remote plugins
  for the sidebar.

## Decision

1. **The panes follow the mounts** (`skwir_core::volumes`,
   `App::volumes_changed`): the table's points (`reach::points`), and
   the folders a plugin knows mounted that the table does not list (a
   GVfs bridge's, `Registry::remotes`, which the remote plugin tells as
   it publishes one more or one fewer), seen once at start and again at
   each change, on a worker. A change is the points holding more mounts
   than when last seen and those holding fewer, by their text, from
   memory: nothing of the paths shown is read to find it. A plugin's
   folder is told let go of only while that plugin is on (amended,
   decision 12): one turned off lets go of what it knows, not of the
   mounts.
2. **At or below a mount made, a pane's folder is opened again from its
   path** (`OpenLocation::reopen`), whoever made the mount: the sidebar,
   `udisksctl` in a terminal, an automounter, a remote's program. A
   folder that could not be shown shows; an empty mount point something
   was mounted on shows what was mounted, its entries before taken out
   as the new are listed (no empty frame). What a pane holds open counts:
   the folder shown, those kept beside it (the columns view's, the
   folders opened in place); a tab not shown yet opens what is there
   once it is. A mount let go of never opens a folder again: the folder
   under a mount point would show as the drive empty.
3. **Below a mount let go of, a folder lets go of what it held there and
   says so** (`OpenLocation::volume_gone`, `Failure::Left`): its entries
   go, its directory and its watch are closed in the shared lane (so the
   filesystem can go), and it says its drive was unmounted or removed, or
   its server disconnected, and that the folder shows again once it is
   back. A folder deleted while shown is `Failure::Missing` (was a retry)
   and lets go of its directory too: a refresh opens the folder again
   from its path, never the directory that went. A deletion told after
   the volume's going does not say over it.
4. **What the pane says** (`placeholder::failure`):
   - its volume gone: "This folder is no longer available", why, and Go
     Home (amended: Home whatever the pane showed before, as the step
     before may be on the same volume);
   - **a volume or server not open now that was at or above it**
     (`Registry::reopens`, the nearest; amended, was `Place::reopen`):
     what its owner says ("USB is not mounted", "nas is not connected",
     "The connection to nas is lost"), "This folder is on it, and shows
     here again once it is back." (the system's words for a connection
     lost), and two buttons: its own ("Mount USB", "Unlock USB",
     "Connect to nas"), which runs its command with `pane` and `below`,
     the folder's path below where it was, then Go Home. While its owner
     works on it, what it does ("Mounting…") in place of its button. A
     folder refused stays refused.
   - The places plugin offers it for a volume not mounted that it saw
     mounted this run, at that point (by the volume's key, ADR-034's
     `volume:<uuid>`); the remote plugin for each server whose mount it
     saw this run, kept or typed in the address field (amended), and for
     a mount whose connection is lost. Only this run: nothing is kept for
     later (ADR-000, 3), and no mount point is guessed from a label.
5. **`below`**: `places.mount`, `places.unlock`, `places.open_locked`
   and `remote.connect` take a folder below what they open, by its names
   alone (`Reopen::below` refuses a root or `..`): once mounted, that
   folder is shown in place of the step that could not show it
   (`navigate_in_place`), or read again where it is shown already; a
   server's below its mount's address (`remotes::address_below`), which
   connects and redirects there.
6. **Windows** tells no drive mounted by a table: a folder not there is
   opened again as a window comes to the front (`App::reopen_missing`),
   failing again as it failed.
7. **Skwir's own Eject and Disconnect leave the panes where they are**
   (amended; they went home, as Nautilus, Nemo, Thunar's tab shown and
   Dolphin after its own eject do): `App::release` lets go of what is
   held below the mount (`OpenLocation::hold_nothing`: a folder's listing
   reads by its path from then on) and goes on showing it, the pane
   saying so once the volume goes (decision 4: Mount or Connect, Go
   Home), as a tab not shown yet already did. What cannot hold nothing
   (a location that cannot say) goes home as before, or closes beside
   the folder shown.
8. **A folder on a volume the user may let go of is held by nothing
   between reads** (amended; `fs::volume::lets_go`: a remote, a drive
   that can be unplugged, or one mounted for the user, under
   `/run/media` or `/media`, macOS's `/Volumes`): opened for each read,
   so an unmount by any program, announced or not (`udisksctl`,
   udiskie), is never refused as busy because of it, as with Dolphin,
   Thunar, Nemo and PCManFM (Nautilus holds the folder shown and lets go
   only for its own unmount). Its watch, which holds no mount, keeps its
   identity known: renamed within its folder it is found there and
   followed; moved elsewhere it is gone (`Failure::Missing`). A folder on
   any other volume is held, followed wherever it moves on it.
9. **What another program announces it unmounts is let go of first**
   (amended; `places::follow_unmounts`: GIO's `MountPreUnmount`, sent by
   GNOME's Eject, Nautilus and `gio mount -u`; Solid's
   `teardownRequested`, by Dolphin and KDE's device notifier), as Thunar
   does: `App::release`, as for Skwir's own Eject. For `LETTING_GO`
   (10 s) the windows let go of what shows a file there (a document's or
   an archive's helper, what plays, ending as at its end) and a terminal
   shell there goes home, following or not (`Input::Leave`), as
   Dolphin's terminal does; shown again where the volume stays. Skwir's
   own actions there (a copy) are never cut unasked: they hold the
   volume, and a toast names them with Cancel (`cancel_in`); the other
   program says the volume is busy (GNOME's dialog retries every 2 s).
   What unmounts without announcing it is not told; Linux only.
10. **Eject names Skwir's own work there** (amended): a drive's Eject is
    refused while an action of Skwir's is under way on it, naming it
    (`Executor::refuse_under_way`, which Disconnect asks too), rather
    than after 3 s as "another program is using it"; `force` (an
    agent's, a script's) cuts it.
11. **The offers are a list of their own** (amended:
    `Registry::reopens`, owner-tagged, withdrawn on disable): a server
    typed in the address field has no sidebar place, and offers Connect
    all the same.
12. **A bridge's folder let go of is told** (amended): the remote plugin
    tells the core as the GVfs folders it publishes change, and one it
    publishes no more while it is on is let go of as a mount is.

## Considered

- **Refreshing on the sidebar's click or middle click** (the user's
  first idea): only Skwir's own mounting, and only from there. Following
  the table covers every program that mounts, and leaves the click
  meaning what it means.
- **Re-listing on any change of the table**: a stat of every folder
  shown, a remote's included, for a container's mount; and a folder
  shown again under a mount let go of as the empty folder beneath.
- **A folder deleted and made again at its path** (`rm -rf build &&
  mkdir build`), watched for at the deepest folder above that is there:
  another case than a volume's, left out.
- **Going home on Eject, or closing the tab** (amended), what most file
  managers do: the pane that stays says why, offers to mount it again and
  home, and comes back by itself.
- **Holding no folder open anywhere**: a folder on a fixed disk moved by
  another program elsewhere on it would no longer be followed, where no
  unmount is asked of it anyway.

## Consequences

- `OpenLocation` gains `volume_gone` (nothing by default), `reopen`
  (`refresh` by default) and `hold_nothing` (false by default); a
  location listing through another (the trash's folders, the query
  plugin's within) hands them on.
- `Failure::Left`; `ListingError::missing` and `left`.
- `Registry::reopens` and `registry::Reopen`; the placeholder's buttons
  are invocations.
- `App::release` no longer navigates what can hold nothing;
  `App::letting_go`, `LETTING_GO`; the terminal's `Input::Leave`;
  `Executor::cancel_under`, `refuse_under_way` and the `cancel_in`
  command; `places.unmount`'s `force`.
- A mount let go of and made again within one settled change is taken as
  the one before (as `reach` takes it): a folder lost then is not opened
  again by itself.
- What shows a file on a volume announced is let go of for 10 s: an
  unmount refused (busy) leaves the preview closed that long.
- Two ways back that stayed where the pane stays (found by the remote
  folders' tests): a kept listing that failed is opened anew, never
  shown as it was (`Pane::take_kept`), and an address's location that
  redirected is closed as the pane follows it, so connecting again
  connects (`App::follow_redirect`); before, a pane sent home hid both.
