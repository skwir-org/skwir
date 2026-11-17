# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A row of the contents of a book that has none of its own: its chapter's number, from 1.
books_chapter = Chapter { $number }

# A notice above a chapter of a book that holds more than the preview reads.
books_chapter_cut = The chapter is cut: it holds more than reading mode reads

# A label in the preview's Properties: who wrote the book.
books_detail_author = Author

# A label in the preview's Properties: how many chapters the book has.
books_detail_chapters = Chapters

# A label in the preview's Properties: the book's genres, as the book names them.
books_detail_genre = Genre

# A label in the preview's Properties: the book's International Standard Book Number.
books_detail_isbn = ISBN

# A label in the preview's Properties: the language the book is written in.
books_detail_language = Language

# A label in the preview's Properties: the date the book was published.
books_detail_published = Published

# A label in the preview's Properties: who published the book.
books_detail_publisher = Publisher

# A label in the preview's Properties: the series the book belongs to.
books_detail_series = Series

# A label in the preview's Properties: what the book is about, as it says.
books_detail_subject = Subject

# A label in the preview's Properties: the book's title.
books_detail_title = Title

# A label in the preview's Properties: the year the book was published.
books_detail_year = Year

# A setting: what reads books, the built-in engine or MuPDF.
books_engine = Book engine

# A choice of the Book engine setting.
books_engine_auto = Automatic

# A choice of the Book engine setting: Skwir's own engine.
books_engine_builtin = Built-in

books_engine_description = Automatic reads books with the built-in engine, which reflows their text in the theme and draws fixed-layout pages as the book lays them out.

# What a setting's description says after its first sentence, in its tooltip.
books_engine_description_more = MuPDF lays books out into pages, with their own styles: every EPUB, FB2 and MOBI book it reads well; the built-in engine reads the others.

# Why a book is not shown: its package holds more files than the preview reads (4,000).
books_epub_members = { $count ->
       *[other] This EPUB holds more than { $count } members
    }

# An error: a book names a part of itself it does not hold. href: the part's name in the book.
books_epub_missing = Missing EPUB resource { $href }

# An error: the book's META-INF/container.xml names no package document.
books_epub_no_package = EPUB container names no package

# An error: a book laid out in pages lists none.
books_epub_no_pages = EPUB has no pages

# Why a book, or a chapter of it, is not shown: the book is protected (DRM).
books_epub_protected_chapter = This EPUB is protected: a chapter is encrypted

# The file that names the book's package (META-INF/container.xml) is encrypted.
books_epub_protected_container = This EPUB is protected: its container is encrypted

# The book's table of contents is encrypted.
books_epub_protected_contents = This EPUB is protected: its table of contents is encrypted

# Its list of what it encrypts (META-INF/encryption.xml) is itself encrypted.
books_epub_protected_encryption = This EPUB is protected: even its list of encrypted parts is encrypted

# The book's package document (the OPF), which lists its chapters, is encrypted.
books_epub_protected_package = This EPUB is protected: its package is encrypted

# Why a page of a fixed-layout book is not shown: the book is protected (DRM).
books_epub_protected_page = This EPUB is protected: a page is encrypted

# Why a picture of a book is not shown: the book is protected (DRM).
books_epub_protected_picture = This EPUB is protected: an embedded image is encrypted

# What a chapter names (a font, a file) is encrypted.
books_epub_protected_resource = This EPUB is protected: a resource a chapter uses is encrypted

# Why a book's styles are not read: the book is protected (DRM).
books_epub_protected_stylesheet = This EPUB is protected: a stylesheet is encrypted

# Why a book is not shown: a chapter of an EPUB book unpacks past what the preview reads. size: that bound (16 MB).
books_epub_too_large_chapter = A chapter exceeds the { $size } preview limit

# Why a book is not shown: the file that names its package (META-INF/container.xml) unpacks past what the preview reads. size: that bound (16 MB).
books_epub_too_large_container = The EPUB's container exceeds the { $size } preview limit

# Why a book's contents are not shown: its table of contents unpacks past what the preview reads. size: that bound (16 MB).
books_epub_too_large_contents = The EPUB's table of contents exceeds the { $size } preview limit

# Why a book is not shown: its list of what it encrypts (META-INF/encryption.xml) unpacks past what the preview reads. size: that bound (16 MB).
books_epub_too_large_encryption = The EPUB's list of encrypted parts exceeds the { $size } preview limit

# Why a book is not shown: its package document (the OPF), which lists its chapters, unpacks past what the preview reads. size: that bound (16 MB).
books_epub_too_large_package = The EPUB's package exceeds the { $size } preview limit

# Why a book is not shown: a page of a fixed-layout EPUB book unpacks past what the preview reads. size: that bound (16 MB).
books_epub_too_large_page = A page exceeds the { $size } preview limit

# Why a picture of an EPUB book is not shown: it unpacks past what the preview reads. size: that bound (16 MB).
books_epub_too_large_picture = An embedded image exceeds the { $size } preview limit

# Why what a chapter of an EPUB book names (a font, a file) is not read: it unpacks past what the preview reads. size: that bound (16 MB).
books_epub_too_large_resource = A resource of the EPUB exceeds the { $size } preview limit

# Why a stylesheet of an EPUB book is not read: it unpacks past what the preview reads. size: that bound (16 MB).
books_epub_too_large_stylesheet = A stylesheet exceeds the { $size } preview limit

# An error: a book laid out in pages lists more than can be shown.
books_epub_too_many_pages = EPUB has too many pages

# An error: a picture a FictionBook holds cannot be decoded. id: the picture's id in the book.
books_fb2_damaged_picture = This FB2's picture { $id } is damaged

# An error: the FictionBook a zipped FB2 names is not in it.
books_fb2_document_missing = The FB2 document is missing

