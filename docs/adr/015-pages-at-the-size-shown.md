# ADR-015: Pages have their own size and are rendered at the size shown

Status: accepted, 2026-09-24. Built. Amends ADR-004 (fixed 2048 px renders)
and ADR-013 (drawn pages shown as an inspection's picture).

## Context

The reader rendered every page at 2048 px (the protocol's cap), and a
page's protocol size was those pixels. 100 % meant nothing, a page shown
400 px wide still cost 2048 px, and zooming past 2048 px enlarged pixels.
Fixed-layout books and comics used a separate inspector page view with its
own paging and zoom, and no continuous mode.

## Decision

- **A page has its own size** (`PageContent::width`/`height`), in CSS
  pixels: a PDF's points × 96/72, a drawn page's viewport, a picture's
  pixels. Zoom is a share of it; layout never uses the rendered size.
- **Render at the size shown:** the side the page covers on screen,
  measured in prepaint, in 512 px steps up to `MAX_SIDE` (4096 px, every
  engine), rendered again when shown larger. The first page uses the side
  the last document needed (2048 px at first).
- **Books of pages are documents:** fixed-layout books (built-in engine)
  and comics open through an in-process `DocumentSession` that draws or
  decodes each page at the side asked, with the reader's modes, page field,
  outline, find and zoom. The inspector's page view is removed.

## Consequences

- At Fit in a side panel, pages render at 512–1024 px instead of 2048 (a
  quarter to a sixteenth of the pixels), faster. A 4096 px page is about
  50 MB, held only for pages on screen and their neighbours.
- 100 % is the page's real size at 96 DPI; a zoom step from Fit starts at
  the scale shown.
- Drawn pages have no text layer: search in a book of pages finds pages. A
  comic has no contents and no text.
- The protocol's meaning changed without a version: the three workers and
  the platform's validation changed together, and `tools/pdf/check.py`
  asserts a page's size does not depend on the side rendered.
