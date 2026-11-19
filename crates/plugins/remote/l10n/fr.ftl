# French catalog of skwir-plugin-remote, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
remote_address_bad_device = « { $text } » n’est pas un nom de périphérique

# draft
remote_address_bad_port = « { $port } » n’est pas un port

# draft
remote_address_bad_remote = « { $name } » n’est pas un nom de stockage rclone

# draft
remote_address_bad_server = « { $host } » n’est pas un nom de serveur

# draft
remote_address_bad_ssh_user = « { $user } » n’est pas un nom d’utilisateur que l’on peut donner à ssh

# draft
remote_address_bad_user = « { $user } » n’est pas un nom d’utilisateur

# draft
remote_address_device_user = « { $text } » : l’adresse d’un périphérique ne nomme aucun utilisateur

# draft
remote_address_ipv6 = « { $text } » n’a pas de ] pour terminer son adresse IPv6

# draft
remote_address_no_protocol = « { $text } » ne nomme aucun protocole

# draft
remote_address_no_server = « { $text } » ne nomme aucun serveur

# draft
remote_address_no_slashes = « { $text } » doit avoir // avant le serveur

# draft
remote_address_not_protocol = « { $scheme }: » n’est pas un protocole qu’ouvre Skwir

# draft
remote_address_password = une adresse ne contient pas de mot de passe : le serveur le demande

# draft
remote_ask_connect = Se connecter

# draft
remote_ask_password = Mot de passe

# draft
remote_bucket_label = { $bucket } ({ $profile })

# draft
remote_cancelled = annulé

# draft
remote_chosen_data = Le choix de vignettes de chaque dossier distant, par son adresse

# draft
remote_configure_add_as_server = Ajouter comme serveur…

# draft
remote_configure_add_record = Ajouter un serveur

# draft
remote_configure_add_server = Ajouter un serveur…

# draft
remote_configure_add_server_description = Ajouter un serveur : un stockage rclone (stockage en ligne, S3, WebDAV, SFTP et soixante-dix autres), dans un formulaire que l’utilisateur remplit ; ce dont il part peut être donné, jamais un secret

# draft
remote_configure_add_server_menu = Ajouter un serveur…

# draft
remote_configure_added = { $server } est ajouté

# draft
remote_configure_added_obscured = { $server } est ajouté. rclone ne garde ses secrets qu’obscurcis : chiffrer sa configuration ?

# draft
remote_configure_answers = { $server } répond

# draft
remote_configure_at_login = Connecter à l’ouverture de session

# draft
remote_configure_at_login_encrypted = { $count ->
        [1] { $remotes } se connecte à l’ouverture de session, ce qu’une configuration chiffrée ne permet pas : personne n’est là pour donner son mot de passe. Désactivez d’abord « Connecter à l’ouverture de session ».
        [one] { $remotes } se connecte à l’ouverture de session, ce qu’une configuration chiffrée ne permet pas : personne n’est là pour donner son mot de passe. Désactivez d’abord « Connecter à l’ouverture de session ».
       *[other] { $remotes } se connectent à l’ouverture de session, ce qu’une configuration chiffrée ne permet pas : personne n’est là pour donner son mot de passe. Désactivez d’abord « Connecter à l’ouverture de session ».
    }

# draft
remote_configure_at_login_failed = { $server } n’a pas pu être réglé pour se connecter à l’ouverture de session : { $error }

# draft
remote_configure_at_login_on = Connecter à l’ouverture de session ✓

# draft
remote_configure_at_login_which = seuls les stockages rclone et les serveurs SFTP se connectent à l’ouverture de session

# draft
remote_configure_bookmark_stays = son signet reste : { $error }

# draft
remote_configure_connect_at_login = Connecter à l’ouverture de session

# draft
remote_configure_connect_at_login_description = Faire connecter un serveur par le gestionnaire de services du système à l’ouverture de session de l’utilisateur (une unité utilisateur systemd, un agent de lancement, une tâche d’ouverture de session), ou ne plus le faire ; le connecte aussi tout de suite. Seulement ce qui ne demande rien à l’ouverture de session : un stockage d’une configuration de rclone non chiffrée, un serveur SFTP auquel on se connecte par clé

# draft
remote_configure_connects_at_login = { $server } se connecte à l’ouverture de session

# draft
remote_configure_copied_what = le lien public

# draft
remote_configure_copy_public_link = Copier le lien public

# draft
remote_configure_does_not_answer = { $server } ne répond pas : { $error }

# draft
remote_configure_edit_server = Modifier le serveur…

# draft
remote_configure_edit_server_description = Modifier un serveur dans son formulaire : un stockage rclone, ou un serveur ajouté par son adresse

# draft
remote_configure_edit_server_menu = Modifier le serveur…

# draft
remote_configure_encrypt = Chiffrer la configuration de rclone…

# draft
remote_configure_encrypt_accept = Chiffrer

# draft
remote_configure_encrypt_again = Confirmation du mot de passe

# draft
remote_configure_encrypt_button = Chiffrer…

# draft
remote_configure_encrypt_description = Chiffrer le fichier de configuration de rclone, qui ne garde les secrets de ses stockages qu’obscurcis, avec un mot de passe que l’utilisateur tape et peut conserver dans le trousseau

# draft
remote_configure_encrypt_intro = rclone garde les clés et les mots de passe de ses stockages dans son fichier de configuration, seulement obscurcis : quiconque lit le fichier peut s’en servir. Chiffré, il s’ouvre avec un mot de passe.

# draft
remote_configure_encrypt_keep = Le conserver dans le trousseau

# draft
remote_configure_encrypt_keep_help = Skwir ouvre alors la configuration sans le demander. rclone, dans un terminal, le demande, ou le lit dans RCLONE_CONFIG_PASS.

# draft
remote_configure_encrypt_password = Mot de passe

# draft
remote_configure_encrypt_title = Chiffrer la configuration de rclone

# draft
remote_configure_encrypted = La configuration de rclone est chiffrée

# draft
remote_configure_encrypted_already = La configuration de rclone est déjà chiffrée

# draft
remote_configure_finish = Terminer la configuration…

# draft
remote_configure_forget_password = Oublier le mot de passe

# draft
remote_configure_keep_as_server = Garder comme serveur

