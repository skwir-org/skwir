# Simplified Chinese catalog of skwir-plugin-print, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
print_alone = 每个 PDF 以其文件命名，保存在此文件夹中

# draft
print_already_saved = { $what ->
        [file] { $name }已作为{ $pdf }保存在 { $folder } 中
        [files] { $name }及另外 { $more } 个文件已作为{ $pdf }保存在 { $folder } 中
        [sheet] 包含 { $count } 个文件的联系表已作为{ $pdf }保存在 { $folder } 中
       *[listing] { $name }的列表已作为{ $pdf }保存在 { $folder } 中
    }

# draft
print_already_there = { $name }已在那里

# draft
print_as_shown = 按预览所示

# draft
print_as_shown_detail = 每页一张纸

# draft
print_beside = { $count ->
        [1] 在文件旁边
       *[other] 在这些文件旁边
    }

# draft
print_beside_each = 在每个文件旁边；全部合为一个 PDF 时放在第一个文件旁边

# draft
print_booklet = 小册子

# draft
print_booklet_detail = 双面排版，折叠成册

# draft
print_booklet_needs_pdfium = 排版小册子需要 PDFium，但找不到它

# draft
print_booklet_one_side = 小册子需双面打印：单面打印时，请先打印奇数面，将纸张翻面，再打印偶数面

# draft
print_bound_right = 右侧装订

# draft
print_bound_right_help = 适用于从右向左阅读的书

# draft
print_cancelled = 打印已取消

# draft
print_cancelled_save = 已取消：未保存任何 PDF

# draft
print_chosen_before = 之前选择过

# draft
print_closing = Skwir 正在关闭

# draft
print_column_deleted = 删除时间

# draft
print_column_folder = 文件夹

# draft
print_column_modified = 修改时间

# draft
print_column_name = 名称

# draft
print_column_size = 大小

# draft
print_column_type = 类型

# draft
print_contact_sheet = 联系表

# draft
print_contact_sheet_detail = 图片，每页多张

# draft
print_contact_sheet_title = 联系表

# draft
print_copies_only = { $name }只接受复制进来的文件：不会在那里生成 PDF

# draft
print_count_needs_reader = 统计 PDF 的页数需要文档阅读器，但找不到它

# draft
print_dialog_accept = 打印…

# draft
print_eight_sheets = 每组 8 张纸

# draft
print_every_sheet = 所有纸张合为一组

# draft
print_filtered = { $named }（筛选：{ $filter }）

# draft
print_fold_three_times = 将每张纸对折三次，第 1 页朝外且正放，裁开闭合的折边，然后将各张纸依次叠放

# draft
print_fold_twice = 将每张纸对折两次，第 1 页朝外且正放，裁开顶部的折边，然后将各张纸依次叠放

# draft
print_folded_as = 折叠方式

# draft
print_folded_once = 对折一次

# draft
print_folded_three_times = 对折三次并裁开

# draft
print_folded_twice = 对折两次并裁开顶边

# draft
print_folder_listed = 所列出的文件夹

# draft
print_folder_of = 其所属的文件夹

# draft
print_font_sample = 我能吞下玻璃而不伤身体 0123456789

# draft
print_four_sheets = 每组 4 张纸

# draft
print_handed = 已将{ $what ->
        [file] { $name }
        [files] { " " }{ $count } 个文件
        [sheet] 联系表
       *[listing] { $name }的列表
    }{ $pages ->
        [0] {""}
       *[other] 的 { $pages } 页
    }交给桌面打印服务，以{ $to ->
        [file] 输出到{ $target }
        [printer] 发送到 { $printer }
       *[system] 发送到打印机
    }，但它未确认已接收

# draft
print_in_folder = 在 { $folder } 中

# draft
print_in_folder_listed = 在所列出的文件夹中

# draft
print_in_no_folder = 该文件不在任何文件夹中

# draft
print_job_files = { $first } 及另外 { $more } 个

# draft
print_joined_needs_pdfium = 将多个文件合并打印需要 PDFium，但找不到它

# draft
print_keeping = 保留{ $name }以供打印

