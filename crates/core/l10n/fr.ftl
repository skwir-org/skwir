# French catalog of skwir-core, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
access_another_file = { $name } est maintenant un autre fichier

# draft
access_both_already = { $inside ->
        [yes] les droits d’accès et le groupe de { $name } et de tout son contenu étaient déjà comme demandé
       *[no] les droits d’accès et le groupe de { $name } étaient déjà comme demandé
    }

# draft
access_change_both = { $inside ->
        [yes] modifier les droits d’accès et le groupe de { $name } et de tout son contenu
       *[no] modifier les droits d’accès et le groupe de { $name }
    }

# draft
access_change_group = { $inside ->
        [yes] modifier le groupe de { $name } et de tout son contenu
       *[no] modifier le groupe de { $name }
    }

# draft
access_change_mode = { $inside ->
        [yes] modifier les droits d’accès de { $name } et de tout son contenu ({ $mode })
       *[no] modifier les droits d’accès de { $name } ({ $mode })
    }

# draft
access_changed_both = { $inside ->
        [yes] droits d’accès et groupe de { $name } et de tout son contenu modifiés
       *[no] droits d’accès et groupe de { $name } modifiés
    }

# draft
access_changed_group = { $inside ->
        [yes] groupe de { $name } et de tout son contenu modifié
       *[no] groupe de { $name } modifié
    }

# draft
access_changed_mode = { $inside ->
        [yes] droits d’accès de { $name } et de tout son contenu modifiés
       *[no] droits d’accès de { $name } modifiés
    }

# draft
access_clause_also = { $who ->
        [u] le propriétaire peut aussi { $does }
        [g] le groupe peut aussi { $does }
        [o] les autres peuvent aussi { $does }
        [ug] le propriétaire et le groupe peuvent aussi { $does }
        [uo] le propriétaire et les autres peuvent aussi { $does }
        [go] le groupe et les autres peuvent aussi { $does }
        [umask] tout le monde, moins ce que retire l’umask ({ $umask }), peut aussi { $does }
       *[all] tout le monde peut aussi { $does }
    }

# draft
access_clause_no_longer = { $who ->
        [u] le propriétaire ne peut plus { $does }
        [g] le groupe ne peut plus { $does }
        [o] les autres ne peuvent plus { $does }
        [ug] le propriétaire et le groupe ne peuvent plus { $does }
        [uo] le propriétaire et les autres ne peuvent plus { $does }
        [go] le groupe et les autres ne peuvent plus { $does }
        [umask] plus personne, hors ce que retire l’umask ({ $umask }), ne peut { $does }
       *[all] plus personne ne peut { $does }
    }

# draft
access_clause_none = { $who ->
        [u] le propriétaire ne peut pas l’utiliser
        [g] le groupe ne peut pas l’utiliser
        [o] les autres ne peuvent pas l’utiliser
        [ug] le propriétaire et le groupe ne peuvent pas l’utiliser
        [uo] le propriétaire et les autres ne peuvent pas l’utiliser
        [go] le groupe et les autres ne peuvent pas l’utiliser
        [umask] personne, hors ce que retire l’umask ({ $umask }), ne peut l’utiliser
       *[all] personne ne peut l’utiliser
    }

# draft
access_clause_nothing = rien ne change pour { $who ->
        [u] le propriétaire
        [g] le groupe
        [o] les autres
        [ug] le propriétaire et le groupe
        [uo] le propriétaire et les autres
        [go] le groupe et les autres
        [umask] tout le monde, moins ce que retire l’umask ({ $umask })
       *[all] tout le monde
    }

# draft
access_clause_only = { $who ->
        [u] le propriétaire ne peut que { $does }
        [g] le groupe ne peut que { $does }
        [o] les autres ne peuvent que { $does }
        [ug] le propriétaire et le groupe ne peuvent que { $does }
        [uo] le propriétaire et les autres ne peuvent que { $does }
        [go] le groupe et les autres ne peuvent que { $does }
        [umask] tout le monde, moins ce que retire l’umask ({ $umask }), ne peut que { $does }
       *[all] tout le monde ne peut que { $does }
    }

# draft
access_does_both = { $letters ->
        [r] lire
        [w] écrire
        [x] exécuter
        [rw] lire et écrire
        [rx] lire et exécuter
        [wx] écrire et exécuter
        [X] ouvrir un dossier, ou exécuter un fichier que quelqu’un peut déjà exécuter
        [rX] lire, et ouvrir un dossier ou exécuter un fichier que quelqu’un peut déjà exécuter
        [wX] écrire, et ouvrir un dossier ou exécuter un fichier que quelqu’un peut déjà exécuter
        [rwX] lire, écrire, et ouvrir un dossier ou exécuter un fichier que quelqu’un peut déjà exécuter
       *[rwx] lire, écrire et exécuter
    }

# draft
access_does_copy = faire ce que { $class ->
        [u] peut faire le propriétaire
        [g] peut faire le groupe
       *[o] peuvent faire les autres
    }

# draft
access_does_file = { $letters ->
        [r] lire
        [w] écrire
        [x] exécuter
        [rw] lire et écrire
        [rx] lire et exécuter
        [wx] écrire et exécuter
        [X] exécuter ce que quelqu’un peut déjà exécuter
        [rX] lire, et exécuter ce que quelqu’un peut déjà exécuter
        [wX] écrire, et exécuter ce que quelqu’un peut déjà exécuter
        [rwX] lire, écrire, et exécuter ce que quelqu’un peut déjà exécuter
       *[rwx] lire, écrire et exécuter
    }

# draft
access_does_folder = { $letters ->
        [r] le lister
        [w] changer ce qu’il contient
        [x] l’ouvrir
        [rw] le lister et changer ce qu’il contient
        [rx] le lister et l’ouvrir
        [wx] changer ce qu’il contient et l’ouvrir
       *[rwx] le lister, changer ce qu’il contient et l’ouvrir
    }

# draft
access_drive_read_only = le disque est en lecture seule

# draft
access_each_class = le propriétaire { $owner ->
        [none] ne peut pas l’utiliser
       *[some] peut { $owner_does }
    }, le groupe { $group ->
        [none] ne peut pas l’utiliser
       *[some] peut { $group_does }
    }, les autres { $others ->
        [none] ne peuvent pas l’utiliser
       *[some] peuvent { $others_does }
    }

# draft
access_everyone_may = { $all ->
        [none] personne ne peut l’utiliser
       *[some] tout le monde peut { $all_does }
    }

# draft
access_failed_not_put_back = { $error }, et { $not_put_back }

# draft
access_files_and_folders = fichiers { $files } ; dossiers { $folders }

# draft
access_folder_keeps_set_id = { $said } ; un dossier garde les bits set-ID qu’il a ({ $clears } les efface)

# draft
access_gone = { $name } n’est plus là

# draft
access_group_already = { $inside ->
        [yes] le groupe de { $name } et de tout son contenu était déjà comme demandé
       *[no] le groupe de { $name } était déjà comme demandé
    }

# draft
access_in_trash = { $name } est dans la corbeille : restaurez-le d’abord

# draft
access_inside_not_yours = « { $path } », à l’intérieur, ne vous appartient pas : seul son propriétaire peut modifier ses droits d’accès

# draft
access_mode_already = { $inside ->
        [yes] les droits d’accès de { $name } et de tout son contenu étaient déjà comme demandé
       *[no] les droits d’accès de { $name } étaient déjà comme demandé
    }

# draft
access_mode_comes_to = { $mode } : { $who }

# draft
access_no_class = « { $part } » ne nomme aucune classe, et ce système ne dit pas ce que son umask retire, comme chmod le ferait : nommez les classes, par exemple u{ $part } pour le propriétaire ou a{ $part } pour tout le monde

# draft
access_not_a_mode = « { $text } » n’est pas un mode : écrivez-le comme chmod, par exemple 644, u+x ou go-w

# draft
access_not_changed = les droits d’accès de « { $path } » n’ont pas pu être modifiés

# draft
access_not_given_back = il n’a pas pu être rendu au groupe { $group }

# draft
access_not_given_group = « { $path } » n’a pas pu être attribué au groupe { $group }

# draft
access_not_in_group = vous n’êtes pas dans le groupe { $group }

# draft
access_not_kept = le disque qui contient { $name } n’a pas gardé la modification : son système de fichiers ne garde aucune permission

# draft
access_not_owner = seul son propriétaire peut modifier ses droits d’accès

# draft
access_not_put_back = { $more ->
        [0] les droits d’accès de « { $path } » n’ont pas pu être rétablis : { $error }
        [one] les droits d’accès de « { $path } » n’ont pas pu être rétablis (ni ceux de { $more } autre élément) : { $error }
       *[other] les droits d’accès de « { $path } » n’ont pas pu être rétablis (ni ceux de { $more } autres éléments) : { $error }
    }

# draft
access_not_read_only = Pas en lecture seule

# draft
access_not_yours = « { $path } » ne vous appartient pas

# draft
access_nothing_asked = rien à modifier : donnez un mode ou un groupe

# draft
access_owner_group_then_others = le propriétaire et le groupe { $both ->
        [none] ne peuvent pas l’utiliser
       *[some] peuvent { $both_does }
    }, les autres { $others ->
        [none] ne peuvent pas l’utiliser
       *[some] peuvent { $others_does }
    }

# draft
access_owner_then_rest = le propriétaire { $owner ->
        [none] ne peut pas l’utiliser
       *[some] peut { $owner_does }
    }, le groupe et les autres { $rest ->
        [none] ne peuvent pas l’utiliser
       *[some] peuvent { $rest_does }
    }

# draft
access_put_back = { $more ->
        [0] rétablir les droits d’accès de { $name }
        [one] rétablir les droits d’accès de { $name } et de { $more } autre élément
       *[other] rétablir les droits d’accès de { $name } et de { $more } autres éléments
    }

# draft
access_put_back_already = { $more ->
        [0] les droits d’accès de { $name } étaient déjà comme avant
        [one] les droits d’accès de { $name } et de { $more } autre élément étaient déjà comme avant
       *[other] les droits d’accès de { $name } et de { $more } autres éléments étaient déjà comme avant
    }

# draft
access_put_back_done = { $more ->
        [0] droits d’accès de { $name } rétablis
        [one] droits d’accès de { $name } et de { $more } autre élément rétablis
       *[other] droits d’accès de { $name } et de { $more } autres éléments rétablis
    }

# draft
access_read_only = Lecture seule

# draft
access_reference = Accès

# draft
access_reference_600 = Seul le propriétaire lit et écrit

# draft
access_reference_644 = Le propriétaire lit et écrit ; le groupe et les autres lisent

# draft
access_reference_700 = Seul le propriétaire lit, écrit et exécute

# draft
access_reference_755 = Tout le monde lit et exécute ; le propriétaire écrit aussi

# draft
access_reference_also = Peut aussi

# draft
access_reference_big_x = Ouvrir les dossiers, et exécuter ce que quelqu’un peut déjà exécuter

# draft
access_reference_clauses_note = Les clauses se suivent, séparées par des virgules : u+x,go-w.

# draft
access_reference_digits = Chiffres : propriétaire, groupe, autres

# draft
access_reference_digits_note = Chaque chiffre ajoute 4 pour lire, 2 pour écrire et 1 pour exécuter (pour ouvrir un dossier).

# draft
access_reference_everyone = Tout le monde

# draft
access_reference_group = Le groupe

# draft
access_reference_no_longer = Ne peut plus

# draft
access_reference_only = Ne peut que

# draft
access_reference_others = Les autres

# draft
access_reference_owner = Le propriétaire

# draft
access_reference_r = Lire (lister, pour un dossier)

# draft
access_reference_s = S’exécuter avec les droits de son propriétaire ou de son groupe ; le groupe d’un dossier se transmet

# draft
access_reference_t = Les éléments d’un dossier ne sont supprimés que par leur propriétaire

# draft
access_reference_then = Puis

# draft
access_reference_w = Écrire (changer ce que contient un dossier)

# draft
access_reference_what = Quoi

# draft
access_reference_who = Qui

# draft
access_reference_x = Exécuter (ouvrir, pour un dossier)

# draft
access_refused = { $name } : { $why }

# draft
access_set_gid_cleared = le système a retiré le bit set-group-ID de { $name } : seul un membre de son groupe peut l’activer

