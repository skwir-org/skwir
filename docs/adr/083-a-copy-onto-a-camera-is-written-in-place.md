# ADR-083: A copy onto a camera is written in place

Status: accepted, 2026-10-22, the user's choice ("Write in place"), after
`just mtp-check` ran a camera's copies for the first time and every batch
up failed. Amends the staging rule (operations.md, "Staging"; ADR-078).

## Context

- A step that makes something in a destination stages it: a copy is made
  in a folder of the plan's there (`.skwir-copy-…`) and moved into place
  once whole, so a copy cut short never looks finished (operations.md).
  A batch does the same (ADR-078): folders and taken names in a folder of
  the staging folder, files in a `temp` folder of it, each moved into
  place after.
- GVfs's backend for a camera (gPhoto2, PTP; a phone in its "photos"
  mode too) moves an entry only within one folder ("Not supported (not
  same directory)", `gvfsbackendgphoto2.c`), and never a folder onto a
  folder. Through its FUSE bridge a move between folders fails as
  `EXDEV`. So no copy onto a camera could be placed: `just mtp-check`'s
  camera part, the phone mounted through GVfs's gphoto2 backend, failed
  at its first batch ("“a folder” could not be placed …: Invalid
  cross-device link"). `gio move` between folders falls back to copying
  again on the device and removing, twice the bytes over USB.
- A camera's other way is no help: its bridge sends a file only as it is
  released, after `close` returned (ADR-078), so what is written there
  through the bridge is told whole before it is.

## Decision

1. **A destination that moves nothing between its folders is written in
   place.** `fs::reach::writes_in_place`: the remote holding it says so
   (`Remote::moves_between_folders`: false for GVfs's camera mounts, told
   by its bridge's folder `gphoto2:…`). A copy there (and a move across
   to it) is made under its own name, never over what is there (each file
   and folder made new), its companions beside it
   (`builtin::copy_in_place`); failed or cancelled, what it made is
   removed, and a retry makes it again (no partial is kept, ADR-080).
2. **A batch there writes in place too** (`transfer::batch_in_place`):
   the names free there are given to a transfer that writes in place
   (`Transfer::writes_in_place`: `gio_copy`, whose `gio copy -i` replaces
   nothing) or to Skwir's own batch, with the destination itself as
   `into`; each step is told its copy is placed (`Stamp::Folder` for a
   folder). Failed, what it made goes (the names were free), and each
   step copies its own, in place.
3. **Nothing else is staged there.** `StepContext::stage` refuses such a
   destination with its reason (an archive extracted onto a camera):
   "it moves nothing between its folders (a camera): only copies are made
   there".

## Consequences

- On a camera a copy cut short is taken back by Skwir; a process that
  ends mid-copy (a crash, a kill, the device pulled out) leaves a partial
  file under its name, which nothing sweeps (no staging folder there).
  Elsewhere nothing changes.
- Retry does not go on from a large file cut short on a camera: it starts
  again.
- The rule is by kind: another backend that moves nothing between its
  folders is added to `Remote::moves_between_folders` once known.
- Tested: in core against a camera pretended at a folder named as GVfs's
  (`reach::pretend_camera`), with a count of staging folders made (none),
  a copy that fails leaving nothing, and only a transfer that writes in
  place asked; on the phone as a camera by `just mtp-check`
  (`gvfs_gphoto2_takes_by_gio_up_and_within`: by `gio`, then Skwir's own
  batch and a file alone by its step).
