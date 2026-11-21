# Italian catalog of skwir-plugin-tags, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
tags_add = Aggiungi etichette

# draft
tags_add_description = Aggiunge etichette alle voci indicate o alla selezione (piano)

# draft
tags_add_plan = aggiungi { $count ->
        [1] l’etichetta
       *[other] le etichette
    } { $tags }

# draft
tags_add_remove_plan = aggiungi { $added ->
        [1] l’etichetta
       *[other] le etichette
    } { $add } e rimuovi { $removed ->
        [1] l’etichetta
       *[other] le etichette
    } { $remove }

# draft
tags_add_tag_plan = aggiungi l’etichetta { $tag }

# draft
tags_all_tags = Tutte le etichette

# draft
tags_already_called = l’etichetta si chiama già { $to }

# draft
tags_aside_no_name = non ha un nome da cui prendere quello di un file accanto

# draft
tags_aside_other_layout = { $name } accanto è strutturato in modo diverso da come lo scrive un Mac, e Skwir non riscrive altri file

# draft
tags_aside_too_large = { $name } accanto è più grande di quanto Skwir riscriva

# draft
tags_aside_unreadable = { $name } accanto non può essere letto: { $error }

# draft
tags_aside_unwritable = i suoi attributi Mac sono conservati in una forma che Skwir non scrive

# draft
tags_aspect_close = Torna alla cartella

# draft
tags_aspect_open = Etichette di questa cartella

# draft
tags_browse = Etichette di una cartella

# draft
tags_browse_description = Mostra ogni etichetta dei file in una cartella (`in`, un percorso assoluto) come posizioni da esplorare, ciascuna una cartella dei suoi file: la cartella mostrata dal riquadro se omessa, o quella a cui appartiene una posizione di etichetta o una ricerca; con `toggle` e senza cartella, una posizione di etichetta torna alla sua cartella

# draft
tags_browse_here = Esplora etichette qui

# draft
tags_browse_toggle = Etichette di questa cartella, o ritorno a essa

# draft
tags_category_no_comma = il nome di una categoria non contiene virgole

# draft
tags_change_plan = modifica etichette

# draft
tags_color = Colore dell’etichetta

# draft
tags_color_blue = Blu

# draft
tags_color_choice = Colore

# draft
tags_color_description = Scegli il colore che un’etichetta assume ovunque, o restituiscile quello che le dà il suo nome

# draft
tags_color_from_name = Dal nome

# draft
tags_color_green = Verde

# draft
tags_color_not_saved = Il colore dell’etichetta non è stato salvato: { $error }

# draft
tags_color_one_of = il colore di un’etichetta è uno tra { $colors }

# draft
tags_color_orange = Arancione

# draft
tags_color_pink = Rosa

# draft
tags_color_purple = Viola

# draft
tags_color_red = Rosso

# draft
tags_color_teal = Verde acqua

# draft
tags_color_yellow = Giallo

# draft
tags_colors_data = Colori scelti dall’utente per le etichette, per etichetta in minuscolo: {"{"} etichetta = colore {"}"}

# draft
tags_comment_accept = Salva

# draft
tags_comment_item = Commento…

# draft
tags_comment_menu = Menu del commento

# draft
tags_comment_menu_description = La voce Commento… del menu contestuale, con il commento della voce selezionata

# draft
tags_comment_placeholder = Un commento sulla selezione (vuoto lo rimuove)

# draft
tags_comment_title = Commento

# draft
tags_counted = Etichette contate

# draft
tags_counted_description = `{"{"} root, counting, counted, partial {"}"}`: indica se le etichette dei file sotto `root` (la cartella personale se omessa) sono in corso di conteggio per le sue posizioni di etichetta, da quando un conteggio le copre (`counted`, in secondi dall’epoca), e se alcune sono escluse per mancanza di spazio (`partial`). Risponde dalla memoria

# draft
tags_delete = Rimuovi l’etichetta ovunque

# draft
tags_delete_description = Toglie un’etichetta (o `tags`) da ogni file sotto la cartella personale (o `root`) che la possiede: i file sono cercati in un worker e il piano è proposto per la revisione

# draft
tags_depth_below = Con sotto-etichette