# draft
access_sgid_both_file = un fichier s’exécute avec les droits de son groupe

# draft
access_sgid_both_file_gone = un fichier ne s’exécute plus avec les droits de son groupe

# draft
access_sgid_both_folder = ce qui est créé dans un dossier prend son groupe

# draft
access_sgid_both_folder_gone = ce qui est créé dans un dossier ne prend plus son groupe

# draft
access_sgid_file = il s’exécute avec les droits de son groupe

# draft
access_sgid_file_gone = il ne s’exécute plus avec les droits de son groupe

# draft
access_sgid_folder = ce qui y est créé prend son groupe

# draft
access_sgid_folder_gone = ce qui y est créé ne prend plus son groupe

# draft
access_sticky_both = chaque élément d’un dossier ne peut être supprimé que par son propriétaire

# draft
access_sticky_both_gone = quiconque peut modifier un dossier peut supprimer ce qu’il contient

# draft
access_sticky_file = il garde un bit sticky, qu’un fichier ignore

# draft
access_sticky_file_gone = son bit sticky, qu’un fichier ignore, est retiré

# draft
access_sticky_folder = chaque élément qu’il contient ne peut être supprimé que par son propriétaire

# draft
access_sticky_folder_gone = quiconque peut le modifier peut supprimer ce qu’il contient

# draft
access_stopped_not_put_back = arrêté, et { $not_put_back }

# draft
access_suid_both = un fichier s’exécute avec les droits de son propriétaire

# draft
access_suid_both_gone = un fichier ne s’exécute plus avec les droits de son propriétaire

# draft
access_suid_file = il s’exécute avec les droits de son propriétaire

# draft
access_suid_file_gone = il ne s’exécute plus avec les droits de son propriétaire

# draft
access_suid_folder = il garde un bit set-user-ID, qu’un dossier ignore

# draft
access_suid_folder_gone = son bit set-user-ID, qu’un dossier ignore, est retiré

# draft
access_then = { $said } ; { $then }

# draft
access_unreadable = « { $path } » n’a pas pu être lu

# draft
access_windows_each_stays = Chacun reste tel quel : seul le w du propriétaire est conservé ici

# draft
app_access_group_title = modifier l’accès et le groupe

# draft
app_access_nothing = indiquez un mode, un groupe ou les deux

# draft
app_access_title = modifier l’accès ({ $mode })

# draft
app_apply_plan = Appliquer un plan

# draft
app_apply_plan_description = Appliquer un plan donné sous forme de données (plan) : comme celui de toute commande, l’appelant l’applique, aussitôt pour un geste de l’utilisateur, après revue pour un agent

# draft
app_arriving = Arrivées dans un dossier

# draft
app_arriving_description = Renvoyer ce que les actions en attente et en cours apportent dans un dossier (celui du panneau quand aucun n’est donné) et qui n’y est pas encore arrivé (ADR-135) : le nom de chaque élément là-bas, le plan qui l’apporte, son état (waiting en attente, starting au démarrage, copying copié avec d’autres à la fraction du lot, moving déplacé à sa propre fraction de 0 à 1, landed en place pendant que son action se poursuit), si son action est en pause, d’où il est copié ou déplacé, et s’il est un dossier ou un fichier, de quelle taille

# draft
app_back = Précédent

# draft
app_back_description = Aller à l’emplacement précédent

# draft
app_commands = Lister les commandes

# draft
app_commands_description = Renvoyer chaque commande enregistrée avec le schéma de ses arguments

# draft
app_copy = Copier vers…

# draft
app_copy_description = Copier la sélection, ou les éléments donnés, dans un dossier (plan) ; on_conflict dit ce que devient un nom déjà pris là-bas

# draft
app_delete = Supprimer définitivement

# draft
app_delete_description = Supprimer la sélection, ou les éléments donnés, sans passer par la corbeille (plan, irréversible)

# draft
app_delete_title = supprimer définitivement

# draft
app_destination_path = la destination doit être un chemin

# draft
app_entries = Lister les éléments

# draft
app_entries_description = Ce qu’affiche un panneau, dans son ordre (trié, filtré, avec les dossiers ouverts sur place), ou avec `location` ce que contient un dossier, listé sans l’afficher : l’identifiant, le nom, le chemin, le genre, la taille et la date de modification (en secondes depuis 1970) de chaque élément, et s’il est sélectionné ; à partir de `offset`, au plus `limit`. `complete: false` : la liste arrivait encore, redemandez. `as_of`, quand il est présent : la réponse est celle d’un index à sa dernière mise à jour (`by` dit lequel, `updated` quand, en secondes depuis 1970), si bien qu’un fichier créé, renommé ou supprimé depuis n’y est pas. `excluded`, quand il est présent : combien de fichiers et de dossiers sous la racine la recherche a passés (les dossiers écartés dans Paramètres › Général › Recherches, caches, corbeille, dossiers réseau) ; `excluded:yes` dans la recherche parcourt ceux de la liste

# draft
app_expand = Déplier le dossier

# draft
app_expand_description = Ouvrir des dossiers sur place sous celui affiché, comme les dessine la vue arborescente, ou les refermer

# draft
app_filter = Filtrer

# draft
app_filter_description = N’afficher que les éléments que garde `text` : tel que le lit le lecteur du filtre (le module de requêtes : les mots trouvent des noms, les termes que la liste peut trancher, comme kind:image, ext:pdf ou size>1M, valent pour les fichiers, un dossier s’affichant dès qu’une recherche en lui en trouve un), sinon ceux dont le nom le contient (casse ignorée sauf s’il contient une majuscule) ; sans `text`, tous. Ses termes qui valent pour les fichiers forment le filtre du panneau, gardé quand il affiche d’autres emplacements jusqu’à ce qu’on le retire ; ses mots vont avec l’emplacement

# draft
app_forward = Suivant

# draft
app_forward_description = Retourner à l’emplacement quitté par Précédent

# draft
app_group = Regrouper

# draft
app_group_as_sorted = Regrouper comme le tri

# draft
app_group_by = Regrouper par { $key }

# draft
app_group_by_way = Regrouper par { $key }, { $way }

# draft
app_group_description = Regrouper les éléments d’un panneau sous des titres (ADR-130) : par `by` (none, same_as_sort : selon la clé du tri, dans son sens, ou une clé comme `sort` les prend : name, kind, type, extension, size, modified, created, accessed, deleted, rating, tags, owner, folder ; taken, dimensions, duration, lues dans les fichiers à la demande), dans le sens `ascending` (une clé commence dans le sien : les mots de A à Z, les plus grands, les plus récents, les mieux notés d’abord ; la même clé à nouveau l’inverse). Dans chaque groupe, les dossiers viennent en premier tant qu’ils le font partout, puis le tri décide

# draft
app_group_fold = Replier le groupe

# draft
app_group_fold_description = Replier ou déplier les groupes d’un panneau (ADR-131) : le groupe `group` (compté depuis le premier titre à partir de 0 ; celui du curseur quand il est omis) comme le dit `mode` : toggle, fold, unfold, fold_others (tous les groupes sauf lui), unfold_all. Les éléments d’un groupe replié quittent ce qu’affiche le panneau (non dessinés, sautés par les touches, hors de Tout sélectionner et de ce sur quoi agissent les commandes), son titre gardé avec son nombre ; les replis sont gardés tant que le panneau regroupe selon le même critère, et ne prennent effet que sous une vue qui dessine les titres des groupes (la liste, l’arborescence, la grille) : sous toute autre, chaque fichier reste affiché

# draft
app_group_fold_others = Replier les autres groupes

# draft
app_group_fold_toggle = Replier ou déplier le groupe

# draft
app_group_none = Ne pas regrouper

# draft
app_group_title = modifier le groupe ({ $group })

# draft
app_group_unfold_all = Déplier tous les groupes

# draft
app_history = Historique des annulations

# draft
app_history_description = Renvoyer les actions qu’Annuler peut défaire, la plus récente d’abord (ADR-079, 13). Chacune donne son `entry` (ce que undo et redo reçoivent comme `entry`), son `title`, `at` : quand elle s’est appliquée (en secondes depuis 1970, UTC), son `state` (en cours, en attente, faite ou annulée), `can` : ce que son Annuler, ou le Rétablir d’une action annulée, peut faire maintenant (`now` ; `once_ended`, tant qu’elle s’exécute ou attend ; `after`, une fois l’action qu’il nomme annulée ou rétablie d’abord ; `going`, pendant qu’elle est annulée ou rétablie ; `never`), et `with` : les entrées que undo avec `with_after` défait avec elle

# draft
app_hydrate = Charger les métadonnées

# draft
app_hydrate_description = Demander les métadonnées d’éléments (les vues l’appellent pour les lignes visibles)

# draft
app_in_no_folder = { $path } n’est dans aucun dossier

# draft
app_keep = Ajouter à la barre latérale

# draft
app_keep_cannot = cet emplacement ne peut pas être gardé dans la barre latérale

# draft
app_keep_description = Garder dans la barre latérale ce qu’affiche le panneau, tel qu’il l’affiche (ADR-109) : un dossier comme signet, une recherche comme recherche enregistrée, tout ce qui est affiché filtré comme emplacement enregistré avec son filtre

# draft
app_keep_nothing = le panneau n’affiche rien à garder

# draft
app_link = Créer un lien dans…

# draft
app_link_description = Créer dans un dossier un lien vers chaque élément de la sélection, ou des éléments donnés, sous son nom (plan) ; on_conflict dit ce que devient un nom déjà pris là-bas

# draft
app_mkdir = Nouveau dossier

# draft
app_mkdir_description = Créer un dossier dans le dossier donné en `in`, ou dans celui qu’affiche le panneau (plan)

# draft
app_move = Déplacer vers…

# draft
app_move_description = Déplacer la sélection, ou les éléments donnés, dans un dossier (plan) ; on_conflict dit ce que devient un nom déjà pris là-bas

# draft
app_navigate = Aller à l’emplacement

# draft
app_navigate_description = Ouvrir un chemin ou un emplacement dans un panneau, avec, une fois listé, l’élément que nomme `select` sélectionné, ou sinon, quand on y est remonté, celui par lequel repasse le chemin du retour ; `in_place` le met à la place de l’étape qu’affiche le panneau, sans étape propre ; `as_kept` l’affiche comme est gardé un emplacement enregistré, filtré par `filter` seul, sans le filtre du panneau (ADR-109)

# draft
app_new_file = Nouveau fichier

# draft
app_new_file_description = Créer un fichier vide dans le dossier donné en `in`, ou dans celui qu’affiche le panneau (plan) ; un nom qui finit par / crée un dossier

# draft
app_new_file_title = nouveau fichier { $name }

# draft
app_new_folder_title = nouveau dossier { $name }

# draft
app_no_directory = le panneau n’affiche pas de dossier

# draft
app_no_pane = ce panneau n’existe pas

# draft
app_no_pane_numbered = aucun panneau { $pane }

# draft
app_no_provider = aucun fournisseur pour le schéma `{ $scheme }`

# draft
app_no_work_dir = sélectionnez quelque chose, ou ouvrez un dossier

# draft
app_notes_read = Lire

# draft
app_notes_read_all = Ce stockage peut contenir toutes les annotations d’un fichier.

# draft
app_notes_read_limits = Ce qu’il ne peut pas contenir : { $limits }.

# draft
app_notes_read_merged = Si Lire est activé, ce qu’il garde s’affiche fusionné avec les annotations des autres stockages.

# draft
app_notes_write = Écrire

# draft
app_notes_write_renames = Jamais ; ou, quand le nom d’un fichier contient déjà des étiquettes, renommer le fichier à mesure qu’une modification les change.

# draft
app_notes_write_when = Jamais ; seulement quand un fichier a déjà des annotations ici ; aussi quand le volume d’un fichier ne garde pas d’attributs ; ou pour chaque fichier modifié.

# draft
app_nothing_taken_out = rien ne se retire d’ici : Mettre à la corbeille supprime

# draft
app_nothing_to_redo = rien à rétablir

# draft
app_nothing_to_retry = rien à réessayer

# draft
app_nothing_to_undo = rien à annuler

# draft
app_orders_unreadable = { $key } dans le fichier de données ne contient pas de liste : corrigez-le, ou retirez-le, pour mémoriser un ordre

