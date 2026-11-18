# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# What a plugin from outside Skwir answered, too long for a notification: its start, then a mark that more was cut.
extend_answer_cut = { $start }…

# A notification: what a command of a plugin from outside Skwir answered. Its title (the plugin's words), then its answer, as the plugin wrote it.
extend_answered = { $command }: { $value }

# An error for agents and scripts: a command of a plugin from outside Skwir takes its arguments as a JSON object. In lower case.
extend_arguments_object = the arguments are an object

# A notification: a command of a plugin from outside Skwir failed. Its title (the plugin's words), then why.
extend_command_failed = { $command }: { $error }

# An error: the file a plugin from outside Skwir made of another, kept in Skwir's cache, was removed before it was shown. In lower case.
extend_converted_gone = the converted file is gone

# A notification's button: copies the text a plugin from outside Skwir answered.
extend_copy_answer = Copy

# An error's first part, why follows: the program of a plugin from outside Skwir did not say it was ready. In lower case.
extend_did_not_start = the plugin did not start

# An error: the program of a plugin from outside Skwir was asked something as it ended. In lower case.
extend_ending = the plugin is ending

# An error: the program of a plugin from outside Skwir failed to start a few seconds ago, and is not tried again yet. In lower case.
extend_failed_before = the plugin could not start a moment ago

# An error: a plugin from outside Skwir answered the file it made of another, not written as base64 as the protocol asks. In lower case.
extend_file_not_base64 = the plugin's file is no base64

# Why a plugin from outside Skwir written as a WebAssembly module does not start. program: the helper's name (skwir-module-worker).
extend_install_module_worker = Install { $program } to run plugins that are WebAssembly modules

# An error: a plugin from outside Skwir answered a plan, but did not declare it changes files. In lower case.
extend_may_not_write = this plugin may not change files

# An error: the program of a plugin from outside Skwir did not answer within its time. In lower case.
extend_no_answer = the plugin did not answer in time

# An error in place of a page drawn by a plugin from outside Skwir: the page's number, then how many pages the file has.
extend_no_page = There is no page { $page }: it has { $pages }

# An error: a plan answered by a plugin from outside Skwir names a file by a number (its handle) the call gave it none. In lower case.
extend_no_such_file = no file { $handle } in this call

# An error: the program of a plugin from outside Skwir stopped reading. In lower case.
extend_not_reading = the plugin does not read what it is asked

# An error: a plugin from outside Skwir answered a page's picture not written as base64, as the protocol asks. In lower case.
extend_page_not_base64 = the plugin's page is no base64

# Under pages a plugin from outside Skwir (its title, its author's words) drew, which may not use the network: their links are not followed.
extend_pages_links_closed = Links are not followed in pages “{ $plugin }” drew: it may not use the network

# An error: a plugin from outside Skwir was asked something once turned off. In lower case.
extend_plugin_off = the plugin is off

# Under a preview a plugin from outside Skwir (its title, its author's words) made, which may not use the network: its links are not followed.
extend_preview_links_closed = Links are not followed in a preview “{ $plugin }” made: it may not use the network

# An error: a command of a plugin from outside Skwir was given a file the user keeps private. Settings, Privacy is where. In lower case.
extend_private_file = a private file: a plugin from outside Skwir does not see it (Settings, Privacy)

# An error: a plan answered by a plugin from outside Skwir names a folder the user keeps private. In lower case.
extend_private_folder = a private folder: a plugin does not reach it

# In place of a file's preview a plugin from outside Skwir would make: the user keeps the file private. Settings, Privacy is where.
extend_private_preview = This file is private: a plugin from outside Skwir does not read it (Settings, Privacy)

# An error: a plugin from outside Skwir was written for a version of the protocol (how Skwir and plugins talk) Skwir does not speak. The versions are numbers. In lower case.
extend_protocol_version = the plugin speaks version { $version } of the protocol; Skwir speaks { $oldest } to { $newest }

# What a plugin from outside Skwir, a WebAssembly module, needs Skwir's helper for, in the settings' list of what a plugin needs.
extend_requirement_module = Running this plugin's WebAssembly module

# In place of a file's preview: the plugin from outside Skwir (its title, its author's words) made nothing of it.
extend_shows_nothing = “{ $plugin }” shows nothing of this file

# An error: the program of a plugin from outside Skwir ended while it was asked something. In lower case.
extend_stopped = the plugin stopped

# An error: a plugin from outside Skwir answered a thumbnail it did not write as base64, as the protocol asks. In lower case.
extend_thumbnail_not_base64 = a thumbnail that is no base64

# A notification: a plugin from outside Skwir (its title, its author's words) failed again and again, and Skwir turned it off.
extend_turned_off = { $count ->
        [one] The plugin “{ $plugin }” was turned off: it stopped answering
       *[other] The plugin “{ $plugin }” was turned off: it stopped answering { $count } times in a row
    }

# A notification's button: opens the Plugins page of the settings, where a plugin is turned on again.
extend_turned_off_action = Plugins

# An error: a plugin from outside Skwir was turned off while it was asked something. In lower case.
extend_turned_off_meanwhile = the plugin was turned off
