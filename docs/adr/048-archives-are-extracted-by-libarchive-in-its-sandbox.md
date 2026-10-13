# ADR-048: Archives are extracted by libarchive, in its sandbox, as a step

Status: accepted, 2026-10-09. Revises the archive rule of
`docs/invariants/previews.md` ("member names are never extraction
paths"), which held while Skwir only previewed archives. Built. Revised
by ADR-059: the attributes a copy keeps are put back, an encrypted
member asks its password, and a name taken is asked about.

## Context

A drop asks what to do (`core.drop_asks`), and plugins may add rows
(`drop` menu items); every file manager offers Extract Here. Until now
`skwir-archive-worker` only previewed: it listed members within bounds
and wrote one at a time under a skwir-chosen name (`member_file`), so
no member name reached the filesystem.

Extracting turns untrusted names into paths. Known attacks: `../` or
absolute names, a link the archive makes and a later member written
through it, hard links to outside files, device nodes, set-id bits,
names the destination refuses, bombs that fill the disk.

## Decision

1. **libarchive extracts with its own guards** (helper `--extract`):
   `ARCHIVE_EXTRACT_SECURE_NODOTDOT`, `SECURE_NOABSOLUTEPATHS`,
   `SECURE_SYMLINKS`, `TIME`; never `OWNER` or `PERM` (no owner, no
   set-id or sticky bits, umask obeyed). Folders, files, links and hard
   links are made; pipes, devices and sockets skipped. A refused or
   encrypted member stops everything: an archive that tries to escape is
   never half extracted. bsdtar's guards, not ours: fewer to get wrong.
2. **Sandboxed, writing only where it extracts:** `sandbox::untrusted`
   (bubblewrap, Seatbelt, else `Otherwise::Run`), the extraction folder
   its only writable one, so a missed guard writes nowhere else.
3. **A plan step, placed whole.** `archive.extract` (Extract Here in an
   archive's menu and a drop's) extracts into a staging folder in the
   destination (`StepContext::stage`, swept after a crash like a
   copy's) and places the result only when whole, never over what is
   there.
   - Named after the archive (`photos.tar.gz` → `photos`). An archive
     whose single member has that name (`project-1.0` in
     `project-1.0.tar.gz`, the file in `notes.txt.gz`) gives that
     member, not a folder around it.
   - Undo trashes it, as for a copy. Re-run, a folder holding what the
     listing names counts as done.
4. **Cancel and Pause reach the helper.** It reads a stdin byte before
   each unit (an entry, or a MiB of one) and reports progress; the step
   hands out units a few ahead (`archive::extract_all`), none while
   paused, so pause works on every system, sandboxed or not. The bar
   counts archive bytes read. Cancel kills it; the staging folder goes.
5. **No bound on output.** Extracting an archive the user chose is
   their call, like a copy's size. The bar and Cancel answer a bomb.

## Consequences

- Only the helper links libarchive. `docs/invariants/previews.md` now
  says names reach the filesystem only in extraction, through
  libarchive's guards, in the sandbox.
- The archive plugin declares Write.
- Nothing else in the destination is read: a taken name is a conflict,
  never a merge or rename.
- A Mac's `__MACOSX` is extracted as stored; joining its `._` files to
  their files (ADR-028) is left for later.
- Windows extracts unsandboxed (as it previews) and makes links only
  where the user may.
