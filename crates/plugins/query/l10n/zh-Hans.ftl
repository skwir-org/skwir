# Simplified Chinese catalog of skwir-plugin-query, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
query_accept_filter = 筛选

# draft
query_accept_search = 搜索

# draft
query_another_tag = 其他标签…

# draft
query_app_ended = 应用程序已结束

# draft
query_ask_tag_placeholder = 标签

# draft
query_bad_glob = 无效的 { $key } 通配模式 `{ $glob }`：{ $error }

# draft
query_bad_regex = 无效的正则表达式 `{ $regex }`：{ $error }

# draft
query_bad_size = 无效的大小 `{ $value }`

# draft
query_bad_time = 无效的日期或时长 `{ $value }`（可尝试 2026-05-01、this-week 或 7d）

# draft
query_below = 此文件夹及其子文件夹

# draft
query_below_description = 在窗格的文件夹及其子文件夹中按筛选它的条件搜索：其透镜以及在地址栏中输入的文字

# draft
query_chip_accessed = 访问时间

# draft
query_chip_contains = 包含

# draft
query_chip_created = 创建时间

# draft
query_chip_in = 位于 { $place }

# draft
query_chip_kind = 种类

# draft
query_chip_kinds = { $kinds }

# draft
query_chip_modified = 修改时间

# draft
query_chip_name = 名称

# draft
query_chip_not_tag = 不带标签

# draft
query_chip_size = 大小

# draft
query_chip_subfolders = 及子文件夹

# draft
query_chip_tag = 标签

# draft
query_chip_taken = 拍摄时间

# draft
query_chip_when = { $when }

# draft
query_chip_when_days = { $first } → { $last }

# draft
query_complete = 补全搜索

# draft
query_complete_description = { $shape }：以 `caret`（按字节计；省略时为末尾）结尾的条件可以如何继续，每项的 `text` 用于替换 `range`：某个词开头的键、某个键的值（种类；窗格中文件的标签及用户的其他标签，最多 200 个，`more` 为未列出的数量；时间段、大小、星级）；该条件仍未输入完整时给出 `typing`

# draft
query_contains_placeholder = 文件包含的文本

# draft
query_custom = 自定义…

# draft
query_depth_below = 此文件夹及其子文件夹

# draft
query_depth_chip = 仅此文件夹

# draft
query_depth_here = 仅此文件夹

# draft
query_describe = 描述搜索

# draft
query_describe_description = 搜索的胶囊，即地址栏为显示搜索的窗格所绘制的内容：每个维度（文字、种类、修改时间、拍摄时间、大小、标签、内容、位置；创建时间和访问时间在设置后才有）及其值，以及其每个选项所运行的调用，然后是不属于任何维度的部分

# draft
query_describe_filter = 描述筛选

# draft
query_describe_filter_description = { $shape }：以搜索胶囊的形式（`query.describe`）给出筛选窗格的内容（其透镜以及在地址栏中输入的文字，ADR-107），每个选项都会改变它（`query.filter`）：查找的深度（对文件夹而言：仅此文件夹，或连同子文件夹一起搜索，`query.below`），然后是每个维度，无论是否已设置

# draft
query_edit = 编辑已保存的搜索

# draft
query_edit_accept = 保存

# draft
query_edit_description = 更改已保存的搜索所查找的内容，或已保存的位置的筛选；其名称及其在侧边栏中的位置保持不变

# draft
query_edit_filter_menu = 编辑筛选…

# draft
query_edit_filter_title = 编辑筛选

# draft
query_edit_search_menu = 编辑搜索…

# draft
query_edit_search_placeholder = { $example }（用 { $key } 指定搜索位置）

# draft
query_edit_search_title = 编辑已保存的搜索

# draft
query_empty_regex = { $key } 的正则表达式为空

# draft
query_error = 查询：{ $error }

# draft
query_everything = 所有文件

