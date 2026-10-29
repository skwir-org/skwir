# ADR-103: Text read a screen at a time

Status: accepted and built 2026-10-29 on Linux (Windows compiled only;
macOS not compiled: see Consequences). Gives reflowed text the choice ADR-013's addendum gives
a book laid out in pages (continuous or a page at a time); what turns is
what ADR-073 sets in the reading font.

## Context

- **Documents turn, text scrolls.** The document reader shows a PDF, a
  book laid out in pages or a comic as one column or a page at a time
  (`reading.continuous`), and a notch of the wheel turns a page
  (0099968a). A reflowed EPUB, most books, is an inspection: one chapter
  in a scrolling list, the wheel moving it by lines and, resting at its
  end, going on to the next chapter. The user asked for the same choice
  for books "and similar", the wheel flipping a page a notch.
- **What a page is, reflowed.** A reflowed chapter has no pages but the
  ones the panel makes. E-readers lay a whole book out to number them;
  the inspector's list lays out what is in view, estimating the rest,
  and a book may keep all its text in one chapter. Measuring a whole
  chapter means shaping all of its text on the window's thread
  (ADR-000's first principle).
- **gpui's list fills its view.** A top whose items do not fill the view
  is moved up until they do: a chapter's last page, cut short, would be
  drawn from higher up, repeating the end of the page before.

## Decision

1. **A choice of its own.** `reading.text_continuous` (on by default),
   apart from the document reader's `reading.continuous`: a reader may
   turn pages of PDFs and scroll books, or the other way round. Its
   button stands after Full width in the bar (`ui.text_continuous`),
   lit while the text scrolls, and in the Settings dialog beside the
   documents' choice.
2. **Everything read.** What is shown in the reading font
   (`inspector::reads`: Markdown and HTML in reading mode, word
   processors' documents, reflowed books, mail) turns a screen at a time
   when it is off. Code, text files, tables and the rest scroll.
3. **A page is a screen, cut between lines.** It runs from the line at
   its top to the last line that fits whole: never through a line, nor
   a picture or a row of cells taller than what is left (it starts the
   next page). Only something taller than the panel is cut where the
   panel ends. Where lines are comes from the texts themselves: each,
   prepainted, tells its bounds and line height (`inspector_pages::Drawn`);
   a table row's cells are cut where all of them are between lines.
4. **Pages are where the reader is.** The next page starts with the line
   the last one left out; nothing lays out the chapter first, so a book
   in one long chapter turns at once. Back retraces the pages turned
   through, exactly; with nothing behind (after a link, a match found, a
   chapter's end reached by Ctrl+End or by turning back into it), the
   page before is the one that ends where this one starts, found over
   the frames its items take to be laid out. Pages are not counted: the
   bar keeps counting chapters or printed pages.
5. **Turning.** A notch of the wheel, Down, Page Down and Right turn on;
   Up, Page Up and Left back (Left and Right are the reader's page keys,
   as in the document reader); Ctrl+Home and Ctrl+End go to the
   chapter's first and last page. Past the last page comes the next
   chapter's first, before the first the chapter before's last. A turn
   waits until the page turned to is drawn; nothing turns while a
   chapter is on its way. A trackpad's swipe turns one page, and turns
   again after a pause (`Flip`): gpui's Wayland and X11 report no end to
   a gesture.
6. **The list stays the one source of where the text is.** Links, find,
   printed pages and the contents move it as before; a page starts at
   the item they bring to the top (an item already whole on the page
   shown stays where it is). A top found inside a line (the scrollbar
   before turning pages, another width or text size) moves up to that
   line's top. The list ends with a spacer as tall as the window, so a
   chapter's last page may end short; the pager (`inspector_pages::overlay`),
   after the list in its parent, reads the frame, covers what is past
   the page's end in the panel's paper and keeps the pointer from it,
   and takes the wheel in the capture phase before the list would
   scroll. The vertical scrollbar is not drawn, as for a page that fits
   in the document reader.

## Settled

- No page numbers within a chapter, no estimate of them, no progress
  bar: the user chose screens from where the reader is over numbered
  pages that need the chapter laid out first (2026-10-29).
- One page a screen, never two side by side.
- The choice is the user's for all text, not each file's.

## Consequences

- **Checked** on Linux, the debug build on Xvfb at 1280 × 860 with a
  synthetic book (three chapters of numbered paragraphs) and a Markdown
  file: notches turned one page each, every page starting with the line
  after the last one's end; turning back gave pixel-identical pages;
  the last pages of chapters ended short; past them the next chapter
  opened at its first page, and back from it the last page of the one
  before ended at its end; five notches 50 ms apart turned five pages;
  Page Down, Right, Left, Up, Ctrl+Home and Ctrl+End through their
  commands; find landed on a page holding the match; the text size
  changed between pages kept whole lines. Unit tests: the cut, a page
  found back from its end over frames, the swipe.
- **Not checked**: a trackpad (Xvfb's input is a wheel's notches); a
  high-resolution wheel, whose parts of a notch (Wayland's value120)
  each turn a page by the code, as in the document reader; the
  session's Wayland. Windows is compiled only (`just check-cross`);
  macOS not at all: check-cross leaves `skwir-ui` out of its macOS pass
  (it needs Apple's SDK, `just package-macos`). The pager names no
  system.
- **What a screen reader hears** still holds the lines past the page's
  end that are laid out under the cover.
- A text size or width change keeps the list's pixel offset into the
  item at the top, as when scrolling, then moves up to the line's top:
  the page may start a line or two from where it did.
