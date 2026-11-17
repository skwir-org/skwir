# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A setting's title: which program reads DjVu documents.
djvu_engine = DjVu engine

# A choice of the DjVu engine setting: the first engine installed.
djvu_engine_auto = Automatic

# Automatic is the setting's first choice; DjVuLibre and djvu-rs are the programs' names.
djvu_engine_description = Automatic prefers DjVuLibre, where it is installed, and falls back to djvu-rs, which Skwir carries.

# An error in place of a DjVu document when no engine is installed. first, second: the helper programs' names (skwir-djvulibre-worker, skwir-djvu-worker).
djvu_install_either = Install { $first } or { $second } to read DjVu documents

# An error in place of a DjVu document. helper: the helper program's name (skwir-djvulibre-worker). engine: the engine chosen in the settings (DjVuLibre).
djvu_install_helper = Install { $helper } to read DjVu documents with { $engine }

djvu_plugin_description = Read DjVu documents, scanned books mostly: continuous scrolling, search, text selection, links and contents; and show their first pages as thumbnails

djvu_plugin_title = DjVu

# What stops working without djvu-rs's helper, in the settings' list of what a plugin needs.
djvu_requirement_djvu_rs = DjVu reading with djvu-rs

# What stops working without DjVuLibre's helper, in the settings' list of what a plugin needs.
djvu_requirement_djvulibre = DjVu reading with DjVuLibre
