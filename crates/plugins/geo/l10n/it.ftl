# Italian catalog of skwir-plugin-geo, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
geo_already_at = { $name } era già in { $lat }, { $lon }

# draft
geo_already_no_location = { $name } era già senza posizione

# draft
geo_altitude_flight = { $feet } ft · { $metres } m

# draft
geo_altitude_flight_ground = { $feet } ft · { $metres } m sul suolo

# draft
geo_altitude_flight_takeoff = { $feet } ft · { $metres } m sul punto di decollo

# draft
geo_altitude_ground = { $metres } m sul suolo

# draft
geo_altitude_takeoff = { $metres } m sul punto di decollo

# draft
geo_changed_since = { $name } è cambiato dopo la creazione del piano

# draft
geo_clocks_corrected = { $sign ->
        [plus] orologi delle fotocamere corretti di +{ $seconds } s
       *[minus] orologi delle fotocamere corretti di −{ $seconds } s
    }

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
        [one] { $count } area
       *[other] { $count } aree
    }

# draft
geo_contents_routes = { $count ->
        [one] { $count } percorso
       *[other] { $count } percorsi
    }

# draft
geo_contents_tracks = { $count ->
        [one] { $count } traccia
       *[other] { $count } tracce
    }

# draft
geo_contents_waypoints = { $count ->
        [one] { $count } waypoint
       *[other] { $count } waypoint
    }

# draft
geo_damaged = Il file è danneggiato ({ $why }); viene mostrato ciò che precede

# draft
geo_dropped_places = { $count ->
        [one] { $count } altro luogo non è mostrato sulla mappa
       *[other] { $count } altri luoghi non sono mostrati sulla mappa
    }

# draft
geo_exiftool_cannot_write = ExifTool non può scrivere una posizione in { $name }

# draft
geo_exiftool_could_not_write = ExifTool non è riuscito a scrivere { $name }: { $why }

# draft
geo_exiftool_missing = Installa ExifTool per scrivere le posizioni delle immagini

# draft
geo_exiftool_unreadable = ExifTool non è riuscito a leggerlo

# draft
geo_fact_areas = Aree

# draft
geo_fact_climb = Salita

# draft
geo_fact_created_by = Creato da

# draft
geo_fact_cruise = Crociera

# draft
geo_fact_departure = Partenza

# draft
geo_fact_descent = Discesa

# draft
geo_fact_description = Descrizione

# draft
geo_fact_destination = Destinazione

# draft
geo_fact_distance = Distanza

# draft
geo_fact_duration = Durata

# draft
geo_fact_ended = Terminato

# draft
geo_fact_flight_height = Altezza di volo

# draft
geo_fact_highest_point = Punto più alto

# draft
geo_fact_highest_waypoint = Waypoint più alto

# draft
geo_fact_legs = Tratte

# draft
geo_fact_lowest_point = Punto più basso

# draft
geo_fact_moving_speed = Velocità in movimento

# draft
geo_fact_moving_time = Tempo in movimento

# draft
geo_fact_name = Nome

# draft
geo_fact_points = Punti

# draft
geo_fact_route_distance = Distanza del percorso

# draft
geo_fact_routes = Percorsi

# draft
geo_fact_start = Inizio

# draft
geo_fact_started = Iniziato

# draft
geo_fact_tracks = Tracce

# draft
geo_fact_waypoints = Waypoint

# draft
geo_fit_byte_order = un messaggio ha un ordine dei byte sconosciuto

# draft
geo_fit_checksum_mismatch = il checksum del file non corrisponde al contenuto

# draft
geo_fit_ends_in_record = il file termina nel mezzo di un record

# draft
geo_fit_no_checksum = il file termina prima del checksum

# draft
geo_fit_not_fit = questo non è un file FIT

# draft
geo_fit_record_past_end = un record supera la fine dei dati

# draft
geo_fit_trailing = dopo i dati segue qualcosa che non è un file FIT

# draft
geo_fit_undefined_message = il tipo di un messaggio non è mai stato definito

# draft
geo_fit_unfinished = la registrazione non è stata completata

# draft
geo_flight_distance = { $distance } · { $nautical } NM

# draft
geo_fms_fewer_waypoints = { $expected ->
        [one] il piano elenca { $expected } waypoint e ne contiene { $read }
       *[other] il piano elenca { $expected } waypoint e ne contiene { $read }
    }

# draft
geo_fms_not_plan = non è un piano di volo X-Plane

# draft
geo_fms_version = un piano di volo X-Plane di versione { $version }, che non viene letta

# draft
geo_format_garmin_plan = Piano di volo Garmin

# draft
geo_format_litchi_mission = Missione Litchi

# draft
geo_format_little_navmap_plan = Piano Little Navmap

