# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# Said after extracting, when everything the archive holds was in the folder it is extracted into already.
archives_already_extracted_into = { $archive } was already extracted into { $into }

# Said after extracting, when what the archive holds was where it is extracted to already.
archives_already_extracted_to = { $archive } was already extracted to { $into }

# Said after compressing, when the archive of that name held what goes in it already.
archives_already_held = { $count ->
        [1] { $archive } already held { $name }
       *[other] { $archive } already held { $count } items
    }

# Why an archive is not extracted or made: something else has the name it would take.
archives_already_there = { $name } is already there

# Why an archive is not made: its name is taken by something that goes in it.
archives_among_inputs = { $name } is among what it compresses

# The title of the question asking an archive's name, which follows it in a field.
archives_ask_as = { $count ->
        [1] Compress { $name } as
       *[other] Compress { $count } items as
    }

# Under the field of an archive's name.
archives_ask_as_note = Its ending says the format: { $endings }

# The title of the question asking an encrypted archive's name, which follows it in a field.
archives_ask_with_password = { $count ->
        [1] Compress { $name } with a password, as
       *[other] Compress { $count } items with a password, as
    }

# Under the field of an encrypted archive's name. ZIP is the format's name, as written.
archives_ask_with_password_note = A ZIP archive, encrypted: its password is asked next

# Why an archive was not made: what is kept beside a file going in could not be read; why follows.
archives_attributes_unread = the tags and attributes of { $name } could not be read

# Why an archive is not extracted into a folder already there: what it holds must be known, member by member.
archives_cannot_list_whole = { $name } cannot be listed whole

# An item of the Compress submenu: asks the archive's name, whose ending says its format.
archives_choose_format = Choose Format…

# A column's header in an archive's listing.
archives_column_name = Name

# A column's header in an archive's listing.
archives_column_size = Size

# A column's header in an archive's listing: whether a member is encrypted, a link or special.
archives_column_status = Status

# The action's name, in F1's list of actions.
archives_compress = Compress

# A step of a plan, in its review: what goes in the archive (one by its name, or how many), and the archive's name. Begins in lower case.
archives_compress_as = { $count ->
        [1] compress { $name } as { $archive }
       *[other] compress { $count } items as { $archive }
    }

# The action's name, in F1's list of actions: asks the archive's name first.
archives_compress_ask = Compress…

archives_compress_ask_description = Ask the name of an archive of the selection, its ending saying its format, and make it

# A button: makes the archive (once its name, or its password, is given).
archives_compress_button = Compress

# Read by agents and in F1's list of actions: the words in backticks are the command's arguments, and the endings as written.
archives_compress_description = Make an archive of the selection, or the entries given, beside them or in `dest`: its format is what `name` ends with (.zip, .7z, .tar.gz, .tar.xz, .tar.zst, .tar.bz2, .tar), else `format`, else ZIP (plan)

# A plan's title, in its review, Activity and the History: what goes in the archive, one by its name, or how many. Begins in lower case: said inside other words too (undo compress 3 items).
archives_compress_plan = { $count ->
        [1] compress { $name }
       *[other] compress { $count } items
    }

# What a step did, said once its plan ran. Begins in lower case.
archives_compressed_as = { $count ->
        [1] compressed { $name } as { $archive }
       *[other] compressed { $count } items as { $archive }
    }

# A label of an archive's Properties in its preview: how many of its members are encrypted.
archives_detail_encrypted = Encrypted

# A label of an archive's Properties in its preview: how many files it holds.
archives_detail_files = Files

# A label of an archive's Properties in its preview: how many folders it holds.
archives_detail_folders = Folders

# A label of an archive's Properties in its preview: the size of what it holds, unpacked.
archives_detail_unpacked = Unpacked

# How many members of an archive are encrypted, in its Properties.
archives_encrypted_members = { $count ->
        [one] { $count } member
       *[other] { $count } members
    }

# The endings of the archives' names Skwir makes, each saying its format, as written: a list said inside other words.
archives_endings = .zip, .7z, .tar.gz, .tar.xz, .tar.zst, .tar.bz2 or .tar

archives_extract = Extract here

# The button that gives an archive's password and extracts it.
archives_extract_button = Extract

# Read by agents and in F1's list of actions; `dest` is the command's argument, as written.
archives_extract_description = Extract each archive of the selection, or the entries given, beside it or in `dest`: an archive of one folder or file gives that folder or file, one of several a folder named after it (plan)

# A menu item, in an archive's menu and in what a drop of one offers.
archives_extract_here = Extract Here

# A step of a plan, in its review: the archive, and the folder already there it is extracted into. Begins in lower case.
archives_extract_into = extract { $archive } into { $into }

# A plan's title, in its review, Activity and the History: the archive extracted, or how many. Begins in lower case: said inside other words too (undo extract 3 archives).
archives_extract_plan = { $count ->
        [1] extract { $name }
       *[other] extract { $count } archives
    }

# A step of a plan, in its review: the archive, and the folder it is extracted to. Begins in lower case.
archives_extract_to = extract { $archive } to { $into }

# What a step did, said once its plan ran. Begins in lower case.
archives_extracted_into = extracted { $archive } into { $into }

