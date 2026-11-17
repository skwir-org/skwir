# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A command's title, in the list of actions: opens the folder's listing in the user's text editor.
edit_as_buffer_command = Edit as text…

# Read by agents and in F1's list of actions. $EDITOR is the variable naming the user's editor.
edit_as_buffer_command_description = Open the listing in $EDITOR; saving proposes a plan for review

# An error of a listing edited as text: two lines start with the same number (handle), which names one file. The line's number, then the handle as written.
edit_as_buffer_duplicate_handle = line { $line }: handle { $handle } appears twice

# An error of a listing edited as text: several lines give their files the same name or path, as written.
edit_as_buffer_duplicate_target = `{ $name }` is the target of several lines

# An error once the editor closed with a failure: what the system says of how it ended (exit status: 1).
edit_as_buffer_editor_exited = editor exited with { $status }

# An error: the editor could not be run, or waited for. Why follows.
edit_as_buffer_editor_failed = editor failed

# Why a name written in a listing edited as text is refused: there is none.
edit_as_buffer_empty_name = a name cannot be empty

# A notification: editing a listing as text failed, and why. In lower case.
edit_as_buffer_failed = edit as text: { $error }

# The first lines of a folder's listing opened as text in the user's editor: each line begins with # (a comment, which is not read back). The arrows say what each edit becomes; the words in the left column may be aligned anew. \\, \xHH, \x0a and \x20 are written as they are.
edit_as_buffer_header = # Skwir: edit this listing, save, and quit.
    #   change a name            -> rename
    #   delete a line            -> move to trash
    #   name with a slash        -> move into that folder (created if needed)
    #   new line ending with /   -> new folder
    # Keep the number at the start of each line. Order does not matter.
    # In names, \\ is a backslash and \xHH a raw byte (\x0a a newline,
    # \x20 a space at either end).

# An error of a listing edited as text: a name written there is not one a file can have. The line's number, the name as written, then why.
edit_as_buffer_invalid_name = line { $line }: invalid name `{ $name }`: { $reason }

# A context menu's item, on a folder's background: opens its listing in the user's text editor.
edit_as_buffer_menu = Edit as Text…

# An error of a listing edited as text: a line without a number makes a folder only when it ends with /. The line's number, then the name as written.
edit_as_buffer_new_file = line { $line }: new files are not supported (`{ $name }`); end the line with / for a folder

# A notification once the editor closed: the listing edited as text was not changed. In lower case.
edit_as_buffer_no_changes = edit as text: no changes

# An error: the pane a listing was to be edited from is gone.
edit_as_buffer_no_pane = no such pane

# An error: the pane a listing was to be edited from shows nothing.
edit_as_buffer_nothing_open = pane has no open location

# Why a name written in a listing edited as text is refused: it starts with /, out of the folder listed.
edit_as_buffer_outside = a path must stay inside this folder

# A plan's title: the changes made to a folder's listing edited as text, with the folder's whole path. Begins in lower case.
edit_as_buffer_plan = edit { $folder }

edit_as_buffer_plugin_description = Edit the listing in your editor; the difference becomes a plan to review

edit_as_buffer_plugin_title = Edit as text

# An error of a listing edited as text: the name a line gives its file is taken by another file. The line's number, then the path taken.
edit_as_buffer_taken = line { $line }: `{ $name }` already exists

# An error of a listing edited as text: the number at the start of a line (its handle) names no file of the listing. The line's number, then the handle as written.
edit_as_buffer_unknown_handle = line { $line }: unknown handle { $handle }

# An error once the editor closed: the listing edited as text could not be read back. Why follows.
edit_as_buffer_unreadable = cannot read the edited buffer
