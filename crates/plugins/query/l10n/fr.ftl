# French catalog of skwir-plugin-query, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
query_accept_filter = Filtrer

# draft
query_accept_search = Rechercher

# draft
query_another_tag = Une autre étiquette…

# draft
query_app_ended = l’application s’est terminée

# draft
query_ask_tag_placeholder = Une étiquette

# draft
query_bad_glob = motif { $key } non valide `{ $glob }` : { $error }

# draft
query_bad_regex = regex non valide `{ $regex }` : { $error }

# draft
query_bad_size = taille non valide `{ $value }`

# draft
query_bad_time = date ou durée non valide `{ $value }` (essayez 2026-05-01, this-week ou 7d)

# draft
query_below = Ce dossier et ses sous-dossiers

# draft
query_below_description = Chercher, dans le dossier du panneau et ses sous-dossiers, ce qui le filtre : son filtre et les mots tapés par-dessus

# draft
query_chip_accessed = Consulté

# draft
query_chip_contains = Contient

# draft
query_chip_created = Créé

# draft
query_chip_in = Dans { $place }

# draft
query_chip_kind = Genre

# draft
query_chip_kinds = { $kinds }

# draft
query_chip_modified = Modifié

# draft
query_chip_name = Nom

# draft
query_chip_not_tag = Sans l’étiquette

# draft
query_chip_size = Taille

# draft
query_chip_subfolders = Avec sous-dossiers

# draft
query_chip_tag = Étiquette

# draft
query_chip_taken = Prise de vue

# draft
query_chip_when = { $when }

# draft
query_chip_when_days = { $first } → { $last }

# draft
query_complete = Compléter une recherche

# draft
query_complete_description = { $shape } : ce qui peut prolonger le terme qui finit à `caret` (en octets ; la fin s’il est omis), le `text` de chaque élément écrit à la place de `range` : les clés que commence un mot, les valeurs d’une clé (genres, étiquettes des fichiers du panneau puis les autres de l’utilisateur, 200 au plus, `more` comptant celles laissées de côté, périodes, tailles, étoiles) ; `typing` quand ce terme est encore incomplet

# draft
query_contains_placeholder = Texte que contiennent les fichiers

# draft
query_custom = Personnalisé…

# draft
query_depth_below = Ce dossier et ses sous-dossiers

# draft
query_depth_chip = Ce dossier uniquement

# draft
query_depth_here = Ce dossier uniquement

# draft
query_describe = Décrire la recherche

# draft
query_describe_description = Les pastilles de la recherche, telles que la barre d’adresse les dessine pour un panneau qui affiche une recherche : chaque facette (words, kind, modified, taken, size, tag, content, where ; created et accessed une fois définies) avec sa valeur et l’invocation que lance chacun de ses choix, puis les parties qu’aucune facette ne contient

# draft
query_describe_filter = Décrire le filtre

# draft
query_describe_filter_description = { $shape } : ce qui filtre le panneau (son filtre et les mots tapés par-dessus, ADR-107) sous la forme des pastilles d’une recherche (`query.describe`), chaque choix le modifiant (`query.filter`) : jusqu’où il regarde (pour un dossier : ce dossier uniquement, ou ses sous-dossiers recherchés, `query.below`), puis chaque facette, définie ou non

# draft
query_edit = Modifier une recherche enregistrée

# draft
query_edit_accept = Enregistrer

# draft
query_edit_description = Modifier ce que cherche une recherche enregistrée, ou ce qui filtre un emplacement enregistré ; son nom et sa place dans la barre latérale ne changent pas

# draft
query_edit_filter_menu = Modifier le filtre…

# draft
query_edit_filter_title = Modifier le filtre

# draft
query_edit_search_menu = Modifier la recherche…

# draft
query_edit_search_placeholder = { $example }  ({ $key } choisit la racine)

# draft
query_edit_search_title = Modifier la recherche enregistrée

# draft
query_empty_regex = { $key } regex vide

# draft
query_error = recherche : { $error }

# draft
query_everything = Tout

