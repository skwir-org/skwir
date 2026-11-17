# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# Why who may use a file cannot be put back as it was: another file took its name.
access_another_file = { $name } is now another file

# A step of a plan found already done. inside: yes where it covers everything a folder holds too. Begins in lower case.
access_both_already = { $inside ->
        [yes] the access and group of { $name } and of everything inside were already as asked
       *[no] the access and group of { $name } were already as asked
    }

# A step of a plan, in its review. inside: yes where it changes everything a folder holds too. Begins in lower case.
access_change_both = { $inside ->
        [yes] change the access and group of { $name } and of everything inside
       *[no] change the access and group of { $name }
    }

# A step of a plan, in its review. inside: yes where it changes everything a folder holds too. Begins in lower case.
access_change_group = { $inside ->
        [yes] change the group of { $name } and of everything inside
       *[no] change the group of { $name }
    }

# A step of a plan, in its review. inside: yes where it changes everything a folder holds too; mode: the mode, as chmod writes it (u+x). Begins in lower case.
access_change_mode = { $inside ->
        [yes] change who may use { $name } and everything inside ({ $mode })
       *[no] change who may use { $name } ({ $mode })
    }

# A step of a plan, done. inside: yes where it changed everything a folder holds too. Begins in lower case.
access_changed_both = { $inside ->
        [yes] changed the access and group of { $name } and of everything inside
       *[no] changed the access and group of { $name }
    }

# A step of a plan, done. inside: yes where it changed everything a folder holds too. Begins in lower case.
access_changed_group = { $inside ->
        [yes] changed the group of { $name } and of everything inside
       *[no] changed the group of { $name }
    }

# A step of a plan, done. inside: yes where it changed everything a folder holds too. Begins in lower case.
access_changed_mode = { $inside ->
        [yes] changed who may use { $name } and everything inside
       *[no] changed who may use { $name }
    }

# What a clause of a mode typed in Properties' Access field does (u+x). who: whom it is about (u the owner, g the group, o others, a everyone); umask: the umask's digits, which a clause naming no class leaves out; does: what they are given (run it).
access_clause_also = { $who ->
        [u] the owner
        [g] the group
        [o] others
        [ug] the owner and the group
        [uo] the owner and others
        [go] the group and others
        [umask] everyone but what the umask ({ $umask }) leaves out
       *[all] everyone
    } may also { $does }

# What a clause of a mode typed in Properties' Access field does (go-w). who: whom it is about (u the owner, g the group, o others, a everyone); umask: the umask's digits, which a clause naming no class leaves out; does: what is taken from them (write).
access_clause_no_longer = { $who ->
        [u] the owner
        [g] the group
        [o] others
        [ug] the owner and the group
        [uo] the owner and others
        [go] the group and others
        [umask] everyone but what the umask ({ $umask }) leaves out
       *[all] everyone
    } may no longer { $does }

# What a clause of a mode typed in Properties' Access field does (o=). who: whom it is about (u the owner, g the group, o others, a everyone); umask: the umask's digits, which a clause naming no class leaves out.
access_clause_none = { $who ->
        [u] the owner
        [g] the group
        [o] others
        [ug] the owner and the group
        [uo] the owner and others
        [go] the group and others
        [umask] everyone but what the umask ({ $umask }) leaves out
       *[all] everyone
    } may not use it

# What a clause of a mode typed in Properties' Access field does where it gives nothing (o+s). who: whom it is about (u the owner, g the group, o others, a everyone); umask: the umask's digits, which a clause naming no class leaves out.
access_clause_nothing = nothing changes for { $who ->
        [u] the owner
        [g] the group
        [o] others
        [ug] the owner and the group
        [uo] the owner and others
        [go] the group and others
        [umask] everyone but what the umask ({ $umask }) leaves out
       *[all] everyone
    }

# What a clause of a mode typed in Properties' Access field does (a=r). who: whom it is about (u the owner, g the group, o others, a everyone); umask: the umask's digits, which a clause naming no class leaves out; does: all they are left (read).
access_clause_only = { $who ->
        [u] the owner
        [g] the group
        [o] others
        [ug] the owner and the group
        [uo] the owner and others
        [go] the group and others
        [umask] everyone but what the umask ({ $umask }) leaves out
       *[all] everyone
    } may only { $does }

# What one may do with files and folders alike, said after may (everyone may read). letters: what is given, as chmod writes it: r read, w write, x run, X open a folder or run a file where someone may already.
access_does_both = { $letters ->
        [r] read
        [w] write
        [x] run it
        [rw] read and write
        [rx] read and run it
        [wx] write and run it
        [X] open it, a file where someone may already run it
        [rX] read and open it, a file where someone may already run it
        [wX] write and open it, a file where someone may already run it
        [rwX] read, write and open it, a file where someone may already run it
       *[rwx] read, write and run it
    }

# What a class is given, said after may: what another class may do (the group may only what the owner may).
access_does_copy = what { $class ->
        [u] the owner
        [g] the group
       *[o] others
    } may

# What one may do with a file, said after may (the owner may read and write). letters: what is given, as chmod writes it: r read, w write, x run, X run where someone may already.
access_does_file = { $letters ->
        [r] read
        [w] write
        [x] run it
        [rw] read and write
        [rx] read and run it
        [wx] write and run it
        [X] run it where someone may already
        [rX] read and run it where someone may already
        [wX] write and run it where someone may already
        [rwX] read, write and run it where someone may already
       *[rwx] read, write and run it
    }

# What one may do with a folder, said after may (the owner may list it and open it). letters: what is given, as chmod writes it: r list, w change what it holds, x open.
access_does_folder = { $letters ->
        [r] list it
        [w] change what it holds
        [x] open it
        [rw] list it and change what it holds
        [rx] list it and open it
        [wx] change what it holds and open it
       *[rwx] list it, change what it holds and open it
    }

# Why who may use a file cannot be changed.
access_drive_read_only = the drive is read-only

# Who may do what with a mode's bits. owner, group, others: none where they may not use it; owner_does, group_does, others_does: what they may do (read and write; for a folder: list it and open it).
access_each_class = the owner { $owner ->
        [none] may not use it
       *[some] may { $owner_does }
    }, the group { $group ->
        [none] may not use it
       *[some] may { $group_does }
    }, others { $others ->
        [none] may not use it
       *[some] may { $others_does }
    }

# Who may do what with a mode's bits, the same for all. all: none where they may not use it; all_does: what they may do (read and write; for a folder: list it and open it).
access_everyone_may = everyone { $all ->
        [none] may not use it
       *[some] may { $all_does }
    }

# Why a change of who may use files failed: error: what stopped it; not_put_back: what it had changed and could not put back, and why.
access_failed_not_put_back = { $error }, and { $not_put_back }

# What a mode typed in Properties' Access field comes to, for files and folders apart where they differ. files, folders: each one's bits and who may do what (rw-r--r-- (644): the owner may read and write…).
access_files_and_folders = files { $files }; folders { $folders }

# What a mode in digits does, then that a folder keeps its set-ID bits. said: what the mode does; clears: the same mode written with five digits (00750), which clears them.
access_folder_keeps_set_id = { $said }; a folder keeps the set-ID bits it has ({ $clears } clears them)

# Why who may use a file cannot be changed, or put back as it was: it is gone.
access_gone = { $name } is not there any more

# A step of a plan found already done. inside: yes where it covers everything a folder holds too. Begins in lower case.
access_group_already = { $inside ->
        [yes] the group of { $name } and of everything inside was already as asked
       *[no] the group of { $name } was already as asked
    }

# Why who may use a file cannot be changed: it is in the trash.
access_in_trash = { $name } is in the trash: restore it first

# Why who may use a folder and what it holds cannot be changed: something inside belongs to someone else. path: where it is.
access_inside_not_yours = “{ $path }” inside is not yours: only its owner can change who may use it

# A step of a plan found already done. inside: yes where it covers everything a folder holds too. Begins in lower case.
access_mode_already = { $inside ->
        [yes] who may use { $name } and everything inside was already as asked
       *[no] who may use { $name } was already as asked
    }

# What a mode typed in Properties' Access field comes to. mode: its bits, as ls writes them (rw-r--r-- (644)); who: who may do what (the owner may read and write, the group and others may read).
access_mode_comes_to = { $mode }: { $who }

# Why a clause of a mode is refused where the system does not say its umask (macOS). part: the clause as typed (+x); u and a before it name the owner and everyone, as chmod writes them.
access_no_class = “{ $part }” names no class, and this system does not say what its umask leaves out, as chmod would: name the classes, as u{ $part } for the owner or a{ $part } for everyone

# Why what was typed in Properties' Access field is refused. text: what was typed.
access_not_a_mode = “{ $text }” is not a mode: write it as chmod does, as 644, u+x or go-w

# What a change of who may use files failed on; why follows. path: where the file is.
access_not_changed = who may use “{ $path }” could not be changed

# What putting back a file's group failed on; why follows. group: the group's number.
access_not_given_back = it could not be given back to the group { $group }

# What a change of a file's group failed on; why follows. path: where the file is; group: the group's number.
access_not_given_group = “{ $path }” could not be given to the group { $group }

# Why a file cannot be given to a group. group: the group's number. Begins in lower case: said after the file's name too.
access_not_in_group = you are not in the group { $group }

# Why a change of who may use a file did not hold (a drive formatted FAT).
access_not_kept = the drive holding { $name } did not keep the change: its filesystem keeps no permissions

# Why who may use a file cannot be changed. Begins in lower case: said after the file's name too.
access_not_owner = only its owner can change who may use it

# What putting back who may use files failed on. path: where the first file is; more: how many others could not be put back either; error: why the first could not.
access_not_put_back = { $more ->
        [0] who may use “{ $path }” could not be put back: { $error }
       *[other] who may use “{ $path }” could not be put back (and { $more } more): { $error }
    }

# What a mode typed in Properties' Access field comes to on Windows.
access_not_read_only = Not read-only

# What a dry run met inside a folder: something of someone else's. path: where it is.
access_not_yours = “{ $path }” is not yours

# Why a change of who may use files cannot run: neither a mode nor a group was given.
access_nothing_asked = nothing to change: give a mode or a group

# Who may do what with a mode's bits. both, others: none where they may not use it; both_does, others_does: what they may do (read and write; for a folder: list it and open it).
access_owner_group_then_others = the owner and the group { $both ->
        [none] may not use it
       *[some] may { $both_does }
    }, others { $others ->
        [none] may not use it
       *[some] may { $others_does }
    }

# Who may do what with a mode's bits. owner, rest: none where they may not use it; owner_does, rest_does: what they may do (read and write; for a folder: list it and open it).
access_owner_then_rest = the owner { $owner ->
        [none] may not use it
       *[some] may { $owner_does }
    }, the group and others { $rest ->
        [none] may not use it
       *[some] may { $rest_does }
    }

# A step of an undo, in its review: who may use a file put back as it was. more: how many other files it puts back. Begins in lower case.
access_put_back = { $more ->
        [0] put back who may use { $name }
       *[other] put back who may use { $name } and { $more } more
    }

# A step of an undo found already done. more: how many other files it covers. Begins in lower case.
access_put_back_already = { $more ->
        [0] who may use { $name } was already as before
       *[other] who may use { $name } and { $more } more was already as before
    }

# A step of an undo, done: who may use a file was put back as it was. more: how many other files it put back. Begins in lower case.
access_put_back_done = { $more ->
        [0] put back who may use { $name }
       *[other] put back who may use { $name } and { $more } more
    }

# What a mode typed in Properties' Access field comes to on Windows.
access_read_only = Read-only

# The title of the list of what a mode is written as, beside Properties' Access field.
access_reference = Access

access_reference_600 = The owner alone reads and writes

access_reference_644 = The owner reads and writes; the group and others read

access_reference_700 = The owner alone reads, writes and runs

access_reference_755 = Everyone reads and runs; the owner writes too

access_reference_also = May also

access_reference_big_x = Open folders, and run what someone may run already

access_reference_clauses_note = Clauses go one after another, by commas: u+x,go-w.

# A heading of the list of what a mode is written as: modes in digits, one for the owner, one for the group, one for others.
access_reference_digits = Digits: owner, group, others

access_reference_digits_note = Each digit adds 4 to read, 2 to write and 1 to run (to open a folder).

access_reference_everyone = Everyone

access_reference_group = The group

access_reference_no_longer = May no longer

access_reference_only = May only

access_reference_others = Others

access_reference_owner = The owner

access_reference_r = Read (list, for a folder)

access_reference_s = Run as its owner or group; a folder's group passes on

access_reference_t = Only their owners remove what a folder holds

# A heading of the list of what a mode is written as: the signs that follow whom a clause is about (+, -, =).
access_reference_then = Then

access_reference_w = Write (change what a folder holds)

# A heading of the list of what a mode is written as: the letters naming what a clause gives or takes (r, w, x…).
access_reference_what = What

# A heading of the list of what a mode is written as: the letters naming whom a clause is about (u, g, o, a).
access_reference_who = Who

access_reference_x = Run (open, for a folder)

# Why who may use a file cannot be changed. why: the reason (only its owner can change who may use it).
access_refused = { $name }: { $why }

# Why a change of who may use a file did not hold.
access_set_gid_cleared = the system cleared the set-group-ID bit of { $name }: only a member of its group may set it

access_sgid_both_file = a file runs as its group

access_sgid_both_file_gone = a file no longer runs as its group

access_sgid_both_folder = what is made in a folder takes its group

access_sgid_both_folder_gone = what is made in a folder no longer takes its group

access_sgid_file = it runs as its group

access_sgid_file_gone = it no longer runs as its group

access_sgid_folder = what is made in it takes its group

access_sgid_folder_gone = what is made in it no longer takes its group

access_sticky_both = only their owners may remove what a folder holds

access_sticky_both_gone = whoever may change a folder may remove what it holds

access_sticky_file = it keeps a sticky bit, which a file ignores

access_sticky_file_gone = its sticky bit, which a file ignores, goes

access_sticky_folder = only their owners may remove what it holds

access_sticky_folder_gone = whoever may change it may remove what it holds

# Why a change of who may use files failed: it was stopped, and what it had changed could not all be put back. not_put_back: what could not, and why.
access_stopped_not_put_back = stopped, and { $not_put_back }

access_suid_both = a file runs as its owner

access_suid_both_gone = a file no longer runs as its owner

access_suid_file = it runs as its owner

access_suid_file_gone = it no longer runs as its owner

access_suid_folder = it keeps a set-user-ID bit, which a folder ignores

access_suid_folder_gone = its set-user-ID bit, which a folder ignores, goes

# Two things a mode does, said one after the other (the owner may also run it; others may no longer write).
access_then = { $said }; { $then }

# What a change of who may use files failed on. path: where the folder or file is; why follows.
access_unreadable = “{ $path }” could not be read

# What a mode typed in Properties' Access field does on Windows, which keeps only the owner's write bit (w, as chmod writes it), where the mode leaves it as each item has it.
access_windows_each_stays = Each stays as it is: only the owner's w is kept here

