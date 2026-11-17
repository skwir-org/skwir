# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A label in the preview's Properties: the program that saved the document, with its version.
documents_detail_application = Application

# A label in the preview's Properties: who wrote the document, as it says.
documents_detail_author = Author

# A label in the preview's Properties: the keywords the document gives itself.
documents_detail_keywords = Keywords

# A label in the preview's Properties: how many pages the document had when it was saved.
documents_detail_pages = Pages

# A label in the preview's Properties: the subject the document gives itself.
documents_detail_subject = Subject

# A label in the preview's Properties: the title the document gives itself.
documents_detail_title = Title

# A label in the preview's Properties: how many words the document holds.
documents_detail_words = Words

# An error in place of a Word document whose text part holds no body.
documents_docx_no_body = This Word document has no body

# An error in place of a file named as a Word document that holds no text.
documents_docx_no_text = This is not a Word document: it has no text part

# A setting's title: which program reads PDF documents.
documents_engine = Document engine

# A choice of the Document engine setting: the first engine installed.
documents_engine_auto = Automatic

# Automatic is the setting's first choice; PDFium, Poppler and MuPDF are the programs' names.
documents_engine_description = Automatic prefers PDFium and falls back to Poppler; MuPDF is used only when chosen.

# A note under a document's preview: only its first paragraphs, headings, tables and pictures (blocks) are shown.
documents_first_blocks = { $count ->
       *[other] First { $count } blocks
    }

# A note under a document's preview: only the first paragraphs, headings, tables and pictures (blocks) of the part shown are shown.
documents_first_blocks_of_part = { $count ->
       *[other] First { $count } blocks of this part
    }

# Why a Word or OpenDocument file is not shown: its package holds more parts than the preview reads (4,000).
documents_members = { $count ->
       *[other] The document holds more than { $count } members
    }

# Why an OpenDocument file is not shown: the list of its parts (META-INF/manifest.xml) is not there.
documents_missing_manifest = Manifest is missing

# Why a Word file's footnotes or endnotes are not shown: the part holding them is not there.
documents_missing_notes = Notes is missing

# Why a Word file is not shown: the part saying how its lists are numbered is not there.
documents_missing_numbering = Numbering is missing

# Why a picture of a Word or OpenDocument file is not shown: the file names one it does not hold.
documents_missing_picture = Embedded image is missing

# Why what a document says of itself (its title, author) is not shown: the part holding it is not there.
documents_missing_properties = Properties is missing

# Why a Word file is not shown: the part that says where its other parts are (its relationships, _rels/*.rels) is not there.
documents_missing_relationships = Relationships is missing

# Why a Word or OpenDocument file is not shown: the part holding its styles is not there.
documents_missing_styles = Styles is missing

# Why a Word or OpenDocument file is not shown: the part holding its text is not there.
documents_missing_text = The document's text is missing

# Why a document's stored picture is not shown: the picture it names is not there.
documents_missing_thumbnail = Thumbnail is missing

# An error in place of a text document when neither skwir nor Pandoc reads its format.
documents_no_reader = Nothing installed reads this document as text

# An error in place of a picture in a document's preview: the picture asked for is none the document holds.
documents_not_its_picture = not a picture of this document

# An error in place of a file named as an OpenDocument text that holds none.
documents_odt_no_content = This is not an OpenDocument text: it has no content

# An error in place of an OpenDocument file (a spreadsheet, a presentation) named as a text.
documents_odt_not_text = This OpenDocument file is not a text document

# An error in place of an OpenDocument text that is encrypted.
documents_odt_password = This document is protected by a password

# The title of a long document's preview, shown a part at a time: which part, of how many.
documents_part_of = Part { $part } of { $parts }

# A note under a long document's preview, shown a part at a time: the [ and ] keys show the part before and the part after.
documents_parts_keys = [ and ] move through its parts

# A setting's title: where the PDFium library is.
documents_pdfium_library = PDFium library

documents_pdfium_library_description = Path to the PDFium shared library.

# What a setting's description says after its first sentence, in its tooltip.
documents_pdfium_library_description_more = Leave empty to search the usual locations.

documents_plugin_description = Read PDF and office documents: continuous scrolling, search, text selection, links and contents; Word and OpenDocument text also as text that fits the panel

