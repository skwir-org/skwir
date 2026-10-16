# ADR-051: Text is read where it is shown

Status: accepted and built, 2026-10-11. Replaces the pages of text and source of
ADR-009's bounds (`TEXT_PAGE`, `SOURCE_PAGE`).

## Context

- **Text was shown a page at a time**: 256 KiB of a text file, 32 KiB of
  a format's source. Each page was a block per line, highlighted whole on
  a worker, in a `gpui::list` with an item per line. Pages bounded three
  costs that grow with the file:
  - a `Block` is built for rich documents (some fifteen fields): millions
    of them would take gigabytes;
  - syntect, through bat on the reference machine, colors Rust at about
    1.1 MB/s and a log at 0.3 MB/s (3.5 MB in 3.2 s, 20 MB in 68 s,
    2026-10-11): a 512 MB log whole would take half an hour;
  - a `gpui::list` keeps a sum tree entry per item, and wrapped lines
    are measured one by one.
- **The pages showed.** A later page said "1746 lines from byte 32725",
  its line numbers started at 1 again, `[` and `]` turned pages, the
  scrollbar described the page, Find searched the page only, a selection
  ended at its edge, and UTF-16 past the first page was read as UTF-8.
- **Zed** holds the whole file in a rope (a sum tree of chunks, each
  summarizing its bytes and lines), lays out only the rows on screen
  (`EditorElement::layout_lines`), parses in the background and asks for
  highlights 16 KB at a time. It refuses files of 6 GB or more:
  `worktree.rs` notes over 64 GB used for a 10 GB file (#27283). An
  editor must hold the text it changes; a preview changes nothing.
- **Pagers and log viewers** (`less`, klogg) keep where lines start and
  read what is shown. Counting the lines of a 512 MB log took 0.05 s
  from the page cache (`wc -l`, 5.1M lines); a cold disk reads at its
  own speed.

## Decision

1. **A text file is one view of the file, read where it is shown.** Plain
   text and every format's source (`inspection::plain`, `source`) carry
   the file (`Inspection::text`) and its first rows, never the whole
   text. What is parsed whole stays blocks, bounded as before: structured
   code with its folds, a chapter's source, rendered documents.
2. **Rows.** A row ends with its line, or after `LINE_BYTES` (4 KiB) at a
   character boundary, so no row is unbounded: a minified file is many
   rows of one line. A line's number is drawn on its first row. The
   encoding is decided once, from the file's first bytes (its mark, else
   UTF-8, else Windows-1252), and UTF-16 is cut in its own units.
3. **The index.** A worker counts the rows once, streaming, and keeps the
   place (offset, line) of every 256th: about 6 KB for 64k rows, 470 KB
   for 5M. The view draws from the first screen; the count grows while
   it is counted, and the scrollbar with it, as a listing streams.
4. **Reading.** The view asks for the chunks of 256 rows around what it
   shows, nearest first, one at a time. A worker reads one from its index
   point, decodes it, finds its links and colors it. A bounded cache
   keeps the chunks near the view.
5. **Colors follow the reading.** A chunk is read plain first, in
   milliseconds whatever its grammar or its lines (a minified file's
   first chunk is a megabyte to color), then colored. The highlighter's
   state at the end of a chunk colors the next. A chunk reached by a jump
   starts from a fresh state at its first row: exact for logs, CSV and
   most code, while a string or comment open across the jump may be
   colored wrongly until it closes. It is colored again once the chunk
   before it has been.
6. **The element** (`elements/rows.rs`) scrolls by row: the row at the
   top and how far into it. Only the rows on screen are laid out, in
   prepaint; lines wrap or scroll sideways (`reading.wrap_lines`,
   More › Wrap lines, `Alt+Z`); the scrollbar counts rows.
7. **Find and copy see the file.** Find reads the file on a worker from
   the row after the one selected, either way, around the end, and lands
   on the row. Copy reads the rows selected, up to `COPY_BYTES` (16 MiB),
   and says when it stopped there. On X11 a clipboard larger than one
   request goes in pieces (INCR, vendored gpui patch 0021).
8. **A file that changes is counted again**, the view kept at its row.

## Consequences

- What a text file costs no longer grows with it: the index, a dozen
  chunks and the rows on screen. A 512 MB log scrolls whole, numbered
  from its first line.
- `TEXT_PAGE`, `SOURCE_PAGE`, the "Shown … at a time" notice and `[`/`]`
  for text are gone; `[`/`]` still turn a book's chapters.
- Highlighting can be wrong after a jump into a long multi-line string or
  comment, until the view has read up to it.
- The byte view is read the same way (2026-10-12): rows of 16 bytes need
  no index, the file's length counts them, and its pages are asked of the
  bytes view (a plugin from outside too) as they come into view.
- Find lands on the match, not only its row (2026-10-16): a row of a
  minified file is 4 KiB of one line, with several matches. It goes from
  match to match, marks them in the rows drawn, and brings the one it is
  on into view along its row.
