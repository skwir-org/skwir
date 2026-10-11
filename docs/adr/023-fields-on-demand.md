# ADR-023: Fields read on demand

Status: accepted, 2026-09-27. Built with the first field, a file's position
(ADR-022, part 1).

## Context

Hydration runs the metadata providers (`skwir_core::metadata`) on every
batch of every listing: right for cheap facts wanted everywhere (sizes,
times, tags). Some facts are costly and rarely wanted: a picture's
position, a video's duration, a page count. ADR-020 kept positions out of
hydration, for the Map view alone; but `near:` needs them in another
plugin, and Details would show some. Two plugins must agree, so the
contract is the core's (principle 5).

## Decision

- **`skwir_core::fields`:** a plugin registers a `FieldProvider` for its
  keys (core keys like `entry::keys::POSITION`, or its own, namespaced).
  `accepts` decides from the name alone; `read` reads on a worker.
  `App::fields` holds providers, owner-tagged, withdrawn with their plugin.
- **Asked, never pushed:** a caller on a worker asks
  `Fields::get(key, path, cancel)`; nothing is read at hydration, per frame
  or per row. Locations get the registry in their `OpenContext`, so a search
  reads fields last, only for candidates the cheaper terms kept.
- **Kept by identity:** an answer (a value or nothing) is kept by file
  identity (device, inode, size, mtime to the nanosecond) and the
  provider's `version`, in memory and the cache's `fields` namespace. A
  cancelled read keeps nothing.
- **Shown when labelled:** a key with a `label` shows in Details for the
  selected file, read on a worker; an unlabelled one (a position) does not.
- **Values are `entry::Value`s;** a position is two floats, latitude and
  longitude (`LatLon::to_value`, `LatLon::from_value`).

## Consequences

- `near:` and `inside:` search by `position`, which geo answers for
  pictures and videos; Details shows `geo.place`.
- A new costly field needs only a provider, no core change.
- Fields are not sortable columns yet; that would be decided then.
- The cache grows with what was asked and is trimmed like the others.
