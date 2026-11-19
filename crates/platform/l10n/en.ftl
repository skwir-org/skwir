# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A row of Properties: who else may read or change the file, beyond its mode (a POSIX access control list).
access_acl = Access list

# A row of a folder's Properties: the access list each item made in it is given.
access_acl_default = Default for new items

# An entry of a file's access list in Properties. group: a group's name or number; permissions: rwx notation, as written.
access_acl_group = group { $group }: { $permissions }

# The last entry of a file's access list: what no user or group it names may exceed. permissions: rwx notation, as written.
access_acl_mask = at most { $permissions }

# An entry of a folder's default access list: what everyone else may do with an item made in it. permissions: rwx notation, as written.
access_acl_others = others: { $permissions }

# An entry of a folder's default access list: what the owner of an item made in it may do. permissions: rwx notation, as written.
access_acl_owner = owner: { $permissions }

# An entry of a folder's default access list: what the group of an item made in it may do. permissions: rwx notation, as written.
access_acl_owning_group = group: { $permissions }

# An entry of a file's access list in Properties. user: a user's name or number; permissions: rwx notation (r read, w write, x run), as written.
access_acl_user = user { $user }: { $permissions }

# A row of a program's Properties: the privileges it is given when it runs (file capabilities, as setcap sets them).
access_capabilities = Capabilities

# An application could not be started. app: its name, as it names itself; error: why.
apps_launch_failed = launch { $app }: { $error }

# Why a default application was not chosen: the desktop's list of them (mimeapps.list) is a link to a file that cannot be written, as a configuration managed elsewhere (Nix's Home Manager) makes it.
apps_list_read_only = the list of default applications is read-only; change it where it is managed

# An application's desktop entry gives no program to start. app: the application's name, as it names itself.
apps_no_command = { $app } has no command to run

# Why a default application cannot be chosen: the user has no configuration folder to write the choice in.
apps_no_config_dir = no configuration directory

# Why a file inside an archive cannot be read: the archive was rewritten since it was shown.
archive_changed = Archive changed since it was listed; select it again

# Why an archive cannot be read: it was moved or deleted.
archive_gone = The archive is gone

# Why an archive could not be made: what reads the files for the helper stopped.
archive_handing_failed = handing the files over failed

# Why an archive's file could not be read: the program that reads archives stopped.
archive_helper_exited = archive helper exited unexpectedly

# Why an archive could not be extracted or made, its helper saying nothing. status: how it ended, as the system says it (exit status: 2).
archive_helper_failed = the archive helper failed ({ $status })

# Why an archive could not be extracted or made, before the system's reason.
archive_helper_not_started = the archive helper could not start

# Why an archive's file could not be read: the program that reads archives took too long.
archive_helper_timed_out = archive helper timed out

# Said where an archive cannot be opened. program: the helper's name (skwir-archive-worker).
archive_install = Install { $program } to open archives

# Why a file inside an archive cannot be shown: the archive no longer lists it.
archive_member_missing = Missing archive member

# Why a file inside an archive cannot be taken out: the place asked has no folder.
archive_member_nowhere = Nowhere to write the member

# Why a file inside an archive is not previewed: it is larger than the preview reads. size: that bound (16 MB).
archive_member_too_large = Member preview exceeds { $size } limit

# Why an archive cannot be made: the place asked has no folder.
archive_nowhere = Nowhere to write the archive

# Why an archive could not be made: a file to put in it was written to while it was read. name: its name in the archive.
archive_part_changed = { $name } changed as it was read

# Why an archive could not be made: a file to put in it could not be read. name: its name in the archive.
archive_part_unread = { $name } could not be read

# Why an archive of another format (7z, tar) cannot be made with a password here.
archive_password_zip_only = Only a ZIP archive can have a password

# macOS: a file taken out of a downloaded archive could not be marked as downloaded too. name: the file's name.
archive_quarantine_unheld = { $name } could not be held in quarantine

# macOS: what a downloaded archive is marked with (where it came from) could not be read, so its files are not marked alike.
archive_quarantine_unread = the archive's quarantine could not be read

# ssh is the program that connects to servers (a name).
askpass_unsupported = asking for a password through ssh is not available on this system yet

# The badge on Skwir's tile in the Dock: how far the actions under way are, in percent (42%).
desktop_dock_percent = { $percent }%

# A button of the desktop's notice that an action ended: it shows Skwir's window.
desktop_notice_show = Show

