# Italian catalog of skwir-core, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
access_another_file = { $name } ora è un altro file

# draft
access_both_already = { $inside ->
        [yes] l’accesso e il gruppo di { $name } e di tutto ciò che contiene erano già come richiesto
       *[no] l’accesso e il gruppo di { $name } erano già come richiesto
    }

# draft
access_change_both = { $inside ->
        [yes] cambia l’accesso e il gruppo di { $name } e di tutto ciò che contiene
       *[no] cambia l’accesso e il gruppo di { $name }
    }

# draft
access_change_group = { $inside ->
        [yes] cambia il gruppo di { $name } e di tutto ciò che contiene
       *[no] cambia il gruppo di { $name }
    }

# draft
access_change_mode = { $inside ->
        [yes] cambia chi può usare { $name } e tutto ciò che contiene ({ $mode })
       *[no] cambia chi può usare { $name } ({ $mode })
    }

# draft
access_changed_both = { $inside ->
        [yes] cambiati l’accesso e il gruppo di { $name } e di tutto ciò che contiene
       *[no] cambiati l’accesso e il gruppo di { $name }
    }

# draft
access_changed_group = { $inside ->
        [yes] cambiato il gruppo di { $name } e di tutto ciò che contiene
       *[no] cambiato il gruppo di { $name }
    }

# draft
access_changed_mode = { $inside ->
        [yes] cambiato chi può usare { $name } e tutto ciò che contiene
       *[no] cambiato chi può usare { $name }
    }

# draft
access_clause_also = { $who ->
        [u] il proprietario può anche { $does }
        [g] il gruppo può anche { $does }
        [o] gli altri possono anche { $does }
        [ug] il proprietario e il gruppo possono anche { $does }
        [uo] il proprietario e gli altri possono anche { $does }
        [go] il gruppo e gli altri possono anche { $does }
        [umask] tutti, tranne ciò che la umask ({ $umask }) esclude, possono anche { $does }
       *[all] tutti possono anche { $does }
    }

# draft
access_clause_no_longer = { $who ->
        [u] il proprietario non può più { $does }
        [g] il gruppo non può più { $does }
        [o] gli altri non possono più { $does }
        [ug] il proprietario e il gruppo non possono più { $does }
        [uo] il proprietario e gli altri non possono più { $does }
        [go] il gruppo e gli altri non possono più { $does }
        [umask] tutti, tranne ciò che la umask ({ $umask }) esclude, non possono più { $does }
       *[all] tutti non possono più { $does }
    }

# draft
access_clause_none = { $who ->
        [u] il proprietario non può usarlo
        [g] il gruppo non può usarlo
        [o] gli altri non possono usarlo
        [ug] il proprietario e il gruppo non possono usarlo
        [uo] il proprietario e gli altri non possono usarlo
        [go] il gruppo e gli altri non possono usarlo
        [umask] tutti, tranne ciò che la umask ({ $umask }) esclude, non possono usarlo
       *[all] nessuno può usarlo
    }

# draft
access_clause_nothing = non cambia nulla per { $who ->
        [u] il proprietario
        [g] il gruppo
        [o] gli altri
        [ug] il proprietario e il gruppo
        [uo] il proprietario e gli altri
        [go] il gruppo e gli altri
        [umask] tutti, tranne ciò che la umask ({ $umask }) esclude
       *[all] tutti
    }

# draft
access_clause_only = { $who ->
        [u] il proprietario può solo { $does }
        [g] il gruppo può solo { $does }
        [o] gli altri possono solo { $does }
        [ug] il proprietario e il gruppo possono solo { $does }
        [uo] il proprietario e gli altri possono solo { $does }
        [go] il gruppo e gli altri possono solo { $does }
        [umask] tutti, tranne ciò che la umask ({ $umask }) esclude, possono solo { $does }
       *[all] tutti possono solo { $does }
    }

# draft
access_does_both = { $letters ->
        [r] leggere
        [w] scrivere
        [x] eseguirlo
        [rw] leggere e scrivere
        [rx] leggere ed eseguirlo
        [wx] scrivere ed eseguirlo
        [X] aprirlo (un file, se qualcuno può già eseguirlo)
        [rX] leggere e aprirlo (un file, se qualcuno può già eseguirlo)
        [wX] scrivere e aprirlo (un file, se qualcuno può già eseguirlo)
        [rwX] leggere, scrivere e aprirlo (un file, se qualcuno può già eseguirlo)
       *[rwx] leggere, scrivere ed eseguirlo
    }

# draft
access_does_copy = { $class ->
        [u] ciò che il proprietario può fare
        [g] ciò che il gruppo può fare
       *[o] ciò che gli altri possono fare
    }

# draft
access_does_file = { $letters ->
        [r] leggere
        [w] scrivere
        [x] eseguirlo
        [rw] leggere e scrivere
        [rx] leggere ed eseguirlo
        [wx] scrivere ed eseguirlo
        [X] eseguirlo se qualcuno può già farlo
        [rX] leggere ed eseguirlo se qualcuno può già farlo
        [wX] scrivere ed eseguirlo se qualcuno può già farlo
        [rwX] leggere, scrivere ed eseguirlo se qualcuno può già farlo
       *[rwx] leggere, scrivere ed eseguirlo
    }

# draft
access_does_folder = { $letters ->
        [r] elencarla
        [w] modificare ciò che contiene
        [x] aprirla
        [rw] elencarla e modificare ciò che contiene
        [rx] elencarla e aprirla
        [wx] modificare ciò che contiene e aprirla
       *[rwx] elencarla, modificare ciò che contiene e aprirla
    }

# draft
access_drive_read_only = l’unità è di sola lettura

# draft
access_each_class = il proprietario { $owner ->
        [none] non può usarlo
       *[some] può { $owner_does }
    }, il gruppo { $group ->
        [none] non può usarlo
       *[some] può { $group_does }
    }, gli altri { $others ->
        [none] non possono usarlo
       *[some] possono { $others_does }
    }

# draft
access_everyone_may = tutti { $all ->
        [none] non possono usarlo
       *[some] possono { $all_does }
    }

# draft
access_failed_not_put_back = { $error }, e { $not_put_back }

# draft
access_files_and_folders = file { $files }; cartelle { $folders }

# draft
access_folder_keeps_set_id = { $said }; una cartella mantiene i bit set-ID che ha ({ $clears } li azzera)

# draft
access_gone = { $name } non c’è più

# draft
access_group_already = { $inside ->
        [yes] il gruppo di { $name } e di tutto ciò che contiene era già come richiesto
       *[no] il gruppo di { $name } era già come richiesto
    }

# draft
access_in_trash = { $name } è nel cestino: ripristinalo prima

# draft
access_inside_not_yours = { $path } al suo interno non è tuo: solo il suo proprietario può cambiare chi può usarlo

# draft
access_mode_already = { $inside ->
        [yes] chi può usare { $name } e tutto ciò che contiene era già come richiesto
       *[no] chi può usare { $name } era già come richiesto
    }

# draft
access_mode_comes_to = { $mode }: { $who }

# draft
access_no_class = { $part } non indica nessuna classe e questo sistema non dice cosa esclude la sua umask, come farebbe chmod: indica le classi, come u{ $clause } per il proprietario o a{ $clause } per tutti

# draft
access_not_a_mode = { $text } non è una modalità: scrivila come fa chmod, ad esempio 644, u+x o go-w

# draft
access_not_changed = non è stato possibile cambiare chi può usare { $path }

# draft
access_not_given_back = non è stato possibile restituirlo al gruppo { $group }

# draft
access_not_given_group = non è stato possibile assegnare { $path } al gruppo { $group }

# draft
access_not_in_group = non fai parte del gruppo { $group }

# draft
access_not_kept = l’unità che contiene { $name } non ha mantenuto la modifica: il suo filesystem non conserva i permessi

# draft
access_not_owner = solo il suo proprietario può cambiare chi può usarlo

# draft
access_not_put_back = { $more ->
        [0] non è stato possibile ripristinare chi può usare { $path }: { $error }
       *[other] non è stato possibile ripristinare chi può usare { $path } (e altri { $more }): { $error }
    }

# draft
access_not_read_only = Non di sola lettura

# draft
access_not_yours = { $path } non è tuo

# draft
access_nothing_asked = nulla da cambiare: indica una modalità o un gruppo

# draft
access_owner_group_then_others = il proprietario e il gruppo { $both ->
        [none] non possono usarlo
       *[some] possono { $both_does }
    }, gli altri { $others ->
        [none] non possono usarlo
       *[some] possono { $others_does }
    }

# draft
access_owner_then_rest = il proprietario { $owner ->
        [none] non può usarlo
       *[some] può { $owner_does }
    }, il gruppo e gli altri { $rest ->
        [none] non possono usarlo
       *[some] possono { $rest_does }
    }

# draft
access_put_back = { $more ->
        [0] ripristina chi può usare { $name }
       *[other] ripristina chi può usare { $name } e altri { $more }
    }

# draft
access_put_back_already = { $more ->
        [0] chi può usare { $name } era già come prima
       *[other] chi può usare { $name } e altri { $more } era già come prima
    }

# draft
access_put_back_done = { $more ->
        [0] ripristinato chi può usare { $name }
       *[other] ripristinato chi può usare { $name } e altri { $more }
    }

# draft
access_read_only = Sola lettura

# draft
access_reference = Accesso

# draft
access_reference_600 = Solo il proprietario legge e scrive

# draft
access_reference_644 = Il proprietario legge e scrive; il gruppo e gli altri leggono

# draft
access_reference_700 = Solo il proprietario legge, scrive ed esegue

# draft
access_reference_755 = Tutti leggono ed eseguono; il proprietario scrive anche

# draft
access_reference_also = Può anche

# draft
access_reference_big_x = Apre le cartelle ed esegue ciò che qualcuno può già eseguire

# draft
access_reference_clauses_note = Le clausole si susseguono, separate da virgole: u+x,go-w.

# draft
access_reference_digits = Cifre: proprietario, gruppo, altri

# draft
access_reference_digits_note = Ogni cifra somma 4 per leggere, 2 per scrivere e 1 per eseguire (per aprire una cartella).

# draft
access_reference_everyone = Tutti

# draft
access_reference_group = Il gruppo

# draft
access_reference_no_longer = Non può più

# draft
access_reference_only = Può solo

# draft
access_reference_others = Gli altri

# draft
access_reference_owner = Il proprietario

# draft
access_reference_r = Leggere (elencare, per una cartella)

# draft
access_reference_s = Eseguire come proprietario o gruppo; il gruppo di una cartella si trasmette

# draft
access_reference_t = Solo i proprietari rimuovono ciò che contiene una cartella

# draft
access_reference_then = Poi

# draft
access_reference_w = Scrivere (modificare ciò che contiene una cartella)

# draft
access_reference_what = Cosa

# draft
access_reference_who = Chi

# draft
access_reference_x = Eseguire (aprire, per una cartella)

# draft
access_refused = { $name }: { $why }

# draft
access_set_gid_cleared = il sistema ha azzerato il bit set-group-ID di { $name }: solo un membro del suo gruppo può impostarlo

# draft
access_sgid_both_file = un file viene eseguito come il suo gruppo

# draft
access_sgid_both_file_gone = un file non viene più eseguito come il suo gruppo

# draft
access_sgid_both_folder = ciò che viene creato in una cartella prende il suo gruppo

# draft
access_sgid_both_folder_gone = ciò che viene creato in una cartella non prende più il suo gruppo

# draft
access_sgid_file = viene eseguito come il suo gruppo

# draft
access_sgid_file_gone = non viene più eseguito come il suo gruppo

# draft
access_sgid_folder = ciò che viene creato al suo interno prende il suo gruppo

# draft
access_sgid_folder_gone = ciò che viene creato al suo interno non prende più il suo gruppo

# draft
access_sticky_both = solo i proprietari possono rimuovere ciò che contiene una cartella

# draft
access_sticky_both_gone = chi può modificare una cartella può rimuovere ciò che contiene

