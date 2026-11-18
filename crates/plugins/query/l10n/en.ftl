# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# The button that filters the folder with a value typed in a chip.
query_accept_filter = Filter

# The button that searches with a value typed in a search's chip.
query_accept_search = Search

# Chooses a tag to type, among the tags the files have.
query_another_tag = Another tag…

query_app_ended = the app has ended

# What the field asking for a tag holds while empty.
query_ask_tag_placeholder = A tag

# An error in a search's text. key: a key of the search language (mime, a file's type); glob: the pattern typed; error: what the pattern library said of it.
query_bad_glob = bad { $key } glob `{ $glob }`: { $error }

# An error in a search's text. regex: the pattern as written; error: what the regex library said of it.
query_bad_regex = bad regex `{ $regex }`: { $error }

# An error in a search's text: what was typed is no size (10M, 1.5GB).
query_bad_size = bad size `{ $value }`

# An error in a search's text. value: what was typed; 2026-05-01, this-week and 7d are written as the search language writes them: keep them.
query_bad_time = bad date or duration `{ $value }` (try 2026-05-01, this-week or 7d)

query_below = This folder and its subfolders

query_below_description = Search the pane's folder and its subfolders for what filters it: its lens and the words typed over it

# A search's chip: when its files were last opened.
query_chip_accessed = Accessed

# A search's chip: text its files hold.
query_chip_contains = Contains

# A search's chip: when its files were made.
query_chip_created = Created

# A search's chip of where it looks: in a place, searched as it lists (In Recent).
query_chip_in = In { $place }

# A search's chip: the kinds of its files (images, documents).
query_chip_kind = Kind

# The value of a search's Kind chip: the kinds its files may be of, a list (Images or Videos).
query_chip_kinds = { $kinds }

# A search's chip: when its files were last changed.
query_chip_modified = Modified

# A search's chip: the words its files' names hold.
query_chip_name = Name

# A search's chip: a tag none of its files has.
query_chip_not_tag = Not tagged

# A search's chip: how big its files are.
query_chip_size = Size

# A search's chip of where it looks: the folder and the folders within it.
query_chip_subfolders = And subfolders

# A search's chip: a tag its files have.
query_chip_tag = Tag

# A search's chip: when its pictures and videos were taken.
query_chip_taken = Taken

# The value of a search's Modified, Created, Accessed or Taken chip set to a year, a month or a day: when, as the reader's locale writes it (2026, May 2026, May 3, 2026).
query_chip_when = { $when }

# The value of a search's Modified, Created, Accessed or Taken chip set to days: the first and the last (Mar 3, 2026 → Mar 9, 2026).
query_chip_when_days = { $first } → { $last }

query_complete = Complete a search

# shape: what it answers, written as JSON.
query_complete_description = { $shape }: what the term ending at `caret` (bytes; the end when omitted) can go on with, each item's `text` written in place of `range`: the keys a word starts, a key's values (kinds, the tags of the pane's files then the user's others, at most 200 of them, `more` counting those left out, periods, sizes, stars); `typing` when that term is still incomplete

# What the field asking for text the files hold holds while empty.
query_contains_placeholder = Text the files contain

# A chip's last choice: asks for a value to type.
query_custom = Custom…

# A choice of a filter's chip: the folder searched with its subfolders.
query_depth_below = This folder and its subfolders

# A filter's chip: how deep its words look, a folder's own files alone.
query_depth_chip = This folder only

# A choice of a filter's chip: the folder's own files alone.
query_depth_here = This folder only

query_describe = Describe search

query_describe_description = The search's chips, as the address field draws them for a pane that shows a search: each facet (words, kind, modified, taken, size, tag, content, where; created and accessed once set) with its value and the invocation each of its choices runs, then the parts no facet holds

query_describe_filter = Describe filter

# shape: what it answers, written as JSON.
query_describe_filter_description = { $shape }: what filters the pane (its lens and the words typed over it, ADR-107) as a search's chips (`query.describe`), each choice changing it (`query.filter`): how deep it looks (a folder's: this folder only, or its subfolders searched, `query.below`), then each facet, set and not

query_edit = Edit saved search

# The button that keeps a saved search or filter as edited.
query_edit_accept = Save

query_edit_description = Change what a saved search looks for, or what filters a saved place; its name and place in the sidebar stay

# A saved place's menu item: edits what filters it.
query_edit_filter_menu = Edit Filter…

query_edit_filter_title = Edit filter

query_edit_search_menu = Edit Search…

# The example of a search in the field where a saved search is edited. example: a search, as written; key: the term that says where it looks.
query_edit_search_placeholder = { $example }  ({ $key } picks the root)

query_edit_search_title = Edit saved search

# An error in a search's text: content:// holds no pattern. key: a key of the search language, as written.
query_empty_regex = { $key } empty regex

# An error in a search's text; error says what is wrong in it (kind: expects …).
query_error = query: { $error }

# What a saved search of every file is named, made from its terms: Everything in Documents.
query_everything = Everything

# An error in a search's text. yes and no: the values an option takes; value: what was typed. All as written.
query_expected_bool = expected { $yes } or { $no }, got `{ $value }`

# An error in a search's text: a comparison (>, <, =) was expected after what was typed.
query_expected_comparison = expected a comparison after `{ $text }`

# An error in a search's text: something to look for is missing (after OR, in parentheses).
query_expected_term = expected a term

# An error in a search's text. key: a key of the search language; 5km and 500m are distances as the search language writes them: keep them; value: what was typed.
query_expects_distance = { $key } expects a distance such as 5km or 500m, got `{ $value }`

