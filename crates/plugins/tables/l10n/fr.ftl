# French catalog of skwir-plugin-tables, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
tables_blob = blob de { $size }

# draft
tables_cells_cut = { $count ->
        [one] { $count } cellule de plus de { $characters } caractères raccourcie
       *[other] { $count } cellules de plus de { $characters } caractères raccourcies
    }

# draft
tables_columns_cut = { $count ->
       *[other] Seules les { $count } premières colonnes sont affichées
    }

# draft
tables_database_empty = La base de données n’a aucune table

# draft
tables_detail_columns = Colonnes

# draft
tables_detail_rows = Lignes

# draft
tables_detail_separator = Séparateur

# draft
tables_detail_sheet = Feuille

# draft
tables_detail_sheets = Feuilles

# draft
tables_detail_table = Table

# draft
tables_detail_tables = Tables

# draft
tables_keys_cut = { $count ->
       *[other] Seules les { $count } premières clés sont affichées
    }

# draft
tables_line_too_long = Une ligne dépasse la limite d’aperçu de { $size }

# draft
tables_lines_invalid = { $count ->
        [one] { $count } ligne qui n’est pas du JSON
       *[other] { $count } lignes qui ne sont pas du JSON
    }

# draft
tables_plugin_description = Afficher les fichiers CSV, TSV et JSON Lines, les classeurs et les bases de données SQLite en tableaux alignés

# draft
tables_plugin_title = Tableaux

# draft
tables_row_too_long = une ligne dépasse { $size }

# draft
tables_rows_cut = { $count ->
        [one] { $count } ligne coupée à { $columns } colonnes
       *[other] { $count } lignes coupées à { $columns } colonnes
    }

# draft
tables_rows_more = { $count ->
       *[other] plus de { $count }
    }

# draft
tables_separator_comma = Virgule

# draft
tables_separator_pipe = Barre verticale

# draft
tables_separator_semicolon = Point-virgule

# draft
tables_separator_space = Espace

# draft
tables_separator_tab = Tabulation

# draft
tables_spreadsheet_empty = Le classeur n’a aucune feuille de calcul

# draft
tables_spreadsheet_members = { $count ->
       *[other] Le classeur contient plus de { $count } parties
    }

# draft
tables_spreadsheet_too_large = Le classeur dépasse { $size } une fois décompressé

# draft
tables_spreadsheets = Classeurs

# draft
tables_spreadsheets_description = Les classeurs Excel (.xlsx, .xlsm, .xlsb) montrent leurs cellules en tableau, une feuille après l’autre.

# draft
tables_spreadsheets_description_more = Pages les montre tels que LibreOffice les imprime, s’il est installé.

# draft
tables_spreadsheets_pages = Pages

# draft
tables_spreadsheets_table = Tableau

# draft
tables_title_database_rows = { $table } · lignes { $first }–{ $last }

# draft
tables_title_json_lines = JSON Lines

# draft
tables_title_json_lines_rows = JSON Lines · lignes { $first }–{ $last }

# draft
tables_title_sheet_rows = { $sheet } · lignes { $first }–{ $last }

# draft
tables_title_table = Tableau

# draft
tables_title_table_rows = Tableau · lignes { $first }–{ $last }
