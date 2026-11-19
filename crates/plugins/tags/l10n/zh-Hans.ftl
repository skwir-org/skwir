# Simplified Chinese catalog of skwir-plugin-tags, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
tags_add = 添加标签

# draft
tags_add_description = 为给定的项目或所选项目添加标签（计划）

# draft
tags_add_plan = 添加标签{ $tags }

# draft
tags_add_remove_plan = 添加标签{ $add }并移除标签{ $remove }

# draft
tags_add_tag_plan = 添加标签{ $tag }

# draft
tags_all_tags = 所有标签

# draft
tags_already_called = 该标签已经名为{ $to }

# draft
tags_aside_no_name = 它没有可供旁边的文件据以命名的名称

# draft
tags_aside_other_layout = 它旁边的{ $name }布局与 Mac 写入的不同，Skwir 不改写其他布局的文件

# draft
tags_aside_too_large = 它旁边的{ $name }超出了 Skwir 可改写的大小

# draft
tags_aside_unreadable = 无法读取它旁边的{ $name }：{ $error }

# draft
tags_aside_unwritable = 它的 Mac 属性以 Skwir 不写入的形式保存

# draft
tags_aspect_close = 返回文件夹

# draft
tags_aspect_open = 此文件夹的标签

# draft
tags_browse = 文件夹的标签

# draft
tags_browse_description = 将某文件夹（`in`，绝对路径）下文件的每个标签显示为可浏览的位置，每个标签都是其文件的一个文件夹：省略时为窗格显示的文件夹，或窗格中的标签位置或搜索所属的文件夹；给出 `toggle` 而未给出文件夹时，标签位置改为返回其文件夹

# draft
tags_browse_here = 在此浏览标签

# draft
tags_browse_toggle = 此文件夹的标签，或返回此文件夹

# draft
tags_category_no_comma = 类别名称中不能有逗号

# draft
tags_change_plan = 更改标签

# draft
tags_color = 标签颜色

# draft
tags_color_blue = 蓝色

# draft
tags_color_choice = 颜色

# draft
tags_color_description = 选择标签在各处显示的颜色，或恢复为由其名称决定的颜色

# draft
tags_color_from_name = 按名称

# draft
tags_color_green = 绿色

# draft
tags_color_not_saved = 未能保存标签的颜色：{ $error }

# draft
tags_color_one_of = 标签的颜色须为 { $colors } 之一

# draft
tags_color_orange = 橙色

# draft
tags_color_pink = 粉色

# draft
tags_color_purple = 紫色

# draft
tags_color_red = 红色

# draft
tags_color_teal = 蓝绿色

# draft
tags_color_yellow = 黄色

# draft
tags_colors_data = 用户为标签选择的颜色，按小写的标签名：{"{"} tag = color {"}"}

# draft
tags_comment_accept = 保存

# draft
tags_comment_item = 注释…

# draft
tags_comment_menu = 注释菜单

# draft
tags_comment_menu_description = 右键菜单中的“注释…”，含所选项目的注释

# draft
tags_comment_placeholder = 对所选项目的注释（留空即移除）

# draft
tags_comment_title = 注释

# draft
tags_counted = 标签统计

# draft
tags_counted_description = `{"{"} root, counting, counted, partial {"}"}`：`root`（省略时为主文件夹）下文件的标签是否正在为其标签位置统计、统计涵盖到的时间（`counted`，自纪元起的秒数），以及是否因空间不足而遗漏了部分文件（`partial`）。从内存中作答

# draft
tags_delete = 从所有文件移除标签

# draft
tags_delete_description = 从主文件夹（或 `root`）下所有带有某标签（或 `tags`）的文件上移除该标签：在工作线程上搜索这些文件，并提交计划供审阅

# draft
tags_depth_below = 包括子标签

# draft
tags_depth_here = 仅此标签

# draft
tags_depth_top_below = 所有带标签的文件

# draft
tags_depth_top_here = 这些标签

# draft
tags_drop = 拖放到标签上

# draft
tags_drop_description = 为拖放或粘贴到标签位置（`location`）的文件添加该标签：窗格的所选项目（`pane`）或 `entries`；给出 `move` 时，同时将其移出来源的标签位置（窗格的，或 `from`）。拖放的标签放入其中，生成待审阅的计划

