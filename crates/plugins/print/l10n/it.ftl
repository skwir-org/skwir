# Italian catalog of skwir-plugin-print, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
print_alone = Ogni PDF prende il nome del suo file, in questa cartella

# draft
print_already_saved = { $what ->
        [file] { $name }: già salvato come { $pdf } in { $folder }
        [files] { $name } e { $more } in più: già salvati come { $pdf } in { $folder }
        [sheet] { $count ->
            [one] un foglio di provini di { $count } file era già salvato come { $pdf } in { $folder }
           *[other] un foglio di provini di { $count } file era già salvato come { $pdf } in { $folder }
        }
       *[listing] l’elenco di { $name } era già salvato come { $pdf } in { $folder }
    }

# draft
print_already_there = { $name } è già presente

# draft
print_as_shown = Come mostrato

# draft
print_as_shown_detail = Ogni pagina su un foglio

# draft
print_beside = { $count ->
        [1] Accanto al file
       *[other] Accanto ai file
    }

# draft
print_beside_each = Accanto a ogni file; un PDF con tutti accanto al primo

# draft
print_booklet = Opuscolo

# draft
print_booklet_detail = Pagine su entrambi i lati, piegate

# draft
print_booklet_needs_pdfium = Piegare un opuscolo richiede PDFium, che non è presente

# draft
print_booklet_one_side = Un opuscolo si stampa su entrambi i lati: per stampare su un lato solo, stampa le facce dispari, gira i fogli, poi stampa le pari

# draft
print_bound_right = Rilegato a destra

# draft
print_bound_right_help = Per un libro che si legge da destra a sinistra

# draft
print_cancelled = La stampa è stata annullata

# draft
print_cancelled_save = Annullato: nessun PDF salvato

# draft
print_chosen_before = Scelte in precedenza

# draft
print_closing = Skwir si sta chiudendo

# draft
print_column_deleted = Eliminato

# draft
print_column_folder = Cartella

# draft
print_column_modified = Modificato

# draft
print_column_name = Nome

# draft
print_column_size = Dimensione

# draft
print_column_type = Tipo

# draft
print_contact_sheet = Foglio di provini

# draft
print_contact_sheet_detail = Le immagini, diverse per pagina

# draft
print_contact_sheet_title = Foglio di provini

# draft
print_copies_only = { $name } accetta solo copie: qui non viene creato nessun PDF

# draft
print_count_needs_reader = Contare le pagine di un PDF richiede un lettore di documenti, che non è presente

# draft
print_dialog_accept = Stampa…

# draft
print_eight_sheets = 8 fogli per fascicolo

# draft
print_every_sheet = Tutti i fogli in un fascicolo

# draft
print_filtered = { $named }, filtrato per { $filter }

# draft
print_fold_three_times = Piega ogni foglio a metà tre volte, con la pagina 1 all’esterno e dritta, taglia le pieghe chiuse e metti i fogli uno dopo l’altro

# draft
print_fold_twice = Piega ogni foglio a metà due volte, con la pagina 1 all’esterno e dritta, taglia la piega superiore e metti i fogli uno dopo l’altro

# draft
print_folded_as = Piegato in

# draft
print_folded_once = Piegato una volta

# draft
print_folded_three_times = Piegato tre volte, tagliato

# draft
print_folded_twice = Piegato due volte, con la piega superiore tagliata

# draft
print_folder_listed = La cartella elencata

# draft
print_folder_of = La cartella a cui appartiene

# draft
print_font_sample = Quel vituperabile xenofobo zelante assaggia il whisky ed esclama: alleluja! 0123456789

# draft
print_four_sheets = 4 fogli per fascicolo

# draft
print_handed = Stampa di { $pages ->
        [0] {""}
        [one] { $pages } pagina di{" "}
       *[other] { $pages } pagine di{" "}
    }{ $what ->
        [file] { $name }
        [files] { $count ->
            [one] { $count } file
           *[other] { $count } file
        }
        [sheet] un foglio di provini
       *[listing] l’elenco di { $name }
    } consegnata al servizio di stampa del desktop per { $to ->
        [file] { $target }
        [printer] la stampante { $printer }
       *[system] la stampante
    }, che non ha confermato di averla presa in carico

# draft
print_in_folder = In { $folder }

# draft
print_in_folder_listed = Nella cartella elencata

# draft
print_in_no_folder = Il file non è in nessuna cartella

# draft
print_job_files = { $first } e { $more } in più

# draft
print_joined_needs_pdfium = Stampare più file come uno richiede PDFium, che non è presente

# draft
print_keeping = Conservazione di { $name } per stamparlo

# draft
print_keeping_document = Conservazione del PDF del documento per stamparlo

# draft
print_kind_folder = Cartella

# draft
print_kind_place = Posizione

# draft
print_layout = Disposizione

