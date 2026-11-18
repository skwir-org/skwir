# The template's English (protocol 2, ADR-139, 14), written by its author as
# every plugin from outside writes its own: never made from its code. The
# manifest names its messages (`title = { id = "plugin-title" }`), and so do
# its answers (`Words::message("count-said")`). A translation is
# `<language>.ftl` beside this file, its messages by the same ids; what it
# lacks is said in English, and what this file lacks as its id.

# The plugin's name, on Settings › Plugins.
plugin-title = Word count

# What it does, under its name on Settings › Plugins.
plugin-description = How many words a text holds, in Properties, and for the selection

# What Properties calls the field: how many words the file holds.
words-label = Words

# A command, in the palette: counts the words the selected files hold.
count-title = Count words

# What the command does, in the palette and for agents.
count-description = How many words the files hold, each and in all: the selection, or the entries given

# A command, in the palette: how many words the selected files hold in all, as a number for the clipboard.
copy-title = Copy word count

# What the command does, in the palette and for agents.
copy-description = How many words the files hold in all, as a number for the clipboard: the selection, or the entries given

# Said with the number the command gives for the clipboard. count: the words the files hold in all.
count-said = { $count ->
        [one] One word in all
       *[other] { $count } words in all
    }

# An error: the plugin was asked for a command it does not have. command: the command's name, as written. In lower case.
no-command = no command { $command }
