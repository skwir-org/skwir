# Simplified Chinese catalog of skwir-plugin-rename-bulk, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
rename_bulk_bad_date_format = 日期格式 `{ $format }` 有误：{ $why }

# draft
rename_bulk_bad_width = 计数器宽度 `{ $width }` 有误

# draft
rename_bulk_brace = 花括号本身（{ $open } 和 { $close }）

# draft
rename_bulk_cancelled = 已取消

# draft
rename_bulk_command = 按模板重命名多个项目

# draft
rename_bulk_command_description = 按模板、正则表达式和大小写规则重命名所选项目或给定的项目（计划）

# draft
rename_bulk_conflict = { $item }将变为{ $name }，但该名称已存在

# draft
rename_bulk_counter = 计数器

# draft
rename_bulk_counter_overflows = 计数器溢出

# draft
rename_bulk_counter_too_wide = { $width } 位宽的计数器超出了名称所能容纳的长度（最多 { $most }）

# draft
rename_bulk_counts = 从“起始值”开始计数，按显示顺序每项加一

# draft
rename_bulk_date_note = 在 { $placeholder } 中：%Y 为年份，%m 为月份，%d 为日，%H 为小时，%M 为分钟，%S 为秒，%B 为月份名称（%b 为简写），%A 为星期名称，%j 为一年中的第几天。使用本系统的时区。

# draft
rename_bulk_day = 日期

# draft
rename_bulk_day_and_time = 日期和时间

# draft
rename_bulk_day_first = 日期，日在前

# draft
rename_bulk_day_without_dashes = 日期，不含连字符

# draft
rename_bulk_dollar = 美元符号

# draft
rename_bulk_duplicate = { $count ->
        [2] 多个项目将变为{ $name }：{ $first }、{ $second }
        [3] 多个项目将变为{ $name }：{ $first }、{ $second }、{ $third }
       *[other] 多个项目将变为{ $name }：{ $first }、{ $second }、{ $third }及另外 { $more } 个
    }

# draft
rename_bulk_empty_replacement_note = 留空时，将删去模式所匹配的内容。

# draft
rename_bulk_extension = 扩展名，不含点

# draft
rename_bulk_extension_with_dot = 扩展名，含点；没有扩展名时为空

# draft
rename_bulk_first_group = 第一个分组匹配的内容（$2、$3…）

# draft
rename_bulk_group_before_letter = 同上，后面可接字母、数字或 _

# draft
rename_bulk_group_name_note = { $written } 会被读作名为 { $read } 的分组，而该分组为空：请写成 { $meant }。

# draft
rename_bulk_invalid_name = { $item }将变为无效的名称{ $name }：{ $reason }

# draft
rename_bulk_invalid_regex = 无效的正则表达式：{ $why }

# draft
rename_bulk_invalid_template = 无效的模板：{ $why }

# draft
rename_bulk_menu = 批量重命名…

# draft
rename_bulk_missing_date = { $item }尚无修改时间（{ $placeholder } 需要它）

# draft
rename_bulk_month_and_year = 月份和年份

# draft
rename_bulk_name_without_extension = 不含扩展名的名称

# draft
rename_bulk_named_group = 名为 year 的分组匹配的内容

# draft
rename_bulk_no_location = 窗格中没有打开的位置

# draft
rename_bulk_no_such_pane = 没有这个窗格

# draft
rename_bulk_not_read_yet = 尚未读取占位符 `{ $placeholder }`

# draft
rename_bulk_not_text = { $item }的名称不是文本；请单独重命名它

# draft
rename_bulk_nothing = 没有要重命名的项目

# draft
rename_bulk_parent = 所在的文件夹

# draft
rename_bulk_parts_note = 正则表达式最先执行：这里是它处理后留下的名称。大小写规则最后应用，且不改动扩展名。文件夹没有扩展名。

# draft
rename_bulk_parts_of_the_name = 名称的各部分

# draft
rename_bulk_plan = 重命名 { $count } 个项目

# draft
rename_bulk_plugin_description = 按模板、正则表达式和大小写规则重命名所选项目

# draft
rename_bulk_plugin_title = 批量重命名

# draft
rename_bulk_preview = 预览批量重命名

# draft
rename_bulk_preview_description = 返回批量重命名将产生的旧名称 → 新名称（`renames`，给定 `limit` 时只返回前 `limit` 个；每项附带正则表达式在旧名称中 `matched` 的位置，以及它在新名称中 `put` 的内容，均为字节范围）、共有多少个（`count`），以及第一个项目的名称（`first`），正则表达式和替换内容的参考只针对它

# draft
rename_bulk_recipe_brackets = 删去括号中的内容

# draft
rename_bulk_recipe_jpeg = JPEG 图片统一扩展名 → { $shown }

# draft
rename_bulk_recipe_numbers = 删去开头的数字

# draft
rename_bulk_recipe_one_space = 多个空格合并为一个 → 空格

# draft
rename_bulk_recipe_spaces = 空格改为下划线 → { $shown }

# draft
rename_bulk_recipe_start = 删去开头部分（IMG_）

# draft
rename_bulk_recipe_swap = 交换“ - ”两侧的部分 → { $shown }

# draft
rename_bulk_recipe_underscores = 下划线改为空格 → 空格

# draft
rename_bulk_recipes = 常用模式

# draft
rename_bulk_reference = 说明批量重命名的字段可填写的内容

# draft
rename_bulk_reference_description = 返回 `field`（template、regex、replacement）可填写的内容：其中的占位符或符号，每项附带它对给定或所选的第一个项目给出的结果（重命名的其他参数按给定值），以及填写正则表达式及其替换内容的常用模式

# draft
rename_bulk_regex = 正则表达式

# draft
rename_bulk_regex_groups_note = 在替换内容中，分组匹配的内容写作 $1、$2…；名为 year 的分组匹配的内容写作 { $named }。

# draft
rename_bulk_regex_note = ^ 和 $ 表示名称的开头和结尾。它作用于整个名称（包括扩展名），替换每一处匹配；之后再应用模板。

# draft
rename_bulk_replacement = 替换为

# draft
rename_bulk_signs = 符号

# draft
rename_bulk_template = 模板

# draft
rename_bulk_three_digits = 同上，至少三位数

# draft
rename_bulk_two_digits = 同上，至少两位数

# draft
rename_bulk_unclosed = `{ $template }` 中的 `{ $brace }` 未闭合

# draft
rename_bulk_unknown_placeholder = 未知的占位符 `{ $placeholder }`

# draft
rename_bulk_what_a_match_becomes = 匹配内容变为

# draft
rename_bulk_when_modified = 修改时间

# draft
rename_bulk_whole_match = 整个匹配内容

# draft
rename_bulk_whole_name = 完整名称
