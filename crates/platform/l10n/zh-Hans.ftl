# Simplified Chinese catalog of skwir-platform, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
access_acl = 访问控制列表

# draft
access_acl_default = 新项目的默认权限

# draft
access_acl_group = 组 { $group }：{ $permissions }

# draft
access_acl_mask = 最多 { $permissions }

# draft
access_acl_others = 其他人：{ $permissions }

# draft
access_acl_owner = 所有者：{ $permissions }

# draft
access_acl_owning_group = 所属组：{ $permissions }

# draft
access_acl_user = 用户 { $user }：{ $permissions }

# draft
access_capabilities = 能力

# draft
apps_launch_failed = 无法启动 { $app }：{ $error }

# draft
apps_list_read_only = 默认应用程序列表是只读的；请在管理它的地方修改

# draft
apps_no_command = { $app } 没有可运行的命令

# draft
apps_no_config_dir = 没有配置目录

# draft
archive_changed = 压缩文件在列出后已更改；请重新选择它

# draft
archive_gone = 压缩文件已不存在

# draft
archive_handing_failed = 移交文件失败

# draft
archive_helper_exited = 压缩文件辅助程序意外退出

# draft
archive_helper_failed = 压缩文件辅助程序失败（{ $status }）

# draft
archive_helper_not_started = 压缩文件辅助程序无法启动

# draft
archive_helper_timed_out = 压缩文件辅助程序超时

# draft
archive_install = 安装 { $program } 以打开压缩文件

# draft
archive_member_missing = 压缩文件中没有此文件

# draft
archive_member_nowhere = 没有可写入此文件的位置

# draft
archive_member_too_large = 此文件超过 { $size } 的预览上限

# draft
archive_nowhere = 没有可写入压缩文件的位置

# draft
archive_part_changed = { $name }在读取时被更改

# draft
archive_part_unread = 无法读取{ $name }

# draft
archive_password_zip_only = 只有 ZIP 压缩文件可以设置密码

# draft
archive_quarantine_unheld = 无法将{ $name }置于隔离状态

# draft
archive_quarantine_unread = 无法读取压缩文件的隔离信息

# draft
askpass_unsupported = 此系统暂不支持通过 ssh 询问密码

# draft
desktop_dock_percent = { $percent }%

# draft
desktop_notice_show = 显示

# draft
desktop_notice_stop = 全部停止

# draft
desktop_open = 打开 Skwir

# draft
desktop_style_portal = 桌面门户

# draft
desktop_tray_go_on = 继续

# draft
desktop_tray_open = 打开 Skwir

# draft
desktop_tray_pause = 全部暂停

# draft
desktop_tray_stop = 全部停止并退出

# draft
diagram_failed = 无法绘制图表：{ $said }

# draft
diagram_install = 安装 { $program } 以绘制图表

# draft
diagram_too_long = 图表太长，无法绘制

# draft
document_changed = 文档已更改；请重新打开预览

# draft
document_compose_folder = 无法创建用于合成文档的文件夹

# draft
document_compose_keep = 无法保存合成的文档

# draft
document_locate_too_long = 要定位的文本超出上限

# draft
document_reopen = 文档工作进程已重新启动，但未打开该文档

# draft
document_search_too_long = 搜索内容超出上限

# draft
document_worker_exited = 文档工作进程意外退出

# draft
document_worker_stopped = 文档工作进程在启动时停止

# draft
document_worker_timed_out = 文档工作进程超时

# draft
exif_aperture = 光圈

# draft
exif_aperture_value = f/{ $f }

# draft
exif_artist = 作者

# draft
exif_camera = 相机

# draft
exif_copyright = 版权

# draft
exif_exposure = 曝光时间

# draft
exif_exposure_fraction = 1/{ $denominator } 秒

# draft
exif_exposure_seconds = { $seconds } 秒

# draft
exif_flash = 闪光灯

# draft
exif_focal_length = 焦距

# draft
exif_focal_length_value = { $mm } mm

# draft
exif_iso = ISO

# draft
exif_latitude_longitude = { $latitude }, { $longitude }

# draft
exif_lens = 镜头

# draft
exif_orientation = 方向

# draft
exif_position = 位置

# draft
exif_software = 软件

# draft
exif_taken = 拍摄时间

