# Italian catalog of skwir-plugin-query, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
query_accept_filter = Filtra

# draft
query_accept_search = Cerca

# draft
query_another_tag = Altra etichetta…

# draft
query_app_ended = l’app è terminata

# draft
query_ask_tag_placeholder = Un’etichetta

# draft
query_bad_glob = glob `{ $glob }` non valido per { $key }: { $error }

# draft
query_bad_regex = espressione regolare `{ $regex }` non valida: { $error }

# draft
query_bad_size = dimensione `{ $value }` non valida

# draft
query_bad_time = data o durata `{ $value }` non valida (prova 2026-05-01, this-week o 7d)

# draft
query_below = Questa cartella e le sue sottocartelle

# draft
query_below_description = Cerca nella cartella del riquadro e nelle sue sottocartelle ciò che la filtra: la sua lente e le parole digitate sopra di essa

# draft
query_chip_accessed = Ultimo accesso

# draft
query_chip_contains = Contiene

# draft
query_chip_created = Creato

# draft
query_chip_in = Dentro { FORM($place, case: "definite") }

# draft
query_chip_kind = Genere

# draft
query_chip_kinds = { $kinds }

# draft
query_chip_modified = Modificato

# draft
query_chip_name = Nome

# draft
query_chip_not_tag = Senza etichetta

# draft
query_chip_size = Dimensione

# draft
query_chip_subfolders = E sottocartelle

# draft
query_chip_tag = Etichetta

# draft
query_chip_taken = Scattato

# draft
query_chip_when = { $when }

# draft
query_chip_when_days = { $first } → { $last }

# draft
query_complete = Completa una ricerca

# draft
query_complete_description = { $shape }: ciò con cui può proseguire il termine che termina a `caret` (byte; la fine se omesso), con il `text` di ogni elemento scritto al posto di `range`: le chiavi che una parola inizia, i valori di una chiave (generi, le etichette dei file del riquadro e poi le altre dell’utente, al massimo 200, con `more` che conta quelle escluse, periodi, dimensioni, stelle); `typing` quando quel termine è ancora incompleto

# draft
query_contains_placeholder = Testo contenuto nei file

# draft
query_custom = Personalizzato…

# draft
query_depth_below = Questa cartella e le sue sottocartelle

# draft
query_depth_chip = Solo questa cartella

# draft
query_depth_here = Solo questa cartella

# draft
query_describe = Descrivi ricerca

# draft
query_describe_description = I chip della ricerca, come li disegna la barra degli indirizzi per un riquadro che mostra una ricerca: ogni sfaccettatura (parole, genere, modificato, scattato, dimensione, etichetta, contenuto, dove; creato e ultimo accesso una volta impostati) con il suo valore e l’invocazione che esegue ciascuna delle sue scelte, poi le parti che nessuna sfaccettatura contiene

# draft
query_describe_filter = Descrivi filtro

# draft
query_describe_filter_description = { $shape }: ciò che filtra il riquadro (la sua lente e le parole digitate sopra di essa, ADR-107) come chip di una ricerca (`query.describe`), con ogni scelta che lo modifica (`query.filter`): quanto in profondità cerca (quella di una cartella: solo questa cartella, oppure le sue sottocartelle cercate, `query.below`), poi ogni sfaccettatura, impostata e non

# draft
query_edit = Modifica ricerca salvata

# draft
query_edit_accept = Salva

# draft
query_edit_description = Cambia ciò che cerca una ricerca salvata, o ciò che filtra una posizione salvata; il suo nome e il suo posto nella barra laterale restano invariati

# draft
query_edit_filter_menu = Modifica filtro…

# draft
query_edit_filter_title = Modifica filtro

# draft
query_edit_search_menu = Modifica ricerca…

# draft
query_edit_search_placeholder = { $example }  ({ $key } sceglie la radice)

# draft
query_edit_search_title = Modifica ricerca salvata

# draft
query_empty_regex = { $key } espressione regolare vuota

# draft
query_error = ricerca: { $error }

# draft
query_everything = Tutto

# draft
query_expected_bool = previsto { $yes } o { $no }, ricevuto `{ $value }`

# draft
query_expected_comparison = previsto un confronto dopo `{ $text }`

# draft
query_expected_term = previsto un termine