# draft
access_sticky_file = mantiene un bit sticky, che un file ignora

# draft
access_sticky_file_gone = il suo bit sticky, che un file ignora, viene rimosso

# draft
access_sticky_folder = solo i proprietari possono rimuovere ciò che contiene

# draft
access_sticky_folder_gone = chi può modificarla può rimuovere ciò che contiene

# draft
access_stopped_not_put_back = interrotto, e { $not_put_back }

# draft
access_suid_both = un file viene eseguito come il suo proprietario

# draft
access_suid_both_gone = un file non viene più eseguito come il suo proprietario

# draft
access_suid_file = viene eseguito come il suo proprietario

# draft
access_suid_file_gone = non viene più eseguito come il suo proprietario

# draft
access_suid_folder = mantiene un bit set-user-ID, che una cartella ignora

# draft
access_suid_folder_gone = il suo bit set-user-ID, che una cartella ignora, viene rimosso

# draft
access_then = { $said }; { $then }

# draft
access_unreadable = non è stato possibile leggere { $path }

# draft
access_windows_each_stays = Ognuno resta com’è: qui viene mantenuto solo il w del proprietario

# draft
app_access_group_title = cambia accesso e gruppo

# draft
app_access_nothing = indica una modalità, un gruppo o entrambi

# draft
app_access_title = cambia accesso ({ $mode })

# draft
app_apply_plan = Applica piano

# draft
app_apply_plan_description = Applica un piano fornito come dati (plan): come per ogni comando, ad applicarlo è il chiamante, subito per un gesto dell’utente, dopo la revisione per un agente

# draft
app_arriving = In arrivo in una cartella

# draft
app_arriving_description = Restituisce ciò che le azioni in attesa e in corso portano in una cartella (quella del riquadro, se non ne viene indicata una) e non è ancora arrivato (ADR-135): il nome di ogni elemento, il piano che lo porta, il suo stato (in attesa, in avvio, in copia insieme ad altri alla frazione del gruppo, in spostamento alla propria frazione da 0 a 1, arrivato: a destinazione mentre la sua azione prosegue), se la sua azione è in pausa, da dove viene copiato o spostato, e se è una cartella o un file e di quale dimensione

# draft
app_back = Indietro

# draft
app_back_description = Vai alla posizione precedente

# draft
app_commands = Elenca comandi

# draft
app_commands_description = Restituisce ogni comando registrato con il suo schema degli argomenti

# draft
app_copy = Copia in…

# draft
app_copy_description = Copia la selezione, o le voci indicate, in una cartella (piano); on_conflict dice cosa succede a un nome già in uso lì

# draft
app_delete = Elimina definitivamente

# draft
app_delete_description = Elimina la selezione, o le voci indicate, senza passare dal cestino (piano, irreversibile)

# draft
app_delete_title = elimina definitivamente

# draft
app_destination_path = la destinazione deve essere un percorso

# draft
app_entries = Elenca voci

# draft
app_entries_description = Ciò che mostra un riquadro, nel suo ordine (ordinato, filtrato, con le cartelle aperte sul posto), oppure con `location` ciò che contiene una cartella, elencato senza mostrarla: id, nome, percorso, genere, dimensione e data di modifica (secondi dal 1970) di ogni voce, e se è selezionata; a partire da `offset`, al massimo `limit`. `complete: false`: l’elenco stava ancora arrivando, chiedi di nuovo. `as_of`, se presente: la risposta è quella di un indice all’ultimo aggiornamento (`by` dice quale, `updated` quando, in secondi dal 1970), quindi un file creato, rinominato o rimosso da allora non vi compare. `excluded`, se presente: quanti file e cartelle sotto la radice la ricerca ha saltato (le cartelle escluse in Impostazioni › Esclusioni, cache, cestino, cartelle di rete); `excluded:yes` nella ricerca attraversa anche quelle cartelle

# draft
app_expand = Espandi cartella

# draft
app_expand_description = Apre le cartelle sul posto sotto quella mostrata, come le disegna la vista ad albero, oppure le chiude

# draft
app_filter = Filtra

# draft
app_filter_description = Mostra solo le voci che `text` mantiene: come lo legge il lettore del filtro (il plugin query: le parole trovano nomi, i termini che l’elenco può decidere, come kind:image, ext:pdf o size>1M, valgono per i file, mentre una cartella compare se una ricerca al suo interno ne trova uno), altrimenti quelle il cui nome lo contiene (senza distinguere maiuscole e minuscole, a meno che contenga una maiuscola); senza `text`, tutte. I suoi termini che valgono per i file sono la lente del riquadro, mantenuta quando mostra altre posizioni finché non viene tolta; le sue parole restano con la posizione

# draft
app_forward = Avanti

# draft
app_forward_description = Torna alla posizione da cui si era tornati indietro

# draft
app_group = Raggruppa

# draft
app_group_as_sorted = Raggruppa come l’ordinamento

# draft
app_group_by = Raggruppa per { $key }

# draft
app_group_by_way = Raggruppa per { $key }, { $way }

# draft
app_group_description = Raggruppa le voci di un riquadro sotto intestazioni (ADR-130): per `by` (none, same_as_sort: per la chiave dell’ordinamento nella sua direzione, o una chiave come la accetta `sort`: name, kind, type, extension, size, modified, created, accessed, deleted, rating, tags, owner, folder; taken, dimensions, duration, lette dai file su richiesta), nella direzione `ascending` (ogni chiave parte dalla propria: le parole da A a Z, il più grande, il più recente, il meglio valutato per primi; la stessa chiave di nuovo la inverte). All’interno di ogni gruppo le cartelle vengono per prime finché lo sono ovunque, poi decide l’ordinamento

# draft
app_group_fold = Comprimi il gruppo

# draft
app_group_fold_description = Comprime o apre i gruppi di un riquadro (ADR-131): il gruppo `group` (contato dalla prima intestazione a partire da 0; quello del cursore se omesso) come dice `mode`: toggle, fold, unfold, fold_others (ogni gruppo tranne quello), unfold_all. Le voci di un gruppo compresso escono da ciò che il riquadro mostra (non disegnate, saltate dai tasti, fuori da Seleziona tutto e da ciò su cui agiscono i comandi), con l’intestazione mantenuta insieme al suo conteggio; la compressione si mantiene finché il riquadro raggruppa con lo stesso criterio e ha effetto solo in una vista che disegna le intestazioni dei gruppi (l’elenco, l’albero, la griglia): in ogni altra, tutti i file restano visibili

# draft
app_group_fold_others = Comprimi gli altri gruppi

# draft
app_group_fold_toggle = Comprimi o espandi il gruppo

# draft
app_group_none = Non raggruppare

# draft
app_group_title = cambia gruppo ({ $group })

# draft
app_group_unfold_all = Espandi tutti i gruppi

# draft
app_history = Cronologia annullamenti

# draft
app_history_description = Restituisce le azioni che Annulla può revocare, dalla più recente (ADR-079, 13). Ognuna indica il suo `entry` (come vengono dati annulla e ripeti), il suo `title`, `at`: quando è stata applicata (secondi dal 1970, UTC), il suo `state` (running, waiting, done o undone), `can`: cosa può fare ora il suo Annulla, o il Ripeti di un’azione annullata (`now`; `once_ended`, mentre è in corso o in attesa; `after`, dopo che l’azione indicata è stata annullata o ripetuta prima; `going`, mentre viene annullata o ripetuta; `never`), e `with`: le voci che un annullamento con `with_after` revoca insieme ad essa

# draft
app_hydrate = Carica metadati

# draft
app_hydrate_description = Richiede i metadati delle voci (le viste lo chiamano per le righe visibili)

# draft
app_in_no_folder = { $path } non è in nessuna cartella

# draft
app_keep = Aggiungi alla barra laterale

# draft
app_keep_cannot = questa posizione non può essere tenuta nella barra laterale

# draft
app_keep_description = Tiene nella barra laterale ciò che il riquadro mostra, così com’è mostrato (ADR-109): una cartella come segnalibro, una ricerca come ricerca salvata, qualsiasi cosa mostrata filtrata come posizione salvata con il suo filtro

# draft
app_keep_nothing = il riquadro non mostra nulla da tenere

# draft
app_link = Crea collegamento in…

# draft
app_link_description = Crea in una cartella un collegamento a ciascuno degli elementi della selezione, o delle voci indicate, con il loro nome (piano); on_conflict dice cosa succede a un nome già in uso lì

# draft
app_mkdir = Nuova cartella

# draft
app_mkdir_description = Crea una cartella nella cartella indicata in `in`, o in quella mostrata dal riquadro (piano)

# draft
app_move = Sposta in…

# draft
app_move_description = Sposta la selezione, o le voci indicate, in una cartella (piano); on_conflict dice cosa succede a un nome già in uso lì

# draft
app_navigate = Vai alla posizione

# draft
app_navigate_description = Apre un percorso o una posizione in un riquadro, con una sua voce selezionata una volta elencata se `select` ne indica una, altrimenti, risalendo fin lì, quella da cui passa la via del ritorno; `in_place` la mette al posto del passaggio mostrato dal riquadro, senza un passaggio proprio; `as_kept` la mostra come viene tenuta una posizione salvata, filtrata dal solo `filter`, senza la lente del riquadro (ADR-109)

# draft
app_new_file = Nuovo file

# draft
app_new_file_description = Crea un file vuoto nella cartella indicata in `in`, o in quella mostrata dal riquadro (piano); un nome che termina con / crea una cartella

# draft
app_new_file_title = nuovo file { $name }

# draft
app_new_folder_title = nuova cartella { $name }

# draft
app_no_directory = il riquadro non mostra una cartella

# draft
app_no_pane = nessun riquadro di questo tipo

# draft
app_no_pane_numbered = nessun riquadro { $pane }

# draft
app_no_provider = nessun provider per lo schema `{ $scheme }`

# draft
app_no_work_dir = seleziona qualcosa o apri una cartella

# draft
app_notes_read = Leggi

# draft
app_notes_read_all = Contiene tutte le annotazioni di un file.

# draft
app_notes_read_limits = Ciò che non può contenere: { $limits }.

# draft
app_notes_read_merged = Leggi: ciò che conserva viene mostrato unito alle annotazioni delle altre posizioni.

# draft
app_notes_write = Scrivi

# draft
app_notes_write_renames = Mai; oppure, dove il nome di un file contiene già etichette, rinomina il file quando una modifica le cambia.

# draft
app_notes_write_when = Mai; solo dove un file ha già annotazioni qui; anche dove il volume di un file non conserva attributi; oppure per ogni file modificato.

# draft
app_nothing_taken_out = da qui non si toglie nulla: è Sposta nel cestino a eliminare

# draft
app_nothing_to_redo = niente da ripetere

# draft
app_nothing_to_retry = niente da riprovare

# draft
app_nothing_to_undo = niente da annullare

# draft
app_orders_unreadable = { $key } nel file dei dati non contiene un elenco: correggilo, o toglilo, per ricordare un ordinamento

# draft
app_pane_went_elsewhere = il riquadro è passato altrove

# draft
app_passphrase = Passphrase

# draft
app_password = Password

# draft
app_plan_diff = Anteprima piano

# draft
app_plan_diff_description = Esegue un piano in prova e restituisce cosa farebbe ogni passaggio

# draft
app_redo = Ripeti

# draft
app_redo_description = Ripete il piano annullato più di recente, o la voce di cronologia annullata indicata se nulla di annullato dopo di essa ha toccato ciò che ha fatto; se richiesto dall’esterno della finestra, dice cosa ha applicato di nuovo; dalla finestra, risponde con l’id con cui è noto il suo avvio (runs), come fa Annulla

# draft
app_refresh = Aggiorna

# draft
app_refresh_description = Elenca di nuovo la posizione del riquadro

# draft
app_relative_nowhere = { $path } è relativo e non c’è alcuna cartella mostrata in cui possa trovarsi

