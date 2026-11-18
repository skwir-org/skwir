# Simplified Chinese catalog of skwir-plugin-shell, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
shell_ask_accept = 运行

# draft
shell_ask_run = 对所选项目运行命令

# draft
shell_ask_run_here = 在此文件夹中运行命令

# draft
shell_ask_run_here_placeholder = 命令；在此文件夹中运行

# draft
shell_ask_run_placeholder = 命令；所选路径作为其参数

# draft
shell_copied_paths = 已复制 { $count } 个路径

# draft
shell_copy_paths = 复制供 shell 使用的路径

# draft
shell_copy_paths_description = 所选路径或给定项目的路径，按 shell 规则加引号，复制到剪贴板

# draft
shell_example_df = 所在磁盘的使用情况

# draft
shell_example_du = 各自占用的空间

# draft
shell_example_du_here = 占用的空间

# draft
shell_example_file = 各自的类型

# draft
shell_example_git = git 给出的状态

# draft
shell_example_ls = 其中的内容，包括隐藏文件

# draft
shell_example_ls_ld = 详细信息

# draft
shell_example_sha = 校验和

# draft
shell_example_wc = 各自的行数

# draft
shell_examples = 示例

# draft
shell_line_more = { $line } … 以及另外 { $more } 个路径

# draft
shell_line_named = { $command }  路径放在它指定的位置：{ $paths }

# draft
shell_line_no_paths = { $command }  不带路径

# draft
shell_no_folder = 这些项目不在任何文件夹中

# draft
shell_plugin_description = 复制所选路径供 shell 使用；在终端中对所选项目运行命令

# draft
shell_plugin_title = Shell

# draft
shell_program = 运行命令的程序

# draft
shell_program_description = 对所选项目运行命令的 shell：POSIX shell 或 fish。

# draft
shell_program_description_more = 留空时，若您的 shell（$SHELL）属于其中之一则使用它，否则使用 sh。

# draft
shell_reference_title = 命令

# draft
shell_run = 对所选项目运行命令…

# draft
shell_run_describe = 说明命令将运行什么

# draft
shell_run_describe_description = 说明 `shell.run` 以 `command` 作用于所选路径或给定项目时将运行什么：带有前几个路径的命令行（按 shell 接收的形式，放在命令之后或其指定参数的位置）、另外还有多少个路径，以及所在文件夹；命令为空或不会运行任何内容时返回错误

# draft
shell_run_description = 在终端中运行 shell 命令，以所选路径或给定项目的路径作为其参数

# draft
shell_run_here = 在此处运行命令…

# draft
shell_run_here_describe = 说明命令在此处将运行什么

# draft
shell_run_here_describe_description = 说明 `shell.run_here` 以 `command` 将运行什么：命令行和所在文件夹；命令为空或不会运行任何内容时返回错误

# draft
shell_run_here_description = 在终端中、在某个文件夹（`in`，否则为窗格显示的文件夹）中运行 shell 命令，不带路径

# draft
shell_run_menu = 运行命令…

# draft
shell_run_not_yet = 在 { $system } 上暂不支持在终端中运行命令

# draft
shell_runs = 在 { $folder } 中运行 { $line }

# draft
shell_runs_here_note = 它在终端中、在此文件夹中运行，后面不带路径，使用“设置 › 终端”指定的 shell，运行后终端保持打开。

# draft
shell_runs_note = 所选路径放在命令之后，每个路径作为一个参数；若命令自己指定了它们的位置（sh、bash 或 zsh 中的 "$@" 或 $1…，fish 中的 $argv），则只放在那里。它在终端中、在这些路径所在的文件夹中运行，使用“设置 › 终端”指定的 shell，运行后终端保持打开。

# draft
shell_write_command = 输入要运行的命令：所选路径作为其参数

# draft
shell_write_command_here = 输入要在此文件夹中运行的命令