# draft
query_expected_bool = 应为 { $yes } 或 { $no }，实际为 `{ $value }`

# draft
query_expected_comparison = `{ $text }` 之后应有比较符

# draft
query_expected_term = 此处应有一个条件

# draft
query_expects_distance = { $key } 需要一个距离，例如 5km 或 500m，实际为 `{ $value }`

# draft
query_expects_edges = { $key } 需要南、西、北、东四条边界，例如 9.9,19.9,10.2,20.3，实际为 `{ $value }`

# draft
query_expects_either = { $key } 需要 { $one } 或 { $other }，实际为 `{ $value }`

# draft
query_expects_extension = { $key } 需要至少一个扩展名

# draft
query_expects_folder = { $key } 需要一个文件夹

# draft
query_expects_kind = { $key } 需要 { $kinds }，实际为 `{ $value }`

# draft
query_expects_kinds = { $key } 需要 { $kinds }

# draft
query_expects_number = { $key } 需要一个数字，实际为 `{ $value }`

# draft
query_expects_point = { $key } 需要纬度和经度，例如 45.1,6.2，实际为 `{ $value }`

# draft
query_expects_stars = { $key } 需要 0 到 5 的星级（可用半星），实际为 `{ $value }`

# draft
query_expects_type = { $key } 需要 { $file }、{ $dir } 或 { $link }，实际为 `{ $value }`

# draft
query_expects_value = { $key } 需要一个值

# draft
query_field_among = 在 { $place } 的文件中

# draft
query_field_close_quote = 闭合引号后即可搜索

# draft
query_field_home = 在主文件夹的所有位置

# draft
query_field_in_folders = 在 { $place } 及其子文件夹中

# draft
query_field_more_tags = 还有 { $count } 个标签

# draft
query_field_narrow_tags = 输入更多名称以缩小范围

# draft
query_field_recent_in = 在 { $place } 中

# draft
query_field_saved_in = { $place } 中的 { $filter }

# draft
query_field_search = 搜索{ $search }

# draft
query_field_search_contents = 内容

# draft
query_field_search_disk = 整个磁盘

# draft
query_field_search_for = 搜索{ $words }

# draft
query_field_search_names = 名称

# draft
query_field_type_value = 输入一个值以搜索

# draft
query_field_whole_disk = 在整个磁盘上

# draft
query_field_within = 在 { $place } 内的所有文件中

# draft
query_filter = 筛选文件夹

# draft
query_filter_description = 更改筛选窗格的某一维度（在地址栏中输入的文字、其透镜），如同 `query.refine` 更改搜索的维度；位置保持显示

# draft
query_filter_tags = 筛选标签

# draft
query_filter_taken = 此筛选已保存为{ $name }

# draft
query_find_similar_submenu = 查找相似项

# draft
query_forget = 移除已保存的搜索

# draft
query_forget_description = 从侧边栏中移除一个已保存的搜索，按其搜索内容（已保存的位置按其筛选和 `at`）或其名称指定

# draft
query_forget_recent = 忘记一个最近的搜索

# draft
query_forget_recent_description = 从地址栏提供的最近运行过的搜索中移除一个搜索

# draft
query_group_100_to_1 = 100 { $unit } 至 1 { $next }

# draft
query_group_12_to_24_mp = 12 至 24 MP

# draft
query_group_1_to_100 = 1 至 100 { $unit }

# draft
query_group_1_to_4_mp = 1 至 4 MP

# draft
query_group_1_to_5_minutes = 1 至 5 分钟

# draft
query_group_20_minutes_to_1_hour = 20 分钟至 1 小时

# draft
query_group_4_to_12_mp = 4 至 12 MP

# draft
query_group_5_to_20_minutes = 5 至 20 分钟

# draft
query_group_digits = 0–9

# draft
query_group_earlier_this_month = 本月早些时候

# draft
query_group_earlier_this_week = 本周早些时候

# draft
query_group_earlier_this_year = 今年早些时候

