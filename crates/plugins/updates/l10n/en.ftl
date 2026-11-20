# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

updates_answer = Answer whether to be told of new versions

# site: Skwir's website, as written (skwir.org).
updates_answer_description = The answer to the question a downloaded copy asks once: whether to ask { $site } once a day

# Said when the answer to the question about new versions could not be saved in the settings; why follows (the settings file is read-only, the drive is full).
updates_answer_not_kept = Skwir could not keep your answer

# Asked once, in a message that stays, of a copy downloaded from the website. site: the website, as written (skwir.org).
updates_ask = Tell you when a new version of Skwir is out? Skwir would ask { $site } once a day; Cloudflare, which serves it, sees your IP address.

# The button saying no to being told of new versions.
updates_ask_no = No

# A button of the question about new versions, for a .deb or an .rpm: opens the website's page on adding Skwir's repository, so the system updates it.
updates_ask_repository = Update with your system…

# The button saying yes to being told of new versions.
updates_ask_yes = Yes

updates_check = Check for a new version

# site: Skwir's website, as written (skwir.org).
updates_check_description = Ask { $site } now whether a newer version of Skwir is out, for a copy that is told of them, on a metered connection too: a toast says what it found, that Skwir is up to date, or why it could not tell

# Said when a check for a new version asked for now got no answer it reads; why follows (the website's answer, no connection).
updates_check_failed = Skwir could not check for a new version

updates_kept_description = What the updates plugin keeps: when this copy was first seen and asked, the last check and what it told.

updates_latest = What the last check found

updates_latest_description = How this copy came, whether it is told of new versions, and the newest version the last check found, from memory

# Why a check for a new version was not made: a package manager (the AUR, Nix, a distribution, Skwir's repository) or a build from source.
updates_managed = This copy of Skwir is not checked: whatever installed it tells of new versions

# Why a check for a new version was not made: just started, Skwir has not read yet how it was installed.
updates_not_ready = Skwir is still reading how it was installed: try again

# Why a check for a new version was not made: the system refused a thread for it.
updates_not_started = The check could not be started

# Why the check for a new version failed: the website's file holds no version Skwir reads. site: the website, as written (skwir.org).
updates_not_understood = { $site }'s answer was not understood

# Why a check for a new version was not made: the setting Tell me when a new version is out is off.
updates_off = This copy is not told of new versions: turn it on in Settings › General

# Said once, and in About until updated, when a newer version of Skwir is out: version is its number (0.6.2).
updates_out = Skwir { $version } is out

# Said once, and in About until updated, when a newer version is out that fixes a loss of data or a hole in security: version is its number (0.6.2).
updates_out_important = Skwir { $version } is out, with an important fix

updates_plugin_description = Say when a new version of Skwir is out, for a copy downloaded from its website that no package manager updates, and what is new after an update

updates_plugin_title = Updates

# A heading of a section of the settings: being told of new versions of Skwir.
updates_section = Updates

# Why the check for a new version failed: the website's answer, an HTTP status (404). site: the website, as written (skwir.org).
updates_status = { $site } answered { $status }

updates_tell_setting = Tell me when a new version is out

# site: Skwir's website, as written (skwir.org).
updates_tell_setting_description = Skwir asks { $site } once a day whether a newer version is out. Cloudflare, which serves it, sees your IP address.

updates_tell_setting_description_more = Only a copy downloaded from the website asks: one a package manager updates is told by it.

# Said once the question about new versions was answered no: the setting Tell me when a new version is out, in Settings › General.
updates_told_no = Skwir will not ask again; Settings can turn it on

# Said once the question about new versions was answered yes.
updates_told_yes = Skwir will tell you when a new version is out

# The answer to a check for a new version asked for now: no newer version is out.
updates_up_to_date = Skwir is up to date

# Said once, the first time a newer version of Skwir runs: version is its number (0.6.2).
updates_updated = Skwir was updated to { $version }

# The button opening a version's notes on Skwir's website, in the browser.
updates_whats_new = What’s new

updates_whats_new_setting = Say what’s new after an update

updates_whats_new_setting_description = Once Skwir is updated, a message says so once, with a button for the new version’s notes.
