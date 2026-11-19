# French catalog of skwir-plugin-tags, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
tags_add = Ajouter des étiquettes

# draft
tags_add_description = Ajouter des étiquettes aux éléments donnés, ou à la sélection (plan)

# draft
tags_add_plan = ajouter { $count ->
        [1] l’étiquette
        [one] l’étiquette
       *[other] les étiquettes
    } { $tags }

# draft
tags_add_remove_plan = ajouter { $added ->
        [1] l’étiquette
        [one] l’étiquette
       *[other] les étiquettes
    } { $add } et retirer { $removed ->
        [1] l’étiquette
        [one] l’étiquette
       *[other] les étiquettes
    } { $remove }

# draft
tags_add_tag_plan = ajouter l’étiquette { $tag }

# draft
tags_all_tags = Toutes les étiquettes

# draft
tags_already_called = l’étiquette s’appelle déjà { $to }

# draft
tags_aside_no_name = il n’a pas de nom d’après lequel nommer un fichier à côté de lui

# draft
tags_aside_other_layout = { $name }, à côté, n’est pas agencé comme ceux qu’écrit un Mac, et Skwir ne réécrit que ceux-là

# draft
tags_aside_too_large = { $name }, à côté, est plus grand que ce que Skwir réécrit

# draft
tags_aside_unreadable = { $name }, à côté, ne peut pas être lu : { $error }

# draft
tags_aside_unwritable = ses attributs Mac sont conservés sous une forme que Skwir n’écrit pas

# draft
tags_aspect_close = Revenir au dossier

# draft
tags_aspect_open = Étiquettes de ce dossier

# draft
tags_browse = Étiquettes d’un dossier

# draft
tags_browse_description = Afficher toutes les étiquettes des fichiers d’un dossier et de ses sous-dossiers (`in`, un chemin absolu) comme des emplacements à parcourir, chacun un dossier de ses fichiers : à défaut, le dossier que montre le panneau, ou celui dont relève l’emplacement d’étiquette ou la recherche qui s’y trouve ; avec `toggle` et sans dossier, un emplacement d’étiquette revient plutôt à son dossier

# draft
tags_browse_here = Parcourir les étiquettes ici

# draft
tags_browse_toggle = Étiquettes de ce dossier, ou retour à celui-ci

# draft
tags_category_no_comma = le nom d’une catégorie ne contient pas de virgule

# draft
tags_change_plan = modifier les étiquettes

# draft
tags_color = Couleur d’étiquette

# draft
tags_color_blue = Bleu

# draft
tags_color_choice = Couleur

# draft
tags_color_description = Choisir la couleur qu’une étiquette porte partout, ou lui rendre celle que lui donne son nom

# draft
tags_color_from_name = D’après son nom

# draft
tags_color_green = Vert

# draft
tags_color_not_saved = La couleur de l’étiquette n’a pas été enregistrée : { $error }

# draft
tags_color_one_of = la couleur d’une étiquette est l’une de celles-ci : { $colors }

# draft
tags_color_orange = Orange

# draft
tags_color_pink = Rose

# draft
tags_color_purple = Violet

# draft
tags_color_red = Rouge

# draft
tags_color_teal = Bleu canard

# draft
tags_color_yellow = Jaune

# draft
tags_colors_data = Couleurs choisies pour les étiquettes, par étiquette en minuscules : {"{"} tag = color {"}"}

# draft
tags_comment_accept = Enregistrer

# draft
tags_comment_item = Commenter…

# draft
tags_comment_menu = Menu du commentaire

# draft
tags_comment_menu_description = L’élément Commenter… du menu contextuel, qui contient le commentaire de l’élément sélectionné

# draft
tags_comment_placeholder = Un commentaire sur la sélection (vide pour le retirer)

# draft
tags_comment_title = Commentaire

# draft
tags_counted = Étiquettes comptées

# draft
tags_counted_description = `{"{"} root, counting, counted, partial {"}"}` : si les étiquettes des fichiers de `root` (le dossier personnel s’il est omis) sont en cours de comptage pour ses emplacements d’étiquettes, l’instant dont date le comptage qui les couvre (`counted`, en secondes depuis l’epoch Unix), et si certains sont laissés de côté faute de place (`partial`). Répond de mémoire

# draft
tags_delete = Retirer l’étiquette partout