# draft
file_no_folder = { $path } 没有上级文件夹

# draft
git_bare = 裸仓库没有检出任何文件

# draft
git_history_failed = 无法读取历史记录：{ $said }

# draft
git_install_history = 安装 { $program } 以显示提交历史

# draft
git_install_status = 安装 { $program } 以显示文件的 Git 状态

# draft
git_not_commit = 不是提交：{ $id }

# draft
git_patch_not_written = 未写入补丁

# draft
git_status_failed = 无法读取文件的 Git 状态：{ $said }

# draft
grammars_config_invalid = 其配置不是有效的 JSON（{ $error }），因此不会读取其中列出的任何解析器目录

# draft
grammars_config_unreadable = 无法读取其配置，因此不会读取其中列出的任何解析器目录

# draft
grammars_emacs_no_colors = Emacs 的语法不附带着色规则：请将该语法的仓库放入 Skwir 的语法文件夹（无需构建）

# draft
grammars_library_outside = 其库 { $library } 位于其文件夹之外

# draft
grammars_library_refused = { $source ->
        [skwir] Skwir 文件夹中的库
        [tree-sitter] tree-sitter CLI 的库
        [zed] Zed 的库
        [helix] Helix 的库
        [neovim] Neovim 的库
        [emacs] Emacs 的库
       *[system] 系统的库
    }无法使用（{ $why }）

# draft
grammars_no_highlights = 未找到其语言的高亮查询：请将该语法的仓库放入 Skwir 的语法文件夹（无需构建）

# draft
grammars_no_highlights_repository = 其仓库中没有高亮查询，系统中也没有

# draft
grammars_no_library = 没有库：请构建该语法（`tree-sitter build`）

# draft
grammars_parser_dir_missing = 其配置列出了此解析器目录，但该目录不存在

# draft
grammars_queries_not_lent = 未随其安装查询，而 { $editor ->
        [zed] Zed 的
        [helix] Helix 的
        [emacs] Emacs 的
       *[neovim] Neovim 的
    }查询不能借给其他程序的库使用：请将该语法的仓库放入 Skwir 的语法文件夹（无需构建）

# draft
grammars_repository_queries_refused = { $source ->
        [skwir] 其仓库的查询与 Skwir 文件夹中的库不匹配（{ $why }）：请将仓库检出到构建其库时所用的版本
        [tree-sitter] 其仓库的查询与 tree-sitter CLI 的库不匹配（{ $why }）：请将仓库检出到构建其库时所用的版本
        [zed] 其仓库的查询与 Zed 的库不匹配（{ $why }）：请将仓库检出到构建其库时所用的版本
        [helix] 其仓库的查询与 Helix 的库不匹配（{ $why }）：请将仓库检出到构建其库时所用的版本
        [neovim] 其仓库的查询与 Neovim 的库不匹配（{ $why }）：请将仓库检出到构建其库时所用的版本
        [emacs] 其仓库的查询与 Emacs 的库不匹配（{ $why }）：请将仓库检出到构建其库时所用的版本
       *[system] 其仓库的查询与系统的库不匹配（{ $why }）：请将仓库检出到构建其库时所用的版本
    }

# draft
grammars_source_label = { $source ->
        [skwir] Skwir
        [tree-sitter] tree-sitter CLI
        [zed] Zed
        [helix] Helix
        [neovim] Neovim
        [emacs] Emacs
       *[system] 系统
    }

# draft
grammars_source_short = { $source ->
        [skwir] Skwir 的文件夹
        [tree-sitter] tree-sitter CLI
        [zed] Zed
        [helix] Helix
        [neovim] Neovim
        [emacs] Emacs
       *[system] 系统
    }

