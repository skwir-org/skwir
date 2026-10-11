# ADR-020: Maps, their backgrounds, and Skwir's first network requests

Status: accepted, 2026-09-27. Built: previews on a map, their backgrounds
and the Map view (steps A, B and C of `docs/PLAN.md`, "Maps"). Amended
2026-09-27 (tile servers, loading, the Map view, more formats).

## Context

Two features need a map: a track, route, flight or drone plan should
preview as its line with distance, time and climb; photos with a position
(EXIF GPS) should show where they were taken, in a pane view that selects,
opens and previews like any other.

Lines and markers are easy; the map under them is not. Every usable street
map is served over the network, which Skwir had never used, and tile
requests reveal where the user looks (for photos, where they have been).
This ADR decides how the network arrives.

The OpenStreetMap Foundation's tile policy
(<https://operations.osmfoundation.org/policies/tiles/>) requires: a
User-Agent naming the application, "© OpenStreetMap contributors" visible,
tiles kept at least 7 days or as their caching headers say, conditional
requests for expired tiles, no tiles the user is not viewing, no offline
downloads. Access can be blocked without notice.

## Decision

- **Every background is raster tiles** behind one core contract,
  `tiles::TileSource`: owner-tagged, asked for `{z, x, y, scale, palette}`
  with a cancel token, answering RGBA through a callback. Asynchronous, so a
  download never holds a scheduler worker. The UI draws tiles and overlays
  and knows no source. Vector tiles (a styling engine) are out of scope.
- **Sources, in the `map-tiles` plugin:**
  - *Outline* (default): Natural Earth land, lakes and borders (public
    domain), simplified, drawn on a worker in theme colours. No network;
    loaded only when a map is drawn.
  - *OpenStreetMap*, off until chosen (`map-tiles.source`), following the
    policy point by point (User-Agent with Skwir's version, 7 days without
    caching headers, `If-None-Match`/`If-Modified-Since`).
  - *Custom*: a URL template with its own attribution, for a keyed
    provider or one's own server (a list since the first amendment).
- **The network stays in `map-tiles`.** No file ever causes a request: a
  KML `NetworkLink`, a remote icon or style is shown as text. Disabling the
  plugin withdraws the sources and cancels requests; maps then draw a
  themed background with a graticule, and keep working. `map-tiles`
  declares the `network` capability (ADR-044).
- **A network source is the user's choice.** A map on the outline says once
  that street detail needs a map source, linking the setting
  (`set_status_about`).
- **HTTP is private to `map-tiles`:** hyper, rustls with the system's
  certificates (`rustls-platform-verifier`), HTTP/2 where offered, on the
  plugin's own runtime (`ureq` at first). The core has no network layer;
  remote file systems come from installed tools (ADR-021). If a second
  plugin needs HTTP, the client moves to `skwir-platform` as a utility, not
  a contract.
- **Caches.** Tiles and their validators live in a namespace of the
  deletable cache (`skwir_core::cache`), trimmed like ADR-003's. The UI
  keeps a bounded set of decoded tiles, released when no map shows. Nothing
  loads at startup.
- **Projection:** Web Mercator clamped at ±85.0511°, pure tested math in
  the map element, placement in prepaint. Longitudes stay continuous across
  ±180° (each point within half a world of the previous), so a line crosses
  the meridian, not the world.
- **Plugins.** `geo` owns formats, the preview provider, track thumbnails,
  the Map view (a `ViewRole::Lens`, ADR-007) and the positions scanner;
  `map-tiles` owns backgrounds. Neither depends on the other. Core gains
  only what they and the UI share: the `geo` types, the inspection's `geo`
  field (ADR-009), and `tiles`.
- **Positions are not a hydration field:** reading EXIF for every listed
  picture would slow every folder for users who never open a map. The Map
  view asks `geo` for what it shows, on workers, cached by file identity.
  The `near:` facet's field read on demand was decided in ADR-023 and built
  (ADR-022).

## Consequences

- Skwir makes network requests only after the user chose a network source;
  until then the outline shows countries and regions, not streets.
- The outline adds 514 KB to the binary (Natural Earth 5.1.2: 1:110m below
  zoom 3, 1:50m above; ~110 000 points, packed by
  `tools/maps/natural_earth.py`). Decoded on first use in 7 ms and kept
  (~1 MB) while the plugin is on; a tile draws in 0.25–7 ms on a worker
  (release build, reference machine).
- OSM can refuse Skwir at any time; custom servers and the outline remain.
  Offline street maps would come from local PMTiles or MBTiles files, as
  another source.
- Location data is personal: tests, fixtures and bench data use invented
  coordinates, never a real track or photo.

## Amendments

### 2026-09-27: tile servers, their headers and keys

- **Servers are a list** (`map-tiles.servers`, a records setting,
  ADR-010): name, address, subdomains, attribution, deepest zoom, headers,
  API key. `map-tiles.source` lists the outline, OpenStreetMap and each
  server (`server:<id>`), and falls back to the outline when the chosen
  server is removed.
- **Address placeholders:** `{z}`, `{x}`, `{y}`, `{-y}` (TMS), `{q}` (Bing
  quadkey), `{s}` (subdomain, stable per tile), `{r}` (`@2x` on dense
  screens), `{key}`. Unknown ones, or `{s}` without subdomains, are refused
  on save.
- **Headers** go with every request to that server; `User-Agent` replaces
  Skwir's. Headers the HTTP layer writes (`Host`, `Content-Length`,
  `Connection`, conditional headers…) are refused. The built-in OSM source
  takes none: its policy wants Skwir's User-Agent.