# draft
app_pane_went_elsewhere = le panneau est allé ailleurs

# draft
app_passphrase = Phrase secrète

# draft
app_password = Mot de passe

# draft
app_plan_diff = Aperçu du plan

# draft
app_plan_diff_description = Simuler un plan et renvoyer ce que ferait chaque étape

# draft
app_redo = Rétablir

# draft
app_redo_description = Rétablir le dernier plan annulé, ou l’entrée d’historique annulée nommée quand rien d’annulé après elle n’a touché à ce qu’elle a fait ; demandé hors de la fenêtre, dit ce qu’il a appliqué à nouveau ; depuis la fenêtre, répond l’identifiant de son exécution (runs), comme Annuler

# draft
app_refresh = Actualiser

# draft
app_refresh_description = Lister à nouveau l’emplacement du panneau

# draft
app_relative_nowhere = { $path } est un chemin relatif, et aucun dossier affiché ne lui sert de point de départ

# draft
app_remember_order = Mémoriser l’ordre de ce dossier

# draft
app_remember_order_description = Mémoriser comment est trié et regroupé le dossier qu’affiche un panneau (ADR-132), et la vue dans laquelle il est affiché (`view` : list ou grid), ou l’oublier (`on` ; basculé quand il est omis) : un dossier qui mémorise est affiché ainsi dans chaque panneau, un changement de son tri, de ses groupes ou de sa vue fait là changeant ce qu’il mémorise, l’ordre et la vue propres au panneau mis de côté tant qu’il l’affiche et rétablis dès qu’il en affiche un autre ; oublié, ceux du panneau reviennent. Sa vue n’est montrée que tant que le panneau affiche une vue qu’un dossier peut mémoriser, jamais à la place des colonnes ou de l’arborescence ; chaque colonne de la vue en colonnes suit l’ordre de son propre dossier. Vaut pour tout emplacement (la corbeille, l’emplacement d’une étiquette) sauf une recherche, dont les termes sont ce qu’elle est ; les fichiers cachés restent ceux du panneau, les dossiers en premier ceux de l’utilisateur

# draft
app_rename = Renommer

# draft
app_rename_description = Renommer l’élément sélectionné, ou celui donné (plan)

# draft
app_rename_one = renommer demande exactement un élément

# draft
app_rename_title = renommer en { $name }

# draft
app_retry = Réessayer

# draft
app_retry_description = Exécuter à nouveau ce qu’un plan arrêté par une erreur a laissé, le dernier arrêté sauf indication contraire ; ce qu’il applique s’annule avec ce plan ; demandé hors de la fenêtre, dit ce qu’il a appliqué

# draft
app_room_detail = { $title } : un disque, un partage réseau surtout, peut annoncer moins de place qu’il n’en a. Copiez quand même, ou annulez et faites de la place.

# draft
app_running = Actions en cours

# draft
app_running_description = Renvoyer les actions en cours, dans l’ordre où elles ont commencé : pour chacune son titre, les étapes terminées, les octets traités et le total quand il est connu (planned), la vitesse en octets par seconde et le temps restant en secondes tant qu’on peut les dire, et les fichiers qu’un serveur prend encore (ADR-113)

# draft
app_search_order = l’ordre d’une recherche n’est pas mémorisé : ses termes sont ce qu’elle est

# draft
app_select = Sélectionner

# draft
app_select_all = Tout sélectionner

# draft
app_select_all_description = Sélectionner tous les éléments

# draft
app_select_description = Sélectionner des éléments par identifiant ou par nom

# draft
app_select_invert = Inverser la sélection

# draft
app_select_invert_description = Sélectionner ce qui est affiché et non sélectionné, et désélectionner le reste

# draft
app_select_no_entry = aucun élément nommé `{ $name }`

# draft
app_select_none = Tout désélectionner

# draft
app_select_none_description = Ne plus rien sélectionner

# draft
app_select_not_shown = `{ $name }` n’est pas affiché : caché, ou écarté par le filtre

# draft
app_select_range_target = une plage demande l’identifiant de l’élément où elle finit

# draft
app_select_sweep = Sélectionner en déplaçant le curseur

# draft
app_select_sweep_description = Tenir une plage depuis le curseur : se déplacer sélectionne, ou avec `unset` désélectionne, ce qui se trouve entre les deux

# draft
app_select_sweep_end = Arrêter de sélectionner en déplaçant le curseur

# draft
app_select_sweep_end_description = Terminer la plage tenue depuis le curseur, en gardant ce qu’elle couvre

# draft
app_select_sweep_unset = Désélectionner en déplaçant le curseur

# draft
app_set_access = Modifier l’accès

# draft
app_set_access_describe = Dire ce que fait un mode

# draft
app_set_access_describe_description = Dire en mots ce que `mode` (tel que chmod le prend) fait d’éléments dont le mode est `current` (tous le partageant, un nombre ; sinon ce que chaque clause fait à chacun), `folders` ou `files` quand ils en sont tous (sinon les deux, dits à part là où ils diffèrent) ; une erreur pour ce qui n’est pas un mode, comme `set_access` le refuse

# draft
app_set_access_description = Modifier qui peut lire, écrire et exécuter la sélection, ou les éléments donnés : `mode` tel que chmod le prend, `group` par son numéro, `inside` pour ce que contiennent les dossiers (plan). C’est la cible d’un lien qui change ; le propriétaire, jamais

# draft
app_show_companions = Afficher les fichiers cachés et annexes

# draft
app_show_companions_description = Afficher les fichiers cachés dans le panneau, et activer Afficher les fichiers annexes (`core.show_companions`) : les fichiers qui gardent les annotations d’un autre fichier à côté de lui, dans chaque dossier

# draft
app_show_in_folder = Afficher dans le dossier

# draft
app_show_in_folder_description = Ouvrir le dossier qui contient le fichier sélectionné, ce fichier sélectionné : depuis une recherche, une étiquette, l’étagère, où sont listés des fichiers de nombreux dossiers

# draft
app_show_in_folder_one = sélectionnez un seul fichier à afficher dans son dossier

# draft
app_shows_nothing = le panneau n’affiche rien

# draft
app_sort = Trier

# draft
app_sort_ascending = Trier par ordre croissant

# draft
app_sort_by = Trier par { $key }

# draft
app_sort_by_way = Trier par { $key }, { $way }

# draft
app_sort_descending = Trier par ordre décroissant

# draft
app_sort_description = Régler la clé de tri, le sens et l’affichage des fichiers cachés d’un panneau

# draft
app_take_out = Retirer d’ici

# draft
app_take_out_description = Retirer la sélection d’un emplacement qui liste des fichiers gardés ailleurs, à sa façon (celui d’une étiquette leur retire son étiquette, celui de l’étagère les retire de l’étagère) : les fichiers restent où ils sont. Ce que fait la touche Suppr à cet endroit

# draft
app_toast_redo = Rétablir

# draft
app_toast_undo = Annuler

# draft
app_toggle_hidden = Afficher ou masquer les fichiers cachés

# draft
app_toggle_hidden_description = Afficher ou masquer les éléments dont le nom commence par un point

# draft
app_trash = Mettre à la corbeille

# draft
app_trash_description = Mettre à la corbeille la sélection, ou les éléments donnés (plan)

# draft
app_trash_none_kept = { $count ->
        [1] Il se trouve dans un dossier réseau, qui ne garde pas de corbeille
        [one] Ils se trouvent dans des dossiers réseau, qui ne gardent pas de corbeille
       *[other] Ils se trouvent dans des dossiers réseau, qui ne gardent pas de corbeille
    }

# draft
app_trash_others_kept = { $count ->
        [1] Il se trouve dans un dossier réseau, qui ne garde pas de corbeille
        [one] Ils se trouvent dans des dossiers réseau, qui ne gardent pas de corbeille
       *[other] Ils se trouvent dans des dossiers réseau, qui ne gardent pas de corbeille
    } ; { $kept ->
        [1] l’autre élément va à la corbeille
        [one] { $kept } autre élément va à la corbeille
       *[other] les { $kept } autres éléments vont à la corbeille
    }

# draft
app_trash_title = mettre à la corbeille

# draft
app_undo = Annuler

# draft
app_undo_description = Annuler le plan le plus récent, ou le plan ou l’entrée d’historique nommés quand rien de demandé après eux n’a touché à ce qu’ils ont fait (with_after, avec une entrée : avec les entrées suivantes qui y ont touché, la plus récente d’abord) ; demandé hors de la fenêtre, dit ce qu’il a annulé, par les titres des entrées de l’historique ; depuis la fenêtre, répond les identifiants de ses exécutions (runs), pour que la fenêtre ne suive que les siennes

# draft
app_undo_plan_or_entry = nommez un plan ou une entrée, pas les deux

# draft
app_undo_with_after = with_after annule une entrée avec ce qui est venu après elle : nommez l’entrée

# draft
app_up = Dossier parent

# draft
app_up_description = Aller au dossier qui contient celui affiché, ou à l’emplacement qui le précède dans son fil d’Ariane (la catégorie d’une étiquette, le dossier où cherche une recherche), et y sélectionner celui qu’on vient de quitter

# draft
clash_taken = { $name } est déjà là

# draft
command_invalid_args = arguments non valides pour `{ $command }` : { $reason }

# draft
command_nothing_selected = rien n’est sélectionné

# draft
command_nothing_to_paste = rien à coller

# draft
command_panicked = la commande « { $command } » s’est arrêtée sur une erreur qui lui est propre (le journal dit où)

# draft
command_unknown = commande inconnue `{ $command }`

# draft
command_with_args = { $title } ({ $args })

# draft
entry_kept_elsewhere = { $count ->
        [one] { $count } fichier a d’autres noms ailleurs : le disque garde ses { $size }
       *[other] { $count } fichiers ont d’autres noms ailleurs : le disque garde leurs { $size }
    }

# draft
exclusions_agent_exposes = un agent ne peut pas rendre lisibles des dossiers privés ({ $folders }) : vous seul pouvez le faire, dans Paramètres, Exclusions

# draft
exclusions_agent_shell = un agent ne peut pas changer si les variables du shell de connexion sont lues : un dossier privé qu’elles seules nomment cesserait d’être privé. Vous seul pouvez le faire, dans Paramètres, Confidentialité

# draft
exclusions_bad_glob = { $pattern } : { $error }

# draft
exclusions_bad_names = names vaut files, folders ou both, pas { $names }

# draft
exclusions_bad_regex = regex : { $error }

# draft
exclusions_bad_variable = { $pattern } : une variable s’écrit $NOM, ${"{"}NOM{"}"} ou ${"{"}NOM:-ce qui la remplace si elle n’est pas définie{"}"}

# draft
exclusions_beside_files = { $pattern } : l’exclusion à côté d’un fichier vaut pour les dossiers, pas pour les fichiers

# draft
exclusions_beside_name = { $sibling } : beside désigne un fichier ou un dossier, pas un chemin

# draft
exclusions_beside_path = { $pattern } : à côté d’un fichier, le dossier se donne par un nom (target), pas par un chemin

# draft
exclusions_builtin_id = skwir. désigne les dossiers propres à Skwir ; donnez aux vôtres un nom sans point

# draft
exclusions_categories = Catégories d’exclusions

# draft
exclusions_categories_description = Ce qui a changé dans les catégories d’exclusions de Skwir (un rôle activé ou désactivé : searches, agents, index), et les vôtres (un nom)

# draft
exclusions_categories_table = les catégories de dossiers sont une table, par identifiant

# draft
exclusions_category_name = une catégorie à vous doit avoir un nom

# draft
exclusions_category_not_table = une catégorie est une table

# draft
exclusions_empty = un dossier a besoin d’un nom ou d’un chemin

# draft
exclusions_entries = Exclusions

# draft
exclusions_entries_description = Ce qui a changé dans les exclusions de Skwir (un rôle décoché, une catégorie), et les vôtres : un motif (un nom, un chemin, un glob ou regex:…, de fichiers, de dossiers ou des deux, avec beside), ou holding (un fichier que contient un dossier), ce qu’elle est, sa catégorie et les rôles dont ce qu’elle désigne est exclu

# draft
exclusions_entries_table = les exclusions sont une table, par identifiant

# draft
exclusions_entry_not_table = une entrée est une table

# draft
exclusions_holding_alone = beside et names vont avec pattern, pas avec holding