# An action's title. Begins in lower case: said inside other words too.
app_access_group_title = change access and group

# An error answered to a script or an agent that asked to change access and gave neither a mode (as chmod takes it) nor a group.
app_access_nothing = say a mode, a group or both

# An action's title: the mode as chmod takes it (644, u+x), as written. Begins in lower case: said inside other words too.
app_access_title = change access ({ $mode })

# An action's name, for scripts and agents.
app_apply_plan = Apply plan

app_apply_plan_description = Apply a plan given as data (plan): as any command's, the caller applies it, at once for the user's own gesture, after review for an agent

# An action's name, for scripts and agents: what actions still bring into a folder.
app_arriving = Arriving in a folder

app_arriving_description = Return what the actions waiting and running bring into a folder (the pane's when none is given) and has not landed (ADR-135): each item's name there, the plan that brings it, its state (waiting, starting, copying with others at the batch's fraction, moving at its own fraction from 0 to 1, landed: in place while its action goes on), whether its action is paused, what it is copied or moved from, and whether it is a folder or a file of what size

# An action's name: go to the folder or place shown before.
app_back = Back

app_back_description = Go to the previous location

# An action's name, for scripts and agents.
app_commands = List commands

app_commands_description = Return every registered command with its argument schema

# The name of an action, in the palette, menus and F1's list.
app_copy = Copy to…

app_copy_description = Copy the selection, or the entries given, to a directory (plan); on_conflict says what becomes of a name taken there

# The name of an action, in the palette, menus and F1's list.
app_delete = Delete permanently

app_delete_description = Delete the selection, or the entries given, without trash (plan, irreversible)

# An action's title: deleting files without the trash. Begins in lower case: said inside other words too.
app_delete_title = delete permanently

# An error answered to a script or an agent that gave a move or a copy an address that is no folder's path.
app_destination_path = destination must be a path

# An action's name, for scripts and agents.
app_entries = List entries

app_entries_description = What a pane shows, in its order (sorted, filtered, with the folders opened in place), or with `location` what a folder holds, listed without showing it: each entry's id, name, path, kind, size and modification time (seconds since 1970), and whether it is selected; from `offset`, at most `limit`. `complete: false`: the listing was still arriving, ask again. `as_of`, when present: the answer is an index's as of its last update (`by` says which, `updated` when, in seconds since 1970), so a file made, renamed or removed since is not in it. `excluded`, when present: how many files and folders below the root the search walked past (the folders left out in Settings › General › Searches, caches, trash, network folders); `excluded:yes` in the search walks the list's

# An action's name: open a folder in place, under the one shown, as a tree does.
app_expand = Expand folder

app_expand_description = Open folders in place under the one shown, as the tree view draws them, or close them

# An action's name, a verb: show only the files that match.
app_filter = Filter

app_filter_description = Show only the entries `text` keeps: as the filter's reader reads it (the query plugin: words find names, terms the listing can decide such as kind:image, ext:pdf or size>1M hold for files, a folder showing once a search of it finds one), else those whose name holds it (case ignored unless it has a capital); without `text`, all of them. Its terms that hold for files are the pane's lens, kept as it shows other locations until taken off; its words go with the location

# An action's name: go again where Back came from.
app_forward = Forward

app_forward_description = Return to the location Back came from

# An action's name, a verb: show a pane's files under headings.
app_group = Group

# An action's name: group the files by what they are sorted by, in its direction.
app_group_as_sorted = Group the same as the sort

# An action's name in the palette: what the files are grouped by (name, date modified).
app_group_by = Group by { $key }

# An action's name in the palette: what the files are grouped by (date modified), then the groups' direction (oldest first, A to Z).
app_group_by_way = Group by { $key }, { $way }

app_group_description = Group a pane's entries under headings (ADR-130): by `by` (none, same_as_sort: by the sort's key in its direction, or a key as `sort` takes: name, kind, type, extension, size, modified, created, accessed, deleted, rating, tags, owner, folder; taken, dimensions, duration, read from the files on demand), in direction `ascending` (a key starts in its own: words A to Z, the largest, the newest, the best rated first; the same key again flips it). Within each group, folders come first while they do everywhere, then the sort decides

# An action's name: fold a group of files under its heading.
app_group_fold = Fold the group

app_group_fold_description = Fold or open a pane's groups (ADR-131): group `group` (counted from the first heading from 0; the cursor's when omitted) as `mode` says: toggle, fold, unfold, fold_others (every group but it), unfold_all. A folded group's entries leave what the pane shows (not drawn, passed over by the keys, out of Select All and of what commands act on), its heading kept with its count; folds are kept while the pane groups by the same criterion, and take effect only under a view that draws the groups' headings (the list, the tree, the grid): under any other, every file stays shown

app_group_fold_others = Fold the other groups

# An action's name: fold the group of files the cursor is in under its heading, or open it.
app_group_fold_toggle = Fold or unfold the group

app_group_none = Do not group

# An action's title: the group by its number. Begins in lower case: said inside other words too.
app_group_title = change group ({ $group })

app_group_unfold_all = Unfold all groups

# The name of an action, in the palette, menus and F1's list.
app_history = Undo history

app_history_description = Return the actions Undo can take back, newest first (ADR-079, 13). Each gives its `entry` (what undo and redo are given as `entry`), its `title`, `at`: when it applied (seconds since 1970, UTC), its `state` (running, waiting, done or undone), `can`: what its Undo, or an undone action's Redo, can do now (`now`; `once_ended`, while it runs or waits; `after`, once the action it names is undone or redone first; `going`, while it is being undone or redone; `never`), and `with`: the entries that undo with `with_after` takes back with it

# An action's name, for scripts and agents: read the files' sizes, dates and notes.
app_hydrate = Load metadata

app_hydrate_description = Request metadata for entries (views call this for visible rows)

# An error: Show in folder asked of a path that is a drive's top.
app_in_no_folder = { $path } is in no folder

# An action's name: keep what the pane shows as a place in the sidebar.
app_keep = Add to sidebar

app_keep_cannot = this location cannot be kept in the sidebar

app_keep_description = Keep what the pane shows in the sidebar, as it shows it (ADR-109): a folder as a bookmark, a search as a saved search, anything shown filtered as a saved place with its filter

# An error: Add to sidebar asked of a pane that shows nothing.
app_keep_nothing = the pane shows nothing to keep

# An action's name: make links to the files in another folder.
app_link = Link to…

app_link_description = Make a link in a directory to each of the selection, or the entries given, under its name (plan); on_conflict says what becomes of a name taken there

# The name of an action, in the palette, menus and F1's list.
app_mkdir = New folder

app_mkdir_description = Create a folder in the folder given in `in`, or the one the pane shows (plan)

# The name of an action, in the palette, menus and F1's list.
app_move = Move to…

app_move_description = Move the selection, or the entries given, to a directory (plan); on_conflict says what becomes of a name taken there

# An action's name: go to a folder or place by its path or address.
app_navigate = Go to location

# What the action does, read by agents and in F1's list of actions.
app_navigate_description = Open a path or location in a pane, an entry of it selected once listed when `select` names one, else, gone up to it, the one the way back down goes through; `in_place` puts it in place of the step the pane shows, with no step of its own; `as_kept` shows it as a saved place is kept, filtered by `filter` alone, the pane's lens left out (ADR-109)

# The name of an action, in the palette, menus and F1's list.
app_new_file = New file

app_new_file_description = Create an empty file in the folder given in `in`, or the one the pane shows (plan); a name ending in / makes a folder

# An action's title: the new file's name. Begins in lower case: said inside other words too.
app_new_file_title = new file { $name }

# An action's title: the new folder's name. Begins in lower case: said inside other words too.
app_new_folder_title = new folder { $name }

# An error: an action that needs a folder asked of a pane that shows a search, a tag or another place.
app_no_directory = pane is not showing a directory

# An error answered to a script or an agent that named a pane the window does not have.
app_no_pane = no such pane

# An error answered to a script or an agent: the pane it named, by its number (as written).
app_no_pane_numbered = no pane { $pane }

# An error: nothing Skwir has opens the address's scheme (sftp, tag…), which is as written.
app_no_provider = no provider for scheme `{ $scheme }`

# An error: an action that works in a folder (a terminal, a command) asked where the pane shows no folder and nothing is selected.
app_no_work_dir = select something, or open a folder

# A switch of a place notes are kept: whether Skwir reads the notes kept there.
app_notes_read = Read

# Under the Read switch of a place notes are kept (a file's attributes, a file beside it, its name).
app_notes_read_all = It holds all of a file's notes.

# Under the Read switch of a place notes are kept: what it cannot hold that the others can, a phrase the place gives (a comma in a tag).
app_notes_read_limits = What it cannot hold: { $limits }.

# In the tooltip of the Read switch of a place notes are kept, after what it holds.
app_notes_read_merged = Read, what it keeps is shown merged with the other places' notes.

# A choice of a place notes are kept: when Skwir writes notes there.
app_notes_write = Write

# Under the Write choice of the notes kept in a file's name: its choices, Off and Update existing, in order.
app_notes_write_renames = Never; or, where a file's name already holds tags, rename the file as an edit changes them.

# Under the Write choice of a place notes are kept: its choices (Off, Update existing, Where attributes are missing, Create), in order.
app_notes_write_when = Never; only where a file already has notes here; also where a file's volume keeps no attributes; or for every file edited.

# An error: Take out of here asked in a folder, where files are not listed from elsewhere. Move to Trash is the action of that name.
app_nothing_taken_out = nothing is taken out of here: Move to Trash deletes

app_nothing_to_redo = nothing to redo

# An error: Retry asked where no action was stopped by an error.
app_nothing_to_retry = nothing to retry

app_nothing_to_undo = nothing to undo

# An error: the folders' remembered orders, kept under a key of the data file (as written), cannot be read.
app_orders_unreadable = { $key } in the data file holds no list: mend it, or take it out, to remember an order

# An error: the pane showed another place before its folder's order could be remembered.
app_pane_went_elsewhere = the pane went elsewhere

# What the empty field asking for an encrypted volume's passphrase says.
app_passphrase = Passphrase

# What the empty field asking for a password (an archive's, a server's) says.
app_password = Password

# An action's name, for scripts and agents: say what each step of a plan would do, doing nothing.
app_plan_diff = Preview plan

app_plan_diff_description = Dry-run a plan and return what each step would do

# The name of an action, in the palette, menus and F1's list.
app_redo = Redo

app_redo_description = Redo the most recently undone plan, or the undone history entry named when nothing undone after it touched what it did; asked from outside the window, says what it applied again; from the window, it answers the id its run goes by (runs), as Undo does

# An action's name: list the folder shown again.
app_refresh = Refresh

app_refresh_description = Re-list the pane's location

# An error: a path given as ./a or ../b where the pane shows a search or a tag, not a folder.
app_relative_nowhere = { $path } is relative, and no folder is shown for it to be in

# The name of an action, in the palette, menus and F1's list.
app_remember_order = Remember this folder's order

app_remember_order_description = Remember how the folder a pane shows is sorted and grouped (ADR-132), and the view it is shown in (`view`: list or grid), or forget it (`on`; toggled when omitted): a folder that remembers is shown so in every pane, a change of its sort, groups or view made there changing what it remembers, the pane's own order and view set aside while it shows it and back once it shows another; forgotten, the pane's own come back. Its view is shown only while the pane shows a view a folder may remember, never in place of the columns or the tree; each column of the columns is in its own folder's order. Any place but a search, whose terms are what it is (the trash, a tag's place); hidden files stay the pane's, folders first the user's

# The name of an action, in the palette, menus and F1's list.
app_rename = Rename

app_rename_description = Rename the selected entry, or the one given (plan)

# An error: Rename asked with none, or several, selected.
app_rename_one = rename needs exactly one entry

# An action's title: the file's new name. Begins in lower case: said inside other words too.
app_rename_title = rename to { $name }

# An action's name: run again what an action stopped by an error had left.
app_retry = Retry

app_retry_description = Run again what a plan stopped by an error had left, the last one stopped unless told which; what it applies is undone with that plan; asked from outside the window, says what it applied

# The question asked when a drive says it has too little room for an action: its title first, in lower case (copy 3 items to Docs).
app_room_detail = { $title }: a drive, a network share above all, may say it has less room than it has. Copy anyway, or cancel and make room.

# An action's name, for scripts and agents.
app_running = Running actions

app_running_description = Return the actions running, in the order they started: each one's title, steps ended, bytes done and in all when known (planned), speed in bytes a second and time left in seconds while they can be told, and the files a server still takes (ADR-113)

# An error: Remember this folder's order asked of a search.
app_search_order = a search's order is not remembered: its terms are what it is

# An action's name: select entries named.
app_select = Select

# The name of an action, in the palette, menus and F1's list.
app_select_all = Select all

app_select_all_description = Select every entry

app_select_description = Select entries by id or name

# The name of an action, in the palette, menus and F1's list.
app_select_invert = Invert selection

app_select_invert_description = Select what is shown and not selected, and deselect the rest

# An error answered to a script or an agent: the name as it gave it.
app_select_no_entry = no entry named `{ $name }`

# The name of an action, in the palette, menus and F1's list.
app_select_none = Clear selection

app_select_none_description = Deselect everything

# An error answered to a script or an agent: the name as it gave it.
app_select_not_shown = `{ $name }` is not shown: hidden, or left out by the filter

# An error answered to a script or an agent that asked to select a range and gave no entry it ends at.
app_select_range_target = a range needs a target id

# The name of an action, in the palette, menus and F1's list.
app_select_sweep = Select as the cursor moves

app_select_sweep_description = Hold a range from the cursor: moving selects, or with `unset` deselects, what lies between

# The name of an action, in the palette, menus and F1's list.
app_select_sweep_end = Stop selecting as the cursor moves

app_select_sweep_end_description = End the range held from the cursor, keeping what it covers

app_select_sweep_unset = Deselect as the cursor moves

# An action's name: change who may read, write and run files.
app_set_access = Change access

# An action's name, for scripts and agents: say in words what a mode as chmod takes it does.
app_set_access_describe = Say what a mode does

app_set_access_describe_description = Say in words what `mode` (as chmod takes it) makes of items whose mode is `current` (all sharing it, a number; else what each clause does to each), `folders` or `files` when they all are one (else both, said apart where they differ); an error for what is no mode, as `set_access` refuses it

app_set_access_description = Change who may read, write and run the selection, or the entries given: `mode` as chmod takes it, `group` by number, `inside` for what folders hold (plan). What a link leads to changes; the owner never does

# An action's name: companion files keep another file's notes beside it.
app_show_companions = Show hidden and companion files

app_show_companions_description = Show hidden files in the pane, and turn on Show companion files (`core.show_companions`): the files that keep another file's notes beside it, in every folder

# The name of an action, in the palette, menus and F1's list.
app_show_in_folder = Show in folder

app_show_in_folder_description = Open the folder the selected file is in, the file selected: from a search, a tag, the shelf, where files from many folders are listed