# draft
query_expects_distance = { $key } richiede una distanza come 5km o 500m, ricevuto `{ $value }`

# draft
query_expects_edges = { $key } richiede i bordi sud, ovest, nord ed est, come 9.9,19.9,10.2,20.3, ricevuto `{ $value }`

# draft
query_expects_either = { $key } richiede { $one } o { $other }, ricevuto `{ $value }`

# draft
query_expects_extension = { $key } richiede almeno un’estensione

# draft
query_expects_folder = { $key } richiede una cartella

# draft
query_expects_kind = { $key } richiede { $kinds }, ricevuto `{ $value }`

# draft
query_expects_kinds = { $key } richiede { $kinds }

# draft
query_expects_number = { $key } richiede un numero, ricevuto `{ $value }`

# draft
query_expects_point = { $key } richiede una latitudine e una longitudine, come 45.1,6.2, ricevuto `{ $value }`

# draft
query_expects_stars = { $key } richiede stelle da 0 a 5 (mezze stelle ammesse), ricevuto `{ $value }`

# draft
query_expects_type = { $key } richiede { $file }, { $dir } o { $link }, ricevuto `{ $value }`

# draft
query_expects_value = { $key } richiede un valore

# draft
query_field_among = tra i file { GENDER($place) ->
        [masculine] { FORM($place, case: "di") }
        [feminine] { FORM($place, case: "di") }
       *[other] di { $place }
    }

# draft
query_field_close_quote = Chiudi le virgolette per cercare

# draft
query_field_home = ovunque nella Cartella personale

# draft
query_field_in_folders = { GENDER($place) ->
        [masculine] { FORM($place, case: "in") }
        [feminine] { FORM($place, case: "in") }
       *[other] in { $place }
    } e nelle sue cartelle

# draft
query_field_more_tags = { $count ->
        [one] { $count } altra etichetta
       *[other] { $count } altre etichette
    }

# draft
query_field_narrow_tags = Digita altre lettere del nome per restringerle

# draft
query_field_recent_in = { GENDER($place) ->
        [masculine] { FORM($place, case: "in") }
        [feminine] { FORM($place, case: "in") }
       *[other] in { $place }
    }

# draft
query_field_saved_in = { $filter } { GENDER($place) ->
        [masculine] { FORM($place, case: "in") }
        [feminine] { FORM($place, case: "in") }
       *[other] in { $place }
    }

# draft
query_field_search = Cerca { $search }

# draft
query_field_search_contents = Contenuto

# draft
query_field_search_disk = Tutto il disco

# draft
query_field_search_for = Cerca { $words }

# draft
query_field_search_names = Nomi

# draft
query_field_type_value = Digita un valore per cercare

# draft
query_field_whole_disk = su tutto il disco

# draft
query_field_within = tra tutti i file { GENDER($place) ->
        [masculine] { FORM($place, case: "in") }
        [feminine] { FORM($place, case: "in") }
       *[other] in { $place }
    }

# draft
query_filter = Filtra cartella

# draft
query_filter_description = Cambia una sfaccettatura di ciò che filtra il riquadro (le parole digitate sopra di esso nella barra degli indirizzi, la sua lente), come `query.refine` cambia quella di una ricerca; la posizione resta visualizzata

# draft
query_filter_tags = Filtra etichette

# draft
query_filter_taken = questo filtro è già salvato come { $name }

# draft
query_find_similar_submenu = Trova simili

# draft
query_forget = Rimuovi ricerca salvata

# draft
query_forget_description = Rimuove una ricerca salvata dalla barra laterale, indicata dalla sua ricerca (una posizione salvata dal suo filtro e da `at`) o dal suo nome

# draft
query_forget_recent = Dimentica una ricerca recente

# draft
query_forget_recent_description = Rimuove una ricerca da quelle che la barra degli indirizzi propone come eseguite di recente

# draft
query_group_100_to_1 = da 100 { $unit } a 1 { $next }

# draft
query_group_12_to_24_mp = da 12 a 24 MP

# draft
query_group_1_to_100 = da 1 a 100 { $unit }

# draft
query_group_1_to_4_mp = da 1 a 4 MP

# draft
query_group_1_to_5_minutes = da 1 a 5 minuti

# draft
query_group_20_minutes_to_1_hour = da 20 minuti a 1 ora