# draft
query_group_empty = 空

# draft
query_group_folders = 文件夹

# draft
query_group_future = 将来

# draft
query_group_last_month = 上个月

# draft
query_group_last_week = 上周

# draft
query_group_no_date = 无日期

# draft
query_group_no_dimensions = 无尺寸

# draft
query_group_no_duration = 无时长

# draft
query_group_no_extension = 无扩展名

# draft
query_group_no_folder = 无文件夹

# draft
query_group_no_owner = 无所有者

# draft
query_group_no_tags = 无标签

# draft
query_group_other_kind = 其他

# draft
query_group_other_letters = 其他文字

# draft
query_group_other_names = 其他

# draft
query_group_over_1 = 大于 1 { $unit }

# draft
query_group_over_1_hour = 超过 1 小时

# draft
query_group_over_24_mp = 大于 24 MP

# draft
query_group_state_unknown = 未知

# draft
query_group_strokes = { $strokes } 画

# draft
query_group_today = 今天

# draft
query_group_unchanged = 未更改

# draft
query_group_under_100 = 小于 100 { $unit }

# draft
query_group_under_1_minute = 不到 1 分钟

# draft
query_group_under_1_mp = 小于 1 MP

# draft
query_group_unknown_type = 未知类型

# draft
query_group_unrated = 未评分

# draft
query_group_yesterday = 昨天

# draft
query_here = 仅此文件夹

# draft
query_here_description = 若仅凭文件夹本身就能判断搜索的文字，则将窗格的搜索显示为其所查找的文件夹，并按这些文字筛选；否则仅搜索该文件夹。文件夹保持不变

# draft
query_home = 主文件夹

# draft
query_interpret = 将文字解读为搜索

# draft
query_interpret_description = { $shape } 或 null：按规则将普通文字解读为条件（时间段和年份、种类、扩展名、大小、窗格中的标签），其余作为名称保留；从不自行应用

# draft
query_keep_place = 连同此筛选一起保存

# draft
query_keep_search = 保存此搜索

# draft
query_key_accessed = 上次打开时间

# draft
query_key_case = 是否区分大小写

# draft
query_key_comment = 注释中包含

# draft
query_key_comment_example = 文件注释中的文字

# draft
query_key_content = 包含文本

# draft
query_key_content_example = "文字" 或 /正则表达式/

# draft
query_key_created = 创建时间

# draft
query_key_depth = 查找深度

# draft
query_key_depth_example = 向下查找几层文件夹：2

# draft
query_key_excluded = 也查找排除的文件夹

# draft
query_key_excluded_example = 也包括排除项所排除的内容：{ $yes }、{ $no }

# draft
query_key_ext = 带有扩展名

# draft
query_key_hidden = 是否包括隐藏文件

# draft
query_key_in = 查找位置

# draft
query_key_in_example = 一个文件夹，如 "~/My Files"

# draft
query_key_inside = 拍摄于矩形区域内

# draft
query_key_inside_example = 南,西,北,东

# draft
query_key_kind = 属于某种类

# draft
query_key_mime = 属于某类型

# draft
query_key_modified = 修改时间

# draft
query_key_name = 名称

# draft
query_key_near = 拍摄于某点附近

# draft
query_key_rating = 评分

# draft
query_key_size = 大小

# draft
query_key_tag = 带有标签

# draft
query_key_tag_example = 一个标签，或用 * 表示任意标签；也可写 #tag

# draft
query_key_taken = 拍摄时间（照片和视频）

# draft
query_key_taken_example = 2023、last-year，按相机的时钟

# draft
query_key_type = 文件、文件夹或链接

# draft
query_list = 列出已保存的搜索

# draft
query_list_description = 按侧边栏中的顺序返回已保存的搜索

# draft
query_location = 搜索的位置

# draft
query_location_description = { $shape }：从 `root` 或主文件夹开始，查找某一维度之值（`tag` `work`，或仅 `tag` 表示任意标签）的搜索，供不编写其语法而显示或运行搜索的调用者使用

