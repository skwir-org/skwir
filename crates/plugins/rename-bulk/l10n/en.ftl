# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# The date's format as typed after {date: in the template, then the date library's explanation, as written.
rename_bulk_bad_date_format = bad date format `{ $format }`: { $why }

# What was typed after {n: in the template, which is no number of digits.
rename_bulk_bad_width = bad counter width `{ $width }`

# The arguments are what is written in the template for each brace, as written.
rename_bulk_brace = A brace itself ({ $open } and { $close })

# Working out a bulk rename's names was stopped: what was typed since asks again.
rename_bulk_cancelled = cancelled

rename_bulk_command = Rename several from a template

# Read by agents and in F1's list of actions.
rename_bulk_command_description = Rename the selection, or the entries given, from a template, regex, and case rule (plan)

rename_bulk_conflict = { $item } would become { $name }, which already exists

# A heading of the template's reference: the number counted for each item.
rename_bulk_counter = Counter

# The counter's start and step make a number too large for the items.
rename_bulk_counter_overflows = the counter overflows

# The counter's width typed in the template, and the most a name holds, in bytes.
rename_bulk_counter_too_wide = { $width ->
        [one] a counter { $width } digit wide is wider than a name can be ({ $most } at most)
       *[other] a counter { $width } digits wide is wider than a name can be ({ $most } at most)
    }

# Start is the name of the dialog's field the counter starts at.
rename_bulk_counts = Counts from Start, one more for each, in the order shown

# The placeholder and the signs after % are the template's, as written.
rename_bulk_date_note = Inside { $placeholder }: %Y the year, %m the month, %d the day, %H the hour, %M the minute, %S the second, %B the month's name (%b short), %A the day's, %j the day of the year. In this system's time zone.

rename_bulk_day = Its day

rename_bulk_day_and_time = Its day and time

rename_bulk_day_first = Its day, the day first

rename_bulk_day_without_dashes = Its day, without dashes

rename_bulk_dollar = A dollar sign

# The new name several files would take, then the first three of them, and how many more.
rename_bulk_duplicate = { $count ->
        [2] several items would become { $name }: { $first }, { $second }
        [3] several items would become { $name }: { $first }, { $second }, { $third }
       *[other] several items would become { $name }: { $first }, { $second }, { $third } and { $more } more
    }

# Of the replacement field.
rename_bulk_empty_replacement_note = Left empty, what the pattern matched is taken away.

rename_bulk_extension = The extension, without its dot

rename_bulk_extension_with_dot = The extension with its dot; nothing when there is none

# Said beside $1 in the replacement's reference; $2 and $3 are signs, as written.
rename_bulk_first_group = What the first group held ($2, $3…)

# Said beside ${1}: the first group's, written so a letter, a digit or an underscore may follow.
rename_bulk_group_before_letter = The same, before a letter, a digit or _

# The arguments are what is written in the replacement, as written.
rename_bulk_group_name_note = { $written } reads as a group named { $read }, which holds nothing: write { $meant }.

# A file's name, the name it would get, and why that name is not allowed.
rename_bulk_invalid_name = { $item } would become an invalid name { $name }: { $reason }

# Said under the regex's field; what follows is the regex library's own explanation, as written.
rename_bulk_invalid_regex = invalid regex: { $why }

# Said under the template's field; what follows says what is wrong.
rename_bulk_invalid_template = invalid template: { $why }

# A context menu item: renames the files selected together, from a template.
rename_bulk_menu = Rename Several…

# The placeholder is the template's, as written: its modification date.
rename_bulk_missing_date = { $item } has no modification time yet (needed for { $placeholder })

rename_bulk_month_and_year = Its month and year

rename_bulk_name_without_extension = The name without its extension

# Said beside ${year}: year is the group's name, as written in the pattern.
rename_bulk_named_group = What the group named year held

# An error an agent or a script reads: the pane shows no folder yet.
rename_bulk_no_location = pane has no open location

