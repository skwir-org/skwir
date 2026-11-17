# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A row's label in Properties: the keywords, rating and description written inside a picture (its XMP).
xmp_embedded_label = XMP in the file

# What XMP sidecars cannot hold that other places notes are kept can, placed after "What it cannot hold:" in its settings: a rating in half stars, a tag's color unless it is one of XMP's color labels.
xmp_limits = half stars, or a tag's color other than as a color label

xmp_plugin_description = Tags, ratings and descriptions in XMP sidecars, as photo applications keep them; XMP inside pictures and PDFs shown in Properties

xmp_plugin_title = XMP

xmp_store_title = XMP sidecars (darktable, digiKam, Lightroom)

# Part of what Properties says of the XMP inside a picture: its description, as written in it, quoted.
xmp_summary_comment = “{ $comment }”

# Part of what Properties says of the XMP inside a picture: its keywords, as written in it.
xmp_summary_keywords = Keywords { $keywords }

# Two parts of what Properties says of the XMP inside a picture (its keywords, its rating, its description), on one line.
xmp_summary_parts = { $before } · { $after }
