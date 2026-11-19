# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# The button adding a tile server to the settings' list of them.
map_tiles_add_server = Add tile server

# status: an HTTP status number (404).
map_tiles_answered = the tile server answered { $status }

# Where a tile archive has tiles: its south-west corner to its north-east one, latitude then longitude, each a number of degrees (45.12345). Where a language's decimal sign is a comma, the numbers want another separator.
map_tiles_area = { $south }, { $west } to { $north }, { $east }

map_tiles_avif_notice = AVIF tiles are not drawn: the map shows where the file has tiles

# name: a tile server's, as the user named it.
map_tiles_check_header_line = { $name }: a header's value is one line

# name: a tile server's, as the user named it; header: what was typed as a header's name.
map_tiles_check_header_name = { $name }: { $header } is not a header name

# name: a tile server's, as the user named it; header: an HTTP header's name (Host).
map_tiles_check_header_reserved = { $name }: Skwir sets the { $header } header itself

# name: a tile server's, as the user named it.
map_tiles_check_https = { $name }: the address must start with https://

# name: a tile server's, as the user named it. What is in braces is typed as it is in its address.
map_tiles_check_needs = { $name }: the address needs { "{z}" }, { "{x}" } and { "{y}" } (or { "{-y}" }), or { "{q}" }

# name: a tile server's, as the user named it. What is in braces is typed as it is in its address.
map_tiles_check_no_key = { $name }: { "{key}" } is used, but no API key is set

# name: a tile server's, as the user named it. What is in braces is typed as it is in its address.
map_tiles_check_subdomains = { $name }: the address uses { "{s}" }, so the server needs subdomains

# name: a tile server's, as the user named it; part: what its address holds in braces that skwir does not know ({zoom}).
map_tiles_check_unknown_part = { $name }: the address has an unknown part { $part }

# compression: the name of a way to compress (Brotli, Zstandard).
map_tiles_compressed = The tiles are compressed with { $compression }, which Skwir does not read

# A tile archive's detail: where on Earth it has tiles.
map_tiles_detail_area = Area

map_tiles_detail_attribution = Attribution

map_tiles_detail_count = Tile count

map_tiles_detail_description = Description

map_tiles_detail_format = Format

map_tiles_detail_layers = Layers

map_tiles_detail_name = Name

# A tile archive's detail: what its tiles are (PNG pictures, vector shapes).
map_tiles_detail_tiles = Tiles

map_tiles_detail_version = Version

map_tiles_detail_zoom = Zoom

map_tiles_directory_damaged = A directory of the archive is damaged

map_tiles_directory_ends = A directory of the archive ends early

map_tiles_directory_number = A directory of the archive holds a number too large

map_tiles_format_avif = AVIF pictures

map_tiles_format_jpeg = JPEG pictures

map_tiles_format_png = PNG pictures

# What a tile archive's tiles are, when it does not say.
map_tiles_format_unknown = Unknown

map_tiles_format_vector = Vector shapes

map_tiles_format_webp = WebP pictures

# compression: the name of a way to compress (Brotli, Zstandard).
map_tiles_its_compressed = Its tiles are compressed with { $compression }, which Skwir does not read

# The layers of a tile archive's vector tiles, by the names the archive gives them, a list.
map_tiles_layers = { $layers }

map_tiles_mbtiles_no_metadata = Not an MBTiles file: it has no metadata table

map_tiles_mbtiles_no_tiles = Not an MBTiles file: it has no tiles

# Said where the settings file names a background not listed: it may be among these.
map_tiles_more_choices = the tile servers you add

# name: a tile server's, as the user named it.
map_tiles_no_key = The API key of { $name } is not in the keyring

# id: the id a command was given, which no tile server has.
map_tiles_no_server = No tile server { $id }

# name: a tile server's, as the user named it. What is in braces is typed as it is in its address.
map_tiles_no_subdomains = { $name } has no subdomains for { "{s}" }

map_tiles_not_archive = Not a tile archive

# An error: a tile server answered “not modified” for a tile Skwir did not keep.
map_tiles_not_modified = the server said the tile had not changed, but Skwir never had it