# draft
remote_configure_keep_as_server_description = Faire de l’adresse d’un dossier distant un serveur de la section Réseau, conservé dans la liste de Skwir ou dans les dossiers réseau de KDE, sous le nom de son signet ; son signet, s’il en a un, est retiré

# draft
remote_configure_keep_as_server_item = Garder comme serveur

# draft
remote_configure_keep_as_server_menu = Proposer « Garder comme serveur » dans le menu d’un emplacement

# draft
remote_configure_keep_as_server_menu_description = L’élément Garder comme serveur du menu d’un emplacement de la barre latérale, pour l’adresse d’un dossier distant qui n’est pas encore un serveur (celle d’un signet). Répond de mémoire

# draft
remote_configure_kept_as_server = { $server } est un serveur de la section Réseau, conservé { $kept }

# draft
remote_configure_kept_by_tools = { $protocol } est conservé par rclone ou par les outils AWS

# draft
remote_configure_kept_elsewhere = { $server } est { $kept } : modifiez-le là-bas

# draft
remote_configure_kept_stays = Ce que Skwir conservait pour { $server } reste : { $error }

# draft
remote_configure_key_login = Configurer la connexion par clé…

# draft
remote_configure_key_login_failed = La connexion par clé n’a pas été configurée sur { $server } : { $error }

# draft
remote_configure_key_login_sftp = la connexion par clé est celle de ssh : serveurs SFTP seulement

# draft
remote_configure_mount_options = Options de montage…

# draft
remote_configure_mount_options_button = Options de montage…

# draft
remote_configure_mount_options_description = Choisir comment un serveur est monté à sa prochaine connexion : lecture seule, cache de fichiers et bande passante de rclone, compression de sshfs

# draft
remote_configure_moved_off_login = { $server } ne se connecte plus à l’ouverture de session : son adresse a changé

# draft
remote_configure_nearby_menu = Menu d’un serveur à proximité

# draft
remote_configure_nearby_menu_description = Les éléments du menu contextuel pour le seul serveur sélectionné dans l’emplacement À proximité : Ajouter comme serveur…, son formulaire prérempli avec le nom et l’adresse qu’il annonce, là où aucun serveur n’a cette adresse. Répond de mémoire

# draft
remote_configure_no_longer_at_login = { $server } ne se connecte plus à l’ouverture de session

# draft
remote_configure_no_options = aucun des outils avec lesquels Skwir monte { $protocol } ne prend d’options

# draft
remote_configure_no_public_link = Aucun lien public n’a été créé : { $error }

# draft
remote_configure_not_done = { $done ->
        [added] Le serveur n’a pas été ajouté : { $error }
        [saved] Le serveur n’a pas été enregistré : { $error }
        [signed_in] La connexion au serveur a échoué : { $error }
        [tested] Le serveur n’a pas été testé : { $error }
        [kept] Le serveur n’a pas été gardé : { $error }
       *[removed] Le serveur n’a pas été retiré : { $error }
    }

# draft
remote_configure_not_encrypted = La configuration de rclone n’a pas été chiffrée : { $error }

# draft
remote_configure_not_remote = il n’est pas dans un dossier distant

# draft
remote_configure_nothing_to_link = rien vers quoi faire un lien

# draft
remote_configure_options_not_at_login = Les options de montage ont été enregistrées, mais la connexion à l’ouverture de session monte comme avant : { $error }

# draft
remote_configure_options_saved = { $server } sera monté ainsi à sa prochaine connexion

# draft
remote_configure_options_unsaved = Les options de montage n’ont pas été enregistrées : { $error }

# draft
remote_configure_password_stays = Le mot de passe que Skwir conserve pour { $server } reste dans le trousseau : { $error }

# draft
remote_configure_passwords_differ = Les deux mots de passe diffèrent

# draft
remote_configure_public_link = Copier le lien public

# draft
remote_configure_public_link_description = Créer un lien public vers un fichier d’un stockage rclone qui sait en créer un (le lien de partage d’un service en ligne), et le mettre dans le presse-papiers

# draft
remote_configure_public_link_menu = Proposer un lien public dans le menu contextuel

# draft
remote_configure_public_link_menu_description = L’élément Copier le lien public du menu contextuel, pour un fichier d’un stockage rclone qui sait en créer un. Répond de mémoire

# draft
remote_configure_reconnect = Se reconnecter…

# draft
remote_configure_reconnect_description = Reconnecter un stockage rclone par le navigateur, quand son accès a expiré ou a été révoqué

# draft
remote_configure_record_kept = { $kind } · { $kept }

# draft
remote_configure_record_kind = Type

# draft
remote_configure_record_name = Nom

# draft
remote_configure_record_unfinished = { $kind } · { $kept } · configuration non terminée

# draft
remote_configure_remove = Retirer

# draft
remote_configure_remove_intro = Skwir oublie le serveur, conservé { $kept } : ce qu’il contient reste où il est, et l’ajouter de nouveau le fait revenir.

# draft
remote_configure_remove_intro_password = Skwir oublie le serveur, conservé { $kept }, et le mot de passe qu’il conserve pour lui dans le trousseau : ce que le serveur contient reste où il est, et l’ajouter de nouveau le fait revenir.

# draft
remote_configure_remove_server = Retirer le serveur…

# draft
remote_configure_remove_server_description = Retirer un serveur, une fois que l’utilisateur a confirmé, de là où il est conservé (la configuration de rclone, la liste de Skwir, les dossiers réseau de KDE), avec ses options de montage et l’entrée qui le connecte à l’ouverture de session : ce qu’il contient reste où il est

# draft
remote_configure_remove_server_menu = Retirer le serveur…

# draft
remote_configure_remove_title = Retirer { $server } ?

# draft
remote_configure_removed = { $server } est retiré

# draft
remote_configure_saved = { $server } est enregistré

# draft
remote_configure_server_records = Serveurs

# draft
remote_configure_server_records_description = Chaque serveur tel que les Paramètres le listent : le nom de chacun, son type et l’endroit où il est conservé, ce que fait sa ligne, et si Skwir le modifie (`fixed` : un autre programme le fait)

# draft
remote_configure_servers = Serveurs

# draft
remote_configure_servers_description = Les serveurs de la section Réseau de la barre latérale, chacun modifié là où il est conservé.

