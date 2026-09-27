# ADR-022: Maps, later: search by place, writing locations, names and backgrounds offline

Status: proposed, 2026-09-27, and deferred. Nothing here is built. Each
part is decided when its work starts, by amending this ADR (accepted,
with what was chosen) or by an ADR of its own where it says so. What it
records now is what each part must respect and the options seen, so
that the work does not start from nothing.

## Context

The maps of ADR-020 are built through its steps A to D′ (`docs/PLAN.md`,
"Maps"): previews of tracks, plans and missions, backgrounds offline and
from tile servers, the Map view of a pane's pictures, videos and tracks.
Four things were left for later because each closes a door:

1. Finding files by where they are (`near:`), which needs positions a
   search can ask for, and so a contract between plugins.
2. Writing locations: geotagging pictures from a track, and removing
   locations before sharing. The first mutation of a file's content that
   bibli would make on its own.
3. Place names without the network: "Near Example Town" rather than two
   numbers.
4. Backgrounds from a local file (PMTiles, MBTiles), for streets without
   a server.

What holds today, and binds each part:

- **Positions are not metadata.** ADR-020 kept them out of hydration:
  reading EXIF for every listing would slow every folder. The `geo`
  plugin finds them for the Map view on workers, keyed by identity
  (device, inode, size, modification time) in memory and in the cache's
  `geo` namespace, and hands them over with `geo.points`.
- **Plugins do not depend on each other.** The query plugin cannot call
  the geo plugin's code; they meet in core-owned data or in commands.
- **Mutations are plans with inverses.** A plugin may register its own
  operation kind (`OpRegistry`, owner-tagged, as the tags plugin's
  `set_tags`); a batch, or anything not asked for by a single user
  gesture, goes through `App::propose` for review; undo and redo go
  through the journal.
- **The network is `map-tiles`' only** (ADR-020), and a new use is an
  ADR of its own. Nothing a file names is fetched.
- **Dates are the user's** (ADR-017). A camera's clock has no time zone:
  the scanner keeps it as digits (`exif::Taken::clock`), an order, not
  an instant.
- **Light** (ADR-000): the binary already carries 514 KB of outline;
  idle memory and startup are budgeted.

## 1. Search by place: `near:`

What it is: `near:10.12,20.12~5km` finds the files within 5 km of a
point; the Map view's rectangle, or its visible area, becomes a search
of the files inside (`inside:` a box); later `near:"Example Town"` once
names exist (part 3). The query plugin owns the syntax and its chips
(`facets.rs`); distances are geodesic, boxes may cross ±180°.

The question is how a search gets a candidate's position, cheaply
enough for a search over thousands of files:

- **A core contract for fields filled on demand.** A plugin registers a
  provider for a metadata key (here `position`), like a thumbnail
  provider: asked in batches, on workers, cancellable, its answers kept
  by identity in the cache. The query plugin asks for the key; the view
  that shows a column could too. It would serve other fields that are
  costly to read and worth searching (a video's duration, a document's
  page count). This is the core ADR `docs/PLAN.md` foresaw.
- **A batched command between plugins.** The query plugin dispatches
  `geo.positions { paths }` for batches of candidates and handles a
  disabled Maps plugin as any disabled command. No core change, but a
  contract only these two know, and a second plugin wanting positions
  would reach for the same command.
- **An index.** Ruled out: bibli keeps no database (ADR-000, principle 3).

Leaning: the core contract, since two consumers (search, views) and more
than one field are foreseen; the geo plugin's cache becomes its store.
Whatever is chosen: a position term is evaluated late, as `content:` is
(only when the cheaper terms have not decided), and a search of a large
tree reports it is still looking rather than blocking.

## 2. Writing locations: geotag from a track, remove before sharing

What it is: choose pictures and a track (GPX, FIT, any format read);
each picture taken while the track was recorded gets the track's
position at that moment, interpolated between its points, none where
the track has a gap longer than a few minutes. And the reverse: take the
location out of pictures, in place or in copies, before they leave the
machine.

What must hold:

- **Plans, reviewed, undoable.** One step per file, an operation kind
  the geo plugin registers (`set_position`), whose inverse restores the
  tags the file had (read when the plan is made and kept in it), whose
  check refuses a file changed since. A batch is proposed for review
  (`App::propose`), shown on the Map view as suggestions (markers of
  their own, the track under them) before anything is written. Re-applying
  an applied plan is a no-op.