# draft
print_keeping_document = 保留文档的 PDF 以供打印

# draft
print_kind_folder = 文件夹

# draft
print_kind_place = 位置

# draft
print_layout = 版式

# draft
print_left_out = 已略去{ $name }：{ $error }

# draft
print_name_of_one = 名称只属于一个 PDF：这些文件会各自生成一个 PDF（`together` 会将它们合为一个）

# draft
print_no_folder_of_its_own = 它没有自己的文件夹

# draft
print_no_home = 另存为：没有可供“~”指代的主文件夹

# draft
print_no_listing_folder = 没有可保存列表的文件夹：请选择一个

# draft
print_no_longer_shown = { $name }已不再显示，因此无法得知其列表

# draft
print_no_page_asked = 它的页面均不在所要求的范围内

# draft
print_no_page_dialog = 它的页面均不在打印对话框所要求的范围内

# draft
print_no_page_shown = 未显示它的任何页面

# draft
print_no_paper = { $paper }不是纸张规格：a4、letter、a5、a3 或 legal

# draft
print_no_picture = 其中没有图片

# draft
print_no_such_folder = 另存为：没有文件夹{ $path }

# draft
print_no_such_pane = 没有该窗格

# draft
print_no_whole_path = 另存为：{ $typed }不是完整路径：请从根目录或从 ~ 开始输入

# draft
print_none_printable = 其中没有可打印的内容

# draft
print_not_a_folder = { $name }不是文件夹

# draft
print_not_own_file = 生成的 PDF 不是独立的文件

# draft
print_not_pictures = { $count ->
        [1] { $name }不是图片，已略去
       *[other] { $count } 个文件不是图片，已略去
    }

# draft
print_not_placed = 无法将{ $name }放入 { $folder }

# draft
print_not_printed = { $what ->
        [file] 未能打印{ $name }
        [files] 未能打印 { $count } 个文件
        [sheet] 未能打印联系表
       *[listing] 未能打印{ $name }的列表
    }

# draft
print_not_smaller = 无法减小 PDF 的体积

# draft
print_not_there = { $name }不存在

# draft
print_nothing_printable = { $name }没有可打印的内容

# draft
print_nothing_saved = 在您做出选择之前，不会保存任何 PDF。

# draft
print_nothing_selected = 其中没有选中任何内容

# draft
print_nothing_there = 这里没有可打印的内容

# draft
print_nothing_to_print = 没有可打印的内容

# draft
print_notice_of = { $name }：{ $notice }

# draft
print_notice_of_file = { $name }：{ $notice }

# draft
print_one_booklet = 单本小册子

# draft
print_other_pane = 另一个窗格

# draft
print_page_alt = 第 { $page } 页

# draft
print_page_count = { $count } 页

# draft
print_pages = 页面

# draft
print_pages_all = 全部

# draft
print_pages_current = 第 { $page } 页

# draft
print_pages_need_pdfium = 打印其页面需要 PDFium，但找不到它

# draft
print_pages_ranges = 页面…

# draft
print_pages_refused = 要打印的页面：{ $why }

# draft
print_pages_selection = 所选内容

# draft
print_pages_to_print = 要打印的页面

# draft
print_paper = 纸张

# draft
print_past = 它只有 { $count } 页；超出的页面已略去

# draft
print_pdf_each = 每个文件一个 PDF

# draft
print_pdf_one = 全部合为一个 PDF

# draft
print_pdfs = PDF

# draft
print_per_side = 每面 { $count } 页：小册子每面须为 2、4 或 8 页

# draft
print_per_side_choice = 每面 { $count } 页

# draft
print_per_side_field = 每面页数

# draft
print_place_files = { $count } 个文件

# draft
print_places = 位置

# draft
print_plugin_description = 按预览所示原样打印，或打印为小册子或联系表，或另存为 PDF

# draft
print_plugin_off = 另存为 PDF 需要启用打印插件

# draft
print_plugin_title = 打印

# draft
print_print = 打印…