# draft
geo_format_mavlink_mission = Missione MAVLink

# draft
geo_format_qgc_plan = Piano QGroundControl

# draft
geo_format_sim_plan = Piano Flight Simulator

# draft
geo_format_xplane_plan = Piano X-Plane

# draft
geo_fpl_not_plan = non è un piano di volo Garmin

# draft
geo_geojson_invalid = GeoJSON non valido: { $error }

# draft
geo_geojson_too_large = Questo file GeoJSON supera il limite di { $size } dell’anteprima

# draft
geo_geotag = Geotagga le immagini da una traccia

# draft
geo_geotag_description = Posiziona le immagini selezionate lungo l’unica traccia selezionata (GPX, FIT, qualsiasi traccia leggibile): ogni immagine scattata durante la registrazione riceve la posizione di quel momento; viene proposta per la revisione. Funziona in un worker: il risultato arriva come proposta o come notifica

# draft
geo_geotag_menu = Geotagga da traccia

# draft
geo_gone = { $name } non esiste più

# draft
geo_holds_no_track = { $name } non contiene alcuna traccia

# draft
geo_invalid_points = { $count ->
        [one] { $count } punto escluso: non è una posizione
       *[other] { $count } punti esclusi: non sono posizioni
    }

# draft
geo_kilometres = { $km } km

# draft
geo_kmz_members = { $count ->
       *[other] Questo KMZ contiene più di { $count } elementi
    }

# draft
geo_kmz_no_kml = Questo KMZ non contiene alcun documento KML

# draft
geo_kmz_unreadable = Il documento di questo KMZ non può essere letto

# draft
geo_left_others = { $count ->
        [one] { $count } altro file escluso
       *[other] { $count } altri file esclusi
    }

# draft
geo_left_out = { $what }

# draft
geo_left_outside = { $count ->
        [one] { $count } immagine fuori dagli orari della traccia
       *[other] { $count } immagini fuori dagli orari della traccia
    }

# draft
geo_left_placed = { $count ->
        [one] { $count } immagine già posizionata, lasciata com’è
       *[other] { $count } immagini già posizionate, lasciate come sono
    }

# draft
geo_left_raw = { $count ->
        [one] { $count } immagine RAW esclusa: i relativi sidecar non sono ancora scritti
       *[other] { $count } immagini RAW escluse: i relativi sidecar non sono ancora scritti
    }

# draft
geo_left_undated = { $count ->
        [one] { $count } immagine senza l’ora di scatto
       *[other] { $count } immagini senza l’ora di scatto
    }

# draft
geo_left_unreadable = { $count ->
        [one] { $count } immagine che non è stato possibile leggere
       *[other] { $count } immagini che non è stato possibile leggere
    }

# draft
geo_left_unwritable = { $count ->
        [one] { $count } immagine che ExifTool non può scrivere
       *[other] { $count } immagini che ExifTool non può scrivere
    }

# draft
geo_litchi_columns = Non è una missione Litchi: le colonne del CSV non sono quelle di una missione

# draft
geo_litchi_empty = Non è una missione Litchi: il file è vuoto

# draft
geo_litchi_point_of_interest = Punto di interesse

# draft
geo_lnmpln_not_plan = non è un piano Little Navmap

# draft
geo_locate = Dov’è un luogo

# draft
geo_locate_description = Restituisce i luoghi con quel nome che Skwir conosce offline, dal più grande, con le loro posizioni: per script e agenti

# draft
geo_location_already_back = la posizione di { $name } era già stata ripristinata

# draft
geo_mavlink_empty = Non è una missione MAVLink: il file è vuoto

# draft
geo_mavlink_no_header = Non è una missione MAVLink: la prima riga non è «QGC WPL 110»

# draft
geo_metres = { $metres } m

# draft
geo_mission_exclusion_zone = Zona di esclusione

# draft
geo_mission_geofence = Geofence

# draft
geo_mission_home = Casa

# draft
geo_mission_survey = Rilevamento

# draft
geo_no_pane = nessun riquadro { $pane }

# draft
geo_no_picture_selected = Nessuna immagine selezionata

# draft
geo_no_picture_selected_because = Nessuna immagine selezionata: { $why }

# draft
geo_no_position = Nulla in questo file ha una posizione

# draft
geo_not_a_map = Non è un file di mappa

# draft
geo_nothing_along = Nessuna immagine da posizionare lungo { $track }

# draft
geo_nothing_along_because = Nessuna immagine da posizionare lungo { $track }: { $why }

# draft
geo_place_along = { $count ->
        [1] posiziona { $name } lungo { $track }
       *[other] posiziona { $count } immagini lungo { $track }
    }

# draft
geo_place_at = posiziona { $name } in { $lat }, { $lon }

