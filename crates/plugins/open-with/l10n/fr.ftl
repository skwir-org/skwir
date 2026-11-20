# French catalog of skwir-plugin-open-with, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
open_with_already_opens = { $app } ouvre déjà { $kind ->
        [folder] les dossiers
       *[file] les fichiers de type { $description }
    }

# draft
open_with_always_use = Toujours utiliser pour { $kind ->
        [folder] les dossiers
       *[file] les fichiers de type { $description }
    }

# draft
open_with_cancelled = ouverture annulée

# draft
open_with_change = Changer…

# draft
open_with_choices = Lister les applications pour la sélection

# draft
open_with_choices_description = Lister les applications qui ouvrent les éléments sélectionnés (ou le dossier du panneau avec `here`), les recommandées en premier, chacune avec l’invocation `open_with` qui les ouvre ; avec `defaults`, chacune avec le `open_with.set_default` qui en fait l’application par défaut de leur type

# draft
open_with_chooser = Ouvrir avec

# draft
open_with_chooser_title = { $count ->
        [1] Ouvrir { $name } avec
        [one] Ouvrir { $count } élément avec
       *[other] Ouvrir { $count } éléments avec
    }

# draft
open_with_default = Par défaut

# draft
open_with_default_generic = Par défaut · { $generic }

# draft
open_with_does_not_exist = { $path } n’existe pas

# draft
open_with_edit_associations = Réécrire les lignes d’application par défaut d’un type

# draft
open_with_edit_associations_description = Définir les lignes d’un type MIME dans un mimeapps.list (associations par défaut, ajoutées et retirées) ; ce qu’exécutent l’annulation et le rétablissement d’un choix d’application par défaut

# draft
open_with_file_manager = Gestionnaire de fichiers par défaut

# draft
open_with_file_manager_another = Un autre programme

# draft
open_with_file_manager_description = Quelle application ouvre les dossiers pour tous les programmes : le même choix que « Toujours utiliser » d’Ouvrir avec sur un dossier, et que les réglages du bureau.

# draft
open_with_file_manager_description_more = Avec Skwir choisi, Afficher dans le dossier d’un autre programme ouvre le dossier dans Skwir, ses fichiers sélectionnés, même quand Skwir n’est pas lancé.

# draft
open_with_file_manager_held = { $program } affiche encore les fichiers que d’autres programmes montrent dans leur dossier, jusqu’à ce qu’il se termine

# draft
open_with_file_manager_list = Le gestionnaire de fichiers par défaut

# draft
open_with_file_manager_list_description = L’application qui ouvre les dossiers pour tous les programmes, telle que les réglages la listent : les applications qui ouvrent les dossiers, au choix, chacune avec le `open_with.set_default` qui la rend par défaut, et si Skwir affiche les fichiers que d’autres programmes montrent dans leur dossier. Répond de mémoire

# draft
open_with_file_manager_not_installed = Skwir ne peut pas être choisi ici : son entrée de bureau n’est pas installée

# draft
open_with_file_manager_offer = Choisi, Skwir affiche aussi les fichiers que d’autres programmes montrent dans leur dossier

# draft
open_with_file_manager_reading = Lecture des applications qui ouvrent les dossiers…

# draft
open_with_file_manager_shows = Skwir affiche aussi les fichiers que d’autres programmes montrent dans leur dossier

# draft
open_with_file_manager_unchosen = Non choisi

# draft
open_with_file_manager_unchosen_opens = Non choisi ({ $app })

# draft
open_with_menu = Nommer l’application par défaut dans un menu

# draft
open_with_menu_description = L’élément Ouvrir d’un menu pour la sélection, ou pour les fichiers nommés (ceux de l’aperçu : `entries`, tels que le panneau les liste), qui nomme l’application par défaut qu’ils partagent, et les autres applications de leurs types dans son sous-menu Ouvrir avec ; aucun élément quand leurs types ou les applications ne sont pas encore connus. Répond de mémoire

# draft
open_with_no_application = Aucune application

# draft
open_with_no_applications = Aucune application trouvée

# draft
open_with_no_location = le panneau n’a pas d’emplacement

# draft
open_with_no_such_pane = panneau introuvable

# draft
open_with_none_opens = Aucune application n’ouvre { $kind ->
        [folder] les dossiers
       *[file] les fichiers de type { $description }
    }

# draft
open_with_not_a_file = { $name } n’est pas un fichier : Skwir ne l’ouvre pas

# draft
open_with_not_changed = L’application par défaut n’a pas été changée : { $error }

# draft
open_with_not_installed = aucune application { $id } n’est installée

# draft
open_with_not_your_list = { $path } n’est pas l’une de vos listes d’applications par défaut

# draft
open_with_nothing_to_open = rien à ouvrir

# draft
open_with_now = Actuelle

# draft
open_with_now_opens = { $app } ouvre désormais { $kind ->
        [folder] les dossiers
       *[file] les fichiers de type { $description }
    }

# draft
open_with_one_type_at_a_time = Une application par défaut se choisit pour un seul type de fichier à la fois

# draft
open_with_open = Ouvrir

# draft
open_with_open_description = Entrer dans le dossier sélectionné, ou ouvrir les fichiers sélectionnés avec l’application par défaut de leur type, telle que la choisissent les listes du bureau (celle que nomme le menu contextuel)

# draft
open_with_open_files_with = Ouvrir { $kind ->
        [folder] les dossiers
       *[file] les fichiers de type { $description }
    } avec

# draft
open_with_open_terminal = Ouvrir un terminal ici

# draft
open_with_open_terminal_description = Ouvrir un terminal dans le dossier sélectionné, sinon dans le dossier affiché, sinon (dans une recherche) dans le dossier qui contient la sélection

# draft
open_with_open_terminal_menu = Ouvrir un terminal ici

# draft
open_with_open_with = Ouvrir avec une application par son identifiant .desktop

# draft
open_with_open_with_app = Ouvrir avec { $app }

# draft
open_with_open_with_description = Ouvrir les éléments sélectionnés (ou le dossier du panneau avec `here`) avec l’application dont l’identifiant de fichier .desktop est `app`

# draft
open_with_opens_with = S’ouvre avec

# draft
open_with_other_application = Autre application…

# draft
open_with_other_applications = Autres applications

# draft
open_with_plugin_description = Ouvrir les fichiers avec leur application par défaut ou celle de votre choix, et ouvrir un terminal ici

# draft
open_with_plugin_title = Ouvrir avec

# draft
open_with_properties = Nommer l’application par défaut dans les Propriétés

# draft
open_with_properties_description = La ligne S’ouvre avec des Propriétés pour un fichier : l’application avec laquelle son type s’ouvre, et le moyen de la changer

# draft
open_with_recommended = Recommandées

# draft
open_with_related = Apparentées

# draft
open_with_search_applications = Rechercher des applications

# draft
open_with_set_default = Définir l’application par défaut d’un type

# draft
open_with_set_default_description = Faire de l’application dont l’identifiant de fichier .desktop est `app` l’application par défaut du type MIME `mime`, dans le mimeapps.list de l’utilisateur ; la notification propose Annuler
