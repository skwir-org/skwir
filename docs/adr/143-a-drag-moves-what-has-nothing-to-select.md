# ADR-143: A drag moves what has nothing to select

Status: accepted, 2026-11-19, on the user's answers (asked for a drag
mode beside the readers' selection, to move a document or a picture
zoomed in; chose Select text as the default, a default key only if one
is free and every key changeable, and the drag with no switch where
nothing can be selected). Built on Linux, checked in a window on Xvfb;
linted for Windows (`just check-windows`); macOS's check leaves the UI
out. Amended the same day after review: a press on a page where no
character of its text is under it moves the pages (decisions 1 and 2),
as nothing was selected from there; the I-beam shows only over
characters (5). Amended on 2026-11-20 after the review before the
release: a press selects from anywhere within a line of the text
(between words, in a paragraph's leading, a little before its first
letter and after its last), the margins, pictures and the room between
paragraphs moving the pages (1, 2); the cursor follows the pointer
within a page, and a selection's I-beam goes beside the pages (5).

## Context

- **Zoomed in, only the wheel, the touchpad, the arrows and the
  scrollbars moved what is shown.** A left drag on a document's page
  selected its text and a click followed a link; on a picture a drag did
  nothing. A page or a picture larger than the panel, both ways, was
  read in steps of a scrollbar.
- **Readers answer it with a tool.** Okular's Browse tool (Ctrl+1)
  beside its Text Selection tool (Ctrl+4), Firefox's Hand tool beside its
  Text Selection tool, Acrobat's Hand (H) beside Select (V), and Space
  held for the hand meanwhile. Image viewers have nothing to select: a
  drag moves the picture.
- **What a press on a page does today is the page's**
  (`elements/document.rs`): a press on its text begins a selection and
  is taken there; a press around it reaches nothing.

## Decision

1. **Where there is nothing to select, a drag moves what is shown, with
   no switch.** A picture zoomed past its room (in the panel, peek and
   the whole screen), a page with no text (a scan, a comic, a DjVu
   without its text layer: `reader::drags`), a page outside the lines
   of its text (its margins, between its paragraphs, a picture in it:
   `document::Page::selects_at`; amended), and the room
   around and between pages: the press holds what is shown, which then
   moves as far as the pointer, the point grabbed under it, until the
   button is let go.
2. **On a page's text (a press within one of its lines: on a letter,
   between words, in the leading between the lines of a paragraph, a
   little before its first letter and after its last; amended), a drag
   selects from its nearest letter, unless the reader's switch says to
   move.** The reader's bar
   has **Drag to move the page** (`ui.reader_drag`, lit while on), the
   same choice as the setting `reading.drag_moves` (Settings › Reading ›
   Pages), off by default: as Continuous scrolling, it is the user's,
   remembered, read when the reader for the next file is made. On, a
   drag across the text moves the page; a click still follows a link (a
   release within `pan::CLICK_SLOP` of the press, the page unmoved); a
   selection made before stays on its page.
3. **The middle button's drag moves what is shown anywhere**, whatever
   the switch says: what moves now and then needs no switch. Space held
   is not taken: it closes peek.
4. **Its key is H** in the reader (Acrobat's), free there (Okular's
   Ctrl+1 and Ctrl+4 are the views'), and changeable or freed on the
   Keyboard page as every command's.
5. **One mechanism, under what is shown.** A view that scrolls both ways
   lays `elements::pan::layer` under it, first drawn, so what is drawn
   over it (a page, a scrollbar, a drawer, the bar) hears a press first
   and keeps what it takes. It moves the handles the arrows and the
   scrollbars move (`pan::Moves`: a box's offset; the column's list and
   its sideways scroll), so it goes no further than they do: a drag never
   turns a page read a page at a time. The press is a `pan::Grab` shared
   with the pages, which hold it themselves where their drag moves them
   and tell a click by it, and mark it while they select. Each move is
   taken once (a guard: no two layers share a grab). The hand shows open
   where a drag would move something (over a page, outside its lines
   and links), and closed while it does; the I-beam shows only over the
   lines of its text, and wherever a selection's drag goes (the layer asks for
   it too, beside the pages), with no hand. Each follows the pointer: a
   move onto a link, text or the room beside them draws once for its
   cursor (`document::cursor_at`; amended). The
   layer takes a press only where what it moves can move, and draws
   only for what changes (a hand, an offset held within its bounds).
6. **A film keeps its click**: a click plays or pauses it, so only the
   middle button drags one zoomed in.
7. **Nothing is thrown.** What is shown stops when the pointer does: a
   throw would draw frame after frame (ADR-000).

## Consequences

- A page's room (its hitbox) is the page drawn, not the letterbox
  around it read a page at a time: the hand shows there, and a press
  there is the layer's.
- What a press on text does is unchanged with the switch off; a press
  beside a page, or on a page outside the lines of its text, which did nothing,
  now holds the pages.
- Nothing shows the cursor in a capture on Xvfb: the hands were read in
  the code, not seen.
