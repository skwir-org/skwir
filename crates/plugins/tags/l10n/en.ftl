# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

tags_add = Add tags

tags_add_description = Add tags to the entries given, or the selection (plan)

# A plan's title. tags: the tags' names, a list; count: how many.
tags_add_plan = add { $count ->
        [1] tag
       *[other] tags
    } { $tags }

# A plan's title. add, remove: the tags' names, each a list; added, removed: how many.
tags_add_remove_plan = add { $added ->
        [1] tag
       *[other] tags
    } { $add } and remove { $removed ->
        [1] tag
       *[other] tags
    } { $remove }

# A plan's title: files dropped on a tag's place. tag: its name.
tags_add_tag_plan = add tag { $tag }

# The last place of the sidebar's Tags: every tag of the home folder.
tags_all_tags = All Tags

tags_already_called = the tag is already called { $to }

# Why a Mac's ._ file is not written for a file, said after the file's path.
tags_aside_no_name = it has no name a file beside it is named after

# Said after a file's path. name: the Mac's ._ file beside it.
tags_aside_other_layout = { $name } beside it is laid out otherwise than a Mac writes one, and Skwir rewrites no other

# Said after a file's path. name: the Mac's ._ file beside it.
tags_aside_too_large = { $name } beside it is larger than Skwir rewrites

# Said after a file's path. name: the Mac's ._ file beside it; error: why it could not be read.
tags_aside_unreadable = { $name } beside it cannot be read: { $error }

tags_aside_unwritable = its Mac attributes are kept in a form Skwir does not write

# The same button's tooltip while the folder's tags are shown.
tags_aspect_close = Back to the folder

# The tooltip of the button at the end of a folder's address that shows its tags.
tags_aspect_open = Tags of this folder

tags_browse = Tags of a folder

tags_browse_description = Show every tag of the files under a folder (`in`, an absolute path) as places to browse, each a folder of its files: the folder the pane shows when omitted, or the one a tag location or a search there is of; with `toggle` and no folder, a tag location goes back to its folder instead

# An item of a folder's menu, on its background: its files' tags as places.
tags_browse_here = Browse tags here

# A way to run Tags of a folder, in F1's list of actions.
tags_browse_toggle = Tags of this folder, or back to it

tags_category_no_comma = a category's name has no comma

# A plan's title.
tags_change_plan = change tags

tags_color = Tag color

# A color a tag can wear, in its menu.
tags_color_blue = Blue

# The name of a tag's menu's row of colors.
tags_color_choice = Color

tags_color_description = Choose the color a tag wears everywhere, or give it back the one its name gives it

# A tag's color: the one its name gives it, none chosen.
tags_color_from_name = From Its Name

# A color a tag can wear, in its menu.
tags_color_green = Green

tags_color_not_saved = The tag's color was not saved: { $error }

# An error. colors: the colors' names as a command takes them (red, orange…), a list.
tags_color_one_of = a tag's color is one of { $colors }

# A color a tag can wear, in its menu.
tags_color_orange = Orange

# A color a tag can wear, in its menu.
tags_color_pink = Pink

# A color a tag can wear, in its menu.
tags_color_purple = Purple

# A color a tag can wear, in its menu.
tags_color_red = Red

# A color a tag can wear, in its menu.
tags_color_teal = Teal

# A color a tag can wear, in its menu.
tags_color_yellow = Yellow

tags_colors_data = Colors the user chose for tags, by tag in lowercase: {"{"} tag = color {"}"}

# The button that keeps a comment typed.
tags_comment_accept = Save

# An item of the files' menu: it asks for the selection's comment.
tags_comment_item = Comment…

tags_comment_menu = Comment menu

tags_comment_menu_description = The context menu's Comment…, holding the selected entry's comment

tags_comment_placeholder = A comment on the selection (empty removes it)

# The title of the question that edits the selection's comment.
tags_comment_title = Comment

tags_counted = Tags counted

tags_counted_description = `{"{"} root, counting, counted, partial {"}"}`: whether the tags of the files under `root` (the home folder when omitted) are being counted for its tag locations, as of when a count covers them (`counted`, in seconds since the epoch), and whether some are left out for want of room (`partial`). Answers from memory

tags_delete = Remove tag everywhere

tags_delete_description = Take a tag (or `tags`) from every file under the home folder (or `root`) that has it: the files are searched on a worker and the plan proposed for review