# draft
grammars_system_queries_refused = { $source ->
        [skwir] 系统的查询与 Skwir 文件夹中的库不匹配（{ $why }）：请将该语法的仓库（检出到构建其库时所用的版本）放入 Skwir 的语法文件夹
        [tree-sitter] 系统的查询与 tree-sitter CLI 的库不匹配（{ $why }）：请将该语法的仓库（检出到构建其库时所用的版本）放入 Skwir 的语法文件夹
        [zed] 系统的查询与 Zed 的库不匹配（{ $why }）：请将该语法的仓库（检出到构建其库时所用的版本）放入 Skwir 的语法文件夹
        [helix] 系统的查询与 Helix 的库不匹配（{ $why }）：请将该语法的仓库（检出到构建其库时所用的版本）放入 Skwir 的语法文件夹
        [neovim] 系统的查询与 Neovim 的库不匹配（{ $why }）：请将该语法的仓库（检出到构建其库时所用的版本）放入 Skwir 的语法文件夹
        [emacs] 系统的查询与 Emacs 的库不匹配（{ $why }）：请将该语法的仓库（检出到构建其库时所用的版本）放入 Skwir 的语法文件夹
       *[system] 系统的查询与系统的库不匹配（{ $why }）：请将该语法的仓库（检出到构建其库时所用的版本）放入 Skwir 的语法文件夹
    }

# draft
grammars_too_deep_cli = 多嵌套了一层文件夹：它只读取直接位于 { $folder } 中的文件夹；请将 { $inner } 添加到其 parser-directories，或将仓库链接到 Skwir 的语法文件夹

# draft
grammars_too_deep_skwir = 多嵌套了一层文件夹：Skwir 只读取直接位于其中的文件夹（{ $folder }）；请将仓库移动或链接到那里

# draft
highlight_bat_first = bat 优先

# draft
highlight_block_too_long = 代码块超过 { $size }

# draft
highlight_default = 默认

# draft
highlight_not_found = { $theme }（未找到）

# draft
highlight_tree_sitter_first = Tree-sitter 优先

# draft
inspection_blocks_notice = { $count ->
        [1] 仅显示第一个块 · 在“源代码”中查看其余部分
       *[other] 仅显示前 { $count } 个块 · 在“源代码”中查看其余部分
    }

# draft
inspection_count = { $count }

# draft
inspection_drawing_inflated = 图形太大，无法读取

# draft
inspection_drawing_too_large = 图形超过 { $size } 的预览上限

# draft
inspection_embedded_image_limit = 嵌入的图像超过 { $size } 的预览上限

# draft
inspection_hex_offset = 偏移量

# draft
inspection_image_too_large = 嵌入的图像超过 { $size }

# draft
inspection_lines = { $count } 行

# draft
inspection_not_regular = 预览需要普通文件

# draft
inspection_read_as = 按 { $encoding } 读取

# draft
inspection_show_on_map = 在地图上显示

# draft
inspection_svg_no_size = SVG 图片没有尺寸

# draft
inspection_text_too_large = 文本超过 { $size } 的预览上限

# draft
inspection_xml_doctype = 无法读取其文档类型声明

# draft
inspection_xml_entities = 其实体展开后超过 { $size }

# draft
inspection_xml_too_deep = 其元素嵌套超过 { $count } 层

# draft
line_channel_closed = 工作进程已关闭其请求通道

# draft
line_channel_not_reading = 工作进程未读取其请求

# draft
line_channel_start = 无法启动 { $program }

# draft
math_failed = 无法排版公式：{ $said }

# draft
math_install = 安装 { $program } 以排版公式

# draft
media_control_refused = 播放器无法执行该操作：{ $why }

# draft
media_controls_busy = 视频控制繁忙或会话已关闭

# draft
media_frame_slot = 无法创建视频帧缓冲区

# draft
media_worker_exited = 视频工作进程已退出

# draft
media_worker_stopped = 视频工作进程在启动时停止

# draft
media_worker_timed_out = 视频工作进程超时

# draft
mime_kind_archives = 压缩文件

# draft
mime_kind_audio = 音乐和音频

# draft
mime_kind_documents = 文档

# draft
mime_kind_folders = 文件夹

# draft
mime_kind_fonts = 字体

# draft
mime_kind_images = 图像

# draft
mime_kind_presentations = 演示文稿

# draft
mime_kind_programs = 程序

# draft
mime_kind_spreadsheets = 电子表格

# draft
mime_kind_text = 文本和代码

# draft
mime_kind_videos = 视频

# draft
mime_unnamed = 文件

# draft
mime_unnamed_extension = { $extension } 文件

# draft
model_failed = 无法绘制模型：{ $said }

# draft
model_install = 安装 { $program } 以绘制 3D 模型

# draft
mp4_position = 位置

# draft
open_launch_failed = 无法启动 { $program }：{ $error }