# draft
app_remember_order = Ricorda l’ordinamento di questa cartella

# draft
app_remember_order_description = Ricorda come è ordinata e raggruppata la cartella mostrata da un riquadro (ADR-132), e la vista in cui è mostrata (`view`: elenco o griglia), oppure la dimentica (`on`; alternato se omesso): una cartella che ricorda viene mostrata così in ogni riquadro delle finestre dell’utente (se ricordata in una finestra che risponde alla finestra di dialogo di un altro programma, viene mostrata così in ogni riquadro di tali finestre: ciò che ricordano le finestre di dialogo è tenuto a parte, ADR-145), e una modifica del suo ordinamento, dei gruppi o della vista fatta lì cambia ciò che ricorda, mentre l’ordinamento e la vista propri del riquadro vengono messi da parte finché la mostra e tornano quando ne mostra un’altra; dimenticata, tornano quelli propri del riquadro. La sua vista viene mostrata solo finché il riquadro mostra una vista che una cartella può ricordare, mai al posto delle colonne o dell’albero; ogni colonna delle colonne segue l’ordinamento della propria cartella. Vale per qualsiasi posizione tranne una ricerca, i cui termini sono ciò che essa è (il cestino, la posizione di un’etichetta); i file nascosti restano quelli del riquadro, mentre cartelle per prime è l’impostazione dell’utente (quella propria delle finestre di dialogo nelle finestre di dialogo)

# draft
app_rename = Rinomina

# draft
app_rename_description = Rinomina la voce selezionata, o quella indicata (piano)

# draft
app_rename_one = rinominare richiede esattamente una voce

# draft
app_rename_title = rinomina in { $name }

# draft
app_retry = Riprova

# draft
app_retry_description = Esegue di nuovo ciò che un piano interrotto da un errore ha lasciato, l’ultimo interrotto se non viene indicato quale; ciò che applica viene annullato insieme a quel piano; se richiesto dall’esterno della finestra, dice cosa ha applicato

# draft
app_room_detail = { $title }: un’unità, soprattutto una condivisione di rete, può dire di avere meno spazio di quanto ne abbia. Copia comunque, oppure annulla e libera spazio.

# draft
app_running = Azioni in corso

# draft
app_running_description = Restituisce le azioni in corso, nell’ordine in cui sono partite: per ognuna il titolo, i passaggi conclusi, i byte fatti e in totale quando noti (pianificati), la velocità in byte al secondo e il tempo rimasto in secondi finché si possono stabilire, e i file che un server sta ancora accettando (ADR-113)

# draft
app_search_order = l’ordinamento di una ricerca non viene ricordato: i suoi termini sono ciò che essa è

# draft
app_select = Seleziona

# draft
app_select_all = Seleziona tutto

# draft
app_select_all_description = Seleziona ogni voce

# draft
app_select_description = Seleziona voci per id o nome

# draft
app_select_invert = Inverti selezione

# draft
app_select_invert_description = Seleziona ciò che è mostrato e non selezionato, e deseleziona il resto

# draft
app_select_no_entry = nessuna voce di nome `{ $name }`

# draft
app_select_none = Deseleziona tutto

# draft
app_select_none_description = Deseleziona ogni cosa

# draft
app_select_not_shown = `{ $name }` non è mostrato: nascosto, o escluso dal filtro

# draft
app_select_range_target = un intervallo richiede un id di destinazione

# draft
app_select_sweep = Seleziona al movimento del cursore

# draft
app_select_sweep_description = Mantiene un intervallo a partire dal cursore: muovendosi seleziona, o con `unset` deseleziona, ciò che sta in mezzo

# draft
app_select_sweep_end = Smetti di selezionare al movimento del cursore

# draft
app_select_sweep_end_description = Termina l’intervallo mantenuto a partire dal cursore, conservando ciò che copre

# draft
app_select_sweep_unset = Deseleziona al movimento del cursore

# draft
app_set_access = Cambia accesso

# draft
app_set_access_describe = Descrivi cosa fa una modalità

# draft
app_set_access_describe_description = Dice a parole cosa fa `mode` (come la accetta chmod) agli elementi la cui modalità è `current` (se condivisa da tutti, un numero; altrimenti cosa fa ogni clausola a ciascuno), `folders` o `files` quando sono tutti dello stesso tipo (altrimenti entrambi, detti separatamente dove differiscono); un errore per ciò che non è una modalità, come lo rifiuta `set_access`

# draft
app_set_access_description = Cambia chi può leggere, scrivere ed eseguire la selezione, o le voci indicate: `mode` come la accetta chmod, `group` per numero, `inside` per ciò che contengono le cartelle (piano). Cambia ciò a cui porta un collegamento; il proprietario mai

# draft
app_show_companions = Mostra file nascosti e di accompagnamento

# draft
app_show_companions_description = Mostra i file nascosti nel riquadro e attiva Mostra file di accompagnamento (`core.show_companions`): i file che conservano accanto le annotazioni di un altro file, in ogni cartella

# draft
app_show_in_folder = Mostra nella cartella

# draft
app_show_in_folder_description = Apre la cartella in cui si trova il file selezionato, con il file selezionato: da una ricerca, un’etichetta, la Mensola, dove sono elencati file di molte cartelle

# draft
app_show_in_folder_one = seleziona un file da mostrare nella sua cartella

# draft
app_shows_nothing = il riquadro non mostra nulla

# draft
app_sort = Ordina

# draft
app_sort_ascending = Ordine crescente

# draft
app_sort_by = Ordina per { $key }

# draft
app_sort_by_way = Ordina per { $key }, { $way }

# draft
app_sort_descending = Ordine decrescente

# draft
app_sort_description = Imposta la chiave di ordinamento, la direzione e la visibilità dei file nascosti di un riquadro, e se le cartelle vengono per prime (`folders_first`: quella propria della finestra di dialogo in una finestra che risponde alla finestra di dialogo di un altro programma, altrimenti l’impostazione dell’utente in tutte le sue finestre, che la chiamata di un agente chiede prima all’utente)

# draft
app_take_out = Togli da qui

# draft
app_take_out_description = Toglie la selezione da una posizione che elenca file conservati altrove, come dice essa stessa (quella di un’etichetta toglie loro l’etichetta, quella della Mensola li toglie dalla Mensola): i file restano dove sono. Ciò che fa il tasto Canc lì

# draft
app_take_out_not_in_dialogs = { $action } non si può fare in una finestra che risponde alla finestra di dialogo di un altro programma

# draft
app_toast_redo = Ripeti

# draft
app_toast_undo = Annulla

# draft
app_toggle_hidden = Mostra/nascondi file nascosti

# draft
app_toggle_hidden_description = Mostra o nasconde le voci che iniziano con un punto

# draft
app_trash = Sposta nel cestino

# draft
app_trash_description = Sposta nel cestino la selezione, o le voci indicate (piano)

# draft
app_trash_none_kept = { $count ->
        [1] È in una cartella di rete, che non ha un cestino
       *[other] Sono in cartelle di rete, che non hanno un cestino
    }

# draft
app_trash_others_kept = { $count ->
        [1] È in una cartella di rete, che non ha un cestino
       *[other] Sono in cartelle di rete, che non hanno un cestino
    }; { $kept ->
        [1] l’altro elemento va nel cestino
       *[other] gli altri { $kept } elementi vanno nel cestino
    }

# draft
app_trash_title = sposta nel cestino

# draft
app_undo = Annulla

# draft
app_undo_description = Annulla il piano più recente, o il piano o la voce di cronologia indicata se nulla di richiesto dopo di essa ha toccato ciò che ha fatto (with_after, con una voce: insieme alle voci successive che l’hanno fatto, dalla più recente); se richiesto dall’esterno della finestra, dice cosa ha revocato, con i titoli delle voci della cronologia; dalla finestra, risponde con gli id con cui sono noti i suoi avvii (runs), perché la finestra segua solo i propri

# draft
app_undo_plan_or_entry = indica un piano o una voce, non entrambi

# draft
app_undo_with_after = with_after revoca una voce insieme a ciò che è venuto dopo: indica la voce

# draft
app_up = Cartella superiore

# draft
app_up_description = Vai alla cartella che contiene quella mostrata, o alla posizione precedente nel suo percorso (la categoria di un’etichetta, la cartella in cui cerca una ricerca), e seleziona lì quella appena lasciata

# draft
clash_taken = { $name } c’è già

# draft
command_invalid_args = argomenti non validi per `{ $command }`: { $reason }

# draft
command_nothing_selected = nessuna selezione

# draft
command_nothing_to_paste = niente da incollare

# draft
command_panicked = { $command } si è interrotto per un errore interno (il registro dice dove)

# draft
command_unknown = comando sconosciuto `{ $command }`

# draft
command_with_args = { $title } ({ $args })

# draft
entry_kept_elsewhere = { $count ->
        [one] { $count } file ha nomi altrove: il disco conserva { $size }
       *[other] { $count } file hanno nomi altrove: il disco conserva { $size }
    }

# draft
exclusions_agent_exposes = un agente non può rendere leggibili cartelle private ({ $folders }): solo tu puoi, in Impostazioni, Esclusioni

# draft
exclusions_agent_shell = un agente non può cambiare se le variabili della shell di login vengono lette: una cartella privata che solo quelle indicano smetterebbe di essere privata. Solo tu puoi, in Impostazioni, Privacy

# draft
exclusions_bad_glob = { $pattern }: { $error }

# draft
exclusions_bad_names = names è files, folders o both, non { $names }

# draft
exclusions_bad_regex = regex: { $error }

# draft
exclusions_bad_variable = { $pattern }: una variabile è $NAME, ${"{"}NAME{"}"} o ${"{"}NAME:-ciò che la sostituisce se non è impostata{"}"}

# draft
exclusions_beside_files = { $pattern }: beside vale per le cartelle, non per i file

# draft
exclusions_beside_name = { $sibling }: beside indica un file o una cartella, non un percorso

# draft
exclusions_beside_path = { $pattern }: accanto a un file, la cartella è un nome (target), non un percorso

# draft
exclusions_builtin_id = skwir. indica le cartelle di Skwir stesso; dai alle tue un nome senza punto

# draft
exclusions_categories = Categorie di esclusione

# draft
exclusions_categories_description = Cosa è cambiato nelle categorie di esclusione di Skwir (un ruolo attivato o disattivato: searches, agents, index) e nelle tue (un nome)

# draft
exclusions_categories_table = le categorie di cartelle sono una tabella, per id

# draft
exclusions_category_name = una tua categoria ha bisogno di un nome

# draft
exclusions_category_not_table = una categoria è una tabella

# draft
exclusions_empty = una cartella ha bisogno di un nome o di un percorso

# draft
exclusions_entries = Esclusioni

# draft
exclusions_entries_description = Cosa è cambiato nelle esclusioni di Skwir (un ruolo deselezionato, una categoria) e nelle tue: un pattern (un nome, un percorso, un glob o regex:…, di file, cartelle o entrambi, con beside), oppure holding (un file contenuto in una cartella), cosa è, la sua categoria e i ruoli da cui è escluso

# draft
exclusions_entries_table = le esclusioni sono una tabella, per id

# draft
exclusions_entry_not_table = una voce è una tabella

# draft
exclusions_holding_alone = beside e names vanno con un pattern, non con holding

# draft
exclusions_holding_name = holding è il nome di un file contenuto in una cartella

# draft
exclusions_id_dot = un tuo id non ha punti: quelli sono di Skwir e dei suoi plugin

# draft
exclusions_legacy_folders = Cartelle private (prima delle esclusioni)

# draft
exclusions_legacy_folders_description = Le cartelle private di 0.3.1, lette come esclusioni tenute lontane dagli agenti

# draft
exclusions_legacy_marker = Cartelle contrassegnate come private (prima delle esclusioni)

# draft
exclusions_legacy_marker_description = L’interruttore di 0.3.1 per .private, letto come l’esclusione che contiene un .private

# draft
exclusions_names_nothing = pattern o holding dice cosa indica