# draft
query_group_4_to_12_mp = da 4 a 12 MP

# draft
query_group_5_to_20_minutes = da 5 a 20 minuti

# draft
query_group_digits = 0–9

# draft
query_group_earlier_this_month = Prima, questo mese

# draft
query_group_earlier_this_week = Prima, questa settimana

# draft
query_group_earlier_this_year = Prima, quest’anno

# draft
query_group_empty = Vuoti

# draft
query_group_folders = Cartelle

# draft
query_group_future = Nel futuro

# draft
query_group_last_month = Mese scorso

# draft
query_group_last_week = Settimana scorsa

# draft
query_group_no_date = Nessuna data

# draft
query_group_no_dimensions = Nessuna dimensione

# draft
query_group_no_duration = Nessuna durata

# draft
query_group_no_extension = Nessuna estensione

# draft
query_group_no_folder = Nessuna cartella

# draft
query_group_no_owner = Nessun proprietario

# draft
query_group_no_tags = Nessuna etichetta

# draft
query_group_other_kind = Altro

# draft
query_group_other_letters = Altre lettere

# draft
query_group_other_names = Altri

# draft
query_group_over_1 = Oltre 1 { $unit }

# draft
query_group_over_1_hour = Oltre 1 ora

# draft
query_group_over_24_mp = Oltre 24 MP

# draft
query_group_state_unknown = Sconosciuto

# draft
query_group_strokes = { $strokes ->
        [one] { $strokes } tratto
       *[other] { $strokes } tratti
    }

# draft
query_group_today = Oggi

# draft
query_group_unchanged = Invariati

# draft
query_group_under_100 = Meno di 100 { $unit }

# draft
query_group_under_1_minute = Meno di 1 minuto

# draft
query_group_under_1_mp = Meno di 1 MP

# draft
query_group_unknown_type = Tipo sconosciuto

# draft
query_group_unrated = Non valutati

# draft
query_group_yesterday = Ieri

# draft
query_here = Solo questa cartella

# draft
query_here_description = Mostra la ricerca del riquadro come la cartella in cui cerca, filtrata dalle sue parole, quando la cartella da sola può esprimerle; altrimenti cerca solo in quella cartella. Una cartella resta com’è

# draft
query_home = Cartella personale

# draft
query_interpret = Interpreta le parole come ricerca

# draft
query_interpret_description = { $shape } o null: parole semplici lette come termini secondo regole (periodi e anni, generi, estensioni, dimensioni, le etichette del riquadro), il resto mantenuto come nomi; mai applicata da sola

# draft
query_keep_place = Salva con questo filtro

# draft
query_keep_search = Salva questa ricerca

# draft
query_key_accessed = Ultimo accesso

# draft
query_key_case = Maiuscole distinte o no

# draft
query_key_comment = Con un commento contenente

# draft
query_key_comment_example = parole nel commento di un file

# draft
query_key_content = Contenente il testo

# draft
query_key_content_example = "parole" o /regex/

# draft
query_key_created = Creati

# draft
query_key_depth = Quanto in profondità cercare

# draft
query_key_depth_example = quante cartelle in basso cerca: 2

# draft
query_key_excluded = Anche le cartelle escluse

# draft
query_key_excluded_example = anche ciò che le tue esclusioni lasciano fuori: { $yes }, { $no }

# draft
query_key_ext = Con un’estensione

# draft
query_key_hidden = Anche i file nascosti, o mai

# draft
query_key_in = Dove cercare

# draft
query_key_in_example = una cartella, "~/My Files"

# draft
query_key_inside = Scattati dentro un riquadro

# draft
query_key_inside_example = south,west,north,east

# draft
query_key_kind = Di un genere

# draft
query_key_mime = Di un tipo

# draft
query_key_modified = Modificati

# draft
query_key_name = Con nome

# draft
query_key_near = Scattati vicino a un punto

# draft
query_key_rating = Valutati

# draft
query_key_size = Di una dimensione

# draft
query_key_tag = Con un’etichetta

# draft
query_key_tag_example = un’etichetta, o * per qualsiasi; anche #tag

# draft
query_key_taken = Scattati, immagini e video

# draft
query_key_taken_example = 2023, last-year, secondo l’orologio della fotocamera

# draft
query_key_type = File, cartelle o collegamenti

# draft
query_list = Elenca ricerche salvate