# An error an agent or a script reads: the pane it named is not there.
rename_bulk_no_such_pane = no such pane

rename_bulk_not_read_yet = placeholder `{ $placeholder }` not read yet

# A name whose bytes are not text: the rules work on text, so it can only be renamed alone.
rename_bulk_not_text = { $item } is not a name made of text; rename it on its own

rename_bulk_nothing = nothing to rename

rename_bulk_parent = The folder it is in

rename_bulk_parts_note = The regex runs first: these are the name it leaves. The case comes last, and leaves the extension as it is. A folder has no extension.

# A heading of the template's reference.
rename_bulk_parts_of_the_name = Parts of the name

# A bulk rename's title, in the review, Activity and the History. Begins in lower case: said inside other words too (undo rename 3 items).
rename_bulk_plan = { $count ->
        [one] rename { $count } item
       *[other] rename { $count } items
    }

rename_bulk_plugin_description = Rename the selection from a template, a regex, and case rules

rename_bulk_plugin_title = Bulk rename

rename_bulk_preview = Preview bulk rename

# Read by agents: the words in backticks are the answer's fields and the command's arguments, as written.
rename_bulk_preview_description = Return the old → new names a bulk rename would produce (`renames`, the first `limit` of them when given; each with where the regex `matched` in the old name and what it `put` in the new one, byte ranges), how many there are (`count`), and the first item's name (`first`), which the regex's and the replacement's references work on alone

rename_bulk_recipe_brackets = Take away what is in brackets

rename_bulk_recipe_jpeg = One extension for JPEG pictures → { $shown }

rename_bulk_recipe_numbers = Take away the numbers at the start

rename_bulk_recipe_one_space = Several spaces to one → a space

rename_bulk_recipe_spaces = Spaces to underscores → { $shown }

# IMG_ is the start of a name a camera gives, as written.
rename_bulk_recipe_start = Take away a start (IMG_)

# Two parts of a name on either side of a dash between spaces; what follows the arrow is the replacement, as written.
rename_bulk_recipe_swap = Swap two parts around “ - ” → { $shown }

rename_bulk_recipe_underscores = Underscores to spaces → a space

# A heading of the regex's and the replacement's reference: patterns that do what names often need.
rename_bulk_recipes = Recipes

rename_bulk_reference = Say what a bulk rename's field takes

# Read by agents: `field` is the argument, and template, regex and replacement its values, as written.
rename_bulk_reference_description = Return what `field` (template, regex, replacement) takes: its placeholders or signs, each with what it gives the first of the entries given or selected under the rename as given, and recipes filling the regex and its replacement

# The title of what the regex field takes: a regular expression.
rename_bulk_regex = Regex

# $1, $2 and the argument are signs of the replacement, as written; year is the group's name in it.
rename_bulk_regex_groups_note = What a group held is $1, $2… in the replacement; what a group named year held, { $named }.

# ^ and $ are signs of the pattern, as written.
rename_bulk_regex_note = ^ and $ are the start and the end of the name. It runs on the whole name, extension included, every match replaced; the template comes after it.

# The title of what the replacement field takes: what each match of the regex becomes.
rename_bulk_replacement = Replacement

# A heading of the template's reference: what is written for a sign itself.
rename_bulk_signs = Signs

# The title of what the template field takes: the new name, with placeholders.
rename_bulk_template = Template

rename_bulk_three_digits = The same, three digits at least

rename_bulk_two_digits = The same, two digits at least

# The template as it was typed: a brace opens a placeholder that never closes.
rename_bulk_unclosed = unclosed `{ $brace }` in `{ $template }`

# The placeholder as it was typed in the template.
rename_bulk_unknown_placeholder = unknown placeholder `{ $placeholder }`

# A heading of the replacement's reference.
rename_bulk_what_a_match_becomes = What a match becomes

# A heading of the template's reference: the placeholders of the item's modification date.
rename_bulk_when_modified = When it was modified

rename_bulk_whole_match = The whole match

rename_bulk_whole_name = The whole name
