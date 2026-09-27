# ADR-020: Maps, their backgrounds, and bibli's first network requests

Status: accepted, 2026-09-27. Previews on a map, their backgrounds and the
Map view are built (steps A, B and C of `docs/PLAN.md`, "Maps").

## Context

Two features need a map. A GPS track or route (GPX, KML, GeoJSON), or a
flight or drone plan, should
preview as its line on a map, with the facts a hiker or cyclist asks for
(distance, time, climb). Photos that carry a position (EXIF GPS) should be
shown where they were taken, in a pane view beside List, Grid and Columns,
with selection, opening and the preview working as in any other view.

Tracks and markers are easy. The land and the streets under them are not.
A map with nothing under its lines says little, and every usable map of
the streets is served over the network, which bibli has never used. Tile
requests say where the user looks, so for photos, where the user has
been. This ADR decides how the network arrives for maps.

The OpenStreetMap Foundation's tile policy
(<https://operations.osmfoundation.org/policies/tiles/>) binds any
application using `tile.openstreetmap.org`: a User-Agent that names the
application, "© OpenStreetMap contributors" visible on the map, tiles kept
at least 7 days or as long as their caching headers say, conditional
requests for expired tiles, no fetching of tiles the user is not viewing,
and no offline downloads. Access can be blocked without notice.

## Decision

- **Every background is raster tiles** served through one core contract,
  `tiles::TileSource`: owner-tagged, asked for `{z, x, y, scale, palette}`
  with a cancel token, answering RGBA pixels through a callback. A request
  is asynchronous so a download never holds a scheduler worker: the source
  decides where the work runs. The UI draws tiles and its overlays (lines,
  markers) and knows no source. Rendering vector tiles, a styling engine of its own, is out
  of scope.
- **Three sources, in a plugin of their own, `map-tiles`**:
  - *Outline*, the default: Natural Earth's land, lakes and borders
    (public domain), simplified, drawn into tiles on a worker in the
    theme's colors. It never uses the network, and is loaded only when a
    map is drawn.
  - *OpenStreetMap*, off until the user chooses it
    (`map-tiles.source`). Requests follow the policy above: a User-Agent
    that names bibli and its version, the attribution drawn on the map
    whenever one of its tiles is, caching headers honoured (7 days when
    there are none), `If-None-Match`/`If-Modified-Since` on expired tiles,
    only tiles in view, no offline download.
  - *Custom*, a URL template (`https://…/{z}/{x}/{y}.png`) with its own
    attribution text, for a provider with a key or a server of one's own.
- **The network stays in `map-tiles`.** No file ever causes a request: a
  KML `NetworkLink`, a remote icon or style is shown as text, never
  fetched. Disabling the plugin withdraws every source and cancels
  requests in flight. Maps then draw a plain themed background with a
  graticule, and the preview and the view keep working. When Phase 6's
  permission layer exists, `map-tiles` is the plugin that asks for
  `network`.
- **Choosing a network source is the user's act.** A map shown on the
  outline says, once, that street detail needs a map source, with a link
  to the setting (`set_status_about`).
- **HTTP** (changed by the addendum on loading below): hyper with rustls
  and the system's certificates (`rustls-platform-verifier`), HTTP/2 where
  the server speaks it, on a runtime of the plugin's own; first `ureq` on
  two threads. The client is private
  to `map-tiles`. There is no network layer in the core: remote file
  systems come from installed tools (mounts, rclone; ADR-021), not from
  protocols implemented in bibli. If a second plugin needs HTTP (a
  model provider), the client moves to `bibli-platform`, a utility beside
  the others, not a contract.
- **Caches**: downloaded tiles live in a namespace of the one deletable
  cache (`bibli_core::cache`) with their validators, trimmed like ADR-003's
  namespaces. The UI keeps a bounded set of decoded tiles, released when no
  map is shown. Nothing is loaded at startup.
- **Projection**: Web Mercator, as every tile source uses, clamped at
  ±85.0511°. Pure, tested math in `bibli-ui`'s map element. Size-dependent
  placement happens in prepaint. A line keeps its longitude continuous
  across the ±180° meridian (each point within half a world of the one
  before), so it is drawn across the meridian, never across the world.
- **Plugins**: `geo` owns the formats (GPX, KML/KMZ, GeoJSON, then TCX,
  FIT, IGC, NMEA, flight plans and drone missions), the preview provider,
  track thumbnails, the Map pane view (a `ViewRole::Lens`, ADR-007) and
  the positions scanner. `map-tiles` owns the backgrounds. Neither depends on the other. Core gains only what
  the two plugins and the UI share: the `geo` geometry types, the preview
  inspection's `geo` field (ADR-009), and `tiles`.
- **Positions are not a hydration field.** Reading EXIF for every picture
  of every listing would slow every folder for users who never open a map.
  The Map view asks `geo` for positions of what it shows, on workers,
  cached by file identity, size and mtime in the deletable cache (the
  addendum on the Map view below). A query facet over positions (`near:`) needs a field
  filled on demand, a core decision left to its own ADR.

## Consequences

Bibli makes network requests for the first time, and only after the user
chose a network source. Until then, maps show the outline, which is enough
to see a country or a region and not a street. The outline's data adds
514 KB to the binary (Natural Earth 5.1.2: 1:110m below zoom 3, 1:50m
from it; about 110 000 points, packed by `tools/maps/natural_earth.py`).
It is decoded when a map first needs it, in 7 ms, and kept, about 1 MB,
while the plugin is enabled; a tile is drawn in 0.25 to 7 ms on a worker
(release build, reference machine).

The OSM server can refuse bibli at any time; the custom source is the way
out, and the outline remains. Offline street maps would come from a local
PMTiles or MBTiles file, which the contract admits as another source.

Location data is personal. Tests, fixtures and bench data use invented
coordinates, never a real track or photo.

## Addendum (2026-09-27): tile servers, their headers and keys

One custom address was not enough: people keep several backgrounds
(topographic, satellite, a provider's own), many need an API key, and some
need headers (a Referer the key is bound to, an Authorization).

- **Tile servers are a list** (`map-tiles.servers`, a records setting,
  ADR-010 addendum): a name, an address, subdomains, an attribution, the
  deepest zoom, headers, and an API key. The background choice
  (`map-tiles.source`) lists the outline, OpenStreetMap and every server
  (`server:<id>`); the plugin declares it again when the servers change,
  and chooses the outline again when the server chosen is removed. The
  single address of the first version becomes the list's first server.
- **Addresses take the common forms**: `{z}`, `{x}`, `{y}`; `{-y}` for a
  server counting rows from the bottom (TMS); `{q}` for a quadkey (Bing);
  `{s}` for a subdomain (the same one for the same tile); `{r}`, `@2x` on
  a dense screen; `{key}`. An unknown part, or `{s}` without subdomains, is
  refused when the server is saved.
- **Headers** are sent with every request to that server: `User-Agent`
  replaces bibli's, `Referer`, `Authorization` and the like as written.
  What the HTTP layer writes itself (`Host`, `Content-Length`,
  `Connection`, the conditional headers…) is refused. OpenStreetMap's
  built-in source takes none: its policy wants bibli's own User-Agent.
- **The API key lives in the keyring**, never in the settings: `{key}` in
  the address or a header is replaced when a request is made, on the
  download threads, which read the keyring once per server until the
  settings change. The cache keeps a tile under the server's id and what
  shapes its tiles, never under an address holding the key, and an error
  quoting the address has the key replaced before anyone sees it.
- **The map chooses its background too**: a Background button in the map
  preview's toolbar lists the same choices and applies them through
  `settings.apply`, with a way to the servers in Settings. Each server has
  a Test, which asks it for one tile now and says what it answered.

## Addendum (2026-09-27): loading tiles as a map library does

Measured against a local server answering each tile after 150 ms (the
debug trace `map tiles: the view has them all`, `map_tiles.rs`), a map
waited 626 ms for the six tiles it opened on, 1 s after a fast pan and
1.9 s after zooming out two levels, and its middle arrived last. The
causes, and what MapLibre does instead:

- **The middle first.** A map asks for the tiles farthest from its middle
  first, since a source serves its newest request first (the `tiles`
  contract says so). Downloads are served by a ticket taken when the map
  asks, not by when the job reached the queue: the scheduler runs its jobs
  newest first too, and two such queues in a row gave the reverse order.
- **A tile downloaded is kept**, even when its view moved on before it
  arrived: the request was made, and the view may come back.
- **Finer tiles stand in** when zooming out, as coarser ones already did
  when zooming in.
- **HTTP/2.** `ureq` (HTTP/1.1, two connections) is replaced by hyper,
  hyper-rustls and h2, the versions gpui already links, on one thread
  running a tokio runtime of its own: six requests in flight, which on
  HTTP/2 are streams of one connection and on HTTP/1.1 as many kept-alive
  connections as a browser opens. Whether a server speaks HTTP/2 is learnt
  only once a connection is open, so the first request to a server not
  connected to for 50 s goes alone and the others wait for its
  connection: without that, a view's burst opened one connection per
  tile. The TLS configuration is the client's parameter: the application
  passes the system's, and only its tests pass another (a throwaway
  authority in `src/testdata`). Arrivals needed no batching: the window
  already draws them once per frame.

Measured again the same way, over HTTP/1.1: the middle of the view in
175 ms and the whole view in 431 ms on opening; the middle 45 ms after a
pan began; the whole view in about 1 s after zooming out, the middle in
about 230 ms. Over HTTP/2 (the client's test, a local TLS server, 150 ms a
tile), twelve tiles took about two round trips on one connection. Zooming
out now asks for 24 tiles where it asked 18: with more in flight, the
tiles of the zooms passed through are fetched while shown. Not measured:
a real network, and OpenStreetMap's own server.

## Addendum (2026-09-27): the Map view

- **A scan, then batches.** `geo.scan { pane }` snapshots what the pane
  lists (a folder, a search, a timeline's bucket alike) and returns; which
  files can say where they are (pictures by the MIME database's kind,
  tracks by extension) and what they say are found on four workers.
  `geo.points { pane, since }` hands over the points found after the
  first `since`, so each travels once; the plugin advances its view
  descriptor's revision and wakes the UI at most every 200 ms. A track is
  its line thinned to 400 points. Positions are kept by identity (device,
  inode, size, modification time to the nanosecond) in memory and in the
  cache's `geo` namespace, so a folder seen before is placed at once.
- **What was shown stays until something moved** (the rule Usage follows,
  ADR-007). A scan ends complete only if its listing was: one started
  while the folder streams goes on, in the next scan, from what it looked
  at, so markers are not taken back at each batch. Once complete, a change
  of the listing's names or modification times (a picture geotagged by
  another program) has the files looked at again beside what is shown,
  and the result replaces it, under a new generation, only if a point
  differs. Hydration moving the times therefore costs a stat per candidate
  on a worker and nothing on screen.
- **Not in view order.** The plan said visible entries first; a map has
  no rows, and before the positions are known nothing says which files
  are in view. The scan goes in listing order; its cache makes a second
  look immediate.
- **Subfolders are a search**, not a walker of the plugin's own: the
  placeholder's action and the header's Subfolders button dispatch
  `query.search` with `(kind:image OR ext:gpx,kml,kmz,geojson)`, and the
  view scans that search's listing like any other.
- **Drawn by `elements/map.rs`**, over the same backgrounds as the preview.
  Markers closer than 48 px merge greedily into a bubble with their count,
  which a click opens (the map zooms to fit its members, or selects them
  when they cannot be told apart). While at most 60 markers stand alone in
  view, a picture's marker is its thumbnail, cropped to a 40 px square.
  A click selects (Ctrl toggles), Shift and a drag selects what a
  rectangle holds, a double click opens; the arrows move the map, the zoom
  keys zoom it and Home fits it to its files.
- **Files at one place are spread, not only counted.** A bubble that
  zooming cannot pull apart (its files share a position, or the map is at
  its deepest zoom) opens in place on a click, until a click beside it;
  Spread (`ui.map_view_spread`, the header's button) does it for every
  place instead of bubbles. The files go on a sunflower spiral around
  their point (`math::spread`), in the order their cameras' clocks give
  (EXIF `DateTimeOriginal`, kept by the scanner as digits: an order, not
  an instant), the undated last, each with a line to the point. Spirals
  that would overlap join around their mean point until none do, so
  nothing hides anything. A spiral of up to eight shows 40 px pictures,
  a larger one smaller ones down to 20 px, past 120 dots; at most 200
  pictures are asked for in view, the rest are dots in their places.
  Markers up to 400 px outside the map are laid out too, so a spiral
  reaching into view is whole; past 3 000 in that reach, Spread gives
  way to bubbles.
