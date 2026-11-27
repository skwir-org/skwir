# Italian catalog of skwir-plugin-remote, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
remote_address_bad_device = { $text } non è il nome di un dispositivo

# draft
remote_address_bad_port = { $port } non è una porta

# draft
remote_address_bad_remote = { $name } non è il nome di un remote

# draft
remote_address_bad_server = { $host } non è il nome di un server

# draft
remote_address_bad_ssh_user = { $user } non è un nome utente che ssh possa ricevere

# draft
remote_address_bad_user = { $user } non è un nome utente

# draft
remote_address_device_user = { $text }: l’indirizzo di un dispositivo non indica alcun utente

# draft
remote_address_ipv6 = { $text } non ha il ] che chiude il suo indirizzo IPv6

# draft
remote_address_no_protocol = { $text } non indica alcun protocollo

# draft
remote_address_no_server = { $text } non indica alcun server

# draft
remote_address_no_slashes = { $text } richiede // prima del server

# draft
remote_address_not_protocol = { $scheme } non è un protocollo che Skwir apre

# draft
remote_address_password = un indirizzo non contiene password: la chiede il server

# draft
remote_ask_connect = Connetti

# draft
remote_ask_password = Password

# draft
remote_bucket_label = { $bucket } ({ $profile })

# draft
remote_cancelled = annullato

# draft
remote_chosen_data = Le miniature scelte per ogni remote, in base al suo indirizzo

# draft
remote_configure_add_as_server = Aggiungi come server…

# draft
remote_configure_add_record = Aggiungi server

# draft
remote_configure_add_server = Aggiungi server…

# draft
remote_configure_add_server_description = Aggiungi un server: un remote di rclone (archiviazione cloud, S3, WebDAV, SFTP e settanta altri), in un modulo che l’utente compila; si può indicare da dove parte, mai un segreto

# draft
remote_configure_add_server_menu = Aggiungi server…

# draft
remote_configure_added = { $server } aggiunto

# draft
remote_configure_added_obscured = { $server } aggiunto. rclone conserva i suoi segreti solo offuscati: cifrare la sua configurazione?

# draft
remote_configure_answers = { $server } risponde

# draft
remote_configure_at_login = Connetti all’accesso

# draft
remote_configure_at_login_encrypted = { $count ->
        [1] { $remotes } si connette all’accesso, cosa che una configurazione cifrata non permette: nessuno può fornire la sua password. Disattiva prima Connetti all’accesso.
       *[other] { $remotes } si connettono all’accesso, cosa che una configurazione cifrata non permette: nessuno può fornire la sua password. Disattiva prima Connetti all’accesso.
    }

# draft
remote_configure_at_login_failed = { $server } non è stato impostato per connettersi all’accesso: { $error }

# draft
remote_configure_at_login_on = Connetti all’accesso ✓

# draft
remote_configure_at_login_which = solo i remote di rclone e i server SFTP si connettono all’accesso

# draft
remote_configure_bookmark_stays = il suo segnalibro resta: { $error }

# draft
remote_configure_connect_at_login = Connetti all’accesso

# draft
remote_configure_connect_at_login_description = Fa connettere un server al gestore dei servizi del sistema quando l’utente accede (un’unità utente di systemd, un launch agent, un’attività di accesso), oppure non più; lo connette anche subito. Solo ciò che non chiede nulla all’accesso: un remote di una configurazione di rclone non cifrata, un server SFTP con accesso tramite chiave

# draft
remote_configure_connects_at_login = { $server } si connette all’accesso

# draft
remote_configure_copied_what = il collegamento pubblico

# draft
remote_configure_copy_public_link = Copia collegamento pubblico

# draft
remote_configure_does_not_answer = { $server } non risponde: { $error }

# draft
remote_configure_edit_server = Modifica server…

# draft
remote_configure_edit_server_description = Modifica un server nel suo modulo: un remote di rclone, oppure uno aggiunto tramite il suo indirizzo

# draft
remote_configure_edit_server_menu = Modifica server…

# draft
remote_configure_encrypt = Cifra la configurazione di rclone…

# draft
remote_configure_encrypt_accept = Cifra

# draft
remote_configure_encrypt_again = Ripeti la password

# draft
remote_configure_encrypt_button = Cifra…

# draft
remote_configure_encrypt_description = Cifra il file di configurazione di rclone, che conserva i segreti dei suoi remote solo offuscati, con una password che l’utente digita e può tenere nel portachiavi

# draft
remote_configure_encrypt_intro = rclone conserva chiavi e password dei suoi remote nel file di configurazione, solo offuscate: chiunque legga il file può usarle. Cifrato, si apre con una password.

# draft
remote_configure_encrypt_keep = Tienila nel portachiavi

# draft
remote_configure_encrypt_keep_help = Skwir apre poi la configurazione senza chiedere. rclone in un terminale la chiede, oppure la legge da RCLONE_CONFIG_PASS.

# draft
remote_configure_encrypt_password = Password

# draft
remote_configure_encrypt_title = Cifra la configurazione di rclone

# draft
remote_configure_encrypted = La configurazione di rclone è cifrata

# draft
remote_configure_encrypted_already = La configurazione di rclone è già cifrata

# draft
remote_configure_finish = Completa la configurazione…

# draft
remote_configure_forget_password = Dimentica password

# draft
remote_configure_keep_as_server = Mantieni come server

# draft
remote_configure_keep_as_server_description = Rende l’indirizzo di una cartella remota un server della sezione Rete, conservato nell’elenco di Skwir o nelle cartelle di rete di KDE, con il nome del suo segnalibro; il suo segnalibro, se ne ha uno, viene rimosso

# draft
remote_configure_keep_as_server_item = Mantieni come server

# draft
remote_configure_keep_as_server_menu = Offri Mantieni come server nel menu di una posizione

# draft
remote_configure_keep_as_server_menu_description = Mantieni come server di una posizione della barra laterale, per l’indirizzo di una cartella remota che non è ancora un server (quello di un segnalibro). Risponde dalla memoria