# draft
tags_depth_here = Questa etichetta

# draft
tags_depth_top_below = Ogni file con etichetta

# draft
tags_depth_top_here = Queste etichette

# draft
tags_drop = Rilascia su un’etichetta

# draft
tags_drop_description = Assegna la sua etichetta ai file rilasciati o incollati su una posizione di etichetta (`location`): la selezione di un riquadro (`pane`) o `entries`; con `move`, fuori dalla posizione di etichetta da cui provengono (quella del riquadro, o `from`). Le etichette rilasciate vi confluiscono, in un piano da esaminare

# draft
tags_drop_on_one = ai file si dà una sola etichetta: rilasciali su di essa

# draft
tags_edit = Modifica etichette

# draft
tags_edit_description = Aggiunge alcune etichette alle voci indicate o alla selezione e ne rimuove altre, in un unico piano: ogni voce conserva le altre sue etichette

# draft
tags_edit_item = Modifica etichette…

# draft
tags_empty_tag = Nessun file qui ha questa etichetta

# draft
tags_empty_tag_detail = Rilascia qui dei file per assegnare loro l’etichetta.

# draft
tags_empty_top = Nessun file qui ha un’etichetta

# draft
tags_empty_top_detail = Assegna etichette ai file con Etichette… nel loro menu, o rilasciali su un’etichetta nella barra laterale.

# draft
tags_exclusive = Una sola etichetta alla volta per categoria

# draft
tags_exclusive_data = Le categorie di cui un file ha una sola etichetta alla volta: campi, i cui valori sono le etichette al loro interno

# draft
tags_exclusive_description = Fa sì che una categoria (`tag`) contenga una sola etichetta per file alla volta, un campo i cui valori sono le sue etichette (`on`), o di nuovo più etichette

# draft
tags_exclusive_not_changed = { $tag } non è stata modificata: { $error }

# draft
tags_exclusive_off = Un file può di nuovo avere più etichette di { $tag }

# draft
tags_exclusive_on = Un file ha ora una sola etichetta di { $tag } alla volta

# draft
tags_export = Copia un’etichetta in una cartella

# draft
tags_export_description = Crea in `dest` una cartella con i file elencati da una posizione di etichetta (`location`), con il nome della sua etichetta, e al suo interno una cartella per ogni sotto-etichetta: un piano proposto per la revisione. Ciò che fa un’etichetta rilasciata o incollata in una cartella

# draft
tags_export_one = si copia una sola etichetta, non tutte

# draft
tags_export_plan = copia in { $folder } i file con l’etichetta { $tag }

# draft
tags_files_menu = Menu delle etichette della selezione

# draft
tags_files_menu_description = Il sottomenu Etichette del menu contestuale: le etichette dei file selezionati, poi quelle fissate, ciascuna spuntata quando le hanno tutti, parzialmente quando le hanno alcuni; premendola, la si attiva o disattiva su tutti

# draft
tags_files_not_found = I file con l’etichetta { $tag } non sono stati trovati: { $error }

# draft
tags_filter_not_kept = { $done }; il suo filtro { $filter } non viene conservato con essa

# draft
tags_finder_limits = una valutazione, o un colore di etichetta diverso dai sette del Finder

# draft
tags_follow_both_already = { $to } era già fissata e colorata come { $count ->
        [1] lo era { $from }
       *[other] lo erano { $from }
    }

# draft
tags_follow_both_did = { $to } mantenuta fissata e colorata come { $count ->
        [1] lo era { $from }
       *[other] lo erano { $from }
    }

# draft
tags_follow_both_will = mantieni { $to } fissata e colorata come { $count ->
        [1] lo era { $from }
       *[other] lo erano { $from }
    }

# draft
tags_follow_color_already = { $to } aveva già il colore che { $count ->
        [1] aveva { $from }
       *[other] avevano { $from }
    }

# draft
tags_follow_color_did = dato a { $to } il colore che { $count ->
        [1] aveva { $from }
       *[other] avevano { $from }
    }

# draft
tags_follow_color_will = dai a { $to } il colore che { $count ->
        [1] aveva { $from }
       *[other] avevano { $from }
    }