- **The camera's clock is not an instant.** Matching needs the offset
  between the camera's clock and the track's UTC: the user's time zone on
  that date (the calendar of ADR-017) plus the camera's own error. The
  review shows the offset, lets it be changed (a picture whose place is
  known sets it), and the matches follow.
- **Provenance.** A position written this way is the user's once they
  confirm the plan (`Provenance::User`); until then it is a suggestion.
- **Writing is harnessed, not reimplemented** (principle 4). ExifTool
  writes every format that matters (JPEG, HEIC, TIFF, the RAW formats,
  XMP sidecars, videos' QuickTime keys) and preserves what it does not
  touch; it is a `Plugin::requirements` probe, run through
  `bibli_platform::process` with separate arguments. Without it, the
  commands say what is missing and where to get it. A writer of bibli's
  own for JPEG alone is the fallback considered, not the plan.
- **What else changes.** Whether the file's modification time is kept
  (a folder sorted by date should not reorder) is decided with the
  first user; a RAW file is written through its XMP sidecar, never in
  place.
- **Removing** is the same operation with no position; "copy without
  location" is a copy plan followed by it on the copies, so the
  originals stay as they are.

## 3. Place names offline

What it is: the nearest place's name for a position, in Details ("Near
Example Town"), for a track's ends ("from … to …"), as the Map view's
labels at low zooms, and for `near:` by name.

The network is not an option here without an ADR of its own, and no
desktop service answers offline (GNOME's geocoding asks Nominatim over
the network). The data comes with bibli or is installed beside it:

- **Natural Earth's populated places** (public domain, about 7 300
  places at 1:10m): a few hundred kilobytes packed as the outline is;
  names at the scale of cities and regions.
- **GeoNames' `cities1000`** (CC BY 4.0, about 150 000 places of more
  than 1 000 people): several megabytes; names at the scale of a walk,
  with attribution to carry.
- **OpenStreetMap-derived data** (ODbL): its share-alike terms on a
  derived database make shipping it a decision of its own; not
  proposed.

Leaning: Natural Earth in the geo plugin, loaded when a name is first
asked and let go with the plugin, looked up through a grid; GeoNames as
an optional data file found at runtime, as the native workers are, for
those who install it, with its attribution in Details' source line.
Names are shown in the language the data has; localized names are left
out (they multiply the data).

## 4. Backgrounds from a local file

What it is: a map background read from a file the user chose, streets
and all, with no server: PMTiles (one file, a header and compressed
directories, tiles by offset) and MBTiles (SQLite). A new kind of record
in `map-tiles.servers`, "a file", beside the addresses; its zoom range,
bounds and attribution from the file's own metadata.

What must hold:

- **Raster tiles only**, as ADR-020 decided: PNG, JPEG and WebP tiles are
  decoded like downloaded ones. Most offline maps are vector tiles (MVT,
  Protomaps' basemaps): drawing them needs a style and a renderer, which
  ADR-020 put out of scope; a file of vector tiles is said to be one and
  refused, until an ADR of its own decides whether bibli draws them (a
  fixed simple style on the workers that draw the outline is the option
  seen).
- **Readers bounded like every other**: PMTiles' directories are read
  with every length checked; MBTiles needs SQLite, the system's
  `libsqlite3` rather than a bundled copy (or `rusqlite`'s bundled build
  if the system's cannot be relied on, a size to weigh).
- **No network**: a PMTiles address over HTTP (range requests) is a tile
  server, and would be `map-tiles`' under ADR-020's rules, not this.

## Consequences

Nothing changes in the code. `docs/PLAN.md`'s "Later" line points here.
The parts depend on one another in one direction: place names serve
`near:` by name; the field contract, if chosen, serves the Map view's
area search and the geotagging review; each can start alone. When one
starts, its section here is amended with what was decided and measured,
or replaced by an ADR where it says so (the field contract in the core,
vector tiles, any use of the network).

Tests and fixtures stay synthetic: invented coordinates, invented place
names, and pictures written by the tests; never a real track, photo or
location.