# What a step did, said once its plan ran. Begins in lower case.
archives_extracted_to = extracted { $archive } to { $into }

# Above an archive's listing cut short: how many of its members are listed.
archives_first_members = { $count ->
        [one] First { $count } member · open it in an archive application for the rest
       *[other] First { $count } members · open it in an archive application for the rest
    }

# Why a member of an archive is not extracted into a folder already there: the folder it goes in went.
archives_folder_not_there = the folder { $name } goes in is not there

# An error an agent or a script reads: the archive's name ends with one format, and it asked another. The formats are named as written (ZIP, 7z).
archives_format_differs = { $name } says { $said }, and the format { $asked }

# Why an archive is not extracted: the folder of its name holds the archive itself.
archives_holds_archive = { $there } holds { $archive }, which it would be extracted from

# Why an archive is not made: the folder of its name holds something that goes in it.
archives_holds_input = { $there } holds { $input }, which it compresses

# An error an agent or a script reads: the archive named has no folder (a root).
archives_in_a_folder = an archive is in a folder

# Said in the preview of a member of an archive chosen in its listing.
archives_member_not_previewed = This member cannot be previewed (directory, link or encrypted file)

# Why an archive is not made: its name ends with none of the endings Skwir makes.
archives_no_format = { $name } says no format: end it with { $endings }

# Why an archive was not made: something to go in it went.
archives_no_longer_there = { $name } is no longer there

# Why an archive is not extracted or made: where it goes is no folder.
archives_not_a_folder = { $name } is not a folder

archives_not_a_folder_to_extract_into = { $name } is not a folder to extract into

archives_not_an_archive = { $name } is not an archive

# The member is named by its path within the archive.
archives_not_in_archive = { $member } is not in { $archive } any more

# What an archive extracted or made could not be put in place; why follows.
archives_not_placed = { $name } could not be placed in { $folder }

archives_not_writable = { $name } cannot be written in: nothing is extracted into it

archives_nothing_to_compress = Nothing to compress

# ZIP is the format's name, as written.
archives_only_zip_password = Only a ZIP archive can have a password

# ZIP is the format's name, and .zip its ending, as written.
archives_only_zip_password_end = Only a ZIP archive can have a password: end its name with .zip

# The title of the question asking an archive's new password a second time.
archives_password_again = The same password again, to be sure of it

# The question asked again, the password typed the second time not being the first.
archives_password_differed = The two passwords differed: a password for { $name }

# The title of the question asking an archive's password again, nothing having been typed.
archives_password_empty = Password for { $name }: it cannot be empty

# The title of the question asking an encrypted archive's password.
archives_password_for = Password for { $name }

# The title of the question asking the password an archive being made is encrypted by.
archives_password_new = A password for { $name }

# The question asked again, nothing having been typed.
archives_password_new_empty = A password for { $name }: it cannot be empty

# The button that gives an archive's new password, before it is asked again.
archives_password_next = Next

# Why an archive was not made where no window can ask its password (a script's, an agent's). Skwir is the program's name.
archives_password_unasked = { $name } would be encrypted: its password is asked in Skwir's window

archives_plugin_description = List the contents of ZIP, tar, 7z and other archives, preview their members without extracting, extract them (Extract Here, in an archive's menu and a drop's), and make them (Compress, in the menu of what you select)

archives_plugin_title = Archives

# What stops working without the archive helper program, on the Plugins page.
archives_requirement = Listing, reading, extracting and making archives

# A member's status in an archive's listing.
archives_status_encrypted = Encrypted

# A member's status in an archive's listing: a link, a device or a pipe, which is not previewed.
archives_status_link = Link / special

archives_through_link = { $name } is a link: nothing is extracted through it

# The title of an archive's preview: how many members it lists.
archives_title = { $count ->
        [one] Archive · { $count } member
       *[other] Archive · { $count } members
    }

# An item of the Compress submenu: makes a ZIP archive at once. ZIP is the format's name, as written.
archives_to_zip = To ZIP

# Why an archive is not listed: a member's path is nested deeper than this many folders.
archives_too_deep = { $count ->
        [one] Archive path exceeds { $count } component
       *[other] Archive path exceeds { $count } components
    }

# The name is a path within the folder extracted into.
archives_two_as_one = two items would be extracted as { $name }

# Why an archive was not read or made where no window can ask its password (a script's, an agent's). Skwir is the program's name.
archives_unasked = { $name } is encrypted: its password is asked in Skwir's window

# An error an agent or a script reads: the format it gave, as written, is none Skwir makes.
archives_unknown_format = “{ $format }” is no format here: say one of { $endings }

# Why an archive was not made: a folder going in could not be read; why follows.
archives_unreadable = { $name } could not be read

# Why extracting or making an archive failed: what it put in place was gone at once.
archives_went_missing = { $name } went missing as it was made

# An item of the Compress submenu: an encrypted ZIP archive, its name and password asked.
archives_with_password = With a Password…

# Why an archive is not extracted: a member's path leads out of the folder it is extracted into.
archives_would_leave = what is extracted into { $name } would leave it

# The title of the question asking an archive's password again, the one given being wrong.
archives_wrong_password = Wrong password for { $name }: try again
