# Italian catalog of skwir-plugin-documents, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
documents_detail_application = Applicazione

# draft
documents_detail_author = Autore

# draft
documents_detail_keywords = Parole chiave

# draft
documents_detail_pages = Pagine

# draft
documents_detail_subject = Oggetto

# draft
documents_detail_title = Titolo

# draft
documents_detail_words = Parole

# draft
documents_docx_no_body = Questo documento Word non ha un corpo

# draft
documents_docx_no_text = Questo non è un documento Word: non ha una parte di testo

# draft
documents_engine = Motore dei documenti

# draft
documents_engine_auto = Automatico

# draft
documents_engine_description = Automatico preferisce PDFium e ripiega su Poppler; MuPDF è usato solo se scelto.

# draft
documents_first_blocks = { $count ->
        [1] Viene mostrato solo il primo blocco
       *[other] Vengono mostrati solo i primi { $count } blocchi
    }

# draft
documents_first_blocks_of_part = { $count ->
        [1] Viene mostrato solo il primo blocco di questa parte
       *[other] Vengono mostrati solo i primi { $count } blocchi di questa parte
    }

# draft
documents_members = { $count ->
       *[other] Il documento contiene più di { $count } componenti
    }

# draft
documents_missing_manifest = Manca il manifesto del documento

# draft
documents_missing_notes = Mancano le note del documento

# draft
documents_missing_numbering = Manca la numerazione del documento

# draft
documents_missing_picture = Manca un’immagine incorporata

# draft
documents_missing_properties = Mancano le proprietà del documento

# draft
documents_missing_relationships = Mancano le relazioni del documento

# draft
documents_missing_styles = Mancano gli stili del documento

# draft
documents_missing_text = Manca il testo del documento

# draft
documents_missing_thumbnail = Manca la miniatura

# draft
documents_no_reader = Nessun programma installato legge questo documento come testo

# draft
documents_not_its_picture = non è un’immagine di questo documento

# draft
documents_odt_no_content = Questo non è un testo OpenDocument: non ha contenuto

# draft
documents_odt_not_text = Questo file OpenDocument non è un documento di testo

# draft
documents_odt_password = Questo documento è protetto da una password

# draft
documents_part_of = Parte { $part } di { $parts }

# draft
documents_parts_keys = [ e ] scorrono tra le sue parti

# draft
documents_pdfium_library = Libreria PDFium

# draft
documents_pdfium_library_description = Percorso della libreria condivisa di PDFium.

# draft
documents_pdfium_library_description_more = Lascia vuoto per cercare nelle posizioni consuete.

# draft
documents_plugin_description = Leggi documenti PDF e d’ufficio: scorrimento continuo, ricerca, selezione del testo, collegamenti e indice; il testo Word e OpenDocument anche come testo adattato al pannello

# draft
documents_plugin_title = Documenti

# draft
documents_read_by_pandoc = Letto da Pandoc · impaginazione, intestazioni e commenti non sono mostrati

# draft
documents_reading_mode = Modalità lettura · impaginazione, intestazioni e commenti non sono mostrati

# draft
documents_requirement_libreoffice = Documenti d’ufficio mostrati come pagine

# draft
documents_requirement_mupdf = Lettura dei PDF con MuPDF

# draft
documents_requirement_pandoc = Documenti di testo letti da Pandoc e file RTF letti come testo

# draft
documents_requirement_pdfium = Lettura dei PDF con PDFium

# draft
documents_requirement_poppler = Lettura dei PDF con Poppler

# draft
documents_requirement_sandbox = Isolamento di LibreOffice, che lo richiede, e di Pandoc

# draft
documents_start_only = Viene mostrato solo l’inizio

# draft
documents_text_documents = Documenti di testo

# draft
documents_text_documents_description = Come vengono mostrati i documenti Word, OpenDocument e RTF.

# draft
documents_text_documents_description_more = Lettura mostra subito testo, titoli, elenchi, tabelle, note e immagini, adattati al pannello: Skwir legge i file Word e OpenDocument, Pandoc gli RTF. Pagine li impagina come li stampa LibreOffice, e ci vogliono alcuni secondi per ciascuno. Pandoc fa leggere tutto a Pandoc. Se ciò che una scelta richiede non è installato, ne viene usata un’altra.

# draft
documents_text_documents_pages = Pagine

# draft
documents_text_documents_reading = Lettura

# draft
documents_too_large_manifest = Il manifesto del documento supera il limite di { $size } dell’anteprima

# draft
documents_too_large_notes = Le note del documento superano il limite di { $size } dell’anteprima

# draft
documents_too_large_numbering = La numerazione del documento supera il limite di { $size } dell’anteprima

# draft
documents_too_large_picture = Un’immagine incorporata supera il limite di { $size } dell’anteprima

# draft
documents_too_large_properties = Le proprietà del documento superano il limite di { $size } dell’anteprima

# draft
documents_too_large_relationships = Le relazioni del documento superano il limite di { $size } dell’anteprima

# draft
documents_too_large_styles = Gli stili del documento superano il limite di { $size } dell’anteprima

# draft
documents_too_large_text = Il testo del documento supera il limite di { $size } dell’anteprima

# draft
documents_too_large_thumbnail = La miniatura supera il limite di { $size } dell’anteprima