# A button of the notice that shows while Skwir works without a window: it stops every action.
desktop_notice_stop = Stop all

# A button of the notice, or an entry of the tray's menu, that shows while Skwir works without a window: it opens a window.
desktop_open = Open Skwir

# Where the desktop's fonts were read: the desktop portal (xdg-desktop-portal), placed where a desktop's or a toolkit's name goes (sizes read from the portal).
desktop_style_portal = the portal

# An entry of the menu of Skwir's icon in the tray, in place of Pause all: every paused action goes on.
desktop_tray_go_on = Go on

# An entry of the menu of Skwir's icon in the tray, while it works without a window: it opens a window.
desktop_tray_open = Open Skwir

# An entry of the menu of Skwir's icon in the tray: it pauses every action under way.
desktop_tray_pause = Pause all

# An entry of the menu of Skwir's icon in the tray: it stops every action, and Skwir ends.
desktop_tray_stop = Stop all and quit

# said: what the diagram helper wrote of why (often the diagram's mistake), as it wrote it.
diagram_failed = The diagram cannot be drawn: { $said }

# What to do to see a document's Mermaid diagrams drawn. program: the diagram helper's name (skwir-mermaid-worker).
diagram_install = Install { $program } to draw diagrams

# Why a Mermaid diagram in a document is shown as its text: it is longer than is drawn.
diagram_too_long = The diagram is too long to draw

# Why a page of a document is not shown: the file was rewritten since it was opened.
document_changed = document changed; reopen the preview

# What could not be made when pages were joined into a PDF, before the system's reason: a temporary folder beside the result.
document_compose_folder = a folder to compose the document in

# What could not be done when pages were joined into a PDF, before the system's reason: putting the result in its place.
document_compose_keep = keep the composed document

# Why a passage of a document was not looked for in its pages: the passage is too long.
document_locate_too_long = text to locate exceeds limit

# Why a page of a document is not shown: the program that reads it was started again and did not open it.
document_reopen = the document worker was started again but did not open the document

# Why a search in a PDF's text was not run: what was typed is too long.
document_search_too_long = search query exceeds limit

# Why a page of a PDF or another document is not shown: the program that reads it stopped.
document_worker_exited = document worker exited unexpectedly

# Why a PDF or another document could not be opened: the program that reads it stopped before it was ready.
document_worker_stopped = document worker stopped during startup

# Why a page of a PDF or another document is not shown: the program that reads it took too long.
document_worker_timed_out = document worker timed out

# A label in Properties: a photo's aperture (f/2.8).
exif_aperture = Aperture

# A photo's aperture, as cameras write it (f/2.8).
exif_aperture_value = f/{ $f }

# A label in Properties: who took a photo, as written in it.
exif_artist = Artist

# A label in Properties: the camera a photo was taken with.
exif_camera = Camera

# A label in Properties: a photo's copyright notice, as written in it.
exif_copyright = Copyright

# A label in Properties: a photo's exposure time (1/250 s).
exif_exposure = Exposure

# A photo's exposure time under a second, as cameras write it (1/250 s).
exif_exposure_fraction = 1/{ $denominator } s

# A photo's exposure time of a second or more (2.5 s).
exif_exposure_seconds = { $seconds } s

# A label in Properties: whether the flash fired, as the camera wrote.
exif_flash = Flash

# A label in Properties: the focal length a photo was taken at (35 mm).
exif_focal_length = Focal length

# A photo's focal length in millimetres (35 mm, 4.3 mm).
exif_focal_length_value = { $mm } mm

# A label in Properties: a photo's sensitivity, as cameras name it.
exif_iso = ISO

# Where a photo was taken, its latitude then its longitude in degrees, five digits after the decimal sign (48.85661, 2.35222): a language writing a decimal comma parts them otherwise.
exif_latitude_longitude = { $latitude }, { $longitude }

# A label in Properties: the lens a photo was taken with.
exif_lens = Lens

# A label in Properties: how a photo is turned, as the camera wrote.
exif_orientation = Orientation

# A label in Properties: where a photo was taken, as latitude and longitude.
exif_position = Position

# A label in Properties: the program that last wrote a photo.
exif_software = Software

# A label in Properties: when a photo was taken, as its camera wrote.
exif_taken = Taken

# Why skwir, started to show files for other programs or to answer their dialogs, ends at once on a system where neither is written yet. system: its name (macOS, Windows).
file_manager_not_yet = Answering other programs is not available on { $system } yet