# draft
tags_drop_on_one = 一次只能为文件添加一个标签：请将文件拖放到该标签上

# draft
tags_edit = 编辑标签

# draft
tags_edit_description = 在同一个计划中为给定的项目或所选项目添加一些标签并移除另一些：每个项目保留其他标签

# draft
tags_edit_item = 编辑标签…

# draft
tags_empty_tag = 此处没有文件带有此标签

# draft
tags_empty_tag_detail = 将文件拖放到此处即可为其添加此标签。

# draft
tags_empty_top = 此处没有带标签的文件

# draft
tags_empty_top_detail = 可通过文件菜单中的“标签…”为文件添加标签，或将文件拖放到侧边栏中的标签上。

# draft
tags_exclusive = 每个类别仅限一个标签

# draft
tags_exclusive_data = 文件一次只能带有其中一个标签的类别：即字段，其值为其中的标签

# draft
tags_exclusive_description = 使类别（`tag`）在每个文件上一次只容纳一个标签，成为以其标签为值的字段（`on`），或恢复为可容纳多个

# draft
tags_exclusive_not_changed = 未能更改{ $tag }：{ $error }

# draft
tags_exclusive_off = 文件又可以带有{ $tag }中的多个标签了

# draft
tags_exclusive_on = 文件现在一次只能带有{ $tag }中的一个标签

# draft
tags_export = 将标签复制到文件夹

# draft
tags_export_description = 在 `dest` 中创建以标签命名的文件夹，内含标签位置（`location`）列出的文件，其中每个子标签各有一个文件夹：生成计划并提交审阅。即将标签拖放或粘贴到文件夹时执行的操作

# draft
tags_export_one = 只能复制单个标签，不能复制全部

# draft
tags_export_plan = 将带有标签{ $tag }的文件复制到 { $folder }

# draft
tags_files_menu = 所选项目菜单中的标签

# draft
tags_files_menu_description = 右键菜单中的“标签”子菜单：先是所选文件带有的标签，然后是已固定的标签；所有文件都带有时勾选，部分带有时部分勾选，点击则在所有文件上切换

# draft
tags_files_not_found = 未能找到带有标签{ $tag }的文件：{ $error }

# draft
tags_filter_not_kept = { $done }；其筛选{ $filter }未随之保留

# draft
tags_finder_limits = 评分，或访达七种颜色以外的标签颜色

# draft
tags_follow_both_already = { $to }已沿用{ $from }的固定位置和颜色

# draft
tags_follow_both_did = 已让{ $to }沿用{ $from }的固定位置和颜色

# draft
tags_follow_both_will = 让{ $to }沿用{ $from }的固定位置和颜色

# draft
tags_follow_color_already = { $to }已是{ $from }原来的颜色

# draft
tags_follow_color_did = 已为{ $to }设置{ $from }原来的颜色

# draft
tags_follow_color_will = 为{ $to }设置{ $from }原来的颜色

# draft
tags_follow_pin_already = { $to }已固定在{ $from }原来的位置

# draft
tags_follow_pin_did = 已将{ $to }固定在{ $from }原来的位置

# draft
tags_follow_pin_will = 将{ $to }固定在{ $from }原来的位置

# draft
tags_follow_unpin_already = { $from }已取消固定

# draft
tags_follow_unpin_did = 已取消固定{ $from }

# draft
tags_follow_unpin_will = 取消固定{ $from }

# draft
tags_give_file_already = { $name }已带有标签{ $tag }

# draft
tags_give_file_did = 已为{ $name }添加标签{ $tag }

# draft
tags_give_file_will = 为{ $name }添加标签{ $tag }

# draft
tags_in_category_already = 这些标签已在{ $category }中

# draft
tags_in_no_category = 这些标签不属于任何类别

# draft
tags_include_sub_tags = 包括子标签

# draft
tags_index_full = 部分带标签的文件未计入标签：Skwir 为其分配的空间已满

# draft
tags_inside_itself = { $tag }不能放入其自身

# draft
tags_keep = 固定所显示的标签

# draft
tags_keep_description = 固定窗格中标签位置所显示的标签：须为主文件夹的单个标签，且未缩小范围

