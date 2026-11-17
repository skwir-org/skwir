# French catalog of skwir-plugin-rename-bulk, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
rename_bulk_bad_date_format = format de date incorrect `{ $format }` : { $why }

# draft
rename_bulk_bad_width = largeur de compteur incorrecte `{ $width }`

# draft
rename_bulk_brace = Une accolade elle-même ({ $open } et { $close })

# draft
rename_bulk_cancelled = annulé

# draft
rename_bulk_command = Renommer en lot d’après un modèle

# draft
rename_bulk_command_description = Renommer la sélection, ou les éléments donnés, d’après un modèle, une regex et une règle de casse (plan)

# draft
rename_bulk_conflict = { $item } deviendrait { $name }, qui existe déjà

# draft
rename_bulk_counter = Compteur

# draft
rename_bulk_counter_overflows = le compteur déborde

# draft
rename_bulk_counter_too_wide = { $width ->
        [one] un compteur de { $width } chiffre de large dépasse la longueur possible d’un nom ({ $most } au plus)
       *[other] un compteur de { $width } chiffres de large dépasse la longueur possible d’un nom ({ $most } au plus)
    }

# draft
rename_bulk_counts = Compte à partir de Début, un de plus pour chaque élément, dans l’ordre affiché

# draft
rename_bulk_date_note = Dans { $placeholder } : %Y l’année, %m le mois, %d le jour, %H l’heure, %M la minute, %S la seconde, %B le nom du mois (%b abrégé), %A celui du jour, %j le jour de l’année. Dans le fuseau horaire de ce système.

# draft
rename_bulk_day = La date

# draft
rename_bulk_day_and_time = La date et l’heure

# draft
rename_bulk_day_first = La date, le jour en premier

# draft
rename_bulk_day_without_dashes = La date, sans tirets

# draft
rename_bulk_dollar = Un signe dollar

# draft
rename_bulk_duplicate = { $count ->
        [2] plusieurs éléments deviendraient { $name } : { $first }, { $second }
        [3] plusieurs éléments deviendraient { $name } : { $first }, { $second }, { $third }
       *[other] plusieurs éléments deviendraient { $name } : { $first }, { $second }, { $third } et { $more } autres
    }

# draft
rename_bulk_empty_replacement_note = Si ce champ est vide, ce que le motif a trouvé est retiré.

# draft
rename_bulk_extension = L’extension, sans son point

# draft
rename_bulk_extension_with_dot = L’extension avec son point ; rien s’il n’y en a pas

# draft
rename_bulk_first_group = Ce que contenait le premier groupe ($2, $3…)

# draft
rename_bulk_group_before_letter = De même, devant une lettre, un chiffre ou _

# draft
rename_bulk_group_name_note = { $written } se lit comme un groupe nommé { $read }, qui ne contient rien : écrivez { $meant }.

# draft
rename_bulk_invalid_name = { $item } deviendrait { $name }, un nom non valide : { $reason }

# draft
rename_bulk_invalid_regex = regex non valide : { $why }

# draft
rename_bulk_invalid_template = modèle non valide : { $why }

# draft
rename_bulk_menu = Renommer en lot…

# draft
rename_bulk_missing_date = { $item } n’a pas encore de date de modification (nécessaire pour { $placeholder })

# draft
rename_bulk_month_and_year = Le mois et l’année

# draft
rename_bulk_name_without_extension = Le nom sans son extension

# draft
rename_bulk_named_group = Ce que contenait le groupe nommé year

# draft
rename_bulk_no_location = le panneau n’a pas d’emplacement ouvert

# draft
rename_bulk_no_such_pane = panneau introuvable

# draft
rename_bulk_not_read_yet = variable `{ $placeholder }` pas encore lue

# draft
rename_bulk_not_text = { $item } n’est pas un nom fait de texte ; renommez-le seul

# draft
rename_bulk_nothing = rien à renommer

# draft
rename_bulk_parent = Le dossier qui le contient

# draft
rename_bulk_parts_note = La regex s’applique d’abord : voici le nom qu’elle laisse. La casse vient en dernier, et laisse l’extension telle quelle. Un dossier n’a pas d’extension.

# draft
rename_bulk_parts_of_the_name = Parties du nom

# draft
rename_bulk_plan = { $count ->
        [one] renommer { $count } élément
       *[other] renommer { $count } éléments
    }

# draft
rename_bulk_plugin_description = Renommer la sélection d’après un modèle, une regex et des règles de casse

# draft
rename_bulk_plugin_title = Renommage en lot

# draft
rename_bulk_preview = Prévisualiser un renommage en lot

# draft
rename_bulk_preview_description = Renvoyer les noms ancien → nouveau que produirait un renommage en lot (`renames`, les `limit` premiers s’il est donné ; chacun avec l’endroit de l’ancien nom où la regex a trouvé une correspondance, `matched`, et ce qu’elle a mis dans le nouveau, `put`, en plages d’octets), combien il y en a (`count`), et le nom du premier élément (`first`), le seul sur lequel portent les références de la regex et du remplacement

# draft
rename_bulk_recipe_brackets = Retirer ce qui est entre parenthèses ou crochets

# draft
rename_bulk_recipe_jpeg = Une seule extension pour les images JPEG → { $shown }

# draft
rename_bulk_recipe_numbers = Retirer les numéros du début

# draft
rename_bulk_recipe_one_space = Plusieurs espaces en un seul → un espace

# draft
rename_bulk_recipe_spaces = Espaces en tirets bas → { $shown }

# draft
rename_bulk_recipe_start = Retirer un début (IMG_)

# draft
rename_bulk_recipe_swap = Échanger deux parties autour de « - » → { $shown }

# draft
rename_bulk_recipe_underscores = Tirets bas en espaces → un espace

# draft
rename_bulk_recipes = Recettes

# draft
rename_bulk_reference = Dire ce que prend un champ du renommage en lot

# draft
rename_bulk_reference_description = Renvoyer ce que prend `field` (template, regex, replacement) : ses variables ou ses signes, chacun avec ce qu’il donne pour le premier des éléments donnés ou sélectionnés, les autres arguments du renommage étant tels que donnés, et des recettes qui remplissent la regex et son remplacement

# draft
rename_bulk_regex = Regex

# draft
rename_bulk_regex_groups_note = Ce que contenait un groupe s’écrit $1, $2… dans le remplacement ; ce que contenait un groupe nommé year, { $named }.

# draft
rename_bulk_regex_note = ^ et $ sont le début et la fin du nom. La regex porte sur le nom entier, extension comprise, et remplace chaque correspondance ; le modèle vient après elle.

# draft
rename_bulk_replacement = Remplacement

# draft
rename_bulk_signs = Signes

# draft
rename_bulk_template = Modèle

# draft
rename_bulk_three_digits = De même, trois chiffres au moins

# draft
rename_bulk_two_digits = De même, deux chiffres au moins

# draft
rename_bulk_unclosed = accolade `{ $brace }` non fermée dans `{ $template }`

# draft
rename_bulk_unknown_placeholder = variable inconnue `{ $placeholder }`

# draft
rename_bulk_what_a_match_becomes = Ce que devient une correspondance

# draft
rename_bulk_when_modified = Date de modification

# draft
rename_bulk_whole_match = La correspondance entière

# draft
rename_bulk_whole_name = Le nom entier