# Why a file of the desktop's (its bookmarks, its default applications) was not changed: its path names no folder. path: the file's path.
file_no_folder = { $path } has no folder

# Why the files' Git state is not shown: the repository has no work tree (a bare repository, in Git's words).
git_bare = A bare repository has no files checked out

# said: what the Git helper wrote of why, as it wrote it.
git_history_failed = The history cannot be read: { $said }

# program: the Git helper's name (skwir-git-worker).
git_install_history = Install { $program } to show a history of commits

# program: the Git helper's name (skwir-git-worker). The files' Git state: whether each is changed, new, ignored.
git_install_status = Install { $program } to show the files' Git state

# Why a commit's changes are not shown: what was asked is no commit's id. id: what was asked, as written.
git_not_commit = Not a commit: { $id }

# Why a commit's changes are not shown: the Git helper wrote no file of them.
git_patch_not_written = The patch was not written

# said: what the Git helper wrote of why, as it wrote it.
git_status_failed = The files' Git state cannot be read: { $said }

# Said of the tree-sitter CLI's configuration file on the Reading page: it is not JSON. error: what the JSON reader said, in English.
grammars_config_invalid = its configuration is not valid JSON ({ $error }), so none of the parser directories it names is read

# Said of the tree-sitter CLI's configuration file on the Reading page: it could not be read.
grammars_config_unreadable = its configuration cannot be read, so none of the parser directories it names is read

# Why a grammar of Emacs's is not offered: Emacs colors code otherwise. Emacs is an editor's name.
grammars_emacs_no_colors = Emacs keeps no colors with its grammars: put the grammar's repository in Skwir's grammars folder (it need not be built)

# Why a grammar is not offered: its library, a path, is not in the folder it was found in, which is all its helper may read.
grammars_library_outside = its library { $library } lies outside its folder

# Why a grammar made of one program's library and another's queries is not offered. source: whose library it is; why: what the tree-sitter helper said, as it wrote it.
grammars_library_refused = { $source ->
        [skwir] a library in Skwir's folder
        [tree-sitter] the tree-sitter CLI's library
        [zed] Zed's library
        [helix] Helix's library
        [neovim] Neovim's library
        [emacs] Emacs's library
       *[system] the system's library
    } cannot be used ({ $why })

# Why a grammar is not offered: nothing says how its language is colored (a highlights query, a file of tree-sitter's).
grammars_no_highlights = no highlights query found for its language: put the grammar's repository in Skwir's grammars folder (it need not be built)

# Why a grammar is not offered: nothing says how its language is colored (a highlights query, a file of tree-sitter's).
grammars_no_highlights_repository = no highlights query in its repository, nor in the system's

# Why a grammar is not offered: its repository holds no compiled library. `tree-sitter build` is a command, as written.
grammars_no_library = no library: build the grammar (`tree-sitter build`)

# Said of a folder on the Reading page: the tree-sitter CLI's configuration names it, and it does not exist.
grammars_parser_dir_missing = its configuration names this parser directory, which does not exist

# Why a grammar is not offered: an editor's queries color only its own grammars. editor: which editor's (Zed, Helix, Neovim, Emacs are their names).
grammars_queries_not_lent = no queries installed with it, and { $editor ->
        [zed] Zed's
        [helix] Helix's
        [emacs] Emacs's
       *[neovim] Neovim's
    } are not lent to another's library: put the grammar's repository in Skwir's grammars folder (it need not be built)

# Why a grammar made of one program's library and a repository's queries is not offered. source: whose library it is; why: what the tree-sitter helper said, as it wrote it.
grammars_repository_queries_refused = its repository's queries do not fit { $source ->
        [skwir] a library in Skwir's folder
        [tree-sitter] the tree-sitter CLI's library
        [zed] Zed's library
        [helix] Helix's library
        [neovim] Neovim's library
        [emacs] Emacs's library
       *[system] the system's library
    } ({ $why }): check the repository out at the revision its library was built from

# Where tree-sitter grammars were found, as the Reading page names it. Zed, Helix, Neovim and Emacs are editors' names.
grammars_source_label = { $source ->
        [skwir] Skwir
        [tree-sitter] the tree-sitter CLI
        [zed] Zed
        [helix] Helix
        [neovim] Neovim
        [emacs] Emacs
       *[system] the system
    }

