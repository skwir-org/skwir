# French catalog of skwir-platform, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
access_acl = Liste d’accès

# draft
access_acl_default = Par défaut pour les nouveaux éléments

# draft
access_acl_group = groupe { $group } : { $permissions }

# draft
access_acl_mask = au plus { $permissions }

# draft
access_acl_others = autres : { $permissions }

# draft
access_acl_owner = propriétaire : { $permissions }

# draft
access_acl_owning_group = groupe : { $permissions }

# draft
access_acl_user = utilisateur { $user } : { $permissions }

# draft
access_capabilities = Capacités

# draft
apps_launch_failed = lancer { $app } : { $error }

# draft
apps_list_read_only = la liste des applications par défaut est en lecture seule ; modifiez-la là où elle est gérée

# draft
apps_no_command = { $app } n’indique aucune commande à lancer

# draft
apps_no_config_dir = aucun dossier de configuration

# draft
archive_changed = L’archive a changé depuis qu’elle a été listée ; sélectionnez-la de nouveau

# draft
archive_gone = L’archive n’est plus là

# draft
archive_handing_failed = la transmission des fichiers a échoué

# draft
archive_helper_exited = le programme auxiliaire d’archives s’est arrêté de façon inattendue

# draft
archive_helper_failed = le programme auxiliaire d’archives a échoué ({ $status })

# draft
archive_helper_not_started = le programme auxiliaire d’archives n’a pas pu démarrer

# draft
archive_helper_timed_out = le programme auxiliaire d’archives a dépassé le délai imparti

# draft
archive_install = Installez { $program } pour ouvrir les archives

# draft
archive_member_missing = Fichier absent de l’archive

# draft
archive_member_nowhere = Aucun dossier où écrire ce fichier de l’archive

# draft
archive_member_too_large = Ce fichier de l’archive dépasse la limite d’aperçu de { $size }

# draft
archive_nowhere = Aucun dossier où écrire l’archive

# draft
archive_part_changed = { $name } a changé pendant sa lecture

# draft
archive_part_unread = { $name } n’a pas pu être lu

# draft
archive_password_zip_only = Seule une archive ZIP peut avoir un mot de passe

# draft
archive_quarantine_unheld = { $name } n’a pas pu être mis en quarantaine

# draft
archive_quarantine_unread = la quarantaine de l’archive n’a pas pu être lue

# draft
askpass_unsupported = la demande d’un mot de passe par ssh n’est pas encore disponible sur ce système

# draft
desktop_dock_percent = { $percent } %

# draft
desktop_notice_show = Afficher

# draft
desktop_notice_stop = Tout arrêter

# draft
desktop_open = Ouvrir Skwir

# draft
desktop_style_portal = le portail

# draft
desktop_tray_go_on = Reprendre

# draft
desktop_tray_open = Ouvrir Skwir

# draft
desktop_tray_pause = Tout mettre en pause

# draft
desktop_tray_stop = Tout arrêter et quitter

# draft
diagram_failed = Le diagramme ne peut pas être dessiné : { $said }

# draft
diagram_install = Installez { $program } pour dessiner les diagrammes

# draft
diagram_too_long = Le diagramme est trop long pour être dessiné

# draft
document_changed = le document a changé ; rouvrez l’aperçu

# draft
document_compose_folder = un dossier où composer le document

# draft
document_compose_keep = garder le document composé

# draft
document_locate_too_long = le texte à trouver dépasse la longueur permise

# draft
document_reopen = le programme auxiliaire de documents, relancé, n’a pas pu rouvrir le document

# draft
document_search_too_long = la recherche dépasse la longueur permise

# draft
document_worker_exited = le programme auxiliaire de documents s’est arrêté de façon inattendue

# draft
document_worker_stopped = le programme auxiliaire de documents s’est arrêté pendant son démarrage

# draft
document_worker_timed_out = le programme auxiliaire de documents a dépassé le délai imparti

# draft
exif_aperture = Ouverture

# draft
exif_artist = Auteur

# draft
exif_camera = Appareil

# draft
exif_copyright = Droits d’auteur

# draft
exif_exposure = Temps de pose

# draft
exif_flash = Flash

# draft
exif_focal_length = Focale

# draft
exif_iso = ISO

# draft
exif_lens = Objectif

# draft
exif_orientation = Orientation

# draft
exif_position = Position

# draft
exif_software = Logiciel

# draft
exif_taken = Prise de vue