# draft
query_list_description = Restituisce le ricerche salvate, nell’ordine della barra laterale

# draft
query_location = Posizione di ricerca

# draft
query_location_description = { $shape }: la ricerca per il valore di una sfaccettatura (`tag` `work`, o solo `tag` per qualsiasi etichetta) a partire da `root` o dalla cartella personale, per chi mostra o esegue una ricerca senza scriverne la sintassi

# draft
query_matches_nothing = { $term } non corrisponde a nulla

# draft
query_missing_paren = manca `)`

# draft
query_move = Sposta ricerca salvata

# draft
query_move_description = Mette una ricerca salvata prima di un’altra nella barra laterale, o per ultima

# draft
query_name_change = Cambia…

# draft
query_name_contains = Il nome contiene

# draft
query_name_contains_menu = Il nome contiene…

# draft
query_name_placeholder = Parole o pattern: invoice *.pdf

# draft
query_named_in = { $what } { GENDER($place) ->
        [masculine] { FORM($place, case: "in") }
        [feminine] { FORM($place, case: "in") }
       *[other] in { $place }
    }

# draft
query_narrow = Cambia la lente

# draft
query_narrow_description = Cambia una sfaccettatura della lente del riquadro (ADR-107: ciò del suo filtro o della sua ricerca che l’elenco può decidere, i suoi file di un genere, di un anno, con o senza un’etichetta), mantenuta come il riquadro mostra altre posizioni finché non viene tolta: quella di una ricerca al suo posto, il filtro di ogni altra cosa; senza `facet`, toglie la lente

# draft
query_needs_value = `{ $facet }` richiede un valore

# draft
query_no_facet = nessuna sfaccettatura `{ $facet }`

# draft
query_no_kind = nessun genere `{ $kind }`

# draft
query_no_pane = nessun riquadro del genere

# draft
query_no_pane_numbered = nessun riquadro { $pane }

# draft
query_no_part = nessuna parte `{ $part }`

# draft
query_no_saved_named = nessuna ricerca salvata di nome { $name }

# draft
query_no_search_no_filter = il riquadro non mostra né una ricerca né un filtro

# draft
query_no_search_no_folder = il riquadro non mostra né una ricerca né una cartella

# draft
query_no_tags_among = Nessuna etichetta tra questi file

# draft
query_not_a_lens = una lente è ciò che si sa dei file: `{ $facet }` appartiene a una ricerca (ciò che contengono, quando è stata scattata un’immagine, i nomi)

# draft
query_not_a_place = { $key } `{ $value }` non è un luogo

# draft
query_not_allowed = { $key } non è consentito qui

# draft
query_not_on_earth = { $key } una distanza di `{ $value }` non esiste sulla Terra

# draft
query_not_one_condition = `{ $term }` non è una condizione singola

# draft
query_not_tagged_instead = Senza etichetta, invece

# draft
query_not_within_search = una ricerca non può cercare dentro un’altra ricerca

# draft
query_nothing_found = Nessun risultato

# draft
query_nothing_found_detail = Nessun file corrisponde a { $text } qui. Controlla l’ortografia o cerca in una cartella più in alto.

# draft
query_nothing_opens = nessun plugin apre { $place }: una cartella si scrive con il percorso completo

# draft
query_nothing_to_save = il riquadro non mostra nulla da salvare

# draft
query_only_folders_below = il riquadro non mostra alcuna cartella: si possono cercare solo le sottocartelle di una cartella

# draft
query_only_in_bytes = solo { $key } accetta byte che non sono UTF-8 (\xNN)

# draft
query_period_last_month = Mese scorso

# draft
query_period_last_week = Settimana scorsa

# draft
query_period_last_year = Anno scorso

# draft
query_period_older = Più di un anno fa

# draft
query_period_this_month = Questo mese

# draft
query_period_this_week = Questa settimana

# draft
query_period_this_year = Quest’anno

# draft
query_period_today = Oggi

# draft
query_period_yesterday = Ieri

# draft
query_place_changed = Filtro di { $name } modificato

# draft
query_place_moved = { $name } spostato

# draft
query_place_named = { $place } · { $filter }

# draft
query_place_needs_filter = una posizione salvata richiede un filtro

# draft
query_place_not_below = una posizione si cerca come si elenca, mai al di sotto di essa