# draft
print_print_description = 按预览所示打印文件，按其顺序作为一个作业（`layout`：按所示；按每帖 `signature` 页折叠的小册子，可加 `trim`；图片的联系表；窗格的列表），打印所选的 `pages`；未给出 `asked` 时，窗口先在 Skwir 的对话框中询问，然后由系统对话框选择打印机、纸张和单双面

# draft
print_print_listing = 打印列表…

# draft
print_ranges_placeholder = 从 1 开始：1-3, 7, 10-（到末尾）

# draft
print_save = 保存

# draft
print_save_as = 另存为

# draft
print_save_as_help = 从 ~ 或根目录开始的文件夹，然后是名称：按 Tab 补全文件夹

# draft
print_save_as_refused = 另存为：{ $why }

# draft
print_save_each = { $form ->
        [booklet] 将 { $count } 个文件分别保存为小册子
       *[pdf] 将 { $count } 个文件分别保存为 PDF
    }

# draft
print_save_listing = 将{ $name }的列表保存为 PDF

# draft
print_save_one = { $form ->
        [booklet] 将{ $name }保存为小册子
       *[pdf] 将{ $name }保存为 PDF
    }

# draft
print_save_pdf = 另存为 PDF

# draft
print_save_pdf_description = 按预览所示为每个文件生成一个 PDF，放在其旁边或 `dest` 中（给出 `together` 时全部合为一个），使用系统区域设置的纸张，版式和页面与“打印…”所用的相同（计划）

# draft
print_save_sheet = 将包含 { $count } 个文件的联系表保存为 PDF

# draft
print_save_step = { $what ->
        [file] 将{ $name }作为{ $pdf }保存到 { $folder }
        [files] 将{ $name }及另外 { $more } 个文件作为{ $pdf }保存到 { $folder }
        [sheet] 将包含 { $count } 个文件的联系表作为{ $pdf }保存到 { $folder }
       *[listing] 将{ $name }的列表作为{ $pdf }保存到 { $folder }
    }

# draft
print_save_together = { $form ->
        [booklet] 将 { $count } 个文件保存为一本小册子
       *[pdf] 将 { $count } 个文件保存为一个 PDF
    }

# draft
print_save_verb = 另存为 PDF

# draft
print_saved_step = { $what ->
        [file] 已将{ $name }作为{ $pdf }保存到 { $folder }
        [files] 已将{ $name }及另外 { $more } 个文件作为{ $pdf }保存到 { $folder }
        [sheet] 已将包含 { $count } 个文件的联系表作为{ $pdf }保存到 { $folder }
       *[listing] 已将{ $name }的列表作为{ $pdf }保存到 { $folder }
    }

# draft
print_sent = 已将{ $what ->
        [file] { $name }
        [files] { " " }{ $count } 个文件
        [sheet] 联系表
       *[listing] { $name }的列表
    }{ $pages ->
        [0] {""}
       *[other] 的 { $pages } 页
    }{ $to ->
        [file] 输出到{ $target }
        [printer] 发送到 { $printer }
       *[system] 发送到打印机
    }

# draft
print_sets_of_16 = 每组 16 页

# draft
print_sets_of_32 = 每组 32 页

# draft
print_sheets_in_fold = { $count } 张纸合为一组

# draft
print_shows_nothing = 窗格未显示任何内容

# draft
print_smaller = 减小文件体积

# draft
print_smaller_folder = 用于减小 PDF 体积的文件夹

# draft
print_smaller_help = 其中的图片最高 150 dpi，与“减小体积”的“屏幕”相同

# draft
print_then = { $said }。{ $then }

# draft
print_title = { $what ->
        [file] 打印{ $name }
        [files] 打印 { $count } 个文件
        [sheet] 打印联系表
       *[listing] 打印{ $name }的列表
    }

# draft
print_to = 输出到

# draft
print_to_pdf = PDF

# draft
print_to_printer = 打印机

# draft
print_trim = 裁去白边

# draft
print_trim_help = 按内容区域缩放页面，所有页面一致

# draft
print_went_missing = { $name }在生成时丢失

# draft
print_work_folder = 用于生成文档的文件夹

# draft
print_wrong_size = 尺寸不对的图片
