# Italian catalog of skwir-platform, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
access_acl = Elenco di accesso

# draft
access_acl_default = Predefinito per i nuovi elementi

# draft
access_acl_group = gruppo { $group }: { $permissions }

# draft
access_acl_mask = al massimo { $permissions }

# draft
access_acl_others = altri: { $permissions }

# draft
access_acl_owner = proprietario: { $permissions }

# draft
access_acl_owning_group = gruppo: { $permissions }

# draft
access_acl_user = utente { $user }: { $permissions }

# draft
access_capabilities = Capacità

# draft
apps_launch_failed = avvio di { $app }: { $error }

# draft
apps_list_read_only = l’elenco delle applicazioni predefinite è di sola lettura; modificalo dove viene gestito

# draft
apps_no_command = { $app } non ha alcun comando da eseguire

# draft
apps_no_config_dir = nessuna cartella di configurazione

# draft
archive_changed = L’archivio è cambiato dopo essere stato elencato; selezionalo di nuovo

# draft
archive_gone = L’archivio non esiste più

# draft
archive_handing_failed = la consegna dei file non è riuscita

# draft
archive_helper_exited = l’assistente degli archivi è terminato in modo imprevisto

# draft
archive_helper_failed = l’assistente degli archivi non è riuscito ({ $status })

# draft
archive_helper_not_started = l’assistente degli archivi non è stato avviato

# draft
archive_helper_timed_out = l’assistente degli archivi ha impiegato troppo tempo

# draft
archive_install = Installa { $program } per aprire gli archivi

# draft
archive_member_missing = Elemento dell’archivio mancante

# draft
archive_member_nowhere = Nessun posto in cui scrivere l’elemento

# draft
archive_member_too_large = L’anteprima dell’elemento supera il limite di { $size }

# draft
archive_nowhere = Nessun posto in cui scrivere l’archivio

# draft
archive_part_changed = { $name } è cambiato durante la lettura

# draft
archive_part_unread = { $name } non ha potuto essere letto

# draft
archive_password_zip_only = Solo un archivio ZIP può avere una password

# draft
archive_quarantine_unheld = { $name } non ha potuto essere messo in quarantena

# draft
archive_quarantine_unread = la quarantena dell’archivio non ha potuto essere letta

# draft
askpass_unsupported = la richiesta di una password tramite ssh non è ancora disponibile su questo sistema

# draft
desktop_dock_percent = { $percent }%

# draft
desktop_notice_show = Mostra

# draft
desktop_notice_stop = Interrompi tutto

# draft
desktop_open = Apri Skwir

# draft
desktop_style_portal = il portale

# draft
desktop_tray_go_on = Riprendi

# draft
desktop_tray_open = Apri Skwir

# draft
desktop_tray_pause = Sospendi tutto

# draft
desktop_tray_stop = Interrompi tutto ed esci

# draft
diagram_failed = Impossibile disegnare il diagramma: { $said }

# draft
diagram_install = Installa { $program } per disegnare i diagrammi

# draft
diagram_too_long = Il diagramma è troppo lungo per essere disegnato

# draft
document_changed = documento cambiato; riapri l’anteprima

# draft
document_compose_folder = una cartella in cui comporre il documento

# draft
document_compose_keep = conservare il documento composto

# draft
document_locate_too_long = il testo da cercare supera il limite

# draft
document_reopen = il processo dei documenti è stato riavviato ma non ha aperto il documento

# draft
document_search_too_long = la ricerca supera il limite

# draft
document_worker_exited = il processo dei documenti è terminato in modo imprevisto

# draft
document_worker_stopped = il processo dei documenti si è fermato durante l’avvio

# draft
document_worker_timed_out = il processo dei documenti ha impiegato troppo tempo

# draft
exif_aperture = Apertura

# draft
exif_aperture_value = f/{ $f }

# draft
exif_artist = Artista

# draft
exif_camera = Fotocamera

# draft
exif_copyright = Copyright

# draft
exif_exposure = Esposizione

# draft
exif_exposure_fraction = 1/{ $denominator } s

# draft
exif_exposure_seconds = { $seconds } s

# draft
exif_flash = Flash

# draft
exif_focal_length = Lunghezza focale

# draft
exif_focal_length_value = { $mm } mm

# draft
exif_iso = ISO

# draft
exif_latitude_longitude = { $latitude }, { $longitude }

# draft
exif_lens = Obiettivo

# draft
exif_orientation = Orientamento

# draft
exif_position = Posizione

# draft
exif_software = Software

# draft
exif_taken = Scattata