# draft
tags_keep_forget = 从侧边栏取消固定此标签

# draft
tags_keep_one = 只有主文件夹中未缩小范围的单个标签可保留到侧边栏，即将其固定

# draft
tags_keep_save = 将此标签固定到侧边栏

# draft
tags_kind_field = 字段

# draft
tags_kind_tag = 标签

# draft
tags_known = 已知标签

# draft
tags_known_description = `{"{"} generation, tags? {"}"}`：用户拥有的每个标签，各列一次，从内存读取：先是已固定的标签，按其顺序；然后是最近添加的；然后是在文件上见到的；最后是在主文件夹下统计到的，带有它的文件最多的在前。`generation` 为其编号：以所持有的编号询问时，若该编号仍是最新的，则省略 `tags`

# draft
tags_limit_description = Skwir 可为其所知的带标签文件分配多少内存（每兆字节约一万六千个文件），用于以文件夹方式浏览标签、侧边栏的建议以及更快的标签搜索。

# draft
tags_limit_description_more = 超出后，新的带标签文件不会计入标签的文件夹，Skwir 会给出提示。

# draft
tags_limit_title = 带标签文件的空间

# draft
tags_list = 列出标签

# draft
tags_list_description = 窗格列表中使用的标签及其计数（仅大小写不同的标签计为一个）；给出 `read` 时，也从列表尚未读取其标签的文件中读取

# draft
tags_looking_home = 正在主文件夹中查找带有标签{ $tags }的文件…

# draft
tags_looking_in = 正在{ $root }中查找带有标签{ $tags }的文件…

# draft
tags_merge_accept = 合并

# draft
tags_merge_ask = 在所有文件上将这些标签合并为一个

# draft
tags_merge_item = 合并为一个…

# draft
tags_merge_placeholder = 合并后的标签

# draft
tags_merge_plan = 将标签{ $tags }合并为{ $to }

# draft
tags_merged_into = 其中 { $merged } 个文件已带有{ $to }，或带有多个被重命名的标签：每个文件只保留一个{ $to }。

# draft
tags_merged_names = 其中 { $merged } 个文件带有两个现在同名的标签：每个文件只保留一个该标签。

# draft
tags_move = 将标签移入类别

# draft
tags_move_accept = 移动

# draft
tags_move_ask = 移入类别，保留名称

# draft
tags_move_description = 在主文件夹（或 `root`）下的所有文件上，将标签（或 `tags`）放入类别 `into` 并保留其自身名称（`Paris` 放入 `Places/France` 即为 `Places/France/Paris`），未给出 `into` 时则将其移出所在类别：在工作线程上搜索这些文件，并提交计划供审阅

# draft
tags_move_file_already = { $name }已在{ $tag }中

# draft
tags_move_file_did = 已将{ $name }从{ $from }移到{ $tag }

# draft
tags_move_file_will = 将{ $name }从{ $from }移到{ $tag }

# draft
tags_move_into_plan = 将标签{ $tag }移入{ $into }

# draft
tags_move_item = 移到类别…

# draft
tags_move_out_all_item = 移出所在类别

# draft
tags_move_out_item = 移出{ $category }

# draft
tags_move_out_plan = 将标签{ $tag }移出其类别

# draft
tags_move_placeholder = 类别（如 地点/法国）

# draft
tags_move_several_into_plan = 将标签{ $tags }移入{ $into }

# draft
tags_move_several_plan = 移动标签{ $tags }

# draft
tags_move_tag_plan = 从标签{ $from }移到标签{ $tag }

# draft
tags_narrow = 缩小标签位置的范围

# draft
tags_narrow_deep_only = 标签位置以 `deep` 缩小范围；`{ $facet }` 属于窗格的透镜（query.narrow）

# draft
tags_narrow_description = 在窗格显示的标签位置中，显示该标签之内的所有文件（`facet` `deep`），而不仅是恰好带有该标签的文件；`on` 为 false 时撤回，省略时切换。其他标签、种类和年份属于窗格的透镜（`query.narrow`）

# draft
tags_needs_name = 标签需要名称

# draft
tags_needs_search = 查找带标签的文件需要搜索插件

# draft
tags_new = 新建标签

# draft
tags_new_accept = 创建

# draft
tags_new_ask = 新建标签

