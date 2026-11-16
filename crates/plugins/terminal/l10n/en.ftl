# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A setting's title: whether the terminal and the pane go to the same folder.
terminal_follow = Follow the folder shown

# cd is the shell's command that changes folder.
terminal_follow_description = The terminal changes to the folder the pane shows, and the pane goes where you cd. Nothing is typed while a program runs in the terminal.

terminal_plugin_description = A terminal under the panes (F4) that goes where the pane goes, and takes the pane where you cd

terminal_plugin_title = Terminal

# A setting's title: the program the terminal runs (bash, fish).
terminal_program = Shell

terminal_program_description = The program the terminal runs. Leave empty for your login shell.

# A setting's title: how many lines of what the terminal showed can be scrolled back to.
terminal_scrollback = History

# A setting's description: the most lines it allows (30,000), and how much memory 10,000 lines take.
terminal_scrollback_description = How many lines you can scroll back to, for terminals opened from now on, at most { $most }. Once full, every { $lines } lines of a wide terminal take about 30 MB.

# An error when files are dropped on the terminal: the shell's program (its name, bash, fish) is not one Skwir knows how to quote a path for.
terminal_unknown_quoting = Skwir does not know how { $shell } quotes a path: it types none there

# An error when files are dropped on the terminal: its shell is none Skwir knows how to quote a path for.
terminal_unknown_quoting_unnamed = Skwir does not know how this shell quotes a path: it types none there

# An error when a file is dropped on the terminal: its name holds what a shell's prompt cannot take.
terminal_untypable = { $name } cannot be typed at a prompt