# Of the address field's two depths in a tag's place: every file within it, the tags within it included.
tags_depth_below = With sub-tags

# Of the address field's two depths in a tag's place: the files tagged with it exactly.
tags_depth_here = This tag

# Of the address field's two depths in the place of every tag: every file that has one.
tags_depth_top_below = Every tagged file

# Of the address field's two depths in the place of every tag: the tags.
tags_depth_top_here = These tags

tags_drop = Drop on a tag

tags_drop_description = Give files dropped or pasted on a tag location (`location`) its tag: a pane's selection (`pane`) or `entries`; with `move`, out of the tag location they come from (the pane's, or `from`). Tags dropped go into it, a plan to review

# An error: files were dropped on every tag at once, which gives them none.
tags_drop_on_one = files are given one tag: drop them on it

tags_edit = Edit tags

tags_edit_description = Add some tags to the entries given, or the selection, and remove others, as one plan: each entry keeps its other tags

# The first item of the files' menu's Tags submenu: it opens the tag editor.
tags_edit_item = Edit Tags…

tags_empty_tag = No file here has this tag

tags_empty_tag_detail = Drop files here to give them the tag.

tags_empty_top = No file here has a tag

tags_empty_top_detail = Give files tags with Tags… in their menu, or drop them on a tag in the sidebar.

tags_exclusive = One tag of a category at a time

tags_exclusive_data = The categories a file has one tag of at a time: fields, whose values are the tags within them

tags_exclusive_description = Make a category (`tag`) hold one tag per file at a time, a field whose values are its tags (`on`), or several again

# tag: a category of tags (Status), marked as holding one tag at a time or not.
tags_exclusive_not_changed = { $tag } was not changed: { $error }

# tag: a category of tags (Status).
tags_exclusive_off = A file can have several tags of { $tag } again

# tag: a category of tags (Status), its tags now the values of one field.
tags_exclusive_on = A file now has one tag of { $tag } at a time

tags_export = Copy a tag into a folder

tags_export_description = Make in `dest` a folder of the files a tag location (`location`) lists, named as its tag, with a folder inside for each tag within it: a plan proposed for review. What a tag dropped or pasted in a folder does

# An error: the place of every tag was dropped in a folder, where one tag's place makes a folder of its files.
tags_export_one = a tag is copied, not all of them

# A plan's title. tag: the tag's name; folder: where the copies go.
tags_export_plan = copy the files tagged { $tag } to { $folder }

tags_files_menu = Tags of the selection's menu

tags_files_menu_description = The context menu's Tags submenu: the tags the selected files have, then the pinned ones, each checked when they all have it, partly when some do, pressing it toggling it on them all

tags_files_not_found = The files tagged { $tag } were not found: { $error }

# done: what was done to the sidebar's pinned tags (tags_pinned…); filter: the filter the tag's place was shown with, as typed.
tags_filter_not_kept = { $done }; its filter “{ $filter }” is not kept with it

# What a Mac's Finder tags cannot hold that other places notes are kept can, placed after "What it cannot hold:" in its settings (app_notes_read_limits).
tags_finder_limits = a rating, or a tag color other than the Finder's seven

# As tags_follow_both_will, found done already.
tags_follow_both_already = { $to } was already pinned and colored as { $count ->
        [1] { $from } was
       *[other] { $from } were
    }

# As tags_follow_both_will, done.
tags_follow_both_did = kept { $to } pinned and colored as { $count ->
        [1] { $from } was
       *[other] { $from } were
    }

# A step of renaming tags. to: the tag's new name; from: the tags renamed, a list; count: how many.
tags_follow_both_will = keep { $to } pinned and colored as { $count ->
        [1] { $from } was
       *[other] { $from } were
    }

# As tags_follow_color_will, found done already.
tags_follow_color_already = { $to } already had the color { $count ->
        [1] { $from } had
       *[other] { $from } had
    }

# As tags_follow_color_will, done.
tags_follow_color_did = gave { $to } the color { $count ->
        [1] { $from } had
       *[other] { $from } had
    }

# A step of renaming tags. to: the tag's new name; from: the tags renamed, a list; count: how many.
tags_follow_color_will = give { $to } the color { $count ->
        [1] { $from } had
       *[other] { $from } had
    }

