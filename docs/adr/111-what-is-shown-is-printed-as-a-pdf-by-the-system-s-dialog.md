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
- **The portal's front end answers 2 too, when the back end failed**:
  xdg-desktop-portal's `print_done` starts its answer at 2 and keeps it
  when its call to the back end errs ("Backend call failed"), so on GNOME
  a job GTK's back end could not take answers 2 as well (found in review,
  2026-11-02).
- **So Skwir reads `Print`'s answer by the back end the desktop likely
  runs** (`portal::BackEnd`: KDE's where `XDG_CURRENT_DESKTOP` names KDE,
  as the portal's `kde-portals.conf` picks it): 0 is sent everywhere; 2
  is sent from KDE's, and elsewhere said as handed to the desktop's
  printing, which did not confirm it took it (`print::Sent::Unconfirmed`),
  never as sent; 1 is the user cancelling a dialog the back end showed
  again, said as the first dialog's cancel is (nothing), except from
  KDE's, where it is `lpr`'s refusal; any other code is not taken. A 2
  from `PreparePrint`, the dialog, is still an error. What reaches paper
  is the printing system's to say, as it was: "Sent" means handed to it.

## Revision (2026-11-06): any place's listing, where a PDF goes said

The user asked for Print Listing… in any place, not only folders (a
search, a filtered folder, a tag, the trash), and, for where such a
listing's PDF goes, its folder, else Documents, said in the dialog and
changeable, for every PDF saved.

- **Offered wherever nothing is selected** (`when` `empty`, the menu and
  Ctrl+P), any location's listing printed as the pane shows it
  (`Pane::order`: its filter and lens applied, the folders opened in
  place indented). A filtered folder printed already; its title now says
  its filter's words. The title is a folder's path, else the location's
  name (`App::location_title`: "Trash", "report in Papers", the tag).
- **The list's columns, asked of the provider, never of its scheme**:
  Folder where files come from many folders (not a folder, nor a place
  of one folder's files, `Trail::one_folder`), the trash's from where it
  was put there from (`keys::TRASHED_FROM`); Deleted where the place
  restores (`Verbs::restore`, `keys::TRASHED`). A place listed says its
  kind (`keys::PLACE_KIND`) and how many files it holds (`keys::FILES`).
- **What the pane has not read is read in each file's lane**
  (`fs::reach::by_lane`), a job of that lane per remote: a tag's or
  Recent's files may be on several servers. The listing is no longer
  made in one folder's lane: its rows are copied under the workspace's
  lock and the PDF made of the table reads no file.
- **Save as PDF's step names a location** (`Of::Listing { pane,
  location, named }`, `folder` gone), applied while the pane still shows
  it. A listing's PDF is named after the folder, else after the place's
  name, a separator a dash, cut to what a folder takes
  (`op::clash::kept_name`), "Listing.pdf" where this system refuses it.
- **Where it goes**: the folder listed; else the folder the place is of
  (its trail's `base`: a search's folder, a tag's; else the first of its
  crumbs that is a folder); else the user's Documents
  (`skwir_platform::places::user_dirs`), else home.
- **Save in, in the dialog, for every PDF** (`dialog::Saved`): an open
  choice shown when PDF is chosen, opening on where it goes by itself
  (beside the file, beside them, "Beside each file" for files of several
  folders, the folder of a listing, said under it), then the folder
  chosen last, the other pane's, Documents, each once; any other typed,
  a whole path or from `~`, checked as a folder in its lane within
  400 ms (one on a server that answers slowly is taken as typed, the
  plan checking it again). "To" says "PDF", no longer "PDF beside it". A
  folder chosen is kept with the dialog's other choices until Skwir
  quits (printing in between keeps it); the first choice taken again is
  no folder kept. `print.save_pdf`'s `dest` must be a whole path. The
  window's open choice (`crates/ui/src/form.rs`, `shown_of`) listed only
  the choices matching the text it held, so a field opening on a value
  offered that value alone: it now lists every choice while its text is
  the value given or chosen, and narrows once something else is typed.
- **Enter takes what was typed** (the user, the same day: `~/` typed
  saved in the folder first offered). Enter in an open choice chose its
  list's highlighted row, and the highlight was always on one, so `~/`,
  which every folder offered matches, became the first of them. While
  the list is of what was typed (`FormState::typed`) no row is
  highlighted: Enter answers the form with the text; Down goes into the
  list at its first row, Up at its last, and Enter then chooses; a click
  chooses.
- **Name, for one PDF** (the user, the same day): a text field after
  Save in, one for each layout (as shown and a listing `name`, a
  booklet's `name_booklet`, a contact sheet's `name_sheet`), each shown
  with its layout and opening on what it names it (`dialog::Named`:
  `notes.pdf`, `notes (booklet).pdf`, `Contact sheet.pdf`, a listing's),
  so the name follows the layout chosen; of several files only as one
  PDF. Never kept: a name is a document's own, as its pages are. Typed,
  `.pdf` is added where it has no such ending (`pdf_named`); one with a
  separator is refused (the folder is Save in's). `print.save_pdf` takes
  `name`, refused for several files saved a PDF each. A name taken in
  the folder is kept beside, numbered, as before.
- **Rejected**: a folder chooser of the system's (a portal's), for a
  file manager that shows folders itself; a setting for PDFs' folder,
  the dialog's choice being kept as its others are. Left: a folder typed
  in Save in is not completed as the address field completes one
  (ADR-070), which no form's field does yet.

## Revision (2026-11-06, later): Save as, folder and name in one line

The user asked whether Save in and Name could be one control, with the
UX ideas proposed (folders completed as typed, Show after saving, a
name already there said, the name's stem selected, the folders chosen
before).

- **One field, `Kind::Save`** (core's forms): a file to write, a whole
  path written from `~`, which the window draws as one line
  (`crates/ui/src/form/save.rs`). It replaces Save in and the Name
  fields (`save_as`, key `save_as`). Its value is the folder alone,
  ending with its separator; the window adds the name the first of its
  `names` gives (`Named`, each with `when` as a field's), so the name
  follows the layout and Together until one is typed. A name with no
  folder saves where the PDF goes by itself (`beside` says it); none of
  `names` holding (several files saved a PDF each), it holds a folder
  alone (`alone`).
- **Completed as the address field completes a path** (ADR-070,
  `complete.rs`): once its text is edited, its list holds the folders
  within the folder typed that its name begins (`.pdf` aside), the
  folder read once on a worker in its lane; untouched or unfolded by its
  button, the folders offered (where it goes by itself, the folders
  chosen before, the other pane's, Documents). Choosing one, Tab
  (`ui.form_complete`, while `Form completes`), Down and Enter or a
  click, puts it in place of the folder typed and keeps the name: the
  one typed, or, where what was typed began that folder's name, the one
  put there before. The name but `.pdf` is selected whenever the window
  puts one.
- **Said under it**, from the folder read: a folder that is none, or a
  name it holds already ("saving asks whether to replace it": the
  clash's question, `op::clash`, as before); else where a name alone
  goes, or that a folder alone is held; else its help.
- **Read by the plugin** (`dialog::target`): the folder part (a whole
  path or from `~`; `~` alone is home), the name (`.pdf` added; its own
  left as `None`); asked again when the folder typed is none; a name
  that is a folder's there is that folder, the PDF named by itself
  (`into_folder`). The folders chosen are kept, the last four, until
  Skwir quits (`Remembered::recent`); the last opens the next Save as.
- **Show** (`toast::made_alone`, `show_action`): the notice of a plan
  that made one thing new in a folder no pane shows offers Show before
  Undo, `navigate` to its folder with it selected. The step's words name
  the folder ("saved … as “x.pdf” in “Work”").
- **Left**: Alt+Up to the parent folder in the field; completing a
  name shared by several folders (Tab takes the first); a folder chosen
  before that is gone is still offered until Skwir quits.

## Revision (2026-11-06, evening): Save as's list a popover, its answer held back

The user asked for the suggestions as a scrollable popover closing when
the field loses focus, the name given back when only a folder is
typed, the Save button disabled while the path is invalid, and more
usability.

- **A popover** (`form/save.rs::popover`): deferred over the form's
  panel (priority 4, the form's modal 3), under the line, as wide, at
  most 300 px at 100 % and scrolling, the row the arrows reach kept in
  view (`FormState::list_scroll`); up to 64 folders completing what was
  typed (`complete::rank_at_most`), one line each where they all are in
  the folder typed. Folded when the field loses the keyboard (`on_blur`)
  but to its own button, and by Escape.
- **What it lists**: what completes the text once edited, nothing when
  nothing does (no list while a name is typed); the asker's folders only
  when asked for, by its button or Down while it is folded
  (`FormState::offered`), the keyboard then left in the text.
- **The name given back** when the field is left holding a folder alone
  (`~/Documents/`) or a folder's own name without its separator
  (`save_left`); the answer added it already.
- **Held back** (`Note::blocks`, `FormState::blocked`): a folder that is
  none, a relative one, or a name the system refuses (`valid_file_name`)
  draws the answer's button inert, its reason its description, and
  Enter does nothing; a name already there is only said. While the
  folder is read, nothing is held back: the plugin checks again.
- **Keys**: Tab as the address field's (a row highlighted taken, else
  the start shared filled in, else the one there is, else the first
  highlighted); Alt+Up the folder above, the name kept, repeating as a
  step (`ui.form_up`), as Alt+Up goes up in the files; a folder put
  there by the window an edit Ctrl+Z takes back (`replace_all`), which
  the field does not take for typing (`put`).
- **The folders chosen before** offered, and the one Save as opens on,
  only while still folders (`dialog::kept_folders`, looked at as the
  dialog opens, within 400 ms each; a slow server's kept).
- **Rejected**: making a folder typed that is none, which holds Save
  back instead (a plan making folders is another question); a folder
  picker of its own.

## Revision (2026-11-06, night): places offered, folders kept, focus

The user chose the sidebar's places among the folders offered and the
folders chosen kept across runs, and asked that lists fold on a click
outside and whether choosing PDF should give Save as the keyboard.

- **Offered, under headings** (`Choice::section`, drawn in the
  popover): where it goes by itself and the other pane's folder first;
  "Chosen before"; "Places", the sidebar's places and bookmarks that are
  folders open now (`registry.places`, groups `places` and `bookmarks`,
  none `unavailable` or to `activate`), by their titles, in the
  sidebar's order (`sidebar_folders`), Documents where it lists none;
  each once, each said by its path. Devices are left out: a volume's
  root is seldom where a PDF goes, and a folder on it is typed.
- **The folders chosen are kept in the cache** (`print` namespace,
  `save-folders`, JSON of paths, `dialog::keep_chosen`, written on the
  dialog's worker one change at a time), the last four, the last first,
  looked at as the dialog opens; never a setting, never logged. The
  folder Save as opens on stays the session's (`Remembered::dest`):
  after a restart it opens where the PDF goes by itself, as the
  dialog's other choices do. A cache cleared forgets them.
- **A press elsewhere in the form folds its list** (`form_fold`), the
  open field's row and its popover keeping their presses.
- **Focus**: a choice clicked that shows a field to type in gives it the
  keyboard (`form_row_click`): PDF clicked puts it in Save as, its name
  selected, so typing renames; Pages… in its pages. With the keyboard on
  the row, the arrows change the choice and the keyboard stays there:
  moving it would make Left, back to Printer, move the caret instead.
  The dialog opening on PDF keeps the keyboard on its first field.

