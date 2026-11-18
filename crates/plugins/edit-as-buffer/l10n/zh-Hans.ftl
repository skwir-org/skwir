# Simplified Chinese catalog of skwir-plugin-edit-as-buffer, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
edit_as_buffer_command = 以文本形式编辑…

# draft
edit_as_buffer_command_description = 在 $EDITOR 中打开列表；保存后会提出一个计划供审阅

# draft
edit_as_buffer_duplicate_handle = 第 { $line } 行：编号 { $handle } 出现了两次

# draft
edit_as_buffer_duplicate_target = 多行的目标都是 `{ $name }`

# draft
edit_as_buffer_editor_exited = 编辑器已退出（{ $status }）

# draft
edit_as_buffer_editor_failed = 编辑器运行失败

# draft
edit_as_buffer_empty_name = 名称不能为空

# draft
edit_as_buffer_failed = 以文本形式编辑：{ $error }

# draft
edit_as_buffer_header = # Skwir：编辑此列表，保存并退出。
    #   修改名称            -> 重命名
    #   删除一行            -> 移到回收站
    #   名称中含斜杠        -> 移入该文件夹（必要时创建）
    #   以 / 结尾的新行     -> 新建文件夹
    # 保留每行开头的编号。行的顺序无关紧要。
    # 名称中 \\ 表示反斜杠，\xHH 表示一个原始字节（\x0a 为换行，
    # \x20 为位于开头或结尾的空格）。

# draft
edit_as_buffer_invalid_name = 第 { $line } 行：无效的名称 `{ $name }`：{ $reason }

# draft
edit_as_buffer_menu = 以文本形式编辑…

# draft
edit_as_buffer_new_file = 第 { $line } 行：不支持新建文件（`{ $name }`）；要新建文件夹，请以 / 结束该行

# draft
edit_as_buffer_no_changes = 以文本形式编辑：没有更改

# draft
edit_as_buffer_no_pane = 没有这个窗格

# draft
edit_as_buffer_nothing_open = 窗格中没有打开的位置

# draft
edit_as_buffer_outside = 路径必须位于此文件夹内

# draft
edit_as_buffer_plan = 编辑 { $folder }

# draft
edit_as_buffer_plugin_description = 在您的编辑器中编辑列表；所做的改动将成为一个待审阅的计划

# draft
edit_as_buffer_plugin_title = 以文本形式编辑

# draft
edit_as_buffer_taken = 第 { $line } 行：`{ $name }` 已存在

# draft
edit_as_buffer_unknown_handle = 第 { $line } 行：未知编号 { $handle }

# draft
edit_as_buffer_unreadable = 无法读回编辑后的内容
