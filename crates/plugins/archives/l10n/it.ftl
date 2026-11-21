# Italian catalog of skwir-plugin-archives, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
archives_already_extracted_into = { $archive } era già stato estratto in { $into }

# draft
archives_already_extracted_to = { $archive } era già stato estratto in { $into }

# draft
archives_already_held = { $count ->
        [1] { $archive } conteneva già { $name }
       *[other] { $archive } conteneva già { $count } elementi
    }

# draft
archives_already_there = { $name } è già presente

# draft
archives_among_inputs = { $name } è tra gli elementi da inserire nell’archivio

# draft
archives_ask_as = { $count ->
        [1] Comprimi { $name } come
       *[other] Comprimi { $count } elementi come
    }

# draft
archives_ask_as_note = L’estensione indica il formato: { $endings }

# draft
archives_ask_with_password = { $count ->
        [1] Comprimi { $name } con una password, come
       *[other] Comprimi { $count } elementi con una password, come
    }

# draft
archives_ask_with_password_note = Un archivio ZIP cifrato: la password verrà chiesta subito dopo

# draft
archives_attributes_unread = non è stato possibile leggere le etichette e gli attributi di { $name }

# draft
archives_cancelled_compress = Annullato: non è stato compresso nulla

# draft
archives_cancelled_extract = Annullato: non è stato estratto nulla

# draft
archives_cannot_list_whole = Non è possibile elencare { $name } per intero

# draft
archives_choose_format = Scegli il formato…

# draft
archives_column_name = Nome

# draft
archives_column_size = Dimensione

# draft
archives_column_status = Stato

# draft
archives_compress = Comprimi

# draft
archives_compress_as = { $count ->
        [1] comprimere { $name } come { $archive }
       *[other] comprimere { $count } elementi come { $archive }
    }

# draft
archives_compress_ask = Comprimi…

# draft
archives_compress_ask_description = Chiede il nome di un archivio della selezione, la cui estensione ne indica il formato, e lo crea

# draft
archives_compress_button = Comprimi

# draft
archives_compress_description = Crea un archivio con la selezione, o con le voci indicate, accanto a esse o in `dest`: il formato è quello con cui termina `name` (.zip, .7z, .tar.gz, .tar.xz, .tar.zst, .tar.bz2, .tar), altrimenti `format`, altrimenti ZIP (piano)

# draft
archives_compress_plan = { $count ->
        [1] comprimere { $name }
       *[other] comprimere { $count } elementi
    }

# draft
archives_compressed_as = { $count ->
        [1] { $name } compresso come { $archive }
       *[other] { $count } elementi compressi come { $archive }
    }

# draft
archives_detail_encrypted = Cifrati

# draft
archives_detail_files = File

# draft
archives_detail_folders = Cartelle

# draft
archives_detail_unpacked = Decompresso

# draft
archives_encrypted_members = { $count ->
        [one] { $count } membro
       *[other] { $count } membri
    }

# draft
archives_endings = .zip, .7z, .tar.gz, .tar.xz, .tar.zst, .tar.bz2 o .tar

# draft
archives_extract = Estrai qui

# draft
archives_extract_button = Estrai

# draft
archives_extract_description = Estrae ogni archivio della selezione, o delle voci indicate, accanto a esso o in `dest`: un archivio con una sola cartella o un solo file produce quella cartella o quel file, uno con più elementi una cartella che ne porta il nome (piano)

# draft
archives_extract_here = Estrai qui

# draft
archives_extract_into = estrarre { $archive } in { $into }

# draft
archives_extract_plan = { $count ->
        [1] estrarre { $name }
       *[other] estrarre { $count } archivi
    }

# draft
archives_extract_to = estrarre { $archive } in { $into }

# draft
archives_extract_verb = Estrai

# draft
archives_extracted_into = { $archive } estratto in { $into }

# draft
archives_extracted_to = { $archive } estratto in { $into }