# draft
exclusions_holding_name = holding est le nom d’un fichier que contient un dossier

# draft
exclusions_id_dot = un identifiant à vous ne contient pas de point : ceux qui en ont sont à Skwir et à ses modules

# draft
exclusions_legacy_folders = Dossiers privés (avant les exclusions)

# draft
exclusions_legacy_folders_description = Les dossiers privés de la version 0.3.1, lus comme des exclusions tenues à l’écart des agents

# draft
exclusions_legacy_marker = Dossiers marqués privés (avant les exclusions)

# draft
exclusions_legacy_marker_description = L’interrupteur .private de la version 0.3.1, lu comme l’exclusion des dossiers contenant un fichier .private

# draft
exclusions_names_nothing = pattern ou holding indique ce que l’exclusion désigne

# draft
exclusions_no_category = aucune catégorie { $category }

# draft
exclusions_no_field = aucun champ { $key }

# draft
exclusions_no_id = un identifiant est nécessaire

# draft
exclusions_not_switch = { $key } : true ou false

# draft
exclusions_path_starts = { $pattern } : un chemin commence par ~, par une variable ($CARGO_HOME), ou est entier ({ $whole })

# draft
exclusions_pattern_or_holding = un motif ou un fichier qu’il contient, pas les deux

# draft
exclusions_ref_any_depth = À toute profondeur en dessous

# draft
exclusions_ref_beside = Seulement à côté de

# draft
exclusions_ref_beside_note = Un nom ou un motif de nom, jamais un chemin.

# draft
exclusions_ref_by_name = Par nom, partout

# draft
exclusions_ref_by_path = Par chemin

# draft
exclusions_ref_cache_mark = La marque propre d’un cache (signée)

# draft
exclusions_ref_either = L’un ou l’autre

# draft
exclusions_ref_file_held = Un fichier qu’il contient

# draft
exclusions_ref_from_home = Un seul, depuis votre dossier personnel

# draft
exclusions_ref_from_root = Un seul, depuis la racine

# draft
exclusions_ref_from_variable = Depuis une variable

# draft
exclusions_ref_holding = Contenant

# draft
exclusions_ref_holding_named = Tout dossier contenant un fichier de ce nom

# draft
exclusions_ref_holding_note = Le nom d’un fichier : le dossier qui le contient est exclu, où qu’il soit.

# draft
exclusions_ref_like_beside = Seulement à côté d’un nom de cette forme

# draft
exclusions_ref_name_beside = Un nom à côté

# draft
exclusions_ref_named_like = Tout nom de cette forme

# draft
exclusions_ref_one = Un caractère

# draft
exclusions_ref_one_of = L’un de ceux-ci

# draft
exclusions_ref_path_like = Ceux dont le chemin a cette forme

# draft
exclusions_ref_path_note = Un chemin commence par ~, une variable ou une racine ({ $whole }) ; tout le reste est un nom.

# draft
exclusions_ref_pattern = Nom ou chemin

# draft
exclusions_ref_regex = Une expression régulière, ~ pour le dossier personnel

# draft
exclusions_ref_regex_note = Après regex:, sur le chemin écrit avec ~ pour votre dossier personnel.

# draft
exclusions_ref_signs = Signes d’un nom ou d’un chemin

# draft
exclusions_ref_so_named = Tout ce qui porte ce nom

# draft
exclusions_ref_so_named_beside = Seulement là où un fichier ou dossier de ce nom est à côté

# draft
exclusions_ref_star = N’importe quels caractères, dans un seul nom

# draft
exclusions_ref_stars = N’importe quels dossiers, dans un chemin

# draft
exclusions_ref_starts_so = Tout nom qui commence ainsi

# draft
exclusions_ref_variable_unset = Depuis une variable, ou ce qui la remplace si elle n’est pas définie

# draft
exclusions_ref_whole_path = Sur tout le chemin

# draft
exclusions_refused = { $at } : { $why }

# draft
exclusions_said_beside = { $what } à côté d’un { $sibling }

# draft
exclusions_said_holding = tout dossier contenant un fichier { $file }

# draft
exclusions_said_like = { $names ->
        [files] tout fichier
        [folders] tout dossier
       *[both] tout fichier ou dossier
    } dont le nom est de la forme { $pattern }

# draft
exclusions_said_like_beside = { $names ->
        [files] tout fichier
        [folders] tout dossier
       *[both] tout fichier ou dossier
    } dont le nom est de la forme { $pattern } à côté d’un { $sibling }

# draft
exclusions_said_matching = { $names ->
        [files] les fichiers
        [folders] les dossiers
       *[both] les fichiers et dossiers
    } correspondant à { $pattern }

# draft
exclusions_said_name = { $names ->
        [files] tout fichier
        [folders] tout dossier
       *[both] tout fichier ou dossier
    } nommé { $pattern }

# draft
exclusions_said_name_beside = { $names ->
        [files] tout fichier
        [folders] tout dossier
       *[both] tout fichier ou dossier
    } nommé { $pattern } à côté d’un { $sibling }

# draft
exclusions_said_now = { $pattern }, actuellement { $path }

# draft
exclusions_said_path_like = { $names ->
        [files] les fichiers
        [folders] les dossiers
       *[both] les fichiers et dossiers
    } de la forme { $pattern }

# draft
exclusions_said_unset = { $pattern }, tant que { $variables } n’est pas définie : aucun

# draft
exclusions_shell_variables = Lire les variables de votre shell

# draft
exclusions_shell_variables_description = Lire aussi depuis votre shell de connexion des variables telles que CARGO_HOME, là où un chemin commence par l’une d’elles.

# draft
exclusions_shell_variables_description_more = Un Skwir lancé depuis votre bureau ne voit pas ce qu’exportent .zshrc ou config.fish. Le shell est interrogé en arrière-plan dès que ses fichiers changent, et seules les variables que nomment ces chemins sont gardées.

# draft
exclusions_unreadable = { $count ->
        [1] Une exclusion ne peut pas être lue, elle ne s’applique donc pas : { $first }
        [one] { $count } exclusion ne peut pas être lue, elle ne s’applique donc pas. La première : { $first }
       *[other] { $count } exclusions ne peuvent pas être lues, elles ne s’appliquent donc pas. La première : { $first }
    }

# draft
exclusions_what_also = { $what } (un autre de sa liste)

# draft
exclusions_what_before = { $what } (là où il était avant d’être déplacé)

# draft
exclusions_what_cache_home = Votre dossier de cache (XDG_CACHE_HOME)

# draft
exclusions_what_legacy = Privé (de la liste de la version 0.3.1)

# draft
exec_being_redone = « { $title } » est déjà en cours de rétablissement

# draft
exec_being_undone = « { $title } » est déjà en cours d’annulation

# draft
exec_cannot_be_undone = « { $title } » ne peut pas être annulé

# draft
exec_chain_cancelled = « { $title } » a été abandonné

# draft
exec_chain_did_not_run = « { $title } » ne s’est pas exécuté

# draft
exec_chain_stopped = « { $title } » s’est arrêté : { $error }

# draft
exec_gone_from_history_redo = cette action n’est plus dans l’historique : rien à rétablir

# draft
exec_gone_from_history_undo = cette action n’est plus dans l’historique : rien à annuler

# draft
exec_not_undone = « { $title } » n’a pas été annulé : { $why }

# draft
exec_not_undone_nothing = « { $title } » n’est pas annulé : rien à rétablir

# draft
exec_nothing_to_redo = rien à rétablir

# draft
exec_nothing_to_undo = rien à annuler

# draft
exec_redo = rétablir

# draft
exec_redo_held = « { $title } » ne peut pas être rétabli seul : « { $other } », annulé après, a touché à ce qu’il a fait ; rétablissez-le d’abord

# draft
exec_redo_held_for_good = « { $title } » ne peut pas être rétabli : « { $other } », annulé après, a touché à ce qu’il a fait et ne peut pas être annulé

# draft
exec_that_not_undone = cette action n’a pas été annulée : { $why }

# draft
exec_undo = annuler

# draft
exec_undo_held = « { $title } » ne peut pas être annulé seul : « { $other } », demandé après, a touché à ce qu’il a fait ; annulez-le d’abord

# draft
exec_undo_held_for_good = « { $title } » ne peut pas être annulé : « { $other } », demandé après, a touché à ce qu’il a fait et ne peut pas être annulé

# draft
exec_undo_held_running = « { $title } » ne peut pas être annulé seul : « { $other } », demandé après et en cours, modifie ce qu’il a fait ; annulez-le d’abord

# draft
exec_undo_held_waiting = « { $title } » ne peut pas être annulé seul : « { $other } », demandé après et en attente, modifiera ce qu’il a fait ; annulez-le d’abord

# draft
exec_undone_already = « { $title } » est déjà annulé

# draft
folders_not_private = { $path } n’appartient pas qu’à vous

# draft
form_needed = { $field } est obligatoire

# draft
form_one_of_choices = { $field } doit prendre l’une des valeurs proposées

# draft
form_whole_number = { $field } est un nombre entier de { $min } à { $max }

# draft
fs_folder_gone = Ce dossier a été supprimé ou n’est plus disponible.

# draft
fs_not_a_path = pas un chemin : { $location }

# draft
fs_not_regular = pas un fichier ordinaire

# draft
fs_server_no_xattrs = ce serveur ne garde pas d’attributs étendus

# draft
fs_size_bytes = { $n } o

# draft
fs_size_gb = { $n } Go

# draft
fs_size_gib = { $n } Gio

# draft
fs_size_kb = { $n } ko

# draft
fs_size_kib = { $n } Kio

# draft
fs_size_mb = { $n } Mo

# draft
fs_size_mib = { $n } Mio

# draft
fs_size_pb = { $n } Po

# draft
fs_size_pib = { $n } Pio

# draft
fs_size_tb = { $n } To

# draft
fs_size_tib = { $n } Tio

# draft
fs_sync_not_yet = L’écriture jusqu’au disque n’est pas encore disponible sous { $system }

# draft
fs_system_no_hidden = { $system } ne connaît pas l’attribut caché

# draft
fs_system_no_xattrs = { $system } ne garde pas d’attributs étendus

# draft
fs_taken = il est en cours de déplacement ou de suppression

# draft
fs_too_large = fichier trop volumineux

# draft
fs_volume_no_hidden = son volume ne connaît pas l’attribut caché

# draft
inspection_mesh_colours = le maillage a { $colours ->
        [one] { $colours } couleur
       *[other] { $colours } couleurs
    } pour { $positions ->
        [one] { $positions } position
       *[other] { $positions } positions
    }

# draft
inspection_mesh_empty = le maillage n’a aucun triangle

# draft
inspection_mesh_index = l’indice { $index } du maillage ne désigne aucune position (il en a { $count })

# draft
inspection_mesh_infinite = le maillage a une position qui n’est pas finie

# draft
inspection_mesh_partial = les indices du maillage ne forment pas des triangles entiers

# draft
inspection_notices = { $before } · { $after }

# draft
io_bad_name = le nom est trop long, ou n’est pas permis à cet endroit

# draft
io_busy = il est en cours d’utilisation

# draft
io_denied = accès refusé

# draft
io_full = le disque est plein

# draft
io_gone = il n’est plus là

# draft
io_is_folder = c’est un dossier

# draft
io_not_empty = le dossier n’est pas vide

# draft
io_not_folder = une partie de son chemin n’est pas un dossier

# draft
io_quota = le quota de disque est épuisé

# draft
io_read_only = le disque est en lecture seule

# draft
io_share_gone = le partage réseau a disparu

# draft
io_taken = un élément de ce nom est déjà là

# draft
io_timed_out = le disque n’a pas répondu à temps

# draft
io_too_large = le fichier est trop gros pour ce disque

# draft
keymap_add_shortcut = Ajouter un raccourci

# draft
keymap_args = Arguments

# draft
keymap_args_description = Les arguments de la commande, sous forme d’objet JSON

# draft
keymap_args_not_json = les arguments de { $keys } ne sont pas du JSON : { $error }

# draft
keymap_args_object = les arguments de { $keys } sont un objet : { $example }

# draft
keymap_command = Commande

# draft
keymap_command_description = Ce que lancent les touches, par le nom sous lequel les commandes de la palette sont enregistrées : mkdir, ui.jump, tab.new. Vide, les touches sont libérées

