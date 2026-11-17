# French catalog of skwir-plugin-archives, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
archives_already_extracted_into = Le contenu de { $archive } était déjà dans { $into }

# draft
archives_already_extracted_to = Le contenu de { $archive } était déjà extrait vers { $into }

# draft
archives_already_held = { $count ->
        [1] { $archive } contenait déjà { $name }
        [one] { $archive } contenait déjà { $count } élément
       *[other] { $archive } contenait déjà { $count } éléments
    }

# draft
archives_already_there = { $name } existe déjà

# draft
archives_among_inputs = { $name } fait partie de ce qui va dans l’archive

# draft
archives_ask_as = { $count ->
        [1] Compresser { $name } sous le nom
        [one] Compresser { $count } élément sous le nom
       *[other] Compresser { $count } éléments sous le nom
    }

# draft
archives_ask_as_note = Son extension indique le format : { $endings }

# draft
archives_ask_with_password = { $count ->
        [1] Compresser { $name } avec un mot de passe, sous le nom
        [one] Compresser { $count } élément avec un mot de passe, sous le nom
       *[other] Compresser { $count } éléments avec un mot de passe, sous le nom
    }

# draft
archives_ask_with_password_note = Une archive ZIP chiffrée : son mot de passe est demandé ensuite

# draft
archives_attributes_unread = les étiquettes et attributs de { $name } n’ont pas pu être lus

# draft
archives_cancelled_compress = Annulé : rien n’a été compressé

# draft
archives_cancelled_extract = Annulé : rien n’a été extrait

# draft
archives_cannot_list_whole = Impossible de lister { $name } en entier

# draft
archives_choose_format = Choisir le format…

# draft
archives_column_name = Nom

# draft
archives_column_size = Taille

# draft
archives_column_status = État

# draft
archives_compress = Compresser

# draft
archives_compress_as = { $count ->
        [1] compresser { $name } dans { $archive }
        [one] compresser { $count } élément dans { $archive }
       *[other] compresser { $count } éléments dans { $archive }
    }

# draft
archives_compress_ask = Compresser…

# draft
archives_compress_ask_description = Demander le nom d’une archive de la sélection, dont l’extension indique le format, puis la créer

# draft
archives_compress_button = Compresser

# draft
archives_compress_description = Créer une archive de la sélection, ou des éléments donnés, à côté d’eux ou dans `dest` : son format est celui que donne la fin de `name` (.zip, .7z, .tar.gz, .tar.xz, .tar.zst, .tar.bz2, .tar), sinon `format`, sinon ZIP (plan)

# draft
archives_compress_plan = { $count ->
        [1] compresser { $name }
        [one] compresser { $count } élément
       *[other] compresser { $count } éléments
    }

# draft
archives_compressed_as = { $count ->
        [1] { $name } compressé dans { $archive }
        [one] { $count } élément compressé dans { $archive }
       *[other] { $count } éléments compressés dans { $archive }
    }

# draft
archives_detail_encrypted = Chiffrés

# draft
archives_detail_files = Fichiers

# draft
archives_detail_folders = Dossiers

# draft
archives_detail_unpacked = Taille décompressée

# draft
archives_encrypted_members = { $count ->
        [one] { $count } élément
       *[other] { $count } éléments
    }

# draft
archives_endings = .zip, .7z, .tar.gz, .tar.xz, .tar.zst, .tar.bz2 ou .tar

# draft
archives_extract = Extraire ici

# draft
archives_extract_button = Extraire

# draft
archives_extract_description = Extraire chaque archive de la sélection, ou des éléments donnés, à côté d’elle ou dans `dest` : une archive d’un seul dossier ou fichier donne ce dossier ou ce fichier, une archive de plusieurs éléments un dossier nommé d’après elle (plan)

# draft
archives_extract_here = Extraire ici

# draft
archives_extract_into = extraire { $archive } dans { $into }

# draft
archives_extract_plan = { $count ->
        [1] extraire { $name }
        [one] extraire { $count } archive
       *[other] extraire { $count } archives
    }

# draft
archives_extract_to = extraire { $archive } vers { $into }

# draft
archives_extract_verb = Extraire

# draft
archives_extracted_into = { $archive } extrait dans { $into }

# draft
archives_extracted_to = { $archive } extrait vers { $into }