# Where tree-sitter grammars were found, at the head of a choice on the Reading page. Zed, Helix, Neovim and Emacs are editors' names.
grammars_source_short = { $source ->
        [skwir] Skwir's folder
        [tree-sitter] tree-sitter CLI
        [zed] Zed
        [helix] Helix
        [neovim] Neovim
        [emacs] Emacs
       *[system] System
    }

# Why a grammar made of one program's library and the system's queries is not offered. source: whose library it is; why: what the tree-sitter helper said, as it wrote it.
grammars_system_queries_refused = the system's queries do not fit { $source ->
        [skwir] a library in Skwir's folder
        [tree-sitter] the tree-sitter CLI's library
        [zed] Zed's library
        [helix] Helix's library
        [neovim] Neovim's library
        [emacs] Emacs's library
       *[system] the system's library
    } ({ $why }): put the grammar's repository, at the revision its library was built from, in Skwir's grammars folder

# Said of a grammar's repository on the Reading page: the tree-sitter CLI does not look this deep. folder: the folder it reads, a path; inner: the folder between, a path; parser-directories is its setting's name.
grammars_too_deep_cli = one folder too deep: it reads only the folders directly in { $folder }; add { $inner } to its parser-directories, or link the repository into Skwir's grammars folder

# Said of a grammar's repository on the Reading page: it is in a folder inside Skwir's grammars folder. folder: Skwir's grammars folder, a path.
grammars_too_deep_skwir = one folder too deep: Skwir reads only the folders directly in it ({ $folder }); move or link the repository there

# A choice of which grammars color code: bat's (bat is a program's name, written in lower case), unless only a tree-sitter grammar knows the language.
highlight_bat_first = bat first

# Why a block of code in a document is not colored by its tree-sitter grammar. size: the longest colored.
highlight_block_too_long = a block longer than { $size }

# A choice of the code theme settings: the colors of the palette chosen, else Skwir's own.
highlight_default = Default

# A choice of the code theme settings: the theme chosen, which bat's themes folder no longer has. theme: its name.
highlight_not_found = { $theme } (not found)

# A choice of which grammars color code: the tree-sitter grammars found, unless only bat's know the language.
highlight_tree_sitter_first = Tree-sitter first

# A preview's notice: only the beginning of a long document is shown; Source is the button that shows the file's own text.
inspection_blocks_notice = { $count ->
        [1] Only the first block is shown · Source shows the rest
       *[other] Only the first { $count } blocks are shown · Source shows the rest
    }

# A count alone, as the value of a row in a preview's Properties (Pages: 1,234).
inspection_count = { $count }

# Why a compressed drawing (.svgz) is not shown: unpacked, it is larger than the preview reads.
inspection_drawing_inflated = The drawing is too large to read

# Why an SVG drawing is not previewed: it is larger than the preview reads. size: that bound (16 MB).
inspection_drawing_too_large = The drawing exceeds the { $size } preview limit

# Why a picture a document names is not shown. size: the largest read (16 MB).
inspection_embedded_image_limit = An embedded image exceeds the { $size } preview limit

# A column's header in the hexadecimal view: where each row of bytes is in the file.
inspection_hex_offset = Offset

# Why a picture inside a document is not shown. size: the largest read (16 MB).
inspection_image_too_large = Embedded image exceeds { $size }

# A text preview's title: how many lines the file has.
inspection_lines = { $count ->
        [one] { $count } line
       *[other] { $count } lines
    }

# Why a pipe, a socket or a device is not previewed.
inspection_not_regular = preview needs a regular file

# A preview's notice: the text had no mark of its encoding and was read in this one. encoding: its name (Windows-1252, UTF-16LE).
inspection_read_as = Read as { $encoding }

# A link after a picture's position in Properties: it opens the Map view.
inspection_show_on_map = Show on map

# Why a drawing is not shown: it declares a width or height of zero.
inspection_svg_no_size = SVG picture has no size

# Why a text is not previewed: it is larger than the preview reads. size: that bound (2 MB).
inspection_text_too_large = The text exceeds the { $size } preview limit

# Why an XML file is not shown: its DOCTYPE declaration is written in a way the reader does not take.
inspection_xml_doctype = its document type is not read

# Why an XML file is not shown: the entities its document type declares would make it larger than this. size: that bound (2 MB).
inspection_xml_entities = its entities expand past { $size }