# An error in a search's text. key: a key of the search language; 9.9,19.9,10.2,20.3 is an area as the search language writes it: keep it; value: what was typed.
query_expects_edges = { $key } expects south, west, north and east edges, such as 9.9,19.9,10.2,20.3, got `{ $value }`

# An error in a search's text. key: a key of the search language; one and other: the values it takes; value: what was typed. All as written.
query_expects_either = { $key } expects { $one } or { $other }, got `{ $value }`

# An error in a search's text. key: a key of the search language, as written.
query_expects_extension = { $key } expects at least one extension

# An error in a search's text. key: a key of the search language, as written.
query_expects_folder = { $key } expects a folder

# An error in a search's text. key: a key of the search language; kinds: the words it takes, as written, a list (folder, image… or program); value: what was typed.
query_expects_kind = { $key } expects { $kinds }, got `{ $value }`

# An error in a search's text. key: a key of the search language; kinds: the words it takes, as written, a list (folder, image… or program).
query_expects_kinds = { $key } expects { $kinds }

# An error in a search's text. key: a key of the search language, as written; value: what was typed after it.
query_expects_number = { $key } expects a number, got `{ $value }`

# An error in a search's text. key: a key of the search language; 45.1,6.2 is a point as the search language writes it: keep it; value: what was typed.
query_expects_point = { $key } expects a latitude and a longitude, such as 45.1,6.2, got `{ $value }`

# An error in a search's text. key: a key of the search language, as written; value: what was typed.
query_expects_stars = { $key } expects stars from 0 to 5 (halves allowed), got `{ $value }`

# An error in a search's text. key: a key of the search language; file, dir and link: the values it takes; value: what was typed. All as written.
query_expects_type = { $key } expects { $file }, { $dir }, or { $link }, got `{ $value }`

# An error in a search's text. key: a key of the search language, as written (kind:).
query_expects_value = { $key } expects a value

# Beside a search the address field offers: where it looks. place: a place that lists files (Recent).
query_field_among = among the files in { $place }

# In the address field, in place of the search: a quote typed is not closed yet.
query_field_close_quote = Close the quote to search

# Beside a search the address field offers: where it looks, the home folder and every folder within it.
query_field_home = everywhere in Home

# Beside a search the address field offers: where it looks. place: a folder's name.
query_field_in_folders = in { $place } and its folders

# Below the tags the address field offers: how many more go on with what is typed.
query_field_more_tags = { $count ->
        [one] { $count } more tag
       *[other] { $count } more tags
    }

# Beside how many more tags go on with what is typed in the address field.
query_field_narrow_tags = Type more of the name to narrow them

# Beside a search run lately, offered by the address field: where it looked.
query_field_recent_in = in { $place }

# Beside a saved place, offered by the address field: what filters it, as written, and where it is.
query_field_saved_in = { $filter } in { $place }

# The address field's first row: runs the search typed. search: the search, as written.
query_field_search = Search “{ $search }”

# A row of the address field: the words typed searched in what the files hold.
query_field_search_contents = Contents

# A row of the address field: the same search on the whole disk.
query_field_search_disk = Whole disk

# A row of the address field: the words typed after @ searched in names, as written.
query_field_search_for = Search for “{ $words }”

# A row of the address field: words written #tag searched in names, not as tags.
query_field_search_names = Names

# In the address field, in place of the search: a key typed has no value yet (kind:).
query_field_type_value = Type a value to search

# Beside a search the address field offers: where it looks.
query_field_whole_disk = on the whole disk

# Beside a search the address field offers: where it looks. place: a place with places within it (a tag holding tags).
query_field_within = among every file within { $place }

query_filter = Filter folder

query_filter_description = Change one facet of what filters the pane (the words typed over it in the address field, its lens), as `query.refine` changes a search's; the location stays shown

# What the field filtering the tags to choose holds while empty.
query_filter_tags = Filter tags

# Why a saved place's filter was not changed: another saved place is that.
query_filter_taken = this filter is already saved as { $name }

# A submenu of a file's menu: searches for files like the one selected (same kind, size, date).
query_find_similar_submenu = Find Similar

query_forget = Remove saved search

query_forget_description = Take a saved search out of the sidebar, named by its search (a saved place by its filter and `at`) or its name

query_forget_recent = Forget a recent search

query_forget_recent_description = Take a search out of those the address field offers as run lately

# A group's heading, grouped by size. unit and next: units of size, as sizes are shown (kB and MB).
query_group_100_to_1 = 100 { $unit } to 1 { $next }

# A group's heading, grouped by a picture's dimensions: MP, megapixels.
query_group_12_to_24_mp = 12 to 24 MP

# A group's heading, grouped by size. unit: a unit of size, as sizes are shown (MB, MiB).
query_group_1_to_100 = 1 to 100 { $unit }

# A group's heading, grouped by a picture's dimensions: MP, megapixels.
query_group_1_to_4_mp = 1 to 4 MP

# A group's heading, grouped by how long a sound or a film lasts.
query_group_1_to_5_minutes = 1 to 5 minutes

# A group's heading, grouped by how long a sound or a film lasts.
query_group_20_minutes_to_1_hour = 20 minutes to 1 hour

# A group's heading, grouped by a picture's dimensions: MP, megapixels.
query_group_4_to_12_mp = 4 to 12 MP

# A group's heading, grouped by how long a sound or a film lasts.
query_group_5_to_20_minutes = 5 to 20 minutes