# draft
exclusions_no_category = nessuna categoria { $category }

# draft
exclusions_no_field = nessun campo { $key }

# draft
exclusions_no_id = serve un id

# draft
exclusions_not_switch = { $key }: true o false

# draft
exclusions_path_starts = { $pattern }: un percorso inizia con ~, una variabile ($CARGO_HOME) o è intero ({ $whole })

# draft
exclusions_pattern_or_holding = un pattern o un file che contiene, non entrambi

# draft
exclusions_ref_any_depth = A qualsiasi profondità sotto

# draft
exclusions_ref_beside = Solo accanto a

# draft
exclusions_ref_beside_note = Un nome o il pattern di un nome, mai un percorso.

# draft
exclusions_ref_by_name = Per nome, ovunque

# draft
exclusions_ref_by_path = Per percorso

# draft
exclusions_ref_cache_mark = Il contrassegno di una cache (firmato)

# draft
exclusions_ref_either = Uno dei due

# draft
exclusions_ref_file_held = Un file che contiene

# draft
exclusions_ref_from_home = Una, a partire dalla tua cartella personale

# draft
exclusions_ref_from_root = Una, a partire dalla radice

# draft
exclusions_ref_from_variable = A partire da una variabile

# draft
exclusions_ref_holding = Che contiene

# draft
exclusions_ref_holding_named = Qualsiasi cartella che contiene un file con quel nome

# draft
exclusions_ref_holding_note = Il nome di un file: la cartella che lo contiene è esclusa, ovunque si trovi.

# draft
exclusions_ref_like_beside = Solo accanto a uno con un nome simile

# draft
exclusions_ref_name_beside = Un nome accanto

# draft
exclusions_ref_named_like = Qualsiasi con un nome simile

# draft
exclusions_ref_one = Un carattere

# draft
exclusions_ref_one_of = Uno di questi

# draft
exclusions_ref_path_like = Quelle il cui percorso è simile

# draft
exclusions_ref_path_note = Un percorso inizia da ~, da una variabile o da una radice ({ $whole }); tutto il resto è un nome.

# draft
exclusions_ref_pattern = Nome o percorso

# draft
exclusions_ref_regex = Un’espressione regolare, ~ per la cartella personale

# draft
exclusions_ref_regex_note = Dopo regex:, sul percorso scritto con ~ per la tua cartella personale.

# draft
exclusions_ref_signs = Simboli di un nome o di un percorso

# draft
exclusions_ref_so_named = Qualsiasi con quel nome

# draft
exclusions_ref_so_named_beside = Solo dove accanto c’è un file o una cartella con quel nome

# draft
exclusions_ref_star = Qualsiasi carattere, all’interno di un nome

# draft
exclusions_ref_stars = Qualsiasi cartella, in un percorso

# draft
exclusions_ref_starts_so = Qualsiasi il cui nome inizia così

# draft
exclusions_ref_variable_unset = A partire da una variabile, o da ciò che la sostituisce se non è impostata

# draft
exclusions_ref_whole_path = Sull’intero percorso

# draft
exclusions_refused = { $at }: { $why }

# draft
exclusions_said_beside = { $what } accanto a { $sibling }

# draft
exclusions_said_holding = qualsiasi cartella che contiene { $file }

# draft
exclusions_said_like = { $names ->
        [files] qualsiasi file
        [folders] qualsiasi cartella
       *[both] qualsiasi file o cartella
    } il cui nome è simile a { $pattern }

# draft
exclusions_said_like_beside = { $names ->
        [files] qualsiasi file
        [folders] qualsiasi cartella
       *[both] qualsiasi file o cartella
    } il cui nome è simile a { $pattern } accanto a { $sibling }

# draft
exclusions_said_matching = { $names ->
        [files] i file
        [folders] le cartelle
       *[both] i file e le cartelle
    } corrispondenti a { $pattern }

# draft
exclusions_said_name = { $names ->
        [files] qualsiasi file
        [folders] qualsiasi cartella
       *[both] qualsiasi file o cartella
    } di nome { $pattern }

# draft
exclusions_said_name_beside = { $names ->
        [files] qualsiasi file
        [folders] qualsiasi cartella
       *[both] qualsiasi file o cartella
    } di nome { $pattern } accanto a { $sibling }

# draft
exclusions_said_now = { $pattern }, ora { $path }

# draft
exclusions_said_path_like = { $names ->
        [files] i file
        [folders] le cartelle
       *[both] i file e le cartelle
    } simili a { $pattern }

# draft
exclusions_said_unset = { $pattern }, con { $variables } non impostata: nessuna

# draft
exclusions_shell_variables = Leggi le variabili della tua shell

# draft
exclusions_shell_variables_description = Leggi dalla tua shell di login anche variabili come CARGO_HOME, dove un percorso parte da una di esse.

# draft
exclusions_shell_variables_description_more = Uno Skwir avviato dal tuo desktop non vede ciò che .zshrc o config.fish esportano. La shell viene interrogata in background quando i suoi file cambiano, e vengono conservate solo le variabili che questi percorsi nominano.

# draft
exclusions_unreadable = { $count ->
        [1] Un’esclusione non può essere letta, quindi non si applica: { $first }
       *[other] { $count } esclusioni non possono essere lette, quindi non si applicano. La prima: { $first }
    }

# draft
exclusions_what_also = { $what } (un altro della sua lista)

# draft
exclusions_what_before = { $what } (dove si trovava prima dello spostamento)

# draft
exclusions_what_cache_home = La tua cartella delle cache (XDG_CACHE_HOME)

# draft
exclusions_what_legacy = Privata (dall’elenco di 0.3.1)

# draft
exec_being_redone = { $title } è già in corso di ripetizione

# draft
exec_being_undone = { $title }: annullamento già in corso

# draft
exec_cannot_be_undone = { $title } non può essere annullato

# draft
exec_chain_cancelled = { $title }: annullato

# draft
exec_chain_did_not_run = { $title }: non eseguito

# draft
exec_chain_stopped = { $title }: interrotto: { $error }

# draft
exec_gone_from_history_redo = quell’azione non è più nella cronologia: niente da ripetere

# draft
exec_gone_from_history_undo = quell’azione non è più nella cronologia: niente da annullare

# draft
exec_not_undone = { $title }: non annullato: { $why }

# draft
exec_not_undone_nothing = { $title } non è annullato: niente da ripetere

# draft
exec_nothing_to_redo = niente da ripetere

# draft
exec_nothing_to_undo = niente da annullare

# draft
exec_redo = ripeti

# draft
exec_redo_held = { $title } non può essere ripetuto da solo: { $other }, annullato dopo, ha toccato ciò che aveva fatto; ripeti prima quello

# draft
exec_redo_held_for_good = { $title } non può essere ripetuto: { $other }, annullato dopo, ha toccato ciò che aveva fatto e non può essere ripetuto

# draft
exec_that_not_undone = quell’azione non è stata annullata: { $why }

# draft
exec_undo = annulla

# draft
exec_undo_held = { $title } non può essere annullato da solo: { $other }, richiesto dopo, ha toccato ciò che aveva fatto; annulla prima quello

# draft
exec_undo_held_for_good = { $title } non può essere annullato: { $other }, richiesto dopo, ha toccato ciò che aveva fatto e non può essere annullato

# draft
exec_undo_held_running = { $title } non può essere annullato da solo: { $other }, richiesto dopo e in esecuzione ora, sta cambiando ciò che aveva fatto; annulla prima quello

# draft
exec_undo_held_waiting = { $title } non può essere annullato da solo: { $other }, richiesto dopo e in attesa di essere eseguito, cambierà ciò che aveva fatto; annulla prima quello

# draft
exec_undone_already = { $title } è già annullato

# draft
folders_not_private = { $path } non è riservata al solo utente

# draft
form_needed = { $field } è obbligatorio

# draft
form_one_of_choices = { $field } deve essere una delle scelte previste

# draft
form_whole_number = { $field } è un numero intero da { $min } a { $max }

# draft
fs_folder_gone = Questa cartella è stata eliminata o non è più disponibile.

# draft
fs_not_a_path = non è un percorso: { $location }

# draft
fs_not_regular = non è un file regolare

# draft
fs_server_no_xattrs = questo server non conserva attributi estesi

# draft
fs_size_bytes = { $n } B

# draft
fs_size_gb = { $n } GB

# draft
fs_size_gib = { $n } GiB

# draft
fs_size_kb = { $n } kB

# draft
fs_size_kib = { $n } KiB

# draft
fs_size_mb = { $n } MB

# draft
fs_size_mib = { $n } MiB

# draft
fs_size_pb = { $n } PB

# draft
fs_size_pib = { $n } PiB

# draft
fs_size_tb = { $n } TB

# draft
fs_size_tib = { $n } TiB

# draft
fs_sync_not_yet = La scrittura completa su un’unità non è ancora disponibile su { $system }

# draft
fs_system_no_hidden = { $system } non prevede il contrassegno di nascosto

# draft
fs_system_no_xattrs = { $system } non conserva attributi estesi

# draft
fs_taken = è in corso lo spostamento o la rimozione

# draft
fs_too_large = file troppo grande

# draft
fs_volume_no_hidden = il suo volume non prevede il contrassegno di nascosto

# draft
inspection_mesh_colours = la mesh ha { $colours ->
        [one] { $colours } colore
       *[other] { $colours } colori
    } per { $positions ->
        [one] { $positions } posizione
       *[other] { $positions } posizioni
    }

# draft
inspection_mesh_empty = la mesh non ha triangoli

# draft
inspection_mesh_index = l’indice { $index } della mesh non corrisponde ad alcuna posizione (ce ne sono { $count })

# draft
inspection_mesh_infinite = la mesh ha una posizione non finita

# draft
inspection_mesh_partial = gli indici della mesh non formano triangoli interi

# draft
inspection_notices = { $before } · { $after }

# draft
io_bad_name = il nome è troppo lungo o non è consentito lì

# draft
io_busy = è in uso

# draft
io_denied = permesso negato

# draft
io_full = l’unità è piena

# draft
io_gone = non esiste più

# draft
io_is_folder = è una cartella

# draft
io_not_empty = la cartella non è vuota

# draft
io_not_folder = una parte del suo percorso non è una cartella

# draft
io_quota = la quota disco è esaurita

# draft
io_read_only = l’unità è di sola lettura

# draft
io_share_gone = la condivisione di rete non è più disponibile

# draft
io_taken = esiste già qualcosa con quel nome

# draft
io_timed_out = l’unità non ha risposto in tempo

# draft
io_too_large = il file è troppo grande per quell’unità

# draft
keymap_add_shortcut = Aggiungi scorciatoia

# draft
keymap_args = Argomenti

# draft
keymap_args_description = Gli argomenti del comando, come oggetto JSON

# draft
keymap_args_not_json = gli argomenti di { $keys } non sono JSON: { $error }

# draft
keymap_args_object = gli argomenti di { $keys } sono un oggetto: { $example }

# draft
keymap_command = Comando

# draft
keymap_command_description = Ciò che eseguono i tasti, per nome come sono registrati i comandi della tavolozza: mkdir, ui.jump, tab.new. Se vuoto, libera i tasti

# draft
keymap_keys = Tasti

# draft
keymap_keys_description = Un tasto con i suoi modificatori, o più tasti uno dopo l’altro: ctrl-shift-n, g h, shift-g

# draft
keymap_needs_keys = una scorciatoia richiede i suoi tasti

# draft
keymap_no_key = { $stroke } non indica alcun tasto: scrivi ctrl-- per Ctrl e il tasto meno

# draft
keymap_no_modifier = { $modifier } in { $stroke } non è un modificatore: usa ctrl, alt, shift o super

# draft
keymap_refused = { $keys }: { $error }

# draft
keymap_shortcuts = Le tue scorciatoie

# draft
keymap_shortcuts_description = Tasti che assegni, oltre a quelli impostati da Skwir e dai suoi plugin.