# draft
tags_follow_pin_already = { $to } era già fissata dove { $count ->
        [1] lo era { $from }
       *[other] lo erano { $from }
    }

# draft
tags_follow_pin_did = { $to } fissata dove { $count ->
        [1] lo era { $from }
       *[other] lo erano { $from }
    }

# draft
tags_follow_pin_will = fissa { $to } dove { $count ->
        [1] lo era { $from }
       *[other] lo erano { $from }
    }

# draft
tags_follow_unpin_already = { $count ->
        [1] { $from } era già sbloccata
       *[other] { $from } erano già sbloccate
    }

# draft
tags_follow_unpin_did = sbloccate { $count ->
        [1] { $from }
       *[other] { $from }
    }

# draft
tags_follow_unpin_will = sblocca { $count ->
        [1] { $from }
       *[other] { $from }
    }

# draft
tags_give_file_already = { $name } aveva già l’etichetta { $tag }

# draft
tags_give_file_did = assegnata a { $name } l’etichetta { $tag }

# draft
tags_give_file_will = assegna a { $name } l’etichetta { $tag }

# draft
tags_in_category_already = queste etichette sono già in { $category }

# draft
tags_in_no_category = queste etichette non sono in nessuna categoria

# draft
tags_include_sub_tags = Includi sotto-etichette

# draft
tags_index_full = Alcuni file con etichetta sono esclusi dalle etichette: lo spazio che Skwir riserva loro è esaurito

# draft
tags_inside_itself = { $tag } non può essere inserita in se stessa

# draft
tags_keep = Fissa l’etichetta mostrata

# draft
tags_keep_description = Fissa l’etichetta mostrata dalla posizione di etichetta di un riquadro: una sola etichetta della cartella personale, non ristretta

# draft
tags_keep_forget = Sblocca questa etichetta dalla barra laterale

# draft
tags_keep_one = nella barra laterale si fissa un’etichetta della cartella personale, non ristretta

# draft
tags_keep_save = Fissa questa etichetta nella barra laterale

# draft
tags_kind_field = Campo

# draft
tags_kind_tag = Etichetta

# draft
tags_known = Etichette note

# draft
tags_known_description = `{"{"} generation, tags? {"}"}`: ogni etichetta dell’utente, una volta sola, dalla memoria: quelle fissate, nel loro ordine; poi le più recentemente assegnate; poi quelle viste sui file; poi quelle contate sotto la cartella personale, a partire da quelle con più file. `generation` le numera: se richiesta con quella in possesso, `tags` è omesso finché è aggiornata

# draft
tags_limit_description = Quanta memoria Skwir può dedicare a ciò che sa dei file con etichetta, circa sedicimila file per megabyte, per le etichette esplorate come cartelle, i suggerimenti della barra laterale e le ricerche di etichette più rapide.

# draft
tags_limit_description_more = Oltre questo limite, i nuovi file con etichetta sono esclusi dalle cartelle delle etichette, e Skwir lo segnala.

# draft
tags_limit_title = Spazio per i file con etichetta

# draft
tags_list = Elenca etichette

# draft
tags_list_description = Etichette usate nell’elenco del riquadro, con i conteggi (le etichette che differiscono solo per maiuscole e minuscole contano come una); con `read`, legge anche dai file le cui etichette l’elenco non ha ancora letto

# draft
tags_looking_home = Ricerca dei file con etichetta { $tags } nella cartella personale…

# draft
tags_looking_in = Ricerca dei file con etichetta { $tags } in { $root }…

# draft
tags_merge_accept = Unisci

# draft
tags_merge_ask = Unisci queste etichette in una sola, su ogni file

# draft
tags_merge_item = Unisci in una sola…

# draft
tags_merge_placeholder = L’etichetta risultante

# draft
tags_merge_plan = unisci le etichette { $tags } in { $to }

# draft
tags_merged_into = { $merged ->
        [one] { $merged } file aveva già { $to }, o più delle etichette rinominate: la conserva una sola volta.
       *[other] { $merged } file avevano già { $to }, o più delle etichette rinominate: ciascuno la conserva una sola volta.
    }

