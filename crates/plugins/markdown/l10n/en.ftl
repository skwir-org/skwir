# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A label in the preview's Properties: how many cells a Jupyter notebook holds.
markdown_detail_cells = Cells

# A label in the preview's Properties: how many headings the document holds.
markdown_detail_headings = Headings

# A label in the preview's Properties: the Jupyter kernel a notebook runs on.
markdown_detail_kernel = Kernel

# A label in the preview's Properties: the programming language a notebook's code is in.
markdown_detail_language = Language

# A label in the preview's Properties: how many words the document holds.
markdown_detail_words = Words

# A note under an HTML file's preview: it is read as a document, inert.
markdown_html_reading_mode = Reading mode · scripts, fonts and remote content are not loaded

# Why a notebook is not previewed: the file is larger than the preview reads. size: that bound (16 MB).
markdown_notebook_file_too_large = Notebook exceeds { $size } preview limit

# An error in place of a Jupyter notebook written in an older format.
markdown_notebook_format = Only notebooks in format 4 (Jupyter 4 and later) are read

# Why a notebook is not previewed: its text is longer than the preview reads. size: that bound (2 MB).
markdown_notebook_too_large = Notebook text exceeds { $size } preview limit

# A note under a notebook: the results of its cells that printed too many lines are cut.
markdown_outputs_shortened = { $count ->
        [one] { $count } output
       *[other] { $count } outputs
    } shortened to their first lines

markdown_plugin_description = Read Markdown and HTML files and Jupyter notebooks as documents, with headings, tables, code, formulas, Mermaid diagrams and local images; switch to the source at any time. HTML is inert: no scripts, fonts or remote content

markdown_plugin_title = Markdown, HTML and notebooks

# What stops working without the formulas' helper, in the settings' list of what a plugin needs.
markdown_requirement_math = Formulas typeset in Markdown, notebooks and HTML

# What stops working without the Mermaid helper, in the settings' list of what a plugin needs.
markdown_requirement_mermaid = Mermaid diagrams drawn in Markdown

# The title of a Jupyter notebook's preview.
markdown_title_notebook = Notebook
