# ADR-012: Reading mode keeps the part of a document's CSS that says how text reads

Status: accepted, 2026-09-24. Amends ADR-009, which said reading mode does
not apply publisher CSS.

## Context

Reading mode (`bibli_platform::rich_preview`, shared by EPUB, HTML files,
Markdown's raw HTML and mail) read a page for its tags alone. What books
actually contain made that lose a lot:

- Most books carry their emphasis and layout in stylesheet classes, not in
  `<i>` and `<em>`. Calibre flattens styling into generated classes
  (`.calibre5 { font-style: italic }`); InDesign exports local formatting as
  `CharOverride-N` spans. Those books read with no italics, no centred titles
  and no hidden Kindle-only text.
- EPUB chapters are XHTML. `<title/>` or `<script/>` closes itself there, but
  the HTML parser reads it as an open element and takes the rest of the file as
  its text: such a chapter showed nothing.
- Only the innermost block's `id` was a link target, so footnote links (to an
  `<aside id>`), links to a `<section id>` and the targets of a page list went
  nowhere; footnotes sat in the middle of the text, and printed page numbers
  were read into sentences ("Before12 after").
- An SVG-framed cover page (what Calibre and Sigil write) was blank.

The alternatives (a browser engine, MuPDF, crengine, Blitz, litehtml) were
surveyed for this decision; each costs tens of megabytes, a C++ binding or a
second rendering path, and none keeps the reader's theme, text size, selection
and find. They remain possible later as an optional "original layout" view in
the document reader, not as a replacement for reading mode.

## Decision

Reading mode computes a small cascade over the parsed tree
(`rich_preview/style.rs`), with the selector matching and CSS tokenizer scraper
already builds on (`selectors`, `cssparser`; no new crate enters the tree):

- Order as in CSS: a built-in stylesheet that states what tags mean (which
  make blocks, which never show, HTML's own emphasis), then the document's
  linked and `<style>` sheets, then `style` attributes; `!important` above
  each origin; specificity and order within one.
- Only properties that say how text reads: `display` (block, inline, none),
  weight, style, underline and line-through, `text-align`, `white-space`
  (preserved lines), `font-size` relative to the page's body text, and left
  margins and padding as an indent in ems (bounded). Fonts, colours, absolute
  sizes, boxes, floats and positioning are not computed: the text stays in the
  theme and at the reader's size.
- `@media` rules apply only for `all` or `screen` without conditions.
  `@import`, `@font-face`, `@page` and `:has()` are not followed. A page's
  stylesheets are bounded together by `TEXT_INPUT` and to `STYLE_RULES` rules.
  Rules are indexed by the rightmost id, class or tag of their selector.
- Linked stylesheets are resources like pictures: the caller resolves them
  inside the document's bounds (an EPUB's container through its bounded reader,
  an HTML file's own folder with symlinks resolved), never over a network.

XHTML is read as XML first (`rich_preview/xml.rs`, roxmltree, already a
dependency), into the same tree the HTML parser builds; HTML's named entities
become characters first, since a parser that fetches no DTD cannot know them.
What is not well-formed goes to the HTML parser as before.

The block model gains what this needs, as structure rather than style:
`Block::align` (replacing `centered`), `indent`, `size` (relative, only for
running text), `anchors` (every `id` that lands in a block), and
`Run::underline`. Footnotes (`epub:type` `footnote`/`note`/`rearnote`,
`role="doc-footnote"`) move to the end of the page under a rule; page breaks
(`pagebreak`, `doc-pagebreak`) are link targets, never text. Ruby reads as
`base(reading)` (HTML's own fallback), an SVG frame shows the picture it
frames and other SVG drawings are rasterized by resvg (the version gpui links)
with every external reference refused. A fixed-layout book's page that is a
single picture shows as a page, like a comic's.

## Consequences

- A book reads with its italics, titles, verse indents and hidden text as its
  publisher meant, in the reader's theme. Search in a book reads chapters with
  their stylesheets, so hidden text is not a match.
- Superscripts and small capitals are not drawn as such: a GPUI text run has
  no size or baseline of its own (`TextRun`), so a note reference reads as a
  link beside the word. A vendored patch could add them.
- Pictures are decoded as they come into view (see below), so a page names
  as many as it has.
- Embedded fonts are not used: GPUI cannot unregister a font, and fonts are
  never registered with the UI permanently (ADR-009).

## Addendum, 2026-09-24: pictures as they come into view

A page decoded its pictures with its text, at most `EMBEDDED_IMAGES` (eight)
of them, so an illustrated chapter lost the rest, and the eight cost their
pixels whether or not they were ever scrolled to. A provider that can read a
picture again now names it (`Picture::Later`) with the size it will be shown
at, read from the file's header (up to `PICTURE_HEADER` bytes, the whole file
for SVG), and decodes it on request (`PreviewProvider::picture`, dispatched to
the provider that answered the inspection). The view reserves the picture's
box, asks for what its rows drew and their neighbours one at a time, and keeps
64 MiB of decoded pixels, dropping atlas entries with them, like the reader's
pages. Pictures that cannot be read again (a message's content-ID parts) and
SVG drawings inside a page are still decoded with it, within the old bound.

On a synthetic book of 40 plates of 1200 × 800 in one chapter, in a debug
build on an isolated display, every plate showed; placeholders held their
place while scrolling fast; PSS rose from 241 MB with the first plates shown
to 327 MB with every plate seen, and stayed at 361 MB over two further passes
from end to end; the idle process used the same CPU before and after.