# draft
tags_new_description = 在标签位置（`location`）显示的标签中创建标签 `name`：在有文件带上它之前，它一直显示在那里

# draft
tags_new_in_ask = 在{ $tag }中新建标签

# draft
tags_new_placeholder = 名称

# draft
tags_new_verb = 新建标签…

# draft
tags_no_comma = 标签名称中不能有逗号

# draft
tags_no_home = 没有主文件夹

# draft
tags_no_home_to_find = 没有可在其中查找标签的主文件夹

# draft
tags_no_rating = 未评分

# draft
tags_no_such_pane = 没有该窗格

# draft
tags_none_tagged_home = 主文件夹中没有带有标签{ $tags }的文件

# draft
tags_none_tagged_in = { $root }中没有带有标签{ $tags }的文件

# draft
tags_one_at_a_time = { $category }中仅限一个标签

# draft
tags_open = 打开标签

# draft
tags_open_description = 在给定窗格或活动窗格中，显示主文件夹中带有某标签的文件，以及其中每个子标签的位置，与点击其固定的胶囊时相同

# draft
tags_open_tag = 打开标签

# draft
tags_pane_shows_no_tags = 窗格未显示标签

# draft
tags_pane_shows_nothing = 窗格未显示任何内容

# draft
tags_pill = 此处的标签

# draft
tags_pill_description = 按住 Shift（`how` `also`：所显示的文件中同时带有该标签的）或 Alt（`here`：所显示文件夹中带有该标签的文件）点击侧边栏中的标签

# draft
tags_pin = 固定标签

# draft
tags_pin_description = 将标签（`tag`、`tags`）保留在侧边栏中，放在固定标签 `before` 之前或最后；已固定的标签会被移动

# draft
tags_pin_item = 固定到侧边栏

# draft
tags_pin_moved = 已移动标签{ $tag }

# draft
tags_pinned = { $count ->
        [1] 已将标签{ $tag }固定到侧边栏
       *[other] 已将 { $count } 个标签固定到侧边栏
    }

# draft
tags_pinned_data = 固定到侧边栏的标签，按其顺序

# draft
tags_pins = 设置固定标签

# draft
tags_pins_back = 已将固定标签恢复原样

# draft
tags_pins_description = 按顺序恰好固定 `tags`：即撤销固定时执行的操作

# draft
tags_pins_not_saved = 未能保存固定标签

# draft
tags_pins_not_saved_said = 未能保存固定标签：{ $error }

# draft
tags_place_menu = 标签菜单

# draft
tags_place_menu_description = 在标签位置中选中标签时右键菜单中的项目。从内存中作答

# draft
tags_plan_note_home = 主文件夹中的 { $count } 个文件；排除项所排除的内容不计在内。

# draft
tags_plan_note_home_merged = 主文件夹中的 { $count } 个文件；排除项所排除的内容不计在内。{ $merged }

# draft
tags_plan_note_in = { $root }中的 { $count } 个文件；排除项所排除的内容不计在内。

# draft
tags_plan_note_in_merged = { $root }中的 { $count } 个文件；排除项所排除的内容不计在内。{ $merged }

# draft
tags_plugin_description = 保存在文件扩展属性中的标签、评分和注释，无需数据库；以及下载文件的来源

# draft
tags_plugin_off = 标签插件已停用

# draft
tags_plugin_title = 标签

# draft
tags_rating_choice = 评分

# draft
tags_remove = 移除标签

# draft
tags_remove_all_plan = 移除标签

# draft
tags_remove_all_verb = 移除所有标签

# draft
tags_remove_description = 从给定的项目或所选项目中移除标签（计划）

# draft
tags_remove_everywhere_item = 从所有文件移除标签…

# draft
tags_remove_everywhere_plan = 从所有文件移除标签{ $tag }

# draft
tags_remove_from_files = { $count ->
        [1] 从此文件移除
       *[other] 从所选文件移除
    }

# draft
tags_remove_from_verb = 从{ $tag }中移除

# draft
tags_remove_plan = 移除标签{ $tags }

# draft
tags_remove_several_everywhere_plan = 从所有文件移除标签{ $tags }

# draft
tags_rename = 在所有文件上重命名标签