# draft
open_no_command = 没有要运行的命令

# draft
open_no_terminal = 未找到终端模拟器（请设置 $TERMINAL）

# draft
open_not_geo = 不是地图位置

# draft
open_not_web_or_mail = 此处只能打开网页链接和邮件链接

# draft
open_path_failed = 无法打开 { $path }：{ $error }

# draft
open_terminal_program_not_yet = { $system } 暂不支持在终端中运行程序

# draft
places_bitlocker_asks = Windows 会自行询问 BitLocker 驱动器的密码：请打开该驱动器

# draft
places_bookmarks_read_only = 书签文件是只读的；请在管理它的地方修改

# draft
places_diskutil_no_mount_point = diskutil 未说明将卷挂载到了何处

# draft
places_diskutil_unrun = 无法运行 diskutil：{ $error }

# draft
places_drive = { $label } ({ $letter })

# draft
places_local_disk = 本地磁盘 ({ $letter })

# draft
places_mount_not_yet = { $system } 暂不支持挂载卷

# draft
places_no_config_dir = 没有配置目录

# draft
places_passphrase_refused = 密码短语未被接受

# draft
places_root = 根目录

# draft
places_system_asks_not = 此系统不会自行询问卷的密码短语

# draft
places_udisks_no_device = udisksctl 未给出已解锁设备的名称

# draft
places_udisks_no_mount_point = udisksctl 未说明将卷挂载到了何处

# draft
places_udisks_unrun = 无法运行 udisksctl：{ $error }

# draft
portals_changed_since = 自 Skwir 写入后该文件已被修改，因此保持原样

# draft
portals_linked = 它是指向 { $target } 的链接，由另一个程序维护：请在那里选择

# draft
portals_no_config_folder = 没有可写入此选择的配置文件夹

# draft
portals_not_utf8 = 它不是以 UTF-8 编写的

# draft
preview_changed = 预览期间文件已更改

# draft
preview_changed_converting = 转换期间文件已更改

# draft
preview_changed_opening = 打开文档期间文件已更改

# draft
preview_install_audio = 安装 { $program } 和 GStreamer 的播放插件以播放此文件

# draft
preview_install_engine = 安装 { $program } 及其库，以便用 { $engine } 阅读文档

# draft
preview_install_video = 安装 { $program } 和 GStreamer 的播放插件以播放此文件，或将“视频引擎”设为“静止画面”

# draft
preview_no_pages = PDF 中没有可读取的页面

# draft
preview_office_no_pdf = LibreOffice 未生成 PDF（文件可能已加密或不受支持）

# draft
preview_office_not_pdf = 转换未生成 PDF

# draft
preview_output_too_large = 预览输出太大

# draft
preview_pdfium_library_missing = PDFium 的库不在设置中指定的位置“{ $path }”（设置 › 阅读 › 文档 › PDFium 库）

# draft
preview_too_wide = 预览尺寸超出上限

# draft
preview_unknown_backend = 未知的 PDF 后端；请选择 auto、pdfium、poppler 或 mupdf

# draft
print_accept = 打印

# draft
print_dialog_ended = 打印对话框已结束（{ $code }）

# draft
print_dialog_pages = 打印对话框的页面范围“{ $ranges }”：{ $refused }

# draft
print_dialog_stopped = 打印对话框已停止

# draft
print_job_empty_pdf = 打印辅助程序生成了空的 PDF

# draft
print_job_failed = 无法生成 PDF（{ $status }）：{ $said }

# draft
print_job_install = 安装 { $program } 以打印

# draft
print_job_lines_cut = { $count ->
        [1] 仅打印第一行
       *[other] 仅打印前 { $count } 行
    }

# draft
print_job_no_pdf = 打印辅助程序未生成 PDF

# draft
print_job_silent = 打印辅助程序没有给出任何回应

# draft
print_job_unwritten = 无法写入 { $path }

# draft
print_needs_pdfium = 打印需要 PDFium，但未找到它

# draft
print_no_dialog = 此桌面没有打印对话框（其打印门户：{ $error }）

# draft
print_no_token = 打印对话框未返回令牌

# draft
print_not_taken = 桌面的打印服务未接受该任务（{ $code }）

# draft
print_not_yet = { $system } 暂不支持打印

