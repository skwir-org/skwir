# ADR-111: What is shown is printed as a PDF, through the system's dialog

Status: accepted 2026-10-31 with the user (GitHub #2); building, step by
step, as `docs/PLAN.md` ("Grammars and printing") records.

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
   (`Compose::Pages` in the PDF helper), never the printer.
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
