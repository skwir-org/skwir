# Italian catalog of skwir-plugin-tables, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
tables_blob = blob di { $size }

# draft
tables_cells_cut = { $count ->
        [one] { $count } cella
       *[other] { $count } celle
    } più lunghe di { $characters ->
       *[other] { $characters } caratteri
    } abbreviate

# draft
tables_columns_cut = { $count ->
       *[other] Sono mostrate solo le prime { $count } colonne
    }

# draft
tables_database_empty = Il database non ha tabelle

# draft
tables_detail_columns = Colonne

# draft
tables_detail_rows = Righe

# draft
tables_detail_separator = Separatore

# draft
tables_detail_sheet = Foglio

# draft
tables_detail_sheets = Fogli

# draft
tables_detail_table = Tabella

# draft
tables_detail_tables = Tabelle

# draft
tables_keys_cut = { $count ->
       *[other] Sono mostrate solo le prime { $count } chiavi
    }

# draft
tables_line_too_long = Una riga supera il limite di anteprima di { $size }

# draft
tables_lines_invalid = { $count ->
        [one] { $count } riga non JSON
       *[other] { $count } righe non JSON
    }

# draft
tables_plugin_description = Mostra file CSV, TSV e JSON Lines, fogli di calcolo e database SQLite come tabelle allineate

# draft
tables_plugin_title = Tabelle

# draft
tables_row_too_long = una riga è più lunga di { $size }

# draft
tables_rows_cut = { $count ->
        [one] { $count } riga
       *[other] { $count } righe
    } ridotte a { $columns ->
       *[other] { $columns } colonne
    }

# draft
tables_rows_more = { $count ->
       *[other] più di { $count }
    }

# draft
tables_separator_comma = Virgola

# draft
tables_separator_pipe = Barra verticale

# draft
tables_separator_semicolon = Punto e virgola

# draft
tables_separator_space = Spazio

# draft
tables_separator_tab = Tabulazione

# draft
tables_spreadsheet_empty = Il foglio di calcolo non ha fogli di celle

# draft
tables_spreadsheet_members = { $count ->
       *[other] Il foglio di calcolo contiene più di { $count } parti
    }

# draft
tables_spreadsheet_too_large = Il foglio di calcolo supera { $size } una volta decompresso

# draft
tables_spreadsheets = Fogli di calcolo

# draft
tables_spreadsheets_description = Le cartelle di lavoro Excel (.xlsx, .xlsm, .xlsb) mostrano le loro celle come tabella, un foglio dopo l’altro.

# draft
tables_spreadsheets_description_more = Pagine le mostra come le stampa LibreOffice, quando è installato.

# draft
tables_spreadsheets_pages = Pagine

# draft
tables_spreadsheets_table = Tabella

# draft
tables_title_database_rows = { $table } · righe { $first }–{ $last }

# draft
tables_title_json_lines = JSON Lines

# draft
tables_title_json_lines_rows = JSON Lines · righe { $first }–{ $last }

# draft
tables_title_sheet_rows = { $sheet } · righe { $first }–{ $last }

# draft
tables_title_table = Tabella

# draft
tables_title_table_rows = Tabella · righe { $first }–{ $last }
