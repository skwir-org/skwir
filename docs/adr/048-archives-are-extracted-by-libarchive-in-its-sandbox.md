# ADR-048: Archives are extracted by libarchive, in its sandbox, as a step

Status: accepted, 2026-10-09. Revises the archive rule of
`docs/invariants/previews.md` ("member names are never extraction
paths"), which held while Skwir only previewed archives. Built. Revised
by ADR-059: the attributes a copy keeps are put back, an encrypted
member asks its password, and a name taken is asked about. Amended
2026-10-15: an archive of one folder or one file gives it, whatever its
name.

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
     member, not a folder around it. (Amended 2026-10-15: whatever the
     member's name.)
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

## Amendments

### 2026-10-15: one member, whatever its name

An archive of one folder named otherwise than the archive (`v1.0.tar.gz`
holding `project-1.0/`, as a release is downloaded) gave
`v1.0/project-1.0/`: a level the user opens for nothing. GNOME Files,
Ark's "autodetect subfolder" and macOS's Archive Utility give the
folder.

- What extracting the archive leaves at its top (`left_by`) is one
  member, a folder and all in it or a file: it is placed as itself
  (`one_member`), under its own name. Of several, the folder named after
  the archive holds them, as before.
- A hidden member alone (`.config/`, `.profile`) stays in that folder:
  placed as itself it would be out of sight where hidden files are, and
  an archive would drop a dotfile straight into the folder it is
  extracted in.
- Pipes, devices and sockets, which are never extracted, do not count;
  an archive refused whole (a member that would leave the folder) has no
  one member.
- The name comes from the archive's listing, read on the plan's thread:
  `Operation::makes` takes the plan's context, and settling the plan fixes
  the name found in the step (`made_as`), so the dry run checks, and the
  name-taken dialog asks about, the name it will be placed under. A plan
  not settled (a script's) reads it as it runs.
- A listing cut short (`archive::list`'s bounds, a source tree of
  thousands of files) tells a folder by what it listed, a file only when
  it lists every member. What was extracted decides what is placed: its
  one member under the step's name, else a folder of all of it. A level
  is never added; at worst, a folder of several members is called as
  the folder that every listed member was in.