# draft
query_matches_nothing = { $term } 不匹配任何内容

# draft
query_missing_paren = 缺少 `)`

# draft
query_move = 移动已保存的搜索

# draft
query_move_description = 在侧边栏中将一个已保存的搜索放到另一个之前，或放到最后

# draft
query_name_change = 更改…

# draft
query_name_contains = 名称包含

# draft
query_name_contains_menu = 名称包含…

# draft
query_name_placeholder = 文字或模式：invoice *.pdf

# draft
query_named_in = { $place } 中的 { $what }

# draft
query_narrow = 更改透镜

# draft
query_narrow_description = 更改窗格透镜的某一维度（ADR-107：其筛选或搜索中列表能够判断的部分，即某一种类、某一年份、带或不带某个标签的文件），在窗格显示其他位置时保留，直到被移除：对搜索而言改变的是搜索本身，对其他位置而言改变的是其筛选；不带 `facet` 时移除透镜

# draft
query_needs_value = `{ $facet }` 需要一个值

# draft
query_no_facet = 没有维度 `{ $facet }`

# draft
query_no_kind = 没有种类 `{ $kind }`

# draft
query_no_pane = 没有这个窗格

# draft
query_no_pane_numbered = 没有窗格 { $pane }

# draft
query_no_part = 没有编号为 `{ $part }` 的部分

# draft
query_no_saved_named = 没有名为{ $name }的已保存搜索

# draft
query_no_search_no_filter = 窗格既未显示搜索，也未显示筛选

# draft
query_no_search_no_folder = 窗格既未显示搜索，也未显示文件夹

# draft
query_no_tags_among = 这些文件都没有标签

# draft
query_not_a_lens = 透镜是关于文件的已知信息：`{ $facet }` 属于搜索（文件包含的内容、照片的拍摄时间、名称）

# draft
query_not_a_place = { $key } `{ $value }` 不是一个地点

# draft
query_not_allowed = 此处不允许使用 { $key }

# draft
query_not_on_earth = { $key } 的距离 `{ $value }` 超出了地球上的范围

# draft
query_not_one_condition = `{ $term }` 不是单个条件

# draft
query_not_tagged_instead = 改为不带此标签

# draft
query_not_within_search = 搜索不能在另一个搜索中查找

# draft
query_nothing_found = 未找到任何内容

# draft
query_nothing_found_detail = 此处没有与{ $text }匹配的文件。请检查拼写，或在更上层的文件夹中搜索。

# draft
query_nothing_opens = 没有插件能打开 { $place }：文件夹应写成完整路径

# draft
query_nothing_to_save = 窗格中没有可保存的内容

# draft
query_only_folders_below = 窗格未显示文件夹：只能搜索文件夹的子文件夹

# draft
query_only_in_bytes = 只有 { $key } 接受非 UTF-8 的字节（\xNN）

# draft
query_period_last_month = 上个月

# draft
query_period_last_week = 上周

# draft
query_period_last_year = 去年

# draft
query_period_older = 一年以前

# draft
query_period_this_month = 本月

# draft
query_period_this_week = 本周

# draft
query_period_this_year = 今年

# draft
query_period_today = 今天

# draft
query_period_yesterday = 昨天

# draft
query_place_changed = 已更改{ $name }的筛选

# draft
query_place_moved = 已移动{ $name }

# draft
query_place_named = { $place } · { $filter }

# draft
query_place_needs_filter = 已保存的位置需要一个筛选

# draft
query_place_not_below = 位置按其列出的内容搜索，从不搜索其下层

# draft
query_place_not_saved = 此项尚未保存

# draft
query_place_removed = 已将{ $name }从“已保存”中移除

# draft
query_place_renamed = 已将{ $name }重命名为{ $new }

# draft
query_place_saved = 已保存{ $name }

