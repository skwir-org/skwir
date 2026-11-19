# French catalog of skwir-plugin-print, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
print_alone = Chaque PDF au nom de son fichier, dans ce dossier

# draft
print_already_saved = { $what ->
        [file] { $name } était déjà enregistré sous { $pdf } dans { $folder }
        [files] { $name } et { $more } de plus étaient déjà enregistrés sous { $pdf } dans { $folder }
        [sheet] { $count ->
            [one] une planche contact de { $count } fichier était déjà enregistrée sous { $pdf } dans { $folder }
           *[other] une planche contact de { $count } fichiers était déjà enregistrée sous { $pdf } dans { $folder }
        }
       *[listing] la liste de { $name } était déjà enregistrée sous { $pdf } dans { $folder }
    }

# draft
print_already_there = { $name } existe déjà

# draft
print_as_shown = Tel qu’affiché

# draft
print_as_shown_detail = Chaque page sur une feuille

# draft
print_beside = { $count ->
        [1] À côté du fichier
        [one] À côté des fichiers
       *[other] À côté des fichiers
    }

# draft
print_beside_each = À côté de chaque fichier ; un PDF unique de tous à côté du premier

# draft
print_booklet = Livret

# draft
print_booklet_detail = Pages recto verso, pliées

# draft
print_booklet_needs_pdfium = Mettre en livret nécessite PDFium, introuvable ici

# draft
print_booklet_one_side = Un livret s’imprime recto verso : en recto seul, imprimez les faces impaires, retournez les feuilles, puis imprimez les paires

# draft
print_bound_right = Reliure à droite

# draft
print_bound_right_help = Pour un livre qui se lit de droite à gauche

# draft
print_cancelled = L’impression a été annulée

# draft
print_cancelled_save = Annulé : rien n’a été enregistré en PDF

# draft
print_chosen_before = Choisis précédemment

# draft
print_closing = Skwir se ferme

# draft
print_column_deleted = Supprimé le

# draft
print_column_folder = Dossier

# draft
print_column_modified = Modifié le

# draft
print_column_name = Nom

# draft
print_column_size = Taille

# draft
print_column_type = Type

# draft
print_contact_sheet = Planche contact

# draft
print_contact_sheet_detail = Les images, plusieurs par page

# draft
print_contact_sheet_title = Planche contact

# draft
print_copies_only = { $name } n’accepte que des copies : aucun PDF n’y est créé

# draft
print_count_needs_reader = Compter les pages d’un PDF nécessite un lecteur de documents, introuvable ici

# draft
print_dialog_accept = Imprimer…

# draft
print_eight_sheets = 8 feuilles par cahier

# draft
print_every_sheet = Toutes les feuilles en un cahier

# draft
print_filtered = { $named }, filtré par { $filter }

# draft
print_fold_three_times = Pliez chaque feuille en huit, la page 1 à l’extérieur et à l’endroit, coupez ses plis fermés, puis rangez les feuilles les unes à la suite des autres

# draft
print_fold_twice = Pliez chaque feuille en quatre, la page 1 à l’extérieur et à l’endroit, coupez son pli du haut, puis rangez les feuilles les unes à la suite des autres

# draft
print_folded_as = Pliage

# draft
print_folded_once = Pliée en deux

# draft
print_folded_three_times = Pliée en huit, coupée

# draft
print_folded_twice = Pliée en quatre, le haut coupé

# draft
print_folder_listed = Le dossier listé

# draft
print_folder_of = Le dossier concerné

# draft
print_font_sample = Voix ambiguë d’un cœur qui, au zéphyr, préfère les jattes de kiwis 0123456789

# draft
print_four_sheets = 4 feuilles par cahier

# draft
print_handed = Impression de { $pages ->
        [0] {""}
        [one] { $pages } page de{" "}
       *[other] { $pages } pages de{" "}
    }{ $what ->
        [file] { $name }
        [files] { $count ->
            [one] { $count } fichier
           *[other] { $count } fichiers
        }
        [sheet] la planche contact
       *[listing] la liste de { $name }
    } confiée au service d’impression du bureau pour { $to ->
        [file] { $target }
        [printer] l’imprimante { $printer }
       *[system] l’imprimante
    }, sans confirmation de prise en charge

# draft
print_in_folder = Dans { $folder }

# draft
print_in_folder_listed = Dans le dossier listé

# draft
print_in_no_folder = Le fichier n’est dans aucun dossier

# draft
print_job_files = { $first } et { $more } de plus

# draft
print_joined_needs_pdfium = Imprimer plusieurs fichiers en un seul nécessite PDFium, introuvable ici

# draft
print_keeping = Conservation de { $name } pour l’imprimer

# draft
print_keeping_document = Conservation du PDF du document pour l’imprimer

# draft
print_kind_folder = Dossier

# draft
print_kind_place = Emplacement

# draft
print_layout = Mise en page

# draft
print_left_out = { $name } a été omis : { $error }