# A group's heading, grouped by name: names that begin with a digit.
query_group_digits = 0–9

# A group's heading, grouped by a date: this month, before last week.
query_group_earlier_this_month = Earlier this month

# A group's heading, grouped by a date: this week, before yesterday.
query_group_earlier_this_week = Earlier this week

# A group's heading, grouped by a date: this year, before last month.
query_group_earlier_this_year = Earlier this year

# A group's heading, grouped by size: empty files.
query_group_empty = Empty

# A group's heading: the folders, which have none of what the files are grouped by.
query_group_folders = Folders

# A group's heading, grouped by a date: dates after today.
query_group_future = In the future

# A group's heading, grouped by a date.
query_group_last_month = Last month

# A group's heading, grouped by a date.
query_group_last_week = Last week

# A group's heading, grouped by a date: what says none.
query_group_no_date = No date

# A group's heading, grouped by a picture's dimensions: what says none.
query_group_no_dimensions = No dimensions

# A group's heading, grouped by how long a sound or a film lasts: what says none.
query_group_no_duration = No duration

# A group's heading, grouped by extension.
query_group_no_extension = No extension

# A group's heading, grouped by folder: what is in no folder (a place).
query_group_no_folder = No folder

# A group's heading, grouped by owner: what says none.
query_group_no_owner = No owner

# A group's heading, grouped by tags: files with none.
query_group_no_tags = No tags

# A group's heading, grouped by kind: files of no kind (images, documents…).
query_group_other_kind = Other

# A group's heading, grouped by name, after the letters' groups: names that begin with a letter no heading is for (a Japanese name's kanji, a Han character in an order other than Chinese's).
query_group_other_letters = Other letters

# A group's heading, grouped by name: names that begin with neither a digit nor a letter.
query_group_other_names = Other

# A group's heading, grouped by size. unit: a unit of size, as sizes are shown (GB, GiB).
query_group_over_1 = Over 1 { $unit }

# A group's heading, grouped by how long a sound or a film lasts.
query_group_over_1_hour = Over 1 hour

# A group's heading, grouped by a picture's dimensions: MP, megapixels.
query_group_over_24_mp = Over 24 MP

# A group's heading, grouped by Git state: files nothing was said of (in no repository, not read yet).
query_group_state_unknown = Not known

# A group's heading, grouped by name in Traditional Chinese's order, which heads Han characters by how many strokes they are written in (1劃, 2劃…).
query_group_strokes = { $strokes ->
        [one] { $strokes } stroke
       *[other] { $strokes } strokes
    }

# A group's heading, grouped by a date.
query_group_today = Today

# A group's heading, grouped by Git state: files as the last commit has them.
query_group_unchanged = Unchanged

# A group's heading, grouped by size. unit: a unit of size, as sizes are shown (kB, KiB).
query_group_under_100 = Under 100 { $unit }

# A group's heading, grouped by how long a sound or a film lasts.
query_group_under_1_minute = Under 1 minute

# A group's heading, grouped by a picture's dimensions: MP, megapixels.
query_group_under_1_mp = Under 1 MP

# A group's heading, grouped by type: files whose type is not known.
query_group_unknown_type = Unknown type

# A group's heading, grouped by rating: files with no stars.
query_group_unrated = Unrated

# A group's heading, grouped by a date.
query_group_yesterday = Yesterday

query_here = This folder only

query_here_description = Show the pane's search as the folder it looks in, filtered by its words, when the folder alone can tell them; else search that folder alone. A folder stays as it is

# The home folder, as a search's chip and a saved search's name say it.
query_home = Home

query_interpret = Read words as a search

