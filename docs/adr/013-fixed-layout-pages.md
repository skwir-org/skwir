# ADR-013: A fixed-layout book's pages are drawn, through SVG

Status: accepted, 2026-09-24. Built.

## Context

A fixed-layout EPUB (`rendition:layout pre-paginated`) positions every box
itself. Comics converted from PDF (pdf2htmlEX) are a picture per page with
empty balloons, plus lettering as absolutely positioned, CSS-transformed
text in embedded subset fonts that map glyphs to private-use code points:
the text alone is garbage. Reading mode (ADR-012) showed the empty balloons,
then that garbage.

Such a page must be drawn. The engines surveyed for ADR-012 (a browser,
MuPDF, crengine, Blitz, litehtml) cost tens of megabytes, a C++ binding or a
second rendering stack; MuPDF does not lay out CSS transforms at all.

## Decision

A fixed-layout page that states its size (`<meta name="viewport">`) is laid
out by a small layout of our own (`rich_preview/page.rs`) and drawn as one
image. A page that states no size is read as text.

- The ADR-012 cascade also computes `PageStyle`: position, box model, 2D
  transforms, `@font-face` fonts, text spacing, colour, background,
  visibility, opacity.
- The layout handles absolute, relative and static boxes, lines with
  alignment and wrapping, and inline spacers. No floats, tables, columns or
  CSS background images.
- Text is SVG `<text>` drawn by resvg (already linked by gpui) with the
  book's fonts, each renamed to its CSS family (converters split one font
  into files sharing one internal name). Obfuscated fonts (IDPF, Adobe) are
  restored; anything else `encryption.xml` names is DRM and is not read. A
  text with no font in the book gets a system face.
- Pictures are decoded by the `image` crate and composited between SVG
  fragments in paint order. resvg's raster decoders stay off: they would add
  older copies of two decoders. An SVG frame's pictures go through its
  `viewBox`.
- The page is drawn at display size (at most 4× its own), on white, and
  redrawn when zoomed (`InspectRequest::picture_side`, up to
  `limits::PICTURE_SIDE`, 4096 px). 100 % is its CSS size
  (`Inspection::image_size`).

## Consequences

- Converted comics show their lettering in the balloons, in their fonts. On
  one book (950 × 1282 pages, release build, 2026-09-24): layout ~8 ms and
  drawing ~100 ms a page, plus ~10 ms once per process to index system
  fonts.
- No text layer: no selection or copy. Search still reads the page's text.
- Pages relying on floats, tables or multi-column text are approximate.

## Amendments

### 2026-09-24: a book of pages is a document

A fixed-layout book opens in the document reader like a PDF, through an
in-process `DocumentSession` (`preview-epub/src/pages.rs`); comics too
(ADR-015). Pages are drawn at the side the reader asks. A page with no size
holding one picture is that picture; other pages with no size take the size
of the book's first pages. The contents become the outline; search finds
pages.