# Why an XML file is not shown, after what could not be read. count: the most levels read (512).
inspection_xml_too_deep = it nests its elements deeper than { $count ->
        [one] { $count } level
       *[other] { $count } levels
    }

# Why a helper program was asked nothing more: it stopped reading what Skwir asks.
line_channel_closed = worker closed its requests

# Why a helper program was asked nothing more: it has stopped, or hangs.
line_channel_not_reading = worker is not reading its requests

# What could not be done, before why (start skwir-archive: No such file or directory). program: the helper's name.
line_channel_start = start { $program }

# said: what the math helper wrote of why, as it wrote it.
math_failed = The formulas cannot be typeset: { $said }

# What to do to see a document's formulas drawn. program: the math helper's name (skwir-math-worker).
math_install = Install { $program } to typeset formulas

# A notice in the player: a control (a seek, a track switch) could not be carried out, and playback goes on. why: the player's reason, in English as it wrote it.
media_control_refused = The player could not do that: { $why }

# Why a play, pause or seek was not done: the player is still taking the ones before, or it has stopped.
media_controls_busy = video controls busy or session closed

# What could not be done before a film plays, before the system's reason: a place in memory for its pictures.
media_frame_slot = create video frame slot

# Why a film or a sound stopped playing: the program that plays it stopped.
media_worker_exited = video worker exited

# Why a film or a sound does not play: the program that plays it stopped before it was ready.
media_worker_stopped = video worker stopped during startup

# Why a film or a sound does not play: the program that plays it took too long to start.
media_worker_timed_out = video worker timed out

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_archives = Archives

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_audio = Music and audio

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_documents = Documents

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_folders = Folders

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_fonts = Fonts

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_images = Images

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_presentations = Presentations

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_programs = Programs

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_spreadsheets = Spreadsheets

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_text = Text and code

# A kind of files, as their group is named: in a search grouped by kind, Usage, the Quick filters.
mime_kind_videos = Videos

# The type of a file with no extension whose type the type database names not (or not yet). In the Type column and a heading of files grouped by type.
mime_unnamed = File

# The type of a file whose type the type database names not (or not yet), by its extension, written in capitals: CSV file. In the Type column and a heading of files grouped by type.
mime_unnamed_extension = { $extension } file

# said: what the model helper wrote of why, as it wrote it.
model_failed = The model cannot be drawn: { $said }

# What to do to see 3D models. program: the model helper's name (skwir-model-worker).
model_install = Install { $program } to draw 3D models

# A label in Properties: where a film was made, as latitude and longitude.
mp4_position = Position

# A terminal could not be started. program: the terminal's program; error: why.
open_launch_failed = launch { $program }: { $error }

# A terminal was asked to run a program, and none was given.
open_no_command = no command to run

# No terminal program is installed that Skwir knows; $TERMINAL is the environment variable that names one (not translated).
open_no_terminal = no terminal emulator found (set $TERMINAL)

# Why a position is not shown in the desktop's maps application: it is no geo: address.
open_not_geo = not a map location

# Why a link in a document or the terminal is not followed: it is neither a web page nor a mail address.
open_not_web_or_mail = only web and mail links open from here

# The desktop's default application could not be started for a file. path: the file; error: why.
open_path_failed = open { $path }: { $error }

# Why a program is not run in a terminal window. system: the system's name (macOS).
open_terminal_program_not_yet = A terminal running a program is not available on { $system } yet

# BitLocker is Windows' encryption of drives (a name).
places_bitlocker_asks = Windows asks for a BitLocker drive's password itself: open the drive

# Why a bookmark was not kept: the desktop's bookmarks file is a link to a file that cannot be written, as a configuration managed elsewhere (Nix's Home Manager) makes it.
places_bookmarks_read_only = the bookmarks file is read-only; change it where it is managed

# diskutil is the program that mounts a Mac's volumes (a name).
places_diskutil_no_mount_point = diskutil did not say where it mounted the volume

# diskutil is the program that mounts and unlocks a Mac's volumes (a name). error: why it could not be run.
places_diskutil_unrun = diskutil could not be run: { $error }

# A Windows drive, as Windows names it. label: the drive's own name; letter: its drive letter (C:).
places_drive = { $label } ({ $letter })

# A Windows drive that has no name of its own, as Windows names it. letter: its drive letter (C:).
places_local_disk = Local Disk ({ $letter })

# Why a volume shown in the sidebar is not mounted. system: the system's name (Windows).
places_mount_not_yet = Mounting a volume is not available on { $system } yet

