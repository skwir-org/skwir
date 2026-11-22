# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

places_add_to_bookmarks = Add to Bookmarks

# The bookmark's name, as the sidebar shows it.
places_already_bookmarked = { $name } is already bookmarked

places_bookmark = Add to bookmarks

places_bookmark_address = Bookmark an address

places_bookmark_address_description = Add a remote folder's address (sftp://, smb://…) to the desktop's shared bookmarks

places_bookmark_description = Add a folder to the desktop's shared bookmarks (the GTK bookmarks file)

# The tooltip of a bookmark whose folder is not there.
places_bookmark_missing = Not found. It may have moved, or be on a volume that is not mounted.

# The bookmark's name, moved among the sidebar's bookmarks.
places_bookmark_moved = Moved bookmark { $name }

# The bookmark's name before, and the one it shows now.
places_bookmark_renamed = Renamed bookmark { $name } to { $to }

# The pane's button (and Ctrl+D) that bookmarks the folder shown.
places_bookmark_this_folder = Bookmark this folder

# A menu item on a folder's background: bookmarks the folder shown.
places_bookmark_this_folder_menu = Bookmark This Folder

# The bookmark's name, as the sidebar shows it.
places_bookmarked = Bookmarked { $name }

# The error: why the bookmarks file was not written.
places_bookmarks_not_changed = The bookmarks were not changed: { $error }

# What was done to a bookmark (a sentence of its own: "Bookmarked “Docs”"), then that the filter the folder was shown with, as typed, is not kept with the bookmark.
places_filter_not_kept = { $done }; its filter { $filter } is not kept with it

places_forget_passphrase = Forget passphrase

places_forget_passphrase_description = Remove from the keyring the passphrase Skwir keeps for an encrypted volume (never one another program kept)

# A menu item of an encrypted volume's place in the sidebar.
places_forget_passphrase_menu = Forget Passphrase

# The home folder's place in the sidebar.
# noun
places_home = Home

# The folder asked to be bookmarked.
places_in_trash = { $name } is in the trash: restore it first

# Under Remember, while it is on, in the question asking a volume's passphrase.
places_keeping = Once remembered, anyone who can open your keyring can unlock this volume.

# What the desktop's keyring shows for a volume's passphrase Skwir keeps. volume: its label, as written, or what Skwir calls a volume without one (4 TB encrypted).
places_keyring_label = Skwir: { $volume } (encrypted volume)

# What the desktop's keyring shows for a volume's passphrase Skwir keeps, the volume unnamed.
places_keyring_label_this = Skwir: this volume (encrypted volume)

# The tooltip of a locked volume's place in the sidebar.
places_locked = Locked. Click to unlock and open it.

places_mount = Mount volume

places_mount_description = Mount a present volume through udisks and open it

# The volume's name: where it is mounted.
places_mounted = Mounted { $name }

# The volume's name: where it is mounted.
places_mounted_in_tab = Mounted { $name }, opened in a new tab

# A volume's place in the sidebar while it is being mounted.
places_mounting = Mounting…

places_move_bookmark = Move bookmark

places_move_bookmark_description = Put a bookmark (a folder's, or a remote folder's by its address) before another in the sidebar, or last

# What stops working without the program udisksctl.
places_needs_udisksctl = Unmounting removable volumes from the sidebar

places_no_passphrase_kept = Skwir keeps no passphrase for this volume

places_not_a_folder = { $name } is not a folder: only folders can be bookmarked

# The folder's name, or the last part of a remote folder's address.
places_not_bookmarked = { $name } is not bookmarked

# The tooltip of a volume's place in the sidebar that is not mounted.
places_not_mounted = Not mounted. Click to mount and open it.

# The error: why, as the system said it.
places_not_mounted_error = The volume was not mounted: { $error }

places_not_removable = not a removable volume

places_only_folders = only folders can be bookmarked

places_open_locked = Unlock and open volume

places_open_locked_description = Unlock an encrypted volume with the passphrase the keyring keeps for it, else the one the user types (offering to remember it), or through the system's own dialog where it asks itself; then mount it and open it

places_passphrase_forgotten = The passphrase is forgotten

places_passphrase_not_forgotten = The passphrase was not forgotten

# The error: why the keyring did not forget it.
places_passphrase_not_forgotten_error = The passphrase was not forgotten: { $error }

# The error: why the keyring did not keep it.
places_passphrase_not_remembered = The passphrase was not remembered: { $error }

places_plugin_description = Sidebar entries from your home, XDG user folders, GTK bookmarks, and mounted volumes

places_plugin_title = Places

places_refresh = Refresh places