# draft
print_name_of_one = Un nom désigne un seul PDF : ces fichiers en font un chacun (`together` en fait un pour tous)

# draft
print_no_folder_of_its_own = Sans dossier propre

# draft
print_no_home = Enregistrer sous : il n’y a pas de dossier personnel pour « ~ »

# draft
print_no_listing_folder = Il n’y a pas de dossier où enregistrer la liste : choisissez-en un

# draft
print_no_longer_shown = { $name } n’est plus affiché, sa liste n’est donc pas connue

# draft
print_no_page_asked = Aucune de ses pages n’est parmi celles demandées

# draft
print_no_page_dialog = Aucune de ses pages n’est parmi celles que la boîte de dialogue d’impression a demandées

# draft
print_no_page_shown = Aucune de ses pages n’est affichée

# draft
print_no_paper = { $paper } n’est pas un format de papier : a4, letter, a5, a3 ou legal

# draft
print_no_picture = Aucun n’est une image

# draft
print_no_such_folder = Enregistrer sous : le dossier { $path } n’existe pas

# draft
print_no_such_pane = Ce panneau n’existe pas

# draft
print_no_whole_path = Enregistrer sous : { $typed } n’est pas un chemin complet : tapez-le depuis la racine, ou depuis ~

# draft
print_none_printable = Aucun ne peut être imprimé

# draft
print_not_a_folder = { $name } n’est pas un dossier

# draft
print_not_own_file = Le PDF créé n’est pas un fichier à part entière

# draft
print_not_pictures = { $count ->
        [1] { $name } n’est pas une image et a été omis
        [one] { $count } fichier n’est pas une image et a été omis
       *[other] { $count } fichiers ne sont pas des images et ont été omis
    }

# draft
print_not_placed = { $name } n’a pas pu être placé dans { $folder }

# draft
print_not_printed = { $what ->
        [file] { $name } n’a pas été imprimé
        [files] { $count ->
            [one] { $count } fichier n’a pas été imprimé
           *[other] { $count } fichiers n’ont pas été imprimés
        }
        [sheet] La planche contact n’a pas été imprimée
       *[listing] La liste de { $name } n’a pas été imprimée
    }

# draft
print_not_smaller = Le PDF n’a pas pu être allégé

# draft
print_not_there = { $name } est introuvable

# draft
print_nothing_printable = { $name } ne montre rien d’imprimable

# draft
print_nothing_saved = Rien n’est enregistré en PDF tant que vous n’avez pas choisi.

# draft
print_nothing_selected = Rien n’y est sélectionné

# draft
print_nothing_there = Il n’y a rien à imprimer

# draft
print_nothing_to_print = Rien à imprimer

# draft
print_notice_of = { $name } : { $notice }

# draft
print_notice_of_file = { $name } : { $notice }

# draft
print_one_booklet = Un seul livret

# draft
print_other_pane = L’autre panneau

# draft
print_page_alt = Page { $page }

# draft
print_page_count = { $count ->
        [one] { $count } page
       *[other] { $count } pages
    }

# draft
print_pages = Pages

# draft
print_pages_all = Toutes

# draft
print_pages_current = Page { $page }

# draft
print_pages_need_pdfium = Imprimer ses pages nécessite PDFium, introuvable ici

# draft
print_pages_ranges = Pages…

# draft
print_pages_refused = Pages à imprimer : { $why }

# draft
print_pages_selection = Sélection

# draft
print_pages_to_print = Pages à imprimer

# draft
print_paper = Papier

# draft
print_past = { $count ->
        [one] Il a { $count } page ; celles au-delà ont été omises
       *[other] Il a { $count } pages ; celles au-delà ont été omises
    }

# draft
print_pdf_each = Un PDF par fichier

# draft
print_pdf_one = Un seul PDF pour tous

# draft
print_pdfs = PDF

# draft
print_per_side = { $count ->
        [one] { $count } page par face : un livret en a 2, 4 ou 8
       *[other] { $count } pages par face : un livret en a 2, 4 ou 8
    }

# draft
print_per_side_choice = { $count } par face

# draft
print_per_side_field = Pages par face

# draft
print_place_files = { $count ->
        [one] { $count } fichier
       *[other] { $count } fichiers
    }

# draft
print_places = Emplacements

# draft
print_plugin_description = Imprimer ce que montre un aperçu, tel quel, en livret ou en planche contact, ou l’enregistrer en PDF

# draft
print_plugin_off = L’enregistrement en PDF nécessite le module Impression activé

# draft
print_plugin_title = Impression

# draft
print_print = Imprimer…

# draft
print_print_description = Imprimer des fichiers tels que leurs aperçus les montrent, en une seule tâche et dans leur ordre (`layout` : tels qu’affichés, un livret plié en cahiers de `signature` pages avec `trim`, une planche contact des images, la liste d’un panneau), les `pages` choisies ; sans `asked`, la fenêtre demande d’abord dans la boîte de dialogue de Skwir, puis celle du système choisit l’imprimante, le papier et le recto verso

