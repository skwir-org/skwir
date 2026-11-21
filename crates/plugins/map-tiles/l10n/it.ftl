# Italian catalog of skwir-plugin-map-tiles, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
map_tiles_add_server = Aggiungi server di tessere

# draft
map_tiles_answered = il server di tessere ha risposto { $status }

# draft
map_tiles_area = da { $south }, { $west } a { $north }, { $east }

# draft
map_tiles_avif_notice = Le tessere AVIF non vengono disegnate: la mappa mostra dove il file ha tessere

# draft
map_tiles_check_header_line = { $name }: il valore di un’intestazione deve stare su una riga

# draft
map_tiles_check_header_name = { $name }: { $header } non è un nome di intestazione

# draft
map_tiles_check_header_reserved = { $name }: Skwir imposta da sé l’intestazione { $header }

# draft
map_tiles_check_https = { $name }: l’indirizzo deve iniziare con https://

# draft
map_tiles_check_needs = { $name }: l’indirizzo richiede { "{z}" }, { "{x}" } e { "{y}" } (o { "{-y}" }), oppure { "{q}" }

# draft
map_tiles_check_no_key = { $name }: { "{key}" } è usato, ma non è impostata nessuna chiave API

# draft
map_tiles_check_subdomains = { $name }: l’indirizzo usa { "{s}" }, quindi il server richiede dei sottodomini

# draft
map_tiles_check_unknown_part = { $name }: l’indirizzo contiene una parte sconosciuta { $part }

# draft
map_tiles_compressed = Le tessere sono compresse con { $compression }, che Skwir non legge

# draft
map_tiles_detail_area = Area

# draft
map_tiles_detail_attribution = Attribuzione

# draft
map_tiles_detail_count = Numero di tessere

# draft
map_tiles_detail_description = Descrizione

# draft
map_tiles_detail_format = Formato

# draft
map_tiles_detail_layers = Livelli

# draft
map_tiles_detail_name = Nome

# draft
map_tiles_detail_tiles = Tessere

# draft
map_tiles_detail_version = Versione

# draft
map_tiles_detail_zoom = Zoom

# draft
map_tiles_directory_damaged = Una directory dell’archivio è danneggiata

# draft
map_tiles_directory_ends = Una directory dell’archivio termina prematuramente

# draft
map_tiles_directory_number = Una directory dell’archivio contiene un numero troppo grande

# draft
map_tiles_format_avif = Immagini AVIF

# draft
map_tiles_format_jpeg = Immagini JPEG

# draft
map_tiles_format_png = Immagini PNG

# draft
map_tiles_format_unknown = Sconosciuto

# draft
map_tiles_format_vector = Forme vettoriali

# draft
map_tiles_format_webp = Immagini WebP

# draft
map_tiles_its_compressed = Le sue tessere sono compresse con { $compression }, che Skwir non legge

# draft
map_tiles_layers = { $layers }

# draft
map_tiles_mbtiles_no_metadata = Non è un file MBTiles: non ha una tabella dei metadati

# draft
map_tiles_mbtiles_no_tiles = Non è un file MBTiles: non ha tessere

# draft
map_tiles_more_choices = i server di tessere che aggiungi

# draft
map_tiles_no_key = La chiave API di { $name } non è nel portachiavi

# draft
map_tiles_no_server = Nessun server di tessere { $id }

# draft
map_tiles_no_subdomains = { $name } non ha sottodomini per { "{s}" }

# draft
map_tiles_not_archive = Non è un archivio di tessere

# draft
map_tiles_not_modified = il server ha detto che la tessera non era cambiata, ma Skwir non l’ha mai avuta

# draft
map_tiles_not_pmtiles = Non è un file PMTiles

# draft
map_tiles_outline = Contorni

# draft
map_tiles_outline_not = non sono dati dei contorni

# draft
map_tiles_outline_number = un numero nei dati dei contorni è troppo lungo

# draft
map_tiles_outline_short = i dati dei contorni terminano prematuramente

# draft
map_tiles_outline_too_large = una tessera non può essere così grande

# draft
map_tiles_part_too_large = Una parte dell’archivio è troppo grande

