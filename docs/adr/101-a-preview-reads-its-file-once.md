# ADR-101: A preview reads its file once

Status: accepted 2026-10-28, built 2026-10-29 on Linux (Windows and
macOS compiled only: see Consequences). Amends ADR-009 (providers read
what the request holds, not the file's path) and ADR-051 (a text file of
at most 16 MiB is shown from memory; a remote's larger one counted as far
as it is shown).

## Context

- **A preview reads its file again at every step.** Measured 2026-10-28
  (the release build of 1fdfe0c9 in a window on Xvfb, `strace -f -k -e
  trace=open`): selecting a 55-byte `note.txt` with the preview shown
  opens it 13 times, a 10 KB `.docx` 4 times, a 535-byte JPEG 5 times, a
  PDF once; the same in a local folder and through a mount. Where:
  - **Choosing who shows it.** `Previews::converted`, `inspect`, `media`
    and `render` each ask every provider's `accepts` again. Geo's reads
    the first 4 KiB of every `.txt` and `.csv` it is asked about
    (`Format::detect`: MAVLink logs, Litchi missions): 4 opens for a text
    file. The documents plugin asks `package::is_zip` of a `.docx` 3
    times before it reads it.
  - **Whether it is text.** `compute` reads its start (`read_head`): the
    bytes decide, whatever the name says. 1 open.
  - **The text view** (ADR-051: nothing holds the file):
    `inspection::plain` reads the encoding, then the first rows (2);
    `text_view::start` the same again, given only the path (2);
    `text_rows::count` the encoding and the whole file (2), though
    `plain` saw all of it; the first chunk (1).
  - **A picture.** `decode_image` opens it for its size and again to
    decode it; `exif::details` reads it a third time.
- **Free on a disk, a download on a mount.** On a local disk the system's
  page cache answers every open after the first. rclone, as Skwir mounts
  it (`--vfs-cache-mode minimal`, ADR-077), keeps nothing read: each open
  is a new download, and so is a seek backward or far ahead. Over WebDAV
  on this machine, counted at the server: 13 GETs for `note.txt`, 6 for
  the `.docx` (4 opens; a zip's directory is read from its end). On
  Dropbox each is a request to its API, one after another: the user
  found tiny text files "utterly slow" to preview (2026-10-28). sshfs pays
  a round trip per open; GVfs and kio-fuse theirs.
- **rclone's `full` cache hides it**: the 13 opens became one GET, then
  none once cached (measured the same day). It is offered for each
  server (Mount Options, File Cache) but is not the default: a file
  written there goes up after `close` returns, not before. And it is
  rclone's alone.
- **Steps see different files.** A file written while it is previewed
  can give the encoding of one version and the rows of the next.
- **Every provider takes a path**: `request.entry.resolve()`, then
  `inspection::open` or `read`, `package::is_zip`, `picture::open`,
  `file::read_regular`. A plugin from outside reads through the host's
  `read(file, offset, length)`. Programs (Pandoc, LibreOffice, the
  archive and model helpers, GStreamer) take a path, bound into their
  sandbox. `PreviewRequest` is serializable: an agent asks for previews
  too.

## Decision

1. **The request carries what was read.** A preview's job reads its file
   once, in the file's lane, into a `preview::Held` (name to settle) that
   `PreviewRequest` carries (`Arc`, not serialized: a request an agent
   sends is given one on the worker). Providers read it, never the path:
   - `head()`: the first 64 KiB, always held;
   - `whole()`: all of it, when it is at most `limits::BINARY_INPUT`
     (16 MiB: the most a quick preview already reads whole), or
     `WHOLE_BINARY_INPUT` (128 MiB) under Full preview (ADR-068), which
     reads it whole anyway;
   - `tail()`: the last 64 KiB of a file not held whole (a zip's
     directory, a PDF's trailer, a film's index);
   - `read_at(offset, buf, cancel)`: from memory where held, else from
     the file, opened for the read and closed after it;
   - `reader()`: `Read + Seek` over it, for libraries that take one (zip,
     the image decoders, exif);
   - `path_for_program()`: the file's own path where it is local; for a
     remote file held whole, a copy written from memory into the
     preview's private directory (where members are written,
     `members_dir`), removed with it. A remote file not held is handed by
     its path: the program reads it once, its own way.
2. **Asked once.** `accepts` decides on the name, the type and `head()`,
   never by reading. The job asks each provider once and its stages
   (`converted`, `inspect`, `media`, `render`, `open_document`) use the
   answer.