# draft
print_print_listing = Imprimer la liste…

# draft
print_ranges_placeholder = À partir de 1 : 1-3, 7, 10- (jusqu’à la fin)

# draft
print_save = Enregistrer

# draft
print_save_as = Enregistrer sous

# draft
print_save_as_help = Un dossier depuis ~ ou la racine, puis le nom : Tab complète un dossier

# draft
print_save_as_refused = Enregistrer sous : { $why }

# draft
print_save_each = { $count ->
        [one] { $form ->
            [booklet] enregistrer { $count } fichier en livret
           *[pdf] enregistrer { $count } fichier en PDF
        }
       *[other] { $form ->
            [booklet] enregistrer chacun des { $count } fichiers en livret
           *[pdf] enregistrer chacun des { $count } fichiers en PDF
        }
    }

# draft
print_save_listing = enregistrer la liste de { $name } en PDF

# draft
print_save_one = { $form ->
        [booklet] enregistrer { $name } en livret
       *[pdf] enregistrer { $name } en PDF
    }

# draft
print_save_pdf = Enregistrer en PDF

# draft
print_save_pdf_description = Faire un PDF de chaque fichier tel que son aperçu le montre, à côté de lui ou dans `dest` (un seul pour tous avec `together`), sur le papier des paramètres régionaux du système, avec la mise en page et les pages que prend Imprimer… (plan)

# draft
print_save_sheet = { $count ->
        [one] enregistrer une planche contact de { $count } fichier en PDF
       *[other] enregistrer une planche contact de { $count } fichiers en PDF
    }

# draft
print_save_step = { $what ->
        [file] enregistrer { $name } sous { $pdf } dans { $folder }
        [files] enregistrer { $name } et { $more } de plus sous { $pdf } dans { $folder }
        [sheet] { $count ->
            [one] enregistrer une planche contact de { $count } fichier sous { $pdf } dans { $folder }
           *[other] enregistrer une planche contact de { $count } fichiers sous { $pdf } dans { $folder }
        }
       *[listing] enregistrer la liste de { $name } sous { $pdf } dans { $folder }
    }

# draft
print_save_together = { $count ->
        [one] { $form ->
            [booklet] enregistrer { $count } fichier en un seul livret
           *[pdf] enregistrer { $count } fichier en un seul PDF
        }
       *[other] { $form ->
            [booklet] enregistrer { $count } fichiers en un seul livret
           *[pdf] enregistrer { $count } fichiers en un seul PDF
        }
    }

# draft
print_save_verb = Enregistrer en PDF

# draft
print_saved_step = { $what ->
        [file] { $name } enregistré sous { $pdf } dans { $folder }
        [files] { $name } et { $more } de plus enregistrés sous { $pdf } dans { $folder }
        [sheet] { $count ->
            [one] une planche contact de { $count } fichier enregistrée sous { $pdf } dans { $folder }
           *[other] une planche contact de { $count } fichiers enregistrée sous { $pdf } dans { $folder }
        }
       *[listing] la liste de { $name } enregistrée sous { $pdf } dans { $folder }
    }

# draft
print_sent = Impression de { $pages ->
        [0] {""}
        [one] { $pages } page de{" "}
       *[other] { $pages } pages de{" "}
    }{ $what ->
        [file] { $name }
        [files] { $count ->
            [one] { $count } fichier
           *[other] { $count } fichiers
        }
        [sheet] la planche contact
       *[listing] la liste de { $name }
    } envoyée { $to ->
        [file] vers { $target }
        [printer] à l’imprimante { $printer }
       *[system] à l’imprimante
    }

# draft
print_sets_of_16 = Cahiers de 16 pages

# draft
print_sets_of_32 = Cahiers de 32 pages

# draft
print_sheets_in_fold = { $count ->
        [one] { $count } feuille, un cahier
       *[other] { $count } feuilles en un cahier
    }

# draft
print_shows_nothing = Le panneau n’affiche rien

# draft
print_smaller = Fichier allégé

# draft
print_smaller_folder = Un dossier où alléger le PDF

# draft
print_smaller_help = Images à 150 dpi au plus, comme Écran dans Réduire la taille

# draft
print_then = { $said }. { $then }

# draft
print_title = { $what ->
        [file] Imprimer { $name }
        [files] { $count ->
            [one] Imprimer { $count } fichier
           *[other] Imprimer { $count } fichiers
        }
        [sheet] Imprimer une planche contact
       *[listing] Imprimer la liste de { $name }
    }

# draft
print_to = Destination

# draft
print_to_pdf = PDF

# draft
print_to_printer = Imprimante

# draft
print_trim = Rogner les marges blanches

# draft
print_trim_help = Pages ajustées à leur contenu, toutes à la même échelle

# draft
print_went_missing = { $name } a disparu pendant sa création

# draft
print_work_folder = Un dossier où créer le document

# draft
print_wrong_size = une image de mauvaise taille
