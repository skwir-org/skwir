# ADR-015: Pages have their own size and are rendered at the size shown

Status: accepted, 2026-09-24. Amends ADR-004 (fixed 2048 px renders) and
ADR-013 (drawn pages shown as an inspection's picture).

## Context

The reader rendered every page at a fixed side: 2048 px, the protocol's
cap, in continuous mode, and a page's size in the protocol was the size of
those pixels. Zoom was a share of them, so 100% meant nothing about the
page, a page shown at 400 px wide still cost 2048 px of rendering and
memory, and a zoom past 2048 px enlarged pixels. Fixed-layout books and
comics were shown by a separate page view in the inspector, with its own
paging, zoom and sharpening, and no continuous mode.

## Decision

- A page's size in the protocol (`PageContent::width`/`height`) is its own,
  in CSS pixels: a PDF's points times 96/72, a drawn page's viewport, a
  picture's pixels. Zoom is a share of it; layout never uses the pixels'
  size.
- The reader renders a page at the side it covers on screen, measured in
  prepaint, in 512 px steps up to `MAX_SIDE`, now 4096 px for every
  engine, and renders shown pages again when they are shown larger than
  they were asked for. Its first page, before anything was shown, is
  rendered at the side the last document needed (2048 px at first).
- A fixed-layout book (built-in engine) and a comic are documents: an
  in-process `DocumentSession` draws or decodes each page at the side asked.
  They get the reader's continuous and single-page modes, page field,
  outline, find and zoom. The inspector's page view is removed.

## Consequences

- At Fit in a side panel, pages render at 512 to 1024 px instead of 2048:
  about a quarter to a sixteenth of the pixels, and faster. Zooming in
  renders them again, up to 4096 px; a 4096 px page is about 50 MB, held
  only for pages on screen and their neighbours.
- 100% means the page's real size at 96 DPI; zoom presets and steps read
  accordingly. A zoom step from Fit starts at the scale shown.
- Drawn pages still have no text layer; search in a book of pages finds
  pages. A comic has no contents and no text.
- The protocol's meaning changed without a version: all three workers and
  the platform's validation changed together, and `tools/pdf/check.py`
  asserts a page's size is independent of the side rendered.
