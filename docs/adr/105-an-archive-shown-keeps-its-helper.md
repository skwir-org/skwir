# ADR-105: An archive shown keeps its helper

Status: accepted and built 2026-10-29 on Linux (the helper's own tests,
the platform's session tests, comics and archives with the helper
sandboxed, `plugins_toggle` reading `/proc`); Windows compiled at the
release's checks. Amends ADR-101's fourth rule (no descriptor kept
between reads) and ADR-048 (only the helper links libarchive, which
stands).

## Context

- **Each member was a run of its own.** A comic's page, or a member of an
  archive opened in its preview, started `skwir-archive-worker`, which
  opened the archive and read headers from its start to the member. In a
  ZIP or a CBZ libarchive seeks past what comes before; in a compressed
  tar (`.tar.gz`, `.tar.xz`, `.tar.zst`) or a solid RAR or 7z (most CBR
  and CB7) everything before the member is decompressed again. Reading a
  300-page, 600 MB solid comic decompressed about 90 GB in all, the last
  pages taking seconds each (the review of 2026-10-29, preview#2 and
  comics#1). Each page also went through a temporary folder that may be
  on disk.
- **ADR-101 keeps no descriptor between reads**: Eject and Disconnect
  fail while a file there is open ("busy"), and on Windows another
  program's save by replacement fails while anyone holds the file.

## Decision

1. **One helper per archive shown** (`skwir-archive-worker <archive>
   --serve`, `platform::archive::Session`), answering tagged requests
   (`List`, `Read`). It keeps libarchive's reader where it stopped: a
   later member reads on from there, an earlier one from the start.
2. **As long as it is shown, never longer.** The archives plugin ties it
   to the preview's lifetime for that file (`PreviewRequest::shown`,
   cancelled once the last holder of its `HeldSlot` lets go), the comics
   plugin to its reader's (`open_document`'s lifetime); either ends with
   its plugin. No idle timer: before an unmount `App::release` sends the
   panes there home, the preview shows another file, and the helper ends.
3. **Its version, before each read.** The archive's listing key is
   checked before and after each request; another version ends the
   helper and the read fails ("Archive changed since it was listed"). A
   cancelled read ends it too, its place in the archive unknown.
4. **It opens the archive itself**, on Windows sharing read, write and
   delete, and hands libarchive read, skip and seek callbacks: replacing
   or deleting the archive is not refused while it is shown.
5. **Comics read ahead in archive order**: a page and the two after it in
   one pass; a cover is one run (list, then the first page). Members go
   through `file::memory_dir`, never a disk folder.

## Rejected

- **Unpacking a solid archive whole into memory**: up to
  `CONTAINER_EXPANSION` of RAM per comic, nothing shown until it is done,
  and a larger comic falling back to today's cost.
- **Several pages per run, nothing kept open**: ADR-101 untouched, but
  still quadratic, only N times less.
- **Helpers kept by an idle timer**: a file held open after nothing shows
  it, Eject meeting it busy.

## Consequences

- Reading a comic, or walking an archive's members forward, decompresses
  each byte once; going back costs a read from the start on a solid
  archive (the reader's 64 MiB of pages and the comics plugin's cache
  cover the usual step back).
- An archive shown is open in a helper while it is shown. The document
  readers already kept their file open for their session (PDFium,
  Poppler, MuPDF and DjVuLibre open it; djvu-rs reads it whole): ADR-101's
  rule now names both. Whether their engines' opens let another program
  replace the file on Windows is not checked.
- What changes: the worker's `--serve` mode and its callbacks;
  `process::Helper`, shared with the document sessions; `archive::Session`;
  the comics and archives plugins; `HeldSlot::shown`.