# draft
keymap_shortcuts_description_more = Lascia vuoto il comando per liberare un tasto.

# draft
keymap_unknown_command = { $keys } esegue { $command }, che non corrisponde ad alcun comando (il suo plugin è attivo?)

# draft
keymap_where = Dove

# draft
keymap_where_description = Dove funzionano i tasti: vuoto per ovunque, pane per l’elenco dei file, con condizioni come selection, single, dir, file, folder

# draft
name_quote_close = »

# draft
name_quote_open = «

# draft
notes_agree = allinea le annotazioni di { $name }

# draft
notes_agree_command = Allinea annotazioni

# draft
notes_agree_description = Scrive le annotazioni delle voci indicate, o della selezione, come lette da ogni archivio, in ogni archivio che le conserva, così gli archivi che differiscono tornano allineati (piano)

# draft
notes_agree_title = allinea le annotazioni

# draft
notes_agreed = allineate le annotazioni di { $name }

# draft
notes_agreed_already = le annotazioni di { $name } erano già allineate

# draft
notes_and = { $first } e { $then }

# draft
notes_changed_since = le annotazioni di { $what } sono cambiate nel frattempo, altrove

# draft
notes_comment_command = Commenta

# draft
notes_comment_description = Imposta il commento delle voci indicate, o della selezione, oppure lo rimuove con un testo vuoto (piano)

# draft
notes_comment_title = imposta il commento

# draft
notes_commented = impostato il commento di { $name }

# draft
notes_commented_already = { $name } aveva già quel commento

# draft
notes_gone = { $name } non esiste più

# draft
notes_kept_nowhere = { $path }: il suo volume non conserva annotazioni e nessun archivio è impostato per conservarle accanto al file

# draft
notes_no_folder = { $path } non ha una cartella

# draft
notes_no_stores = nessun archivio di annotazioni su cui scrivere

# draft
notes_put_back = ripristina le annotazioni di { $name }

# draft
notes_put_back_already = le annotazioni di { $name } erano già ripristinate

# draft
notes_put_back_done = ripristinate le annotazioni di { $name }

# draft
notes_put_back_failed = ripristina le annotazioni di { $path }

# draft
notes_rate = valuta { $name } con { $stars }

# draft
notes_rate_command = Valuta

# draft
notes_rate_description = Valuta le voci indicate, o la selezione, da 1 a 5 stelle, oppure ne rimuove la valutazione con 0 (piano)

# draft
notes_rate_none_title = rimuovi la valutazione

# draft
notes_rate_title = valuta con { $stars }

# draft
notes_rated = valutazione di { $name } impostata a { $stars }

# draft
notes_rated_already = { $name } aveva già la valutazione { $stars }

# draft
notes_rating_range = una valutazione va da 1 a { $max }

# draft
notes_rating_stars = una valutazione va da 1 a 5 stelle

# draft
notes_refused = { $path }: { $why }

# draft
notes_rename_tag = rinomina l’etichetta { $tag } in { $to } su { $name }

# draft
notes_renamed_tag = etichetta { $tag } rinominata in { $to } su { $name }

# draft
notes_renamed_tag_already = l’etichetta { $tag } si chiamava già { $to } su { $name }

# draft
notes_set_comment = imposta il commento di { $name }

# draft
notes_stars = { $whole ->
        [one] { $whole } stella
       *[other] { $whole } stelle
    }

# draft
notes_stars_half = { $whole ->
        [0] ½ stella
       *[other] { $whole }½ stelle
    }

# draft
notes_tag = aggiungi le etichette { $tags } a { $name }

# draft
notes_tagged = aggiunte le etichette { $tags } a { $name }

# draft
notes_tagged_already = { $name } aveva già le etichette { $tags }

# draft
notes_take_tag = togli l’etichetta { $tag } da { $name }

# draft
notes_take_tags = { $count ->
        [1] togli l’etichetta { $tags } da { $name }
       *[other] togli le etichette { $tags } da { $name }
    }

# draft
notes_took_tag = etichetta { $tag } tolta da { $name }

# draft
notes_took_tag_already = l’etichetta { $tag } era già assente da { $name }

# draft
notes_took_tags = { $count ->
        [1] etichetta { $tags } tolta da { $name }
       *[other] etichette { $tags } tolte da { $name }
    }

# draft
notes_took_tags_already = { $count ->
        [1] l’etichetta { $tags } era già assente da { $name }
       *[other] le etichette { $tags } erano già assenti da { $name }
    }

# draft
notes_uncomment = rimuovi il commento di { $name }

# draft
notes_uncomment_title = rimuovi il commento

# draft
notes_uncommented = rimosso il commento di { $name }

# draft
notes_uncommented_already = { $name } era già senza commento

# draft
notes_unrate = rimuovi la valutazione di { $name }

# draft
notes_unrated = rimossa la valutazione di { $name }

# draft
notes_unrated_already = { $name } era già senza valutazione

# draft
notes_untag = rimuovi le etichette di { $name }

# draft
notes_untagged = rimosse le etichette di { $name }

# draft
notes_untagged_already = { $name } era già senza etichette

# draft
notes_write_create = Crea

# draft
notes_write_failed = scrivi le annotazioni di { $path }

# draft
notes_write_fallback = Dove mancano gli attributi

# draft
notes_write_off = Disattivato

# draft
notes_write_update = Aggiorna quelle esistenti

# draft
op_all_in_trash = tutto si trova nel cestino

# draft
op_already_there = { $name } esiste già

# draft
op_attribute_name_not_utf8 = un attributo di { $name } ha un nome non UTF-8, che l’interfaccia degli attributi della copia non può preservare

# draft
op_attributes_not_kept = conserva etichette e altri attributi durante la copia: { $folder } non conserva attributi e nulla è impostato per conservarli accanto ai file; disattiva { $setting } nelle Impostazioni per copiare senza di essi

# draft
op_attributes_refused = conserva etichette e altri attributi durante la copia: verifica che la destinazione conservi gli attributi estesi e ti consenta di scriverli, oppure disattiva { $setting } nelle Impostazioni

# draft
op_attributes_unread = non è stato possibile leggere le etichette e gli attributi di { $name }

# draft
op_batch_cancelled = annullata

# draft
op_batch_copying = { $transfer }: copia di { $count ->
        [one] { $count } voce
       *[other] { $count } voci
    }

# draft
op_batch_link = { $path } è un collegamento

# draft
op_batch_read_short = { $read ->
        [one] { $path }: si è potuto leggere solo { $read } byte su { $bytes }
       *[other] { $path }: si è potuto leggere solo { $read } byte su { $bytes }
    }

# draft
op_change_copy = copia { $from } -> { $to }

# draft
op_change_create = crea { $path }

# draft
op_change_delete = elimina { $path }

# draft
op_change_link = collega { $link } -> { $target }

# draft
op_change_metadata = imposta { $field } su { $path }

# draft
op_change_mkdir = mkdir { $path }

# draft
op_change_move = sposta { $from } -> { $to }

# draft
op_change_remove = rimuovi { $path }

# draft
op_change_rename = rinomina { $from } -> { $to }

# draft
op_change_restore = ripristina { $path }

# draft
op_change_swap = scambia { $a } <-> { $b }

# draft
op_change_trash = sposta nel cestino { $path }

# draft
op_changed_after_copy = { $name } è cambiato dopo la copia in { $folder }: l’originale rimane dov’era

# draft
op_copied_into_itself = una cartella non può essere copiata in se stessa

# draft
op_copied_onto_itself = verrebbe copiato su se stesso

# draft
op_copy_stopped = la copia si è interrotta

# draft
op_copy_title = { $count ->
        [1] copia { $name } in { $folder }
       *[other] copia { $count } elementi in { $folder }
    }

# draft
op_copy_went_missing = la copia { $name } è scomparsa appena creata

# draft
op_device_not_copied = { $name } è un dispositivo e non può essere copiato

# draft
op_entry_gone = { $name } non esiste più

# draft
op_entry_replaced = { $name } ora è un altro file

# draft
op_folder_attribute_not_restored = non è stato possibile ripristinare l’attributo { $attribute } della cartella

# draft
op_folder_attributes_differ = { $name } esiste con attributi di cartella diversi

# draft
op_folder_not_made = non è stato possibile creare la cartella { $name }

# draft
op_folder_not_removed = non è stato possibile rimuovere la cartella { $name }

# draft
op_folder_path_not_made = non è stato possibile creare la cartella { $path }

# draft
op_foreign_folder = questa cartella appartiene a un altro utente e i suoi permessi impedirebbero ad Annulla di ripristinarne il contenuto

# draft
op_holds_mount_point = { $name } contiene { $point }, dove è montata un’unità o una cartella remota: espelline prima il contenuto

# draft
op_holes_not_yet = La copia di un file con buchi (un file sparso) non è ancora disponibile su { $system }

# draft
op_identity_unread = non è stato possibile leggere l’identità della voce

# draft
op_in_trash_already = { $name } è già nel cestino

# draft
op_in_trash_restore_first = { $name } è nel cestino: ripristinalo prima

# draft
op_left_after_removal = { $more ->
        [0] rimane { $path }
       *[other] rimangono { $path } e altri { $more }
    }

# draft
op_left_parked = { $name } rimane con il nome { $parked }

# draft
op_link_elsewhere = { $name } ora porta altrove

# draft
op_link_title = { $count ->
        [1] collega { $name } in { $folder }
       *[other] collega { $count } elementi in { $folder }
    }

# draft
op_made_beside_other = { $companion } è accanto a { $name } e non è stato creato con esso: Annulla li lascia dove sono

# draft
op_made_changed = { $name } è cambiato dopo la creazione: Annulla lo lascia dov’è

# draft
op_made_companion_changed = { $companion } è cambiato dopo la creazione: Annulla lo lascia dov’è, insieme a { $name }

# draft
op_mount_point = { $name } è dove è montata un’unità o una cartella remota: espellila invece di eliminarla

# draft
op_move_original_changed = { $name } è rimasto dov’era, cambiato dopo lo spostamento: Annulla lo lascia dov’è, insieme alla sua copia

# draft
op_move_title = { $count ->
        [1] sposta { $name } in { $folder }
       *[other] sposta { $count } elementi in { $folder }
    }

# draft
op_moved_into_itself = una cartella non può essere spostata in se stessa

# draft
op_moved_stays = ciò che è stato spostato rimane dov’era

# draft
op_moves_nothing = questo dispositivo (una fotocamera) non può spostare file tra le sue cartelle: lì si possono solo fare copie

# draft
op_name_character = un nome non può contenere { $character }

# draft
op_name_control = un nome non può contenere un carattere di controllo

# draft
op_name_device = { $name } è il nome di un dispositivo su Windows

# draft
op_name_dots = { $name } non può essere usato come nome

# draft
op_name_empty = un nome non può essere vuoto

# draft
op_name_end = un nome non può terminare con un punto o uno spazio

# draft
op_name_nul = un nome non può contenere il carattere NUL

# draft
op_name_slash = un nome non può contenere «/»

# draft
op_no_folder_to_go_in = la cartella di destinazione non esiste

# draft
op_no_longer_a_file = { $name } non è più un file

# draft
op_no_longer_a_link = { $name } non è più un collegamento

# draft
op_no_longer_empty = { $name } non è più vuoto

# draft
op_no_longer_in_trash = { $name } non è più nel cestino

# draft
op_no_longer_there = { $name } non è più lì

# draft
op_no_record = { $name } non è il record del cestino di un elemento

# draft
op_no_room = Spazio insufficiente in { $folder }: servono { $needs }, liberi { $free }

# draft
op_no_trash_s = { $name } non corrisponde ad alcun elemento di un cestino

# draft
op_no_trash_there = { $name } si trova in una cartella di rete montata da un programma, che non ha un cestino

# draft
op_not_a_file = { $name } non è un file

# draft
op_not_a_folder = { $name } non è una cartella

# draft
op_not_all_deleted = non è stato possibile eliminare tutto { $name }

