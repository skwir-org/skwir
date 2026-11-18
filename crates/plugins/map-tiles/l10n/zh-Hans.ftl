# Simplified Chinese catalog of skwir-plugin-map-tiles, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
map_tiles_add_server = 添加瓦片服务器

# draft
map_tiles_answered = 瓦片服务器返回了 { $status }

# draft
map_tiles_area = { $south }, { $west } 至 { $north }, { $east }

# draft
map_tiles_avif_notice = 不绘制 AVIF 瓦片：地图显示文件中有瓦片的区域

# draft
map_tiles_check_header_line = { $name }：请求头的值须为一行

# draft
map_tiles_check_header_name = { $name }：“{ $header }”不是有效的请求头名称

# draft
map_tiles_check_header_reserved = { $name }：Skwir 会自行设置 { $header } 请求头

# draft
map_tiles_check_https = { $name }：地址必须以 https:// 开头

# draft
map_tiles_check_needs = { $name }：地址需要包含 { "{z}" }、{ "{x}" } 和 { "{y}" }（或 { "{-y}" }），或 { "{q}" }

# draft
map_tiles_check_no_key = { $name }：使用了 { "{key}" }，但未设置 API 密钥

# draft
map_tiles_check_subdomains = { $name }：地址使用了 { "{s}" }，因此服务器需要子域名

# draft
map_tiles_check_unknown_part = { $name }：地址中有未知部分 { $part }

# draft
map_tiles_compressed = 瓦片使用 { $compression } 压缩，Skwir 无法读取

# draft
map_tiles_detail_area = 区域

# draft
map_tiles_detail_attribution = 版权信息

# draft
map_tiles_detail_count = 瓦片数

# draft
map_tiles_detail_description = 描述

# draft
map_tiles_detail_format = 格式

# draft
map_tiles_detail_layers = 图层

# draft
map_tiles_detail_name = 名称

# draft
map_tiles_detail_tiles = 瓦片

# draft
map_tiles_detail_version = 版本

# draft
map_tiles_detail_zoom = 缩放级别

# draft
map_tiles_directory_damaged = 瓦片包的某个目录已损坏

# draft
map_tiles_directory_ends = 瓦片包的某个目录提前结束

# draft
map_tiles_directory_number = 瓦片包的某个目录包含过大的数字

# draft
map_tiles_downloads_stopped = 下载已停止

# draft
map_tiles_format_avif = AVIF 图片

# draft
map_tiles_format_jpeg = JPEG 图片

# draft
map_tiles_format_png = PNG 图片

# draft
map_tiles_format_unknown = 未知

# draft
map_tiles_format_vector = 矢量图形

# draft
map_tiles_format_webp = WebP 图片

# draft
map_tiles_its_compressed = 其瓦片使用 { $compression } 压缩，Skwir 无法读取

# draft
map_tiles_layers = { $layers }

# draft
map_tiles_mbtiles_no_metadata = 不是 MBTiles 文件：没有 metadata 表

# draft
map_tiles_mbtiles_no_tiles = 不是 MBTiles 文件：没有瓦片

# draft
map_tiles_more_choices = 您添加的瓦片服务器

# draft
map_tiles_no_key = 密钥环中没有 { $name } 的 API 密钥

# draft
map_tiles_no_server = 没有瓦片服务器 { $id }

# draft
map_tiles_no_subdomains = { $name } 没有可用于 { "{s}" } 的子域名

# draft
map_tiles_not_address = 不是有效的地址

# draft
map_tiles_not_archive = 不是瓦片包

# draft
map_tiles_not_modified = 服务器称该瓦片未更改，但 Skwir 从未保存过它

# draft
map_tiles_not_pmtiles = 不是 PMTiles 文件

# draft
map_tiles_outline = 陆地轮廓

# draft
map_tiles_outline_not = 不是轮廓数据

# draft
map_tiles_outline_number = 轮廓数据中的某个数字过长

# draft
map_tiles_outline_short = 轮廓数据提前结束

# draft
map_tiles_outline_too_large = 瓦片不可能这么大