# draft
query_place_saved_already = 此项已保存为{ $name }

# draft
query_plocate_database = plocate 的数据库

# draft
query_plugin_description = 将搜索作为位置：按名称、大小、日期、标签、MIME 和内容查询；在侧边栏中显示已保存的搜索

# draft
query_plugin_title = 搜索

# draft
query_rating_five = 五星

# draft
query_rating_none = 未评分

# draft
query_rating_one = 一星及以上

# draft
query_rating_three = 三星及以上

# draft
query_reading = { $subject }{ $named }{ $tagged }{ $year }{ $period }{ $size }

# draft
query_reading_archives = { $at ->
        [first] 压缩文件
       *[after] 压缩文件
    }

# draft
query_reading_audio = { $at ->
        [first] 音乐和音频
       *[after] 音乐和音频
    }

# draft
query_reading_documents = { $at ->
        [first] 文档
       *[after] 文档
    }

# draft
query_reading_empty = { $of ->
        [extension] ，内容为空
        [folder] ，内容为空
        [image] ，内容为空
        [video] ，内容为空
        [pictures] ，内容为空
        [audio] ，内容为空
        [document] ，内容为空
        [spreadsheet] ，内容为空
        [presentation] ，内容为空
        [text] ，内容为空
        [archive] ，内容为空
        [font] ，内容为空
        [program] ，内容为空
        [several] ，内容为空
       *[file] ，内容为空
    }

# draft
query_reading_extension = { $ext } 文件

# draft
query_reading_files = 文件

# draft
query_reading_folders = { $at ->
        [first] 文件夹
       *[after] 文件夹
    }

# draft
query_reading_fonts = { $at ->
        [first] 字体
       *[after] 字体
    }

# draft
query_reading_images = { $at ->
        [first] 图像
       *[after] 图像
    }

# draft
query_reading_modified = { $of ->
        [extension] ，{ $when }修改
        [folder] ，{ $when }修改
        [image] ，{ $when }修改
        [video] ，{ $when }修改
        [pictures] ，{ $when }修改
        [audio] ，{ $when }修改
        [document] ，{ $when }修改
        [spreadsheet] ，{ $when }修改
        [presentation] ，{ $when }修改
        [text] ，{ $when }修改
        [archive] ，{ $when }修改
        [font] ，{ $when }修改
        [program] ，{ $when }修改
        [several] ，{ $when }修改
       *[file] ，{ $when }修改
    }

# draft
query_reading_named = { $of ->
        [extension] ，名称包含{ $names }
        [folder] ，名称包含{ $names }
        [image] ，名称包含{ $names }
        [video] ，名称包含{ $names }
        [pictures] ，名称包含{ $names }
        [audio] ，名称包含{ $names }
        [document] ，名称包含{ $names }
        [spreadsheet] ，名称包含{ $names }
        [presentation] ，名称包含{ $names }
        [text] ，名称包含{ $names }
        [archive] ，名称包含{ $names }
        [font] ，名称包含{ $names }
        [program] ，名称包含{ $names }
        [several] ，名称包含{ $names }
       *[file] ，名称包含{ $names }
    }

# draft
query_reading_over_1 = ，大于 1 { $unit }

# draft
query_reading_over_100 = ，大于 100 { $unit }

# draft
query_reading_presentations = { $at ->
        [first] 演示文稿
       *[after] 演示文稿
    }

# draft
query_reading_programs = { $at ->
        [first] 程序
       *[after] 程序
    }

# draft
query_reading_spreadsheets = { $at ->
        [first] 电子表格
       *[after] 电子表格
    }

# draft
query_reading_tagged = { $of ->
        [extension] ，带有标签
        [folder] ，带有标签
        [image] ，带有标签
        [video] ，带有标签
        [pictures] ，带有标签
        [audio] ，带有标签
        [document] ，带有标签
        [spreadsheet] ，带有标签
        [presentation] ，带有标签
        [text] ，带有标签
        [archive] ，带有标签
        [font] ，带有标签
        [program] ，带有标签
        [several] ，带有标签
       *[file] ，带有标签
    }{ $tags }

