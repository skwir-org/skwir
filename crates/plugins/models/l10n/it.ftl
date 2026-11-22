# Italian catalog of skwir-plugin-models, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
models_bgcode_title = G-code binario

# draft
models_blend_ends_early = Il file Blender termina prematuramente

# draft
models_blend_newer = Salvato da una versione di Blender più recente di questo lettore

# draft
models_blend_picture_damaged = L’immagine del file Blender è danneggiata

# draft
models_blend_title = File Blender

# draft
models_blend_unknown_header = Intestazione Blender sconosciuta

# draft
models_detail_blocks = Blocchi

# draft
models_detail_entities = Entità

# draft
models_detail_faces = Facce

# draft
models_detail_filament = Filamento

# draft
models_detail_filament_length = Lunghezza del filamento

# draft
models_detail_format = Formato

# draft
models_detail_frames = Fotogrammi

# draft
models_detail_layer_height = Altezza dello strato

# draft
models_detail_layers = Strati

# draft
models_detail_made_with = Creato con

# draft
models_detail_material = Materiale

# draft
models_detail_objects = Oggetti

# draft
models_detail_pages = Pagine

# draft
models_detail_plates = Piatti

# draft
models_detail_print_time = Tempo di stampa

# draft
models_detail_printer = Stampante

# draft
models_detail_saved_by = Salvato da

# draft
models_detail_scenes = Scene

# draft
models_detail_scenes_named = { $count ->
        [1] Scena
       *[other] Scene
    }

# draft
models_detail_size = Dimensione

# draft
models_detail_sliced_with = Elaborato con

# draft
models_detail_title = Titolo

# draft
models_detail_triangles = Triangoli

# draft
models_detail_unit = Unità

# draft
models_detail_volume = Volume

# draft
models_detail_watertight = A tenuta stagna

# draft
models_document_title = Documento { $program }

# draft
models_drawing_exceeds = Il disegno supera il limite di anteprima di { $size }

# draft
models_drawing_title = Disegno { $format }

# draft
models_duration_hours = { $h }h { $m }m { $s }s

# draft
models_duration_minutes = { $m }m { $s }s

# draft
models_duration_seconds = { $s }s

# draft
models_dxf_drawn_in_part = Disegnato in parte: il disegno contiene più di quanto è mostrato qui

# draft
models_faces_missing = { $missing ->
        [one] Non è stato possibile disegnare { $missing } faccia su { $count }
       *[other] Non è stato possibile disegnare { $missing } facce su { $count }
    }

# draft
models_filament_metres = { $metres } m

# draft
models_frames = { $first }–{ $last }

# draft
models_grams = { $grams } g

# draft
models_install_worker = Installa { $program } per disegnare i modelli che non contengono un’immagine

# draft
models_layer_height_mm = { $mm } mm

# draft
models_member_missing = { $member } manca

# draft
models_member_too_large = { $member } supera il limite di anteprima di { $size }

# draft
models_model_title = Modello { $format }

# draft
models_no_picture = Questo file non contiene alcuna immagine

# draft
models_not_bgcode = Non è un file G-code binario

# draft
models_not_blend = Non è un file Blender

# draft
models_not_dwg = Non è un disegno DWG

# draft
models_not_obj = Non è un modello Wavefront OBJ

# draft
models_not_rhino = Non è un modello Rhino

# draft
models_not_sketchup = Non è un modello SketchUp

# draft
models_object_file = File oggetto

# draft
models_objects_unmeshed = { $count ->
        [one] { $count } oggetto non contiene alcuna mesh e non viene disegnato
       *[other] { $count } oggetti non contengono alcuna mesh e non vengono disegnati
    }

# draft
models_package_members = { $count ->
       *[other] Il pacchetto contiene più di { $count } membri
    }

# draft
models_plugin_description = Mostra file Blender, pacchetti 3MF, documenti FreeCAD, modelli SketchUp e Rhino, disegni DWG e stampe G-code tramite l’immagine contenuta al loro interno, e disegna i disegni DXF, i percorsi utensile G-code, le mesh contenute nei modelli Rhino e i modelli STL, OBJ, PLY, OFF, glTF, STEP e IGES; tempo di stampa, filamento, dimensione, volume e versione in Proprietà

# draft
models_plugin_title = Modelli e stampe 3D

# draft
models_rhino_damaged = Modello Rhino danneggiato

# draft
models_rhino_format = 3dm versione { $version }

# draft
models_rhino_properties_too_large = Le proprietà del modello superano { $size }

# draft
models_shown_as_picture = Mostrato come immagine: ha { $millions } milioni di triangoli. Il pulsante 3D lo elabora comunque

# draft
models_size = { $unit ->
        [mm] { $x } × { $y } × { $z } mm
        [cm] { $x } × { $y } × { $z } cm
        [m] { $x } × { $y } × { $z } m
        [in] { $x } × { $y } × { $z } in
        [ft] { $x } × { $y } × { $z } ft
       *[other] { $x } × { $y } × { $z } { $unit }
    }

# draft
models_size_2d = { $unit ->
        [in] { $width } × { $height } in
        [ft] { $width } × { $height } ft
        [mi] { $width } × { $height } mi
        [mm] { $width } × { $height } mm
        [cm] { $width } × { $height } cm
        [m] { $width } × { $height } m
        [km] { $width } × { $height } km
        [mil] { $width } × { $height } mil
        [yd] { $width } × { $height } yd
        [dm] { $width } × { $height } dm
       *[other] { $width } × { $height } { $unit }
    }

# draft
models_size_2d_unitless = { $width } × { $height }

# draft
models_size_unitless = { $x } × { $y } × { $z }

# draft
models_too_large = Troppo grande per essere disegnato (oltre { $size })

# draft
models_toolpath_no_size = L’immagine non ha dimensioni

# draft
models_volume_cm3 = { $volume } cm³

# draft
models_volume_ft3 = { $volume } ft³

# draft
models_volume_in3 = { $volume } in³

# draft
models_volume_m3 = { $volume } m³

# draft
models_volume_mm3 = { $volume } mm³

# draft
models_watertight_no = No

# draft
models_watertight_yes = Sì

# draft
models_worker_purpose = Modelli 3D senza immagine incorporata disegnati e misurati
