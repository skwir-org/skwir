# Simplified Chinese catalog of skwir-plugin-documents, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
documents_detail_application = 应用程序

# draft
documents_detail_author = 作者

# draft
documents_detail_keywords = 关键词

# draft
documents_detail_pages = 页数

# draft
documents_detail_subject = 主题

# draft
documents_detail_title = 标题

# draft
documents_detail_words = 字数

# draft
documents_docx_no_body = 此 Word 文档没有正文

# draft
documents_docx_no_text = 这不是 Word 文档：其中没有文本部分

# draft
documents_engine = 文档引擎

# draft
documents_engine_auto = 自动

# draft
documents_engine_description = “自动”优先使用 PDFium，不可用时改用 Poppler；只有选中时才使用 MuPDF。

# draft
documents_first_blocks = { $count ->
        [1] 只显示了第一个块
       *[other] 只显示了前 { $count } 个块
    }

# draft
documents_first_blocks_of_part = { $count ->
        [1] 只显示了此部分的第一个块
       *[other] 只显示了此部分的前 { $count } 个块
    }

# draft
documents_members = 文档包含的部件超过 { $count } 个

# draft
documents_missing_manifest = 缺少文档的清单

# draft
documents_missing_notes = 缺少文档的脚注和尾注

# draft
documents_missing_numbering = 缺少文档的编号定义

# draft
documents_missing_picture = 缺少一张嵌入的图片

# draft
documents_missing_properties = 缺少文档的属性

# draft
documents_missing_relationships = 缺少文档的关系定义

# draft
documents_missing_styles = 缺少文档的样式

# draft
documents_missing_text = 缺少文档的文本

# draft
documents_missing_thumbnail = 缺少缩略图

# draft
documents_no_reader = 已安装的程序都无法将此文档作为文本读取

# draft
documents_not_its_picture = 不是此文档中的图片

# draft
documents_odt_no_content = 这不是 OpenDocument 文本：其中没有内容

# draft
documents_odt_not_text = 此 OpenDocument 文件不是文本文档

# draft
documents_odt_password = 此文档受密码保护

# draft
documents_part_of = 第 { $part } 部分，共 { $parts } 部分

# draft
documents_parts_keys = 按 [ 和 ] 在各部分之间切换

# draft
documents_pdfium_library = PDFium 库

# draft
documents_pdfium_library_description = PDFium 共享库的路径。

# draft
documents_pdfium_library_description_more = 留空则在常用位置中查找。

# draft
documents_plugin_description = 阅读 PDF 和办公文档：连续滚动、搜索、选择文本、链接和目录；Word 和 OpenDocument 文本还可作为适应面板宽度的文本阅读

# draft
documents_plugin_title = 文档

# draft
documents_read_by_pandoc = 由 Pandoc 读取 · 不显示版式、页眉和批注

# draft
documents_reading_mode = 阅读模式 · 不显示版式、页眉和批注

# draft
documents_requirement_libreoffice = 以分页方式显示办公文档

# draft
documents_requirement_mupdf = 使用 MuPDF 阅读 PDF

# draft
documents_requirement_pandoc = 由 Pandoc 读取文本文档，以及将 RTF 文件作为文本读取

# draft
documents_requirement_pdfium = 使用 PDFium 阅读 PDF

# draft
documents_requirement_poppler = 使用 Poppler 阅读 PDF

# draft
documents_requirement_sandbox = 隔离运行 LibreOffice（必需）和 Pandoc

# draft
documents_start_only = 只显示了开头部分

# draft
documents_text_documents = 文本文档

# draft
documents_text_documents_description = Word、OpenDocument 和 RTF 文档的显示方式。

# draft
documents_text_documents_description_more = “阅读”一次显示其中的文本、标题、列表、表格、脚注和图片，并适应面板宽度：Word 和 OpenDocument 文件由 Skwir 读取，RTF 由 Pandoc 读取。“分页”按 LibreOffice 打印的样子排版，每个文档需要几秒钟。“Pandoc”让 Pandoc 读取所有这些文档。所选方式需要的程序未安装时，将改用其他方式。

# draft
documents_text_documents_pages = 分页

# draft
documents_text_documents_reading = 阅读

# draft
documents_too_large_manifest = 文档的清单超过了 { $size } 的预览上限

# draft
documents_too_large_notes = 文档的脚注和尾注超过了 { $size } 的预览上限

# draft
documents_too_large_numbering = 文档的编号定义超过了 { $size } 的预览上限

# draft
documents_too_large_picture = 一张嵌入的图片超过了 { $size } 的预览上限

# draft
documents_too_large_properties = 文档的属性超过了 { $size } 的预览上限

# draft
documents_too_large_relationships = 文档的关系定义超过了 { $size } 的预览上限

# draft
documents_too_large_styles = 文档的样式超过了 { $size } 的预览上限

# draft
documents_too_large_text = 文档的文本超过了 { $size } 的预览上限

# draft
documents_too_large_thumbnail = 缩略图超过了 { $size } 的预览上限
