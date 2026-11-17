# French catalog of skwir-plugin-geo, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
geo_already_at = { $name } était déjà à { $lat }, { $lon }

# draft
geo_already_no_location = { $name } n’avait déjà aucune position

# draft
geo_altitude_flight = { $feet } ft · { $metres } m

# draft
geo_altitude_flight_ground = { $feet } ft · { $metres } m au-dessus du sol

# draft
geo_altitude_flight_takeoff = { $feet } ft · { $metres } m au-dessus du point de décollage

# draft
geo_altitude_ground = { $metres } m au-dessus du sol

# draft
geo_altitude_takeoff = { $metres } m au-dessus du point de décollage

# draft
geo_changed_since = { $name } a changé depuis que le plan a été établi

# draft
geo_clocks_corrected = horloges des appareils photo corrigées de { $seconds } s

# draft
geo_compass_e = E

# draft
geo_compass_n = N

# draft
geo_compass_ne = NE

# draft
geo_compass_nw = NO

# draft
geo_compass_s = S

# draft
geo_compass_se = SE

# draft
geo_compass_sw = SO

# draft
geo_compass_w = O

# draft
geo_contents_areas = { $count ->
        [one] { $count } zone
       *[other] { $count } zones
    }

# draft
geo_contents_routes = { $count ->
        [one] { $count } itinéraire
       *[other] { $count } itinéraires
    }

# draft
geo_contents_tracks = { $count ->
        [one] { $count } trace
       *[other] { $count } traces
    }

# draft
geo_contents_waypoints = { $count ->
        [one] { $count } point de passage
       *[other] { $count } points de passage
    }

# draft
geo_damaged = Le fichier est endommagé ({ $why }) ; ce qui précède est affiché

# draft
geo_dropped_places = { $count ->
       *[other] { $count } lieux de plus que la carte n’en montre
    }

# draft
geo_exiftool_cannot_write = ExifTool ne peut pas écrire de position dans { $name }

# draft
geo_exiftool_could_not_write = ExifTool n’a pas pu écrire { $name } : { $why }

# draft
geo_exiftool_missing = Installez ExifTool pour écrire la position des photos

# draft
geo_exiftool_unreadable = ExifTool n’a pas pu le lire

# draft
geo_fact_areas = Zones

# draft
geo_fact_climb = Dénivelé positif

# draft
geo_fact_created_by = Créé par

# draft
geo_fact_cruise = Croisière

# draft
geo_fact_departure = Départ

# draft
geo_fact_descent = Dénivelé négatif

# draft
geo_fact_description = Description

# draft
geo_fact_destination = Destination

# draft
geo_fact_distance = Distance

# draft
geo_fact_duration = Durée

# draft
geo_fact_ended = Fin

# draft
geo_fact_flight_height = Hauteur de vol

# draft
geo_fact_highest_point = Point le plus haut

# draft
geo_fact_highest_waypoint = Point de passage le plus haut

# draft
geo_fact_legs = Tronçons

# draft
geo_fact_lowest_point = Point le plus bas

# draft
geo_fact_moving_speed = Vitesse en mouvement

# draft
geo_fact_moving_time = Temps en mouvement

# draft
geo_fact_name = Nom

# draft
geo_fact_points = Points

# draft
geo_fact_route_distance = Distance des itinéraires

# draft
geo_fact_routes = Itinéraires

# draft
geo_fact_start = Premier point

# draft
geo_fact_started = Début

# draft
geo_fact_tracks = Traces

# draft
geo_fact_waypoints = Points de passage

# draft
geo_fit_byte_order = un message a un ordre d’octets inconnu

# draft
geo_fit_checksum_mismatch = la somme de contrôle du fichier ne correspond pas à son contenu

# draft
geo_fit_ends_in_record = le fichier se termine au milieu d’un enregistrement

# draft
geo_fit_no_checksum = le fichier se termine avant sa somme de contrôle

# draft
geo_fit_not_fit = ce n’est pas un fichier FIT

# draft
geo_fit_record_past_end = un enregistrement dépasse la fin des données

# draft
geo_fit_trailing = autre chose qu’un fichier FIT suit les données

# draft
geo_fit_undefined_message = le type d’un message n’a jamais été défini

# draft
geo_fit_unfinished = l’enregistrement n’a pas été terminé

# draft
geo_flight_distance = { $distance } · { $nautical } NM

# draft
geo_fms_fewer_waypoints = le plan annonce { $expected ->
       *[other] { $expected } points de passage
    } et en contient { $read }

# draft
geo_fms_not_plan = ce n’est pas un plan de vol X-Plane

# draft
geo_fms_version = un plan de vol X-Plane en version { $version }, que Skwir ne lit pas

# draft
geo_format_garmin_plan = Plan de vol Garmin

# draft
geo_format_litchi_mission = Mission Litchi