# draft
query_expected_bool = { $yes } ou { $no } attendu, et non `{ $value }`

# draft
query_expected_comparison = comparaison attendue après `{ $text }`

# draft
query_expected_term = terme attendu

# draft
query_expects_distance = { $key } attend une distance comme 5km ou 500m, et non `{ $value }`

# draft
query_expects_edges = { $key } attend les bords sud, ouest, nord et est, comme 9.9,19.9,10.2,20.3, et non `{ $value }`

# draft
query_expects_either = { $key } attend { $one } ou { $other }, et non `{ $value }`

# draft
query_expects_extension = { $key } attend au moins une extension

# draft
query_expects_folder = { $key } attend un dossier

# draft
query_expects_kind = { $key } attend { $kinds }, et non `{ $value }`

# draft
query_expects_kinds = { $key } attend { $kinds }

# draft
query_expects_number = { $key } attend un nombre, et non `{ $value }`

# draft
query_expects_point = { $key } attend une latitude et une longitude, comme 45.1,6.2, et non `{ $value }`

# draft
query_expects_stars = { $key } attend de 0 à 5 étoiles (demi-étoiles permises), et non `{ $value }`

# draft
query_expects_type = { $key } attend { $file }, { $dir } ou { $link }, et non `{ $value }`

# draft
query_expects_value = { $key } attend une valeur

# draft
query_field_among = parmi les fichiers de { $place }

# draft
query_field_close_quote = Fermez le guillemet pour rechercher

# draft
query_field_home = partout dans le dossier personnel

# draft
query_field_in_folders = dans { $place } et ses sous-dossiers

# draft
query_field_more_tags = { $count ->
        [one] { $count } autre étiquette
       *[other] { $count } autres étiquettes
    }

# draft
query_field_narrow_tags = Tapez davantage du nom pour affiner

# draft
query_field_recent_in = dans { $place }

# draft
query_field_saved_in = { $filter } dans { $place }

# draft
query_field_search = Rechercher « { $search } »

# draft
query_field_search_contents = Contenu

# draft
query_field_search_disk = Tout le disque

# draft
query_field_search_for = Rechercher « { $words } »

# draft
query_field_search_names = Noms

# draft
query_field_type_value = Tapez une valeur pour rechercher

# draft
query_field_whole_disk = sur tout le disque

# draft
query_field_within = parmi tous les fichiers sous { $place }

# draft
query_filter = Filtrer le dossier

# draft
query_filter_description = Modifier une facette de ce qui filtre le panneau (les mots tapés par-dessus dans la barre d’adresse, son filtre), comme `query.refine` modifie celle d’une recherche ; l’emplacement reste affiché

# draft
query_filter_tags = Filtrer les étiquettes

# draft
query_filter_taken = ce filtre est déjà enregistré sous { $name }

# draft
query_find_similar_submenu = Trouver des fichiers similaires

# draft
query_forget = Retirer une recherche enregistrée

# draft
query_forget_description = Retirer une recherche enregistrée de la barre latérale, désignée par sa recherche (un emplacement enregistré par son filtre et `at`) ou par son nom

# draft
query_forget_recent = Oublier une recherche récente

# draft
query_forget_recent_description = Retirer une recherche de celles que la barre d’adresse propose comme lancées récemment

# draft
query_group_100_to_1 = De 100 { $unit } à 1 { $next }

# draft
query_group_12_to_24_mp = De 12 à 24 Mpx

# draft
query_group_1_to_100 = De 1 à 100 { $unit }

# draft
query_group_1_to_4_mp = De 1 à 4 Mpx

# draft
query_group_1_to_5_minutes = De 1 à 5 minutes

# draft
query_group_20_minutes_to_1_hour = De 20 minutes à 1 heure

# draft
query_group_4_to_12_mp = De 4 à 12 Mpx

# draft
query_group_5_to_20_minutes = De 5 à 20 minutes

# draft
query_group_digits = 0–9

# draft
query_group_earlier_this_month = Plus tôt ce mois-ci

# draft
query_group_earlier_this_week = Plus tôt cette semaine