# shape: what it answers, written as JSON.
query_interpret_description = { $shape } or null: plain words read as terms by rules (periods and years, kinds, extensions, sizes, the pane's tags), the rest kept as names; never applied by itself

# The pane's keep button over a filtered place: keeps it with its filter in the sidebar's Saved.
query_keep_place = Save with this filter

# The pane's keep button over a search: keeps it in the sidebar's Saved.
query_keep_search = Save this search

# What the key accessed: of a search tests.
query_key_accessed = Last opened

# What the option case: of a search sets.
query_key_case = Capitals told apart, or not

# What the key comment: of a search tests: words a file's comment holds.
query_key_comment = With a comment holding

# The values of the key comment: of a search.
query_key_comment_example = words in a file's comment

# What the key content: of a search tests: text the files hold.
query_key_content = Holding text

# The values of the key content: of a search: words in quotes, or a pattern between slashes; keep the quotes and the slashes.
query_key_content_example = "words" or /regex/

# What the key created: of a search tests: when files were made.
query_key_created = Made

# What the option depth: of a search sets.
query_key_depth = How deep to look

# The values of the option depth: of a search: how many folders down it looks.
query_key_depth_example = how many folders down it looks: 2

# What the option excluded: of a search sets: whether it looks in the folders searches leave out.
query_key_excluded = Excluded folders too

# The values of the option excluded: of a search; yes and no are its values, as written.
query_key_excluded_example = what your exclusions leave out too: { $yes }, { $no }

# What the key ext: of a search tests.
query_key_ext = With an extension

# What the option hidden: of a search sets.
query_key_hidden = Hidden files too, or never

# What the option in: of a search sets.
query_key_in = Where to look

# The values of the option in: of a search; "~/My Files" is a folder written as the search language writes it.
query_key_in_example = a folder, "~/My Files"

# What the key inside: of a search tests: where a picture was taken.
query_key_inside = Made inside a box

# The values of the key inside: of a search: the box's edges, in that order, separated by commas as the search language writes them.
query_key_inside_example = south,west,north,east

# What the key kind: of a search tests, in its reference and beside it when offered.
query_key_kind = Of a kind

# What the key mime: of a search tests: a file's type (image/png).
query_key_mime = Of a type

# What the key modified: of a search tests: when files were last changed.
query_key_modified = Modified

# What the key name: of a search tests.
query_key_name = Named

# What the key near: of a search tests: where a picture was taken.
query_key_near = Made near a point

# What the key rating: of a search tests.
query_key_rating = Rated

# What the key size: of a search tests.
query_key_size = Of a size

# What the key tag: of a search tests.
query_key_tag = With a tag

# The values of the key tag: of a search; * and #tag are written as the search language writes them: keep them.
query_key_tag_example = a tag, or * for any; #tag too

# What the key taken: of a search tests: when a picture or a video was taken.
query_key_taken = Taken, pictures and videos

# The values of the key taken: of a search; 2023 and last-year are written as the search language writes them: keep them.
query_key_taken_example = 2023, last-year, by the camera's clock

# What the key type: of a search tests.
query_key_type = Files, folders or links

query_list = List saved searches

query_list_description = Return the saved searches, in sidebar order

query_location = Search location

# shape: what it answers, written as JSON.
query_location_description = { $shape }: the search for one facet's value (`tag` `work`, or `tag` alone for any tag) from `root` or the home folder, for whoever shows or runs a search without writing its syntax

# An error in a search's text. term: the term typed, as written (size<0).
query_matches_nothing = { $term } matches nothing

# An error in a search's text: a parenthesis opened and never closed.
query_missing_paren = missing `)`

query_move = Move saved search

query_move_description = Put a saved search before another in the sidebar, or last

# The Name chip's choice once set: asks for other words.
query_name_change = Change…

# The title of what asks for words the names hold.
query_name_contains = Name contains

# The Name chip's choice: asks for words the names hold.
query_name_contains_menu = Name contains…

# What the field asking for words of names holds while empty; invoice *.pdf is an example as written.
query_name_placeholder = Words or patterns: invoice *.pdf

# The name made for a saved search: what it looks for, as the search writes it (*.pdf modified<7d), and the folder it looks in.
query_named_in = { $what } in { $place }

query_narrow = Change the lens

query_narrow_description = Change one facet of the pane's lens (ADR-107: what of its filter or its search the listing can decide, its files of a kind, a year, with or without a tag), kept as the pane shows other locations until taken off: a search's in place of it, anything else's filter; without `facet`, take the lens off

# facet: the facet asked, as a command names it (tag, kind).
query_needs_value = `{ $facet }` needs a value

# An error of a choice of a search's chip: facet, as given, is no chip's.
query_no_facet = no facet `{ $facet }`

# An error of a value given a search's Kind chip: kind, as given, is no kind: of the search language.
query_no_kind = no kind `{ $kind }`

query_no_pane = no such pane

# pane: a pane's number, as agents name it.
query_no_pane_numbered = no pane { $pane }

# An error of a choice of a search's chip: part, as given, numbers no part of the search.
query_no_part = no part `{ $part }`

query_no_saved_named = no saved search named { $name }

query_no_search_no_filter = the pane shows neither a search nor a filter

query_no_search_no_folder = the pane shows no search and no folder

query_no_tags_among = No tags among these files

# An error of Change the lens. facet: the facet asked, as a command names it (content, taken, words).
query_not_a_lens = a lens is what is known of the files: `{ $facet }` is a search's (what they contain, when a picture was taken, the names)

# An error in a search's text. key: a key of the search language, as written; value: the point typed, not one on the Earth.
query_not_a_place = { $key } `{ $value }` is not a place

# An error in a search's text. key: a key of the search language, as written.
query_not_allowed = { $key } is not allowed here

# An error in a search's text. key: a key of the search language, as written; value: the distance typed.
query_not_on_earth = { $key } a distance of `{ $value }` is not one on the Earth

# An error of a value given a search's chip. term: the term it makes, as written.
query_not_one_condition = `{ $term }` is not one condition

# A tag chip's choice: the files without the tag, rather than those with it.
query_not_tagged_instead = Not tagged so instead

# An error of a search whose in: names another search.
query_not_within_search = a search cannot look within another search

# A search's pane when it found no file.
query_nothing_found = Nothing found

# Under Nothing found, in a search's pane. text: the words searched for, as typed.
query_nothing_found_detail = No file matches “{ $text }” here. Check the spelling or search a folder higher up.

# An error of a search whose in: names what no plugin opens. place: what it names, as written.
query_nothing_opens = nothing opens { $place }: a folder is written as its whole path

query_nothing_to_save = the pane shows nothing to save

query_only_folders_below = the pane shows no folder: only a folder's subfolders can be searched

# An error in a search's text. key: a key of the search language, as written; \xNN is how such a byte is written: keep it.
query_only_in_bytes = only { $key } accepts bytes that are not UTF-8 (\xNN)

query_period_last_month = Last month

query_period_last_week = Last week

query_period_last_year = Last year

query_period_older = Older than a year

query_period_this_month = This month

query_period_this_week = This week

query_period_this_year = This year

# A period files were changed in, offered by a search's chip.
query_period_today = Today

query_period_yesterday = Yesterday

query_place_changed = Changed the filter of { $name }

# A saved place put elsewhere in the sidebar's Saved.
query_place_moved = Moved { $name }

# The name made for a saved place (ADR-109): the place, then what filters it (Pictures · kind:image).
query_place_named = { $place } · { $filter }

query_place_needs_filter = a saved place needs a filter

query_place_not_below = a place is searched as it lists, never below it

# A place with what filters it, not kept in the sidebar's Saved.
query_place_not_saved = This is not saved

# Saved: the sidebar's section of saved searches and places.
query_place_removed = Removed { $name } from Saved

# A saved place renamed in the sidebar's Saved.
query_place_renamed = Renamed { $name } to { $new }

# Said once a place was kept with what filters it in the sidebar's Saved.
query_place_saved = Saved { $name }

# A place with what filters it, kept already in the sidebar's Saved.
query_place_saved_already = This is already saved as { $name }

# What a search's answer rests on, said in its footer: the index plocate keeps of every file's name.
query_plocate_database = plocate's database

query_plugin_description = Search as a location: name, size, date, tag, MIME, and content queries; saved searches in the sidebar

query_plugin_title = Search

# A rating a search offers.
query_rating_five = Five stars

# A rating a search offers: files not rated.
query_rating_none = Unrated

# A rating a search offers.
query_rating_one = One star or more

# A rating a search offers.
query_rating_three = Three stars or more

# Plain words read as a search, offered below them (Did you mean: PDFs modified last week). subject: what files, a list (Images and videos); each other part is said beginning with a space, or is nothing.
query_reading = { $subject }{ $named }{ $tagged }{ $year }{ $period }{ $size }

query_reading_archives = { $at ->
        [first] Archives
       *[after] archives
    }

query_reading_audio = { $at ->
        [first] Music and audio
       *[after] music and audio
    }

query_reading_documents = { $at ->
        [first] Documents
       *[after] documents
    }

# The end of a reading of words as a search (Files that are empty): it begins with a space. of: what the files named before it are, for these words to agree with (as query_reading_named).
query_reading_empty = { $of ->
        [extension] {" "}that are empty
        [folder] {" "}that are empty
        [image] {" "}that are empty
        [video] {" "}that are empty
        [pictures] {" "}that are empty
        [audio] {" "}that are empty
        [document] {" "}that are empty
        [spreadsheet] {" "}that are empty
        [presentation] {" "}that are empty
        [text] {" "}that are empty
        [archive] {" "}that are empty
        [font] {" "}that are empty
        [program] {" "}that are empty
        [several] {" "}that are empty
       *[file] {" "}that are empty
    }

# Files of an extension, as a reading of words as a search names them (PDFs modified last week). ext: the extension in capitals.
query_reading_extension = { $ext }s

# The start of a reading of words as a search that names no kind (Files modified today).
query_reading_files = Files

# Files of a kind, as a reading of words as a search names them: first (at the start of the sentence) or after another kind.
query_reading_folders = { $at ->
        [first] Folders
       *[after] folders
    }

query_reading_fonts = { $at ->
        [first] Fonts
       *[after] fonts
    }

query_reading_images = { $at ->
        [first] Images
       *[after] images
    }

# A part of a reading of words as a search (PDFs modified last week): it begins with a space. when: a period (query_reading_when_*) or a year (query_reading_when_year); of: as in query_reading_named.
query_reading_modified = { $of ->
        [extension] {" "}modified { $when }
        [folder] {" "}modified { $when }
        [image] {" "}modified { $when }
        [video] {" "}modified { $when }
        [pictures] {" "}modified { $when }
        [audio] {" "}modified { $when }
        [document] {" "}modified { $when }
        [spreadsheet] {" "}modified { $when }
        [presentation] {" "}modified { $when }
        [text] {" "}modified { $when }
        [archive] {" "}modified { $when }
        [font] {" "}modified { $when }
        [program] {" "}modified { $when }
        [several] {" "}modified { $when }
       *[file] {" "}modified { $when }
    }

# A part of a reading of words as a search (PDFs named “invoice”): it begins with a space. names: the words typed, as written. of: what the files named before it are, for these words to agree with: extension (PDFs, JPGs), one kind (folder, image, video, audio: music and audio, document, spreadsheet, presentation, text: text and code, archive, font, program), pictures (Images and videos), several (other kinds of several, as Images and documents, of any gender), file (any kind).
query_reading_named = { $of ->
        [extension] {" "}named { $names }
        [folder] {" "}named { $names }
        [image] {" "}named { $names }
        [video] {" "}named { $names }
        [pictures] {" "}named { $names }
        [audio] {" "}named { $names }
        [document] {" "}named { $names }
        [spreadsheet] {" "}named { $names }
        [presentation] {" "}named { $names }
        [text] {" "}named { $names }
        [archive] {" "}named { $names }
        [font] {" "}named { $names }
        [program] {" "}named { $names }
        [several] {" "}named { $names }
       *[file] {" "}named { $names }
    }

# The end of a reading of words as a search (Videos over 1 GB): it begins with a space. unit: a unit of size as the size setting writes it.
query_reading_over_1 = {" "}over 1 { $unit }

# The end of a reading of words as a search (PDFs over 100 MB): it begins with a space. unit: a unit of size as the size setting writes it.
query_reading_over_100 = {" "}over 100 { $unit }

query_reading_presentations = { $at ->
        [first] Presentations
       *[after] presentations
    }

query_reading_programs = { $at ->
        [first] Programs
       *[after] programs
    }

query_reading_spreadsheets = { $at ->
        [first] Spreadsheets
       *[after] spreadsheets
    }

# A part of a reading of words as a search (Images tagged “work”): it begins with a space. tags: the tags' names, a list; count: how many; of: as in query_reading_named.
query_reading_tagged = { $of ->
        [extension] {" "}tagged
        [folder] {" "}tagged
        [image] {" "}tagged
        [video] {" "}tagged
        [pictures] {" "}tagged
        [audio] {" "}tagged
        [document] {" "}tagged
        [spreadsheet] {" "}tagged
        [presentation] {" "}tagged
        [text] {" "}tagged
        [archive] {" "}tagged
        [font] {" "}tagged
        [program] {" "}tagged
        [several] {" "}tagged
       *[file] {" "}tagged
    } { $count ->
        [one] { $tags }
       *[other] { $tags }
    }

# A part of a reading of words as a search (Images taken in 2023), said of pictures and videos: it begins with a space. when: a year (query_reading_when_year); of: as in query_reading_named (image, video, pictures, or extension for JPGs and the like).
query_reading_taken = { $of ->
        [extension] {" "}taken { $when }
        [folder] {" "}taken { $when }
        [image] {" "}taken { $when }
        [video] {" "}taken { $when }
        [pictures] {" "}taken { $when }
        [audio] {" "}taken { $when }
        [document] {" "}taken { $when }
        [spreadsheet] {" "}taken { $when }
        [presentation] {" "}taken { $when }
        [text] {" "}taken { $when }
        [archive] {" "}taken { $when }
        [font] {" "}taken { $when }
        [program] {" "}taken { $when }
        [several] {" "}taken { $when }
       *[file] {" "}taken { $when }
    }

query_reading_text = { $at ->
        [first] Text and code
       *[after] text and code
    }

# The end of a reading of words as a search (PDFs under 100 kB): it begins with a space. unit: a unit of size as the size setting writes it.
query_reading_under_100 = {" "}under 100 { $unit }

query_reading_videos = { $at ->
        [first] Videos
       *[after] videos
    }

query_reading_when_last_month = last month

query_reading_when_last_week = last week

query_reading_when_last_year = last year

query_reading_when_older = over a year ago

query_reading_when_this_month = this month

query_reading_when_this_week = this week

query_reading_when_this_year = this year

# When files were modified, placed in query_reading_modified (PDFs modified today).
query_reading_when_today = today

# When files were modified or taken, placed in query_reading_modified or query_reading_taken: a year, as written (2023).
query_reading_when_year = in { $year }

query_reading_when_yesterday = yesterday

# What brackets mean in a name searched: keep […] first.
query_ref_brackets = […] one of these

# What content:"two words" means in a search.
query_ref_content = Files holding these words

# What content:/to.?do/ means in a search: a regular expression.
query_ref_content_regex = Files holding what a pattern matches

# A heading of what a search takes: what the files hold (content:).
query_ref_contents = Contents

# What modified:2026-05 means in a search; the dates are written as the search language writes them.
query_ref_date = A year, a month or a day: 2026, 2026-05-14

# What a range of dates means in a search (modified:2026-01..2026-03): both ends included, whole (January to the end of March).
query_ref_date_range = From the start of the first to the end of the second

# What modified<7d means in a search, and the units of time it reads: each unit as written (m, h, d…), then what it counts.
query_ref_duration = Within the last 7 days: { $m } minutes, { $h } hours, { $d } days, { $w } weeks, { $mo } months, { $y } years

# What the flags after a pattern mean in a search (content:/todo/i); i and c are the flags, as written.
query_ref_flags = { $i }: capitals or not; { $c }: capitals told apart

# What a key of a search means, then examples of its values (Of a kind: image, video, document…).
query_ref_key = { $means }: { $example }

# A note under the signs of a pattern; ^ and $ are signs as written.
query_ref_line = ^ and $ are the start and the end of a line.

# What a - before a term means in a search; the terms are examples as written.
query_ref_minus = Not this: -draft, -#tag, -kind:image

# A heading of what a search takes: words and wildcards matched against names.
query_ref_names = Names

# What NOT means in a search; NOT a and -a are examples as written.
query_ref_not = Not: NOT a, also written -a

# What modified>1y means in a search.
query_ref_older = Longer ago than a year

# A heading of what a search takes: the options (in:, depth:, hidden:…).
query_ref_options = Where and how it looks

# What OR means in a search; a OR b is an example as written: keep OR.
query_ref_or = Either: a OR b

# What parentheses mean in a search; (a OR b) c is an example as written.
query_ref_parens = Together: (a OR b) c

# What modified:today means in a search: a period. periods: the other periods it reads, as written, a list.
query_ref_period = A period: { $periods }

# What ? means in a name searched: keep ? first.
query_ref_question = ? one character

# What quotes mean in a search: what they hold is a name as written; the examples are written as the search language writes them.
query_ref_quotes = As written: "two words", "OR", "#3"

# What rating>=3 means in a search; the signs and rating:0 are written as the search language writes them.
query_ref_rating = At least (> < >= <=); rating:0 finds files with no rating

# What size>10MB means in a search, and the units of size it reads. mb: a unit as the size setting writes it; decimal (kB, MB…), binary (KiB, MiB…) and alone (k, M…): lists of units as written.
query_ref_size = More than 10 { $mb }. Each of { $decimal } is 1000 times the one before; each of { $binary } is 1024 times, and so is each of { $alone } written alone.

# What a range of sizes means in a search (size:1MB..100MB): both ends included.
query_ref_size_range = From one size to the other, both included

# What \/ means in a pattern between slashes.
query_ref_slash = A slash itself: one alone ends the pattern

# What * means in a name searched: keep * first.
query_ref_star = * any characters

# A heading of what a search takes (F1 in the address field): the keys that test files.
query_ref_terms = Terms

# The heading of what a search takes (F1 in the address field).
query_ref_title = Search

# A heading of what a search takes: how terms go together; two terms side by side both hold.
query_ref_together = Together (side by side: both)

# A heading of what a search takes: the values sizes, times and ratings take.
query_ref_values = Values

# What a word means in a search (report): the names holding it.
query_ref_word = Names holding it, capitals or not

query_reference = Say what a search takes

query_reference_description = What a search's text takes (ADR-137): its terms and options with what each means and examples, how terms go together, values, names' wildcards and what `content:` reads, regex signs included; each item as written, what it says and what choosing it writes

query_refine = Refine search

query_refine_description = Change one facet of the pane's search (or start one in its folder, its lens added) and show the result: a change of its lens alone in place of it, any other as a step Back undoes; its last term taken away, the pane shows the place it searched

query_remember_setting = Remember recent searches

query_remember_setting_description = The address field offers the searches run from it lately, kept in the cache; turned off, they are forgotten.

# The choice of a chip holding a part of a search no other chip holds: takes it out.
query_remove_part = Remove

query_rename = Rename saved search

# The button that renames a saved search or place.
query_rename_accept = Rename

query_rename_description = Change the name a saved search shows in the sidebar

# The field where a saved place is renamed, while empty.
query_rename_place_placeholder = Name (empty: made from the filter)

# The title of the dialog renaming a saved place.
query_rename_place_title = Rename

# The field where a saved search is renamed, while empty.
query_rename_search_placeholder = Name (empty: made from the search)

query_rename_search_title = Rename saved search

# What stops working without plocate, on the Plugins page.
query_requirement_plocate = Instant name searches over whole disks (a directory walk is used otherwise)

# An error of a search whose folder is not written whole (from /). path: the folder, as written.
query_root_not_absolute = query root { $path } is not an absolute path

# A search's pane whose folder cannot be opened. path: the folder; error: why (permission denied).
query_root_not_folder = query root { $path } is not a folder: { $error }

# An error of a command given a root that is no folder's path: root is the argument's name.
query_root_not_path = `root` must be a path

query_save = Save search

query_save_description = Keep a search in the sidebar's Saved, or with `at` a place and what filters it there, shown so when opened (ADR-109); what the pane shows when `expr` is omitted

query_saved_not_changed = The saved searches were not changed: { $error }

query_saved_not_written = the saved searches were not written

# Read by agents: the setting holding the saved searches. shape: how each is written, as written.
query_saved_setting = Saved searches shown in the sidebar, in order: { $shape }

query_saved_unreadable = the saved searches in the settings file do not read; nothing was written

query_search = Search

# Said once what a saved search looks for was edited.
query_search_changed = Changed saved search { $name }

# Read by agents and in F1's list of actions. What is between backticks is written as a search writes it: keep it as it is.
query_search_description = Search, shown in the pane, the pane's lens added unless `as_is` (ADR-107: the terms of a filter or a search the listing can decide, kept as the pane goes elsewhere until taken off; then `entries` lists what it found; a search the pane shows already is left as it is, `refresh` lists it again): words match names; `name:`, `ext:pdf,md`, `kind:image,video` (document, text, audio, archive…), `type:dir`, `tag:x`, `content:words` or `content:/regex/`, `comment:`, `size>10M`, `modified<7d` (also `today`, `this-week`, `2026-05`; `created`, `accessed`, and `taken` for a picture's camera clock), `rating>=4`, `near:lat,lon~5km`; `OR`, `NOT` or `-`, parentheses; `hidden:yes` looks in hidden folders, `excluded:yes` in the folders searches leave out (Settings › General › Searches: `node_modules`, caches; never a trash nor a network folder), `depth:2` bounds it. `root` (or `in:`) is where it looks