# draft
print_portal_silent = 打印门户已停止响应

# draft
print_portal_stopped = 打印门户已停止

# draft
print_portal_unheard = 无法监听打印门户

# draft
print_refused = 打印机拒绝了该任务：{ $error }

# draft
print_settings_short = 打印机的设置不完整

# draft
process_failed = { $program } 失败（{ $status }）：{ $said }

# draft
process_not_installed = 未安装 { $program }

# draft
process_older_build = { $error }。{ $program } 来自较旧的 Skwir 构建，这可能是原因所在：请重新构建或重新安装它

# draft
process_other_build = { $error }。{ $program } 来自另一个 Skwir 构建（{ $theirs }；Skwir 为 { $ours }），这可能是原因所在：请重新构建或重新安装它

# draft
process_stalled_minutes = { $program } 已停止：已有 { $count } 分钟未报告进度

# draft
process_stalled_seconds = { $program } 已停止：已有 { $count } 秒未报告进度

# draft
process_start = 无法启动 { $program }

# draft
process_timed_out = { $program } 超时

# draft
process_too_much_input = { $program }：输入太大

# draft
process_too_much_output = { $program }：输出过多

# draft
rich_preview_page_no_size = 页面没有尺寸

# draft
rich_preview_stylesheet_limit = 样式表超过 { $size } 的预览上限

# draft
sandbox_converter_not_yet = { $system } 暂不支持在与您的文件隔离的环境中运行转换器

# draft
sandbox_needs_bubblewrap = 此功能需要 bubblewrap，以便在与您的文件隔离的环境中运行

# draft
sandbox_needs_sandbox = 此功能需要系统的沙盒，以便在与您的文件隔离的环境中运行

# draft
sandbox_plugin_not_yet = { $system } 暂不支持在与您的文件隔离的环境中运行插件的程序

# draft
secrets_no_keyring = 没有密钥环响应：{ $error }

# draft
secrets_not_text = 密钥环中的机密不是文本

# draft
secrets_unsupported = 此系统暂不支持任何密钥环

# draft
shares_login_refused = 服务器拒绝了登录

# draft
shares_netfs_not_yet = { $system } 暂不支持通过 NetFS 挂载共享

# draft
shares_no_address = “{ $address }”不是系统能识别的地址

# draft
shares_no_mount_point = NetFS 未给出挂载点

# draft
shares_windows_not_yet = { $system } 暂不支持 Windows 共享

# draft
shell_environment_failed = { $shell } 以 { $status } 结束

# draft
shrink_copy_unread = 无法读取精简辅助程序生成的副本

# draft
shrink_empty_copy = 精简辅助程序生成了空的副本

# draft
shrink_failed = 无法制作精简副本（{ $status }）：{ $said }

# draft
shrink_gone = { $path } 已不存在

# draft
shrink_install = 安装 { $program } 以制作精简副本

# draft
shrink_no_copy = 精简辅助程序未生成副本

# draft
shrink_no_smaller = 精简辅助程序生成的副本并不比原文件小

# draft
shrink_silent = 精简辅助程序没有给出任何回应

# draft
shrink_unread = 无法读取 { $path }

# draft
shrink_unwritten = 无法写入 { $path }

# draft
tree_sitter_ended = tree-sitter 辅助程序已结束

# draft
tree_sitter_ended_why = tree-sitter 辅助程序已结束：{ $why }

# draft
tree_sitter_exited = tree-sitter 辅助程序意外退出

# draft
tree_sitter_install = 安装 { $program } 以用 tree-sitter 语法为代码着色

# draft
tree_sitter_no_answer = tree-sitter 辅助程序在 { $count } 秒内未响应

# draft
tree_sitter_not_started = tree-sitter 辅助程序在 { $count } 秒内未启动

# draft
tree_sitter_timed_out = tree-sitter 辅助程序超时

# draft
tree_sitter_too_large = 文本超过 { $size }

# draft
tree_sitter_too_many = 找到的语法多于 tree-sitter 辅助程序一次可接收的数量

# draft
windows_no_user = Windows 未说明当前用户是谁

# draft
windows_unlock_silent = Windows 的解锁对话框没有响应

# draft
windows_unlock_unopened = 无法打开 Windows 的解锁对话框：{ $error }