map_tiles_not_pmtiles = Not a PMTiles file

# A map background: the outline of the land, drawn without the network.
map_tiles_outline = Outline

map_tiles_outline_not = not the outline data

map_tiles_outline_number = a number in the outline data is too long

map_tiles_outline_short = the outline data ends early

map_tiles_outline_too_large = a tile cannot be that large

map_tiles_part_too_large = A part of the archive is too large

map_tiles_plugin_description = Draw maps on the outline of the land, offline, or on OpenStreetMap or tile servers you add once you choose one, and preview maps kept in files (PMTiles, MBTiles). Only this plugin uses the network

map_tiles_plugin_title = Map backgrounds

map_tiles_pmtiles_short = Not a PMTiles file: it ends before its header

map_tiles_pmtiles_version = PMTiles version { $version } is not read; version 3 is

map_tiles_points_outside = The archive points outside itself

map_tiles_server_address = Address

# What is in braces is typed as it is in a tile server's address: never translated.
map_tiles_server_address_description = Where a tile is: { "{z}" }, { "{x}" } and { "{y}" } (or { "{-y}" }, counted from the bottom), or { "{q}" } for a quadkey; { "{s}" } for a subdomain, { "{r}" } for @2x on a dense screen, { "{key}" } for the API key

# name: a tile server's; width and height: the tile's, in pixels.
map_tiles_server_answered = { $name } answered with a tile of { $width }×{ $height }

map_tiles_server_attribution = Attribution

map_tiles_server_attribution_description = What the server's licence asks to show on the map under its tiles

# name: a tile server's; error: why it gave none.
map_tiles_server_failed = { $name } gave no tile: { $error }

map_tiles_server_headers = Headers

map_tiles_server_headers_description = Sent with every request: User-Agent replaces Skwir's, Referer, Authorization… { "{key}" } is replaced by the API key

map_tiles_server_key = API key

map_tiles_server_key_description = Kept in your keyring, never in the settings. Put { "{key}" } in the address or a header where it goes

map_tiles_server_max_zoom = Deepest zoom

map_tiles_server_max_zoom_description = Closer views enlarge the tiles of this zoom

map_tiles_server_name = Name

map_tiles_server_name_description = What the server is called in the list

# An example of a tile server's name, shown in its empty field.
map_tiles_server_name_example = Topographic

map_tiles_server_subdomains = Subdomains

map_tiles_server_subdomains_description = What { "{s}" } stands for, one after another

# A button on each tile server: asks it for one tile.
map_tiles_server_test = Test

map_tiles_servers = Tile servers

map_tiles_servers_description = Backgrounds from other servers, to choose in Map background or from a map.

map_tiles_source = Map background

map_tiles_source_description = What maps are drawn on.

# What a setting's description says after its first sentence, in its tooltip.
map_tiles_source_description_more = The outline of the land needs no network. OpenStreetMap and other servers download the tiles you look at, which tells them where you look.

map_tiles_test_server = Test a tile server

map_tiles_test_server_description = Ask a tile server for one tile, now, and say what it answered

# How many tiles a tile archive holds, a number (beside Tile count).
map_tiles_tile_count = { $count }

map_tiles_tile_too_large = A tile of the archive is too large

map_tiles_tiles_notice = Its tiles are not drawn: the map shows where the file has tiles

# The title of a tile archive's preview: its format's name (PMTiles 3, MBTiles), then the name the archive gives itself, both as written.
map_tiles_title = { $format } · { $name }

map_tiles_too_deep = The archive's directories nest too deep

# A tile server not named yet, at the start of what is wrong with it.
map_tiles_unnamed_server = A tile server

# why: why a tile archive's tiles cannot be read (they are compressed in a way skwir does not read).
map_tiles_unreadable_notice = { $why }: the map shows where it has tiles

map_tiles_vector_notice = Vector tiles are not drawn: the map shows where the file has tiles

# The one zoom level a tile archive has tiles for, a number.
map_tiles_zoom_level = { $level }

# The zoom levels a tile archive has tiles for: from the farthest to the closest.
map_tiles_zoom_range = { $from } to { $to }