# As tags_follow_pin_will, found done already.
tags_follow_pin_already = { $to } was already pinned where { $count ->
        [1] { $from } was
       *[other] { $from } were
    }

# As tags_follow_pin_will, done.
tags_follow_pin_did = pinned { $to } where { $count ->
        [1] { $from } was
       *[other] { $from } were
    }

# A step of renaming tags. to: the tag's new name; from: the tags renamed, a list; count: how many.
tags_follow_pin_will = pin { $to } where { $count ->
        [1] { $from } was
       *[other] { $from } were
    }

# As tags_follow_unpin_will, found done already.
tags_follow_unpin_already = { $count ->
        [1] { $from } was already unpinned
       *[other] { $from } were already unpinned
    }

# As tags_follow_unpin_will, done.
tags_follow_unpin_did = unpinned { $count ->
        [1] { $from }
       *[other] { $from }
    }

# A step of removing tags from every file. from: the tags, a list; count: how many.
tags_follow_unpin_will = unpin { $count ->
        [1] { $from }
       *[other] { $from }
    }

# As tags_give_file_will, found done already.
tags_give_file_already = { $name } already had the tag { $tag }

# As tags_give_file_will, done.
tags_give_file_did = gave { $name } the tag { $tag }

# A step: a file dropped on a tag's place. name: the file's; tag: the tag's.
tags_give_file_will = give { $name } the tag { $tag }

# An error: the tags moved into a category are there already.
tags_in_category_already = these tags are in { $category } already

tags_in_no_category = these tags are in no category

# A switch of a tag's place: it lists the files of the tags within it too.
tags_include_sub_tags = Include Sub-Tags

# A notice with a button to the setting Room for tagged files.
tags_index_full = Some tagged files are left out of the tags: the room Skwir gives them is full

# An error: a tag moved or renamed into a category within it.
tags_inside_itself = { $tag } cannot go inside itself

tags_keep = Pin the tag shown

tags_keep_description = Pin the tag a pane's tag location shows: one tag of the home folder, not narrowed

tags_keep_forget = Unpin this tag from the sidebar

# An error: what Ctrl+D keeps of a tag's place.
tags_keep_one = a tag of the home folder, not narrowed, is kept in the sidebar: pinned

tags_keep_save = Pin this tag to the sidebar

# What a category of tags holding one tag at a time is, in the Type column of a tag's place.
tags_kind_field = Field

# What a tag is, in the Type column of a tag's place.
tags_kind_tag = Tag

tags_known = Tags known

tags_known_description = `{"{"} generation, tags? {"}"}`: every tag the user has, each once, from memory: those pinned, in their order; then the most recently given; then those seen on files; then those counted under the home folder, on the most files first. `generation` numbers them: asked with the one held, `tags` is left out while it is current

tags_limit_description = How much memory Skwir may give what it knows of tagged files, about sixteen thousand files a megabyte, for tags browsed as folders, the sidebar's suggestions and quicker tag searches.

# What a setting's description says after its first sentence, in its tooltip.
tags_limit_description_more = Past it, new tagged files are left out of the tags' folders, and Skwir says so.

tags_limit_title = Room for tagged files

tags_list = List tags

tags_list_description = Tags used in the pane's listing, with counts (tags that differ only in case count as one); with `read`, read from the files whose tags the listing has not read yet too

# tags: the tags' names, a list of those any of which the files have.
tags_looking_home = Looking for the files tagged { $tags } in the home folder…

# tags: the tags' names, a list of those any of which the files have; root: a folder's path.
tags_looking_in = Looking for the files tagged { $tags } in “{ $root }”…

# The button that merges the tags into the one typed.
tags_merge_accept = Merge

tags_merge_ask = Merge these tags into one, on every file

# An item of the menu of several tags: it asks for the tag they become.
tags_merge_item = Merge Into One…

tags_merge_placeholder = The tag they become

# A plan's title. tags: the tags' names, each quoted, a list.
tags_merge_plan = merge the tags { $tags } into { $to }

# Said of a plan renaming tags, after how many files it changes. merged: how many of them had the new name already, or several of the tags it renames; to: the tags' new name.
tags_merged_into = { $merged ->
        [one] { $merged } file already had { $to }, or several of the tags renamed: it keeps { $to } once.
       *[other] { $merged } files already had { $to }, or several of the tags renamed: each keeps { $to } once.
    }

