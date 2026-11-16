# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

count_files = { $count ->
        [one] 1 file
       *[other] { $count } files
    }

count_folders = { $count ->
        [one] 1 folder
       *[other] { $count } folders
    }

count_items = { $count ->
        [one] 1 item
       *[other] { $count } items
    }

placeholder_companions = Only companion files here

# Show companion files, Settings and Tags and notes are the names of a setting and of where it is.
placeholder_companions_detail = They keep other files' notes: TagSpaces' .ts folder, a Mac's ._ files, XMP sidecars. Showing them turns on Show companion files in Settings (Tags and notes), for every folder.

placeholder_denied = You don't have permission to open this folder

placeholder_drop = Drop files here.

# keys: the keys that make a folder, as the keymap says them (Ctrl+Shift+N).
placeholder_drop_or_make = Drop files here, or press { $keys } for a new folder.

placeholder_empty = This folder is empty

placeholder_failed = This location could not be opened

placeholder_filter_empty = Nothing here is “{ $filter }”. Press Enter in the address field to search the folders inside too.

placeholder_filtered = Hidden by filters

placeholder_gone = This folder no longer exists

placeholder_hidden = Only hidden files here

placeholder_hidden_show = Show hidden files to see them.

# keys: the keys that show hidden files (Ctrl+H).
placeholder_hidden_show_keys = Show hidden files to see them ({ $keys }).

placeholder_lens_empty = Nothing here is “{ $filter }”. The filters stay as you go to other folders and searches, until you take them off in the address.

placeholder_looking = Looking in the folders here…

placeholder_looking_detail = The folders that lead to “{ $filter }” show as they are found.

placeholder_name_empty = No name here holds “{ $filter }”. Press Enter in the address field to search the folders inside too.

placeholder_nothing = Nothing here

placeholder_reading = Reading the details of the files…

# filter: what was typed in the address to filter the folder.
placeholder_reading_detail = The files “{ $filter }” keeps show as they are read.

placeholder_reading_files = Reading the details of { $count ->
        [one] one file
       *[other] { $count } files
    }…

placeholder_retry = Try again

placeholder_show_companions = Show hidden and companion files

placeholder_show_everything = Show everything

placeholder_show_hidden = Show hidden files

placeholder_up = Go to the parent folder

toast_already_as_asked = { $name } was already as asked

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

# How far an action is: the items it finished of all (0 of 2 items).
toast_bar_items = { $finished } of { $count ->
        [one] { $count } item
       *[other] { $count } items
    }

# How far an action is: the items it finished of all, then what it moved of all it moves (1 of 3 items · 3.2 GB of 8.6 GB).
toast_bar_items_bytes = { $finished } of { $count ->
        [one] { $count } item
       *[other] { $count } items
    } · { $done } of { $whole }

# How far an action is: the items it finished of all, then what it moved (1,286 of 40,000 items · 82.3 kB).
toast_bar_items_moved = { $finished } of { $count ->
        [one] { $count } item
       *[other] { $count } items
    } · { $done }

# done: what the action did before it was cancelled (Copied 3 items to Docs).
toast_cancelled_after = Cancelled. { $done }

toast_close = Close

toast_copied = { $count ->
        [1] Copied { $name } to { $folder }
       *[other] Copied { $count } items to { $folder }
    }

# why: what stopped it, as the system or the step said it (permission denied).
toast_copy_failed = Couldn't copy { $name } to { $folder }: { $why }

# what: what the step said it would do (extract “a.zip” to Downloads); why: what stopped it, as the system or the step said it (permission denied).
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

# trash: the trash the server keeps what is deleted in, as it names it (Google Drive's trash).
toast_deleted_kept = { $count ->
        [1] Deleted { $name } (in { $trash })
       *[other] Deleted { $count } items (in { $trash })
    }

# done: what an action did (Copied 3 items to Docs); found: what it found done already (2 were already there).
toast_done_and_found = { $done }; { $found }

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

# What a copy onto a server's folder does while the server takes what it wrote; count: the files it still sends. Said after other words: begins in lower case.
toast_sending = { $count ->
        [0] sending to the server
        [one] sending { $count } file to the server
       *[other] sending { $count } files to the server
    }

# A button on what an action made: show it, selected in its folder.
toast_show = Show

# What an action waiting its turn waits for: nothing more, it starts.
toast_starts = Starts

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

toast_waits_for_disk = Waits for the disk

toast_waits_for_everything = Waits for what was asked before it

# count: how many actions run at once (4).
toast_waits_for_room = Waits for one of the { $count } running to end

toast_waits_for_server = Waits for the server
