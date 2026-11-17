# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# Under Save as, while it holds a folder alone: several files saved a PDF each.
print_alone = Each PDF named after its file, in this folder

# What a step found done already, said once its plan ran: the PDF of that name in that folder is this one already. Begins in lower case.
print_already_saved = { $what ->
        [file] { $name } was already saved as { $pdf } in { $folder }
        [files] { $name } and { $more } more were already saved as { $pdf } in { $folder }
        [sheet] { $count ->
            [one] a contact sheet of { $count } file was already saved as { $pdf } in { $folder }
           *[other] a contact sheet of { $count } files was already saved as { $pdf } in { $folder }
        }
       *[listing] the listing of { $name } was already saved as { $pdf } in { $folder }
    }

print_already_there = { $name } is already there

# A card of the Layout field: each page as the preview shows it.
print_as_shown = As shown

print_as_shown_detail = Each page on a sheet

# Under the folder offered where a PDF is saved, and under Save as: the folder of the file printed, or of the files.
print_beside = { $count ->
        [1] Beside it
       *[other] Beside them
    }

# Under Save as, where files of several folders are saved.
print_beside_each = Beside each file; one PDF of them all beside the first

# A card of the Layout field: pages laid out to be folded.
print_booklet = Booklet

print_booklet_detail = Pages on both sides, folded

# PDFium is the name of the library that reads and writes PDFs, as written.
print_booklet_needs_pdfium = Folding a booklet needs PDFium, which is not there

# Said after a booklet was sent to a printer printing on one side.
print_booklet_one_side = A booklet is printed on both sides: on one side, print the odd sides, turn the sheets over, then the even

print_cancelled = Printing was cancelled

# A PDF being saved found its name taken, asked what becomes of it, and the user cancelled it.
print_cancelled_save = Cancelled: nothing was saved as PDF

# A heading of the folders Save as offers: those chosen for earlier PDFs.
print_chosen_before = Chosen before

# Why a print or a PDF was not made. Skwir is the program's name.
print_closing = Skwir is closing

# A column's header in a printed listing of the trash: when each file was put there.
print_column_deleted = Deleted

# A column's header in a printed listing: the folder each file is in.
print_column_folder = Folder

# A column's header in a printed listing: what each file is (PDF document, Folder…).
print_column_kind = Kind

# A column's header in a printed listing: when each file was last changed.
print_column_modified = Modified

# A column's header in a printed listing.
print_column_name = Name

# A column's header in a printed listing.
print_column_size = Size

# A card of the Layout field: the pictures among the files, small, several to a page.
print_contact_sheet = Contact sheet

print_contact_sheet_detail = The pictures, several to a page

# The title of a contact sheet's document, in its header and the system's print dialog.
print_contact_sheet_title = Contact sheet

# Why a PDF is not saved: the folder (a camera's) cannot take a file made aside and moved in.
print_copies_only = { $name } takes only copies: no PDF is made there

print_count_needs_reader = Counting a PDF's pages needs a document reader, which is not there

# The dialog's button, a printer chosen: the system's dialog asks next.
print_dialog_accept = Print…

print_eight_sheets = 8 sheets to a fold

print_every_sheet = Every sheet in one fold

# The title of a listing printed: the folder's path or the place's name, then what filters it, as typed.
print_filtered = { $named }, filtered by { $filter }

# Said once a booklet of eight pages a side is printed or saved: how to fold it.
print_fold_three_times = Fold each sheet in half three times, page 1 outside and upright, cut its closed folds open, and put the sheets one after another

# Said once a booklet of four pages a side is printed or saved: how to fold it.
print_fold_twice = Fold each sheet in half twice, page 1 outside and upright, cut its top fold open, and put the sheets one after another

# A field of the Print dialog: a booklet's sheets folded all together, or in sets.
print_folded_as = Folded as

print_folded_once = Folded once

print_folded_three_times = Folded three times, cut

print_folded_twice = Folded twice, its top cut

# Under a folder offered where a listing's PDF is saved: the folder whose listing it is.
print_folder_listed = The folder listed

# Under a folder offered where a listing's PDF is saved: the folder a search or a tag is of.
print_folder_of = The folder it is of

# The words a font is printed in, at several sizes: a sentence using every letter of the language, then the digits.
print_font_sample = The quick brown fox jumps over the lazy dog 0123456789

print_four_sheets = 4 sheets to a fold

# Said once a print was handed to the desktop's printing, which did not say it took it: how many pages (0 when not counted), what (a file, several, a contact sheet, a listing), and where (a file, a printer by its name, or the printer).
print_handed = Handed { $pages ->
        [0] {""}
        [one] { $pages } page of{" "}
       *[other] { $pages } pages of{" "}
    }{ $what ->
        [file] { $name }
        [files] { $count ->
            [one] { $count } file
           *[other] { $count } files
        }
        [sheet] a contact sheet
       *[listing] the listing of { $name }
    } to the desktop's printing for { $to ->
        [file] { $target }
        [printer] { $printer }
       *[system] the printer
    }, which did not confirm it took it