# draft
query_place_not_saved = Non è salvato

# draft
query_place_removed = { $name } rimosso dai Salvati

# draft
query_place_renamed = { $name } rinominato in { $new }

# draft
query_place_saved = { $name } salvato

# draft
query_place_saved_already = È già salvato come { $name }

# draft
query_plocate_database = il database di plocate

# draft
query_plugin_description = Cerca come posizione: query per nome, dimensione, data, etichetta, MIME e contenuto; ricerche salvate nella barra laterale

# draft
query_plugin_title = Cerca

# draft
query_rating_five = Cinque stelle

# draft
query_rating_none = Non valutati

# draft
query_rating_one = Una stella o più

# draft
query_rating_three = Tre stelle o più

# draft
query_reading = { $subject }{ $named }{ $tagged }{ $year }{ $period }{ $size }

# draft
query_reading_archives = { $at ->
        [first] Archivi
       *[after] archivi
    }

# draft
query_reading_audio = { $at ->
        [first] Musica e audio
       *[after] musica e audio
    }

# draft
query_reading_documents = { $at ->
        [first] Documenti
       *[after] documenti
    }

# draft
query_reading_empty = { $of ->
        [extension] {" "}vuoti
        [folder] {" "}vuote
        [image] {" "}vuote
        [video] {" "}vuoti
        [pictures] {" "}vuoti
        [audio] {" "}vuoti
        [document] {" "}vuoti
        [spreadsheet] {" "}vuoti
        [presentation] {" "}vuote
        [text] {" "}vuoti
        [archive] {" "}vuoti
        [font] {" "}vuoti
        [program] {" "}vuoti
        [several] {" "}vuoti
       *[file] {" "}vuoti
    }

# draft
query_reading_extension = { $ext }

# draft
query_reading_files = File

# draft
query_reading_folders = { $at ->
        [first] Cartelle
       *[after] cartelle
    }

# draft
query_reading_fonts = { $at ->
        [first] Font
       *[after] font
    }

# draft
query_reading_images = { $at ->
        [first] Immagini
       *[after] immagini
    }

# draft
query_reading_modified = { $of ->
        [extension] {" "}modificati { $when }
        [folder] {" "}modificate { $when }
        [image] {" "}modificate { $when }
        [video] {" "}modificati { $when }
        [pictures] {" "}modificati { $when }
        [audio] {" "}modificati { $when }
        [document] {" "}modificati { $when }
        [spreadsheet] {" "}modificati { $when }
        [presentation] {" "}modificate { $when }
        [text] {" "}modificati { $when }
        [archive] {" "}modificati { $when }
        [font] {" "}modificati { $when }
        [program] {" "}modificati { $when }
        [several] {" "}modificati { $when }
       *[file] {" "}modificati { $when }
    }

# draft
query_reading_named = { $of ->
        [extension] {" "}chiamati { $names }
        [folder] {" "}chiamate { $names }
        [image] {" "}chiamate { $names }
        [video] {" "}chiamati { $names }
        [pictures] {" "}chiamati { $names }
        [audio] {" "}chiamati { $names }
        [document] {" "}chiamati { $names }
        [spreadsheet] {" "}chiamati { $names }
        [presentation] {" "}chiamate { $names }
        [text] {" "}chiamati { $names }
        [archive] {" "}chiamati { $names }
        [font] {" "}chiamati { $names }
        [program] {" "}chiamati { $names }
        [several] {" "}chiamati { $names }
       *[file] {" "}chiamati { $names }
    }

# draft
query_reading_over_1 = {" "}di oltre 1 { $unit }

# draft
query_reading_over_100 = {" "}di oltre 100 { $unit }

# draft
query_reading_presentations = { $at ->
        [first] Presentazioni
       *[after] presentazioni
    }

# draft
query_reading_programs = { $at ->
        [first] Programmi
       *[after] programmi
    }

# draft
query_reading_spreadsheets = { $at ->
        [first] Fogli di calcolo
       *[after] fogli di calcolo
    }

