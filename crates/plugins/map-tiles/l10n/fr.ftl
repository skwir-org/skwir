# French catalog of skwir-plugin-map-tiles, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
map_tiles_add_server = Ajouter un serveur de tuiles

# draft
map_tiles_answered = le serveur de tuiles a répondu { $status }

# draft
map_tiles_area = { $south } ; { $west } à { $north } ; { $east }

# draft
map_tiles_avif_notice = Les tuiles AVIF ne sont pas dessinées : la carte montre où le fichier a des tuiles

# draft
map_tiles_check_header_line = { $name } : la valeur d’un en-tête tient sur une seule ligne

# draft
map_tiles_check_header_name = { $name } : « { $header } » n’est pas un nom d’en-tête

# draft
map_tiles_check_header_reserved = { $name } : Skwir définit lui-même l’en-tête { $header }

# draft
map_tiles_check_https = { $name } : l’adresse doit commencer par https://

# draft
map_tiles_check_needs = { $name } : l’adresse doit contenir { "{z}" }, { "{x}" } et { "{y}" } (ou { "{-y}" }), ou { "{q}" }

# draft
map_tiles_check_no_key = { $name } : { "{key}" } est utilisé, mais aucune clé d’API n’est définie

# draft
map_tiles_check_subdomains = { $name } : l’adresse utilise { "{s}" }, le serveur a donc besoin de sous-domaines

# draft
map_tiles_check_unknown_part = { $name } : l’adresse contient une partie inconnue, { $part }

# draft
map_tiles_compressed = Les tuiles sont compressées en { $compression }, que Skwir ne sait pas lire

# draft
map_tiles_detail_area = Zone

# draft
map_tiles_detail_attribution = Attribution

# draft
map_tiles_detail_count = Nombre de tuiles

# draft
map_tiles_detail_description = Description

# draft
map_tiles_detail_format = Format

# draft
map_tiles_detail_layers = Couches

# draft
map_tiles_detail_name = Nom

# draft
map_tiles_detail_tiles = Tuiles

# draft
map_tiles_detail_version = Version

# draft
map_tiles_detail_zoom = Zoom

# draft
map_tiles_directory_damaged = Un répertoire de l’archive est endommagé

# draft
map_tiles_directory_ends = Un répertoire de l’archive se termine trop tôt

# draft
map_tiles_directory_number = Un répertoire de l’archive contient un nombre trop grand

# draft
map_tiles_downloads_stopped = les téléchargements se sont arrêtés

# draft
map_tiles_format_avif = Images AVIF

# draft
map_tiles_format_jpeg = Images JPEG

# draft
map_tiles_format_png = Images PNG

# draft
map_tiles_format_unknown = Inconnu

# draft
map_tiles_format_vector = Formes vectorielles

# draft
map_tiles_format_webp = Images WebP

# draft
map_tiles_its_compressed = Ses tuiles sont compressées en { $compression }, que Skwir ne sait pas lire

# draft
map_tiles_mbtiles_no_metadata = Ce n’est pas un fichier MBTiles : il n’a pas de table de métadonnées

# draft
map_tiles_mbtiles_no_tiles = Ce n’est pas un fichier MBTiles : il n’a pas de tuiles

# draft
map_tiles_more_choices = les serveurs de tuiles que vous ajoutez

# draft
map_tiles_no_key = La clé d’API de { $name } n’est pas dans le trousseau

# draft
map_tiles_no_server = Aucun serveur de tuiles { $id }

# draft
map_tiles_no_subdomains = { $name } n’a pas de sous-domaines pour { "{s}" }

# draft
map_tiles_not_address = ce n’est pas une adresse

# draft
map_tiles_not_archive = Ce n’est pas une archive de tuiles

# draft
map_tiles_not_modified = le serveur a répondu qu’une tuile n’avait pas changé, mais elle n’a jamais été reçue

# draft
map_tiles_not_pmtiles = Ce n’est pas un fichier PMTiles

# draft
map_tiles_outline = Contours

# draft
map_tiles_outline_not = ce ne sont pas les données des contours

# draft
map_tiles_outline_number = un nombre des données des contours est trop long

# draft
map_tiles_outline_short = les données des contours se terminent trop tôt

# draft
map_tiles_outline_too_large = une tuile ne peut pas être aussi grande