documents_plugin_title = Documents

# A note under a text document's preview read by Pandoc, a program.
documents_read_by_pandoc = Read by Pandoc · layout, headers and comments are not shown

# A note under a Word or OpenDocument file's preview, read by skwir as text that fits the panel.
documents_reading_mode = Reading mode · layout, headers and comments are not shown

# What stops working without LibreOffice, in the settings' list of what a plugin needs.
documents_requirement_libreoffice = Office documents shown as pages

# What stops working without MuPDF's helper, in the settings' list of what a plugin needs.
documents_requirement_mupdf = PDF reading with MuPDF

# What stops working without Pandoc, in the settings' list of what a plugin needs.
documents_requirement_pandoc = Text documents read by Pandoc, and RTF files read as text

# What stops working without PDFium's helper, in the settings' list of what a plugin needs.
documents_requirement_pdfium = PDF reading with PDFium

# What stops working without Poppler's helper, in the settings' list of what a plugin needs.
documents_requirement_poppler = PDF reading with Poppler

# What stops working without the sandbox program (bubblewrap), in the settings' list of what a plugin needs: LibreOffice is not run without it, Pandoc runs in its own sandbox where it has one.
documents_requirement_sandbox = Isolation of LibreOffice, which needs it, and of Pandoc

# A note under a document's preview: Pandoc's reading of it was longer than the preview shows.
documents_start_only = Only the start is shown

# A setting's title: how Word, OpenDocument and RTF documents are shown in the preview.
documents_text_documents = Text documents

# Reading, Pages and Pandoc are the setting's three choices; Pandoc and LibreOffice are programs' names.
documents_text_documents_description = How Word, OpenDocument and RTF documents are shown.

# Reading, Pages and Pandoc are the setting's three choices; Pandoc and LibreOffice are programs' names.
documents_text_documents_description_more = Reading shows their text, headings, lists, tables, notes and pictures at once, to fit the panel: skwir reads Word and OpenDocument files, Pandoc RTF. Pages lays them out as LibreOffice prints them, which takes a few seconds each. Pandoc has Pandoc read them all. When what a choice needs is not installed, another is used.

# A choice of the Text documents setting: the document laid out in pages, as LibreOffice prints it.
documents_text_documents_pages = Pages

# A choice of the Text documents setting: the document's text read to fit the panel (a reading mode).
documents_text_documents_reading = Reading

# Why an OpenDocument file is not shown: the list of its parts (META-INF/manifest.xml) unpacks past what the preview reads. size: that bound (16 MB).
documents_too_large_manifest = Manifest exceeds { $size } preview limit

# Why a Word file's footnotes or endnotes are not shown: the part holding them unpacks past what the preview reads. size: that bound (16 MB).
documents_too_large_notes = Notes exceeds { $size } preview limit

# Why a Word file is not shown: the part saying how its lists are numbered unpacks past what the preview reads. size: that bound (16 MB).
documents_too_large_numbering = Numbering exceeds { $size } preview limit

# Why a picture of a Word or OpenDocument file is not shown: it unpacks past what the preview reads. size: that bound (16 MB).
documents_too_large_picture = Embedded image exceeds { $size } preview limit

# Why what a document says of itself (its title, author) is not shown: the part holding it unpacks past what the preview reads. size: that bound (16 MB).
documents_too_large_properties = Properties exceeds { $size } preview limit

# Why a Word file is not shown: the part that says where its other parts are (its relationships) unpacks past what the preview reads. size: that bound (16 MB).
documents_too_large_relationships = Relationships exceeds { $size } preview limit

# Why a Word or OpenDocument file is not shown: the part holding its styles unpacks past what the preview reads. size: that bound (16 MB).
documents_too_large_styles = Styles exceeds { $size } preview limit

# Why a Word or OpenDocument file is not shown: the part holding its text unpacks past what the preview reads. size: that bound (16 MB).
documents_too_large_text = The document's text exceeds { $size } preview limit

# Why a document's stored picture is not shown: it unpacks past what the preview reads. size: that bound (16 MB).
documents_too_large_thumbnail = Thumbnail exceeds { $size } preview limit