# Why a bookmark cannot be kept: the user has no configuration folder to write it in.
places_no_config_dir = no configuration directory

# Why an encrypted volume was not unlocked: the passphrase was typed wrong, or has changed.
places_passphrase_refused = The passphrase was not accepted

# The name of the volume the system runs from (/), in the sidebar's Devices.
# noun
places_root = Root

# Skwir asks for an encrypted volume's passphrase on this system; the system has no dialog of its own for it.
places_system_asks_not = This system does not ask for a volume's passphrase itself

# udisksctl is the program that unlocks encrypted volumes (a name).
places_udisks_no_device = udisksctl did not name the unlocked device

# udisksctl is the program that mounts volumes (a name).
places_udisks_no_mount_point = udisksctl did not say where it mounted the volume

# udisksctl is the program that mounts and unlocks volumes (a name). error: why it could not be run.
places_udisks_unrun = udisksctl could not be run: { $error }

# Why Undo of a choice of what answers other programs' Open and Save dialogs did nothing: the portal's configuration file was edited after the choice. Begins in lower case: said after other words.
portals_changed_since = the file was changed since Skwir wrote it, and is kept as it is

# Why a choice of what answers other programs' Open and Save dialogs was not written: the user's portal configuration (its file or its folder) is a link, kept by a program such as home-manager or a dotfiles folder; target: where the link leads. Begins in lower case: said after other words.
portals_linked = it is a link to { $target }, which another program keeps: choose there

# An error choosing what answers other programs' Open and Save dialogs: the user has no configuration folder (no XDG_CONFIG_HOME, no home).
portals_no_config_folder = There is no configuration folder to write the choice in

# Why the desktop portal's configuration file was left alone: its text is not UTF-8, which the portal reads. Begins in lower case: said after other words.
portals_not_utf8 = it is not written in UTF-8

# Why a picture of a file is not shown: the file was written to while it was drawn.
preview_changed = file changed during preview

# Why an office document is not shown: it was written to while it was turned into a PDF.
preview_changed_converting = file changed during conversion

# Why a document is not shown: it was written to while it was opened.
preview_changed_opening = file changed during document open

# Why a sound file does not play. program: the helper's name (skwir-media-worker).
preview_install_audio = Install { $program } and GStreamer's playback plugins to play this

# Why a PDF is not shown with the engine the settings chose. program: its helper's name (skwir-poppler-worker); engine: the engine's name (Poppler, MuPDF).
preview_install_engine = Install { $program } and its library to read documents with { $engine }

# Why a film does not play. program: the helper's name (skwir-media-worker); Still frames is a choice of the Video engine setting, Video the settings page.
preview_install_video = Install { $program } and GStreamer's playback plugins to play this, or choose Still frames as the Video engine

# Why a PDF is not shown: the program that reads it found no page.
preview_no_pages = PDF has no readable pages

# Why an office document is not shown: LibreOffice, which turns it into a PDF to show, made none.
preview_office_no_pdf = LibreOffice did not produce a PDF (the file may be encrypted or unsupported)

# Why an office document is not shown: what LibreOffice made of it is not a PDF.
preview_office_not_pdf = conversion did not produce a PDF

# Why a picture of a file is not shown: the program that drew it wrote more than the preview reads.
preview_output_too_large = preview output is too large

# Why a PDF is not shown. path: where the setting says the library is; Settings › Reading › Documents › PDFium library is where that setting is, as the window names it.
preview_pdfium_library_missing = PDFium's library is not at “{ $path }”, where the settings say it is (Settings › Reading › Documents › PDFium library)

# Why a picture of a file is not shown: the program that drew it made it larger than asked.
preview_too_wide = preview dimensions exceed limit

# Why a PDF is not shown: the settings name an engine that is none. auto, pdfium, poppler and mupdf are the setting's values, as written in a settings file.
preview_unknown_backend = unknown PDF backend; choose auto, pdfium, poppler or mupdf

# The button of the desktop's print dialog that prints.
print_accept = Print

# The desktop's print dialog ended in a way it does not explain. code: the number it answered.
print_dialog_ended = the print dialog ended ({ $code })

# The pages chosen in the desktop's print dialog cannot be read. ranges: what it answered (1-2,4); refused: why.
print_dialog_pages = the print dialog's pages “{ $ranges }”: { $refused }

# Windows' print dialog ended before it answered.
print_dialog_stopped = the print dialog stopped

