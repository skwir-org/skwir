# ADR-012: Reading mode keeps the part of a document's CSS that says how text reads

Status: accepted, 2026-09-24. Amends ADR-009, which said reading mode does
not apply publisher CSS. Amended the same day twice (pictures as they come
into view; positions within a line and notes beside the text).

## Context

Reading mode (`skwir_platform::rich_preview`: EPUB, HTML files, Markdown's
raw HTML, mail) read a page for its tags alone, and lost much of what books
hold:

- Books carry emphasis and layout in classes, not `<i>`/`<em>`: Calibre
  flattens styling into generated classes (`.calibre5 { font-style: italic }`),
  InDesign exports `CharOverride-N` spans. They read with no italics, no
  centred titles, and hidden Kindle-only text shown.
- EPUB chapters are XHTML, where `<title/>` or `<script/>` closes itself; the
  HTML parser reads it as open and swallows the rest of the file: the chapter
  showed nothing.
- Only the innermost block's `id` was a link target, so links to footnotes
  (`<aside id>`), to a `<section id>` and from a page list went nowhere.
  Footnotes sat mid-text; printed page numbers ran into sentences ("Before12
  after").
- SVG-framed cover pages (Calibre, Sigil) were blank.

Alternatives surveyed: a browser engine, MuPDF, crengine, Blitz, litehtml.
Each costs tens of megabytes, a C++ binding or a second rendering path, and
none keeps the reader's theme, text size, selection and find. They remain
possible as an optional "original layout" view in the document reader, not as
a replacement for reading mode.

## Decision

**A small cascade** over the parsed tree (`rich_preview/style.rs`) on the
selector matching and CSS tokenizer scraper already uses (`selectors`,
`cssparser`; no new crate):

- CSS order: a built-in sheet stating what tags mean (blocks, never shown,
  HTML's own emphasis), then the document's linked and `<style>` sheets, then
  `style` attributes; `!important` above each origin; specificity and order
  within one.
- Only what says how text reads: `display` (block, inline, none), weight,
  style, underline, line-through, `text-align`, `white-space` (preserved
  lines), `font-size` relative to the body text, and left margin and padding
  as a bounded indent in ems. Fonts, colours, absolute sizes, boxes, floats
  and positioning are ignored: text keeps the theme and the reader's size.
- `@media` applies only for `all` or `screen` without conditions; `@import`,
  `@font-face`, `@page` and `:has()` are not followed. A page's sheets are
  bounded together by `TEXT_INPUT` and to `STYLE_RULES` rules, indexed by the
  rightmost id, class or tag of each selector.
- Linked sheets are resources like pictures, resolved by the caller inside
  the document's bounds (an EPUB's container through its bounded reader, an
  HTML file's folder with symlinks resolved), never over a network.

**XHTML is read as XML first** (`rich_preview/xml.rs`, roxmltree, already a
dependency) into the tree the HTML parser builds. HTML's named entities
become characters first, since a parser that fetches no DTD cannot know them.
What is not well-formed goes to the HTML parser.

**The block model** gains, as structure rather than style, `Block::align`
(replacing `centered`), `indent`, `size` (relative, running text only),
`anchors` (every `id` landing in the block) and `Run::underline`.

- Footnotes (`epub:type` `footnote`/`note`/`rearnote`, `role="doc-footnote"`)
  move to the page's end under a rule.
- Page breaks (`pagebreak`, `doc-pagebreak`) are link targets, never text.
- Ruby reads as `base(reading)`, HTML's own fallback.
- An SVG frame shows its picture; other SVG is rasterized by resvg (gpui's
  version) with every external reference refused.
- A fixed-layout book's single-picture page shows as a page, like a comic's.

## Consequences

- Books read with their italics, titles, verse indents and hidden text as
  the publisher meant, in the reader's theme. Book search reads chapters with
  their stylesheets, so hidden text never matches.
- Superscripts, subscripts and small capitals are drawn, and pictures decode
  as they come into view, so a page shows them all (Amendments).
- Embedded fonts are not used: GPUI cannot unregister a font, and fonts are
  never registered with the UI permanently (ADR-009).

## Amendments

### 2026-09-24: pictures as they come into view

A page decoded its pictures with its text, at most `EMBEDDED_IMAGES` (eight):
an illustrated chapter lost the rest, and the eight cost their pixels even if
never seen.

- A provider that can read a picture again names it (`Picture::Later`) with
  its display size, read from the file's header (up to `PICTURE_HEADER`
  bytes; all of an SVG), and decodes it on request (`PreviewProvider::picture`,
  sent to the provider that answered the inspection).
- The view reserves the picture's box, asks one at a time for what its rows
  drew and their neighbours, and keeps 64 MiB of decoded pixels, dropping
  atlas entries with them, like the reader's pages.
- Pictures that cannot be read again (a message's content-ID parts) and SVG
  drawn inside a page still decode with it, within the old bound.

Synthetic book of 40 plates of 1200 × 800 in one chapter, debug build,
isolated display: every plate showed, placeholders held while scrolling fast,
idle CPU unchanged. PSS: 241 MB with the first plates shown, 327 MB with all
seen, 361 MB (stable) after two more end-to-end passes.

### 2026-09-24: positions within a line, and notes beside the text

A GPUI text run had no size or baseline of its own, so `<sup>`, `<sub>` and
`font-variant: small-caps` read as plain text ("note1", "mc2").

- Vendored patch 0006 gives a run a position (`gpui::RunPosition`). A line
  whose runs all sit on the baseline is shaped and painted as before;
  otherwise each stretch of one position is shaped at its own size, the
  stretches joined, and each run painted at its size and height.
- The cascade computes `vertical-align` and `font-variant`; small capitals are
  drawn as reduced capitals of lowercase letters, keeping text offsets.
- The patch touches the three platform shapers only to name the field; the
  macOS and Windows edits were not compiled at the time.

A note reference (`noteref`, or a raised link) opens the note in a popover
under it instead of scrolling away; "Go to note" still goes there. A note in
another chapter is read on a worker. Checked in a window on an isolated
display: a footnote on the page and an endnote in another chapter opened
under their references, Escape closed the popover, "Go to note" reached the
endnote's chapter.
