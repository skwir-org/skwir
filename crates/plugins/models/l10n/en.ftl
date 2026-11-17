# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# The title of the preview of Prusa's binary G-code (.bgcode), a 3D print's instructions.
models_bgcode_title = Binary G-code

# An error in place of a Blender file cut short.
models_blend_ends_early = The Blender file ends early

# An error in place of a Blender file whose header is laid out as no version of Blender known here writes it.
models_blend_newer = Saved by a Blender newer than this reader

# An error in place of the picture Blender stored in a file, when it cannot be read.
models_blend_picture_damaged = The Blender file's picture is damaged

# The title of a Blender file's preview.
models_blend_title = Blender file

# An error in place of a Blender file whose header does not say its version as Blender writes it.
models_blend_unknown_header = Unknown Blender header

# A label in the preview's Properties: how many blocks (named groups of entities, placed again and again) a DXF drawing defines.
models_detail_blocks = Blocks

# A label in the preview's Properties: how many entities (lines, arcs, texts…) a DXF drawing holds.
models_detail_entities = Entities

# A label in the preview's Properties: how many surfaces a CAD model (STEP, IGES) is made of.
models_detail_faces = Faces

# A label in the preview's Properties: how much plastic filament a 3D print uses, in grams.
models_detail_filament = Filament

# A label in the preview's Properties: how much plastic filament a 3D print uses, in metres.
models_detail_filament_length = Filament length

# A label in the preview's Properties: the file's format and its version (DWG 2018, STEP AP214).
models_detail_format = Format

# A label in the preview's Properties: the first and last frame of a Blender file's first scene.
models_detail_frames = Frames

# A label in the preview's Properties: how thick each layer of a 3D print is.
models_detail_layer_height = Layer height

# A label in the preview's Properties: how many layers a 3D print has.
models_detail_layers = Layers

# A label in the preview's Properties: the program that wrote the file.
models_detail_made_with = Made with

# A label in the preview's Properties: the filament's material a 3D print is sliced for (PLA, PETG).
models_detail_material = Material

# A label in the preview's Properties: how many separate objects the model holds.
models_detail_objects = Objects

# A label in the preview's Properties: how many pages a LayOut document has.
models_detail_pages = Pages

# A label in the preview's Properties: how many build plates a 3D print's project holds.
models_detail_plates = Plates

# A label in the preview's Properties: how long a 3D print takes, as its slicer predicted.
models_detail_print_time = Print time

# A label in the preview's Properties: the 3D printer the print was sliced for.
models_detail_printer = Printer

# A label in the preview's Properties: the program and its version that saved the file (Blender 4.2).
models_detail_saved_by = Saved by

# A label in the preview's Properties: how many scenes (saved views) a SketchUp model holds.
models_detail_scenes = Scenes

# A label in the preview's Properties: the names of the scenes a Blender file holds. count: how many.
models_detail_scenes_named = { $count ->
        [1] Scene
       *[other] Scenes
    }

# A label in the preview's Properties: the model's or the drawing's dimensions, not its size in bytes.
models_detail_size = Size

# A label in the preview's Properties: the program (a slicer) that turned the model into this print's G-code.
models_detail_sliced_with = Sliced with

# A label in the preview's Properties: the model's title, as its 3MF package states it.
models_detail_title = Title

# A label in the preview's Properties: how many triangles the model's mesh has.
models_detail_triangles = Triangles

# A label in the preview's Properties: the length unit a 3MF model states, as the file writes it (millimeter, inch).
models_detail_unit = Unit

# A label in the preview's Properties: how much space the 3D model encloses.
models_detail_volume = Volume

# A label in the preview's Properties: whether the model's mesh is closed, with no holes, as a 3D print needs.
models_detail_watertight = Watertight

# The title of a document's preview. program: the program that makes it (FreeCAD, LayOut).
models_document_title = { $program } document

# Why a DXF drawing is not drawn: it grew past what the preview reads while it was read. size: that bound (33.6 MB).
models_drawing_exceeds = The drawing exceeds { $size } preview limit

# The title of an AutoCAD drawing's preview. format: the format's name (DWG, DXF).
models_drawing_title = { $format } drawing

# A print's time, in the preview's Properties, as slicers write it: h for hours, m for minutes, s for seconds.
models_duration_hours = { $h }h { $m }m { $s }s

# A print's time under an hour, in the preview's Properties, as slicers write it: m for minutes, s for seconds.
models_duration_minutes = { $m }m { $s }s

# A print's time under a minute, in the preview's Properties, as slicers write it: s for seconds.
models_duration_seconds = { $s }s

# Said under a DXF drawing too large to draw whole: what is past the preview's bounds is left out.
models_dxf_drawn_in_part = Drawn in part: the drawing holds more than is drawn here

# Said under a CAD model drawn with holes: some of its surfaces could not be turned into triangles. missing: how many; count: how many surfaces the model has.
models_faces_missing = { $missing } of { $count ->
       *[other] { $count } faces
    } could not be drawn

# A length of filament in metres, in the preview's Properties. metres: the number.
models_filament_metres = { $metres } m

# The range of frames of a Blender scene, in the preview's Properties (1–250). first, last: frame numbers.
models_frames = { $first }–{ $last }

# A weight in grams, in the preview's Properties. grams: the number.
models_grams = { $grams } g

# Said in the preview of a 3MF model that stores no picture of itself. program: the helper program's name (skwir-model-worker).
models_install_worker = Install { $program } to draw models that store no picture

# A print's layer height in millimetres, in the preview's Properties. mm: the number.
models_layer_height_mm = { $mm } mm