# draft
query_reading_tagged = { $of ->
        [extension] {" "}etichettati
        [folder] {" "}etichettate
        [image] {" "}etichettate
        [video] {" "}etichettati
        [pictures] {" "}etichettati
        [audio] {" "}etichettati
        [document] {" "}etichettati
        [spreadsheet] {" "}etichettati
        [presentation] {" "}etichettate
        [text] {" "}etichettati
        [archive] {" "}etichettati
        [font] {" "}etichettati
        [program] {" "}etichettati
        [several] {" "}etichettati
       *[file] {" "}etichettati
    } { $count ->
        [one] { $tags }
       *[other] { $tags }
    }

# draft
query_reading_taken = { $of ->
        [extension] {" "}scattati { $when }
        [folder] {" "}scattate { $when }
        [image] {" "}scattate { $when }
        [video] {" "}girati { $when }
        [pictures] {" "}scattati { $when }
        [audio] {" "}scattati { $when }
        [document] {" "}scattati { $when }
        [spreadsheet] {" "}scattati { $when }
        [presentation] {" "}scattate { $when }
        [text] {" "}scattati { $when }
        [archive] {" "}scattati { $when }
        [font] {" "}scattati { $when }
        [program] {" "}scattati { $when }
        [several] {" "}scattati { $when }
       *[file] {" "}scattati { $when }
    }

# draft
query_reading_text = { $at ->
        [first] Testo e codice
       *[after] testo e codice
    }

# draft
query_reading_under_100 = {" "}di meno di 100 { $unit }

# draft
query_reading_videos = { $at ->
        [first] Video
       *[after] video
    }

# draft
query_reading_when_last_month = il mese scorso

# draft
query_reading_when_last_week = la settimana scorsa

# draft
query_reading_when_last_year = l’anno scorso

# draft
query_reading_when_older = più di un anno fa

# draft
query_reading_when_this_month = questo mese

# draft
query_reading_when_this_week = questa settimana

# draft
query_reading_when_this_year = quest’anno

# draft
query_reading_when_today = oggi

# draft
query_reading_when_year = nel { $year }

# draft
query_reading_when_yesterday = ieri

# draft
query_ref_brackets = […] uno di questi

# draft
query_ref_content = File che contengono queste parole

# draft
query_ref_content_regex = File che contengono ciò che corrisponde a un pattern

# draft
query_ref_contents = Contenuti

# draft
query_ref_date = Un anno, un mese o un giorno: 2026, 2026-05-14

# draft
query_ref_date_range = Dall’inizio del primo alla fine del secondo

# draft
query_ref_duration = Negli ultimi 7 giorni: { $m } minuti, { $h } ore, { $d } giorni, { $w } settimane, { $mo } mesi, { $y } anni

# draft
query_ref_flags = { $i }: maiuscole o minuscole indifferenti; { $c }: maiuscole e minuscole distinte

# draft
query_ref_key = { $means }: { $example }

# draft
query_ref_line = ^ e $ sono l’inizio e la fine di una riga.

# draft
query_ref_minus = Non questo: -draft, -#tag, -kind:image

# draft
query_ref_names = Nomi

# draft
query_ref_not = Non: NOT a, scritto anche -a

# draft
query_ref_older = Più di un anno fa

# draft
query_ref_options = Dove e come cerca

# draft
query_ref_or = Uno o l’altro: a OR b

# draft
query_ref_parens = Insieme: (a OR b) c

# draft
query_ref_period = Un periodo: { $periods }

# draft
query_ref_question = ? un carattere

# draft
query_ref_quotes = Così com’è: "two words", "OR", "#3"

# draft
query_ref_rating = Almeno (> < >= <=); rating:0 trova i file senza valutazione

# draft
query_ref_size = Più di 10 { $mb }. Ciascuna di { $decimal } vale 1000 volte la precedente; ciascuna di { $binary } vale 1024 volte, e così ciascuna di { $alone } scritta da sola.

# draft
query_ref_size_range = Da una dimensione all’altra, entrambe incluse

# draft
query_ref_slash = Una barra vera e propria: una da sola chiude il pattern

# draft
query_ref_star = * un numero qualsiasi di caratteri

# draft
query_ref_terms = Termini

# draft
query_ref_title = Cerca

# draft
query_ref_together = Insieme (affiancati: entrambi)

# draft
query_ref_values = Valori

# draft
query_ref_word = Nomi che la contengono, maiuscole o minuscole

# draft
query_reference = Descrivi cosa accetta una ricerca