# draft
tags_delete_description = Retirer une étiquette (ou `tags`) de tous les fichiers du dossier personnel (ou de `root`) qui la portent : les fichiers sont cherchés en arrière-plan et le plan proposé pour examen

# draft
tags_depth_below = Avec les sous-étiquettes

# draft
tags_depth_here = Cette étiquette

# draft
tags_depth_top_below = Tous les fichiers étiquetés

# draft
tags_depth_top_here = Ces étiquettes

# draft
tags_drop = Déposer sur une étiquette

# draft
tags_drop_description = Donner à des fichiers déposés ou collés sur un emplacement d’étiquette (`location`) son étiquette : la sélection d’un panneau (`pane`) ou `entries` ; avec `move`, en les retirant de l’emplacement d’étiquette d’où ils viennent (celui du panneau, ou `from`). Les étiquettes déposées y sont rangées, par un plan à examiner

# draft
tags_drop_on_one = les fichiers reçoivent une étiquette à la fois : déposez-les sur l’une d’elles

# draft
tags_edit = Modifier les étiquettes

# draft
tags_edit_description = Ajouter certaines étiquettes aux éléments donnés, ou à la sélection, et en retirer d’autres, en un seul plan : chaque élément garde ses autres étiquettes

# draft
tags_edit_item = Modifier les étiquettes…

# draft
tags_empty_tag = Aucun fichier ici n’a cette étiquette

# draft
tags_empty_tag_detail = Déposez des fichiers ici pour leur donner l’étiquette.

# draft
tags_empty_top = Aucun fichier ici n’a d’étiquette

# draft
tags_empty_top_detail = Étiquetez des fichiers avec Étiquettes… dans leur menu, ou déposez-les sur une étiquette de la barre latérale.

# draft
tags_exclusive = Une étiquette d’une catégorie à la fois

# draft
tags_exclusive_data = Les catégories dont un fichier n’a qu’une étiquette à la fois : des champs, dont les valeurs sont les étiquettes qu’elles contiennent

# draft
tags_exclusive_description = Faire qu’une catégorie (`tag`) ne contienne qu’une étiquette par fichier à la fois, un champ dont les valeurs sont ses étiquettes (`on`), ou de nouveau plusieurs

# draft
tags_exclusive_not_changed = La catégorie { $tag } n’a pas été modifiée : { $error }

# draft
tags_exclusive_off = Un fichier peut de nouveau avoir plusieurs étiquettes de { $tag }

# draft
tags_exclusive_on = Un fichier n’a plus qu’une étiquette de { $tag } à la fois

# draft
tags_export = Copier une étiquette dans un dossier

# draft
tags_export_description = Créer dans `dest` un dossier des fichiers que liste un emplacement d’étiquette (`location`), nommé comme son étiquette, avec à l’intérieur un dossier pour chaque étiquette qu’elle contient : un plan proposé pour examen. Ce que fait une étiquette déposée ou collée dans un dossier

# draft
tags_export_one = on copie une étiquette, pas toutes à la fois

# draft
tags_export_plan = copier les fichiers étiquetés { $tag } dans { $folder }

# draft
tags_files_menu = Menu des étiquettes de la sélection

# draft
tags_files_menu_description = Le sous-menu Étiquettes du menu contextuel : les étiquettes des fichiers sélectionnés, puis les étiquettes épinglées, chacune cochée quand tous la portent, en partie quand certains la portent ; l’activer la donne à tous ou la leur retire

# draft
tags_files_not_found = Les fichiers étiquetés { $tag } n’ont pas été trouvés : { $error }

# draft
tags_filter_not_kept = { $done } ; son filtre « { $filter } » n’est pas gardé avec elle

# draft
tags_finder_limits = une note, ou une couleur d’étiquette autre que les sept du Finder

# draft
tags_follow_both_already = l’étiquette { $to } était déjà épinglée et colorée comme { $from }

# draft
tags_follow_both_did = étiquette { $to } gardée épinglée et colorée comme { $from }

# draft
tags_follow_both_will = garder l’étiquette { $to } épinglée et colorée comme { $from }

# draft
tags_follow_color_already = l’étiquette { $to } avait déjà la couleur de { $from }

# draft
tags_follow_color_did = couleur de { $from } donnée à l’étiquette { $to }

# draft
tags_follow_color_will = donner à l’étiquette { $to } la couleur de { $from }

# draft
tags_follow_pin_already = l’étiquette { $to } était déjà épinglée à la position de { $from }

# draft
tags_follow_pin_did = étiquette { $to } épinglée à la position de { $from }