# draft
file_no_folder = { $path } n’est dans aucun dossier

# draft
git_bare = Un dépôt nu n’a aucun fichier extrait

# draft
git_history_failed = L’historique ne peut pas être lu : { $said }

# draft
git_install_history = Installez { $program } pour afficher l’historique des commits

# draft
git_install_status = Installez { $program } pour afficher l’état Git des fichiers

# draft
git_not_commit = Ce n’est pas un commit : { $id }

# draft
git_patch_not_written = Le patch n’a pas été écrit

# draft
git_status_failed = L’état Git des fichiers ne peut pas être lu : { $said }

# draft
grammars_config_invalid = sa configuration est illisible ({ $error }) : aucun de ses dossiers d’analyseurs n’est lu

# draft
grammars_config_unreadable = sa configuration est illisible (elle ne peut pas être lue) : aucun de ses dossiers d’analyseurs n’est lu

# draft
grammars_emacs_no_colors = Emacs ne garde aucune couleur avec ses grammaires : placez le dépôt de la grammaire dans le dossier des grammaires de Skwir (inutile de le compiler)

# draft
grammars_library_outside = sa bibliothèque { $library } se trouve hors de son dossier

# draft
grammars_library_refused = { $source ->
        [skwir] une bibliothèque du dossier de Skwir
        [tree-sitter] la bibliothèque de la CLI tree-sitter
        [zed] la bibliothèque de Zed
        [helix] la bibliothèque d’Helix
        [neovim] la bibliothèque de Neovim
        [emacs] la bibliothèque d’Emacs
       *[system] la bibliothèque du système
    } ne peut pas être utilisée ({ $why })

# draft
grammars_no_highlights = aucune requête highlights trouvée pour son langage : placez le dépôt de la grammaire dans le dossier des grammaires de Skwir (inutile de le compiler)

# draft
grammars_no_highlights_repository = aucune requête highlights dans son dépôt, ni dans celles du système

# draft
grammars_no_library = aucune bibliothèque : compilez la grammaire (`tree-sitter build`)

# draft
grammars_parser_dir_missing = un dossier d’analyseurs que nomme sa configuration, absent

# draft
grammars_queries_not_lent = aucune requête installée avec elle, et celles { $editor ->
        [zed] de Zed
        [helix] d’Helix
        [emacs] d’Emacs
       *[neovim] de Neovim
    } ne servent pas à la bibliothèque d’un autre : placez le dépôt de la grammaire dans le dossier des grammaires de Skwir (inutile de le compiler)

# draft
grammars_repository_queries_refused = les requêtes de son dépôt ne conviennent pas à { $source ->
        [skwir] une bibliothèque du dossier de Skwir
        [tree-sitter] la bibliothèque de la CLI tree-sitter
        [zed] la bibliothèque de Zed
        [helix] la bibliothèque d’Helix
        [neovim] la bibliothèque de Neovim
        [emacs] la bibliothèque d’Emacs
       *[system] la bibliothèque du système
    } ({ $why }) : ramenez le dépôt à la révision à partir de laquelle sa bibliothèque a été compilée

# draft
grammars_source_label = { $source ->
        [skwir] Skwir
        [tree-sitter] la CLI tree-sitter
        [zed] Zed
        [helix] Helix
        [neovim] Neovim
        [emacs] Emacs
       *[system] le système
    }

# draft
grammars_source_short = { $source ->
        [skwir] Dossier de Skwir
        [tree-sitter] CLI tree-sitter
        [zed] Zed
        [helix] Helix
        [neovim] Neovim
        [emacs] Emacs
       *[system] Système
    }

# draft
grammars_system_queries_refused = les requêtes du système ne conviennent pas à { $source ->
        [skwir] une bibliothèque du dossier de Skwir
        [tree-sitter] la bibliothèque de la CLI tree-sitter
        [zed] la bibliothèque de Zed
        [helix] la bibliothèque d’Helix
        [neovim] la bibliothèque de Neovim
        [emacs] la bibliothèque d’Emacs
       *[system] la bibliothèque du système
    } ({ $why }) : placez le dépôt de la grammaire, à la révision à partir de laquelle sa bibliothèque a été compilée, dans le dossier des grammaires de Skwir

# draft
grammars_too_deep_cli = un dossier trop profond : la CLI tree-sitter ne lit que les dossiers placés directement dans { $folder } ; ajoutez { $inner } à ses parser-directories, ou créez un lien vers le dépôt dans le dossier des grammaires de Skwir