# draft
query_reading_taken = { $of ->
        [extension] ，{ $when }拍摄
        [folder] ，{ $when }拍摄
        [image] ，{ $when }拍摄
        [video] ，{ $when }拍摄
        [pictures] ，{ $when }拍摄
        [audio] ，{ $when }拍摄
        [document] ，{ $when }拍摄
        [spreadsheet] ，{ $when }拍摄
        [presentation] ，{ $when }拍摄
        [text] ，{ $when }拍摄
        [archive] ，{ $when }拍摄
        [font] ，{ $when }拍摄
        [program] ，{ $when }拍摄
        [several] ，{ $when }拍摄
       *[file] ，{ $when }拍摄
    }

# draft
query_reading_text = { $at ->
        [first] 文本和代码
       *[after] 文本和代码
    }

# draft
query_reading_under_100 = ，小于 100 { $unit }

# draft
query_reading_videos = { $at ->
        [first] 视频
       *[after] 视频
    }

# draft
query_reading_when_last_month = 上个月

# draft
query_reading_when_last_week = 上周

# draft
query_reading_when_last_year = 去年

# draft
query_reading_when_older = 一年多以前

# draft
query_reading_when_this_month = 本月

# draft
query_reading_when_this_week = 本周

# draft
query_reading_when_this_year = 今年

# draft
query_reading_when_today = 今天

# draft
query_reading_when_year = { $year } 年

# draft
query_reading_when_yesterday = 昨天

# draft
query_ref_brackets = […] 其中之一

# draft
query_ref_content = 包含这些文字的文件

# draft
query_ref_content_regex = 包含与模式匹配之内容的文件

# draft
query_ref_contents = 内容

# draft
query_ref_date = 年、月或日：2026、2026-05-14

# draft
query_ref_date_range = 从前者的开始到后者的结束

# draft
query_ref_duration = 最近 7 天内：{ $m } 分钟，{ $h } 小时，{ $d } 天，{ $w } 周，{ $mo } 个月，{ $y } 年

# draft
query_ref_flags = { $i }：不区分大小写；{ $c }：区分大小写

# draft
query_ref_key = { $means }：{ $example }

# draft
query_ref_line = ^ 和 $ 表示行的开头和结尾。

# draft
query_ref_minus = 排除：-draft、-#tag、-kind:image

# draft
query_ref_names = 名称

# draft
query_ref_not = 非：NOT a，也可写作 -a

# draft
query_ref_older = 一年以前

# draft
query_ref_options = 查找位置和方式

# draft
query_ref_or = 或：a OR b

# draft
query_ref_parens = 组合：(a OR b) c

# draft
query_ref_period = 时间段：{ $periods }

# draft
query_ref_question = ? 一个字符

# draft
query_ref_quotes = 按原样："two words"、"OR"、"#3"

# draft
query_ref_rating = 至少（> < >= <=）；rating:0 查找未评分的文件

# draft
query_ref_size = 大于 10 { $mb }。{ $decimal } 中的每个单位是前一个的 1000 倍；{ $binary } 中的每个单位是前一个的 1024 倍，单独书写的 { $alone } 也是如此。

# draft
query_ref_size_range = 从一个大小到另一个大小，包括两端

# draft
query_ref_slash = 斜杠本身：单独一个斜杠会结束模式

# draft
query_ref_star = * 任意字符

# draft
query_ref_terms = 条件

# draft
query_ref_title = 搜索

# draft
query_ref_together = 组合（并列：两者都须满足）

# draft
query_ref_values = 值

# draft
query_ref_word = 名称中包含它，不区分大小写

# draft
query_reference = 搜索语法说明

