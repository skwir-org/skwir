# ADR-022: Maps, later: search by place, writing locations, names and backgrounds offline

Status: parts 1 to 3 accepted and built, 2026-09-27 (the "Decided"
paragraphs below). Part 4: tile archives previewed, 2026-10-05; using them
as backgrounds is proposed, not built (checked in `crates/plugins/map-tiles`
on 2026-10-11). Amended 2026-10-05 (SQLite on Windows).

## Context

ADR-020's maps are built (steps A to D′ of `docs/PLAN.md`, "Maps"). Four
features were left for later because each closes a door:

1. Finding files by where they are (`near:`): a contract between plugins.
2. Writing locations (geotag from a track, remove before sharing): the
   first change to file content Skwir would make on its own.
3. Place names without the network ("Near Example Town").
4. Backgrounds from a local file (PMTiles, MBTiles).

Constraints binding every part:

- **Positions are not metadata** (ADR-020): `geo` finds them on workers,
  keyed by identity (device, inode, size, mtime) in memory and the cache's
  `geo` namespace.
- **Plugins do not depend on each other:** query and geo meet only in core
  data or commands.
- **Mutations are plans with inverses:** a plugin may register an operation
  kind (`OpRegistry`, owner-tagged, like `set_tags`); a batch, or anything
  not from a single user gesture, goes through `App::propose`; undo and
  redo go through the journal.
- **The network is `map-tiles`' only** (ADR-020); a new use needs its own
  ADR. Nothing a file names is fetched.
- **Dates are the user's** (ADR-017): a camera clock has no zone and is
  kept as digits (`exif::Taken::clock`), an order, not an instant.
- **Light** (ADR-000): the binary already carries 514 KB of outline.

## Decision

Each part is decided when its work starts, by amending its section or by
its own ADR where it says so.

### 1. Search by place: `near:`

`near:10.12,20.12~5km` finds files within 5 km of a point; the Map view's
area becomes a search of the files inside (`inside:` a box). The query
plugin owns the syntax and chips (`facets.rs`); distances are geodesic,
boxes may cross ±180°. How does a search get a candidate's position
cheaply over thousands of files?

- **A core contract for fields read on demand** (batched, on workers,
  cancellable, kept by identity): chosen, since search and views both need
  it and other costly fields will follow (a duration, a page count).
- A batched `geo.positions { paths }` command: rejected, a contract only
  two plugins know, which the next consumer would copy.
- An index: rejected, Skwir keeps no database (principle 3).

**Decided (2026-09-27):** the core contract, in ADR-023
(`skwir_core::fields`; geo answers `position` for pictures and videos).
`near:LAT,LON~DISTANCE` (1 km by default) and `inside:S,W,N,E` are
evaluated last, like `content:` (only when cheaper terms have not
decided), and a large search reports it is still looking rather than
blocking. The Map view's "This area" searches the folder and subfolders for
pictures and videos in the area shown (`geo.search_area`). Terms typed in
the address bar are read as a location's scheme, as `kind:` always was: a
search is typed in the Search box or after `query:`. Not built: `near:` by
place name; meanwhile `geo.locate { name }` gives scripts and agents a
name's positions.

### 2. Writing locations: geotag from a track, remove before sharing

Geotag: each picture taken while a track was recorded gets the track's
position at that moment, interpolated, none across a gap of more than a few
minutes. Remove: take the location out, in place or in copies, before
pictures leave the machine. What must hold:

- **Plans, reviewed, undoable:** one step per file, a geo operation kind
  (`set_position`) whose inverse restores the tags read when the plan was
  made and whose check refuses a file changed since; batches proposed for
  review; re-applying an applied plan is a no-op.
- **The camera clock is not an instant:** matching needs its offset from
  the track's UTC (the user's zone on that date, ADR-017, plus the
  camera's error), shown and editable in the review.
- **Provenance:** a written position is `Provenance::User` once the user
  confirms the plan; until then a suggestion.
- **Writing is harnessed** (principle 4): ExifTool writes every format that
  matters (JPEG, HEIC, TIFF, RAW, XMP sidecars, QuickTime keys) and keeps
  what it does not touch. It is a `Plugin::requirements` probe, run through
  `skwir_platform::process` with separate arguments; without it the
  commands say what is missing. Rejected: a JPEG-only writer of our own.
- A RAW file is written through its XMP sidecar, never in place.
- **Removing** is the same operation with no position; "copy without
  location" is a copy plan, then removal on the copies.

**Decided (2026-09-27):** as above, ExifTool the only writer, mtime kept to
the nanosecond (`-P` keeps whole seconds; the exact time is put back).

- `geo.geotag` matches the selected pictures with the one selected track
  on a worker and proposes the plan; `geo.remove_location` applies at
  once, undoable.
- A picture stating its zone (`OffsetTimeOriginal`) is matched through it;
  one already placed is left alone; RAW pictures are left out (sidecars
  not written yet), which the plan's note says.