# Under Save as, where a name typed alone goes: this folder.
print_in_folder = In { $folder }

# Under Save as, where a name typed alone goes.
print_in_folder_listed = In the folder listed

# An error an agent or a script reads: the file named is a root.
print_in_no_folder = The file is in no folder

# The title of a print of several files, in the system's print dialog and the queue: the first's name, then how many more.
print_job_files = { $first } and { $more } more

# PDFium is the name of the library that reads and writes PDFs, as written.
print_joined_needs_pdfium = Printing several files as one needs PDFium, which is not there

# What could not be done, before why: a copy of a file's PDF made to print it.
print_keeping = Keeping { $name } to print it

# What could not be done, before why: a copy of the PDF a document's reader made, to print it.
print_keeping_document = Keeping the document's PDF to print it

# In a printed listing's Kind column: what a folder is.
print_kind_folder = Folder

# In a printed listing's Kind column: a place listed (a tag) that says no kind of its own.
print_kind_place = Place

# A field of the Print dialog: how the pages are laid out.
print_layout = Layout

# Said once several files were printed as one: this one could not be, and why.
print_left_out = { $name } was left out: { $error }

# An error an agent or a script reads: `together` is the command's argument, as written.
print_name_of_one = A name is a PDF's: these files make one each (`together` makes one of them all)

# Under the folder offered where a listing's PDF is saved, for a place of no folder (the trash): the user's Documents.
print_no_folder_of_its_own = It has no folder of its own

# Said in the dialog: a folder typed from ~ where the user has no home folder.
print_no_home = Save as: there is no home folder for “~”

# An error: the place listed has no folder, and the user has no Documents nor home folder.
print_no_listing_folder = There is no folder to save the listing in: choose one

# Why a listing's PDF was not saved: the pane whose listing it is shows another place now.
print_no_longer_shown = { $name } is no longer shown, so its listing is not known

# Why a print or a PDF was not made: the pages asked are all past the document's last.
print_no_page_asked = No page of it is among those asked

# Why nothing was printed: the pages chosen in the system's print dialog are all past the document's last.
print_no_page_dialog = No page of it is among those the print dialog asked

# An error an agent or a script reads: it asked the page shown, and the preview shows none.
print_no_page_shown = No page of it is shown

# An error an agent or a script reads: the paper it gave, and those the command takes, as written.
print_no_paper = “{ $paper }” is no paper: a4, letter, a5, a3 or legal

# Why a contact sheet was not made: none of the files chosen is a picture.
print_no_picture = None of them is a picture

# Said in the dialog: the folder typed in the Save as field is none. The path is as typed, from ~ where it is the home's.
print_no_such_folder = Save as: there is no folder “{ $path }”

# An error: the pane whose listing is printed is not there any more.
print_no_such_pane = No such pane

# Said in the dialog: the folder typed, as typed, is neither from the top nor from the home folder (~).
print_no_whole_path = Save as: “{ $typed }” is no whole path: type it from the top, or from ~

print_none_printable = None of them can be printed

# Why a PDF is not saved: the folder listed, or the one it goes in, is no folder.
print_not_a_folder = { $name } is not a folder

# Why a PDF made by a helper program was refused: it is a link or a pipe, not a file.
print_not_own_file = The PDF made is not a file of its own

# Said once a contact sheet was printed or saved: the file left out of it, or how many.
print_not_pictures = { $count ->
        [1] { $name } is not a picture and was left out
       *[other] { $count } files are not pictures and were left out
    }

# A PDF made could not be put in place; why follows.
print_not_placed = { $name } could not be placed in { $folder }

# Said when a print was cancelled or failed (why follows): a file by its name, several by how many, a contact sheet, or the listing of a place by its name.
print_not_printed = { $what ->
        [file] { $name } was not printed
        [files] { $count ->
            [one] { $count } file was not printed
           *[other] { $count } files were not printed
        }
        [sheet] A contact sheet was not printed
       *[listing] The listing of { $name } was not printed
    }

print_not_smaller = The PDF could not be made smaller

print_not_there = { $name } is not there

# Why a file was not printed: its preview shows nothing to print (a song, a program).
print_nothing_printable = { $name } shows nothing that can be printed

# Under the title of the question asking what becomes of the name a PDF being saved finds taken.
print_nothing_saved = Nothing is saved as PDF until you choose.

# An error an agent or a script reads: it asked what is selected in the preview, and nothing is.
print_nothing_selected = Nothing of it is selected

print_nothing_there = There is nothing to print

print_nothing_to_print = Nothing to print

