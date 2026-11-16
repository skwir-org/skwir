# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

shelf_add = Add to shelf

shelf_add_description = Put the selection (or the given entries) on the shelf

shelf_add_menu = Add to Shelf

shelf_clear = Clear shelf

shelf_clear_description = Empty the shelf

shelf_copy_here = Copy shelf here

shelf_copy_here_description = Copy every shelved entry into the current directory (plan)

# A plan's title: the folder they are copied into, by its name.
shelf_copy_here_plan = copy { $count ->
        [one] { $count } shelved item
       *[other] { $count } shelved items
    } to { $folder }

shelf_copy_tool = Copy shelf files to the other pane

shelf_empty = The shelf is empty

# Under "The shelf is empty", in the empty shelf. Add to Shelf is a menu item's words.
shelf_empty_detail = Collect files from different folders with Add to Shelf, then move or copy them together.

shelf_list = List shelf

shelf_list_description = Return the shelved entries

shelf_move_here = Move shelf here

shelf_move_here_description = Move every shelved entry into the current directory (plan)

# A plan's title: the folder they are moved into, by its name.
shelf_move_here_plan = move { $count ->
        [one] { $count } shelved item
       *[other] { $count } shelved items
    } to { $folder }

shelf_move_tool = Move shelf files to the other pane

shelf_no_such_pane = no such pane

shelf_nothing_to_bring = nothing on the shelf to bring here

# An error: the shelf's files were to be moved to the trash.
shelf_nothing_to_trash = the shelf is empty

shelf_open = Open shelf

shelf_open_description = Show the shelf in the pane

shelf_pane_not_open = pane has no open location

shelf_plugin_description = Collect entries from anywhere and move, copy, or trash them together

shelf_plugin_title = Shelf

shelf_remove = Remove from shelf

shelf_remove_description = Take entries off the shelf

# What Delete does on the shelf, as its menu item says it.
shelf_remove_verb = Remove from Shelf

# The shelf as a place: in the sidebar, its tab, its count in the footer.
shelf_title = Shelf

shelf_trash = Trash shelf

shelf_trash_description = Move every shelved entry to the trash (plan)

# A plan's title: the shelf's files moved to the trash.
shelf_trash_plan = trash { $count ->
        [one] { $count } shelved item
       *[other] { $count } shelved items
    }