# draft
query_reference_description = Cosa accetta il testo di una ricerca (ADR-137): i suoi termini e le sue opzioni con il significato di ciascuno ed esempi, come si combinano i termini, i valori, i caratteri jolly dei nomi e cosa legge `content:`, segni delle regex inclusi; ogni voce così com’è scritta, cosa dice e cosa scrive sceglierla

# draft
query_refine = Affina la ricerca

# draft
query_refine_description = Modifica una faccetta della ricerca del riquadro (o ne avvia una nella sua cartella, con il suo filtro aggiunto) e mostra il risultato: la modifica del solo filtro avviene al suo posto, ogni altra come passaggio che Indietro annulla; tolto l’ultimo termine, il riquadro mostra la posizione cercata

# draft
query_remember_setting = Ricorda le ricerche recenti

# draft
query_remember_setting_description = Il campo indirizzo propone le ricerche eseguite da lì di recente, conservate nella cache; se disattivato, vengono dimenticate.

# draft
query_remove_part = Rimuovi

# draft
query_rename = Rinomina ricerca salvata

# draft
query_rename_accept = Rinomina

# draft
query_rename_description = Cambia il nome che una ricerca salvata mostra nella barra laterale

# draft
query_rename_place_placeholder = Nome (vuoto: ricavato dal filtro)

# draft
query_rename_place_title = Rinomina

# draft
query_rename_search_placeholder = Nome (vuoto: ricavato dalla ricerca)

# draft
query_rename_search_title = Rinomina ricerca salvata

# draft
query_requirement_plocate = Ricerche istantanee per nome su interi dischi (altrimenti si scorre l’albero delle cartelle)

# draft
query_root_not_absolute = la radice della ricerca { $path } non è un percorso assoluto

# draft
query_root_not_folder = la radice della ricerca { $path } non è una cartella: { $error }

# draft
query_root_not_path = `root` deve essere un percorso

# draft
query_save = Salva ricerca

# draft
query_save_description = Conserva una ricerca nei Salvati della barra laterale, oppure con `at` una posizione e ciò che la filtra, mostrata così all’apertura (ADR-109); ciò che mostra il riquadro quando `expr` è omesso

# draft
query_saved_not_changed = Le ricerche salvate non sono state modificate: { $error }

# draft
query_saved_not_written = le ricerche salvate non sono state scritte

# draft
query_saved_setting = Ricerche salvate mostrate nella barra laterale, in ordine: { $shape }

# draft
query_saved_unreadable = le ricerche salvate nel file delle impostazioni non sono leggibili; non è stato scritto nulla

# draft
query_search = Cerca

# draft
query_search_changed = Ricerca salvata { $name } modificata

# draft
query_search_description = Cerca e mostra il risultato nel riquadro, con il filtro del riquadro aggiunto salvo con `as_is` (ADR-107: i termini di un filtro o di una ricerca che l’elenco può decidere, mantenuti quando il riquadro va altrove finché non vengono tolti; poi `entries` elenca ciò che è stato trovato; una ricerca che il riquadro mostra già resta così com’è, `refresh` la elenca di nuovo): le parole corrispondono ai nomi; `name:`, `ext:pdf,md`, `kind:image,video` (document, text, audio, archive…), `type:dir`, `tag:x`, `content:words` o `content:/regex/`, `comment:`, `size>10M`, `modified<7d` (anche `today`, `this-week`, `2026-05`; `created`, `accessed` e `taken` per l’orologio della fotocamera di un’immagine), `rating>=4`, `near:lat,lon~5km`; `OR`, `NOT` o `-`, parentesi; `hidden:yes` cerca nelle cartelle nascoste, `excluded:yes` nelle cartelle che le ricerche escludono (Impostazioni › Esclusioni: `node_modules`, cache; mai un cestino né una cartella di rete), `depth:2` ne limita la profondità. `root` (o `in:`) è dove cerca

# draft
query_search_here = Cerca qui…

# draft
query_search_moved = Ricerca salvata { $name } spostata

# draft
query_search_not_saved = Questa ricerca non è salvata

# draft
query_search_removed = Ricerca salvata { $name } rimossa

# draft
query_search_renamed = Ricerca salvata { $name } rinominata in { $new }

# draft
query_search_saved = Ricerca salvata { $name }

# draft
query_search_saved_already = Questa ricerca è già salvata come { $name }