# Why a part of a model's package is not read: the package has none by that name. member: its name in the package, as written.
models_member_missing = { $member } is missing

# Why a part of a model's package is not read: it inflates past what the preview reads. member: its name in the package, as written; size: that bound.
models_member_too_large = { $member } exceeds { $size } preview limit

# The title of a 3D model's preview. format: the format's or the program's name (STL, glTF, STEP, Rhino, SketchUp).
models_model_title = { $format } model

# Said in the preview of a model or drawing whose program stored no picture of it.
models_no_picture = No picture is stored in this file

# An error in place of a .bgcode file that is not Prusa's binary G-code.
models_not_bgcode = Not a binary G-code file

# An error in place of a .blend file that is not Blender's.
models_not_blend = Not a Blender file

# An error in place of a .dwg file that is not an AutoCAD drawing.
models_not_dwg = Not a DWG drawing

# Said in the preview of a .obj file that is not a 3D model (Wavefront OBJ is the format's name).
models_not_obj = Not a Wavefront OBJ model

# An error in place of a .3dm file that is not a Rhino model.
models_not_rhino = Not a Rhino model

# An error in place of a .skp file that is not a SketchUp model.
models_not_sketchup = Not a SketchUp model

# The preview's title for a .obj file that is a compiled program's object, not a 3D model.
models_object_file = Object file

# Said under a Rhino model drawn from the meshes it keeps: objects that keep none are left out.
models_objects_unmeshed = { $count ->
        [one] { $count } object stores no mesh and is not drawn
       *[other] { $count } objects store no mesh and are not drawn
    }

# Why a model's package (3MF, SketchUp, LayOut) is not shown: it holds more parts than the preview reads (4,000).
models_package_members = { $count ->
       *[other] The package holds more than { $count } members
    }

models_plugin_description = Show Blender files, 3MF packages, FreeCAD documents, SketchUp and Rhino models, DWG drawings and G-code prints by the picture stored in them, and draw DXF drawings, G-code toolpaths, the meshes Rhino models keep and STL, OBJ, PLY, OFF, glTF, STEP and IGES models; print time, filament, size, volume and version in Properties

models_plugin_title = 3D models and prints

# An error in place of a Rhino model whose structure cannot be read.
models_rhino_damaged = Damaged Rhino model

# A Rhino model's format in the preview's Properties: 3dm is the format's name. version: its version's number (5, 6, 7, 8).
models_rhino_format = 3dm version { $version }

# An error in place of a Rhino model whose table of properties (where its picture is) is larger than the preview reads. size: that bound.
models_rhino_properties_too_large = The model's properties are larger than { $size }

# Said under a model too large to turn at once: a still picture is shown, and the 3D button turns it anyway. millions: the number of triangles in millions (2.4).
models_shown_as_picture = Shown as a picture: it has { $millions } million triangles. 3D turns it all the same

# A 3D model's width, depth and height in the preview's Properties. unit: the length unit the file states (mm, cm, m, in, ft, or µm, which the last variant writes as the file's unit).
models_size = { $unit ->
        [mm] { $x } × { $y } × { $z } mm
        [cm] { $x } × { $y } × { $z } cm
        [m] { $x } × { $y } × { $z } m
        [in] { $x } × { $y } × { $z } in
        [ft] { $x } × { $y } × { $z } ft
       *[other] { $x } × { $y } × { $z } { $unit }
    }

# A drawing's width and height in the preview's Properties. unit: the length unit the drawing states (in, ft, mi, mm, cm, m, km, mil, yd, dm, or µm, which the last variant writes as the drawing's unit).
models_size_2d = { $unit ->
        [in] { $width } × { $height } in
        [ft] { $width } × { $height } ft
        [mi] { $width } × { $height } mi
        [mm] { $width } × { $height } mm
        [cm] { $width } × { $height } cm
        [m] { $width } × { $height } m
        [km] { $width } × { $height } km
        [mil] { $width } × { $height } mil
        [yd] { $width } × { $height } yd
        [dm] { $width } × { $height } dm
       *[other] { $width } × { $height } { $unit }
    }

# A drawing's width and height in the preview's Properties, in a drawing that states no unit.
models_size_2d_unitless = { $width } × { $height }

# A 3D model's width, depth and height in the preview's Properties, in a file that states no unit.
models_size_unitless = { $x } × { $y } × { $z }

# Why a model or a drawing is not drawn. size: the largest file drawn (16 MB).
models_too_large = Too large to draw (more than { $size })

# An error: a G-code program's moves could not be drawn, as the picture they make would be empty.
models_toolpath_no_size = The picture has no size

# A model's volume in cubic centimetres, in the preview's Properties. volume: the number.
models_volume_cm3 = { $volume } cm³

# A model's volume in cubic feet, in the preview's Properties. volume: the number.
models_volume_ft3 = { $volume } ft³

# A model's volume in cubic inches, in the preview's Properties. volume: the number.
models_volume_in3 = { $volume } in³

# A model's volume in cubic metres, in the preview's Properties. volume: the number.
models_volume_m3 = { $volume } m³

# A model's volume in cubic millimetres, in the preview's Properties. volume: the number.
models_volume_mm3 = { $volume } mm³

# The Watertight row's value in the preview's Properties: the model's mesh has holes.
models_watertight_no = No

# The Watertight row's value in the preview's Properties: the model's mesh is closed.
models_watertight_yes = Yes

# What the 3D model helper program is for, in the plugin's requirements in Settings.
models_worker_purpose = 3D models without a stored picture drawn and measured