# draft
op_not_all_removed = non è stato possibile rimuovere tutto { $name }

# draft
op_not_copied = non è stato possibile copiare { $name }

# draft
op_not_empty = { $name } non è vuota

# draft
op_not_in_trash = { $name } non è nel cestino

# draft
op_not_linked = non è stato possibile collegare { $name } in { $folder }

# draft
op_not_made = non è stato possibile creare { $name }

# draft
op_not_moved = non è stato possibile spostare { $name } in { $folder }

# draft
op_not_moved_as = non è stato possibile spostare { $name } in { $folder } come { $new }

# draft
op_not_placed = non è stato possibile collocare { $name } in { $folder }

# draft
op_not_removed = non è stato possibile rimuovere { $name }

# draft
op_not_renamed = non è stato possibile rinominare { $name } in { $new }

# draft
op_not_restored = non è stato possibile ripristinare { $name }

# draft
op_not_served = il trasferimento non gestisce queste cartelle

# draft
op_not_swapped = non è stato possibile scambiare i nomi di { $a } e { $b }

# draft
op_not_trashed = non è stato possibile spostare { $name } nel cestino

# draft
op_not_written_out = non è stato possibile scrivere la copia sull’unità

# draft
op_original_stays = { $name } è stato copiato per intero in { $folder }, ma non è stato possibile rimuovere tutto l’originale

# draft
op_original_stays_path = { $path } è stato copiato per intero in { $folder }, ma non è stato possibile rimuovere tutto l’originale

# draft
op_panicked = si è interrotto in modo imprevisto

# draft
op_part_left = una parte della copia rimane in { $path } ({ $why })

# draft
op_partly_restored_folder = la cartella ripristinata in parte rimane in { $path }

# draft
op_path_climbs = { $path } contiene «..»

# draft
op_path_missing = { $path } non esiste

# draft
op_path_no_name = { $path } non ha un nome

# draft
op_pipe_not_made = non è stato possibile creare la pipe { $name }

# draft
op_place_taken_in_trash = un altro { $name } ha preso il suo posto nel cestino

# draft
op_plan_steps_kind = un piano elenca ciascuno dei suoi passaggi: `{ $kind }` non lo è

# draft
op_read_short = { $read ->
        [one] si è potuto leggere solo { $read } byte su { $bytes }
       *[other] si è potuto leggere solo { $read } byte su { $bytes }
    }

# draft
op_record_unread = non è stato possibile leggere il record del cestino { $name }

# draft
op_record_unwritten = non è stato possibile scrivere il record del cestino { $name }

# draft
op_redo_title = ripeti { $title }

# draft
op_refused_on_close = la sua unità o il suo server l’ha rifiutato alla chiusura

# draft
op_rename_taken = { $name } esiste già

# draft
op_renames_duplicate = più elementi avrebbero il nome { $path }

# draft
op_socket_not_copied = { $name } è un socket e non può essere copiato

# draft
op_spellings_undistinguished = non è stato possibile distinguere le grafie dei collegamenti reali di questo file

# draft
op_stopped_being_read = la lettura del file si è interrotta

# draft
op_swap_windows = Windows non può scambiare due nomi in un solo passaggio

# draft
op_there_again = { $name } esiste di nuovo: rinominalo o spostalo prima

# draft
op_there_by_earlier_step = { $name } esiste già, a causa di un passaggio precedente

# draft
op_there_not_a_folder = { $name } esiste e non è una cartella

# draft
op_time_not_given = non è stato possibile assegnare la sua data a { $path }

# draft
op_trash_copy_left = una sua copia rimane in { $path } ({ $why })

# draft
op_trash_copy_not_removed = non è stato possibile rimuovere tutta la copia di { $name } nel cestino

# draft
op_trash_folder_gone = la cartella del cestino in cui si trovava { $name } non esiste più

# draft
op_trash_unread = non è stato possibile leggere il cestino: { $error }

# draft
op_trash_unread_context = non è stato possibile leggere il cestino

# draft
op_undo_title = annulla { $title }

# draft
op_unknown_kind = tipo di operazione sconosciuto `{ $kind }`

# draft
op_unreadable = non è stato possibile leggere { $name }: { $error }

# draft
op_went_missing_restored = { $name } è scomparso mentre veniva ripristinato

# draft
op_would_stay_in_trash = { $name } rimarrebbe nel cestino

# draft
op_write_out_stopped = la scrittura sull’unità si è interrotta in modo imprevisto

# draft
orders_dialog_order_state = L’ordinamento con cui le finestre che rispondono ai dialoghi di altri programmi elencano i file, l’ultimo lasciato (ADR-145): { $fields }

# draft
orders_kept = Gli ordinamenti che le posizioni ricordano (ADR-132), ciascuno { $fields }: mostrati così in ogni riquadro

# draft
orders_kept_dialogs = Gli ordinamenti che le posizioni ricordano nelle finestre che rispondono ai dialoghi di altri programmi (ADR-145), ciascuno { $fields }: separati da quelli ricordati dalle finestre dell’utente

# draft
orders_not_saved = L’ordinamento non è stato ricordato: { $error }

# draft
os_path_not_absolute = { $path } non è un percorso assoluto

# draft
permissions_names_private = indica una cartella riservata, che Skwir tiene lontana dagli agenti (Impostazioni, Privacy)

# draft
permissions_network = usa la rete

# draft
permissions_programs = avvia programmi

# draft
permissions_reaches_machine = cercare { $path } farebbe raggiungere al sistema un’altra macchina, cosa che un agente non può fare senza richiesta: l’utente la apre prima in Skwir

# draft
permissions_read = legge il contenuto dei file

# draft
permissions_write = modifica i file

# draft
playing_not_answering = il lettore non risponde

# draft
playing_nothing = nessuna riproduzione in corso

# draft
playing_now = In riproduzione

# draft
playing_now_description = La traccia in riproduzione, il suo stato e a che punto si trova, oppure null

# draft
playing_request = Controlla il lettore

# draft
playing_request_description = Chiede a ciò che è in riproduzione di riprodurre, mettere in pausa, fermarsi, passare alla traccia successiva o precedente, spostarsi, cambiare volume o velocità, scegliere l’audio o i sottotitoli di un film o mostrarlo a schermo intero; viene eseguito alla successiva attivazione del lettore

# draft
playing_senseless = questa richiesta non ha senso

# draft
plugins_browsing = Navigazione e ricerca

# draft
plugins_desktop = Integrazione con il desktop

# draft
plugins_files = Lavoro con i file

# draft
plugins_list = Elenca plugin

# draft
plugins_list_description = Ogni plugin con titolo, descrizione e stato di attivazione

# draft
plugins_not_saved = La scelta dei plugin non è stata salvata: { $error }

# draft
plugins_previews = Anteprime e miniature

# draft
plugins_set_enabled = Attiva o disattiva un plugin

# draft
plugins_set_enabled_description = Attiva o disattiva un plugin durante l’esecuzione e ricorda la scelta; con la finestra aperta, il completamento è segnalato da un risveglio e da `plugins.list`

# draft
plugins_unknown = plugin sconosciuto: { $id }

# draft
preview_changed = { $path } è cambiato mentre veniva mostrato

# draft
print_no_page = nessuna pagina indicata

# draft
print_not_a_page = { $text } non è una pagina

# draft
print_pages_backwards = { $range } procede all’indietro

# draft
print_pages_from_one = le pagine si contano da 1

# draft
print_paper_legal = Legal

# draft
print_paper_letter = Letter

# draft
properties_content = Contenuto

# draft
properties_general = Generale

# draft
properties_location = Posizione

# draft
properties_more = Altro

# draft
properties_notes = Annotazioni

# draft
properties_permissions = Permessi

# draft
properties_selection = Selezione

# draft
properties_shared_permissions = Permessi condivisi

# draft
properties_source = Origine

# draft
properties_times = Date

# draft
properties_volume = Volume

# draft
reference_any = Qualsiasi carattere

# draft
reference_any_times = Ciò che precede, un numero qualsiasi di volte, anche nessuna

# draft
reference_boundary = Dove una parola inizia o finisce

# draft
reference_case = Da qui in poi, maiuscole e minuscole sono equivalenti

# draft
reference_digit = Una cifra

# draft
reference_either = L’uno o l’altro

# draft
reference_end = La fine

