# French catalog of skwir-plugin-books, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
books_chapter = Chapitre { $number }

# draft
books_chapter_cut = Ce chapitre est tronqué : il contient plus que ce que lit le mode lecture

# draft
books_detail_author = Auteur

# draft
books_detail_chapters = Chapitres

# draft
books_detail_genre = Genre littéraire

# draft
books_detail_isbn = ISBN

# draft
books_detail_language = Langue

# draft
books_detail_published = Publié le

# draft
books_detail_publisher = Éditeur

# draft
books_detail_series = Série

# draft
books_detail_subject = Sujet

# draft
books_detail_title = Titre

# draft
books_detail_year = Année

# draft
books_engine = Moteur de livres

# draft
books_engine_auto = Automatique

# draft
books_engine_builtin = Intégré

# draft
books_engine_description = Automatique lit les livres avec le moteur intégré, qui recompose leur texte selon le thème et dessine les pages à mise en page fixe comme le livre les dispose.

# draft
books_engine_description_more = MuPDF met en pages, avec leurs propres styles, tous les livres EPUB, FB2 et MOBI qu’il lit bien ; le moteur intégré lit les autres.

# draft
books_epub_members = { $count ->
       *[other] Cet EPUB contient plus de { $count } fichiers
    }

# draft
books_epub_missing = Ressource EPUB manquante : { $href }

# draft
books_epub_no_package = Le conteneur EPUB ne désigne aucun paquetage

# draft
books_epub_no_pages = Cet EPUB n’a aucune page

# draft
books_epub_protected_chapter = Cet EPUB est protégé : un chapitre est chiffré

# draft
books_epub_protected_container = Cet EPUB est protégé : son fichier conteneur est chiffré

# draft
books_epub_protected_contents = Cet EPUB est protégé : son sommaire est chiffré

# draft
books_epub_protected_encryption = Cet EPUB est protégé : même sa liste des parties chiffrées est chiffrée

# draft
books_epub_protected_package = Cet EPUB est protégé : son paquetage est chiffré

# draft
books_epub_protected_page = Cet EPUB est protégé : une page est chiffrée

# draft
books_epub_protected_picture = Cet EPUB est protégé : une image incorporée est chiffrée

# draft
books_epub_protected_resource = Cet EPUB est protégé : une ressource qu’utilise un chapitre est chiffrée

# draft
books_epub_protected_stylesheet = Cet EPUB est protégé : une feuille de style est chiffrée

# draft
books_epub_too_large_chapter = Un chapitre dépasse la limite d’aperçu de { $size }

# draft
books_epub_too_large_container = Le conteneur de l’EPUB dépasse la limite d’aperçu de { $size }

# draft
books_epub_too_large_contents = Le sommaire de l’EPUB dépasse la limite d’aperçu de { $size }

# draft
books_epub_too_large_encryption = La liste des parties chiffrées de l’EPUB dépasse la limite d’aperçu de { $size }

# draft
books_epub_too_large_package = Le paquetage de l’EPUB dépasse la limite d’aperçu de { $size }

# draft
books_epub_too_large_page = Une page dépasse la limite d’aperçu de { $size }

# draft
books_epub_too_large_picture = Une image incorporée dépasse la limite d’aperçu de { $size }

# draft
books_epub_too_large_resource = Une ressource de l’EPUB dépasse la limite d’aperçu de { $size }

# draft
books_epub_too_large_stylesheet = Une feuille de style dépasse la limite d’aperçu de { $size }

# draft
books_epub_too_many_pages = Cet EPUB a trop de pages

# draft
books_fb2_damaged_picture = L’image { $id } de ce livre FB2 est endommagée

# draft
books_fb2_document_missing = Le document FB2 est introuvable

# draft
books_fb2_document_too_large = Le document FB2 dépasse la limite d’aperçu de { $size }

# draft
books_fb2_missing_chapter = Chapitre FB2 manquant : { $href }

# draft
books_fb2_missing_picture = Image FB2 manquante : { $id }

# draft
books_fb2_no_body = Ce livre FB2 n’a pas de corps de texte

# draft
books_fb2_not_fb2 = Ce n’est pas un document FB2

# draft
books_fb2_notes = Notes

# draft
books_fb2_too_large = Ce livre FB2 est trop volumineux pour être lu

# draft
books_fb2_too_many_chapters = { $count ->
       *[other] Ce livre FB2 compte plus de { $count } chapitres
    }

# draft
books_fb2_zip_empty = Ce FB2 compressé ne contient aucun livre FB2

# draft
books_fb2_zip_members = { $count ->
       *[other] Ce FB2 compressé contient plus de { $count } fichiers
    }

# draft
books_kf8_no_fragments = Ce livre KF8 n’a pas d’index des fragments

# draft
books_kf8_no_skeletons = Ce livre KF8 n’a pas d’index des squelettes

# draft
books_kf8_text_damaged = Le texte KF8 de ce livre est endommagé

# draft
books_mobi_dictionary_too_large = Le dictionnaire de ce livre est trop volumineux pour être lu

# draft
books_mobi_huffman_damaged = Les tables de Huffman de ce livre sont endommagées

# draft
books_mobi_index_damaged = L’index de ce livre est endommagé

# draft
books_mobi_kfx = Ce livre est au format KFX, propre à Amazon, qui n’est pas pris en charge

# draft
books_mobi_missing_chapter = Chapitre manquant : { $href }

# draft
books_mobi_missing_picture = Image manquante : { $key }

# draft
books_mobi_no_huffman = Ce livre n’indique aucune table de Huffman

# draft
books_mobi_not_mobi = Ce n’est pas un livre Mobipocket

# draft
books_mobi_protected = Ce livre est protégé : son texte est chiffré (DRM)

# draft
books_mobi_text_damaged = Le texte de ce livre est endommagé

# draft
books_mupdf_missing = La lecture des livres avec MuPDF nécessite skwir-mupdf-worker ; installez-le ou choisissez le moteur de livres intégré

# draft
books_needs_math = Composition des formules MathML des livres

# draft
books_needs_mupdf = Mise en pages des livres par MuPDF

# draft
books_no_chapters = Aucun chapitre lisible dans ce fichier { $format }

# draft
books_page_not_drawn = La page n’a pas pu être dessinée

# draft
books_palm_compression = Le texte de ce livre est compressé selon une méthode non prise en charge ({ $code })

# draft
books_palm_cut_short = Ce livre est tronqué

# draft
books_palm_first_record_short = Le premier enregistrement de ce livre est tronqué

# draft
books_palm_no_record = Ce livre n’a pas d’enregistrement { $index }

# draft
books_palm_no_records = Ce livre n’a aucun enregistrement

# draft
books_palm_out_of_order = Les enregistrements de ce livre sont dans le désordre

# draft
books_palm_record_too_large = Un enregistrement de ce livre est trop volumineux pour être lu

# draft
books_plugin_description = Lire les livres EPUB, FB2 et MOBI avec leur sommaire

# draft
books_plugin_title = Livres

# draft
books_read_as = Lu en { $encoding }

# draft
books_reading_mode = Mode lecture · les polices, les couleurs et la mise en page du livre ne sont pas appliquées

# draft
books_search_found = Résultat de recherche dans ce chapitre

# draft
books_search_none = Aucun chapitre correspondant

# draft
books_search_too_long = Le texte recherché est trop long

# draft
books_series_number = { $series }, { $number }

# draft
books_title = { $format } · chapitre { $chapter } / { $count }