# An error: the FictionBook a zipped FB2 holds unpacks past what the preview reads. size: that bound (16 MB).
books_fb2_document_too_large = The FB2 document exceeds the { $size } preview limit

# An error: a chapter of a FictionBook asked for is not there. href: the chapter's name.
books_fb2_missing_chapter = Missing FB2 chapter { $href }

# An error: a FictionBook names a picture it does not hold. id: the picture's id in the book.
books_fb2_missing_picture = Missing FB2 picture { $id }

# An error: a FictionBook holds no text (its `body` element).
books_fb2_no_body = This FB2 has no body

# An error: a file typed as a FictionBook does not hold one.
books_fb2_not_fb2 = This is not an FB2 document

# The heading of a book's notes, and its row in the contents, where the book gives them none.
books_fb2_notes = Notes

# An error: a FictionBook is larger than the preview reads.
books_fb2_too_large = This FB2 is too large to read

# An error: a FictionBook has more chapters than the preview reads.
books_fb2_too_many_chapters = { $count ->
       *[other] This FB2 book has more than { $count } chapters
    }

# An error: a ZIP file typed as a zipped FictionBook holds no FictionBook.
books_fb2_zip_empty = This zipped FB2 holds no FB2

# An error: a ZIP file typed as a zipped FictionBook holds more files than the preview reads (4,000).
books_fb2_zip_members = { $count ->
       *[other] This zipped FB2 holds more than { $count } members
    }

# An error: a Kindle (KF8) book lacks the index of the pieces of its text.
books_kf8_no_fragments = This KF8 book has no fragment index

# An error: a Kindle (KF8) book lacks the index of how its parts are laid out.
books_kf8_no_skeletons = This KF8 book has no skeleton index

# An error: the text of a Kindle (KF8) book cannot be put together.
books_kf8_text_damaged = This book's KF8 text is damaged

# An error: the phrases a book's compressed text is made of decode to more than the preview reads.
books_mobi_dictionary_too_large = This book's dictionary is too large to read

# An error: the tables a book's compressed text is decoded with cannot be read.
books_mobi_huffman_damaged = This book's Huffman tables are damaged

# An error: an index in a Mobipocket book (its contents, its parts) cannot be read.
books_mobi_index_damaged = This book's index is damaged

# An error: KFX is the name of a format of Amazon's Kindle books.
books_mobi_kfx = This is a KFX book, Amazon's own format, which is not read

# An error: a chapter of a book asked for is not there. href: the chapter's name.
books_mobi_missing_chapter = Missing chapter { $href }

# An error: a book names a picture it does not hold. key: the picture's record in the book.
books_mobi_missing_picture = Missing picture { $key }

# An error: a book compressed with Huffman coding does not say where its tables are.
books_mobi_no_huffman = This book names no Huffman tables

# An error: a file typed as a Mobipocket book does not hold one.
books_mobi_not_mobi = This is not a Mobipocket book

# An error: a Mobipocket book whose text is encrypted (digital rights management).
books_mobi_protected = This book is protected: its text is encrypted (DRM)

# An error: a Mobipocket book's compressed text cannot be decoded.
books_mobi_text_damaged = This book's text is damaged

# skwir-mupdf-worker is the name of a program, as it is installed.
books_mupdf_missing = Reading books with MuPDF needs skwir-mupdf-worker; install it, or choose the built-in book engine

# What stops working without the formulas helper, in the plugin's requirements.
books_needs_math = Formulas a book writes in MathML typeset

# What stops working without the MuPDF helper, in the plugin's requirements.
books_needs_mupdf = Books laid out into pages by MuPDF

# format: the book's format, as it is named (EPUB, FB2, MOBI, AZW3, PalmDOC).
books_no_chapters = { $format } has no readable chapters

# An error: a page of a book laid out in pages could not be drawn.
books_page_not_drawn = The page could not be drawn

# An error: a Mobipocket book's text is compressed in a way Skwir does not decode. code: the number the book names it by.
books_palm_compression = This book's text is compressed in a way not read ({ $code })

# An error: a Mobipocket book's file ends before what it says it holds.
books_palm_cut_short = This book is cut short

# An error: the part of a Mobipocket book that says how it is stored is damaged.
books_palm_first_record_short = This book's first record is cut short

# An error: a Mobipocket book names a part of its file it does not have. index: the part's number.
books_palm_no_record = This book has no record { $index }

# An error: a Palm database (a Mobipocket book's file) holds no records.
books_palm_no_records = This book has no records

# An error: a Palm database (a Mobipocket book's file) is damaged.
books_palm_out_of_order = This book's records are out of order

# An error: a part of a Palm database (a Mobipocket book's file) is larger than the preview reads.
books_palm_record_too_large = A record of this book is too large to read

books_plugin_description = Read EPUB, FB2 and MOBI books with their table of contents

books_plugin_title = Books

# A notice above a book: the text encoding its file was decoded from, as it is named (windows-1251).
books_read_as = Read as { $encoding }

# The notice above a book read in reading mode.
books_reading_mode = Reading mode · the book's fonts, colours and page layout are not applied

# The notice above a book's chapter shown because it holds what was searched for.
books_search_found = Search match in this chapter

# The notice above a book none of whose chapters holds what was searched for.
books_search_none = No matching chapter

# An error: what was searched for in a book is too long.
books_search_too_long = The text searched for is too long

# A book's series in its Properties, and the book's number in it, both as the book writes them (Foundation, 2).
books_series_number = { $series }, { $number }

# The title of a book's preview: its format as it is named (EPUB, FB2, MOBI), the chapter shown, from 1, and how many there are.
books_title = { $format } · chapter { $chapter } / { $count }
