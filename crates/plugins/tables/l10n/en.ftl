# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A cell of a SQLite table holding binary data (a blob), shown by its size.
tables_blob = { $size } blob

# A note under a table: cells whose text is cut. characters: the bound (1,024).
tables_cells_cut = { $count ->
        [one] { $count } cell
       *[other] { $count } cells
    } longer than { $characters ->
       *[other] { $characters } characters
    } shortened

# A note under a table that has more columns than are shown.
tables_columns_cut = { $count ->
       *[other] Only the first { $count } columns shown
    }

# Why a SQLite database could not be shown as a table.
tables_database_empty = The database has no table

# A label in the preview's Properties: how many columns the table has.
tables_detail_columns = Columns

# A label in the preview's Properties: how many rows the table has.
tables_detail_rows = Rows

# A label in the preview's Properties: what splits a CSV's cells (Comma, Semicolon, Tab…).
tables_detail_separator = Separator

# A label in the preview's Properties: the name of the spreadsheet's sheet shown.
tables_detail_sheet = Sheet

# A label in the preview's Properties: how many sheets the spreadsheet has.
tables_detail_sheets = Sheets

# A label in the preview's Properties: the name of the database's table shown.
tables_detail_table = Table

# A label in the preview's Properties: how many tables the database has.
tables_detail_tables = Tables

# A note under a JSON Lines table whose objects have more keys (its columns) than are shown.
tables_keys_cut = { $count ->
       *[other] Only the first { $count } keys shown
    }

# Why a JSON Lines file could not be shown. size: the bound on a line (2 MB).
tables_line_too_long = A line exceeds { $size } preview limit

# A note under a JSON Lines table: lines that are not JSON, shown as they are.
tables_lines_invalid = { $count ->
        [one] { $count } line not JSON
       *[other] { $count } lines not JSON
    }

tables_plugin_description = Show CSV, TSV and JSON Lines files, spreadsheets and SQLite databases as aligned tables

tables_plugin_title = Tables

# Why a table could not be shown: a row (a quote never closed, a line never ended) runs past the bound. size: that bound (2 MB).
tables_row_too_long = a row is longer than { $size }

# A note under a table: rows with more cells than are shown. columns: how many are shown (64).
tables_rows_cut = { $count ->
        [one] { $count } row
       *[other] { $count } rows
    } cut to { $columns ->
       *[other] { $columns } columns
    }

# The value beside Rows in the preview's Properties, for a table read only as far as the page shown: more than that many rows.
tables_rows_more = { $count ->
       *[other] more than { $count }
    }

# What splits a CSV's cells, beside Separator in the preview's Properties.
tables_separator_comma = Comma

# What splits a CSV's cells, beside Separator in the preview's Properties: the character |.
tables_separator_pipe = Pipe

# What splits a CSV's cells, beside Separator in the preview's Properties.
tables_separator_semicolon = Semicolon

# What splits a CSV's cells, beside Separator in the preview's Properties.
tables_separator_space = Space

# What splits a CSV's cells, beside Separator in the preview's Properties: the tab character.
tables_separator_tab = Tab

# Why a spreadsheet could not be shown as a table: it has no worksheet (only charts, say).
tables_spreadsheet_empty = The spreadsheet has no sheet of cells

# Why a spreadsheet could not be shown: its parts, once unpacked, are larger than size (128 MB).
tables_spreadsheet_too_large = Spreadsheet exceeds { $size } unpacked

# A setting's title: how spreadsheets are shown in the preview.
tables_spreadsheets = Spreadsheets

# Table and Pages are the setting's two choices.
tables_spreadsheets_description = Excel workbooks (.xlsx, .xlsm, .xlsb) show their cells as a table, a sheet after another. Pages shows them as LibreOffice prints them, when it is installed.

# A choice of the Spreadsheets setting: a workbook shown as LibreOffice prints it, page by page.
tables_spreadsheets_pages = Pages

# A choice of the Spreadsheets setting: a workbook's cells shown as a table.
tables_spreadsheets_table = Table

# The title of a SQLite database's preview: the table's name as the database names it, then the rows shown, by number, from first to last.
tables_title_database_rows = { $table } · rows { $first }–{ $last }

# The title of a JSON Lines file's preview: the format's name.
tables_title_json_lines = JSON Lines

# The title of a JSON Lines file's preview: the rows shown, by number, from first to last.
tables_title_json_lines_rows = JSON Lines · rows { $first }–{ $last }

# The title of a spreadsheet's preview: the sheet's name as the file names it, then the rows shown, by number, from first to last.
tables_title_sheet_rows = { $sheet } · rows { $first }–{ $last }

# The title of a CSV or TSV file's preview, shown as a table.
tables_title_table = Table

# The title of a CSV or TSV file's preview: the rows shown, by number, from first to last.
tables_title_table_rows = Table · rows { $first }–{ $last }