# draft
archives_first_members = { $count ->
        [1] È elencato solo il primo membro · apri l’archivio in un’applicazione per archivi per vedere il resto
       *[other] Sono elencati solo i primi { $count } membri · apri l’archivio in un’applicazione per archivi per vedere il resto
    }

# draft
archives_folder_not_there = la cartella in cui va { $name } non esiste

# draft
archives_format_differs = { $name } indica { $said }, ma è stato richiesto il formato { $asked }

# draft
archives_holds_archive = { $there } contiene { $archive }, l’archivio da estrarre

# draft
archives_holds_input = { $there } contiene { $input }, che va inserito nell’archivio

# draft
archives_in_a_folder = un archivio si trova in una cartella

# draft
archives_member_not_previewed = Non è possibile visualizzare l’anteprima di questo membro (cartella, collegamento o file cifrato)

# draft
archives_no_format = { $name } non indica alcun formato: termina il nome con { $endings }

# draft
archives_no_longer_there = { $name } non esiste più

# draft
archives_not_a_folder = { $name } non è una cartella

# draft
archives_not_a_folder_to_extract_into = { $name } non è una cartella in cui estrarre

# draft
archives_not_an_archive = { $name } non è un archivio

# draft
archives_not_in_archive = { $member } non è più in { $archive }

# draft
archives_not_placed = Non è stato possibile mettere { $name } in { $folder }

# draft
archives_not_writable = Non è possibile scrivere in { $name }: non verrà estratto nulla al suo interno

# draft
archives_nothing_compressed = Non verrà compresso nulla finché non scegli.

# draft
archives_nothing_extracted = Non verrà estratto nulla finché non scegli.

# draft
archives_nothing_to_compress = Niente da comprimere

# draft
archives_only_zip_password = Solo un archivio ZIP può avere una password

# draft
archives_only_zip_password_end = Solo un archivio ZIP può avere una password: termina il nome con .zip

# draft
archives_password_again = Di nuovo la stessa password, per sicurezza

# draft
archives_password_differed = Le due password non coincidono: una password per { $name }

# draft
archives_password_empty = Password per { $name }: non può essere vuota

# draft
archives_password_for = Password per { $name }

# draft
archives_password_new = Una password per { $name }

# draft
archives_password_new_empty = Una password per { $name }: non può essere vuota

# draft
archives_password_next = Avanti

# draft
archives_password_unasked = { $name } verrebbe cifrato: la password viene chiesta nella finestra di Skwir

# draft
archives_plugin_description = Elenca il contenuto di archivi ZIP, tar, 7z e altri, mostra l’anteprima dei loro membri senza estrarli, li estrae (Estrai qui, nel menu di un archivio e in quello di un trascinamento) e li crea (Comprimi, nel menu di ciò che selezioni)

# draft
archives_plugin_title = Archivi

# draft
archives_requirement = Elencare, leggere, estrarre e creare archivi

# draft
archives_status_encrypted = Cifrato

# draft
archives_status_link = Collegamento / speciale

# draft
archives_through_link = { $name } è un collegamento: non viene estratto nulla attraverso di esso

# draft
archives_title = { $count ->
        [one] Archivio · { $count } membro
       *[other] Archivio · { $count } membri
    }

# draft
archives_to_zip = In ZIP

# draft
archives_too_deep = { $count ->
        [one] Il percorso dell’archivio supera { $count } componente
       *[other] Il percorso dell’archivio supera { $count } componenti
    }

# draft
archives_two_as_one = due elementi verrebbero estratti come { $name }

# draft
archives_unasked = { $name } è cifrato: la password viene chiesta nella finestra di Skwir

# draft
archives_unknown_format = { $format } non è un formato supportato: indicane uno tra { $endings }

# draft
archives_unreadable = Non è stato possibile leggere { $name }

# draft
archives_went_missing = { $name } è scomparso durante la creazione

# draft
archives_with_password = Con password…

# draft
archives_would_leave = ciò che viene estratto in { $name } ne uscirebbe

# draft
archives_wrong_password = Password errata per { $name }: riprova
