# ADR-013: A fixed-layout book's pages are drawn, through SVG

Status: accepted, 2026-09-24

## Context

A fixed-layout EPUB (`rendition:layout pre-paginated`) places every box of
a page itself. Comics made from PDF by converters such as pdf2htmlEX are a
picture per page with empty speech balloons, and the lettering is a few
dozen lines of text, each positioned absolutely in pixels, scaled by a CSS
matrix about its bottom-left corner, spaced by letter and word spacing and
by empty inline blocks, and set in one of a dozen subset fonts the book
embeds. Those fonts map their glyph variants to private-use code points, so
the text alone reads as nothing ("Bo\u{e01b}j\u{e00a}u\u{e01c}" for
"Bonjour"). Reading mode (ADR-012) showed such a page as its picture with
empty balloons, then the garbled text after it.

A page like that cannot be read; it has to be drawn. The engines surveyed
for ADR-012 (a browser, MuPDF, crengine, Blitz, litehtml) could draw it at a
cost of tens of megabytes, a C++ binding or a second rendering stack; MuPDF
does not lay out CSS transforms at all.

## Decision

A fixed-layout page that states its size (`<meta name="viewport">`) is laid
out by a small layout of our own (`rich_preview/page.rs`) and drawn as one
image, shown as the page, the width of the panel:

- The cascade of ADR-012 also computes what a page needs (`PageStyle`):
  position and offsets, sizes, margins and padding, 2D transforms and their
  origin, font families from `@font-face`, sizes, line heights, letter and
  word spacing, colour, background, visibility and opacity.
- The layout knows absolutely positioned, relative and static boxes, their
  explicit or content sizes, lines of text with alignment and wrapping, and
  inline spacers (an empty inline block's width, an inline margin). It does
  not know floats, tables, columns or CSS background images.
- Text is written as SVG `<text>` at the positions laid out, and resvg
  (already linked by gpui) shapes and draws it with the book's own fonts,
  loaded when a style first names them and renamed to the family the CSS
  gives them: converters subset one font into several files with one
  internal name. Obfuscated fonts (IDPF, Adobe) are restored with the
  package's identifier; anything else `encryption.xml` names is DRM and is
  not read. Text whose book brings no font is set in a system face.
- Pictures are decoded with the `image` crate, within the bounds of every
  embedded picture, and composited between the SVG fragments in paint order:
  resvg is linked without its raster decoders, and enabling them would add
  older copies of two decoders to the tree. An SVG frame's pictures (a
  cover page) are placed through its `viewBox`.
- The page is drawn at the display side, at most four times its size, on
  white. A page that states no size is read as text, as before. Zoomed in, it is drawn again at the side it
  covers (`InspectRequest::picture_side`, at most `limits::PICTURE_SIDE`,
  4096 px); its own size in CSS pixels (`Inspection::image_size`) is what
  100% is, whatever its pixels.

## Consequences

- Converted comics show their lettering in their balloons, in their fonts.
  On one such book (950 × 1282 pages) a release build lays a page out in
  about 8 ms and draws it in about 100 ms, once the system's fonts are
  indexed (about 10 ms, once per process).
- The drawn page has no text layer: no selection or copy on it. The text a
  converter wrote is unreadable anyway; search still reads the page's text.
- Fixed-layout pages built on what this layout does not know (floats,
  tables, multi-column text) come out approximately.

## Addendum (2026-09-24): a book of pages is a document

A fixed-layout book opens in the document reader, like a PDF, through an
in-process `DocumentSession` (`preview-epub/src/pages.rs`), continuously or
a page at a time; comics do the same (ADR-015). Each page is drawn at the
side the reader asks. A page that states no size and holds a single picture
is that picture; any other page without a size is drawn at the size the
book's first pages state. The book's contents become the reader's outline,
and search finds the pages whose text holds the words.