# draft
file_manager_not_yet = Rispondere ad altri programmi non è ancora disponibile su { $system }

# draft
file_no_folder = { $path } non ha una cartella

# draft
git_bare = Un repository bare non ha file estratti

# draft
git_history_failed = Impossibile leggere la cronologia: { $said }

# draft
git_install_history = Installa { $program } per mostrare la cronologia dei commit

# draft
git_install_status = Installa { $program } per mostrare lo stato Git dei file

# draft
git_not_commit = Non è un commit: { $id }

# draft
git_patch_not_written = La patch non è stata scritta

# draft
git_status_failed = Impossibile leggere lo stato Git dei file: { $said }

# draft
grammars_config_invalid = la sua configurazione non è un JSON valido ({ $error }), quindi nessuna delle cartelle dei parser che indica viene letta

# draft
grammars_config_unreadable = la sua configurazione non può essere letta, quindi nessuna delle cartelle dei parser che indica viene letta

# draft
grammars_emacs_no_colors = Emacs non conserva colori con le sue grammatiche: metti il repository della grammatica nella cartella delle grammatiche di Skwir (non occorre compilarlo)

# draft
grammars_library_outside = la sua libreria { $library } si trova fuori dalla sua cartella

# draft
grammars_library_refused = { $source ->
        [skwir] una libreria nella cartella di Skwir
        [tree-sitter] la libreria della CLI di tree-sitter
        [zed] la libreria di Zed
        [helix] la libreria di Helix
        [neovim] la libreria di Neovim
        [emacs] la libreria di Emacs
       *[system] la libreria di sistema
    } non può essere usata ({ $why })

# draft
grammars_no_highlights = nessuna query di evidenziazione trovata per il suo linguaggio: metti il repository della grammatica nella cartella delle grammatiche di Skwir (non occorre compilarlo)

# draft
grammars_no_highlights_repository = nessuna query di evidenziazione nel suo repository, né in quelle del sistema

# draft
grammars_no_library = nessuna libreria: compila la grammatica (`tree-sitter build`)

# draft
grammars_parser_dir_missing = la sua configurazione indica questa cartella dei parser, che non esiste

# draft
grammars_queries_not_lent = nessuna query installata con essa, e quelle di { $editor ->
        [zed] Zed
        [helix] Helix
        [emacs] Emacs
       *[neovim] Neovim
    } non vengono prestate alla libreria di un altro: metti il repository della grammatica nella cartella delle grammatiche di Skwir (non occorre compilarlo)

# draft
grammars_repository_queries_refused = le query del suo repository non si adattano a { $source ->
        [skwir] una libreria nella cartella di Skwir
        [tree-sitter] la libreria della CLI di tree-sitter
        [zed] la libreria di Zed
        [helix] la libreria di Helix
        [neovim] la libreria di Neovim
        [emacs] la libreria di Emacs
       *[system] la libreria di sistema
    } ({ $why }): estrai il repository alla revisione da cui è stata compilata la sua libreria

# draft
grammars_source_label = { $source ->
        [skwir] Skwir
        [tree-sitter] la CLI di tree-sitter
        [zed] Zed
        [helix] Helix
        [neovim] Neovim
        [emacs] Emacs
       *[system] il sistema
    }

# draft
grammars_source_short = { $source ->
        [skwir] Cartella di Skwir
        [tree-sitter] CLI di tree-sitter
        [zed] Zed
        [helix] Helix
        [neovim] Neovim
        [emacs] Emacs
       *[system] Sistema
    }

# draft
grammars_system_queries_refused = le query del sistema non si adattano a { $source ->
        [skwir] una libreria nella cartella di Skwir
        [tree-sitter] la libreria della CLI di tree-sitter
        [zed] la libreria di Zed
        [helix] la libreria di Helix
        [neovim] la libreria di Neovim
        [emacs] la libreria di Emacs
       *[system] la libreria di sistema
    } ({ $why }): metti il repository della grammatica, alla revisione da cui è stata compilata la sua libreria, nella cartella delle grammatiche di Skwir

# draft
grammars_too_deep_cli = una cartella troppo in profondità: legge solo le cartelle direttamente in { $folder }; aggiungi { $inner } al suo parser-directories, oppure collega il repository alla cartella delle grammatiche di Skwir

# draft
grammars_too_deep_skwir = una cartella troppo in profondità: Skwir legge solo le cartelle direttamente al suo interno ({ $folder }); sposta o collega il repository lì

# draft
highlight_bat_first = Prima bat

# draft
highlight_block_too_long = un blocco più lungo di { $size }

# draft
highlight_default = Predefinito

