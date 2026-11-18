# Simplified Chinese catalog of skwir-plugin-models, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
models_bgcode_title = 二进制 G-code

# draft
models_blend_ends_early = Blender 文件提前结束

# draft
models_blend_newer = 由比此读取器更新的 Blender 版本保存

# draft
models_blend_picture_damaged = Blender 文件中的图片已损坏

# draft
models_blend_title = Blender 文件

# draft
models_blend_unknown_header = 未知的 Blender 文件头

# draft
models_detail_blocks = 块

# draft
models_detail_entities = 图元

# draft
models_detail_faces = 面

# draft
models_detail_filament = 耗材

# draft
models_detail_filament_length = 耗材长度

# draft
models_detail_format = 格式

# draft
models_detail_frames = 帧

# draft
models_detail_layer_height = 层高

# draft
models_detail_layers = 层数

# draft
models_detail_made_with = 制作工具

# draft
models_detail_material = 材料

# draft
models_detail_objects = 对象

# draft
models_detail_pages = 页数

# draft
models_detail_plates = 打印板

# draft
models_detail_print_time = 打印时间

# draft
models_detail_printer = 打印机

# draft
models_detail_saved_by = 保存程序

# draft
models_detail_scenes = 场景

# draft
models_detail_scenes_named = 场景

# draft
models_detail_size = 尺寸

# draft
models_detail_sliced_with = 切片软件

# draft
models_detail_title = 标题

# draft
models_detail_triangles = 三角形

# draft
models_detail_unit = 单位

# draft
models_detail_volume = 体积

# draft
models_detail_watertight = 水密

# draft
models_document_title = { $program } 文档

# draft
models_drawing_exceeds = 图纸超出了 { $size } 的预览上限

# draft
models_drawing_title = { $format } 图纸

# draft
models_duration_hours = { $h } 小时 { $m } 分 { $s } 秒

# draft
models_duration_minutes = { $m } 分 { $s } 秒

# draft
models_duration_seconds = { $s } 秒

# draft
models_dxf_drawn_in_part = 仅绘制了一部分：图纸包含的内容多于此处绘制的内容

# draft
models_faces_missing = { $count } 个面中有 { $missing } 个无法绘制

# draft
models_filament_metres = { $metres } m

# draft
models_frames = { $first }–{ $last }

# draft
models_grams = { $grams } g

# draft
models_install_worker = 安装 { $program } 以绘制未存储图片的模型

# draft
models_layer_height_mm = { $mm } mm

# draft
models_member_missing = 缺少 { $member }

# draft
models_member_too_large = { $member } 超出了 { $size } 的预览上限

# draft
models_model_title = { $format } 模型

# draft
models_no_picture = 此文件中没有存储图片

# draft
models_not_bgcode = 不是二进制 G-code 文件

# draft
models_not_blend = 不是 Blender 文件

# draft
models_not_dwg = 不是 DWG 图纸

# draft
models_not_obj = 不是 Wavefront OBJ 模型

# draft
models_not_rhino = 不是 Rhino 模型

# draft
models_not_sketchup = 不是 SketchUp 模型

# draft
models_object_file = 目标文件

# draft
models_objects_unmeshed = { $count } 个对象未存储网格，因此未绘制

# draft
models_package_members = 此包包含的成员超过 { $count } 个

# draft
models_plugin_description = 根据文件中存储的图片显示 Blender 文件、3MF 包、FreeCAD 文档、SketchUp 和 Rhino 模型、DWG 图纸以及 G-code 打印文件，并绘制 DXF 图纸、G-code 工具路径、Rhino 模型保存的网格以及 STL、OBJ、PLY、OFF、glTF、STEP 和 IGES 模型；在属性中显示打印时间、耗材、尺寸、体积和版本

# draft
models_plugin_title = 3D 模型与打印

# draft
models_rhino_damaged = Rhino 模型已损坏

# draft
models_rhino_format = 3dm 版本 { $version }

# draft
models_rhino_properties_too_large = 模型的属性大于 { $size }

# draft
models_shown_as_picture = 以图片显示：它有 { $millions } 百万个三角形。按“3D”仍可旋转查看

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
models_too_large = 过大，无法绘制（超过 { $size }）

# draft
models_toolpath_no_size = 图像没有尺寸

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
models_watertight_no = 否

# draft
models_watertight_yes = 是

# draft
models_worker_purpose = 绘制和测量未存储图片的 3D 模型