# draft
grammars_too_deep_skwir = un dossier trop profond : Skwir ne lit que les dossiers placés directement dans le sien ({ $folder }) ; déplacez-y le dépôt ou créez-y un lien vers lui

# draft
highlight_bat_first = bat d’abord

# draft
highlight_block_too_long = un bloc de plus de { $size }

# draft
highlight_default = Par défaut

# draft
highlight_not_found = { $theme } (introuvable)

# draft
highlight_tree_sitter_first = Tree-sitter d’abord

# draft
inspection_blocks_notice = { $count ->
        [one] Premier bloc
       *[other] { $count } premiers blocs
    } · Source affiche la suite

# draft
inspection_drawing_inflated = Le dessin est trop grand pour être lu

# draft
inspection_drawing_too_large = Le dessin dépasse la limite d’aperçu de { $size }

# draft
inspection_embedded_image_limit = L’image incorporée dépasse la limite d’aperçu de { $size }

# draft
inspection_hex_offset = Décalage

# draft
inspection_image_too_large = L’image incorporée dépasse { $size }

# draft
inspection_lines = { $count ->
        [one] { $count } ligne
       *[other] { $count } lignes
    }

# draft
inspection_not_regular = l’aperçu nécessite un fichier ordinaire

# draft
inspection_read_as = Lu en { $encoding }

# draft
inspection_show_on_map = Afficher sur la carte

# draft
inspection_svg_no_size = L’image SVG n’a pas de taille

# draft
inspection_text_too_large = Le texte dépasse la limite d’aperçu de { $size }

# draft
inspection_xml_doctype = sa déclaration de type de document n’est pas prise en charge

# draft
inspection_xml_entities = ses entités, une fois développées, dépassent { $size }

# draft
inspection_xml_too_deep = ses éléments s’imbriquent sur plus de { $count ->
        [one] { $count } niveau
       *[other] { $count } niveaux
    }

# draft
line_channel_closed = le programme auxiliaire a cessé de lire ses demandes

# draft
line_channel_not_reading = le programme auxiliaire ne lit pas ses demandes

# draft
line_channel_start = lancer { $program }

# draft
math_failed = Les formules ne peuvent pas être composées : { $said }

# draft
math_install = Installez { $program } pour composer les formules

# draft
media_controls_busy = commandes vidéo occupées, ou session fermée

# draft
media_frame_slot = réserver la mémoire des images de la vidéo

# draft
media_worker_exited = le programme auxiliaire vidéo s’est arrêté

# draft
media_worker_stopped = le programme auxiliaire vidéo s’est arrêté pendant son démarrage

# draft
media_worker_timed_out = le programme auxiliaire vidéo a mis trop de temps à démarrer

# draft
mime_kind_archives = Archives

# draft
mime_kind_audio = Musique et audio

# draft
mime_kind_documents = Documents

# draft
mime_kind_folders = Dossiers

# draft
mime_kind_fonts = Polices

# draft
mime_kind_images = Images

# draft
mime_kind_presentations = Présentations

# draft
mime_kind_programs = Programmes

# draft
mime_kind_spreadsheets = Classeurs

# draft
mime_kind_text = Texte et code

# draft
mime_kind_videos = Vidéos

# draft
model_failed = Le modèle ne peut pas être dessiné : { $said }

# draft
model_install = Installez { $program } pour dessiner les modèles 3D

# draft
mp4_position = Position

# draft
open_launch_failed = lancer { $program } : { $error }

# draft
open_no_command = aucune commande à lancer

# draft
open_no_terminal = aucun émulateur de terminal trouvé (définissez $TERMINAL)

# draft
open_not_geo = ce n’est pas une position géographique

# draft
open_not_web_or_mail = seuls les liens web et de courriel s’ouvrent d’ici

# draft
open_path_failed = ouvrir { $path } : { $error }

# draft
open_terminal_program_not_yet = Lancer un programme dans un terminal n’est pas encore disponible sous { $system }

# draft
places_bitlocker_asks = Windows demande lui-même le mot de passe d’un disque BitLocker : ouvrez le disque

# draft
places_bookmarks_read_only = le fichier des signets est en lecture seule ; modifiez-le là où il est géré

# draft
places_diskutil_no_mount_point = diskutil n’a pas indiqué où il a monté le volume

# draft
places_diskutil_unrun = diskutil n’a pas pu être lancé : { $error }

# draft
places_drive = { $label } ({ $letter })

