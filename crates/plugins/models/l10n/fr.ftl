# French catalog of skwir-plugin-models, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
models_bgcode_title = G-code binaire

# draft
models_blend_ends_early = Le fichier Blender se termine trop tôt

# draft
models_blend_newer = Enregistré par un Blender plus récent que ce lecteur

# draft
models_blend_picture_damaged = L’image du fichier Blender est endommagée

# draft
models_blend_title = Fichier Blender

# draft
models_blend_unknown_header = En-tête Blender inconnu

# draft
models_detail_blocks = Blocs

# draft
models_detail_entities = Entités

# draft
models_detail_faces = Faces

# draft
models_detail_filament = Filament

# draft
models_detail_filament_length = Longueur de filament

# draft
models_detail_format = Format

# draft
models_detail_frames = Images

# draft
models_detail_layer_height = Hauteur de couche

# draft
models_detail_layers = Couches

# draft
models_detail_made_with = Créé avec

# draft
models_detail_material = Matériau

# draft
models_detail_objects = Objets

# draft
models_detail_pages = Pages

# draft
models_detail_plates = Plateaux

# draft
models_detail_print_time = Durée d’impression

# draft
models_detail_printer = Imprimante

# draft
models_detail_saved_by = Enregistré par

# draft
models_detail_scenes = Scènes

# draft
models_detail_scenes_named = { $count ->
        [1] Scène
        [one] Scène
       *[other] Scènes
    }

# draft
models_detail_size = Dimensions

# draft
models_detail_sliced_with = Découpé avec

# draft
models_detail_title = Titre

# draft
models_detail_triangles = Triangles

# draft
models_detail_unit = Unité

# draft
models_detail_volume = Volume

# draft
models_detail_watertight = Étanche

# draft
models_document_title = Document { $program }

# draft
models_drawing_exceeds = Le dessin dépasse la limite d’aperçu de { $size }

# draft
models_drawing_title = Dessin { $format }

# draft
models_duration_hours = { $h } h { $m } min { $s } s

# draft
models_duration_minutes = { $m } min { $s } s

# draft
models_duration_seconds = { $s } s

# draft
models_dxf_drawn_in_part = Dessin partiel : le dessin contient plus que ce qui est dessiné ici

# draft
models_faces_missing = { $missing ->
        [one] { $missing } face sur { $count } n’a pas pu être dessinée
       *[other] { $missing } faces sur { $count } n’ont pas pu être dessinées
    }

# draft
models_filament_metres = { $metres } m

# draft
models_frames = { $first }–{ $last }

# draft
models_grams = { $grams } g

# draft
models_install_worker = Installez { $program } pour dessiner les modèles qui ne contiennent pas d’image

# draft
models_layer_height_mm = { $mm } mm

# draft
models_member_missing = { $member } est introuvable

# draft
models_member_too_large = { $member } dépasse la limite d’aperçu de { $size }

# draft
models_model_title = Modèle { $format }

# draft
models_no_picture = Aucune image n’est enregistrée dans ce fichier

# draft
models_not_bgcode = Ce n’est pas un fichier G-code binaire

# draft
models_not_blend = Ce n’est pas un fichier Blender

# draft
models_not_dwg = Ce n’est pas un dessin DWG

# draft
models_not_obj = Ce n’est pas un modèle Wavefront OBJ

# draft
models_not_rhino = Ce n’est pas un modèle Rhino

# draft
models_not_sketchup = Ce n’est pas un modèle SketchUp

# draft
models_object_file = Fichier objet

# draft
models_objects_unmeshed = { $count ->
        [one] { $count } objet ne contient aucun maillage et n’est pas dessiné
       *[other] { $count } objets ne contiennent aucun maillage et ne sont pas dessinés
    }

# draft
models_package_members = { $count ->
       *[other] Le paquet contient plus de { $count } parties
    }

# draft
models_plugin_description = Afficher les fichiers Blender, paquets 3MF, documents FreeCAD, modèles SketchUp et Rhino, dessins DWG et impressions G-code par l’image qu’ils contiennent, et dessiner les dessins DXF, les parcours d’outil G-code, les maillages que gardent les modèles Rhino et les modèles STL, OBJ, PLY, OFF, glTF, STEP et IGES ; durée d’impression, filament, dimensions, volume et version dans Propriétés

# draft
models_plugin_title = Modèles 3D et impressions

# draft
models_rhino_damaged = Modèle Rhino endommagé

# draft
models_rhino_format = 3dm version { $version }

# draft
models_rhino_properties_too_large = Les propriétés du modèle dépassent { $size }

# draft
models_shown_as_picture = Affiché en image : il compte { $millions } millions de triangles. Le bouton 3D le fait tourner malgré tout

# draft
models_size = { $unit ->
        [mm] { $x } × { $y } × { $z } mm
        [cm] { $x } × { $y } × { $z } cm
        [m] { $x } × { $y } × { $z } m
        [in] { $x } × { $y } × { $z } in
        [ft] { $x } × { $y } × { $z } ft
       *[other] { $x } × { $y } × { $z } { $unit }
    }

# draft
models_size_2d = { $unit ->
        [in] { $width } × { $height } in
        [ft] { $width } × { $height } ft
        [mi] { $width } × { $height } mi
        [mm] { $width } × { $height } mm
        [cm] { $width } × { $height } cm
        [m] { $width } × { $height } m
        [km] { $width } × { $height } km
        [mil] { $width } × { $height } mil
        [yd] { $width } × { $height } yd
        [dm] { $width } × { $height } dm
       *[other] { $width } × { $height } { $unit }
    }

# draft
models_size_2d_unitless = { $width } × { $height }

# draft
models_size_unitless = { $x } × { $y } × { $z }

# draft
models_too_large = Trop volumineux pour être dessiné (plus de { $size })

# draft
models_toolpath_no_size = L’image n’a pas de dimensions

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
models_watertight_no = Non

# draft
models_watertight_yes = Oui

# draft
models_worker_purpose = Dessin et mesure des modèles 3D sans image enregistrée