# A context menu's item over a folder's background: opens the search in the address field.
query_search_here = Search here…

# Said once a saved search was put elsewhere in the sidebar's list.
query_search_moved = Moved saved search { $name }

query_search_not_saved = This search is not saved

query_search_removed = Removed saved search { $name }

query_search_renamed = Renamed saved search { $name } to { $new }

query_search_saved = Saved search { $name }

query_search_saved_already = This search is already saved as { $name }

# Why a saved search was not changed: another saved search looks for that.
query_search_taken = this search is already saved as { $name }

query_similar = Find similar

# An item of the context menu's Find Similar: files modified the same day.
query_similar_day = Same Day

query_similar_description = Search for files like the selected one: of the same kind, modified the same day, of a similar size, or sharing a tag, in the folder searched or shown

# An item of the context menu's Find Similar: files of the same kind.
query_similar_kind = Same Kind

# Why Find Similar › Same Day found nothing to search by.
query_similar_no_date = { $name } has no modification date

# Why Find Similar › Same Kind found nothing to search by.
query_similar_no_kind = { $name } is of no kind the desktop knows

# Why Find Similar › Similar Size found nothing to search by.
query_similar_no_size = { $name } has no size

# Why Find Similar › Same Tags found nothing to search by.
query_similar_no_tags = { $name } has no tags