# draft
query_group_earlier_this_year = Plus tôt cette année

# draft
query_group_empty = Vide

# draft
query_group_folders = Dossiers

# draft
query_group_future = Dans le futur

# draft
query_group_last_month = Le mois dernier

# draft
query_group_last_week = La semaine dernière

# draft
query_group_no_date = Sans date

# draft
query_group_no_dimensions = Sans dimensions

# draft
query_group_no_duration = Sans durée

# draft
query_group_no_extension = Sans extension

# draft
query_group_no_folder = Sans dossier

# draft
query_group_no_owner = Sans propriétaire

# draft
query_group_no_tags = Sans étiquette

# draft
query_group_other_kind = Autres

# draft
query_group_other_names = Autres

# draft
query_group_over_1 = Plus de 1 { $unit }

# draft
query_group_over_1_hour = Plus d’une heure

# draft
query_group_over_24_mp = Plus de 24 Mpx

# draft
query_group_state_unknown = Inconnu

# draft
query_group_today = Aujourd’hui

# draft
query_group_unchanged = Inchangé

# draft
query_group_under_100 = Moins de 100 { $unit }

# draft
query_group_under_1_minute = Moins d’une minute

# draft
query_group_under_1_mp = Moins de 1 Mpx

# draft
query_group_unknown_type = Type inconnu

# draft
query_group_unrated = Non noté

# draft
query_group_yesterday = Hier

# draft
query_here = Ce dossier uniquement

# draft
query_here_description = Afficher la recherche du panneau comme le dossier où elle cherche, filtré par ses mots, quand le dossier seul peut les trancher ; sinon, chercher dans ce dossier seul. Un dossier reste tel quel

# draft
query_home = Dossier personnel

# draft
query_interpret = Lire des mots comme une recherche

# draft
query_interpret_description = { $shape } ou null : des mots simples lus comme des termes selon des règles (périodes et années, genres, extensions, tailles, étiquettes du panneau), le reste gardé comme noms ; jamais appliqué de lui-même

# draft
query_keep_place = Enregistrer avec ce filtre

# draft
query_keep_search = Enregistrer cette recherche

# draft
query_key_accessed = Consulté

# draft
query_key_case = Majuscules distinguées, ou non

# draft
query_key_comment = Avec un commentaire contenant

# draft
query_key_comment_example = des mots du commentaire d’un fichier

# draft
query_key_content = Contenant un texte

# draft
query_key_content_example = {"\u0022"}mots{"\u0022"} ou /regex/

# draft
query_key_created = Créé

# draft
query_key_depth = Jusqu’à quelle profondeur chercher

# draft
query_key_depth_example = de combien de niveaux de dossiers elle descend : 2

# draft
query_key_excluded = Dossiers exclus aussi

# draft
query_key_excluded_example = ce que vos exclusions écartent aussi : { $yes }, { $no }

# draft
query_key_ext = Avec une extension

# draft
query_key_hidden = Fichiers cachés aussi, ou jamais

# draft
query_key_in = Où chercher

# draft
query_key_in_example = un dossier, {"\u0022"}~/My Files{"\u0022"}

# draft
query_key_inside = Pris dans un rectangle

# draft
query_key_inside_example = sud,ouest,nord,est

# draft
query_key_kind = D’un genre

# draft
query_key_mime = D’un type

# draft
query_key_modified = Modifié

# draft
query_key_name = Nommé

# draft
query_key_near = Pris près d’un point

# draft
query_key_rating = Noté

# draft
query_key_size = D’une taille

# draft
query_key_tag = Avec une étiquette

# draft
query_key_tag_example = une étiquette, ou * pour n’importe laquelle ; #tag aussi

# draft
query_key_taken = Prise de vue, images et vidéos

# draft
query_key_taken_example = 2023, last-year, selon l’horloge de l’appareil

# draft
query_key_type = Fichiers, dossiers ou liens

# draft
query_list = Lister les recherches enregistrées

# draft
query_list_description = Renvoyer les recherches enregistrées, dans l’ordre de la barre latérale