# Said of a plan renaming several tags (moved into a category), after how many files it changes. merged: how many of them had two tags whose new names are the same.
tags_merged_names = { $merged ->
        [one] { $merged } file had two tags that now have the same name: it keeps that tag once.
       *[other] { $merged } files had two tags that now have the same name: each keeps that tag once.
    }

tags_move = Move tag into category

# The button that moves tags into the category typed.
tags_move_accept = Move

# The title of the question Move to Category… asks.
tags_move_ask = Move into a category, keeping the names

tags_move_description = Put a tag (or `tags`) into the category `into`, keeping its own name (`Paris` into `Places/France` is `Places/France/Paris`), or out of its category without `into`, on every file under the home folder (or `root`): the files are searched on a worker and the plan proposed for review

# As tags_move_file_will, found done already.
tags_move_file_already = { $name } was already in { $tag }

# As tags_move_file_will, done.
tags_move_file_did = moved { $name } from { $from } to { $tag }

# A step: a file dragged from one tag's place to another's. name: the file's; from, tag: the tags'.
tags_move_file_will = move { $name } from { $from } to { $tag }

# A plan's title. into: a category of tags (Places/France).
tags_move_into_plan = move the tag { $tag } into { $into }

# An item of a tag's menu: it asks for the category the tags go into.
tags_move_item = Move to Category…

tags_move_out_all_item = Move Out of Their Categories

# An item of a tag's menu. category: the category it is in.
tags_move_out_item = Move Out of { $category }

# A plan's title.
tags_move_out_plan = move the tag { $tag } out of its category

tags_move_placeholder = Category (Places/France)

# A plan's title. tags: the tags' names, each quoted, a list; into: a category of tags (Places/France).
tags_move_several_into_plan = move the tags { $tags } into { $into }

# A plan's title. tags: the tags' names, each quoted, a list.
tags_move_several_plan = move the tags { $tags }

# A plan's title: files dragged from one tag's place to another's. from, tag: the tags' names.
tags_move_tag_plan = move from tag { $from } to tag { $tag }

tags_narrow = Narrow a tag location

# An error for a script or an agent. facet: what it asked to narrow by, as it wrote it.
tags_narrow_deep_only = a tag's place is narrowed by `deep`; `{ $facet }` is the pane's lens (query.narrow)

tags_narrow_description = Show, in the tag location the pane shows, every file within the tag (`facet` `deep`), not only those tagged with it exactly; `on` false takes it back, omitted it switches. Other tags, kinds and years are the pane's lens (`query.narrow`)

tags_needs_name = a tag needs a name

tags_needs_search = finding the tagged files needs the Search plugin

tags_new = New tag

# The button that makes the tag named.
tags_new_accept = Create

# The title of the question for a new tag's name.
tags_new_ask = New tag

tags_new_description = Make a tag `name` in the tag a tag location shows (`location`): shown there until a file is given it

# The title of the question for a new tag's name. tag: the tag it goes in.
tags_new_in_ask = New tag in { $tag }

# What the empty field asks for: a new tag's name.
tags_new_placeholder = Name

# An item of a tag's place's menu, where a folder's says New Folder….
tags_new_verb = New Tag…

tags_no_comma = a tag's name has no comma

# An error: the system names no home folder.
tags_no_home = no home folder

tags_no_home_to_find = no home folder to find tags in

# The rating's value of none, in the files' menu.
tags_no_rating = No rating

# An error: the pane a command named is not there.
tags_no_such_pane = no such pane

# tags: the tags' names, a list of those any of which the files have.
tags_none_tagged_home = No file in the home folder is tagged { $tags }

# tags: the tags' names, a list of those any of which the files have; root: a folder's path.
tags_none_tagged_in = No file in “{ $root }” is tagged { $tags }

# An item of a tag's menu, checked while on: the tags within a category become the values of one field, a file having one of them at a time. category: its name.
tags_one_at_a_time = One { $category } Tag at a Time

tags_open = Open a tag

tags_open_description = Show the files of the home folder that have a tag, and a place for each tag within it, as its pinned pill does, in the pane given or the active one

# An item of a tag's menu: its place, every file that has it.
tags_open_tag = Open Tag

tags_pane_shows_no_tags = the pane shows no tags

tags_pane_shows_nothing = the pane shows nothing

