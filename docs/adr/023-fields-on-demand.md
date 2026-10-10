# ADR-023: Fields read on demand

Status: accepted, 2026-09-27. Built with the first field, a file's
position (ADR-022, part 1).

## Context

A file's metadata is filled at hydration: a stat, then the metadata
providers (`skwir_core::metadata`), run on every batch of every listing.
That suits what is cheap and wanted everywhere (sizes, times, tags).
Some facts are costly to read and wanted rarely: where a picture was
taken (its EXIF), a video's duration, a document's page count. ADR-020
kept positions out of hydration for that reason, and the geo plugin kept
them for the Map view alone. A search by place (`near:`) needs them too,
in another plugin, which cannot call the geo plugin's code; and Details
would show some of them (a picture's place name).

Two plugins must agree on how such a fact is asked for and answered, so
the contract is the core's (ADR-000, principle 5).

## Decision

- **`skwir_core::fields`**: a plugin registers a `FieldProvider` for the
  keys it answers (core keys such as `entry::keys::POSITION`, or its own,
  namespaced); `accepts` says, from the name alone, whether a file may
  have one; `read` reads it, on a worker. `App::fields` holds them,
  owner-tagged like every contribution and withdrawn with their plugin.
- **Asked, never pushed.** A caller on a worker asks `Fields::get(key,
  path, cancel)`: nothing is read at hydration, per frame or per row of a
  view. Locations get the registry in their `OpenContext`, so a search
  reads fields as it reads content, last, only for the candidates the
  cheaper terms kept.
- **Kept by identity.** An answer, a value or nothing, is kept by the
  file's identity (device, inode, size, modification time to the
  nanosecond) and the provider's `version`, in memory and in the one
  deletable cache (namespace `fields`): a file is read once until it
  changes. A read cut short by a cancel keeps nothing.
- **Shown when labelled.** A provider that labels a key (`label`) has it
  shown in Details for the file selected, read on a worker when the
  selection changes; a key without a label (a position, which a
  picture's own facts show) is not.
- **Values are the metadata's** (`entry::Value`); a position is a list of
  two floats, latitude and longitude (`LatLon::to_value`,
  `LatLon::from_value`).

## Consequences

`near:` and `inside:` search by the `position` field, which the geo
plugin answers for pictures and videos; Details shows the geo plugin's
`geo.place`. A field a new plugin reads (a duration, a page count) needs
no core change, only a provider. Fields are not sortable columns yet: a
view that wanted one would read it for its rows on workers, a change to
decide then. The cache grows with what was asked and is trimmed like
the others; removing it costs only reading again.
