# Simplified Chinese catalog of skwir-plugin-geo, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
geo_already_at = { $name }已位于 { $lat }, { $lon }

# draft
geo_already_no_location = { $name }已没有位置信息

# draft
geo_altitude_flight = { $feet } ft · { $metres } m

# draft
geo_altitude_flight_ground = { $feet } ft · 离地 { $metres } m

# draft
geo_altitude_flight_takeoff = { $feet } ft · 相对起飞点 { $metres } m

# draft
geo_altitude_ground = 离地 { $metres } m

# draft
geo_altitude_takeoff = 相对起飞点 { $metres } m

# draft
geo_changed_since = { $name }在计划制定后已被更改

# draft
geo_clocks_corrected = { $sign ->
        [plus] 相机时钟已校正 +{ $seconds } s
       *[minus] 相机时钟已校正 −{ $seconds } s
    }

# draft
geo_compass_e = 东

# draft
geo_compass_n = 北

# draft
geo_compass_ne = 东北

# draft
geo_compass_nw = 西北

# draft
geo_compass_s = 南

# draft
geo_compass_se = 东南

# draft
geo_compass_sw = 西南

# draft
geo_compass_w = 西

# draft
geo_contents_areas = { $count } 个区域

# draft
geo_contents_routes = { $count } 条路线

# draft
geo_contents_tracks = { $count } 条轨迹

# draft
geo_contents_waypoints = { $count } 个航点

# draft
geo_damaged = 文件已损坏（{ $why }）；显示的是损坏处之前的内容

# draft
geo_dropped_places = 另有 { $count } 个地点未在地图上显示

# draft
geo_exiftool_cannot_write = ExifTool 无法将位置写入{ $name }

# draft
geo_exiftool_could_not_write = ExifTool 未能写入{ $name }：{ $why }

# draft
geo_exiftool_missing = 安装 ExifTool 以写入图片的位置

# draft
geo_exiftool_unreadable = ExifTool 未能读取它

# draft
geo_fact_areas = 区域

# draft
geo_fact_climb = 累计爬升

# draft
geo_fact_created_by = 创建者

# draft
geo_fact_cruise = 巡航高度

# draft
geo_fact_departure = 出发地

# draft
geo_fact_descent = 累计下降

# draft
geo_fact_description = 描述

# draft
geo_fact_destination = 目的地

# draft
geo_fact_distance = 距离

# draft
geo_fact_duration = 时长

# draft
geo_fact_ended = 结束时间

# draft
geo_fact_flight_height = 飞行高度

# draft
geo_fact_highest_point = 最高点

# draft
geo_fact_highest_waypoint = 最高航点

# draft
geo_fact_legs = 航段

# draft
geo_fact_lowest_point = 最低点

# draft
geo_fact_moving_speed = 移动速度

# draft
geo_fact_moving_time = 移动时间

# draft
geo_fact_name = 名称

# draft
geo_fact_points = 点数

# draft
geo_fact_route_distance = 路线距离

# draft
geo_fact_routes = 路线

# draft
geo_fact_start = 起点

# draft
geo_fact_started = 开始时间

# draft
geo_fact_tracks = 轨迹

# draft
geo_fact_waypoints = 航点

# draft
geo_fit_byte_order = 某条消息的字节序未知

# draft
geo_fit_checksum_mismatch = 文件的校验和与其内容不符

# draft
geo_fit_ends_in_record = 文件在某条记录的中间结束

# draft
geo_fit_no_checksum = 文件在校验和之前就已结束

# draft
geo_fit_not_fit = 这不是 FIT 文件

# draft
geo_fit_record_past_end = 某条记录超出了数据的末尾

# draft
geo_fit_trailing = 数据之后跟有并非 FIT 文件的内容

# draft
geo_fit_undefined_message = 某条消息的类型从未定义

# draft
geo_fit_unfinished = 记录未完成

# draft
geo_flight_distance = { $distance } · { $nautical } NM

# draft
geo_fms_fewer_waypoints = 计划列出 { $expected } 个航点，实际只有 { $read } 个

# draft
geo_fms_not_plan = 不是 X-Plane 飞行计划

# draft
geo_fms_version = 版本为 { $version } 的 X-Plane 飞行计划，无法读取