# draft
tags_follow_pin_will = épingler l’étiquette { $to } à la position de { $from }

# draft
tags_follow_unpin_already = { $count ->
        [1] l’épinglage de { $from } était déjà retiré
       *[other] les épinglages de { $from } étaient déjà retirés
    }

# draft
tags_follow_unpin_did = { $count ->
        [1] épinglage de { $from } retiré
       *[other] épinglages de { $from } retirés
    }

# draft
tags_follow_unpin_will = désépingler { $from }

# draft
tags_give_file_already = { $name } était déjà étiqueté avec { $tag }

# draft
tags_give_file_did = { $name } étiqueté avec { $tag }

# draft
tags_give_file_will = étiqueter { $name } avec { $tag }

# draft
tags_in_category_already = ces étiquettes sont déjà dans { $category }

# draft
tags_in_no_category = ces étiquettes ne sont dans aucune catégorie

# draft
tags_include_sub_tags = Inclure les sous-étiquettes

# draft
tags_index_full = Des fichiers étiquetés sont laissés hors des étiquettes : la place que Skwir leur réserve est épuisée

# draft
tags_inside_itself = l’étiquette { $tag } ne peut pas être placée à l’intérieur d’elle-même

# draft
tags_keep = Épingler l’étiquette affichée

# draft
tags_keep_description = Épingler l’étiquette que montre l’emplacement d’étiquette d’un panneau : une étiquette du dossier personnel, non restreinte

# draft
tags_keep_forget = Désépingler cette étiquette de la barre latérale

# draft
tags_keep_one = seule une étiquette du dossier personnel, non restreinte, se garde dans la barre latérale : épinglée

# draft
tags_keep_save = Épingler cette étiquette dans la barre latérale

# draft
tags_kind_field = Champ

# draft
tags_kind_tag = Étiquette

# draft
tags_known = Étiquettes connues

# draft
tags_known_description = `{"{"} generation, tags? {"}"}` : toutes les étiquettes de l’utilisateur, chacune une fois, de mémoire : les épinglées, dans leur ordre ; puis les plus récemment données ; puis celles vues sur des fichiers ; puis celles comptées dans le dossier personnel, les plus répandues d’abord. `generation` les numérote : demandées avec la génération détenue, `tags` est omis tant qu’elle est à jour

# draft
tags_limit_description = La mémoire que Skwir peut consacrer à ce qu’il sait des fichiers étiquetés, environ seize mille fichiers par mégaoctet, pour parcourir les étiquettes comme des dossiers, les suggestions de la barre latérale et des recherches d’étiquettes plus rapides.

# draft
tags_limit_description_more = Au-delà, les nouveaux fichiers étiquetés sont laissés hors des dossiers des étiquettes, et Skwir le signale.

# draft
tags_limit_title = Place réservée aux fichiers étiquetés

# draft
tags_list = Lister les étiquettes

# draft
tags_list_description = Les étiquettes utilisées dans ce qu’affiche le panneau, avec leurs nombres (celles qui ne diffèrent que par la casse comptent pour une) ; avec `read`, lues aussi dans les fichiers dont les étiquettes n’ont pas encore été lues

# draft
tags_looking_home = Recherche des fichiers étiquetés { $tags } dans le dossier personnel…

# draft
tags_looking_in = Recherche des fichiers étiquetés { $tags } dans « { $root } »…

# draft
tags_merge_accept = Fusionner

# draft
tags_merge_ask = Fusionner ces étiquettes en une seule, sur tous les fichiers

# draft
tags_merge_item = Fusionner en une seule…

# draft
tags_merge_placeholder = L’étiquette qu’elles deviennent

# draft
tags_merge_plan = fusionner les étiquettes { $tags } en { $to }

# draft
tags_merged_into = { $merged ->
        [one] { $merged } fichier avait déjà { $to }, ou plusieurs des étiquettes renommées : il ne garde { $to } qu’une fois.
       *[other] { $merged } fichiers avaient déjà { $to }, ou plusieurs des étiquettes renommées : chacun ne garde { $to } qu’une fois.
    }

# draft
tags_merged_names = { $merged ->
        [one] { $merged } fichier avait deux étiquettes qui portent désormais le même nom : il ne garde cette étiquette qu’une fois.
       *[other] { $merged } fichiers avaient deux étiquettes qui portent désormais le même nom : chacun ne garde cette étiquette qu’une fois.
    }