# draft
highlight_not_found = { $theme } (non trovato)

# draft
highlight_tree_sitter_first = Prima tree-sitter

# draft
http_broke_off = la connessione al server si è interrotta

# draft
http_no_route = la rete non raggiunge il server

# draft
http_not_address = non è un indirizzo

# draft
http_not_secure = non è stato possibile stabilire una connessione sicura con il server

# draft
http_refused = il server ha rifiutato la connessione

# draft
http_stopped = le richieste si sono interrotte

# draft
http_timed_out = il server non ha risposto in tempo

# draft
http_too_large = la risposta del server è più grande del previsto

# draft
http_unreachable = non è stato possibile raggiungere il server

# draft
inspection_blocks_notice = { $count ->
        [1] Viene mostrato solo il primo blocco · Sorgente mostra il resto
       *[other] Vengono mostrati solo i primi { $count } blocchi · Sorgente mostra il resto
    }

# draft
inspection_count = { $count }

# draft
inspection_drawing_inflated = Il disegno è troppo grande per essere letto

# draft
inspection_drawing_too_large = Il disegno supera il limite di anteprima di { $size }

# draft
inspection_embedded_image_limit = Un’immagine incorporata supera il limite di anteprima di { $size }

# draft
inspection_hex_offset = Offset

# draft
inspection_image_too_large = L’immagine incorporata supera { $size }

# draft
inspection_lines = { $count ->
        [one] { $count } riga
       *[other] { $count } righe
    }

# draft
inspection_not_regular = l’anteprima richiede un file regolare

# draft
inspection_read_as = Letto come { $encoding }

# draft
inspection_show_on_map = Mostra sulla mappa

# draft
inspection_svg_no_size = L’immagine SVG non ha dimensioni

# draft
inspection_text_too_large = Il testo supera il limite di anteprima di { $size }

# draft
inspection_xml_doctype = il suo tipo di documento non viene letto

# draft
inspection_xml_entities = le sue entità si espandono oltre { $size }

# draft
inspection_xml_too_deep = annida i suoi elementi più in profondità di { $count ->
        [one] { $count } livello
       *[other] { $count } livelli
    }

# draft
line_channel_closed = il processo ha chiuso le sue richieste

# draft
line_channel_not_reading = il processo non sta leggendo le sue richieste

# draft
line_channel_start = avvio di { $program }

# draft
math_failed = Impossibile comporre le formule: { $said }

# draft
math_install = Installa { $program } per comporre le formule

# draft
media_control_refused = Il lettore non ha potuto eseguire l’operazione: { $why }

# draft
media_controls_busy = controlli video occupati o sessione chiusa

# draft
media_frame_slot = creare lo spazio per i fotogrammi del video

# draft
media_worker_exited = il processo video è terminato

# draft
media_worker_stopped = il processo video si è fermato durante l’avvio

# draft
media_worker_timed_out = il processo video ha impiegato troppo tempo

# draft
mime_kind_archives = Archivi

# draft
mime_kind_audio = Musica e audio

# draft
mime_kind_documents = Documenti

# draft
mime_kind_folders = Cartelle

# draft
mime_kind_fonts = Caratteri

# draft
mime_kind_images = Immagini

# draft
mime_kind_presentations = Presentazioni

# draft
mime_kind_programs = Programmi

# draft
mime_kind_spreadsheets = Fogli di calcolo

# draft
mime_kind_text = Testo e codice

# draft
mime_kind_videos = Video

# draft
mime_unnamed = File

# draft
mime_unnamed_extension = File { $extension }

# draft
model_failed = Impossibile disegnare il modello: { $said }

# draft
model_install = Installa { $program } per disegnare i modelli 3D

# draft
mp4_position = Posizione

# draft
open_launch_failed = avvio di { $program }: { $error }

# draft
open_no_command = nessun comando da eseguire

# draft
open_no_terminal = nessun emulatore di terminale trovato (imposta $TERMINAL)

# draft
open_not_geo = non è una posizione sulla mappa

# draft
open_not_web_or_mail = da qui si aprono solo collegamenti web e di posta

# draft
open_path_failed = apertura di { $path }: { $error }

# draft
open_terminal_program_not_yet = Un terminale che esegue un programma non è ancora disponibile su { $system }

# draft
places_bitlocker_asks = Windows chiede da sé la password di un’unità BitLocker: apri l’unità

# draft
places_bookmarks_read_only = il file dei segnalibri è di sola lettura; modificalo dove viene gestito

# draft
places_diskutil_no_mount_point = diskutil non ha indicato dove ha montato il volume

