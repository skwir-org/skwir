# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

usage_cancel = Cancel disk usage scan

# Said in the Usage view once its measuring was stopped.
usage_cancelled = Scan cancelled

usage_changed_during = Directory changed during scan; totals are incomplete

# Said in the Usage view: entries of the folder could not be read.
usage_entries_unread = { $count ->
        [one] { $count } entry could not be read; totals are incomplete
       *[other] { $count } entries could not be read; totals are incomplete
    }

# A context menu's item: shows the folder in the Usage view.
usage_menu = Usage view

usage_needs_directory = Disk usage needs a directory

usage_needs_folder = disk usage needs a filesystem directory

# Said in the Usage view of a folder on a server, measured only when asked.
usage_network_folder = A network folder: Refresh (F5) measures it, reading every folder of it across the network

usage_no_pane = no such pane

# Why a folder's disk usage is not shown. system: the system's name (Windows).
usage_not_yet = Disk usage is not available on { $system } yet

# Said in the Usage view: some folders' changes cannot be followed as they happen.
usage_partly_watched = Some folders cannot be monitored; rechecking periodically

usage_plugin_description = Browse folder sizes in a treemap with live updates

usage_plugin_title = Disk usage

usage_release = Release disk usage view

usage_scan = Scan disk usage

# Read by agents and in F1's list of actions.
usage_scan_description = Show cached directory sizes, computing missing or changed branches in the background

usage_status = Disk usage status

usage_status_description = The selected pane's latest size breakdown

usage_too_deep = Directory nesting exceeds the scan limit

usage_unreadable = Unable to read this directory

# Said in the Usage view: no folder's changes can be followed as they happen.
usage_unwatched = Live monitoring unavailable; rechecking periodically

# The view that shows what each folder takes on disk, among the views (List, Grid…).
usage_view = Usage

# Said in the Usage view of /proc or /sys, whose files are made up by the system.
usage_virtual = Disk usage is not available for virtual filesystems