# draft
tags_move = Déplacer l’étiquette dans une catégorie

# draft
tags_move_accept = Déplacer

# draft
tags_move_ask = Déplacer dans une catégorie, en gardant les noms

# draft
tags_move_description = Placer une étiquette (ou `tags`) dans la catégorie `into`, en gardant son propre nom (`Paris` dans `Lieux/France` devient `Lieux/France/Paris`), ou la sortir de sa catégorie sans `into`, sur tous les fichiers du dossier personnel (ou de `root`) : les fichiers sont cherchés en arrière-plan et le plan proposé pour examen

# draft
tags_move_file_already = { $name } était déjà dans { $tag }

# draft
tags_move_file_did = { $name } déplacé de { $from } vers { $tag }

# draft
tags_move_file_will = déplacer { $name } de { $from } vers { $tag }

# draft
tags_move_into_plan = déplacer l’étiquette { $tag } dans { $into }

# draft
tags_move_item = Déplacer dans une catégorie…

# draft
tags_move_out_all_item = Sortir de leurs catégories

# draft
tags_move_out_item = Sortir de { $category }

# draft
tags_move_out_plan = sortir l’étiquette { $tag } de sa catégorie

# draft
tags_move_placeholder = Catégorie (Lieux/France)

# draft
tags_move_several_into_plan = déplacer les étiquettes { $tags } dans { $into }

# draft
tags_move_several_plan = déplacer les étiquettes { $tags }

# draft
tags_move_tag_plan = déplacer de l’étiquette { $from } vers l’étiquette { $tag }

# draft
tags_narrow = Restreindre un emplacement d’étiquette

# draft
tags_narrow_deep_only = un emplacement d’étiquette se restreint par `deep` ; `{ $facet }` relève du filtre du panneau (query.narrow)

# draft
tags_narrow_description = Afficher, dans l’emplacement d’étiquette que montre le panneau, tous les fichiers compris dans l’étiquette (`facet` `deep`), pas seulement ceux qui la portent exactement ; `on` à false l’annule, omis il bascule. Les autres étiquettes, genres et années relèvent du filtre du panneau (`query.narrow`)

# draft
tags_needs_name = une étiquette doit avoir un nom

# draft
tags_needs_search = trouver les fichiers étiquetés nécessite le module Recherche

# draft
tags_new = Nouvelle étiquette

# draft
tags_new_accept = Créer

# draft
tags_new_ask = Nouvelle étiquette

# draft
tags_new_description = Créer une étiquette `name` dans l’étiquette que montre un emplacement d’étiquette (`location`) : affichée là jusqu’à ce qu’un fichier la reçoive

# draft
tags_new_in_ask = Nouvelle étiquette dans { $tag }

# draft
tags_new_placeholder = Nom

# draft
tags_new_verb = Nouvelle étiquette…

# draft
tags_no_comma = le nom d’une étiquette ne contient pas de virgule

# draft
tags_no_home = aucun dossier personnel

# draft
tags_no_home_to_find = aucun dossier personnel où chercher les étiquettes

# draft
tags_no_rating = Aucune note

# draft
tags_no_such_pane = ce panneau n’existe pas

# draft
tags_none_tagged_home = Aucun fichier du dossier personnel n’est étiqueté { $tags }

# draft
tags_none_tagged_in = Aucun fichier de « { $root } » n’est étiqueté { $tags }

# draft
tags_one_at_a_time = Une étiquette de { $category } à la fois

# draft
tags_open = Ouvrir une étiquette

# draft
tags_open_description = Afficher les fichiers du dossier personnel qui portent une étiquette, et un emplacement pour chaque étiquette qu’elle contient, comme le fait sa pastille épinglée, dans le panneau donné ou le panneau actif

# draft
tags_open_tag = Ouvrir l’étiquette

# draft
tags_pane_shows_no_tags = le panneau ne montre aucune étiquette

# draft
tags_pane_shows_nothing = le panneau ne montre rien

# draft
tags_pill = Une étiquette, ici

# draft
tags_pill_description = Une étiquette de la barre latérale cliquée avec Maj (`how` `also` : les fichiers affichés qui la portent aussi) ou Alt (`here` : ses fichiers dans le dossier affiché)

# draft
tags_pin = Épingler des étiquettes

# draft
tags_pin_description = Garder des étiquettes (`tag`, `tags`) dans la barre latérale, avant l’étiquette épinglée `before` ou en dernier ; déplace celles déjà épinglées