# draft
map_tiles_part_too_large = 瓦片包的某个部分过大

# draft
map_tiles_plugin_description = 在离线的陆地轮廓上绘制地图，或在您选择后使用 OpenStreetMap 或您添加的瓦片服务器，并预览保存在文件中的地图（PMTiles、MBTiles）。只有此插件会使用网络

# draft
map_tiles_plugin_title = 地图背景

# draft
map_tiles_pmtiles_short = 不是 PMTiles 文件：在文件头之前就已结束

# draft
map_tiles_pmtiles_version = 不读取版本为 { $version } 的 PMTiles；只读取版本 3

# draft
map_tiles_points_outside = 瓦片包指向其自身以外的位置

# draft
map_tiles_server_address = 地址

# draft
map_tiles_server_address_description = 瓦片所在位置：{ "{z}" }、{ "{x}" } 和 { "{y}" }（或从底部计数的 { "{-y}" }），或表示 quadkey 的 { "{q}" }；{ "{s}" } 表示子域名，{ "{r}" } 表示高密度屏幕上的 @2x，{ "{key}" } 表示 API 密钥

# draft
map_tiles_server_answered = { $name }返回了一个 { $width }×{ $height } 的瓦片

# draft
map_tiles_server_attribution = 版权信息

# draft
map_tiles_server_attribution_description = 服务器的许可要求在地图上的瓦片下方显示的内容

# draft
map_tiles_server_failed = { $name }未返回瓦片：{ $error }

# draft
map_tiles_server_headers = 请求头

# draft
map_tiles_server_headers_description = 随每个请求发送：User-Agent 会替换 Skwir 的，以及 Referer、Authorization… { "{key}" } 会被替换为 API 密钥

# draft
map_tiles_server_key = API 密钥

# draft
map_tiles_server_key_description = 保存在您的密钥环中，绝不保存在设置里。在地址或请求头中需要它的位置填入 { "{key}" }

# draft
map_tiles_server_max_zoom = 最大缩放级别

# draft
map_tiles_server_max_zoom_description = 更近的视图会放大此缩放级别的瓦片

# draft
map_tiles_server_name = 名称

# draft
map_tiles_server_name_description = 服务器在列表中的名称

# draft
map_tiles_server_name_example = 地形图

# draft
map_tiles_server_subdomains = 子域名

# draft
map_tiles_server_subdomains_description = { "{s}" } 依次代表的内容

# draft
map_tiles_server_test = 测试

# draft
map_tiles_servers = 瓦片服务器

# draft
map_tiles_servers_description = 来自其他服务器的背景，可在“地图背景”中或从地图上选择。

# draft
map_tiles_source = 地图背景

# draft
map_tiles_source_description = 绘制地图所用的背景。

# draft
map_tiles_source_description_more = 陆地轮廓无需网络。OpenStreetMap 和其他服务器会下载您查看的瓦片，这会让它们知道您在查看哪里。

# draft
map_tiles_test_server = 测试瓦片服务器

# draft
map_tiles_test_server_description = 立即向瓦片服务器请求一个瓦片，并报告其应答

# draft
map_tiles_tile_count = { $count }

# draft
map_tiles_tile_too_large = 瓦片包中的某个瓦片过大

# draft
map_tiles_tiles_notice = 不绘制其瓦片：地图显示文件中有瓦片的区域

# draft
map_tiles_timed_out = 瓦片服务器未及时应答

# draft
map_tiles_title = { $format } · { $name }

# draft
map_tiles_too_deep = 瓦片包的目录嵌套过深

# draft
map_tiles_unnamed_server = 某个瓦片服务器

# draft
map_tiles_unread = 无法读取瓦片：{ $error }

# draft
map_tiles_unreadable_notice = { $why }：地图显示其中有瓦片的区域

# draft
map_tiles_vector_notice = 不绘制矢量瓦片：地图显示文件中有瓦片的区域

# draft
map_tiles_zoom_level = { $level }

# draft
map_tiles_zoom_range = { $from } 至 { $to }