- Removal takes out every tag saying where the picture was made, and undo
  writes back exactly what the plan read, structures included (ExifTool's
  JSON import): EXIF GPS, XMP `GPS*` tags in any namespace (drone makers'
  included), QuickTime coordinates, a drone's altitudes, IPTC city,
  sub-location, province and country (IIM and XMP), Photoshop's, and IPTC
  `LocationCreated` and `LocationShown`. Where the camera looked (a drone's
  flight and gimbal angles) stays.
- Writing a position clears only the position: place names stay, and a
  picture with only a place name is not "already placed".
- A step may state its words (`op::Words`) and a plan a note under its
  title (`Plan::note`).
- Not built: editing the camera offset in the review (the command takes
  `offset_seconds`), and showing the matches on the Map view as
  suggestions.

### 3. Place names offline

The nearest place's name for a position: in Details, for a track's ends,
as Map labels at low zooms, and for `near:` by name. The network needs its
own ADR, and no desktop service answers offline (GNOME's geocoding asks
Nominatim).

| Data | Licence | Verdict |
| --- | --- | --- |
| Natural Earth populated places (1:10m) | public domain | built in: cities and regions |
| GeoNames `cities1000` (places over 1 000 people) | CC BY 4.0 | optional data file, with attribution |
| OpenStreetMap-derived | ODbL | not proposed: share-alike on a derived database is a decision of its own |

Names are in the data's language; localized names are left out (they
multiply the data).

**Decided (2026-09-27):** as proposed. Natural Earth's 7 342 places are
built in (126 KB; 178 KB decoded on first use in 0.5 ms), looked up
through a grid, released with the plugin. GeoNames `cities1000`, packed by
`tools/maps/places.py`, replaces it when installed under the XDG data
directories (`skwir/places/geonames.bin`, 161 535 places, 4.2 MB decoded),
credited in Details. A position takes the name of the place whose extent
holds it, else "12 km NE of Example Town" for the nearest place relative
to how far its name carries (15 to 100 km with its size). Details shows it
as "Place" (field `geo.place`). Not built: map labels.

### 4. Backgrounds from a local file

A map background read from a user-chosen file, no server: PMTiles (header,
compressed directories, tiles by offset) or MBTiles (SQLite). Proposed as a
new record kind in `map-tiles.servers`, "a file", with zoom range, bounds
and attribution from the file's metadata. What must hold:

- **Raster tiles only** (ADR-020): PNG, JPEG, WebP decoded like downloads.
  Most offline maps are vector tiles (MVT, Protomaps): a vector file is
  identified and refused until an ADR decides whether Skwir draws them (the
  option seen: a fixed simple style on the outline's workers).
- **Bounded readers:** PMTiles directories with every length checked;
  MBTiles through the system's `libsqlite3`, not a bundled copy, unless the
  system's proves unreliable (a size to weigh).
- **No network:** PMTiles over HTTP range requests is a tile server, under
  ADR-020's rules.

**Decided (2026-10-05), first step: archives previewed.** A PMTiles or
MBTiles file previews as the map it holds; using one as a background stays
proposed.

- `map-tiles` reads them (`archive.rs`, `pmtiles.rs`, `mbtiles.rs`) and
  previews them ahead of the tables preview, which would take an
  `.mbtiles` as any SQLite database. Not in the MIME database: claimed by
  extension, confirmed by first bytes. PMTiles v3 only, gzip or
  uncompressed (Brotli and Zstandard named, not read); every offset checked
  against its section and the file, inflation bounded. MBTiles opened as
  the tables preview opens a database (read-only, immutable, schema
  untrusted, interrupted on cancel).
- **The file's tiles draw over the background** where it has them. The core
  contract grew for it: a source states its zooms and area
  (`SourceInfo::min_zoom`, `bounds`, `holds`) and answers `NoTile` for a
  tile it may hold but does not (left blank, not asked again); an
  inspection carries a file's tiles as a `tiles::Layer` (id, source, info
  read once). The UI keeps a layer's tiles by id beside the background's,
  draws its attribution above, and draws its tiles up to two zooms out of
  its shallowest and enlarged past its deepest.
- **Vector tiles are not drawn:** the preview says so, outlines the area
  covered, and lists the layers in Details.
- Checked against files from the reference writer (`pmtiles convert`
  1.31.2, from MBTiles that `tools/maps/test_archives.py` writes; kept in
  `crates/plugins/map-tiles/src/testdata/`): every tile in place, told by
  its colour.

## Consequences

- The parts depend one way: place names serve `near:` by name; the field
  contract serves the Map view's area search and the geotag review.
- What remains (backgrounds from a file, vector tiles, any network use) is
  decided by amending its section, or by its own ADR where it says so.
- Tests and fixtures stay synthetic: invented coordinates and place names,
  pictures written by the tests; never a real track, photo or location.

## Amendments

### 2026-10-05: SQLite on Windows (ADR-035)

Windows has no system `libsqlite3`, so SQLite is built in there
(`rusqlite`'s `bundled-windows`); Linux and macOS keep the system's.