# draft
tags_rename_accept = 重命名

# draft
tags_rename_ask = 在所有文件上重命名标签{ $tag }

# draft
tags_rename_description = 在主文件夹（或 `root`）下所有带有该标签的文件上重命名标签（或合并 `tags`），文件同时带有新名称的标签时将二者合并：在工作线程上搜索这些文件，并提交计划供审阅

# draft
tags_rename_placeholder = 新名称（使用已有标签的名称会将它们合并）

# draft
tags_rename_plan = 将标签{ $tag }重命名为{ $to }

# draft
tags_rename_verb = 重命名标签…

# draft
tags_restore_already = 固定标签及其颜色已经恢复

# draft
tags_restore_did = 已恢复固定标签及其颜色

# draft
tags_restore_will = 恢复固定标签及其颜色

# draft
tags_search_cancelled = 已取消

# draft
tags_search_no_location = 搜索未返回位置

# draft
tags_search_stopped = 搜索已停止

# draft
tags_selected = 所选项目的标签

# draft
tags_selected_description = `{"{"} items, tags: [{"{"} name, count, color? {"}"}], known {"}"}`：所选项目的标签，从其文件读取，包括带有每个标签的文件数、文件为标签指定的颜色（访达标签的颜色），以及已知标签，先是已固定的，然后是最近使用的

# draft
tags_set = 设置标签

# draft
tags_set_description = 替换给定项目或所选项目的标签（计划）

# draft
tags_set_plan = 将标签设为{ $tags }

# draft
tags_show_files_here = 在此显示其文件

# draft
tags_stars = { $stars } 星

# draft
tags_store_appledouble = Mac 的 ._ 文件，位于不支持属性的卷上的文件旁边

# draft
tags_store_finder = Mac 访达的标签和注释

# draft
tags_store_xdg = 桌面环境的属性（KDE、Dolphin、Baloo）

# draft
tags_suggested_description = 在固定标签之前，侧边栏建议的标签数量，带有该标签的文件最多的在前；0 表示不建议。

# draft
tags_suggested_title = 侧边栏中建议的标签

# draft
tags_suggestion_detail = 标签 · { $tag }

# draft
tags_tag_menu = 标签的菜单

# draft
tags_tag_menu_description = 标签在其显示处的菜单项目：属性中的标签块（tags）、侧边栏中的胶囊（location）。从内存中作答

# draft
tags_title = 标签

# draft
tags_toggle = 切换标签

# draft
tags_toggle_description = 为给定项目或所选项目添加某标签，若所有项目都已带有则从每个项目上移除：可绑定到按键的命令；给出 `on` 时，无论现状如何都添加（true）或移除（false）（计划）

# draft
tags_toggle_form = 切换标签{ $tag }

# draft
tags_unpin = 取消固定标签

# draft
tags_unpin_description = 将标签（`tag`、`tags`）移出侧边栏；文件仍保留这些标签

# draft
tags_unpin_item = 从侧边栏取消固定

# draft
tags_unpinned = { $count ->
        [1] 已取消固定标签{ $tag }
       *[other] 已取消固定 { $count } 个标签
    }

# draft
tags_untag = 从标签中移除

# draft
tags_untag_all_file_already = { $name }已经没有标签

# draft
tags_untag_all_file_did = 已移除{ $name }的标签

# draft
tags_untag_all_file_will = 移除{ $name }的标签

# draft
tags_untag_all_plan = 移除所有标签

# draft
tags_untag_description = 从所选文件上移除标签位置（窗格的，或 `location`）显示的标签；在那里选中的标签会从所有文件上移除，生成待审阅的计划。即在标签位置中按 Delete 执行的操作

# draft
tags_untag_file_already = { $name }已不带标签{ $tag }

# draft
tags_untag_file_did = 已从{ $name }移除标签{ $tag }

# draft
tags_untag_file_will = 从{ $name }移除标签{ $tag }

# draft
tags_untag_plan = 移除标签{ $tag }

# draft
tags_what_new_name = 新名称是什么？

# draft
tags_which_category = 哪个类别？

# draft
tags_which_tag = 哪个标签？

# draft
tags_with_sub_tags = 包括子标签

# draft
tags_xdg_limits = 标签中的逗号，或标签的颜色