- **The API key lives in the keyring**, never in settings. `{key}` is
  substituted at request time on the download threads (keyring read once
  per server until settings change). Tiles are cached by server id and tile
  shape, never by an address holding the key, and errors quoting the
  address have the key masked.
- **Background button** in the map preview's toolbar: the same choices,
  applied through `settings.apply`, with a link to the servers. Each server
  has a Test that fetches one tile and reports the answer.

### 2026-09-27: loading tiles as a map library does

Against a local server answering each tile after 150 ms (trace
`map tiles: the view has them all`, `map_tiles.rs`), a map waited 626 ms
for its six opening tiles, 1 s after a fast pan, 1.9 s after zooming out two
levels, the middle arriving last. Changes, after MapLibre:

- **Middle first.** The map asks for the farthest tiles first, since a
  source serves the newest request first (a `tiles` contract rule).
  Downloads are ordered by a ticket taken when the map asks, not by queue
  arrival: the scheduler also runs newest first, and two such queues
  reversed the order.
- **A downloaded tile is kept** even if the view moved on: it may come back.
- **Finer tiles stand in** when zooming out, as coarser ones do zooming in.
- **HTTP/2.** hyper, hyper-rustls and h2 (versions gpui already links) on
  one thread with its own tokio runtime replace `ureq` (HTTP/1.1, two
  connections): six requests in flight, as streams of one HTTP/2
  connection or as many kept-alive HTTP/1.1 connections. A server idle for
  50 s gets one request first and the rest wait for its connection,
  otherwise a burst opens one connection per tile. TLS configuration is a
  client parameter: the system's in the app, a throwaway authority in tests
  (`src/testdata`).

Measured again the same way (HTTP/1.1): opening, middle 175 ms and whole
view 431 ms; pan, middle 45 ms; zoom out, middle ~230 ms and whole view
~1 s. Over HTTP/2 (client test, local TLS, 150 ms a tile), twelve tiles took
about two round trips on one connection. Zooming out now asks for 24 tiles
instead of 18 (passed-through zooms are fetched while shown). Not measured:
a real network, OSM's own server.

### 2026-09-27: the Map view