# draft
map_tiles_plugin_description = Disegna mappe sui contorni delle terre emerse, offline, oppure su OpenStreetMap o sui server di tessere che aggiungi, una volta che ne scegli uno, e mostra l’anteprima delle mappe salvate in file (PMTiles, MBTiles). Solo questo plugin usa la rete

# draft
map_tiles_plugin_title = Sfondi della mappa

# draft
map_tiles_pmtiles_short = Non è un file PMTiles: termina prima della sua intestazione

# draft
map_tiles_pmtiles_version = La versione { $version } di PMTiles non viene letta; la versione 3 sì

# draft
map_tiles_points_outside = L’archivio punta fuori da sé stesso

# draft
map_tiles_server_address = Indirizzo

# draft
map_tiles_server_address_description = Dove si trova una tessera: { "{z}" }, { "{x}" } e { "{y}" } (o { "{-y}" }, contato dal basso), oppure { "{q}" } per un quadkey; { "{s}" } per un sottodominio, { "{r}" } per @2x su uno schermo ad alta densità, { "{key}" } per la chiave API

# draft
map_tiles_server_answered = { $name } ha risposto con una tessera di { $width }×{ $height }

# draft
map_tiles_server_attribution = Attribuzione

# draft
map_tiles_server_attribution_description = Ciò che la licenza del server chiede di mostrare sulla mappa sotto le sue tessere

# draft
map_tiles_server_failed = { $name } non ha fornito nessuna tessera: { $error }

# draft
map_tiles_server_headers = Intestazioni

# draft
map_tiles_server_headers_description = Inviate a ogni richiesta: User-Agent sostituisce quello di Skwir, Referer, Authorization… { "{key}" } è sostituito dalla chiave API

# draft
map_tiles_server_key = Chiave API

# draft
map_tiles_server_key_description = Conservata nel portachiavi, mai nelle impostazioni. Inserisci { "{key}" } nell’indirizzo o in un’intestazione dove deve andare

# draft
map_tiles_server_max_zoom = Zoom massimo

# draft
map_tiles_server_max_zoom_description = Le viste più ravvicinate ingrandiscono le tessere di questo zoom

# draft
map_tiles_server_name = Nome

# draft
map_tiles_server_name_description = Come si chiama il server nell’elenco

# draft
map_tiles_server_name_example = Topografica

# draft
map_tiles_server_subdomains = Sottodomini

# draft
map_tiles_server_subdomains_description = Ciò che rappresenta { "{s}" }, uno dopo l’altro

# draft
map_tiles_server_test = Prova

# draft
map_tiles_servers = Server di tessere

# draft
map_tiles_servers_description = Sfondi da altri server, da scegliere in Sfondo della mappa o da una mappa.

# draft
map_tiles_source = Sfondo della mappa

# draft
map_tiles_source_description = Ciò su cui vengono disegnate le mappe.

# draft
map_tiles_source_description_more = I contorni delle terre emerse non richiedono la rete. OpenStreetMap e gli altri server scaricano le tessere che guardi, e così sanno dove guardi.

# draft
map_tiles_test_server = Prova un server di tessere

# draft
map_tiles_test_server_description = Chiedi subito una tessera a un server di tessere e mostra che cosa ha risposto

# draft
map_tiles_tile_count = { $count }

# draft
map_tiles_tile_too_large = Una tessera dell’archivio è troppo grande

# draft
map_tiles_tiles_notice = Le sue tessere non vengono disegnate: la mappa mostra dove il file ha tessere

# draft
map_tiles_title = { $format } · { $name }

# draft
map_tiles_too_deep = Le directory dell’archivio sono annidate troppo in profondità

# draft
map_tiles_unnamed_server = Un server di tessere

# draft
map_tiles_unreadable_notice = { $why }: la mappa mostra dove ha tessere

# draft
map_tiles_vector_notice = Le tessere vettoriali non vengono disegnate: la mappa mostra dove il file ha tessere

# draft
map_tiles_zoom_level = { $level }

# draft
map_tiles_zoom_range = da { $from } a { $to }