# draft
print_left_out = Escluso { $name }: { $error }

# draft
print_name_of_one = Un nome è quello di un PDF: questi file ne creano uno ciascuno (`together` ne crea uno solo per tutti)

# draft
print_no_folder_of_its_own = Non ha una cartella propria

# draft
print_no_home = Salva come: non esiste una cartella personale per «~»

# draft
print_no_listing_folder = Non c’è nessuna cartella in cui salvare l’elenco: scegline una

# draft
print_no_longer_shown = { $name } non è più mostrato, quindi il suo elenco non è noto

# draft
print_no_page_asked = Nessuna pagina è tra quelle richieste

# draft
print_no_page_dialog = Nessuna pagina è tra quelle richieste dalla finestra di stampa

# draft
print_no_page_shown = Nessuna pagina è mostrata

# draft
print_no_paper = { $paper } non è una carta: a4, letter, a5, a3 o legal

# draft
print_no_picture = Nessuno di essi è un’immagine

# draft
print_no_such_folder = Salva come: non esiste la cartella { $path }

# draft
print_no_such_pane = Nessun riquadro di questo tipo

# draft
print_no_whole_path = Salva come: { $typed } non è un percorso completo: scrivilo dalla radice, o da ~

# draft
print_none_printable = Nessuno di essi può essere stampato

# draft
print_not_a_folder = { $name } non è una cartella

# draft
print_not_own_file = Il PDF creato non è un file a sé

# draft
print_not_pictures = { $count ->
        [1] Escluso perché non è un’immagine: { $name }
       *[other] { $count } file non sono immagini e sono stati esclusi
    }

# draft
print_not_placed = Impossibile collocare { $name } in { $folder }

# draft
print_not_printed = { $what ->
        [file] Stampa di { $name } non eseguita
        [files] { $count ->
            [one] Stampa di { $count } file non eseguita
           *[other] Stampa di { $count } file non eseguita
        }
        [sheet] Stampa del foglio di provini non eseguita
       *[listing] Stampa dell’elenco di { $name } non eseguita
    }

# draft
print_not_smaller = Non è stato possibile ridurre il PDF

# draft
print_not_there = { $name } non è presente

# draft
print_nothing_printable = { $name } non mostra nulla che si possa stampare

# draft
print_nothing_saved = Nessun PDF viene salvato finché non scegli.

# draft
print_nothing_selected = Nulla è selezionato

# draft
print_nothing_there = Non c’è niente da stampare

# draft
print_nothing_to_print = Niente da stampare

# draft
print_notice_of = { $name }: { $notice }

# draft
print_notice_of_file = { $name }: { $notice }

# draft
print_one_booklet = Un opuscolo

# draft
print_other_pane = L’altro riquadro

# draft
print_page_alt = Pagina { $page }

# draft
print_page_count = { $count ->
        [one] { $count } pagina
       *[other] { $count } pagine
    }

# draft
print_pages = Pagine

# draft
print_pages_all = Tutte

# draft
print_pages_current = Pagina { $page }

# draft
print_pages_need_pdfium = Stampare le sue pagine richiede PDFium, che non è presente

# draft
print_pages_ranges = Pagine…

# draft
print_pages_refused = Pagine da stampare: { $why }

# draft
print_pages_selection = Selezione

# draft
print_pages_to_print = Pagine da stampare

# draft
print_paper = Carta

# draft
print_past = { $count ->
        [one] Ha { $count } pagina; quelle oltre l’ultima sono state escluse
       *[other] Ha { $count } pagine; quelle oltre l’ultima sono state escluse
    }

# draft
print_pdf_each = Un PDF ciascuno

# draft
print_pdf_one = Un PDF con tutti

# draft
print_pdfs = PDF

# draft
print_per_side = { $count ->
        [one] { $count } pagina per lato: un opuscolo ne ha 2, 4 o 8
       *[other] { $count } pagine per lato: un opuscolo ne ha 2, 4 o 8
    }

# draft
print_per_side_choice = { $count } per lato

# draft
print_per_side_field = Pagine per lato

# draft
print_place_files = { $count ->
        [one] { $count } file
       *[other] { $count } file
    }

# draft
print_places = Posizioni

# draft
print_plugin_description = Stampa ciò che mostra un’anteprima, così com’è, come opuscolo o come foglio di provini, oppure salvalo come PDF

# draft
print_plugin_off = Salvare come PDF richiede il plugin di stampa attivo

# draft
print_plugin_title = Stampa

# draft
print_print = Stampa…

# draft
print_print_description = Stampa i file come li mostrano le loro anteprime, un lavoro per volta nel loro ordine (`layout`: come mostrato, un opuscolo piegato in fascicoli di `signature` pagine con `trim`, un foglio di provini delle immagini, l’elenco di un riquadro), con le `pages` scelte; senza `asked` la finestra chiede prima nella finestra di Skwir, poi la finestra di sistema sceglie la stampante, la carta e i lati