- **A scan, then batches.** `geo.scan { pane }` snapshots what the pane
  lists (folder, search, timeline bucket) and returns; four workers find
  and read positions (pictures by MIME kind, tracks by extension).
  `geo.points { pane, since }` hands over points found since `since`; the
  plugin bumps its view revision and wakes the UI at most every 200 ms. A
  track is thinned to 400 points. Positions are cached by identity (device,
  inode, size, mtime to the nanosecond) in memory and the cache's `geo`
  namespace.
- **What was shown stays until something moved** (ADR-007's rule). A scan
  is complete only if its listing was; one started while the folder
  streams continues in the next scan, so markers are not withdrawn per
  batch. Once complete, changed names or mtimes rescan those files beside
  what is shown, replacing it (new generation) only if a point differs.
- **Listing order, not view order:** before positions are known nothing says
  what is in view; the cache makes a second look immediate.
- **Subfolders are a search:** the placeholder's action and the Subfolders
  button dispatch `query.search` with
  `(kind:image OR ext:gpx,kml,kmz,geojson)`, scanned like any listing.
- **Drawn by `elements/map.rs`** over the preview's backgrounds, chosen in
  the pane toolbar's menu (since 2026-10-05: `map_preview::background_rows`,
  `ui.toolbar_menu` with `kind: map_backgrounds`). Markers closer than
  48 px merge into a counted bubble; a click zooms to fit it, or selects its
  files when zoom cannot separate them. With at most 60 lone markers in
  view, a picture's marker is its 40 px thumbnail. Click selects (Ctrl
  toggles), Shift+drag selects a rectangle, double click opens; arrows pan,
  zoom keys zoom, Home fits.
- **Files at one place are spread.** A bubble zoom cannot split opens in
  place on click; Spread (`ui.map_view_spread`) does it everywhere. Files
  lie on a sunflower spiral (`math::spread`) in camera-clock order (EXIF
  `DateTimeOriginal`, kept as digits: an order, not an instant), undated
  last, each linked to the point. Overlapping spirals merge around their
  mean point. Pictures are 40 px up to eight files, smaller down to 20 px,
  dots past 120; at most 200 pictures are requested in view. Markers up to
  400 px outside are laid out too, so a spiral reaching into view is whole;
  past 3 000 there, Spread falls back to bubbles.

### 2026-09-27: more formats, plans and missions

- **Twelve formats, each confirmed by content.** Tracks: TCX, FIT (our own
  bounded decoder: lengths checked, a damaged file keeps what came before),
  IGC, NMEA. Flight plans: Garmin `.fpl`, Flight Simulator `.pln`, X-Plane
  `.fms`, Little Navmap `.lnmpln`. Drone missions: QGroundControl `.plan`,
  `MAVLink` `QGC WPL` (`.waypoints`, `.mission`, `.txt`), Litchi `.csv`,
  DJI WPML (a KMZ with `wpmz/`). Extensions are shared and MIME names
  differ, so a file counts only when its first 4 KiB say so
  (`Format::detect`, on a worker); the geo provider then precedes the
  table and text readers (priority 10). The Map view skips `.txt` and
  `.csv`.
- **A plan is a route:** one line, stops as waypoints (airports, fixes,
  takeoff, landing, home, rally), fences and surveys as areas;
  `collect::Plan` gives departure, destination and cruising altitude.
  Altitudes are metres from the file's datum (a mission's are above
  takeoff, not converted); a flight's also show in feet, distances in
  nautical miles beside kilometres.
- **One document for profile and thumbnail.** The elevation profile
  (tracks as recorded, else routes as planned) is built on the worker with
  the map's shapes; a track's thumbnail is its lines drawn to scale, a
  fallback after the desktop's thumbnailers.
- **A position leads to the map.** Pictures (EXIF) and videos (MP4 and
  QuickTime location boxes, `skwir_platform::mp4`, read without the media
  data) show their position in Details with "Show on map", a row command
  (`inspection::Detail::action`) running `ui.view` to the Map view on that
  file.