# draft
tags_merged_names = { $merged ->
        [one] { $merged } file aveva due etichette che ora hanno lo stesso nome: conserva quell’etichetta una sola volta.
       *[other] { $merged } file avevano due etichette che ora hanno lo stesso nome: ciascuno conserva quell’etichetta una sola volta.
    }

# draft
tags_move = Sposta etichetta in una categoria

# draft
tags_move_accept = Sposta

# draft
tags_move_ask = Sposta in una categoria, mantenendo i nomi

# draft
tags_move_description = Mette un’etichetta (o `tags`) nella categoria `into`, mantenendo il suo nome (`Paris` in `Places/France` diventa `Places/France/Paris`), o la toglie dalla sua categoria senza `into`, su ogni file sotto la cartella personale (o `root`): i file sono cercati in un worker e il piano è proposto per la revisione

# draft
tags_move_file_already = { $name } era già in { $tag }

# draft
tags_move_file_did = spostato { $name } da { $from } a { $tag }

# draft
tags_move_file_will = sposta { $name } da { $from } a { $tag }

# draft
tags_move_into_plan = sposta l’etichetta { $tag } in { $into }

# draft
tags_move_item = Sposta in categoria…

# draft
tags_move_out_all_item = Sposta fuori dalle categorie

# draft
tags_move_out_item = Sposta fuori da { $category }

# draft
tags_move_out_plan = sposta l’etichetta { $tag } fuori dalla sua categoria

# draft
tags_move_placeholder = Categoria (Places/France)

# draft
tags_move_several_into_plan = sposta le etichette { $tags } in { $into }

# draft
tags_move_several_plan = sposta le etichette { $tags }

# draft
tags_move_tag_plan = sposta dall’etichetta { $from } all’etichetta { $tag }

# draft
tags_narrow = Restringi una posizione di etichetta

# draft
tags_narrow_deep_only = una posizione di etichetta si restringe con `deep`; `{ $facet }` è la lente del riquadro (query.narrow)

# draft
tags_narrow_description = Mostra, nella posizione di etichetta mostrata dal riquadro, ogni file dentro l’etichetta (`facet` `deep`), non solo quelli con esattamente quell’etichetta; `on` false lo annulla, se omesso lo inverte. Altre etichette, generi e anni sono la lente del riquadro (`query.narrow`)

# draft
tags_needs_name = un’etichetta ha bisogno di un nome

# draft
tags_needs_search = per trovare i file con etichetta serve il plugin Ricerca

# draft
tags_new = Nuova etichetta

# draft
tags_new_accept = Crea

# draft
tags_new_ask = Nuova etichetta

# draft
tags_new_description = Crea un’etichetta `name` nell’etichetta mostrata da una posizione di etichetta (`location`): vi resta visibile finché non viene assegnata a un file

# draft
tags_new_in_ask = Nuova etichetta in { $tag }

# draft
tags_new_placeholder = Nome

# draft
tags_new_verb = Nuova etichetta…

# draft
tags_no_comma = il nome di un’etichetta non contiene virgole

# draft
tags_no_home = nessuna cartella personale

# draft
tags_no_home_to_find = nessuna cartella personale in cui cercare etichette

# draft
tags_no_rating = Nessuna valutazione

# draft
tags_no_such_pane = riquadro inesistente

# draft
tags_none_tagged_home = Nessun file nella cartella personale ha l’etichetta { $tags }

# draft
tags_none_tagged_in = Nessun file in { $root } ha l’etichetta { $tags }

# draft
tags_one_at_a_time = Una sola etichetta { $category } alla volta

# draft
tags_open = Apri un’etichetta

# draft
tags_open_description = Mostra i file della cartella personale che hanno un’etichetta, e una posizione per ogni sotto-etichetta, come fa la sua pillola fissata, nel riquadro indicato o in quello attivo

# draft
tags_open_tag = Apri etichetta

# draft
tags_pane_shows_no_tags = il riquadro non mostra etichette

# draft
tags_pane_shows_nothing = il riquadro non mostra nulla

# draft
tags_pill = Un’etichetta, qui

# draft
tags_pill_description = Un’etichetta della barra laterale cliccata con Maiusc (`how` `also`: anche i file mostrati che la hanno) o Alt (`here`: i suoi file nella cartella mostrata)

