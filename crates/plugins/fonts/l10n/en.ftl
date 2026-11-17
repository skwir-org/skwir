# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A label in the preview's Properties: how many characters the font has a glyph for.
fonts_detail_characters = Characters

# A label in the preview's Properties: who designed the font.
fonts_detail_designer = Designer

# A label in the preview's Properties: the font's family name.
fonts_detail_family = Family

# A label in the preview's Properties: how many glyphs the font holds.
fonts_detail_glyphs = Glyphs

# A label in the preview's Properties: the font's licence, as the font states it.
fonts_detail_license = License

# A label in the preview's Properties: what the font's glyphs are drawn with (TrueType, PostScript (CFF), Bitmaps only).
fonts_detail_outlines = Outlines

# A label in the preview's Properties: the web font format the font came packed in (WOFF, WOFF2).
fonts_detail_packed_as = Packed as

# A label in the preview's Properties: the font's style in its family (Bold Italic).
fonts_detail_style = Style

# A label in the preview's Properties: whether the font is a variable font, one whose weight or width can be varied.
fonts_detail_variable = Variable

# A label in the preview's Properties: the font's version, as the font states it.
fonts_detail_version = Version

# Why a font is not previewed: it is larger than the preview reads. size: that bound (16 MB).
fonts_file_too_large = Font exceeds { $size } preview limit

# A note under a font's specimen: glyphs too large to draw were left out.
fonts_glyphs_skipped = { $count ->
        [one] { $count } oversized glyph was not drawn
       *[other] { $count } oversized glyphs were not drawn
    }

# An error in place of a font whose header table is missing or damaged.
fonts_no_header = This font has no readable header

# A note under a font's specimen: what is typed in the preview's search is drawn instead of the sample text; of a file holding several fonts, the first is shown.
fonts_notice = Search sets the specimen text · font collections show their first face

# An error in place of a font whose glyphs claim to be larger than any font's. ems: how many times the font's size they may span (8).
fonts_outline_bounds = Font declares outline bounds beyond { $ems ->
       *[other] { $ems } ems
    }

# What a font's glyphs are drawn with, in the preview's Properties: pictures, no outlines.
fonts_outlines_bitmaps = Bitmaps only

fonts_plugin_description = Show a specimen of font files without installing them

fonts_plugin_title = Fonts

# The specimen text a font preview draws in the font, at three sizes. A pangram of your language (a sentence holding every letter it uses) is best, then the ten digits; keep it under 200 characters.
fonts_sample = The quick brown fox jumps over the lazy dog. 0123456789

# The title of a font's preview. name: the font's full name, as the font gives it.
fonts_title = Font · { $name }

# Why a web font is not previewed: unpacked, it is larger than the preview reads. size: that bound (16 MB).
fonts_too_large = Font exceeds { $size } preview limit

# An error in place of a font whose scale is out of the OpenType range. units: how many units the font divides its em square into.
fonts_units_per_em = Font declares { $units ->
        [one] { $units } unit
       *[other] { $units } units
    } per em (expected 16–16384)

# Stands for a font's name in the preview's title when the font gives none.
fonts_unnamed = Font specimen

# An error in place of a font file its reader does not understand.
fonts_unreadable = This font cannot be read

# The Variable row's value in the preview's Properties: the font is a variable font.
fonts_variable_yes = Yes

# An error in place of a WOFF or WOFF2 font that could not be unpacked.
fonts_web_font_damaged = The web font is damaged or uses an unknown feature
