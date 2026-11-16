# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# Said when the application chosen as the default was it already.
open_with_already_opens = { $app } already opens { $kind ->
        [folder] folders
       *[file] { $description } files
    }

# A check in the chooser: makes the application chosen the default for the type. The type is as the system describes it (PNG image, plain text document).
open_with_always_use = Always use for { $kind ->
        [folder] folders
       *[file] { $description } files
    }

open_with_cancelled = opening cancelled

# A link after Properties' Opens with: chooses another application for the file's type.
open_with_change = Change…

open_with_choices = List applications for the selection

# Read by agents: the words in backticks are the command's arguments and commands, as written.
open_with_choices_description = List the applications that open the selected entries (or the pane's folder with `here`), recommended first, each with the `open_with` invocation that opens them; with `defaults`, each with the `open_with.set_default` that makes it the default for their type

# The title of the chooser of applications for the selection.
open_with_chooser = Open with

# The title of the chooser of applications: the file it opens, or how many.
open_with_chooser_title = { $count ->
        [1] Open { $name } with
       *[other] Open { $count } items with
    }

# Under an application's name in the chooser: it is the default.
open_with_default = Default

# Under an application's name in the chooser: it is the default, and what it is, as its desktop entry says (Image Viewer).
open_with_default_generic = Default · { $generic }

# An error: the path named is not there.
open_with_does_not_exist = { $path } does not exist

open_with_edit_associations = Rewrite a type's default application lines

open_with_edit_associations_description = Set the lines a MIME type has in one mimeapps.list (default, added and removed associations); what Undo and Redo of a default run

open_with_menu = Name the default application in a menu

open_with_menu_description = A menu's Open for the selection, or for the files named (the preview's: `entries`, as the pane lists them), naming the default application they share, and the other applications for their types in its Open With submenu; no items when their types or the applications are not known yet. Answers from memory

# Properties' Opens with, for a file no application opens.
open_with_no_application = No application

open_with_no_applications = No applications found

# An error an agent or a script reads: the pane shows no folder yet.
open_with_no_location = pane has no location

# An error an agent or a script reads: the pane it named is not there.
open_with_no_such_pane = no such pane

# Said once a default is taken back and nothing opens the type.
open_with_none_opens = No application opens { $kind ->
        [folder] folders
       *[file] { $description } files
    }

# Said when a pipe, a socket or a device is opened: what would wait or act is never opened.
open_with_not_a_file = { $name } is not a file: skwir does not open it

open_with_not_changed = The default application was not changed: { $error }

# An error: the application's desktop file ID (org.gnome.Loupe.desktop) is as it was given.
open_with_not_installed = no application “{ $id }” is installed

# An error: the file named is not one of the user's mimeapps.list files.
open_with_not_your_list = { $path } is not one of your lists of default applications

open_with_nothing_to_open = nothing to open

# Under an application's name in the chooser of a default: the one that opens the type now.
open_with_now = Now

# Said once a default application is chosen, with Undo. The application is named as its desktop entry names it; the type as the system describes it (PNG image).
open_with_now_opens = { $app } now opens { $kind ->
        [folder] folders
       *[file] { $description } files
    }

open_with_one_type_at_a_time = A default application is chosen for one type of file at a time

# A menu item and the action's name: enters the folder, or opens the files with their default applications.
open_with_open = Open

# Read by agents and in F1's list of actions.
open_with_open_description = Enter the selected directory, or open the selected files with the default application of their type, as the desktop's lists choose it (the one the context menu names)

# The title of the chooser of a default application for a type, as the system describes it (PNG image, plain text document).
open_with_open_files_with = Open { $kind ->
        [folder] folders
       *[file] { $description } files
    } with

open_with_open_terminal = Open terminal here

open_with_open_terminal_description = Open a terminal in the selected folder, else the folder shown, else (in a search) the folder holding the selection

# A context menu item: opens a terminal in the folder.
open_with_open_terminal_menu = Open Terminal Here

open_with_open_with = Open with an application by desktop ID

# A menu item opening the selection with its default application, named as its desktop entry names it.
open_with_open_with_app = Open With { $app }

open_with_open_with_description = Open the selected entries (or the pane's folder with `here`) with the application whose desktop file ID is `app`

# A row of Properties: the application the file's type opens with.
open_with_opens_with = Opens with

# The last item of the Open With submenu: chooses among every application.
open_with_other_application = Other Application…

# A heading in the chooser of applications: every other application.
open_with_other_applications = Other applications

open_with_plugin_description = Open files with their default application or one you choose, and open a terminal here

open_with_plugin_title = Open with

open_with_properties = Name the default application in Properties

open_with_properties_description = Properties' Opens with row for one file: the application its type opens with, and the way to change it

# A heading in the chooser of applications: those that open the type itself.
open_with_recommended = Recommended

# A heading in the chooser of applications: those that open a type it is a kind of (plain text, for a C source).
open_with_related = Related

# The chooser's field, before anything is typed in it.
open_with_search_applications = Search applications

open_with_set_default = Set the default application for a type

open_with_set_default_description = Make the application whose desktop file ID is `app` the default for the MIME type `mime`, in the user's mimeapps.list; the notification offers Undo