# draft
geo_place_label = Luogo

# draft
geo_place_near = { $distance } { $direction } di { $place }

# draft
geo_placed_at = { $name } posizionato in { $lat }, { $lon }

# draft
geo_plan_too_large = Questo piano supera il limite di { $size } dell’anteprima

# draft
geo_pln_not_plan = non è un piano del simulatore di volo

# draft
geo_plugin_description = Mostra su una mappa tracce e percorsi GPS (GPX, KML, KMZ, GeoJSON, TCX, FIT, IGC, NMEA), piani di volo (Garmin, Flight Simulator, X-Plane, Little Navmap) e missioni di droni (QGroundControl, MAVLink, Litchi, DJI), con distanza, tempo e salita, e le immagini e i video di una cartella nella vista Mappa. Nulla di ciò a cui un file rimanda viene scaricato

# draft
geo_plugin_title = Mappe

# draft
geo_points = Dove sono i file

# draft
geo_points_description = Le immagini e le tracce di ciò che un riquadro elenca che sono state posizionate sulla mappa, dopo il primo `since`

# draft
geo_position = { $lat }, { $lon }

# draft
geo_qgc_invalid = Piano QGroundControl non valido: { $error }

# draft
geo_qgc_structure_scan = Scansione di struttura

# draft
geo_rally_point = R{ $number }

# draft
geo_read_first = Sono stati letti solo i primi { $size }

# draft
geo_release = Dimentica dove sono i file

# draft
geo_release_description = Smette di cercare dove sono i file di un riquadro: la sua vista Mappa è chiusa

# draft
geo_remove_location = Rimuovi le posizioni GPS delle immagini

# draft
geo_remove_location_description = Toglie la posizione dalle immagini selezionate (piano); quelle senza posizione restano come sono

# draft
geo_remove_location_menu = Rimuovi posizione GPS

# draft
geo_remove_location_of = rimuovi la posizione di { $name }

# draft
geo_remove_locations = { $count ->
        [1] rimuovi la posizione di { $name }
       *[other] rimuovi la posizione di { $count } immagini
    }

# draft
geo_removed_location_of = posizione di { $name } rimossa

# draft
geo_requirement = Scrittura e rimozione delle posizioni delle immagini

# draft
geo_restore_location_of = ripristina la posizione di { $name }

# draft
geo_restored_location_of = posizione di { $name } ripristinata

# draft
geo_scan = Trova dove sono i file

# draft
geo_scan_description = Trova le immagini e le tracce di ciò che un riquadro elenca che indicano dove si trovano, per la vista Mappa

# draft
geo_search_area = Cerca in quest’area

# draft
geo_search_area_description = Mostra le immagini e i video della cartella e delle sue sottocartelle realizzati all’interno di un’area

# draft
geo_search_subfolders = Cerca luoghi nelle sottocartelle

# draft
geo_search_subfolders_description = Mostra le immagini e le tracce della cartella e di tutte le sue sottocartelle

# draft
geo_select_one_track = Seleziona una sola traccia: { $count ->
       *[other] ne sono selezionate { $count }
    }

# draft
geo_select_pictures = Seleziona le immagini di cui rimuovere la posizione

# draft
geo_select_track = Seleziona una traccia e le immagini da posizionare lungo di essa

# draft
geo_speed = { $speed } km/h

# draft
geo_stop_named = { $ident } ({ $name })

# draft
geo_summary_flight = Piano di volo

# draft
geo_summary_flight_route = Piano di volo · { $from } → { $to }

# draft
geo_summary_mission = Missione drone

# draft
geo_summary_mission_waypoints = Missione drone · { $count ->
        [one] { $count } waypoint
       *[other] { $count } waypoint
    }

# draft
geo_text_line_not_text = la riga { $line } non è testo

# draft
geo_text_line_too_long = la riga { $line } è più lunga di { $size }

# draft
geo_thinned = La mappa mostra un punto ogni { $stride }

# draft
geo_title = { $format } · { $contents }

# draft
geo_track_no_times = { $name } non ha orari: solo una traccia registrata può posizionare le immagini

# draft
geo_unreadable = impossibile leggere { $name }: { $error }

# draft
geo_view = Mappa

# draft
geo_wpml_first_waypoint = Primo waypoint

# draft
geo_wpml_takeoff = Decollo

# draft
geo_wpml_unreadable = Questa missione non può essere letta

# draft
geo_xml_end_closes_nothing = un tag di chiusura non chiude nulla

# draft
geo_xml_error_at = { $error } (al byte { $byte })

# draft
geo_xml_text_too_long = un elemento contiene più di { $size } di testo

# draft
geo_xml_too_deep = gli elementi sono annidati più in profondità di { $depth }
