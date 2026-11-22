# ADR-149: A folder comes back with its volume

Status: accepted, 2026-11-22, on the user's answers (asked that a pane
showing a folder that is not there because its volume is not mounted
show it again once the volume is mounted, a middle click on the volume
in the sidebar for instance, and only when the folder is under the volume
mounted; chose the general way proposed, the clearer message and the
button). Built on Linux the same day.

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
   it publishes one more), seen once at start and again at each change,
   on a worker. A change is the points holding more mounts than when
   last seen and those holding fewer, by their text, from memory: nothing
   of the paths shown is read to find it. A plugin's folders are only
   ever told as mounted: one turned off lets go of what it knows, not of
   the mounts.
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
     Back (Go Home where it has no step before);
   - **a place not open now that was open at or above it**
     (`Place::reopen`, the nearest): what that place says ("USB is not
     mounted", "nas is not connected", "The connection to nas is
     lost"), "This folder is on it, and shows here again once it is
     back." (the system's words for a connection lost), and its button
     ("Mount USB", "Unlock USB", "Connect to nas"), which runs the
     place's command with `pane` and `below`, the folder's path below
     where the place was. While its owner works on it, what it does
     ("Mounting…") in place of the button. A folder refused stays
     refused.
   - The places plugin offers it on a volume not mounted that it saw
     mounted this run, at that point (by the volume's key, ADR-034's
     `volume:<uuid>`); the remote plugin on a server not connected
     whose mount it saw this run, and on a mount whose connection is
     lost. Only this run: nothing is kept for later (ADR-000, 3), and no
     mount point is guessed from a label.
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

## Consequences

- `OpenLocation` gains `volume_gone` (nothing by default) and `reopen`
  (`refresh` by default); a location listing through another (the
  trash's folders, the query plugin's within) hands both on.
- `Failure::Left`; `ListingError::missing` and `left`.
- `Place::reopen` and `registry::Reopen`; the placeholder's button is an
  invocation.
- A mount let go of and made again within one settled change is taken as
  the one before (as `reach` takes it): a folder lost then is not opened
  again by itself.
- A folder on a server typed in the address field and never kept has no
  place once disconnected: it says so and offers Back, and comes back
  once connected again.