# draft
geo_format_garmin_plan = Garmin 飞行计划

# draft
geo_format_litchi_mission = Litchi 任务

# draft
geo_format_little_navmap_plan = Little Navmap 计划

# draft
geo_format_mavlink_mission = MAVLink 任务

# draft
geo_format_qgc_plan = QGroundControl 计划

# draft
geo_format_sim_plan = Flight Simulator 计划

# draft
geo_format_xplane_plan = X-Plane 计划

# draft
geo_fpl_not_plan = 不是 Garmin 飞行计划

# draft
geo_geojson_invalid = 不是有效的 GeoJSON：{ $error }

# draft
geo_geojson_too_large = 此 GeoJSON 文件超出了 { $size } 的预览上限

# draft
geo_geotag = 按轨迹为图片添加地理标记

# draft
geo_geotag_description = 沿所选的那一条轨迹（GPX、FIT 或任何可读取的轨迹）放置所选图片：在轨迹记录期间拍摄的每张图片都会获得其拍摄时刻的位置；提交审阅。在工作线程上运行：结果以提议或通知的形式送达

# draft
geo_geotag_menu = 按轨迹添加地理标记

# draft
geo_gone = { $name }已不存在

# draft
geo_holds_no_track = { $name }中没有轨迹

# draft
geo_invalid_points = 已略去 { $count } 个点：不是有效的位置

# draft
geo_kilometres = { $km } km

# draft
geo_kmz_members = 此 KMZ 包含的成员超过 { $count } 个

# draft
geo_kmz_no_kml = 此 KMZ 中没有 KML 文档

# draft
geo_kmz_unreadable = 无法读取此 KMZ 中的文档

# draft
geo_left_others = 其他 { $count } 个文件

# draft
geo_left_out = 已略去：{ $what }

# draft
geo_left_outside = { $count } 张超出轨迹时间范围的图片

# draft
geo_left_placed = { $count } 张已有位置的图片（保持原样）

# draft
geo_left_raw = { $count } 张 RAW 图片（尚不写入其附属文件）

# draft
geo_left_undated = { $count } 张没有拍摄时间的图片

# draft
geo_left_unreadable = { $count } 张无法读取的图片

# draft
geo_left_unwritable = { $count } 张 ExifTool 无法写入的图片

# draft
geo_litchi_columns = 不是 Litchi 任务：CSV 的列不是任务的列

# draft
geo_litchi_empty = 不是 Litchi 任务：文件为空

# draft
geo_litchi_point_of_interest = 兴趣点

# draft
geo_lnmpln_not_plan = 不是 Little Navmap 计划

# draft
geo_locate = 查找地点

# draft
geo_locate_description = 返回 Skwir 离线所知的该名称的地点及其位置，规模最大的在前：供脚本和智能体使用

# draft
geo_location_already_back = { $name }的位置已经恢复

# draft
geo_mavlink_empty = 不是 MAVLink 任务：文件为空

# draft
geo_mavlink_no_header = 不是 MAVLink 任务：其第一行不是“QGC WPL 110”

# draft
geo_metres = { $metres } m

# draft
geo_mission_exclusion_zone = 禁飞区

# draft
geo_mission_geofence = 地理围栏

# draft
geo_mission_home = 返航点

# draft
geo_mission_survey = 测绘区域

# draft
geo_no_pane = 没有窗格 { $pane }

# draft
geo_no_picture_selected = 未选择图片

# draft
geo_no_picture_selected_because = 未选择图片：{ $why }

# draft
geo_no_position = 此文件中没有任何带位置的内容

# draft
geo_not_a_map = 不是地图文件

# draft
geo_nothing_along = 没有可沿{ $track }放置的图片

# draft
geo_nothing_along_because = 没有可沿{ $track }放置的图片：{ $why }

# draft
geo_place_along = { $count ->
        [1] 沿{ $track }放置{ $name }
       *[other] 沿{ $track }放置 { $count } 张图片
    }

# draft
geo_place_at = 将{ $name }放置在 { $lat }, { $lon }

# draft
geo_place_label = 地点

# draft
geo_place_near = { $place } { $direction } { $distance }