# draft
query_location = Emplacement de recherche

# draft
query_location_description = { $shape } : la recherche d’une valeur d’une facette (`tag` `work`, ou `tag` seul pour n’importe quelle étiquette) depuis `root` ou le dossier personnel, pour qui affiche ou lance une recherche sans en écrire la syntaxe

# draft
query_matches_nothing = { $term } ne correspond à rien

# draft
query_missing_paren = il manque `)`

# draft
query_move = Déplacer une recherche enregistrée

# draft
query_move_description = Placer une recherche enregistrée avant une autre dans la barre latérale, ou en dernier

# draft
query_name_change = Modifier…

# draft
query_name_contains = Le nom contient

# draft
query_name_contains_menu = Le nom contient…

# draft
query_name_placeholder = Mots ou motifs : invoice *.pdf

# draft
query_named_in = { $what } dans { $place }

# draft
query_narrow = Modifier le filtre

# draft
query_narrow_description = Modifier une facette du filtre du panneau (ADR-107 : ce que la liste peut trancher de ce qui le filtre ou de sa recherche, ses fichiers d’un genre, d’une année, avec ou sans une étiquette), gardé quand le panneau affiche d’autres emplacements jusqu’à ce qu’on le retire : celui d’une recherche à sa place, le filtre de tout autre emplacement ; sans `facet`, retirer le filtre

# draft
query_needs_value = `{ $facet }` demande une valeur

# draft
query_no_facet = aucune facette `{ $facet }`

# draft
query_no_kind = aucun genre `{ $kind }`

# draft
query_no_pane = panneau introuvable

# draft
query_no_pane_numbered = pas de panneau { $pane }

# draft
query_no_part = aucune partie `{ $part }`

# draft
query_no_saved_named = aucune recherche enregistrée nommée { $name }

# draft
query_no_search_no_filter = le panneau n’affiche ni recherche ni filtre

# draft
query_no_search_no_folder = le panneau n’affiche ni recherche ni dossier

# draft
query_no_tags_among = Aucune étiquette parmi ces fichiers

# draft
query_not_a_lens = un filtre porte sur ce que l’on sait des fichiers : `{ $facet }` relève d’une recherche (ce qu’ils contiennent, la date de prise de vue d’une image, les noms)

# draft
query_not_a_place = { $key } `{ $value }` n’est pas un lieu

# draft
query_not_allowed = { $key } n’est pas permis ici

# draft
query_not_on_earth = { $key } une distance de `{ $value }` est impossible sur la Terre

# draft
query_not_one_condition = `{ $term }` n’est pas une condition unique

# draft
query_not_tagged_instead = Sans cette étiquette, plutôt

# draft
query_not_within_search = une recherche ne peut pas chercher dans une autre recherche

# draft
query_nothing_found = Aucun résultat

# draft
query_nothing_found_detail = Aucun fichier ne correspond à « { $text } » ici. Vérifiez l’orthographe ou cherchez dans un dossier plus haut.

# draft
query_nothing_opens = rien n’ouvre { $place } : un dossier s’écrit avec son chemin complet

# draft
query_nothing_to_save = le panneau n’affiche rien à enregistrer

# draft
query_only_folders_below = le panneau n’affiche aucun dossier : seuls les sous-dossiers d’un dossier peuvent être recherchés

# draft
query_only_in_bytes = seul { $key } accepte des octets qui ne sont pas de l’UTF-8 (\xNN)

# draft
query_period_last_month = Le mois dernier

# draft
query_period_last_week = La semaine dernière

# draft
query_period_last_year = L’année dernière

# draft
query_period_older = Il y a plus d’un an

# draft
query_period_this_month = Ce mois-ci

# draft
query_period_this_week = Cette semaine

# draft
query_period_this_year = Cette année

# draft
query_period_today = Aujourd’hui

# draft
query_period_yesterday = Hier

# draft
query_place_changed = Filtre de { $name } modifié

# draft
query_place_moved = { $name } déplacé