# An error of Find Similar given several files or none.
query_similar_one = select one file to find others like it

# An item of the context menu's Find Similar.
query_similar_size = Similar Size

# An item of the context menu's Find Similar: files sharing a tag.
query_similar_tags = Same Tags

# Why Find Similar found nothing: the file selected cannot be read.
query_similar_unreadable = { $name } cannot be read

# A size a search offers; unit: a unit of size as the size setting writes it (MB, MiB).
query_size_1_to_100 = 1 to 100 { $unit }

# A size a search offers: files of no bytes.
query_size_empty = Empty

# A size a search offers; unit: a unit of size as the size setting writes it (GB, GiB).
query_size_over_1 = Over 1 { $unit }

# A size a search offers; unit: a unit of size as the size setting writes it (MB, MiB).
query_size_over_100 = Over 100 { $unit }

# What the field asking for a size holds while empty; the examples are written as the search language writes them, m and k units of size as the size setting writes them (MB, kB).
query_size_placeholder = A size or a range: >10{ $m }, 1{ $m }..5{ $m }, <500{ $k }

# A size a search offers; unit: a unit of size as the size setting writes it (MB, MiB).
query_size_under_1 = Under 1 { $unit }

# A size a search offers; unit: a unit of size as the size setting writes it (kB, KiB).
query_size_under_100 = Under 100 { $unit }