# draft
remote_configure_kept_as_server = { $server } è un server della sezione Rete, conservato { $kept }

# draft
remote_configure_kept_by_tools = { $protocol } è conservato da rclone o dagli strumenti AWS

# draft
remote_configure_kept_elsewhere = { $server } è { $kept }: modificalo lì

# draft
remote_configure_kept_stays = Ciò che Skwir ha conservato per { $server } resta: { $error }

# draft
remote_configure_key_login = Imposta accesso con chiave…

# draft
remote_configure_key_login_failed = L’accesso con chiave non è stato impostato su { $server }: { $error }

# draft
remote_configure_key_login_sftp = l’accesso con chiave è di ssh: solo server SFTP

# draft
remote_configure_mount_options = Opzioni di montaggio…

# draft
remote_configure_mount_options_button = Opzioni di montaggio…

# draft
remote_configure_mount_options_description = Scegli come viene montato un server alla prossima connessione: sola lettura, cache dei file e banda di rclone, compressione di sshfs

# draft
remote_configure_moved_off_login = { $server } non si connette più all’accesso: il suo indirizzo è cambiato

# draft
remote_configure_nearby_menu = Menu del server vicino

# draft
remote_configure_nearby_menu_description = Le voci del menu contestuale per l’unico server selezionato in Dispositivi vicini: Aggiungi come server…, con il modulo compilato con il nome e l’indirizzo che ha annunciato, dove nessun server ha quell’indirizzo. Risponde dalla memoria

# draft
remote_configure_no_longer_at_login = { $server } non si connette più all’accesso

# draft
remote_configure_no_options = nessuno strumento con cui Skwir monta { $protocol } accetta opzioni

# draft
remote_configure_no_public_link = Nessun collegamento pubblico creato: { $error }

# draft
remote_configure_not_done = { $done ->
        [added] Il server non è stato aggiunto: { $error }
        [saved] Il server non è stato salvato: { $error }
        [signed_in] Accesso al server non riuscito: { $error }
        [tested] Il server non è stato testato: { $error }
        [kept] Il server non è stato mantenuto: { $error }
       *[removed] Il server non è stato rimosso: { $error }
    }

# draft
remote_configure_not_encrypted = La configurazione di rclone non è stata cifrata: { $error }

# draft
remote_configure_not_remote = non è in una cartella remota

# draft
remote_configure_nothing_to_link = niente a cui collegarsi

# draft
remote_configure_options_not_at_login = Le opzioni di montaggio sono state salvate, ma la connessione all’accesso monta come prima: { $error }

# draft
remote_configure_options_saved = { $server } verrà montato così alla prossima connessione

# draft
remote_configure_options_unsaved = Le opzioni di montaggio non sono state salvate: { $error }

# draft
remote_configure_password_stays = La password che Skwir conserva per { $server } resta nel portachiavi: { $error }

# draft
remote_configure_passwords_differ = Le due password sono diverse

# draft
remote_configure_public_link = Copia collegamento pubblico

# draft
remote_configure_public_link_description = Crea un collegamento pubblico a un file di un remote il cui archivio ne offre uno (il collegamento di condivisione di un cloud) e lo mette negli appunti

# draft
remote_configure_public_link_menu = Offri un collegamento pubblico nel menu contestuale

# draft
remote_configure_public_link_menu_description = Copia collegamento pubblico del menu contestuale, per un file di un remote il cui archivio ne offre uno. Risponde dalla memoria

# draft
remote_configure_reconnect = Accedi di nuovo…

# draft
remote_configure_reconnect_description = Fa accedere di nuovo un remote di rclone tramite il browser, perché il suo accesso è scaduto o è stato revocato

# draft
remote_configure_record_kept = { $kind } · { $kept }

# draft
remote_configure_record_kind = Tipo

# draft
remote_configure_record_name = Nome

# draft
remote_configure_record_unfinished = { $kind } · { $kept } · configurazione non completata

# draft
remote_configure_remove = Rimuovi

# draft
remote_configure_remove_intro = Skwir dimentica il server, conservato { $kept }: ciò che contiene resta dov’è e, aggiungendolo di nuovo, ritorna.

# draft
remote_configure_remove_intro_password = Skwir dimentica il server, conservato { $kept }, e la password che conserva per esso nel portachiavi: ciò che contiene resta dov’è e, aggiungendolo di nuovo, ritorna.

# draft
remote_configure_remove_server = Rimuovi server…

# draft
remote_configure_remove_server_description = Rimuove un server dopo la conferma dell’utente, dove è conservato (configurazione di rclone, elenco di Skwir, cartelle di rete di KDE), con le sue opzioni di montaggio e la sua voce che lo connette all’accesso: ciò che contiene resta dov’è

# draft
remote_configure_remove_server_menu = Rimuovi server…

# draft
remote_configure_remove_title = Rimuovere { $server }?

# draft
remote_configure_removed = { $server } rimosso

# draft
remote_configure_saved = { $server } salvato

# draft
remote_configure_server_records = Server

# draft
remote_configure_server_records_description = Ogni server come lo elenca Impostazioni: nome, tipo e dove è conservato di ciascuno, cosa fa la sua riga e se Skwir lo modifica (`fixed`: lo fa un altro programma)

# draft
remote_configure_servers = Server

# draft
remote_configure_servers_description = I server della sezione Rete della barra laterale, ciascuno modificato dove è conservato.

# draft
remote_configure_servers_description_more = Quelli aggiunti tramite il loro indirizzo sono nell’elenco di Skwir o nelle cartelle di rete di KDE; i remote di rclone nella configurazione propria di rclone, dove li trova anche rclone in un terminale.

# draft
remote_configure_setup_key_login = Imposta accesso con chiave…

# draft
remote_configure_setup_key_login_description = Permette a ssh di accedere a un server SFTP con una chiave, senza password: una chiave in possesso dell’agente ssh, quella predefinita dell’utente o una nuova, con la sua metà pubblica aggiunta alle chiavi autorizzate del server una volta scelta dall’utente