tags_pill = A tag, here

tags_pill_description = A tag of the sidebar clicked with Shift (`how` `also`: the files shown that have it too) or Alt (`here`: its files in the folder shown)

tags_pin = Pin tags

tags_pin_description = Keep tags (`tag`, `tags`) in the sidebar, before the pinned tag `before` or last; moves those pinned

tags_pin_item = Pin to Sidebar

# A pinned tag was moved among the sidebar's tags.
tags_pin_moved = Moved the tag { $tag }

# tag: the tag's name, when one was pinned; count: how many.
tags_pinned = { $count ->
        [1] Pinned the tag { $tag } to the sidebar
       *[other] Pinned { $count } tags to the sidebar
    }

tags_pinned_data = The tags pinned to the sidebar, in their order

tags_pins = Set pinned tags

# Said once the sidebar's pinned tags were put back, by an undo or a redo.
tags_pins_back = Put the pinned tags back as they were

tags_pins_description = Pin exactly `tags`, in their order: what an Undo of a pin runs

tags_pins_not_saved = the pinned tags were not saved

tags_pins_not_saved_said = The pinned tags were not saved: { $error }

tags_place_menu = Tags' menu

tags_place_menu_description = The context menu's items for the tags selected in a tag location. Answers from memory

# Said of a plan renaming or removing tags on every file: how many files it changes.
tags_plan_note_home = { $count ->
        [one] { $count } file
       *[other] { $count } files
    } in the home folder; what your exclusions leave out is left out.

# As tags_plan_note_home, then merged: how many files kept one tag of two (tags_merged_into, tags_merged_names).
tags_plan_note_home_merged = { $count ->
        [one] { $count } file
       *[other] { $count } files
    } in the home folder; what your exclusions leave out is left out. { $merged }

# Said of a plan renaming or removing tags on every file under a folder: how many files it changes. root: the folder's path.
tags_plan_note_in = { $count ->
        [one] { $count } file
       *[other] { $count } files
    } in “{ $root }”; what your exclusions leave out is left out.

# As tags_plan_note_in, then merged: how many files kept one tag of two (tags_merged_into, tags_merged_names).
tags_plan_note_in_merged = { $count ->
        [one] { $count } file
       *[other] { $count } files
    } in “{ $root }”; what your exclusions leave out is left out. { $merged }

tags_plugin_description = Tags, ratings and comments kept in the files' extended attributes, no database; where downloads came from

# Why a step of a plan about tags' pins and colors cannot run.
tags_plugin_off = the Tags plugin is off

tags_plugin_title = Tags

# The name of the files' menu's row of stars.
tags_rating_choice = Rating

tags_remove = Remove tags

# A plan's title.
tags_remove_all_plan = remove tags

# An item of the files' menu in the place of every tag.
tags_remove_all_verb = Remove All Tags

tags_remove_description = Remove tags from the entries given, or the selection (plan)

tags_remove_everywhere_item = Remove Tag From Every File…

# A plan's title.
tags_remove_everywhere_plan = remove the tag { $tag } from every file

# An item of a tag's menu where a file shows it: takes it from the files selected. count: how many.
tags_remove_from_files = { $count ->
        [1] Remove From This File
       *[other] Remove From Selected Files
    }

# An item of the files' menu in a tag's place: takes the tag off them. tag: its name.
tags_remove_from_verb = Remove from { $tag }

# A plan's title. tags: the tags' names, a list; count: how many.
tags_remove_plan = remove { $count ->
        [1] tag
       *[other] tags
    } { $tags }

# A plan's title. tags: the tags' names, each quoted, a list.
tags_remove_several_everywhere_plan = remove the tags { $tags } from every file

tags_rename = Rename tag everywhere

# The button that renames a tag on every file.
tags_rename_accept = Rename

# The title of the question that asks for a tag's new name.
tags_rename_ask = Rename the tag { $tag } on every file

tags_rename_description = Rename a tag (or merge `tags`) on every file under the home folder (or `root`) that has it, merging it with a tag of the new name where a file has both: the files are searched on a worker and the plan proposed for review