# draft
places_local_disk = Disque local ({ $letter })

# draft
places_mount_not_yet = Le montage d’un volume n’est pas encore disponible sous { $system }

# draft
places_no_config_dir = aucun dossier de configuration

# draft
places_passphrase_refused = La phrase secrète n’a pas été acceptée

# draft
places_root = Racine

# draft
places_system_asks_not = Ce système ne demande pas lui-même la phrase secrète d’un volume

# draft
places_udisks_no_device = udisksctl n’a pas nommé le périphérique déverrouillé

# draft
places_udisks_no_mount_point = udisksctl n’a pas indiqué où il a monté le volume

# draft
places_udisks_unrun = udisksctl n’a pas pu être lancé : { $error }

# draft
preview_changed = le fichier a changé pendant l’aperçu

# draft
preview_changed_converting = le fichier a changé pendant la conversion

# draft
preview_changed_opening = le fichier a changé pendant l’ouverture du document

# draft
preview_install_audio = Installez { $program } et les greffons de lecture de GStreamer pour lire ceci

# draft
preview_install_engine = Installez { $program } et sa bibliothèque pour lire les documents avec { $engine }

# draft
preview_install_video = Installez { $program } et les greffons de lecture de GStreamer pour lire ceci, ou choisissez Images fixes pour Moteur vidéo

# draft
preview_no_pages = Le PDF n’a aucune page lisible

# draft
preview_office_no_pdf = LibreOffice n’a produit aucun PDF (le fichier est peut-être chiffré, ou non pris en charge)

# draft
preview_office_not_pdf = la conversion n’a pas produit de PDF

# draft
preview_output_too_large = la sortie de l’aperçu est trop grande

# draft
preview_pdfium_library_missing = La bibliothèque de PDFium n’est pas à « { $path } », où les paramètres l’indiquent (Paramètres › Lecture › Documents › Bibliothèque PDFium)

# draft
preview_too_wide = les dimensions de l’aperçu dépassent la limite

# draft
preview_unknown_backend = moteur PDF inconnu ; choisissez auto, pdfium, poppler ou mupdf

# draft
print_accept = Imprimer

# draft
print_dialog_ended = la boîte de dialogue d’impression s’est terminée ({ $code })

# draft
print_dialog_pages = les pages « { $ranges } » de la boîte de dialogue d’impression : { $refused }

# draft
print_dialog_stopped = la boîte de dialogue d’impression s’est arrêtée

# draft
print_job_empty_pdf = le programme auxiliaire d’impression a écrit un PDF vide

# draft
print_job_failed = Le PDF n’a pas pu être créé ({ $status }) : { $said }

# draft
print_job_install = Installez { $program } pour imprimer

# draft
print_job_lines_cut = { $count ->
        [one] Seule la première ligne est imprimée
       *[other] Seules les { $count } premières lignes sont imprimées
    }

# draft
print_job_no_pdf = le programme auxiliaire d’impression n’a écrit aucun PDF

# draft
print_job_silent = le programme auxiliaire d’impression n’a rien dit

# draft
print_job_unwritten = { $path } n’a pas pu être écrit

# draft
print_needs_pdfium = l’impression nécessite PDFium, qui est absent

# draft
print_no_dialog = ce bureau n’affiche aucune boîte de dialogue d’impression (son portail d’impression : { $error })

# draft
print_no_token = la boîte de dialogue d’impression n’a renvoyé aucun jeton

# draft
print_not_taken = le service d’impression du bureau ne l’a pas pris en charge ({ $code })

# draft
print_not_yet = L’impression n’est pas encore disponible sous { $system }

# draft
print_portal_silent = le portail d’impression ne répond plus

# draft
print_portal_stopped = le portail d’impression s’est arrêté

# draft
print_portal_unheard = le portail d’impression ne peut pas être écouté

# draft
print_refused = l’imprimante l’a refusé : { $error }

# draft
print_settings_short = les paramètres de l’imprimante sont incomplets

# draft
process_failed = { $program } a échoué ({ $status }) : { $said }

# draft
process_not_installed = { $program } n’est pas installé

# draft
process_older_build = { $error }. { $program } provient d’une version plus ancienne de skwir, ce qui en est peut-être la cause : recompilez-le ou réinstallez-le

# draft
process_other_build = { $error }. { $program } provient d’une autre version de skwir ({ $theirs } ; skwir est en { $ours }), ce qui en est peut-être la cause : recompilez-le ou réinstallez-le