# draft
query_place_named = { $place } · { $filter }

# draft
query_place_needs_filter = un emplacement enregistré a besoin d’un filtre

# draft
query_place_not_below = un emplacement est recherché dans ce qu’il liste, jamais en dessous

# draft
query_place_not_saved = Ceci n’est pas enregistré

# draft
query_place_removed = { $name } retiré de la section Enregistrés

# draft
query_place_renamed = { $name } renommé en { $new }

# draft
query_place_saved = { $name } enregistré

# draft
query_place_saved_already = C’est déjà enregistré sous { $name }

# draft
query_plocate_database = la base de données de plocate

# draft
query_plugin_description = La recherche comme emplacement : par nom, taille, date, étiquette, type MIME et contenu ; recherches enregistrées dans la barre latérale

# draft
query_plugin_title = Recherche

# draft
query_rating_five = Cinq étoiles

# draft
query_rating_none = Non noté

# draft
query_rating_one = Une étoile ou plus

# draft
query_rating_three = Trois étoiles ou plus

# draft
query_reading = { $subject }{ $size }{ $named }{ $tagged }{ $year }{ $period }

# draft
query_reading_archives = { $at ->
        [first] Archives
       *[after] archives
    }

# draft
query_reading_audio = { $at ->
        [first] Musique et audio
       *[after] musique et audio
    }

# draft
query_reading_documents = { $at ->
        [first] Documents
       *[after] documents
    }

# draft
query_reading_empty = {" "}vides

# draft
query_reading_extension = { $ext }

# draft
query_reading_files = Fichiers

# draft
query_reading_folders = { $at ->
        [first] Dossiers
       *[after] dossiers
    }

# draft
query_reading_fonts = { $at ->
        [first] Polices
       *[after] polices
    }

# draft
query_reading_images = { $at ->
        [first] Images
       *[after] images
    }

# draft
query_reading_modified = { $of ->
        [extension] {" "}modifiés { $when }
        [folder] {" "}modifiés { $when }
        [image] {" "}modifiées { $when }
        [video] {" "}modifiées { $when }
        [pictures] {" "}modifiées { $when }
        [audio] {" "}modifiés { $when }
        [document] {" "}modifiés { $when }
        [spreadsheet] {" "}modifiés { $when }
        [presentation] {" "}modifiées { $when }
        [text] {" "}modifiés { $when }
        [archive] {" "}modifiées { $when }
        [font] {" "}modifiées { $when }
        [program] {" "}modifiés { $when }
        [several] {" "}modifiés { $when }
       *[file] {" "}modifiés { $when }
    }

# draft
query_reading_named = {" "}au nom contenant { $names }

# draft
query_reading_over_1 = {" "}de plus de 1 { $unit }

# draft
query_reading_over_100 = {" "}de plus de 100 { $unit }

# draft
query_reading_presentations = { $at ->
        [first] Présentations
       *[after] présentations
    }

# draft
query_reading_programs = { $at ->
        [first] Programmes
       *[after] programmes
    }

# draft
query_reading_spreadsheets = { $at ->
        [first] Classeurs
       *[after] classeurs
    }

# draft
query_reading_tagged = {" "}avec { $count ->
        [one] l’étiquette { $tags }
       *[other] les étiquettes { $tags }
    }

# draft
query_reading_taken = { $of ->
        [extension] {" "}pris { $when }
        [folder] {" "}pris { $when }
        [image] {" "}prises { $when }
        [video] {" "}tournées { $when }
        [pictures] {" "}prises { $when }
        [audio] {" "}pris { $when }
        [document] {" "}pris { $when }
        [spreadsheet] {" "}pris { $when }
        [presentation] {" "}prises { $when }
        [text] {" "}pris { $when }
        [archive] {" "}prises { $when }
        [font] {" "}prises { $when }
        [program] {" "}pris { $when }
        [several] {" "}pris { $when }
       *[file] {" "}pris { $when }
    }

# draft
query_reading_text = { $at ->
        [first] Texte et code
       *[after] texte et code
    }