# draft
map_tiles_part_too_large = Une partie de l’archive est trop grande

# draft
map_tiles_plugin_description = Dessiner les cartes sur le contour des terres, hors ligne, ou, une fois que vous en choisissez un, sur OpenStreetMap ou des serveurs de tuiles que vous ajoutez, et afficher l’aperçu des cartes gardées dans des fichiers (PMTiles, MBTiles). Seul ce module utilise le réseau

# draft
map_tiles_plugin_title = Fonds de carte

# draft
map_tiles_pmtiles_short = Ce n’est pas un fichier PMTiles : il se termine avant son en-tête

# draft
map_tiles_pmtiles_version = La version { $version } de PMTiles n’est pas lue ; la version 3 l’est

# draft
map_tiles_points_outside = L’archive pointe en dehors d’elle-même

# draft
map_tiles_server_address = Adresse

# draft
map_tiles_server_address_description = Où se trouve une tuile : { "{z}" }, { "{x}" } et { "{y}" } (ou { "{-y}" }, compté depuis le bas), ou { "{q}" } pour un quadkey ; { "{s}" } pour un sous-domaine, { "{r}" } pour @2x sur un écran dense, { "{key}" } pour la clé d’API

# draft
map_tiles_server_answered = { $name } a répondu avec une tuile de { $width } × { $height }

# draft
map_tiles_server_attribution = Attribution

# draft
map_tiles_server_attribution_description = Ce que la licence du serveur demande d’afficher sur la carte sous ses tuiles

# draft
map_tiles_server_failed = { $name } n’a donné aucune tuile : { $error }

# draft
map_tiles_server_headers = En-têtes

# draft
map_tiles_server_headers_description = Envoyés avec chaque requête : User-Agent remplace celui de Skwir, Referer, Authorization… { "{key}" } est remplacé par la clé d’API

# draft
map_tiles_server_key = Clé d’API

# draft
map_tiles_server_key_description = Gardée dans votre trousseau, jamais dans les paramètres. Mettez { "{key}" } dans l’adresse ou dans un en-tête, là où elle doit aller

# draft
map_tiles_server_max_zoom = Zoom maximal

# draft
map_tiles_server_max_zoom_description = Les vues plus proches agrandissent les tuiles de ce zoom

# draft
map_tiles_server_name = Nom

# draft
map_tiles_server_name_description = Le nom du serveur dans la liste

# draft
map_tiles_server_name_example = Topographique

# draft
map_tiles_server_subdomains = Sous-domaines

# draft
map_tiles_server_subdomains_description = Ce que désigne { "{s}" }, tour à tour

# draft
map_tiles_server_test = Tester

# draft
map_tiles_servers = Serveurs de tuiles

# draft
map_tiles_servers_description = Des fonds de carte venus d’autres serveurs, à choisir dans Fond de carte ou depuis une carte.

# draft
map_tiles_source = Fond de carte

# draft
map_tiles_source_description = Ce sur quoi les cartes sont dessinées.

# draft
map_tiles_source_description_more = Le contour des terres n’a pas besoin du réseau. Avec OpenStreetMap et les autres serveurs, les tuiles que vous regardez sont téléchargées, ce qui leur apprend où vous regardez.

# draft
map_tiles_test_server = Tester un serveur de tuiles

# draft
map_tiles_test_server_description = Demander une tuile à un serveur de tuiles, maintenant, et dire ce qu’il a répondu

# draft
map_tiles_tile_too_large = Une tuile de l’archive est trop grande

# draft
map_tiles_tiles_notice = Ses tuiles ne sont pas dessinées : la carte montre où le fichier a des tuiles

# draft
map_tiles_timed_out = le serveur de tuiles n’a pas répondu à temps

# draft
map_tiles_title = { $format } · { $name }

# draft
map_tiles_too_deep = Les répertoires de l’archive sont imbriqués trop profondément

# draft
map_tiles_unnamed_server = Un serveur de tuiles

# draft
map_tiles_unread = la tuile n’a pas pu être lue : { $error }

# draft
map_tiles_unreadable_notice = { $why } : la carte montre où le fichier a des tuiles

# draft
map_tiles_vector_notice = Les tuiles vectorielles ne sont pas dessinées : la carte montre où le fichier a des tuiles

# draft
map_tiles_zoom_range = de { $from } à { $to }
