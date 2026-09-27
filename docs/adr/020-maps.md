# ADR-020: Maps, their backgrounds, and bibli's first network requests

Status: accepted, 2026-09-27. Previews on a map and their backgrounds are
built (steps A and B of `docs/PLAN.md`, "Maps"); the Map view is not yet.

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
- **HTTP**: `ureq` with rustls and the system's certificates
  (`rustls-platform-verifier`), blocking, on two threads of the plugin's
  own, one kept-alive connection each, newest request first, a deadline
  per request, and one retry when a connection fails (a server closes
  idle ones). A cached tile is read on a scheduler worker and never waits
  behind a download. HTTP/2, which the policy recommends, is not available
  in ureq. The client is private
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
  The Map view asks `geo` for positions of what it shows, on a worker,
  visible entries first, cached by file identity, size and mtime in the
  deletable cache. A query facet over positions (`near:`) needs a field
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