# A tag chip's choice: goes up to the category tag, every tag within it.
query_tag_all_of = All of { $tag }

query_tag_choices = Tags to refine by

query_tag_choices_description = The Tag chip's picker: the tags on the pane's files, read from them, each refining the search (or what `refines` names: the folder's `narrowing` or `filter`)

# Beside a tag to choose: how many of the files shown have it.
query_tag_files = { $count ->
        [one] { $count } file
       *[other] { $count } files
    }

# The heading of the choices that are no tag of the files.
query_tag_section_other = Other

# The title of what asks for, or chooses, a tag a search's files have.
query_tagged = Tagged

# A Not tagged chip's choice: the files with the tag, rather than those without it.
query_tagged_instead = Tagged instead

# The Tag chip's choice: chooses or asks for a tag the files have.
query_tagged_menu = Tagged…

# A term the address field offers to add to a search: text files hold.
query_term_contains = Contains

# A term the address field offers to add to a search before anything is typed: of a kind.
query_term_kind = Kind

# A term the address field offers to add to a search: when files were last changed.
query_term_modified = Modified

# A term the address field offers to add to a search: how big files are.
query_term_size = Size

# A term the address field offers to add to a search: a tag files have.
query_term_tag = Tag

# A term the address field offers to add to a search: when pictures were taken.
query_term_taken = Taken

# The Contains chip's choice: asks for text the files hold.
query_text_in_file = Text in the file…

# Where a search looks: in the folder, not the folders within it.
query_this_folder_only = This folder only

query_tokens = Pieces of a search

# shape: what it answers, written as JSON.
query_tokens_description = { $shape }: the pieces of a search's text as the parser reads them (word, key, value, option, operator, or an error with the parser's message), byte ranges; a term still being typed at `caret` is not an error yet