# draft
geo_placed_at = 已将{ $name }放置在 { $lat }, { $lon }

# draft
geo_plan_too_large = 此计划超出了 { $size } 的预览上限

# draft
geo_pln_not_plan = 不是 Flight Simulator 计划

# draft
geo_plugin_description = 在地图上显示 GPS 轨迹和路线（GPX、KML、KMZ、GeoJSON、TCX、FIT、IGC、NMEA）、飞行计划（Garmin、Flight Simulator、X-Plane、Little Navmap）和无人机任务（QGroundControl、MAVLink、Litchi、DJI），以及其距离、时间和爬升，并在地图视图中显示文件夹中的图片和视频。不获取文件所链接的任何内容

# draft
geo_plugin_title = 地图

# draft
geo_points = 文件所在位置

# draft
geo_points_description = 窗格所列内容中已放置到地图上的图片和轨迹，跳过前 `since` 个

# draft
geo_position = { $lat }, { $lon }

# draft
geo_qgc_invalid = 不是有效的 QGroundControl 计划：{ $error }

# draft
geo_qgc_structure_scan = 结构扫描

# draft
geo_rally_point = R{ $number }

# draft
geo_read_first = 只读取了前 { $size }

# draft
geo_release = 忘记文件所在位置

# draft
geo_release_description = 停止查找窗格中文件的位置：其地图视图已关闭

# draft
geo_remove_location = 移除图片的 GPS 位置

# draft
geo_remove_location_description = 从所选图片中移除位置信息（计划）；没有位置信息的图片保持不变

# draft
geo_remove_location_menu = 移除 GPS 位置

# draft
geo_remove_location_of = 移除{ $name }的位置

# draft
geo_remove_locations = { $count ->
        [1] 移除{ $name }的位置
       *[other] 移除 { $count } 张图片的位置
    }

# draft
geo_removed_location_of = 已移除{ $name }的位置

# draft
geo_requirement = 写入和移除图片的位置

# draft
geo_restore_location_of = 恢复{ $name }的位置

# draft
geo_restored_location_of = 已恢复{ $name }的位置

# draft
geo_scan = 查找文件所在位置

# draft
geo_scan_description = 为地图视图查找窗格所列内容中注明了所在位置的图片和轨迹

# draft
geo_search_area = 搜索此区域

# draft
geo_search_area_description = 显示文件夹及其子文件夹中在某区域内拍摄的图片和视频

# draft
geo_search_subfolders = 搜索子文件夹中的位置

# draft
geo_search_subfolders_description = 显示文件夹及其所有子文件夹中的图片和轨迹

# draft
geo_select_one_track = 请只选择一条轨迹：已选择 { $count } 条

# draft
geo_select_pictures = 请选择要移除位置的图片

# draft
geo_select_track = 请选择一条轨迹以及要沿其放置的图片

# draft
geo_speed = { $speed } km/h

# draft
geo_stop_named = { $ident }（{ $name }）

# draft
geo_summary_flight = 飞行计划

# draft
geo_summary_flight_route = 飞行计划 · { $from } → { $to }

# draft
geo_summary_mission = 无人机任务

# draft
geo_summary_mission_waypoints = 无人机任务 · { $count } 个航点

# draft
geo_text_line_not_text = 第 { $line } 行不是文本

# draft
geo_text_line_too_long = 第 { $line } 行超过 { $size }

# draft
geo_thinned = 地图上每 { $stride } 个点显示一个

# draft
geo_title = { $format } · { $contents }

# draft
geo_track_no_times = { $name }没有时间信息：只有记录的轨迹才能用来放置图片

# draft
geo_unreadable = 无法读取{ $name }：{ $error }

# draft
geo_view = 地图

# draft
geo_wpml_first_waypoint = 第一个航点

# draft
geo_wpml_takeoff = 起飞点

# draft
geo_wpml_unreadable = 无法读取此任务

# draft
geo_xml_end_closes_nothing = 某个结束标记没有对应的开始标记

# draft
geo_xml_error_at = { $error }（位于第 { $byte } 字节）

# draft
geo_xml_text_too_long = 某个元素包含的文本超过 { $size }

# draft
geo_xml_too_deep = 元素嵌套超过 { $depth } 层