# draft
archives_first_members = { $count ->
        [1] Seul le premier élément est listé · ouvrez l’archive dans un gestionnaire d’archives pour voir le reste
        [one] Seul le premier élément est listé · ouvrez l’archive dans un gestionnaire d’archives pour voir le reste
       *[other] Seuls les { $count } premiers éléments sont listés · ouvrez l’archive dans un gestionnaire d’archives pour voir le reste
    }

# draft
archives_folder_not_there = le dossier où doit aller { $name } n’existe pas

# draft
archives_format_differs = { $name } indique { $said }, mais le format demandé est { $asked }

# draft
archives_holds_archive = { $there } contient { $archive }, l’archive à extraire

# draft
archives_holds_input = { $there } contient { $input }, qui va dans l’archive

# draft
archives_in_a_folder = une archive se trouve dans un dossier

# draft
archives_member_not_previewed = Aucun aperçu pour cet élément (dossier, lien ou fichier chiffré)

# draft
archives_no_format = { $name } n’indique aucun format : terminez son nom par { $endings }

# draft
archives_no_longer_there = { $name } n’existe plus

# draft
archives_not_a_folder = { $name } n’est pas un dossier

# draft
archives_not_a_folder_to_extract_into = { $name } n’est pas un dossier où extraire

# draft
archives_not_an_archive = { $name } n’est pas une archive

# draft
archives_not_in_archive = { $member } n’est plus dans { $archive }

# draft
archives_not_placed = { $name } n’a pas pu être placé dans { $folder }

# draft
archives_not_writable = Impossible d’écrire dans { $name } : rien n’y est extrait

# draft
archives_nothing_compressed = Rien n’est compressé tant que vous n’avez pas choisi.

# draft
archives_nothing_extracted = Rien n’est extrait tant que vous n’avez pas choisi.

# draft
archives_nothing_to_compress = Rien à compresser

# draft
archives_only_zip_password = Seule une archive ZIP peut avoir un mot de passe

# draft
archives_only_zip_password_end = Seule une archive ZIP peut avoir un mot de passe : terminez son nom par .zip

# draft
archives_password_again = Le même mot de passe une seconde fois, pour confirmer

# draft
archives_password_differed = Les deux mots de passe différaient : un mot de passe pour { $name }

# draft
archives_password_empty = Mot de passe de { $name } : il ne peut pas être vide

# draft
archives_password_for = Mot de passe de { $name }

# draft
archives_password_new = Un mot de passe pour { $name }

# draft
archives_password_new_empty = Un mot de passe pour { $name } : il ne peut pas être vide

# draft
archives_password_next = Suivant

# draft
archives_password_unasked = { $name } serait chiffré : son mot de passe est demandé dans la fenêtre de Skwir

# draft
archives_plugin_description = Lister le contenu des archives ZIP, tar, 7z et autres, afficher un aperçu de leurs éléments sans les extraire, les extraire (Extraire ici, dans le menu d’une archive et dans celui d’un dépôt) et en créer (Compresser, dans le menu de ce que vous sélectionnez)

# draft
archives_plugin_title = Archives

# draft
archives_requirement = Listage, lecture, extraction et création d’archives

# draft
archives_status_encrypted = Chiffré

# draft
archives_status_link = Lien / spécial

# draft
archives_through_link = { $name } est un lien : rien n’est extrait à travers lui

# draft
archives_title = { $count ->
        [one] Archive · { $count } élément
       *[other] Archive · { $count } éléments
    }

# draft
archives_to_zip = En ZIP

# draft
archives_too_deep = { $count ->
        [one] Le chemin dans l’archive dépasse { $count } niveau
       *[other] Le chemin dans l’archive dépasse { $count } niveaux
    }

# draft
archives_two_as_one = deux éléments seraient extraits sous le nom { $name }

# draft
archives_unasked = { $name } est chiffré : son mot de passe est demandé dans la fenêtre de Skwir

# draft
archives_unknown_format = « { $format } » n’est pas un format connu ici : indiquez { $endings }

# draft
archives_unreadable = { $name } n’a pas pu être lu

# draft
archives_went_missing = { $name } a disparu dès sa création

# draft
archives_with_password = Avec un mot de passe…

# draft
archives_would_leave = ce qui est extrait dans { $name } en sortirait

# draft
archives_wrong_password = Mot de passe incorrect pour { $name } : réessayez
