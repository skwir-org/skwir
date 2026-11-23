# ADR-147: A page shows what it covers

Status: accepted, 2026-11-22, on the user's answers (asked to give comic
and PDF pages the rule ADR-146 gave pictures; chose to draw only what
shows of a page zoomed past what is held of it whole, the reader's 64 MiB
of pages unchanged). Built on Linux that night; the five helpers checked
by `tools/pdf/check.py` and `tools/djvu/check.py`, the reader in a window
on Xvfb.

## Context

- **The reader drew every page whole, at most 4096 pixels on its longer
  side** (`document::MAX_SIDE`), each helper clamping to it. An A4 page at
  400 % on a screen of two device pixels a pixel covers 8982 pixels down,
  shown from 4096: letters twice enlarged. A webtoon's page (800 × 12 000)
  was held at 273 × 4096, enlarged three times at 100 %, at Fit width
  already in a wide panel.
- **A side alone bounded what a page held**, as it bounded pictures
  (ADR-146): a tall page got a fraction of what an A4 one did.
- **Every engine can draw part of a page**: PDFium into a bitmap the
  part's size with the page's corner off it (`set_origin`), Poppler through
  cairo's translation, MuPDF through its matrix, DjVuLibre with a page
  rectangle and a rectangle to draw, djvu-rs with a region of its
  `RenderRequest`, in the page as shown. A
  comic's page decodes its region alone (`picture::decode_region`).

## Decision

1. **A page drawn whole is counted in pixels**: at most as many as an A4
   page at `MAX_SIDE` (`document::PAGE_PIXELS`, 11.9 megapixels), never
   fewer than `MAX_SIDE` held on its longer side (`document::held_side`).
   A squarer page is held as before; a tall one as long as an A4 one is
   wide (a webtoon's page whole at its own width). The reader asks each
   page at that (`DocumentScroll::whole_side`), so a zoom past it draws
   the whole no again. The helpers bound a whole page by the same
   function; the manager takes at most a square page's pixels at
   `MAX_SIDE`.
2. **Past it, what shows is drawn apart**: a page whose drawing covers
   more device pixels than its pixels whole hold has the part that shows,
   and a margin of half as much each way where a page's pixels allow it,
   drawn at the side it covers (`reader_scroll::part_wanted`,
   `Shell::document_parts`), up to `MAX_PART_SIDE`, over the page in its
   place (`DocumentElement::part`). One part a page, one asked at a time
   beside the whole pages, kept while the page shows and needs one, let
   go with it; what is drawn apart is chosen again whenever what shows
   changes, a part comes back or a search ends (amended 2026-11-23:
   chosen only as the view moved, a second page in view, or a view moved
   while a part was drawn, kept the whole enlarged).
   The user chose this over holding pages larger (the pictures' 16.7
   megapixels, or twice the cache): the reader's 64 MiB stays.
3. **A part is the protocol's**: `WorkerRequest::Render` carries an
   optional `part`, in shares of the page (`document::Part`), never in
   pixels: each engine lays its own pixels on the page at the side, draws
   whole pixels round the part, and says back the part it drew exactly.
   A part carries no text again (the whole carried it). The reader and
   the engines count a part alike (amended 2026-11-23): the reader sizes
   it by `Part::most_pixels`, never fewer than an engine draws, to
   `PAGE_PIXELS`; an engine draws `Part::drawn`, refused only past the
   manager's bound (`MAX_PAGE_PIXELS`). Counted by its shares alone, a
   part sized to the bound was refused once its whole pixels rounded it
   out.
   `DocumentSession::render_part` draws one; by default the whole is
   drawn and the part cut from it (`DocumentPage::cut`): a book laid out,
   a plugin from outside Skwir. Such a part holds no more than the whole
   shown, so a session says whether it draws parts alone
   (`draws_parts`), and the reader asks parts only of one that does; a
   part that comes back no sharper (a picture at its own size) is not
   asked again (amended 2026-11-23).
4. **Where a page shows is the element's**: each page says in prepaint
   what of it shows within its view, what it covers and what its drawing
   holds (`elements::document::Seen`), read once the column is laid out
   (`report_seen`), a page at a time and continuously alike.

## Alternatives

- **Pages as pictures** (whole to 16.7 megapixels): one zoomed page
  could fill the reader's 64 MiB, pushing its neighbours out.
- **Tiles in a grid**, each kept apart: more pieces to keep and drop for
  what one part with a margin does; a part is asked again only when the
  view leaves it.
- **Parts in pixels**: an engine rounds the page's size its own way
  (PDFium rounds, Poppler and MuPDF round up), and a part in another's
  pixels lands a pixel off.

## Consequences

- A page zoomed past its whole shows sharp where it shows, at any zoom up
  to `MAX_PART_SIDE`, after the part is drawn; meanwhile the whole,
  enlarged.
- A part's pixels count apart from the 64 MiB of whole pages: one a page
  shown, each at most `PAGE_PIXELS`.
- PDFium places a glyph's anti-aliasing by the bitmap's own pixels: drawn
  at an offset, a few edge pixels of a part differ from the whole's
  (fewer than one in two hundred, `tools/pdf/check.py`).
