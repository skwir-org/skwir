# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

shrink_already = { $copy } was already a smaller copy of { $file }

shrink_already_there = { $name } is already there

shrink_as_small = { $name } is as small as it gets already: no copy was made

# A field of the dialog: the size each copy fits in.
shrink_at_most = At most

# In the empty At most field. The sizes are examples of what may be typed, with their units as written.
shrink_at_most_placeholder = No limit, or a size: 2 MB, 500 KB

# Said in the dialog: why what was typed in the At most field is no size.
shrink_at_most_refused = At most: { $why }

# On a card, where pictures and PDFs are chosen: a PDF's images brought down to this many dots per inch, pictures to this many pixels across.
shrink_both = { $pixels ->
        [one] Images at most { $dpi } dpi; pictures { $pixels } pixel
       *[other] Images at most { $dpi } dpi; pictures { $pixels } pixels
    }

shrink_cancelled = Making the copy was cancelled

# A smaller copy found its name taken, asked what becomes of it, and the user cancelled it.
shrink_clash_cancelled = Cancelled: nothing was made smaller

# Why a smaller copy was not made. Skwir is the program's name.
shrink_closing = Skwir is closing

# The dialog's button: makes the smaller copy, or copies.
shrink_dialog_accept = { $count ->
        [1] Make Copy
       *[other] Make Copies
    }

shrink_dialog_intro = { $count ->
        [1] A smaller copy is made beside it; the file stays as it is.
       *[other] A smaller copy is made beside each file; the files stay as they are.
    }

# The title of the dialog asking how small copies of pictures and PDFs are made.
shrink_dialog_title = { $count ->
        [1] Reduce Size of { $name }
       *[other] Reduce Size of { $count } files
    }

# A switch of the dialog: copies made in shades of gray.
shrink_gray = In gray

shrink_gray_help = In shades of gray

shrink_gray_help_pdfs = Pictures, and a PDF's images, in shades of gray

# Why a smaller copy is not made: the folder (a camera's) cannot take a file made aside and moved in.
shrink_in_place_only = { $name } takes only copies written in place: no smaller copy is made there

# A card of the dialog: nothing re-encoded, what is stored twice kept once.
shrink_lossless = Lossless

# An error an agent or a script reads: `at_most` is the command's argument, and lossless, print, screen and small the values its `quality` takes, as written.
shrink_lossless_at_most = The `lossless` quality re-encodes nothing, so it cannot keep a copy under a size: give `at_most` with the `print`, `screen` or `small` quality

# Said in the dialog; Print, Screen and Small are its cards' titles, and the last words its switch's.
shrink_lossless_jpeg = Lossless leaves a JPEG as it is: choose Print, Screen or Small, or remove its location and camera details

# What a step did, said once its plan ran. Begins in lower case.
shrink_made = made { $copy }, a smaller copy of { $file }

# A step of a plan, in its review: the copy's name, then its file's. Begins in lower case.
shrink_make = make { $copy }, a smaller copy of { $file }

# The button of the question asking what becomes of the name a smaller copy finds taken: make it, once that is answered.
shrink_make_verb = Make Smaller Copy

# An error an agent or a script reads: the file named is a root.
shrink_no_folder = The file is in no folder

# What was typed for a size, as typed.
shrink_no_size = { $typed } is no size

# The unit typed after a size, as typed; KB and MB are units as written.
shrink_no_unit = { $unit } is no unit: write KB or MB

# What follows the file's name is why (it is protected by a password).
shrink_none_made_note = No smaller copy of { $name } was made: { $said }

shrink_none_made_strip = No smaller copy of { $name } was made, so its location and camera details were not removed either

# Why a smaller copy is not made: where it goes is no folder.
shrink_not_a_folder = { $name } is not a folder

# JPEG, PNG and PDF are the formats' names, as written.
shrink_not_a_picture = { $name } is neither a JPEG or PNG picture nor a PDF

# A smaller copy could not be put in place; why follows.
shrink_not_placed = { $name } could not be placed in { $folder }

# Said once a plan made smaller copies: the copy of this file is kept, larger than asked.
shrink_not_under = { $name } could not be brought under the size asked: its smallest copy is { $size }

# What was left as it was in a file's smaller copy, and why, after the file's name.
shrink_note = { $name }: { $said }

shrink_nothing = Nothing to make smaller

# Under the title of the question asking what becomes of the name a smaller copy finds taken.
shrink_nothing_made = Nothing is made smaller until you choose.

# On the Lossless card: no picture or image is written again at a loss.
shrink_nothing_re_encoded = Nothing re-encoded

# On a card: a PDF's images brought down to this many dots per inch.
shrink_pdf_dpi = Images at most { $dpi } dpi

# On a card: pictures brought down to this many pixels on their longest side.
shrink_picture_pixels = { $pixels ->
        [one] At most { $pixels } pixel across
       *[other] At most { $pixels } pixels across
    }

# A plan's title, in its review, Activity and the History: the file, or how many. Begins in lower case: said inside other words too.
shrink_plan = { $count ->
        [1] make a smaller copy of { $name }
       *[other] make smaller copies of { $count } files
    }

shrink_plugin_description = Make smaller copies of pictures and PDFs beside them: Reduce Size…, in Compress and in a preview's menu

# Why a step of a plan made before was refused: the Smaller copies plugin was turned off since.
shrink_plugin_off = Smaller copies need the plugin on

shrink_plugin_title = Smaller copies

# A card of the dialog: copies good enough to print.
shrink_print = Print

# A field of the dialog: four cards, from Lossless to Small.
shrink_quality = Quality

shrink_reduce = Make Smaller Copies

shrink_reduce_ask_description = Ask how small, then make a smaller copy of each picture and PDF beside it, as `shrink.reduce` does

# Read by agents and in F1's list of actions: the words in backticks are the command's arguments, and lossless, print, screen and small the values `quality` takes, as written.
shrink_reduce_description = Make a smaller copy of each picture (JPEG, PNG) and PDF beside it, or in `dest`, named `name (reduced).ext`: `quality` lossless, print (pictures 3840 pixels across, a PDF's images 300 dpi), screen (1920, 150 dpi) or small (1280, 96 dpi); `gray`; `strip`, a picture's location and camera details left out; `at_most` a size each copy fits, smaller steps tried. A copy that would not be smaller is not made (plan)

# A menu item, in the Compress submenu and the preview's menu: asks how small, then makes smaller copies of pictures and PDFs.
shrink_reduce_size = Reduce Size…

# What stops working without the print helper program, on the Plugins page.
shrink_requirement = Making smaller copies

# On the Lossless card, where pictures are chosen.
shrink_same_pixels = Same pixels, nothing lost

# A card of the dialog: copies good enough for a screen.
shrink_screen = Screen

# A card of the dialog: the smallest copies.
shrink_small = Small

# A switch of the dialog: what a picture says of where and with what it was taken is left out of its copy.
shrink_strip = Remove location and camera details

# Under the switch removing a picture's location and camera details.
shrink_strip_help = The date it was taken stays

shrink_went_missing = { $name } went missing as it was made

# What could not be made, before why: a folder in Skwir's cache.
shrink_work_folder = A folder to make the copy in
