# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A command's title, in the list of actions: nothing waits to be pasted any more.
ops_basic_clipboard_clear = Forget cut or copied files

# Read by agents and in F1's list of actions.
ops_basic_clipboard_clear_description = Empty the clipboard; files that were cut stay where they are

# An error of Paste in the trash: only files cut (moved) go there, never copies.
ops_basic_copied_into_trash = Copied files cannot be put in the trash: cut them instead

# What Copy Path says once the paths are on the clipboard, as text.
ops_basic_copied_paths = { $count ->
        [1] Copied the path
       *[other] Copied { $count } paths
    }

# A command's title, in the list of actions: the files are copied where they are pasted.
ops_basic_copy = Copy

# Read by agents and in F1's list of actions.
ops_basic_copy_description = Remember the selection, or the entries given, for a later paste (copy)

# A command's title, in the list of actions: the files' paths are copied as text.
ops_basic_copy_paths = Copy paths

# Read by agents and in F1's list of actions.
ops_basic_copy_paths_description = The selected paths, or the entries', or with nothing selected the folder shown, as text for the clipboard, one per line

# A command's title, in the list of actions: the files are moved where they are pasted.
ops_basic_cut = Cut

# Read by agents and in F1's list of actions.
ops_basic_cut_description = Remember the selection, or the entries given, for a later paste (move)

# A context menu's item: the files are copied where they are pasted.
ops_basic_menu_copy = Copy

# A context menu's item: copies the file's path (or the folder's) as text.
ops_basic_menu_copy_path = Copy Path

# A context menu's item, for several files: copies their paths as text, one a line.
ops_basic_menu_copy_paths = Copy Paths

# A context menu's item: the files are moved where they are pasted.
ops_basic_menu_cut = Cut

# A context menu's item: deletes the files for good, after asking.
ops_basic_menu_delete = Delete Permanently

# A context menu's item, on a folder's background: makes an empty file in it, asking its name.
ops_basic_menu_new_file = New File

# A context menu's item, on a folder's background: makes a folder in it, asking its name.
ops_basic_menu_new_folder = New Folder

# A context menu's item: opens the folder in a new tab.
ops_basic_menu_open_tab = Open in New Tab

# A context menu's item: opens the folder in a new window.
ops_basic_menu_open_window = Open in New Window

# A context menu's item, on a folder's background: puts what was cut or copied in the folder.
ops_basic_menu_paste = Paste

# A context menu's item, on a folder's background: reads the folder again.
ops_basic_menu_refresh = Refresh

# A context menu's item: renames the file.
ops_basic_menu_rename = Rename

# A context menu's item, where files of many folders are listed (a search): shows the file in the folder it is in.
ops_basic_menu_show_in_folder = Show in Folder

# A context menu's item: moves the files to the trash, from where they can be restored.
ops_basic_menu_trash = Move to Trash

# An error of Paste: what the pane shows (a search) takes no files. In lower case.
ops_basic_nothing_pasted_here = nothing can be pasted here

# An error of Paste: the clipboard named no file. In lower case.
ops_basic_nothing_to_paste = nothing to paste

# An error of Paste: the pane pasted into shows no folder or place. In lower case.
ops_basic_pane_shows_nothing = the pane shows nothing

# A command's title, in the list of actions: what was cut or copied is put in the folder shown.
ops_basic_paste = Paste

# A plan's title: files copied, pasted in a folder (by its name). One by its name, or how many. Begins in lower case.
ops_basic_paste_copy = { $count ->
        [1] copy { $name } to { $folder }
       *[other] copy { $count } items to { $folder }
    }

# Read by agents and in F1's list of actions. clipboard is an argument's name, as typed.
ops_basic_paste_description = Copy or move the clipboard into the current directory (plan): Skwir's, or the files given as `clipboard`

# A plan's title: files cut, pasted in a folder (by its name). One by its name, or how many. Begins in lower case.
ops_basic_paste_move = { $count ->
        [1] move { $name } to { $folder }
       *[other] move { $count } items to { $folder }
    }

# An error of Copy Path: a file's name is bytes no text spells (not UTF-8).
ops_basic_path_not_text = The path of { $name } is not text the clipboard can hold

ops_basic_plugin_description = Default key bindings, context menu, and clipboard

ops_basic_plugin_title = Basic operations
