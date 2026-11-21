# Italian catalog of skwir-plugin-books, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
books_chapter = Capitolo { $number }

# draft
books_chapter_cut = Il capitolo è troncato: contiene più di quanto la modalità lettura legga

# draft
books_detail_author = Autore

# draft
books_detail_chapters = Capitoli

# draft
books_detail_genre = Genere letterario

# draft
books_detail_isbn = ISBN

# draft
books_detail_language = Lingua

# draft
books_detail_published = Pubblicazione

# draft
books_detail_publisher = Editore

# draft
books_detail_series = Serie

# draft
books_detail_subject = Argomento

# draft
books_detail_title = Titolo

# draft
books_detail_year = Anno

# draft
books_engine = Motore dei libri

# draft
books_engine_auto = Automatico

# draft
books_engine_builtin = Integrato

# draft
books_engine_description = Automatico legge i libri con il motore integrato, che riadatta il loro testo al tema e disegna le pagine a layout fisso come le impagina il libro.

# draft
books_engine_description_more = MuPDF impagina i libri con i loro stili: legge bene tutti i libri EPUB, FB2 e MOBI; gli altri li legge il motore integrato.

# draft
books_epub_members = { $count ->
       *[other] Questo EPUB contiene più di { $count } componenti
    }

# draft
books_epub_missing = Risorsa EPUB mancante { $href }

# draft
books_epub_no_package = Il contenitore EPUB non indica nessun pacchetto

# draft
books_epub_no_pages = L’EPUB non ha pagine

# draft
books_epub_protected_chapter = Questo EPUB è protetto: un capitolo è cifrato

# draft
books_epub_protected_container = Questo EPUB è protetto: il suo contenitore è cifrato

# draft
books_epub_protected_contents = Questo EPUB è protetto: il suo indice è cifrato

# draft
books_epub_protected_encryption = Questo EPUB è protetto: perfino l’elenco delle parti cifrate è cifrato

# draft
books_epub_protected_package = Questo EPUB è protetto: il suo pacchetto è cifrato

# draft
books_epub_protected_page = Questo EPUB è protetto: una pagina è cifrata

# draft
books_epub_protected_picture = Questo EPUB è protetto: un’immagine incorporata è cifrata

# draft
books_epub_protected_resource = Questo EPUB è protetto: una risorsa usata da un capitolo è cifrata

# draft
books_epub_protected_stylesheet = Questo EPUB è protetto: un foglio di stile è cifrato

# draft
books_epub_too_large_chapter = Un capitolo supera il limite di { $size } dell’anteprima

# draft
books_epub_too_large_container = Il contenitore dell’EPUB supera il limite di { $size } dell’anteprima

# draft
books_epub_too_large_contents = L’indice dell’EPUB supera il limite di { $size } dell’anteprima

# draft
books_epub_too_large_encryption = L’elenco delle parti cifrate dell’EPUB supera il limite di { $size } dell’anteprima

# draft
books_epub_too_large_package = Il pacchetto dell’EPUB supera il limite di { $size } dell’anteprima

# draft
books_epub_too_large_page = Una pagina supera il limite di { $size } dell’anteprima

# draft
books_epub_too_large_picture = Un’immagine incorporata supera il limite di { $size } dell’anteprima

# draft
books_epub_too_large_resource = Una risorsa dell’EPUB supera il limite di { $size } dell’anteprima

# draft
books_epub_too_large_stylesheet = Un foglio di stile supera il limite di { $size } dell’anteprima

# draft
books_epub_too_many_pages = L’EPUB ha troppe pagine

# draft
books_fb2_damaged_picture = L’immagine { $id } di questo FB2 è danneggiata

# draft
books_fb2_document_missing = Il documento FB2 manca

# draft
books_fb2_document_too_large = Il documento FB2 supera il limite di { $size } dell’anteprima

# draft
books_fb2_missing_chapter = Capitolo FB2 mancante { $href }

# draft
books_fb2_missing_picture = Immagine FB2 mancante { $id }

# draft
books_fb2_no_body = Questo FB2 non ha un corpo

# draft
books_fb2_not_fb2 = Questo non è un documento FB2

# draft
books_fb2_notes = Note

# draft
books_fb2_too_large = Questo FB2 è troppo grande per essere letto

# draft
books_fb2_too_many_chapters = { $count ->
       *[other] Questo libro FB2 ha più di { $count } capitoli
    }

# draft
books_fb2_zip_empty = Questo FB2 compresso non contiene nessun FB2

# draft
books_fb2_zip_members = { $count ->
       *[other] Questo FB2 compresso contiene più di { $count } componenti
    }

# draft
books_kf8_no_fragments = Questo libro KF8 non ha un indice dei frammenti

# draft
books_kf8_no_skeletons = Questo libro KF8 non ha un indice degli scheletri

# draft
books_kf8_text_damaged = Il testo KF8 di questo libro è danneggiato

# draft
books_mobi_dictionary_too_large = Il dizionario di questo libro è troppo grande per essere letto

# draft
books_mobi_huffman_damaged = Le tabelle di Huffman di questo libro sono danneggiate

# draft
books_mobi_index_damaged = L’indice di questo libro è danneggiato

# draft
books_mobi_kfx = Questo è un libro KFX, il formato proprio di Amazon, che non viene letto

# draft
books_mobi_missing_chapter = Capitolo mancante { $href }

# draft
books_mobi_missing_picture = Immagine mancante { $key }

# draft
books_mobi_no_huffman = Questo libro non indica nessuna tabella di Huffman

# draft
books_mobi_not_mobi = Questo non è un libro Mobipocket

# draft
books_mobi_protected = Questo libro è protetto: il suo testo è cifrato (DRM)

# draft
books_mobi_text_damaged = Il testo di questo libro è danneggiato

# draft
books_mupdf_missing = Leggere i libri con MuPDF richiede skwir-mupdf-worker; installalo, oppure scegli il motore dei libri integrato

# draft
books_needs_math = Formule che un libro scrive in MathML, composte tipograficamente

# draft
books_needs_mupdf = Libri impaginati da MuPDF

# draft
books_no_chapters = { $format } non ha capitoli leggibili

# draft
books_page_not_drawn = Non è stato possibile disegnare la pagina

# draft
books_palm_compression = Il testo di questo libro è compresso in un modo che non viene letto ({ $code })

# draft
books_palm_cut_short = Questo libro è troncato

# draft
books_palm_first_record_short = Il primo record di questo libro è troncato

# draft
books_palm_no_record = Questo libro non ha il record { $index }

# draft
books_palm_no_records = Questo libro non ha record

# draft
books_palm_out_of_order = I record di questo libro sono fuori ordine

# draft
books_palm_record_too_large = Un record di questo libro è troppo grande per essere letto

# draft
books_plugin_description = Leggi libri EPUB, FB2 e MOBI con il loro indice

# draft
books_plugin_title = Libri

# draft
books_read_as = Letto come { $encoding }

# draft
books_reading_mode = Modalità lettura · caratteri, colori e impaginazione del libro non sono applicati

# draft
books_search_found = Corrispondenza della ricerca in questo capitolo

# draft
books_search_none = Nessun capitolo corrispondente

# draft
books_search_too_long = Il testo cercato è troppo lungo

# draft
books_series_number = { $series }, { $number }

# draft
books_title = { $format } · capitolo { $chapter } / { $count }
