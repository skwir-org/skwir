# ADR-030: Text documents read as text, by skwir or by Pandoc

Status: accepted, 2026-10-03. Built. Extended by ADR-039 (isolation on
macOS and Windows).

## Context

Office documents were previewed as pages: LibreOffice converts to PDF
in bubblewrap with a fresh profile, the PDF reader shows it (ADR-003).
Faithful but heavy. Small synthetic Word file, reference machine (one
file: an order of magnitude, not a budget):

| Conversion | Time | Resident memory |
|---|---|---|
| LibreOffice → PDF | 1.2–2.7 s | up to 370 MB |
| Pandoc → HTML | 0.03 s | 120 MB |

Arrowing through reports waits for an office suite at every file;
without LibreOffice, or outside Linux, there was no preview. A preview
is mostly for reading, and skwir already reads Markdown, HTML, EPUB and
mail as semantic blocks (ADR-009, ADR-012). Word and OpenDocument text
are zips of XML that map onto those blocks. Also, a document's
thumbnail existed only once previewed (`OfficePages` never converts),
though most files carry a first-page picture. Spreadsheets already
worked this way (`preview-tables.spreadsheets`: cells by default,
LibreOffice's pages on request).

## Decision

### Three presentations, one setting

`preview-documents.text_documents` ("Text documents") for Word (`.docx`,
`.docm`, `.dotx`, `.dotm`), OpenDocument text (`.odt`, `.ott`) and RTF:

| Choice | What | If its program is missing |
|---|---|---|
| **Reading** (default) | skwir reads Word and ODF; RTF goes to Pandoc | Pandoc, then Pages |
| **Pages** | LibreOffice, as before | skwir's reader, then Pandoc |
| **Pandoc** | every format Pandoc reads | skwir's reader, then Pages |

- Installed programs are detected, never assumed. Pandoc's formats are
  its own answer (`pandoc --list-input-formats`, once per version).
- A file that is not a zip where one is expected (an encrypted Word
  file) is not taken by a reader.
- Legacy `.doc`, presentations and spreadsheets stay LibreOffice's.
- The provider sits before the document reader (priority 10) and
  decides in `accepts`, on the worker, from the setting, the first
  bytes and the programs found.

### skwir's reader builds blocks, not HTML

`docx.rs` and `odt.rs` walk the XML into `inspection::Block`s, so a list
keeps its own markers (`1.2.`, `a)`) and a heading its number.

- **Reads:** headings (outline level or built-in styles), emphasis,
  underline, strike, super/subscript, small caps, alignment, left
  indents, numbered lists, tables (merged cells keep columns), links
  (addresses, bookmarks), footnotes and endnotes, pictures, text boxes,
  fields as shown, tracked changes as accepted, equations as their
  characters; title, author, subject, keywords, page count for Details.
- **Leaves out:** headers and footers, comments, page and column
  breaks, fonts, colours, sizes, charts and shapes without text, WMF
  and EMF pictures.
- Pictures are read lazily (`Picture::Later`) through the same bounded
  package reader. Their descriptions are not shown (Word invents them:
  "Description automatically generated").
- **Bounds:** members within `limits::BINARY_INPUT` of inflated bytes,
  the package within `limits::CONTAINER_MEMBERS`, the XML within
  roxmltree's node limit.
- **Long documents** come in parts of `PART_BLOCKS`, cut between
  paragraphs or rows, so notes fit `limits::BLOCKS`; `[`, `]` and the
  wheel move between parts. Numbering counts from the start.

### Pandoc's HTML is read as any HTML

- Pandoc runs in LibreOffice's sandbox (`skwir_platform::sandbox`: no
  network, input read-only, a private writable folder); its HTML is
  read by `rich_preview::html`.
- Reading mode now honours `<ol type>` (`inspection::Numbering`, shared
  with the word processors) and takes no cell text from white space
  between cells, for every HTML page.
- Pictures are extracted and cached under the file's and Pandoc's
  versions. Output is read within `limits::TEXT_INPUT`; longer output
  is cut and says so.

### Thumbnails from the package's picture

ODF's `Thumbnails/thumbnail.png`, or Office's `docProps/thumbnail.jpeg`
where its relationships say, is read by a fallback-stage provider: after
the desktop's thumbnailer, before the converted-PDF one. A document has
a thumbnail before it is ever previewed.

## Consequences

- A text document reads in milliseconds without LibreOffice, on every
  platform (the built-in reader runs no program, so needs no sandbox).
  Pandoc and LibreOffice need an isolation adapter: bubblewrap on
  Linux, ADR-039 for macOS and Windows.
- Reading mode drops the layout (page size, columns, fonts, floats).
  That is the point; Pages is one setting away. Reading is the default
  because a preview is for glancing while the selection moves.
- Pandoc is in the dev shell, optional at run time, listed in the
  plugin's requirements with bubblewrap.
- Find searches the part shown.
- Not covered: `.fodt`, presentations and spreadsheets as text, a
  document's fonts and colours, right-to-left paragraph order, Word's
  toggle semantics for emphasis inherited through several styles.