# An error in a search's text: a closing parenthesis with none opened.
query_unexpected_paren = unexpected `)`

# The pane's keep button over a saved place: takes it out of the sidebar's Saved.
query_unkeep_place = Remove from Saved

# The pane's keep button over a saved search: takes it out of the sidebar's Saved.
query_unkeep_search = Remove saved search

# An error in a search's text: what follows content:/…/ is no flag. key: a key of the search language; flag: what was typed; i and c: the flags, as written.
query_unknown_flag = { $key } unknown regex flag `{ $flag }` ({ $i }: capitals or not, { $c }: capitals told apart; quote text that starts with /)

# An error in a search's text: a quote is opened and never closed.
query_unterminated_quote = unterminated quote

# A value of the key type: of a search, offered as it is typed: folders.
query_value_dir = Dir

# A value of an option of a search, offered as it is typed: no.
query_value_false = False

# A value of the key type: of a search, offered as it is typed: files.
query_value_file = File

# A value of the option case: of a search, offered as it is typed: capitals or not.
query_value_insensitive = Insensitive

# A value of the option case: of a search, offered as it is typed: capitals told apart.
query_value_sensitive = Sensitive

# A value of the key type: of a search, offered as it is typed: links.
query_value_symlink = Symlink

# A value of an option of a search, offered as it is typed: yes.
query_value_true = True

# What a filter leaves to a search (Enter), an item of a list placed after “Only a search can tell” (omnibox_waits).
query_waits_contents = what files contain

# What a filter leaves to a search (Enter), an item of a list placed after “Only a search can tell” (omnibox_waits).
query_waits_places = where pictures were taken

# What a filter leaves to a search (Enter), an item of a list placed after “Only a search can tell” (omnibox_waits).
query_waits_taken = when pictures were taken

# What the field asking for a date holds while empty; the examples are written as the search language writes them: keep them.
query_when_placeholder = A date, a range or a period: 2026-05, 2026-01-01..2026-03-31, <30d

# Said by the where chip of a search that looks in the folders searches leave out too.
query_where_excluded = excluded

# The where chip's choice, on or off: look in the folders searches leave out too (Settings › Exclusions).
query_where_excluded_files = Excluded files and folders

# Said by the where chip of a search that looks at hidden files too.
query_where_hidden = hidden files

# Said by the where chip of a search that looks at hidden files and in the folders searches leave out too.
query_where_hidden_excluded = hidden files, excluded

# The where chip's choice, on or off: look at hidden files too.
query_where_hidden_files = Hidden files

# The where chip's choice: search the folder and the folders within it.
query_where_subfolders = This folder and its subfolders

# An error of a command: expr and name are its arguments' names, never translated.
query_which_saved = which saved search? Pass its `expr` or its `name`