# Said once PDFs were saved: what was left out of this one.
print_notice_of = { $name }: { $notice }

# Said once several files were printed as one: what was left out of this one.
print_notice_of_file = { $name }: { $notice }

# A card of the Folded as field: every sheet folded together.
print_one_booklet = One booklet

# Under a folder Save as offers: the one the split's other pane shows.
print_other_pane = The other pane

# What a page drawn as a picture is in a PDF made, for a screen reader: its number.
print_page_alt = Page { $page }

# Under the Print dialog's title: how many pages the document has.
print_page_count = { $count ->
        [one] { $count } page
       *[other] { $count } pages
    }

# A field of the Print dialog: which pages are printed.
print_pages = Pages

# A choice of the Pages field: every page.
print_pages_all = All

# A choice of the Pages field: the page the preview shows, by its number.
print_pages_current = Page { $page }

# PDFium is the name of the library that reads and writes PDFs, as written.
print_pages_need_pdfium = Printing its pages needs PDFium, which is not there

# A choice of the Pages field: the pages typed in the field it shows.
print_pages_ranges = Pages…

# Said in the dialog: why the pages typed are none, as the reader of pages explains.
print_pages_refused = Pages to print: { $why }

# A choice of the Pages field: what is selected in the preview.
print_pages_selection = Selection

# A field of the Print dialog: the pages typed, from 1.
print_pages_to_print = Pages to print

# A field of the Print dialog, a PDF chosen: the paper it is laid out on (A4, Letter…).
print_paper = Paper

# Said once printed or saved: pages were asked past the document's last.
print_past = { $count ->
        [one] It has { $count } page; those past it were left out
       *[other] It has { $count } pages; those past them were left out
    }

print_pdf_each = A PDF each

print_pdf_one = One PDF of them all

# A field of the Print dialog, several files saved as PDF: one each, or one of them all.
print_pdfs = PDFs

# An error an agent or a script reads: the pages to a side of a booklet's sheet it gave.
print_per_side = { $count ->
        [one] { $count } page a side: a booklet has 2, 4 or 8
       *[other] { $count } pages a side: a booklet has 2, 4 or 8
    }

# A card of the Pages a side field: this many pages on each side of a sheet.
print_per_side_choice = { $count } a side

# A field of the Print dialog: how many pages a booklet puts on each side of a sheet.
print_per_side_field = Pages a side

# In a printed listing's Size column: how many files a place listed (a tag) holds.
print_place_files = { $count ->
        [one] { $count } file
       *[other] { $count } files
    }

# A heading of the folders Save as offers: the sidebar's places and bookmarks.
print_places = Places

print_plugin_description = Print what a preview shows, as it is, as a booklet or as a contact sheet, or save it as a PDF

# Why a step of a plan made before was refused: the Printing plugin was turned off since.
print_plugin_off = Saving as PDF needs the print plugin on

print_plugin_title = Printing

# A menu item and the action's name: asks how to print, then the system's dialog chooses the printer.
print_print = Print…