# draft
print_print_listing = Stampa elenco…

# draft
print_ranges_placeholder = Da 1: 1-3, 7, 10- (fino alla fine)

# draft
print_save = Salva

# draft
print_save_as = Salva come

# draft
print_save_as_help = Una cartella da ~ o dalla radice, poi il nome: Tab completa una cartella

# draft
print_save_as_refused = Salva come: { $why }

# draft
print_save_each = { $count ->
        [one] { $form ->
            [booklet] salva { $count } file come opuscoli
           *[pdf] salva { $count } file come PDF
        }
       *[other] { $form ->
            [booklet] salva { $count } file come opuscoli
           *[pdf] salva { $count } file come PDF
        }
    }

# draft
print_save_listing = salva l’elenco di { $name } come PDF

# draft
print_save_one = { $form ->
        [booklet] salva { $name } come opuscolo
       *[pdf] salva { $name } come PDF
    }

# draft
print_save_pdf = Salva come PDF

# draft
print_save_pdf_description = Crea un PDF di ogni file come lo mostra la sua anteprima, accanto al file o in `dest` (uno solo per tutti con `together`), sulla carta delle impostazioni regionali del sistema, con la disposizione e le pagine che Stampa… accetta (piano)

# draft
print_save_sheet = { $count ->
        [one] salva un foglio di provini di { $count } file come PDF
       *[other] salva un foglio di provini di { $count } file come PDF
    }

# draft
print_save_step = { $what ->
        [file] salva { $name } come { $pdf } in { $folder }
        [files] salva { $name } e { $more } in più come { $pdf } in { $folder }
        [sheet] { $count ->
            [one] salva un foglio di provini di { $count } file come { $pdf } in { $folder }
           *[other] salva un foglio di provini di { $count } file come { $pdf } in { $folder }
        }
       *[listing] salva l’elenco di { $name } come { $pdf } in { $folder }
    }

# draft
print_save_together = { $count ->
        [one] { $form ->
            [booklet] salva { $count } file come un unico opuscolo
           *[pdf] salva { $count } file come un unico PDF
        }
       *[other] { $form ->
            [booklet] salva { $count } file come un unico opuscolo
           *[pdf] salva { $count } file come un unico PDF
        }
    }

# draft
print_save_verb = Salva come PDF

# draft
print_saved_step = { $what ->
        [file] salvato { $name } come { $pdf } in { $folder }
        [files] salvato { $name } e { $more } in più come { $pdf } in { $folder }
        [sheet] { $count ->
            [one] salvato un foglio di provini di { $count } file come { $pdf } in { $folder }
           *[other] salvato un foglio di provini di { $count } file come { $pdf } in { $folder }
        }
       *[listing] salvato l’elenco di { $name } come { $pdf } in { $folder }
    }

# draft
print_sent = Stampa di { $pages ->
        [0] {""}
        [one] { $pages } pagina di{" "}
       *[other] { $pages } pagine di{" "}
    }{ $what ->
        [file] { $name }
        [files] { $count ->
            [one] { $count } file
           *[other] { $count } file
        }
        [sheet] un foglio di provini
       *[listing] l’elenco di { $name }
    } inviata { $to ->
        [file] a { $target }
        [printer] alla stampante { $printer }
       *[system] alla stampante
    }

# draft
print_sets_of_16 = Gruppi di 16 pagine

# draft
print_sets_of_32 = Gruppi di 32 pagine

# draft
print_sheets_in_fold = { $count ->
        [one] { $count } foglio, un fascicolo
       *[other] { $count } fogli in un fascicolo
    }

# draft
print_shows_nothing = Il riquadro non mostra nulla

# draft
print_smaller = File più piccolo

# draft
print_smaller_folder = Una cartella in cui ridurre il PDF

# draft
print_smaller_help = Immagini al massimo a 150 dpi, come Schermo di Riduci dimensione

# draft
print_then = { $said }. { $then }

# draft
print_title = { $what ->
        [file] Stampa { $name }
        [files] { $count ->
            [one] Stampa { $count } file
           *[other] Stampa { $count } file
        }
        [sheet] Stampa un foglio di provini
       *[listing] Stampa l’elenco di { $name }
    }

# draft
print_to = A

# draft
print_to_pdf = PDF

# draft
print_to_printer = Stampante

# draft
print_trim = Rimuovi i margini bianchi

# draft
print_trim_help = Le pagine si adattano al loro inchiostro, tutte allo stesso modo

# draft
print_went_missing = { $name } è scomparso mentre veniva creato

# draft
print_work_folder = Una cartella in cui creare il documento

# draft
print_wrong_size = un’immagine di dimensioni errate