# draft
query_search_taken = questa ricerca è già salvata come { $name }

# draft
query_similar = Trova simili

# draft
query_similar_day = Stesso giorno

# draft
query_similar_description = Cerca file simili a quello selezionato: dello stesso genere, modificati lo stesso giorno, di dimensione simile o con un’etichetta in comune, nella cartella cercata o mostrata

# draft
query_similar_kind = Stesso genere

# draft
query_similar_no_date = { $name } non ha una data di modifica

# draft
query_similar_no_kind = { $name } non è di un genere noto al desktop

# draft
query_similar_no_size = { $name } non ha una dimensione

# draft
query_similar_no_tags = { $name } non ha etichette

# draft
query_similar_one = seleziona un solo file per trovarne altri simili

# draft
query_similar_size = Dimensione simile

# draft
query_similar_tags = Stesse etichette

# draft
query_similar_unreadable = { $name } non può essere letto

# draft
query_size_1_to_100 = Da 1 a 100 { $unit }

# draft
query_size_empty = Vuoti

# draft
query_size_over_1 = Oltre 1 { $unit }

# draft
query_size_over_100 = Oltre 100 { $unit }

# draft
query_size_placeholder = Una dimensione o un intervallo: >10{ $m }, 1{ $m }..5{ $m }, <500{ $k }

# draft
query_size_under_1 = Sotto 1 { $unit }

# draft
query_size_under_100 = Sotto 100 { $unit }

# draft
query_tag_all_of = Tutto in { $tag }

# draft
query_tag_choices = Etichette con cui affinare

# draft
query_tag_choices_description = Il selettore del chip Etichetta: le etichette dei file del riquadro, lette da essi, ciascuna affina la ricerca (o ciò che indica `refines`: il `narrowing` o il `filter` della cartella)

# draft
query_tag_files = { $count ->
        [one] { $count } file
       *[other] { $count } file
    }

# draft
query_tag_section_other = Altre

# draft
query_tagged = Con etichetta

# draft
query_tagged_instead = Con etichetta, invece

# draft
query_tagged_menu = Con etichetta…

# draft
query_term_contains = Contiene

# draft
query_term_kind = Genere

# draft
query_term_modified = Modificato

# draft
query_term_size = Dimensione

# draft
query_term_tag = Etichetta

# draft
query_term_taken = Scattato

# draft
query_text_in_file = Testo nel file…

# draft
query_this_folder_only = Solo questa cartella

# draft
query_tokens = Parti di una ricerca

# draft
query_tokens_description = { $shape }: le parti del testo di una ricerca come le legge il parser (parola, chiave, valore, opzione, operatore o un errore con il messaggio del parser), intervalli di byte; un termine ancora in corso di digitazione a `caret` non è ancora un errore

# draft
query_unexpected_paren = `)` inattesa

# draft
query_unkeep_place = Rimuovi dai Salvati

# draft
query_unkeep_search = Rimuovi ricerca salvata

# draft
query_unknown_flag = { $key } flag regex sconosciuto `{ $flag }` ({ $i }: maiuscole o minuscole indifferenti, { $c }: maiuscole e minuscole distinte; metti tra virgolette il testo che inizia con /)

# draft
query_unterminated_quote = virgolette non chiuse

# draft
query_value_dir = Cartella

# draft
query_value_false = No

# draft
query_value_file = File

# draft
query_value_insensitive = Maiuscole non distinte

# draft
query_value_sensitive = Maiuscole distinte

# draft
query_value_symlink = Collegamento simbolico

# draft
query_value_true = Sì

# draft
query_waits_contents = cosa contengono i file

# draft
query_waits_places = dove sono state scattate le immagini

# draft
query_waits_taken = quando sono state scattate le immagini

# draft
query_when_placeholder = Una data, un intervallo o un periodo: 2026-05, 2026-01-01..2026-03-31, <30d

# draft
query_where_excluded = esclusi

# draft
query_where_excluded_files = File e cartelle esclusi

# draft
query_where_hidden = file nascosti

# draft
query_where_hidden_excluded = file nascosti, esclusi

# draft
query_where_hidden_files = File nascosti

# draft
query_where_subfolders = Questa cartella e le sue sottocartelle

# draft
query_which_saved = quale ricerca salvata? Indica il suo `expr` o il suo `name`