# draft
tags_pin_item = Épingler dans la barre latérale

# draft
tags_pin_moved = Étiquette { $tag } déplacée

# draft
tags_pinned = { $count ->
        [1] Étiquette { $tag } épinglée dans la barre latérale
        [one] { $count } étiquette épinglée dans la barre latérale
       *[other] { $count } étiquettes épinglées dans la barre latérale
    }

# draft
tags_pinned_data = Les étiquettes épinglées dans la barre latérale, dans leur ordre

# draft
tags_pins = Définir les étiquettes épinglées

# draft
tags_pins_back = Étiquettes épinglées remises comme elles étaient

# draft
tags_pins_description = Épingler exactement `tags`, dans leur ordre : ce qu’exécute l’annulation d’un épinglage

# draft
tags_pins_not_saved = les étiquettes épinglées n’ont pas été enregistrées

# draft
tags_pins_not_saved_said = Les étiquettes épinglées n’ont pas été enregistrées : { $error }

# draft
tags_place_menu = Menu des étiquettes

# draft
tags_place_menu_description = Les éléments du menu contextuel pour les étiquettes sélectionnées dans un emplacement d’étiquette. Répond de mémoire

# draft
tags_plan_note_home = { $count ->
        [one] { $count } fichier
       *[other] { $count } fichiers
    } dans le dossier personnel ; ce que vos exclusions écartent est laissé de côté.

# draft
tags_plan_note_home_merged = { $count ->
        [one] { $count } fichier
       *[other] { $count } fichiers
    } dans le dossier personnel ; ce que vos exclusions écartent est laissé de côté. { $merged }

# draft
tags_plan_note_in = { $count ->
        [one] { $count } fichier
       *[other] { $count } fichiers
    } dans « { $root } » ; ce que vos exclusions écartent est laissé de côté.

# draft
tags_plan_note_in_merged = { $count ->
        [one] { $count } fichier
       *[other] { $count } fichiers
    } dans « { $root } » ; ce que vos exclusions écartent est laissé de côté. { $merged }

# draft
tags_plugin_description = Étiquettes, notes et commentaires conservés dans les attributs étendus des fichiers, sans base de données ; la provenance des téléchargements

# draft
tags_plugin_off = le module Étiquettes est désactivé

# draft
tags_plugin_title = Étiquettes

# draft
tags_rating_choice = Note

# draft
tags_remove = Retirer des étiquettes

# draft
tags_remove_all_plan = retirer les étiquettes

# draft
tags_remove_all_verb = Retirer toutes les étiquettes

# draft
tags_remove_description = Retirer des étiquettes des éléments donnés, ou de la sélection (plan)

# draft
tags_remove_everywhere_item = Retirer l’étiquette de tous les fichiers…

# draft
tags_remove_everywhere_plan = retirer l’étiquette { $tag } de tous les fichiers

# draft
tags_remove_from_files = { $count ->
        [1] Retirer de ce fichier
        [one] Retirer des fichiers sélectionnés
       *[other] Retirer des fichiers sélectionnés
    }

# draft
tags_remove_from_verb = Retirer de { $tag }

# draft
tags_remove_plan = retirer { $count ->
        [1] l’étiquette
        [one] l’étiquette
       *[other] les étiquettes
    } { $tags }

# draft
tags_remove_several_everywhere_plan = retirer les étiquettes { $tags } de tous les fichiers

# draft
tags_rename = Renommer l’étiquette partout

# draft
tags_rename_accept = Renommer

# draft
tags_rename_ask = Renommer l’étiquette { $tag } sur tous les fichiers

# draft
tags_rename_description = Renommer une étiquette (ou fusionner `tags`) sur tous les fichiers du dossier personnel (ou de `root`) qui la portent, en la fusionnant avec une étiquette du nouveau nom là où un fichier a les deux : les fichiers sont cherchés en arrière-plan et le plan proposé pour examen

# draft
tags_rename_placeholder = Nouveau nom (celui d’une étiquette existante les fusionne)

# draft
tags_rename_plan = renommer l’étiquette { $tag } en { $to }

# draft
tags_rename_verb = Renommer l’étiquette…

# draft
tags_restore_already = les étiquettes épinglées et leurs couleurs étaient déjà remises

# draft
tags_restore_did = étiquettes épinglées et leurs couleurs remises