# Read by agents and in F1's list of actions: the words in backticks are the command's arguments, as written.
print_print_description = Print files as their previews show them, one job in their order (`layout`: as shown, a booklet folded in signatures of `signature` pages with `trim`, a contact sheet of the pictures, a pane's listing), the `pages` chosen; the window asks first in Skwir's dialog without `asked`, then the system's dialog chooses the printer, the paper and the sides

# A menu item, with nothing selected: prints what the pane lists, as a table.
print_print_listing = Print Listing…

# In the empty field of the pages to print: what may be typed, as written.
print_ranges_placeholder = From 1: 1-3, 7, 10- (to the end)

# The dialog's button, a PDF chosen: saves it.
print_save = Save

# A field of the Print dialog, a PDF chosen: its folder and name, in one line.
print_save_as = Save as

# Under the Save as field: a folder typed from the home folder (~) or from the top, then the PDF's name. Tab is the key.
print_save_as_help = A folder from ~ or the top, then the name: Tab completes a folder

# Said in the dialog: why the name typed in the Save as field is no file's.
print_save_as_refused = Save as: { $why }

# A plan's title: several files saved as a PDF each, or a booklet's each. Begins in lower case.
print_save_each = { $count ->
        [one] { $form ->
            [booklet] save { $count } file as booklets
           *[pdf] save { $count } file as PDFs
        }
       *[other] { $form ->
            [booklet] save { $count } files as booklets
           *[pdf] save { $count } files as PDFs
        }
    }

# A plan's title: what a folder or a place lists saved as a PDF table. Begins in lower case.
print_save_listing = save the listing of { $name } as PDF

# A plan's title, in its review, Activity and the History: a file saved as a PDF, or as a booklet's PDF. Begins in lower case: said inside other words too.
print_save_one = { $form ->
        [booklet] save { $name } as booklet
       *[pdf] save { $name } as PDF
    }

# The action's name, in F1's list of actions.
print_save_pdf = Save as PDF

# Read by agents and in F1's list of actions: the words in backticks are the command's arguments, as written.
print_save_pdf_description = Make a PDF of each file as its preview shows it, beside it or in `dest` (one of them all with `together`), on the paper of the system's locale, laid out and of the pages Print… takes (plan)

# A plan's title: a contact sheet of the pictures among these files saved as a PDF. Begins in lower case.
print_save_sheet = { $count ->
        [one] save a contact sheet of { $count } file as PDF
       *[other] save a contact sheet of { $count } files as PDF
    }

# A step of a plan, in its review: what is saved (a file; a file and how many more; a contact sheet of how many files; a place's listing), the PDF's name and its folder. Begins in lower case.
print_save_step = { $what ->
        [file] save { $name } as { $pdf } in { $folder }
        [files] save { $name } and { $more } more as { $pdf } in { $folder }
        [sheet] { $count ->
            [one] save a contact sheet of { $count } file as { $pdf } in { $folder }
           *[other] save a contact sheet of { $count } files as { $pdf } in { $folder }
        }
       *[listing] save the listing of { $name } as { $pdf } in { $folder }
    }

# A plan's title: several files saved as one PDF, or one booklet's. Begins in lower case.
print_save_together = { $count ->
        [one] { $form ->
            [booklet] save { $count } file as one booklet
           *[pdf] save { $count } file as one PDF
        }
       *[other] { $form ->
            [booklet] save { $count } files as one booklet
           *[pdf] save { $count } files as one PDF
        }
    }

# The button of the question asking what becomes of the name a PDF being saved finds taken: save it, once that is answered.
print_save_verb = Save as PDF

# What a step did, said once its plan ran. Begins in lower case.
print_saved_step = { $what ->
        [file] saved { $name } as { $pdf } in { $folder }
        [files] saved { $name } and { $more } more as { $pdf } in { $folder }
        [sheet] { $count ->
            [one] saved a contact sheet of { $count } file as { $pdf } in { $folder }
           *[other] saved a contact sheet of { $count } files as { $pdf } in { $folder }
        }
       *[listing] saved the listing of { $name } as { $pdf } in { $folder }
    }

# Said once a print was sent: how many pages (0 when not counted), what (a file, several, a contact sheet, a listing), and where (a file, a printer by its name, or the printer).
print_sent = Sent { $pages ->
        [0] {""}
        [one] { $pages } page of{" "}
       *[other] { $pages } pages of{" "}
    }{ $what ->
        [file] { $name }
        [files] { $count ->
            [one] { $count } file
           *[other] { $count } files
        }
        [sheet] a contact sheet
       *[listing] the listing of { $name }
    } { $to ->
        [file] to { $target }
        [printer] to { $printer }
       *[system] to the printer
    }

print_sets_of_16 = Sets of 16 pages

print_sets_of_32 = Sets of 32 pages

# Under the One booklet card: how many sheets its one fold holds.
print_sheets_in_fold = { $count ->
        [one] { $count } sheet, one fold
       *[other] { $count } sheets in one fold
    }

# An error: the pane whose listing is printed shows no folder or place.
print_shows_nothing = The pane shows nothing

# A switch of the Print dialog, a PDF chosen: its pictures made smaller.
print_smaller = Smaller file

# What could not be made, before why: a temporary folder.
print_smaller_folder = A folder to make the PDF smaller in

# Under the Smaller file switch. Reduce Size is a command's name and Screen its card's, as their own words say them.
print_smaller_help = Pictures in it at most 150 dpi, as Reduce Size's Screen

# Two sentences said one after the other in a notice; the first has no full stop of its own.
print_then = { $said }. { $then }

# The title of the Print dialog, and of a print under way in Activity: a file by its name, several by how many, a contact sheet, or the listing of a folder or a place by its name.
print_title = { $what ->
        [file] Print { $name }
        [files] { $count ->
            [one] Print { $count } file
           *[other] Print { $count } files
        }
        [sheet] Print a contact sheet
       *[listing] Print the listing of { $name }
    }

# A field of the Print dialog: where what is printed goes, a printer or a PDF.
print_to = To

# A choice of the To field: saved as a PDF file.
print_to_pdf = PDF

# A choice of the To field.
print_to_printer = Printer

# A switch of the Print dialog, for a booklet.
print_trim = Trim white margins

# Under the switch trimming a booklet's white margins: every page scaled alike, to fit what is printed on it.
print_trim_help = Pages fit by their ink, all alike

print_went_missing = { $name } went missing as it was made

# What could not be made, before why: a temporary folder the PDF is made in.
print_work_folder = A folder to make the document in

print_wrong_size = a picture of the wrong size