# draft
query_reading_under_100 = {" "}de moins de 100 { $unit }

# draft
query_reading_videos = { $at ->
        [first] Vidéos
       *[after] vidéos
    }

# draft
query_reading_when_last_month = le mois dernier

# draft
query_reading_when_last_week = la semaine dernière

# draft
query_reading_when_last_year = l’année dernière

# draft
query_reading_when_older = il y a plus d’un an

# draft
query_reading_when_this_month = ce mois-ci

# draft
query_reading_when_this_week = cette semaine

# draft
query_reading_when_this_year = cette année

# draft
query_reading_when_today = aujourd’hui

# draft
query_reading_when_year = en { $year }

# draft
query_reading_when_yesterday = hier

# draft
query_ref_brackets = […] l’un de ceux-ci

# draft
query_ref_content = Fichiers contenant ces mots

# draft
query_ref_content_regex = Fichiers contenant ce que trouve un motif

# draft
query_ref_contents = Contenu

# draft
query_ref_date = Une année, un mois ou un jour : 2026, 2026-05-14

# draft
query_ref_date_range = Du début de la première date à la fin de la seconde

# draft
query_ref_duration = Dans les 7 derniers jours : { $m } minutes, { $h } heures, { $d } jours, { $w } semaines, { $mo } mois, { $y } ans

# draft
query_ref_flags = { $i } : majuscules et minuscules confondues ; { $c } : majuscules et minuscules distinguées

# draft
query_ref_key = { $means } : { $example }

# draft
query_ref_line = ^ et $ sont le début et la fin d’une ligne.

# draft
query_ref_minus = Pas ceci : -draft, -#tag, -kind:image

# draft
query_ref_names = Noms

# draft
query_ref_not = Négation : NOT a, aussi écrit -a

# draft
query_ref_older = Il y a plus d’un an

# draft
query_ref_options = Où et comment chercher

# draft
query_ref_or = L’un ou l’autre : a OR b

# draft
query_ref_parens = Regroupés : (a OR b) c

# draft
query_ref_period = Une période : { $periods }

# draft
query_ref_question = ? un caractère

# draft
query_ref_quotes = Tel quel : {"\u0022"}deux mots{"\u0022"}, {"\u0022"}OR{"\u0022"}, {"\u0022"}#3{"\u0022"}

# draft
query_ref_rating = Au moins (> < >= <=) ; rating:0 trouve les fichiers sans note

# draft
query_ref_size = Plus de 10 { $mb }. Les unités { $decimal } valent chacune 1 000 fois la précédente ; les unités { $binary } valent chacune 1 024 fois la précédente, tout comme { $alone } écrites seules.

# draft
query_ref_size_range = D’une taille à l’autre, toutes deux comprises

# draft
query_ref_slash = Une barre oblique elle-même : seule, elle termine le motif

# draft
query_ref_star = * des caractères quelconques

# draft
query_ref_terms = Termes

# draft
query_ref_title = Recherche

# draft
query_ref_together = Ensemble (côte à côte : les deux à la fois)

# draft
query_ref_values = Valeurs

# draft
query_ref_word = Les noms qui le contiennent, majuscules ou non

# draft
query_reference = Dire ce qu’accepte une recherche

# draft
query_reference_description = Ce qu’accepte le texte d’une recherche (ADR-137) : ses termes et options avec le sens de chacun et des exemples, comment les termes se combinent, les valeurs, les jokers des noms et ce que lit `content:`, signes des regex compris ; chaque élément tel qu’il s’écrit, ce qu’il dit et ce qu’écrit son choix

# draft
query_refine = Affiner la recherche

# draft
query_refine_description = Modifier une facette de la recherche du panneau (ou en lancer une dans son dossier, son filtre ajouté) et afficher le résultat : un changement de son seul filtre à sa place, tout autre comme une étape qu’annule Précédent ; son dernier terme retiré, le panneau affiche l’emplacement où elle cherchait

# draft
query_remember_setting = Retenir les recherches récentes

