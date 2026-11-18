# Simplified Chinese catalog of skwir-plugin-open-with, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
open_with_already_opens = { $app } 已是{ $kind ->
        [folder] 文件夹
       *[file] { $description } 文件
    }的默认打开方式

# draft
open_with_always_use = 始终用于打开{ $kind ->
        [folder] 文件夹
       *[file] { $description } 文件
    }

# draft
open_with_cancelled = 已取消打开

# draft
open_with_change = 更改…

# draft
open_with_choices = 列出可打开所选项目的应用

# draft
open_with_choices_description = 列出可打开所选项目（或使用 `here` 时窗格所在的文件夹）的应用，推荐的在前，每个附带打开它们的 `open_with` 调用；使用 `defaults` 时，每个附带将其设为该类型默认应用的 `open_with.set_default`

# draft
open_with_chooser = 打开方式

# draft
open_with_chooser_title = { $count ->
        [1] 选择打开{ $name }的应用
       *[other] 选择打开 { $count } 个项目的应用
    }

# draft
open_with_default = 默认

# draft
open_with_default_generic = 默认 · { $generic }

# draft
open_with_does_not_exist = { $path } 不存在

# draft
open_with_edit_associations = 改写某个类型的默认应用条目

# draft
open_with_edit_associations_description = 设置某个 MIME 类型在一个 mimeapps.list 中的条目（默认、已添加和已移除的关联）；默认应用的撤销和重做即执行此命令

# draft
open_with_menu = 在菜单中显示默认应用

# draft
open_with_menu_description = 所选项目或给定文件（预览的：`entries`，按窗格所列）的菜单中的“打开”，显示它们共同的默认应用，并在“打开方式”子菜单中列出可打开其类型的其他应用；类型或应用尚未知时不返回任何条目。从内存中作答

# draft
open_with_no_application = 无应用

# draft
open_with_no_applications = 未找到应用

# draft
open_with_no_location = 窗格中没有位置

# draft
open_with_no_such_pane = 没有这个窗格

# draft
open_with_none_opens = 没有应用可打开{ $kind ->
        [folder] 文件夹
       *[file] { $description } 文件
    }

# draft
open_with_not_a_file = { $name }不是普通文件：Skwir 不会打开它

# draft
open_with_not_changed = 未更改默认应用：{ $error }

# draft
open_with_not_installed = 未安装应用“{ $id }”

# draft
open_with_not_your_list = { $path } 不是您的默认应用列表之一

# draft
open_with_nothing_to_open = 没有要打开的项目

# draft
open_with_now = 当前

# draft
open_with_now_opens = 现在用 { $app } 打开{ $kind ->
        [folder] 文件夹
       *[file] { $description } 文件
    }

# draft
open_with_one_type_at_a_time = 一次只能为一种文件类型选择默认应用

# draft
open_with_open = 打开

# draft
open_with_open_description = 进入所选目录，或用所选文件类型的默认应用（由桌面环境的列表选定，即右键菜单中显示的那个）打开所选文件

# draft
open_with_open_files_with = 选择打开{ $kind ->
        [folder] 文件夹
       *[file] { $description } 文件
    }的应用

# draft
open_with_open_terminal = 在此处打开终端

# draft
open_with_open_terminal_description = 在所选文件夹中打开终端；没有所选文件夹时，在显示的文件夹中打开；在搜索中时，在所选项目所在的文件夹中打开

# draft
open_with_open_terminal_menu = 在此处打开终端

# draft
open_with_open_with = 按桌面 ID 用应用打开

# draft
open_with_open_with_app = 用 { $app } 打开

# draft
open_with_open_with_description = 用桌面文件 ID 为 `app` 的应用打开所选项目（或使用 `here` 时窗格所在的文件夹）

# draft
open_with_opens_with = 打开方式

# draft
open_with_other_application = 其他应用…

# draft
open_with_other_applications = 其他应用

# draft
open_with_plugin_description = 用默认应用或您选择的应用打开文件，以及在此处打开终端

# draft
open_with_plugin_title = 打开方式

# draft
open_with_properties = 在属性中显示默认应用

# draft
open_with_properties_description = 单个文件的属性中的“打开方式”一行：其类型的打开应用，以及更改它的方法

# draft
open_with_recommended = 推荐

# draft
open_with_related = 相关

# draft
open_with_search_applications = 搜索应用

# draft
open_with_set_default = 设置某个类型的默认应用

# draft
open_with_set_default_description = 在用户的 mimeapps.list 中，将桌面文件 ID 为 `app` 的应用设为 MIME 类型 `mime` 的默认应用；通知中提供撤销