# draft
remote_configure_sign_in_again = Accedi di nuovo…

# draft
remote_configure_signed_in = { $server } ha eseguito di nuovo l’accesso

# draft
remote_configure_stays_mounted = resta montato: { $error }

# draft
remote_configure_test = Prova

# draft
remote_configure_test_server = Prova server

# draft
remote_configure_test_server_description = Elenca la radice di un remote di rclone, come farebbe la connessione, e dice se risponde

# draft
remote_configure_test_server_menu = Prova server

# draft
remote_configure_turned_off = le cartelle remote sono disattivate

# draft
remote_configure_unknown_server = nessun server noto all’indirizzo { $address }

# draft
remote_configure_window_needed = questo si chiede in un modulo, compilato nella finestra di Skwir: aprila prima

# draft
remote_connect = Connetti al server

# draft
remote_connect_description = Apre una cartella remota dal suo indirizzo (sftp://, smb://, dav://, ftp://, nfs://, afp://, rclone://)

# draft
remote_connecting = Connessione…

# draft
remote_connectors = Strumenti che aprono indirizzi

# draft
remote_connectors_description = Gli strumenti che montano un indirizzo sftp://, smb:// o altro, provati in quest’ordine tra quelli installati: sshfs, kmtp (un telefono gestito da KDE), gvfs, kio, rclone, fstab.

# draft
remote_connectors_description_more = Sul desktop KDE, kio viene prima di gvfs.

# draft
remote_device_held = { $count ->
        [1] { $programs } lo tiene
       *[other] { $programs } lo tengono
    } aperto, e solo un programma alla volta può usare { $device ->
        [phone] un telefono
       *[camera] una fotocamera
    }. Chiudi { $count ->
        [1] { $programs }
       *[other] questi programmi
    }, poi riprova.

# draft
remote_device_held_through = { $count ->
        [1] { $programs } lo tiene
       *[other] { $programs } lo tengono
    } aperto, e solo un programma alla volta può usare { $device ->
        [phone] un telefono
       *[camera] una fotocamera
    }. Skwir lo aprirebbe tramite { $holder }, ma { $why }; altrimenti chiudi { $count ->
        [1] { $programs }
       *[other] questi programmi
    }, poi riprova.

# draft
remote_device_not_mounted = Non montato. Fai clic per montarlo e aprirlo.

# draft
remote_disconnect = Disconnetti

# draft
remote_disconnect_description = Rilascia una cartella remota: ciò che la mostra lo dice, e la mostra di nuovo una volta riconnessa

# draft
remote_disconnected = Disconnesso da { $name }

# draft
remote_disconnected_kept = Disconnesso da { $name }: { $count ->
        [one] { $count } file non ancora inviato resta
       *[other] { $count } file non ancora inviati restano
    } nella cache di rclone e viene inviato alla prossima connessione

# draft
remote_disconnecting = Disconnessione…

# draft
remote_forget_password = Dimentica password

# draft
remote_forget_password_description = Rimuove dal portachiavi la password che Skwir conserva per un server (mai una conservata da un altro programma)

# draft
remote_forget_password_menu = Dimentica password

# draft
remote_fstab = i montaggi del sistema

# draft
remote_fstab_administrator = l’ha montato il sistema: solo un amministratore può rilasciarlo

# draft
remote_fstab_linux_only = fstab viene letto solo su Linux

# draft
remote_fstab_no_entry = nessuna voce di /etc/fstab che puoi montare lo nomina

# draft
remote_fstab_not_mounted = il sistema non ha montato { $folder }: forse ci sta ancora provando

# draft
remote_gvfs_camera_label = { $device } (fotocamera)

# draft
remote_gvfs_connect_to = Connettersi a { $host }?

# draft
remote_gvfs_device_silent = non ha risposto entro { $seconds ->
        [one] { $seconds } secondo
       *[other] { $seconds } secondi
    }

# draft
remote_gvfs_ios_label = { $device } (iOS)

# draft
remote_gvfs_linux_only = GVfs funziona solo su Linux

# draft
remote_gvfs_mtp_label = { $device } (MTP)

# draft
remote_gvfs_no_address = GVfs non indica alcun indirizzo per questo montaggio

# draft
remote_gvfs_no_bridge = GVfs ha montato { $address } ma il suo ponte FUSE non è in esecuzione: gvfsd-fuse è installato?

# draft
remote_gvfs_no_folder = GVfs ha montato { $address } ma la sua cartella non c’è

# draft
remote_gvfs_other_label = { $host } ({ $kind })

# draft
remote_gvfs_proxy_user = GVfs chiede un nome utente che non è quello del server (quello di un proxy), che Skwir non può chiedere

# draft
remote_gvfs_server_silent = { $host } non ha risposto entro { $seconds ->
        [one] { $seconds } secondo
       *[other] { $seconds } secondi
    }

# draft
remote_gvfs_without_bridge = GVfs è in esecuzione senza il suo ponte FUSE (gvfsd-fuse)

# draft
remote_gvfs_you_cancelled = hai annullato

# draft
remote_half_made = La configurazione di { $name } non è completata: Completa la configurazione… nel suo menu

# draft
remote_keys_already_there = c’è già una chiave in { $path }

# draft
remote_keys_comment_line = il commento di una chiave è di una sola riga

# draft
remote_keys_form_accept = Imposta

# draft
remote_keys_form_again = Ripeti la passphrase

# draft
remote_keys_form_intro = La metà pubblica della tua chiave viene aggiunta alle chiavi autorizzate del server: ssh poi accede con essa, senza password. La sua metà privata non lascia mai questo computer. La password del server viene chiesta una sola volta.

# draft
remote_keys_form_key = Chiave

# draft
remote_keys_form_passphrase = Passphrase della nuova chiave

# draft
remote_keys_form_passphrase_help = Cifra una nuova chiave: viene chiesta quando la si usa, a meno che l’agente non la possieda. Vuota per nessuna.

# draft
remote_keys_form_title = Imposta accesso con chiave su { $server }

# draft
remote_keys_had_it = { $server } aveva già la chiave: ssh accede con essa

# draft
remote_keys_in_agent = { $key }, nell’agente ssh

# draft
remote_keys_in_file = { $key }, in { $file }

# draft
remote_keys_key = { $comment } ({ $kind })

# draft
remote_keys_keygen_failed = ssh-keygen non è riuscito: { $said }

# draft
remote_keys_keygen_no_passphrase = a ssh-keygen non è stata data la passphrase

# draft
remote_keys_keygen_no_public = ssh-keygen non ha scritto alcuna chiave pubblica

# draft
remote_keys_new = Una nuova chiave

# draft
remote_keys_new_made_in = Creata in { $folder }

# draft
remote_keys_no_home = non c’è una cartella personale per le chiavi di ssh

# draft
remote_keys_no_runtime = non c’è una cartella di runtime

# draft
remote_keys_no_such_key = nessuna chiave simile

# draft
remote_keys_none_chosen = nessuna chiave scelta

# draft
remote_keys_none_offered = ssh non offre alcuna chiave, e una nuova verrebbe creata sopra quella in { $path }

# draft
remote_keys_not_added = il server non ha potuto aggiungere le chiavi: { $why }

# draft
remote_keys_not_added_unsaid = il server non ha potuto aggiungere le chiavi: non ha detto perché

# draft
remote_keys_not_taken = La chiave è su { $server }, ma ssh non accede con essa: il server potrebbe non accettare chiavi

# draft
remote_keys_passphrases_differ = Le due passphrase sono diverse

# draft
remote_keys_refused = il server ha rifiutato le chiavi: { $said }

# draft
remote_keys_set_up = Accesso con chiave impostato su { $server }

# draft
remote_keys_sftp_only = l’accesso con chiave è impostato per i server SFTP

# draft
remote_keys_unread = non è stato possibile leggere le chiavi autorizzate del server: { $said }

# draft
remote_kio_could_not = KIO non ha potuto aprirlo: { $reason }

# draft
remote_kio_could_not_open = KIO non ha potuto aprirlo

# draft
remote_kio_keeps = KIO tiene le sue cartelle fino alla fine della sessione

# draft
remote_kio_linux_only = Le cartelle di KIO (kio-fuse) sono solo su Linux

# draft
remote_kio_no_answer = kio-fuse non ha risposto

# draft
remote_kio_no_folder = kio-fuse l’ha montato ma la sua cartella non c’è

# draft
remote_kio_opens_no = KIO non apre indirizzi { $protocol }

# draft
remote_kmtp_not_held = kmtpd di KDE non gestisce questo telefono

# draft
remote_kmtpd = kmtpd di KDE

# draft
remote_kmtpd_linux_only = kmtpd di KDE è raggiungibile solo su Linux

# draft
remote_login_agent_read_only = l’agente di accesso è di sola lettura; modificalo dove è gestito

# draft
remote_login_control_character = { $part } contiene un carattere che un gestore dei servizi non legge

# draft
remote_login_ended = è terminato ({ $status })

# draft
remote_login_journal = { $error } (journalctl --user -u { $unit } ne dice il motivo)

# draft
remote_login_needs_password = il server vuole una password o la passphrase di una chiave: all’accesso nessuno può fornirla (una chiave in possesso dell’agente ssh, o senza passphrase, accede da sola)

# draft
remote_login_no_config = non c’è una cartella di configurazione

# draft
remote_login_no_folder = non c’è una cartella in cui montare

# draft
remote_login_no_home = non c’è una cartella personale

# draft
remote_login_no_runtime = non c’è una cartella di runtime in cui montare

# draft
remote_login_no_schtasks = manca schtasks dell’Utilità di pianificazione

# draft
remote_login_no_systemd = systemd non è presente per connettere all’accesso (nessun systemctl)

# draft
remote_login_no_user = Windows non dice chi è l’utente (USERNAME)

# draft
remote_login_not_reached = non è stato raggiunto, per sapere se accede da solo: { $why }

# draft
remote_login_not_text = { $part } non è testo che un gestore dei servizi legge

# draft
remote_login_not_yet = La connessione all’accesso non è ancora disponibile su { $system }

# draft
remote_login_rclone_encrypted = la configurazione di rclone è cifrata: all’accesso nessuno può fornire la sua password

# draft
remote_login_rclone_no_remote = la configurazione di rclone non nomina alcun remote { $remote }

# draft
remote_login_rclone_not_yet = Il montaggio dei remote di rclone non è ancora disponibile su { $system }

# draft
remote_login_rclone_remotes_only = solo un remote della configurazione di rclone (rclone://nome/) si connette all’accesso tramite rclone: nessuno può fornire la password di un server

# draft
remote_login_sshfs_sftp_only = sshfs connette solo indirizzi SFTP

# draft
remote_login_unit_read_only = l’unità di accesso è di sola lettura; modificala dove è gestita

# draft
remote_login_unknown_host = ssh non conosce ancora l’identità del server: connettiti prima una volta

# draft
remote_lost = La connessione è persa: aprila per connetterti di nuovo

# draft
remote_macfuse_missing = macFUSE o FUSE-T non è installato

# draft
remote_mount_options_menu = Opzioni di montaggio…

# draft
remote_mounter_connected_already = è già connesso

# draft
remote_mounter_ended = è terminato ({ $status })

# draft
remote_mounter_no_runtime_folder = non c’è una cartella di runtime in cui montare

# draft
remote_mounter_no_runtime_folder_at_all = non c’è una cartella di runtime

# draft
remote_mounter_nothing_mounted = ha detto di aver montato, ma lì non è montato nulla

# draft
remote_mounter_trust = Connettersi a questo server?

# draft
remote_mounter_used_by = { $count ->
        [1] { $programs } lo sta usando
       *[other] { $programs } lo stanno usando
    }

# draft
remote_mounter_used_by_another = un altro programma lo sta usando

# draft
remote_mounter_you_cancelled = hai annullato

# draft
remote_nearby_browser_failed = { $finder }: { $error }

# draft
remote_nearby_empty = Nessun server trovato nelle vicinanze

# draft
remote_nearby_empty_detail = I server che si annunciano su questa rete (un NAS, un Mac, un PC Windows) compaiono qui, trovati da Avahi e wsdd mentre questa posizione è mostrata.

# draft
remote_nearby_failures = { $first }; { $then }

# draft
remote_nearby_no_runtime = nessuna cartella di runtime

# draft
remote_nearby_none_answers = I server vicini vengono trovati da Avahi o wsdd, e qui nessuno dei due risponde ({ $why })

# draft
remote_nearby_not_yet = La ricerca dei server vicini non è ancora disponibile su { $system }

# draft
remote_nearby_server_kind = server { $protocol }

# draft
remote_nearby_title = Dispositivi vicini
    .gender = masculine
    .definite = i Dispositivi vicini
    .di = dei Dispositivi vicini
    .in = nei Dispositivi vicini
    .da = dai Dispositivi vicini

# draft
remote_nearby_wsdd_ended = wsdd è terminato come è partito ({ $status }): potrebbe essere troppo vecchio per la scoperta

# draft
remote_nearby_wsdd_no_start = wsdd non è partito

# draft
remote_needs_gio = Apertura di indirizzi tramite GVfs, i remote del desktop

# draft
remote_needs_kmtp = Apertura di un telefono gestito da kmtpd di KDE

# draft
remote_needs_rclone = Apertura dei remote di rclone e di altri indirizzi

# draft
remote_needs_sshfs = Apertura di indirizzi SFTP tramite il tuo ssh

# draft
remote_no_password_kept = Skwir non conserva alcuna password per { $server }

# draft
remote_no_tool = nessuno strumento noto a Skwir apre indirizzi { $protocol }

# draft
remote_not_disconnected = { $name } non è stato disconnesso: { $error }

# draft
remote_not_installed = { $program } non è installato

# draft
remote_not_mounted_there = nessuna cartella remota è montata lì

# draft
remote_nothing_installed = nulla di ciò che è installato può aprirlo: { $why }

# draft
remote_options_bandwidth = Limite di banda

# draft
remote_options_bandwidth_help = quello di rclone: 10M, oppure uno per l’upload e uno per il download come 10M:1M; vuoto per nessun limite

# draft
remote_options_bandwidth_refused = Una banda è una velocità, come 10M o 512k, oppure una per l’upload e una per il download, come 10M:1M

# draft
remote_options_cache = Cache dei file

# draft
remote_options_cache_automatic = automatica

# draft
remote_options_cache_automatic_detail = { $cache } qui: { $why ->
        [wraps] cifra o incapsula altri remote, di cui full terrebbe i file su questo disco
        [near] è sulla rete locale
       *[far] ciò che viene letto viene riletto da questo disco
    }

# draft
remote_options_cache_full = Ciò che viene letto e scritto resta su questo disco per un’ora: riletto da lì, inviato dopo la chiusura, ritentato

# draft
remote_options_cache_help = Ciò che rclone conserva dei suoi file su questo disco

# draft
remote_options_cache_minimal = Ogni lettura interroga il server; un file scritto per intero è sul server una volta chiuso

# draft
remote_options_cache_off = Nulla resta su questo disco: alcuni programmi non possono scrivere

# draft
remote_options_cache_writes = I file scritti restano su questo disco e vengono inviati dopo la chiusura; ogni lettura interroga il server

# draft
remote_options_compression = Compressione

# draft
remote_options_compression_help = ssh comprime ciò che invia (sshfs): più veloce su una rete lenta, più lento su una veloce

# draft
remote_options_data = Le opzioni di montaggio scelte per ogni server, in base al suo indirizzo

# draft
remote_options_intro = Usate alla prossima connessione, da rclone o sshfs. GVfs e KIO montano come fa il desktop.

# draft
remote_options_read_only = Sola lettura

# draft
remote_options_read_only_help = Nulla viene scritto lì, da nessun programma

# draft
remote_options_save = Salva

# draft
remote_options_title = Opzioni di montaggio di { $server }

# draft
remote_password_for = Password per { $label }

# draft
remote_password_forgotten = La password è stata dimenticata

# draft
remote_password_not_forgotten = La password non è stata dimenticata: { $error }

# draft
remote_password_not_kept = La password non è stata memorizzata: { $why }

# draft
remote_plugin_description = Apre indirizzi SFTP, SMB, WebDAV, FTP, NFS e rclone come cartelle, tramite sshfs, GVfs, rclone o i montaggi del sistema; telefoni e fotocamere tramite GVfs

# draft
remote_plugin_title = Cartelle remote

# draft
remote_provider_agent_connects = Un agente chiede di connettersi a { $host }

# draft
remote_provider_agent_connects_detail = Ha chiesto di aprire { $address }. Connettersi dice al server chi sei.

# draft
remote_provider_agent_opens = Un agente chiede di aprire { $label }

# draft
remote_provider_agent_opens_detail = Ha chiesto di aprire { $address }.

# draft
remote_provider_camera_on = Una fotocamera mostra i suoi file finché è accesa.

# draft
remote_provider_connecting = Connessione a { $label }…

# draft
remote_provider_not_allowed = non l’hai consentito

# draft
remote_provider_not_connected = Impossibile connettersi a { $label }: { $error }

# draft
remote_provider_not_opened = Impossibile aprire { $label }: { $error }

# draft
remote_provider_opening = Apertura di { $label }…

# draft
remote_provider_phone_unlocked = Un telefono mostra i suoi file una volta sbloccato.

# draft
remote_provider_quitting = Skwir si sta chiudendo

# draft
remote_provider_window_asks = Se il server chiede una password, o di essere considerato attendibile la prima volta, te lo chiede la finestra.

# draft
remote_rclone_add = Aggiungi

# draft
remote_rclone_add_kind = Aggiungi { $kind }

# draft
remote_rclone_add_server = Aggiungi server

# draft
remote_rclone_add_server_intro = Un server in base al suo indirizzo, conservato nell’elenco di Skwir o nelle cartelle di rete di KDE, oppure un remote di rclone, conservato nella configurazione di rclone, dove lo trova anche rclone in un terminale.

# draft
remote_rclone_answer = Risposta

# draft
remote_rclone_config_open = Apri

# draft
remote_rclone_config_password = Password della configurazione di rclone

# draft
remote_rclone_config_unsaid = rclone non ha detto dove si trova la sua configurazione

# draft
remote_rclone_connected_already = è già connesso

# draft
remote_rclone_connection_refused = il server ha rifiutato la connessione

# draft
remote_rclone_continue = Continua

# draft
remote_rclone_edit = Modifica { $remote }

# draft
remote_rclone_include = Includi { $remote }

# draft
remote_rclone_keep_it = Mantieni

# draft
remote_rclone_keeps_no_link = { $path }: rclone non conserva collegamenti

# draft
remote_rclone_kind_detail = { $kind }: un remote di rclone

# draft
remote_rclone_kind_field = Tipo di archiviazione

# draft
remote_rclone_login_refused = il server ha rifiutato l’accesso (password errata?)

# draft
remote_rclone_login_refused_rc = il suo accesso è stato rifiutato

# draft
remote_rclone_mount_late = rclone non l’ha montato in tempo

# draft
remote_rclone_name = Nome

# draft
remote_rclone_name_end = Il nome di un remote non termina con uno spazio

# draft
remote_rclone_name_help = Come lo chiamano la barra laterale e rclone: lettere a–z, cifre e _ - . +

# draft
remote_rclone_name_letters = Il nome di un remote contiene solo lettere, cifre, spazi e _ - . + @

# draft
remote_rclone_name_needed = Un remote ha bisogno di un nome

# draft
remote_rclone_name_start = Il nome di un remote non inizia né con - né con uno spazio

# draft
remote_rclone_name_taken = rclone ha già un remote chiamato { $remote }

# draft
remote_rclone_next = Avanti

# draft
remote_rclone_no_answer = { $remote } non risponde

# draft
remote_rclone_no_kind = rclone non ha un tipo di archiviazione { $kind }

# draft
remote_rclone_no_link = rclone non ha creato alcun collegamento

# draft
remote_rclone_no_log_folder = nessuna cartella per il suo registro

# draft
remote_rclone_no_mounts_folder = non c’è una cartella in cui montare

# draft
remote_rclone_no_option = { $kind } non ha un’opzione { $option }

# draft
remote_rclone_no_reason = nessun motivo indicato

# draft
remote_rclone_no_remote = nessun remote montato

# draft
remote_rclone_no_remote_named = la configurazione di rclone non contiene alcun remote chiamato { $remote }

# draft
remote_rclone_no_runtime_folder = nessuna cartella di runtime

# draft
remote_rclone_no_such_host = il nome del server non è stato trovato

# draft
remote_rclone_no_such_name = rclone non accetta un nome simile

# draft
remote_rclone_no_such_remote = la configurazione di rclone non contiene un remote simile

# draft
remote_rclone_not_asked = Skwir non l’ha montato, quindi non si può interrogare rclone

# draft
remote_rclone_not_obscured = rclone non ha offuscato la password

# draft
remote_rclone_not_ours = Skwir non l’ha montato, quindi non può rilasciarlo

# draft
remote_rclone_not_reached = rclone non è stato raggiunto per rilasciarlo

# draft
remote_rclone_not_serving = rclone non ha iniziato a servire la sua configurazione

# draft
remote_rclone_ok = OK

# draft
remote_rclone_on_remotes = Su altri remote

# draft
remote_rclone_option_advanced = { $option } è un’opzione avanzata: l’utente la imposta nella finestra

# draft
remote_rclone_option_secret = { $option } è un segreto: l’utente lo digita nella finestra

# draft
remote_rclone_provider = Provider

# draft
remote_rclone_rc_login = rclone ha rifiutato { $method }: il suo accesso è stato rifiutato

# draft
remote_rclone_rc_no_http = la risposta di rclone a { $method } non è HTTP

# draft
remote_rclone_rc_refused = rclone ha rifiutato { $method }: { $why }

# draft
remote_rclone_remove = Rimuovi

# draft
remote_rclone_remove_intro = La configurazione di rclone dimentica { $remote }: ciò che contiene resta dov’è e aggiungerlo di nuovo lo ripristina.

# draft
remote_rclone_remove_mounted_intro = La configurazione di rclone dimentica { $remote }: ciò che contiene resta dov’è e aggiungerlo di nuovo lo ripristina. Skwir lo disconnette prima.

# draft
remote_rclone_remove_title = Rimuovere { $remote }?

# draft
remote_rclone_save = Salva

# draft
remote_rclone_scheme = rclone non apre indirizzi { $scheme }

# draft
remote_rclone_server_label = { $name } ({ $kind })

# draft
remote_rclone_set_up = Configura { $remote }

# draft
remote_rclone_sign_in = Accedi a { $remote }

# draft
remote_rclone_sign_in_page = Apri la pagina di accesso

# draft
remote_rclone_sign_in_waiting = Completa l’accesso nel browser: rclone attende la risposta. Se non si è aperta alcuna pagina, aprila da qui.

# draft
remote_rclone_stopped = rclone si è fermato

# draft
remote_rclone_storage = Archiviazione

# draft
remote_rclone_trash_box = il cestino di Box

# draft
remote_rclone_trash_drive = il cestino di Google Drive

# draft
remote_rclone_trash_jottacloud = il cestino di Jottacloud

# draft
remote_rclone_trash_mega = il cestino di MEGA

# draft
remote_rclone_trash_onedrive = il cestino di OneDrive

# draft
remote_rclone_trash_pcloud = il cestino di pCloud

# draft
remote_rclone_trash_yandex = il cestino di Yandex Disk

# draft
remote_rclone_trying = Prova di { $remote }

# draft
remote_rclone_trying_what = rclone elenca la sua radice, come farebbe la connessione.

# draft
remote_rclone_unlisted = rclone non ha potuto elencarlo: { $why }. Mantienilo così com’è, oppure torna indietro e modificalo.

# draft
remote_rclone_unlisted_new = rclone non ha potuto elencarlo: { $why }. Mantienilo così com’è, oppure torna indietro e modificalo. Il pulsante Annulla lo rimuove.

# draft
remote_rclone_unmount_late = rclone non l’ha rilasciato

# draft
remote_rclone_unsaid = { $path } non può essere detto a rclone

# draft
remote_rclone_you_cancelled = hai annullato

# draft
remote_refresh = Aggiorna cartelle remote

# draft
remote_refresh_description = Rilegge ciò che è montato e quali server conoscono gli strumenti

# draft
remote_remembered_data = I server la cui password è conservata nel portachiavi, in base al loro indirizzo

# draft
remote_reopen_connect = Connetti a { $name }

# draft
remote_reopen_lost = La connessione a { $name } è persa

# draft
remote_reopen_not_connected = { $name } non è connesso

# draft
remote_rsync_between = rsync copia tra questa macchina e un server

# draft
remote_rsync_failed = rsync non è riuscito ({ $status }): { $said }

# draft
remote_rsync_unnamed = { $path } ha un nome che rsync non può ricevere

# draft
remote_runs_failed = { $tool } non è riuscito ({ $status }): { $said }

# draft
remote_sending = Invio di { $count ->
        [one] { $count } file
       *[other] { $count } file
    } al server…

# draft
remote_server_form_add = Aggiungi

# draft
remote_server_form_add_afp = Aggiungi volume AFP

# draft
remote_server_form_add_dav = Aggiungi server WebDAV

# draft
remote_server_form_add_davs = Aggiungi server WebDAV (HTTPS)

# draft
remote_server_form_add_ftp = Aggiungi server FTP

# draft
remote_server_form_add_ftps = Aggiungi server FTP (TLS)

# draft
remote_server_form_add_intro = Un server in base al suo indirizzo. Con rclone installato, settanta tipi di archiviazione in più.

# draft
remote_server_form_add_nfs = Aggiungi esportazione NFS

# draft
remote_server_form_add_other = Aggiungi { $kind }

# draft
remote_server_form_add_server = Aggiungi server

# draft
remote_server_form_add_sftp = Aggiungi server SFTP

# draft
remote_server_form_add_smb = Aggiungi condivisione Windows

# draft
remote_server_form_afp = Volume AFP

# draft
remote_server_form_afp_detail = Il volume condiviso di un vecchio Mac o NAS

# draft
remote_server_form_already = { $server } è già un server, a quell’indirizzo

# draft
remote_server_form_dav = Server WebDAV

# draft
remote_server_form_dav_detail = Nextcloud, ownCloud e altri, tramite HTTP

# draft
remote_server_form_davs = Server WebDAV (HTTPS)

# draft
remote_server_form_davs_detail = Nextcloud, ownCloud e altri

# draft
remote_server_form_domain = Dominio

# draft
remote_server_form_domain_help = Il dominio o gruppo di lavoro Windows, dove il server ne richiede uno

# draft
remote_server_form_edit = Modifica { $server }

# draft
remote_server_form_edit_intro = Conservato { $kept }. Le sue opzioni di montaggio lo seguono a un nuovo indirizzo.

# draft
remote_server_form_export = Esportazione

# draft
remote_server_form_export_help = Il percorso della cartella esportata sul server: /srv/photos

# draft
remote_server_form_folder = Cartella

# draft
remote_server_form_folder_help = Dove aprire, sul server: vuoto per la sua radice (SFTP: la tua cartella personale)

# draft
remote_server_form_ftp = Server FTP

# draft
remote_server_form_ftp_detail = I file di un server tramite FTP

# draft
remote_server_form_ftps = Server FTP (TLS)

# draft
remote_server_form_ftps_detail = I file di un server tramite FTP con TLS

# draft
remote_server_form_host = Server

# draft
remote_server_form_host_help = Il suo nome o indirizzo: nas.local, files.example.com, 192.168.1.20

# draft
remote_server_form_host_needed = Serve un server

# draft
remote_server_form_intro_kde = Conservato nelle cartelle di rete di KDE, dove lo elenca anche Dolphin. La sua password viene richiesta alla prima connessione.

# draft
remote_server_form_intro_skwir = Conservato nell’elenco dei server di Skwir. La sua password viene richiesta alla prima connessione.

# draft
remote_server_form_kind = Tipo di server

# draft
remote_server_form_name = Nome

# draft
remote_server_form_name_help = Come lo chiama la barra laterale; il nome del suo indirizzo, se vuoto

# draft
remote_server_form_next = Avanti

# draft
remote_server_form_nfs = Esportazione NFS

# draft
remote_server_form_nfs_detail = La cartella condivisa di un server Unix

# draft
remote_server_form_no_part = l’indirizzo di un server non ha la parte { $key }

# draft
remote_server_form_port = Porta

# draft
remote_server_form_port_refused = Una porta è un numero da 1 a 65535

# draft
remote_server_form_save = Salva

# draft
remote_server_form_section = Server, in base al loro indirizzo

# draft
remote_server_form_sftp = Server SFTP

# draft
remote_server_form_sftp_detail = Un computer raggiunto tramite ssh

# draft
remote_server_form_share = Condivisione

# draft
remote_server_form_share_help = Il nome della cartella condivisa, e una sua cartella dopo una /

# draft
remote_server_form_smb = Condivisione Windows (SMB)

# draft
remote_server_form_smb_detail = La cartella condivisa di un NAS o di un computer

# draft
remote_server_form_user = Utente

# draft
remote_server_form_user_help = Il nome con cui accedi; vuoto per il tuo (SFTP: come dice la tua configurazione ssh)

# draft
remote_server_form_volume = Volume

# draft
remote_server_form_volume_help = Il nome del volume condiviso, e una sua cartella dopo una /

# draft
remote_servers_kde_read_only = La cartella di rete di KDE è di sola lettura; modificala dove è gestita

# draft
remote_servers_kept_automatic = Automatico

# draft
remote_servers_kept_dolphin = una delle posizioni di Dolphin

# draft
remote_servers_kept_fstab = nel fstab del sistema

# draft
remote_servers_kept_in = Dove sono conservati i server

# draft
remote_servers_kept_in_description = Dove viene conservato un server aggiunto in base al suo indirizzo.

# draft
remote_servers_kept_in_description_more = Automatico: nelle cartelle di rete di KDE sul desktop KDE, dove li elenca anche Dolphin, e nell’elenco di Skwir altrove; un tipo che KIO non apre (FTPS, AFP) sempre in quello di Skwir.

# draft
remote_servers_kept_kde = nelle cartelle di rete di KDE

# draft
remote_servers_kept_rclone = nella configurazione di rclone

# draft
remote_servers_kept_skwir = nell’elenco di Skwir

# draft
remote_servers_kio_opens_no = KIO non apre indirizzi { $protocol }

# draft
remote_servers_list = I server aggiunti in base al loro indirizzo che Skwir conserva da sé: il nome e l’indirizzo di ciascuno

# draft
remote_servers_no_kde_folder = non c’è una cartella dati per le cartelle di rete di KDE

# draft
remote_servers_none_kept = nessun server è conservato a { $address }

# draft
remote_servers_other_there = un altro server è conservato a quell’indirizzo

# draft
remote_servers_taken = { $name } è già un server, a quell’indirizzo

# draft
remote_sftp_from_here = sftp copia da questa macchina su un server, o all’interno di uno

# draft
remote_sftp_onto_server = sftp copia su un server

# draft
remote_sftp_unsaid = { $path } non può essere detto a sftp

# draft
remote_share_label = { $share } su { $server }

# draft
remote_ssh_still_there = { $path } è ancora lì

# draft
remote_sshfs_connection_refused = il server ha rifiutato la connessione

# draft
remote_sshfs_identity_changed = l’identità del server è cambiata dall’ultima volta che è stata vista: ssh la rifiuta (vedi ~/.ssh/known_hosts)

# draft
remote_sshfs_identity_unconfirmed = l’identità del server non è stata confermata

# draft
remote_sshfs_login_refused = il server ha rifiutato l’accesso (password errata, o nessuna chiave che accetti)

# draft
remote_sshfs_name_not_found = il nome del server non è stato trovato

# draft
remote_sshfs_no_answer = il server non ha risposto

# draft
remote_sshfs_not_windows = sshfs non funziona su Windows

# draft
remote_system_shares_elsewhere = le condivisioni native del sistema sono quelle di Windows e macOS: qui le montano sshfs, GVfs, KIO o rclone

# draft
remote_system_shares_login_refused = il server ha rifiutato l’accesso

# draft
remote_system_shares_no_share = { $address } non indica alcuna condivisione: smb://server/condivisione

# draft
remote_thumbnails = Miniature nelle cartelle remote

# draft
remote_thumbnails_always = Sempre

# draft
remote_thumbnails_automatic = Automatico

# draft
remote_thumbnails_choice = Miniature

# draft
remote_thumbnails_description = Per un remote per cui non hai scelto nulla nel suo menu: Automatico le mostra solo sulla rete locale, perché leggere ogni immagine attraverso internet sarebbe lento.

# draft
remote_thumbnails_menu_always = Sempre

# draft
remote_thumbnails_menu_automatic = Automatico

# draft
remote_thumbnails_menu_never = Mai

# draft
remote_thumbnails_menu_small = File piccoli

# draft
remote_thumbnails_never = Mai

# draft
remote_thumbnails_not_saved = La scelta di quando i file del remote mostrano le miniature non è stata salvata: { $error }

# draft
remote_thumbnails_of = Miniature di un remote

# draft
remote_thumbnails_of_description = Sceglie quando i file di una cartella remota mostrano le miniature: automatic, never, small o always

# draft
remote_thumbnails_rule = una regola è automatic, never, small o always

# draft
remote_thumbnails_small = File piccoli

# draft
remote_thumbnails_submenu = Miniature

# draft
remote_tool_failed = { $tool }: { $error }

# draft
remote_tool_unknown = Skwir non conosce lo strumento che l’ha montato

# draft
remote_tools_failed = { $failures }

# draft
remote_unfinished_data = I remote di rclone che Skwir ha iniziato a configurare e non ha finito, in base al nome

# draft
remote_uploads_mount_gone = il montaggio di rclone di { $path } è sparito prima che il suo server avesse ciò che era stato scritto

# draft
remote_uploads_never_queued = { $path } non è mai arrivato nella coda di rclone: un programma lo tiene ancora aperto lì e il server non lo ha

# draft
remote_uploads_no_answer = rclone non ha risposto entro { $seconds } s: il suo server potrebbe aver smesso di rispondere

# draft
remote_uploads_no_cache_mode = rclone non ha detto quale cache tiene

# draft
remote_uploads_refused = il server ha rifiutato { $name } { $tries ->
        [one] { $tries } volta
       *[other] { $tries } volte
    }: rclone lo tiene su questo disco e riprova

# draft
remote_winfsp_loopback = un montaggio su Windows risponde sul loopback

# draft
remote_winfsp_missing = WinFsp non è installato: su Windows rclone monta tramite esso

# draft
remote_winfsp_no_login_folder = nessuna cartella per il suo accesso