# draft
reference_escaped = Il simbolo stesso: \. \( \[ \* \+ \? \$ \^ \|

# draft
reference_group = Un gruppo, che un simbolo dopo di esso considera per intero: (ab)+

# draft
reference_lazy = Il minor numero di volte possibile (anche *?)

# draft
reference_maybe = Ciò che precede, oppure niente

# draft
reference_named_group = Un gruppo con un nome: year

# draft
reference_none_of = Uno qualsiasi tranne questi

# draft
reference_once_or_more = Ciò che precede, una o più volte

# draft
reference_one_of = Uno di questi

# draft
reference_pattern = Modello

# draft
reference_pattern_note = Niente lookaround né backreference: (?=…) e \1 non vengono letti.

# draft
reference_range = Uno di questo intervallo

# draft
reference_space = Qualsiasi spazio: uno spazio, una tabulazione, una fine riga, qualunque altro

# draft
reference_start = L’inizio

# draft
reference_times = Ciò che precede, da 2 a 4 volte

# draft
reference_word_char = Una lettera, una cifra o _

# draft
registry_compress = Comprimi

# draft
registry_group_bookmarks = Segnalibri

# draft
registry_group_devices = Dispositivi

# draft
registry_group_network = Rete

# draft
registry_group_places = Posizioni

# draft
registry_group_recent_folders = Cartelle recenti

# draft
registry_group_saved = Salvati

# draft
registry_group_skwir_tabs = Schede di Skwir

# draft
registry_group_tabs = Schede

# draft
registry_group_tags = Etichette

# draft
registry_open_folder_with = Apri cartella con

# draft
registry_open_with = Apri con

# draft
registry_reopen_not_below = { $path } non indica alcuna cartella all’interno di quella riaperta

# draft
registry_tags = Etichette

# draft
settings_apart_own = { $file ->
        [state] lo stato della finestra, non un’impostazione: Skwir legge il proprio, nel suo file di stato, mai questo
       *[data] ciò che crei in Skwir (ricerche salvate, etichette fissate, colori delle etichette), non un’impostazione: Skwir legge il proprio, nel suo file di dati, mai questo
    }

# draft
settings_apart_until = { $file ->
        [state] lo stato della finestra, non un’impostazione: letto da qui solo finché Skwir non tiene il proprio, nel suo file di stato
       *[data] ciò che crei in Skwir (ricerche salvate, etichette fissate, colori delle etichette), non un’impostazione: letto da qui solo finché Skwir non tiene il proprio, nel suo file di dati
    }

# draft
settings_apply = Salva impostazioni

# draft
settings_apply_description = Modifica le preferenze, ogni chiave con il testo che la sua riga in Impostazioni accetta; vengono salvate tutte o nessuna

# draft
settings_check = Controlla impostazioni

# draft
settings_check_description = Controlla un file di impostazioni, o impostazioni date come tabella, rispetto alle impostazioni dichiarate da questa versione di Skwir: fallisce indicando cosa non va (un valore che un’impostazione non accetta, una chiave che nessuna impostazione ha) e segnala le chiavi dei proprietari che non conosce

# draft
settings_check_invalid = { $path } non è valido: { $problem }

# draft
settings_check_not_settings = non sono impostazioni: { $error }

# draft
settings_check_one = indica un file o delle impostazioni, uno dei due

# draft
settings_check_unreadable = { $path } non può essere letto: { $why }

# draft
settings_choice_now = { $choice } ({ $now })

# draft
settings_choose_one = scegli uno tra { $choices }

# draft
settings_close_split = Riquadro da chiudere

# draft
settings_close_split_active = Attivo

# draft
settings_close_split_description = Quale riquadro viene chiuso quando chiudi la vista divisa con Dividi o F3.

# draft
settings_close_split_description_more = Altro mantiene il riquadro in cui ti trovi. Attivo lo chiude e mantiene l’altro. Destro chiude sempre il riquadro di destra, o quello in basso quando i riquadri sono impilati.

# draft
settings_close_split_other = Altro

# draft
settings_close_split_right = Destro

# draft
settings_drop_asks = Chiedi cosa fare con i file rilasciati

# draft
settings_drop_asks_description = I file rilasciati in una cartella senza alcun tasto premuto aprono un menu: sposta, copia o collega lì.

# draft
settings_drop_asks_description_more = Disattiva per spostarli all’interno di un’unità e copiarli su un’altra. In entrambi i casi Maiusc sposta, Ctrl copia e Ctrl+Maiusc collega.

# draft
settings_expected_boolean = atteso true o false

# draft
settings_expected_finite = atteso un numero finito

# draft
settings_expected_json = atteso JSON: { $error }

# draft
settings_expected_kind = atteso { $want ->
        [integer] un numero intero
        [float] un numero
        [boolean] true o false
        [datetime] una data
        [array] un elenco
        [table] una tabella
       *[string] testo
    }, non { $got ->
        [integer] un numero intero
        [float] un numero
        [boolean] true o false
        [datetime] una data
        [array] un elenco
        [table] una tabella
       *[string] testo
    }

# draft
settings_expected_number = atteso un numero

# draft
settings_expected_records = atteso un elenco di record: { $error }

# draft
settings_expected_whole_number = atteso un numero intero

# draft
settings_field_name_value_pairs = { $title } è un elenco di coppie nome e valore

# draft
settings_field_needed = { $title } è obbligatorio

# draft
settings_field_pairs = { $title } è un elenco di coppie

# draft
settings_field_pairs_text = { $title }: nomi e valori sono testo

# draft
settings_field_range = { $title }: inserisci un numero intero da { $min } a { $max }

# draft
settings_field_secret = { $title } è conservato nel portachiavi, non nelle impostazioni

# draft
settings_field_text = { $title } è testo

# draft
settings_field_value_needs_name = { $title }: un valore richiede un nome

# draft
settings_field_whole_number = { $title } è un numero intero

# draft
settings_file_invalid = { $path } non è valido ({ $problem }): non viene salvato nulla al suo posto finché non lo correggi

# draft
settings_file_not_text = { $path } non è testo

# draft
settings_file_unreadable = { $path } non ha potuto essere letto: { $why }

# draft
settings_folders_first = Cartelle per prime

# draft
settings_folders_first_description = Elenca le cartelle prima dei file, ordinando ciascun gruppo al suo interno.

# draft
settings_folders_first_description_more = Se disattivato, li ordina insieme: per data di modifica, il più recente viene per primo, cartella o file. Per dimensione, le cartelle, che non vengono pesate, si ordinano come se non ne avessero.

# draft
settings_invalid_key = chiave di impostazione non valida

# draft
settings_keep_attributes = Mantieni etichette e attributi durante la copia

# draft
settings_keep_attributes_description = Le copie portano con sé le etichette, la valutazione, il commento e la provenienza del download dell’originale.

# draft
settings_keep_attributes_description_more = Dove la destinazione non conserva attributi, vengono tenuti accanto alla copia, come fa un Mac; la copia fallisce solo quando nulla può conservarli. Disattiva per copiare senza di essi.

# draft
settings_key_not_json = { $key }: atteso JSON: { $error }

# draft
settings_language = Lingua

# draft
settings_language_automatic = Automatica

# draft
settings_language_description = La lingua in cui parla la finestra di Skwir.

# draft
settings_language_description_more = Automatica usa la prima delle lingue del tuo sistema in cui Skwir è tradotto, altrimenti l’inglese; una traduzione ancora in bozza lo dice. Una lingua indicata come incompleta viene usata solo se la scegli, con ciò che manca in inglese. Agenti e script ricevono risposte in inglese qualunque lingua tu scelga.

# draft
settings_language_draft = { $language } — bozza, { $reviewed }% revisionato

# draft
settings_language_incomplete = { $language } — incompleta, { $translated }% tradotto, { $reviewed }% revisionato

# draft
settings_language_pseudo = Pseudo (per test)

# draft
settings_language_pseudo_rtl = Pseudo, da destra a sinistra (per test)

# draft
settings_language_self = Italiano

# draft
settings_last_location = La cartella mostrata quando la finestra è stata chiusa l’ultima volta

# draft
settings_managed = { $key } è gestita dai comandi a cui appartiene

# draft
settings_no_plugin = nessun plugin { $id } qui: uno esterno, o non integrato, potrebbe usarla

# draft
settings_no_such = nessuna impostazione di questo tipo

# draft
settings_not_file_value = non è un valore che un file possa contenere: { $error }

# draft
settings_not_text = non è testo

# draft
settings_other_choice = non una che Skwir conosca qui: potrebbe essere tra { $which }

# draft
settings_owner_unknown = nulla qui dichiara { $owner }: potrebbe farlo un plugin disattivato o esterno

# draft
settings_parent_not_table = { $key }: il genitore non è una tabella

# draft
settings_plugin_table = atteso una tabella: enabled = true o false

# draft
settings_problems = { $first } { $then }

# draft
settings_program_alone = Indica solo il programma, senza argomenti: «bash», non «bash -l»

# draft
settings_range = inserisci un numero intero da { $min } a { $max }

# draft
settings_read_only = { $file ->
        [state] il file di stato è
        [data] il file di dati è
       *[settings] il file delle impostazioni è
    } di sola lettura, quindi la modifica non viene conservata

# draft
settings_read_only_link = il file delle impostazioni è un collegamento a uno che non può essere scritto (quello di Home Manager, ad esempio), quindi la modifica non viene conservata: modificala dove è dichiarata

# draft
settings_read_only_link_apart = { $file ->
        [state] il file di stato è
       *[data] il file di dati è
    } un collegamento a uno che non può essere scritto, quindi la modifica non viene conservata

# draft
settings_record_needs_id = ogni record richiede un id

# draft
settings_records_share_id = due record condividono l’id { $id }

# draft
settings_renamed = ora { $new }: Skwir la sposta lì

# draft
settings_renamed_both = non letta: { $new } è impostata, e Skwir legge quella

# draft
settings_save_did_not_run = il salvataggio delle impostazioni non è stato eseguito

# draft
settings_saving_already = le impostazioni sono già in fase di salvataggio

# draft
settings_section_actions = Azioni

# draft
settings_section_address = Indirizzo

# draft
settings_section_audio = Audio

# draft
settings_section_books = Libri

# draft
settings_section_code = Codice

# draft
settings_section_commands = Comandi

# draft
settings_section_documents = Documenti

# draft
settings_section_drag_and_drop = Trascinamento

# draft
settings_section_git = Git

# draft
settings_section_grammars = Grammatiche

# draft
settings_section_grid = Griglia

# draft
settings_section_language = Lingua

# draft
settings_section_list_columns = Colonne dell’elenco

# draft
settings_section_maps = Mappe

# draft
settings_section_models_3d = Modelli 3D

# draft
settings_section_opening_addresses = Indirizzi di apertura

# draft
settings_section_pages = Pagine

# draft
settings_section_pictures = Immagini

# draft
settings_section_playback = Riproduzione

# draft
settings_section_previews = Anteprime

# draft
settings_section_servers = Server

# draft
settings_section_sizes = Dimensioni

# draft
settings_section_sorting = Ordinamento

# draft
settings_section_split = Vista divisa

# draft
settings_section_spreadsheets = Fogli di calcolo

# draft
settings_section_startup = Avvio

# draft
settings_section_tabs = Schede

# draft
settings_section_tags = Etichette

# draft
settings_section_terminal = Terminale

# draft
settings_section_text = Testo

# draft
settings_section_thumbnails = Miniature

# draft
settings_section_video = Video

# draft
settings_show_companions = Mostra file di accompagnamento

# draft
settings_show_companions_description = Con i file nascosti visibili, elenca anche i file che conservano le annotazioni di un altro file accanto a esso: i file ._ di un Mac, i file sidecar XMP e TagSpaces.

# draft
settings_show_companions_description_more = Proprietà elenca comunque i file di accompagnamento di un file.

# draft
settings_size_units = Unità

# draft
settings_size_units_binary = Binarie

# draft
settings_size_units_decimal = Decimali

# draft
settings_size_units_description = Come vengono contate le dimensioni.

# draft
settings_size_units_description_more = Decimali conta un kilobyte come 1000 byte (kB, MB, GB), come si vendono i dischi e come macOS e GNOME mostrano le dimensioni; Binarie conta 1024 (KiB, MiB, GiB), come fa KDE. Una ricerca legge 10M come basato su 1024 in entrambi i casi, 10MB come 1000 e 10MiB come 1024.

# draft
settings_startup = Apri all’avvio

# draft
settings_startup_description = Cosa apre Skwir quando si avvia senza una cartella indicata sulla riga di comando.

# draft
settings_startup_folder = Cartella di avvio

# draft
settings_startup_folder_choice = Una cartella scelta

# draft
settings_startup_folder_description = Usata quando all’avvio si apre una cartella scelta.

# draft
settings_startup_folder_description_more = Lascia vuoto per la tua cartella personale.

# draft
settings_startup_last = Le finestre e le schede lasciate aperte

# draft
settings_toml_on_line = { $problem } alla riga { $line }

# draft
settings_unit_megabytes = { $n } MB

# draft
settings_unit_percent = { $n }%

# draft
settings_unit_seconds = { $n } s

# draft
settings_unknown = impostazione sconosciuta: { $key }

# draft
settings_unreadable_kept = { $file ->
        [data] Le tue ricerche salvate, le etichette fissate e i colori delle etichette non hanno potuto essere letti ({ $why }), quindi Skwir è partito senza di essi e non salva nulla lì finché { $path } non viene corretto o rimosso ({ $error }).
       *[settings] Le tue impostazioni non hanno potuto essere lette ({ $why }), quindi Skwir è partito con quelle predefinite e non salva nulla lì finché { $path } non viene corretto o rimosso ({ $error }).
    }

# draft
settings_unreadable_set_aside = { $file ->
        [data] Le tue ricerche salvate, le etichette fissate e i colori delle etichette non hanno potuto essere letti ({ $why }), quindi Skwir è partito senza di essi. Il file è stato messo da parte come { $aside }.
       *[settings] Le tue impostazioni non hanno potuto essere lette ({ $why }), quindi Skwir è partito con quelle predefinite. Il file è stato messo da parte come { $aside }.
    }

# draft
settings_unreadable_unsaved = { $file ->
        [data] Le tue ricerche salvate, le etichette fissate e i colori delle etichette non hanno potuto essere letti ({ $why }), quindi Skwir è partito senza di essi e questa volta non salva nulla lì.
       *[settings] Le tue impostazioni non hanno potuto essere lette ({ $why }), quindi Skwir è partito con quelle predefinite e questa volta non salva nulla lì.
    }

# draft
shrink_lossless = Senza perdite

# draft
shrink_note_damaged = è danneggiato, e una copia ridotta perderebbe ciò che un lettore ripara mentre lo mostra

# draft
shrink_note_date_not_kept = La sua data non ha potuto essere conservata, quindi è stata omessa.

# draft
shrink_note_date_part_unread = Una parte della sua data non ha potuto essere letta, quindi è stata omessa.

# draft
shrink_note_exif_unread = I suoi dati EXIF non hanno potuto essere letti, quindi la sua data è stata omessa.

# draft
shrink_note_pixels_as_stored = I suoi pixel sono stati mantenuti come memorizzati, raddrizzati dai suoi dati EXIF come prima.

# draft
shrink_note_pixels_kept = I suoi pixel sono stati mantenuti com’erano: riscritti, risultavano più grandi.

# draft
shrink_note_profile_left_out = Il suo profilo colore è stato omesso: non è adatto ai colori in cui è scritta la copia, che potrebbe apparire diversa.

# draft
shrink_note_protected = è protetto da una password, che una copia ridotta non manterrebbe

# draft
shrink_note_put_back = le sue immagini ridotte apparivano diverse in una copia e sono state rimesse com’erano, il che non l’ha reso più piccolo

# draft
shrink_note_signed = è firmato, e una copia ridotta romperebbe la sua firma

# draft
shrink_note_signed_rights = contiene diritti firmati dal suo autore (compilarlo e salvarlo in Adobe Reader), che una copia ridotta romperebbe

# draft
shrink_note_still_unlike = una copia creata appariva ancora diversa dall’originale con le sue immagini rimesse a posto, quindi nessuna è stata conservata

# draft
shrink_note_unlike = una copia creata appariva diversa dall’originale oltre alle sue immagini, quindi nessuna è stata conservata

# draft
shrink_print = Stampa

# draft
shrink_screen = Schermo

# draft
shrink_small = Piccola

# draft
sort_a_to_z = Dalla A alla Z

# draft
sort_a_to_z_phrase = dalla A alla Z

# draft
sort_changes_first = Modificati per primi

# draft
sort_changes_first_phrase = modificati per primi

# draft
sort_changes_last = Modificati per ultimi

# draft
sort_changes_last_phrase = modificati per ultimi

# draft
sort_highest = Più alta

# draft
sort_highest_first = più alta per prima

# draft
sort_largest = Più grande

# draft
sort_largest_first = più grande per primo

# draft
sort_longest = Più lunga

# draft
sort_longest_first = più lunga per prima

# draft
sort_lowest = Più bassa

# draft
sort_lowest_first = più bassa per prima

# draft
sort_newest = Più recente

# draft
sort_newest_first = più recente per primo

# draft
sort_oldest = Più vecchio

# draft
sort_oldest_first = più vecchio per primo

# draft
sort_phrase_accessed = data di ultimo accesso

# draft
sort_phrase_created = data di creazione

# draft
sort_phrase_deleted = data di eliminazione

# draft
sort_phrase_dimensions = dimensioni

# draft
sort_phrase_duration = durata

# draft
sort_phrase_extension = estensione

# draft
sort_phrase_folder = cartella

# draft
sort_phrase_kind = genere

# draft
sort_phrase_modified = data di modifica

# draft
sort_phrase_name = nome

# draft
sort_phrase_owner = proprietario

# draft
sort_phrase_rating = valutazione

# draft
sort_phrase_size = dimensione

# draft
sort_phrase_tags = etichette

# draft
sort_phrase_taken = data di scatto

# draft
sort_phrase_type = tipo

# draft
sort_phrase_vcs = stato Git

# draft
sort_shortest = Più corta

# draft
sort_shortest_first = più corta per prima

# draft
sort_smallest = Più piccolo

# draft
sort_smallest_first = più piccolo per primo

# draft
sort_title_accessed = Ultimo accesso

# draft
sort_title_created = Creato

# draft
sort_title_deleted = Eliminato

# draft
sort_title_dimensions = Dimensioni

# draft
sort_title_duration = Durata

# draft
sort_title_extension = Estensione

# draft
sort_title_folder = Cartella

# draft
sort_title_kind = Genere

# draft
sort_title_modified = Modificato

# draft
sort_title_name = Nome

# draft
sort_title_owner = Proprietario

# draft
sort_title_rating = Valutazione

# draft
sort_title_size = Dimensione

# draft
sort_title_tags = Etichette

# draft
sort_title_taken = Scattato

# draft
sort_title_type = Tipo

# draft
sort_title_vcs = Git

# draft
sort_z_to_a = Dalla Z alla A

# draft
sort_z_to_a_phrase = dalla Z alla A

# draft
suggest_actions = Azioni

# draft
suggest_complete = Completa

# draft
suggest_go = Vai a

# draft
suggest_help = Cosa accetta il campo

# draft
suggest_history = Indietro

# draft
suggest_reading = Intendevi

# draft
suggest_recent = Ricerche recenti

# draft
suggest_saved = Ricerche salvate

# draft
suggest_search_in = Cerca in

# draft
suggest_syntax = Termini di una ricerca

# draft
suggest_tabs = Schede aperte

# draft
suggest_terms = Aggiungi un termine

# draft
sys_bad_folder_permissions = permessi della cartella non validi

# draft
sys_group_not_yet = Cambiare il gruppo di un file non è ancora disponibile su { $system }

# draft
sys_no_proc = /proc del sistema non è montato, e serve per cambiare l’accesso

# draft
sys_now_a_link = ora è un collegamento

# draft
sys_now_another_file = ora è un altro file

# draft
sys_path_not_utf8 = { $path } non è un percorso UTF-8

# draft
sys_pipe_not_yet = Copiare una pipe non è ancora disponibile su { $system }

# draft
sys_swap_windows = Windows non può scambiare due nomi in un solo passaggio

# draft
sys_trash_list_not_yet = Elencare il cestino non è ancora disponibile su { $system }

# draft
sys_trash_refused = cestino { $path }: { $error }

# draft
sys_trash_restore_not_yet = Ripristinare dal cestino non è ancora disponibile su { $system }

# draft
sys_unix_permissions_not_yet = Ripristinare i permessi delle cartelle Unix non è ancora disponibile su { $system }

# draft
sys_windows_permissions_not_yet = Ripristinare i permessi delle cartelle Windows non è ancora disponibile su { $system }

# draft
sys_write_through_not_yet = La scrittura diretta su un’unità non è ancora disponibile su { $system }

# draft
system_this = questo sistema

# draft
tabs_close = Chiudi scheda

# draft
tabs_close_description = Chiude una scheda (quella mostrata per impostazione predefinita); la notifica offre Annulla. L’ultima scheda di una finestra chiude la finestra se un’altra è aperta; l’ultima scheda dell’ultima finestra resta (il Ctrl+W della sua finestra chiude la finestra)

# draft
tabs_close_window = Chiudi finestra

# draft
tabs_close_window_description = Chiude una finestra (quella attiva per impostazione predefinita) e le sue schede, che Riapri scheda chiusa riporta indietro; l’ultima finestra si chiude solo con il proprio pulsante di chiusura o con Esci

# draft
tabs_closed = Scheda { $title } chiusa

# draft
tabs_closed_undo = Annulla

# draft
tabs_go_to_family = Vai a una scheda

# draft
tabs_last = Ultima scheda

# draft
tabs_last_tabs = Le schede mostrate quando la finestra è stata chiusa l’ultima volta, prima che esistessero le finestre

# draft
tabs_last_window = L’ultima finestra si chiude con il suo pulsante di chiusura o con Esci

# draft
tabs_last_windows = Le finestre mostrate per ultime e le loro schede

# draft
tabs_list = Elenca schede

# draft
tabs_list_description = Le schede in ordine, finestra dopo finestra: la finestra di ciascuna, i riquadri, le loro posizioni, quale riquadro è attivo e se la sua finestra la mostra

# draft
tabs_list_windows = Elenca finestre

# draft
tabs_list_windows_description = Le finestre: le schede di ciascuna, la scheda che mostra e quale è attiva

# draft
tabs_move = Sposta scheda

# draft
tabs_move_description = Mette una scheda in un’altra posizione, o la sposta per passaggi; oppure la sposta in un’altra finestra, o in una nuova

# draft
tabs_move_family = Sposta la scheda

# draft
tabs_move_left = Sposta la scheda a sinistra

# draft
tabs_move_right = Sposta la scheda a destra

# draft
tabs_move_to_window = Sposta la scheda in una nuova finestra

# draft
tabs_new = Nuova scheda

# draft
tabs_new_description = Apre una scheda che mostra una posizione, o ciò che mostra il riquadro; mostrata subito a meno che `show` sia false

# draft
tabs_new_window = Nuova finestra

# draft
tabs_new_window_description = Apre una finestra che mostra una posizione, o ciò che mostra il riquadro, con la forma della finestra del riquadro; diventa la finestra attiva

# draft
tabs_next = Scheda successiva

# draft
tabs_no_pane = nessun riquadro di questo tipo

# draft
tabs_no_tab = nessuna scheda di questo tipo

# draft
tabs_no_window = nessuna finestra di questo tipo

# draft
tabs_nothing_for_tab = il riquadro non mostra nulla da aprire in una scheda

# draft
tabs_nothing_for_window = il riquadro non mostra nulla da aprire in una finestra

# draft
tabs_nth = Scheda { $number }

# draft
tabs_only_folders = Solo cartelle e posizioni si aprono in una scheda o in una finestra

# draft
tabs_open = Apri in una nuova scheda

# draft
tabs_open_description = Apre ogni cartella selezionata in una scheda a parte, dietro la scheda mostrata; i file non si aprono nelle schede

# draft
tabs_open_window = Apri in una nuova finestra

# draft
tabs_open_window_description = Apre ogni cartella selezionata in una finestra a parte, con la forma di quella del riquadro; i file non si aprono nelle finestre

# draft
tabs_previous = Scheda precedente

# draft
tabs_reopen = Riapri scheda chiusa

# draft
tabs_reopen_description = Riapre la scheda o la finestra chiusa per ultima, o quella indicata da una chiusura, con la sua cronologia

# draft
tabs_select = Mostra scheda

# draft
tabs_select_description = Mostra una scheda: per id, o tra le schede della finestra attiva per posizione (negativa da destra) o per passaggi a partire da quella mostrata

# draft
tabs_select_window = Vai alla finestra

# draft
tabs_select_window_description = Rende attiva una finestra, portandola in primo piano

# draft
tabs_sleep = Libera la memoria delle schede nascoste

# draft
tabs_sleep_description = Una scheda che non stai guardando rilascia il proprio elenco di file e lo elenca di nuovo quando viene mostrata.

# draft
tabs_sleep_description_more = Disattiva per mantenere ogni scheda elencata e aggiornata, il che usa più memoria e più lavoro in background.

# draft
tabs_which_window = quale finestra: serve `window`

# draft
tabs_window_one_tab = quella finestra contiene una sola scheda

# draft
topic_files = File

# draft
topic_find = Ricerca

# draft
topic_go = Spostamenti

# draft
topic_notes = Etichette e annotazioni

# draft
topic_places = Posizioni e server

# draft
topic_player = Riproduzione

# draft
topic_preview = Anteprime

# draft
topic_select = Selezione

# draft
topic_skwir = Skwir

# draft
topic_tabs = Schede e riquadri

# draft
topic_terminal = Terminale

# draft
topic_view = Viste e pannelli

# draft
topic_window = La finestra

# draft
trash_op_mounted_remote = una cartella di rete montata da un programma non ha un cestino: eliminala invece

# draft
trash_op_no_home = non esiste una cartella personale per il suo cestino

# draft
trash_op_share_refused = la condivisione di rete non ha un cestino che tu possa usare: eliminalo invece

# draft
trash_op_top = la radice di un filesystem non può essere spostata nel cestino

# draft
vcs_changed = Modificato

# draft
vcs_conflict = In conflitto

# draft
vcs_folder_ignored = Ignorata

# draft
vcs_holds_changes = Contiene modifiche

# draft
vcs_holds_conflict = Contiene un conflitto

# draft
vcs_holds_new = Contiene nuovi file

# draft
vcs_holds_staged = Contiene modifiche in stage

# draft
vcs_ignored = Ignorato

# draft
vcs_new = Nuovo

# draft
vcs_staged = In stage

# draft
volumes_drive_gone = L’unità che contiene questa cartella è stata smontata o rimossa. La cartella tornerà a essere mostrata qui appena sarà montata.

# draft
volumes_server_gone = Il server che contiene questa cartella è stato disconnesso. La cartella tornerà a essere mostrata qui appena sarà di nuovo connesso.

# draft
workspace_no_tab = nessuna scheda di questo tipo

# draft
workspace_no_window = nessuna finestra di questo tipo

# draft
workspace_tab_alone = la scheda è sola nella sua finestra

# draft
workspace_tab_there = la scheda è già in quella finestra