# An error: Show in folder asked with none, or several, selected.
app_show_in_folder_one = select one file to show in its folder

# An error: an action asked something of a pane that shows no folder or place yet.
app_shows_nothing = the pane shows nothing

# An action's name, a verb: set the order of a pane's files.
app_sort = Sort

app_sort_ascending = Sort ascending

# An action's name in the palette: what the files are sorted by (name, date modified).
app_sort_by = Sort by { $key }

# An action's name in the palette: what the files are sorted by (date modified), then its direction (oldest first, A to Z).
app_sort_by_way = Sort by { $key }, { $way }

app_sort_descending = Sort descending

app_sort_description = Set the sort key, direction, and hidden-file visibility of a pane

# An action's name: take files out of a tag's or the shelf's list, the files left where they are.
app_take_out = Take out of here

app_take_out_description = Take the selection out of a location that lists files kept elsewhere, as it says (a tag's takes its tag off them, the shelf's unshelves them): the files stay where they are. What the Delete key does there

# A button on a message saying what an edit did: does it again.
app_toast_redo = Redo

# A button on a message saying what an edit did: takes it back.
app_toast_undo = Undo

# The name of an action, in the palette, menus and F1's list.
app_toggle_hidden = Toggle hidden files

app_toggle_hidden_description = Show or hide dot-prefixed entries

# The name of an action, in the palette, menus and F1's list.
app_trash = Move to trash

app_trash_description = Trash the selection, or the entries given (plan)

# Said under the question asking to delete files for good, of those in network folders, which keep no trash: how many they are.
app_trash_none_kept = { $count ->
        [1] It is in a network folder, which keeps no trash
       *[other] They are in network folders, which keep no trash
    }

# Said under the question asking to delete files for good, of those in network folders, which keep no trash (count), and the others, which go to the trash (kept).
app_trash_others_kept = { $count ->
        [1] It is in a network folder, which keeps no trash
       *[other] They are in network folders, which keep no trash
    }; { $kept ->
        [1] the other item goes to the trash
       *[other] the other { $kept } items go to the trash
    }

# An action's title: moving files to the trash. Begins in lower case: said inside other words too.
app_trash_title = move to trash

# The name of an action, in the palette, menus and F1's list.
app_undo = Undo

app_undo_description = Undo the most recent plan, or the plan or history entry named when nothing asked after it touched what it did (with_after, with an entry: with the entries after it that did, newest first); asked from outside the window, says what it took back, by the titles of the history's entries; from the window, it answers the ids its runs go by (runs), for the window to follow its own runs alone

# An error answered to a script or an agent that asked Undo for both a plan and an entry of the history.
app_undo_plan_or_entry = name a plan or an entry, not both

# An error answered to a script or an agent: with_after and entry are the arguments' names, as written.
app_undo_with_after = with_after takes back an entry with what came after it: name the entry

# An action's name: go up to the folder holding the one shown.
app_up = Parent directory