# draft
keymap_keys = Touches

# draft
keymap_keys_description = Une touche avec ses modificateurs, ou plusieurs l’une après l’autre : ctrl-shift-n, g h, shift-g

# draft
keymap_needs_keys = un raccourci a besoin de ses touches

# draft
keymap_no_key = « { $stroke } » ne désigne aucune touche : écrivez ctrl-- pour Ctrl et la touche moins

# draft
keymap_no_modifier = « { $modifier } » dans « { $stroke } » n’est pas un modificateur : utilisez ctrl, alt, shift ou super

# draft
keymap_refused = { $keys } : { $error }

# draft
keymap_shortcuts = Vos raccourcis

# draft
keymap_shortcuts_description = Les touches que vous associez, par-dessus celles que définissent Skwir et ses modules.

# draft
keymap_shortcuts_description_more = Laissez la commande vide pour libérer une touche.

# draft
keymap_unknown_command = { $keys } lance { $command }, qui n’est le nom d’aucune commande (son module est-il activé ?)

# draft
keymap_where = Où

# draft
keymap_where_description = Où les touches agissent : vide pour partout, pane pour la liste des fichiers, avec des conditions telles que selection, single, dir, file, folder

# draft
notes_agree = mettre en accord les annotations de { $name }

# draft
notes_agree_command = Mettre les annotations en accord

# draft
notes_agree_description = Écrire les annotations des éléments donnés, ou celles de la sélection, telles que lues dans chaque stockage, dans chaque stockage qui les conserve, pour que les stockages qui diffèrent s’accordent de nouveau (plan)

# draft
notes_agree_title = mettre les annotations en accord

# draft
notes_agreed = annotations de { $name } mises en accord

# draft
notes_agreed_already = les annotations de { $name } étaient déjà en accord

# draft
notes_and = { $first } et { $then }

# draft
notes_changed_since = les annotations de { $what } ont été modifiées ailleurs entre-temps

# draft
notes_comment_command = Commenter

# draft
notes_comment_description = Définir le commentaire des éléments donnés, ou celui de la sélection, ou le retirer avec un texte vide (plan)

# draft
notes_comment_title = définir le commentaire

# draft
notes_commented = commentaire de { $name } défini

# draft
notes_commented_already = { $name } avait déjà ce commentaire

# draft
notes_gone = { $name } n’est plus là

# draft
notes_kept_nowhere = { $path } : son volume ne conserve pas d’annotations, et aucun stockage n’est réglé pour les garder à côté

# draft
notes_no_folder = { $path } n’est dans aucun dossier

# draft
notes_no_stores = aucun stockage d’annotations où écrire

# draft
notes_put_back = remettre les annotations de { $name }

# draft
notes_put_back_already = les annotations de { $name } étaient déjà remises

# draft
notes_put_back_done = annotations de { $name } remises

# draft
notes_put_back_failed = remettre les annotations de { $path }

# draft
notes_rate = donner { $stars } à { $name }

# draft
notes_rate_command = Noter

# draft
notes_rate_description = Noter les éléments donnés, ou la sélection, de 1 à 5 étoiles, ou retirer leur note avec 0 (plan)

# draft
notes_rate_none_title = retirer la note

# draft
notes_rate_title = donner { $stars }

# draft
notes_rated = { $name } noté { $stars }

# draft
notes_rated_already = { $name } avait déjà { $stars }

# draft
notes_rating_range = une note va de 1 à { $max }

# draft
notes_rating_stars = une note va de 1 à 5 étoiles

# draft
notes_refused = { $path } : { $why }

# draft
notes_rename_tag = renommer l’étiquette { $tag } en { $to } sur { $name }

# draft
notes_renamed_tag = étiquette { $tag } renommée en { $to } sur { $name }

# draft
notes_renamed_tag_already = l’étiquette { $tag } était déjà renommée en { $to } sur { $name }

# draft
notes_set_comment = définir le commentaire de { $name }

# draft
notes_stars = { $whole ->
        [one] { $whole } étoile
       *[other] { $whole } étoiles
    }

# draft
notes_stars_half = { $whole ->
        [0] ½ étoile
        [one] { $whole }½ étoile
       *[other] { $whole }½ étoiles
    }

# draft
notes_tag = étiqueter { $name } avec { $tags }

# draft
notes_tagged = { $name } étiqueté avec { $tags }

# draft
notes_tagged_already = { $name } était déjà étiqueté avec { $tags }

# draft
notes_take_tag = retirer l’étiquette { $tag } de { $name }

# draft
notes_take_tags = { $count ->
        [1] retirer l’étiquette { $tags } de { $name }
        [one] retirer les étiquettes { $tags } de { $name }
       *[other] retirer les étiquettes { $tags } de { $name }
    }

# draft
notes_took_tag = étiquette { $tag } retirée de { $name }

# draft
notes_took_tag_already = l’étiquette { $tag } était déjà absente de { $name }

# draft
notes_took_tags = { $count ->
        [1] étiquette { $tags } retirée de { $name }
        [one] étiquettes { $tags } retirées de { $name }
       *[other] étiquettes { $tags } retirées de { $name }
    }

# draft
notes_took_tags_already = { $count ->
        [1] l’étiquette { $tags } était déjà absente de { $name }
        [one] les étiquettes { $tags } étaient déjà absentes de { $name }
       *[other] les étiquettes { $tags } étaient déjà absentes de { $name }
    }

# draft
notes_uncomment = retirer le commentaire de { $name }

# draft
notes_uncomment_title = retirer le commentaire

# draft
notes_uncommented = commentaire de { $name } retiré

# draft
notes_uncommented_already = { $name } n’avait déjà aucun commentaire

# draft
notes_unrate = retirer la note de { $name }

# draft
notes_unrated = note de { $name } retirée

# draft
notes_unrated_already = { $name } n’avait déjà aucune note

# draft
notes_untag = retirer les étiquettes de { $name }

# draft
notes_untagged = étiquettes de { $name } retirées

# draft
notes_untagged_already = { $name } n’avait déjà aucune étiquette

# draft
notes_write_create = Créer

# draft
notes_write_failed = écrire les annotations de { $path }

# draft
notes_write_fallback = Là où les attributs manquent

# draft
notes_write_off = Désactivé

# draft
notes_write_update = Mettre à jour l’existant

# draft
op_all_in_trash = tout est dans la corbeille

# draft
op_already_there = { $name } est déjà là

# draft
op_attribute_name_not_utf8 = un attribut de { $name } a un nom qui n’est pas en UTF-8, que l’interface d’attributs de la copie ne peut pas conserver

# draft
op_attributes_not_kept = conserver les étiquettes et autres attributs lors de la copie : { $folder } ne conserve aucun attribut, et rien n’est réglé pour les conserver à côté ; désactivez « { $setting } » dans Paramètres pour copier sans eux

# draft
op_attributes_refused = conserver les étiquettes et autres attributs lors de la copie : vérifiez que la destination garde les attributs étendus et vous permet de les écrire, ou désactivez « { $setting } » dans Paramètres

# draft
op_attributes_unread = les étiquettes et attributs de { $name } n’ont pas pu être lus

# draft
op_batch_cancelled = annulé

# draft
op_batch_copying = copie de { $count ->
        [one] { $count } élément
       *[other] { $count } éléments
    } par { $transfer }

# draft
op_batch_link = « { $path } » est un lien

# draft
op_batch_read_short = { $read ->
        [one] « { $path } » : seul { $read } octet sur { $bytes } a pu être lu
       *[other] « { $path } » : seuls { $read } octets sur { $bytes } ont pu être lus
    }

# draft
op_change_copy = copier { $from } -> { $to }

# draft
op_change_create = créer { $path }

# draft
op_change_delete = supprimer { $path }

# draft
op_change_link = lier { $link } -> { $target }

# draft
op_change_metadata = définir { $field } pour { $path }

# draft
op_change_mkdir = mkdir { $path }

# draft
op_change_move = déplacer { $from } -> { $to }

# draft
op_change_remove = retirer { $path }

# draft
op_change_rename = renommer { $from } -> { $to }

# draft
op_change_restore = restaurer { $path }

# draft
op_change_swap = échanger { $a } <-> { $b }

# draft
op_change_trash = mettre à la corbeille { $path }

# draft
op_changed_after_copy = { $name } a changé après sa copie dans « { $folder } » : son original reste où il était

# draft
op_copied_into_itself = un dossier ne peut pas être copié dans lui-même

# draft
op_copied_onto_itself = il serait copié sur lui-même

# draft
op_copy_stopped = la copie s’est arrêtée

# draft
op_copy_title = { $count ->
        [1] copier { $name } dans { $folder }
        [one] copier { $count } élément dans { $folder }
       *[other] copier { $count } éléments dans { $folder }
    }

# draft
op_copy_went_missing = la copie { $name } a disparu aussitôt créée

# draft
op_device_not_copied = { $name } est un périphérique, qui ne peut pas être copié

# draft
op_entry_gone = { $name } n’est plus là

# draft
op_entry_replaced = { $name } est maintenant un autre fichier

# draft
op_folder_attribute_not_restored = l’attribut « { $attribute } » du dossier n’a pas pu être restauré

# draft
op_folder_attributes_differ = { $name } est là avec d’autres attributs de dossier

# draft
op_folder_not_made = le dossier { $name } n’a pas pu être créé

# draft
op_folder_not_removed = le dossier { $name } n’a pas pu être supprimé

# draft
op_folder_path_not_made = le dossier « { $path } » n’a pas pu être créé

# draft
op_foreign_folder = ce dossier appartient à un autre utilisateur, et ses permissions empêcheraient Annuler de restaurer son contenu

# draft
op_holds_mount_point = { $name } contient « { $point } », où un disque ou un dossier distant est monté : éjectez-le d’abord

# draft
op_holes_not_yet = Copier un fichier à trous (un fichier creux) n’est pas encore disponible sous { $system }

# draft
op_identity_unread = l’identité de l’élément n’a pas pu être lue

# draft
op_in_trash_already = { $name } est déjà dans la corbeille

# draft
op_in_trash_restore_first = { $name } est dans la corbeille : restaurez-le d’abord

# draft
op_left_after_removal = { $more ->
        [0] il reste « { $path } »
        [one] il reste « { $path } » et { $more } autre
       *[other] il reste « { $path } » et { $more } autres
    }

# draft
op_left_parked = { $name } est resté sous le nom { $parked }

# draft
op_link_elsewhere = { $name } mène ailleurs désormais

# draft
op_link_title = { $count ->
        [1] créer un lien vers { $name } dans { $folder }
        [one] créer des liens vers { $count } élément dans { $folder }
       *[other] créer des liens vers { $count } éléments dans { $folder }
    }

# draft
op_made_beside_other = { $companion } est à côté de { $name } et n’a pas été créé avec lui : Annuler les laisse en place

# draft
op_made_changed = { $name } a changé depuis sa création : Annuler le laisse en place

# draft
op_made_companion_changed = { $companion } a changé depuis sa création : Annuler le laisse en place, ainsi que { $name }

# draft
op_mount_point = { $name } est l’endroit où un disque ou un dossier distant est monté : éjectez-le plutôt que de le supprimer

# draft
op_move_original_changed = { $name } est resté où il était, modifié depuis le déplacement : Annuler le laisse en place, ainsi que sa copie

# draft
op_move_title = { $count ->
        [1] déplacer { $name } dans { $folder }
        [one] déplacer { $count } élément dans { $folder }
       *[other] déplacer { $count } éléments dans { $folder }
    }

# draft
op_moved_into_itself = un dossier ne peut pas être déplacé dans lui-même

# draft
op_moved_stays = ce qui a été déplacé reste où il était

# draft
op_moves_nothing = cet appareil (un appareil photo) ne peut pas déplacer de fichiers entre ses dossiers : seules des copies peuvent y être faites

# draft
op_name_character = un nom ne peut pas contenir « { $character } »

# draft
op_name_control = un nom ne peut pas contenir de caractère de contrôle

# draft
op_name_device = { $name } est un nom de périphérique sous Windows

# draft
op_name_dots = { $name } ne peut pas servir de nom

# draft
op_name_empty = un nom ne peut pas être vide

# draft
op_name_end = un nom ne peut pas se terminer par un point ou un espace

# draft
op_name_nul = un nom ne peut pas contenir de caractère NUL