# draft
tags_pin = Fissa etichette

# draft
tags_pin_description = Mantiene etichette (`tag`, `tags`) nella barra laterale, prima dell’etichetta fissata `before` o in fondo; sposta quelle già fissate

# draft
tags_pin_item = Fissa nella barra laterale

# draft
tags_pin_moved = Spostata l’etichetta { $tag }

# draft
tags_pinned = { $count ->
        [1] Fissata l’etichetta { $tag } nella barra laterale
       *[other] Fissate { $count } etichette nella barra laterale
    }

# draft
tags_pinned_data = Le etichette fissate nella barra laterale, nel loro ordine

# draft
tags_pins = Imposta etichette fissate

# draft
tags_pins_back = Ripristinate le etichette fissate com’erano

# draft
tags_pins_description = Fissa esattamente `tags`, nel loro ordine: ciò che esegue l’annullamento di una fissazione

# draft
tags_pins_not_saved = le etichette fissate non sono state salvate

# draft
tags_pins_not_saved_said = Le etichette fissate non sono state salvate: { $error }

# draft
tags_place_menu = Menu delle etichette

# draft
tags_place_menu_description = Le voci del menu contestuale per le etichette selezionate in una posizione di etichetta. Risponde dalla memoria

# draft
tags_plan_note_home = { $count ->
        [one] { $count } file
       *[other] { $count } file
    } nella cartella personale; ciò che le tue esclusioni lasciano fuori resta fuori.

# draft
tags_plan_note_home_merged = { $count ->
        [one] { $count } file
       *[other] { $count } file
    } nella cartella personale; ciò che le tue esclusioni lasciano fuori resta fuori. { $merged }

# draft
tags_plan_note_in = { $count ->
        [one] { $count } file
       *[other] { $count } file
    } in { $root }; ciò che le tue esclusioni lasciano fuori resta fuori.

# draft
tags_plan_note_in_merged = { $count ->
        [one] { $count } file
       *[other] { $count } file
    } in { $root }; ciò che le tue esclusioni lasciano fuori resta fuori. { $merged }

# draft
tags_plugin_description = Etichette, valutazioni e commenti conservati negli attributi estesi dei file, senza database; da dove provengono i download

# draft
tags_plugin_off = il plugin Etichette è disattivato

# draft
tags_plugin_title = Etichette

# draft
tags_rating_choice = Valutazione

# draft
tags_remove = Rimuovi etichette

# draft
tags_remove_all_plan = rimuovi etichette

# draft
tags_remove_all_verb = Rimuovi tutte le etichette

# draft
tags_remove_description = Rimuove etichette dalle voci indicate o dalla selezione (piano)

# draft
tags_remove_everywhere_item = Rimuovi etichetta da ogni file…

# draft
tags_remove_everywhere_plan = rimuovi l’etichetta { $tag } da ogni file

# draft
tags_remove_from_files = { $count ->
        [1] Rimuovi da questo file
       *[other] Rimuovi dai file selezionati
    }

# draft
tags_remove_from_verb = Rimuovi da { $tag }

# draft
tags_remove_plan = rimuovi { $count ->
        [1] l’etichetta
       *[other] le etichette
    } { $tags }

# draft
tags_remove_several_everywhere_plan = rimuovi le etichette { $tags } da ogni file

# draft
tags_rename = Rinomina l’etichetta ovunque

# draft
tags_rename_accept = Rinomina

# draft
tags_rename_ask = Rinomina l’etichetta { $tag } su ogni file

# draft
tags_rename_description = Rinomina un’etichetta (o unisce `tags`) su ogni file sotto la cartella personale (o `root`) che la possiede, unendola a un’etichetta con il nuovo nome dove un file le ha entrambe: i file sono cercati in un worker e il piano è proposto per la revisione

# draft
tags_rename_placeholder = Nuovo nome (quello di un’etichetta esistente le unisce)

# draft
tags_rename_plan = rinomina l’etichetta { $tag } in { $to }

# draft
tags_rename_verb = Rinomina etichetta…

# draft
tags_restore_already = le etichette fissate e i loro colori erano già tornati com’erano

