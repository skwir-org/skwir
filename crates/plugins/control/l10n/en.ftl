# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# Read by agents: why this window does not listen for agents and scripts. In lower case.
control_another_listens = another Skwir window listens

# A question in the window: may an AI agent run this command (its title)? The user allows or refuses.
control_ask = An agent asks to run { $command }

# Under the question whether an agent may run a command: the command's id, then what it does.
control_ask_detail = { $command }: { $description }

# Under the question whether an agent may run a command: the command's id, what it does, then on the next line the arguments the agent gave it (JSON).
control_ask_detail_arguments = { $command }: { $description }
    { $arguments }

# A notification once Agents and scripts (a plugin) is turned on: the window cannot listen for them, and why.
control_cannot_listen = Agents and scripts cannot listen: { $error }

# An error's first part, why follows: the folder where the window listens for agents and scripts could not be made.
control_channel_folder = the channel's folder

# Read by agents: the window closed while the user was asked whether their command may run.
control_closed_unasked = Skwir closed before the user answered

# Read by agents: the window closed while the plan they proposed waited for the user's review.
control_closed_unreviewed = Skwir closed before the plan was answered

# Read by agents: why a command was not run.
control_closing = Skwir is closing

# Read by agents: why their command is refused. The path is one the agent named.
control_elsewhere = looking up { $path } would make the system reach another machine, which an agent may not do unasked: the user opens it in Skwir first

# Read by agents: what an AI agent is told of Skwir as it connects. entries and location are arguments' names, as typed.
control_instructions = Skwir, a file manager. Each tool is one of its commands, its dots written as two underscores. Name the files a command acts on in its `entries`, by absolute path: the selection is the user's, and can change between two calls (the `entries` tool lists what a pane shows, or with `location` a folder, without changing what the user sees). A command that would change files proposes a plan the user reviews in Skwir's window; its answer says whether the user applied it, and how, or dismissed it. One that acts otherwise asks the user first. Private folders are never shown to you.

# Read by agents and scripts: why their command is refused. The command's id, as typed; Open and Save are what programs call their dialogs for choosing a file.
control_names_dialog = `{ $command }` names the window answering another program's Open or Save dialog, which only the user answers in Skwir's window

# Read by scripts: the window running is an older Skwir's, which takes no script's command.
control_no_script_endpoint = the window has no script endpoint: restart Skwir before running this command

# Read by agents: the user refused to let their command run.
control_not_allowed = the user did not allow this command to run

# Read by scripts and agents: no window answers. Agents and scripts is a plugin's name; Settings, Plugins is where it is turned on.
control_not_listening = Skwir is not open, or Agents and scripts is off: open Skwir and turn it on in Settings, Plugins

# Read by agents and scripts: a command needs files, named in its entries argument (as typed).
control_nothing_selected = nothing is selected: name the files in `entries`, by absolute path

# Read by agents and scripts: Paste needs files, named in its clipboard.entries argument (as typed).
control_nothing_to_paste = nothing to paste: name the files in `clipboard.entries`, by absolute path, and whether they were cut

# Read by agents: the user has not reviewed the plan they proposed yet.
control_plan_waits = the plan waits for the user's review in Skwir

control_plugin_description = Let AI agents (over MCP: skwir mcp) and scripts (skwir cmd) drive this window. What they would change is proposed for your review. Any program running as you can then use it.

control_plugin_title = Agents and scripts

# Read by agents: why their command is refused. Settings, Privacy is where the user keeps folders from agents.
control_private = this names a private folder, which Skwir keeps from agents (Settings, Privacy)

# Who proposes a plan, in its review: an AI agent.
control_proposer = An agent

# Read by agents: why their command is refused, a path being found from what they gave (a name in the folder shown, a location), which is not said.
control_reads_elsewhere = looking up a path this call reads would make the system reach another machine, which an agent may not do unasked: the user opens it in Skwir first

# Read by agents: why their command is refused, a path being found from what they gave (a name in the folder shown, a location), which is not said. Settings, Privacy is where the user keeps folders from agents.
control_reads_private = a path this call reads is private: Skwir keeps it from agents (Settings, Privacy)

# A command's title: whether this window listens for AI agents and scripts.
control_status = Agents and scripts

# Read by agents: how an AI agent reaches the window. skwir mcp is a command line, as typed.
control_status_agent = an MCP server started as: skwir mcp

# Read by agents and in F1's list of actions.
control_status_description = Whether this window listens for agents and scripts, where, and how an agent reaches it

# Read by agents: how a script runs a command, as typed; only what is between < and > is words.
control_status_script = skwir cmd [--apply] <command> [<arguments, a JSON object>]

# Read by agents: a command's description, for a command an agent runs only once the user allows it.
control_tool_asks = { $description } (asks the user in Skwir's window first)

# Read by agents: the user did not answer whether their command may run.
control_unanswered = the user did not answer: ask again when they are there

# Read by agents and scripts: no command has the id they named, as typed.
control_unknown_command = unknown command `{ $command }`

# Read by agents: why their command is refused. The command's id, as typed.
control_user_only = `{ $command }` answers the user in Skwir's window: only the user runs it