# draft
query_remember_setting_description = La barre d’adresse propose les recherches lancées récemment depuis elle, gardées dans le cache ; une fois désactivé, elles sont oubliées.

# draft
query_remove_part = Retirer

# draft
query_rename = Renommer une recherche enregistrée

# draft
query_rename_accept = Renommer

# draft
query_rename_description = Modifier le nom qu’affiche une recherche enregistrée dans la barre latérale

# draft
query_rename_place_placeholder = Nom (vide : tiré du filtre)

# draft
query_rename_place_title = Renommer

# draft
query_rename_search_placeholder = Nom (vide : tiré de la recherche)

# draft
query_rename_search_title = Renommer la recherche enregistrée

# draft
query_requirement_plocate = Recherches instantanées par nom sur des disques entiers (sinon, les dossiers sont parcourus)

# draft
query_root_not_absolute = la racine de recherche { $path } n’est pas un chemin absolu

# draft
query_root_not_folder = la racine de recherche { $path } n’est pas un dossier : { $error }

# draft
query_root_not_path = `root` doit être un chemin

# draft
query_save = Enregistrer la recherche

# draft
query_save_description = Garder une recherche dans la section Enregistrés de la barre latérale, ou avec `at` un emplacement et ce qui le filtre, affiché ainsi à son ouverture (ADR-109) ; ce qu’affiche le panneau quand `expr` est omis

# draft
query_saved_not_changed = Les recherches enregistrées n’ont pas été modifiées : { $error }

# draft
query_saved_not_written = les recherches enregistrées n’ont pas été écrites

# draft
query_saved_setting = Recherches enregistrées affichées dans la barre latérale, dans l’ordre : { $shape }

# draft
query_saved_unreadable = les recherches enregistrées du fichier de paramètres sont illisibles ; rien n’a été écrit

# draft
query_search = Rechercher

# draft
query_search_changed = Recherche enregistrée { $name } modifiée

# draft
query_search_description = Rechercher et afficher le résultat dans le panneau, le filtre du panneau ajouté sauf avec `as_is` (ADR-107 : les termes d’un filtre ou d’une recherche que la liste peut trancher, gardés quand le panneau va ailleurs jusqu’à ce qu’on les retire ; `entries` liste ensuite ce qui a été trouvé ; une recherche que le panneau affiche déjà est laissée telle quelle, `refresh` la liste à nouveau) : les mots correspondent aux noms ; `name:`, `ext:pdf,md`, `kind:image,video` (document, text, audio, archive…), `type:dir`, `tag:x`, `content:words` ou `content:/regex/`, `comment:`, `size>10M`, `modified<7d` (aussi `today`, `this-week`, `2026-05` ; `created`, `accessed`, et `taken` pour l’horloge de l’appareil photo), `rating>=4`, `near:lat,lon~5km` ; `OR`, `NOT` ou `-`, parenthèses ; `hidden:yes` cherche dans les dossiers cachés, `excluded:yes` dans les dossiers que les recherches écartent (Paramètres › Général › Recherches : `node_modules`, caches ; jamais une corbeille ni un dossier réseau), `depth:2` en limite la profondeur. `root` (ou `in:`) est l’endroit où elle cherche

# draft
query_search_here = Rechercher ici…

# draft
query_search_moved = Recherche enregistrée { $name } déplacée

# draft
query_search_not_saved = Cette recherche n’est pas enregistrée

# draft
query_search_removed = Recherche enregistrée { $name } retirée

# draft
query_search_renamed = Recherche enregistrée { $name } renommée en { $new }

# draft
query_search_saved = Recherche { $name } enregistrée

# draft
query_search_saved_already = Cette recherche est déjà enregistrée sous { $name }

# draft
query_search_taken = cette recherche est déjà enregistrée sous { $name }

# draft
query_similar = Trouver des fichiers similaires

# draft
query_similar_day = Même jour

# draft
query_similar_description = Rechercher des fichiers semblables à celui qui est sélectionné : du même genre, modifiés le même jour, d’une taille proche ou partageant une étiquette, dans le dossier recherché ou affiché