# draft
tags_restore_will = remettre les étiquettes épinglées et leurs couleurs

# draft
tags_search_cancelled = recherche annulée

# draft
tags_search_no_location = la recherche n’a donné aucun emplacement

# draft
tags_search_stopped = la recherche s’est arrêtée

# draft
tags_selected = Étiquettes de la sélection

# draft
tags_selected_description = `{"{"} items, tags: [{"{"} name, count, color? {"}"}], known {"}"}` : les étiquettes de la sélection, lues dans ses fichiers, avec le nombre de fichiers qui portent chacune et la couleur qu’un fichier donne à l’une d’elles (celle d’une étiquette du Finder), et les étiquettes connues, les épinglées d’abord, puis les plus récemment utilisées

# draft
tags_set = Définir les étiquettes

# draft
tags_set_description = Remplacer les étiquettes des éléments donnés, ou celles de la sélection (plan)

# draft
tags_set_plan = définir { $count ->
        [1] l’étiquette
        [one] l’étiquette
       *[other] les étiquettes
    } { $tags }

# draft
tags_show_files_here = Afficher ses fichiers ici

# draft
tags_stars = { $stars ->
        [one] { $stars } étoile
       *[other] { $stars } étoiles
    }

# draft
tags_store_appledouble = Fichiers ._ d’un Mac, à côté des fichiers sur les volumes sans attributs

# draft
tags_store_finder = Étiquettes et commentaires du Finder d’un Mac

# draft
tags_store_xdg = Attributs de l’environnement de bureau (KDE, Dolphin, Baloo)

# draft
tags_suggested_description = Tant qu’aucune étiquette n’est épinglée, le nombre d’étiquettes que suggère la barre latérale, les plus répandues d’abord ; 0 n’en suggère aucune.

# draft
tags_suggested_title = Étiquettes suggérées dans la barre latérale

# draft
tags_suggestion_detail = Étiquette · { $tag }

# draft
tags_tag_menu = Menu d’une étiquette

# draft
tags_tag_menu_description = Les éléments du menu d’une étiquette là où elle est affichée : sa pastille dans Propriétés (tags) ou dans la barre latérale (location). Répond de mémoire

# draft
tags_title = Étiquettes

# draft
tags_toggle = Ajouter ou retirer une étiquette

# draft
tags_toggle_description = Donner une étiquette aux éléments donnés, ou à la sélection, ou la retirer de chaque élément quand tous la portent : une commande à associer à une touche ; avec `on`, la donner (true) ou la retirer (false) quoi qu’ils portent (plan)

# draft
tags_toggle_form = Ajouter ou retirer l’étiquette { $tag }

# draft
tags_unpin = Désépingler des étiquettes

# draft
tags_unpin_description = Retirer des étiquettes (`tag`, `tags`) de la barre latérale ; leurs fichiers les gardent

# draft
tags_unpin_item = Désépingler de la barre latérale

# draft
tags_unpinned = { $count ->
        [1] Étiquette { $tag } désépinglée
        [one] { $count } étiquette désépinglée
       *[other] { $count } étiquettes désépinglées
    }

# draft
tags_untag = Retirer de l’étiquette

# draft
tags_untag_all_file_already = { $name } n’avait déjà aucune étiquette

# draft
tags_untag_all_file_did = étiquettes de { $name } retirées

# draft
tags_untag_all_file_will = retirer les étiquettes de { $name }

# draft
tags_untag_all_plan = retirer toutes les étiquettes

# draft
tags_untag_description = Retirer l’étiquette que montre un emplacement d’étiquette (celui du panneau, ou `location`) des fichiers sélectionnés ; les étiquettes qui y sont sélectionnées sont retirées de tous les fichiers, par un plan à examiner. Ce que fait Suppr dans un emplacement d’étiquette

# draft
tags_untag_file_already = l’étiquette { $tag } était déjà absente de { $name }

# draft
tags_untag_file_did = étiquette { $tag } retirée de { $name }

# draft
tags_untag_file_will = retirer l’étiquette { $tag } de { $name }

# draft
tags_untag_plan = retirer l’étiquette { $tag }

# draft
tags_what_new_name = quel nouveau nom ?

# draft
tags_which_category = quelle catégorie ?

# draft
tags_which_tag = quelle étiquette ?

# draft
tags_with_sub_tags = Avec les sous-étiquettes

# draft
tags_xdg_limits = une virgule dans une étiquette, ou la couleur d’une étiquette
