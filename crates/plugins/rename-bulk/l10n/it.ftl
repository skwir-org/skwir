# Italian catalog of skwir-plugin-rename-bulk, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
rename_bulk_bad_date_format = formato di data non valido `{ $format }`: { $why }

# draft
rename_bulk_bad_width = larghezza del contatore non valida `{ $width }`

# draft
rename_bulk_brace = Una graffa stessa ({ $open } e { $close })

# draft
rename_bulk_cancelled = annullato

# draft
rename_bulk_command = Rinomina più elementi da un modello

# draft
rename_bulk_command_description = Rinomina la selezione, o le voci indicate, da un modello, un’espressione regolare e una regola per le maiuscole (piano)

# draft
rename_bulk_conflict = { $item } diventerebbe { $name }, che esiste già

# draft
rename_bulk_counter = Contatore

# draft
rename_bulk_counter_overflows = il contatore va in overflow

# draft
rename_bulk_counter_too_wide = { $width ->
        [one] un contatore di { $width } cifra è più largo di quanto possa essere un nome ({ $most } al massimo)
       *[other] un contatore di { $width } cifre è più largo di quanto possa essere un nome ({ $most } al massimo)
    }

# draft
rename_bulk_counts = Conta da Inizio, uno in più per ciascuno, nell’ordine mostrato

# draft
rename_bulk_date_note = Dentro { $placeholder }: %Y l’anno, %m il mese, %d il giorno, %H l’ora, %M i minuti, %S i secondi, %B il nome del mese (%b abbreviato), %A quello del giorno, %j il giorno dell’anno. Nel fuso orario di questo sistema.

# draft
rename_bulk_day = Il suo giorno

# draft
rename_bulk_day_and_time = Il suo giorno e la sua ora

# draft
rename_bulk_day_first = Il suo giorno, prima il giorno

# draft
rename_bulk_day_without_dashes = Il suo giorno, senza trattini

# draft
rename_bulk_dollar = Un simbolo del dollaro

# draft
rename_bulk_duplicate = { $count ->
        [2] più elementi diventerebbero { $name }: { $first }, { $second }
        [3] più elementi diventerebbero { $name }: { $first }, { $second }, { $third }
       *[other] più elementi diventerebbero { $name }: { $first }, { $second }, { $third } e altri { $more }
    }

# draft
rename_bulk_empty_replacement_note = Se lasciato vuoto, ciò che il modello ha trovato viene rimosso.

# draft
rename_bulk_extension = L’estensione, senza il punto

# draft
rename_bulk_extension_with_dot = L’estensione con il punto; niente se non c’è

# draft
rename_bulk_first_group = Ciò che conteneva il primo gruppo ($2, $3…)

# draft
rename_bulk_group_before_letter = Lo stesso, prima di una lettera, una cifra o _

# draft
rename_bulk_group_name_note = { $written } viene letto come un gruppo chiamato { $read }, che non contiene nulla: scrivi { $meant }.

# draft
rename_bulk_invalid_name = { $item } diventerebbe un nome non valido { $name }: { $reason }

# draft
rename_bulk_invalid_regex = espressione regolare non valida: { $why }

# draft
rename_bulk_invalid_template = modello non valido: { $why }

# draft
rename_bulk_menu = Rinomina più elementi…

# draft
rename_bulk_missing_date = { $item } non ha ancora un’ora di modifica (necessaria per { $placeholder })

# draft
rename_bulk_month_and_year = Il suo mese e anno

# draft
rename_bulk_name_without_extension = Il nome senza l’estensione

# draft
rename_bulk_named_group = Ciò che conteneva il gruppo chiamato year

# draft
rename_bulk_no_location = il riquadro non ha una posizione aperta

# draft
rename_bulk_no_such_pane = nessun riquadro di questo tipo

# draft
rename_bulk_not_read_yet = segnaposto `{ $placeholder }` non ancora letto

