# Simplified Chinese catalog of skwir-plugin-archives, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
archives_already_extracted_into = { $archive }已解压到 { $into } 中

# draft
archives_already_extracted_to = { $archive }已解压到 { $into }

# draft
archives_already_held = { $count ->
        [1] { $archive }中已有{ $name }
       *[other] { $archive }中已有 { $count } 个项目
    }

# draft
archives_already_there = { $name }已存在

# draft
archives_among_inputs = { $name }属于要放入压缩文件的内容

# draft
archives_ask_as = { $count ->
        [1] 将{ $name }压缩为
       *[other] 将 { $count } 个项目压缩为
    }

# draft
archives_ask_as_note = 其扩展名决定格式：{ $endings }

# draft
archives_ask_with_password = { $count ->
        [1] 使用密码将{ $name }压缩为
       *[other] 使用密码将 { $count } 个项目压缩为
    }

# draft
archives_ask_with_password_note = 加密的 ZIP 压缩文件：接下来会询问其密码

# draft
archives_attributes_unread = 无法读取{ $name }的标签和属性

# draft
archives_cancelled_compress = 已取消：未压缩任何内容

# draft
archives_cancelled_extract = 已取消：未解压任何内容

# draft
archives_cannot_list_whole = 无法完整列出{ $name }的内容

# draft
archives_choose_format = 选择格式…

# draft
archives_column_name = 名称

# draft
archives_column_size = 大小

# draft
archives_column_status = 状态

# draft
archives_compress = 压缩

# draft
archives_compress_as = { $count ->
        [1] 将{ $name }压缩为{ $archive }
       *[other] 将 { $count } 个项目压缩为{ $archive }
    }

# draft
archives_compress_ask = 压缩…

# draft
archives_compress_ask_description = 询问所选项目的压缩文件名称（其扩展名决定格式），然后创建该压缩文件

# draft
archives_compress_button = 压缩

# draft
archives_compress_description = 将所选项目或给定的项目制作成压缩文件，放在其旁边或 `dest` 中：格式由 `name` 的结尾决定（.zip、.7z、.tar.gz、.tar.xz、.tar.zst、.tar.bz2、.tar），否则由 `format` 决定，再否则为 ZIP（计划）

# draft
archives_compress_plan = { $count ->
        [1] 压缩{ $name }
       *[other] 压缩 { $count } 个项目
    }

# draft
archives_compressed_as = { $count ->
        [1] 已将{ $name }压缩为{ $archive }
       *[other] 已将 { $count } 个项目压缩为{ $archive }
    }

# draft
archives_detail_encrypted = 已加密

# draft
archives_detail_files = 文件

# draft
archives_detail_folders = 文件夹

# draft
archives_detail_unpacked = 解压后大小

# draft
archives_encrypted_members = { $count } 个成员

# draft
archives_endings = .zip、.7z、.tar.gz、.tar.xz、.tar.zst、.tar.bz2 或 .tar

# draft
archives_extract = 解压到此处

# draft
archives_extract_button = 解压

# draft
archives_extract_description = 将所选项目或给定项目中的每个压缩文件解压到其旁边或 `dest` 中：只含一个文件夹或文件的压缩文件解压为该文件夹或文件，含多个的则解压到以其命名的文件夹中（计划）

# draft
archives_extract_here = 解压到此处

# draft
archives_extract_into = 将{ $archive }解压到已有的 { $into } 中

# draft
archives_extract_plan = { $count ->
        [1] 解压{ $name }
       *[other] 解压 { $count } 个压缩文件
    }

# draft
archives_extract_to = 将{ $archive }解压到 { $into }

# draft
archives_extract_verb = 解压

# draft
archives_extracted_into = 已将{ $archive }解压到已有的 { $into } 中

# draft
archives_extracted_to = 已将{ $archive }解压到 { $into }

# draft
archives_first_members = { $count ->
        [1] 仅列出了第一个成员 · 在压缩文件应用中打开它以查看其余内容
       *[other] 仅列出了前 { $count } 个成员 · 在压缩文件应用中打开它以查看其余内容
    }

# draft
archives_folder_not_there = { $name }要放入的文件夹不存在

# draft
archives_format_differs = { $name }表明格式为 { $said }，而所要求的格式为 { $asked }

# draft
archives_holds_archive = { $there }中有{ $archive }，即要解压的压缩文件

# draft
archives_holds_input = { $there }中有{ $input }，它是要放入压缩文件的内容

# draft
archives_in_a_folder = 压缩文件须位于文件夹中

# draft
archives_member_not_previewed = 无法预览此成员（目录、链接或加密文件）

# draft
archives_no_format = { $name }未表明格式：请以 { $endings } 结尾

# draft
archives_no_longer_there = { $name }已不存在

# draft
archives_not_a_folder = { $name }不是文件夹

# draft
archives_not_a_folder_to_extract_into = { $name }不是可解压到其中的文件夹

# draft
archives_not_an_archive = { $name }不是压缩文件

# draft
archives_not_in_archive = { $member }已不在{ $archive }中

# draft
archives_not_placed = 无法将{ $name }放入{ $folder }

# draft
archives_not_writable = 无法写入{ $name }：不会向其中解压任何内容

# draft
archives_nothing_compressed = 在您做出选择之前，不会压缩任何内容。

# draft
archives_nothing_extracted = 在您做出选择之前，不会解压任何内容。

# draft
archives_nothing_to_compress = 没有可压缩的内容

# draft
archives_only_zip_password = 只有 ZIP 压缩文件可以设置密码

# draft
archives_only_zip_password_end = 只有 ZIP 压缩文件可以设置密码：请让名称以 .zip 结尾

# draft
archives_password_again = 再次输入相同的密码以确认

# draft
archives_password_differed = 两次输入的密码不一致：请输入{ $name }的密码

# draft
archives_password_empty = { $name }的密码：密码不能为空

# draft
archives_password_for = { $name }的密码

# draft
archives_password_new = 为{ $name }设置密码

# draft
archives_password_new_empty = 为{ $name }设置密码：密码不能为空

# draft
archives_password_next = 下一步

# draft
archives_password_unasked = { $name }将被加密：其密码只能在 Skwir 的窗口中输入

# draft
archives_plugin_description = 列出 ZIP、tar、7z 等压缩文件的内容，无需解压即可预览其成员，解压它们（压缩文件的菜单和拖放时提供的“解压到此处”），以及创建压缩文件（所选内容菜单中的“压缩”）

# draft
archives_plugin_title = 压缩文件

# draft
archives_requirement = 列出、读取、解压和创建压缩文件

# draft
archives_status_encrypted = 已加密

# draft
archives_status_link = 链接/特殊文件

# draft
archives_through_link = { $name }是链接：不会通过它解压任何内容

# draft
archives_title = 压缩文件 · { $count } 个成员

# draft
archives_to_zip = 压缩为 ZIP

# draft
archives_too_deep = 压缩文件中的路径超过 { $count } 层

# draft
archives_two_as_one = 两个项目会被解压为同一个{ $name }

# draft
archives_unasked = { $name }已加密：其密码只能在 Skwir 的窗口中输入

# draft
archives_unknown_format = { $format }不是此处可用的格式：请使用 { $endings } 之一

# draft
archives_unreadable = 无法读取{ $name }

# draft
archives_went_missing = { $name }在生成时丢失

# draft
archives_with_password = 使用密码…

# draft
archives_would_leave = 解压到{ $name }中的内容会落到该文件夹之外

# draft
archives_wrong_password = { $name }的密码错误：请重试