# draft
remote_configure_servers_description_more = Ceux ajoutés par leur adresse sont dans la liste de Skwir ou dans les dossiers réseau de KDE ; les stockages rclone, dans la configuration propre de rclone, où rclone les trouve aussi dans un terminal.

# draft
remote_configure_setup_key_login = Configurer la connexion par clé…

# draft
remote_configure_setup_key_login_description = Permettre à ssh de se connecter à un serveur SFTP par clé, sans mot de passe : une clé que détient l’agent ssh, celle par défaut de l’utilisateur ou une nouvelle, dont la partie publique est ajoutée aux clés autorisées du serveur une fois que l’utilisateur l’a choisie

# draft
remote_configure_sign_in_again = Se reconnecter…

# draft
remote_configure_signed_in = { $server } est reconnecté

# draft
remote_configure_stays_mounted = il reste monté : { $error }

# draft
remote_configure_test = Tester

# draft
remote_configure_test_server = Tester le serveur

# draft
remote_configure_test_server_description = Lister la racine d’un stockage rclone, comme le ferait une connexion, et dire s’il répond

# draft
remote_configure_test_server_menu = Tester le serveur

# draft
remote_configure_turned_off = les dossiers distants sont désactivés

# draft
remote_configure_unknown_server = aucun serveur n’est connu à l’adresse { $address }

# draft
remote_configure_window_needed = cela se demande dans un formulaire, rempli dans la fenêtre de Skwir : ouvrez-la d’abord

# draft
remote_connect = Se connecter à un serveur