places_refresh_description = Re-scan user folders, bookmarks, and mounts

places_remembered_setting = The encrypted volumes whose passphrase is kept in the keyring, by their UUID

places_rename_bookmark = Rename bookmark

# The button that renames a bookmark.
places_rename_bookmark_accept = Rename

# The title of the question asking a bookmark's new name.
places_rename_bookmark_ask = Rename bookmark

places_rename_bookmark_description = Change the name a bookmark shows in the sidebar (a folder's, or a remote folder's by its address); the folder keeps its own

# The empty field asking a remote folder's bookmark's new name: left empty, the bookmark shows its address's last part.
places_rename_empty_address = Name (empty: its address's last part)

# The empty field asking a folder's bookmark's new name: left empty, the bookmark shows the folder's own name.
places_rename_empty_folder = Name (empty: the folder's own)

# Shown in place of a folder's files when the encrypted volume the folder is on was locked again. name: the volume's name, as the sidebar shows it.
places_reopen_locked = { $name } is locked

# The button under "… is not mounted" in place of a folder's files: mount the volume and show the folder again.
places_reopen_mount = Mount { $name }

# Shown in place of a folder's files when the volume the folder is on is plugged in but not mounted. name: the volume's name, as the sidebar shows it.
places_reopen_not_mounted = { $name } is not mounted

# The button under "… is locked" in place of a folder's files: unlock and mount the volume, and show the folder again.
places_reopen_unlock = Unlock { $name }

# An error when several files are selected and one folder is to be bookmarked.
places_select_one_folder = select one folder

places_unbookmark = Remove from bookmarks

places_unbookmark_address = Remove an address from bookmarks

places_unbookmark_address_description = Remove a remote folder's address from the desktop's shared bookmarks

places_unbookmark_description = Remove a folder from the desktop's shared bookmarks; the folder stays where it is

# The pane's button that removes the folder shown from the bookmarks.
places_unbookmark_this_folder = Remove from bookmarks

# The bookmark's name, as the sidebar showed it.
places_unbookmarked = Removed { $name } from bookmarks

# An action's title (copy 3 items to USB), quoted.
places_under_way_title = { $title }

places_unlock = Unlock volume

# The button of the question asking a volume's passphrase.
places_unlock_accept = Unlock

# The question asking a volume's passphrase. volume: its label, quoted, or what Skwir calls a volume without one (4 TB encrypted).
places_unlock_ask = Unlock { $volume }

places_unlock_description = Unlock an encrypted volume with the passphrase given, mount it and open it. The passphrase is a secret: it is handed to the system on standard input, never logged, and never kept

# The question asking a volume's passphrase again. volume: its label, quoted, or what Skwir calls a volume without one (4 TB encrypted).
places_unlock_refused = Unlock { $volume }: that passphrase was not accepted

# The question asking an unnamed volume's passphrase.
places_unlock_this = Unlock “this volume”

# The question asking an unnamed volume's passphrase again.
places_unlock_this_refused = Unlock “this volume”: that passphrase was not accepted

# An encrypted volume's place in the sidebar while it is being unlocked.
places_unlocking = Unlocking…

places_unmount = Unmount volume

# The volume's label.
places_unmount_busy = { $name } was not unmounted: another program is using it

places_unmount_description = Detach a removable volume through udisks so it can be unplugged

# The volume's label; the error: why, as the system said it.
places_unmount_failed = { $name } was not unmounted: { $error }

# Another program unmounts a drive while Skwir copies to it (or moves, deletes there): volume: the drive's name; actions: what Skwir does there. Its button cancels them.
places_unmount_held = { $volume } is being unmounted, but { $count ->
        [1] { $actions } is still under way on it
       *[other] { $actions } are still under way on it
    }

# The button of "… is being unmounted, but … is still under way on it": cancel what Skwir does on the drive.
places_unmount_held_cancel = Cancel

# The volume's label; the error: why the system could not write it.
places_unmount_unwritten = { $name } was not unmounted: not everything could be written to it ({ $error })

# The volume's label: it was unmounted and can be unplugged.
places_unmounted = { $name } can be removed

# A volume's place in the sidebar while it is being unmounted.
places_unmounting = Unmounting…

# A locked volume without a label, in the sidebar: its size ("4 TB").
places_volume_encrypted = { $size } encrypted

# A volume without a label, in the sidebar: its size ("4 TB").
places_volume_unlabelled = { $size } volume

# An error a script or an agent reads when it names no bookmark.
places_which_bookmark = which bookmark? pass its path or its address

# An error a script or an agent reads when it names no volume.
places_which_volume = which volume? pass its mount point
