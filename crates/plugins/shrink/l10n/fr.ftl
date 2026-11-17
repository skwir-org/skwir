# French catalog of skwir-plugin-shrink, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
shrink_already = { $copy } était déjà une copie allégée de { $file }

# draft
shrink_already_there = { $name } existe déjà

# draft
shrink_as_small = { $name } est déjà aussi léger que possible : aucune copie n’a été faite

# draft
shrink_at_most = Taille maximale

# draft
shrink_at_most_placeholder = Sans limite, ou une taille : 2 Mo, 500 ko

# draft
shrink_at_most_refused = Taille maximale : { $why }

# draft
shrink_both = { $pixels ->
        [one] Images des PDF à { $dpi } dpi au plus ; images à { $pixels } pixel
       *[other] Images des PDF à { $dpi } dpi au plus ; images à { $pixels } pixels
    }

# draft
shrink_cancelled = La création de la copie a été annulée

# draft
shrink_clash_cancelled = Annulé : aucune copie allégée n’a été faite

# draft
shrink_closing = Skwir se ferme

# draft
shrink_dialog_accept = { $count ->
        [1] Faire la copie
        [one] Faire les copies
       *[other] Faire les copies
    }

# draft
shrink_dialog_intro = { $count ->
        [1] Une copie allégée est créée à côté ; le fichier reste tel quel.
        [one] Une copie allégée est créée à côté de chaque fichier ; les fichiers restent tels quels.
       *[other] Une copie allégée est créée à côté de chaque fichier ; les fichiers restent tels quels.
    }

# draft
shrink_dialog_title = { $count ->
        [1] Réduire la taille de { $name }
        [one] Réduire la taille de { $count } fichier
       *[other] Réduire la taille de { $count } fichiers
    }

# draft
shrink_gray = En gris

# draft
shrink_gray_help = En niveaux de gris

# draft
shrink_gray_help_pdfs = Les images, et celles des PDF, en niveaux de gris

# draft
shrink_in_place_only = { $name } n’accepte que des copies écrites sur place : aucune copie allégée n’y est faite

# draft
shrink_lossless = Sans perte

# draft
shrink_lossless_at_most = La qualité lossless ne réencode rien, elle ne peut donc pas ramener une copie sous une taille : demandez `at_most` avec print, screen ou small

# draft
shrink_lossless_jpeg = Sans perte laisse un JPEG tel quel : choisissez Impression, Écran ou Petite taille, ou retirez sa position et les données de l’appareil photo

# draft
shrink_made = { $copy } créé, copie allégée de { $file }

# draft
shrink_make = créer { $copy }, copie allégée de { $file }

# draft
shrink_make_verb = Faire une copie allégée

# draft
shrink_no_folder = Le fichier n’est dans aucun dossier

# draft
shrink_no_size = « { $typed } » n’est pas une taille

# draft
shrink_no_unit = « { $unit } » n’est pas une unité : écrivez ko ou Mo

# draft
shrink_none_made_note = Aucune copie allégée de { $name } n’a été faite : { $said }

# draft
shrink_none_made_strip = Aucune copie allégée de { $name } n’a été faite : sa position et les données de l’appareil photo n’ont donc pas été retirées non plus

# draft
shrink_not_a_folder = { $name } n’est pas un dossier

# draft
shrink_not_a_picture = { $name } n’est ni une image JPEG ou PNG, ni un PDF

# draft
shrink_not_placed = { $name } n’a pas pu être placé dans { $folder }

# draft
shrink_not_under = { $name } n’a pas pu passer sous la taille demandée : sa plus petite copie fait { $size }

# draft
shrink_note = { $name } : { $said }

# draft
shrink_nothing = Rien à alléger

# draft
shrink_nothing_made = Aucune copie allégée n’est faite tant que vous n’avez pas choisi.

# draft
shrink_nothing_re_encoded = Aucun réencodage

# draft
shrink_pdf_dpi = Images à { $dpi } dpi au plus

# draft
shrink_picture_pixels = { $pixels ->
        [one] Au plus { $pixels } pixel de côté
       *[other] Au plus { $pixels } pixels de côté
    }

# draft
shrink_plan = { $count ->
        [1] faire une copie allégée de { $name }
        [one] faire des copies allégées de { $count } fichier
       *[other] faire des copies allégées de { $count } fichiers
    }

# draft
shrink_plugin_description = Faire des copies allégées d’images et de PDF à côté d’eux : Réduire la taille…, dans Compresser et dans le menu d’un aperçu

# draft
shrink_plugin_off = Les copies allégées nécessitent le module activé

# draft
shrink_plugin_title = Copies allégées

# draft
shrink_print = Impression

# draft
shrink_quality = Qualité

# draft
shrink_reduce = Faire des copies allégées

# draft
shrink_reduce_ask_description = Demander à quel point alléger, puis faire une copie allégée de chaque image et PDF à côté de lui, comme le fait `shrink.reduce`

# draft
shrink_reduce_description = Faire une copie allégée de chaque image (JPEG, PNG) et PDF à côté de lui, ou dans `dest`, nommée `name (reduced).ext` : `quality` lossless, print (images de 3840 pixels de côté, images d’un PDF à 300 dpi), screen (1920, 150 dpi) ou small (1280, 96 dpi) ; `gray` ; `strip`, la position et les données de l’appareil photo d’une image retirées ; `at_most` une taille que chaque copie respecte, des paliers plus petits essayés. Une copie qui ne serait pas plus petite n’est pas faite (plan)

# draft
shrink_reduce_size = Réduire la taille…

# draft
shrink_requirement = Création de copies allégées

# draft
shrink_same_pixels = Mêmes pixels, rien de perdu

# draft
shrink_screen = Écran

# draft
shrink_small = Petite taille

# draft
shrink_strip = Retirer la position et les données de l’appareil photo

# draft
shrink_strip_help = La date de prise de vue reste

# draft
shrink_went_missing = { $name } a disparu pendant sa création

# draft
shrink_work_folder = Un dossier où faire la copie