# draft
remote_connect_description = Ouvrir un dossier distant par son adresse (sftp://, smb://, dav://, ftp://, nfs://, afp://, rclone://)

# draft
remote_connectors = Outils qui ouvrent les adresses

# draft
remote_connectors_description = Les outils qui montent une adresse sftp://, smb:// ou autre, essayés dans cet ordre parmi ceux qui sont installés : sshfs, kmtp (un téléphone que gère KDE), gvfs, kio, rclone, fstab.

# draft
remote_connectors_description_more = Sur le bureau KDE, kio passe avant gvfs.

# draft
remote_device_held = { $count ->
        [1] { $programs } l’a ouvert
        [one] { $programs } l’a ouvert
       *[other] { $programs } l’ont ouvert
    }, et un seul programme à la fois peut utiliser { $device ->
        [phone] un téléphone
       *[camera] un appareil photo
    }. { $count ->
        [1] Quittez { $programs }
        [one] Quittez { $programs }
       *[other] Quittez-les
    }, puis réessayez.

# draft
remote_device_held_through = { $count ->
        [1] { $programs } l’a ouvert
        [one] { $programs } l’a ouvert
       *[other] { $programs } l’ont ouvert
    }, et un seul programme à la fois peut utiliser { $device ->
        [phone] un téléphone
       *[camera] un appareil photo
    }. Skwir l’ouvrirait par { $holder }, mais { $why } ; sinon, { $count ->
        [1] quittez { $programs }
        [one] quittez { $programs }
       *[other] quittez-les
    }, puis réessayez.

# draft
remote_device_not_mounted = Non monté. Cliquez pour le monter et l’ouvrir.

# draft
remote_disconnect = Se déconnecter

# draft
remote_disconnect_description = Se déconnecter d’un dossier distant : ce qui l’affiche revient au dossier personnel

# draft
remote_disconnected = Déconnecté de { $name }

# draft
remote_disconnected_kept = Déconnecté de { $name } : { $count ->
        [one] { $count } fichier pas encore envoyé reste dans le cache de rclone, et sera envoyé à la prochaine connexion
       *[other] { $count } fichiers pas encore envoyés restent dans le cache de rclone, et seront envoyés à la prochaine connexion
    }

# draft
remote_disconnecting = Déconnexion…

# draft
remote_forget_password = Oublier le mot de passe

# draft
remote_forget_password_description = Retirer du trousseau le mot de passe que Skwir conserve pour un serveur (jamais celui qu’un autre programme y a conservé)

# draft
remote_forget_password_menu = Oublier le mot de passe

# draft
remote_fstab = les montages du système

# draft
remote_fstab_administrator = le système l’a monté : seul un administrateur peut le démonter

# draft
remote_fstab_linux_only = fstab n’est lu que sous Linux

# draft
remote_fstab_no_entry = aucune entrée de /etc/fstab que vous pouvez monter ne le nomme

# draft
remote_gvfs_camera_label = { $device } (appareil photo)

# draft
remote_gvfs_connect_to = Se connecter à { $host } ?

# draft
remote_gvfs_device_silent = il n’a pas répondu en { $seconds ->
        [one] { $seconds } seconde
       *[other] { $seconds } secondes
    }

# draft
remote_gvfs_ios_label = { $device } (iOS)

# draft
remote_gvfs_linux_only = GVfs ne fonctionne que sous Linux

# draft
remote_gvfs_mtp_label = { $device } (MTP)

# draft
remote_gvfs_no_address = GVfs ne donne aucune adresse pour ce montage

# draft
remote_gvfs_no_bridge = GVfs a monté { $address }, mais son pont FUSE ne tourne pas : gvfsd-fuse est-il installé ?

# draft
remote_gvfs_no_folder = GVfs a monté { $address }, mais son dossier n’est pas là

# draft
remote_gvfs_other_label = { $host } ({ $kind })

# draft
remote_gvfs_server_silent = { $host } n’a pas répondu en { $seconds ->
        [one] { $seconds } seconde
       *[other] { $seconds } secondes
    }

# draft
remote_gvfs_you_cancelled = vous avez annulé

# draft
remote_half_made = La configuration de { $name } n’est pas terminée : Terminer la configuration… dans son menu

# draft
remote_keys_already_there = il y a déjà une clé à { $path }

# draft
remote_keys_comment_line = le commentaire d’une clé tient sur une ligne

# draft
remote_keys_form_accept = Configurer

# draft
remote_keys_form_again = Confirmation de la phrase de passe

# draft
remote_keys_form_intro = La partie publique de votre clé est ajoutée aux clés autorisées du serveur : ssh se connecte alors avec elle, sans mot de passe. Sa partie privée ne quitte jamais cette machine. Le mot de passe du serveur est demandé une fois.

# draft
remote_keys_form_key = Clé

# draft
remote_keys_form_passphrase = Phrase de passe de la nouvelle clé

# draft
remote_keys_form_passphrase_help = Chiffre une nouvelle clé : demandée à chaque utilisation, sauf si l’agent la détient. Vide pour aucune.

# draft
remote_keys_form_title = Configurer la connexion par clé sur { $server }

# draft
remote_keys_had_it = { $server } avait déjà la clé : ssh se connecte avec elle

# draft
remote_keys_in_agent = { $key }, dans l’agent ssh

# draft
remote_keys_in_file = { $key }, dans { $file }

# draft
remote_keys_key = { $comment } ({ $kind })

# draft
remote_keys_keygen_failed = ssh-keygen a échoué : { $said }

# draft
remote_keys_keygen_no_passphrase = ssh-keygen n’a pas reçu la phrase de passe

# draft
remote_keys_keygen_no_public = ssh-keygen n’a écrit aucune clé publique

# draft
remote_keys_new = Une nouvelle clé

# draft
remote_keys_new_made_in = Créée dans { $folder }

# draft
remote_keys_no_home = il n’y a pas de dossier personnel pour les clés de ssh

# draft
remote_keys_no_runtime = il n’y a pas de dossier d’exécution

# draft
remote_keys_no_such_key = cette clé n’existe pas

# draft
remote_keys_none_chosen = aucune clé n’a été choisie

# draft
remote_keys_none_offered = ssh ne propose aucune clé, et une nouvelle écraserait celle qui se trouve à { $path }

# draft
remote_keys_not_added = le serveur n’a pas pu ajouter les clés : { $why }

# draft
remote_keys_not_added_unsaid = le serveur n’a pas pu ajouter les clés, sans dire pourquoi

# draft
remote_keys_not_taken = La clé est sur { $server }, mais ssh ne se connecte pas avec elle : le serveur n’accepte peut-être pas les clés

# draft
remote_keys_passphrases_differ = Les deux phrases de passe diffèrent

# draft
remote_keys_refused = le serveur a refusé les clés : { $said }

# draft
remote_keys_set_up = La connexion par clé est configurée sur { $server }

# draft
remote_keys_sftp_only = la connexion par clé ne se configure que pour les serveurs SFTP

# draft
remote_keys_unread = les clés autorisées du serveur n’ont pas pu être lues : { $said }

# draft
remote_kio_could_not = KIO n’a pas pu l’ouvrir : { $reason }

# draft
remote_kio_could_not_open = KIO n’a pas pu l’ouvrir

# draft
remote_kio_keeps = KIO garde ses dossiers jusqu’à la fin de la session

# draft
remote_kio_linux_only = les dossiers de KIO (kio-fuse) n’existent que sous Linux

# draft
remote_kio_no_answer = kio-fuse n’a pas répondu

# draft
remote_kio_no_folder = kio-fuse l’a monté, mais son dossier n’est pas là

# draft
remote_kio_opens_no = KIO n’ouvre aucune adresse { $protocol }

# draft
remote_kmtp_not_held = kmtpd de KDE ne gère pas ce téléphone

# draft
remote_kmtpd = kmtpd de KDE

# draft
remote_kmtpd_linux_only = kmtpd de KDE n’est joignable que sous Linux

# draft
remote_login_agent_read_only = l’agent de connexion est en lecture seule ; modifiez-le là où il est géré

# draft
remote_login_control_character = « { $part } » contient un caractère qu’un gestionnaire de services ne lit pas

# draft
remote_login_ended = il s’est terminé ({ $status })

# draft
remote_login_journal = { $error } (journalctl --user -u { $unit } dit pourquoi)

# draft
remote_login_needs_password = le serveur demande un mot de passe ou la phrase de passe d’une clé : à l’ouverture de session, personne n’est là pour répondre (une clé que détient l’agent ssh, ou une clé sans phrase de passe, se connecte seule)

# draft
remote_login_no_config = il n’y a pas de dossier de configuration

# draft
remote_login_no_folder = il n’y a pas de dossier où monter

# draft
remote_login_no_home = il n’y a pas de dossier personnel

# draft
remote_login_no_runtime = il n’y a pas de dossier d’exécution où monter

# draft
remote_login_no_schtasks = schtasks, du Planificateur de tâches, est absent

# draft
remote_login_no_systemd = systemd n’est pas là pour connecter à l’ouverture de session (pas de systemctl)

# draft
remote_login_no_user = Windows ne dit pas qui est l’utilisateur (USERNAME)

# draft
remote_login_not_reached = il n’a pas pu être joint pour savoir s’il se connecte seul : { $why }

# draft
remote_login_not_text = « { $part } » n’est pas un texte que lit un gestionnaire de services

# draft
remote_login_not_yet = La connexion à l’ouverture de session n’est pas encore disponible sous { $system }

# draft
remote_login_rclone_encrypted = la configuration de rclone est chiffrée : à l’ouverture de session, personne n’est là pour donner son mot de passe

# draft
remote_login_rclone_no_remote = la configuration de rclone ne nomme aucun stockage { $remote }

# draft
remote_login_rclone_not_yet = Le montage des stockages rclone n’est pas encore disponible sous { $system }

# draft
remote_login_rclone_remotes_only = seul un stockage de la configuration de rclone (rclone://name/) se connecte à l’ouverture de session par rclone : personne n’est là pour donner le mot de passe d’un serveur

# draft
remote_login_sshfs_sftp_only = sshfs ne connecte que les adresses SFTP

# draft
remote_login_unit_read_only = l’unité de connexion est en lecture seule ; modifiez-la là où elle est gérée

# draft
remote_login_unknown_host = ssh ne connaît pas encore l’identité du serveur : connectez-vous-y d’abord une fois

# draft
remote_lost = La connexion est perdue : ouvrez-le pour vous reconnecter

# draft
remote_macfuse_missing = macFUSE ou FUSE-T n’est pas installé

# draft
remote_mount_options_menu = Options de montage…

# draft
remote_mounter_connected_already = il est déjà connecté

# draft
remote_mounter_ended = il s’est terminé ({ $status })

# draft
remote_mounter_no_runtime_folder = il n’y a pas de dossier d’exécution où monter

# draft
remote_mounter_no_runtime_folder_at_all = il n’y a pas de dossier d’exécution

# draft
remote_mounter_nothing_mounted = il a dit avoir monté, mais rien n’y est monté

# draft
remote_mounter_trust = Se connecter à ce serveur ?

# draft
remote_mounter_used_by = { $count ->
        [1] { $programs } l’utilise
        [one] { $programs } l’utilise
       *[other] { $programs } l’utilisent
    }

# draft
remote_mounter_used_by_another = un autre programme l’utilise

# draft
remote_mounter_you_cancelled = vous avez annulé

# draft
remote_nearby_browser_failed = { $finder } : { $error }

# draft
remote_nearby_empty = Aucun serveur trouvé à proximité

# draft
remote_nearby_empty_detail = Les serveurs qui s’annoncent sur ce réseau (un NAS, un Mac, un PC Windows) apparaissent ici, trouvés par Avahi et wsdd tant que cet emplacement est affiché.

# draft
remote_nearby_failures = { $first } ; { $then }

# draft
remote_nearby_no_runtime = pas de dossier d’exécution

# draft
remote_nearby_none_answers = Les serveurs à proximité sont trouvés par Avahi ou wsdd, et aucun des deux ne répond ici ({ $why })

# draft
remote_nearby_not_yet = La recherche de serveurs à proximité n’est pas encore disponible sous { $system }

# draft
remote_nearby_server_kind = Serveur { $protocol }

# draft
remote_nearby_title = À proximité
    .gender = masculine
    .definite = À proximité
    .de = d’À proximité

# draft
remote_nearby_wsdd_ended = wsdd s’est arrêté dès son démarrage ({ $status }) : il est peut-être trop ancien pour découvrir les serveurs

# draft
remote_nearby_wsdd_no_start = wsdd n’a pas démarré

# draft
remote_needs_gio = Ouverture des adresses par GVfs, les dossiers distants du bureau

# draft
remote_needs_kmtp = Ouverture d’un téléphone que gère kmtpd de KDE

# draft
remote_needs_rclone = Ouverture des stockages rclone et d’autres adresses

# draft
remote_needs_sshfs = Ouverture des adresses SFTP par votre ssh

# draft
remote_no_password_kept = Skwir ne conserve aucun mot de passe pour { $server }

# draft
remote_no_tool = aucun outil que Skwir connaît n’ouvre les adresses { $protocol }

# draft
remote_not_disconnected = { $name } n’a pas été déconnecté : { $error }

# draft
remote_not_installed = { $program } n’est pas installé

# draft
remote_not_mounted_there = aucun dossier distant n’est monté là

# draft
remote_nothing_installed = rien d’installé ne peut l’ouvrir : { $why }

# draft
remote_options_bandwidth = Limite de bande passante

# draft
remote_options_bandwidth_help = Celle de rclone : 10M, ou celle de l’envoi et celle du téléchargement, comme 10M:1M ; vide pour aucune

# draft
remote_options_bandwidth_refused = Une bande passante est un débit, comme 10M ou 512k, ou celui de l’envoi et celui du téléchargement, comme 10M:1M

# draft
remote_options_cache = Cache de fichiers

# draft
remote_options_cache_automatic = automatique

# draft
remote_options_cache_automatic_detail = { $cache } ici : { $why ->
        [wraps] il chiffre ou enveloppe d’autres stockages, dont full garderait les fichiers sur ce disque
        [near] il est sur le réseau local
       *[far] ce qui est lu est relu depuis ce disque
    }

# draft
remote_options_cache_full = Ce qui est lu et écrit est gardé sur ce disque pendant une heure : relu depuis le disque, envoyé après sa fermeture, réessayé en cas d’échec

# draft
remote_options_cache_help = Ce que rclone garde de ses fichiers sur ce disque

# draft
remote_options_cache_minimal = Chaque lecture interroge le serveur ; un fichier écrit en entier s’y trouve dès sa fermeture

# draft
remote_options_cache_off = Rien n’est gardé sur ce disque : certains programmes ne peuvent pas écrire

# draft
remote_options_cache_writes = Les fichiers écrits sont gardés sur ce disque et envoyés après leur fermeture ; chaque lecture interroge le serveur

# draft
remote_options_compression = Compression

# draft
remote_options_compression_help = ssh compresse ce qu’il envoie (sshfs) : plus rapide sur un réseau lent, plus lent sur un réseau rapide

# draft
remote_options_data = Les options de montage choisies pour chaque serveur, par son adresse

# draft
remote_options_intro = Utilisées à sa prochaine connexion, par rclone ou sshfs. GVfs et KIO montent comme le fait le bureau.

# draft
remote_options_read_only = Lecture seule

# draft
remote_options_read_only_help = Aucun programme n’y écrit rien

# draft
remote_options_save = Enregistrer

# draft
remote_options_title = Options de montage de { $server }

# draft
remote_password_for = Mot de passe pour { $label }

# draft
remote_password_forgotten = Le mot de passe est oublié

# draft
remote_password_not_forgotten = Le mot de passe n’a pas été oublié : { $error }

# draft
remote_password_not_kept = Le mot de passe n’a pas été retenu : { $why }

# draft
remote_plugin_description = Ouvrir les adresses SFTP, SMB, WebDAV, FTP, NFS et rclone comme des dossiers, par sshfs, GVfs, rclone ou les montages du système ; les téléphones et les appareils photo par GVfs

# draft
remote_plugin_title = Dossiers distants

# draft
remote_provider_agent_connects = Un agent demande à se connecter à { $host }

# draft
remote_provider_agent_connects_detail = Il a demandé à ouvrir { $address }. Se connecter dit au serveur qui vous êtes.

# draft
remote_provider_agent_opens = Un agent demande à ouvrir { $label }

# draft
remote_provider_agent_opens_detail = Il a demandé à ouvrir { $address }.

# draft
remote_provider_camera_on = Un appareil photo montre ses fichiers tant qu’il est allumé.

# draft
remote_provider_connecting = Connexion à { $label }…

# draft
remote_provider_not_allowed = vous ne l’avez pas autorisé

# draft
remote_provider_not_connected = Impossible de se connecter à { $label } : { $error }

# draft
remote_provider_not_opened = Impossible d’ouvrir { $label } : { $error }

# draft
remote_provider_opening = Ouverture de { $label }…

# draft
remote_provider_phone_unlocked = Un téléphone montre ses fichiers une fois déverrouillé.

# draft
remote_provider_quitting = Skwir se ferme

# draft
remote_provider_window_asks = Si le serveur demande un mot de passe, ou qu’on lui fasse confiance la première fois, la fenêtre vous le demande.

# draft
remote_rclone_add = Ajouter

# draft
remote_rclone_add_kind = Ajouter { $kind }

# draft
remote_rclone_add_server = Ajouter un serveur

# draft
remote_rclone_add_server_intro = Un serveur par son adresse, conservé dans la liste de Skwir ou dans les dossiers réseau de KDE, ou un stockage rclone, conservé dans la configuration propre de rclone, où rclone le trouve aussi dans un terminal.

# draft
remote_rclone_answer = Réponse

# draft
remote_rclone_config_open = Ouvrir

# draft
remote_rclone_config_password = Mot de passe de la configuration de rclone

# draft
remote_rclone_config_unsaid = rclone n’a pas dit où se trouve sa configuration

# draft
remote_rclone_connected_already = il est déjà connecté

# draft
remote_rclone_connection_refused = le serveur a refusé la connexion

# draft
remote_rclone_continue = Continuer

# draft
remote_rclone_edit = Modifier { $remote }

# draft
remote_rclone_include = Inclure { $remote }

# draft
remote_rclone_keep_it = Le garder

# draft
remote_rclone_keeps_no_link = « { $path } » : rclone ne garde aucun lien

# draft
remote_rclone_kind_detail = { $kind } : un stockage rclone

# draft
remote_rclone_kind_field = Type de stockage

# draft
remote_rclone_login_refused = le serveur a refusé l’authentification (mot de passe erroné ?)

# draft
remote_rclone_login_refused_rc = son authentification a été refusée

# draft
remote_rclone_mount_late = rclone ne l’a pas monté à temps

# draft
remote_rclone_name = Nom

# draft
remote_rclone_name_end = Le nom d’un stockage ne se termine pas par un espace

# draft
remote_rclone_name_help = Son nom dans la barre latérale et pour rclone : lettres a–z, chiffres et _ - . +

# draft
remote_rclone_name_letters = Le nom d’un stockage ne contient que des lettres, des chiffres, des espaces et _ - . + @

# draft
remote_rclone_name_needed = Un stockage doit avoir un nom

# draft
remote_rclone_name_start = Le nom d’un stockage ne commence ni par - ni par un espace

# draft
remote_rclone_name_taken = rclone a déjà un stockage nommé { $remote }

# draft
remote_rclone_next = Suivant

# draft
remote_rclone_no_answer = { $remote } ne répond pas

# draft
remote_rclone_no_kind = rclone n’a pas de type de stockage « { $kind } »

# draft
remote_rclone_no_link = rclone n’a créé aucun lien

# draft
remote_rclone_no_log_folder = aucun dossier pour son journal

# draft
remote_rclone_no_mounts_folder = il n’y a pas de dossier où monter

# draft
remote_rclone_no_option = { $kind } n’a pas d’option « { $option } »

# draft
remote_rclone_no_reason = aucune raison donnée

# draft
remote_rclone_no_remote = aucun stockage monté

# draft
remote_rclone_no_remote_named = la configuration de rclone ne nomme aucun stockage { $remote }

# draft
remote_rclone_no_runtime_folder = pas de dossier d’exécution

# draft
remote_rclone_no_such_host = le nom du serveur est introuvable

# draft
remote_rclone_no_such_name = rclone n’accepte pas ce nom

# draft
remote_rclone_no_such_remote = la configuration de rclone ne nomme aucun stockage de ce nom

# draft
remote_rclone_not_asked = Skwir ne l’a pas monté : rclone ne peut donc pas être interrogé

# draft
remote_rclone_not_obscured = rclone n’a pas obscurci le mot de passe

# draft
remote_rclone_not_ours = Skwir ne l’a pas monté : il ne peut donc pas le démonter

# draft
remote_rclone_not_reached = rclone n’a pas pu être joint pour le démonter

# draft
remote_rclone_not_serving = rclone n’a pas commencé à servir sa configuration

# draft
remote_rclone_ok = OK

# draft
remote_rclone_on_remotes = Sur d’autres stockages

# draft
remote_rclone_option_advanced = « { $option } » est une option avancée : l’utilisateur la règle dans la fenêtre

# draft
remote_rclone_option_secret = « { $option } » est un secret : l’utilisateur le tape dans la fenêtre

# draft
remote_rclone_provider = Fournisseur

# draft
remote_rclone_rc_login = rclone a refusé { $method } : son authentification a été refusée

# draft
remote_rclone_rc_no_http = la réponse de rclone à { $method } n’est pas du HTTP

# draft
remote_rclone_rc_refused = rclone a refusé { $method } : { $why }

# draft
remote_rclone_remove = Retirer

# draft
remote_rclone_remove_intro = La configuration de rclone oublie { $remote } : ce qu’il contient reste où il est, et l’ajouter de nouveau le fait revenir.

# draft
remote_rclone_remove_mounted_intro = La configuration de rclone oublie { $remote } : ce qu’il contient reste où il est, et l’ajouter de nouveau le fait revenir. Skwir le déconnecte d’abord.

# draft
remote_rclone_remove_title = Retirer { $remote } ?

# draft
remote_rclone_save = Enregistrer

# draft
remote_rclone_scheme = rclone n’ouvre pas les adresses { $scheme }

# draft
remote_rclone_server_label = { $name } ({ $kind })

# draft
remote_rclone_set_up = Configurer { $remote }

# draft
remote_rclone_sign_in = Se connecter à { $remote }

# draft
remote_rclone_sign_in_page = Ouvrir la page de connexion

# draft
remote_rclone_sign_in_waiting = Terminez la connexion dans votre navigateur : rclone attend la réponse. Si aucune page ne s’est ouverte, ouvrez-la ici.

# draft
remote_rclone_stopped = rclone s’est arrêté

# draft
remote_rclone_storage = Stockages

# draft
remote_rclone_trash_box = la corbeille de Box

# draft
remote_rclone_trash_drive = la corbeille de Google Drive

# draft
remote_rclone_trash_jottacloud = la corbeille de Jottacloud

# draft
remote_rclone_trash_mega = la corbeille de MEGA

# draft
remote_rclone_trash_onedrive = la corbeille de OneDrive

# draft
remote_rclone_trash_pcloud = la corbeille de pCloud

# draft
remote_rclone_trash_yandex = la corbeille de Yandex Disk

# draft
remote_rclone_trying = Essai de { $remote }

# draft
remote_rclone_trying_what = rclone liste sa racine, comme le ferait une connexion.

# draft
remote_rclone_unlisted = rclone n’a pas pu le lister : { $why }. Gardez-le tel quel, ou revenez en arrière pour le modifier.

# draft
remote_rclone_unlisted_new = rclone n’a pas pu le lister : { $why }. Gardez-le tel quel, ou revenez en arrière pour le modifier. Le bouton Annuler le retire.

# draft
remote_rclone_unmount_late = rclone ne l’a pas démonté

# draft
remote_rclone_unsaid = « { $path } » ne peut pas être transmis à rclone

# draft
remote_rclone_you_cancelled = vous avez annulé

# draft
remote_refresh = Actualiser les dossiers distants

# draft
remote_refresh_description = Relire ce qui est monté et les serveurs que connaissent les outils

# draft
remote_remembered_data = Les serveurs dont le mot de passe est conservé dans le trousseau, par leur adresse

# draft
remote_rsync_between = rsync copie entre cette machine et un serveur

# draft
remote_rsync_failed = rsync a échoué ({ $status }) : { $said }

# draft
remote_rsync_unnamed = « { $path } » a un nom qui ne peut pas être donné à rsync

# draft
remote_runs_failed = { $tool } a échoué ({ $status }) : { $said }

# draft
remote_sending = Envoi de { $count ->
        [one] { $count } fichier
       *[other] { $count } fichiers
    } au serveur…

# draft
remote_server_form_add = Ajouter

# draft
remote_server_form_add_afp = Ajouter un volume AFP

# draft
remote_server_form_add_dav = Ajouter un serveur WebDAV

# draft
remote_server_form_add_davs = Ajouter un serveur WebDAV (HTTPS)

# draft
remote_server_form_add_ftp = Ajouter un serveur FTP

# draft
remote_server_form_add_ftps = Ajouter un serveur FTP (TLS)

# draft
remote_server_form_add_intro = Un serveur par son adresse. Avec rclone installé, soixante-dix types de stockage de plus.

# draft
remote_server_form_add_nfs = Ajouter un export NFS

# draft
remote_server_form_add_other = Ajouter { $kind }

# draft
remote_server_form_add_server = Ajouter un serveur

# draft
remote_server_form_add_sftp = Ajouter un serveur SFTP

# draft
remote_server_form_add_smb = Ajouter un partage Windows

# draft
remote_server_form_afp = Volume AFP

# draft
remote_server_form_afp_detail = Le volume partagé d’un ancien Mac ou d’un NAS

# draft
remote_server_form_already = { $server } est déjà un serveur, à cette adresse

# draft
remote_server_form_dav = Serveur WebDAV

# draft
remote_server_form_dav_detail = Nextcloud, ownCloud et d’autres, en HTTP

# draft
remote_server_form_davs = Serveur WebDAV (HTTPS)

# draft
remote_server_form_davs_detail = Nextcloud, ownCloud et d’autres

# draft
remote_server_form_domain = Domaine

# draft
remote_server_form_domain_help = Le domaine ou le groupe de travail Windows, si le serveur en demande un

# draft
remote_server_form_edit = Modifier { $server }

# draft
remote_server_form_edit_intro = Conservé { $kept }. Ses options de montage le suivent à une nouvelle adresse.

# draft
remote_server_form_export = Export

# draft
remote_server_form_export_help = Le chemin du dossier exporté, sur le serveur : /srv/photos

# draft
remote_server_form_folder = Dossier

# draft
remote_server_form_folder_help = Où ouvrir, sur le serveur : vide pour sa racine (SFTP : votre dossier personnel)

# draft
remote_server_form_ftp = Serveur FTP

# draft
remote_server_form_ftp_detail = Les fichiers d’un serveur, en FTP

# draft
remote_server_form_ftps = Serveur FTP (TLS)

# draft
remote_server_form_ftps_detail = Les fichiers d’un serveur, en FTP avec TLS

# draft
remote_server_form_host = Serveur

# draft
remote_server_form_host_help = Son nom ou son adresse : nas.local, files.example.com, 192.168.1.20

# draft
remote_server_form_host_needed = Un serveur est nécessaire

# draft
remote_server_form_intro_kde = Conservé dans les dossiers réseau de KDE, où Dolphin le liste aussi. Son mot de passe est demandé à sa première connexion.

# draft
remote_server_form_intro_skwir = Conservé dans la liste de serveurs de Skwir. Son mot de passe est demandé à sa première connexion.

# draft
remote_server_form_kind = Type de serveur

# draft
remote_server_form_name = Nom

# draft
remote_server_form_name_help = Son nom dans la barre latérale ; celui de son adresse s’il est vide

# draft
remote_server_form_next = Suivant

# draft
remote_server_form_nfs = Export NFS

# draft
remote_server_form_nfs_detail = Le dossier partagé d’un serveur Unix

# draft
remote_server_form_no_part = l’adresse d’un serveur n’a pas de partie « { $key } »

# draft
remote_server_form_port = Port

# draft
remote_server_form_port_refused = Un port est un nombre de 1 à 65 535

# draft
remote_server_form_save = Enregistrer

# draft
remote_server_form_section = Serveurs, par leur adresse

# draft
remote_server_form_sftp = Serveur SFTP

# draft
remote_server_form_sftp_detail = Un ordinateur joint par ssh

# draft
remote_server_form_share = Partage

# draft
remote_server_form_share_help = Le nom du dossier partagé, et l’un de ses dossiers après une barre oblique (/)

# draft
remote_server_form_smb = Partage Windows (SMB)

# draft
remote_server_form_smb_detail = Le dossier partagé d’un NAS ou d’un ordinateur

# draft
remote_server_form_user = Utilisateur

# draft
remote_server_form_user_help = Le nom avec lequel vous vous connectez ; vide pour le vôtre (SFTP : selon votre configuration ssh)

# draft
remote_server_form_volume = Volume

# draft
remote_server_form_volume_help = Le nom du volume partagé, et l’un de ses dossiers après une barre oblique (/)

# draft
remote_servers_kde_read_only = le dossier réseau de KDE est en lecture seule ; modifiez-le là où il est géré

# draft
remote_servers_kept_automatic = Automatique

# draft
remote_servers_kept_dolphin = l’un des emplacements de Dolphin

# draft
remote_servers_kept_fstab = dans le fstab du système

# draft
remote_servers_kept_in = Où les serveurs sont conservés

# draft
remote_servers_kept_in_description = Où est conservé un serveur ajouté par son adresse.

# draft
remote_servers_kept_in_description_more = Automatique : dans les dossiers réseau de KDE sur le bureau KDE, où Dolphin les liste aussi, et ailleurs dans la liste propre à Skwir ; un type que KIO n’ouvre pas (FTPS, AFP), toujours dans celle de Skwir.

# draft
remote_servers_kept_kde = dans les dossiers réseau de KDE

# draft
remote_servers_kept_rclone = dans la configuration de rclone

# draft
remote_servers_kept_skwir = dans la liste de Skwir

# draft
remote_servers_kio_opens_no = KIO n’ouvre aucune adresse { $protocol }

# draft
remote_servers_list = Les serveurs ajoutés par leur adresse que Skwir conserve lui-même : le nom et l’adresse de chacun

# draft
remote_servers_no_kde_folder = il n’y a pas de dossier de données pour les dossiers réseau de KDE

# draft
remote_servers_none_kept = aucun serveur n’est conservé à l’adresse { $address }

# draft
remote_servers_other_there = un autre serveur est conservé à cette adresse

# draft
remote_servers_taken = { $name } est déjà un serveur, à cette adresse

# draft
remote_sftp_from_here = sftp copie de cette machine vers un serveur, ou au sein d’un même serveur

# draft
remote_sftp_onto_server = sftp copie vers un serveur

# draft
remote_sftp_unsaid = « { $path } » ne peut pas être transmis à sftp

# draft
remote_share_label = { $share } sur { $server }

# draft
remote_ssh_still_there = « { $path } » est toujours là

# draft
remote_sshfs_connection_refused = le serveur a refusé la connexion

# draft
remote_sshfs_identity_changed = l’identité du serveur a changé depuis la dernière fois : ssh la refuse (voir ~/.ssh/known_hosts)

# draft
remote_sshfs_identity_unconfirmed = l’identité du serveur n’a pas été confirmée

# draft
remote_sshfs_login_refused = le serveur a refusé l’authentification (mot de passe erroné, ou aucune clé qu’il accepte)

# draft
remote_sshfs_name_not_found = le nom du serveur est introuvable

# draft
remote_sshfs_no_answer = le serveur n’a pas répondu

# draft
remote_sshfs_not_windows = sshfs ne fonctionne pas sous Windows

# draft
remote_system_shares_elsewhere = les partages du système lui-même sont ceux de Windows et de macOS : ici, sshfs, GVfs, KIO ou rclone les montent

# draft
remote_system_shares_login_refused = le serveur a refusé l’authentification

# draft
remote_system_shares_no_share = « { $address } » ne nomme aucun partage : smb://serveur/partage

# draft
remote_thumbnails = Vignettes dans les dossiers distants

# draft
remote_thumbnails_always = Toujours

# draft
remote_thumbnails_automatic = Automatique

# draft
remote_thumbnails_choice = Vignettes

# draft
remote_thumbnails_description = Pour un dossier distant pour lequel vous n’avez rien choisi dans son menu : Automatique ne les affiche que sur le réseau local, car lire chaque image à travers Internet serait lent.

# draft
remote_thumbnails_menu_always = Toujours

# draft
remote_thumbnails_menu_automatic = Automatique

# draft
remote_thumbnails_menu_never = Jamais

# draft
remote_thumbnails_menu_small = Petits fichiers

# draft
remote_thumbnails_never = Jamais

# draft
remote_thumbnails_not_saved = Le choix du moment où les fichiers du dossier distant affichent des vignettes n’a pas été enregistré : { $error }

# draft
remote_thumbnails_of = Vignettes d’un dossier distant

# draft
remote_thumbnails_of_description = Choisir quand les fichiers d’un dossier distant montrent des vignettes : automatic, never, small ou always

# draft
remote_thumbnails_rule = une règle est automatic, never, small ou always

# draft
remote_thumbnails_small = Petits fichiers

# draft
remote_thumbnails_submenu = Vignettes

# draft
remote_tool_failed = { $tool } : { $error }

# draft
remote_tool_unknown = Skwir ne connaît pas l’outil qui l’a monté

# draft
remote_tools_failed = { $failures }

# draft
remote_under_way = { $count ->
        [1] { $actions } y est en cours
        [one] { $actions } y est en cours
       *[other] { $actions } y sont en cours
    }

# draft
remote_under_way_title = « { $title } »

# draft
remote_unfinished_data = Les stockages rclone dont Skwir a commencé la configuration sans la terminer, par nom

# draft
remote_uploads_mount_gone = le montage de « { $path } » par rclone a disparu avant que son serveur ait reçu ce qui y a été écrit

# draft
remote_uploads_never_queued = « { $path } » n’a jamais atteint la file d’attente de rclone : un programme le tient encore ouvert là-bas, et le serveur ne l’a pas

# draft
remote_uploads_no_answer = rclone n’a pas répondu en { $seconds } s : son serveur a peut-être cessé de répondre

# draft
remote_uploads_no_cache_mode = rclone n’a pas dit quel cache il garde

# draft
remote_uploads_refused = le serveur a refusé « { $name } » { $tries ->
        [one] { $tries } fois
       *[other] { $tries } fois
    } : rclone le garde sur ce disque et réessaie

# draft
remote_winfsp_loopback = un montage sous Windows répond sur l’interface de bouclage

# draft
remote_winfsp_missing = WinFsp n’est pas installé : rclone monte par lui sous Windows

# draft
remote_winfsp_no_login_folder = aucun dossier pour ses identifiants
