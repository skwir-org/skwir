# Simplified Chinese catalog of skwir-plugin-shrink, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
shrink_already = { $copy }已是{ $file }的精简副本

# draft
shrink_already_there = { $name }已存在

# draft
shrink_as_small = { $name }已无法再缩小：未制作副本

# draft
shrink_at_most = 大小上限

# draft
shrink_at_most_placeholder = 不限，或填写大小：2 MB、500 KB

# draft
shrink_at_most_refused = 大小上限：{ $why }

# draft
shrink_both = 图像最高 { $dpi } dpi；图片 { $pixels } 像素

# draft
shrink_cancelled = 已取消制作副本

# draft
shrink_clash_cancelled = 已取消：未缩小任何文件

# draft
shrink_closing = Skwir 正在关闭

# draft
shrink_dialog_accept = 制作副本

# draft
shrink_dialog_intro = { $count ->
        [1] 将在文件旁制作一个精简副本；原文件保持不变。
       *[other] 将在每个文件旁制作一个精简副本；原文件保持不变。
    }

# draft
shrink_dialog_title = { $count ->
        [1] 减小{ $name }的体积
       *[other] 减小 { $count } 个文件的体积
    }

# draft
shrink_gray = 灰度

# draft
shrink_gray_help = 转为灰度

# draft
shrink_gray_help_pdfs = 图片及 PDF 中的图像转为灰度

# draft
shrink_in_place_only = { $name }只接受原地写入的副本：无法在此制作精简副本

# draft
shrink_lossless = 无损

# draft
shrink_lossless_at_most = `lossless` 质量不重新编码任何内容，因此无法将副本控制在某个大小以下：请将 `at_most` 与 `print`、`screen` 或 `small` 质量一起使用

# draft
shrink_lossless_jpeg = “无损”不改动 JPEG：请选择“打印”“屏幕”或“最小”，或移除位置和相机信息

# draft
shrink_made = 已制作{ $file }的精简副本{ $copy }

# draft
shrink_make = 制作{ $file }的精简副本{ $copy }

# draft
shrink_make_verb = 制作精简副本

# draft
shrink_no_folder = 该文件不在任何文件夹中

# draft
shrink_no_size = { $typed }不是有效的大小

# draft
shrink_no_unit = { $unit }不是有效的单位：请写 KB 或 MB

# draft
shrink_none_made_note = 未制作{ $name }的精简副本：{ $said }

# draft
shrink_none_made_strip = 未制作{ $name }的精简副本，因此也未移除其位置和相机信息

# draft
shrink_not_a_folder = { $name }不是文件夹

# draft
shrink_not_a_picture = { $name }既不是 JPEG 或 PNG 图片，也不是 PDF

# draft
shrink_not_placed = 无法将{ $name }放入{ $folder }

# draft
shrink_not_under = 无法将{ $name }缩小到所要求的大小以下：其最小的副本为 { $size }

# draft
shrink_note = { $name }：{ $said }

# draft
shrink_nothing = 没有可缩小的文件

# draft
shrink_nothing_made = 在您做出选择之前，不会缩小任何文件。

# draft
shrink_nothing_re_encoded = 不重新编码任何内容

# draft
shrink_pdf_dpi = 图像最高 { $dpi } dpi

# draft
shrink_picture_pixels = 长边最多 { $pixels } 像素

# draft
shrink_plan = { $count ->
        [1] 制作{ $name }的精简副本
       *[other] 制作 { $count } 个文件的精简副本
    }

# draft
shrink_plugin_description = 在图片和 PDF 旁制作其精简副本：见“压缩”子菜单和预览菜单中的“减小体积…”

# draft
shrink_plugin_off = 制作精简副本需要启用此插件

# draft
shrink_plugin_title = 精简副本

# draft
shrink_print = 打印

# draft
shrink_quality = 质量

# draft
shrink_reduce = 制作精简副本

# draft
shrink_reduce_ask_description = 先询问缩小程度，再像 `shrink.reduce` 一样在每张图片和每个 PDF 旁制作精简副本

# draft
shrink_reduce_description = 在每张图片（JPEG、PNG）和每个 PDF 旁或在 `dest` 中制作精简副本，命名为 `name (reduced).ext`：`quality` 为 lossless、print（图片宽 3840 像素，PDF 中的图像 300 dpi）、screen（1920，150 dpi）或 small（1280，96 dpi）；`gray`，转为灰度；`strip`，不保留图片的位置和相机信息；`at_most`，每个副本须符合的大小，会依次尝试更小的档位。不会制作不比原文件小的副本（计划）

# draft
shrink_reduce_size = 减小体积…

# draft
shrink_requirement = 制作精简副本

# draft
shrink_same_pixels = 像素不变，毫无损失

# draft
shrink_screen = 屏幕

# draft
shrink_small = 最小

# draft
shrink_strip = 移除位置和相机信息

# draft
shrink_strip_help = 拍摄日期会保留

# draft
shrink_went_missing = { $name }在制作过程中消失了

# draft
shrink_work_folder = 用于制作副本的文件夹