# draft
geo_format_little_navmap_plan = Plan Little Navmap

# draft
geo_format_mavlink_mission = Mission MAVLink

# draft
geo_format_qgc_plan = Plan QGroundControl

# draft
geo_format_sim_plan = Plan Flight Simulator

# draft
geo_format_xplane_plan = Plan X-Plane

# draft
geo_fpl_not_plan = ce n’est pas un plan de vol Garmin

# draft
geo_geojson_invalid = GeoJSON non valide : { $error }

# draft
geo_geojson_too_large = Ce fichier GeoJSON dépasse la limite d’aperçu de { $size }

# draft
geo_geotag = Géolocaliser des photos d’après une trace

# draft
geo_geotag_description = Placer les photos sélectionnées le long de l’unique trace sélectionnée (GPX, FIT, toute trace lisible) : chaque photo prise pendant son enregistrement reçoit sa position à cet instant ; proposé pour revue. S’exécute en arrière-plan : le résultat arrive sous forme de proposition ou de notification

# draft
geo_geotag_menu = Géolocaliser d’après une trace

# draft
geo_gone = { $name } n’existe plus

# draft
geo_holds_no_track = { $name } ne contient aucune trace

# draft
geo_invalid_points = { $count ->
        [one] { $count } point omis : ce n’est pas une position
       *[other] { $count } points omis : ce ne sont pas des positions
    }

# draft
geo_kilometres = { $km } km

# draft
geo_kmz_members = { $count ->
       *[other] Ce KMZ contient plus de { $count } fichiers
    }

# draft
geo_kmz_no_kml = Ce KMZ ne contient aucun document KML

# draft
geo_kmz_unreadable = Le document de ce KMZ ne peut pas être lu

# draft
geo_left_others = { $count ->
        [one] { $count } autre fichier laissé de côté
       *[other] { $count } autres fichiers laissés de côté
    }

# draft
geo_left_out = { $what }

# draft
geo_left_outside = { $count ->
        [one] { $count } photo hors de la période de la trace
       *[other] { $count } photos hors de la période de la trace
    }

# draft
geo_left_placed = { $count ->
        [one] { $count } photo déjà placée, laissée telle quelle
       *[other] { $count } photos déjà placées, laissées telles quelles
    }

# draft
geo_left_raw = { $count ->
        [one] { $count } photo RAW laissée de côté : les fichiers annexes ne sont pas encore écrits
       *[other] { $count } photos RAW laissées de côté : les fichiers annexes ne sont pas encore écrits
    }

# draft
geo_left_undated = { $count ->
        [one] { $count } photo sans heure de prise de vue
       *[other] { $count } photos sans heure de prise de vue
    }

# draft
geo_left_unreadable = { $count ->
        [one] { $count } photo qui n’a pas pu être lue
       *[other] { $count } photos qui n’ont pas pu être lues
    }

# draft
geo_left_unwritable = { $count ->
        [one] { $count } photo qu’ExifTool ne peut pas écrire
       *[other] { $count } photos qu’ExifTool ne peut pas écrire
    }

# draft
geo_litchi_columns = Pas une mission Litchi : les colonnes du CSV ne sont pas celles d’une mission

# draft
geo_litchi_empty = Pas une mission Litchi : le fichier est vide

# draft
geo_litchi_point_of_interest = Point d’intérêt

# draft
geo_lnmpln_not_plan = ce n’est pas un plan Little Navmap

# draft
geo_locate = Où se trouve un lieu

# draft
geo_locate_description = Les lieux de ce nom que Skwir connaît hors ligne, les plus grands d’abord, avec leur position : pour les scripts et les agents

# draft
geo_location_already_back = la position de { $name } était déjà restaurée

# draft
geo_mavlink_empty = Pas une mission MAVLink : le fichier est vide

# draft
geo_mavlink_no_header = Pas une mission MAVLink : sa première ligne n’est pas « QGC WPL 110 »

# draft
geo_metres = { $metres } m

# draft
geo_mission_exclusion_zone = Zone d’exclusion

# draft
geo_mission_geofence = Périmètre virtuel

# draft
geo_mission_home = Point de départ

# draft
geo_mission_survey = Relevé

# draft
geo_no_pane = aucun panneau { $pane }

# draft
geo_no_picture_selected = Aucune photo sélectionnée

# draft
geo_no_picture_selected_because = Aucune photo sélectionnée : { $why }

# draft
geo_no_position = Rien dans ce fichier n’a de position

# draft
geo_not_a_map = Ce n’est pas un fichier de carte

# draft
geo_nothing_along = Aucune photo à placer le long de { $track }

# draft
geo_nothing_along_because = Aucune photo à placer le long de { $track } : { $why }

# draft
geo_place_along = { $count ->
        [1] placer { $name } le long de { $track }
        [one] placer { $count } photo le long de { $track }
       *[other] placer { $count } photos le long de { $track }
    }