# draft
places_diskutil_unrun = impossibile eseguire diskutil: { $error }

# draft
places_drive = { $label } ({ $letter })

# draft
places_local_disk = Disco locale ({ $letter })

# draft
places_mount_not_yet = Il montaggio di un volume non è ancora disponibile su { $system }

# draft
places_no_config_dir = nessuna cartella di configurazione

# draft
places_passphrase_refused = La passphrase non è stata accettata

# draft
places_root = Radice
    .gender = feminine
    .definite = la Radice
    .di = della Radice
    .in = nella Radice
    .da = dalla Radice

# draft
places_system_asks_not = Questo sistema non chiede da sé la passphrase di un volume

# draft
places_udisks_no_device = udisksctl non ha indicato il dispositivo sbloccato

# draft
places_udisks_no_mount_point = udisksctl non ha indicato dove ha montato il volume

# draft
places_udisks_unrun = impossibile eseguire udisksctl: { $error }

# draft
portals_changed_since = il file è stato modificato dopo che Skwir lo ha scritto, e resta com’è

# draft
portals_linked = è un collegamento a { $target }, che è gestito da un altro programma: scegli lì

# draft
portals_no_config_folder = Non c’è alcuna cartella di configurazione in cui scrivere la scelta

# draft
portals_not_utf8 = non è scritto in UTF-8

# draft
preview_changed = file cambiato durante l’anteprima

# draft
preview_changed_converting = file cambiato durante la conversione

# draft
preview_changed_opening = file cambiato durante l’apertura del documento

# draft
preview_install_audio = Installa { $program } e i plugin di riproduzione di GStreamer per riprodurlo

# draft
preview_install_engine = Installa { $program } e la sua libreria per leggere i documenti con { $engine }

# draft
preview_install_video = Installa { $program } e i plugin di riproduzione di GStreamer per riprodurlo, oppure scegli Fotogrammi fissi come motore video

# draft
preview_no_pages = Il PDF non ha pagine leggibili

# draft
preview_office_no_pdf = LibreOffice non ha prodotto un PDF (il file potrebbe essere cifrato o non supportato)

# draft
preview_office_not_pdf = la conversione non ha prodotto un PDF

# draft
preview_output_too_large = l’output dell’anteprima è troppo grande

# draft
preview_pdfium_library_missing = la libreria di PDFium non si trova in { $path }, dove le impostazioni dicono che sia (Impostazioni › Lettura › Documenti › Libreria PDFium)

# draft
preview_too_wide = le dimensioni dell’anteprima superano il limite

# draft
preview_unknown_backend = motore PDF sconosciuto; scegli auto, pdfium, poppler o mupdf

# draft
print_accept = Stampa

# draft
print_dialog_ended = la finestra di stampa è terminata ({ $code })

# draft
print_dialog_pages = le pagine della finestra di stampa { $ranges }: { $refused }

# draft
print_dialog_stopped = la finestra di stampa si è interrotta

# draft
print_job_empty_pdf = l’assistente di stampa ha scritto un PDF vuoto

# draft
print_job_failed = Impossibile creare il PDF ({ $status }): { $said }

# draft
print_job_install = Installa { $program } per stampare

# draft
print_job_lines_cut = { $count ->
        [1] Viene stampata solo la prima riga
       *[other] Vengono stampate solo le prime { $count } righe
    }

# draft
print_job_no_pdf = l’assistente di stampa non ha scritto alcun PDF

# draft
print_job_silent = l’assistente di stampa non ha detto nulla

# draft
print_job_unwritten = { $path } non ha potuto essere scritto

# draft
print_needs_pdfium = la stampa richiede PDFium, che non è presente

# draft
print_no_dialog = questo desktop non mostra alcuna finestra di stampa (il suo portale di stampa: { $error })

# draft
print_no_token = la finestra di stampa non ha risposto con alcun token

# draft
print_not_taken = la stampa del desktop non l’ha accettato ({ $code })

# draft
print_not_yet = La stampa non è ancora disponibile su { $system }

# draft
print_portal_silent = il portale di stampa ha smesso di rispondere

# draft
print_portal_stopped = il portale di stampa si è interrotto

# draft
print_portal_unheard = non è possibile mettersi in ascolto del portale di stampa

# draft
print_refused = la stampante l’ha rifiutato: { $error }

# draft
print_settings_short = le impostazioni della stampante non sono complete

# draft
process_failed = { $program } non è riuscito ({ $status }): { $said }

# draft
process_not_installed = { $program } non è installato