app_up_description = Go to the folder holding the one shown, or to the place before it in its trail (a tag's category, the folder a search looks in), and select there the one just left

# Why a copy, a move or a link does not go on, where no one is asked: its name is taken in the folder it goes to.
clash_taken = { $name } is already there

# Said to an agent or a script: the action's name, then why its arguments were refused, as the JSON reader wrote it (English).
command_invalid_args = invalid arguments for `{ $command }`: { $reason }

# Why a command that acts on the selection did nothing. Begins in lower case: said after other words too.
command_nothing_selected = nothing selected

# Why Paste did nothing: neither Skwir nor the system's clipboard holds files. Begins in lower case.
command_nothing_to_paste = nothing to paste

# An action failed through a fault of Skwir's or of a plugin's: its name (an identifier, as written).
command_panicked = “{ $command }” stopped on a fault of its own (the log says where)

# Said to an agent, a script or a key bound by the user: no action has this name (an identifier, as written).
command_unknown = unknown command `{ $command }`

# A command named with the arguments it runs with, where none of its forms names them (Sort (key name)). args: the arguments as a script writes them, never translated.
command_with_args = { $title } ({ $args })

# A note of a deletion's review: files deleted here have other names (hard links) elsewhere, so what they hold stays on the disk.
entry_kept_elsewhere = { $count ->
        [one] { $count } file has names elsewhere: the disk keeps its { $size }
       *[other] { $count } files have names elsewhere: the disk keeps their { $size }
    }

# Said to an agent that asked to change the exclusions: what it would have let agents read, each as the Exclusions page says it. Settings and Exclusions are a dialog's and its page's names.
exclusions_agent_exposes = an agent may not make private folders readable ({ $folders }): only you can, in Settings, Exclusions

# Said to an agent that asked to change the setting. Settings and Privacy are a dialog's and its page's names.
exclusions_agent_shell = an agent may not change whether the login shell's variables are read: a private folder only they name would stop being private. Only you can, in Settings, Privacy

# An exclusion refused: its pattern as written, then what the pattern's reader said of it (English).
exclusions_bad_glob = { $pattern }: { $error }

# An exclusion refused: names is the field's name, files, folders and both what it takes, all as written; then what it holds.
exclusions_bad_names = names is files, folders or both, not { $names }

# An exclusion refused: its regular expression does not read. regex: is written so; then what the regular expression's reader said (English).
exclusions_bad_regex = regex: { $error }

# An exclusion refused: the path as written, then how a variable is written in it. NAME stands for a variable's name; the signs are written so.
exclusions_bad_variable = { $pattern }: a variable is $NAME, ${"{"}NAME{"}"} or ${"{"}NAME:-what stands for it unset{"}"}

# An exclusion refused: it names files, but only a folder is excluded where a file is beside it. The pattern as written; beside is the field's name.
exclusions_beside_files = { $pattern }: beside a file goes with folders, not files

# An exclusion refused: what its beside field holds, as written, is a path, where it takes a name. beside is the field's name.
exclusions_beside_name = { $sibling }: beside names a file or a folder, not a path

# An exclusion refused: a folder excluded where a file is beside it is written as a name (target is an example), not a path. The pattern as written.
exclusions_beside_path = { $pattern }: beside a file, the folder is a name (target), not a path

# An exclusion in the settings refused: its id begins with skwir. (as written), which only Skwir's own have, yet it names a folder.
exclusions_builtin_id = skwir. names Skwir's own folders; give yours a name without a dot

# A setting drawn by the Exclusions page, read in the settings file and by agents.
exclusions_categories = Exclusion categories

# Skwir is the program's name; searches, agents and index are the roles' keys, as written.
exclusions_categories_description = What changed of Skwir's categories of exclusions (a role switched: searches, agents, index), and yours (a name)

# The categories of exclusions in the settings refused: they are written as a TOML table, each under its id.
exclusions_categories_table = folder categories are a table, by id

# A category of the user's in the settings refused: it has no name.
exclusions_category_name = a category of yours needs a name

# A category in the settings refused: it is written as a TOML table.
exclusions_category_not_table = a category is a table

# An exclusion refused: what it names is empty.
exclusions_empty = a folder needs a name or a path

# A setting drawn by the Exclusions page: the files and folders left out of searches, kept from agents.
exclusions_entries = Exclusions

# Read in the settings file and by agents. regex:, beside and holding are written so.
exclusions_entries_description = What changed of Skwir's exclusions (a role unticked, a category), and yours: a pattern (a name, a path, a glob or regex:…, of files, folders or both, with beside), or holding (a file a folder holds), what it is, its category, and the roles it is excluded from

# The exclusions in the settings refused: they are written as a TOML table, each under its id.
exclusions_entries_table = exclusions are a table, by id

# An exclusion in the settings refused: it is written as a TOML table.
exclusions_entry_not_table = an entry is a table

# An exclusion refused: beside, names, pattern and holding are the fields' names, as written.
exclusions_holding_alone = beside and names go with a pattern, not with holding

# An exclusion refused: holding is the field's name, which takes a file's name, not a path.
exclusions_holding_name = holding is the name of a file a folder holds

# An exclusion or a category of the user's in the settings refused: an id with a dot is Skwir's (Skwir is the program's name).
exclusions_id_dot = an id of yours has no dot: those are Skwir's and its plugins'

# A setting of an older version (0.3.1) still read, in the settings file.
exclusions_legacy_folders = Private folders (before exclusions)

# 0.3.1 is the version of Skwir that had them.
exclusions_legacy_folders_description = 0.3.1's private folders, read as exclusions kept from agents

# A setting of an older version (0.3.1) still read, in the settings file.
exclusions_legacy_marker = Folders marked private (before exclusions)

# .private is a file's name, as written.
exclusions_legacy_marker_description = 0.3.1's switch for .private, read as the exclusion holding a .private

# An exclusion refused: it says neither. pattern and holding are the fields' names, as written.
exclusions_names_nothing = pattern or holding says what it names

# An exclusion in the settings names a category that is not there: its id, as written.
exclusions_no_category = no category { $category }

# An exclusion or a category in the settings refused: it holds a field of this name (as written), which none has.
exclusions_no_field = no field { $key }

# An exclusion or a category in the settings refused: it has no id.
exclusions_no_id = an id is needed

# An exclusion or a category in the settings refused: the field's name (searches, agents, index), which takes true or false, as written.
exclusions_not_switch = { $key }: true or false

# An exclusion refused: the path as written, then how a path begins: ~ (home), a variable, or the root, a whole path given as an example.
exclusions_path_starts = { $pattern }: a path starts with ~, a variable ($CARGO_HOME) or is whole ({ $whole })

# An exclusion refused: it says both a pattern and a file a folder holds.
exclusions_pattern_or_holding = a pattern or a file it holds, not both

# What an example of an exclusion names: the folders named target at any depth below ~/Projects.
exclusions_ref_any_depth = At any depth below

# The title of the list under an exclusion's beside field: a folder is excluded only where this is beside it.
exclusions_ref_beside = Only beside

# Under the examples of an exclusion's beside field: what it takes.
exclusions_ref_beside_note = A name or a name's pattern, never a path.

# A heading of the examples an exclusion's field takes: a name excludes what is so named in any folder.
exclusions_ref_by_name = By name, anywhere

# A heading of the examples an exclusion's field takes: a path names one folder, or those like it.
exclusions_ref_by_path = By path

# What an example of the holding field names: CACHEDIR.TAG, the file a program leaves in its cache folders, which begins with a signature.
exclusions_ref_cache_mark = A cache's own mark (signed)

# What {a,b} matches in an exclusion's pattern: what is on either side of the comma.
exclusions_ref_either = Either

# A heading of the examples an exclusion's holding field takes.
exclusions_ref_file_held = A file it holds

# What an example of an exclusion names: one folder, its path from the home folder (~).
exclusions_ref_from_home = One, from your home

# What an example of an exclusion names: one folder, its whole path.
exclusions_ref_from_root = One, from the root

# What an example of an exclusion names: a folder whose path starts at a variable.
exclusions_ref_from_variable = From a variable

# The title of the list under an exclusion's holding field: a folder is excluded where it holds this file.
exclusions_ref_holding = Holding

# What an example of the holding field names: any folder holding a file named .nosearch.
exclusions_ref_holding_named = Any folder holding a file so named

# Under the examples of an exclusion's holding field: what it takes.
exclusions_ref_holding_note = A file's name: the folder holding it is excluded, wherever it is.

# What an example of the beside field asks: a file named like *.csproj beside the folder.
exclusions_ref_like_beside = Only beside one named like it

# A heading of the examples an exclusion's beside field takes.
exclusions_ref_name_beside = A name beside it

# What an example of an exclusion names: what is named like *.egg-info.
exclusions_ref_named_like = Any named like it

# What ? matches in an exclusion's pattern.
exclusions_ref_one = One character

# What [ab] matches in an exclusion's pattern: one of the letters in brackets.
exclusions_ref_one_of = One of these

# What an example of an exclusion names: the folders whose paths are like it (* for any name).
exclusions_ref_path_like = Those whose path is like it

# Under the examples of paths an exclusion takes: how a path is told from a name. The root is given as an example path.
exclusions_ref_path_note = A path starts at ~, a variable or a root ({ $whole }); anything else is a name.

# The title of the list under an exclusion's field: what it takes.
exclusions_ref_pattern = Name or path

# What an example of an exclusion is: regex: then a regular expression, matched over the path written with ~ for the home folder.
exclusions_ref_regex = A regular expression, ~ for home

# Under the signs of a regular expression, in an exclusion's field: where it is read. regex: is written so.
exclusions_ref_regex_note = After regex:, over the path written with ~ for your home.

# A heading of the signs a pattern of an exclusion reads.
exclusions_ref_signs = Signs of a name or a path

# What an example of an exclusion names: any folder or file named node_modules.
exclusions_ref_so_named = Any so named

# What an example of the beside field asks: a file or folder named Cargo.toml beside the folder.
exclusions_ref_so_named_beside = Only where a file or folder so named is beside it

# What * matches in an exclusion's pattern.
exclusions_ref_star = Any characters, within one name

# What ** matches in an exclusion's pattern.
exclusions_ref_stars = Any folders, in a path

# What an example of an exclusion names: what is named cmake-build-, then anything.
exclusions_ref_starts_so = Any whose name starts so

# What an example of an exclusion names: a path from a variable, and the path used where the variable is not set.
exclusions_ref_variable_unset = From a variable, or what stands for it unset

# A heading of the example of a regular expression an exclusion takes, matched over the folder's path.
exclusions_ref_whole_path = Over the whole path

# An exclusion or a category in the settings that cannot be read: where (its id, as written), then why.
exclusions_refused = { $at }: { $why }

# What an exclusion means that is written wrong (a path where a name goes), then the file beside it as written.
exclusions_said_beside = { $what } beside a { $sibling }

# What an exclusion means, on the Exclusions page: the file's name as written (a .nosearch, a CACHEDIR.TAG).
exclusions_said_holding = any folder holding a { $file }

# What an exclusion means, on the Exclusions page: what it names and the pattern of its name as written (*.kdbx).
exclusions_said_like = { $names ->
        [files] any file
        [folders] any folder
       *[both] any file or folder
    } whose name is like { $pattern }

# What an exclusion means, on the Exclusions page: what it names, the pattern of its name (*-build), and the file beside it (a *.pro), both as written.
exclusions_said_like_beside = { $names ->
        [files] any file
        [folders] any folder
       *[both] any file or folder
    } whose name is like { $pattern } beside a { $sibling }

# What an exclusion means, on the Exclusions page: what it names and the regular expression their paths match, as written.
exclusions_said_matching = { $names ->
        [files] the files
        [folders] the folders
       *[both] the files and folders
    } matching { $pattern }

# What an exclusion means, on the Exclusions page: what it names (files, folders or both) and its name as written (node_modules).
exclusions_said_name = { $names ->
        [files] any file
        [folders] any folder
       *[both] any file or folder
    } named { $pattern }

# What an exclusion means, on the Exclusions page: what it names (files, folders or both), its name, and the file beside it (target beside a Cargo.toml), both as written.
exclusions_said_name_beside = { $names ->
        [files] any file
        [folders] any folder
       *[both] any file or folder
    } named { $pattern } beside a { $sibling }

# What an exclusion means, on the Exclusions page: a path from a variable as written ($CARGO_HOME/registry), then the folder it is on this computer.
exclusions_said_now = { $pattern }, now { $path }

# What an exclusion means, on the Exclusions page: what it names and the pattern of their paths as written (~/.var/app/*/cache).
exclusions_said_path_like = { $names ->
        [files] the files
        [folders] the folders
       *[both] the files and folders
    } like { $pattern }

# What an exclusion means, on the Exclusions page: a path from a variable as written, and the variables it needs (their names) that are not set here, so it names no folder.
exclusions_said_unset = { $pattern }, while { $variables } is not set: none

# A switch on the Privacy page: whether the login shell is asked for variables a path of the exclusions starts at.
exclusions_shell_variables = Read your shell's variables

# CARGO_HOME, .zshrc and config.fish are names, as written.
exclusions_shell_variables_description = Read variables such as CARGO_HOME from your login shell too, where a path starts at one.

# CARGO_HOME, .zshrc and config.fish are names, as written.
exclusions_shell_variables_description_more = A Skwir started from your desktop does not see what .zshrc or config.fish export. The shell is asked in the background once its files change, and only the variables these paths name are kept.

# A notice leading to the Exclusions page: exclusions in the settings that cannot be read, so they keep nothing private. count: how many; first: the first of them, where it is and why.
exclusions_unreadable = { $count ->
        [1] An exclusion cannot be read, so it does not apply: { $first }
       *[other] { $count } exclusions cannot be read, so they do not apply. The first: { $first }
    }

# What one of Skwir's exclusions is, where it names several folders: what the first says it is, then this.
exclusions_what_also = { $what } (another of its list)

# What one of Skwir's exclusions is, for the folder a program used before it moved it: what the exclusion says it is, then this.
exclusions_what_before = { $what } (where it was before it moved)

# What one of Skwir's exclusions is: the folder programs keep caches in, moved from ~/.cache by the variable named.
exclusions_what_cache_home = Your cache folder (XDG_CACHE_HOME)

# What an exclusion is that the user's list of private folders of an older version (0.3.1) held.
exclusions_what_legacy = Private (from 0.3.1's list)

# Why a redo asked of an action waits for nothing: a redo of it already runs or waits. title: the action's title.
exec_being_redone = “{ $title }” is being redone already

# Why an undo asked of an action waits for nothing: an undo of it already runs or waits. title: the action's title.
exec_being_undone = “{ $title }” is being undone already

# Why an undo did nothing: nothing the action did can be taken back (a file deleted for good). title: the action's title.
exec_cannot_be_undone = “{ $title }” cannot be undone

# Why Undo with what came after it stopped: one of its undos was cancelled. title: what that undo is called (undo, then the action).
exec_chain_cancelled = “{ $title }” was cancelled

# Why Undo with what came after it stopped: one of its undos did not run. title: what that undo is called (undo, then the action).
exec_chain_did_not_run = “{ $title }” did not run

# Why Undo with what came after it stopped: one of its undos stopped. title: what that undo is called (undo, then the action); error: why it stopped.
exec_chain_stopped = “{ $title }” stopped: { $error }

# Why a redo asked of an action does nothing: the history keeps it no more. Begins in lower case.
exec_gone_from_history_redo = that action is no longer in the history: nothing to redo

# Why an undo asked of an action does nothing: the history keeps it no more. Begins in lower case.
exec_gone_from_history_undo = that action is no longer in the history: nothing to undo

# Why an action of a chain of undos was left: an undo before it stopped. title: the action's title; why: what stopped.
exec_not_undone = “{ $title }” was not undone: { $why }

# Why a redo asked of an action does nothing: it is not undone. title: the action's title.
exec_not_undone_nothing = “{ $title }” is not undone: nothing to redo

# Why a redo did nothing: when its turn came, nothing was left to apply again. Begins in lower case.
exec_nothing_to_redo = nothing to redo

# Why an undo did nothing: when its turn came, nothing was left to take back. Begins in lower case.
exec_nothing_to_undo = nothing to undo

# What a redo is called while the action it applies again is not known yet. Begins in lower case: said inside other words too.
exec_redo = redo

# Why an undone action cannot be redone by itself: another action, undone after it, changed the same files. title, other: the two actions' titles.
exec_redo_held = “{ $title }” cannot be redone alone: “{ $other }”, undone after it, touched what it did; redo that first

# Why an undone action cannot be redone: another action, undone after it, changed the same files and cannot be undone. title, other: the two actions' titles.
exec_redo_held_for_good = “{ $title }” cannot be redone: “{ $other }”, undone after it, touched what it did and cannot be undone

# Why an action of a chain of undos was left (its title is not known any more): an undo before it stopped. why: what stopped. Begins in lower case.
exec_that_not_undone = that action was not undone: { $why }

# What an undo is called while the action it takes back is not known yet. Begins in lower case: said inside other words too.
exec_undo = undo

# Why an action cannot be undone by itself: another action, asked after it, changed the same files. title, other: the two actions' titles.
exec_undo_held = “{ $title }” cannot be undone alone: “{ $other }”, asked after it, touched what it did; undo that first

# Why an action cannot be undone: another action, asked after it, changed the same files and cannot be undone itself. title, other: the two actions' titles.
exec_undo_held_for_good = “{ $title }” cannot be undone: “{ $other }”, asked after it, touched what it did and cannot be undone

# Why an action cannot be undone by itself: another action, asked after it and running now, changed the same files. title, other: the two actions' titles.
exec_undo_held_running = “{ $title }” cannot be undone alone: “{ $other }”, asked after it and running now, is changing what it did; undo that first

# Why an action cannot be undone by itself: another action, asked after it and waiting to run, will change the same files. title, other: the two actions' titles.
exec_undo_held_waiting = “{ $title }” cannot be undone alone: “{ $other }”, asked after it and waiting to run, will change what it did; undo that first

# Why an undo asked of an action does nothing: it was undone before. title: the action's title.
exec_undone_already = “{ $title }” is undone already

# Why Skwir will not keep its session's files (its sockets, its mounts) in a folder: someone else owns it. The folder's path.
folders_not_private = { $path } is not the user's alone

# A field of a form left empty: its title.
form_needed = { $field } is needed

# A field of a form given a value none of its choices has: its title.
form_one_of_choices = { $field } is one of its choices

# A field of a form that takes a whole number: its title, then the least and the most it takes.
form_whole_number = { $field } is a whole number from { $min } to { $max }

# Shown in place of a folder's files: the folder shown went away (deleted, or its drive or server let go).
fs_folder_gone = This folder was deleted or is no longer available.

# Said to an agent or a script: what was given to open as a folder is no path (an address, as written).
fs_not_a_path = not a path: { $location }

# Why a file was not opened: it is a folder, a pipe, a device. Begins in lower case: said after other words.
fs_not_regular = not a regular file

# Why tags or notes kept in a file's attributes are not read or written on a network folder. Begins in lower case: said after other words.
fs_server_no_xattrs = this server keeps no extended attributes

# A size in bytes, as a file's size is shown (999 B). B is the symbol of a byte.
fs_size_bytes = { $n } B

fs_size_gb = { $n } GB

fs_size_gib = { $n } GiB

# A size in kilobytes (1000 bytes): n has one digit after the decimal sign (1.5 kB). Symbols of 1000-based units: kB, MB, GB, TB, PB.
fs_size_kb = { $n } kB

# A size in kibibytes (1024 bytes): n has one digit after the decimal sign (1.5 KiB). Symbols of 1024-based units: KiB, MiB, GiB, TiB, PiB.
fs_size_kib = { $n } KiB

fs_size_mb = { $n } MB

fs_size_mib = { $n } MiB

fs_size_pb = { $n } PB

fs_size_pib = { $n } PiB

fs_size_tb = { $n } TB

fs_size_tib = { $n } TiB

# Why a copy could not wait for the drive to write what it holds (before a drive is ejected): the system's name.
fs_sync_not_yet = Writing through to a drive is not available on { $system } yet

# Why a file could not be marked hidden (as Windows and FAT drives mark it): the system's name. Begins with it.
fs_system_no_hidden = { $system } keeps no hidden mark

# Why tags or notes kept in a file's attributes are not read or written: the system's name. Begins with it.
fs_system_no_xattrs = { $system } keeps no extended attributes

# Why a file could not be read just now: an action is moving or removing it. Begins in lower case: said after other words.
fs_taken = it is being moved or removed

# Why a file kept beside another (a sidecar, a note) was not read: it is larger than such a file is. Begins in lower case: said after other words.
fs_too_large = file too large

# Why a file could not be marked hidden: its drive's filesystem has no such mark. Begins in lower case: said after other words.
fs_volume_no_hidden = its volume keeps no hidden mark

# Why a 3D model cannot be shown: its colours and its points are not as many.
inspection_mesh_colours = the mesh has { $colours ->
        [one] { $colours } colour
       *[other] { $colours } colours
    } for { $positions ->
        [one] { $positions } position
       *[other] { $positions } positions
    }

# Why a 3D model cannot be shown: its mesh is empty.
inspection_mesh_empty = the mesh has no triangles

# Why a 3D model cannot be shown: a corner's number, and how many positions the mesh has.
inspection_mesh_index = the mesh's index { $index } names no position (it has { $count })

# Why a 3D model cannot be shown: a point of it is at no place.
inspection_mesh_infinite = the mesh has a position that is not finite

# Why a 3D model cannot be shown: its triangles' corners do not come in threes.
inspection_mesh_partial = the mesh's indices are not whole triangles

# Two things a preview says of how it shows a file, on one line above it: each is a whole sentence of its own.
inspection_notices = { $before } · { $after }

io_bad_name = the name is too long, or not allowed there

io_busy = it is in use

io_denied = permission denied

io_full = the drive is full

io_gone = it is not there any more

io_is_folder = it is a folder

io_not_empty = the folder is not empty

io_not_folder = a part of its path is not a folder

io_quota = the disk quota is used up

io_read_only = the drive is read-only

io_share_gone = the network share went away

io_taken = something of that name is already there

io_timed_out = the drive did not answer in time

io_too_large = the file is too large for that drive

# The button adding a shortcut to the list of keys the user binds.
keymap_add_shortcut = Add shortcut

# A column of the list of shortcuts: what the action is given.
keymap_args = Arguments

keymap_args_description = The command's arguments, as a JSON object

# A shortcut refused: its keys as written, then what the JSON reader said (English).
keymap_args_not_json = the arguments of { $keys } are not JSON: { $error }

# A shortcut refused: its keys as written, then an example of an object in JSON, as written.
keymap_args_object = the arguments of { $keys } are an object: { $example }

# A column of the list of shortcuts: the action the keys run.
keymap_command = Command

# mkdir, ui.jump and tab.new are actions' names: never translated.
keymap_command_description = What the keys run, by name as the palette's commands are registered: mkdir, ui.jump, tab.new. Empty frees the keys

# A column of the list of shortcuts: the keys pressed.
keymap_keys = Keys

# The examples are keys as Skwir writes them: never translated.
keymap_keys_description = One key with its modifiers, or several one after another: ctrl-shift-n, g h, shift-g

# A shortcut refused: its keys are empty.
keymap_needs_keys = a shortcut needs its keys

# A shortcut refused: what was written. ctrl-- is how it is written: never translated.
keymap_no_key = “{ $stroke }” names no key: write ctrl-- for Ctrl and the minus key

# A shortcut refused: both as written. ctrl, alt, shift and super are written so: never translated.
keymap_no_modifier = “{ $modifier }” in “{ $stroke }” is not a modifier: use ctrl, alt, shift or super

# A shortcut refused: its keys as written, then why its action refused it.
keymap_refused = { $keys }: { $error }

# A setting on the Keyboard page: the list of keys the user binds.
keymap_shortcuts = Your shortcuts

# Skwir is the program's name.
keymap_shortcuts_description = Keys you bind, over the ones Skwir and its plugins set.

# A setting's further description on the Keyboard page: a shortcut with no command frees its key.
keymap_shortcuts_description_more = Leave the command empty to free a key.

# A shortcut refused: its keys and the action's name, both as written.
keymap_unknown_command = { $keys } runs { $command }, which no command is called (is its plugin on?)

# A column of the list of shortcuts: where in the window the keys work.
keymap_where = Where

# pane, selection, single, dir, file and folder are words the field takes: never translated.
keymap_where_description = Where the keys work: empty for anywhere, pane for the file list, with conditions such as selection, single, dir, file, folder

# A step of a plan: write a file's notes alike to every place that keeps them. Begins in lower case.
notes_agree = make the notes of { $name } agree

notes_agree_command = Make notes agree

# Read by agents and in F1's list of actions.
notes_agree_description = Write the notes of the entries given, or the selection's, as read from every store, to each store that keeps them, so the stores that differ agree again (plan)

# A plan's title: the notes of the files are written alike to every place that keeps them. Begins in lower case.
notes_agree_title = make the notes agree

# A step of a plan, done. Begins in lower case.
notes_agreed = made the notes of { $name } agree

# A step of a plan found done already. Begins in lower case.
notes_agreed_already = the notes of { $name } already agreed

# Two changes of a file's tags, each a step's words (rename the tag “a” to “b” on “x.txt”), one after the other.
notes_and = { $first } and { $then }

# Undo of an edit of notes refused: another program changed them since. The file's name or path.
notes_changed_since = the notes of { $what } changed since, elsewhere

notes_comment_command = Comment

# Read by agents and in F1's list of actions.
notes_comment_description = Set the comment of the entries given, or the selection's, or remove it with an empty text (plan)

# A plan's title: the files' comment is set. Begins in lower case.
notes_comment_title = set the comment

# A step of a plan, done: a file was given a comment (the past of “set”, as notes_uncommented is of notes_uncomment). Begins in lower case.
notes_commented = set the comment of { $name }

notes_commented_already = { $name } already had that comment

notes_gone = { $name } is not there any more

notes_kept_nowhere = { $path }: its volume keeps no notes, and no store is set to keep them beside it

# A file's notes cannot be written: its path names no folder it is in.
notes_no_folder = { $path } has no folder

notes_no_stores = no notes stores to write with

# A step of a plan that undoes an edit of a file's notes. Begins in lower case.
notes_put_back = put back the notes of { $name }

notes_put_back_already = the notes of { $name } were already back

# The same step, done: the past of “put back”. Begins in lower case.
notes_put_back_done = put back the notes of { $name }

# What could not be done, the reason after it: put back the notes of /a/b.txt: permission denied.
notes_put_back_failed = put back the notes of { $path }

# A step of a plan: rate a file, in stars (4 stars). Begins in lower case.
notes_rate = rate { $name } { $stars }

notes_rate_command = Rate

# Read by agents and in F1's list of actions.
notes_rate_description = Rate the entries given, or the selection, from 1 to 5 stars, or remove their rating with 0 (plan)

# A plan's title: the rating of the files is removed. Begins in lower case.
notes_rate_none_title = remove the rating

# A plan's title: the files are rated, in stars (rate 4 stars). Begins in lower case.
notes_rate_title = rate { $stars }

notes_rated = rated { $name } { $stars }

notes_rated_already = { $name } was already rated { $stars }

# The most a rating is, counted in half stars.
notes_rating_range = a rating is from 1 to { $max }

notes_rating_stars = a rating is from 1 to 5 stars

# A file whose notes no store would keep: its path, then each store's reason, as a list (another file's there that it does not rewrite).
notes_refused = { $path }: { $why }

# A step of a plan: a tag's name, its new name, the file. Begins in lower case.
notes_rename_tag = rename the tag { $tag } to { $to } on { $name }

notes_renamed_tag = renamed the tag { $tag } to { $to } on { $name }

notes_renamed_tag_already = the tag { $tag } was already { $to } on { $name }

# A step of a plan, to do: give a file a comment (its words are not said). Begins in lower case.
notes_set_comment = set the comment of { $name }

# A rating in whole stars (4 stars).
notes_stars = { $whole ->
        [one] { $whole } star
       *[other] { $whole } stars
    }

# A rating in stars with a half star: the whole stars before it (3½ stars).
notes_stars_half = { $whole ->
        [0] ½ star
       *[other] { $whole }½ stars
    }

# A step of a plan: give a file tags (their names, as a list). Begins in lower case.
notes_tag = tag { $name } { $tags }

notes_tagged = tagged { $name } { $tags }

notes_tagged_already = { $name } was already tagged { $tags }

# A step of a plan: take a tag (its name) off a file. Begins in lower case.
notes_take_tag = take { $tag } off { $name }

# A step of a plan: take tags off a file. count: how many; tags: their names, as a list. Begins in lower case.
notes_take_tags = { $count ->
        [1] take the tag { $tags } off { $name }
       *[other] take the tags { $tags } off { $name }
    }

notes_took_tag = took { $tag } off { $name }

notes_took_tag_already = { $tag } was already off { $name }

# A step of a plan, done: tags taken off a file. count: how many; tags: their names, as a list. Begins in lower case.
notes_took_tags = { $count ->
        [1] took the tag { $tags } off { $name }
       *[other] took the tags { $tags } off { $name }
    }

# Tags found already off a file. count: how many; tags: their names, as a list. Begins in lower case.
notes_took_tags_already = { $count ->
        [1] the tag { $tags } was already off { $name }
       *[other] the tags { $tags } were already off { $name }
    }

# A step of a plan. Begins in lower case.
notes_uncomment = remove the comment of { $name }

# A plan's title: the files' comment is removed. Begins in lower case.
notes_uncomment_title = remove the comment

notes_uncommented = removed the comment of { $name }

notes_uncommented_already = { $name } already had no comment

# A step of a plan. Begins in lower case.
notes_unrate = remove the rating of { $name }

notes_unrated = removed the rating of { $name }

notes_unrated_already = { $name } already had no rating

# A step of a plan. Begins in lower case.
notes_untag = remove the tags of { $name }

notes_untagged = removed the tags of { $name }

notes_untagged_already = { $name } already had no tags

# How a place that keeps notes is written: for every file edited.
notes_write_create = Create

# What could not be done, the reason after it: write the notes of /a/b.txt: permission denied.
notes_write_failed = write the notes of { $path }

# How a place that keeps notes is written: also where a file's volume keeps no attributes.
notes_write_fallback = Where attributes are missing

# How a place that keeps notes is written: never.
notes_write_off = Off

# How a place that keeps notes is written: only where a file already has notes there.
notes_write_update = Update existing

# Said after why a move into the trash on another drive stopped: part of the original went, and the trash holds all of it.
op_all_in_trash = all of it is in the trash

# Why a step of an action does not go on: a file of that name is where it goes.
op_already_there = { $name } is already there

# Why a copy failed: an extended attribute of the file has a name that is not text.
op_attribute_name_not_utf8 = an attribute of { $name } has a non-UTF-8 name, which the copy's attribute interface cannot preserve

# Why a copy failed: the drive it goes to keeps no tags or attributes, and no companion file is set to keep them. folder: the path copied into; setting: the setting's title. Begins in lower case.
op_attributes_not_kept = keep tags and other attributes while copying: { $folder } keeps no attributes, and nothing is set to keep them beside it; turn off “{ $setting }” in Settings to copy without them

# What a copy could not do, its why after it: write a file's tags and other extended attributes where it goes. setting: the setting's title. Begins in lower case.
op_attributes_refused = keep tags and other attributes while copying: check that the destination keeps extended attributes and lets you write them, or turn off “{ $setting }” in Settings

# What a step could not do: read a file's extended attributes (its tags among them); why follows.
op_attributes_unread = the tags and attributes of { $name } could not be read

# Why a batch of copies stopped: it was cancelled.
op_batch_cancelled = cancelled

# What a batch of copies was doing when it failed, terse; why follows. transfer: the program's name (rsync, skwir).
op_batch_copying = { $transfer } copying { $count ->
        [one] { $count } entry
       *[other] { $count } entries
    }

# Why a batch of copies failed on Windows: it met a link, which it does not copy. path: the link's path.
op_batch_link = “{ $path }” is a link

# Why a batch of copies failed: a source ended early (a server stopped serving it). path: its path; read, bytes: how many bytes were read, how many it holds.
op_batch_read_short = { $read ->
        [one] “{ $path }”: only { $read } byte of { $bytes } could be read
       *[other] “{ $path }”: only { $read } bytes of { $bytes } could be read
    }

# A line of a plan's review, terse as a listing: a file copied, its path then its copy's. Begins in lower case.
op_change_copy = copy { $from } -> { $to }

# A line of a plan's review, terse as a listing: an empty file made. Begins in lower case.
op_change_create = create { $path }

# A line of a plan's review, terse as a listing: a file deleted for good. Begins in lower case.
op_change_delete = delete { $path }

# A line of a plan's review, terse as a listing: a link made, where it is then where it leads, as ls -l shows it. Begins in lower case.
op_change_link = link { $link } -> { $target }

# A line of a plan's review, terse as a listing: a field of a file written; field is its key (position, tags). Begins in lower case.
op_change_metadata = set { $field } on { $path }

# A line of a plan's review, terse as a listing: a folder made, as the command that makes one is named. Begins in lower case.
op_change_mkdir = mkdir { $path }

# A line of a plan's review, terse as a listing: a file moved, its path then where it goes. Begins in lower case.
op_change_move = move { $from } -> { $to }

# A line of a plan's review, terse as a listing: an empty folder, or a file made empty, removed. Begins in lower case.
op_change_remove = remove { $path }

# A line of a plan's review, terse as a listing: a file renamed, its path then its new path. Begins in lower case.
op_change_rename = rename { $from } -> { $to }

# A line of a plan's review, terse as a listing: a file put back from the trash. Begins in lower case.
op_change_restore = restore { $path }

# A line of a plan's review, terse as a listing: two files' names swapped. Begins in lower case.
op_change_swap = swap { $a } <-> { $b }

# A line of a plan's review, terse as a listing: a file put in the trash. Begins in lower case.
op_change_trash = trash { $path }

# Why a move across drives ended with its original still there: it was changed while it was copied. folder: the path of the folder it was moved to.
op_changed_after_copy = { $name } changed after it was copied to “{ $folder }”: its original stays where it was

# Why a copy does not go on: the folder it goes to is inside the one copied.
op_copied_into_itself = a folder cannot be copied into itself

# Why a copy does not go on: where it goes is the file copied.
op_copied_onto_itself = it would be copied onto itself

# Why reading a copy's source stopped: the copy ended (cancelled, or failed elsewhere).
op_copy_stopped = the copy stopped

# An action's title: the file copied (or how many), then the folder it goes to, by its name. Begins in lower case: said inside other words too.
op_copy_title = { $count ->
        [1] copy { $name } to { $folder }
       *[other] copy { $count } items to { $folder }
    }

# Why a copy failed: what it made was gone as soon as it was made.
op_copy_went_missing = the copy { $name } went missing as it was made

# Why a copy failed: a device file is a door to hardware, not a file.
op_device_not_copied = { $name } is a device, which cannot be copied

# Why a step does not go on: the file it acts on is gone.
op_entry_gone = { $name } is not there any more

# Why a step does not go on: another file took the name of the one it acts on.
op_entry_replaced = { $name } is now another file

# What the undo of a folder's removal could not do: give it back an extended attribute it had. attribute: the attribute's name (user.xdg.tags); why follows.
op_folder_attribute_not_restored = the folder's attribute “{ $attribute }” could not be restored

# Why the undo of a folder's removal does not go on: a folder of its name is there again, with other permissions or attributes.
op_folder_attributes_differ = { $name } is there with different folder attributes

# What a step could not do; why follows.
op_folder_not_made = the folder { $name } could not be made

# What a step could not do: remove an empty folder; why follows.
op_folder_not_removed = the folder { $name } could not be removed

# What a step could not do: make a folder, by its path; why follows.
op_folder_path_not_made = the folder “{ $path }” could not be made

# Why a step does not remove a folder: it is another user's, and Undo could not put back what it holds.
op_foreign_folder = this folder belongs to another user and its permissions would prevent Undo from restoring its contents

# Why a folder is not deleted or put in the trash: a drive or a server's files are shown in a folder inside it. point: that folder's path from it.
op_holds_mount_point = { $name } holds “{ $point }”, where a drive or a remote is mounted: eject it first

# Why a copy failed: a file some parts of which were never written (a sparse file) is copied as such only on Unix. system: the system's name (Windows).
op_holes_not_yet = Copying a file with holes (a sparse file) is not available on { $system } yet

# Why a step does not go on: what tells a file apart from others (its device and inode) could not be read.
op_identity_unread = the entry's identity could not be read

# Why a file is not put in the trash: it is in the trash.
op_in_trash_already = { $name } is in the trash already

# Why a step does not act on a file: it is in the trash.
op_in_trash_restore_first = { $name } is in the trash: restore it first

# What a folder removed in part left: the first file still there (its path from the folder above) and how many more besides it.
op_left_after_removal = { $more ->
        [0] “{ $path }” is left
       *[other] “{ $path }” and { $more } more are left
    }

# What a rename that changes only the case of a name left behind when it failed halfway: the file under a name of its own making; why follows.
op_left_parked = { $name } was left named { $parked }

# Why the undo of a link does not remove it: it was changed to lead elsewhere since.
op_link_elsewhere = { $name } leads elsewhere now

# An action's title: the file a link is made to (or how many), then the folder the links are made in, by its name. Begins in lower case: said inside other words too.
op_link_title = { $count ->
        [1] link { $name } in { $folder }
       *[other] link { $count } items in { $folder }
    }

# Why Undo does not remove what an action made (where no trash is kept): a file that goes with it (its tags, its sidecar) is there and was not made by the action.
op_made_beside_other = { $companion } is beside { $name } and was not made with it: Undo leaves them

# Why Undo does not remove what an action made (where no trash is kept): it was changed since.
op_made_changed = { $name } changed since it was made: Undo leaves it

# Why Undo does not remove what an action made (where no trash is kept): a file made with it (its tags, its sidecar) was changed since.
op_made_companion_changed = { $companion } changed since it was made: Undo leaves it and { $name }

# Why a folder is not deleted or put in the trash: a drive or a server's files are shown there.
op_mount_point = { $name } is where a drive or a remote is mounted: eject it rather than delete it

# Why Undo does not take back a move across drives whose original could not be removed: the original was changed since.
op_move_original_changed = { $name } stayed where it was, changed since it was moved: Undo leaves it and its copy

# An action's title, in its review, the list of actions and Undo: the file moved (or how many), then the folder it goes to, by its name. Begins in lower case: said inside other words too.
op_move_title = { $count ->
        [1] move { $name } to { $folder }
       *[other] move { $count } items to { $folder }
    }

# Why a move does not go on: the folder it goes to is inside the one moved.
op_moved_into_itself = a folder cannot be moved into itself

# Said after why a move to a removable drive could not be written out: the originals were not removed.
op_moved_stays = what was moved stays where it was

# Why a step that moves or stages files in a folder failed: the device it is on (a camera) does not move files between its folders.
op_moves_nothing = this device (a camera) cannot move files between its folders: only copies can be made there

# Why a name typed for a file is refused on Windows: it holds a character Windows keeps for itself (\ : * ? " < > |).
op_name_character = a name cannot contain “{ $character }”

# Why a name typed for a file is refused on Windows: it holds an invisible control character.
op_name_control = a name cannot contain a control character

# Why a name typed for a file is refused on Windows: CON, NUL, COM1… name devices there, with an extension or without.
op_name_device = { $name } is a device's name on Windows

# Why a name typed for a file is refused: it is . or .., which name folders themselves.
op_name_dots = { $name } cannot be used as a name

# Why a name typed for a file is refused. Begins in lower case: said after other words too.
op_name_empty = a name cannot be empty

# Why a name typed for a file is refused on Windows.
op_name_end = a name cannot end with a dot or a space

# Why a name typed for a file is refused: it holds the character of code 0.
op_name_nul = a name cannot contain a NUL character

# Why a name typed for a file is refused: / separates folders.
op_name_slash = a name cannot contain “/”

# Why a step of an action does not go on: the folder it makes or puts something in is gone.
op_no_folder_to_go_in = the folder it goes in is not there

# Why a copy failed: what was listed as a file is something else now (a pipe, a folder).
op_no_longer_a_file = { $name } is no longer a file

# Why the undo of a link does not go on: something else has its name now.
op_no_longer_a_link = { $name } is no longer a link

# Why the undo of a new file does not remove it: something was written in it since.
op_no_longer_empty = { $name } is no longer empty

# Why a file of a trashed folder is not restored: it left the trash.
op_no_longer_in_trash = { $name } is no longer in the trash

# Why a step of an action does not go on: the file it acts on is gone.
op_no_longer_there = { $name } is no longer there

# Why a step on the trash does not go on: the file named as what the trash keeps of an item (its .trashinfo) is not one.
op_no_record = { $name } is not a trash's record of an item

# Why an action does not go on: the drive it writes to has too little free. folder: the folder written into, by its name; needs, free: sizes.
op_no_room = Not enough room in { $folder }: { $needs } needed, { $free } free

# Why the undo of a folder restored into one there does not go on: the trash's record of it names no item of a trash.
op_no_trash_s = { $name } names no item of a trash

# Why a file is not put in the trash: its folder is a server's, mounted by a program (sshfs, rclone, GVfs), where no trash is kept.
op_no_trash_there = { $name } is in a network folder a program mounts, which keeps no trash

# Why the undo of a new file does not go on: something else has its name now.
op_not_a_file = { $name } is not a file

# Why a step of an action does not go on: what it goes into, or acts on as a folder, is a file.
op_not_a_folder = { $name } is not a folder

# What a step deleting for good could not do: remove a folder with all it holds; why follows.
op_not_all_deleted = { $name } could not all be deleted

# What a step could not do: remove a folder with all it holds; why follows.
op_not_all_removed = { $name } could not all be removed

# What a step could not do; why follows.
op_not_copied = { $name } could not be copied

# Why a folder is not removed: it holds something (a step removes only an empty one).
op_not_empty = { $name } is not empty

# Why a step on a file of the trash does not go on: it is not in the trash (any more).
op_not_in_trash = { $name } is not in the trash

# What a step could not do: make a link to a file in a folder, by their names; why follows.
op_not_linked = { $name } could not be linked in { $folder }

# What a step could not do: make a new empty file; why follows.
op_not_made = { $name } could not be made

# What a step could not do: a file and the folder it was to go to, by their names; why follows.
op_not_moved = { $name } could not be moved to { $folder }

# What a step could not do: a file, the folder it was to go to and the name it was to take there; why follows.
op_not_moved_as = { $name } could not be moved to { $folder } as { $new }

# What a step could not do: put a copy made in its place; why follows.
op_not_placed = { $name } could not be placed in { $folder }

# What a step could not do; why follows.
op_not_removed = { $name } could not be removed

# What a step could not do: a file's name and the new name it was to take; why follows.
op_not_renamed = { $name } could not be renamed { $new }

# What a step putting a file back from the trash could not do; why follows.
op_not_restored = { $name } could not be restored

# Why a plugin's transfer (rsync) did not copy: it cannot between these folders, and Skwir copies them itself.
op_not_served = the transfer does not serve these folders

# What a step could not do: exchange two files' names; why follows.
op_not_swapped = { $a } and { $b } could not swap names

# What a step could not do; why follows.
op_not_trashed = { $name } could not be moved to the trash

# What a copy to a removable drive could not do: hand the drive what it wrote; why follows.
op_not_written_out = the copy could not be written out to the drive

# Why a move across drives ended with its original still there: the copy is whole. folder: the path of the folder it was moved to.
op_original_stays = { $name } was copied whole to “{ $folder }”, but its original could not all be removed

# Why a move across drives ended with its original still there: the copy is whole. path, folder: the original's path and the folder it was moved to.
op_original_stays_path = “{ $path }” was copied whole to “{ $folder }”, but its original could not all be removed

# Why a step of an action stopped: it failed in a way Skwir did not foresee. Begins in lower case: said after the step's own words.
op_panicked = it failed unexpectedly

# Said after why a copy failed: what it had made could not all be removed. path: where it is; why: why it stayed.
op_part_left = a part of the copy is left in “{ $path }” ({ $why })

# Said after why the undo of a folder's removal failed: the folder it began to make could not be removed. path: where it is.
op_partly_restored_folder = the partially restored folder remains at “{ $path }”

# Why a new folder or file is not made: its path climbs out of the folder it is made in.
op_path_climbs = “{ $path }” contains “..”

# Why an entry given to a command (by an agent or a script) is refused: nothing is at its path. path: the path given.
op_path_missing = { $path } does not exist

# Why a new folder or file is not made: its path ends without a name.
op_path_no_name = “{ $path }” has no name

# What a copy of a named pipe could not do: make a new pipe in its place; why follows.
op_pipe_not_made = the pipe { $name } could not be made

# Why a file restored from a trashed folder is not put back in it: another file took its place there.
op_place_taken_in_trash = another { $name } has taken its place in the trash

# Why a plan given as data is refused: one of its steps is of a kind that holds several. kind: that kind's identifier.
op_plan_steps_kind = a plan lists each of its steps: `{ $kind }` is none

# Why a copy failed: its source ended early (a server stopped serving it). read: how many bytes were read; bytes: how many it holds. Begins in lower case.
op_read_short = { $read ->
        [one] only { $read } byte of { $bytes } could be read
       *[other] only { $read } bytes of { $bytes } could be read
    }

# What a step on the trash could not do: read what the trash keeps of an item (its .trashinfo); why follows.
op_record_unread = the trash's record { $name } could not be read

# What a step on the trash could not do: write what the trash keeps of an item (its .trashinfo); why follows.
op_record_unwritten = the trash's record { $name } could not be written

# What a redo is called while it waits or runs: redo, then the action it applies again (redo copy 3 items to Docs). Begins in lower case: said inside other words too.
op_redo_title = redo { $title }

# Why a copy failed: the drive or the server it was written to refused it whole once written.
op_refused_on_close = its drive or server refused it as it was closed

# Why a batch of renames is refused: a new name is taken by a file the batch does not rename.
op_rename_taken = { $name } is already there

# Why a batch of renames is refused: two of them or more give the same name. path: that name's path.
op_renames_duplicate = several items would be named { $path }

# Why a copy failed: a socket is a door to a running program, not a file.
op_socket_not_copied = { $name } is a socket, which cannot be copied

# Why a step does not go on: on a drive that ignores case, two names of one file could not be told apart.
op_spellings_undistinguished = the spellings of this file's hard links could not be distinguished

# Why a copy failed: reading its source stopped before its end.
op_stopped_being_read = the file stopped being read

# Why a step that exchanges two files' names does not go on, on Windows.
op_swap_windows = Windows cannot swap two names in one step

# Why a file is not put back from the trash: a file of its name is where it came from now.
op_there_again = { $name } is there again: rename or move it first

# Why a step of an action does not go on: a step before it in the same action puts a file of that name where it goes.
op_there_by_earlier_step = { $name } is already there, by an earlier step

# Why a folder is not made, or put back: a file has its name.
op_there_not_a_folder = { $name } is there, and is not a folder

# What a batch of copies could not do: give a copy its original's modification time; why follows. path: the copy's path.
op_time_not_given = “{ $path }” could not be given its time

# Said after why a move into the trash on another drive failed: its copy in the trash could not be removed. path: where it is; why: why it stayed.
op_trash_copy_left = a copy of it is left in “{ $path }” ({ $why })

# What the undo of a move into the trash on another drive could not do: remove the copy the trash held; why follows.
op_trash_copy_not_removed = the copy of { $name } in the trash could not all be removed

# Why a file restored from a trashed folder is not put back in it: that folder left the trash.
op_trash_folder_gone = the folder of the trash { $name } was in is gone

# Why a step on the trash does not go on. error: why, as the system's trash said it.
op_trash_unread = the trash could not be read: { $error }

# What a step on the trash could not do; why follows.
op_trash_unread_context = the trash could not be read

# What an undo is called while it waits or runs, and in the History: undo, then the action it takes back (undo copy 3 items to Docs). Begins in lower case: said inside other words too.
op_undo_title = undo { $title }

# Why a plan given as data is refused: no step of that kind is known (its plugin is off, or it is misspelt). kind: the identifier given.
op_unknown_kind = unknown operation kind `{ $kind }`

# Why a step does not go on: what it acts on could not be read. error: why.
op_unreadable = { $name } could not be read: { $error }

# Why a restore failed: the file put back was gone as soon as it was.
op_went_missing_restored = { $name } went missing as it was restored

# Why a file of a trashed folder is not restored: where it would go is in the trash.
op_would_stay_in_trash = { $name } would stay in the trash

# Why a copy to a removable drive failed: handing the drive what was written stopped unexpectedly.
op_write_out_stopped = the write-out stopped

# What a setting holds, read in the settings file and by agents: the fields of each record, as written.
orders_kept = The orders places remember (ADR-132), each { $fields }: shown so in every pane

# A folder's order was chosen to be remembered, but the settings could not be written: why follows.
orders_not_saved = The order was not remembered: { $error }

# Said to an agent or a script that gave a path relative to its own folder: the path as given.
os_path_not_absolute = { $path } is not an absolute path

# Told to an agent that named a private folder; Settings and Privacy name the page where folders are made private.
permissions_names_private = this names a private folder, which Skwir keeps from agents (Settings, Privacy)

# What a plugin may do, on the Plugins page after “May:”. Begins in lower case.
permissions_network = uses the network

# What a plugin may do, on the Plugins page after “May:”. Begins in lower case.
permissions_programs = starts programs

# Told to an agent that named a path on a server: the path.
permissions_reaches_machine = looking up { $path } would make the system reach another machine, which an agent may not do unasked: the user opens it in Skwir first

# What a plugin may do, on the Plugins page after “May:”. Begins in lower case.
permissions_read = reads what files hold

# What a plugin may do, on the Plugins page after “May:”. Begins in lower case.
permissions_write = changes files

playing_not_answering = the player is not answering

playing_nothing = nothing is playing

playing_now = What is playing

# Read by agents: null is the JSON value.
playing_now_description = The track that plays, its state and where it stands, or null

playing_request = Steer the player

playing_request_description = Ask whatever is playing to play, pause, stop, step to the next or previous track, seek, change its volume or its speed, choose a film's audio or subtitles or show it on the whole screen; it is carried out when the player next wakes

# Said to an agent or a script: what it asked of the player contradicts itself.
playing_senseless = this request makes no sense

# A heading above plugins in the settings.
plugins_browsing = Browsing and finding

plugins_desktop = Desktop integration

plugins_files = Working with files

plugins_list = List plugins

plugins_list_description = Every plugin with its title, description and enabled state

# A plugin was turned on or off, but the settings could not be written: why follows.
plugins_not_saved = The plugin choice was not saved: { $error }

plugins_previews = Previews and thumbnails

plugins_set_enabled = Enable or disable a plugin

# Read by agents and in F1's list of actions; `plugins.list` is an action's name.
plugins_set_enabled_description = Toggle a plugin at runtime and remember the choice; with the window open, completion is signalled by a wake and `plugins.list`

# Said to an agent or a script: no plugin has this id (as written).
plugins_unknown = unknown plugin: { $id }

# A file previewed was changed by another program as it was read: its path.
preview_changed = { $path } changed while it was shown

# Pages to print typed with no page in them.
print_no_page = no page named

# Pages to print typed with something not a page number: what was typed.
print_not_a_page = “{ $text }” is not a page

# Pages to print typed as a range whose end is before its start: the range as typed (5-3).
print_pages_backwards = { $range } goes backwards

# Pages to print typed with a page 0.
print_pages_from_one = pages are counted from 1

# A paper's name: US Legal, 8.5 by 14 inches.
print_paper_legal = Legal

# A paper's name: US Letter, 8.5 by 11 inches.
print_paper_letter = Letter

# A group's heading in Properties: what the file says of itself (a picture's camera, a document's author).
properties_content = Content

# A group's heading in Properties: the file's size and what it takes.
properties_general = General

# A group's heading in Properties: the file's path and folder.
properties_location = Location

# A group's heading in Properties: what plugins or the user attached to the file beside the rest.
properties_more = More

# A group's heading in Properties: the file's tags, rating and comment.
properties_notes = Notes

# A group's heading in Properties: who owns the file and who may use it.
properties_permissions = Permissions

# A group's heading in Properties, with several items selected: how many, what they weigh.
properties_selection = Selection

# A group's heading in Properties, with several items selected: the owner, group and access they share.
properties_shared_permissions = Shared permissions

# A group's heading in Properties: where a download came from.
properties_source = Source

# A group's heading in Properties: when the file changed, was made, was read.
properties_times = Times

# A group's heading in Properties: the drive the file is on.
properties_volume = Volume

# What a sign of a regular expression matches: . (a dot).
reference_any = Any character

# What a sign of a regular expression matches: *, after something.
reference_any_times = What is before it, any number of times, or none

# What a sign of a regular expression matches: \b.
reference_boundary = Where a word starts or ends

# What (?i) does in a regular expression: case is ignored after it.
reference_case = From here on, capitals and small letters alike

# What a sign of a regular expression matches: \d.
reference_digit = A digit

# What a sign of a regular expression matches: a|b, what is on either side.
reference_either = Either

# What a sign of a regular expression matches: $, the end of what is matched.
reference_end = The end

# A backslash before a sign of a regular expression matches the sign itself. The signs are as written.
reference_escaped = A sign itself: \. \( \[ \* \+ \? \$ \^ \|

# What brackets do in a regular expression. (ab)+ is an example, as written.
reference_group = A group, which a sign after it takes whole: (ab)+

# What a sign of a regular expression does: +? (and *?) match as little as they can.
reference_lazy = As few times as it can (*? too)

# What a sign of a regular expression matches: ?, after something.
reference_maybe = What is before it, or nothing

# What (?<year>…) does in a regular expression: year is the example's name, as written in the sign.
reference_named_group = A group with a name: year

# What a sign of a regular expression matches: [^abc], anything but the letters in brackets.
reference_none_of = One of anything but these

# What a sign of a regular expression matches: +, after something.
reference_once_or_more = What is before it, once or more

# What a sign of a regular expression matches: [abc], one of the letters in brackets.
reference_one_of = One of these

# A heading of the signs a regular expression reads, in the list under a field (a rename's pattern, a search's content).
reference_pattern = Pattern

# Under the signs of a regular expression: what it does not read. The signs are as written.
reference_pattern_note = No look-around and no back-references: (?=…) and \1 are not read.

# What a sign of a regular expression matches: [a-z], one letter from a to z.
reference_range = One in this range

# What a sign of a regular expression matches: \s.
reference_space = Any space: a space, a tab, a line's end, any other

# What a sign of a regular expression matches: ^, the start of what is matched (a name, a line).
reference_start = The start

# What a sign of a regular expression matches: {2,4}, after something. The numbers are the sign's.
reference_times = What is before it, 2 to 4 times

# What a sign of a regular expression matches: \w. _ is the underscore.
reference_word_char = A letter, a digit or _

# A submenu of a file's menu: the ways to compress it (archives' formats, a smaller copy).
registry_compress = Compress

# A heading of the sidebar: folders the user keeps there.
registry_group_bookmarks = Bookmarks

# A heading of the sidebar: drives, phones and other volumes.
registry_group_devices = Devices

# A heading of the sidebar: servers and shares.
registry_group_network = Network

# A heading of the sidebar: the home folder, the desktop, the trash and other places.
registry_group_places = Places

# A heading of the sidebar: saved searches and places kept with their filters.
registry_group_saved = Saved

# A heading of the sidebar: the window's open tabs.
registry_group_tabs = Tabs

# A heading of the sidebar: the tags given to files.
registry_group_tags = Tags

# A submenu of a folder's background menu: the applications that open the folder.
registry_open_folder_with = Open Folder With

# A submenu of a file's menu: the applications that open it.
registry_open_with = Open With

# A submenu of a file's menu: its tags, each a check, and the tags editor.
registry_tags = Tags

# A settings file's key that Skwir keeps in a file of its own, which holds it already.
settings_apart_own = { $file ->
        [state] the window's state, not a setting: Skwir reads its own, in its state file, never this one
       *[data] what you make in Skwir (saved searches, pinned tags, tags' colours), not a setting: Skwir reads its own, in its data file, never this one
    }

# A settings file's key that Skwir keeps in a file of its own, which does not hold it yet.
settings_apart_until = { $file ->
        [state] the window's state, not a setting: read from here only until Skwir keeps its own, in its state file
       *[data] what you make in Skwir (saved searches, pinned tags, tags' colours), not a setting: read from here only until Skwir keeps its own, in its data file
    }

settings_apply = Save settings

# Read by agents and in F1's list of actions.
settings_apply_description = Change preferences, each key with the text its row in Settings takes; all of them are saved, or none

settings_check = Check settings

# Read by agents and in F1's list of actions.
settings_check_description = Check a settings file, or settings given as a table, against the settings this build of Skwir declares: fails with what is wrong (a value a setting does not take, a key no setting has), and notes the keys of owners it does not know

# A settings file to check that is no TOML: its path, then what is wrong and where.
settings_check_invalid = “{ $path }” is not valid: { $problem }

# Settings given as a table that a settings file cannot hold; the writer's own words follow.
settings_check_not_settings = not settings: { $error }

# Said to an agent that gave both a file and settings, or neither.
settings_check_one = give a file or settings, one of them

# A settings file to check: its path, then why it cannot be read.
settings_check_unreadable = “{ $path }” cannot be read: { $why }

# A setting's default choice and what it comes to here and now: Automatic (Dark).
settings_choice_now = { $choice } ({ $now })

# The values a setting takes, as a file writes them.
settings_choose_one = choose one of { $choices }

settings_close_split = Pane that closes

# Which pane closes: the one you are in.
settings_close_split_active = Active

# Split is a command's name; Other, Active and Right are the setting's choices.
settings_close_split_description = Which pane goes when you close the split with Split or F3.

# Split is a command's name; Other, Active and Right are the setting's choices.
settings_close_split_description_more = Other keeps the pane you are in. Active closes it and keeps the other. Right always closes the right pane, or the lower one when the panes are stacked.

# Which pane closes: the other one.
settings_close_split_other = Other

# Which pane closes: the right one.
settings_close_split_right = Right

settings_drop_asks = Ask what dropped files do

# Shift, Ctrl and Ctrl+Shift are keys, as the keyboard names them.
settings_drop_asks_description = Files dropped in a folder with no key held open a menu: move, copy or link them there.

# Shift, Ctrl and Ctrl+Shift are keys, as the keyboard names them.
settings_drop_asks_description_more = Turn this off to move them within a drive and copy them to another. Shift moves, Ctrl copies and Ctrl+Shift links, either way.

# As a settings file writes a switch's value: true and false are its words.
settings_expected_boolean = expected true or false

settings_expected_finite = expected a finite number

# A value given as JSON could not be read; the reader's own words follow.
settings_expected_json = expected JSON: { $error }

# A settings file's value of the wrong kind: what the setting takes, then what the file holds.
settings_expected_kind = expected { $want ->
        [integer] a whole number
        [float] a number
        [boolean] true or false
        [datetime] a date
        [array] a list
        [table] a table
       *[string] text
    }, not { $got ->
        [integer] a whole number
        [float] a number
        [boolean] true or false
        [datetime] a date
        [array] a list
        [table] a table
       *[string] text
    }

settings_expected_number = expected a number

# A list of records given as JSON could not be read; the reader's own words follow.
settings_expected_records = expected a list of records: { $error }

settings_expected_whole_number = expected a whole number

settings_field_name_value_pairs = { $title } is a list of name and value pairs

# A record's field left empty: its title.
settings_field_needed = { $title } is needed

settings_field_pairs = { $title } is a list of pairs

settings_field_pairs_text = { $title }: names and values are text

# A record's field that takes a whole number: its title, the least and the most it takes.
settings_field_range = { $title }: enter a whole number from { $min } to { $max }

settings_field_secret = { $title } is kept in the keyring, not in the settings

settings_field_text = { $title } is text

settings_field_value_needs_name = { $title }: a value needs a name

settings_field_whole_number = { $title } is a whole number

# A file of the settings that is no TOML (a hand edit under way): its path, and what is wrong where.
settings_file_invalid = “{ $path }” is not valid ({ $problem }): nothing is saved over it until it is fixed

settings_file_not_text = “{ $path }” is not text

# A file of the settings: its path, then why it could not be read.
settings_file_unreadable = “{ $path }” could not be read: { $why }

settings_folders_first = Folders first

settings_folders_first_description = List folders before files, each sorted among themselves.

# What a setting's description says after its first sentence, in its tooltip.
settings_folders_first_description_more = Off sorts them together: by date modified, the newest comes first, folder or file. By size, folders, which are not weighed, sort as having none.

# A setting's key with an empty part (a.b..c).
settings_invalid_key = invalid setting key

# A setting on the Tags and notes page; a copy that cannot keep them names it too.
settings_keep_attributes = Keep tags and attributes when copying

settings_keep_attributes_description = Copies carry their source's tags, rating, comment and where it was downloaded from.

# What a setting's description says after its first sentence, in its tooltip.
settings_keep_attributes_description_more = Where the destination keeps no attributes they are kept beside the copy, as a Mac does; the copy fails only when nothing can keep them. Turn this off to copy without them.

# A setting's key, then the JSON reader's own words.
settings_key_not_json = { $key }: expected JSON: { $error }

settings_language = Language

# The language chosen by the system's languages.
settings_language_automatic = Automatic

# Automatic is a choice of the setting (settings_language_automatic).
settings_language_description = The language Skwir's window speaks.

# Automatic is a choice of the setting (settings_language_automatic).
settings_language_description_more = Automatic speaks the first of your system's languages Skwir has words in, else English; agents and scripts are answered in English whichever you choose.

# The pseudo-language, English accented and longer, offered in builds for testing.
settings_language_pseudo = Pseudo (for testing)

# This language's name in itself, as the Language setting lists it in every language: Français in the French catalog, Deutsch in the German one.
settings_language_self = English

# What a key of the state file holds, for whoever reads it.
settings_last_location = The folder shown when the window was last closed

# The key is a setting's, as a file writes it.
settings_managed = { $key } is managed by its owning commands

# A settings file names a plugin Skwir does not have: its id.
settings_no_plugin = no plugin “{ $id }” here: one from outside, or not built in, may take it

settings_no_such = no such setting

# A JSON value a settings file cannot hold; the writer's own words follow.
settings_not_file_value = not a value a file holds: { $error }

# Why a settings file could not be read: its bytes are no text.
settings_not_text = it is not text

# A settings file's choice Skwir does not know, which may be one found where it runs: which those are (themes you added to bat).
settings_other_choice = not one Skwir knows here: it may be among { $which }

# A settings file's key under an owner nothing here declares: the owner's id.
settings_owner_unknown = nothing here declares “{ $owner }”: a plugin turned off, or from outside, may

# A setting's key whose part before it holds a value, not a table of settings.
settings_parent_not_table = { $key }: parent is not a table

# A plugin's entry in a settings file; enabled, true and false are as the file writes them.
settings_plugin_table = expected a table: enabled = true or false

# Two problems told at startup, each a sentence or more: the settings', then the saved searches'.
settings_problems = { $first } { $then }

# Refused where a setting names a program to run: the examples are commands, as typed.
settings_program_alone = Give the program alone, without arguments: “bash”, not “bash -l”

# A setting that takes a whole number: the least and the most it takes.
settings_range = enter a whole number from { $min } to { $max }

settings_read_only = the { $file ->
        [state] state file
        [data] data file
       *[settings] settings file
    } is read-only, so the change is not kept

# Home Manager is a program that declares a user's configuration.
settings_read_only_link = the settings file is a link to one that cannot be written (Home Manager's, say), so the change is not kept: change it where it is declared

settings_read_only_link_apart = the { $file ->
        [state] state file
       *[data] data file
    } is a link to one that cannot be written, so the change is not kept

settings_record_needs_id = every record needs an id

# The id is an identifier, as written.
settings_records_share_id = two records share the id { $id }

# A settings file's key that has a new name: the new key.
settings_renamed = now “{ $new }”: Skwir moves it there

# A settings file's key that has a new name, set too: the new key.
settings_renamed_both = not read: “{ $new }” is set, which Skwir reads

settings_save_did_not_run = settings save did not run

settings_saving_already = settings are already being saved

# A heading of a section of the settings.
settings_section_actions = Actions

# A heading of a section of the settings.
settings_section_address = Address

# A heading of a section of the settings.
settings_section_audio = Audio

# A heading of a section of the settings.
settings_section_books = Books

# A heading of a section of the settings.
settings_section_code = Code

# A heading of a section of the settings.
settings_section_commands = Commands

# A heading of a section of the settings.
settings_section_documents = Documents

# A heading of a section of the settings.
settings_section_drag_and_drop = Drag and drop

# A heading of a section of the settings.
settings_section_git = Git

# A heading of a section of the settings.
settings_section_grammars = Grammars

# A heading of a section of the settings.
settings_section_grid = Grid

# A heading of a section of the settings.
settings_section_language = Language

# A heading of a section of the settings.
settings_section_list_columns = List columns

# A heading of a section of the settings.
settings_section_maps = Maps

# A heading of a section of the settings.
settings_section_models_3d = 3D models

# A heading of a section of the settings.
settings_section_opening_addresses = Opening addresses

# A heading of a section of the settings.
settings_section_pages = Pages

# A heading of a section of the settings.
settings_section_pictures = Pictures

# A heading of a section of the settings.
settings_section_playback = Playback

# A heading of a section of the settings.
settings_section_previews = Previews

# A heading of a section of the settings.
settings_section_servers = Servers

# A heading of a section of the settings.
settings_section_sizes = Sizes

# A heading of a section of the settings.
settings_section_sorting = Sorting

# A heading of a section of the settings.
settings_section_split = Split

# A heading of a section of the settings.
settings_section_spreadsheets = Spreadsheets

# A heading of a section of the settings.
settings_section_startup = Startup

# A heading of a section of the settings.
settings_section_tabs = Tabs

# A heading of a section of the settings.
settings_section_tags = Tags

# A heading of a section of the settings.
settings_section_terminal = Terminal

# A heading of a section of the settings.
settings_section_text = Text

# A heading of a section of the settings.
settings_section_thumbnails = Thumbnails

# A heading of a section of the settings.
settings_section_video = Video

settings_show_companions = Show companion files

# ._ is how a Mac's companion files begin their names; Properties is a dialog.
settings_show_companions_description = With hidden files shown, also list the files that keep another file's notes beside it: a Mac's ._ files, XMP and TagSpaces sidecars.

# ._ is how a Mac's companion files begin their names; Properties is a dialog.
settings_show_companions_description_more = Properties lists a file's companions either way.

# How sizes are counted: the setting's title.
settings_size_units = Units

settings_size_units_binary = Binary

settings_size_units_decimal = Decimal

# kB, MB, KiB… are units' symbols; 10M, 10MB and 10MiB are as a search is typed, never translated.
settings_size_units_description = How sizes are counted.

# kB, MB, KiB… are units' symbols; 10M, 10MB and 10MiB are as a search is typed, never translated.
settings_size_units_description_more = Decimal counts a kilobyte as 1000 bytes (kB, MB, GB), as disks are sold and as macOS and GNOME show sizes; Binary counts 1024 (KiB, MiB, GiB), as KDE does. A search reads 10M as 1024-based either way, 10MB as 1000 and 10MiB as 1024.

settings_startup = Open at startup

settings_startup_description = What Skwir opens when it starts with no folder given on the command line.

settings_startup_folder = Startup folder

settings_startup_folder_choice = A chosen folder

settings_startup_folder_description = Used when a chosen folder opens at startup.

# What a setting's description says after its first sentence, in its tooltip.
settings_startup_folder_description_more = Leave empty for your home folder.

settings_startup_last = The windows and tabs left open

# What the TOML reader found wrong in a settings file, in its own words, and the line it is on.
settings_toml_on_line = { $problem } on line { $line }

# A setting's number in megabytes, after its field or slider (64 MB).
settings_unit_megabytes = { $n } MB

# A setting's number in percent, after its slider (100%).
settings_unit_percent = { $n }%

# A setting's number in seconds, after its slider (1.5 s): n may be a decimal.
settings_unit_seconds = { $n } s

# A setting's key no setting has.
settings_unknown = unknown setting: { $key }

# Told at startup: a file of Skwir's could not be read (why) nor renamed (its path, and why not).
settings_unreadable_kept = { $file ->
        [data] Your saved searches, pinned tags and tags’ colours could not be read ({ $why }), so Skwir started without them, and saves nothing there until “{ $path }” is fixed or removed ({ $error }).
       *[settings] Your settings could not be read ({ $why }), so Skwir started with its defaults, and saves nothing there until “{ $path }” is fixed or removed ({ $error }).
    }

# Told at startup: a file of Skwir's could not be read (why, in a few words) and was renamed (its new name).
settings_unreadable_set_aside = { $file ->
        [data] Your saved searches, pinned tags and tags’ colours could not be read ({ $why }), so Skwir started without them. The file was set aside as { $aside }.
       *[settings] Your settings could not be read ({ $why }), so Skwir started with its defaults. The file was set aside as { $aside }.
    }

# Told at startup: a file of Skwir's could not be read; why, in a few words.
settings_unreadable_unsaved = { $file ->
        [data] Your saved searches, pinned tags and tags’ colours could not be read ({ $why }), so Skwir started without them, and saves nothing there this time.
       *[settings] Your settings could not be read ({ $why }), so Skwir started with its defaults, and saves nothing there this time.
    }

# A quality of a smaller copy: nothing lost.
shrink_lossless = Lossless

# Why no smaller copy of a PDF was made, after “No smaller copy of … was made:”.
shrink_note_damaged = it is damaged, and a smaller copy would lose what a reader repairs as it shows it

# Said of a picture made smaller, after its name.
shrink_note_date_not_kept = Its date could not be kept, so it was left out.

# Said of a picture made smaller, after its name.
shrink_note_date_part_unread = Part of its date could not be read, so it was left out.

# Said of a picture made smaller, after its name: EXIF is where a photo keeps its date.
shrink_note_exif_unread = Its EXIF could not be read, so its date was left out.

# Said of a photo whose location and camera details were removed, after its name.
shrink_note_pixels_as_stored = Its pixels were kept as stored, turned upright by its EXIF as before.

# Said of a picture made smaller, after its name.
shrink_note_pixels_kept = Its pixels were kept as they were: written again, they came out larger.

# Said of a picture made smaller, after its name.
shrink_note_profile_left_out = Its colour profile was left out: it is not for the colours the copy is written in, which may look different.

# Why no smaller copy of a PDF was made, after “No smaller copy of … was made:”.
shrink_note_protected = it is protected by a password, which a smaller copy would not keep

# Why no smaller copy of a PDF was kept, after “No smaller copy of … was made:”.
shrink_note_put_back = its images made smaller looked different in a copy and were put back as they were, which left it no smaller

# Why no smaller copy of a PDF was made, after “No smaller copy of … was made:”.
shrink_note_signed = it is signed, and a smaller copy would break its signature

# Why no smaller copy of a PDF was made, after “No smaller copy of … was made:”.
shrink_note_signed_rights = it carries rights its maker signed (to fill it in and save it in Adobe's Reader), which a smaller copy would break

# Why no smaller copy of a PDF was kept, after “No smaller copy of … was made:”.
shrink_note_still_unlike = a copy made still looked different from it with its images put back, so none was kept

# Why no smaller copy of a PDF was kept, after “No smaller copy of … was made:”: its pages, drawn, did not look the same.
shrink_note_unlike = a copy made looked different from it beyond its images, so none was kept

# A quality of a smaller copy: good enough to print.
shrink_print = Print

# A quality of a smaller copy: good enough for a screen.
shrink_screen = Screen

# A quality of a smaller copy: the smallest.
shrink_small = Small

# A sort's direction by words, on a button: alphabetical.
sort_a_to_z = A to Z

# A sort's direction by words inside a sentence: Sort by name, A to Z.
sort_a_to_z_phrase = A to Z

# A sort's direction by Git state, on a button: changed files first.
sort_changes_first = Changes first

# A sort's direction inside a sentence: Sort by Git state, changes first.
sort_changes_first_phrase = changes first

# A sort's direction by Git state, on a button: changed files last.
sort_changes_last = Changes last

# A sort's direction inside a sentence: Sort by Git state, changes last.
sort_changes_last_phrase = changes last

# A sort's direction, on a button: highest rating first.
sort_highest = Highest

# A sort's direction inside a sentence: Sort by rating, highest first.
sort_highest_first = highest first

# A sort's direction, on a button: largest first.
sort_largest = Largest

# A sort's direction inside a sentence: Sort by size, largest first.
sort_largest_first = largest first

# A sort's direction, on a button: longest first.
sort_longest = Longest

# A sort's direction inside a sentence: Sort by duration, longest first.
sort_longest_first = longest first

# A sort's direction, on a button: lowest rating first.
sort_lowest = Lowest

# A sort's direction inside a sentence: Sort by rating, lowest first.
sort_lowest_first = lowest first

# A sort's direction, on a button: newest first.
sort_newest = Newest

# A sort's direction inside a sentence: Sort by date modified, newest first.
sort_newest_first = newest first

# A sort's direction, on a button: oldest first.
sort_oldest = Oldest

# A sort's direction inside a sentence: Sort by date modified, oldest first.
sort_oldest_first = oldest first

# Inside a sentence: Sort by date accessed.
sort_phrase_accessed = date accessed

# Inside a sentence: Sort by date created.
sort_phrase_created = date created

# Inside a sentence: Sort by date deleted (in the Trash).
sort_phrase_deleted = date deleted

# Inside a sentence: Sort by dimensions (a picture's).
sort_phrase_dimensions = dimensions

# Inside a sentence: Sort by duration (a song's, a film's).
sort_phrase_duration = duration

# Inside a sentence: Sort by extension.
sort_phrase_extension = extension

# Inside a sentence: Sort by folder.
sort_phrase_folder = folder

# Inside a sentence: Sort by kind.
sort_phrase_kind = kind

# Inside a sentence: Sort by date modified.
sort_phrase_modified = date modified

# A sort's name inside a sentence: Sort by name, Group by name.
sort_phrase_name = name

# Inside a sentence: Sort by owner.
sort_phrase_owner = owner

# Inside a sentence: Sort by rating.
sort_phrase_rating = rating

# Inside a sentence: Sort by size.
sort_phrase_size = size

# Inside a sentence: Sort by tags.
sort_phrase_tags = tags

# Inside a sentence: Sort by date taken (a picture's).
sort_phrase_taken = date taken

# Inside a sentence: Sort by type.
sort_phrase_type = type

# Inside a sentence: Sort by Git state (changed, new). Git is a program's name.
sort_phrase_vcs = Git state

# A sort's direction, on a button: shortest first.
sort_shortest = Shortest

# A sort's direction inside a sentence: Sort by duration, shortest first.
sort_shortest_first = shortest first

# A sort's direction, on a button: smallest first.
sort_smallest = Smallest

# A sort's direction inside a sentence: Sort by size, smallest first.
sort_smallest_first = smallest first

# A column's header and a sort's name: when the file was last read.
sort_title_accessed = Accessed

# A column's header and a sort's name: when the file was made.
sort_title_created = Created

# A column's header and a sort's name in the Trash: when the file was put there.
sort_title_deleted = Deleted

# A column's header and a sort's name: a picture's width and height.
sort_title_dimensions = Dimensions

# A column's header and a sort's name: how long a song or a film plays.
sort_title_duration = Duration

# A column's header and a sort's name: what the name ends with (.png).
sort_title_extension = Extension

# A column's header and a sort's name: the folder a file is in (in a search).
sort_title_folder = Folder

# A column's header and a sort's name: what a file is (a picture, a document).
sort_title_kind = Kind

# A column's header and a sort's name: when the file last changed.
sort_title_modified = Modified

# A column's header and a sort's name in menus: the file's name.
sort_title_name = Name

# A column's header and a sort's name: who owns the file.
sort_title_owner = Owner

# A column's header and a sort's name: the stars a file was given.
sort_title_rating = Rating

# A column's header and a sort's name.
sort_title_size = Size

# A column's header and a sort's name.
sort_title_tags = Tags

# A column's header and a sort's name: when a picture or a film was taken.
sort_title_taken = Taken

# A column's header and a sort's name: the file's type by name (PNG image).
sort_title_type = Type

# A column's header and a sort's name: the file's state in Git (changed, new). Git is a program's name.
sort_title_vcs = Git

# A sort's direction by words, on a button: reverse alphabetical.
sort_z_to_a = Z to A

# A sort's direction by words inside a sentence: Sort by name, Z to A.
sort_z_to_a_phrase = Z to A

# A heading in the list under the address field: actions whose names hold the words typed.
suggest_actions = Actions

# A heading in the list under the address field: ways to finish what is being typed.
suggest_complete = Complete

# A heading in the list under the address field: folders, tags and places to go to.
suggest_go = Go to

# A heading in the list under the address field: what may be typed there, with examples.
suggest_help = What the field takes

# A heading in the list under the address field: where the pane was before, as Back goes.
suggest_history = Back

# A heading in the list under the address field: the words typed read as a search's terms.
suggest_reading = Did you mean

# A heading in the list under the address field.
suggest_recent = Recent searches

# A heading in the list under the address field: searches kept in the sidebar.
suggest_saved = Saved searches

# A heading in the list under the address field: places to search in.
suggest_search_in = Search in

# A heading in the list under the address field: the terms a search takes (type:, size:), with examples.
suggest_syntax = Terms of a search

# A heading in the list under the address field: the window's other tabs.
suggest_tabs = Open tabs

# A heading in the list under the address field: a row of buttons adding a search term.
suggest_terms = Add a term

# Why a folder removed is not made again by its undo: the permissions it kept are none a folder can have.
sys_bad_folder_permissions = invalid directory permissions

# Why a file's group cannot be changed. system: the system's name (Windows).
sys_group_not_yet = Changing a file's group is not available on { $system } yet

# Why a change of access failed on Linux: the system's /proc folder, through which it reaches the file, is missing.
sys_no_proc = the system's /proc is not mounted, which changing access needs

# Why a change of access stopped: a link took the place of the file it acts on. Begins in lower case: said after the step's own words.
sys_now_a_link = it is a link now

# Why a change of access stopped: another file took the name of the one it acts on. Begins in lower case: said after the step's own words.
sys_now_another_file = it is now another file

# Why a file is not put in the trash on macOS: its path is not text the system's trash takes.
sys_path_not_utf8 = { $path } is no UTF-8 path

# Why a named pipe is not copied. system: the system's name (Windows).
sys_pipe_not_yet = Copying a pipe is not available on { $system } yet

# Why two files' names are not exchanged, on Windows.
sys_swap_windows = Windows cannot swap two names in one step

# Why the trash shows nothing. system: the system's name (macOS).
sys_trash_list_not_yet = Listing the trash is not available on { $system } yet

# Why a file is not put in the trash on macOS, terse: its path, then what the system said.
sys_trash_refused = trash { $path }: { $error }

# Why a file is not put back from the trash. system: the system's name (macOS).
sys_trash_restore_not_yet = Restoring from the trash is not available on { $system } yet

# Why a folder removed on Linux or macOS is not made again on another system by its undo. system: the system's name.
sys_unix_permissions_not_yet = Restoring Unix directory permissions is not available on { $system } yet

# Why a folder removed on Windows is not made again on another system by its undo. system: the system's name.
sys_windows_permissions_not_yet = Restoring Windows directory permissions is not available on { $system } yet

# Why a copy to a removable drive is not written out as it goes. system: the system's name (macOS, Windows).
sys_write_through_not_yet = Writing through to a drive is not available on { $system } yet

# Said where a system's name goes, for a system Skwir does not name: Printing is not available on this system yet.
system_this = this system

tabs_close = Close tab

tabs_close_description = Close a tab (the one shown by default); the notification offers Undo. A window's last tab closes the window while another is open; the last tab of the last window stays (its window's own Ctrl+W closes the window)

tabs_close_window = Close window

# Reopen closed tab and Quit are the names of two actions.
tabs_close_window_description = Close a window (the active one by default) and its tabs, which Reopen closed tab brings back; the last window closes only by its own close button or Quit

# A notification once a tab is closed, with an Undo button: the tab's title.
tabs_closed = Closed tab “{ $title }”

# The button of the notification a closed tab shows: opens it again.
tabs_closed_undo = Undo

# One row in the list of keys for the keys that show a tab by its number.
tabs_go_to_family = Go to a tab

tabs_last = Last tab

tabs_last_tabs = The tabs shown when the window was last closed, before there were windows

# Why Close window did nothing: Quit is the name of an action.
tabs_last_window = The last window closes with its close button or Quit

tabs_last_windows = The windows shown last and their tabs

tabs_list = List tabs

tabs_list_description = The tabs in order, window after window: each one's window, panes, their locations, which pane is active and whether its window shows it

tabs_list_windows = List windows

tabs_list_windows_description = The windows: each one's tabs, the tab it shows, and which is active

tabs_move = Move tab

tabs_move_description = Put a tab at another position, or move it by steps; or move it to another window, or a new one

# One row in the list of keys for the keys that move a tab left or right.
tabs_move_family = Move the tab

tabs_move_left = Move the tab left

tabs_move_right = Move the tab right

tabs_move_to_window = Move the tab to a new window

tabs_new = New tab

# Read by agents and in F1's list of actions; `show` is an argument's name.
tabs_new_description = Open a tab showing a location, or what the pane shows; shown at once unless `show` is false

tabs_new_window = New window

tabs_new_window_description = Open a window showing a location, or what the pane shows, shaped as the pane's window; it becomes the active window

tabs_next = Next tab

tabs_no_pane = no such pane

# A tab asked for by id (by an agent, a script, a key) is not open.
tabs_no_tab = no such tab

# A window asked for by id (by an agent, a script) is not open.
tabs_no_window = no such window

tabs_nothing_for_tab = the pane shows nothing to open in a tab

tabs_nothing_for_window = the pane shows nothing to open in a window

# A key's action: show the window's tab of this number, counted from 1 on the left.
tabs_nth = Tab { $number }

tabs_only_folders = Only folders and places open in a tab or a window

tabs_open = Open in new tab

tabs_open_description = Open each selected folder in a tab of its own, behind the tab shown; files do not open in tabs

tabs_open_window = Open in new window

tabs_open_window_description = Open each selected folder in a window of its own, shaped as the pane's; files do not open in windows

tabs_previous = Previous tab

tabs_reopen = Reopen closed tab

tabs_reopen_description = Open again the tab or window closed last, or the one a close reported, with its history

tabs_select = Show tab

tabs_select_description = Show a tab: by id, or among the active window's tabs by position (negative from the right) or by steps from the one shown

tabs_select_window = Go to window

tabs_select_window_description = Make a window the active one, brought forward

tabs_sleep = Free the memory of hidden tabs

tabs_sleep_description = A tab you are not looking at lets go of its list of files and lists it again when shown.

# What a setting's description says after its first sentence, in its tooltip.
tabs_sleep_description_more = Turn this off to keep every tab listed and up to date, which uses more memory and background work.

# Said to an agent or a script: `window` is the argument's name.
tabs_which_window = which window: `window` is needed

# A heading of the list of keys: doing something to files (copying, renaming, trashing).
topic_files = Files

# A heading of the list of keys: searching and filtering.
topic_find = Finding

# A heading of the list of keys: going to folders and places, history, the address.
topic_go = Going places

# A heading of the list of keys: tags, ratings and comments.
topic_notes = Tags and notes

# A heading of the list of keys: the sidebar's places, drives and servers.
topic_places = Places and servers

# A heading of the list of keys: the audio and video player.
topic_player = Playing

# A heading of the list of keys: inside a file's preview.
topic_preview = Previews

# A heading of the list of keys: the cursor and the selection.
topic_select = Selecting

# A heading of the list of keys: the program itself (settings, plugins, help, quitting). Skwir is its name.
topic_skwir = Skwir

# A heading of the list of keys: windows, tabs and the split.
topic_tabs = Tabs and panes

# A heading of the list of keys: the terminal panel.
topic_terminal = Terminal

# A heading of the list of keys: how files are shown, and which panels are.
topic_view = Views and panels

# A heading of the list of keys: moving about the window, its menus and dialogs.
topic_window = The window

# Why a file is not put in the trash: its folder is a server's, mounted by a program (sshfs, rclone, GVfs).
trash_op_mounted_remote = a network folder a program mounts keeps no trash: delete it instead

# Why a file is not put in the trash: the user's home folder, which holds the trash, is not found.
trash_op_no_home = there is no home folder for its trash

# Why a file is not put in the trash: its network share refuses a trash folder to the user.
trash_op_share_refused = the network share keeps no trash you may use: delete it instead

# Why a folder is not put in the trash: it is the top of a drive (/).
trash_op_top = the top of a filesystem cannot be put in the trash

# A file's state in Git: changed since the last commit.
vcs_changed = Changed

# A file's state in Git, in a column, a caption or a group's heading: a merge left it with both sides.
vcs_conflict = In conflict

# A folder's state in Git: Git is told to leave it out.
vcs_folder_ignored = Ignored

# A folder's state in Git: files below it changed since the last commit.
vcs_holds_changes = Holds changes

# A folder's state in Git: a file below it is in conflict.
vcs_holds_conflict = Holds a conflict

# A folder's state in Git: files below it are in no commit yet.
vcs_holds_new = Holds new files

# A folder's state in Git: files below it have changes added for the next commit.
vcs_holds_staged = Holds staged changes

# A file's state in Git: Git is told to leave it out.
vcs_ignored = Ignored

# A file's state in Git: not yet in any commit.
vcs_new = New

# A file's state in Git: its changes are added for the next commit.
vcs_staged = Staged

# A tab asked for by id (by an agent, a script, a key) is not open.
workspace_no_tab = no such tab

# A window asked for by id (by an agent, a script) is not open.
workspace_no_window = no such window

# Why a tab was not moved to a new window of its own.
workspace_tab_alone = the tab is alone in its window

# Why a tab was not moved to the window asked for.
workspace_tab_there = the tab is in that window already