# draft
rename_bulk_not_text = { $item } non è un nome fatto di testo; rinominalo da solo

# draft
rename_bulk_nothing = niente da rinominare

# draft
rename_bulk_parent = La cartella che lo contiene

# draft
rename_bulk_parts_note = L’espressione regolare viene eseguita per prima: queste sono le parti del nome che lascia. Le maiuscole vengono per ultime e lasciano l’estensione com’è. Una cartella non ha estensione.

# draft
rename_bulk_parts_of_the_name = Parti del nome

# draft
rename_bulk_plan = { $count ->
        [one] rinomina { $count } elemento
       *[other] rinomina { $count } elementi
    }

# draft
rename_bulk_plugin_description = Rinomina la selezione da un modello, un’espressione regolare e regole per le maiuscole

# draft
rename_bulk_plugin_title = Rinomina multipla

# draft
rename_bulk_preview = Anteprima della rinomina multipla

# draft
rename_bulk_preview_description = Restituisce i nomi vecchio → nuovo che una rinomina multipla produrrebbe (`renames`, i primi `limit` quando indicato; ciascuno con dove l’espressione regolare ha trovato corrispondenza (`matched`) nel vecchio nome e cosa ha inserito (`put`) nel nuovo, intervalli di byte), quanti sono (`count`) e il nome del primo elemento (`first`), sul quale agiscono da soli i riferimenti dell’espressione regolare e della sostituzione

# draft
rename_bulk_recipe_brackets = Rimuovi ciò che è tra parentesi

# draft
rename_bulk_recipe_jpeg = Un’unica estensione per le immagini JPEG → { $shown }

# draft
rename_bulk_recipe_numbers = Rimuovi i numeri all’inizio

# draft
rename_bulk_recipe_one_space = Più spazi in uno → uno spazio

# draft
rename_bulk_recipe_spaces = Spazi in trattini bassi → { $shown }

# draft
rename_bulk_recipe_start = Rimuovi un inizio (IMG_)

# draft
rename_bulk_recipe_swap = Scambia due parti attorno a “ - ” → { $shown }

# draft
rename_bulk_recipe_underscores = Trattini bassi in spazi → uno spazio

# draft
rename_bulk_recipes = Ricette

# draft
rename_bulk_reference = Spiega cosa accetta un campo della rinomina multipla

# draft
rename_bulk_reference_description = Restituisce cosa accetta `field` (template, regex, replacement): i suoi segnaposto o simboli, ciascuno con ciò che dà per la prima delle voci indicate o selezionate, con gli altri argomenti della rinomina come indicati, e ricette che compilano l’espressione regolare e la sua sostituzione

# draft
rename_bulk_regex = Espressione regolare

# draft
rename_bulk_regex_groups_note = Ciò che conteneva un gruppo è $1, $2… nella sostituzione; ciò che conteneva il gruppo chiamato year, { $named }.

# draft
rename_bulk_regex_note = ^ e $ sono l’inizio e la fine del nome. Viene eseguita sull’intero nome, estensione inclusa, sostituendo ogni corrispondenza; il modello viene dopo.

# draft
rename_bulk_replacement = Sostituzione

# draft
rename_bulk_signs = Simboli

# draft
rename_bulk_template = Modello

# draft
rename_bulk_three_digits = Lo stesso, almeno tre cifre

# draft
rename_bulk_two_digits = Lo stesso, almeno due cifre

# draft
rename_bulk_unclosed = `{ $brace }` non chiusa in `{ $template }`

# draft
rename_bulk_unknown_placeholder = segnaposto sconosciuto `{ $placeholder }`

# draft
rename_bulk_what_a_match_becomes = In cosa diventa una corrispondenza

# draft
rename_bulk_when_modified = Quando è stato modificato

# draft
rename_bulk_whole_match = L’intera corrispondenza

# draft
rename_bulk_whole_name = L’intero nome
