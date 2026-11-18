# Simplified Chinese catalog of skwir-plugin-books, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
books_chapter = 第 { $number } 章

# draft
books_chapter_cut = 本章已截断：其内容超出了阅读模式可读取的范围

# draft
books_detail_author = 作者

# draft
books_detail_chapters = 章节数

# draft
books_detail_genre = 体裁

# draft
books_detail_isbn = ISBN

# draft
books_detail_language = 语言

# draft
books_detail_published = 出版日期

# draft
books_detail_publisher = 出版者

# draft
books_detail_series = 系列

# draft
books_detail_subject = 主题

# draft
books_detail_title = 书名

# draft
books_detail_year = 年份

# draft
books_engine = 电子书引擎

# draft
books_engine_auto = 自动

# draft
books_engine_builtin = 内置

# draft
books_engine_description = “自动”使用内置引擎阅读电子书，它会按主题重排文本，并按书本身的排版绘制固定版式的页面。

# draft
books_engine_description_more = MuPDF 会按电子书自身的样式将其排成页面：它能很好读取的所有 EPUB、FB2 和 MOBI 电子书都由它处理；其余的由内置引擎读取。

# draft
books_epub_members = 此 EPUB 包含的成员超过 { $count } 个

# draft
books_epub_missing = 缺少 EPUB 资源 { $href }

# draft
books_epub_no_package = EPUB 容器未指定包文档

# draft
books_epub_no_pages = EPUB 没有页面

# draft
books_epub_protected_chapter = 此 EPUB 受保护：某个章节已加密

# draft
books_epub_protected_container = 此 EPUB 受保护：其容器已加密

# draft
books_epub_protected_contents = 此 EPUB 受保护：其目录已加密

# draft
books_epub_protected_encryption = 此 EPUB 受保护：连其加密部分的列表也已加密

# draft
books_epub_protected_package = 此 EPUB 受保护：其包文档已加密

# draft
books_epub_protected_page = 此 EPUB 受保护：某个页面已加密

# draft
books_epub_protected_picture = 此 EPUB 受保护：某张嵌入的图片已加密

# draft
books_epub_protected_resource = 此 EPUB 受保护：某个章节使用的资源已加密

# draft
books_epub_protected_stylesheet = 此 EPUB 受保护：某个样式表已加密

# draft
books_epub_too_large_chapter = 某个章节超出了 { $size } 的预览上限

# draft
books_epub_too_large_container = 此 EPUB 的容器超出了 { $size } 的预览上限

# draft
books_epub_too_large_contents = 此 EPUB 的目录超出了 { $size } 的预览上限

# draft
books_epub_too_large_encryption = 此 EPUB 的加密部分列表超出了 { $size } 的预览上限

# draft
books_epub_too_large_package = 此 EPUB 的包文档超出了 { $size } 的预览上限

# draft
books_epub_too_large_page = 某个页面超出了 { $size } 的预览上限

# draft
books_epub_too_large_picture = 某张嵌入的图片超出了 { $size } 的预览上限

# draft
books_epub_too_large_resource = 此 EPUB 的某个资源超出了 { $size } 的预览上限

# draft
books_epub_too_large_stylesheet = 某个样式表超出了 { $size } 的预览上限

# draft
books_epub_too_many_pages = EPUB 的页面过多

# draft
books_fb2_damaged_picture = 此 FB2 的图片 { $id } 已损坏

# draft
books_fb2_document_missing = 缺少 FB2 文档

# draft
books_fb2_document_too_large = FB2 文档超出了 { $size } 的预览上限

# draft
books_fb2_missing_chapter = 缺少 FB2 章节 { $href }

# draft
books_fb2_missing_picture = 缺少 FB2 图片 { $id }

# draft
books_fb2_no_body = 此 FB2 没有正文

# draft
books_fb2_not_fb2 = 这不是 FB2 文档

# draft
books_fb2_notes = 脚注

# draft
books_fb2_too_large = 此 FB2 过大，无法读取

# draft
books_fb2_too_many_chapters = 此 FB2 电子书的章节超过 { $count } 个

# draft
books_fb2_zip_empty = 此压缩的 FB2 中没有 FB2 文档

# draft
books_fb2_zip_members = 此压缩的 FB2 包含的成员超过 { $count } 个

# draft
books_kf8_no_fragments = 此 KF8 电子书没有片段索引

# draft
books_kf8_no_skeletons = 此 KF8 电子书没有骨架索引

# draft
books_kf8_text_damaged = 此电子书的 KF8 文本已损坏

# draft
books_mobi_dictionary_too_large = 此电子书的字典过大，无法读取

# draft
books_mobi_huffman_damaged = 此电子书的 Huffman 表已损坏

# draft
books_mobi_index_damaged = 此电子书的索引已损坏

# draft
books_mobi_kfx = 这是 KFX 电子书，即 Amazon 的专有格式，无法读取

# draft
books_mobi_missing_chapter = 缺少章节 { $href }

# draft
books_mobi_missing_picture = 缺少图片 { $key }

# draft
books_mobi_no_huffman = 此电子书未指明 Huffman 表

# draft
books_mobi_not_mobi = 这不是 Mobipocket 电子书

# draft
books_mobi_protected = 此电子书受保护：其文本已加密（DRM）

# draft
books_mobi_text_damaged = 此电子书的文本已损坏

# draft
books_mupdf_missing = 使用 MuPDF 阅读电子书需要 skwir-mupdf-worker；请安装它，或选择内置的电子书引擎

# draft
books_needs_math = 排版电子书中以 MathML 编写的公式

# draft
books_needs_mupdf = 由 MuPDF 排成页面的电子书

# draft
books_no_chapters = { $format } 没有可读的章节

# draft
books_page_not_drawn = 无法绘制该页面

# draft
books_palm_compression = 此电子书的文本采用无法读取的方式压缩（{ $code }）

# draft
books_palm_cut_short = 此电子书不完整

# draft
books_palm_first_record_short = 此电子书的第一条记录不完整

# draft
books_palm_no_record = 此电子书没有记录 { $index }

# draft
books_palm_no_records = 此电子书没有任何记录

# draft
books_palm_out_of_order = 此电子书的记录顺序错乱

# draft
books_palm_record_too_large = 此电子书的某条记录过大，无法读取

# draft
books_plugin_description = 阅读 EPUB、FB2 和 MOBI 电子书，带目录

# draft
books_plugin_title = 电子书

# draft
books_read_as = 按 { $encoding } 读取

# draft
books_reading_mode = 阅读模式 · 不应用电子书的字体、颜色和页面版式

# draft
books_search_found = 本章包含所搜索的内容

# draft
books_search_none = 没有匹配的章节

# draft
books_search_too_long = 所搜索的文本过长

# draft
books_series_number = { $series }，{ $number }

# draft
books_title = { $format } · 第 { $chapter } 章，共 { $count } 章