# draft
process_stalled_minutes = { $program } a été arrêté : il n’a rien dit de sa progression pendant { $count ->
        [one] { $count } minute
       *[other] { $count } minutes
    }

# draft
process_stalled_seconds = { $program } a été arrêté : il n’a rien dit de sa progression pendant { $count ->
        [one] { $count } seconde
       *[other] { $count } secondes
    }

# draft
process_start = lancer { $program }

# draft
process_timed_out = { $program } a dépassé le délai imparti

# draft
process_too_much_input = { $program } : entrée trop grande

# draft
process_too_much_output = { $program } : sortie excessive

# draft
rich_preview_page_no_size = la page n’a pas de taille

# draft
rich_preview_stylesheet_limit = La feuille de style dépasse la limite d’aperçu de { $size }

# draft
sandbox_converter_not_yet = Lancer un convertisseur à l’écart de vos fichiers n’est pas encore disponible sous { $system }

# draft
sandbox_needs_bubblewrap = Ceci nécessite bubblewrap, pour s’exécuter à l’écart de vos fichiers

# draft
sandbox_needs_sandbox = Ceci nécessite le bac à sable du système, pour s’exécuter à l’écart de vos fichiers

# draft
sandbox_plugin_not_yet = Lancer un module en tant que programme, à l’écart de vos fichiers, n’est pas encore disponible sous { $system }

# draft
secrets_no_keyring = Aucun trousseau n’a répondu : { $error }

# draft
secrets_not_text = Le secret du trousseau n’est pas du texte

# draft
secrets_unsupported = Aucun trousseau n’est encore pris en charge sur ce système

# draft
shares_login_refused = le serveur a refusé l’identification

# draft
shares_netfs_not_yet = Le montage d’un partage par NetFS n’est pas encore disponible sous { $system }

# draft
shares_no_address = « { $address } » n’est pas une adresse que le système sait lire

# draft
shares_no_mount_point = NetFS n’a indiqué aucun point de montage

# draft
shares_windows_not_yet = Un partage Windows n’est pas encore disponible sous { $system }

# draft
shell_environment_failed = { $shell } s’est terminé avec { $status }

# draft
shrink_copy_unread = la copie du programme auxiliaire de réduction n’a pas pu être lue

# draft
shrink_empty_copy = le programme auxiliaire de réduction a écrit une copie vide

# draft
shrink_failed = Aucune copie allégée n’a pu être faite ({ $status }) : { $said }

# draft
shrink_gone = { $path } n’est plus là

# draft
shrink_install = Installez { $program } pour faire des copies allégées

# draft
shrink_no_copy = le programme auxiliaire de réduction n’a écrit aucune copie

# draft
shrink_no_smaller = le programme auxiliaire de réduction a écrit une copie qui n’est pas plus petite que le fichier

# draft
shrink_silent = le programme auxiliaire de réduction n’a rien dit

# draft
shrink_unread = { $path } n’a pas pu être lu

# draft
shrink_unwritten = { $path } n’a pas pu être écrit

# draft
tree_sitter_ended = le programme auxiliaire tree-sitter s’est arrêté

# draft
tree_sitter_ended_why = le programme auxiliaire tree-sitter s’est arrêté : { $why }

# draft
tree_sitter_exited = le programme auxiliaire tree-sitter s’est arrêté de façon inattendue

# draft
tree_sitter_install = Installez { $program } pour colorer le code avec les grammaires tree-sitter

# draft
tree_sitter_no_answer = le programme auxiliaire tree-sitter n’a pas répondu en { $count ->
        [one] { $count } seconde
       *[other] { $count } secondes
    }

# draft
tree_sitter_not_started = le programme auxiliaire tree-sitter n’a pas démarré en { $count ->
        [one] { $count } seconde
       *[other] { $count } secondes
    }

# draft
tree_sitter_timed_out = le programme auxiliaire tree-sitter a dépassé le délai imparti

# draft
tree_sitter_too_large = le texte dépasse { $size }

# draft
tree_sitter_too_many = les grammaires trouvées sont plus nombreuses que le programme auxiliaire tree-sitter n’en reçoit à la fois

# draft
windows_no_user = Windows n’indique pas qui est l’utilisateur

# draft
windows_unlock_silent = la boîte de dialogue de déverrouillage de Windows n’a pas répondu

# draft
windows_unlock_unopened = la boîte de dialogue de déverrouillage de Windows n’a pas pu s’ouvrir : { $error }
