# French catalog of skwir-plugin-documents, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
documents_detail_application = Application

# draft
documents_detail_author = Auteur

# draft
documents_detail_keywords = Mots-clés

# draft
documents_detail_pages = Pages

# draft
documents_detail_subject = Sujet

# draft
documents_detail_title = Titre

# draft
documents_detail_words = Mots

# draft
documents_docx_no_body = Ce document Word n’a pas de corps

# draft
documents_docx_no_text = Ce n’est pas un document Word : il n’a pas de partie texte

# draft
documents_engine = Moteur de documents

# draft
documents_engine_auto = Automatique

# draft
documents_engine_description = Automatique préfère PDFium et se rabat sur Poppler ; MuPDF n’est utilisé que s’il est choisi.

# draft
documents_first_blocks = { $count ->
        [1] Seul le premier bloc est affiché
        [one] Seul le premier bloc est affiché
       *[other] Seuls les { $count } premiers blocs sont affichés
    }

# draft
documents_first_blocks_of_part = { $count ->
        [1] Seul le premier bloc de cette partie est affiché
        [one] Seul le premier bloc de cette partie est affiché
       *[other] Seuls les { $count } premiers blocs de cette partie sont affichés
    }

# draft
documents_members = { $count ->
       *[other] Le document contient plus de { $count } parties
    }

# draft
documents_missing_manifest = Le manifeste du document est introuvable

# draft
documents_missing_notes = Les notes du document sont introuvables

# draft
documents_missing_numbering = La numérotation du document est introuvable

# draft
documents_missing_picture = Une image incorporée est introuvable

# draft
documents_missing_properties = Les propriétés du document sont introuvables

# draft
documents_missing_relationships = Les relations du document sont introuvables

# draft
documents_missing_styles = Les styles du document sont introuvables

# draft
documents_missing_text = Le texte du document est introuvable

# draft
documents_missing_thumbnail = La vignette est introuvable

# draft
documents_no_reader = Aucun programme installé ne lit ce document comme du texte

# draft
documents_not_its_picture = pas une image de ce document

# draft
documents_odt_no_content = Ce n’est pas un texte OpenDocument : il n’a pas de contenu

# draft
documents_odt_not_text = Ce fichier OpenDocument n’est pas un document texte

# draft
documents_odt_password = Ce document est protégé par un mot de passe

# draft
documents_part_of = Partie { $part } sur { $parts }

# draft
documents_parts_keys = [ et ] passent d’une partie à l’autre

# draft
documents_pdfium_library = Bibliothèque PDFium

# draft
documents_pdfium_library_description = Chemin de la bibliothèque partagée PDFium.

# draft
documents_pdfium_library_description_more = Laissez vide pour la chercher aux emplacements habituels.

# draft
documents_plugin_description = Lire les documents PDF et bureautiques : défilement continu, recherche, sélection de texte, liens et sommaire ; les textes Word et OpenDocument aussi comme un texte ajusté au volet

# draft
documents_plugin_title = Documents

# draft
documents_read_by_pandoc = Lu par Pandoc · la mise en page, les en-têtes et les commentaires ne sont pas affichés

# draft
documents_reading_mode = Mode lecture · la mise en page, les en-têtes et les commentaires ne sont pas affichés

# draft
documents_requirement_libreoffice = Documents bureautiques affichés en pages

# draft
documents_requirement_mupdf = Lecture des PDF avec MuPDF

# draft
documents_requirement_pandoc = Documents texte lus par Pandoc, et fichiers RTF lus comme du texte

# draft
documents_requirement_pdfium = Lecture des PDF avec PDFium

# draft
documents_requirement_poppler = Lecture des PDF avec Poppler

# draft
documents_requirement_sandbox = Isolation de LibreOffice, qui en a besoin, et de Pandoc

# draft
documents_start_only = Seul le début est affiché

# draft
documents_text_documents = Documents texte

# draft
documents_text_documents_description = Comment les documents Word, OpenDocument et RTF sont affichés.

# draft
documents_text_documents_description_more = Lecture montre aussitôt leur texte, leurs titres, listes, tableaux, notes et images, ajustés au volet : Skwir lit les fichiers Word et OpenDocument, Pandoc les RTF. Pages les met en page comme LibreOffice les imprime, ce qui prend quelques secondes pour chacun. Pandoc les fait tous lire par Pandoc. Quand ce dont un choix a besoin n’est pas installé, un autre est utilisé.

# draft
documents_text_documents_pages = Pages

# draft
documents_text_documents_reading = Lecture

# draft
documents_too_large_manifest = Le manifeste du document dépasse la limite d’aperçu de { $size }

# draft
documents_too_large_notes = Les notes du document dépassent la limite d’aperçu de { $size }

# draft
documents_too_large_numbering = La numérotation du document dépasse la limite d’aperçu de { $size }

# draft
documents_too_large_picture = Une image incorporée dépasse la limite d’aperçu de { $size }

# draft
documents_too_large_properties = Les propriétés du document dépassent la limite d’aperçu de { $size }

# draft
documents_too_large_relationships = Les relations du document dépassent la limite d’aperçu de { $size }

# draft
documents_too_large_styles = Les styles du document dépassent la limite d’aperçu de { $size }

# draft
documents_too_large_text = Le texte du document dépasse la limite d’aperçu de { $size }

# draft
documents_too_large_thumbnail = La vignette dépasse la limite d’aperçu de { $size }