# draft
query_similar_kind = Même genre

# draft
query_similar_no_date = { $name } n’a pas de date de modification

# draft
query_similar_no_kind = { $name } n’est d’aucun genre que connaît le bureau

# draft
query_similar_no_size = { $name } n’a pas de taille

# draft
query_similar_no_tags = { $name } n’a pas d’étiquette

# draft
query_similar_one = sélectionnez un seul fichier pour en trouver de semblables

# draft
query_similar_size = Taille proche

# draft
query_similar_tags = Mêmes étiquettes

# draft
query_similar_unreadable = { $name } ne peut pas être lu

# draft
query_size_1_to_100 = De 1 à 100 { $unit }

# draft
query_size_empty = Vide

# draft
query_size_over_1 = Plus de 1 { $unit }

# draft
query_size_over_100 = Plus de 100 { $unit }

# draft
query_size_placeholder = Une taille ou une plage : >10{ $m }, 1{ $m }..5{ $m }, <500{ $k }

# draft
query_size_under_1 = Moins de 1 { $unit }

# draft
query_size_under_100 = Moins de 100 { $unit }

# draft
query_tag_all_of = Tout { $tag }

# draft
query_tag_choices = Étiquettes pour affiner

# draft
query_tag_choices_description = Le sélecteur de la pastille Étiquette : les étiquettes des fichiers du panneau, lues sur eux, chacune affinant la recherche (ou ce que nomme `refines` : le `narrowing` ou le `filter` du dossier)

# draft
query_tag_files = { $count ->
        [one] { $count } fichier
       *[other] { $count } fichiers
    }

# draft
query_tag_section_other = Autres

# draft
query_tagged = Avec l’étiquette

# draft
query_tagged_instead = Avec cette étiquette, plutôt

# draft
query_tagged_menu = Avec l’étiquette…

# draft
query_term_contains = Contient

# draft
query_term_kind = Genre

# draft
query_term_modified = Modifié

# draft
query_term_size = Taille

# draft
query_term_tag = Étiquette

# draft
query_term_taken = Prise de vue

# draft
query_text_in_file = Texte dans le fichier…

# draft
query_this_folder_only = Ce dossier uniquement

# draft
query_tokens = Morceaux d’une recherche

# draft
query_tokens_description = { $shape } : les morceaux du texte d’une recherche tels que l’analyseur les lit (word, key, value, option, operator, ou une erreur avec le message de l’analyseur), en plages d’octets ; un terme encore en cours de frappe à `caret` n’est pas encore une erreur

# draft
query_unexpected_paren = `)` inattendue

# draft
query_unkeep_place = Retirer des Enregistrés

# draft
query_unkeep_search = Retirer la recherche enregistrée

# draft
query_unknown_flag = { $key } modificateur de regex inconnu `{ $flag }` ({ $i } : majuscules et minuscules confondues, { $c } : distinguées ; mettez entre guillemets un texte qui commence par /)

# draft
query_unterminated_quote = guillemet non fermé

# draft
query_value_dir = Dossier

# draft
query_value_false = Faux

# draft
query_value_file = Fichier

# draft
query_value_insensitive = Insensible à la casse

# draft
query_value_sensitive = Sensible à la casse

# draft
query_value_symlink = Lien symbolique

# draft
query_value_true = Vrai

# draft
query_waits_contents = ce que contiennent les fichiers

# draft
query_waits_places = où les images ont été prises

# draft
query_waits_taken = quand les images ont été prises

# draft
query_when_placeholder = Une date, une plage ou une période : 2026-05, 2026-01-01..2026-03-31, <30d

# draft
query_where_excluded = exclus

# draft
query_where_excluded_files = Fichiers et dossiers exclus

# draft
query_where_hidden = fichiers cachés

# draft
query_where_hidden_excluded = fichiers cachés, exclus

# draft
query_where_hidden_files = Fichiers cachés

# draft
query_where_subfolders = Ce dossier et ses sous-dossiers

# draft
query_which_saved = quelle recherche enregistrée ? Passez son `expr` ou son `name`
