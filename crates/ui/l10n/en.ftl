# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# The button closing About.
about_close = Close

# Said once Copy details in About put them on the clipboard.
about_copied = Copied Skwir’s version and what it runs on

# The button in About putting skwir's version and what it runs on on the clipboard, for a report.
about_copy_details = Copy details

# skwir's version, in a build made for development, slower than a release: version is its number (0.5.1).
about_debug_build = { $version } (debug build)

# A line of what Copy details puts on the clipboard, for a report: label is what it is (System), value what it says.
about_detail = { $label }: { $value }

# In About, the label of the display server skwir draws on.
about_display = Display

# In About, the display server (Wayland), the window's scale (1.5), and the scale the variable forces.
about_display_forced = { $display }, scale { $scale }, GPUI_X11_SCALE_FACTOR={ $forced }

# In About, the display server (Wayland) and the window's scale (1.5).
about_display_is = { $display }, scale { $scale }

# In About, the label of the graphics card and driver that draw the window.
about_graphics = Graphics

# In About, the graphics where no graphics card draws the window: graphics is the device and its driver as they name themselves (llvmpipe · llvmpipe).
about_graphics_software = { $graphics } (drawn by the processor)

# In About, the label of the languages Skwir speaks and how it writes numbers and dates.
about_language = Language

# In About, where the window speaks a translation a person who speaks it has not reviewed yet: the languages Skwir speaks, most wanted first (fr, en), the translation by its name in itself (Français), then the locales it writes numbers and dates in (fr-FR), as written. Say a draft, not who made it.
about_language_draft = { $languages } ({ $draft }: a draft, not fully reviewed yet); numbers { $numbers }, dates { $dates }

# In About, the languages Skwir speaks, most wanted first (fr, en), then the locales it writes numbers and dates in (fr-FR), as written.
about_language_is = { $languages }; numbers { $numbers }, dates { $dates }

# The button in About showing the licences installed with skwir.
about_licences = Licences

# What the About dialog is named to a screen reader.
about_named = About Skwir

# The button in About opening where skwir's source code is.
about_source = Source code

# What skwir is, under its name in About.
about_summary = An augmented file manager, for humans and agents.

# In About, the label of the system skwir runs on.
about_system = System

# In About, the interface's font and size and where they were read, then each part's text size (100%).
about_text = { $font } { $px } px ({ $pt } pt) { $source }; sizes: interface { $interface }, files { $files }, reading { $reading }, code { $code }

# In About, where the text's size was read: desktop is where (GTK, Plasma).
about_text_from = from { $desktop }

# In About, the label of the text's font and sizes.
about_text_label = Text

# In About, where the text's size comes from when no desktop says one.
about_text_own_size = Skwir's own size

# In About, the interface's font and size, where they were read and the desktop's text scaling (1.25), then each part's text size (100%).
about_text_scaled = { $font } { $px } px ({ $pt } pt) { $source }, text scaling { $scaling }; sizes: interface { $interface }, files { $files }, reading { $reading }, code { $code }

# Under skwir's name in About: version is its number (0.5.1), licence its licence's identifier (GPL-3.0-or-later).
about_version = Version { $version } · { $licence }

# The button in About opening skwir's website.
about_website = Website

# How many actions run, beginning the strip's words.
activity_actions = { $count ->
        [one] { $count } action
       *[other] { $count } actions
    }

# What the strip says of an action that joined: its title, capitalised (Copy 3 items to Docs), then how far it is (42%, getting ready…).
activity_announced = { $title } · { $doing }

# What a screen reader hears of an action that joined: its title, capitalised (Copy 3 items to Docs).
activity_announced_heard = { $title } started

# What the strip says of an action that joined and waits its turn: its title, capitalised (Rename to b.txt), then what it waits for (waits for the disk).
activity_announced_waiting = { $what } · { $why }

# What a screen reader hears of an action that joined and waits its turn: its title, capitalised (Rename to b.txt), then what it waits for (waits for the disk).
activity_announced_waiting_heard = { $what } { $why }

# What the strip says of an action that joined and waits its turn, where there is little room: its title, capitalised (Rename to b.txt).
activity_announced_waits_turn = { $what } · waits its turn

# How far what runs is (42%), then how long it has left (about 3 min left).
activity_at_left = { $percent } · { $left }

# A button of an action under way or waiting: cancel it.
activity_cancel = Cancel

# A button over Activity's cards: cancel every action under way or waiting.
activity_cancel_all = Cancel all

# What Cancel all says it cancels: every action under way or waiting.
activity_cancel_all_tip = { $count ->
        [1] Cancel 1 action
       *[other] Cancel all { $count } actions
    }

# A button over the actions waiting their turn: cancel them all.
activity_cancel_waiting = Cancel waiting

# Under the title of an action making its checks, before its first step or byte: how far it is (0 of 5 items).
activity_card_getting_ready = Getting ready, { $amount }

# Under the title of an action its server stopped, going on by itself: how far it is (3 of 5 items).
activity_card_going_on = Going on, { $amount }

# Under the title of an action paused: how far it went (3 of 5 items, 1.1 GB of 4.3 GB).
activity_card_paused = Paused at { $amount }

# Under the title of a copy onto a server's folder: what the server still takes (Sending 3 files to the server), then how far it is (3 of 5 items).
activity_card_sending = { $sending }, { $amount }

# Under the title of an action cancelled, which stops: how far it went (3 of 5 items, 1.1 GB of 4.3 GB).
activity_card_stopping = Stopping at { $amount }

# A button of the strip, beside an action an error stopped: stop saying it.
activity_dismiss = Dismiss

# What a screen reader hears of the list at Activity's foot: what ended this session.
activity_earlier = Earlier

# The line at Activity's foot that unfolds what ended this session: how many it holds.
activity_earlier_count = Earlier ({ $count })

# The chevron of Activity's heading: fold it into the strip in the footer.
activity_fold = Fold into the footer

# The tooltip of the chevron of Activity's heading.
activity_fold_tip = Fold Activity into the footer

# What an action does before its first step or byte: its checks. Said after other words.
activity_getting_ready = getting ready…

# What an action does before its first step or byte: its checks.
activity_getting_ready_opening = Getting ready…

# How what runs goes (42% · about 3 min left), then how many actions wait their turn.
activity_how_waiting = { $how } · { $count ->
        [one] { $count } waiting
       *[other] { $count } waiting
    }

# Where an action goes, as its card says it when pointed at: the paths of the folders it brings something into, joined, the last item perhaps how many others.
activity_into = Into { $folders }

# The last item of where an action goes, as its card says it when pointed at, after the paths of the first folders it brings something into: how many others (Into /a, /b, /c and 2 other folders).
activity_into_more = { $count ->
        [one] { $count } other folder
       *[other] { $count } other folders
    }

# Over Activity's cards: how long what runs has left (about 3 min left), then how many actions wait their turn.
activity_left_waiting = { $left } · { $count ->
        [one] { $count } waiting
       *[other] { $count } waiting
    }

# The name of the panel of what runs, opened above the footer.
activity_name = Activity

# What the strip says while only an action waits its turn: its title, capitalised (Rename to b.txt), then what it waits for (waits for the disk).
activity_next = { $what } · { $why }

# What Activity says, opened with nothing under way and nothing kept in it.
activity_nothing_under_way = Nothing under way. What was done is in the Undo history.

# A button of an action under way: pause it.
activity_pause = Pause

# A button of the strip: pause every action running.
activity_pause_all = Pause all

# What the strip says while every action running is paused, after other words: how far (42%).
activity_paused_at = paused at { $percent }

# What the strip says while every action running is paused: how far (42%).
activity_paused_at_opening = Paused at { $percent }

# How far an action is, in percent (42%).
activity_percent = { $percent }%

# How far the queue of actions started: those ended or running of all of them (2 of 5).
activity_queue = { $at } of { $of }

# How far the queue of actions started (2 of 5), then how what runs goes (42% · about 3 min left).
activity_queue_how = { $queue } · { $how }

# A button of an action paused: let it go on.
activity_resume = Resume

# A button of the strip: let every action paused go on.
activity_resume_all = Resume all

# A button of the strip, beside an action an error stopped: try what failed again.
activity_retry = Retry

# Over Activity's cards: how far the queue of actions started, those ended or running of all of them (2 of 5), and its share (42%).
activity_spell_at = { $at } of { $of } · { $percent }

# What a screen reader hears of the bar over Activity's cards: how far the queue of actions started, those ended or running (at) of all (of): 2 of 5 actions. Each count chooses its words: where the noun follows the first number, choose it by at.
activity_spell_label = { $at ->
        [one] { $at } of { $of ->
            [one] { $of } action
           *[other] { $of } actions
        }
       *[other] { $at } of { $of ->
            [one] { $of } action
           *[other] { $of } actions
        }
    }

# What the strip says of an action an error stopped, where its title does not fit.
activity_stopped = Stopped

# A heading of Activity, over what an error stopped.
activity_stopped_heading = Stopped

# What the strip says of an action an error stopped: its title, capitalised (Copy 3 items to Docs).
activity_stopped_title = { $title }: stopped

# What the strip says while every action running was cancelled and stops, after other words.
activity_stopping = stopping…

# On the button of an action cancelled, while it stops: nothing more to ask of it.
activity_stopping_button = Stopping…

# What the strip says while every action running was cancelled and stops.
activity_stopping_opening = Stopping…

# What a screen reader hears of the strip in the footer, which opens Activity: what it says (Copy 3 items to Docs · 42%).
activity_strip_label = Activity: { $said }

# How many actions wait their turn (3 waiting).
activity_waiting_count = { $count ->
        [one] { $count } waiting
       *[other] { $count } waiting
    }

# A heading of Activity, over the actions waiting their turn.
activity_waiting_heading = Waiting

# What a screen reader hears of Activity's list of the actions waiting their turn.
activity_waiting_list = Waiting

# A heading of Activity, over the actions waiting their turn: how many of them end as they start (a single rename), which the strip does not count.
activity_waiting_not_counted = Waiting · { $count ->
        [one] { $count } quick one
       *[other] { $count } quick ones
    } not counted

# The strip's words: what runs (Copy 3 items to Docs, 2 actions), how it goes (42% · about 3 min left).
activity_what_how = { $what } · { $how }

# The strip's words: what runs (Copy 3 items to Docs, 2 actions), how far the queue started (2 of 5), how it goes (42% · about 3 min left).
activity_what_queue_how = { $what } · { $queue } · { $how }

# A sort's direction, alphabetical.
address_a_to_z = A to Z

# A sort's direction after its criterion, in the View menu's row: Name, A to Z.
address_a_to_z_said = A to Z

address_about = About Skwir

# The tooltip of a chevron of the address's trail: the places above this one; place: its name.
address_above = Above { $place }

# A choice of where a panel goes, in the More menu.
address_at_the_bottom = At the bottom

# The first choice of where a panel goes, in the More menu: chosen by the window's shape.
address_auto = Auto

# The tooltip of the Back button.
address_back_tip = Back · hold or right-click for the history

# At the end of the View menu's Views heading: choose the buttons beside the address.
address_buttons = Buttons…

address_buttons_tip = Choose the buttons beside the address: which views have one, and whether Up stands after Back and Forward

# The tooltip of a row of the grid's Under names: whether tiles say when the file was last read.
address_caption_accessed = Whether each tile says under its name when it was last read

# The tooltip of a row of the grid's Under names: whether tiles say when the file was created.
address_caption_created = Whether each tile says under its name when it was created

# The tooltip of a row of the grid's Under names, in the trash: whether tiles say when the file was deleted.
address_caption_deleted = Whether each tile says under its name when it was deleted

address_caption_dimensions = Whether each tile says its dimensions under its name

address_caption_duration = Whether each tile says its duration under its name

address_caption_extension = Whether each tile says its extension under its name

address_caption_folder = Whether each tile says its folder under its name

address_caption_kind = Whether each tile says its kind under its name

# The tooltip of a row of the grid's Under names: whether tiles say when the file was modified.
address_caption_modified = Whether each tile says under its name when it was modified

address_caption_name = Whether each tile says its name

address_caption_owner = Whether each tile says its owner under its name

address_caption_rating = Whether each tile says its rating under its name

address_caption_size = Whether each tile says its size under its name

address_caption_tags = Whether each tile says its tags under its name

# The tooltip of a row of the grid's Under names: whether tiles say when a picture was taken.
address_caption_taken = Whether each tile says under its name when it was taken

address_caption_type = Whether each tile says its type under its name

# The tooltip of a row of the grid's Under names: whether tiles say the file's state in Git.
address_caption_vcs = Whether each tile says its state in Git under its name

# A sort by the files' state in Git: the files changed first.
address_changes_first = Changes first

# A sort's direction after its criterion, in the View menu's row: Git, Changes first.
address_changes_first_said = changes first

# A sort by the files' state in Git: the files changed last.
address_changes_last = Changes last

# A sort's direction after its criterion, in the View menu's row: Git, Changes last.
address_changes_last_said = changes last

# An item of the View menu, in the tree: close every folder opened in place.
address_collapse_all = Collapse all

address_collapse_all_tip = Close every folder opened in place

# A switch of the View menu: whether the files that keep another file's notes beside it are listed.
address_companions = Companion files

address_companions_tip = Also list the files that keep another file's notes beside it (a Mac's ._ files, sidecars)

# An item of the menu of the buttons beside the address: the buttons as they first were.
address_default_buttons = Default buttons

address_default_buttons_tip = Back to the buttons Skwir comes with

address_default_levels = Default levels

address_default_size = Default size

# A row of the View menu's Sort by and Group by: the two directions (A to Z, Z to A).
address_direction = Direction

# The tooltip of the magnifier at the address's end over a search.
address_edit_search = Edit this search

# A button of the View menu, in Usage: its map draws fewer levels of folders.
address_fewer_levels = Fewer levels (or Ctrl and the wheel)

# The tooltip of a chevron of the address's trail: the folders that hold this one; folder: its name.
address_folders_above = Folders above { $folder }

# A switch of the View menu: folders are listed before files.
address_folders_first = Folders first

address_folders_first_tip = List folders before files, in every folder; off, sort them together

# The tooltip of the Forward button.
address_forward_tip = Forward · hold or right-click for the history

# A switch of the View menu, in the grid: pictures cropped to fill their tiles.
address_grid_fill = Fill thumbnails

address_grid_fill_tip = Crop thumbnails in the grid to fill their box

# The tooltip of a criterion of the View menu's Group by; key: the criterion inside a sentence (name, date modified).
address_group_by = Group by { $key }

# A row of the View menu opening the choices of how the files are grouped beside it.
address_group_by_menu = Group by

# A choice of the View menu's Group by: the files are not grouped.
address_group_none = None

address_group_none_tip = Do not group

# A choice of the View menu's Group by: grouped by what the files are sorted by.
address_group_same = Same as sort

address_group_same_tip = Group by what the files are sorted by, in its direction

# A switch of the View menu: whether hidden files (their names begin with a dot) are listed.
address_hidden = Hidden files

address_hidden_tip = Show hidden files

# A sort's direction after its criterion, in the View menu's row: Rating, highest.
address_highest = highest

address_highest_first = Highest first

# The tooltip of a chevron of the address's trail: the folders and places inside this one; place: its name.
address_in = In { $place }

# A button of the View menu: draw the files larger.
address_larger = Larger (or Ctrl and the wheel)

# A sort's direction after its criterion, in the View menu's row: Size, largest.
address_largest = largest

address_largest_first = Largest first

# A row of the More menu: where a file of an archive is previewed beside its files.
address_layout_member = File in an archive

# A row of the More menu: where the preview goes (Auto, on the right, at the bottom).
address_layout_preview = Preview

# A row of the More menu: where split panes go (Auto, side by side, one above the other).
address_layout_split = Split

# The View menu's row of Usage's map: how many levels of folders it draws.
address_levels = Levels

# A sort's direction after its criterion, in the View menu's row: Duration, longest.
address_longest = longest

address_longest_first = Longest first

# A sort's direction after its criterion, in the View menu's row: Rating, lowest.
address_lowest = lowest

address_lowest_first = Lowest first

address_member_auto = Automatic: under the archive's files while the preview is at the right, at their right while it is at the bottom

# The tooltip of the button of the window's menu (layout, undo, settings, quit).
address_more = More

# A button of the View menu, in Usage: its map draws more levels of folders.
address_more_levels = More levels (or Ctrl and the wheel)

# A heading of the menu of the buttons beside the address: Back, Forward, Up.
address_navigation_buttons = Navigation buttons

address_new_tab = New tab

address_new_window = New window

# A sort's direction after its criterion, in the View menu's row: Modified, newest.
address_newest = newest

address_newest_first = Newest first

# A sort's direction after its criterion, in the View menu's row: Modified, oldest.
address_oldest = oldest

address_oldest_first = Oldest first

# A choice of where a panel goes, in the More menu.
address_on_the_right = On the right

# A choice of where split panes go, in the More menu.
address_one_above = One above the other

address_palette = Command palette…

address_palette_tip = Command palette

address_panes_auto = Automatic: one above the other while the window is taller than wide, else side by side

# What a screen reader calls the address's trail of folders.
address_path = Path

# A button that shows or hides the preview panel.
address_preview = Preview

address_preview_auto = Automatic: at the bottom while the window is taller than wide, else on the right

# A button beside the preview's: show the selected file's properties in the panel.
address_properties = Properties

# A switch of the View menu: a row above the files offering their kinds, years and tags to filter by.
address_quick_filters = Quick filters

address_quick_filters_tip = Offer the kinds, years and tags of this pane's files above them, each with how many files, to show only those until taken off

# The last item of the More menu: end skwir, every window.
address_quit = Quit

address_quit_tip = Quit Skwir

# An item of the More menu, while there is nothing to redo; and its tooltip.
address_redo = Redo

# An item of the More menu; plan: what it does again (new folder Reports).
address_redo_plan = Redo { $plan }

# A switch of the View menu: this folder keeps its order and groups wherever the user's windows show it (in a window answering another program's dialog, wherever such a window shows it).
address_remember_folder = Remember for this folder

address_remember_folder_dialog_tip = Show this folder sorted and grouped as now wherever a window answering another program's dialog shows it, whatever your own windows remember, in this view where the pane shows the list or the grid, apart from the pane's own; off, it takes the pane's

address_remember_folder_tip = Show this folder sorted and grouped as now wherever your windows show it, in this view where the pane shows the list or the grid, apart from the pane's own; off, it takes the pane's

# A switch of the View menu: this place (a tag, a search) keeps its order and groups wherever the user's windows show it (in a window answering another program's dialog, wherever such a window shows it).
address_remember_place = Remember for this place

address_remember_place_dialog_tip = Show this place sorted and grouped as now wherever a window answering another program's dialog shows it, whatever your own windows remember, in this view where the pane shows the list or the grid, apart from the pane's own; off, it takes the pane's

address_remember_place_tip = Show this place sorted and grouped as now wherever your windows show it, in this view where the pane shows the list or the grid, apart from the pane's own; off, it takes the pane's

address_remember_search_tip = A search's order is not remembered: its terms are what it is

# The tooltip of the button at the address's end while what is shown is kept in the sidebar.
address_remove_kept = Remove from sidebar

# The tooltip of the magnifier at the address's end over a folder.
address_search_folder = Search this folder and its subfolders

# The tooltip of the magnifier at the address's end over a place words can search (a tag, Recent); place: its name.
address_search_place = Search { $place }

address_settings = Settings…

address_settings_tip = Settings

address_shortcuts = Shortcuts…

# The tooltip of the More menu's Shortcuts…: the keys that work where one is.
address_shortcuts_tip = Shortcuts here

# A sort's direction after its criterion, in the View menu's row: Duration, shortest.
address_shortest = shortest

address_shortest_first = Shortest first

# A choice of where split panes go, in the More menu.
address_side_by_side = Side by side

# The tooltip of the button that shows or hides the sidebar.
address_sidebar = Sidebar

# The View menu's row of how large the files are drawn.
address_size = Size

# A button of the View menu: draw the files smaller.
address_smaller = Smaller (or Ctrl and the wheel)

# A sort's direction after its criterion, in the View menu's row: Size, smallest.
address_smallest = smallest

address_smallest_first = Smallest first

# The tooltip of a criterion of the View menu's Sort by; key: the criterion inside a sentence (name, date modified).
address_sort_by = Sort by { $key }

# A row of the View menu opening the sort's choices beside it; what it is set to follows (Name, A to Z).
address_sort_by_menu = Sort by

# What the View menu's Sort by and Group by rows are set to; key: the criterion (Name, Modified); direction: its direction (A to Z, newest).
address_sort_said = { $key }, { $direction }

# A button's label: split the pane in two, or close the split.
address_split = Split

# An item of the More menu: open or close the terminal panel.
address_terminal = Terminal

# A switch of the View menu: a strip under the files showing when they changed.
address_timeline = Timeline

address_timeline_tip = Show when this pane's files changed under them, a bar for each day, week, month or year; a bar pressed shows only its files

# A row of the View menu, in the grid: what each tile says under its name (its size, its date).
address_under_names_menu = Under names

# What the View menu's Under names row says when tiles say nothing under their names.
address_under_names_none = None

# What the View menu's Under names row says tiles say under their names, joined (Size and Modified).
address_under_names_shown = { $captions }

# An item of the More menu, while there is nothing to undo; and its tooltip.
address_undo = Undo

# An item of the More menu: the list of what was done, to undo or redo.
address_undo_history = Undo history…

address_undo_history_tip = Undo history

# An item of the More menu: undo what is still running, once it ends; plan: what it does (copy a.txt to Backup).
address_undo_once_ended = Undo { $plan } (once it ends)

# An item of the More menu; plan: what it undoes (rename to final.txt).
address_undo_plan = Undo { $plan }

# An item of the menu of the buttons beside the address: whether the Up button is there.
address_up_button = Up

address_up_button_tip = Whether Up stands after Back and Forward: it goes to the folder that holds this one, as Alt+Up does

# The tooltip of the Up button: the folder that holds this one.
address_up_tip = Up one level

# view: a view's name (List, Grid, Columns).
address_view_button_tip = Whether the { $view } view has a button

# A heading with a switch, in the menu of the buttons beside the address: whether the views have buttons.
address_view_buttons = View buttons

address_view_buttons_tip = Whether the views have buttons beside the address; off, one button, the view shown, opens the View menu

# The tooltip of the button beside the view buttons that opens the View menu.
address_view_menu = Sort, hidden files and zoom

# The tooltip of the button that opens the View menu where the pane's row is narrow; view: the view shown (List, Grid).
address_view_menu_of = { $view } view, sort, hidden files and zoom

# The tooltip of the button that opens the View menu.
address_view_menu_views = Views, sort, hidden files and zoom

# The heading of the View menu's views (list, grid, columns).
address_views = Views

# A sort's direction, alphabetical backwards.
address_z_to_a = Z to A

# A sort's direction after its criterion, in the View menu's row: Name, Z to A.
address_z_to_a_said = Z to A

# The middle button of the View menu's size: back to the size it had.
address_zoom_default = Default

# In the find field of a file's bytes shown in hexadecimal, asked for the match before.
bytes_forward_only = The hex view searches forward only

# Why the bytes of a file cannot be read or searched: what shows them in hexadecimal is turned off.
bytes_no_view = no bytes view

# Said where what was typed to jump to in a file's bytes is no number (1024, or 0x400).
bytes_offset_refused = Enter a decimal or hexadecimal offset

# What a screen reader calls a list to choose from.
choose_choices = Choices

# Said in a list to choose from (Open With's applications) where nothing matches what is typed.
choose_empty = Nothing to choose from

# A command asked to offer a list without one; command: the command's id.
choose_needs = { $command } needs choices or a source

# Said when a plugin asked to offer choices gave none.
choose_nothing = nothing to choose from

choose_placeholder = Type to filter

# What a list to choose from says when the choices a plugin gave cannot be read; the reason follows.
choose_unreadable = unreadable choices

# A button of the bar answering another program's dialog: choose the folder.
chooser_accept_folder = Select

# A button of the bar answering another program's dialog: choose the folders selected; count: how many.
chooser_accept_folders = { $count ->
        [one] Select { $count } folder
       *[other] Select { $count } folders
    }

# A button of the bar answering another program's dialog: open the files chosen.
chooser_accept_open = Open

# A button of the bar answering another program's dialog: open the files selected; count: how many.
chooser_accept_open_files = { $count ->
        [one] Open { $count } file
       *[other] Open { $count } files
    }

# A button of the bar answering another program's dialog: save there.
chooser_accept_save = Save

# A button of the bar answering another program's dialog: save the files it names in the folder shown; count: how many.
chooser_accept_save_files = { $count ->
        [one] Save { $count } file
       *[other] Save { $count } files
    }

# In the filter dropdown of the bar answering another program's dialog: show every file.
chooser_all_files = All files

# A button of the bar answering another program's dialog: choose nothing.
chooser_cancel = Cancel

# Said where the bar answering another program's Open dialog was pressed with no file selected.
chooser_choose_file = Choose a file first

# Said where the bar answering another program's dialog for a folder was pressed with none selected or shown.
chooser_choose_folder = Choose a folder first

# What the filter dropdown of the bar answering another program's dialog is called, for a screen reader.
chooser_filter = Files shown

# Said in a window answering another program's dialog once a folder is taken out of the sidebar's Recent Folders, with Undo beside; folder: its name.
chooser_folder_forgotten = { $folder } taken out of Recent Folders

# Said once the Undo of a folder taken out of the sidebar's Recent Folders puts it back; folder: its name.
chooser_folder_put_back = { $folder } put back in Recent Folders

# Said where another program saves several files in a folder holding a folder of the name of one of them, which no file can replace: name: that folder's name; folder: where it is.
chooser_folder_taken = There is a folder named { $name } in { $folder }: choose another folder to save in

# Said in a window answering another program's dialog once the sidebar's Recent Folders are cleared, with Undo beside.
chooser_folders_cleared = Recent Folders cleared

# Said once the Undo of the sidebar's Recent Folders cleared puts them back.
chooser_folders_put_back = Recent Folders put back

# Before the name field of the bar answering another program's Save dialog.
chooser_name = Name

# In the empty name field of the bar answering another program's Save dialog.
chooser_name_placeholder = Name of the file

# Said where a name typed in another program's Save dialog names a folder that does not exist.
chooser_no_folder = There is no folder { $folder }

# Said where another program's dialog was asked to answer with a file of a place whose files are not where they were (the Trash). place: its name (Trash).
chooser_not_given = Files in { $place } cannot be chosen

# Asked before another program saves over a file: name: the file's name; folder: where it is.
chooser_replace = Replace { $name }? A file of that name is already in { $folder }.

# Asked before another program saves several files over files already there: count: how many are there; folder: where.
chooser_replace_several = { $count ->
        [one] Replace the file of the same name already in { $folder }?
       *[other] Replace the { $count } files of the same names already in { $folder }?
    }

# Said where the bar answering another program's Save dialog was pressed while the pane shows a search or a tag, not a folder.
chooser_save_in_folder = Go to a folder to save in

# The title of the window another program opened to choose a folder, where it gave none.
chooser_title_folder = Choose a folder

# The title of the window another program opened to choose files to open, where it gave none.
chooser_title_open = Open

# The title of the window another program opened to choose where to save, where it gave none.
chooser_title_save = Save

# Said where the bar answering another program's Save dialog was pressed with its name field empty.
chooser_type_name = Type a name to save under

# The button giving up what was asked, rather than answer the names taken.
clashes_cancel = Cancel

# The button copying what was asked, once the names taken are answered.
clashes_copy = Copy

# The row choosing what becomes of every file and folder whose name is taken (but folders onto folders), beside the rows of each.
clashes_files_and_folders_for_all = Files and folders, for all

# The row choosing what becomes of every file whose name is taken, beside the rows of each.
clashes_files_for_all = Files, for all

# The row choosing what becomes of every folder whose name is taken, beside the rows of each.
clashes_folders_for_all = Folders, for all

# The row choosing what becomes of every folder coming where a folder of its name is, beside the rows of each.
clashes_folders_onto_folders_for_all = Folders onto folders, for all

# In the dialog asking what becomes of the names taken, where what is replaced cannot go to the trash: folder is where it goes.
clashes_for_good = { $folder } is in a network folder, which keeps no trash: what is replaced there is deleted for good.

# A choice for a name already taken where something goes: keep what is there and what comes, the one coming under a name of its own.
clashes_keep_both = Keep both

# The keys of the dialog asking what becomes of the names taken, at its foot.
clashes_keys = ↑↓ a row, ←→ its choice · Enter goes on, Escape cancels

# The button making the links asked, once the names taken are answered.
clashes_link = Link

# A choice for a folder coming where a folder of its name is: what it holds goes into the one there.
clashes_merge = Merge

# Beside the title of a question of names taken: how many other questions wait after it.
clashes_more_waiting = { $count ->
        [one] { $count } more waiting
       *[other] { $count } more waiting
    }

# The button moving what was asked, once the names taken are answered.
clashes_move = Move

# What the dialog asking what becomes of the names already taken is named to a screen reader.
clashes_named = Names already taken

# Under the title of the dialog asking what becomes of the names taken where something is copied.
clashes_nothing_copied = Nothing is copied until you choose.

# Under the title of the dialog asking what becomes of the names taken where links are made.
clashes_nothing_linked = Nothing is linked until you choose.

# Under the title of the dialog asking what becomes of the names taken where something is moved.
clashes_nothing_moved = Nothing is moved until you choose.

# A choice for a name already taken where something goes: what comes replaces what is there.
clashes_replace = Replace

# A choice for a name already taken where something goes: what comes replaces what is there only where it changed later.
clashes_replace_if_newer = Replace if newer

# One side of a name taken, a file whose size is unknown, under its name.
clashes_side_file = file

# One side of a name taken, a file whose size is unknown, under its name, and when it changed (Today 14:02).
clashes_side_file_at = file, { $time }

# One side of a name taken, a folder, under its name: what comes, or what is there.
clashes_side_folder = folder

# One side of a name taken, a folder, under its name, and when it changed (Today 14:02).
clashes_side_folder_at = folder, { $time }

# One side of a name taken, a file, under its name: its size (12 KB).
clashes_side_size = { $size }

# One side of a name taken, a file, under its name: its size (12 KB) and when it changed (Today 14:02).
clashes_side_size_at = { $size }, { $time }

# Under a name taken: what comes (yours), then what is there; each a size or folder and when it changed.
clashes_sides = Yours: { $yours } — There: { $theirs }

# Under a name taken: what comes (yours), then what is there, newer; each a size or folder and when it changed.
clashes_sides_theirs_newer = Yours: { $yours } — There: { $theirs } · newer

# Under a name taken: what comes (yours), newer, then what is there; each a size or folder and when it changed.
clashes_sides_yours_newer = Yours: { $yours } · newer — There: { $theirs }

# A choice for a name already taken where something goes: what comes is left where it is.
clashes_skip = Skip

# The title of the dialog asking what becomes of the names already taken where something goes: count is how many, folder where.
clashes_title = { $count ->
        [one] { $count } name already in { $folder }
       *[other] { $count } names already in { $folder }
    }

clipboard_all_gone = The copied items no longer exist

# Said after a paste where the clipboard holds no files.
clipboard_nothing_to_paste = Nothing to paste

# What another application put on the clipboard names files that are not there; count: how many.
clipboard_some_gone = { $count ->
        [one] { $count } copied item no longer exists
       *[other] { $count } copied items no longer exist
    }

# What another application copied is on another protocol, which Skwir does not speak; scheme: the protocol, as its address writes it (sftp, smb).
clipboard_unreachable = { $count ->
        [one] The clipboard holds { $count } { $scheme }: item Skwir cannot reach
       *[other] The clipboard holds { $count } { $scheme }: items Skwir cannot reach
    }

# What another application copied is on another protocol, which Skwir does not speak, one whose address names none.
clipboard_unreachable_remote = { $count ->
        [one] The clipboard holds { $count } remote item Skwir cannot reach
       *[other] The clipboard holds { $count } remote items Skwir cannot reach
    }

# What stands for a folded block of code: how many lines it hides.
code_preview_folded = … { $count ->
        [one] { $count } line
       *[other] { $count } lines
    }

commands_about = About Skwir

commands_about_description = Show Skwir's version and licence, and what it runs on

# A window command's title: answer yes to the question a dialog asks.
commands_accept = Confirm

commands_accept_description = Answer yes to the question asked

# A window command's title: the panel of what runs, waits and ended this session.
commands_activity = Activity

commands_activity_cancel_all = Cancel all actions

commands_activity_cancel_all_description = Cancel every action under way and every one waiting its turn: what each did stays, undoable, and what it left half done is taken back

commands_activity_description = Open Activity above its strip in the footer, with the keyboard, or fold it once it has the keyboard: what runs, with Pause and Cancel, what waits, and what ended this session that the Undo history does not hold (what failed, was cancelled or cannot be undone, and answers that are no action's), with Retry or Undo. It stays open until folded, and opens by itself once an action asked in the window lasts (Open Activity after, 1.5 s by default) (ADR-114)

commands_activity_earlier = Show Earlier in Activity

commands_activity_earlier_description = Unfold Earlier at the foot of Activity, or fold it again: what ended this session that the Undo history does not hold

commands_activity_pause_all = Pause all actions

commands_activity_pause_all_description = Pause every action under way, or resume them all once every one is paused; what waits keeps waiting

# Why an agent's call is refused. command: the command's id (ui.inspect); ui.preview_copy is another's, as written.
commands_agent_copies = for an agent, { $command } copies through ui.preview_copy, which asks the user first

# Why an agent's call is refused. command: the command's id (ui.inspect); `link` is the argument naming a link's target, as written.
commands_agent_follows_label = for an agent, { $command } follows a link by its target (`link`), not by a label typed

# Why an agent's call is refused. command: the command's id (ui.inspect); ui.open_url is another's, as written.
commands_agent_opens_address = for an agent, { $command } opens an address through ui.open_url, which asks the user first

commands_ask = Ask for a line of text

# Read by agents and in F1's list of actions.
commands_ask_description = Ask the user for a line of text and run a command with it

commands_choose = Ask for a choice

commands_choose_description = Ask the user to choose among choices, given or listed by a command, and run the one chosen

commands_choose_option = Switch the option

commands_choose_option_description = Switch the choice's option (Always use for…)

commands_chooser_accept = Answer the dialog

commands_chooser_accept_description = In a window answering another program's Open or Save dialog: choose what is selected, or the name typed in the folder shown, as the program asked

commands_chooser_cancel = Cancel the dialog

commands_chooser_cancel_description = In a window answering another program's Open or Save dialog: choose nothing, and close the window

commands_chooser_choice = Set a choice of the dialog

commands_chooser_choice_description = In a window answering another program's dialog: set its choice `id` to `value`, true or false for a switch, an option's id for a list

# An item of the menu of the sidebar's Recent Folders heading, in a window answering another program's dialog: forget every folder that program chose in.
commands_chooser_clear_folders = Clear Recent Folders

commands_chooser_filter = Show the files of a filter

commands_chooser_filter_description = In a window answering another program's dialog: show the files of the program's filter `index` (from 0), or all files without it

commands_chooser_forget_folders = Forget where the program chose

commands_chooser_forget_folders_description = In a window answering another program's dialog: take `folder` out of the folders that program last chose in, or every one without it (with none left, its next dialog starts as one that chose nowhere would); with `restore`, put back the folders given, newest first, and when it last chose (`used`), as the Undo of taking out `folder` (or every one) does, its Redo taking it out again; `undo` says it is an Undo, said with Redo

commands_close_window = Close window

commands_close_window_description = Close this window and its tabs, which Reopen closed tab brings back; the last window closes as its close button does, what runs going on without it

commands_columns = Show or hide a column

commands_columns_description = Show or hide one of the list's detail columns

commands_columns_reset = Reset column widths

commands_columns_reset_description = Give the list's detail columns their default widths

commands_compare_copied = Compare with copied

commands_compare_copied_description = Say whether the clipboard holds a text (a checksum a download page gives, one word of it or all), ignoring case

commands_contents_pin = Preview: keep contents open

commands_contents_pin_description = Keep the contents open beside what is read, the content narrowed, or let choosing a part close them

commands_context_menu = Open the menu

commands_context_menu_description = Open the menu of the selection, or of the folder shown

commands_copy_other = Copy to the other pane

commands_copy_other_description = Copy the selection to what the other pane shows

commands_copy_text = Copy

commands_copy_text_description = Put a text on the clipboard

commands_create_here = New here…

commands_create_here_description = Make what the location makes (a tag in a tag's folder)

commands_cursor = Move the cursor

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_cursor_description = Move the file list's cursor (`to`), and with it the selection as `step` says

# A move of the file list's cursor, in F1 and the Keyboard settings: the selection grows to the item reached (Shift). to: where the cursor goes, as the identifier in brackets names it (never translated).
commands_cursor_extend_to = { $to ->
        [up] Extend the selection up
        [down] Extend the selection down
        [left] Extend the selection left
        [right] Extend the selection right
        [page_up] Extend the selection a page up
        [page_down] Extend the selection a page down
        [half_page_up] Extend the selection half a page up
        [half_page_down] Extend the selection half a page down
        [home] Extend the selection to the first item
        [end] Extend the selection to the last item
        [here] Extend the selection to the item under the cursor
        [previous] Extend the selection to the previous item
       *[next] Extend the selection to the next item
    }

# A family of the cursor's moves, one row of F1's list of keys: the selection grows to the item reached (up, down, to the first item…).
commands_cursor_family_extend = Extend the selection

# A family of the cursor's moves, one row of F1's list of keys: the selection grows to the item reached.
commands_cursor_family_extend_paged = Extend the selection a page up or down

# A family of the cursor's moves, one row of F1's list of keys: the selection follows the cursor if it did (up, down, to the first item…).
commands_cursor_family_go = Go

# A family of the cursor's moves, one row of F1's list of keys: the selection follows the cursor if it did.
commands_cursor_family_go_paged = Go a page up or down

# A family of the cursor's moves, one row of F1's list of keys: the selection stays (up, down, to the first item…).
commands_cursor_family_move = Move the cursor

# A family of the cursor's moves, one row of F1's list of keys: the selection stays.
commands_cursor_family_move_paged = Move the cursor a page up or down

# A family of the cursor's moves, one row of F1's list of keys: the selection becomes the item reached (up, down, to the first item…).
commands_cursor_family_select = Select

# A family of the cursor's moves, one row of F1's list of keys: the selection becomes the item reached.
commands_cursor_family_select_paged = Select a page up or down

# A move of the file list's cursor, in F1 and the Keyboard settings: the selection follows the cursor if it did, and stays if it was picked. to: where the cursor goes, as the identifier in brackets names it (never translated).
commands_cursor_go_to = { $to ->
        [up] Go up
        [down] Go down
        [left] Go left
        [right] Go right
        [page_up] Go a page up
        [page_down] Go a page down
        [half_page_up] Go half a page up
        [half_page_down] Go half a page down
        [home] Go to the first item
        [end] Go to the last item
        [here] Go to the item under the cursor
        [previous] Go to the previous item
       *[next] Go to the next item
    }

# A move of the file list's cursor, in F1 and the Keyboard settings: the selection stays (Ctrl). to: where the cursor goes, as the identifier in brackets names it (never translated).
commands_cursor_move_to = { $to ->
        [up] Move the cursor up
        [down] Move the cursor down
        [left] Move the cursor left
        [right] Move the cursor right
        [page_up] Move the cursor a page up
        [page_down] Move the cursor a page down
        [half_page_up] Move the cursor half a page up
        [half_page_down] Move the cursor half a page down
        [home] Move the cursor to the first item
        [end] Move the cursor to the last item
        [here] Move the cursor to the item under the cursor
        [previous] Move the cursor to the previous item
       *[next] Move the cursor to the next item
    }

# A window command's title. To pick is to mark an item as chosen, as yazi's mark does, beside the selection.
commands_cursor_pick = Pick or unpick

commands_cursor_pick_description = Pick or unpick the item under the cursor, as yazi's mark, then go to the next

# A move of the file list's cursor, in F1 and the Keyboard settings: the selection becomes the item reached (the arrows). to: where the cursor goes, as the identifier in brackets names it (never translated).
commands_cursor_select_to = { $to ->
        [up] Select up
        [down] Select down
        [left] Select left
        [right] Select right
        [page_up] Select a page up
        [page_down] Select a page down
        [half_page_up] Select half a page up
        [half_page_down] Select half a page down
        [home] Select to the first item
        [end] Select to the last item
        [here] Select only the item under the cursor
        [previous] Select to the previous item
       *[next] Select to the next item
    }

commands_cursor_toggle = Select or deselect

commands_cursor_toggle_description = Select or deselect the item under the cursor

# A window command's title: answer no to the question a dialog asks.
commands_dismiss = Cancel

commands_dismiss_description = Answer no to the question asked

# A family of keys, one row of F1's list: the preview's next or previous page.
commands_family_preview_page = Preview: turn the page

# A family of keys, one row of F1's list: the preview zoomed in, out or back.
commands_family_preview_zoom = Preview: zoom

# A family of keys, one row of F1's list: the text larger, smaller or back at 100%.
commands_family_text_size = Text size

# A family of keys, one row of F1's list: zoom in, out or back.
commands_family_zoom = Zoom

# A window command's title: take one of the items the field being typed in offers.
commands_field_complete = Take what the field offers

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_field_complete_description = Among what the field being typed in offers as it goes on (a placeholder begun, a sign), move the highlight by `step`, take the item `group` and `item` name, or the one lit: it takes the place of the part typed (ADR-137)

# A window command's title: what the field being typed in takes (its placeholders, terms, signs), shown under it.
commands_field_reference = What the field takes

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_field_reference_description = Show under the field being typed in what it takes: its placeholders, terms or signs, each with what it means and gives here, a row taken written where the caret is (ADR-137); shown, F1 shows the keys of the place. The address field says it in its card; `field` (its `?`) turns that on and off

commands_file_chooser_list = What answers open and save dialogs

commands_file_chooser_list_description = What answers other programs' Open and Save dialogs, as Settings lists it: the desktop portal's back ends installed for them, each with the `dialogs.choose` that chooses it, and the one its configuration names. Answers from memory

commands_file_chooser_put_back = Put back what answered open and save dialogs

commands_file_chooser_put_back_description = Put the user's portal configuration back as it was before the choice `done` (what Undo of `dialogs.choose` runs, from its notification), only while the file holds what that choice wrote

commands_file_chooser_set = Choose what answers open and save dialogs

commands_file_chooser_set_description = Name `portal` (a back end of the desktop portal, by its .portal file's name; empty for none) for other programs' Open and Save dialogs in the user's portal configuration, its other lines kept; it applies when the portal starts again. The notification offers Undo

commands_filter = Filter…

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's argument.
commands_filter_description = Filter the folder shown from the address field as you type, its subfolders left out, starting from `text` where it is given; what is typed there is looked for in the folder alone

commands_find = Find a name…

commands_find_description = Find a name in the pane as it is typed, after the cursor or before it

commands_find_next = Find the next name

commands_find_next_description = Find the name last looked for again, after the cursor (or before it)

commands_focus_address = Focus address bar

commands_focus_address_description = Edit the active pane's path

commands_focus_next_part = Focus next part of the window

commands_focus_next_part_description = Give the keyboard to the next part of the window shown: the sidebar, the files, the preview

commands_focus_other_pane = Focus other pane

commands_focus_other_pane_description = Give the keyboard to the split's other pane

commands_focus_pane = Focus file list

commands_focus_pane_description = Give the keyboard to the active pane

commands_focus_sidebar = Focus sidebar

commands_focus_sidebar_description = Give the keyboard to the sidebar

commands_form_complete = Complete the folder

commands_form_complete_description = Complete the folder typed in a dialog's Save as: the folder highlighted in its list, else the start the folders completing it share, else the first of them, the name kept

commands_form_up = Folder above

commands_form_up_description = In a dialog's Save as, the folder above the one typed, the name kept

commands_full_width = Toggle full-width reading

commands_full_width_description = Read text across the whole panel

commands_fullscreen_controls = Full screen: controls

commands_fullscreen_controls_description = Show or hide the bar over the preview on the whole screen

commands_fullscreen_step = Full screen: next

commands_fullscreen_step_description = On the whole screen, show the next or previous picture of the pane, or turn the page

commands_give_other = Send to the other pane…

commands_give_other_description = Ask what the selection does in what the other pane shows, as a drop asks: move, copy or link it there, or what a plugin offers (extract an archive)

commands_grammar_choose = Choose a language's grammar

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_grammar_choose_description = Color `language` (its name in lower case) with `grammar`: auto (as Automatic uses says), bat, plain, or where its tree-sitter grammar was found (skwir, tree-sitter, zed, helix, neovim, system, emacs)

commands_grammar_folders = Where grammars were looked for

commands_grammar_folders_description = Where tree-sitter grammars were looked for when last found: what was nearly found, each saying what to do, then each folder read and the languages it held

commands_grid_caption = Show or hide a grid caption

commands_grid_caption_description = Say one caption under every name in the grid, or no longer: its tags or one of the list's details, by its setting (ui.grid_caption_tags, ui.grid_caption_size…)

# A window command's title: thumbnails cropped to fill their box in the grid, or not.
commands_grid_fill = Toggle filled thumbnails

commands_grid_fill_description = Crop thumbnails in the grid to fill their box

# A window command's title: Back is the button; this lists the history behind it.
commands_history_menu = Back: history…

commands_history_menu_description = List the history behind Back, or ahead of Forward

# A window command's title: something done in a structured preview (search, copy, outline, source, hex).
commands_inspect = Preview action

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_inspect_description = Do something in a structured preview (`action`: search, copy, outline, source, hex…)

# A window command's title: go to a place by typing in the address field.
commands_jump = Jump…

commands_jump_description = Type in the address field, the places one goes to offered first

commands_key_about_close = Close About

commands_key_actions_down = Go to the action below

commands_key_actions_first = Go to the first action

commands_key_actions_last = Go to the last action

commands_key_actions_left = Go to the key on the left

commands_key_actions_open = Change the key, or add one

commands_key_actions_page_down = Go a page down the actions

commands_key_actions_page_up = Go a page up the actions

commands_key_actions_remove = Remove the key

commands_key_actions_right = Go to the key on the right

commands_key_actions_up = Go to the action above

# What a key does on a slider of the settings: to its lowest.
commands_key_adjust_least = The least

# What a key does on a setting's control: a number made smaller, or the choice before.
commands_key_adjust_less = Less, or the previous choice

# What a key does on a setting's control: a number made larger, or the choice after.
commands_key_adjust_more = More, or the next choice

# What a key does on a slider of the settings: to its highest.
commands_key_adjust_most = The most

# What a key does on a slider of the settings: a tenth of its range less.
commands_key_adjust_much_less = Much less

# What a key does on a slider of the settings: a tenth of its range more.
commands_key_adjust_much_more = Much more

commands_key_bar_first = First bar

commands_key_bar_last = Last bar

commands_key_bar_next = Next bar

commands_key_bar_next_too = Choose the next bar too

commands_key_bar_previous = Previous bar

commands_key_bar_previous_too = Choose the previous bar too

commands_key_bars_earlier = Earlier bars

commands_key_bars_later = Later bars

commands_key_bars_to_first = Choose the bars back to the first

commands_key_bars_to_last = Choose the bars up to the last

# What a key does in Rename several: the case its names are given (lowercase, UPPERCASE, Title Case…).
commands_key_case_next = Next case

# What a key does in Rename several: the case its names are given (lowercase, UPPERCASE, Title Case…).
commands_key_case_previous = Previous case

commands_key_exclusions_down = Go to the folder below

commands_key_exclusions_first = Go to the first category

commands_key_exclusions_last = Go to the last folder

commands_key_exclusions_left = Fold the category, or go to it

commands_key_exclusions_menu = Open the folder's menu

# What a key does in the list of the settings' Exclusions page: turn the switch or the tick of the row.
commands_key_exclusions_open = Turn the switch or the tick

commands_key_exclusions_page_down = Go a page down the folders

commands_key_exclusions_page_up = Go a page up the folders

commands_key_exclusions_remove = Remove the folder you added

commands_key_exclusions_right = Unfold the category

commands_key_exclusions_up = Go to the folder above

commands_key_fullscreen_next = Full screen: next picture or page

commands_key_fullscreen_previous = Full screen: previous picture or page

# What a window command is, read by agents and in F1's list of actions: one of the keys of a menu.
commands_key_in_menu = A key in a menu

# What a window command is, read by agents and in F1's list of actions: one of the keys of the settings.
commands_key_in_settings = A key in the settings

commands_key_inspect_back = Close the file shown

commands_key_inspect_end = Scroll to the end

commands_key_inspect_fold = Fold the folder, or go to it

commands_key_inspect_larger = Preview: larger text

commands_key_inspect_links_close = Stop labelling the links

commands_key_inspect_next = Next file

commands_key_inspect_page_down = Scroll a page down

commands_key_inspect_page_up = Scroll a page up

commands_key_inspect_previous = Previous file

# What a key does in a structured preview: its text back at its own size.
commands_key_inspect_reset = Preview: text at its size

commands_key_inspect_scroll_down = Scroll down

commands_key_inspect_scroll_up = Scroll up

commands_key_inspect_smaller = Preview: smaller text

commands_key_inspect_start = Scroll to the start

commands_key_inspect_unfold = Unfold the folder

commands_key_list_next = Next in the list

commands_key_list_page_down = A page down the list

commands_key_list_page_up = A page up the list

commands_key_list_previous = Previous in the list

commands_key_map_down = Map: move down

commands_key_map_left = Map: move left

commands_key_map_next = Map: next place

commands_key_map_previous = Map: previous place

commands_key_map_right = Map: move right

commands_key_map_up = Map: move up

commands_key_media_mute = Mute or unmute

commands_key_media_play = Play or pause

commands_key_media_volume_down = Volume down

commands_key_media_volume_up = Volume up

commands_key_model_move_down = Model: move down

commands_key_model_move_left = Model: move left

commands_key_model_move_right = Model: move right

commands_key_model_move_up = Model: move up

commands_key_model_turn_down = Model: turn down

commands_key_model_turn_left = Model: turn left

commands_key_model_turn_right = Model: turn right

commands_key_model_turn_up = Model: turn up

# What a key does in a field being typed in: the next of the items it offers as it is typed (a placeholder begun, a sign).
commands_key_offer_next = Next of what the field offers

# What a key does in a field being typed in: the previous of the items it offers as it is typed.
commands_key_offer_previous = Previous of what the field offers

commands_key_open_named = Open the file named

# What a key does in the file list. To pick is to mark an item as chosen, as yazi's mark does.
commands_key_pick = Pick or unpick

# What a key does in the file list. To pick is to mark an item as chosen, as yazi's mark does.
commands_key_pick_advance = Pick or unpick, then go to the next

commands_key_player_back = Player: back five seconds

commands_key_player_forward = Player: forward five seconds

commands_key_player_fullscreen = Player: full screen

commands_key_player_mute = Player: mute or unmute

commands_key_player_next = Player: next track

commands_key_player_previous = Player: previous track

commands_key_player_volume_down = Player: volume down

commands_key_player_volume_up = Player: volume up

commands_key_preview_down = Preview: scroll down

commands_key_preview_left = Preview: scroll left, or the previous page or picture

commands_key_preview_right = Preview: scroll right, or the next page or picture

commands_key_preview_up = Preview: scroll up

commands_key_record_down = Move the record down

commands_key_record_up = Move the record up

commands_key_review_next = Next choice

commands_key_review_previous = Previous choice

commands_key_search_contents = Search the contents…

commands_key_search_everywhere = Search everywhere

commands_key_settings_page_next = Next page

commands_key_settings_page_previous = Previous page

commands_key_sidebar_down = Go to the place below

commands_key_sidebar_first = Go to the first place

commands_key_sidebar_last = Go to the last place

commands_key_sidebar_left = Go to its group, or fold the group

commands_key_sidebar_menu = Open its menu

commands_key_sidebar_move_down = Move it down

commands_key_sidebar_move_up = Move it up

commands_key_sidebar_open = Open the place, or fold the group

commands_key_sidebar_remove = Remove or hide it

commands_key_sidebar_rename = Rename it

commands_key_sidebar_right = Unfold the group, or go into it

commands_key_sidebar_tab = Open the place in a new tab

commands_key_sidebar_up = Go to the place above

commands_key_submenu_back = Back from the submenu

commands_key_submenu_change_value = Change the value in the submenu

commands_key_submenu_first = First in the submenu

commands_key_submenu_last = Last in the submenu

commands_key_submenu_next = Next in the submenu

commands_key_submenu_next_value = Next value in the submenu

commands_key_submenu_open = Open the submenu

commands_key_submenu_previous = Previous in the submenu

commands_key_submenu_previous_value = Previous value in the submenu

commands_key_terminal_page_down = Terminal: a page down

commands_key_terminal_page_up = Terminal: a page up

# What a key does in the terminal: paste the text selected last anywhere on the desktop (the middle button's paste).
commands_key_terminal_paste_primary = Terminal: paste the text selected last

# What a key does: choose which buttons the row above the files has, the views' and Up's.
commands_key_view_buttons = Choose the row's buttons: the views', Up

commands_key_viewer_next = Viewer: next file

commands_key_viewer_previous = Viewer: previous file

commands_key_volume_down = Volume: down

commands_key_volume_highest = Volume: highest

commands_key_volume_lowest = Volume: lowest

commands_key_volume_up = Volume: up

commands_languages = Languages and their grammars

commands_languages_description = The languages a grammar was found or added for, as last found: each one's files and what colors it, Automatic or chosen, with what may be chosen; the grammars combined from two sources checked first

commands_layout = Arrange the window

commands_layout_description = Where split panes, the preview and a file of an archive go

commands_list_move = Next in the list

commands_list_move_description = Move in the list being typed into (the palette, a choice, Jump, the tag editor, the address's suggestions) or answered (the names a copy found taken): rows round its ends, or a page, or to an end

commands_map_open = Map: open in Maps

commands_map_open_description = Open the place shown in the desktop's maps

commands_map_pan = Map: pan

commands_map_pan_description = Move the map

commands_map_point = Map: next place

commands_map_point_description = Go to the next place on the map (or the previous one), the map centred on it

commands_map_view_fit = Map: fit to its files

commands_map_view_fit_description = Fit the Map view to its files

commands_map_view_search_area = Map: search the pictures and videos of this area

commands_map_view_search_area_description = Search the pictures and videos taken in the area shown

commands_map_view_spread = Map: spread the files at one place, or group them

commands_map_view_spread_description = Spread the files at one place, or group them

commands_maximize_window = Maximize window

commands_maximize_window_description = Maximize this window, or give it back the size it had when it is maximized

commands_media_autoplay = Player: play on selection

commands_media_autoplay_description = Play what is selected at once

commands_media_continue = Player: when a track ends

commands_media_continue_description = What plays when a track ends: nothing, the next one, or one shuffled

commands_media_control = Player: control

commands_media_control_description = Play, pause, seek or set the volume of what the preview plays

# A window command's title: leave full screen, else close the preview.
commands_media_escape = Player: leave

commands_media_escape_description = Leave full screen, else close the preview

commands_media_keep_playing = Player: keep playing while you browse

commands_media_keep_playing_description = Keep playing when the selection moves

# A window command's title: keep what plays playing while the selection moves, or let it go.
commands_media_pin = Player: pin or let go of this track

commands_media_pin_description = Keep what plays playing while the selection moves, or let it go

commands_media_play = Player: play the selection

commands_media_play_description = Play what is selected without opening the preview: music in the player at the foot of the panel, the tracks selected in their order (a playlist's tracks in its place, a track alone with the folder's around it); a film in the quick preview

# A window command's title: drag the playing position.
commands_media_scrub = Player: scrub

commands_media_scrub_description = Drag the playing position

commands_media_step = Player: next track

commands_media_step_description = Select the next or previous track

commands_menu_activate = Run the item

commands_menu_close = Close the menu

commands_menu_collapse = Leave the submenu, or the previous value

commands_menu_down = Next item

commands_menu_expand = Open the submenu, or the next value

commands_menu_first = First item

commands_menu_last = Last item

commands_menu_up = Previous item

commands_minimize_window = Minimize window

commands_minimize_window_description = Hide this window until it is chosen again

commands_mkdir_prompt = New folder…

commands_mkdir_prompt_description = Ask for a name and make a folder in the folder shown

commands_model_pan = Model: move

commands_model_pan_description = Move the view of the 3D model shown, by steps right (dx) and down (dy)

commands_model_perspective = Model: perspective

commands_model_perspective_description = Draw 3D models in perspective, nearer parts larger, or not

commands_model_smooth = Model: smooth shading

commands_model_smooth_description = Shade 3D models smoothly across their faces, or each face flat

commands_model_turn = Model: turn

commands_model_turn_description = Go round the 3D model shown, by steps right (dx) and down (dy)

commands_move_other = Move to the other pane

commands_move_other_description = Move the selection to what the other pane shows

commands_new_file_prompt = New file…

commands_new_file_prompt_description = Ask for a name and make an empty file (a folder for a name ending in /)

# A window command's title: a book's note (a footnote), closed or gone to.
commands_note = Note

commands_note_description = Close a book's note, or go to where it is

# A window command's title: complete what is typed in the address field (Tab there).
commands_omnibox_complete = Complete

commands_omnibox_complete_description = Complete what is typed in the address field: with the part the names that match it share, else with the suggestion highlighted; a folder completed is then listed

commands_omnibox_forget = Forget the suggestion

commands_omnibox_forget_description = Forget the recent search highlighted in the address field's suggestions

commands_omnibox_scopes = Where to search

commands_omnibox_scopes_description = List the scopes the address field's words could search in, or close the list

# A window command's title: run what the address field holds (Enter there).
commands_omnibox_submit = Go

commands_omnibox_submit_description = Run the address's suggestion, search the home folder, or open the file it names

commands_open_url = Open an address

commands_open_url_description = Open an address (http, https, mailto) with the desktop's application

commands_palette = Command palette

commands_palette_description = Find a command and run it

# A window command's title: the folders above the trail's first, listed.
commands_parents_menu = Folders above…

commands_parents_menu_description = List the folders above the trail's first, from the root down

# A window command's title: look at the file under the cursor in a large preview, while its key is held.
commands_peek = Peek

commands_peek_close = Close peek

commands_peek_close_description = Close the large preview

commands_peek_description = Look at the file under the cursor in a large preview, which closes when this runs again, or when its key, held, is let go

# A window command's title: let go of the plan being worked out for what was asked.
commands_plan_cancel = Stop working out

commands_plan_cancel_description = Let go of the plan being worked out for what was asked (Escape while it is): its answer is forgotten, and what waited for it runs on

# A window command's title: the player pinned at the foot of the side panel.
commands_player = Player

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's argument and the values it takes.
commands_player_description = Ask the pinned player what `request` says: `play`, `pause`, `next`, `stop`, `seek_by` some seconds…

commands_player_volume = Player: volume

commands_player_volume_description = Change the pinned player's volume

commands_preview_close = Preview: close

commands_preview_close_description = Close peek, or hide the preview in the side panel

commands_preview_contents = Preview: contents

commands_preview_contents_description = Show or hide the contents: a document's, a structured preview's headings

commands_preview_copy = Preview: copy

commands_preview_copy_description = Copy the text selected, else what is selected (a block), else all the preview shows

commands_preview_copy_path = Preview: copy path

commands_preview_copy_path_description = Copy the path of the file shown

commands_preview_find = Preview: find text

commands_preview_find_close = Preview: close find

commands_preview_find_close_description = Close the preview's find field

commands_preview_find_description = Find text in what the preview shows

commands_preview_fullscreen = Preview: full screen

commands_preview_fullscreen_description = Show the preview on the whole screen (a picture, pages, text, a film), or leave it

commands_preview_goto = Preview: go to page or chapter…

commands_preview_goto_description = Type the page to show, or a book's chapter

commands_preview_invoke = Preview: run with what is shown

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_preview_invoke_description = Run `command` with the file the preview shows, the page it is on (`page`) and what is selected in it (`selection`) added to its `args`: what a plugin's items of the preview's menu and its keys there run

commands_preview_match = Preview: next search match

commands_preview_match_description = Go to the next or previous match of what the find looks for

commands_preview_menu = Preview menu

commands_preview_menu_description = Open or close one of the preview's menus

commands_preview_open_external = Preview: open externally

commands_preview_open_external_description = Open the file shown with the desktop's application

commands_preview_page = Preview: next page

commands_preview_page_description = Go a page, a section or a chapter on or back

commands_preview_scroll = Preview: scroll

commands_preview_scroll_description = Move what the preview shows a step up, down or sideways; where it fits, Left and Right go to the page or section before or after, and on the whole screen to the picture

commands_preview_seek = Preview: seek

commands_preview_seek_description = Go seconds on or back in what plays

commands_preview_select_all = Preview: select all

commands_preview_select_all_description = Select all the text the preview shows

commands_preview_zoom = Preview: zoom

commands_preview_zoom_description = Zoom in or out of what the preview shows (a picture, a page, a map, a model), or make larger or smaller the text of what it reads (a book, a document)

commands_printed_pages = Printed page numbers

commands_printed_pages_description = Count a book's pages as printed, where it lists them

commands_properties = Properties

commands_properties_count = Properties: count what folders hold

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_properties_count_description = Count the size, room taken, files and folders of what Properties shows (the selection, with several selected), or stop counting with `stop`

commands_properties_description = Show or hide the properties of the selection (or of the folder shown) in the side panel, over the preview or alone; the panel stays while the preview is shown

# A window command's title: fold one of the groups of Properties (Permissions, Checksums).
commands_properties_fold = Properties: fold a group

# Read by agents and in F1's list of actions. `Checksums` and `Volume` are the keys of two of Properties' groups, written as they are in every language.
commands_properties_fold_description = Fold one of Properties' groups, by its key (`Checksums`, `Volume`…), or unfold it, for every item from now on

commands_quick_filters = Toggle Quick filters

commands_quick_filters_description = Show Quick filters above the active pane's files, wherever it goes: their kinds, years and tags, each with how many files, a choice filtering the pane until taken off

commands_quit = Quit

commands_quit_description = Quit, finishing what runs without the window or stopping it

commands_reader_continuous = Document: toggle continuous scrolling

commands_reader_continuous_description = Scroll a document's pages as one, or one at a time

commands_reader_drag = Document: toggle dragging to move the page

commands_reader_drag_description = Drag across a page's text to move the page, or to select the text

commands_reader_link = Document: follow a link

commands_reader_link_description = Follow one of a page's links

commands_reader_page = Document: go to page

commands_reader_page_cancel = Document: leave the page field

commands_reader_page_cancel_description = Leave the page field as it was

commands_reader_page_description = Show a document's page

commands_reader_page_submit = Document: show the page typed

commands_reader_page_submit_description = Show the page typed

commands_reader_pointer = Document: pointer

commands_reader_pointer_description = A press, a drag or a release on a document's page

commands_reader_search = Document: search

commands_reader_search_description = Search a document for what the find field holds

commands_rename_bulk_case = Change case

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's argument.
commands_rename_bulk_case_description = Choose the next case Rename several gives the names, back to the first after the last; with a `delta`, move that many cases along, stopping at the first or the last

commands_rename_bulk_next = Next field

commands_rename_bulk_next_description = Go to the next field of Rename several

commands_rename_bulk_prev = Previous field

commands_rename_bulk_prev_description = Go to the previous field of Rename several

commands_rename_bulk_prompt = Rename several…

commands_rename_bulk_prompt_description = Rename the selected items together

commands_rename_prompt = Rename…

commands_rename_prompt_description = Rename the selected item, or a place of the sidebar in the way its location renames it

commands_reveal = Show in folder

commands_reveal_description = Show a file in its folder, at a line of its preview

commands_review_apply = Apply the plan

commands_review_apply_description = Apply the plan being reviewed

commands_review_cancel = Discard the plan

commands_review_cancel_description = Discard the plan being reviewed

commands_review_choose = Next choice

commands_review_choose_description = In the names a copy, a move or a link found taken: the next choice of the row the keyboard is on, or the previous with a negative delta

commands_search = Search…

# A window command's title: a pill is a term of a search, drawn in the address field.
commands_search_chip = Search pill's menu

commands_search_chip_description = Open or close a search pill's menu

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's argument.
commands_search_description = Search the folder shown and its subfolders from the address field, starting from `text` where it is given; what is typed there is looked for below the folder

commands_select_group = Select the group

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_select_group_description = Select the entries of a group the pane draws under its heading (ADR-130): group `group`, counted from the first heading from 0, else the cursor's; with `add`, beside what is selected. A heading's click and its menu's Select these run it

commands_settings = Settings…

commands_settings_adjust = Change the value

commands_settings_close = Close settings

commands_settings_description = Open the settings, at a setting or a page

# A key in the settings: the keyboard goes from the list of pages into the page.
commands_settings_enter = Into the page

# A window command's title: what a key does in the list of the settings' Exclusions page.
commands_settings_exclusion = Exclusions page key

commands_settings_exclusion_description = A key in the Exclusions page's list: walk the categories and their folders, fold one, turn a switch or a tick, remove a folder you added, open a folder's menu

commands_settings_exclusion_item = Exclusions page menu item

commands_settings_exclusion_item_description = What the menu of a row of the Exclusions page does: duplicate, edit, move, reset or remove a folder; rename or delete one of your categories, or add a folder to one

# A window command's title: what a key does in the list of actions of the settings' Keyboard page.
commands_settings_key = Keyboard page key

commands_settings_key_description = A key in the Keyboard page's list of actions: walk the actions and their keys, change, add or remove one

# A key in the settings: move a record of a list up or down.
commands_settings_move = Move the record

commands_settings_next = Next control

commands_settings_page = Next page

commands_settings_prev = Previous control

commands_settings_search = Search settings

# A window command's title: the keys of where the keyboard is.
commands_shortcuts = Shortcuts

commands_shortcuts_description = List the keys of where the keyboard is

# A window command's title: what a key does in the sidebar.
commands_sidebar_key = Sidebar key

commands_sidebar_key_description = A key in the sidebar: walk, open, move, rename or remove its places

commands_sidebar_place = Arrange a sidebar place

commands_sidebar_place_description = Move, rename, show or hide a place the sidebar offers

commands_sidebar_section = Arrange a sidebar group

commands_sidebar_section_description = Move, show, hide or reset the sidebar's groups

commands_slideshow = Preview: slideshow

commands_slideshow_description = Play the pane's pictures one after another on the whole screen, from the one shown, or pause

commands_slideshow_pace = Slideshow: pace

commands_slideshow_pace_description = How long a slideshow shows each picture, in seconds

commands_split = Split pane

commands_split_description = Split the window in two panes, or join them

# A window command's title: the folders inside one of the trail's, listed.
commands_subfolder_menu = Folders inside…

commands_subfolder_menu_description = List the folders or places inside one of the trail's

commands_suggest = Address suggestions

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_suggest_description = What the address field offers for `text`: its rows, each with its section, title, detail and the invocation Enter runs, and the spans the text is drawn with

commands_tag_editor_apply = Apply tags

commands_tag_editor_apply_description = Apply what the tag editor changed

commands_tags_prompt = Tags…

commands_tags_prompt_description = Edit the tags of the selection, or of the files named

commands_terminal = Toggle terminal

commands_terminal_clear = Terminal: clear the history

commands_terminal_clear_description = Forget the lines scrolled away above the terminal's screen

commands_terminal_copy = Terminal: copy

commands_terminal_copy_description = Copy the terminal's selection

commands_terminal_description = Show or hide the terminal

# A key's action in the terminal: Escape, which stops labelling its links, or else goes to the program running there.
commands_terminal_escape = Terminal: Escape

commands_terminal_escape_description = Stop labelling the terminal's links, or send Escape to the program running there

commands_terminal_links = Terminal: follow a link…

commands_terminal_links_description = Label the terminal's links for the keyboard

commands_terminal_menu = Terminal: menu

commands_terminal_menu_description = Open the terminal's menu: copy, paste, its links

commands_terminal_page = Terminal: scroll

commands_terminal_page_description = Scroll the terminal by pages

commands_terminal_paste = Terminal: paste

commands_terminal_paste_description = Paste into the terminal

commands_terminal_paths = Terminal: type the selection's paths

commands_terminal_paths_description = Type the paths of the selected files at the terminal's prompt, quoted for its shell

commands_terminal_select_all = Terminal: select all

commands_terminal_select_all_description = Select everything the terminal shows and kept

commands_text_continuous = Continuous scrolling of text

commands_text_continuous_description = Read a book, a document or a message as one scrolling column, or a screen at a time

commands_text_size = Text size

commands_text_size_description = Write a part of the window larger or smaller, a step at a time, or back at 100%: the file views by default, else the interface, reading or code, as its setting does

commands_timeline = Timeline

commands_timeline_bucket = Timeline bars

commands_timeline_bucket_description = What each bar of the timeline counts, a day, a week, a month or a year, while the pane shows what it shows: by default the finest that shows every date

commands_timeline_cursor = Timeline: go to a bar

commands_timeline_cursor_description = Move the timeline's keyboard bar, or extend the bars chosen

commands_timeline_date = Timeline: which time

commands_timeline_date_description = Count files on the timeline by when they were modified, created, last opened, or taken (a picture's or a video's clock)

commands_timeline_description = Show or hide the timeline under the active pane's files, wherever it goes: when they changed, a bar for each period, a bar pressed showing only its files

commands_timeline_list = Timeline: list the bars chosen

commands_timeline_list_description = Show the files of the timeline's bars chosen

commands_timeline_measure = Timeline: files or size

commands_timeline_measure_description = Make the timeline's bars as tall as their number of files, or as their size

commands_timeline_pan = Timeline: move along

commands_timeline_pan_description = Show the timeline's earlier or later bars, when there are too many of them to show at once

commands_timeline_subtree = Timeline: folders inside

commands_timeline_subtree_description = Count the files of the folders inside too

commands_toggle_preview = Toggle preview

commands_toggle_preview_description = Show or hide the preview in the side panel; the panel stays while Properties is shown

commands_toggle_sidebar = Toggle sidebar

commands_toggle_sidebar_description = Show or hide the sidebar

commands_toolbar_activate = Press the button

commands_toolbar_activate_description = Press the button that has the keyboard

commands_toolbar_close = Close the menu

commands_toolbar_close_description = Close the toolbar's open menu

commands_toolbar_first = First row

commands_toolbar_first_description = Give the keyboard to the first row of the open menu

commands_toolbar_last = Last row

commands_toolbar_last_description = Give the keyboard to the last row of the open menu

# A window command's title: one of the menus of the row above the files or of the preview.
commands_toolbar_menu = Toolbar menu

commands_toolbar_menu_description = Open or close More, the View menu, the choice of view buttons, the Map view's backgrounds or the pinned player's More (kind: more, view_options, view_buttons, map_backgrounds, player)

commands_toolbar_next = Next button

commands_toolbar_next_description = Give the keyboard to the next button

commands_toolbar_prev = Previous button

commands_toolbar_prev_description = Give the keyboard to the previous button

commands_toolbar_submenu = Preview menu: submenu

# Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take.
commands_toolbar_submenu_description = Open a submenu of the preview's More or the View menu beside its row (`open`, or `here`: the one whose row has the keyboard), go back to its row (`back`), move the keyboard along its rows (`step`, `end`), or choose the value before (`value` -1), after (1) or the next round (0) on a setting's row

# An action's name: let go of what was typed in the file list to reach a file by its name, its label at the foot of the list going.
commands_type_end = Stop typing to find

commands_type_end_description = Let go of what was typed in the file list to reach a file by its name: its label goes, and the next character begins a new name

commands_undo_history = Undo history

commands_undo_history_description = Open or close the history of what Undo can take back, above the footer's Undo. Each action there has its own Undo, an undone one its own Redo, which takes it back alone when nothing done after it touched what it did; else the history names the action that did, and offers to undo it with those after it (ADR-079, 13)

commands_up_button = Show the Up button

commands_up_button_description = Show Up after Back and Forward on each pane's row, or leave it to the trail and Alt+Up

# A window command's title: what the blocks of the Usage view's map say (room taken, files, when they changed, kinds).
commands_usage_mode = Usage map shows

commands_usage_mode_description = Size the Usage map's blocks by the room or the files they hold, or color them by when they changed or the kinds of their files

commands_usage_refresh = Measure again

commands_usage_refresh_description = Measure what the Usage view shows again

commands_view = Show a view

commands_view_button = Show or hide a view's button

commands_view_button_description = Give a view a button beside the address, or take it away; the view shown keeps its button while shown

commands_view_buttons_reset = Default view buttons

commands_view_buttons_reset_description = Give a button to the views that show the files as listed (list, grid, columns), and no other

commands_view_buttons_shown = Show view buttons

commands_view_buttons_shown_description = Show the views' buttons beside the address, or one button for them, the view shown, that opens the View menu

commands_view_description = Show the pane's files in a view

commands_viewer_reveal = Show in Skwir

commands_viewer_reveal_description = Open the folder of the file a viewer's window shows in Skwir, the file selected

commands_viewer_step = Viewer: next file

commands_viewer_step_description = In a viewer's window, show the next or previous file of its folder, or of the files it was given

commands_volume_step = Volume: step

commands_volume_step_description = Step the volume of the player whose volume popup is open

commands_wrap_lines = Wrap lines

commands_wrap_lines_description = Wrap a text's long lines to the panel, or let them go on sideways

# A window command's title: the size of what the view draws (tiles, rows, a map's scale).
commands_zoom = Zoom

commands_zoom_description = Size what the view sizes (tiles, rows, a map's scale): in, out or back

count_files = { $count ->
        [one] { $count } file
       *[other] { $count } files
    }

count_folders = { $count ->
        [one] { $count } folder
       *[other] { $count } folders
    }

count_items = { $count ->
        [one] { $count } item
       *[other] { $count } items
    }

# Under what a deletion takes: none of it is there any more.
deleting_already_gone = Already gone

# Under what a deletion takes: what it holds in all, of which some could not be read or is on another drive.
deleting_at_least_empty_folders = At least { $folders ->
        [one] { $folders } empty folder
       *[other] { $folders } empty folders
    }

# Under what a deletion takes: what it holds in all, of which some could not be read or is on another drive.
deleting_at_least_files = At least { $size } in { $files ->
        [one] { $files } file
       *[other] { $files } files
    }

# Under what a deletion takes: what it holds in all, of which some could not be read or is on another drive.
deleting_at_least_files_folders = At least { $size } in { $files ->
        [one] { $files } file
       *[other] { $files } files
    } and { $folders ->
        [one] { $folders } folder
       *[other] { $folders } folders
    }

# Under what a deletion takes, while what it holds is counted.
deleting_counting = Counting…

# Under what a deletion takes, while what it holds is counted: how much so far.
deleting_counting_so_far = Counting… { $size } so far

# When pointed at the mark after a name in what a deletion takes: the companion files deleted with it (its notes, its sidecars), how many, then one path a line, as written.
deleting_deleted_with = { $count ->
        [one] Deleted with it:
       *[other] Deleted with it:
    }
        { $paths }

# Under what a deletion takes: what it holds in all.
deleting_empty_folders = { $folders ->
        [one] { $folders } empty folder
       *[other] { $folders } empty folders
    }

# Under what a deletion takes: what it holds in all.
deleting_files = { $size } in { $files ->
        [one] { $files } file
       *[other] { $files } files
    }

# Under what a deletion takes: what it holds in all.
deleting_files_folders = { $size } in { $files ->
        [one] { $files } file
       *[other] { $files } files
    } and { $folders ->
        [one] { $folders } folder
       *[other] { $folders } folders
    }

# On a row of what a deletion takes: a folder counted with the folder above it, also deleted.
deleting_folder = folder

# On a folder's row of what a deletion takes: what it holds, of which some could not be read or is on another drive.
deleting_folder_at_least = at least { $size } · { $count ->
        [one] { $count } file
       *[other] { $count } files
    }

# On a folder's row of what a deletion takes, while what it holds is counted: how many files so far.
deleting_folder_counting = { $count ->
        [one] { $count } file
       *[other] { $count } files
    }…

# On a folder's row of what a deletion takes: it holds nothing.
deleting_folder_empty = empty

# On a folder's row of what a deletion takes: what it holds.
deleting_folder_holds = { $size } · { $count ->
        [one] { $count } file
       *[other] { $count } files
    }

# Asked before deleting items of a trash for good.
deleting_from_trash = { $count ->
        [one] Delete { $count } item from the trash permanently? This cannot be undone.
       *[other] Delete { $count } items from the trash permanently? This cannot be undone.
    }

# Under what a deletion takes: the folder all its items are in, as the address writes it (~/Documents).
deleting_in_folder = In { $folder }

# Under what a deletion takes: where its items are.
deleting_in_trash = In the trash

# Under what a deletion takes: an item of the trash, and the folder it was put there from, as the address writes it (~/Documents).
deleting_in_trash_from = In the trash, from { $folder }

# Asked before deleting items for good.
deleting_items = { $count ->
        [one] Delete { $count } item permanently? This cannot be undone.
       *[other] Delete { $count } items permanently? This cannot be undone.
    }

# Asked before deleting on a server that keeps what is deleted in a trash of its own; trash: that trash, as the server names it (Google Drive's trash), words said inside a sentence that carry their own determiner: add no article before it.
deleting_kept = { $count ->
        [one] Delete { $count } item? It goes to { $trash }; Skwir cannot put it back.
       *[other] Delete { $count } items? They go to { $trash }; Skwir cannot put them back.
    }

# Asked before deleting on a server that keeps what is deleted in a trash of its own; trash: that trash, as the server names it (Google Drive's trash), words said inside a sentence that carry their own determiner: add no article before it.
deleting_kept_one = Delete { $name }? It goes to { $trash }; Skwir cannot put it back.

# On a row of what a deletion takes: it is a link (the link goes, not what it leads to).
deleting_link = link

# What a screen reader calls the list of what a deletion takes.
deleting_list = What is deleted

# On a row of what a deletion takes: it is gone already.
deleting_not_there = not there

# The question asked before deleting, then what the action could tell of what stays (1 of these has other names: its file stays).
deleting_noted = { $asked } { $then }.

# Asked before deleting a file or folder for good.
deleting_one = Delete { $name } permanently? This cannot be undone.

# What a screen reader hears of a row of what a deletion takes, fact after fact: what it said so far (notes.txt), then the next (500 B).
deleting_said_and = { $said }, { $fact }

# What a screen reader hears of a row of what a deletion takes, after its name: how many companion files are deleted with it, and their names, a list. Begins in lower case.
deleting_said_with = { $count ->
        [one] deleted with it: { $names }
       *[other] deleted with it: { $names }
    }

# On a row of what a deletion takes: the file's size, alone.
deleting_size = { $size }

# Under what a deletion takes: where its items are (In ~/Documents), then what they hold in all (2.6 kB in 4 files).
deleting_summary = { $place } · { $held }

# A mark after a name in what a deletion takes: companion files (its notes, its sidecars) are deleted with it. Where room is short, a bare + stands for it.
deleting_with_metadata = + metadata

# The last item of the menu a drop asks with: drop nothing.
dnd_cancel = Cancel

# Said when files are dropped on a place or a pane that takes none.
dnd_cannot_drop_here = Files cannot be dropped here

# Under the files dragged while Ctrl is held: the drop copies them. name: the file's name, bare, as under a picture.
dnd_copy_dragged = { $count ->
        [1] Copy { $name }
       *[other] Copy { $count } items
    }

# An item of the menu a drop asks with: copy the files dropped into this folder.
dnd_copy_here = Copy Here

# Said when the files dropped were gone by the time the drop was carried out.
dnd_dropped_gone = The dropped items no longer exist

# Under the files dragged while Ctrl and Shift are held: the drop makes a link to each. name: the file's name, bare, as under a picture.
dnd_link_dragged = { $count ->
        [1] Link { $name }
       *[other] Link { $count } items
    }

# An item of the menu a drop asks with: make a link to each file dropped in this folder.
dnd_link_here = Link Here

# Under the files dragged while Shift is held: the drop moves them. name: the file's name, bare, as under a picture.
dnd_move_dragged = { $count ->
        [1] Move { $name }
       *[other] Move { $count } items
    }

# An item of the menu a drop asks with: move the files dropped into this folder.
dnd_move_here = Move Here

# Said when the selection is offered to the other pane (Shift+F7) while the window shows one pane.
dnd_no_other_pane = There is no other pane: split the window first

# Said when files are dropped on the Trash with the keys that copy or link them.
dnd_not_copied_to_trash = Files cannot be copied to the trash: move them there instead

# The pinned player's button: show its film on the whole screen.
dock_fullscreen = Fullscreen

# The pinned player's button opening what its transport left out.
dock_more = More

# The pinned player's button: silence it.
dock_mute = Mute

dock_next = Next track

# The pinned player's button: play what it holds, or pause it.
dock_play_pause = Play / pause

# The tooltip of the pinned player's cover: a click plays or pauses, as the Space key does.
dock_play_pause_space = Play / pause · Space

# Where the pinned player is in what it plays, and how long it lasts (1:23 / 4:56).
dock_position = { $position } / { $duration }

dock_previous = Previous track

# The tooltip of the pinned player's title: a click shows the file it plays, selected in its folder.
dock_reveal = Show this track

# The pinned player's button: stop playing and close the player.
dock_stop = Stop and let go

# The pinned player's button: give its sound back.
dock_unmute = Unmute

# Said once, the first time Skwir speaks a translation that a person who speaks the language has not reviewed yet: this one. Say a draft, not who made it.
drafts_notice = This translation is a draft, not fully reviewed yet.

# The button of the toast saying the translation is a draft: Skwir speaks English from then on (Settings › Language).
drafts_use_english = Use English

# A button of the Exclusions page: opens the dialog that adds an exclusion.
exclusions_add = Add…

# The answer of the dialog that adds an exclusion.
exclusions_add_answer = Add

# The title of the dialog that adds an exclusion.
exclusions_add_title = Add an exclusion

# Said on the Exclusions page after an exclusion is added. folder: as its row writes it; category: its name.
exclusions_added = Added { $folder } to { $category }

# In the dialog that adds an exclusion, where the list names these folders already. category: the name of the category it is in; Show it is the dialog's button.
exclusions_already = Already in the list, in { $category }: Show it, rather than a second.

# The title of what the Exclusions page asks before a change lets agents read what was kept from them.
exclusions_asking_title = Let agents read these files and folders?

# Under the folders a change on the Exclusions page would let agents read: what that means.
exclusions_asking_why = An agent driving Skwir, a plugin from outside Skwir and a model could then list them and read what they hold.

# Beside a folder on the Exclusions page: a path that is not on this computer.
exclusions_badge_absent = Not on this computer

# Beside a folder on the Exclusions page: one of skwir's exclusions, changed (Reset takes it back).
exclusions_badge_changed = Changed

# Beside a folder on the Exclusions page: one of skwir's exclusions, moved to another category than skwir's.
exclusions_badge_moved = Moved here

# Beside a folder on the Exclusions page: an exclusion of the user's own.
exclusions_badge_yours = Added by you

# What the Only beside field of the dialog that adds an exclusion shows while empty: examples of a file's name, written as typed.
exclusions_beside_placeholder = Cargo.toml, *.csproj

# The answer of the Exclusions page's dialogs that leaves things as they are.
exclusions_cancel = Cancel

# In the dialog that makes a category on the Exclusions page: what a category does.
exclusions_category_about = Its exclusions hold for every role you tick them for; its switches turn them all off at once.

# Said on the Exclusions page after the user's category is deleted: its exclusions went back to where they were. category: its name.
exclusions_category_deleted = Deleted the category { $category }: its exclusions went back

# Said on the Exclusions page after the user makes a category. category: its name.
exclusions_category_made = Made the category { $category }

# The name field of the dialog that makes or renames a category on the Exclusions page.
exclusions_category_name = Name

# What the name field of the dialog that makes a category on the Exclusions page shows while empty: examples of a category's name.
exclusions_category_placeholder = Work, Virtual machines

# Said on the Exclusions page after the user's category is renamed: its name before, and after.
exclusions_category_renamed = Renamed the category { $old } to { $new }

# Under the name field of the dialog that makes or renames a category on the Exclusions page: another category has the name typed.
exclusions_category_taken = There is a category { $category } already

# Under the name field of the dialog that makes a category on the Exclusions page, while it is empty.
exclusions_category_unnamed = A category needs a name

# A check of the Exclusions page: lists only what differs from skwir's exclusions (changed or added).
exclusions_changed_only = Changed

# The answer of the system's folder picker opened from the dialog that adds an exclusion.
exclusions_choose = Choose

# A button of the dialog that adds an exclusion: opens the system's folder picker.
exclusions_choose_folder = Choose folder…

# Said on the Exclusions page after one of the user's exclusions is duplicated. folder: as its row writes it.
exclusions_copied = Copied { $folder }

# Said on the Exclusions page after one of skwir's exclusions is duplicated. folder: as its row writes it.
exclusions_copied_builtin = Copied { $folder }: the copy is yours to edit, Skwir's is unticked

# The answer of the dialog that makes a category on the Exclusions page.
exclusions_create = Create

# Under the Only beside field of the dialog that adds an exclusion, while it is empty. Cargo.toml and *.csproj are examples, written so.
exclusions_draft_beside = Say which file or folder it is beside: Cargo.toml, *.csproj

# Under the Name field of the dialog that adds an exclusion beside a file, where a path is typed. target is an example, written so.
exclusions_draft_beside_path = Beside a file, give a name (target), not a path

# Under the Holding field of the dialog that adds an exclusion, while it is empty. .nosearch and CACHEDIR.TAG are examples, written so.
exclusions_draft_holding = Say which file a folder holds: .nosearch, CACHEDIR.TAG

# Under the Name or path field of the dialog that adds an exclusion, while it is empty. regex: is written so.
exclusions_draft_pattern = Say what it names: a name, a path from ~ or /, a glob or regex:…

# The title of the dialog that edits the user's exclusion.
exclusions_edit_title = Edit exclusion

# Said on the Exclusions page after the user's exclusion is edited. folder: as its row writes it.
exclusions_edited = Changed { $folder }

# In an unfolded category of the Exclusions page that holds nothing. Add… and Move to are the names of a button and of a menu's row.
exclusions_empty_category = Nothing in it yet: Add… puts an exclusion here, or Move to from a row's menu

# In an unfolded category of the Exclusions page when the filters keep none of its folders.
exclusions_empty_found = None of its exclusions is shown

# In an unfolded category of the Exclusions page with Only on this computer checked, when none of its folders is here.
exclusions_empty_here = Nothing it names is on this computer

# On the Exclusions page of the settings, said before the picker of roles (Searches and tags, Agents): what the ticks keep folders from.
exclusions_excluded_from = Excluded from

# Asked before a change on the Exclusions page that lets agents read what was kept from them: the first two it names, as the page says them, and how many more.
exclusions_exposure_more = Agents will be able to read { $first }, { $second } and { $more } more.

# Asked before a change on the Exclusions page that lets agents read what was kept from them: folder is what it names, as the page says it (~/.ssh, any folder named .gnupg).
exclusions_exposure_one = Agents will be able to read { $folder }.

# Asked before a change on the Exclusions page that lets agents read what was kept from them: the two it names, as the page says them.
exclusions_exposure_two = Agents will be able to read { $first } and { $second }.

# A field of the dialog that adds an exclusion beside a file: the file or folder beside it (Cargo.toml).
exclusions_field_beside = Only beside

# A field of the dialog that adds an exclusion: the category it goes in.
exclusions_field_category = Category

# A field of the dialog that adds an exclusion of folders holding a file: the file's name (.nosearch).
exclusions_field_holding = Holding

# A field of the dialog that adds an exclusion beside a file: the folder's name (target).
exclusions_field_name = Name

# A field of the dialog that adds an exclusion: a name, a path, a glob or a regex.
exclusions_field_pattern = Name or path

# A field of the dialog that adds an exclusion: what the folders are, in the user's words (Virtual machines).
exclusions_field_what = What it is

# What a screen reader names the Exclusions page's filter by.
exclusions_filter_label = Filter exclusions

# What the Exclusions page's filter shows while empty.
exclusions_filter_placeholder = Filter by name, path or what it is

# The tooltip of an unfolded category's heading on the Exclusions page.
exclusions_fold = Fold

# What the settings' search finds on the Exclusions page. query: the words searched, as typed.
exclusions_found = { $count ->
        [1] One exclusion matches { $query }
       *[other] { $count } exclusions match { $query }
    }

# After a category's name on the Exclusions page: how many of its folders are excluded from the role chosen (kept), of all it holds (total).
exclusions_heading_count = { $kept ->
        [one] { $kept } of { $total } excluded
       *[other] { $kept } of { $total } excluded
    }

# After a category's name on the Exclusions page: how many of its folders the filters find (shown), of all it holds (total), then how many are excluded from the role chosen (kept).
exclusions_heading_count_found = { $shown ->
        [one] { $shown } found
       *[other] { $shown } found
    } of { $total } · { $kept ->
        [one] { $kept } excluded
       *[other] { $kept } excluded
    }

# After a category's name on the Exclusions page: how many of its folders are on this computer (shown), of all it holds (total), then how many are excluded from the role chosen (kept).
exclusions_heading_count_here = { $shown } here of { $total } · { $kept ->
        [one] { $kept } excluded
       *[other] { $kept } excluded
    }

# What a screen reader hears of a category's heading on the Exclusions page: its name, how many of its folders are excluded from the role (kept) of all (total); where fewer are shown, how many are on this computer or found (shown); folded where it is.
exclusions_heading_said = { $name }, { $kept ->
        [one] { $kept } of { $total } { $role ->
            [searches] excluded from searches and tags
            [agents] excluded from agents
           *[index] excluded from the index
        }
       *[other] { $kept } of { $total } { $role ->
            [searches] excluded from searches and tags
            [agents] excluded from agents
           *[index] excluded from the index
        }
    }{ $narrowed ->
        [here] , { $shown } here
        [found] , { $shown ->
            [one] { $shown } found
           *[other] { $shown } found
        }
       *[all] {""}
    }{ $folded ->
        [yes] , folded
       *[no] {""}
    }

# In the dialog that adds an exclusion, beside its answers: what Enter does, and the role the new exclusion is kept from.
exclusions_hint_add = Enter adds it, { $role ->
        [searches] excluded from searches and tags
        [agents] excluded from agents
       *[index] excluded from the index
    }

# In the dialog that edits an exclusion, beside its answers.
exclusions_hint_save = Enter saves it

# In the dialog that adds an exclusion, beside its answers, where the list names these folders already.
exclusions_hint_show = Enter shows it in the list

# A row of the Exclusions page: any folder holding this file, the file's name as written (.nosearch). holding is also the field's name in the settings file.
exclusions_holding = holding { $file }

# What the Holding field of the dialog that adds an exclusion shows while empty: examples of a file's name, written as typed.
exclusions_holding_placeholder = .nosearch, CACHEDIR.TAG

# On the Index page of the settings, the way to the Exclusions page: how many exclusions are kept from searches.
exclusions_index = What searches leave out, whatever answers them: { $count ->
        [one] { $count } exclusion
       *[other] { $count } exclusions
    }. Your Secrets are kept out of the indexes Skwir can tell.

# A button of the Index page of the settings: opens the Exclusions page on what searches leave out.
exclusions_index_open = Exclusions…

# An answer to what the Exclusions page asks before a change lets agents read folders: nothing changes.
exclusions_keep_private = Keep private

# A kind of exclusion, in the dialog that adds one: a folder by its name, only where a file is beside it.
exclusions_kind_beside = Name beside a file

# A kind of exclusion, in the dialog that adds one: any folder holding a file of that name.
exclusions_kind_holding = Folders holding a file

# A kind of exclusion, in the dialog that adds one: a name, a path, a glob or a regex.
exclusions_kind_pattern = Name or path

# What a screen reader names the choice of kinds by, in the dialog that adds an exclusion: a name or a path, a name beside a file, folders holding a file.
exclusions_kinds_named = What it names

# An answer to what the Exclusions page asks before a change lets agents read folders: the change is saved.
exclusions_let_agents_read = Let agents read

# Under a folder on the Exclusions page whose category is switched off for the role chosen: its tick waits for its category.
exclusions_line_off = { $role ->
        [searches] Not excluded from searches and tags: its category is off
        [agents] Not excluded from agents: its category is off
       *[index] Not excluded from the index: its category is off
    }

# Under a folder on the Exclusions page whose category is off for the role: that its category is off, then what it is and means.
exclusions_line_off_under = { $off } · { $under }

# Under a folder on the Exclusions page: what it is (Virtual machines, as its entry says), then what it means (any folder named target beside a Cargo.toml).
exclusions_line_under = { $what } · { $meaning }

# What a screen reader names the Exclusions page's list by: the page, and the role it shows.
exclusions_list_named = { $role ->
        [searches] Exclusions excluded from searches and tags
        [agents] Exclusions excluded from agents
       *[index] Exclusions excluded from the index
    }

# The Exclusions page's list while it looks up which of its folders are on this computer.
exclusions_looking = Looking for these files and folders on this computer…

# In the dialog that adds an exclusion, as it is typed: what it means (any folder named target beside a Cargo.toml).
exclusions_means = Means { $means }.

# In the dialog that adds an exclusion, as it is typed: what it means (~/VMs and all it holds, any folder named target), and that the folder is on this computer.
exclusions_means_here = Means { $means }, on this computer.

# In the dialog that adds an exclusion, as it is typed: what it means, and that the folder is not on this computer.
exclusions_means_not_here = Means { $means }, not on this computer.

# A category's menu on the Exclusions page: opens the dialog that adds an exclusion to this category.
exclusions_menu_add_here = Add an exclusion here…

# A category's menu on the Exclusions page: deletes the user's category, its folders going back.
exclusions_menu_delete = Delete

# A row's menu on the Exclusions page: makes a copy of the exclusion, the user's to edit.
exclusions_menu_duplicate = Duplicate

# A row's menu on the Exclusions page: opens the dialog that edits the user's exclusion.
exclusions_menu_edit = Edit…

# A row's menu on the Exclusions page: removes the user's exclusion.
exclusions_menu_remove = Remove

# A category's menu on the Exclusions page: opens the dialog that renames the user's category.
exclusions_menu_rename = Rename…

# A row's menu on the Exclusions page: puts one of skwir's exclusions back as skwir ships it.
exclusions_menu_reset = Reset

# What a screen reader names a row's menu button by, on the Exclusions page.
exclusions_more = More

# The tooltip of a row's menu button on the Exclusions page: the Menu key opens it too.
exclusions_more_tooltip = More (Menu key)

# A row of the Exclusions page, where a path is written for other rows: the folders where the system mounts the kernel's own filesystems (proc, sysfs), wherever they are.
exclusions_mounted_kernel = the kernel's filesystems

# A submenu of a row's menu on the Exclusions page: the categories the folder may move to.
exclusions_move_to = Move to

# Said on the Exclusions page after an exclusion is moved to another category. folder: as its row writes it; category: its name.
exclusions_moved = Moved { $folder } to { $category }

# In the dialog that adds an exclusion: it excludes files and folders alike.
exclusions_names_both = Files and folders

# In the dialog that adds an exclusion: it excludes files only.
exclusions_names_files = Files

# In the dialog that adds an exclusion: it excludes folders only.
exclusions_names_folders = Folders

# What a screen reader names the choice of files, folders or both by, in the dialog that adds an exclusion.
exclusions_names_named = Excludes

# Said on the Exclusions page where Remove is asked of one of skwir's exclusions.
exclusions_never_removed = Skwir's own exclusions are unticked, never removed

# A button of the Exclusions page: opens the dialog that makes a category of the user's.
exclusions_new_category = New category…

# The title of the dialog that makes a category on the Exclusions page.
exclusions_new_category_title = New category

# The Exclusions page's list with Changed checked, when nothing differs from skwir's.
exclusions_none_changed = You changed none of Skwir's exclusions and added none yet.

# The Exclusions page's list when it lists nothing.
exclusions_none_here = No exclusions here.

# The Exclusions page's list when its filter finds nothing. filter: the words typed, as typed.
exclusions_none_match = No exclusion matches { $filter }.

# A check of the Exclusions page: lists only the folders that are on this computer.
exclusions_only_here = Only on this computer

# The name of the settings' page of the folders kept from searches and agents: names its list to a screen reader, heads what the search finds there and the Privacy page's way to it.
exclusions_page = Exclusions

# What an exclusion of one folder means, in the dialog that adds it (after Means): the path as typed, and everything inside it.
exclusions_path_and_all = { $path } and all it holds

# What the Name or path field of the dialog that adds an exclusion shows while empty: examples, each written as it is typed.
exclusions_pattern_placeholder = ~/VMs, node_modules, cmake-build-*, regex:…

# On the Privacy page of the settings, the way to the Exclusions page: how many exclusions are kept from agents.
exclusions_privacy = What agents, plugins from outside Skwir and models never see: { $count ->
        [one] { $count } exclusion
       *[other] { $count } exclusions
    }, Skwir's secrets and yours.

# A button of the Privacy page of the settings: opens the Exclusions page on what agents never see.
exclusions_privacy_open = Exclusions…

# Said on the Exclusions page after one of the user's exclusions is removed. folder: as its row writes it (~/VMs, holding .nosearch).
exclusions_removed = Removed { $folder }

# The answer of the dialog that renames the user's category on the Exclusions page.
exclusions_rename = Rename

# The title of the dialog that renames the user's category on the Exclusions page.
exclusions_rename_category_title = Rename category

# Said on the Exclusions page after one of skwir's exclusions is reset. folder: as its row writes it.
exclusions_reset = { $folder } is back as Skwir ships it

# The tooltip of a changed row's revert button on the Exclusions page.
exclusions_reset_tooltip = Reset: back as Skwir ships it

# On the Exclusions page of the settings, a segment of the picker after Excluded from: the folders kept from agents, plugins from outside skwir and models.
exclusions_role_agents = Agents

# On the Exclusions page of the settings, a segment of the picker after Excluded from: the folders kept from the index of what files hold.
exclusions_role_index = The index

# On the Exclusions page of the settings, a segment of the picker after Excluded from: the folders kept from a search's walk, a tag's count and the timeline.
exclusions_role_searches = Searches and tags

# The answer of the dialog that edits an exclusion.
exclusions_save = Save

# The answer of the dialog that adds an exclusion, where the list names these folders already: shows that one.
exclusions_show_it = Show it

# A button beside what the settings' search finds on the Exclusions page: opens the page, filtered by the words searched.
exclusions_show_them = Show them

# What a screen reader names a category's switch on the Exclusions page by: the category's name, and the role it is for.
exclusions_switch_named = { $category } { $role ->
        [searches] excluded from searches and tags
        [agents] excluded from agents
       *[index] excluded from the index
    }

# Said on the Exclusions page after a category is switched off for the role chosen. category: its name, whose gender is not known: where words would agree with it, say it so they need not (an exclusion switched off).
exclusions_switched_off = { $category }: no longer { $role ->
        [searches] excluded from searches and tags
        [agents] excluded from agents
       *[index] excluded from the index
    }

# Said on the Exclusions page after a category is switched on for the role chosen. category: its name, whose gender is not known: where words would agree with it, say it so they need not (an exclusion switched on).
exclusions_switched_on = { $category }: { $role ->
        [searches] excluded from searches and tags
        [agents] excluded from agents
       *[index] excluded from the index
    }

# Said on the Exclusions page after a folder is ticked. folder: as its row writes it (~/.ssh, holding .nosearch).
exclusions_ticked = { $folder } is { $role ->
        [searches] excluded from searches and tags
        [agents] excluded from agents
       *[index] excluded from the index
    }

# Said on the Exclusions page after a folder is ticked while its category is switched off for the role chosen: the tick waits for it. folder: as its row writes it.
exclusions_ticked_off = Ticked { $folder }; its category is off for { $role ->
        [searches] searches and tags
        [agents] agents
       *[index] the index
    }

# The tooltip of a folded category's heading on the Exclusions page.
exclusions_unfold = Unfold

# Above the Exclusions page's list: what the settings hold of the exclusions and categories that cannot be read. count: how many; problems: a list, each where it is and why, the first few then how many more.
exclusions_unreadable = { $count ->
        [one] Not applied, as it cannot be read: { $problems }
       *[other] Not applied, as they cannot be read: { $problems }
    }

# The last item of the list of exclusions that cannot be read, above the Exclusions page's list, after the first few: how many others (a, b, c and 2 more).
exclusions_unreadable_others = { $count ->
        [one] { $count } more
       *[other] { $count } more
    }

# Said on the Exclusions page after a folder is unticked. folder: as its row writes it.
exclusions_unticked = { $folder } is no longer { $role ->
        [searches] excluded from searches and tags
        [agents] excluded from agents
       *[index] excluded from the index
    }

# Said on the Exclusions page after a folder is unticked while its category is switched off for the role chosen. folder: as its row writes it.
exclusions_unticked_off = Unticked { $folder }; its category is off for { $role ->
        [searches] searches and tags
        [agents] agents
       *[index] the index
    }

# What the What it is field of the dialog that adds an exclusion shows while empty: an example of what a folder is.
exclusions_what_placeholder = Virtual machines

# What the desktop shows while Skwir finishes without its window.
finishing_about = Finishing

# What the desktop shows while a question waits and no window is up.
finishing_about_asks = Open Skwir to answer

# What the desktop shows while what runs is paused and no window is up: the action by its title, capitalised (Copy 3 items to Docs), or how many (2 actions).
finishing_about_paused = { $what }: paused

# What the desktop shows of an action an error stopped while no window is up: its title, capitalised (Copy 3 items to Docs).
finishing_about_stopped = { $title }: stopped

# What the desktop shows while an action waits its turn and no window is up.
finishing_about_waiting = Waiting its turn

# What the desktop shows while an action waits for its server to answer again and no window is up.
finishing_about_waits_for_server = Waiting for its server

# What the desktop shows of what runs while no window is up: how many actions.
finishing_actions = { $count ->
        [one] { $count } action
       *[other] { $count } actions
    }

# The title of what the desktop shows while Skwir finishes without its window.
finishing_title = Finishing

# The title of what the desktop shows while a question waits and no window is up.
finishing_title_asks = An action needs an answer

# The title of what the desktop shows while Skwir finishes without its window: how many actions it has left, running or waiting.
finishing_title_count = Finishing { $count ->
        [one] { $count } action
       *[other] { $count } actions
    }

# The title of what the desktop shows of an action an error stopped while no window is up.
finishing_title_stopped = An action stopped

# The title of what the desktop shows while an action waits for its server and no window is up.
finishing_title_waits_for_server = Waiting for the server

# A font the system keeps apart and names for itself (macOS's interface font), said inside a sentence and after Automatic: Automatic (the system font).
fonts_system = the system font

# A button of a form of several steps: go back to the step before.
form_back = Back

# A button: close a form without answering it.
form_cancel = Cancel

# What a screen reader calls the list of a form's choices.
form_choices = Choices

# A form's list of choices, while none is chosen.
form_choose = Choose…

# A button at a form's end: fold its options again.
form_fewer_options = Fewer options

# What a screen reader calls the list of folders under a field where a file is saved.
form_folders = Folders

# The keys of a form.
form_keys = Enter to confirm, Escape to cancel

# A button at a form's end: unfold its other options; count: how many.
form_more_options = More options ({ $count })

# In a form's list of choices, when none matches what is typed.
form_nothing_matches = Nothing matches

# A form's field that must be filled in, by its label (Name), marked.
form_required = { $field } *

# Under a form's list of choices that shows some of them: which are in sight, of how many.
form_rows_shown = { $first }–{ $last } of { $count }: the arrows move further

# Under a field where a file is saved, a folder's name typed after the folder: the file goes in that folder, under the name it gets.
form_save_in_folder = Saved in the folder { $folder }, named { $name }

# Under a field where a file is saved, a folder typed: the name the file gets there.
form_save_named = Named { $name }

# Under a field where a file is saved: the folder typed is not there; path: the folder as it was typed.
form_save_no_folder = There is no folder { $path }

# Under a field where a file is saved: no file can have that name; why: what makes it no name (it holds a /).
form_save_refused = { $name } cannot be saved: { $why }

# Under a field where a file is saved: a file of that name is in that folder.
form_save_there_already = { $name } is there already: saving asks whether to replace it

# Under a field where a file is saved, when what is typed names no folder from the top; ~ is the home folder, as it is typed.
form_save_whole_path = Type the folder's whole path, from the top or from ~

# In the empty field of a password or a key Skwir keeps already: typing one replaces it.
form_secret_kept = Kept: type another to replace it

# The button at the end of a field that unfolds what it suggests.
form_suggestions = Suggestions

# In the empty field above a list of choices: what is typed narrows the list.
form_type_to_filter = Type to filter

# The button at the end of a field where a file is saved that unfolds the folders it offers.
form_unfold_folders = Folders

# The keys of a form while it waits (a sign-in in the browser).
form_waiting_keys = Escape to cancel

# A picture's size in pixels, width by height, alone: the list's Dimensions column, a preview's caption (4032 × 3024).
format_dimensions = { $width } × { $height }

# Two facts side by side on one line (a block's size, then its share), each its own words; a line of several says the first, then the rest the same way (480 MB · 24% of Photos · changed Today).
format_facts = { $first } · { $then }

# How long an action has left, under its bar. Begins in lower case.
format_left_few_seconds = a few seconds left

# How long an action has left, under its bar; h is the hour's symbol. Begins in lower case.
format_left_hours = about { $hours } h left

# How long an action has left, under its bar, by tens of minutes; h and min are the hour's and the minute's symbols. Begins in lower case.
format_left_hours_minutes = about { $hours } h { $minutes } min left

# How long an action has left, under its bar; min is the minute's symbol. Begins in lower case.
format_left_minutes = about { $minutes } min left

# How long an action has left, under its bar. Begins in lower case.
format_left_more_than_a_day = more than a day left

# How long an action has left, under its bar, by fives of seconds; s is the second's symbol. Begins in lower case.
format_left_seconds = about { $seconds } s left

# A count alone, beside what it counts: a board's column, a quick filter's value.
format_number = { $count }

# A speed: what is moved each second (23.4 MB/s).
format_speed = { $size }/s

# When a file changed, today: time is the time of day (Today 14:02).
format_today = Today { $time }

# How long an action took; h is the hour's symbol.
format_took_hours = { $hours } h

# How long an action took; h and min are the hour's and the minute's symbols.
format_took_hours_minutes = { $hours } h { $minutes } min

# How long an action took; min is the minute's symbol.
format_took_minutes = { $minutes } min

# How long an action took; min and s are the minute's and the second's symbols.
format_took_minutes_seconds = { $minutes } min { $seconds } s

# How long an action took; s is the second's symbol.
format_took_seconds = { $seconds } s

# When a file changed, in the last week: weekday is the day's name, time the time of day (Tuesday 18:45).
format_weekday_time = { $weekday } { $time }

# When a file changed, yesterday: time is the time of day (Yesterday 09:30).
format_yesterday = Yesterday { $time }

# What a screen reader calls the bar of buttons over a preview shown on the whole screen.
fullscreen_controls = Controls

# Said as a slideshow stops at the folder's last picture.
fullscreen_slideshow_ended = The slideshow ended at the last picture

# Said where a slideshow is asked of something that is no picture.
fullscreen_slideshow_pictures = A slideshow shows pictures

# A warning shown once a run, when text in a script the user reads cannot be drawn. The font is a family's name (Noto Sans Devanagari, Noto Sans CJK JP), written as it is.
glyphs_missing = No installed font has the letters of some text here, so they show as boxes. Install { $font } to read them: Skwir's guide names its package on each system.

# A warning shown once a run, when text in a script the user reads cannot be drawn and Skwir knows no font for it. The letter is a code point, written as Unicode writes it (U+1E900).
glyphs_missing_letter = No installed font has the letters of some text here (the first is { $letter }), so they show as boxes. Install a font that has them: Skwir's guide names Noto's packages on each system.

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_accessed_description = Under each name in the grid, when it was last read, as the system keeps it.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_accessed_title = Last access under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_created_description = Under each name in the grid, when it was made, where the system keeps it.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_created_title = Creation under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_deleted_description = Under each name in the grid, when it was put in the trash, where the trash's files are listed.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_deleted_title = Deletion under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_dimensions_description = Under each name in the grid, a picture's size in pixels, where a plugin reads it.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_dimensions_title = Dimensions under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_duration_description = Under each name in the grid, how long a sound or a film lasts, where a plugin reads it.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_duration_title = Duration under names

# A setting's description: whether each tile of the grid says this under its name. Recent is a place in the sidebar.
grid_caption_folder_description = Under each name in the grid, the folder it is in, where files from many folders are listed: a search, a tag, the shelf, Recent.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_folder_title = Folder under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_kind_description = Under each name in the grid, what kind of file it is, such as Image or Document.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_kind_title = Kind under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_modified_description = Under each name in the grid, when it last changed.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_modified_title = Modification under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_owner_description = Under each name in the grid, who owns it.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_owner_title = Owner under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_rating_description = Under each name in the grid, the stars it was given, where its notes are kept.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_rating_title = Rating under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_size_description = Under each name in the grid, the size of the file, or how many files a place holds.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_size_title = Size under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_tags_description = Under each name in the grid, a line of its tags, as many as fit, then dots.

# What the grid's tags caption setting says after its description, in its tooltip: the tags are not drawn over the picture.
grid_caption_tags_more = The picture is then left to itself.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_tags_title = Tags under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_taken_description = Under each name in the grid, when a picture or a film was taken, by its camera's clock, where a plugin reads it.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_taken_title = When taken under names

# A setting's description: whether each tile of the grid says this under its name.
grid_caption_type_description = Under each name in the grid, each file's type, such as PDF document.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_type_title = Type under names

# A setting's description: whether each tile of the grid says this under its name. Git is a program's name.
grid_caption_vcs_description = Under each name in the grid, what Git says of it in a repository: changed, staged, new, in conflict or ignored.

# A setting's name: whether each tile of the grid says this under its name.
grid_caption_vcs_title = Git state under names

# Why an undone action's Redo waits: it is being done again.
history_being_put_back = Being put back…

# Why an action's Undo waits: it is being taken back.
history_being_undone = Being undone…

# What the Undo history says while it holds nothing.
history_empty = Nothing to undo yet

# What a screen reader hears of the link to the row of the action that holds an Undo. title: that action, capitalised (Copy 3 items to Docs).
history_go_to = Go to { $title }

# Why an action's Undo waits: an action done after it touched what it did. title: that action, capitalised (Copy 3 items to Docs), a link to its row.
history_held_done = Undo { $title } first: it touched what this did

# Why an action cannot be undone: an action after it took what it did for good (deleted it). title: that action, capitalised (Copy 3 items to Docs), a link to its row.
history_held_for_good = { $title } took what this did for good

# Why an action's Undo waits: an action running touches what it did. title: that action, capitalised (Copy 3 items to Docs), a link to its row.
history_held_running = { $title } is running, and touches what this did

# Why an undone action's Redo waits: an action undone after it touched what it did. title: that action, capitalised (Copy 3 items to Docs), a link to its row.
history_held_undone = Redo { $title } first: it touched what this did

# Why an action's Undo waits: an action waiting its turn touches what it did. title: that action, capitalised (Copy 3 items to Docs), a link to its row.
history_held_waiting = { $title } waits its turn, and touches what this did

# The list of every action Undo can take back, opened above the footer's Undo.
history_name = Undo history

# Why an action has no Undo.
history_never = Cannot be undone

# Why the Undo of an answer (a toast's, Activity's) is held: what it did is no longer in the Undo history.
history_no_longer = No longer in the history

# The tooltip of the footer's Undo while there is nothing to take back.
history_nothing_to_undo = Nothing to undo

# Why an action's Undo waits: it runs, and is taken back once it ends.
history_once_ended = Taken back once it ends

# A button of an undone action of the Undo history: do it again.
history_redo = Redo

# What a screen reader hears of an action of the Undo history: its title, capitalised (Copy 3 items to Docs), and how it stands (running).
history_row_heard_state = { $title }, { $state }

# What a screen reader hears of an action of the Undo history: its title, capitalised (Copy 3 items to Docs), how it stands (running), and why its Undo waits.
history_row_heard_state_why = { $title }, { $state }, { $why }

# What a screen reader hears of an action of the Undo history: its title, capitalised (Copy 3 items to Docs), and why its Undo waits.
history_row_heard_why = { $title }, { $why }

# On a pill before the title of an action of the Undo history: it runs.
history_state_running = running

# On a pill before the title of an action of the Undo history: it was taken back.
history_state_undone = undone

# On a pill before the title of an action of the Undo history: it waits its turn.
history_state_waiting = waiting

# A button of an action of the Undo history: take it back.
history_undo = Undo

# The footer's button that takes back the latest action.
history_undo_footer = Undo

# The tooltip of the footer's Undo. title: the action it takes back (copy 3 items to Docs).
history_undo_tip = Undo { $title }

# The tooltip of the footer's Undo while the action it takes back runs. title: the action (copy 3 items to Docs).
history_undo_tip_once = Undo { $title } (once it ends)

# A button of an action of the Undo history: take it back with those done after it that touched what it did.
history_undo_with = { $count ->
        [1] Undo both
       *[other] Undo with { $count } after it
    }

# Why the Undo of an answer (a toast's, Activity's) is held: what it did is undone already.
history_undone_already = Undone already

# A menu item of a commit of a history: copy its hash.
history_view_copy_hash = Copy hash

# A menu item of a commit of a history: copy its short hash.
history_view_copy_short_hash = Copy short hash

# A menu item of a commit of a history: copy its summary.
history_view_copy_summary = Copy summary

# What a screen reader hears of a branch at a commit, checked out. name: the branch (main).
history_view_ref_current = { $name } (checked out)

# What a screen reader hears of a remote's branch at a commit. name: the branch (origin/main).
history_view_ref_remote = { $name } (remote)

# What a screen reader hears of a tag at a commit (a version's). name: the tag (v1).
history_view_ref_tag = tag { $name }

# What a screen reader hears of the branches and tags at a commit, each with what it is, joined (main (checked out) and tag v1).
history_view_refs = { $names }

# What was copied, said after Copied (Copied the commit's hash).
history_view_the_hash = the commit's hash

# What was copied, said after Copied (Copied the commit's short hash).
history_view_the_short_hash = the commit's short hash

# What was copied, said after Copied (Copied the commit's summary).
history_view_the_summary = the commit's summary

# A tooltip, then the keys that do the same (Undo copy 3 items to Docs (Ctrl+Z)).
history_with_keys = { $tip } ({ $keys })

# The name a screen reader hears of the badge on the picture of the file the player plays: a button that pauses it.
icons_badge_pause = Pause

# Pointing at the badge on the picture of the file the player paused.
icons_badge_paused_tip = Paused · click to play

# The name a screen reader hears of the badge on the picture of the file the player paused: a button that plays it on.
icons_badge_play = Play

# Pointing at the badge on the picture of the file the player plays.
icons_badge_playing_tip = Playing · click to pause

# What a screen reader hears after a file's name: the player paused it.
icons_plays_paused = paused

# What a screen reader hears after a file's name: the player plays it.
icons_plays_playing = playing

# Why a theme's icon is not drawn: its file is larger than a preview reads. size: that bound.
icons_too_large = Icon exceeds { $size } preview limit

# What a screen reader hears of a cell of a row in a preview's table: its column's header (Skwir's, or the file's own), then what it holds, as the file writes it.
inspector_cell_said = { $column }: { $cell }

# Said where a chapter typed is none of the book's. last: its last chapter's number.
inspector_chapter_refused = Enter a chapter number from 1 to { $last }

# A preview's button: fold every part that folds.
inspector_collapse_all = Collapse all

# A preview's button: open every part folded (a code view's, a document's).
inspector_expand_all = Expand all

# The placeholder of a preview's find field.
inspector_find = Find…

# The placeholder of the find field of a file's bytes: words, or bytes written in hexadecimal.
inspector_find_hex = Text, or hex: 00 ff 7a

# Under a preview's find field, before anything is found.
inspector_find_hint = Enter for the next match

# A preview's button: lines of text as wide as the panel, not the reading width chosen in Settings.
inspector_full_width = Full width

# Pointing at the chapter number between a book's previous and next buttons: a click lets one be typed.
inspector_go_to_chapter = Go to chapter

# What a screen reader hears of a row of a file's bytes: its offset, its bytes and what they read as, joined (00000010, 48 65 6C and Hel).
inspector_hex_row = { $cells }

# An entry of a Git history's contents: the branches and tags of a commit, joined (main and v1.2), then its day (2024-03-01).
inspector_history_heading = { $names } · { $day }

# An entry of a Git history's contents: the branches and tags of a commit, joined (main and v1.2), where its day is not known.
inspector_history_names = { $names }

# A button of a file's bytes shown in hexadecimal, and the title of what it opens: go to the byte at a number typed.
inspector_jump = Jump to byte offset

# In a text preview's bar: how many lines the file has.
inspector_lines = { $count ->
        [one] { $count } line
       *[other] { $count } lines
    }

# In a text preview's bar while a large file's lines are counted: how many so far.
inspector_lines_so_far = { $count ->
        [one] { $count } line
       *[other] { $count } lines
    }…

# Pointing at a book's next button, counted by printed pages. ] is the key that does it too.
inspector_next_page = Next page · ]

# A book's next button, as a menu's row names it: counted by printed pages.
inspector_next_page_name = Next page

# Pointing at a preview's next button: the chapter, sheet or page of rows after. ] is the key that does it too.
inspector_next_section = Next section · ]

# A preview's next button, as a menu's row names it: the chapter, sheet or page of rows after.
inspector_next_section_name = Next section

# In a book's find field, asked for the match before: there is none in this chapter.
inspector_no_earlier_match = No earlier match in this chapter

# Said where what was typed to pick a labelled link names none.
inspector_no_label = No link has that label

# Said where links are asked to be labelled for keys and none is in view.
inspector_no_link = No link in view

# In a preview's find field: the words looked for are nowhere in what is shown (a sheet, a page of rows).
inspector_no_match = No match in this section

# Said where a page typed is none of the book's printed pages. first, last: its first and last printed page numbers.
inspector_page_printed_refused = Enter a page number printed in the book, from { $first } to { $last }

# Pointing at a book's previous button, counted by printed pages. [ is the key that does it too.
inspector_previous_page = Previous page · [

# A book's previous button, as a menu's row names it: counted by printed pages.
inspector_previous_page_name = Previous page

# Pointing at a preview's previous button: the chapter, sheet or page of rows before. [ is the key that does it too.
inspector_previous_section = Previous section · [

# A preview's previous button, as a menu's row names it: the chapter, sheet or page of rows before.
inspector_previous_section_name = Previous section

# Pointing at a preview's text size button and at each size it offers (100%).
inspector_text_size = Text size

# A choice of a preview's text size: the size Settings gives this kind of text.
inspector_text_size_default = As in Settings

# Said where a file inside a file inside a file is asked to be shown: the preview goes no deeper.
inspector_too_deep = Open that file itself to look deeper

# A choice of how a book's place is counted: by its chapters, not by the numbers printed on its pages.
inspector_unit_chapters = Chapters

# Beside an archive's preview: what its files weigh once unpacked.
inspector_unpacked = { $size } unpacked

# A text preview's menu row: break the lines too long for the panel.
inspector_wrap_lines = Wrap lines

# Where keys work: anywhere in the window.
keymap_anywhere = Anywhere

# A key that offers a list to choose from; title: what the list is for, as the binding names it. The ellipsis says it asks more.
keymap_choose = { $title }…

# Where keys work: the conditions that must all hold there, joined (one item selected and not in the trash); alternatives are joined by or (with a selection or in a folder).
keymap_conditions = { $conditions }

# Where keys work: while the window is another program's Open or Save dialog.
keymap_flag_choosing = while the window answers another program's dialog

keymap_flag_completes = while what is typed can be completed

keymap_flag_creates = where new places can be made

keymap_flag_dir = on a folder

keymap_flag_drops = where files can be dropped

keymap_flag_empty = nothing selected

keymap_flag_file = on a file

keymap_flag_filtered = filtered by name

# Where keys work: while a folder is shown, not a search or a tag.
keymap_flag_folder = in a folder

# Where keys work: while a preview's links show letters to follow them by.
keymap_flag_labelling = while links are labelled

# Where keys work: while a list shows under what it belongs to (a field, a dropdown).
keymap_flag_lists = while its list shows

keymap_flag_offers = while a field offers a row to take

keymap_flag_place = on a tag or another place

keymap_flag_planning = while a plan is worked out

keymap_flag_reference = in a field that says what it takes

# Where keys work: a list (a tag's, the shelf) whose Delete takes a file out of it, leaving the file.
keymap_flag_refs = where removing leaves files where they are

keymap_flag_restores = in the trash

keymap_flag_search = in a search

keymap_flag_selection = with a selection

# Where keys work: a condition, after the place (File list, one item selected).
keymap_flag_single = one item selected

keymap_flag_split = split view

keymap_flag_sweep = selecting as the cursor moves

# Where keys work: while what was typed in the file list to reach a file by its name shows at its foot.
keymap_flag_typing = while a name is typed to find a file

# A key that goes to the home folder.
keymap_go_home = Go to your home folder

# A key that goes to a folder; folder: its name.
keymap_go_to = Go to { $folder }

# A key's name on a keyboard, as the desktop shows it in shortcuts (Alt+Enter).
keymap_key_alt = Alt

# A key's name on a keyboard, as the desktop shows it in shortcuts: the key erasing before the caret.
keymap_key_backspace = Backspace

# A key's name on a keyboard, as the desktop shows it in shortcuts (Ctrl+Shift+N): Strg in German.
keymap_key_ctrl = Ctrl

# A key's name on a keyboard, as the desktop shows it in shortcuts (Shift+Delete): Entf in German, Suppr in French.
keymap_key_delete = Delete

# A key's name on a keyboard, as the desktop shows it in shortcuts: Ende in German, Fin in French.
keymap_key_end = End

# A key's name on a keyboard, as the desktop shows it in shortcuts (Ctrl+Enter): Entrée in French.
keymap_key_enter = Enter

# A key's name on a keyboard, as the desktop shows it in shortcuts: Escape, short (Échap in French).
keymap_key_escape = Esc

# A key's name on a keyboard, as the desktop shows it in shortcuts: the Fn key of a laptop.
keymap_key_fn = Fn

# A key's name on a keyboard, as the desktop shows it in shortcuts: a function key (F1, F2…). number: its number, as written on it.
keymap_key_function = F{ $number }

# A key's name on a keyboard, as the desktop shows it in shortcuts: Pos1 in German, Début in French.
keymap_key_home = Home

# A key's name on a keyboard, as the desktop shows it in shortcuts: Einfg in German, Inser in French.
keymap_key_insert = Insert

# A key's name on a keyboard, as the desktop shows it in shortcuts: the key that opens the context menu.
keymap_key_menu = Menu

# A key's name on a keyboard, as the desktop shows it in shortcuts: Bild ab in German.
keymap_key_page_down = Page Down

# A key's name on a keyboard, as the desktop shows it in shortcuts: Bild auf in German.
keymap_key_page_up = Page Up

# A key's name on a keyboard, as the desktop shows it in shortcuts (Ctrl+Shift+N): Umschalt in German.
keymap_key_shift = Shift

# A key's name on a keyboard, as the desktop shows it in shortcuts: the space bar (Ctrl+Space): Leertaste in German.
keymap_key_space = Space

# A key's name on a keyboard, as the desktop shows it in shortcuts: the key with the system's logo (Super+E).
keymap_key_super = Super

# A key's name on a keyboard, as the desktop shows it in shortcuts (Ctrl+Tab).
keymap_key_tab = Tab

# Where keys work: a condition that must not hold (not in the trash); condition: the condition.
keymap_not = not { $condition }

keymap_place_about = About Skwir

# A place in the window: the panel of the copies, moves and other work going on.
keymap_place_activity = Activity

keymap_place_address = Address bar

keymap_place_bulk = Bulk rename

# A place in the window: the row of the bulk rename that changes the names' case.
keymap_place_bulk_case = Bulk rename's case

keymap_place_choose = Choosing from a list

# A place in the window: the bar at its foot while it answers another program's Open or Save dialog.
keymap_place_chooser = Another program's dialog

keymap_place_confirm = Confirmation dialogs

keymap_place_form = Forms

keymap_place_fullscreen = Preview on the whole screen

keymap_place_image = Picture preview

keymap_place_inspector = Structured previews

keymap_place_map = Map preview

keymap_place_media = Media player

keymap_place_member = A file shown from an archive or a message

keymap_place_members = Files of an archive or a message

keymap_place_menu = Context menu

# A place in the window: any dialog that asks something (a question, a name, a form), wherever in it the keyboard is.
keymap_place_modal = Dialogs

keymap_place_model = 3D model preview

keymap_place_palette = Command palette

# A place in the window, as the list of shortcuts names where keys work: the list of files.
keymap_place_pane = File list

# A place in the window: the quick look at a file over the list, opened with Space.
keymap_place_peek = Peek

# A place in the window: the player kept playing while one browses.
keymap_place_player = Pinned player

keymap_place_preview = Documents and structured previews

keymap_place_preview_find = Preview search

keymap_place_reader = Document reader

# A place in the window: the dialog listing what a plan will do before it does it.
keymap_place_review = Plan review

keymap_place_settings = Settings

# A place in the window: the overview of shortcuts F1 opens.
keymap_place_shortcuts = Shortcut overview

keymap_place_sidebar = Sidebar

keymap_place_slideshow = Picture on the whole screen

keymap_place_tag_editor = Tag editor

keymap_place_terminal = Terminal

keymap_place_text = Text in the preview

# A place in the window: the strip of the files' dates.
keymap_place_timeline = Timeline

keymap_place_toolbars = Toolbars

# A place in the window: the buttons that choose the list, the grid or another view.
keymap_place_view_buttons = View buttons

# A place: a window showing one file's preview alone, opened from a terminal (skwir view).
keymap_place_viewer = File viewer

# A place in the window: the media player's volume slider.
keymap_place_volume = Volume

# Where keys work: a place, then the conditions that must hold there (File list (one item selected)).
keymap_place_with_conditions = { $place } ({ $conditions })

# Where a shortcut works, in the list of shortcuts: the places it works in, joined (File list, Sidebar and Address bar).
keymap_places = { $places }

# A command showing the folder in a view; view: the view's name (List, Grid, Columns).
keymap_view = { $view } view

# A row of the list of shortcuts holding the keys of every view (list, grid, columns).
keymap_views = Views

# The tooltip of a row's + on the Keyboard page: more keys for its action.
keys_add_tip = Add keys

# Where a new shortcut's keys work: in every place of the window.
keys_anywhere = Anywhere

# Why a new shortcut's command cannot run: argument is the name of one of its arguments, as its schema writes it (depth).
keys_arg_is_a_number = { $argument } is a number

# Why a new shortcut's command cannot run: argument is the name of one of its arguments, as its schema writes it (location).
keys_arg_needed = { $argument } is needed

# Why a new shortcut's command cannot run: argument is the name of one of its arguments, as its schema writes it; error why what was typed is no JSON.
keys_arg_unreadable = { $argument }: { $error }

# Leaves the shortcuts as stored for the Keyboard page's list.
keys_back_to_list = Back to the list

# A button of the Keyboard page's bar: close it, nothing changed.
keys_cancel = Cancel

# A key's tooltip on the Keyboard page: when its keys work (In a folder, with a selection), then what a click does.
keys_chip_conditions = { $conditions }. { $tip }

# The tooltip of a key of skwir's on the Keyboard page.
keys_chip_default = Click to change or remove.

# The tooltip of a key of skwir's on the Keyboard page that a shortcut of the user's took.
keys_chip_hidden = No longer runs it: your shortcut took these keys. Click to restore.

# The tooltip of a key the user gave an action on the Keyboard page.
keys_chip_yours = Yours. Click to change or remove.

# Why a new shortcut cannot be saved: no command was chosen for it.
keys_choose_first = Choose what the keys run first

# Shows the user's shortcuts as they are stored, each a record.
keys_edit_as_records = Edit as records

# Said while the Keyboard page's recorder takes keys: Escape is taken as a key, Cancel is the bar's button.
keys_esc_is_a_key = Esc is a key here too: Cancel is a click away.

# The placeholder of the Keyboard page's filter: what it finds an action by.
keys_filter = Filter by action or keys

# A button of the Keyboard page: press keys to list the actions they run.
keys_find = Find by keys

# Clears the keys the Keyboard page's list was narrowed to.
keys_find_clear = Clear

# A caption over the actions the settings' search found on the Keyboard page: place is where their keys work (File list).
keys_found_place = Keyboard: { $place }

# A button of the Keyboard page's bar: keys of skwir's no longer run their action.
keys_free = Free these keys

# The heading of the Keyboard page's bar: new keys for action (Quit), working in place (Anywhere, File list).
keys_heading_add = New keys for { $action } · { $place }

# The heading of the Keyboard page's bar: keys (Ctrl+Q) of action (Quit) changed, in place (Anywhere).
keys_heading_change = { $keys } for { $action } · { $place }

# The heading of the Keyboard page's bar while it takes the keys whose actions to list.
keys_heading_find = Find the actions of a key

# The heading of the Keyboard page's bar: action's keys as skwir ships them, in place.
keys_heading_reset = Back to Skwir's keys for { $action } · { $place }

# The heading of the Keyboard page's bar: keys of skwir's (Ctrl+Q) a shortcut of the user's took from action (Quit), in place.
keys_heading_restore = { $keys } no longer runs { $action } · { $place }

# A fold of the Keyboard page's list: the actions whose keys work inside dialogs and menus, and how many.
keys_inside = Keys inside dialogs and menus ({ $count })

# Why a change of keys is refused: action is what the keys run (Command palette), which must keep one.
keys_needs_a_key = { $action } needs a key: give it another first

# Why a change of keys is refused: action is what the keys run (Command palette), which must keep one in place, where keys work (File list, Address field).
keys_needs_a_key_in = { $action } needs a key in { $place }: give it another first

# A button of the new shortcut's form: choose another command for its keys.
keys_new_another = Another command

# A heading of the new shortcut's form: what its command is given, as its schema asks.
keys_new_arguments = Its arguments

# A button of the new shortcut's form: close it, nothing saved.
keys_new_cancel = Cancel

# A heading of the new shortcut's form: the keys it binds.
keys_new_keys = The keys

# Said beside an argument of a new shortcut's command that may be left out.
keys_new_optional = optional

# A button of the new shortcut's form: take the keys it binds.
keys_new_press = Press the keys…

# A heading of the new shortcut's form: the command its keys run.
keys_new_runs = What the keys run

# A button of the new shortcut's form: keep the shortcut.
keys_new_save = Save

# A button of the Keyboard page: keys for an action no row lists (a folder to go to).
keys_new_shortcut = New shortcut…

# A heading of the new shortcut's form: the place its keys work in.
keys_new_where = Where they work

# filter: what the user typed in the Keyboard page's filter.
keys_no_action_matches = No action matches { $filter }.

# filter: what the user typed in the Keyboard page's filter.
keys_no_changed_matches = No key you changed matches { $filter }.

# filter: what the user typed in the Keyboard page's filter.
keys_no_shortcut_matches = No shortcut matches { $filter }.

keys_none_changed = You changed no keys yet.

# The Keyboard page's list, narrowed to the keys pressed, is empty.
keys_none_has_keys = No action has these keys.

# Why the user's new keys were not saved, where the setting gave no reason.
keys_not_kept = These keys cannot be kept

# Said before new keys are saved: keys are the keys (Ctrl+Q), action what they keep running in place, under conditions the new keys do not ask for (File list (with a selection)).
keys_note_besides = { $keys } still runs { $action } in { $place }.

# Said before new keys are saved, anywhere: keys are the keys (Ctrl+Q), action what they keep running in place (File list).
keys_note_deeper = In { $place }, { $keys } still runs { $action }: a place's own keys come first.

# Said before new keys are saved, anywhere: keys are the keys (Ctrl+Q), count the places with keys of their own, some the first three of them, then how many more, joined (File list, Address field, Sidebar and 2 more).
keys_note_deeper_many = { $count ->
       *[other] { $keys } still runs what it runs in { $count } places that bind it themselves ({ $some }): a place's own keys come first.
    }

# The last item of the places that bind keys themselves, after the first three by name: how many others (File list, Address field, Sidebar and 2 more).
keys_note_deeper_more = { $count ->
        [one] { $count } more
       *[other] { $count } more
    }

# Said before new keys for one place are saved: keys are the keys (Ctrl+Q), action what they keep running in the rest of the window.
keys_note_elsewhere = Elsewhere, { $keys } still runs { $action }.

# Said before a key that types a character is bound: keys are the keys (Shift+A).
keys_note_fields = In a text field, { $keys } still types: a field keeps its characters.

# Said when the keys pressed for an action are already its keys there: keys are the keys (Ctrl+Q).
keys_note_has = It has { $keys } here already.

# Said before a change of keys is saved: keys (Escape) no longer run action (Clear the filter) in place (File list).
keys_note_lost = { $keys } also stops running { $action } ({ $place }).

# Said before new keys are saved: keys are the keys (Ctrl+Q), action what they run now (Quit), place where (Anywhere, File list).
keys_note_replaces = { $keys } runs { $action } now ({ $place }): yours replaces it there.

# Said before a letter is bound in the file list: typing there no longer jumps to a name; the / key still finds one.
keys_note_typing = A letter bound in the file list stops typing a name there to jump to it; / then finds a name.

# Said before new keys are saved: a shortcut of the user's on the same keys (keys, Ctrl+Q) runs action, whose plugin is off; it is kept.
keys_note_waiting = Your { $action } waits on { $keys } for its plugin: once it is back, the later of the two runs.

# Said before new keys are saved: a shortcut of the user's on the same keys (keys, Ctrl+Q) runs action, whose plugin is off; it is replaced.
keys_note_waiting_goes = Your { $action }, waiting on { $keys } for its plugin, goes: yours takes these keys there.

# A button of the Keyboard page's recorder: take the keys again from the first.
keys_press_again = Press again

# Why the bar of the Keyboard page saves nothing: no keys were pressed or typed.
keys_press_first = Press the keys first

# What the Keyboard page's recorder waits for after a first key: the next of a sequence (g then h).
keys_press_next = then the next key…

# What the Keyboard page's recorder waits for: the keys to give an action.
keys_press_prompt = Press the keys…

# What the Keyboard page's recorder waits for: the keys whose actions to list.
keys_press_to_find = Press the keys to find…

# A button of the Keyboard page's bar: the user's keys for an action go.
keys_remove = Remove these keys

# Said in the Keyboard page's bar before an action's keys go back to those skwir ships.
keys_reset_detail = Its keys go back to Skwir's, and your shortcuts that took them go.

# A button of the Keyboard page's bar: an action's keys as skwir ships them.
keys_reset_save = Back to Skwir's keys

# The tooltip of a row's revert button on the Keyboard page: its action's keys as skwir ships them.
keys_reset_tip = Back to Skwir's keys

# A button of the Keyboard page's bar: keys of skwir's run their action again.
keys_restore = Restore

# Said in the Keyboard page's bar before keys of skwir's the user freed are restored.
keys_restore_freed = You freed these keys. Restoring gives them back.

# Said in the Keyboard page's bar before keys of skwir's are restored: a shortcut of the user's for action (Quit) has them.
keys_restore_hider = Your shortcut for { $action } took these keys. Restoring removes it.

# A button of the Keyboard page's bar: keep the keys pressed.
keys_save = Save

# The placeholder of the field a new shortcut's command is looked for in.
keys_search_commands = Search commands

# A choice of what the Keyboard page lists: every action, those without keys too.
keys_show_all = All actions

# A choice of what the Keyboard page lists: the actions whose keys the user changed.
keys_show_changed = Changed by you

# A choice of what the Keyboard page lists: the actions whose keys the user changed, and how many.
keys_show_changed_count = Changed by you ({ $count })

# A choice of what the Keyboard page lists: the actions that have keys.
keys_show_keyed = With keys

# A button of the Keyboard page's recorder: one key more after those taken, a sequence (g then h).
keys_then_another = Then another key

# The placeholder of the field keys are typed in: ctrl-shift-n and g h are keys as the keymap writes them, kept as they are.
keys_type_placeholder = ctrl-shift-n, or g h for one key after another

# A button of the Keyboard page's recorder: write the keys rather than press them (those the desktop keeps).
keys_type_them = Type them

keys_waiting = { $count ->
        [1] One of your shortcuts waits for a plugin that is off: it comes back with it.
       *[other] { $count } of your shortcuts wait for plugins that are off: they come back with them.
    }

# The Keyboard page's caption over the actions no key runs, as a place's name stands over its keys.
keys_without_keys = Without keys

# The Split toggle's tooltip while split: the pane it closes.
layout_close_left = Close the left pane

# The Split toggle's tooltip while split: the pane it closes.
layout_close_lower = Close the lower pane

# The Split toggle's tooltip while split: the pane it closes.
layout_close_right = Close the right pane

# The Split toggle's tooltip while split: the pane it closes.
layout_close_upper = Close the upper pane

# value: what ui.layout was given for an archive's file's preview, as given (right, bottom, auto).
layout_not_member = { $value } is not a place for a file's preview in an archive

# value: what ui.layout was given for the panes, as given (row, column, auto).
layout_not_panes = { $value } is not a way to arrange panes

# value: what ui.layout was given for the preview, as given (right, bottom, auto).
layout_not_preview = { $value } is not a place for the preview

# The Split toggle's tooltip: what it makes.
layout_split_rows = Split panes one above the other

# The Split toggle's tooltip: what it makes.
layout_split_side = Split panes side by side

# A link of a document inside an archive names a folder of it; path: the folder's path in it; container: the archive's name.
links_inside_folder = { $path } is a folder of { $container }

# A link of a document inside an archive names nothing there; container: the archive's name; path: the path the link names in it.
links_inside_missing = { $container } holds no { $path }

# A link of a document inside an archive; container: the archive's name.
links_inside_out = This link leads out of { $container }

# A link of a document inside an archive, followed where the archive's files are not listed above the document; container: the archive's name.
links_inside_unlisted = This link names a file inside { $container }, whose files are not listed here to follow it

# A link names a place in a document that has none of that name; file: the document's name; anchor: the place's name, as the link writes it.
links_no_anchor = { $file } has no { $anchor }

links_no_file = That link does not name a file

# A document's link names a file that is missing; name: the file's name.
links_not_there = { $name } is not there

links_nowhere = This link goes nowhere we can open

links_only_addresses = Only web and mail links open from here

# error: why the desktop could not open a web or mail address.
links_open_failed = Could not open the link: { $error }

# A setting's description: whether the list has the Accessed column.
list_column_accessed_description = Show when each item was last read, as the system keeps it.

# A setting's description: whether the list has the Created column.
list_column_created_description = Show when each item was made, where the system keeps it.

# A setting's description: whether the list has the Deleted column.
list_column_deleted_description = Show when each file was put in the trash, where the trash's files are listed.

# A setting's description: whether the list has the Dimensions column.
list_column_dimensions_description = Show the size of pictures in pixels, where a plugin reads it of them.

# A setting's description: whether the list has the Duration column.
list_column_duration_description = Show how long sounds and films last, where a plugin reads it of them.

# A setting's description: whether the list has the Folder column. Recent is a place in the sidebar.
list_column_folder_description = Show the folder each file is in, where files from many folders are listed: a search, a tag, the shelf, Recent.

# What the Folder column's setting says after its description, in its tooltip: in a narrow pane.
list_column_folder_more = It is the last column to give way.

# A setting's description: whether the list has the Kind column.
list_column_kind_description = Show what kind of file each is, such as Image or Document.

# A setting's description: whether the list has the Modified column.
list_column_modified_description = Show when each item last changed.

# A setting's description: whether the list has the Owner column.
list_column_owner_description = Show who owns each item.

# A setting's description: whether the list has the Rating column.
list_column_rating_description = Show the stars each file was given, where its notes are kept.

# What the Rating column's setting says after its description, in its tooltip.
list_column_rating_more = It is the first column to give way in a narrow pane.

# A setting's description: whether the list has the Size column.
list_column_size_description = Show the size of files.

# A setting's description: whether the list has the Taken column.
list_column_taken_description = Show when pictures and films were taken, by their camera's clock, where a plugin reads it of them.

# A setting's description: whether the list has the Type column.
list_column_type_description = Show each file's type, such as PDF document.

# A setting's description: whether the list has the Git column. Git is a program's name.
list_column_vcs_description = Show what Git says of each file in a repository: changed, staged, new, in conflict or ignored, and of a folder what it holds.

# A column's header while the rows are sorted by it, the arrow pointing up: smallest, oldest or A first.
list_header_ascending = { $label } ▲

# A column's header while the rows are sorted by it, the arrow pointing down: largest, newest or Z first.
list_header_descending = { $label } ▼

# The Type of a place listed (a bookmark, a device) that does not say what it is.
list_type_place = Place

# A button of the banner above a location being opened: stop opening it.
loading_cancel = Cancel

# Pointing at the Cancel button of the banner above a location being opened.
loading_cancel_back = Stop opening it and go back

# Pointing at the Cancel button of the banner above a location being opened.
loading_cancel_home = Stop opening it and go to the home folder

# A button of the banner above a remote folder whose server stopped answering.
loading_disconnect = Disconnect

# Pointing at the Disconnect button. Eject is the button of a place in the sidebar.
loading_disconnect_tip = Let go of the server, as the sidebar's Eject does

# A button of the banner above a location that takes long: leave it for the one before.
loading_go_back = Go Back

# Pointing at the Go Back button of the banner above a location that takes long.
loading_go_back_tip = Back

# A button of the banner above a location that takes long: leave it for the home folder.
loading_go_home = Go Home

# Pointing at the Go Home button of the banner above a location that takes long.
loading_go_home_tip = Go to the home folder

# A banner above a folder being read.
loading_location = Loading location…

# A banner above a folder being read: how many of its items were read so far.
loading_location_read = Loading location… { $count ->
        [one] { $count } item
       *[other] { $count } items
    }

# A banner above a remote folder whose server stopped answering. name: the server's or the place's name.
loading_not_answering = { $name } is not answering…

# A banner above a location being opened that is no folder (an address connecting). place: its title.
loading_opening = Opening { $place }…

# A banner above a folder listed in part: what stopped the rest. error: why, as the system said it.
loading_partly_failed = Some of this location could not be read: { $error }

# A banner above a search while it is still looking.
loading_searching = Searching…

# Pointing at the button of a map that chooses what is drawn under its places (streets, a satellite's pictures).
map_background = Map background

# A file that holds its own map tiles (MBTiles, PMTiles) could not be read. error: why, as the reader said it.
map_file_tiles_failed = This file's map tiles could not be read: { $error }

# What a screen reader hears a map holds: its lines, places and areas counted, joined (2 lines and 5 places).
map_holds = { $holds }

# What a screen reader hears a map holds: its areas (shapes drawn filled).
map_holds_areas = { $count ->
        [one] { $count } area
       *[other] { $count } areas
    }

# What a screen reader hears a map holds: its lines (tracks, routes).
map_holds_lines = { $count ->
        [one] { $count } line
       *[other] { $count } lines
    }

# What a screen reader hears a map holds where nothing of its file has a place on the earth.
map_holds_nothing = Nothing with a position

# What a screen reader hears a map holds: its places (points).
map_holds_places = { $count ->
        [one] { $count } place
       *[other] { $count } places
    }

# The last row of the map background's choices, which opens their settings.
map_manage_backgrounds = Manage backgrounds…

# Pointing at the last row of the map background's choices, which opens their settings.
map_manage_backgrounds_tip = Manage backgrounds

# A hint, as the map is zoomed in close on a background that draws no streets.
map_no_streets = Streets are not drawn on this map background; another one can show them

# A map preview's menu row: show the place in the desktop's maps application.
map_open_external = Open in Maps

# Open in Maps failed: no application of the desktop takes a geo: address. error: why.
map_open_failed = No application opens map locations: { $error }

# What a screen reader calls a point of the map that has no name: where a track ends.
map_point_end = End

# What a screen reader hears of the point of the map reached: its name, then which of how many it is.
map_point_of = { $name }, { $index } of { $count }

# What a screen reader calls a point of the map that has no name: where a picture was taken.
map_point_picture = Picture

# What a screen reader calls a point of the map that has no name: a place.
map_point_place = Place

# What a screen reader hears as the map reaches a point: the point (its name, which of how many), then where it is (45.0000° N, 5.0000° E).
map_point_said = { $point }, { $at }

# What a screen reader calls a point of the map that has no name: where a track starts.
map_point_start = Start

# What a screen reader calls a point of the map that has no name: a stop along a track.
map_point_stop = Stop

# What a screen reader calls a point of the map that has no name: a recorded track (GPX).
map_point_track = Track

# What a screen reader calls the preview of a file with places (GPX, KML, GeoJSON). name: its file's name.
map_said = Map: { $name }

# What a screen reader hears of a map's preview: what it holds (2 lines, 5 places), then what its keys do.
map_said_holds = { $holds }. PageDown and PageUp go from place to place, arrows move the map, the zoom keys zoom it, Ctrl+0 fits it again

# The map's background could not be loaded. error: why, as it was said.
map_tiles_failed = Map tiles could not be loaded: { $error }

# Why a map's tile could not be shown: what the file or the server gave for it is no picture. Said after what could not be loaded.
map_tiles_not_a_picture = the tile is not a picture

# A button of the Map view: search for the files made in the area the map shows.
map_view_area = This area

# Pointing at the Map view's This area button.
map_view_area_tip = Search the pictures and videos of this folder and its subfolders made in the area shown

# What a screen reader calls the Map view's map.
map_view_figure = Map

# What a screen reader hears of the Map view's map: what its header says (3 files placed), then what its keys do.
map_view_figure_said = { $said }. PageDown and PageUp go from place to place

# Pointing at the Map view's Fit button. Home is the key that does it too.
map_view_fit = Fit the map to its files · Home

# Pointing at the Map view's button while the files made at one place are spread around it.
map_view_gather = Group the files at one place into a count again

# The Map view's placeholder where no file of the folder has a place on the earth.
map_view_nothing = Nothing here says where it is

# Under the Map view's placeholder where no file of the folder has a place on the earth.
map_view_nothing_detail = No picture or track of this location has a position. Pictures keep one when the camera knew where it was.

# The Map view's header once it read the folder's files: how many it put on the map.
map_view_placed = { $placed ->
        [one] { $placed } file placed
       *[other] { $placed } files placed
    }

# The Map view's header once it read the folder's files: how many it put on the map, then how many say nothing of where they were made.
map_view_placed_without = { $placed ->
        [one] { $placed } file placed
       *[other] { $placed } files placed
    } · { $without } without a position

# The Map view's header while it reads where the folder's files were made. placed: how many it put on the map so far; looked: how many of the pictures and tracks it reads it has read; candidates: how many it reads in all.
map_view_placing = Placing… { $placed ->
        [one] { $placed } file placed
       *[other] { $placed } files placed
    } · { $looked ->
        [one] { $looked } of { $candidates } looked at
       *[other] { $looked } of { $candidates } looked at
    }

# The button of the Map view's placeholder: look for pictures and tracks in the folders inside too.
map_view_search_subfolders = Search the subfolders

# Pointing at the Map view's button while the files made at one place are one mark with their count.
map_view_spread = Spread the files at one place around it, in the order they were taken

# A button of the Map view: put the files of the folders inside on the map too.
map_view_subfolders = Subfolders

# Pointing at the Map view's Subfolders button.
map_view_subfolders_tip = Show the pictures and tracks of the subfolders too

# The title of a film's sound tracks, among which one is chosen (its languages).
media_audio_tracks = Audio

# A player's menu row: start playing a sound or a film as soon as it is selected.
media_autoplay = Play on selection

# Pointing at the button of what a player does when a track ends. mode: Stop, Play next or Shuffle.
media_continue_tip = When a track ends: { $mode }

# The title of the choices of what a player does when a track ends.
media_continue_title = When a track ends

# A player's menu row: go on playing when another file is selected.
media_keep_playing = Keep playing while you browse

# A button of a player's volume.
media_mute = Mute

# What a screen reader hears of the volume slider while the sound is muted.
media_muted = Muted

# A button of the player.
media_next_track = Next track

# A button of the player.
media_play_pause = Play / pause

# What a screen reader calls a player's seek bar.
media_position = Position

# What a screen reader hears of a player's seek bar: where it is (1:02), of how long the track lasts (3:45).
media_position_of = { $position } of { $duration }

# A button of the player.
media_previous_track = Previous track

# A player's menu row: how fast it plays, chosen among 0.5×, 1×, 2×.
media_speed = Playback speed

# A playback speed among the player's choices: speed is how many times as fast (0.5×, 1×, 1.25×).
media_speed_value = { $speed }×

# The title of a film's subtitles, among which one is chosen, or none.
media_subtitles = Subtitles

# The choice of no subtitles for a film.
media_subtitles_off = Off

# A button of a player's volume, while its sound is muted.
media_unmute = Unmute

# A player's speaker button, as a menu's row names it: it opens the player's volume.
media_volume = Volume

# Pointing at a player's speaker button: how loud it plays, in percent (80%).
media_volume_level = Volume · { $percent }%

# Pointing at a player's speaker button while its sound is muted.
media_volume_muted = Volume · muted

# Over a player's volume slider, and what a screen reader hears of it: how loud it plays, in percent (80%).
media_volume_percent = { $percent }%

menu_close_tab = Close Tab

# What was copied, said after “Copied”: a link's web address.
menu_copied_link = the link's address

# What was copied, said after “Copied”: a file's path.
menu_copied_path = the path

menu_copy_link = Copy link address

# An item of a menu: copy where the file is, as text.
menu_copy_path = Copy path

# An item of the terminal's menu on a link.
menu_follow_link = Follow link

menu_follow_link_tab = Follow link in a new tab

# An item of the terminal's menu on a link whose text says something else than where it leads; target: where it leads (an address, a path), as it is.
menu_follow_link_to = Follow link to { $target }

# An item of the menu of a group's heading: hide its files under the heading.
menu_group_fold = Fold

# An item of the menu of a group's heading: hide the files of every other group.
menu_group_fold_others = Fold the others

# An item of the menu of a group's heading: filter the folder down to its files.
menu_group_only = Show only these

# An item of the menu of a group's heading: select its files.
menu_group_select = Select these

# An item of the menu of a group's heading in a folder grouped by something: show its files again.
menu_group_unfold = Unfold

# An item of the menu of a group's heading: show the files of every group.
menu_group_unfold_all = Unfold all

# The last item of a file's menu while the properties are shown over the preview.
menu_hide_properties = Hide Properties

# What a screen reader calls a menu opened on a file, a place or a tab.
menu_name = Context menu

menu_new_tab = New Tab

# An item of a sidebar place's menu: move it below the one after it.
menu_place_down = Move Down

# An item of a device's menu in the sidebar: unmount it, ready to be unplugged.
menu_place_eject = Eject

menu_place_hide = Hide From Sidebar

# An item of a sidebar place's menu: show it in the pane.
menu_place_open = Open

menu_place_open_tab = Open in New Tab

menu_place_open_window = Open in New Window

menu_place_remove = Remove From Sidebar

# An item of a sidebar place's menu: give it another name.
menu_place_rename = Rename…

# An item of a sidebar place's menu: move it above the one before it.
menu_place_up = Move Up

# An item of the preview's menu: copy the text selected.
menu_preview_copy = Copy

# An item of the preview's menu: find a text in what it shows.
menu_preview_find = Find…

menu_preview_select_all = Select all

# The last item of a file's menu: what it is, its size, its dates and more.
menu_properties = Properties

menu_reopen_tab = Reopen Closed Tab

# An item of the menu of the list's header: give every column its width back.
menu_reset_widths = Reset widths

# An item of the menu of files in the trash: put them back where they were.
menu_restore = Restore

menu_tab_to_window = Move to New Window

# An item of the terminal's menu: forget what scrolled out of sight.
menu_terminal_clear = Clear the history

# An item of the terminal's menu: copy the text selected.
menu_terminal_copy = Copy

# An item of the terminal's menu: paste the clipboard's text.
menu_terminal_paste = Paste

menu_terminal_select_all = Select all

# Drawn over a 3D model's view while its shape is sent to the graphics card.
model_loading = Loading…

# A 3D model preview's button: draw it in perspective, the far smaller than the near, or not.
model_perspective = Perspective

# What a screen reader calls the preview of a 3D model. name: its file's name.
model_said = 3D model: { $name }

# What a screen reader hears of the preview of a 3D model: how many triangles its shape has, then what its keys do.
model_said_holds = { $count ->
        [one] { $count } triangle
       *[other] { $count } triangles
    }. Arrows turn it, Shift and the arrows move it, the zoom keys zoom it, Ctrl+0 shows it as it opened

# A 3D model preview's button: shade its faces smoothly rather than each flat.
model_smooth = Smooth shading

# Drawn over a 3D model's view whose shape the graphics card (GPU) cannot hold.
model_too_large = Too large for this GPU to draw

# In the preview of a 3D model whose file could not be read.
model_unreadable = The model could not be read

# The last row of the menu of the folders inside a place, when it lists only some: how many it leaves out; choosing it opens the place.
nav_menu_more = { $count ->
        [one] { $count } more: open it
       *[other] { $count } more: open it
    }

# The menu of the folders inside a place, when it holds none.
nav_menu_nothing_inside = Nothing inside

# The menu of the folders inside a place could not be made; name: the place's name; error: why.
nav_menu_unreadable = Could not read { $name }: { $error }

# The button of a footnote's popup in a document's preview: go to the note where the document has it.
note_popover_go = Go to note

# The address field's card: the search runs as words are typed.
omnibox_as_you_type = As you type

# The address field's card while words filter a folder; shown: how many items they keep; count: how many the folder lists.
omnibox_as_you_type_of = As you type: { $shown } of { $count ->
        [one] { $count } item
       *[other] { $count } items
    }

# What a screen reader calls a side of the switch of how deep words look: the folder shown and every folder in it.
omnibox_depth_below = This folder and its subfolders

# A side of the switch of how deep words look, on the address field's card: the folders in it too.
omnibox_depth_below_label = And subfolders

# What a screen reader calls the side of the switch of how deep words look that is chosen; depth: that side.
omnibox_depth_chosen = { $depth }, chosen

# What a screen reader calls a side of the switch of how deep words look: the folder shown alone.
omnibox_depth_here = This folder only

# A side of the switch of how deep words look, on the address field's card: the folder shown alone.
omnibox_depth_here_label = This folder

# The address field's card: the search runs once Enter is pressed.
omnibox_enter_searches = Enter searches

# Under the address field's list, nothing typed: what it takes; > begins a command's name.
omnibox_footer_nothing = A path (/ or ~), a place, or words to filter; > for a command.

# Under the address field's list, nothing typed: what it takes; keys: the key that searches the folders below; > begins a command's name.
omnibox_footer_nothing_keys = A path (/ or ~), a place, or words to filter; { $keys } searches below; > for a command.

# Under the address field's list while a scope is typed (@).
omnibox_footer_scope = Enter searches there.

# Under the address field's list, nothing typed after the scope (a folder, searched below).
omnibox_footer_scoped_below = Words search there and below; Backspace takes the scope away, @ lists others; > for a command.

# Under the address field's list, nothing typed after the scope (a tag's place).
omnibox_footer_scoped_depths = Words filter what is shown; Backspace takes the scope away, @ lists others; > for a command.

# Under the address field's list, nothing typed after the scope (a folder, filtered).
omnibox_footer_scoped_here = Words filter this folder; Backspace takes the scope away, @ lists others; > for a command.

# Under the list of where words search.
omnibox_footer_scopes = Enter searches there; Escape keeps the scope.

# The line under the address field's list: what only a search can tell, then what the keys do.
omnibox_footer_waits = { $waits } { $hints }

# The address field's card: how many files the search as typed found.
omnibox_found = { $count ->
        [one] { $count } found
       *[other] { $count } found
    }

# A row of the address field's list: go where what is typed leads; text: as typed.
omnibox_go_to = Go to { $text }

# A row of what the address field takes, beside >, which opens the palette.
omnibox_help_command = A command, by its name, in the palette

# A row of what the address field takes, beside the beginnings of a path (~/ / ./).
omnibox_help_path = A path; Tab completes its names

# A row of what the address field takes, beside addresses of places (trash:, recent:).
omnibox_help_place = A place, by its address

# A row of what the address field takes, beside @, which lists where words search.
omnibox_help_scope = Where words search: the folder above, home, a tag…

# A row of what the address field takes, beside words.
omnibox_help_words = Search where the scope says; they filter the folder at once

# A row of what the address field takes, in place of an example: any words typed.
omnibox_help_words_label = words

# A hint under the address field's list; keys: the key that completes what is typed (Tab).
omnibox_hint_completes = { $keys } completes

# A hint under the address field's list: Escape leaves the field.
omnibox_hint_escape = Escape goes back

# A hint under the address field's list while words are typed: Enter goes to the files they keep.
omnibox_hint_files = Enter goes to the files

# A hint under the address field's list while words are typed; keys: the key that searches the whole home folder.
omnibox_hint_home = { $keys } searches the home folder

# A hint under the address field's list while a path is typed; keys: the key that opens the file.
omnibox_hint_opens = { $keys } opens a file

# A hint under the address field's list while words are typed.
omnibox_hint_searches = Enter searches

# A hint under the address field's list while a path is typed; keys: the key that searches the folder named.
omnibox_hint_searches_folder = { $keys } on a folder searches in it

# A hint under the list of what the address field takes: Enter writes the example.
omnibox_hint_writes = Enter writes it

# A hint under the list of what the address field takes: Enter, or the key named (Tab), writes the example.
omnibox_hint_writes_or = Enter or { $keys } writes it

# The line under the address field's list: what the keys do, a list of hints (Tab completes, Ctrl+O opens a file and Escape goes back).
omnibox_hints = { $hints }.

# At the end of the highlighted row of the address field's list: Enter runs it, and the key that completes it (Tab).
omnibox_keys_completes = Enter · { $keys }

# At the end of the highlighted row of the address field's list: the key that runs it.
omnibox_keys_enter = Enter

# At the end of a recent search's row: the key named forgets it.
omnibox_keys_forgets = Enter · { $keys } forgets it

# At the end of a file's row in the address field's list: Enter shows it, the key named opens it.
omnibox_keys_opens = Enter · { $keys } opens

# At the end of a folder's or a place's row: the key named searches in it.
omnibox_keys_searches = Enter · { $keys } searches in it

# At the end of a search's row that cannot run yet: Enter waits for it.
omnibox_keys_waits = Enter waits

# At the end of a row of what the address field takes: Enter, or the key named (Tab), writes its example.
omnibox_keys_writes = Enter · { $keys } writes it

# A path typed in the address that starts from the folder shown (./, ../) where none is (a search, the trash); path: as typed.
omnibox_no_folder = { $path } starts from the folder shown, and no folder is shown

omnibox_no_home = The home folder is not known

# A path typed in the address names nothing; name: its last part.
omnibox_not_there = { $name } is not there

# The empty address field, while words filter a place; scope: the place (a folder's name).
omnibox_placeholder_filter = Filter { $scope }, or type a path

# The empty address field: what it takes.
omnibox_placeholder_filter_any = A path, a place, or words to filter

# The empty address field, while words search a place and its folders; scope: the place.
omnibox_placeholder_search = Search { $scope } and below, or type a path

# The empty address field: what it takes.
omnibox_placeholder_search_any = A path, a place, or words to search

# The address field's card while a folder takes a moment to read (a disk asleep); folder: its path.
omnibox_reading = Reading { $folder }…

# What a screen reader calls a row of the address field's list: its title, then what is said under it.
omnibox_row_named = { $title }, { $detail }

# Under a folder's name in the list of where words search: the one that holds the folder shown.
omnibox_scope_above = The folder above

# Under Whole disk in the list of where words search.
omnibox_scope_disk = Every folder, by the system's index where it has one

# Under a folder's name in the list of where words search.
omnibox_scope_shown = The folder shown

# What a screen reader calls the list under the address field.
omnibox_suggestions = Suggestions

# What a screen reader calls the token before the address field's text while words filter a place; scope: the place.
omnibox_token_filtering = Filtering { $scope }

# What a screen reader calls the token before the address field's text while words search a place and its folders; scope: the place.
omnibox_token_searching = Searching { $scope } and below

# Under the address field's list while words filter: what they name that only a search can tell, a list (what files contain, when pictures were taken: query_waits_*).
omnibox_waits = Only a search can tell { $what }.

# Under the address field's list while words filter a folder: what they name that only a search can tell, a list (what files contain, when pictures were taken: query_waits_*), and the key that searches.
omnibox_waits_keys = Only a search can tell { $what }: { $keys }.

# Where a search looks, the root of the file system: every folder.
omnibox_whole_disk = Whole disk

# A button of the drawer of a preview's contents.
outline_close = Close contents

# In the drawer of a preview's contents, where the document or text has no headings.
outline_empty = No contents

# A button of the drawer of a preview's contents: keep it open from file to file.
outline_pin = Keep contents open

palette_about = About Skwir

# The label before the palette's field while it asks for an argument; command: the command's name; argument: the argument's name (both identifiers).
palette_asks = { $command } · { $argument }

palette_command_palette = Command palette

# What a screen reader calls the palette's list of commands found.
palette_commands = Commands

palette_contents_pin = Preview: keep contents open

palette_filter = Filter…

palette_find = Find a name…

palette_find_backward = Find a name before the cursor…

palette_find_next = Find the next name

palette_find_previous = Find the previous name

palette_focus_address = Focus address bar

palette_focus_next_part = Focus next part of the window

palette_focus_other_pane = Focus other pane

palette_focus_pane = Focus file list

palette_focus_sidebar = Focus sidebar

palette_full_width = Toggle full-width reading

palette_grid_fill = Toggle filled thumbnails

palette_history_menu = Back: history…

palette_history_menu_forward = Forward: history…

# A context menu's item in the palette, named by its submenu (Open With) and its own title.
palette_in_submenu = { $submenu } › { $item }

palette_inspect_auto = Preview: default presentation

palette_inspect_hex = Preview: hexadecimal

palette_inspect_links = Preview: follow a link…

palette_inspect_source = Preview: source / rendered

palette_inspect_turn = Preview: 3D / picture

palette_inspect_whole = Preview: full preview

palette_jump = Jump…

# The label before the palette's field, where a command's name is typed.
palette_label = Command

palette_layout_member_auto = File in an archive: place automatically

palette_layout_member_bottom = File in an archive: at the bottom

palette_layout_member_right = File in an archive: on the right

palette_layout_panes_auto = Panes: arrange automatically

palette_layout_panes_column = Panes: one above the other

palette_layout_panes_row = Panes: side by side

palette_layout_preview_auto = Preview: place automatically

palette_layout_preview_bottom = Preview: at the bottom

palette_layout_preview_right = Preview: on the right

palette_map_open = Map: open in Maps

palette_map_view_fit = Map: fit to its files

palette_map_view_search_area = Map: search the pictures and videos of this area

palette_map_view_spread = Map: spread the files at one place, or group them

palette_media_autoplay = Player: play on selection

palette_media_continue_next = Player: play the next track when one ends

palette_media_continue_off = Player: stop when a track ends

palette_media_continue_shuffle = Player: shuffle

palette_media_keep_playing = Player: keep playing while you browse

palette_media_next = Player: next track

palette_media_pin = Player: pin or let go of this track

palette_media_play = Player: play the selection

palette_media_previous = Player: previous track

palette_mkdir_prompt = New folder…

palette_model_perspective = Model: perspective

palette_model_smooth = Model: smooth shading

# What a screen reader calls the palette's panel.
palette_name = Command palette

palette_new_file_prompt = New file…

palette_none = No matching command

palette_peek = Peek

palette_placeholder = Type a command

palette_player_play_pause = Player: play or pause

palette_player_stop = Player: stop and let go

palette_preview_close = Preview: close

palette_preview_contents = Preview: contents

palette_preview_copy = Preview: copy

palette_preview_copy_path = Preview: copy path

palette_preview_find = Preview: find text

palette_preview_fullscreen = Preview: full screen

palette_preview_goto = Preview: go to page or chapter…

palette_preview_match_next = Preview: next search match

palette_preview_match_previous = Preview: previous search match

palette_preview_open_external = Preview: open externally

palette_preview_page_next = Preview: next page

palette_preview_page_previous = Preview: previous page

palette_preview_seek_back = Preview: back five seconds

palette_preview_seek_forward = Preview: forward five seconds

palette_preview_select_all = Preview: select all

palette_preview_zoom_actual = Preview: actual size (100%)

palette_preview_zoom_in = Preview: zoom in

palette_preview_zoom_out = Preview: zoom out

palette_preview_zoom_reset = Preview: fit

palette_preview_zoom_width = Preview: fit width

palette_properties = Toggle properties

palette_quick_filters = Toggle Quick filters

palette_quit = Quit

palette_reader_continuous = Document: toggle continuous scrolling

palette_reader_drag = Document: toggle dragging to move the page

palette_rename_bulk_prompt = Rename several…

palette_rename_prompt = Rename…

palette_search = Search…

# A command of the palette opening a setting; page: the settings page it is on; setting: its title.
palette_setting = Settings: { $page } › { $setting }

# A command of the palette opening a setting; page: the settings page it is on; section: the section of the page; setting: its title.
palette_setting_in_section = Settings: { $page } › { $section } › { $setting }

palette_settings = Settings…

# A command of the palette opening the settings' page of keyboard shortcuts.
palette_settings_keyboard = Settings: Keyboard shortcuts

# A command of the palette opening the settings' page of plugins.
palette_settings_plugins = Settings: Plugins

palette_sidebar_section_reset = Sidebar: show every group in its place

palette_slideshow = Preview: slideshow

palette_split = Split pane

palette_split_other = Split pane the other way

palette_tags_prompt = Tags…

palette_terminal = Toggle terminal

palette_terminal_clear = Terminal: clear the history

palette_terminal_copy = Terminal: copy

palette_terminal_links = Terminal: follow a link…

palette_terminal_paste = Terminal: paste

palette_terminal_paths = Terminal: type the selection's paths

palette_terminal_select_all = Terminal: select all

palette_text_continuous = Preview: toggle continuous scrolling of text

palette_text_size_files_in = Files text: larger

palette_text_size_files_out = Files text: smaller

palette_text_size_files_reset = Files text: 100%

palette_timeline = Timeline

palette_toggle_preview = Toggle preview

palette_toggle_sidebar = Toggle sidebar

palette_up_button = Toggle the Up button

palette_usage_mode_age = Usage: color by age

palette_usage_mode_files = Usage: blocks by files

palette_usage_mode_size = Usage: blocks by size

palette_usage_mode_types = Usage: color by kind

palette_view_buttons_shown = Toggle view buttons

palette_wrap_lines = Preview: wrap lines

palette_zoom_in = Zoom in

palette_zoom_out = Zoom out

palette_zoom_reset = Zoom reset

# A palette of Skwir's own, its look in white and true black: a choice of Colors from.
palettes_black = Black

# A palette of Skwir's own, its look in warm tints, as paper: a choice of Colors from.
palettes_paper = Paper

# The palette of Skwir's own colors, a choice of Colors from.
palettes_skwir = Skwir

# A palette of Skwir's own, its look in cool tints, as slate: a choice of Colors from.
palettes_slate = Slate

# to: where ui.cursor was asked to go, as given (down, home, page_up); ui.cursor is a command's id.
pane_cursor_cannot_go = ui.cursor cannot go to { $to }

# Beside what was typed to reach a name, where no name begins with it.
pane_no_match = No match

# What a screen reader hears of what was typed to reach a name (sought, as typed) where no name begins with it.
pane_typed_no_match = { $sought }: No match

placeholder_companions = Only companion files here

# Show companion files, Settings and Tags and notes are the names of a setting and of where it is.
placeholder_companions_detail = They keep other files' notes: TagSpaces' .ts folder, a Mac's ._ files, XMP sidecars. Showing them turns on Show companion files in Settings (Tags and notes), for every folder.

placeholder_denied = You don't have permission to open this folder

placeholder_drop = Drop files here.

# keys: the keys that make a folder, as the keymap says them (Ctrl+Shift+N).
placeholder_drop_or_make = Drop files here, or press { $keys } for a new folder.

placeholder_empty = This folder is empty

placeholder_failed = This location could not be opened

placeholder_filter_empty = Nothing here is { $filter }. Press Enter in the address field to search the folders inside too.

placeholder_filtered = Hidden by filters

# A button in place of a folder's files when the drive or server the folder is on was unmounted, disconnected or is not open now: leave it for the home folder.
placeholder_go_home = Go Home

placeholder_gone = This folder no longer exists

placeholder_hidden = Only hidden files here

placeholder_hidden_show = Show hidden files to see them.

# keys: the keys that show hidden files (Ctrl+H).
placeholder_hidden_show_keys = Show hidden files to see them ({ $keys }).

# Shown in place of a folder's files when the drive or server it is on was unmounted or disconnected while it was shown; the reason follows.
placeholder_left = This folder is no longer available

placeholder_lens_empty = Nothing here is { $filter }. The filters stay as you go to other folders and searches, until you take them off in the address.

placeholder_looking = Looking in the folders here…

placeholder_looking_detail = The folders that lead to { $filter } show as they are found.

placeholder_name_empty = No name here holds { $filter }. Press Enter in the address field to search the folders inside too.

placeholder_nothing = Nothing here

# In a window answering another program's Open dialog, where the filter that program asked for (filter: its name, as the program gave it: Pictures, JPEG) leaves out every file of the folder.
placeholder_program_filtered = No files for { $filter } here

# Under "No files for … here", where the program gave its own choice of every file (all: its name, as the program gave it: All Files, Any file), which the filter dropdown of the bar at the window's foot lists among its filters.
placeholder_program_filtered_all_detail = The program asks for { $filter }. Choose { $all } in the bar to see every file.

# Under "No files for … here"; All files is the filter dropdown's last choice in the bar at the window's foot.
placeholder_program_filtered_detail = The program asks for { $filter }. Choose All files in the bar to see every file.

placeholder_reading = Reading the details of the files…

# filter: what was typed in the address to filter the folder.
placeholder_reading_detail = The files { $filter } keeps show as they are read.

placeholder_reading_files = Reading the details of { $count ->
        [one] one file
       *[other] { $count } files
    }…

# Under "… is not mounted", "… is locked" or "… is not connected" in place of a folder's files: the folder shows again by itself once its drive or server is back (mounted, unlocked, connected), whoever brings it back.
placeholder_reopen_detail = This folder is on it, and shows here again once it is back.

placeholder_retry = Try again

placeholder_show_all_files = Show all files

placeholder_show_companions = Show hidden and companion files

placeholder_show_everything = Show everything

placeholder_show_hidden = Show hidden files

placeholder_up = Go to the parent folder

# A button: stop working out the changes a command would make.
planning_cancel = Cancel

# Said while a command works out the changes it will make before they are shown; command: its title (Copy, Extract Here), as a label before what happens, never the subject of a verb.
planning_working_out = { $command }: working out the changes…

# The player was asked to play a file no player of this system takes.
player_cannot_play = This track cannot be played here

# What the player does when a track ends: it plays the next file of the folder.
player_continue_next = Play next

# What the player does when a track ends: it plays another file of the folder, chosen at random.
player_continue_shuffle = Shuffle

# What the player does when a track ends: it stops.
player_continue_stop = Stop

# The player was asked to play a path that leads to no file.
player_no_file = { $path } is no file to play

# Play was asked of a selection that holds no sound nor film.
player_nothing_selected = Nothing selected plays

# The player was asked to play playlists whose tracks are none it can play.
player_playlists_empty = These playlists list no track to play here

# A button of the player pinned to the window: stop playing and close it.
player_stop = Stop and let go

# The player pinned to the window: the track's title, then who plays it, both as the file's tags say them.
player_title_artist = { $title } · { $artist }

# A film preview's button.
preview_back_five = Back five seconds

# The preview of a link whose target is gone.
preview_broken_link = Nothing is at the link's target

# A button of peek, the preview over the window: close it.
preview_close = Close preview

# A button beside a preview's find field.
preview_close_find = Close find

# The button at the end of a preview's bar that leaves a file shown inside another (an archive's member).
preview_close_member = Close

# A preview's button and its drawer's heading: a document's table of contents, a text's outline.
preview_contents = Contents

# A preview's button: its pages one under the other, scrolled through, rather than a screen at a time.
preview_continuous = Continuous scrolling

# Why a preview shows nothing: the file made of it to show it (a document converted) was removed.
preview_converted_gone = The converted file is gone

# Said as the path of the file shown was copied.
preview_copied_path = Copied the path

# A preview's menu row: copy the file's path.
preview_copy_path = Copy path

# Pointing at the count of a document's pages or a book's chapters, which chooses how they are counted: in order, or by the numbers printed on them.
preview_count_by = Count by

# A document reader's button, lit while on: dragging across a page's text moves the page, as a hand would, rather than selecting the text.
preview_drag_moves = Drag to move the page

# The button and the title of the choices of how long a slideshow shows each picture (5 s), which follow it.
preview_each_picture_for = Each picture for

# The facts on the preview's card under the file's name: its type, its size, when it last changed (PDF document · 1.2 MB · Today 14:02). Each fact is drawn apart, with what follows it up to the next, and the line wraps only between them.
preview_facts = { $type_name } · { $size } · { $date }

# The facts on the preview's card under the file's name: its size and when it last changed (1.2 MB · Today 14:02). Each fact is drawn apart, the line wrapping only between them.
preview_facts_size_date = { $size } · { $date }

# The facts on the preview's card under the file's name: its type and when it last changed (Folder · Today 14:02). Each fact is drawn apart, the line wrapping only between them.
preview_facts_type_date = { $type_name } · { $date }

# The facts on the preview's card under the file's name: its type and its size (PDF document · 1.2 MB). Each fact is drawn apart, the line wrapping only between them.
preview_facts_type_size = { $type_name } · { $size }

# A preview's button: find words in what it shows.
preview_find = Find

# A preview's button: show the whole picture, page or model in the room there is.
preview_fit = Fit

# A preview's button: show the picture or page as wide as the room there is.
preview_fit_width = Fit width

# A film preview's button.
preview_forward_five = Forward five seconds

# A preview's button: show it on the whole screen.
preview_fullscreen = Fullscreen

# Pointing at the page number between a preview's previous and next buttons: a click lets one be typed.
preview_go_to_page = Go to page

# A preview's menu row: show the file's bytes in hexadecimal.
preview_hex = View as hex

# Why a preview shows nothing: the picture made of the file cannot be drawn.
preview_invalid_image = Invalid preview image

# Why a picture is not shown: what was decoded of it cannot be drawn.
preview_invalid_picture = invalid image

# A preview's caption for a file too large for a quick preview.
preview_large_file = Large file

# A preview's button: draw the text it shows larger.
preview_larger_text = Larger text

# A preview's caption while the file is read.
preview_loading = Loading…

# In the preview's place while the file is read.
preview_loading_content = Loading…

# Why a file inside an archive is not shown: it was unpacked, then is gone.
preview_member_missing = The extracted member is missing

# Why a file inside an archive is not shown: what unpacked it wrote it elsewhere than asked.
preview_member_outside = A member was written outside its directory

# A preview's last button, the menu of what has no button of its own.
preview_more = More

# A button beside a preview's find field.
preview_next_match = Next match

# A button of a picture shown on the whole screen.
preview_next_picture = Next picture

# A preview's caption for a file nothing shows.
preview_none = No preview

# Said where a command about the file shown is run and the preview shows none.
preview_nothing_shown = Nothing is shown

# Why a film's or a track's picture is not shown: nothing installed draws one.
preview_nothing_shows = Nothing shows this file

# Said where peek (the preview over the window) is asked with no file under the cursor.
preview_nothing_to_peek = nothing to peek at

# The page or chapter shown, between a preview's previous and next buttons: its number (3).
preview_number = { $number }

# After the page or chapter shown, between a preview's previous and next buttons: how many there are (3 / 12), or the last page's name as the document prints it (iv / 212).
preview_of_total = / { $total }

# A preview's menu row: open the file in the application the desktop opens it with.
preview_open_external = Open externally

# The placeholder of the field a document's page or a book's chapter number is typed in.
preview_page_field = Page

# What a screen reader calls the panel beside the files while it shows the file's preview.
preview_panel = Preview

# What a screen reader hears of the preview panel: the name of the file shown, then its type, size and date.
preview_panel_said = { $name }, { $facts }

# What a screen reader hears of peek: what its keys do. Space, Escape and F are keys.
preview_peek_fullscreen_keys = Space or Escape to close · arrows to move · F fullscreen

# What a screen reader hears of peek: what its keys do. Space and Escape are keys.
preview_peek_keys = Space or Escape to close · arrows to move

# What a screen reader hears of peek playing a sound or a film: what its keys do. Space, M, F and Escape are keys.
preview_peek_player_keys = Space play/pause · arrows seek · M mute · F fullscreen · Escape exit

# What a screen reader hears of peek: the type, size and date of the file shown, then what its keys do.
preview_peek_said = { $facts }. { $hint }

# Why a film's or a track's picture is not shown: what read it stopped unexpectedly.
preview_picture_reader_failed = the picture's reader failed

# What a screen reader hears of a picture shown: its size in pixels, width by height (4032 × 3024).
preview_pixels_said = { $width } × { $height } pixels

# What a screen reader calls a preview that plays a sound or a film. name: its file's name.
preview_player_said = Player: { $name }

# A button beside a preview's find field.
preview_previous_match = Previous match

# A button of a picture shown on the whole screen.
preview_previous_picture = Previous picture

# A choice of how a document's pages are counted: by the numbers printed on them (a book's iv, 12), not by their order in the file.
preview_printed_pages = Printed pages

# What the facts of a file are called: the button of peek that shows them, the panel showing them alone.
preview_properties = Properties

# What a search holds, under its name when nothing is selected.
preview_results = { $count ->
        [one] { $count } result
       *[other] { $count } results
    }

# Over a file shown as its own text where it could not be presented (an SVG not drawn). error: why.
preview_shown_as_written = Shown as written: { $error }

# A picture preview's menu row: show the folder's pictures one after another, on the whole screen.
preview_slideshow = Slideshow

# On the button choosing a slideshow's pace: how long it shows each picture, in seconds, short (5 s).
preview_slideshow_pace = { $seconds } s

# A button of a picture shown on the whole screen.
preview_slideshow_pause = Pause the slideshow

# A button of a picture shown on the whole screen: show the pictures one after another.
preview_slideshow_play = Play the slideshow

# A preview's button: draw the text it shows smaller.
preview_smaller_text = Smaller text

# A preview's button: show the file's own text (an HTML page's, a drawing's) instead of what it draws.
preview_source = Source

# Pointing at a preview's Source button.
preview_source_tip = The file's own text, as written

# Refused in Settings' list of the files a folder's preview shows: a pattern of * alone.
preview_stand_in_every = { $name } would find every file: give some of its name

# Why a folder's preview shows nothing: the README or cover it showed was removed.
preview_stand_in_gone = The file that stands for the folder is gone

# Refused in Settings' list of the files a folder's preview shows: a path where a file's name is asked.
preview_stand_in_path = { $name } is a path: list a file's name

# Refused in Settings' list of the files a folder's preview shows: the same name twice, letter case ignored.
preview_stand_in_twice = { $name } is listed twice

# A preview's menu row, opening the setting that names the files a folder's preview shows (README, cover).
preview_stand_ins = Files shown for folders…

# Why a picture is not previewed: it has more pixels than a preview draws. width, height: its size in pixels (8193 × 8193); megapixels: the most a preview draws.
preview_too_many_pixels = Too large to preview ({ $width } × { $height } pixels, more than { $megapixels ->
        [one] { $megapixels } megapixel
       *[other] { $megapixels } megapixels
    })

# A preview's button: turn a 3D model in its view, instead of its still picture.
preview_turn = 3D

# Pointing at a preview's 3D button, which shows a 3D model turned in its view or its still picture.
preview_turn_tip = The model turned in 3D, or its picture

# A preview's caption where reading the file failed.
preview_unavailable = Preview unavailable

# Where a film preview stands, in seconds to a tenth (1.5 s).
preview_video_at = { $seconds } s

# Where a film preview stands and how long it is, in seconds to a tenth (1.5 s / 3.0 s).
preview_video_at_of = { $seconds } s / { $total } s

# A preview's caption while it shows a picture of a film: its size in pixels, width by height (1920 × 1080).
preview_video_still = Video still · { $width } × { $height }

# A row of the More menu of a window showing one file's preview alone (skwir view): open the file's folder in Skwir, the file selected.
preview_viewer_reveal = Show in Skwir

# A row's label in Properties' Volume: the device a drive is (/dev/sda1), or where it is mounted from.
preview_volume_device = Device

# A row's label in Properties' Volume: the drive's filesystem (ext4, NTFS).
preview_volume_filesystem = Filesystem

# A row's label in Properties' Volume: the room left on the drive.
preview_volume_free = Free

# Properties' Volume, folded: the room left on the drive (12 GB free).
preview_volume_free_gist = { $free } free

# In Properties' Volume: the room left on the drive, of all its room (12 GB of 512 GB).
preview_volume_free_of = { $free } of { $total }

# A row's label in Properties' Volume: the folder the drive is mounted at.
preview_volume_mounted_at = Mounted at

# A row's label in Properties' Volume: the drive's name, as the sidebar names it.
preview_volume_name = Volume

# A row's label in Properties' Volume: the drive cannot be written to.
preview_volume_read_only = Read-only

# In Properties' Volume, after Read-only.
preview_volume_read_only_value = Nothing on it can change

# A preview's button: read all of a file past the size a quick preview keeps to.
preview_whole = Full preview

# Pointing at a preview's Full preview button.
preview_whole_tip = All of this file, past the size a preview keeps to

# A preview's button that opens its zoom levels and its ways of fitting.
preview_zoom = Zoom and fit

# On a preview's zoom button, where a percentage would be: the picture or page fits the room there is.
preview_zoom_fit = Fit

# A preview's button: show the picture, page, map or model larger.
preview_zoom_in = Zoom in

# A preview's button: show the picture, page, map or model smaller.
preview_zoom_out = Zoom out

# On a preview's zoom button and among its choices: the zoom, a percentage of the picture's or page's own size (150%).
preview_zoom_percent = { $percent }%

# On a preview's zoom button, where a percentage would be: the picture or page fits the width.
preview_zoom_width = Width

# A distance along a track, on its altitude chart (12.4 km); km is the kilometre's symbol.
profile_distance_kilometres = { $kilometres } km

# A distance along a track, on its altitude chart, under a kilometre (640 m); m is the metre's symbol.
profile_distance_metres = { $metres } m

# The length of a planned route (not a recorded track), on its altitude chart: 12.4 km planned. length: a distance with its unit, which chooses no plural: where a word would agree with it, say it so it need not (planned: 12.4 km).
profile_length_planned = { $length } planned

# An altitude on a track's altitude chart, in metres (812 m); m is the metre's symbol.
profile_metres = { $metres } m

# Pointing at a track's altitude chart: how far along the track (2.4 km), then the altitude there (812 m).
profile_readout = { $along } · { $altitude }

# Pointing at a track's altitude chart: how far along the track, then the altitude there, measured from the ground.
profile_readout_ground = { $along } · { $altitude } above ground

# Pointing at a flight's altitude chart: how far along it, then the altitude there, measured from where it took off.
profile_readout_takeoff = { $along } · { $altitude } above takeoff

# An error for who asked a question: ui.ask, command and field are a command's and its arguments' names, as they are written.
prompt_ask_needs = ui.ask needs a command and a field

# A button: close a question without answering it.
prompt_cancel = Cancel

# The keys of a question asked in a dialog.
prompt_confirm_keys = Enter to confirm, Escape to cancel

# The button that answers the question before a permanent deletion: delete, for good.
prompt_delete = Delete

# An error for who asked a question: ui.ask and describe are a command's and its argument's names, as they are written; command: the command named (rename.describe).
prompt_describe_acts = ui.ask's describe runs as the text is typed, before it is answered: { $command } does more than answer

# An error for who asked a question: ui.ask and describe are a command's and its argument's names, as they are written.
prompt_describe_no_command = ui.ask's describe names no command

# An error for who asked a question: ui.ask and describe are a command's and its argument's names, as they are written; command: the command named.
prompt_describe_unknown = ui.ask's describe names no command known: { $command }

# The button of the prompt that finds a name in the pane as it is typed.
prompt_find = Find

# In the empty field of the prompt that finds a name in the pane: a capital letter typed makes the case count.
prompt_find_placeholder = Part of a name (capitals: exact case)

# A button of what Quit asks while actions run: the window goes, Skwir finishes them, then quits.
prompt_finish_in_background = Finish in the background

# The button of the prompt that jumps to a byte offset of a file shown as hex.
prompt_go = Go

# A button of what Quit asks while actions run: do not quit.
prompt_keep_window = Keep the window

# In the empty field of the prompt that jumps to a byte offset of a file shown as hex; 0x is how a hexadecimal number is written.
prompt_offset_placeholder = Offset (decimal or 0x hexadecimal)

# The button that answers a question asked in a line of text, where its asker named no verb for it.
prompt_ok = OK

# In the empty field of a question asking for a password.
prompt_password_placeholder = Password

# The keys of what Quit asks while actions run.
prompt_quit_keys = Enter to finish in the background

# A switch under a password's field: keep the password in the desktop's keyring.
prompt_remember = Remember in the keyring

# The button that answers the question before another program saves over a file: save over it.
prompt_replace = Replace

# A button of what Quit asks while actions run: stop them now, then quit.
prompt_stop_and_quit = Stop and quit

# Under a question's field, when it was answered before what its text comes to was told (a command line's effect): the answer waits for it.
prompt_working_out = Working out what this does…

# A row's label in Properties: the file's mode, as ls writes it (rw-r--r--).
properties_access = Access

# The button that changes a file's mode to the one typed.
properties_access_change = Change

# The title of where a file's mode is typed. what: the file, by its name, or how many items.
properties_access_of = Access of { $what }

# The placeholder of the field a file's mode is typed in. chmod is a program's name; 644, u+x, go-w and a=rX are modes as it writes them.
properties_access_placeholder = As chmod takes it: 644, u+x, go-w, a=rX

# A row's label in Properties: when the file was last read.
properties_accessed = Accessed

# Pointing at a folder's Accessed in Properties.
properties_accessed_folder_tip = When it was last listed, as far as the system notes it: most note it seldom or never, to spare the drive.

# Pointing at a file's Accessed in Properties.
properties_accessed_tip = When it was last read, as far as the system notes it: most note it seldom or never, to spare the drive.

# A link after the file's tags or comment in Properties, where it has none: give some.
properties_add = Add…

# A link after the stores whose notes say otherwise: write the same notes in all.
properties_agree = Make them agree

# A row's label in Properties: the album a track is of, as its tags say it.
properties_album = Album

# A link after who may run a file: let everyone who can read it run it.
properties_allow_running = Allow running

# A switch under the choices of who may use folders: change what they hold too.
properties_also_inside = Also everything inside

# A row's label in Properties: the system lets the file only grow.
properties_append_only = Append only

# In Properties, after Append only.
properties_append_only_value = It can only grow

# A row's label in Properties: who plays a track, as its tags say it.
properties_artist = Artist

# What a folder holds in Properties, where some of it could not be counted.
properties_at_least = At least { $size }

# What a folder holds in Properties, its count stopped before its end.
properties_at_least_stopped = At least { $size }, stopped

# A row's label in Properties: a film's sound tracks (their languages).
properties_audio_tracks = Audio tracks

# A row's label in Properties, for a file that keeps another's notes: the file it belongs to.
properties_beside = Beside

# A row's label in Properties: the files beside it that keep its notes (a sidecar, a Mac's ._ file).
properties_beside_it = Beside it

# In Properties: the file is compressed, or encrypted, by the drive it is on.
properties_by_the_drive = By the drive

# A row's label in Properties: who may run the file as a program.
properties_can_run = Can run

# A link after a value in Properties: change it.
properties_change = Change…

# A row's label in Properties: when anything about the file last changed (its ctime).
properties_changed = Changed

# Pointing at Changed in Properties.
properties_changed_tip = When anything about it last changed: its contents, or what is said of it (its name, permissions, owner, tags).

# A row's label in Properties: who may change who may use the file, or why no one here can.
properties_changes = Changes

# A row's label in Properties, and the title of where it is written: the comment on the file.
properties_comment = Comment

# The placeholder of the field a file's comment is written in.
properties_comment_placeholder = A comment on this item (empty removes it)

# The button that keeps the comment written for a file.
properties_comment_save = Save

# In Properties, for a file that keeps another's notes: the name of that file, quoted.
properties_companion_of = { $name }

# In Properties, for a file that kept another's notes: the name of that file, which is no longer there.
properties_companion_of_gone = { $name }, which is gone

# A row's label in Properties: the drive keeps the file compressed.
properties_compressed = Compressed

# A row's label in Properties: what a folder holds.
properties_contains = Contains

# What a folder holds, in Properties' Contains: it is empty.
properties_contains_nothing = Nothing

# Said as a row of Properties is clicked: what it said was copied. what: the row's label as it is shown, quoted (“Path”, “Size”), whose gender is not known: where a word would agree with it, say it so none need (the value of “Size” copied).
properties_copied = Copied { $what }

# A link after a web address in Properties: copy it.
properties_copy_link = Copy

# A link in Properties: count what the folder holds, inside its folders too.
properties_count = Count

# A link after what a folder holds in Properties: count it again, as it may have changed.
properties_count_again = Count again

# A link in Properties, while what a folder holds is counted: stop counting.
properties_count_stop = Stop

# A row's label in Properties: when the file was made on this drive.
properties_created = Created

# Pointing at Created in Properties. Modified is the name of the row above it.
properties_created_tip = When it was made on this drive: a copy's is when it was copied, which may be later than Modified.

# A row's label in Properties: when the file was put in the trash.
properties_deleted = Deleted

# A row's label in Properties, for a file of the trash: the folder it was deleted from, where restoring it puts it back.
properties_deleted_from = Deleted from

# Pointing at Deleted in Properties.
properties_deleted_tip = When it was put in the trash, as the trash noted it.

# A folded heading of Properties: the first fact the file's format says of it, its label then its value, as the preview said them (Author: Ann).
properties_detail_gist = { $label }: { $value }

# A row's label in Properties: the stores whose notes on the file say otherwise.
properties_differs_in = Differs in

# A row's label in Properties: a picture's size in pixels.
properties_dimensions = Dimensions

# A picture's size in Properties, in pixels, width by height (4032 × 3024 px).
properties_dimensions_value = { $width } × { $height } px

# A row's label in Properties: the web address a file was downloaded from.
properties_downloaded_from = Downloaded from

# What was copied, inside a sentence (Copied downloaded from): the web address a file was downloaded from.
properties_downloaded_from_what = downloaded from

# In Properties: why who may use the file cannot be changed.
properties_drive_read_only = The drive is read-only

# In Properties: how many drives are mounted inside the folder, left out of what it holds.
properties_drives_inside = { $count ->
        [one] { $count } drive mounted inside
       *[other] { $count } drives mounted inside
    }

# In Properties, with several items selected: why who may use them cannot be changed.
properties_drives_read_only = A drive they are on is read-only

# A row's label in Properties: how long a sound or a film lasts.
properties_duration = Duration

# A link after the file's tags or comment in Properties: change them.
properties_edit = Edit…

# A row's label in Properties: the drive keeps the file encrypted.
properties_encrypted = Encrypted

# A size in Properties: as every size is said (1.2 MB), then exactly, in bytes.
properties_exact_size = { $size } ({ $bytes ->
       *[other] { $bytes } bytes
    })

# In Properties: how many files, then how many folders.
properties_files_and_folders = { $files ->
        [one] { $files } file
       *[other] { $files } files
    }, { $folders ->
        [one] { $folders } folder
       *[other] { $folders } folders
    }

# A row's label in Properties: the folder the file is in, its path.
properties_folder = Folder

# In Properties: how many folders, then how many files.
properties_folders_and_files = { $folders ->
        [one] { $folders } folder
       *[other] { $folders } folders
    }, { $files ->
        [one] { $files } file
       *[other] { $files } files
    }

# A row's label in Properties: the web page that led to a file's download.
properties_found_on = Found on

# What was copied, inside a sentence (Copied found on): the web page that led to a file's download.
properties_found_on_what = found on

# Under a value in Properties: which plugin gave it, by its name.
properties_from_plugin = from { $plugin }

# The title of the choices of the group a file belongs to. what: the file, by its name, or how many items.
properties_give_to_group = Give { $what } to the group

# A row's label in Properties: the group of users the file belongs to (Unix permissions).
properties_group = Group

# A row's label in Properties: what the users of the file's group may do with it.
properties_group_can = Group can

# The title of the choices of what the users of a file's group may do with it. what: the file, by its name, or how many items.
properties_group_may = What its group may do with { $what }

# A row's label in Properties: how many names (hard links) the file has.
properties_hard_links = Hard links

# A row's label in Properties: the system keeps the file as it is.
properties_immutable = Immutable

# In Properties, after Immutable.
properties_immutable_value = No one can change, rename or delete it

# A row's label in Properties: how many files and folders a folder holds, inside its folders too.
properties_in_all = In all

# A row's label in Properties: the stores the file's notes are kept in (its attributes, a sidecar).
properties_kept_in = Kept in

# A folded heading of Properties: the first line of the comment on the file.
properties_lead_comment = Comment: { $gist }

# A folded heading of Properties: the folder a file of the trash was deleted from (a path).
properties_lead_deleted_from = Deleted from { $gist }

# A folded heading of Properties: the site a file was downloaded from (example.org).
properties_lead_downloaded_from = Downloaded from { $gist }

# A folded heading of Properties: a category holding one tag at a time, by its name as the user wrote it, then the file's tag in it (Status: Draft).
properties_lead_field = { $field }: { $gist }

# A folded heading of Properties: the page that led to the download (example.org).
properties_lead_found_on = Found on { $gist }

# A folded heading of Properties: how long a sound or a film lasts (3:45).
properties_lead_lasts = Lasts { $gist }

# A folded heading of Properties: when the file last changed (2024-03-01 14:02).
properties_lead_modified = Modified { $gist }

# A folded heading of Properties: the drive a file is on (On Data).
properties_lead_on = On { $gist }

# A folded heading of Properties: who owns the file (a user's name).
properties_lead_owned_by = Owned by { $gist }

# What a class of users can do with a folder: also make, rename and delete what is in it.
properties_level_create = Create and delete items

# What a class of users can do with a folder: see the names in it, nothing more.
properties_level_list = List items only

# What a class of users (its owner, its group, others) can do with a file or a folder: nothing.
properties_level_none = None

# What a class of users can do with a folder: see the names in it and open them.
properties_level_open = Open items

# What a class of users can do with a file: read it, not change it.
properties_level_read = Read

# What a class of users can do with a file: read it and change it.
properties_level_read_write = Read and write

# What a class of users can do with a file: change it, not read it.
properties_level_write_only = Write only

# A row's label in Properties, for a link: the path it leads to.
properties_links_to = Links to

# A value in Properties, with several items selected, where they differ.
properties_mixed = Mixed

# A row's label in Properties: when the file's contents last changed.
properties_modified = Modified

# Pointing at a folder's Modified in Properties.
properties_modified_folder_tip = When a name in it was last added, removed or renamed; not when a file in it changed.

# Pointing at a file's Modified in Properties.
properties_modified_tip = When its contents last changed. A copy, or a file unpacked, may keep the original's.

# A user or a group in Properties: its name, then its number (alice (1000)).
properties_named_id = { $name } ({ $id })

# The value of a field of a file that is yes or no, in Properties' More.
properties_no = No

# A row's label in Properties: what was left out of what a folder holds.
properties_not_counted = Not counted

# Beside the choice that is the file's now, among the choices of who may use it.
properties_now = Now

# A row's label in Properties, with several items selected: how many folders and files they are.
properties_of_which = Of which

# A row's label in Properties: the room it takes on the drive.
properties_on_disk = On disk

# A link after a web address in Properties: open it in the browser.
properties_open_link = Open

# A row's label in Properties: what every other user may do with the file.
properties_others_can = Others can

# The title of the choices of what every other user may do with a file. what: the file, by its name, or how many items.
properties_others_may = What others may do with { $what }

# A row's label in Properties: the user who owns the file.
properties_owner = Owner

# A row's label in Properties: what the file's owner may do with it (Read and write).
properties_owner_can = Owner can

# In Properties, for a file someone else owns: why who may use it cannot be changed.
properties_owner_changes = Only its owner can change who may use it

# In Properties, with several items selected someone else owns: why who may use them cannot be changed.
properties_owner_changes_them = Only their owner can change who may use them

# The title of the choices of what a file's owner may do with it. what: the file, by its name, or how many items.
properties_owner_may = What its owner may do with { $what }

# A row's label in Properties: how many pages a document has.
properties_pages = Pages

# A folded heading of Properties: how many pages a document has.
properties_pages_gist = { $count ->
        [one] { $count } page
       *[other] { $count } pages
    }

# A row's label in Properties: the file's whole path.
properties_path = Path

# Said where a path to copy is not text (a name in no encoding), which the clipboard cannot hold.
properties_path_not_text = The path is not text the clipboard can hold

# A row's label in Properties: the stars the file was given.
properties_rating = Rating

# A row's label in Properties: the file, or the drive, cannot be changed.
properties_read_only = Read-only

# In Properties, after Read-only: the file's own read-only mark.
properties_read_only_value = It cannot be changed

# In Properties, for a file of the trash: why who may use it cannot be changed.
properties_restore_to_change = Restore it to change who may use it

# Who may run a file as a program, in Properties' Can run.
properties_runs_everyone = Everyone

# Who may run a file as a program, in Properties' Can run: the users of its group.
properties_runs_group = Its group

# Who may run a file as a program, in Properties' Can run: every user but its owner.
properties_runs_group_others = Its group and others

# Who may run a file as a program, in Properties' Can run.
properties_runs_no_one = No one

# Who may run a file as a program, in Properties' Can run: the users neither its owner nor of its group.
properties_runs_others = Others

# Who may run a file as a program, in Properties' Can run.
properties_runs_owner = Its owner

# Who may run a file as a program, in Properties' Can run: its owner and the users of its group.
properties_runs_owner_group = Its owner and group

# Who may run a file as a program, in Properties' Can run: its owner and the users not of its group.
properties_runs_owner_others = Its owner and others

# A row's label in Properties, with several items selected: how many.
properties_selected = Selected

# Under a value in Properties: the user gave it.
properties_set_by_you = set by you

# What a program's set-group-ID bit does, in Properties' Special.
properties_setgid = Runs as its group

# What a folder's set-group-ID bit does, in Properties' Special.
properties_setgid_folder = New items take the folder's group

# What a program's set-user-ID bit does, in Properties' Special.
properties_setuid = Runs as its owner, whoever starts it

# What a folder's set-user-ID bit is called, in Properties' Special: it does nothing on most systems.
properties_setuid_folder = Set-user-ID

# A link after the folder a file of the trash was deleted from: go there.
properties_show_folder = Show

# A row's label in Properties: how much the file holds.
properties_size = Size

# What a folder holds in Properties, while it is counted.
properties_so_far = { $size } so far…

# A row's label in Properties: what a special bit of the file's mode does (set-user-ID, sticky).
properties_special = Special

# What a folder's sticky bit does, in Properties' Special.
properties_sticky = Only an item's owner may rename or delete it

# A link after who may run a file: let no one run it.
properties_stop_running = Stop running

# A row's label in Properties: a film's subtitles (their languages).
properties_subtitles = Subtitles

# Under a value in Properties: a model suggested it, by its name, and how sure it is, in percent (80%). The value's gender is not known: where a word would agree with it, choose one that need not (a suggestion of).
properties_suggested_by = suggested by { $model } · { $confidence }%

# A row's label in Properties: the file's tags.
properties_tags = Tags

# Inside the title of what changes who may use what is selected: how many items.
properties_target_items = { $count ->
        [one] { $count } item
       *[other] { $count } items
    }

# Inside the title of what changes who may use a file: the file, by its name, quoted.
properties_target_named = { $name }

# A row's label in Properties, with several items selected: what they weigh together.
properties_total_size = Total size

# A row's label in Properties: a track's title, as its tags say it.
properties_track_title = Title

# A row's label in Properties: what of a folder could not be told a file or a folder.
properties_unknown = Unknown

# In Properties: how many folders inside could not be read, while what a folder holds was counted.
properties_unlisted = { $count ->
        [one] { $count } folder could not be listed
       *[other] { $count } folders could not be listed
    }

# In Properties: how many items inside could not be read, while what a folder holds was counted.
properties_unread = { $count ->
        [one] { $count } item could not be read
       *[other] { $count } items could not be read
    }

# A row's label in Properties: what of a folder could not be counted.
properties_unreadable = Unreadable

# The value of a row of Properties that holds several (a field's, the tags, the stores notes are kept in): each as it is said, joined as a list.
properties_value_list = { $items }

# The value of a field of a file that is yes or no, in Properties' More.
properties_yes = Yes

# Quick filters have nothing to offer: nothing tells the files apart.
quick_filters_alike = { $count ->
       *[other] all { $count } files are alike
    }

# Quick filters have nothing to offer: every file is of this kind (images).
quick_filters_alike_kind = { $count ->
       *[other] all { $count } files are { $kind }
    }

# Quick filters have nothing to offer: every file is of this kind (images), modified that year (2024).
quick_filters_alike_kind_year = { $count ->
       *[other] all { $count } files are { $kind } from { $year }
    }

# Quick filters have nothing to offer: there is one file. Inside a sentence: Nothing to filter: one file.
quick_filters_alike_one = one file

# Quick filters have nothing to offer: there is one file, of this kind (image).
quick_filters_alike_one_kind = one file ({ $kind })

# Quick filters have nothing to offer: there is one file, of this kind (image), modified that year (2024).
quick_filters_alike_one_kind_year = one file ({ $kind }, { $year })

# Quick filters have nothing to offer: there is one file, modified that year (2024).
quick_filters_alike_one_year = one file ({ $year })

# Quick filters have nothing to offer: every file was modified that year (2024).
quick_filters_alike_year = { $count ->
       *[other] all { $count } files are from { $year }
    }

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_archive = archives

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_audio = music and audio

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_document = documents

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_folder = folders

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_font = fonts

# The Quick filters bar, where the filters chosen leave no file.
quick_filters_all_hidden = Every file here is hidden by the filters

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_image = images

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_presentation = presentations

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_program = programs

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_spreadsheet = spreadsheets

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_text = text and code

# Inside a sentence, what several files all are: all 12 files are images.
quick_filters_all_video = videos

# The Quick filters bar, while it counts what it can offer.
quick_filters_counting = Counting the files…

# A heading of the Quick filters bar, before the tags inside this tag.
quick_filters_in_tag = In { $tag }

# A heading of the Quick filters bar, before the kinds of files (Images, Documents).
quick_filters_kind = Kind

# A heading of the Quick filters bar, before the years the files were modified in.
quick_filters_modified = Modified

# Pointing at a year of the Quick filters bar.
quick_filters_modified_in = Modified in { $year }

# The Quick filters bar, where there is nothing to filter.
quick_filters_no_files = No files here

# The Quick filters bar, where nothing tells the files apart.
quick_filters_nothing = Nothing to filter

# The Quick filters bar, where nothing tells the files apart. alike: what they all are (all 12 files are images from 2024).
quick_filters_nothing_alike = Nothing to filter: { $alike }

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_archive = archive

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_audio = audio

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_document = document

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_folder = folder

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_font = font

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_image = image

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_presentation = presentation

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_program = program

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_spreadsheet = spreadsheet

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_text = text

# Inside a sentence, what one file is: one file (image, 2024).
quick_filters_one_video = video

# The Quick filters bar, where there are only folders to filter, which it does not.
quick_filters_only_folders = No files here, only folders

# The Quick filters bar, while it reads what the files' tags, kinds and dates are.
quick_filters_reading = Reading the details of { $count ->
        [one] { $count } file
       *[other] { $count } files
    }…

# Pointing at a tag of the Quick filters bar: the tag, then that Alt and a click keeps the files without it.
quick_filters_tag_tip = { $tag } · Alt+click: none tagged so

# A heading of the Quick filters bar, before the other tags the files have.
quick_filters_with = With

# The placeholder of the field that finds words in a document (a PDF).
reader_find_field = Find in document…

# Drawn in place of a document's page whose picture the reader gave cannot be shown.
reader_invalid_bitmap = invalid document bitmap

# Under a document's find field: which match is shown, of how many (3 / 12).
reader_match_of = { $at } / { $count }

# Under a document's find field: which match is shown, of how many, where the search stopped at the most it finds (3 / 1,000+).
reader_match_of_most = { $at } / { $count }+

# A document preview's button.
reader_next_page = Next page

# In a document's find field: the words looked for are nowhere in it.
reader_no_match = No match

# Said where a page typed is none of the document's printed pages. first, last: its first and last printed page numbers (i, 212).
reader_page_printed_refused = Enter a page number printed in the document, from { $first } to { $last }

# Said where a page typed is not a page of the document. last: its last page's number.
reader_page_refused = Enter a page number from 1 to { $last }

# Drawn in place of a document's page while it is drawn. number: the page's.
reader_page_waits = Page { $number }…

# A document preview's button.
reader_previous_page = Previous page

# What a screen reader hears of a document's preview: the page shown, of how many.
reader_said_page = Page { $page } of { $pages }

# Under a document's find field while it looks for the words.
reader_searching = Searching…

# A choice of how a document's pages are counted: in their order in the file (1, 2, 3), not by the numbers printed on them.
reader_unit_pages = Pages

# A button of a record's form in the settings: close it, nothing kept.
records_cancel = Cancel

# How many records a long list in the settings holds, beside its filter.
records_count = { $count }

# A button of a record in a list of the settings (a tile server): change it.
records_edit = Edit

# A record's field the user may leave empty: field is its name (Headers).
records_field_optional = { $field } (optional)

# The placeholder of a long list's filter in the settings.
records_filter = Filter

# What a screen reader calls a long list's filter in the settings.
records_filter_label = Filter the list

# How many records a long list's filter in the settings kept (found), of all it holds (count).
records_found_of = { $found } of { $count }

# How many records a group of a list in the settings holds, beside its title.
records_group_count = { $count }

# Said while a record holding a secret (a password) waits for the desktop's keyring.
records_keyring_asking = Asking the keyring…

# Why a record holding a secret (a password) was not saved: the desktop's keyring refused it.
records_keyring_failed = The keyring did not keep it: { $error }

# A record holding a secret (a password) was removed, and the desktop's keyring did not forget the secret.
records_keyring_kept = The keyring kept a secret of the record removed: { $error }

# What the desktop's keyring calls a secret skwir keeps there: record is the record's name or id, as the user wrote it; setting is the setting it belongs to (Tile servers).
records_keyring_label = skwir: { $record } ({ $setting })

# A list of records in the settings that skwir fills (the languages found) is empty.
records_none_found = None found

# A list of records in the settings, filtered: filter is what the user typed.
records_none_match = Nothing matches { $filter }

# A list of records in the settings that the user fills (tile servers) is empty.
records_none_yet = None yet

# A button that adds a name and value pair (an HTTP header) to a record.
records_pair_add = Add

# The placeholder of the first field of a name and value pair (an HTTP header's name).
records_pair_name = Name

# The label of a button that takes a name and value pair (an HTTP header) out of a record.
records_pair_remove = Remove

# The placeholder of the second field of a name and value pair (an HTTP header's value).
records_pair_value = Value

# A button of a record in a list of the settings (a tile server): take it out.
records_remove = Remove

# A button of a record's form in the settings: keep it.
records_save = Save

# A button beside a secret's field (a password): the keyring forgets it once the record is saved.
records_secret_forget = Forget

# The placeholder of a secret's field (a password) whose value the desktop's keyring keeps.
records_secret_kept = Kept in the keyring: type to replace

# The placeholder of a secret's field (a password) the keyring no longer keeps.
records_secret_not_set = Not set

# Unfolds a long list of records in the settings: count is how many it holds.
records_show_all = Show all { $count }

# Folds a long list of records in the settings to its first rows.
records_show_fewer = Show fewer

# What a screen reader calls an item of a field's list of what it takes; written: the item as it is written (a pattern); says: what it means.
reference_item = { $written }, { $says }

# What a screen reader calls an item of a field's list of what it takes; written: the item as it is written (a pattern); says: what it means; gives: what it gives for the first file.
reference_item_gives = { $written }, { $says }, { $gives }

# The tooltip of a field's ? button, which lists what the field takes.
reference_tip = What it takes

# The tooltip of a field's ? button, which lists what the field takes; keys: its shortcut.
reference_tip_keys = What it takes · { $keys }

# A button: close the bulk rename, renaming nothing.
renamebulk_cancel = Cancel

# The bulk rename's choice of the case it gives names.
renamebulk_case = Case

# A case the bulk rename gives names, written as it leaves them: every letter small.
renamebulk_case_lower = lowercase

# A case the bulk rename gives names, written as it leaves them: only the first word begins with a capital.
renamebulk_case_sentence = Sentence case

# A case the bulk rename gives names, written as it leaves them: each word begins with a capital.
renamebulk_case_title = Title Case

# A case the bulk rename gives names: it leaves them as they are.
renamebulk_case_unchanged = Unchanged

# A case the bulk rename gives names, written as it leaves them: every letter a capital.
renamebulk_case_upper = UPPERCASE

# What a screen reader calls the dialog renaming several files at once.
renamebulk_dialog = Bulk rename

# The bulk rename's keys.
renamebulk_keys = Tab switches fields · Enter renames (undoable) · Escape cancels

# The bulk rename's keys. case: the keys that change the case (Ctrl+K).
renamebulk_keys_case = Tab switches fields · { $case } changes case · Enter renames (undoable) · Escape cancels

# The bulk rename's keys. case: the keys that change the case (Ctrl+K); reference: the keys that say what a field takes (F1).
renamebulk_keys_case_reference = Tab switches fields · { $case } changes case · { $reference } says what one takes · Enter renames (undoable) · Escape cancels

# The bulk rename's keys. reference: the keys that say what a field takes (F1).
renamebulk_keys_reference = Tab switches fields · { $reference } says what one takes · Enter renames (undoable) · Escape cancels

# Under the renames the bulk rename lists: how many more it makes.
renamebulk_more = { $count ->
        [one] … { $count } more
       *[other] … { $count } more
    }

# Where the bulk rename lists its renames, when it would rename nothing.
renamebulk_no_changes = No name changes

# The bulk rename's field of a regular expression matched in each name.
renamebulk_regex = Regex

# In the bulk rename's empty Regex field.
renamebulk_regex_placeholder = Optional: a pattern over the whole name

# A button: rename the files as the bulk rename lists.
renamebulk_rename = Rename

# The bulk rename's field of what each match of the regex becomes.
renamebulk_replacement = Replacement

# In the bulk rename's empty Replacement field; $1 is written as it is typed.
renamebulk_replacement_placeholder = Optional: what each match becomes, $1 for its first group

# In the bulk rename, when what it was opened for is no longer selected.
renamebulk_selection_changed = The selection changed since this opened: open it again

# The bulk rename's field of the number the names' counter starts at.
renamebulk_start = Start

# The bulk rename's field of what each new name is made of.
renamebulk_template = Template

# The title of the dialog renaming several files at once.
renamebulk_title = { $count ->
        [one] Rename 1 item
       *[other] Rename { $count } items
    }

# Under what an agent asks to run: a command that acts at once.
review_agent_asks = It acts without a plan to review: allow it this once, or refuse and the agent is told.

# The keys of what an agent asks to run.
review_agent_hint = Enter allows it once, Escape refuses

review_agent_stopped_waiting = The agent stopped waiting: nothing was run

# A button: let an agent run what it asks, this once.
review_allow = Allow

# A button: carry out a plan proposed for review.
review_apply = Apply

# The keys of a plan proposed for review.
review_apply_hint = Enter applies (undoable), Escape discards

# A button: answer a question by doing nothing.
review_cancel = Cancel

# Where the keys of a proposed plan are said, when a step of it cannot be done.
review_cannot_apply = Cannot apply: fix the conflicts first

# Where the keys of a proposed plan are said, while each step is checked.
review_checking = Checking…

# Where a proposed plan's steps are listed, while each is checked.
review_checking_steps = Checking each step…

# A copy that found names taken asked what becomes of them, and the user cancelled it.
review_clashes_cancelled_copy = Cancelled: nothing was copied

# Links that found names taken asked what becomes of them, and the user cancelled them.
review_clashes_cancelled_link = Cancelled: nothing was linked

# A move that found names taken asked what becomes of them, and the user cancelled it.
review_clashes_cancelled_move = Cancelled: nothing was moved

# An action that found names taken asked what becomes of them, and stopped waiting for the answer.
review_clashes_stopped_waiting = It stopped waiting: nothing was done

# A button: connect to the server although Skwir cannot vouch for it.
review_connect = Connect

# Under the question of a connection to a server Skwir cannot vouch for.
review_connection_asks = Nothing is sent to the server until you answer.

# A connection to a server asked whether to go on, and the user cancelled it.
review_connection_cancelled = Cancelled: nothing was connected

# The keys of the question of a connection to a server.
review_connection_hint = Enter connects, Escape cancels

# A connection to a server asked whether to go on, and stopped waiting for the answer.
review_connection_stopped_waiting = It stopped waiting: nothing was connected

# A button: copy although there may not be room where it writes.
review_copy_anyway = Copy anyway

# A copy short of room asked whether to go on, and stopped waiting for the answer.
review_copy_stopped_waiting = The copy stopped waiting: nothing was copied

# A button: drop a plan proposed for review, doing nothing.
review_discard = Discard

# title: the plan proposed, by its title (rename 3 items).
review_discarded = Discarded proposal: { $title }

review_has_conflicts = This proposal has conflicts; discard it or fix them

# Under a plan proposed for review: names it finds taken where it puts items, which the user is asked about as it is applied; its steps are listed as if both were kept.
review_names_taken = { $count ->
        [one] { $count } name already taken: you choose what becomes of it as it is applied (shown here as both kept, folders merged).
       *[other] { $count } names already taken: you choose what becomes of them as it is applied (shown here as both kept, folders merged).
    }

# A proposed plan's own note (what it leaves out), then that names are taken where it puts items.
review_note_then_names_taken = { $said } { $asked }

# The title of a plan proposed for review; title: the plan's title (rename 3 items).
review_proposed = Proposed: { $title }

# The title of a plan proposed for review. who: who proposes it, as it names itself (an agent, a plugin); title: the plan's title (rename 3 items).
review_proposes = { $who } proposes: { $title }

# A button: refuse what an agent asks to run; the agent is told.
review_refuse = Refuse

# An agent asked to run a command that acts, and the user refused.
review_refused = Refused: the agent was told

# Under the question of a copy short of room where it writes.
review_room_asks = Nothing is copied until you answer.

# A copy short of room asked whether to go on, and the user cancelled it.
review_room_cancelled = Cancelled: nothing was copied

# The keys of the question of a copy short of room.
review_room_hint = Enter copies anyway, Escape cancels

# Beside a step of a plan under review: what it does is done already.
review_step_already_done = already done

# A step of a plan under review that cannot be done: the step (rename a.txt -> b.txt), then why (the file is gone).
review_step_conflict = { $change } — { $reason }

# Beside a step of a plan under review: it cannot be done as it is.
review_step_conflicts = conflict

# Beside a step of a plan under review: it is still to be done, and can be undone.
review_step_pending = pending

# Beside a step of a plan under review: it is still to be done, and cannot be undone.
review_step_pending_for_good = pending, not undoable

# What a screen reader says of a proposed plan's step: the step, then its state (pending).
review_step_said = { $step }, { $status }

# Beside a proposed plan's title: how many steps it has.
review_steps = { $count ->
        [one] { $count } step
       *[other] { $count } steps
    }

# Beside a proposed plan's title: how many steps it has, then how many other proposals and questions wait after it.
review_steps_and_waiting = { $count ->
        [one] { $count } step
       *[other] { $count } steps
    } · { $waiting } more waiting

# Beside a question's title: how many other proposals and questions wait after it.
review_waiting = { $waiting } more waiting

# Above a proposed plan's steps: the folder their paths are in, as the address writes it (~/Documents), which the steps leave out.
review_within = In { $folder }

search_bar_advanced_tip = Part of the search no chip holds

# A tag's category in its pill, which goes to every file tagged with what it holds; category: its name.
search_bar_all_of = All of { $category }

# A pill of a search or a filter; facet: what it filters by (Name, Kind, Tag); value: what it is set to.
search_bar_chip = { $facet }: { $value }

# The tooltip of the pill saying how deep a search looks: before the dot, where it looks; after it, its other choice.
search_bar_depth_below = The search looks in the subfolders too · this folder only

# The tooltip of the pill saying how deep a search looks: before the dot, where it looks; after it, its other choice, and the shortcut that chooses it.
search_bar_depth_below_keys = The search looks in the subfolders too · this folder only ({ $keys })

# The tooltip of the pill saying how deep the words typed look: before the dot, what they do; after it, what its other choice does.
search_bar_depth_folder = The words filter this folder only · its subfolders too are searched

# The tooltip of the pill saying how deep the words typed look: before the dot, what they do; after it, what its other choice does, and the shortcut that chooses it.
search_bar_depth_folder_keys = The words filter this folder only · its subfolders too are searched ({ $keys })

# The tooltip of the pill saying how deep a search looks: before the dot, where it looks; after it, its other choice.
search_bar_depth_here = The search looks in this folder only · its subfolders too

# The tooltip of the pill saying how deep a search looks: before the dot, where it looks; after it, its other choice, and the shortcut that chooses it.
search_bar_depth_here_keys = The search looks in this folder only · its subfolders too ({ $keys })

# The tooltip of the pill saying where a search looks, which cannot change; pill: what it reads.
search_bar_depth_listed = { $pill } · searched as it lists

# A choice of the pill of the words typed: type them again in the address field.
search_bar_edit = Edit…

search_bar_edit_address = Edit the address as text

search_bar_edit_search = Edit the search as text

search_bar_facets_folder = Filter this folder by name, kind, date, size or tag

search_bar_facets_search = Narrow the search by name, kind, date, size, tag or contents

# The button after the pills that adds a filter.
search_bar_filter = Filter

# The tooltip of a pill filtering the folder shown; pill: what the pill reads.
search_bar_folder_tip = { $pill } · this folder only, not a search

# The tooltip of a filter's pill that stays as the pane goes elsewhere; pill: what the pill reads.
search_bar_lens_tip = { $pill } · kept as you go to other folders and searches, until taken off

# A filter's pill leaving out the files of a tag, before the tag's name.
search_bar_not_tagged = Not tagged

# The last choice of a pill's menu: take that filter off.
search_bar_remove = Remove

# The × of a search's or a filter's pill; pill: what the pill reads.
search_bar_remove_pill = Remove { $pill }

# A filter's pill: what it filters by, before the tag's name.
search_bar_tag = Tag

# A filter's pill holding the words typed, as typed, in quotes.
search_bar_typed = { $text }

# At the foot of the window after keys typed that begin a binding of several (g, then h): no binding goes on from them where the focus is.
sequence_nothing_follows = Nothing follows these keys here

# At the foot of the window after the keys typed that begin a binding of several (g): what follows lists the keys that can come next and what each does.
sequence_then = then

# What a kept setting holds: the layout of the last dialog, so the next one looks the same.
session_chooser_setting = The view, preview and size of the windows answering other programs' Open and Save dialogs, and the view of those asking for pictures or films

# What a kept setting holds: the names the user gave the places the sidebar offers (Home, the Trash, a drive), which every window says.
session_place_names_setting = The names given in the sidebar to the places it offers, which every window says

# What a kept setting holds: the shape of the last window, so the next one looks the same.
session_setting = The last window's view, split, preview and timeline

settings_activity_opens = Open Activity by itself

settings_activity_opens_description = Activity opens above the footer, over the files, with an action's progress, its speed and its buttons, once an action you asked in the window lasts.

# What a setting's description says after its first sentence, in its tooltip.
settings_activity_opens_description_more = Its chevron folds it into the footer until what runs has ended; the next long action opens it again. Off, it opens only from the footer's strip, or Ctrl+Shift+J.

settings_activity_unfold = Open Activity after

settings_activity_unfold_description = How long an action you asked runs before Activity opens.

# What a setting's description says after its first sentence, in its tooltip.
settings_activity_unfold_description_more = Until then, and for anything quicker, the footer's strip says how it goes.

# The button of the system's folder chooser opened by a list's Add Folder in the settings.
settings_add_folder_choose = Add

# A badge on a setting whose change takes effect at the next start.
settings_after_restart = Applies after restart

# Beside Restore all, when no setting differs from its default.
settings_all_default = Every setting already has its default value.

# A setting's choice: what the desktop, the system or skwir chooses here.
settings_automatic = Automatic

# The button choosing a file or a folder for a setting.
settings_browse = Browse…

# The button of the system's file chooser a setting's Browse opens.
settings_browse_choose = Choose

settings_buttons_for_views = Show view buttons

# What a setting kept as data holds, for agents and scripts: true, false, list, grid and columns are as written.
settings_buttons_for_views_data = The views given a button beside the address (true) or whose button was taken away (false), by id; any other has a button if it shows the files as they are listed (list, grid, columns)

settings_buttons_for_views_description = Show a button for each view beside the address: list, grid, columns, and any other view given a button in the buttons' menu.

# What a setting's description says after its first sentence, in its tooltip.
settings_buttons_for_views_description_more = Off, one button stands for them all: it shows the view shown and opens the View menu, as in a pane too narrow for them; a right-click on it still chooses the buttons.

settings_cards = Cards

settings_cards_description = Draw the panes, the preview's parts and the terminal as cards on the window's frame, set apart by a gutter rather than by lines.

# What a setting's description says after its first sentence, in its tooltip.
settings_cards_description_more = The sidebar, the tabs and the window's buttons lie on the frame. While the window is split, the active pane's top row is underlined and the other pane is a shade darker. The gutter between two cards is the edge to drag. Off, the window's areas are set apart by lines, as in earlier versions.

# A setting's default choice as a screen reader names it, with what it comes to here and now: Automatic, now Dark.
settings_choice_now_said = { $choice }, now { $now }

# What the list of a setting's choices is named to a screen reader.
settings_choices = Choices

# Said in a setting's list of choices when what is typed in its field keeps none.
settings_choices_none = None holds what is typed

# What the window keeps, for agents and scripts.
settings_chooser_folders_kept_state = The folders each program last chose in, through the window answering its Open or Save dialog, by the program's id ("" for one with none): its ten last, newest first (folders), and when it last chose (used, seconds since 1970)

# The button closing the settings.
settings_close = Close

settings_code_appearance = Light or dark

settings_code_appearance_description = Draw source files and code in documents light or dark.

# Automatic, Light and Dark are the setting's choices.
settings_code_appearance_description_more = Automatic follows Skwir's theme; Light or Dark keeps code in the theme chosen below for that look, with its gutter, selection and background, whatever Skwir looks like.

settings_code_dark_theme = Dark theme

# Light or dark is the name of another setting, Dark one of its choices.
settings_code_dark_theme_description = The colors of source files and of code in documents when code is drawn dark: while Skwir looks dark, or always when Light or dark above is set to Dark.

# Default is a choice of the setting; Colors from is the name of another setting; bat is a program's name.
settings_code_dark_theme_description_more = Default is the code theme of the palette Colors from chooses, where it has one for that look, else Skwir's own, on its dark background; a theme brings its own background, bold and italic. Themes you added to bat, in its themes folder, follow bat's own.

settings_code_engine = Automatic uses

# Automatic is a choice of the languages listed below; bat and tree-sitter are programs' names.
settings_code_engine_description = Which grammar colors a language left Automatic below when both bat and a tree-sitter grammar know it.

# bat first and Tree-sitter first are the setting's choices; bat, tree-sitter and Sublime Text are programs' names.
settings_code_engine_description_more = With bat first, bat's grammar colors it, or one you added to bat or Sublime Text; with Tree-sitter first, the tree-sitter grammar. A language only one of them knows is colored by that one. Tree-sitter colors what the preview holds whole; a longer file is colored by bat, else shown plain.

settings_code_font = Code font

settings_code_font_description = The font of source and text files, code in documents, the byte view and the terminal, where each letter takes the same width.

# Fonts from is the name of another setting.
settings_code_font_description_more = Automatic is your desktop's monospace font, as Fonts from reads it, else Skwir's own.

settings_code_light_theme = Light theme

# Light or dark is the name of another setting, Light one of its choices.
settings_code_light_theme_description = The colors of source files and of code in documents when code is drawn light: while Skwir looks light, or always when Light or dark above is set to Light.

# Default is a choice of the setting; Colors from is the name of another setting; bat is a program's name.
settings_code_light_theme_description_more = Default is the code theme of the palette Colors from chooses, where it has one for that look, else Skwir's own, on its light background; a theme brings its own background, bold and italic. Themes you added to bat, in its themes folder, follow bat's own.

settings_code_text_size = Code text size

settings_code_text_size_description = The size of source and text files, the byte view and the terminal; 100% is your desktop's monospace font size where it names one.

# points: the size 100% comes to here, in points (10.5).
settings_code_text_size_now = The size of source and text files, the byte view and the terminal; 100% is your desktop's monospace font size where it names one, now { $points } pt.

# Said by settings.check of a value it does not know: it may be among the themes you added to bat (a program).
settings_code_theme_more = the themes you added to bat

settings_colors_from = Colors from

settings_colors_from_description = Where Skwir takes its colors: your desktop's, or one of Skwir's palettes.

# Plasma, LXQt, Omarchy, GNOME, MATE, Cinnamon, Xfce, Noctalia, Caelestia and Dux Shell are desktops' names; qt6ct, qt5ct, qtengine and hyprqt6engine programs'.
settings_colors_from_description_more = Automatic reads your desktop's own (Plasma's color scheme, LXQt's palette, Omarchy's theme, the GTK theme GNOME, MATE, Cinnamon or Xfce names), else GTK's colors (its theme, with your own GTK style sheet over it, where a desktop shell such as Noctalia, Caelestia or Dux Shell writes its colors), else the color scheme of qt6ct, qt5ct, qtengine or hyprqt6engine; GTK or Qt reads that toolkit's alone. A palette keeps its own accent and brings its code and terminal colors, each in a light look and a dark one. More contrast, when your desktop asks for it, is followed whatever is chosen.

settings_colors_from_macos = Where Skwir takes its colors: macOS's, or one of Skwir's palettes.

# Increase contrast is as macOS names its setting.
settings_colors_from_macos_more = Automatic wears the colors macOS draws its own windows in, with the accent you chose and its Increase contrast. A palette keeps its own accent and brings its code and terminal colors, each in a light look and a dark one. Increase contrast is followed whatever is chosen.

settings_colors_from_windows = Where Skwir takes its colors: Windows' accent, or one of Skwir's palettes.

# What a setting's description says after its first sentence, in its tooltip.
settings_colors_from_windows_more = Automatic wears Skwir's colors with the accent you chose in Windows' settings, or a contrast theme's colors while one is on. A palette keeps its own accent and brings its code and terminal colors, each in a light look and a dark one. A contrast theme's need for more contrast is followed whatever is chosen.

settings_contents_pinned = Keep contents open

settings_contents_pinned_description = Keep a document's or a book's contents open beside it while you read, the text or the pages narrowed to make room.

# What a setting's description says after its first sentence, in its tooltip.
settings_contents_pinned_description_more = Off, they open over what you read and close once you choose a part. The pin at the top of the contents changes it too.

# A setting's choice: the dark look.
settings_dark = Dark

settings_dates = Dates

# Friendly is one of the setting's choices.
settings_dates_description = Friendly dates read as Today 14:02, Tuesday 09:30 or 12 Mar, with the exact time in a tooltip.

# ISO is one of the setting's choices.
settings_dates_description_more = ISO shows 2026-03-12 14:02 everywhere.

# A choice of how dates are written: Today 14:02, Tuesday 09:30, 12 Mar.
settings_dates_friendly = Friendly

# A choice of how dates are written: as the standard ISO 8601 writes them, 2026-03-12 14:02.
settings_dates_iso = ISO 8601

# What the settings dialog is named to a screen reader.
settings_dialog = Settings

# Beside Restore all: count is how many settings it gives back their defaults.
settings_differ = { $count ->
        [one] { $count } setting differs from its default. Plugins stay enabled or disabled as they are.
       *[other] { $count } settings differ from their defaults. Plugins stay enabled or disabled as they are.
    }

# Under a setting whose value does nothing on this system: why is the reason, a sentence without its full stop (X11's window manager keeps its frame).
settings_does_nothing = Does nothing here. { $why }.

# What the window keeps, for agents and scripts.
settings_drafts_told_state = The languages whose translation was said to be a draft

settings_drag_image = Dragged files

settings_drag_image_description = How several files look while you drag them.

# Fan, Pile, Mosaic and Single are the setting's choices.
settings_drag_image_description_more = Fan holds the one you took in front of two more leaning out behind it; Pile lays two more under it, each a little lower; Mosaic sets up to four side by side, the last square saying how many more; Single shows the one you took alone. A badge counts them, but in the mosaic. One file is always its own picture.

# How several files look while dragged: the one taken in front of two more leaning out behind it.
settings_drag_image_fan = Fan

# How several files look while dragged: up to four side by side.
settings_drag_image_mosaic = Mosaic

# How several files look while dragged: two more under the one taken, each a little lower.
settings_drag_image_pile = Pile

# How several files look while dragged: the one taken alone.
settings_drag_image_single = Single

# The row opening the settings file in an editor.
settings_edit_as_text = Edit as text

# The button at a page's end folding its settings back to the few most people come for.
settings_fewer_settings = Fewer settings

# A caption on the General page, above where the settings file is.
settings_file = Settings file

# Where the settings file is, from ~ where it is under the home folder.
settings_file_at = { $path }. It lists only what differs from the defaults.

# A setting's title on the General page: what answers other programs' Open and Save dialogs.
settings_file_chooser = Open and save dialogs

# Said once what answers other programs' dialogs is chosen and the configuration said so already: chooser is the back end's name (Skwir, KDE).
settings_file_chooser_already = Other programs' Open and Save dialogs are already { $chooser }

# Said once the desktop's own choice is chosen for what answers other programs' Open and Save dialogs and the user's portal configuration named nothing for them already.
settings_file_chooser_already_desktop = The desktop already chooses what answers other programs' Open and Save dialogs

# Said under the Open and save dialogs setting: the desktop portal reads its configuration as it starts.
settings_file_chooser_applies = Applies when the desktop portal starts again, at the next login

# Said once what answers other programs' dialogs is chosen, with Undo: chooser is the back end's name (Skwir, KDE); it applies when the portal starts again, at the next login.
settings_file_chooser_chosen = Other programs' Open and Save dialogs: { $chooser }, from the desktop portal's next start

# Said once the desktop's own choice is chosen for what answers other programs' Open and Save dialogs, with Undo: the user's portal configuration names nothing for them any more; it applies when the portal starts again, at the next login.
settings_file_chooser_chosen_desktop = The desktop chooses what answers other programs' Open and Save dialogs, from the desktop portal's next start

# Open and Save are what programs call their dialogs for choosing a file.
settings_file_chooser_description = What answers other programs' Open and Save dialogs, those that ask the desktop for one: Skwir's windows, or another program's dialogs.

# What a setting's description says after its first sentence, in its tooltip. The desktop portal is the desktop's service programs ask for dialogs.
settings_file_chooser_description_more = The desktop portal reads this as it starts: a change applies at the next login. Programs that draw their own dialogs keep them.

# Said when the portal's configuration cannot be read, written or put back: error is why.
settings_file_chooser_failed = What answers Open and Save dialogs is unchanged: { $error }

# Said under the Open and save dialogs setting where the user's portal configuration (its file or its folder) is a link, kept by a program such as home-manager or a dotfiles folder: nothing can be chosen here; target is where the link leads.
settings_file_chooser_linked = Kept by another program: your portal configuration is a link to { $target }, which Skwir leaves as it is. Choose there

# Said under the Open and save dialogs setting where Skwir's package did not install the file that offers it to the desktop portal (a build run from its folder).
settings_file_chooser_not_installed = Skwir cannot be chosen here: its portal file is not installed

# Said under the Open and save dialogs setting when its configuration was changed after the desktop portal read it: what the setting shows is not what answers yet.
settings_file_chooser_pending = Changed since the desktop portal started: this applies at its next start, the next login

# Said once a choice of what answers other programs' dialogs is undone, with Redo.
settings_file_chooser_put_back = What answered other programs' Open and Save dialogs is back, from the desktop portal's next start

# Said where Undo of a choice of what answers other programs' Open and Save dialogs is asked again, or of a choice this Skwir did not make.
settings_file_chooser_put_back_gone = That choice of what answers Open and Save dialogs was undone already

# Said under the Open and save dialogs setting while it is read.
settings_file_chooser_reading = Reading the desktop portal's configuration…

# The Open and save dialogs setting's value where the portal's configuration says none answers them (its 'none'): programs that ask the portal get no dialog.
settings_file_chooser_refused = None: the desktop portal shows no dialog

# The Open and save dialogs setting's choice when the portal's configuration names nothing for them: the desktop's own way of choosing.
settings_file_chooser_unnamed = As the desktop chooses

settings_files_opacity = Files background opacity

settings_files_opacity_description = How much the files and the terminal hide what is behind the window; at 100% they hide it.

settings_files_text_size = Files text size

settings_files_text_size_description = How large names and details are written in the list, the grid, the columns and the board; 100% is the interface's size.

# What the Files text size setting's description says after its first sentence, in its tooltip.
settings_files_text_size_more = Rows and labels grow with the text, while icons and pictures keep the zoom's size.

# points: the size 100% comes to here, in points (10.5).
settings_files_text_size_now = How large names and details are written in the list, the grid, the columns and the board; 100% is the interface's size, now { $points } pt.

# The button adding a name to the list of files a folder's preview looks for.
settings_folder_preview_add_name = Add name

settings_folder_preview_file = Show a README or a cover

settings_folder_preview_file_description = In the preview, show a folder by a file in it that says what it holds: its README, or the cover of an album or of a folder of photos.

# What a setting's description says after its first sentence, in its tooltip.
settings_folder_preview_file_description_more = The file shown is the first of the names below that the folder holds. The card, the count and Properties stay the folder's; the × leaves the file and shows the folder's count. Off, a folder's preview says how many items it holds.

# A field of a file's name a folder's preview looks for.
settings_folder_preview_name = Name

settings_folder_preview_name_description = A file's name, or a pattern where * stands for any characters

settings_folder_preview_names = Names

settings_folder_preview_names_description = The files looked for, the first first: a name, or a pattern where * stands for any characters (cover.* finds cover.jpg and cover.png).

# What a setting's description says after its first sentence, in its tooltip.
settings_folder_preview_names_description_more = Letter case is ignored. A folder is shown by the file of the first name it holds that Skwir can preview; a file it cannot preview is passed over for the next one found.

settings_font = Font

settings_font_description = The font of Skwir's menus, lists, bars and dialogs.

# Fonts from is the name of another setting; Inter is a font's name.
settings_font_description_more = Automatic is your desktop's, as Fonts from reads it, else Skwir's own, Inter.

# Said by settings.check of a value it does not know: it may be among the fonts installed.
settings_font_more = the fonts installed

# Said under a list of fonts in place of the specimen, where the font highlighted fails to load.
settings_font_not_drawn = This font cannot be drawn

settings_fonts_from = Fonts from

settings_fonts_from_description = Where Skwir reads the fonts a font setting's Automatic writes in, and their sizes.

# What Automatic comes to here, after the setting's description. font: a font's family; points: its size in points (10.5); from: where it was read, a desktop's or a toolkit's name (Plasma, GTK).
settings_fonts_from_now_read = Now { $font } at { $points } pt, sizes read from { $from }.

# What Automatic comes to here, after the setting's description. font: a font's family; points: its size in points (10.5).
settings_fonts_from_now_usual = Now { $font } at { $points } pt, the system's usual size, as no desktop names one.

# Inter and JetBrains Mono are fonts' names.
settings_fonts_from_system = Automatic is the system font, at the size its applications write in; Skwir's own uses the fonts Skwir comes with, Inter and JetBrains Mono, at the same size.

# now: what Automatic comes to here, a sentence (settings_fonts_from_now_read). Inter and JetBrains Mono are fonts' names.
settings_fonts_from_system_now = Automatic is the system font, at the size its applications write in; Skwir's own uses the fonts Skwir comes with, Inter and JetBrains Mono, at the same size. { $now }

# Inter and JetBrains Mono are fonts' names; Skwir's own, GTK and Qt are the setting's choices.
settings_fonts_from_toolkits = Automatic reads your desktop's own settings, else GTK's, else Qt's. Skwir's own uses the fonts Skwir comes with, Inter and JetBrains Mono, at the sizes Automatic reads. GTK or Qt reads that toolkit's settings alone, for when the two name different fonts.

# now: what Automatic comes to here, a sentence (settings_fonts_from_now_read). Inter and JetBrains Mono are fonts' names; Skwir's own, GTK and Qt are the setting's choices.
settings_fonts_from_toolkits_now = Automatic reads your desktop's own settings, else GTK's, else Qt's. Skwir's own uses the fonts Skwir comes with, Inter and JetBrains Mono, at the sizes Automatic reads. GTK or Qt reads that toolkit's settings alone, for when the two name different fonts. { $now }

# Text size and Accessibility are as Windows names them; Inter and JetBrains Mono are fonts' names.
settings_fonts_from_windows = Automatic is Windows' interface font, at the size its new applications write in, times the Text size of its Accessibility settings; Skwir's own uses the fonts Skwir comes with, Inter and JetBrains Mono, at the same size.

# now: what Automatic comes to here, a sentence (settings_fonts_from_now_read). Text size and Accessibility are as Windows names them; Inter and JetBrains Mono are fonts' names.
settings_fonts_from_windows_now = Automatic is Windows' interface font, at the size its new applications write in, times the Text size of its Accessibility settings; Skwir's own uses the fonts Skwir comes with, Inter and JetBrains Mono, at the same size. { $now }

# The caption over settings found or changed: the page they are on, then their section.
settings_found_on = { $page } › { $section }

settings_full_path = Show the full path

settings_full_path_description = Show a folder's whole path above its files, from the root, each folder under its own name: / › home › someone › Documents › 2026.

# What a setting's description says after its first sentence, in its tooltip.
settings_full_path_description_more = Off starts it at the deepest folder of the sidebar that holds it, under the sidebar's name: Documents › 2026, a drive's name for a folder on a drive.

# A grammar the user added, as a choice on the Reading page: program is where it was added (bat, Sublime Text), grammar its name.
settings_grammar_added = { $program }: { $grammar }

# A language's line on the Reading page: detail is what it says before, program where a grammar was added (bat, Sublime Text), grammar its name, problem why it was left out.
settings_grammar_added_problem = { $detail } · { $program }'s { $grammar }: { $problem }

# A language's choice of grammar on the Reading page: Automatic, and what it colors with now (bat, Zed (tree-sitter), Plain text).
settings_grammar_automatic = Automatic ({ $now })

# A grammar Automatic colors with while it is checked to fit: grammar is its choice's words (Zed (tree-sitter)).
settings_grammar_being_checked = { $grammar }, being checked

# Under a combined grammar's choice, while it is checked to fit.
settings_grammar_checking = Checking…

# Under a combined grammar's choice on the Reading page: folder is the name of the repository its queries are in.
settings_grammar_colors_folder = its colors from { $folder }

# Under a combined grammar's choice on the Reading page: whose queries color it. Zed, Helix, Neovim and Emacs are editors' names.
settings_grammar_colors_queries = its colors from { $source ->
        [skwir] Skwir's queries
        [tree-sitter] the tree-sitter CLI's queries
        [zed] Zed's queries
        [helix] Helix's queries
        [neovim] Neovim's queries
        [emacs] Emacs's queries
       *[system] the system's queries
    }

# A grammar combined from two sources, as a choice on the Reading page says it: queries is where its queries are from (tree-sitter CLI, Zed), library chooses whose library it loads. Zed, Helix, Neovim and Emacs are editors' names.
settings_grammar_combined = { $queries }, with { $library ->
        [skwir] a library in Skwir's folder
        [tree-sitter] the tree-sitter CLI's library
        [zed] Zed's library
        [helix] Helix's library
        [neovim] Neovim's library
        [emacs] Emacs's library
       *[system] the system's library
    }

# The button adding a folder to the list of where grammars were looked for.
settings_grammar_folder_add = Add Folder

# A column of the list of where grammars were looked for: the languages found in the folder.
settings_grammar_folder_held = What it held

# A column of the list of where grammars were looked for: the folder read.
settings_grammar_folder_name = Folder

settings_grammar_folders = Where grammars were looked for

# tree-sitter is a program's name.
settings_grammar_folders_description = Each folder Skwir read for tree-sitter grammars, and the languages it held.

# tree-sitter is a program's name; CLI is its command-line program.
settings_grammar_folders_description_more = What was nearly found comes first, each saying what to do: a repository a folder too deep, a library with no queries to color with, a folder the tree-sitter CLI's configuration names that is not there.

# A list's title on the Reading page: count is how many grammars were nearly found, listed first.
settings_grammar_folders_nearly = Where grammars were looked for: { $count ->
        [one] { $count } nearly found
       *[other] { $count } nearly found
    }

# What a setting kept as data holds, for agents and scripts: bat, plain, skwir, tree-sitter, zed, helix, neovim, system and emacs are its values, as written.
settings_grammar_for_data = What colors each language chosen on the Reading page: a language's name, in lower case, to bat, plain, or where its tree-sitter grammar was found (skwir, tree-sitter, zed, helix, neovim, system, emacs)

# A language's line on the Reading page: detail is what it says before (the files it colors); the grammar chosen, bat's (a program), is gone, so Automatic colors it.
settings_grammar_gone_bat = { $detail } · bat's grammar chosen is not here: Automatic

# A language's line on the Reading page: detail is what it says before (the files it colors); Plain text, the choice made, is gone, so Automatic colors it.
settings_grammar_gone_plain = { $detail } · Plain text chosen is not here: Automatic

# A language's line on the Reading page: detail is what it says before (the files it colors), source where the grammar chosen was found (Zed, Helix), which is gone, so Automatic colors it.
settings_grammar_gone_tree = { $detail } · { $source }'s grammar chosen is not here: Automatic

# A folder where grammars were looked for: source is whose folder it is (Zed, tree-sitter CLI), languages the languages it held, a list, as their grammars name them.
settings_grammar_held_languages = { $source }: { $languages }

# A folder where grammars were looked for: source is whose folder it is (Zed, tree-sitter CLI), count how many languages it held, languages a list of the first of them, as their grammars name them, then how many more.
settings_grammar_held_more = { $source }: { $count ->
        [one] { $count } language
       *[other] { $count } languages
    }: { $languages }

# A folder where grammars were looked for, which held none: source is whose folder it is (Zed, tree-sitter CLI).
settings_grammar_held_nothing = { $source }: nothing

# The last item of the languages a folder of grammars held, after the first few by name: how many others (rust, python and 12 more).
settings_grammar_held_others = { $count ->
        [one] { $count } more
       *[other] { $count } more
    }

# What was nearly found where grammars were looked for: source is whose folder it is (Zed, tree-sitter CLI), said what is wrong with it and what to do.
settings_grammar_nearly = { $source }: { $said }

# An error of ui.grammar_choose, asked with no language.
settings_grammar_no_language = No language is named

# Said of a source of grammars on the Reading page (Zed, the tree-sitter CLI): folders are the folders it is read in, a list of paths, none of which exists.
settings_grammar_none_there = none of its folders is there: { $folders }

# Under a combined grammar's choice: why is why the helper could not check it.
settings_grammar_not_checked = Not checked ({ $why }): checked again when the settings open

# error: why the settings could not be saved.
settings_grammar_not_saved = The grammar chosen was not saved: { $error }

# A language's choice of grammar on the Reading page: no colors.
settings_grammar_plain = Plain text

# A tree-sitter grammar as a choice on the Reading page says it: source is where it was found (Zed, Helix); tree-sitter is a program's name.
settings_grammar_tree = { $source } (tree-sitter)

# An error of ui.grammar_choose: grammar is what was asked, as written.
settings_grammar_unknown = { $grammar } is no grammar to choose

settings_grid_fill = Fill thumbnails

settings_grid_fill_description = Crop each thumbnail in the grid to a square, so a folder of pictures reads as a gallery.

# What a setting's description says after its first sentence, in its tooltip.
settings_grid_fill_description_more = Off shows every picture whole.

# What the window keeps, for agents and scripts.
settings_hints_state = Commands whose shortcut hint was already shown

settings_icon_theme = Icon theme

settings_icon_theme_description = The icons Skwir draws files, folders and places with.

# What a setting's description says after its first sentence, in its tooltip.
settings_icon_theme_description_more = Automatic follows your desktop's choice.

# Said by settings.check of a value it does not know: it may be among the icon themes installed.
settings_icon_theme_more = the icon themes installed

# Said where the settings file would be named, when there is none.
settings_in_memory = Settings are kept in memory for this session.

# What a page of the settings window says under its title.
settings_intro_appearance = How Skwir looks.

# What a page of the settings window says under its title.
settings_intro_changed = Every setting that differs from its default and every plugin turned on or off from how Skwir comes, each with its way back.

# What a page of the settings window says under its title.
settings_intro_exclusions = The files and folders Skwir leaves out: of searches, tag counts and the timeline below the folder they look in, and of what agents, plugins from outside Skwir and models see. Skwir's own come in categories, each with its switch; tick or untick each, or add yours. The indexes Skwir can tell keep your Secrets out too (Index).

# What the Exclusions page of the settings window says under its title, where no index's page is listed.
settings_intro_exclusions_no_index = The files and folders Skwir leaves out: of searches, tag counts and the timeline below the folder they look in, and of what agents, plugins from outside Skwir and models see. Skwir's own come in categories, each with its switch; tick or untick each, or add yours.

# What a page of the settings window says under its title.
settings_intro_general = The language Skwir speaks, what it does at startup and with folders other programs open, with tabs, the split and the address, with what you drag and drop, and while an action runs. Changes apply as you make them.

# What a page of the settings window says under its title.
settings_intro_index = What makes searches fast: an index that answers which files have a name, where it proves it is current, and which folders it keeps current as they change. Where no index answers, a search looks through every folder.

# What a page of the settings window says under its title.
settings_intro_keyboard = Every action and its keys, grouped by where they work. Click a key to change it, or + to add one: yours replace Skwir's on the same keys there.

# What a page of the settings window says under its title.
settings_intro_media = How pictures follow one another, how audio and video play, how 3D models are drawn and what maps are drawn on.

# What a page of the settings window says under its title.
settings_intro_network = The servers of the sidebar's Network section, added, edited and removed here as from their menus, and where those added by their address are kept; then the tools that open an address.

# What a page of the settings window says under its title.
settings_intro_notes = Tags, ratings and comments: how tags are offered, and the places they are kept in, so that the desktop, a Mac and other applications see them too. Every place is read, and what they hold is merged. Each is written as you choose: never; where the file already has notes there; there, and also where the file's volume keeps no attributes (one place is then started, the one its folder already uses first); or for every file edited.

# What a page of the settings window says under its title.
settings_intro_plugins = Everything beyond the core is a plugin. Turning one off removes its commands, views and settings until it is turned on again.

# What a page of the settings window says under its title.
settings_intro_privacy = What leaves this machine. An agent driving Skwir, and a plugin from outside Skwir, never see a private folder: not its names, not what its files hold. What each plugin may do is said on its line of the Plugins page.

# What a page of the settings window says under its title.
settings_intro_reading = How documents, books, comics, Markdown, text and code read in the preview, and which engine reads each of them.

# What a page of the settings window says under its title.
settings_intro_terminal = The terminal panel, and the shell that runs a command on the selection.

# What a page of the settings window says under its title.
settings_intro_views = How a folder shows its files: their order, how their sizes are counted, the grid, the list's columns, the thumbnails drawn for them, what its preview shows, and what Git says of them.

settings_keep_awake = Keep the computer awake while an action runs

settings_keep_awake_description = While a copy, a move or another action runs, and is not paused, the computer does not go to sleep by itself.

# What a setting's description says after its first sentence, in its tooltip.
settings_keep_awake_description_more = Closing a laptop's lid still puts it to sleep.

# The button adding a language to the list of languages grammars color, on the Reading page.
settings_language_add = Add Language

# A column of the list of languages on the Reading page: the files a language colors.
settings_language_files = Files

# A column of the list of languages on the Reading page: the language's name.
settings_language_name = Language

# A list's title on the Reading page: the languages grammars color.
settings_languages = Languages

settings_languages_description = Each language a grammar was found or added for, with the files it is written in and the grammar that colors them.

# What the Languages list's description says after its first sentence, in its tooltip. folder: where Skwir's grammars folder is (~/.local/share/skwir/grammars). Automatic and Plain text are choices; bat, tree-sitter, Sublime Text, Zed, Helix, Neovim and Emacs programs' names; languages.toml a file's name.
settings_languages_description_at = Automatic follows the choice above. bat colors with its own grammar, or with one you added to bat or Sublime Text. Each tree-sitter grammar is named by where it was found, and one that cannot be used is greyed, with the reason. Plain text shows the files without colors. Tree-sitter grammars are looked for in Skwir's grammars folder ({ $folder }) and where the tree-sitter CLI, Zed, Helix, Neovim, Emacs and the system keep them. A grammar's repository put in Skwir's grammars folder colors with its own queries, built or not: unbuilt, its queries color the grammar of the same language found elsewhere (Emacs keeps no queries with its grammars). There, languages.toml says which files a language colors.

# What the Languages list's description says after its first sentence, in its tooltip. Automatic and Plain text are choices; bat, tree-sitter, Sublime Text, Zed, Helix, Neovim and Emacs programs' names; languages.toml a file's name.
settings_languages_description_more = Automatic follows the choice above. bat colors with its own grammar, or with one you added to bat or Sublime Text. Each tree-sitter grammar is named by where it was found, and one that cannot be used is greyed, with the reason. Plain text shows the files without colors. Tree-sitter grammars are looked for in Skwir's grammars folder and where the tree-sitter CLI, Zed, Helix, Neovim, Emacs and the system keep them. A grammar's repository put in Skwir's grammars folder colors with its own queries, built or not: unbuilt, its queries color the grammar of the same language found elsewhere (Emacs keeps no queries with its grammars). There, languages.toml says which files a language colors.

settings_launch_opens = Open folders from other programs in

# Show in Folder is what browsers call the command; `skwir`, in backquotes, is the command typed in a terminal, written as it is.
settings_launch_opens_description = Where a folder opens when another program opens it in a running Skwir: a browser's Show in Folder, a launcher, or `skwir` and a folder typed in a terminal.

# What a setting's description says after its first sentence, in its tooltip.
settings_launch_opens_description_more = With A new window, the folder opens in a window of its own; with A tab, it opens in a tab of the window used last, and Skwir started again with no folder brings that window forward.

# Where a folder another program opens in Skwir goes: a tab of the window used last.
settings_launch_opens_tab = A tab

# Where a folder another program opens in Skwir goes.
settings_launch_opens_window = A new window

# The button lowering a number in the settings by a step.
settings_less = Less

# A setting's choice: the light look.
settings_light = Light

settings_model_perspective = Perspective

settings_model_perspective_description = Draw 3D models in perspective, nearer parts larger, as a camera sees them.

# What a setting's description says after its first sentence, in its tooltip.
settings_model_perspective_description_more = Off, every part keeps its size at any depth, as in the model's picture and in a drawing.

settings_model_smooth = Smooth shading

settings_model_smooth_description = Shade 3D models smoothly across their faces, as a scanned or sculpted surface reads best.

# What a setting's description says after its first sentence, in its tooltip.
settings_model_smooth_description_more = Off, each face is shaded flat, so a printed part's facets and edges show.

# The button raising a number in the settings by a step.
settings_more = More

# The button at a page's end showing the settings it folds: count is how many.
settings_more_settings = More settings ({ $count })

# What the field above a long list of choices shows while empty.
settings_narrow_choices = Type to narrow

settings_no_file = There is no settings file yet: every setting has its default value

# Said on the Plugins page when its filter keeps none: typed is what was typed.
settings_no_plugin = No plugin matches { $typed }.

# A choice of a setting, kept though it is not installed: name is a font's family or an icon theme's folder, as written.
settings_not_installed = { $name } (not installed)

# Said on Changed by you when nothing differs from how skwir comes.
settings_nothing_changed = Every setting has its default value, and every plugin is as Skwir comes.

# Said when the settings' search finds nothing: typed is what was typed.
settings_nothing_found = Nothing matches { $typed }.

settings_notify_desktop = Tell the desktop when a long action ends

settings_notify_desktop_description = The desktop shows a notification when an action that ran 5 seconds or more ends, or waits for its server, while Skwir's window is in the background.

# What a setting's description says after its first sentence, in its tooltip.
settings_notify_desktop_description_more = With the window closed, every action that ends is told. A click on it brings the window back with Activity open. Notifications are taken back once you are back in the window.

settings_opacity = Background opacity

# What the background opacity settings' descriptions say after their first sentence, in their tooltip: below 100%.
settings_opacity_below = Below it, what is behind the window shows through, blurred where your desktop blurs it; text, icons, pictures, menus and dialogs stay whole. Over a busy or contrasting background, text may read less clearly. While your desktop asks for more contrast, the window stays whole.

settings_opacity_description = How much the bars, the tabs, the sidebar and the preview's bars hide what is behind the window; at 100% they hide it.

# The button opening the settings file in an editor.
settings_open_file = Open

# Said on a page of the settings that has none to show.
settings_page_empty = Nothing to adjust here now.

# How many of a page's settings differ from their defaults, after its name in the list of pages.
settings_page_modified = { $count }

# What the button naming the page shown, which drops the pages down in a narrow dialog, is named to a screen reader.
settings_page_named = Page

# What the list of the settings' pages is named to a screen reader.
settings_pages = Pages

# What a setting's path field shows while empty.
settings_path_not_set = Not set

# Said on a plugin's page when skwir was built without it.
settings_plugin_absent = This plugin is not part of this build.

# The switch atop a plugin's own page, turning the plugin on or off.
settings_plugin_enabled = Enabled

# Under a plugin from outside Skwir: where it was installed from, a path.
settings_plugin_from = From { $origin }

# Under a plugin from outside Skwir: what it is allowed to do, a list of what each permission lets it do, each beginning in lower case (reads what files hold and starts programs).
settings_plugin_may = May: { $what }

# Said on a plugin's page while the plugin is off.
settings_plugin_off = Turn the plugin on to change its settings.

# A caption in the list of the settings' pages, above the plugins' own pages.
settings_plugin_pages = Plugin settings

# Under a plugin: a program or library it uses was found; program is its name (ffmpeg), a program's or a library's, which is not known: where a word would agree with it, choose one that need not (available).
settings_plugin_requirement_found = { $program } found

# Under a plugin: a program or library it uses is missing; program is its name (ffmpeg), purpose what it is needed for.
settings_plugin_requirement_missing = { $program } not found — { $purpose }

# The button on a plugin's row going to where its settings are.
settings_plugin_settings = Settings

# A caption over the plugins the settings' search found or that differ from how skwir comes.
settings_plugins_caption = Plugins

# What the Plugins page's filter shows while empty.
settings_plugins_filter = Filter by name or what it does

# The Plugins page's filter, as a screen reader names it.
settings_plugins_filter_label = Filter plugins

settings_preview_opacity = Preview background opacity

settings_preview_opacity_description = How much a preview hides what is behind the window, whatever it shows.

# Properties is the name of a panel.
settings_preview_opacity_description_more = That is text, a table, music, the room around a picture, a page or a film, and Properties beside it; at 100% it hides it. Below it, what is behind the window shows through, blurred where your desktop blurs it; text, icons, pictures, menus and dialogs stay whole. Over a busy or contrasting background, text may read less clearly. While your desktop asks for more contrast, the window stays whole.

settings_printed_pages = Printed page numbers

settings_printed_pages_description = Count where you are by the page numbers a book's printed edition lists, or a PDF's pages are printed with.

# What a setting's description says after its first sentence, in its tooltip.
settings_printed_pages_description_more = That is page 47 of 312 rather than chapter 3 of 12; a book without them counts chapters.

settings_read_ahead = Read ahead

settings_read_ahead_description = While you step through pictures, in the preview panel, in peek or on the whole screen, decode the next one before you reach it, so it shows at once.

# What a setting's description says after its first sentence, in its tooltip.
settings_read_ahead_description_more = The preview reads ahead in the panel once two steps went the same way, and holds one more picture in memory meanwhile. Off, each picture is decoded when you reach it.

# Above every page of the settings when the settings file cannot be written.
settings_read_only = Your settings file is read-only (Home Manager links it, say): a change here is not kept. Change them where they are declared.

settings_reader_continuous = Continuous scrolling

settings_reader_continuous_description = Read a document, a book laid out in pages or a comic as one scrolling column.

# What a setting's description says after its first sentence, in its tooltip.
settings_reader_continuous_description_more = Off shows one page at a time. The reader's own button changes it too.

settings_reader_drag = Drag to move the page

settings_reader_drag_description = A drag across a page's text moves the page, as a hand would, instead of selecting the text.

# What a setting's description says after its first sentence, in its tooltip.
settings_reader_drag_description_more = Where a drag has no text to select (a page's margins and pictures, a page with no text, around and between pages, a picture zoomed in), it moves what is shown either way, and so does a drag with the middle button. The reader's own button changes it too.

settings_reading_font = Reading font

settings_reading_font_description = The font books, Markdown and HTML files, documents and mail are read in.

# Fonts from is the name of another setting.
settings_reading_font_description_more = Automatic is your desktop's document font, as Fonts from reads it, else the interface's.

settings_reading_text_size = Reading text size

settings_reading_text_size_description = The size books, Markdown and HTML files, documents and mail open at; 100% is the size browsers write a page's text at.

# points: the size 100% comes to here, in points (12).
settings_reading_text_size_now = The size books, Markdown and HTML files, documents and mail open at; 100% is the size browsers write a page's text at, now { $points } pt.

settings_reading_width = Reading width

# How wide a reading column may grow: about 85 characters.
settings_reading_width_comfortable = Comfortable

settings_reading_width_description = How long the lines of a book, a Markdown or HTML file may grow before the text stops widening and sits in the middle of the preview.

# Narrow, Comfortable, Wide and Full width are the setting's choices.
settings_reading_width_description_more = Narrow is about 65 characters, Comfortable 85, Wide 110; Full width uses the whole panel, with small margins.

# How wide a reading column may grow: the whole panel.
settings_reading_width_full = Full width

# How wide a reading column may grow: about 65 characters.
settings_reading_width_narrow = Narrow

# How wide a reading column may grow: about 110 characters.
settings_reading_width_wide = Wide

# Why a choice of a setting does nothing here: name is a program's or a library's name, purpose what it is for.
settings_requirement_missing = { $name } not found: { $purpose }

# The button giving every setting back its default, after a second click.
settings_restore_all = Restore all…

# Restore all, once clicked, asking to be clicked again to give every setting back its default.
settings_restore_confirm = Click again to restore

# The tooltip of a setting's revert button: default is the value it gives back.
settings_restore_default = Restore the default: { $default }

# The row at the end of Changed by you giving every setting back its default.
settings_restore_defaults = Restore defaults

# The tooltip of a setting's revert button where its default is to be not set.
settings_restore_unset = Restore the default (not set)

# The button restoring a setting's default value.
settings_revert = Revert

# After a page's title, for a moment, once a change of the settings is saved.
settings_saved = Saved

# After a page's title while a change of the settings is being saved.
settings_saving = Saving…

# What the settings window's search field shows while empty.
settings_search = Search settings

# The title of the settings while their search shows what it found.
settings_search_results = Search results

settings_single_tab = Show a lone tab

settings_single_tab_description = Show the tabs even when the window has one, named like the place it shows.

# New tab and More are a command's and a menu's names.
settings_single_tab_description_more = Off gives the lists the strip's row; New tab stays in the More menu and on Ctrl+T.

# A text size's choice: its percentage (100%), then the size it comes to in points.
settings_size_step_points = { $step } ({ $points } pt)

settings_slideshow = Slideshow

settings_slideshow_description = How long a slideshow shows each picture.

# What a setting's description says after its first sentence, in its tooltip.
settings_slideshow_description_more = A slideshow plays the pictures of the folder or search on the whole screen, in the order they are listed, from the one shown to the last.

# A slideshow's pace: how long it shows each picture.
settings_slideshow_pace = { $count ->
        [one] { $count } second
       *[other] { $count } seconds
    }

# Written in each font offered, under the list of fonts, to show its letters and figures: a short sentence using most letters of the language (a pangram), then the figures.
settings_specimen = Sphinx of black quartz, judge my vow. 0123456789

# A control on a line of a table of settings, as a screen reader names it: section is the line's, setting the column's (Read: Tags in attributes).
settings_table_cell = { $section }: { $setting }

settings_tabs_at = Show tabs

settings_tabs_at_description = Where the tabs are listed.

# What a setting's description says after its first sentence, in its tooltip.
settings_tabs_at_description_more = Above the files, a strip over the panes holds them, side by side. In the sidebar, they are its first group, a row each, and the panes start at the top of the window; while the sidebar is hidden, or the window too narrow for it, they go back above the files.

# Where the tabs are listed: the sidebar's first group.
settings_tabs_at_sidebar = In the sidebar

# Where the tabs are listed: a strip over the panes.
settings_tabs_at_strip = Above the files

settings_text_continuous = Continuous scrolling of text

settings_text_continuous_description = Read a book, a document, a page in reading mode or a message as one scrolling column.

# What a setting's description says after its first sentence, in its tooltip.
settings_text_continuous_description_more = Off shows a screen at a time, ending at a whole line: the wheel, the arrows and Page Down turn it, and past a chapter's last page comes the next chapter. The button beside the text's size changes it too.

settings_text_size = Text size

# desktop: where 100% is read (system: macOS's or Windows'; desktop: the desktop's font settings; usual: none names one).
settings_text_size_description = How large Skwir writes its menus, bars, the sidebar, dialogs and file views; 100% is { $desktop ->
        [system] your system's text size
        [desktop] your desktop's font size
       *[usual] the usual size, as no desktop names one
    }.

# What the Text size setting's description says after its first sentence, in its tooltip.
settings_text_size_more = Bars, rows and menus grow with the text; icons keep their size.

# desktop: where 100% is read (system: macOS's or Windows'; desktop: the desktop's font settings; usual: none names one). points: the size it comes to here, in points (10.5).
settings_text_size_now = How large Skwir writes its menus, bars, the sidebar, dialogs and file views; 100% is { $desktop ->
        [system] your system's text size
        [desktop] your desktop's font size
       *[usual] the usual size, as no desktop names one
    }, now { $points } pt.

# What the Reading and Code text size settings' descriptions say after their first sentence, in their tooltip.
settings_text_size_preview_buttons = The preview's own size buttons still change only the file shown.

settings_theme = Theme

settings_theme_description = Follow the desktop's light or dark preference, or always use one.

# A page of the settings window, as its list and its header name it.
settings_title_appearance = Appearance

settings_title_bar = Title bar

settings_title_bar_description = Where the window's title and its buttons are.

# System, Compact, In the top row and Automatic are the setting's choices. here: what a refusal of the system's means here, a sentence.
settings_title_bar_description_here = System: the system's title bar, or one of Skwir's like it where the system draws none (GNOME). Compact: a slim bar of Skwir's on its frame, its title in the middle. In the top row: the buttons at the ends of Skwir's top row, whose empty parts move the window, as GNOME's applications have it. Automatic is the system's title bar where it draws one, the top row where it does not. { $here }

# System, Compact, In the top row and Automatic are the setting's choices.
settings_title_bar_description_more = System: the system's title bar, or one of Skwir's like it where the system draws none (GNOME). Compact: a slim bar of Skwir's on its frame, its title in the middle. In the top row: the buttons at the ends of Skwir's top row, whose empty parts move the window, as GNOME's applications have it. Automatic is the system's title bar where it draws one, the top row where it does not.

# A page of the settings window, as its list and its header name it.
settings_title_changed = Changed by you

# A page of the settings window, as its list and its header name it.
settings_title_exclusions = Exclusions

# A page of the settings window, as its list and its header name it.
settings_title_general = General

# A page of the settings window, as its list and its header name it: the indexes searches ask.
settings_title_index = Index

# A page of the settings window, as its list and its header name it.
settings_title_keyboard = Keyboard

# A page of the settings window, as its list and its header name it.
settings_title_media = Pictures and media

# A page of the settings window, as its list and its header name it.
settings_title_network = Network

# A page of the settings window, as its list and its header name it.
settings_title_notes = Tags and notes

# A page of the settings window, as its list and its header name it.
settings_title_plugins = Plugins

# A page of the settings window, as its list and its header name it.
settings_title_privacy = Privacy

# A page of the settings window, as its list and its header name it.
settings_title_reading = Reading

# A page of the settings window, as its list and its header name it.
settings_title_terminal = Terminal

# A page of the settings window, as its list and its header name it.
settings_title_views = Folders and views

# What the window keeps, for agents and scripts.
settings_unfolded_state = The settings pages unfolded to show all their settings

settings_up_button = Show an Up button

settings_up_button_description = Show Up after Back and Forward: it goes to the folder that holds the one shown, or to the place before it in its trail, as Alt+Up does.

# What a setting's description says after its first sentence, in its tooltip.
settings_up_button_description_more = Off, the trail's segment before the last goes there.

settings_wrap_lines = Wrap lines

settings_wrap_lines_description = Wrap a text file's long lines to the panel, a line's number on its first row.

# What a setting's description says after its first sentence, in its tooltip.
settings_wrap_lines_description_more = Off, they go on sideways and the panel scrolls with them.

# What a screen reader calls the address field.
shell_address = Address

# What the address field shows while it is empty; query: is a word of the search language, as typed.
shell_address_placeholder = Path, query:… or location

# Said where F2 asks to rename a place that cannot be.
shell_cannot_rename = This cannot be renamed

# Said where a value is compared with the clipboard and differs; what: what it is, as the link says it (SHA-256).
shell_compared_differs = The { $what } differs from the one copied

# Said where a text is compared with the clipboard and differs.
shell_compared_differs_text = The text differs from the one copied

# Said where a value is compared with the clipboard, which holds none like it; what: what it is, as the link says it (SHA-256).
shell_compared_nothing = No { $what } is copied to compare with

# Said where a text is compared with the clipboard, which holds none like it.
shell_compared_nothing_text = No text is copied to compare with

# Said where a value is compared with the clipboard and it is the same; what: what it is, as the link says it (SHA-256).
shell_compared_same = The { $what } is the one copied

# Said where a text is compared with the clipboard and it is the same.
shell_compared_same_text = The text is the one copied

# Said once a text is copied from a link beside it.
shell_copied_text = Copied text

# Said once a value is copied; what: what it is, as the link that copied it says it (the SHA-256).
shell_copied_what = Copied { $what }

# The title of the prompt finding a name after the cursor.
shell_find = Find

# The title of the prompt finding a name before the cursor.
shell_find_before = Find before

# Said where Find again finds no name holding what was typed (pattern, as typed).
shell_find_none = No name holds { $pattern }

# Said where Find again is asked before anything was found.
shell_find_nothing = Nothing to find again: press / first

# Said where a file renamed or revealed is hidden: hidden files were turned on to show it.
shell_hidden_shown = Hidden files are shown, { $name } among them

# The button of the dialog making a new file that makes it.
shell_new_file_accept = Create

# What the field of the dialog making a new file shows while empty: a name ending in / makes a folder.
shell_new_file_placeholder = File name, or a folder name ending in /

# The title of the dialog making a new file.
shell_new_file_title = New file

# The button of the dialog making a new folder that makes it.
shell_new_folder_accept = Create

# What the field of the dialog making a new folder shows while empty.
shell_new_folder_placeholder = Folder name

# The title of the dialog making a new folder.
shell_new_folder_title = New folder

# Said where Shift+F5 or Shift+F6 gives the selection to the other pane and the window is not split.
shell_no_other_pane = There is no other pane: split the window first

# Said where ui.text_size names a part that is none; part: as given (files, reading).
shell_no_such_part = No part of the window is called { $part }

# What a pane shows where no view can draw it (none is registered).
shell_no_view = no view registered

# Said where the window is asked a command it declares but does not carry out; ui.{ $command } is the command's id.
shell_not_carried_out = ui.{ $command } is not carried out here

# Said where a new place is asked for where the location shown makes none.
shell_nothing_new = Nothing new can be made here

# Said when a command needs a selection and there is none.
shell_nothing_selected = Select a file or folder first

# What a screen reader hears of what a pane shows in place of its files: what it says, then more of it (This folder is empty. Drop files here).
shell_placeholder_said = { $title }. { $detail }

# What Quit asks while an action waits for its server to go on.
shell_quit_one_waits_for_server = An action waits for its server to answer again, to go on by itself. Skwir can wait for it without its window, then quit. Or stop now: it does not go on, and what it finished stays.

# What Quit asks while an action waits for its server to go on. action: the action, by its title, capitalised (Copy to stick), as a label before what is said of it, never the subject of a verb.
shell_quit_waits_for_server = { $action }: this action waits for its server to answer again, to go on by itself. Skwir can wait for it without its window, then quit. Or stop now: it does not go on, and what it finished stays.

# The button of the dialog renaming a file that renames it.
shell_rename_accept = Rename

# Said where F2 asks to rename and not one entry is selected.
shell_rename_one = select exactly one entry to rename

# What the field of the dialog renaming a file shows while empty.
shell_rename_placeholder = New name

# The title of the dialog renaming a file.
shell_rename_title = Rename

# Two sentences said of renamed entries: hidden files turned on to show one (Hidden files are shown, “.a” among them), then what is still not shown (2 renamed items are not shown here).
shell_renamed_both = { $unhid }. { $left }

# Said where what was renamed is not shown, as the pane's filter leaves it out; name: the one renamed.
shell_renamed_filtered = { $count ->
        [1] { $name } is renamed, but the filter leaves it out
       *[other] { $count } renamed items are left out by the filter
    }

# Said where what was renamed is not shown in the pane; name: the one renamed.
shell_renamed_not_shown = { $count ->
        [1] { $name } is renamed, but not shown here
       *[other] { $count } renamed items are not shown here
    }

# Said where a link asked to show a file that is gone.
shell_reveal_gone = { $name } is not there

shell_reveal_not_here = Skwir follows no link to another machine or a drive not mounted yet

# Said as a part's text is made larger or smaller: the part (Files text), then its size (120%).
shell_text_size = { $part }: { $size }

# Said to a command naming a view that is not registered; view: its id, as given.
shell_unknown_view = unknown view `{ $view }`

# A button of the overview of shortcuts: opens the Keyboard page of the settings.
shortcuts_all = All shortcuts…

shortcuts_close = Close

# A heading of the overview of shortcuts at the top of a column whose rows began in the one before; heading: the topic.
shortcuts_continued = { $heading }, continued

# The overview of shortcuts: the heading of the keys found that work in other places than where one is.
shortcuts_elsewhere = Elsewhere

# The field of the overview of shortcuts (F1), before anything is typed.
shortcuts_filter = Filter by action or keys

# The overview of shortcuts finds nothing; query: what was typed.
shortcuts_none = No shortcut matches { $query }.

# Said once after a command was run three times with the pointer; keys: its shortcut; action: the command's title as its menu writes it, quoted (Copy, Rename…).
shortcuts_tip = Tip: press { $keys } for { $action }. F1 lists the keys for where you are.

# The title of the overview of shortcuts F1 opens: the keys that work where one is.
shortcuts_title = Shortcuts here

# What a screen reader calls a volume's eject button in the sidebar; place: the volume, as the sidebar names it.
sidebar_eject = Eject { $place }

# An item of a sidebar section's menu: hide the places the section holds, its heading left.
sidebar_fold = Fold

# Said after a place was hidden from the sidebar, with Undo. place: its name, quoted, whose gender is not known: where words would agree with it, name it as a place (the place “Docs” hidden).
sidebar_hid = Hid { $place } from the sidebar

# The button showing again a place just hidden from the sidebar.
sidebar_hid_undo = Undo

# Said when a place the user keeps (a bookmark, a saved search) is arranged as the sidebar arranges the places it offers: its own menu renames, moves or removes it. place: its name, quoted, whose gender is not known: where words would agree with it, name it as a place (the place “Docs”).
sidebar_kept_by_owner = { $place } is kept in the sidebar by its owner: its menu arranges it

# An item of a sidebar section's menu: put the section after the one below it.
sidebar_move_down = Move Down

# An item of a sidebar section's menu: put the section before the one above it.
sidebar_move_up = Move Up

# What a screen reader calls the sidebar: the places, bookmarks, tags and devices at the window's left.
sidebar_name = Sidebar

# Said when a place of the sidebar was arranged (hidden, moved, renamed) as it went from the sidebar.
sidebar_place_gone = That place is no longer in the sidebar

# The button that renames a place in the sidebar.
sidebar_rename_accept = Rename

# The empty field of the question renaming a place in the sidebar: left empty, the place takes back its own name, own.
sidebar_rename_placeholder = Name (empty: { $own })

# The title of the question renaming a place the sidebar shows (Home, a volume) under a name of the user's, there only.
sidebar_rename_title = Rename { $place } in the sidebar

# An item of a sidebar section's menu: give the section back its places as their owners offer them, none hidden, moved or renamed; section: the section's heading (Places, Devices).
sidebar_reset_section = Reset { $section }

# An item of the sidebar sections' menu: every section back where it was, unfolded and shown.
sidebar_reset_sections = Reset Sections

# Where a folder's trail starts when the sidebar shows none of the folders above it: the home folder.
# noun
sidebar_root_home = Home

# A volume's tooltip in the sidebar: the room left on it, of its size (12.3 GB free of 500 GB).
sidebar_space = { $free } free of { $total }

# A tag's tooltip in the sidebar: its name, then what a click with Shift does (shows only what has this tag too) and with Alt (its files in the folder shown).
sidebar_tag_tip = { $tag } · Shift+click: only what has it too · Alt+click: in this folder

# Said when the sidebar is asked for (F9, F6) in a window too narrow to show it beside the files.
sidebar_too_narrow = The window is too narrow for the sidebar

# An item of a sidebar section's menu: show the places the folded section holds.
sidebar_unfold = Unfold

# The text size of code, named as a change of it is said (Code text: 120%).
sizes_code = Code text

# The text size of the files in a pane, named as a change of it is said (Files text: 120%).
sizes_files = Files text

# The interface's text size, named as a change of it is said (Text size: 120%).
sizes_interface = Text size

# The text size of what previews show to read, named as a change of it is said (Reading text: 120%).
sizes_reading = Reading text

# A text size, a percentage of its part's own size (110%).
sizes_step = { $step }%

# What a screen reader hears of a pane's footer, fact after fact: what it said so far (12 items), then the next (3 selected).
status_and = { $said }, { $fact }

# In a pane's footer, after its count, when what answered a search (an index) was last brought up to date; when: the moment (Today 03:00).
status_as_of = index as of { $when }

# In a pane's footer, after its count: what answered a search, an index whose last update cannot be told (plocate's database).
status_as_of_index = as of { $index }

# The tooltip of a footer's "as of": index: what answered (plocate's database).
status_as_of_index_tip = Found in { $index }, whose last update cannot be told: what was made, renamed or removed since is not in this answer.

# The tooltip of a footer's "index as of": index: what answered (plocate's database); when: the moment it was last brought up to date.
status_as_of_tip = Found in { $index }, last brought up to date { $when }: what was made, renamed or removed since is not in this answer.

# In a pane's footer, after its count, when an index answered a search and had compared what it knew with the folders; when: the moment (Today 14:02).
status_checked = index checked { $when }

# The tooltip of a footer's "index checked": index: what answered (skwird's index); when: the moment it compared.
status_checked_tip = Found in { $index }, which compared what it knew with the folders { $when }.

# In the window's footer: how many items wait to be copied by a paste.
status_copied = { $count ->
        [one] { $count } item copied
       *[other] { $count } items copied
    }

# In the window's footer: how many items wait to be moved by a paste.
status_cut = { $count ->
        [one] { $count } item cut
       *[other] { $count } items cut
    }

# In a search's footer: how many files and folders the user's exclusions left out.
status_excluded = { $count ->
        [one] { $count } excluded
       *[other] { $count } excluded
    }

# The tooltip of a search footer's "excluded"; excluded:yes is a term of the query language, as it is typed.
status_excluded_tip = Files and folders your exclusions leave out (Settings › Exclusions) were walked past: excluded:yes searches them too

# In a pane's footer: how many files its folded groups hold.
status_folded = { $count ->
        [one] { $count } folded
       *[other] { $count } folded
    }

# The tooltip of a footer's "folded"; Right is the arrow key.
status_folded_tip = Files in folded groups: a heading's chevron, or Right on it, opens its group again

# A pane's footer: how many items it lists.
status_items = { $count ->
        [one] { $count } item
       *[other] { $count } items
    }

# A pane's footer, nothing selected: how many items it lists (12 items), then what the files shown weigh.
status_items_weigh = { $items } · { $size }

# In a pane's footer, after its count, when an index that follows every change (skwird's) answered a search.
status_live = index up to date

# The tooltip of a footer's "index up to date": index: what answered (skwird's index).
status_live_tip = Found in { $index }, which follows every change: this answer is the files as they are now.

# A pane's footer while its folder is read, before anything was.
status_loading = Loading…

# A pane's footer while its folder is read: how many items were read so far.
status_loading_items = Loading… { $count ->
        [one] { $count } item
       *[other] { $count } items
    }

# A pane's footer while a filter looks in the folders below for what it matches: how many items it shows so far (3 of 12 items).
status_looking = { $items } · Looking in folders…

# In a search's footer, after its count, when the search went through the folders one by one because the index that would answer it is not installed (skwird). Its tooltip says why; a click opens Settings at Index, where the window offers Settings.
status_no_index = no index

# Said when Paste is asked and nothing waits to be pasted.
status_nothing_to_paste = Nothing to paste

# A button on a toast whose remedy is a setting: opens Settings at it.
status_open_setting = Open setting

# The tooltip of the footer's items waiting for a paste; keys: the keys that paste (Ctrl+V).
status_pending_keys_tip = Paste with { $keys }. Click to forget.

# The tooltip of the footer's items waiting for a paste.
status_pending_tip = Click to forget.

# In the window's footer: how many plans wait to be reviewed.
status_proposals = { $plans ->
        [one] { $plans } proposal to review
       *[other] { $plans } proposals to review
    }

# In the window's footer: how many plans wait to be reviewed, then how many questions wait for an answer.
status_proposals_questions = { $plans ->
        [one] { $plans } proposal
       *[other] { $plans } proposals
    } to review · { $questions ->
        [one] { $questions } question
       *[other] { $questions } questions
    }

# In the window's footer: how many questions wait for an answer.
status_questions = { $questions ->
        [one] { $questions } question
       *[other] { $questions } questions
    }

# A pane's footer while a filter reads what files' metadata says, before it knows how many: how many items it shows (3 of 12 items).
status_reading = { $items } · Reading…

# A pane's footer while a filter reads what files' metadata says: how many items it shows (3 of 12 items), then how many files are still read.
status_reading_files = { $items } · Reading { $count ->
        [one] { $count } file
       *[other] { $count } files
    }…

# A pane's footer while what its order sorts by is read: how many items it lists (12 items), then how many were read of all.
status_reading_order = { $items } · Reading… { $read } of { $asked }

# A pane's footer once a search ended: how many it found.
status_results = { $count ->
        [one] { $count } result
       *[other] { $count } results
    }

# A pane's footer while a search runs: how many it found so far.
status_searching = Searching… { $count ->
        [one] { $count } result
       *[other] { $count } results
    }

# In a pane's footer: how many items are selected.
status_selected = { $count ->
        [one] { $count } selected
       *[other] { $count } selected
    }

# In a pane's footer: how many items are selected, then what they weigh.
status_selected_weigh = { $count ->
        [one] { $count } selected
       *[other] { $count } selected
    } · { $size }

# A filtered pane's footer: how many items are shown, of how many there are.
status_shown_of = { $count ->
        [one] { $count }
       *[other] { $count }
    } of { $total ->
        [one] { $total } item
       *[other] { $total } items
    }

# In a pane's footer while a sweep deselects what the cursor passes.
status_sweep_deselecting = Deselecting as the cursor moves

# In a pane's footer while a sweep deselects what the cursor passes; keys: the keys that end it (Escape).
status_sweep_deselecting_keys = Deselecting as the cursor moves · { $keys } to stop

# In a pane's footer while a sweep selects what the cursor passes.
status_sweep_selecting = Selecting as the cursor moves

# In a pane's footer while a sweep selects what the cursor passes; keys: the keys that end it (Escape).
status_sweep_selecting_keys = Selecting as the cursor moves · { $keys } to stop

# In the window's footer: what a plugin counts (Shelf), and how many.
status_tally = { $what }: { $count }

# A choice of Fonts from: skwir's own fonts, Inter and JetBrains Mono, rather than the desktop's.
style_own = Skwir's own

# Under a kept place's name in the address field's card; location: where it is (a path); filter: what it is filtered by, as typed.
suggestions_place_filtered = { $location } · { $filter }

# Under a tab's title in the address field's card: Enter shows that tab.
suggestions_switch_tab = Switch to tab

# What a screen reader calls a tab's close button.
tabs_close = Close tab

tabs_new = New tab

# A split tab, as a screen reader hears it and a drag of it shows; first, second: the places its two panes show, as their tabs name them.
tabs_split = { $first } and { $second }

# A tab split more than once, as a screen reader hears it: the places its panes show, as their tabs name them, joined (Docs, Maps and Field Guide).
tabs_split_many = { $places }

# What a screen reader calls the strip of the window's tabs.
tabs_strip = Tabs

# The tag editor's row adding the tag typed, which no file has yet.
tag_editor_add = Add { $tag }

# What a screen reader calls the tag editor's row adding the tags typed (print, Paris).
tag_editor_add_named = Add { $tags }

# The tag editor's row adding the tags typed, each quoted, joined (“print” and “Paris”).
tag_editor_add_several = Add { $tags }

# After a tag in the tag editor: Apply gives it to every file edited, which did not all have it.
tag_editor_added = added

# The tag editor's button giving the files their tags as changed.
tag_editor_apply = Apply

# The tooltip of the tag editor's Apply button: its shortcut (Ctrl+Enter).
tag_editor_apply_tip = Apply ({ $keys })

# The tag editor's button closing it, nothing changed.
tag_editor_cancel = Cancel

# Under a tag in the tag editor, after its colors: the color its name gives it, none chosen.
tag_editor_color_automatic = Automatic

# The tag editor's footer: Enter with nothing typed applies what was changed.
tag_editor_enter_applies = Enter on an empty field applies

# What a screen reader calls the list of tags in the tag editor.
tag_editor_list = Tags

# A chip at the end of a row's tags: how many more tags it has than the row has room for.
tag_editor_more = +{ $count }

# What a screen reader calls the dialog editing the selected files' tags.
tag_editor_name = Tag editor

# Why the tag editor closed: reading the files' tags gave no answer.
tag_editor_no_tags = no tags

# The tag editor's list while no file has a tag.
tag_editor_none_yet = No tags yet: type one and press Enter

# After a tag in the tag editor: how many of the files edited have it, of all (on 2 of 5).
tag_editor_on_some = { $count ->
        [one] on { $count } of { $items }
       *[other] on { $count } of { $items }
    }

# The tag editor's empty field: typing filters the tags listed, or names a new one.
tag_editor_placeholder = Filter, or type a new tag

# After a tag in the tag editor: Apply takes it from the files edited that have it.
tag_editor_removed = removed

# What a screen reader hears of a file after its name, among what marks it (cut and tagged print): its tags, by their own names (print and Paris). Begun in lower case: what begins what is heard is cased in code.
tag_editor_said_tagged = tagged { $tags }

# What a screen reader hears of a file after its name, among what marks it: its tags, by their own names (print and Paris), then its rating (4 stars). Begun in lower case: what begins what is heard is cased in code.
tag_editor_said_tagged_rated = tagged { $tags }; { $stars }

# A file's rating, of five stars: halves is the rating in half stars (0 to 10), stars the whole stars (halves / 2), which choose their words where the rating is whole (0 stars, 1 star, 4 stars).
tag_editor_stars = { $halves ->
        [1] ½ star
        [3] 1½ stars
        [5] 2½ stars
        [7] 3½ stars
        [9] 4½ stars
       *[other] { $stars ->
            [one] { $stars } star
           *[other] { $stars } stars
        }
    }

# Said when the tag editor is asked for while the Tags plugin is off.
tag_editor_tags_off = tags are off: turn the Tags plugin on in Settings

# The tag editor's title: the file whose tags it edits, or how many.
tag_editor_title = { $count ->
        [1] Tags of { $name }
       *[other] Tags of { $count } items
    }

# Why the tag editor closed: the tags read from the files came in a form it could not read. Followed by why.
tag_editor_unreadable = unreadable tags

# The tooltip of the chip that counts the tags a strip has no room for: their names, joined (print, Paris and work).
tags_strip_more_named = { $tags }

# error: why its shell did not start, as the system said it.
terminal_did_not_start = The terminal did not start: { $error }

# What a screen reader calls the terminal panel.
terminal_name = Terminal

terminal_no_link = No link on the terminal's screen

# Said where the letters typed over the terminal's labelled links name none.
terminal_no_such_label = No link has that label

# Said where the selection is to be typed at the terminal's prompt and none is.
terminal_nothing_selected = Nothing is selected

# Said where paths are to be typed at the terminal while it has no shell.
terminal_open_first = Open the terminal first

# Said where paths are to be typed at the terminal while it has no shell; keys: the keys that open it (F4).
terminal_open_first_keys = Open the terminal first ({ $keys })

# A button: open Settings at its Plugins page, to turn the terminal on.
terminal_open_plugins = Open plugins

# Said where F4 asks for the terminal while its plugin is turned off.
terminal_plugin_off = The terminal is a plugin, and it is off

# Said where what was selected in a text preview is longer than a copy takes: only its beginning was copied. size: how much (16.8 MB).
text_copied_first = Copied the first { $size }

# A link named a line past a text file's end. line: the line's number.
text_no_line = The file has no line { $line }

# In a text preview's find field: the words looked for are nowhere in the file.
text_no_match = No match in the file

# In a text preview's find field: the words looked for are not in the part of a large file read so far.
text_no_match_yet = No match in what is counted so far

# A button of the timeline: a bar for each day, week, month or year.
timeline_bucket = { $bucket ->
        [day] day
        [week] week
        [month] month
       *[year] year
    }

# Pointing at a button of the timeline choosing what each bar counts.
timeline_bucket_tip = A bar for each { $bucket ->
        [day] day
        [week] week
        [month] month
       *[year] year
    }

# Pointing at a button of the timeline choosing what each bar counts, where there are too many bars to show at once.
timeline_bucket_tip_windowed = A bar for each { $bucket ->
        [day] day
        [week] week
        [month] month
       *[year] year
    }: too many for the strip, shown a part at a time that the wheel and the arrows move

# A button of the timeline: bars as tall as their number of files.
timeline_by_files = files

# Pointing at the timeline's files button.
timeline_by_files_tip = Bars as tall as their number of files

# A button of the timeline: bars as tall as their files' size.
timeline_by_size = size

# Pointing at the timeline's size button.
timeline_by_size_tip = Bars as tall as their files' size

# Beside the timeline's buttons while it counts the files' dates.
timeline_counting = counting…

# The timeline's button choosing which time its bars count: when files were last opened.
timeline_date_accessed = accessed

# The timeline's button choosing which time its bars count: when files were made.
timeline_date_created = created

# The timeline's button choosing which time its bars count: when files were modified.
timeline_date_modified = modified

# The timeline's button choosing which time its bars count: when pictures and videos were taken.
timeline_date_taken = taken

# Pointing at the timeline's button choosing which time its bars count.
timeline_date_tip = Which time the bars count

# Beside the timeline's buttons: how many files have the date its bars count.
timeline_dated = { $count ->
        [one] { $count } dated
       *[other] { $count } dated
    }

# Beside the timeline's buttons: the days the bars span, or the bar pointed at, first to last (3 Mar 2025 → 9 Mar 2025).
timeline_days = { $first } → { $last }

# Beside the timeline's buttons: how many files are dated before the bars shown.
timeline_earlier_files = { $count ->
        [one] { $count } file earlier
       *[other] { $count } files earlier
    }

# Beside the timeline's buttons: how many files are dated far from all the others (a wrong clock), left out of the bars.
timeline_far_outside = { $count ->
        [one] { $count } far outside
       *[other] { $count } far outside
    }

# What a screen reader hears of a bar of the timeline: how many files it counts, and their size.
timeline_holds = { $count ->
        [one] { $count } file
       *[other] { $count } files
    }, { $size }

# What a screen reader hears of a bar of the timeline: how many files it counts, their size, how many of them are selected.
timeline_holds_selected = { $count ->
        [one] { $count } file
       *[other] { $count } files
    }, { $size }, { $selected ->
        [one] { $selected } selected
       *[other] { $selected } selected
    }

# Beside the timeline's buttons, last: what the pointer does on its bars.
timeline_how = click or drag across bars to list their files

# Beside the timeline's buttons: how many files are dated after the bars shown.
timeline_later_files = { $count ->
        [one] { $count } file later
       *[other] { $count } files later
    }

# Beside the timeline's buttons where there are no dates to count.
timeline_no_data = no data

# Pointing at the timeline's left arrow: show the bars before.
timeline_pan_earlier = Earlier { $bucket ->
        [day] days
        [week] weeks
        [month] months
       *[year] years
    }

# Pointing at the timeline's right arrow: show the bars after.
timeline_pan_later = Later { $bucket ->
        [day] days
        [week] weeks
        [month] months
       *[year] years
    }

# What a screen reader hears of the timeline's bars: which time they count (When modified), what each bar counts, what the strip says beside its buttons, then what the keys do.
timeline_said = { $date } by { $bucket ->
        [day] day
        [week] week
        [month] month
       *[year] year
    }, { $info }. Left and Right go from bar to bar, Shift extends, Enter lists the files

# Beside the timeline's buttons, pointing at a bar: how many of its files are selected.
timeline_selected = { $count ->
        [one] { $count } selected
       *[other] { $count } selected
    }

# A count in the room of a timeline's bar, shortened in a power of ten (power) as CLDR's short compact numbers for the catalog's language count: 3, 6, 9 and 12 in English and French (1.2k, 12k, 1.5M), 4, 8 and 12 in Chinese and Japanese (1.2万, 3億), 3, 5, 7… in Hindi (lakhs, crores). n is the count in that power, with at most one digit after the decimal sign. Give a variant for each power your language counts in.
timeline_short_count = { $power ->
        [3] { $n }k
        [6] { $n }M
        [9] { $n }B
        [12] { $n }T
       *[other] { $n }×10^{ $power }
    }

# A button of the timeline: count the files of the folders inside too.
timeline_subtree = subtree

# Pointing at the timeline's subtree button.
timeline_subtree_tip = Count the files of the folders inside too

# The strip under the files with a bar for each day, week, month or year: its heading.
timeline_title = Timeline

# Beside the timeline's buttons: how many files have not the date its bars count.
timeline_undated = { $count ->
        [one] { $count } without a date
       *[other] { $count } without a date
    }

# What a screen reader calls a bar of the timeline counting a week: the day it begins (3 March 2025).
timeline_week_of = Week of { $date }

# A choice of the timeline's menu of which time its bars count.
timeline_when_created = When created

# A choice of the timeline's menu of which time its bars count.
timeline_when_modified = When modified

# A choice of the timeline's menu of which time its bars count.
timeline_when_opened = When last opened

# A choice of the timeline's menu of which time its bars count: by the camera's clock.
timeline_when_taken = When taken (pictures and videos)

# A choice of the Title bar setting: the system's title bar where it draws one, else the buttons in the top row.
title_bar_automatic = Automatic

# A button of the window's title bar, and what a screen reader calls it.
title_bar_close = Close window

# A choice of the Title bar setting: a slim bar of skwir's on its frame.
title_bar_compact = Compact

# A button of the window's title bar, and what a screen reader calls it.
title_bar_maximize = Maximize window

# A button of the window's title bar, and what a screen reader calls it.
title_bar_minimize = Minimize window

# Said beside the Title bar setting where the desktop kept its frame when asked for none. Compact, In the top row and Automatic are the setting's choices.
title_bar_refused = Here the desktop keeps its own frame around windows, whatever Skwir asks: Compact and In the top row draw Skwir's bar inside it, its corners, shadow and edges the desktop's. Should the desktop draw a title bar too, choose Automatic.

# A button of the window's title bar, and what a screen reader calls it: give a maximized window its size back.
title_bar_restore = Restore window

# A choice of the Title bar setting: the system's title bar, or one of skwir's like it.
title_bar_system = System

# A choice of the Title bar setting: the window's buttons at the ends of skwir's top row.
title_bar_top_row = In the top row

# Said beside the Title bar setting's Compact and In the top row where they cannot be had.
title_bar_unsupported = The window manager keeps its own title bar: no compositor runs, or it cannot leave the frame to Skwir

# The window's title, in its title bar and the system's list of windows; place: the title of where its active pane is (Downloads).
title_bar_window_title = { $place } — skwir

toast_already_as_asked = { $name } was already as asked

# title: the action, capitalised (Copy 3 items to Docs), which found all it was asked done already.
toast_already_done = { $title }: already done

toast_already_gone = { $count ->
        [1] { $name } was already gone
       *[other] { $count } items were already gone
    }

toast_already_in = { $count ->
        [1] { $name } was already in { $folder }
       *[other] { $count } items were already in { $folder }
    }

toast_already_named = { $count ->
        [1] { $name } was already named { $to }
       *[other] { $count } items already had their names
    }

toast_already_restored = { $count ->
        [1] { $name } was already restored
       *[other] { $count } items were already restored
    }

toast_already_swapped = { $first } and { $second } were already swapped

toast_already_there = { $name } was already there

# title: the action an undo found taken back already (copy 3 items to Docs).
toast_already_undone = Already undone: { $title }

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_as_asked = { $name } was already as asked

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_done = { $count ->
        [one] { $count } was already done
       *[other] { $count } were already done
    }

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_gone = { $count ->
        [1] { $name } was already gone
       *[other] { $count } were already gone
    }

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_link_there = { $count ->
        [1] a link to { $name } was already there
       *[other] { $count } were already there
    }

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_made_there = { $name } was already there

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_named = { $count ->
        [1] { $name } was already named { $to }
       *[other] { $count } already had their names
    }

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_restored = { $count ->
        [1] { $name } was already restored
       *[other] { $count } were already restored
    }

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_swapped = { $first } and { $second } were already swapped

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_there = { $count ->
        [1] { $name } was already there
       *[other] { $count } were already there
    }

# Said after what an action did (Copied 3 items to Docs; …).
toast_also_undone = part of it was already undone

# How far the copy of one item is: what it moved of its size (1.1 GB of 4.3 GB).
toast_bar_bytes = { $done } of { $whole }

# How far an action is: the items it finished of all (0 of 2 items). Each count chooses its words: where the noun follows the first number, choose it by finished.
toast_bar_items = { $finished ->
        [one] { $finished } of { $count ->
            [one] { $count } item
           *[other] { $count } items
        }
       *[other] { $finished } of { $count ->
            [one] { $count } item
           *[other] { $count } items
        }
    }

# How far an action is: the items it finished of all, then what it moved of all it moves (1 of 3 items · 3.2 GB of 8.6 GB). Each count chooses its words: where the noun follows the first number, choose it by finished.
toast_bar_items_bytes = { $finished ->
        [one] { $finished } of { $count ->
            [one] { $count } item
           *[other] { $count } items
        }
       *[other] { $finished } of { $count ->
            [one] { $count } item
           *[other] { $count } items
        }
    } · { $done } of { $whole }

# How far an action is: the items it finished of all, then what it moved (1,286 of 40,000 items · 82.3 kB). Each count chooses its words: where the noun follows the first number, choose it by finished.
toast_bar_items_moved = { $finished ->
        [one] { $finished } of { $count ->
            [one] { $count } item
           *[other] { $count } items
        }
       *[other] { $finished } of { $count ->
            [one] { $count } item
           *[other] { $count } items
        }
    } · { $done }

# done: what the action did before it was cancelled (Copied 3 items to Docs).
toast_cancelled_after = Cancelled. { $done }

# title: the action, capitalised (Copy 3 items to Docs).
toast_cancelled_before = { $title }: cancelled before anything was done

toast_close = Close

toast_copied = { $count ->
        [1] Copied { $name } to { $folder }
       *[other] Copied { $count } items to { $folder }
    }

# why: what stopped it, as the system or the step said it (permission denied).
toast_copy_failed = Couldn't copy { $name } to { $folder }: { $why }

# what: what the step said it would do, words that begin with a verb as an instruction does (extract “a.zip” to Downloads): where the word before it would change with the verb's first sound (de, d’), say it so none does; why: what stopped it, as the system or the step said it (permission denied).
toast_could_not = Couldn't { $what }: { $why }

# why: what stopped it, as the system or the step said it (permission denied).
toast_create_failed = Couldn't create { $name }: { $why }

# why: what stopped it, as the system or the step said it (permission denied).
toast_create_folder_failed = Couldn't create folder { $name }: { $why }

toast_created = Created { $name }

toast_created_folder = Created folder { $name }

# why: what stopped it, as the system or the step said it (permission denied).
toast_delete_failed = Couldn't delete { $name }: { $why }

toast_deleted = { $count ->
        [1] Deleted { $name } permanently
       *[other] Deleted { $count } items permanently
    }

# trash: the trash the server keeps what is deleted in, as it names it (Google Drive's trash), words said inside a sentence that carry their own determiner: add no article before it.
toast_deleted_kept = { $count ->
        [1] Deleted { $name } (in { $trash })
       *[other] Deleted { $count } items (in { $trash })
    }

# done: what an action did (Copied 3 items to Docs); found: what it found done already (2 were already there).
toast_done_and_found = { $done }; { $found }

# What an action did, by its title, capitalised (Copy 3 items to Docs), and how many items it went through.
toast_done_items = { $title } ({ $count ->
        [one] { $count } item
       *[other] { $count } items
    })

# done: what an action did; said: what its steps said after (Fold each sheet in half).
toast_done_then_said = { $done }. { $said }

# title: the action, capitalised (Copy 3 items to Docs); error: what stopped it, as the system or the step said it (permission denied).
toast_failed = { $title }: { $error }

toast_folder_already_there = Folder { $name } was already there

# said: what the action's answer says, its server not answering (Couldn't copy “a.txt” to Docs: …).
toast_goes_on = { $said }. It goes on by itself once the server answers again

# An answer, then what was said while it was shown (Renamed “a.txt” to “.a.txt”. Hidden files are shown, “.a.txt” among them).
toast_joined = { $said }. { $then }

toast_link_already_in = { $count ->
        [1] A link to { $name } was already in { $folder }
       *[other] { $count } links were already in { $folder }
    }

# why: what stopped it, as the system or the step said it (permission denied).
toast_link_failed = Couldn't make a link to { $name } in { $folder }: { $why }

toast_linked = { $count ->
        [1] Made a link to { $name } in { $folder }
       *[other] Made { $count } links in { $folder }
    }

# why: what stopped it, as the system or the step said it (permission denied).
toast_move_failed = Couldn't move { $name } to { $folder }: { $why }

toast_moved = { $count ->
        [1] Moved { $name } to { $folder }
       *[other] Moved { $count } items to { $folder }
    }

# done: what an action did (Copied 3 items to Docs); size: what it moved; took: how long it ran (3 min 20 s).
toast_moved_in = { $done } ({ $size } in { $took })

# title: the action, capitalised (Copy 3 items to Docs), which found nothing to do.
toast_nothing_to_do = { $title }: nothing to do

# How fast an action goes (23.4 MB/s), and how long it has left (about 3 min left).
toast_pace = { $speed } · { $left }

# How fast an action writes into a server's folder (23.4 MB/s), which then sends it to the server: why no time left is said.
toast_pace_then_sent = { $speed } · then sent to the server

# What Quit asks while actions run. what: what runs (Copy to stick is paused.); how: the two ways on (Skwir can finish it…).
toast_quit = { $what } { $how }

toast_quit_about_to_begin = A change is about to begin.

# What Quit asks while actions run, after what runs. running: how many actions run, which Skwir finishes or stops; waiting: how many wait their turn after them, which Skwir finishes too.
toast_quit_how = { $running ->
        [1] { $waiting ->
            [0] Skwir can finish it without its window, then quit. Or stop now: what it finished stays, what it was in the middle of is taken back.
            [one] Skwir can finish it, and the { $waiting } action waiting after it, without its window, then quit. Or stop now: what it finished stays, what it was in the middle of is taken back.
           *[other] Skwir can finish it, and the { $waiting } actions waiting after it, without its window, then quit. Or stop now: what it finished stays, what it was in the middle of is taken back.
        }
       *[other] { $waiting ->
            [0] Skwir can finish them without its window, then quit. Or stop now: what they finished stays, what they were in the middle of is taken back.
            [one] Skwir can finish them, and the { $waiting } action waiting after them, without its window, then quit. Or stop now: what they finished stays, what they were in the middle of is taken back.
           *[other] Skwir can finish them, and the { $waiting } actions waiting after them, without its window, then quit. Or stop now: what they finished stays, what they were in the middle of is taken back.
        }
    }

# What Quit asks while an action is paused, first: the action by its title, capitalised (Copy 3 items to Docs).
toast_quit_paused = { $title } is paused.

# What Quit asks while actions run, first: the one paused, by its title, capitalised (Copy 3 items to Docs); count: how many others run beside it.
toast_quit_paused_beside = { $title } is paused, and { $count ->
        [one] { $count } other action
       *[other] { $count } other actions
    } beside it.

# What Quit asks while an action runs, first: the action by its title, capitalised (Copy 3 items to Docs).
toast_quit_running = { $title } is still running.

# What Quit asks while actions run, first: one of them, by its title, capitalised (Copy 3 items to Docs); count: how many others run beside it.
toast_quit_running_beside = { $title } is still running, and { $count ->
        [one] { $count } other action
       *[other] { $count } other actions
    } beside it.

# A button on what an undo did: do again what it took back.
toast_redo = Redo

# why: what stopped it, as the system or the step said it (permission denied).
toast_rename_failed = Couldn't rename { $name } to { $to }: { $why }

toast_renamed = { $count ->
        [1] Renamed { $name } to { $to }
       *[other] Renamed { $count } items
    }

# why: what stopped it, as the system or the step said it (permission denied).
toast_restore_failed = Couldn't restore { $name }: { $why }

toast_restored = { $count ->
        [1] Restored { $name }
       *[other] Restored { $count } items
    }

# A button on what an error stopped: try what failed again.
toast_retry = Retry

# What an action's steps said beside what it did, each sentence after the one before (“a.pdf”: its last page was left out. Fold each sheet in half).
toast_said_then = { $said }. { $then }

# What a copy onto a server's folder does while the server takes what it wrote; count: the files it still sends. Said after other words: begins in lower case.
toast_sending = { $count ->
        [0] sending to the server
        [one] sending { $count } file to the server
       *[other] sending { $count } files to the server
    }

# What a copy onto a server's folder does while the server takes what it wrote, beginning a sentence; count: the files it still sends.
toast_sending_opening = { $count ->
        [0] Sending to the server
        [one] Sending { $count } file to the server
       *[other] Sending { $count } files to the server
    }

# A button on what an action made: show it, selected in its folder.
toast_show = Show

# What an action waiting its turn waits for: nothing more, it starts.
toast_starts = Starts

# Said after the title of an action waiting its turn (Rename to b.txt · …): it waits for nothing more, it starts.
toast_starts_after = starts

# Under the bar of an action that writes into a server's folder, which then sends it to the server: why no time left is said.
toast_then_sent = then sent to the server

# done: what an action did before an error stopped it (Copied 3 items to Docs); error: what stopped it, as the system or the step said it.
toast_then_stopped = { $done }, then stopped: { $error }

# why: what stopped it, as the system or the step said it (permission denied).
toast_trash_failed = Couldn't move { $name } to Trash: { $why }

toast_trashed = { $count ->
        [1] Moved { $name } to Trash
       *[other] Moved { $count } items to Trash
    }

# title: the action an undo took back (copy 3 items to Docs).
toast_undid = Undid: { $title }

# A button on what an action did: take it back.
toast_undo = Undo

# action: the action asked before it that touches what it does, by its title (copy to /media/stick).
toast_waits_for = Waits for { $action }

# Said after the title of an action waiting its turn (Rename to b.txt · …). action: the action asked before it that touches what it does, by its title (copy to /media/stick).
toast_waits_for_after = waits for { $action }

toast_waits_for_disk = Waits for the disk

# Said after the title of an action waiting its turn (Rename to b.txt · …).
toast_waits_for_disk_after = waits for the disk

toast_waits_for_everything = Waits for what was asked before it

# Said after the title of an action waiting its turn (Rename to b.txt · …).
toast_waits_for_everything_after = waits for what was asked before it

# count: how many actions run at once (4).
toast_waits_for_room = { $count ->
        [one] Waits for the { $count } running to end
       *[other] Waits for one of the { $count } running to end
    }

# Said after the title of an action waiting its turn (Rename to b.txt · …). count: how many actions run at once (4).
toast_waits_for_room_after = { $count ->
        [one] waits for the { $count } running to end
       *[other] waits for one of the { $count } running to end
    }

toast_waits_for_server = Waits for the server

# Said after the title of an action waiting its turn (Rename to b.txt · …).
toast_waits_for_server_after = waits for the server

# A button's tooltip: what it does, then its shortcut (Back · Alt+Left).
toolbar_title_keys = { $title } · { $keys }

# A setting's row in a menu, as its tooltip and a screen reader say it: its name, then what it is set to (Sort by: Name, A to Z).
toolbar_title_value = { $title }: { $value }

# The type of a device file read in blocks (a disk), in the Type column and the preview.
typenames_block_device = Block device

# The type of a link whose target is gone, in the Type column and the preview.
typenames_broken_link = Broken link

# The type of a device file read a character at a time (a terminal), in the Type column and the preview.
typenames_char_device = Character device

# The type of a folder, in the Type and Kind columns and captions.
typenames_folder = Folder

# What one file is, in the Kind column and caption: an archive: files packed in one.
typenames_kind_archive = Archive

# What one file is, in the Kind column and caption: a sound or music.
typenames_kind_audio = Audio

# What one file is, in the Kind column and caption: a document.
typenames_kind_document = Document

# What one file is, in the Kind column and caption: a folder.
typenames_kind_folder = Folder

# What one file is, in the Kind column and caption: a font.
typenames_kind_font = Font

# What one file is, in the Kind column and caption: a picture.
typenames_kind_image = Image

# What one file is, in the Kind column and caption, where its type is of no kind named.
typenames_kind_other = Other

# What one file is, in the Kind column and caption: a presentation's slides.
typenames_kind_presentation = Presentation

# What one file is, in the Kind column and caption: a program.
typenames_kind_program = Program

# What one file is, in the Kind column and caption: a spreadsheet.
typenames_kind_spreadsheet = Spreadsheet

# What one file is, in the Kind column and caption: a text.
typenames_kind_text = Text

# What one file is, in the Kind column and caption: a film.
typenames_kind_video = Video

# The type of a named pipe (a FIFO), in the Type column and the preview.
typenames_pipe = Pipe

# The type of a socket file, in the Type column and the preview.
typenames_socket = Socket

# Usage's legend, after Changed within: a block's color for what changed within a day.
usage_age_day = a day

# Usage's legend, after Changed within: longer ago than a year.
usage_age_longer = longer

# Usage's legend, after Changed within: within a month.
usage_age_month = a month

# Usage's legend, after Changed within: within a week.
usage_age_week = a week

# Usage's legend, after Changed within: within a year.
usage_age_year = a year

# Under a block's name in Usage: at least this much, what it holds being still counted or partly unreadable.
usage_at_least = ≥ { $said }

# Usage's toolbar before anything is counted.
usage_calculating = Calculating sizes…

# Usage, pointing at a block: when it last changed (Today 14:02, 12 Mar).
usage_changed = changed { $when }

# Usage's legend, before the colors of a day, a week, a month, a year, longer.
usage_changed_within = Changed within

# Usage, pointing at a block still counted: what it holds so far (3 files, 480 MB).
usage_counted_so_far = counted so far: { $said }

# Under a block's name in Usage, while what it holds is counted.
usage_counting = counting…

# Usage's zoom step, in the View menu: draw as many levels of folders as it does at first.
usage_default_levels = Default levels

# Pointing at Usage's Default levels button.
usage_default_levels_tip = Default levels

# The line under Usage's map while something is selected, after what the Trash frees. keys: the keys that delete it for good (Shift+Delete).
usage_delete_keys = { $keys } deletes for good

# Under a block's name in Usage: what it holds (480 MB, 3 files), or when it changed (Today 14:02).
usage_detail = { $said }

# Under the name of a cache folder's block in Usage: its size, then that it is a cache.
usage_detail_cache = { $said } · cache

# Under the name of the Trash's block in Usage: its size, then that it is the trash.
usage_detail_trash = { $said } · trash

# Usage's zoom step, in the View menu: draw the folders inside the folders one level less deep.
usage_fewer_levels = Fewer levels

# Pointing at Usage's Fewer levels button.
usage_fewer_levels_tip = Fewer levels (or Ctrl and the wheel)

# Usage: the room deleting a folder frees, less than it takes where its files have names outside it too.
usage_freed = { $size } freed if deleted

# Usage, pointing at a block drawn inside a folder's block: a double click shows that folder.
usage_go_there = double-click to go there

# Usage: a block standing for the hidden files of a folder, while hidden files are not shown.
usage_hidden_items = { $count ->
        [one] { $count } hidden item
       *[other] { $count } hidden items
    }

# The line under Usage's map, pointing at nothing: what the pointer does there.
usage_how = Click to select · double-click to open · Ctrl and the wheel for more or fewer levels

# Usage, pointing at a block inside a cache folder.
usage_in_cache = in a cache, which its program makes again when needed

# Usage, pointing at a block inside the Trash.
usage_in_trash = in the Trash: emptying it frees this

# Under a block's name in Usage, where what it holds could not all be counted.
usage_incomplete = incomplete

# Usage, pointing at a block that is a cache folder.
usage_is_cache = a cache, which its program makes again when needed

# Usage, pointing at a block: a kind of files and its share of the block (Videos 62%).
usage_kind_share = { $kind } { $share }

# Usage's legend: a kind of files and how much of the folder it takes (Videos 1.2 GB).
usage_legend_kind = { $kind } { $size }

# Between Usage's Fewer and More levels buttons: how many levels of folders it draws.
usage_levels = { $count ->
        [one] { $count } level
       *[other] { $count } levels
    }

# A button of Usage: its blocks colored by when they last changed.
usage_mode_age = Age

# Pointing at Usage's Age button.
usage_mode_age_tip = Blocks colored by when they last changed

# A button of Usage: its blocks as large as the number of files they hold.
usage_mode_files = Files

# Pointing at Usage's Files button.
usage_mode_files_tip = Blocks as large as the files they hold

# A button of Usage: its blocks as large as the room they take on the disk.
usage_mode_size = Size

# Pointing at Usage's Size button.
usage_mode_size_tip = Blocks as large as the room they take

# A button of Usage: its blocks colored by the kind of files that fill them.
usage_mode_types = Types

# Pointing at Usage's Types button.
usage_mode_types_tip = Blocks colored by the kind of files that fill them

# Usage's zoom step, in the View menu: draw the folders inside the folders one level deeper.
usage_more_levels = More levels

# Pointing at Usage's More levels button.
usage_more_levels_tip = More levels (or Ctrl and the wheel)

# Usage, pointing at a file with several names (hard links): deleting this name frees nothing while the others stay.
usage_one_of_names = { $count ->
       *[other] one of { $count } names of its file
    }

# Usage: the files of no kind named (not Images, Videos, Documents…).
usage_other_files = Other files

# Usage: the name of the block standing for entries too small to draw each.
usage_other_items = { $count ->
        [one] { $count } other item
       *[other] { $count } other items
    }

# A button of Usage: count again what the folder holds.
usage_recalculate = Recalculate sizes

# Usage, pointing at a block: how much of it is caches and the Trash, which can be had back.
usage_reclaimable = { $size } of it in caches and the Trash

# Usage's toolbar while it counts what the folder holds.
usage_scanning = Scanning…

# The line under Usage's map while it counts again: how many folders it read so far.
usage_scanning_read = Scanning… { $count ->
        [one] { $count } folder
       *[other] { $count } folders
    } read

# Usage, pointing at the block of entries too small to draw each. List is the name of a view.
usage_see_all = double-click to see all items in List

# Usage: a share of a folder or a block, in percent (24%).
usage_share = { $percent }%

# Usage: a share of a folder of less than one percent (<1%).
usage_share_below = <{ $percent }%

# Usage, pointing at a block: its share of the room the folder holding it takes (24%).
usage_share_of = { $share } of { $folder }

# Usage, pointing at a block: its share of the files of the folder holding it (24%).
usage_share_of_files = { $share } of the files in { $folder }

# Usage, pointing at the block of a folder's hidden files.
usage_show_hidden = double-click to show hidden files

# Usage: how much a file or a folder holds, then the room it takes on the disk, where they differ (480 MB · 120 MB on disk).
usage_sizes = { $size } · { $alloc } on disk

# Usage, pointing at the block standing for entries too small to draw each.
usage_smaller_items = { $count ->
        [one] { $count } smaller item
       *[other] { $count } smaller items
    }

# The line under Usage's map while it counts.
usage_striped = Striped blocks are still being counted · proportions will change as sizes arrive

# Usage's toolbar while it counts: what the folder holds so far (480 MB).
usage_summary_counted = Counted so far: { $size }

# Usage's toolbar: how much of the folder is caches and the Trash, which can be had back.
usage_summary_reclaimable = { $size } in caches and the Trash

# The line under Usage's map while something is selected.
usage_trash_frees_nothing = Moving to the Trash frees no room until it is emptied

# The line under Usage's map while something is selected. keys: the keys that move it to the Trash (Delete).
usage_trash_keys_frees_nothing = { $keys } moves to the Trash, which frees no room until it is emptied

# Usage's toolbar where what is shown is no folder (a search, the Trash).
usage_unavailable = Usage is available for filesystem folders

# Usage's toolbar, last, while it counts again what changed.
usage_updating = updating…

# Usage, pointing at a block of several entries inside a folder: what it holds (3 hidden items), then the folders it is in (Photos › 2024).
usage_within = { $what } in { $trail }

# Said in a viewer's window when Show in Skwir could not start Skwir. error: why, as the system says it.
viewer_reveal_failed = Skwir did not start: { $error }

# What a kept setting holds: the size the last window showing one file's preview alone was left at, so the next opens so.
viewer_size_setting = The size of the last window viewing a file (skwir view)

# On a pill beside a file a copy brings into the folder: it is being copied, how far not known.
views_arriving_copying = copying

# Pointing at a file a copy brings into the folder: the folder it comes from.
views_arriving_from = From { $folder }

# On a pill beside a file a copy brings into the folder: its copy is paused.
views_arriving_paused = paused

# What a screen reader hears after the name of a file a copy brings into the folder. percent: how far it came (42%).
views_arriving_said_at = arriving, { $percent }

# What a screen reader hears after the name of a file a copy brings into the folder.
views_arriving_said_copying = arriving, copying

# What a screen reader hears after the name of a file a copy brings into the folder. percent: how far it came (42%).
views_arriving_said_paused = arriving, paused at { $percent }

# What a screen reader hears after the name of a file a copy brings into the folder.
views_arriving_said_starting = arriving, starting

# What a screen reader hears after the name of a file a copy brings into the folder: its copy waits its turn.
views_arriving_said_waiting = arriving, waiting

# On a pill beside a file a copy brings into the folder: its copy is starting.
views_arriving_starting = starting

# Pointing at a file a copy brings into the folder: a tool copies it with the others, how far not known.
views_arriving_tip_copying = copying with the others

# Pointing at a file a copy brings into the folder: it has arrived.
views_arriving_tip_landed = in place

# Pointing at a file a copy brings into the folder: its copy is paused.
views_arriving_tip_paused = paused

# Pointing at a file a copy brings into the folder: its copy is starting.
views_arriving_tip_starting = getting ready

# Pointing at a file a copy brings into the folder: its copy waits for others before it.
views_arriving_tip_waits = waits its turn

# On a pill beside a file a copy brings into the folder: its copy waits its turn.
views_arriving_waiting = waiting

# A view's name: the folders, or a tag's tags, side by side as columns of cards.
views_board = Board

# The title of the board's last column, which holds the groups past those given a column each.
views_board_more_groups = { $count ->
        [one] { $count } more group
       *[other] { $count } more groups
    }

# A view's name: the folders above shown side by side, each a column.
views_columns = Columns

# What a screen reader hears after a file's name: it was cut, to be moved where it is pasted.
views_entry_cut = cut

# What a screen reader hears after the name of the first file of a group: the group (Today, 3 items), then what marks the file (cut, hidden, its tags).
views_entry_described = { $group }; { $said }

# What a screen reader hears after a file's name: it is a hidden file, though its name does not begin with a dot.
views_entry_hidden = hidden

# What a screen reader hears after a file's name: what marks it, joined (cut, hidden and tagged print).
views_entry_marks = { $marks }

# What a screen reader hears after a folder's name: it is shown only because files the filter keeps are inside it.
views_entry_only_leads = leads to what the filter keeps

# What a screen reader hears after a file's name in a window choosing folders for another program's dialog: it is shown dimmed, to say where one is, and cannot be chosen.
views_entry_untaken = cannot be chosen here

# What a screen reader hears after a file's name: its state in Git, changed since the last commit.
views_entry_vcs_changed = changed

# What a screen reader hears after a file's name: its state in Git, a merge left it with both sides.
views_entry_vcs_conflict = in conflict

# What a screen reader hears after a folder's name: its state in Git, files below it changed.
views_entry_vcs_holds_changes = holds changes

# What a screen reader hears after a folder's name: its state in Git, a file below it is in conflict.
views_entry_vcs_holds_conflict = holds a conflict

# What a screen reader hears after a folder's name: its state in Git, files below it are in no commit yet.
views_entry_vcs_holds_new = holds new files

# What a screen reader hears after a folder's name: its state in Git, files below it have changes added for the next commit.
views_entry_vcs_holds_staged = holds staged changes

# What a screen reader hears after a file's name: its state in Git, in no commit yet.
views_entry_vcs_new = new

# What a screen reader hears after a file's name: its state in Git, its changes added for the next commit.
views_entry_vcs_staged = staged

# What a screen reader hears after a file's name: another program is still writing it.
views_entry_writing = being written

# A view's name: the files of a folder as tiles with their pictures.
views_grid = Grid

# Where a group's heading counts its files, for a group whose files a copy brings and none has arrived yet.
views_group_arriving = arriving

# What a screen reader hears of a group's heading: its label (Today, Images), then how many it holds.
views_group_said = { $group }, { $count ->
        [one] { $count } item
       *[other] { $count } items
    }

# What a screen reader hears of a group whose files a copy brings, none arrived yet. group: its label.
views_group_said_arriving = { $group }, arriving

# The heading of a group of files whose value is not known (no date taken, no size read).
views_group_unknown = Unknown

# A view's name: the files of a folder in rows, one under the other.
views_list = List

# A view's name: the list with folders opened in place below their rows.
views_tree = Tree

# On a pill beside a file another program is still writing.
views_writing = writing

# What a window answering another program's Open or Save dialog is for, as the desktop may show it (a window rule's list, a screen reader).
window_tag_chooser = File chooser

# What an ordinary Skwir window is for, as the desktop may show it (a window rule's list, a screen reader).
window_tag_main = Main window

# What a window showing one file's preview alone (skwir view) is for, as the desktop may show it (a window rule's list, a screen reader).
window_tag_viewer = File viewer

# The title of the desktop's notice of a question waiting while no window is up to ask it.
work_told_asks = Skwir needs an answer

# The desktop's notice of a question waiting while no window is up to ask it.
work_told_asks_body = An action waits for your answer: open Skwir to give it.

# The title of the desktop's notice of what ended while no one looked: how many actions finished.
work_told_finished = { $count ->
        [1] Finished
       *[other] { $count } actions finished
    }

# The title of the desktop's notice of what ended while no one looked: how many actions finished, how many an error stopped.
work_told_finished_stopped = { $finished ->
        [one] { $finished } action
       *[other] { $finished } actions
    } finished, { $stopped ->
        [one] { $stopped } stopped
       *[other] { $stopped } stopped
    }

# The title of the desktop's notice of what ended while no one looked: how many actions an error stopped.
work_told_stopped = { $count ->
        [1] Stopped
       *[other] { $count } actions stopped
    }

# The title of the desktop's notice of an action its server stopped, which goes on by itself once the server answers again.
work_told_waits_for_server = Waiting for the server

# What the machine is kept awake for, and the session's end held off for, as the desktop lists it: how many actions run.
work_why_actions = { $count ->
        [one] { $count } action under way
       *[other] { $count } actions under way
    }

# What the machine is kept awake for, and the session's end held off for, as the desktop lists it.
work_why_under_way = Actions under way
