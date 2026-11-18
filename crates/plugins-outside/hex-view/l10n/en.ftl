# The hexadecimal view's English (protocol 2, ADR-139, 14), written by its
# author: what the built-in hexadecimal preview says (`skwir-plugin-hex`),
# the same in every language it is translated in, from a module.

# The plugin's name, on Settings › Plugins.
plugin-title = Hexadecimal (WebAssembly)

# What it does, under its name on Settings › Plugins.
plugin-description = Inspect any file as hexadecimal bytes, from a WebAssembly module

# The title of a file's preview as hexadecimal bytes, with the file's size.
title = Hex · { $size }

# A column's header in the hexadecimal view: where each row of bytes is in the file.
offset = Offset

# A note under a file's bytes: where the search found what it looked for, as a hexadecimal offset (0x64).
match-at = Match at { $offset }

# A note under a file's bytes: the search found nothing past where it started.
no-further-match = No further match

# An error of a search among a file's bytes written as hex: (hex: 6e 65) needs two digits a byte.
pairs = Use pairs of hexadecimal digits

# An error of a search among a file's bytes: what it looks for is empty, or longer than it may be.
needle-length = { $max ->
       *[other] Search must contain 1–{ $max } bytes
    }

# An error in place of a file's bytes: Skwir did not say how long the file is.
size-unknown = The file's size is not known