# draft
tags_restore_did = ripristinate le etichette fissate e i loro colori

# draft
tags_restore_will = ripristina le etichette fissate e i loro colori

# draft
tags_search_cancelled = annullata

# draft
tags_search_no_location = la ricerca non ha restituito alcuna posizione

# draft
tags_search_stopped = la ricerca si è interrotta

# draft
tags_selected = Etichette della selezione

# draft
tags_selected_description = `{"{"} items, tags: [{"{"} name, count, color? {"}"}], known {"}"}`: le etichette della selezione, lette dai suoi file, con quanti ne portano ciascuna e il colore che un file dà a una (quello di un’etichetta del Finder), e le etichette note, prima quelle fissate, poi le più recentemente usate

# draft
tags_set = Imposta etichette

# draft
tags_set_description = Sostituisce le etichette delle voci indicate, o della selezione (piano)

# draft
tags_set_plan = imposta { $count ->
        [1] l’etichetta
       *[other] le etichette
    } { $tags }

# draft
tags_show_files_here = Mostra i suoi file qui

# draft
tags_stars = { $stars ->
        [one] { $stars } stella
       *[other] { $stars } stelle
    }

# draft
tags_store_appledouble = I file ._ di un Mac, accanto ai file su volumi senza attributi

# draft
tags_store_finder = Le etichette e i commenti del Finder di un Mac

# draft
tags_store_xdg = Gli attributi del desktop (KDE, Dolphin, Baloo)

# draft
tags_suggested_description = Finché un’etichetta non è fissata, quante ne suggerisce la barra laterale, a partire da quelle con più file; 0 non ne suggerisce nessuna.

# draft
tags_suggested_title = Etichette suggerite nella barra laterale

# draft
tags_suggestion_detail = Etichetta · { $tag }

# draft
tags_tag_menu = Menu di un’etichetta

# draft
tags_tag_menu_description = Le voci del menu di un’etichetta dove è mostrata: il suo chip in Proprietà (tags), la sua pillola nella barra laterale (location). Risponde dalla memoria

# draft
tags_title = Etichette

# draft
tags_toggle = Attiva o disattiva un’etichetta

# draft
tags_toggle_description = Assegna un’etichetta alle voci indicate, o alla selezione, o la toglie da ogni voce quando ce l’hanno tutte: un comando da associare a un tasto; con `on`, la assegna (true) o la toglie (false) qualunque sia lo stato delle voci (piano)

# draft
tags_toggle_form = Attiva o disattiva l’etichetta { $tag }

# draft
tags_unpin = Sblocca etichette

# draft
tags_unpin_description = Toglie etichette (`tag`, `tags`) dalla barra laterale; i loro file le conservano

# draft
tags_unpin_item = Sblocca dalla barra laterale

# draft
tags_unpinned = { $count ->
        [1] Sbloccata l’etichetta { $tag }
       *[other] Sbloccate { $count } etichette
    }

# draft
tags_untag = Rimuovi dall’etichetta

# draft
tags_untag_all_file_already = { $name } non aveva già etichette

# draft
tags_untag_all_file_did = rimosse le etichette di { $name }

# draft
tags_untag_all_file_will = rimuovi le etichette di { $name }

# draft
tags_untag_all_plan = rimuovi tutte le etichette

# draft
tags_untag_description = Toglie l’etichetta mostrata da una posizione di etichetta (quella del riquadro, o `location`) dai file selezionati; le etichette selezionate lì vengono tolte da ogni file, in un piano da esaminare. Ciò che fa Canc in una posizione di etichetta

# draft
tags_untag_file_already = l’etichetta { $tag } era già assente da { $name }

# draft
tags_untag_file_did = rimossa l’etichetta { $tag } da { $name }

# draft
tags_untag_file_will = rimuovi l’etichetta { $tag } da { $name }

# draft
tags_untag_plan = rimuovi l’etichetta { $tag }

# draft
tags_what_new_name = quale nuovo nome?

# draft
tags_which_category = quale categoria?

# draft
tags_which_tag = quale etichetta?

# draft
tags_with_sub_tags = Con sotto-etichette

# draft
tags_xdg_limits = una virgola in un’etichetta, o il colore di un’etichetta
