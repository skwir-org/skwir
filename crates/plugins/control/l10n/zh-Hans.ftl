# Simplified Chinese catalog of skwir-plugin-control, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
control_another_listens = 另一个 Skwir 窗口正在监听

# draft
control_ask = 一个智能体请求运行{ $command }

# draft
control_ask_detail = { $command }：{ $description }

# draft
control_ask_detail_arguments = { $command }：{ $description }
    { $arguments }

# draft
control_cannot_listen = 智能体与脚本无法监听：{ $error }

# draft
control_channel_folder = 通道所在的文件夹

# draft
control_closed_unasked = 用户作答之前 Skwir 已关闭

# draft
control_closed_unreviewed = 计划得到答复之前 Skwir 已关闭

# draft
control_closing = Skwir 正在关闭

# draft
control_elsewhere = 查找 { $path } 会使系统访问另一台计算机，智能体未经请求不得这样做：请用户先在 Skwir 中打开它

# draft
control_instructions = Skwir，一款文件管理器。每个工具都是它的一个命令，命令中的点写作两个下划线。在命令的 `entries` 中用绝对路径指明它要处理的文件：所选内容由用户决定，两次调用之间可能改变（`entries` 工具列出窗格显示的内容，或配合 `location` 列出某个文件夹的内容，不会改变用户看到的内容）。会更改文件的命令会提出一个计划，由用户在 Skwir 的窗口中审阅；其回答会说明用户是否应用了计划、如何应用，或是否忽略了它。以其他方式起作用的命令会先询问用户。私密文件夹永远不会向您显示。

# draft
control_names_dialog = `{ $command }` 指向应答其他程序“打开”或“保存”对话框的窗口，只有用户才能在 Skwir 的窗口中应答它

# draft
control_no_script_endpoint = 该窗口没有脚本端点：请先重启 Skwir 再运行此命令

# draft
control_not_allowed = 用户不允许运行此命令

# draft
control_not_listening = Skwir 未打开，或“智能体与脚本”已停用：请打开 Skwir，并在“设置 › 插件”中启用它

# draft
control_nothing_selected = 未选择任何项目：请在 `entries` 中用绝对路径指明文件

# draft
control_nothing_to_paste = 没有可粘贴的内容：请在 `clipboard.entries` 中用绝对路径指明文件，并说明它们是否被剪切

# draft
control_plan_waits = 该计划正在 Skwir 中等待用户审阅

# draft
control_plugin_description = 让 AI 智能体（通过 MCP：skwir mcp）和脚本（skwir cmd）操控此窗口。它们要做的更改会先提议给您审阅。之后，以您的身份运行的任何程序都可以使用它。

# draft
control_plugin_title = 智能体与脚本

# draft
control_private = 这里指明了一个私密文件夹，Skwir 不让智能体访问它（设置 › 隐私）

# draft
control_proposer = 一个智能体

# draft
control_status = 智能体与脚本

# draft
control_status_agent = 以如下命令启动的 MCP 服务器：skwir mcp

# draft
control_status_description = 此窗口是否在监听智能体与脚本、在何处监听，以及智能体如何访问它

# draft
control_status_script = skwir cmd [--apply] <命令> [<参数，一个 JSON 对象>]

# draft
control_tool_asks = { $description }（会先在 Skwir 的窗口中询问用户）

# draft
control_unanswered = 用户没有作答：请在用户在场时再次询问

# draft
control_unknown_command = 未知命令 `{ $command }`

# draft
control_user_only = `{ $command }` 在 Skwir 的窗口中向用户作答：只能由用户运行
