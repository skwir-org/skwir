# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A note under a file's bytes: where the search found what it looked for, as a hexadecimal offset (0x64).
hex_match_at = Match at { $offset }

# An error of a search among a file's bytes: what it looks for is empty, or longer than it may be.
hex_needle_length = { $max ->
       *[other] Search must contain 1–{ $max } bytes
    }

# A note under a file's bytes: the search found nothing past where it started.
hex_no_further_match = No further match

# An error of a search among a file's bytes written as hex: (hex: 6e 65) needs two digits a byte.
hex_pairs = Use pairs of hexadecimal digits

hex_plugin_description = Inspect any file as hexadecimal bytes

hex_plugin_title = Hexadecimal

# The title of a file's preview as hexadecimal bytes, with the file's size.
hex_title = Hex · { $size }
