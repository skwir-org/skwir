# ADR-106: Books beyond EPUB: FB2 and Mobipocket

Status: accepted and built 2026-10-30 on Linux (the books plugin's
tests on calibre's conversions and books made in the tests, skwir's
preview and mutation tests, `just mupdf-check`, a window on Xvfb);
Windows and macOS not compiled for it yet.

## Context

- Skwir read EPUB alone. A FictionBook 2 file (`.fb2`, common for books
  in Russian) showed as its XML, a Mobipocket book (`.mobi`, `.azw3`,
  Kindle's) as nothing.
- MuPDF's helper (ADR-014) reads FB2, MOBI 6 and PalmDOC texts, stored
  as is or with PalmDOC's compression. It finds no document in a zipped
  FB2, refuses Huffman-coded text, draws a KF8 book (AZW3) as one empty
  page, and reads no MOBI book's contents. It is optional, under the
  AGPL, and lays books out in its own styles, not the reader's.
- The one Rust crate for MOBI (`mobi`, MIT, last released 2022) reads
  headers, metadata and text; it depends on `encoding` 0.2, unmaintained,
  beside the `encoding_rs` Skwir has, and on an older `indexmap`, and
  reads neither pictures nor indexes nor KF8.

## Decision

1. **One books plugin.** The EPUB plugin reads FB2 and Mobipocket books
   too, its title "Books". Its id stays `epub`: nothing moves a setting
   to a new key, so a new id would reset `epub.backend` and the plugin's
   toggle. Every book goes through one reading path (`book::inspect`
   over the `Book` a format implements): chapters, a search that wraps,
   contents rows knowing their chapter, links, the source view.
2. **One engine choice for every book** (`epub.backend`). Automatic is
   the built-in engine, for every book. MuPDF, chosen, reads the kinds
   of book it reads well (EPUB, FB2, MOBI 6, a file holding MOBI 6 and
   KF8 through its MOBI 6 part, PalmDOC texts; not encrypted, stored as
   is or with PalmDOC's compression), told from the file's type and
   first record before anything opens it; the built-in engine reads the
   others (a zipped FB2, KF8, Huffman-coded text). An engine never gives
   way because the other failed on the book.
3. **FB2** is read whole, at most 16 MiB (a book is a container, not a
   text parsed whole), in the encoding its XML declaration names, parsed
   once. Its chapters are the main body's sections, or those of the one
   part holding them all; each body of notes is a chapter. A chapter is
   translated to XHTML for reading mode: FB2's elements become the HTML
   that means the same, what HTML has no element for a class of a
   stylesheet the plugin gives the book, note links `noteref`s and notes
   `endnote`s; what nests deeper than 64 levels is its text.
4. **Mobipocket is read by Skwir's own code**: the Palm database, the
   text records with what trails them removed, PalmDOC's LZ77 and the
   Huffman coding with its dictionary (a phrase decoded once, one that
   leads back to itself refused), EXTH metadata, INDX indexes. MOBI 6
   is cut at its page breaks, every byte a link or the contents name
   (`filepos`) an anchor; KF8 is put back together from its skeletons
   and fragments, its `kindle:pos` links and contents led to anchors;
   a file holding both is read through its KF8 part. The text
   decompressed is at most 64 MiB. A PalmDOC text is paragraphs.
5. **What a format works out once is kept while the book is shown**
   (FB2's chapters, a Mobipocket book's text), keyed by path and
   version and let go with the preview (`PreviewRequest::shown`), as
   ADR-105 keeps an archive's helper.
6. **Never read:** a book whose text is encrypted (DRM) says it is
   protected; KFX, Amazon's own format the type database calls a Kindle
   book too, says it is not read. `.azw` has no type in the database
   and is not taken. A `.pdb`, which the database calls a protein's
   structure or a Windows program database by its heaviest globs, is
   read as a book only when its Palm type and creator say so.
7. **Fixtures are synthetic**: an EPUB and FB2s written by
   `tools/books/fixtures.py`, calibre's conversions of the EPUB (MOBI 6,
   MOBI 6 + KF8, AZW3, FB2, PalmDOC), and books built in the tests
   (Huffman coding, which calibre does not write; trailing entries;
   DRM; KFX).

## Rejected

- **The `mobi` crate**: an unmaintained dependency for the easy part.
- **A plugin per format**: the reading path, contents and covers are
  shared, and plugins share nothing but the core; three engine settings
  for one choice.
- **MuPDF as Automatic for FB2 and MOBI**: optional, and not the
  reader's typography.
- **Falling back to the other engine on failure**: a damaged book is
  the book's error, as for documents.

## Consequences

- No new dependency: `base64`, `encoding_rs` and `memchr` were in the
  tree and are now the plugin's own.
- A KF8 book in fixed layout (a comic) reads as chapters of pictures,
  not as pages in the document reader: no tool makes one to test with
  (calibre writes no fixed-layout mark, kindlegen is withdrawn).
- MOBI 6 headings written as `<font size>` (calibre's) read as bold
  paragraphs: reading mode reads no `<font size>`.
- With MuPDF, a MOBI book shows no contents.
