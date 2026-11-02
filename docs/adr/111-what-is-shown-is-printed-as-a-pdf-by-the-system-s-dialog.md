# ADR-111: What is shown is printed as a PDF, through the system's dialog

Status: accepted 2026-10-31 with the user (GitHub #2); built the same
day, as `docs/PLAN.md` ("Grammars and printing") records: checked on
Linux, compiled for Windows and macOS (`just lint-all`), not run there.
Decisions 2 and 5 revised 2026-11-01 with the user, after trying it:
one Print…, Skwir's own dialog first (see Revision). Decision 4 revised
the same evening: four and eight pages a side, the paper of a PDF
chosen.

## Context

- **Skwir could not print.** GitHub #2 asks to print a document from its
  preview, as a booklet too (two pages to a side of a sheet, folded).
- **Every system has a dialog of its own.** The free desktops' is reached
  through the print portal (`org.freedesktop.portal.Print`, version 4
  here, GTK's and KDE's back ends): `PreparePrint` shows it and answers
  its settings and a token, `Print` prints a file handed by descriptor
  with them. macOS has its print panel (`NSPrintPanel`) and CUPS; Windows
  its print dialog (`PrintDlgExW`) and its spooler.
- **Through the portal the application applies what was chosen.** GTK's
  own printing through the portal renders only the pages chosen and
  applies even or odd sides, the reverse order and the scale itself; the
  portal's back end prints the file as it is handed, the printer making
  only the copies and the pages per sheet (`gtkprintoperation-portal.c`,
  xdg-desktop-portal-gtk's `print.c`, read 2026-10-31).
- **Skwir shows far more than PDFs.** Office documents are converted to
  PDF by LibreOffice for their pages; everything else is drawn by Skwir
  from an inspection (blocks and runs, tables, code), a picture, a
  drawing, or pages a reader draws.
- **No PDF was written anywhere.** PDFium writes one of another PDF's
  pages (Chromium's printing does) but lays out no text.

## Decision

1. **Everything printed is a PDF**, and one print path takes it: the file
   itself when it is a PDF; the PDF a document's reader reads
   (`DocumentSession::pdf`: an office document converted, a plugin's PDF);
   else one the print helper makes of what the preview shows, asked as
   the window asks: a provider's converted file, its inspection, a
   reader's pages, then what the window shows itself (a drawing, a
   picture, a text).
2. **The system's dialog chooses; Skwir applies.** Linux: the print
   portal, the pages, sides and order applied by Skwir to the file it
   hands over; macOS: the print panel, then CUPS's `lp` with its printer,
   copies and sides (compiled only); Windows: the print dialog, then the
   PDF helper drawing each page on the printer with the settings it left
   (`FPDF_RenderPage` on the printer's device context; tried under Wine,
   which has no printer). A desktop without a print portal says so and
   offers Save as PDF. Pages are chosen in the dialog (All, Current page,
   Selection, typed) or typed for Save as PDF and scripts
   (`print::Pages`, from 1, one parser); Skwir picks them
   (`Compose::Pages` in the PDF helper), never the printer. Several
   files print as one job: each made whole, joined by the PDF helper
   (`Compose::Append`), the pages chosen counted through them all.
3. **PDFs Skwir makes are made by a helper** (`skwir-print-worker`: krilla
   writes the PDF, parley lays the text out), isolated, fed a description
   (`print::Job`) and the files it names, in the light look whatever the
   window's, code colored and numbered, the file's name above and the page
   below, tagged for screen readers, with bookmarks, title and author.
4. **A booklet is laid out by PDFium in the PDF helper**
   (`Compose::Booklet`): the pages chosen, in the folding order the core
   computes (`print::booklet::sheets`, signatures of a given size), two
   to a side of the sheet the dialog chose, turned landscape; the backs
   turned half a turn when the printer turns the sheet over its long
   edge; white margins trimmed by one ink box for every page when asked.
   A document Skwir makes is laid out at half a sheet, so its text is not
   shrunk. Even or odd sides apply to the sheets' sides, which prints a
   booklet by hand on a printer of one side.
5. **Printing is a plugin** (`print`): Print… (Ctrl+P), Print as
   Booklet…, Save as PDF and Save as Booklet, in the file's menu (3xx,
   made from it), in the preview's menus and on Ctrl+P there. The window
   names no plugin: a plugin's items of the preview's menu run through
   `ui.preview_invoke`, which adds what the preview shows (the file, its
   page, what is selected) and is the user's alone.
6. **An agent's print asks first**, as any command that acts; Save as PDF
   is a plan, reviewed as plans are. Skwir sends nothing over the network:
   the system's print service does.

## Rejected

- **A print dialog of Skwir's own over CUPS** (`lpstat`, `lp`): the
  desktops have theirs, with the printers' own options.
- **GTK's or Qt's print operation in the window**: a toolkit Skwir does
  not link.
- **MuPDF's HTML layout, Typst, or LibreOffice to lay out text**: MuPDF is
  AGPL and used only when chosen (ADR-014); Typst is a large compiler;
  LibreOffice is slow and often absent.
- **Writing PDFs in the window's process**: fonts, layout and images of
  any size in the process the window runs in.

## Consequences

- Printing needs PDFium (packaged) to pick pages and lay booklets out; a
  PDF printed whole and as it is needs nothing more than the dialog.
- A preview the print path does not know (audio, video, an archive, a
  map) is said not printable.
- Two helpers more are packaged and probed: the print helper here, the
  tree-sitter one beside it (ADR-110).
- A font whose licence forbids embedding it (its OS/2 `fsType`
  restricted, or bitmaps only) is never written in a PDF Skwir makes:
  the print helper passes it over for the next face, and refuses a
  specimen of one (decided 2026-11-01, after the review).

## Revision (2026-11-01): one Print…, Skwir's dialog first

Tried by the user the day after: Save as PDF and Save as Booklet did
nothing from the preview's More menu (`ui.preview_invoke` dropped the plan
the command answered); Save as Booklet asked no fold where Print as
Booklet did; and no page could be chosen, because the dialog that should
offer them is the desktop's: KDE's print portal (the user's) folds its
range under Options, never enables Current page or Selection (it reads
neither `has_current_page` nor `has_selected_pages`), and keeps only the
first and last page of `1-3, 7`. Four items and a submenu were also more
than one thing to do.

- **One Print…** (Ctrl+P) in the file's menu (3xx, no submenu) and the
  preview's, for files, and for a folder with nothing selected (its
  listing). It opens **Skwir's own dialog**, a form the window draws
  (`skwir_core::form`, ADR-086, given rows of choices drawn as cards with
  a picture, fields shown only while another holds a value, and a choice
  that names the answer's button): to a printer or a PDF beside the file;
  which pages (All, the page the preview shows, its selection, typed);
  the layout (as shown, a booklet whose folds are drawn with the page
  numbers where they land, a contact sheet of several pictures); one PDF
  of several files or a PDF each. It opens on the last choices but the
  pages, kept until Skwir quits, never a setting.
- **The system's dialog chooses only where and how**: the printer, the
  paper, the copies, the sides. It is handed every page (the portal's
  `print-pages` `all`, neither the current page nor the selection
  offered); macOS's panel and Windows' dialog show no range. What a
  portal's range is still changed to applies to what is sent, a
  booklet's sides once folded, as its even or odd sides do.
- **Two commands**: `print.print` and `print.save_pdf` take `pages`
  (`all`, `current`, `selection`, typed), `layout` (`as_shown`,
  `booklet` with `signature` and `trim`, `contact_sheet`, `listing`) and,
  to save, `together` and `dest`. `print.booklet`, `print.save_booklet`,
  `print.contact_sheet` and `print.listing` are gone. The window's
  dialog answers the plugin alone (a form); a script or an agent passes
  the arguments, `asked` going straight to the system's dialog. A PDF
  chosen in the dialog is a plan the plugin applies as the user's own
  action is (`App::apply_whole`): undone to the trash.
- **Not Skwir's dialog over CUPS**, which stays rejected: it chooses what
  and how, never the printer.

## Revision (2026-11-01, after review): what is picked is made, the scale applied

- **The system's dialog's scale is applied**, decided with the user: a
  portal's back end and CUPS print a file as it is handed, so each page
  of what Skwir hands over is drawn at that scale about its middle, on a
  page of its own size, by the PDF helper (`Compose::Pages`'s `scale`),
  a booklet's sides as any page. What the system's dialog chooses is
  only what sending honours (`print::Chosen`). Windows' printer applies
  its own from the settings its dialog left.
- **Pages are picked where they are made.** A PDF is counted only when
  picking needs it (pages, a booklet, sides, order, scale), by the
  reader the settings choose; sent as it is, it is not read. The print
  helper writes only the pages named before it lays out
  (`Job::pages`), a reader's pages are drawn only when picked. The
  `PDFium` library the settings name is the program's
  (`document::pdfium_choice`): the documents plugin, which declares the
  setting, keeps it there, and every `PDFium` helper is told it, the
  ones that compose and print included.
- **A page turns to fit its paper**: on Windows, where nothing turns it,
  a page wide on paper fed upright (a booklet's sheet) is drawn turned a
  quarter turn. On macOS `lp` is told the printer's CUPS queue
  (`PMPrinterGetID`, never the name the panel shows) and the paper chosen;
  the orientation is the PDF's own, which CUPS turns to fit.
- **Made here, kept where asked.** PDFs are made in the system's
  temporary folder and composed beside them on disk, never in memory;
  Save as PDF stages only the finished PDF where it is kept, and refuses
  in its `check` a folder that takes only copies written in place (a
  camera). What reads the files (a listing, the files, the helpers) runs
  in their remote's lane; the dialogs wait holding none.
- **Through the preview, the entry shown**: `ui.preview_invoke` names a
  link itself, as the file menu does; Save as PDF goes beside it, and
  what is read is what it opens, typed by that file's name.

## Revision (2026-11-01, evening): four and eight pages a side, the paper of a PDF

Asked by the user: booklets of 8 and 16 pages a sheet, and which paper a
PDF or a booklet is made on, chosen rather than assumed.

- **Pages a side: 2, 4 or 8** (`print::booklet::PerSide`, the dialog's
  "Pages a side" row with a picture of each, `per_side` of `print.print`
  and `print.save_pdf`, any other count refused). The sheet is folded in
  half once, twice or three times. Where each page lands is found by
  folding a sheet in simulation (`booklet::sheets`: the last fold left
  over right, before it top over bottom, before that right over left),
  never by a table: it lays a printer's 8- and 16-page signatures (front
  5, 4 / 8, 1 and back 3, 6 / 2, 7 for four a side), which the tests
  check. Each sheet is a gathering of its own; the sheets are put one
  after another, not one inside the other (a folded sheet of 4 or 8 a
  side is too thick to nest), so signatures and "Folded as" are offered
  for 2 a side alone.
- **The pages of a top row are upside down**, turned about their own
  middle in their place: folded, they read upright. A sheet folded an
  odd number of times (2, 8 a side) is laid landscape, else upright; the
  backs are laid for a sheet turned over its vertical edge, turned back
  when the printer turns it over the other (`turn_backs`, from the sides
  the system's dialog chose). A saved booklet is laid for that edge.
- **A page is a cell of the sheet**, as large as the grid leaves it
  (`Paper::page_of`): a document Skwir makes is laid out at that size, so
  its text is not shrunk; its margins at most a tenth of the cell's
  narrower side. Four or eight a side, PDFium places each page
  (`Compose::Booklet`'s `per_side`) and the folds are marked by short
  grey ticks in the front's margins, where a guillotine takes them.
- **How to fold is said** with what was made, for 4 and 8 a side: page 1
  outside and upright, the closed folds cut open, the sheets one after
  another. Said in the plan's own toast after what it did
  (`StepContext::say`, new in core), or after what was sent to a printer;
  never a notification of its own, which the toast would cover.
- **The paper of a PDF is chosen** in the dialog when saving (A4,
  Letter, A5, A3, Legal: `Paper::NAMED`), the system's own first
  (`skwir_platform::print::paper_name`: the locale's, as before), the
  last chosen kept until Skwir quits; `paper` of `print.save_pdf` names
  it for a script. Printing, the system's dialog chooses it, as before.
- **Rejected for now**: a one-sheet zine (eight pages on one side, cut
  in the middle and folded), offered and not asked.

## Revision (2026-11-02): what the portal answers to `Print`

The user printed a page through KDE's print portal: it came out of the
printer, and Skwir said it had not been printed ("the print dialog
ended (2)").

- **KDE's back end answers 2 when the job went.** Its `Print` runs `lpr`
  and reads `lpr`'s success (0) as a failure, answering 2, "ended
  otherwise"; when `lpr` fails it answers `lpr`'s own code, 1 for most
  refusals (xdg-desktop-portal-kde's `PrintPortal::Print`, 6.8.2 and its
  main branch, read 2026-11-02). Seen with KDE's portal on a display
  and bus of their own, a stand-in `lpr` exiting 0: it answered 2.
- **GTK's answers 0 before the job is printed**, once it handed it on
  with the dialog's token, whatever came of it; and GTK's own printing
  through the portal reads no answer to `Print` at all, only whether the
  call was taken (`gtkprintoperation-portal.c`).
- **So Skwir reads `Print`'s answer as sent for 0 and 2**, and as not
  taken for any other code (1, or `lpr`'s own); a 2 from `PreparePrint`,
  the dialog, is still an error. What reaches paper is the printing
  system's to say, as it was: "Sent" means handed to it.