# draft
geo_place_at = placer { $name } à { $lat }, { $lon }

# draft
geo_place_label = Lieu

# draft
geo_place_near = { $place }, { $distance } { $direction }

# draft
geo_placed_at = { $name } placé à { $lat }, { $lon }

# draft
geo_plan_too_large = Ce plan dépasse la limite d’aperçu de { $size }

# draft
geo_pln_not_plan = ce n’est pas un plan Flight Simulator

# draft
geo_plugin_description = Afficher sur une carte les traces et itinéraires GPS (GPX, KML, KMZ, GeoJSON, TCX, FIT, IGC, NMEA), les plans de vol (Garmin, Flight Simulator, X-Plane, Little Navmap) et les missions de drone (QGroundControl, MAVLink, Litchi, DJI), avec leur distance, leur durée et leur dénivelé, ainsi que les photos et vidéos d’un dossier dans la vue Carte. Rien de ce vers quoi pointe un fichier n’est téléchargé

# draft
geo_plugin_title = Cartes

# draft
geo_points = Où sont les fichiers

# draft
geo_points_description = Les photos et traces de ce que liste un panneau qui ont été placées sur la carte, après les `since` premières

# draft
geo_qgc_invalid = Plan QGroundControl non valide : { $error }

# draft
geo_qgc_structure_scan = Balayage de structure

# draft
geo_rally_point = R{ $number }

# draft
geo_read_first = Seul le début du fichier a été lu ({ $size })

# draft
geo_release = Oublier où sont les fichiers

# draft
geo_release_description = Cesser de chercher où se trouvent les fichiers d’un panneau : sa vue Carte a été fermée

# draft
geo_remove_location = Supprimer la position GPS des photos

# draft
geo_remove_location_description = Retirer la position des photos sélectionnées (plan) ; celles qui n’en ont pas sont laissées telles quelles

# draft
geo_remove_location_menu = Supprimer la position GPS

# draft
geo_remove_location_of = supprimer la position de { $name }

# draft
geo_remove_locations = { $count ->
        [1] supprimer la position de { $name }
        [one] supprimer la position de { $count } photo
       *[other] supprimer la position de { $count } photos
    }

# draft
geo_removed_location_of = position de { $name } supprimée

# draft
geo_requirement = Écriture et suppression de la position des photos

# draft
geo_restore_location_of = restaurer la position de { $name }

# draft
geo_restored_location_of = position de { $name } restaurée

# draft
geo_scan = Trouver où sont les fichiers

# draft
geo_scan_description = Trouver les photos et traces de ce que liste un panneau qui indiquent où elles se trouvent, pour la vue Carte

# draft
geo_search_area = Rechercher dans cette zone

# draft
geo_search_area_description = Afficher les photos et vidéos du dossier et de ses sous-dossiers prises dans une zone

# draft
geo_search_subfolders = Chercher des lieux dans les sous-dossiers

# draft
geo_search_subfolders_description = Afficher les photos et traces du dossier et de tous ses sous-dossiers

# draft
geo_select_one_track = Sélectionnez une seule trace : { $count ->
       *[other] { $count } sont sélectionnées
    }

# draft
geo_select_pictures = Sélectionnez les photos dont la position est à supprimer

# draft
geo_select_track = Sélectionnez une trace et les photos à placer le long de celle-ci

# draft
geo_speed = { $speed } km/h

# draft
geo_stop_named = { $ident } ({ $name })

# draft
geo_summary_flight = Plan de vol

# draft
geo_summary_flight_route = Plan de vol · { $from } → { $to }

# draft
geo_summary_mission = Mission de drone

# draft
geo_summary_mission_waypoints = Mission de drone · { $count ->
        [one] { $count } point de passage
       *[other] { $count } points de passage
    }

# draft
geo_text_line_not_text = la ligne { $line } n’est pas du texte

# draft
geo_text_line_too_long = la ligne { $line } dépasse { $size }

# draft
geo_thinned = La carte montre un point sur { $stride }

# draft
geo_title = { $format } · { $contents }

# draft
geo_track_no_times = { $name } n’a pas d’horodatage : seule une trace enregistrée permet de placer des photos

# draft
geo_unreadable = { $name } n’a pas pu être lu : { $error }

# draft
geo_view = Carte

# draft
geo_wpml_first_waypoint = Premier point de passage

# draft
geo_wpml_takeoff = Décollage

# draft
geo_wpml_unreadable = Cette mission ne peut pas être lue

# draft
geo_xml_end_closes_nothing = une balise de fin ne ferme rien

# draft
geo_xml_error_at = { $error } (à l’octet { $byte })

# draft
geo_xml_text_too_long = un élément contient plus de { $size } de texte

# draft
geo_xml_too_deep = les éléments sont imbriqués sur plus de { $depth } niveaux
