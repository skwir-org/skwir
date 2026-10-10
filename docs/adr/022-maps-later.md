# ADR-022: Maps, later: search by place, writing locations, names and backgrounds offline

Status: parts 1 to 3 accepted and built, 2026-09-27 (their decisions are
the "Decided" paragraphs below); of part 4, archives previewed, 2026-10-05,
and backgrounds from them proposed and deferred. Each part is decided when
its work starts, by amending this ADR or by an ADR of its own where it
says so. Amended 2026-10-05 (Phase 5, ADR-035): on Windows SQLite is
built in (`rusqlite`'s `bundled-windows`), there being no system
`libsqlite3` to link; Linux and macOS keep the system's.

## Context

The maps of ADR-020 are built through its steps A to D′ (`docs/PLAN.md`,
"Maps"): previews of tracks, plans and missions, backgrounds offline and
from tile servers, the Map view of a pane's pictures, videos and tracks.
Four things were left for later because each closes a door:

1. Finding files by where they are (`near:`), which needs positions a
   search can ask for, and so a contract between plugins.
2. Writing locations: geotagging pictures from a track, and removing
   locations before sharing. The first mutation of a file's content that
   skwir would make on its own.
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
- **An index.** Ruled out: skwir keeps no database (ADR-000, principle 3).

Leaning: the core contract, since two consumers (search, views) and more
than one field are foreseen; the geo plugin's cache becomes its store.
Whatever is chosen: a position term is evaluated late, as `content:` is
(only when the cheaper terms have not decided), and a search of a large
tree reports it is still looking rather than blocking.

**Decided (2026-09-27):** the core contract, in ADR-023
(`skwir_core::fields`; the geo plugin answers `position` for pictures
and videos). `near:LAT,LON~DISTANCE` (1 km unsaid) and
`inside:S,W,N,E` are query terms decided last, as `content:`; the Map
view's "This area" searches the folder and its subfolders for the
pictures and videos inside the area shown (`geo.search_area`). Not
built: `near:` by a place's name (the query language would need the
names, which are the geo plugin's; `geo.locate { name }` gives a name's
positions to scripts and agents meanwhile). Terms typed in the address
bar are read as a location's scheme, as `kind:` always was: a search is
typed in the Search box, or after `query:`.

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
  `skwir_platform::process` with separate arguments. Without it, the
  commands say what is missing and where to get it. A writer of skwir's
  own for JPEG alone is the fallback considered, not the plan.
- **What else changes.** Whether the file's modification time is kept
  (a folder sorted by date should not reorder) is decided with the
  first user; a RAW file is written through its XMP sidecar, never in
  place.
- **Removing** is the same operation with no position; "copy without
  location" is a copy plan followed by it on the copies, so the
  originals stay as they are.

**Decided (2026-09-27):** as proposed, with ExifTool the only writer and
a file's modification time kept (to the nanosecond: `-P` keeps whole
seconds, the exact time is put back). `geo.geotag` matches the pictures
selected with the one track selected, on a worker, and proposes the
plan; `geo.remove_location` is applied at once, undoable. A picture that
states its own time zone (`OffsetTimeOriginal`) is matched through it;
a picture already placed is left as it is; RAW pictures are left out
(their sidecars are not written yet) and the plan's note says so.
Removal takes out every tag that says where the picture was made, and
undo writes back exactly what the plan read, structures included
(ExifTool's JSON import): the position (EXIF GPS, XMP tags named GPS* in
any namespace, a drone maker's among them, QuickTime's coordinates),
a drone's altitudes, and the place: IPTC's city, sub-location,
province and country (IIM and XMP), Photoshop's, and IPTC's
`LocationCreated` and `LocationShown` structures. What stays is where
the camera was looking (a drone's flight and gimbal angles). Writing a
position clears only the position: a picture's place names stay, and a
picture with only a place name is not "already placed". For the review and
the toast, a step may state its words (`op::Words`, what it will do and
what it did) and a plan a note under its title (`Plan::note`). Not
built: changing the cameras' offset from the review (the command takes
`offset_seconds`), and showing the matches on the Map view.

## 3. Place names offline

What it is: the nearest place's name for a position, in Details ("Near
Example Town"), for a track's ends ("from … to …"), as the Map view's
labels at low zooms, and for `near:` by name.

The network is not an option here without an ADR of its own, and no
desktop service answers offline (GNOME's geocoding asks Nominatim over
the network). The data comes with skwir or is installed beside it:

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

**Decided (2026-09-27):** as proposed. Natural Earth's 7 342 populated
places are built in (126 KB, 178 KB decoded on first use in 0.5 ms);
GeoNames `cities1000`, packed by `tools/maps/places.py`, is used
instead when installed under the XDG data directories
(`skwir/places/geonames.bin`, 161 535 places, 4.2 MB decoded) and
credited in Details. A position is named after the place whose extent
holds it, else "12 km NE of Example Town" for the place nearest relative
to how far its name carries (15 to 100 km with its size). Details shows
it as "Place" (the field `geo.place`); map labels from it are not built.

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
  refused, until an ADR of its own decides whether skwir draws them (a
  fixed simple style on the workers that draw the outline is the option
  seen).
- **Readers bounded like every other**: PMTiles' directories are read
  with every length checked; MBTiles needs SQLite, the system's
  `libsqlite3` rather than a bundled copy (or `rusqlite`'s bundled build
  if the system's cannot be relied on, a size to weigh).
- **No network**: a PMTiles address over HTTP (range requests) is a tile
  server, and would be `map-tiles`' under ADR-020's rules, not this.

**Decided (2026-10-05), the first step: archives previewed.** A PMTiles
or MBTiles file previews as the map it holds; using one as a background
is still as proposed above.

- `map-tiles` reads them (`archive.rs`, `pmtiles.rs`, `mbtiles.rs`) and
  provides their preview, before the tables preview, which would take an
  `.mbtiles` file as any SQLite database. Neither format is in the MIME
  database: each is claimed by its extension and confirmed by its first
  bytes, as the geo plugin's formats are. PMTiles version 3 only, with
  gzipped or uncompressed parts (Brotli and Zstandard are said, not
  read); every offset checked against its section and the file, what is
  inflated bounded. MBTiles through the system's `libsqlite3`, opened as
  the tables preview opens a database (read-only, immutable, schema not
  trusted, cancellation interrupting it).
- **The preview's map draws the file's own tiles over the background**
  where it has them. The core contract grew for it, as a background from
  a file will need too: a source says the zooms and the area it holds
  (`SourceInfo::min_zoom`, `bounds`, `holds`), and answers `NoTile` for a
  tile it may hold but does not, which is left blank and not asked again;
  an inspection carries a file's tiles as a `tiles::Layer` (an id, the
  source, its info read once). The UI keeps a layer's tiles beside the
  background's, keyed by the layer's id, and draws its attribution above
  the background's. Its tiles are drawn up to two zooms out of its
  shallowest, smaller, and enlarged past its deepest.
- **Vector tiles are not drawn**, as above: the preview says so, outlines
  the area the file covers, and lists its layers in Details.
- Checked against files the reference writer made (`pmtiles convert`
  1.31.2, from MBTiles files `tools/maps/test_archives.py` writes; kept in
  `crates/plugins/map-tiles/src/testdata/`): every tile where it was put,
  told by its colour.

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
