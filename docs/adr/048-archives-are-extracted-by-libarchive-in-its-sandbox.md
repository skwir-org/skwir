# ADR-048: Archives are extracted by libarchive, in its sandbox, as a step

Status: accepted, 2026-10-09. Revises the archive rule of
`docs/invariants/previews.md` ("member names are never extraction
paths"), which held while bibli only previewed archives.

## Context

A drop asks what it does (`core.drop_asks`): Move, Copy and Link Here.
Plugins may add rows to that menu (the registry's `drop` items), and the
one every file manager offers there is Extract Here. bibli read archives
only to preview them: `bibli-archive-worker` lists members within bounds
and writes one member at a time under a name of bibli's (`member_file`),
never under its own, so no member's name ever reached the filesystem.

Extracting is the opposite: an archive's names become paths on disk. An
archive is a file nobody vouched for, and the attacks on extraction are
old and many: `../` or an absolute name written outside the folder, a
link the archive makes and a later member written through it, a hard
link to a file outside, a device node, set-id bits, a name the
destination refuses, a bomb that fills the disk.

## Decision

1. **libarchive extracts, with its own guards.** The helper gains
   `--extract`: libarchive's disk writer with
   `ARCHIVE_EXTRACT_SECURE_NODOTDOT`, `SECURE_NOABSOLUTEPATHS` and
   `SECURE_SYMLINKS` (never written through a link it made), and
   `TIME`; never `OWNER` or `PERM` (no owner, no set-id or sticky bits,
   the umask obeyed). Pipes, devices and sockets are left out; folders,
   files, links and hard links are made. A member the guards refuse, or
   one encrypted, stops it all: an archive that tries to leave its
   folder is not half extracted. bsdtar's guards, not ours: fewer to
   get wrong.
2. **In its sandbox, writing only where it extracts.** It runs through
   `sandbox::untrusted` like every program on a file nobody vouched
   for (bubblewrap on Linux, Seatbelt on macOS, `Otherwise::Run`
   elsewhere), its writable folder the one it extracts into: a guard
   libarchive missed still writes nowhere else.
3. **A step of a plan, made whole.** The archive plugin's `extract`
   step (`archive.extract`, Extract Here in an archive's menu and a
   drop's) extracts into a staging folder in the destination
   (`StepContext::stage`, a copy's: a later run sweeps what a crash
   left) and puts the result in place only once whole, never over what
   is there. It is named after the archive (`photos.zip`, `photos.tar.gz`
   give `photos`); an archive whose one member has that name (a folder
   `project-1.0` in `project-1.0.tar.gz`, the file of `notes.txt.gz`) is
   that member, not a folder around it. Undo puts it in the trash, as a
   copy's undo does; run again, a folder that holds what the listing
   names is the step already done.
4. **Cancel and Pause reach the helper.** It takes a byte of standard
   input before each unit of work (an entry, or a MiB of one) and
   reports what it read; the step hands out units a few ahead
   (`archive::extract_all`), none while the plan is paused, so a pause
   stops it within those and works on every system, sandboxed or not.
   The bar counts the archive's bytes read against its size. Cancel
   kills it; the staging folder goes with what it made.
5. **No bound on what it writes.** Extracting is the user's act, of an
   archive they chose: its size is theirs to judge, as a copy's is. The
   bar and Cancel are the answer to a bomb, not a limit.

## Consequences

- Only the helper links libarchive still; the rule in
  `docs/invariants/previews.md` now says names reach the filesystem in
  extraction alone, by libarchive's guards, in the sandbox.
- The archive plugin writes files: its capabilities say so (Write).
- Nothing else of the destination is read to decide: a name that is
  taken is a conflict, never a merge or a rename.
- A Mac's `__MACOSX` folder is extracted as it is stored; joining its
  `._` files to their files (ADR-028) is left for later.
- Windows extracts without a sandbox (as the helper previews there) and
  makes a link only where the system lets the user make links.