3. **Text from memory.** A text file held whole is shown from memory: its
   encoding, first rows, count, chunks and Find read `Held`, and nothing
   is counted when the first read saw all of it. Past the bound, ADR-051
   stands: rows are read where they are shown, through `read_at`.
4. **Memory, never a map or a kept descriptor.** No memory map: a file
   shortened while mapped kills the process (SIGBUS), and a page of a
   mount that stops answering blocks its thread in the kernel, past
   cancellation and lanes. No descriptor kept between reads: Eject and
   Disconnect fail while a file there is open ("busy"), and on Windows
   another program's save by replacement fails while anyone holds the
   file. Open, read, close; keep the bytes.
5. **One version, as long as it is shown.** `Held` belongs to a version
   of the file: its `FileId`, size and time modified, the preview's
   `Version`. A change the watcher sees, or a remote's listing read
   again, makes a new one. It lives in the requests that carry it (the
   job, the pictures asked for later, the reader), and in the one the
   preview showed before it, for flipping between two: that one is let go
   of after 60 s unseen, and the two together hold 32 MiB at most (or the
   one shown alone, under Full preview). Both go when the preview closes.
   What they free is given back to
   the system at once (`scheduler::release_on_workers`, as the grammars
   are), unless mimalloc is measured to return a buffer this large by
   itself.
6. **Past the bound.** On a local disk nothing more is kept: the page
   cache is the cache, and a second copy would only take memory. On a
   remote, its ends (`head`, `tail`), and a cache of 1 MiB blocks (8 MiB
   at most) for reads between them, read forward on one open where they
   can be: rclone keeps one download going while reads move forward,
   and starts another at a seek back. A remote text file's rows are not
   counted by reading all of it (a 1 GB log would be a 1 GB download to
   size the scrollbar): they are estimated from its size and the rows
   read so far, and counted once the end is asked for (the view taken
   there, a Find). On a disk they are counted, as ADR-051 does.
7. **Kept so.** The providers' tests preview from a `Held` made of bytes
   whose path leads nowhere: a provider that reads the path fails them.
   `just remote-check` previews a file of each kind through an rclone
   mount of a WebDAV server on this machine and counts the GETs: one
   for a file held whole. `docs/invariants/previews.md` and `remote.md`
   say: a preview reads its file once, through `Held`.

## Settled

The user settled on 2026-10-28: 16 MiB for every file, the 2 MiB text
bound kept (it bounds what is parsed whole, not what is read), the
previous file kept 60 s, a remote text file's rows estimated, and
rclone's mounts keeping what they read (`full`) with a copy waiting for
the server (ADR-102). Measured that day, rclone 1.75.2 over WebDAV on
this machine: on `minimal`, a file written whole is on the server when
`close` returns, and a file opened to append or to read and write goes
up 5 s after (`--vfs-write-back`); on `full`, a file written whole goes
up 5 s after `close` too, a mount stopped 1 s after `close` left it in
rclone's cache alone, and the next mount with that cache sent it.

## Consequences

- The preview API changes: every preview provider reads `Held`: those of
  17 plugins (archives, circuits, comics, djvu, documents, email, epub,
  fonts, geo, hex, map-tiles, markdown, models, playlists,
  structured-data, tables, thumbnails), the outside plugins' host
  (`extend`), the platform's and the shell's own. The
  platform's readers gain forms over bytes or `Read + Seek`; their path
  forms stay for thumbnails, fields and operations.
- **Memory.** At most 32 MiB while a preview shows files (the one shown
  and the one before, 60 s); 128 MiB under Full preview, which reads the
  file whole already. `just idle-pss` measures
  with no preview shown: unchanged.
- **On a remote** a small file's preview is one download. Scrolling,
  Find and pictures for later read memory, so they go on when the server
  stops answering after the preview came.
- **On a disk** a few system calls fewer; no visible speed is expected
  (to measure). Every step sees the same version of the file.
- A plugin from outside is answered from `Held` by the host's `read`.
- Not covered, read by path as before: thumbnails, fields (Properties'
  facts), notes, the player's streaming.
- Rejected:
  - **`full` instead of this record**: it is rclone's alone (sshfs, GVfs
    and kio-fuse would still pay every open), and a preview of a file
    read once there still opens it 13 times. ADR-102 takes `full` for
    the programs files are opened with, beside this.
  - **Every remote file copied to the disk to be previewed**: writes the
    user's files to disk unasked, and a large one whole when a preview
    reads its start.
  - **A cache across previews, by path**: state to keep current, for
    the few files previewed twice.
