# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# The button that runs the command typed.
shell_ask_accept = Run

shell_ask_run = Run command on selection

shell_ask_run_here = Run command in this folder

shell_ask_run_here_placeholder = Command; it runs in this folder

shell_ask_run_placeholder = Command; the selected paths are its arguments

shell_copied_paths = Copied { $count ->
        [one] { $count } path
       *[other] { $count } paths
    }

shell_copy_paths = Copy paths for a shell

shell_copy_paths_description = The selected paths, or the entries', shell-quoted, for the clipboard

shell_example_df = How full its disk is

shell_example_du = How much each takes

shell_example_du_here = How much it takes

shell_example_file = What each is

shell_example_git = What git says of it

shell_example_ls = What it holds, hidden files too

shell_example_ls_ld = Their details

shell_example_sha = Their checksums

shell_example_wc = How many lines each has

# The heading of commands given as examples under Run Command…'s field.
shell_examples = For example

# line: a command line with the first paths it is given, as its shell reads them; more: how many paths it is given beyond them.
shell_line_more = { $line } … and { $more ->
        [one] { $more } more
       *[other] { $more } more
    }

# command: a command line that names its arguments itself ("$@", $1); paths: the paths it is given, as its shell reads them.
shell_line_named = { $command }  with the paths where it names them: { $paths }

# What a command runs, said as it is typed: command is the command line, given no paths.
shell_line_no_paths = { $command }  with no paths

shell_no_folder = the entries are in no folder

shell_plugin_description = Copy selected paths for a shell; run a command on the selection in a terminal

shell_plugin_title = Shell

shell_program = Program for commands

shell_program_description = The shell that runs a command on the selection: a POSIX shell or fish.

# What a setting's description says after its first sentence, in its tooltip.
shell_program_description_more = Leave empty for yours ($SHELL) when it is one of them, else sh.

# The title of what Run Command…'s field takes.
shell_reference_title = Command

shell_run = Run command on selection…

shell_run_describe = Say what a command runs

shell_run_describe_description = Say what `shell.run` would run with `command` on the selected paths, or the entries': the line with the first paths as its shell is given them (after it, or where it names its arguments), how many more, and the folder; an error for an empty command, or where nothing runs

shell_run_description = Run a shell command in a terminal with the selected paths, or the entries', as its arguments

shell_run_here = Run command here…

shell_run_here_describe = Say what a command runs here

shell_run_here_describe_description = Say what `shell.run_here` would run with `command`: the line and the folder; an error for an empty command, or where nothing runs

shell_run_here_description = Run a shell command in a terminal in a folder (`in`, else the one the pane shows), with no paths

shell_run_menu = Run Command…

# system: Linux, macOS or Windows.
shell_run_not_yet = Running a command in a terminal is not available on { $system } yet

# Said as a command is typed in Run Command…: line is what runs (the command with its paths); folder the path of the folder it runs in.
shell_runs = Runs { $line }  in { $folder }

shell_runs_here_note = It runs in a terminal, in this folder, with no paths after it, with the shell Settings › Terminal names, the terminal staying open after.

shell_runs_note = The selected paths come after it, each one argument, unless it names them itself ("$@" or $1… in sh, bash or zsh, $argv in fish): then they go only there. It runs in a terminal, in the folder holding them, with the shell Settings › Terminal names, the terminal staying open after.

shell_write_command = Write the command to run: the selected paths are its arguments

shell_write_command_here = Write the command to run in this folder