# draft
op_name_slash = un nom ne peut pas contenir « / »

# draft
op_no_folder_to_go_in = le dossier de destination n’est pas là

# draft
op_no_longer_a_file = { $name } n’est plus un fichier

# draft
op_no_longer_a_link = { $name } n’est plus un lien

# draft
op_no_longer_empty = { $name } n’est plus vide

# draft
op_no_longer_in_trash = { $name } n’est plus dans la corbeille

# draft
op_no_longer_there = { $name } n’est plus là

# draft
op_no_record = { $name } n’est pas la fiche de corbeille d’un élément

# draft
op_no_room = Pas assez de place dans { $folder } : { $needs } nécessaires, { $free } libres

# draft
op_no_trash_s = { $name } ne désigne aucun élément d’une corbeille

# draft
op_no_trash_there = { $name } est dans un dossier réseau monté par un programme, qui n’a pas de corbeille

# draft
op_not_a_file = { $name } n’est pas un fichier

# draft
op_not_a_folder = { $name } n’est pas un dossier

# draft
op_not_all_deleted = { $name } n’a pas pu être entièrement supprimé

# draft
op_not_all_removed = { $name } n’a pas pu être entièrement supprimé

# draft
op_not_copied = { $name } n’a pas pu être copié

# draft
op_not_empty = { $name } n’est pas vide

# draft
op_not_in_trash = { $name } n’est pas dans la corbeille

# draft
op_not_linked = le lien vers { $name } n’a pas pu être créé dans { $folder }

# draft
op_not_made = { $name } n’a pas pu être créé

# draft
op_not_moved = { $name } n’a pas pu être déplacé dans { $folder }

# draft
op_not_moved_as = { $name } n’a pas pu être déplacé dans { $folder } sous le nom { $new }

# draft
op_not_placed = { $name } n’a pas pu être mis en place dans { $folder }

# draft
op_not_removed = { $name } n’a pas pu être supprimé

# draft
op_not_renamed = { $name } n’a pas pu être renommé en { $new }

# draft
op_not_restored = { $name } n’a pas pu être restauré

# draft
op_not_served = le transfert ne prend pas en charge ces dossiers

# draft
op_not_swapped = { $a } et { $b } n’ont pas pu échanger leurs noms

# draft
op_not_trashed = { $name } n’a pas pu être mis à la corbeille

# draft
op_not_written_out = la copie n’a pas pu être écrite jusqu’au bout sur le disque

# draft
op_original_stays = { $name } a été copié en entier dans « { $folder } », mais son original n’a pas pu être entièrement supprimé

# draft
op_original_stays_path = « { $path } » a été copié en entier dans « { $folder } », mais son original n’a pas pu être entièrement supprimé

# draft
op_panicked = échec inattendu

# draft
op_part_left = une partie de la copie reste dans « { $path } » ({ $why })

# draft
op_partly_restored_folder = le dossier restauré en partie reste dans « { $path } »

# draft
op_path_climbs = « { $path } » contient « .. »

# draft
op_path_missing = { $path } n’existe pas

# draft
op_path_no_name = « { $path } » ne se termine pas par un nom

# draft
op_pipe_not_made = le tube nommé { $name } n’a pas pu être créé

# draft
op_place_taken_in_trash = un autre élément { $name } a pris sa place dans la corbeille

# draft
op_plan_steps_kind = un plan liste chacune de ses étapes : `{ $kind }` n’en est pas une

# draft
op_read_short = { $read ->
        [one] seul { $read } octet sur { $bytes } a pu être lu
       *[other] seuls { $read } octets sur { $bytes } ont pu être lus
    }

# draft
op_record_unread = la fiche de corbeille { $name } n’a pas pu être lue

# draft
op_record_unwritten = la fiche de corbeille { $name } n’a pas pu être écrite

# draft
op_redo_title = rétablir : { $title }

# draft
op_refused_on_close = son disque ou son serveur l’a refusé à sa fermeture

# draft
op_rename_taken = { $name } est déjà là

# draft
op_renames_duplicate = plusieurs éléments seraient nommés { $path }

# draft
op_socket_not_copied = { $name } est une socket, qui ne peut pas être copiée

# draft
op_spellings_undistinguished = les graphies des liens physiques de ce fichier n’ont pas pu être distinguées

# draft
op_stopped_being_read = la lecture du fichier s’est arrêtée

# draft
op_swap_windows = Windows ne peut pas échanger deux noms en une seule étape

# draft
op_there_again = { $name } est de nouveau là : renommez-le ou déplacez-le d’abord

# draft
op_there_by_earlier_step = { $name } est déjà là, placé par une étape précédente

# draft
op_there_not_a_folder = { $name } est là, et ce n’est pas un dossier

# draft
op_time_not_given = « { $path } » n’a pas pu recevoir sa date de modification

# draft
op_trash_copy_left = une copie en reste dans « { $path } » ({ $why })

# draft
op_trash_copy_not_removed = la copie de { $name } dans la corbeille n’a pas pu être entièrement supprimée

# draft
op_trash_folder_gone = le dossier de la corbeille où se trouvait { $name } n’existe plus

# draft
op_trash_unread = la corbeille n’a pas pu être lue : { $error }

# draft
op_trash_unread_context = la corbeille n’a pas pu être lue

# draft
op_undo_title = annuler : { $title }

# draft
op_unknown_kind = type d’opération inconnu `{ $kind }`

# draft
op_unreadable = { $name } n’a pas pu être lu : { $error }

# draft
op_went_missing_restored = { $name } a disparu aussitôt restauré

# draft
op_would_stay_in_trash = { $name } resterait dans la corbeille

# draft
op_write_out_stopped = l’écriture sur le disque s’est arrêtée

# draft
orders_kept = Les ordres de tri que les emplacements retiennent (ADR-132), chacun { $fields } : appliqués ainsi dans chaque panneau

# draft
orders_not_saved = L’ordre de tri n’a pas été retenu : { $error }

# draft
os_path_not_absolute = { $path } n’est pas un chemin absolu

# draft
permissions_names_private = ceci nomme un dossier privé, que Skwir tient à l’écart des agents (Paramètres, Confidentialité)

# draft
permissions_network = utilise le réseau

# draft
permissions_programs = lance des programmes

# draft
permissions_reaches_machine = consulter { $path } ferait contacter une autre machine par le système, ce qu’un agent ne peut pas faire sans qu’on le lui demande : l’utilisateur l’ouvre d’abord dans Skwir

# draft
permissions_read = lit le contenu des fichiers

# draft
permissions_write = modifie des fichiers

# draft
playing_not_answering = le lecteur ne répond pas

# draft
playing_nothing = rien n’est en cours de lecture

# draft
playing_now = Lecture en cours

# draft
playing_now_description = La piste en cours de lecture, son état et sa position, ou null

# draft
playing_request = Piloter le lecteur

# draft
playing_request_description = Demander à ce qui est en cours de lecture de lire, mettre en pause, arrêter, passer à la piste suivante ou précédente, se déplacer dans la piste, changer son volume ou sa vitesse, choisir l’audio ou les sous-titres d’un film ou l’afficher en plein écran ; la demande est exécutée au prochain réveil du lecteur

# draft
playing_senseless = cette demande n’a pas de sens

# draft
plugins_browsing = Navigation et recherche

# draft
plugins_desktop = Intégration au bureau

# draft
plugins_files = Travail sur les fichiers

# draft
plugins_list = Lister les modules

# draft
plugins_list_description = Chaque module avec son titre, sa description et son état d’activation

# draft
plugins_not_saved = Le choix du module n’a pas été enregistré : { $error }

# draft
plugins_previews = Aperçus et vignettes

# draft
plugins_set_enabled = Activer ou désactiver un module

# draft
plugins_set_enabled_description = Activer ou désactiver un module pendant l’exécution et retenir ce choix ; fenêtre ouverte, la fin est signalée par un réveil et par `plugins.list`

# draft
plugins_unknown = module inconnu : { $id }

# draft
preview_changed = { $path } a été modifié pendant son affichage

# draft
print_no_page = aucune page indiquée

# draft
print_not_a_page = « { $text } » n’est pas une page

# draft
print_pages_backwards = { $range } est à l’envers

# draft
print_pages_from_one = les pages sont comptées à partir de 1

# draft
print_paper_legal = Légal US

# draft
print_paper_letter = Lettre US

# draft
properties_content = Contenu

# draft
properties_general = Général

# draft
properties_location = Emplacement

# draft
properties_more = Plus

# draft
properties_notes = Annotations

# draft
properties_permissions = Droits d’accès

# draft
properties_selection = Sélection

# draft
properties_shared_permissions = Droits d’accès communs

# draft
properties_source = Source

# draft
properties_times = Dates

# draft
properties_volume = Volume

# draft
reference_any = N’importe quel caractère

# draft
reference_any_times = Ce qui précède, un nombre quelconque de fois, ou aucune

# draft
reference_boundary = Là où un mot commence ou finit

# draft
reference_case = À partir d’ici, majuscules et minuscules confondues

# draft
reference_digit = Un chiffre

# draft
reference_either = L’un ou l’autre

# draft
reference_end = La fin