# draft
query_reference_description = 搜索文本可用的内容（ADR-137）：其条件和选项及各自的含义和示例、条件如何组合、值、名称的通配符以及 `content:` 所读取的内容（包括正则表达式符号）；每项按原样给出，并说明其含义以及选择它时写入的内容

# draft
query_refine = 细化搜索

# draft
query_refine_description = 更改窗格搜索的某一维度（或在其文件夹中开始一个搜索，并加上其透镜）并显示结果：仅更改其透镜时就地替换，其他更改则作为可用“后退”撤销的一步；移除最后一个条件后，窗格显示其所搜索的位置

# draft
query_remember_setting = 记住最近的搜索

# draft
query_remember_setting_description = 地址栏会提供最近从中运行过的搜索，这些搜索保存在缓存中；关闭后将忘记它们。

# draft
query_remove_part = 移除

# draft
query_rename = 重命名已保存的搜索

# draft
query_rename_accept = 重命名

# draft
query_rename_description = 更改已保存的搜索在侧边栏中显示的名称

# draft
query_rename_place_placeholder = 名称（留空：根据筛选生成）

# draft
query_rename_place_title = 重命名

# draft
query_rename_search_placeholder = 名称（留空：根据搜索生成）

# draft
query_rename_search_title = 重命名已保存的搜索

# draft
query_requirement_plocate = 在整个磁盘上即时搜索名称（否则将遍历目录）

# draft
query_root_not_absolute = 搜索起点 { $path } 不是绝对路径

# draft
query_root_not_folder = 搜索起点 { $path } 不是文件夹：{ $error }

# draft
query_root_not_path = `root` 必须是路径

# draft
query_save = 保存搜索

# draft
query_save_description = 将搜索保存在侧边栏的“已保存”中，或通过 `at` 保存一个位置及其筛选，打开时按此显示（ADR-109）；省略 `expr` 时保存窗格当前显示的内容

# draft
query_saved_not_changed = 已保存的搜索未被更改：{ $error }

# draft
query_saved_not_written = 未能写入已保存的搜索

# draft
query_saved_setting = 侧边栏中按顺序显示的已保存搜索：{ $shape }

# draft
query_saved_unreadable = 无法读取设置文件中的已保存搜索；未写入任何内容

# draft
query_search = 搜索

# draft
query_search_changed = 已更改已保存的搜索{ $name }

# draft
query_search_description = 搜索并在窗格中显示结果，除非指定 `as_is`，否则会加上窗格的透镜（ADR-107：筛选或搜索中列表能够判断的条件，在窗格转到别处时保留，直到被移除；之后用 `entries` 列出找到的内容；窗格已显示的搜索保持不变，`refresh` 会重新列出）：文字匹配名称；`name:`、`ext:pdf,md`、`kind:image,video`（document、text、audio、archive…）、`type:dir`、`tag:x`、`content:words` 或 `content:/regex/`、`comment:`、`size>10M`、`modified<7d`（也可用 `today`、`this-week`、`2026-05`；`created`、`accessed`，以及按照片相机时钟的 `taken`）、`rating>=4`、`near:lat,lon~5km`；`OR`、`NOT` 或 `-`、括号；`hidden:yes` 会查找隐藏文件夹，`excluded:yes` 会查找搜索所排除的文件夹（设置 › 排除项：`node_modules`、缓存；绝不包括回收站或网络文件夹），`depth:2` 限制查找深度。`root`（或 `in:`）是查找的位置

# draft
query_search_here = 在此搜索…

# draft
query_search_moved = 已移动已保存的搜索{ $name }

# draft
query_search_not_saved = 此搜索尚未保存

# draft
query_search_removed = 已移除已保存的搜索{ $name }

# draft
query_search_renamed = 已将已保存的搜索{ $name }重命名为{ $new }

# draft
query_search_saved = 已保存搜索{ $name }

# draft
query_search_saved_already = 此搜索已保存为{ $name }

# draft
query_search_taken = 此搜索已保存为{ $name }

# draft
query_similar = 查找相似项

