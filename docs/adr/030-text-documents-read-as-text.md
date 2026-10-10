# ADR-030: Text documents read as text, by skwir or by Pandoc

Status: accepted, 2026-10-03.

## Context

An Office document is previewed as pages: LibreOffice converts it to PDF
inside bubblewrap, a fresh profile each time, and the PDF reader shows the
result (ADR-003). That is faithful and heavy. On a small synthetic Word
file, one conversion took 1.2 to 2.7 s and up to 370 MB of resident memory
on the reference machine, against 0.03 s and 120 MB for Pandoc converting
the same file to HTML (one machine, one file: an order of magnitude, not a
budget). Arrowing through a folder of reports waits for an office suite at
every file. Without LibreOffice there is no preview at all, and outside
Linux there is none either: the sandbox is bubblewrap.

What a file manager's preview is for is mostly reading: what does this
document say, is it the right one. skwir already reads documents that way:
Markdown, HTML, EPUB and mail are semantic blocks (headings, paragraphs,
lists, tables, notes, pictures) in the reading mode the preview frame
draws (ADR-009, ADR-012). A Word or OpenDocument text file is a zip of XML
whose structure maps onto those blocks, as an EPUB's does.

Two more costs of the pages path: a document's grid thumbnail exists only
once it has been previewed (`OfficePages` never converts), though most of
these files carry a picture of their first page; and the spreadsheet
precedent already exists (`preview-tables.spreadsheets`: cells by default,
LibreOffice's pages when asked).

## Decision

### Three presentations, one setting

`preview-documents.text_documents` ("Text documents") chooses how Word
(`.docx`, `.docm`, `.dotx`, `.dotm`), OpenDocument text (`.odt`, `.ott`)
and RTF files are shown:

- **Reading** (the default): skwir reads Word and OpenDocument files
  itself; what it does not read (RTF) goes to Pandoc.
- **Pages**: LibreOffice lays them out, as before.
- **Pandoc**: every format Pandoc reads goes to Pandoc.

What is installed is detected, never assumed: a choice whose program is
missing falls back to the next that can show the file (Pages to skwir's
reader then Pandoc; Reading to Pandoc then Pages; Pandoc to skwir's
reader then Pages). Which formats Pandoc reads is its own answer
(`pandoc --list-input-formats`, asked once per installed version), not a
list kept beside it. A file that is not a zip where a zip is expected (an
encrypted Word file is an OLE container) is not taken by a reader; Pages
says what LibreOffice says about it. Legacy `.doc`, presentations and
spreadsheets are LibreOffice's alone, as before.

The reading provider sits before the document reader (priority 10, as
`preview-tables`) and decides in `accepts`, on the worker, from the
setting, the file's first bytes and the programs found.

### skwir's reader builds blocks, not HTML

The built-in reader (`preview-documents`: `docx.rs`, `odt.rs`) walks the
document's XML into `inspection::Block`s directly, as Markdown's parser
does, so a list keeps the marker the document numbers it with (`1.2.`,
`a)`, a bullet) and a heading its number. It reads paragraphs and
headings (by outline level or the built-in heading and title styles),
emphasis, underline, strike, raised and lowered text, small capitals,
alignment and left indents, lists with their numbering, tables (merged
cells keep their columns), links (to addresses and to bookmarks),
footnotes and endnotes (moved to the end of the page under a rule, opened
under their reference), pictures, text boxes (after their paragraph),
fields as their shown result, tracked changes as accepted, and the
document's title, author, subject, keywords and page count for Details.
It leaves out what is layout or not text: headers and footers, comments,
page and column breaks, fonts, colours and sizes, charts and shapes
without text, and pictures it cannot decode (WMF and EMF). Equations read
as their characters.

Pictures are named for later (`Picture::Later`, keyed by their member)
and read through the same bounded package reader when shown. A shown
picture does not say its description: Word writes descriptions nobody
typed ("Description automatically generated"); one that cannot be shown
says it in its place. A style based on the title (an author line) is no
title; a style a document names without defining it counts by its name. Every member
is read within `limits::BINARY_INPUT` of actual inflated bytes; the
package within `limits::CONTAINER_MEMBERS`; the XML within roxmltree's
node limit.

A long document is read in parts, each cut between paragraphs or table
rows once it holds `PART_BLOCKS` blocks, so its notes fit in the
`limits::BLOCKS` a page shows; `[` and `]` and the wheel go from part to
part as through a paged text file. Numbering and note numbers are counted
from the start, so a part says what the whole says.

### Pandoc's HTML is read as any HTML

Pandoc runs in the same sandbox as LibreOffice (`skwir_platform::sandbox`:
no network, the input read-only, a private writable folder), its output
an HTML file that `rich_preview::html` reads like any page: notes, tables
and links included. Pandoc numbers lists with `<ol type>` and lays its
tables out with white space between cells; the reading mode now counts
an ordered list as its type says (`inspection::Numbering`, shared with
the word processors' lists) and takes no cell's text from the space
between cells, for every HTML page, a book's too. Its pictures are extracted into the private folder
and kept in the plugin's cache under the file's version and Pandoc's, so
they are named for later like the built-in reader's; one evicted is
extracted again. Its output is read within `limits::TEXT_INPUT`; a longer
one is cut and says so.

### Thumbnails from the picture a package keeps

OpenDocument files keep their first page as `Thumbnails/thumbnail.png`,
and Office files often keep one where their package relationships say
(`docProps/thumbnail.jpeg`). A fallback-stage provider reads it, before
the one that draws a converted PDF's first page: a desktop thumbnailer
still comes first, and a document has a thumbnail before it is ever
previewed. A picture the reader cannot decode (WMF, EMF) is none.

## Consequences

- A text document reads in milliseconds without LibreOffice, on every
  platform (the built-in reader needs no sandbox: it runs no program).
  Pandoc and LibreOffice stay Linux-only until another isolation adapter
  exists.
- Reading mode is not the document's layout: page size, columns, fonts and
  floating positions are gone. That is the point of the choice, and Pages
  remains one setting away. Reading is the default, for the costs measured
  above: a preview is for glancing at a document while the selection
  moves, and pages that take seconds each hold every step. Whoever relied
  on pages chooses Pages.
- The dev shell carries Pandoc; it is optional at run time, like
  LibreOffice, and listed in the plugin's requirements with bubblewrap.
- Find searches the part shown, as in a paged text file.
- Not covered: flat OpenDocument (`.fodt`), presentations and spreadsheets
  read as text, a document's own fonts and colours, right-to-left
  paragraph order, and Word's toggle semantics for emphasis inherited
  through several styles (a direct setting wins).
