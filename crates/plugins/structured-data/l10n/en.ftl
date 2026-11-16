# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# Why a file is not shown formatted: the text formatted is larger than size.
structured_data_formatted_too_large = Formatted preview exceeds { $size }

# Why a file is not shown as structured data: a line of it is longer than count bytes.
structured_data_line_too_long = { $count ->
       *[other] Code preview exceeds { $count } bytes per line
    }

structured_data_plugin_description = Show JSON (with comments, and JSON5), YAML and TOML with syntax colors and foldable blocks

structured_data_plugin_title = Structured data

# The title of a JSON file's preview printed again, indented: format is the format's name (JSON).
structured_data_title_formatted = { $format } · formatted

# Why a file is not shown as structured data: its values nest deeper than count levels (an object in an array in an object…).
structured_data_too_deep = { $count ->
       *[other] Code preview exceeds { $count } levels
    }

# Why a file is not shown as structured data: it has more lines than count.
structured_data_too_many_lines = { $count ->
       *[other] Code preview exceeds { $count } lines
    }

# Why a JSON file is not shown formatted: it holds more values (objects, arrays, numbers, strings…) than count.
structured_data_too_many_nodes = { $count ->
       *[other] Code preview exceeds { $count } nodes
    }

# Why a JSON file could not be shown formatted (indented, a key a line).
structured_data_unprinted = The JSON could not be printed again