tags_rename_placeholder = New name (an existing tag's merges them)

# A plan's title.
tags_rename_plan = rename the tag { $tag } to { $to }

# An item of a tag's menu in its place: it asks for its new name on every file.
tags_rename_verb = Rename Tag…

# As tags_restore_will, found done already.
tags_restore_already = the pinned tags and their colors were already back

# As tags_restore_will, done.
tags_restore_did = put the pinned tags and their colors back

# A step that undoes what renaming tags did to their pins and colors.
tags_restore_will = put the pinned tags and their colors back

tags_search_cancelled = cancelled

# An error: the search that finds a tag's files did not answer as it should.
tags_search_no_location = the search gave no location

tags_search_stopped = the search stopped

tags_selected = Tags of the selection

tags_selected_description = `{"{"} items, tags: [{"{"} name, count, color? {"}"}], known {"}"}`: the selection's tags, read from its files, with how many carry each and the color a file gives one (a Finder tag's), and the tags known, those pinned first, then the most recently used

tags_set = Set tags

tags_set_description = Replace the tags of the entries given, or the selection's (plan)

# A plan's title: the files' tags replaced by these. tags: the tags' names, a list; count: how many.
tags_set_plan = set { $count ->
        [1] tag
       *[other] tags
    } { $tags }

# An item of a tag's menu: its files in the folder shown.
tags_show_files_here = Show Its Files Here

# A rating's value in the files' menu: stars, 1 to 5.
tags_stars = { $stars ->
        [one] { $stars } star
       *[other] { $stars } stars
    }

tags_store_appledouble = A Mac's ._ files, beside files on volumes without attributes

tags_store_finder = A Mac's Finder tags and comments

tags_store_xdg = The desktop's attributes (KDE, Dolphin, Baloo)

tags_suggested_description = Until a tag is pinned, how many the sidebar suggests, those on the most files first; 0 suggests none.

tags_suggested_title = Tags suggested in the sidebar

# Beside a tag the address field offers: tag is its whole name, its categories included (Places/France/Paris).
tags_suggestion_detail = Tag · { $tag }

tags_tag_menu = A tag's menu

tags_tag_menu_description = The items of a tag's menu where it is shown: its chip in Properties (tags), its pill in the sidebar (location). Answers from memory

# The place of every tag of a folder's files: its tab, its step of the trail.
tags_title = Tags

tags_toggle = Toggle a tag

tags_toggle_description = Give the entries given, or the selection, a tag, or take it from every entry when all of them have it: a command to bind to a key; with `on`, give it (true) or take it (false) whatever they have (plan)

# A way to run Toggle a tag, one per pinned tag, in F1's list of actions.
tags_toggle_form = Toggle tag { $tag }

tags_unpin = Unpin tags

tags_unpin_description = Take tags (`tag`, `tags`) out of the sidebar; their files keep them

tags_unpin_item = Unpin From Sidebar

# tag: the tag's name, when one was unpinned; count: how many.
tags_unpinned = { $count ->
        [1] Unpinned the tag { $tag }
       *[other] Unpinned { $count } tags
    }

tags_untag = Remove from tag

# As tags_untag_all_file_will, found done already.
tags_untag_all_file_already = { $name } already had no tags

# As tags_untag_all_file_will, done.
tags_untag_all_file_did = removed the tags of { $name }

# A step. name: the file's.
tags_untag_all_file_will = remove the tags of { $name }

# A plan's title: files taken out of the place of every tag.
tags_untag_all_plan = remove all tags

tags_untag_description = Take the tag a tag location shows (the pane's, or `location`) off the selected files; tags selected there are taken off every file, a plan to review. What Delete does in a tag location

# As tags_untag_file_will, found done already.
tags_untag_file_already = the tag { $tag } was already off { $name }

# As tags_untag_file_will, done.
tags_untag_file_did = removed the tag { $tag } from { $name }

# A step. tag: the tag's name; name: the file's.
tags_untag_file_will = remove the tag { $tag } from { $name }

# A plan's title: files taken out of a tag's place. tag: its name.
tags_untag_plan = remove tag { $tag }

tags_what_new_name = what new name?

tags_which_category = which category?

# An error: a command about a tag was given no tag.
tags_which_tag = which tag?

# A pill of a tag's trail while it lists the files of the tags within it too.
tags_with_sub_tags = With sub-tags

# What the desktop's attributes cannot hold that other places notes are kept can, placed after "What it cannot hold:" in its settings (app_notes_read_limits).
tags_xdg_limits = a comma in a tag, or a tag's color