# The helper program that makes what is printed left an empty document.
print_job_empty_pdf = the print helper wrote an empty PDF

# The print helper failed. status: how it ended, as the system says; said: the first line it wrote of why.
print_job_failed = The PDF could not be made ({ $status }): { $said }

# The helper program that lays out what is printed is missing. program: its name.
print_job_install = Install { $program } to print

# Said when a long code file is printed: the rest of its lines are left out.
print_job_lines_cut = { $count ->
        [1] Only the first line is printed
       *[other] Only the first { $count } lines are printed
    }

# The helper program that makes what is printed left no document.
print_job_no_pdf = the print helper wrote no PDF

# The helper program that makes what is printed ended without its answer.
print_job_silent = the print helper said nothing

# path: the file Skwir writes the print job to, in a folder of its own.
print_job_unwritten = { $path } could not be written

# PDFium is the library that draws the pages on the printer (a name).
print_needs_pdfium = printing needs PDFium, which is not there

# error: what the desktop's print portal answered.
print_no_dialog = this desktop shows no print dialog (its print portal: { $error })

# The desktop's print dialog answered without what Skwir needs to print what was chosen.
print_no_token = the print dialog answered no token

# The desktop refused to print what its dialog chose. code: the number it answered.
print_not_taken = the desktop's printing did not take it ({ $code })

# Why nothing is printed. system: the system's name, or this system.
print_not_yet = Printing is not available on { $system } yet

# The desktop's printing service no longer answers.
print_portal_silent = the print portal stopped answering

# The desktop's printing service ended while its dialog was shown.
print_portal_stopped = the print portal stopped

# Why no print dialog shows: Skwir cannot hear the desktop's answers (its print portal, on the session bus).
print_portal_unheard = the print portal cannot be listened to

# error: why the printing system did not take the document.
print_refused = the printer refused it: { $error }

# Windows' print dialog answered settings shorter than they are.
print_settings_short = the printer's settings are not whole

# A program Skwir ran ended with an error. program: its name; status: how it ended, as the system says it (exit status: 1); said: what it wrote of why.
process_failed = { $program } failed ({ $status }): { $said }

# program: the name of a program Skwir runs (libreoffice, bwrap).
process_not_installed = { $program } is not installed

# A helper program failed, and it is older than Skwir. error: why it failed; program: the helper's file name.
process_older_build = { $error }. { $program } is from an older build of Skwir, which may be why: rebuild or reinstall it

# A helper program failed, and it is not of the build Skwir is. error: why it failed; program: the helper's file name; theirs, ours: the two builds' versions.
process_other_build = { $error }. { $program } is from another build of Skwir ({ $theirs }; Skwir is { $ours }), which may be why: rebuild or reinstall it

# A program that copies or converts was stopped: it stopped saying how far it went. program: its name (rsync).
process_stalled_minutes = { $program } stopped: it said nothing of its progress for { $count ->
        [one] { $count } minute
       *[other] { $count } minutes
    }

# A program that copies or converts was stopped: it stopped saying how far it went. program: its name (rsync).
process_stalled_seconds = { $program } stopped: it said nothing of its progress for { $count ->
        [one] { $count } second
       *[other] { $count } seconds
    }

# What could not be done, before why (start ffmpeg: No such file or directory). program: the program's name.
process_start = start { $program }

# program: the name of a program Skwir ran, stopped as it took too long.
process_timed_out = { $program } timed out

# program: the name of a program Skwir would run, not run as what it would be given is too large.
process_too_much_input = { $program }: input too large

# program: the name of a program Skwir ran, stopped as it wrote more than Skwir reads.
process_too_much_output = { $program }: excessive output

# Why a book's fixed-layout page is not shown: it declares a width or height of zero.
rich_preview_page_no_size = page has no size

# Why a page's stylesheet beside it is not read. size: the largest read (2 MB).
rich_preview_stylesheet_limit = A stylesheet exceeds the { $size } preview limit

# Why a converter does not run. system: the system's name (Windows).
sandbox_converter_not_yet = Running a converter isolated from your files is not available on { $system } yet

# Why a converter or plugin program does not run: bubblewrap, a program's name, is not installed.
sandbox_needs_bubblewrap = This needs bubblewrap, to run isolated from your files

# Why a converter or plugin program does not run on macOS: the system's sandbox does not work here.
sandbox_needs_sandbox = This needs the system's sandbox, to run isolated from your files