# draft
query_similar_day = 同一天

# draft
query_similar_description = 在所搜索或显示的文件夹中，搜索与所选文件相似的文件：种类相同、在同一天修改、大小相近或有相同的标签

# draft
query_similar_kind = 相同种类

# draft
query_similar_no_date = { $name }没有修改日期

# draft
query_similar_no_kind = { $name }不属于桌面已知的任何种类

# draft
query_similar_no_size = { $name }没有大小

# draft
query_similar_no_tags = { $name }没有标签

# draft
query_similar_one = 请选择一个文件以查找与其相似的文件

# draft
query_similar_size = 大小相近

# draft
query_similar_tags = 相同标签

# draft
query_similar_unreadable = 无法读取{ $name }

# draft
query_size_1_to_100 = 1 至 100 { $unit }

# draft
query_size_empty = 空

# draft
query_size_over_1 = 大于 1 { $unit }

# draft
query_size_over_100 = 大于 100 { $unit }

# draft
query_size_placeholder = 大小或范围：>10{ $m }、1{ $m }..5{ $m }、<500{ $k }

# draft
query_size_under_1 = 小于 1 { $unit }

# draft
query_size_under_100 = 小于 100 { $unit }

# draft
query_tag_all_of = { $tag }下的全部标签

# draft
query_tag_choices = 用于细化的标签

# draft
query_tag_choices_description = 标签胶囊的选择器：窗格中文件上的标签（从文件中读取），每个都会细化搜索（或细化 `refines` 所指定的：文件夹的 `narrowing` 或 `filter`）

# draft
query_tag_files = { $count } 个文件

# draft
query_tag_section_other = 其他

# draft
query_tagged = 带有标签

# draft
query_tagged_instead = 改为带有此标签

# draft
query_tagged_menu = 带有标签…

# draft
query_term_contains = 包含

# draft
query_term_kind = 种类

# draft
query_term_modified = 修改时间

# draft
query_term_size = 大小

# draft
query_term_tag = 标签

# draft
query_term_taken = 拍摄时间

# draft
query_text_in_file = 文件中的文本…

# draft
query_this_folder_only = 仅此文件夹

# draft
query_tokens = 搜索的组成部分

# draft
query_tokens_description = { $shape }：解析器所读取的搜索文本的各个部分（词、键、值、选项、运算符，或带有解析器消息的错误）及其字节范围；在 `caret` 处仍在输入的条件尚不算错误

# draft
query_unexpected_paren = 多余的 `)`

# draft
query_unkeep_place = 从“已保存”中移除

# draft
query_unkeep_search = 移除已保存的搜索

# draft
query_unknown_flag = { $key } 未知的正则表达式标志 `{ $flag }`（{ $i }：不区分大小写，{ $c }：区分大小写；以 / 开头的文本请加引号）

# draft
query_unterminated_quote = 引号未闭合

# draft
query_value_dir = 文件夹

# draft
query_value_false = 否

# draft
query_value_file = 文件

# draft
query_value_insensitive = 不区分大小写

# draft
query_value_sensitive = 区分大小写

# draft
query_value_symlink = 符号链接

# draft
query_value_true = 是

# draft
query_waits_contents = 文件包含的内容

# draft
query_waits_places = 照片的拍摄地点

# draft
query_waits_taken = 照片的拍摄时间

# draft
query_when_placeholder = 日期、范围或时间段：2026-05、2026-01-01..2026-03-31、<30d

# draft
query_where_excluded = 含排除项

# draft
query_where_excluded_files = 排除的文件和文件夹

# draft
query_where_hidden = 含隐藏文件

# draft
query_where_hidden_excluded = 含隐藏文件和排除项

# draft
query_where_hidden_files = 隐藏文件

# draft
query_where_subfolders = 此文件夹及其子文件夹

# draft
query_which_saved = 哪个已保存的搜索？请传入其 `expr` 或 `name`