# draft
reference_escaped = Le signe lui-même : \. \( \[ \* \+ \? \$ \^ \|

# draft
reference_group = Un groupe, qu’un signe placé après prend en entier : (ab)+

# draft
reference_lazy = Aussi peu de fois que possible (*? aussi)

# draft
reference_maybe = Ce qui précède, ou rien

# draft
reference_named_group = Un groupe qui a un nom : year

# draft
reference_none_of = Tout caractère sauf ceux-ci

# draft
reference_once_or_more = Ce qui précède, une fois ou plus

# draft
reference_one_of = L’un de ceux-ci

# draft
reference_pattern = Motif

# draft
reference_pattern_note = Ni assertions de voisinage ni références arrière : (?=…) et \1 ne sont pas lus.

# draft
reference_range = Un caractère de cette plage

# draft
reference_space = Tout caractère d’espacement : une espace, une tabulation, une fin de ligne, tout autre

# draft
reference_start = Le début

# draft
reference_times = Ce qui précède, de 2 à 4 fois

# draft
reference_word_char = Une lettre, un chiffre ou _

# draft
registry_compress = Compresser

# draft
registry_group_bookmarks = Signets

# draft
registry_group_devices = Périphériques

# draft
registry_group_network = Réseau

# draft
registry_group_places = Emplacements

# draft
registry_group_saved = Enregistrés

# draft
registry_group_tabs = Onglets

# draft
registry_group_tags = Étiquettes

# draft
registry_open_folder_with = Ouvrir le dossier avec

# draft
registry_open_with = Ouvrir avec

# draft
registry_tags = Étiquettes

# draft
settings_apart_own = { $file ->
        [state] l’état de la fenêtre, pas un paramètre : Skwir lit le sien, dans son fichier d’état, jamais celui-ci
       *[data] ce que vous créez dans Skwir (recherches enregistrées, étiquettes épinglées, couleurs des étiquettes), pas un paramètre : Skwir lit les siens, dans son fichier de données, jamais ceux-ci
    }

# draft
settings_apart_until = { $file ->
        [state] l’état de la fenêtre, pas un paramètre : lu ici seulement jusqu’à ce que Skwir garde le sien, dans son fichier d’état
       *[data] ce que vous créez dans Skwir (recherches enregistrées, étiquettes épinglées, couleurs des étiquettes), pas un paramètre : lu ici seulement jusqu’à ce que Skwir garde les siens, dans son fichier de données
    }

# draft
settings_apply = Enregistrer les paramètres

# draft
settings_apply_description = Modifier des préférences, chaque clé avec le texte que prend sa ligne dans Paramètres ; toutes sont enregistrées, ou aucune

# draft
settings_check = Vérifier les paramètres

# draft
settings_check_description = Vérifier un fichier de paramètres, ou des paramètres donnés sous forme de table, au regard des paramètres que cette version de Skwir déclare : échoue en disant ce qui ne va pas (une valeur qu’un paramètre n’accepte pas, une clé qu’aucun paramètre n’a), et signale les clés dont il ne connaît pas le propriétaire

# draft
settings_check_invalid = « { $path } » n’est pas valide : { $problem }

# draft
settings_check_not_settings = ce ne sont pas des paramètres : { $error }

# draft
settings_check_one = donnez un fichier ou des paramètres, l’un des deux

# draft
settings_check_unreadable = « { $path } » ne peut pas être lu : { $why }

# draft
settings_choice_now = { $choice } ({ $now })

# draft
settings_choose_one = choisissez une valeur parmi { $choices }

# draft
settings_close_split = Panneau qui se ferme

# draft
settings_close_split_active = Actif

# draft
settings_close_split_description = Le panneau qui disparaît quand vous fermez la vue divisée avec Diviser la vue ou F3.

# draft
settings_close_split_description_more = Autre garde le panneau où vous êtes. Actif le ferme et garde l’autre. Droit ferme toujours le panneau de droite, ou celui du bas quand les panneaux sont empilés.

# draft
settings_close_split_other = Autre

# draft
settings_close_split_right = Droit

# draft
settings_drop_asks = Demander que faire des fichiers déposés

# draft
settings_drop_asks_description = Des fichiers déposés dans un dossier sans touche enfoncée ouvrent un menu : les y déplacer, les y copier ou y créer un lien vers eux.

# draft
settings_drop_asks_description_more = Désactivez cette option pour les déplacer au sein d’un même disque et les copier vers un autre. Maj déplace, Ctrl copie et Ctrl+Maj crée un lien, dans les deux cas.

# draft
settings_expected_boolean = true ou false attendu

# draft
settings_expected_finite = nombre fini attendu

# draft
settings_expected_json = JSON attendu : { $error }

# draft
settings_expected_kind = on attendait { $want ->
        [integer] un nombre entier
        [float] un nombre
        [boolean] true ou false
        [datetime] une date
        [array] une liste
        [table] une table
       *[string] du texte
    }, pas { $got ->
        [integer] un nombre entier
        [float] un nombre
        [boolean] true ou false
        [datetime] une date
        [array] une liste
        [table] une table
       *[string] du texte
    }

# draft
settings_expected_number = nombre attendu

# draft
settings_expected_records = liste d’enregistrements attendue : { $error }

# draft
settings_expected_whole_number = nombre entier attendu

# draft
settings_field_name_value_pairs = { $title } est une liste de paires nom et valeur

# draft
settings_field_needed = { $title } est obligatoire

# draft
settings_field_pairs = { $title } est une liste de paires

# draft
settings_field_pairs_text = { $title } : les noms et les valeurs sont du texte

# draft
settings_field_range = { $title } : saisissez un nombre entier de { $min } à { $max }

# draft
settings_field_secret = { $title } est conservé dans le trousseau, pas dans les paramètres

# draft
settings_field_text = { $title } est un texte

# draft
settings_field_value_needs_name = { $title } : une valeur doit avoir un nom

# draft
settings_field_whole_number = { $title } est un nombre entier

# draft
settings_file_invalid = « { $path } » n’est pas valide ({ $problem }) : rien n’est enregistré par-dessus tant qu’il n’est pas corrigé

# draft
settings_file_not_text = « { $path } » n’est pas du texte

# draft
settings_file_unreadable = « { $path } » n’a pas pu être lu : { $why }

# draft
settings_folders_first = Dossiers en premier

# draft
settings_folders_first_description = Lister les dossiers avant les fichiers, chaque groupe trié de son côté.

# draft
settings_folders_first_description_more = Désactivé, tout est trié ensemble : par date de modification, le plus récent vient en premier, dossier ou fichier. Par taille, les dossiers, qui ne sont pas pesés, sont triés comme s’ils n’avaient pas de taille.

# draft
settings_invalid_key = clé de paramètre non valide

# draft
settings_keep_attributes = Garder les étiquettes et les attributs lors de la copie

# draft
settings_keep_attributes_description = Les copies gardent les étiquettes, la note et le commentaire de leur source, et l’adresse d’où elle a été téléchargée.

# draft
settings_keep_attributes_description_more = Là où la destination ne garde pas d’attributs, ils sont gardés à côté de la copie, comme le fait un Mac ; la copie n’échoue que si rien ne peut les garder. Désactivez cette option pour copier sans eux.

# draft
settings_key_not_json = { $key } : JSON attendu : { $error }

# draft
settings_language = Langue

# draft
settings_language_automatic = Automatique

# draft
settings_language_description = La langue que parle la fenêtre de Skwir.

# draft
settings_language_description_more = Automatique parle la première des langues de votre système que Skwir connaît, sinon l’anglais ; les agents et les scripts reçoivent leurs réponses en anglais quel que soit votre choix.

# draft
settings_language_pseudo = Pseudo (pour les tests)

# draft
settings_language_self = Français

# draft
settings_last_location = Le dossier affiché à la dernière fermeture de la fenêtre

# draft
settings_managed = { $key } est géré par les commandes dont il relève

# draft
settings_no_plugin = aucun module « { $id } » ici : un module externe, ou un module que cette version n’intègre pas, peut s’en servir

# draft
settings_no_such = ce paramètre n’existe pas

# draft
settings_not_file_value = pas une valeur qu’un fichier peut contenir : { $error }

# draft
settings_not_text = ce n’est pas du texte

# draft
settings_other_choice = pas un choix que Skwir connaît ici : il peut figurer parmi { $which }

# draft
settings_owner_unknown = rien ici ne déclare « { $owner } » : un module désactivé, ou externe, peut le faire

# draft
settings_parent_not_table = { $key } : le parent n’est pas une table

# draft
settings_plugin_table = table attendue : enabled = true ou false

# draft
settings_problems = { $first } { $then }

# draft
settings_program_alone = Indiquez le programme seul, sans arguments : « bash », pas « bash -l »

# draft
settings_range = saisissez un nombre entier de { $min } à { $max }

# draft
settings_read_only = le { $file ->
        [state] fichier d’état
        [data] fichier de données
       *[settings] fichier de paramètres
    } est en lecture seule : la modification n’est pas conservée

# draft
settings_read_only_link = le fichier de paramètres est un lien vers un fichier qui ne peut pas être modifié (celui de Home Manager, par exemple) : la modification n’est pas conservée ; modifiez-le là où il est déclaré

# draft
settings_read_only_link_apart = le { $file ->
        [state] fichier d’état
       *[data] fichier de données
    } est un lien vers un fichier qui ne peut pas être modifié : la modification n’est pas conservée

# draft
settings_record_needs_id = chaque enregistrement doit avoir un identifiant

# draft
settings_records_share_id = deux enregistrements ont le même identifiant { $id }

# draft
settings_renamed = désormais « { $new } » : Skwir l’y déplace

# draft
settings_renamed_both = non lu : « { $new } » est défini, et c’est lui que Skwir lit

# draft
settings_save_did_not_run = l’enregistrement des paramètres n’a pas eu lieu

# draft
settings_saving_already = les paramètres sont déjà en cours d’enregistrement

# draft
settings_section_actions = Actions

# draft
settings_section_address = Adresse

# draft
settings_section_audio = Audio

# draft
settings_section_books = Livres

# draft
settings_section_code = Code

# draft
settings_section_commands = Commandes

# draft
settings_section_documents = Documents

# draft
settings_section_drag_and_drop = Glisser-déposer

# draft
settings_section_git = Git

# draft
settings_section_grammars = Grammaires

# draft
settings_section_grid = Grille

# draft
settings_section_language = Langue

# draft
settings_section_list_columns = Colonnes de la liste

# draft
settings_section_maps = Cartes

# draft
settings_section_models_3d = Modèles 3D

# draft
settings_section_opening_addresses = Ouverture des adresses

# draft
settings_section_pages = Pages

# draft
settings_section_pictures = Images

# draft
settings_section_playback = Lecture des médias

# draft
settings_section_previews = Aperçus

# draft
settings_section_servers = Serveurs

# draft
settings_section_sizes = Tailles

# draft
settings_section_sorting = Tri

# draft
settings_section_split = Vue divisée

# draft
settings_section_spreadsheets = Classeurs

# draft
settings_section_startup = Démarrage

# draft
settings_section_tabs = Onglets

# draft
settings_section_tags = Étiquettes

# draft
settings_section_terminal = Terminal

# draft
settings_section_text = Texte

# draft
settings_section_thumbnails = Vignettes

# draft
settings_section_video = Vidéo

# draft
settings_show_companions = Afficher les fichiers annexes

# draft
settings_show_companions_description = Quand les fichiers cachés sont affichés, lister aussi les fichiers qui gardent à côté d’un autre fichier ses annotations : les fichiers ._ d’un Mac, les fichiers annexes XMP et TagSpaces.

# draft
settings_show_companions_description_more = Propriétés liste les fichiers annexes d’un fichier dans tous les cas.

# draft
settings_size_units = Unités

# draft
settings_size_units_binary = Binaire

# draft
settings_size_units_decimal = Décimal

# draft
settings_size_units_description = Comment les tailles sont comptées.

# draft
settings_size_units_description_more = Décimal compte un kilooctet comme 1 000 octets (ko, Mo, Go), comme les disques sont vendus et comme macOS et GNOME affichent les tailles ; Binaire compte 1 024 octets (Kio, Mio, Gio), comme KDE. Une recherche lit toujours 10M en base 1 024, 10MB en base 1 000 et 10MiB en base 1 024.

# draft
settings_startup = Ouvrir au démarrage

# draft
settings_startup_description = Ce que Skwir ouvre quand il démarre sans dossier donné sur la ligne de commande.

# draft
settings_startup_folder = Dossier de démarrage

# draft
settings_startup_folder_choice = Un dossier choisi

# draft
settings_startup_folder_description = Utilisé quand un dossier choisi s’ouvre au démarrage.

# draft
settings_startup_folder_description_more = Laissez vide pour votre dossier personnel.

# draft
settings_startup_last = Les fenêtres et les onglets laissés ouverts

# draft
settings_toml_on_line = { $problem } à la ligne { $line }

# draft
settings_unit_megabytes = { $n } Mo

# draft
settings_unit_percent = { $n } %

# draft
settings_unit_seconds = { $n } s

# draft
settings_unknown = paramètre inconnu : { $key }

# draft
settings_unreadable_kept = { $file ->
        [data] Vos recherches enregistrées, vos étiquettes épinglées et les couleurs des étiquettes n’ont pas pu être lues ({ $why }) : Skwir a démarré sans elles et n’y enregistre rien tant que « { $path } » n’est pas corrigé ou supprimé ({ $error }).
       *[settings] Vos paramètres n’ont pas pu être lus ({ $why }) : Skwir a démarré avec ses valeurs par défaut et n’y enregistre rien tant que « { $path } » n’est pas corrigé ou supprimé ({ $error }).
    }

# draft
settings_unreadable_set_aside = { $file ->
        [data] Vos recherches enregistrées, vos étiquettes épinglées et les couleurs des étiquettes n’ont pas pu être lues ({ $why }) : Skwir a démarré sans elles. Le fichier a été mis de côté sous le nom { $aside }.
       *[settings] Vos paramètres n’ont pas pu être lus ({ $why }) : Skwir a démarré avec ses valeurs par défaut. Le fichier a été mis de côté sous le nom { $aside }.
    }

# draft
settings_unreadable_unsaved = { $file ->
        [data] Vos recherches enregistrées, vos étiquettes épinglées et les couleurs des étiquettes n’ont pas pu être lues ({ $why }) : Skwir a démarré sans elles et n’y enregistre rien cette fois-ci.
       *[settings] Vos paramètres n’ont pas pu être lus ({ $why }) : Skwir a démarré avec ses valeurs par défaut et n’y enregistre rien cette fois-ci.
    }

# draft
shrink_lossless = Sans perte

# draft
shrink_note_damaged = il est endommagé, et une copie allégée perdrait ce qu’un lecteur répare en l’affichant

# draft
shrink_note_date_not_kept = Sa date n’a pas pu être gardée, elle a donc été omise.

# draft
shrink_note_date_part_unread = Une partie de sa date n’a pas pu être lue, elle a donc été omise.

# draft
shrink_note_exif_unread = Ses données EXIF n’ont pas pu être lues, sa date a donc été omise.

# draft
shrink_note_pixels_as_stored = Ses pixels ont été gardés tels qu’enregistrés, redressés selon ses données EXIF comme avant.

# draft
shrink_note_pixels_kept = Ses pixels ont été gardés tels quels : réécrits, ils prenaient plus de place.

# draft
shrink_note_profile_left_out = Son profil de couleurs a été omis : il ne correspond pas aux couleurs dans lesquelles la copie est écrite, qui peuvent donc paraître différentes.

# draft
shrink_note_protected = il est protégé par un mot de passe, qu’une copie allégée ne garderait pas

# draft
shrink_note_put_back = ses images allégées différaient dans une copie et ont été remises telles quelles, ce qui ne l’a pas allégé

# draft
shrink_note_signed = il est signé, et une copie allégée invaliderait sa signature

# draft
shrink_note_signed_rights = il porte des droits signés par son auteur (pour le remplir et l’enregistrer dans Adobe Reader), qu’une copie allégée invaliderait

# draft
shrink_note_still_unlike = une copie faite différait encore de l’original une fois ses images remises, aucune n’a donc été gardée

# draft
shrink_note_unlike = une copie faite différait de l’original ailleurs que dans ses images, aucune n’a donc été gardée

# draft
shrink_print = Impression

# draft
shrink_screen = Écran

# draft
shrink_small = Petite taille

# draft
sort_a_to_z = De A à Z

# draft
sort_a_to_z_phrase = de A à Z

# draft
sort_changes_first = Modifications d’abord

# draft
sort_changes_first_phrase = modifications d’abord

# draft
sort_changes_last = Modifications en dernier

# draft
sort_changes_last_phrase = modifications en dernier

# draft
sort_highest = Mieux notés

# draft
sort_highest_first = les mieux notés d’abord

# draft
sort_largest = Plus grands

# draft
sort_largest_first = les plus grands d’abord

# draft
sort_longest = Plus longs

# draft
sort_longest_first = les plus longs d’abord

# draft
sort_lowest = Moins bien notés

# draft
sort_lowest_first = les moins bien notés d’abord

# draft
sort_newest = Plus récents

# draft
sort_newest_first = les plus récents d’abord

# draft
sort_oldest = Plus anciens

# draft
sort_oldest_first = les plus anciens d’abord

# draft
sort_phrase_accessed = date d’accès

# draft
sort_phrase_created = date de création

# draft
sort_phrase_deleted = date de suppression

# draft
sort_phrase_dimensions = dimensions

# draft
sort_phrase_duration = durée

# draft
sort_phrase_extension = extension

# draft
sort_phrase_folder = dossier

# draft
sort_phrase_kind = genre

# draft
sort_phrase_modified = date de modification

# draft
sort_phrase_name = nom

# draft
sort_phrase_owner = propriétaire

# draft
sort_phrase_rating = note

# draft
sort_phrase_size = taille

# draft
sort_phrase_tags = étiquettes

# draft
sort_phrase_taken = date de prise de vue

# draft
sort_phrase_type = type

# draft
sort_phrase_vcs = état Git

# draft
sort_shortest = Plus courts

# draft
sort_shortest_first = les plus courts d’abord

# draft
sort_smallest = Plus petits

# draft
sort_smallest_first = les plus petits d’abord

# draft
sort_title_accessed = Consulté le

# draft
sort_title_created = Créé le

# draft
sort_title_deleted = Supprimé le

# draft
sort_title_dimensions = Dimensions

# draft
sort_title_duration = Durée

# draft
sort_title_extension = Extension

# draft
sort_title_folder = Dossier

# draft
sort_title_kind = Genre

# draft
sort_title_modified = Modifié le

# draft
sort_title_name = Nom

# draft
sort_title_owner = Propriétaire

# draft
sort_title_rating = Note

# draft
sort_title_size = Taille

# draft
sort_title_tags = Étiquettes

# draft
sort_title_taken = Pris le

# draft
sort_title_type = Type

# draft
sort_title_vcs = Git

# draft
sort_z_to_a = De Z à A

# draft
sort_z_to_a_phrase = de Z à A

# draft
suggest_actions = Actions

# draft
suggest_complete = Compléter

# draft
suggest_go = Aller à

# draft
suggest_help = Ce que le champ accepte

# draft
suggest_history = Précédent

# draft
suggest_reading = Vouliez-vous dire

# draft
suggest_recent = Recherches récentes

# draft
suggest_saved = Recherches enregistrées

# draft
suggest_search_in = Rechercher dans

# draft
suggest_syntax = Termes d’une recherche

# draft
suggest_tabs = Onglets ouverts

# draft
suggest_terms = Ajouter un terme

# draft
sys_bad_folder_permissions = permissions de dossier non valides

# draft
sys_group_not_yet = La modification du groupe d’un fichier n’est pas encore disponible sous { $system }

# draft
sys_no_proc = le dossier /proc du système n’est pas monté, et la modification des droits d’accès en a besoin

# draft
sys_now_a_link = c’est maintenant un lien

# draft
sys_now_another_file = c’est maintenant un autre fichier

# draft
sys_path_not_utf8 = { $path } n’est pas un chemin UTF-8

# draft
sys_pipe_not_yet = La copie d’un tube nommé n’est pas encore disponible sous { $system }

# draft
sys_swap_windows = Windows ne peut pas échanger deux noms en une seule étape

# draft
sys_trash_list_not_yet = L’affichage de la corbeille n’est pas encore disponible sous { $system }

# draft
sys_trash_refused = mettre { $path } à la corbeille : { $error }

# draft
sys_trash_restore_not_yet = La restauration depuis la corbeille n’est pas encore disponible sous { $system }

# draft
sys_unix_permissions_not_yet = La restauration des permissions Unix d’un dossier n’est pas encore disponible sous { $system }

# draft
sys_windows_permissions_not_yet = La restauration des permissions Windows d’un dossier n’est pas encore disponible sous { $system }

# draft
sys_write_through_not_yet = L’écriture directe sur un disque n’est pas encore disponible sous { $system }

# draft
system_this = ce système

# draft
tabs_close = Fermer l’onglet

# draft
tabs_close_description = Fermer un onglet (celui affiché, par défaut) ; la notification propose Annuler. Le dernier onglet d’une fenêtre ferme la fenêtre tant qu’une autre est ouverte ; le dernier onglet de la dernière fenêtre reste (le Ctrl+W propre à sa fenêtre ferme la fenêtre)

# draft
tabs_close_window = Fermer la fenêtre

# draft
tabs_close_window_description = Fermer une fenêtre (la fenêtre active, par défaut) et ses onglets, que Rouvrir l’onglet fermé fait revenir ; la dernière fenêtre ne se ferme que par son propre bouton de fermeture ou par Quitter

# draft
tabs_closed = Onglet « { $title } » fermé

# draft
tabs_closed_undo = Annuler

# draft
tabs_go_to_family = Aller à un onglet

# draft
tabs_last = Dernier onglet

# draft
tabs_last_tabs = Les onglets affichés à la dernière fermeture de la fenêtre, avant que Skwir ait plusieurs fenêtres

# draft
tabs_last_window = La dernière fenêtre se ferme par son bouton de fermeture ou par Quitter

# draft
tabs_last_windows = Les dernières fenêtres affichées et leurs onglets

# draft
tabs_list = Lister les onglets

# draft
tabs_list_description = Les onglets dans l’ordre, fenêtre après fenêtre : la fenêtre de chacun, ses panneaux, leurs emplacements, le panneau actif et si sa fenêtre l’affiche

# draft
tabs_list_windows = Lister les fenêtres

# draft
tabs_list_windows_description = Les fenêtres : les onglets de chacune, l’onglet qu’elle affiche, et laquelle est active

# draft
tabs_move = Déplacer l’onglet

# draft
tabs_move_description = Mettre un onglet à une autre position, ou le déplacer de quelques pas ; ou le déplacer dans une autre fenêtre, ou dans une nouvelle

# draft
tabs_move_family = Déplacer l’onglet

# draft
tabs_move_left = Déplacer l’onglet à gauche

# draft
tabs_move_right = Déplacer l’onglet à droite

# draft
tabs_move_to_window = Déplacer l’onglet dans une nouvelle fenêtre

# draft
tabs_new = Nouvel onglet

# draft
tabs_new_description = Ouvrir un onglet affichant un emplacement, ou ce qu’affiche le panneau ; affiché aussitôt, sauf si `show` vaut false

# draft
tabs_new_window = Nouvelle fenêtre

# draft
tabs_new_window_description = Ouvrir une fenêtre affichant un emplacement, ou ce qu’affiche le panneau, de la même forme que la fenêtre du panneau ; elle devient la fenêtre active

# draft
tabs_next = Onglet suivant

# draft
tabs_no_pane = ce panneau n’existe pas

# draft
tabs_no_tab = cet onglet n’existe pas

# draft
tabs_no_window = cette fenêtre n’existe pas

# draft
tabs_nothing_for_tab = le panneau n’affiche rien à ouvrir dans un onglet

# draft
tabs_nothing_for_window = le panneau n’affiche rien à ouvrir dans une fenêtre

# draft
tabs_nth = Onglet { $number }

# draft
tabs_only_folders = Seuls les dossiers et les emplacements s’ouvrent dans un onglet ou une fenêtre

# draft
tabs_open = Ouvrir dans un nouvel onglet

# draft
tabs_open_description = Ouvrir chaque dossier sélectionné dans un onglet à lui, derrière l’onglet affiché ; les fichiers ne s’ouvrent pas dans des onglets

# draft
tabs_open_window = Ouvrir dans une nouvelle fenêtre

# draft
tabs_open_window_description = Ouvrir chaque dossier sélectionné dans une fenêtre à lui, de la même forme que celle du panneau ; les fichiers ne s’ouvrent pas dans des fenêtres

# draft
tabs_previous = Onglet précédent

# draft
tabs_reopen = Rouvrir l’onglet fermé

# draft
tabs_reopen_description = Rouvrir l’onglet ou la fenêtre fermés en dernier, ou celui qu’une fermeture a signalé, avec son historique

# draft
tabs_select = Afficher l’onglet

# draft
tabs_select_description = Afficher un onglet : par identifiant, ou parmi les onglets de la fenêtre active par position (négative depuis la droite) ou par pas depuis celui affiché

# draft
tabs_select_window = Aller à la fenêtre

# draft
tabs_select_window_description = Faire d’une fenêtre la fenêtre active, mise au premier plan

# draft
tabs_sleep = Libérer la mémoire des onglets en arrière-plan

# draft
tabs_sleep_description = Un onglet que vous ne regardez pas libère sa liste de fichiers et la relit quand il est affiché.

# draft
tabs_sleep_description_more = Désactivez ceci pour garder chaque onglet listé et à jour, ce qui demande plus de mémoire et de travail en arrière-plan.

# draft
tabs_which_window = quelle fenêtre : `window` est nécessaire

# draft
topic_files = Fichiers

# draft
topic_find = Recherche

# draft
topic_go = Se déplacer

# draft
topic_notes = Étiquettes et annotations

# draft
topic_places = Emplacements et serveurs

# draft
topic_player = Lecture

# draft
topic_preview = Aperçus

# draft
topic_select = Sélection

# draft
topic_skwir = Skwir

# draft
topic_tabs = Onglets et vue divisée

# draft
topic_terminal = Terminal

# draft
topic_view = Vues et panneaux

# draft
topic_window = La fenêtre

# draft
trash_op_mounted_remote = un dossier réseau monté par un programme n’a pas de corbeille : supprimez-le plutôt

# draft
trash_op_no_home = il n’y a pas de dossier personnel pour sa corbeille

# draft
trash_op_share_refused = le partage réseau n’a pas de corbeille que vous puissiez utiliser : supprimez-le plutôt

# draft
trash_op_top = la racine d’un système de fichiers ne peut pas être mise à la corbeille

# draft
vcs_changed = Modifié

# draft
vcs_conflict = En conflit

# draft
vcs_folder_ignored = Ignoré

# draft
vcs_holds_changes = Contient des modifications

# draft
vcs_holds_conflict = Contient un conflit

# draft
vcs_holds_new = Contient de nouveaux fichiers

# draft
vcs_holds_staged = Contient des modifications indexées

# draft
vcs_ignored = Ignoré

# draft
vcs_new = Nouveau

# draft
vcs_staged = Indexé

# draft
workspace_no_tab = cet onglet n’existe pas

# draft
workspace_no_window = cette fenêtre n’existe pas

# draft
workspace_tab_alone = l’onglet est seul dans sa fenêtre

# draft
workspace_tab_there = l’onglet est déjà dans cette fenêtre