# Why a plugin from outside, a program, does not run. system: the system's name (Windows).
sandbox_plugin_not_yet = Running a plugin's program isolated from your files is not available on { $system } yet

# The desktop's keyring, which keeps passwords (GNOME Keyring, KWallet), did not answer. error: why.
secrets_no_keyring = No keyring answered: { $error }

# A password kept in the system's keyring cannot be read as text.
secrets_not_text = The keyring's secret is not text

# Skwir keeps passwords in the system's keyring; it knows none on this system.
secrets_unsupported = No keyring is supported on this system yet

# Why a server's shared folder was not connected: the name or the password was not accepted.
shares_login_refused = the server refused the login

# Why a share is not mounted the Mac's way. NetFS is the Mac's name for it; system: the system's name (Windows).
shares_netfs_not_yet = Mounting a share through NetFS is not available on { $system } yet

# address: what was typed to connect to a server (smb://server/share).
shares_no_address = “{ $address }” is no address the system reads

# NetFS is macOS's service that connects servers' shared folders (a name): it said it connected one, and not where.
shares_no_mount_point = NetFS said no mount point

# Why a Windows share is not connected to. system: the system's name (macOS).
shares_windows_not_yet = A Windows share is not available on { $system } yet

# Why the variables a login shell sets were not read: the shell failed. shell: its path; status: how it ended, as the system says it (exit status: 1).
shell_environment_failed = { $shell } ended with { $status }

# The copy the helper program made cannot be read.
shrink_copy_unread = the shrink helper's copy could not be read

# The helper program that makes smaller copies left an empty copy.
shrink_empty_copy = the shrink helper wrote an empty copy

# The helper program that makes smaller copies failed. status: how it ended, as the system says; said: the first line it wrote of why.
shrink_failed = No smaller copy could be made ({ $status }): { $said }

# path: a PDF whose smaller copy is checked, gone meanwhile.
shrink_gone = { $path } is not there

# The helper program that makes smaller copies of files is missing. program: its name.
shrink_install = Install { $program } to make smaller copies

# The helper program that makes smaller copies left no copy.
shrink_no_copy = the shrink helper wrote no copy

# The helper program said it made a smaller copy, and the copy is not smaller.
shrink_no_smaller = the shrink helper wrote a copy no smaller than the file

# The helper program that makes smaller copies ended without its answer.
shrink_silent = the shrink helper said nothing

# path: the file a smaller copy is made of.
shrink_unread = { $path } could not be read

# path: the file Skwir writes the helper's job to, in a folder of its own.
shrink_unwritten = { $path } could not be written

# Why code was not colored: the helper that colors it had stopped.
tree_sitter_ended = the tree-sitter helper ended

# Why code was not colored: the helper that colors it had stopped. why: what stopped it.
tree_sitter_ended_why = the tree-sitter helper ended: { $why }

# Why code was not colored: the helper that colors it stopped.
tree_sitter_exited = the tree-sitter helper exited unexpectedly

# What to do to color code with the grammars found. program: the helper's name (skwir-tree-sitter-worker).
tree_sitter_install = Install { $program } to color code with tree-sitter grammars

# Why code was not colored: the helper that colors it took too long.
tree_sitter_no_answer = the tree-sitter helper did not answer in { $count ->
        [one] { $count } second
       *[other] { $count } seconds
    }

# Why code was not colored: the helper that colors it took too long to start.
tree_sitter_not_started = the tree-sitter helper did not start in { $count ->
        [one] { $count } second
       *[other] { $count } seconds
    }

# Why code was not colored: the helper that colors it took too long, and was stopped.
tree_sitter_timed_out = the tree-sitter helper timed out

# Why a text is not colored by its tree-sitter grammar: it is larger than the helper reads. size: that bound.
tree_sitter_too_large = the text exceeds { $size }

# Why code is not colored by the tree-sitter grammars found: there are too many of them.
tree_sitter_too_many = the grammars found are more than the tree-sitter helper is told at once

# Why a private channel for scripts and agents cannot be made: Windows did not name the user it is for.
windows_no_user = Windows does not say who the user is

# Windows' own dialog for a BitLocker drive's password ended without an answer.
windows_unlock_silent = Windows' unlock dialog did not answer

# Windows' own dialog for a BitLocker drive's password. error: why it did not open.
windows_unlock_unopened = Windows' unlock dialog could not be opened: { $error }