# draft
process_older_build = { $error }. { $program } proviene da una build di Skwir più vecchia, il che potrebbe essere la causa: ricompilalo o reinstallalo

# draft
process_other_build = { $error }. { $program } proviene da un’altra build di Skwir ({ $theirs }; Skwir è { $ours }), il che potrebbe essere la causa: ricompilalo o reinstallalo

# draft
process_stalled_minutes = { $program } è stato interrotto: non ha detto nulla sui suoi progressi per { $count ->
        [one] { $count } minuto
       *[other] { $count } minuti
    }

# draft
process_stalled_seconds = { $program } è stato interrotto: non ha detto nulla sui suoi progressi per { $count ->
        [one] { $count } secondo
       *[other] { $count } secondi
    }

# draft
process_start = avvio di { $program }

# draft
process_timed_out = { $program } ha impiegato troppo tempo

# draft
process_too_much_input = { $program }: input troppo grande

# draft
process_too_much_output = { $program }: output eccessivo

# draft
recent_read_only = L’elenco dei file usati di recente è di sola lettura; modificalo dove viene gestito

# draft
rich_preview_page_no_size = la pagina non ha dimensioni

# draft
rich_preview_stylesheet_limit = Un foglio di stile supera il limite di anteprima di { $size }

# draft
sandbox_converter_not_yet = L’esecuzione di un convertitore isolato dai tuoi file non è ancora disponibile su { $system }

# draft
sandbox_needs_bubblewrap = Serve bubblewrap, per eseguirlo isolato dai tuoi file

# draft
sandbox_needs_sandbox = Serve la sandbox del sistema, per eseguirlo isolato dai tuoi file

# draft
sandbox_plugin_not_yet = L’esecuzione del programma di un plugin isolato dai tuoi file non è ancora disponibile su { $system }

# draft
secrets_no_keyring = Nessun portachiavi ha risposto: { $error }

# draft
secrets_not_text = Il segreto del portachiavi non è testo

# draft
secrets_unsupported = Nessun portachiavi è ancora supportato su questo sistema

# draft
shares_login_refused = il server ha rifiutato l’accesso

# draft
shares_netfs_not_yet = Il montaggio di una condivisione tramite NetFS non è ancora disponibile su { $system }

# draft
shares_no_address = { $address } non è un indirizzo che il sistema legga

# draft
shares_no_mount_point = NetFS non ha indicato alcun punto di montaggio

# draft
shares_windows_not_yet = Una condivisione Windows non è ancora disponibile su { $system }

# draft
shell_environment_failed = { $shell } è terminato con { $status }

# draft
shrink_copy_unread = la copia dell’assistente di riduzione non ha potuto essere letta

# draft
shrink_empty_copy = l’assistente di riduzione ha scritto una copia vuota

# draft
shrink_failed = Impossibile creare una copia ridotta ({ $status }): { $said }

# draft
shrink_gone = { $path } non c’è più

# draft
shrink_install = Installa { $program } per creare copie ridotte

# draft
shrink_no_copy = l’assistente di riduzione non ha scritto alcuna copia

# draft
shrink_no_smaller = l’assistente di riduzione ha scritto una copia non più piccola del file

# draft
shrink_silent = l’assistente di riduzione non ha detto nulla

# draft
shrink_unread = { $path } non ha potuto essere letto

# draft
shrink_unwritten = { $path } non ha potuto essere scritto

# draft
tree_sitter_ended = l’assistente tree-sitter è terminato

# draft
tree_sitter_ended_why = l’assistente tree-sitter è terminato: { $why }

# draft
tree_sitter_exited = l’assistente tree-sitter è terminato in modo imprevisto

# draft
tree_sitter_install = Installa { $program } per colorare il codice con le grammatiche tree-sitter

# draft
tree_sitter_no_answer = l’assistente tree-sitter non ha risposto entro { $count ->
        [one] { $count } secondo
       *[other] { $count } secondi
    }

# draft
tree_sitter_not_started = l’assistente tree-sitter non si è avviato entro { $count ->
        [one] { $count } secondo
       *[other] { $count } secondi
    }

# draft
tree_sitter_timed_out = l’assistente tree-sitter ha impiegato troppo tempo

# draft
tree_sitter_too_large = il testo supera { $size }

# draft
tree_sitter_too_many = le grammatiche trovate sono più di quante l’assistente tree-sitter ne riceva in una volta

# draft
windows_no_user = Windows non indica chi sia l’utente

# draft
windows_unlock_silent = la finestra di sblocco di Windows non ha risposto

# draft
windows_unlock_unopened = impossibile aprire la finestra di sblocco di Windows: { $error }
