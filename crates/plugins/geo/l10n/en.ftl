# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# What a step found done already, said once its plan ran: the picture already said it was taken there. lat, lon: its latitude and longitude in degrees.
geo_already_at = { $name } was already at { $lat }, { $lon }

# What a step found done already, said once its plan ran: the picture did not say where it was taken.
geo_already_no_location = { $name } already had no location

# A flight plan's altitude above the sea, in feet then metres, in a map's Properties. feet, metres: the numbers.
geo_altitude_flight = { $feet } ft · { $metres } m

# A flight plan's altitude above the ground below, in feet then metres, in a map's Properties. feet, metres: the numbers.
geo_altitude_flight_ground = { $feet } ft · { $metres } m above ground

# A flight plan's altitude above where it took off, in feet then metres, in a map's Properties. feet, metres: the numbers.
geo_altitude_flight_takeoff = { $feet } ft · { $metres } m above takeoff

# A drone mission's height above the ground below, in a map's Properties. metres: the number.
geo_altitude_ground = { $metres } m above ground

# A drone mission's height above where the drone took off, in a map's Properties. metres: the number.
geo_altitude_takeoff = { $metres } m above takeoff

# Why a step of a plan that writes a picture's location does not run: the picture was written since the plan was reviewed.
geo_changed_since = { $name } changed since the plan was made

# Under a geotag's plan: the time each picture says it was taken was moved by this many seconds to match the track. seconds: a number with its sign (+120). Begins in lower case.
geo_clocks_corrected = cameras' clocks corrected by { $seconds } s

# East, abbreviated, as a picture's place says it (5.6 km E of Example Town).
geo_compass_e = E

# North, abbreviated, as a picture's place says it (5.6 km N of Example Town).
geo_compass_n = N

# North-east, abbreviated, as a picture's place says it (5.6 km NE of Example Town).
geo_compass_ne = NE

# North-west, abbreviated, as a picture's place says it (5.6 km NW of Example Town).
geo_compass_nw = NW

# South, abbreviated, as a picture's place says it (5.6 km S of Example Town).
geo_compass_s = S

# South-east, abbreviated, as a picture's place says it (5.6 km SE of Example Town).
geo_compass_se = SE

# South-west, abbreviated, as a picture's place says it (5.6 km SW of Example Town).
geo_compass_sw = SW

# West, abbreviated, as a picture's place says it (5.6 km W of Example Town).
geo_compass_w = W

# Part of a map's title, after its format (KML · 3 areas): how many areas (outlined shapes) the file holds.
geo_contents_areas = { $count ->
        [one] { $count } area
       *[other] { $count } areas
    }

# Part of a map's title, after its format (GPX · 1 route, 2 waypoints): how many routes (ways planned) the file holds.
geo_contents_routes = { $count ->
        [one] { $count } route
       *[other] { $count } routes
    }

# Part of a map's title, after its format (GPX · 1 track, 2 waypoints): how many tracks (ways recorded) the file holds.
geo_contents_tracks = { $count ->
        [one] { $count } track
       *[other] { $count } tracks
    }

# Part of a map's title, after its format (GPX · 1 track, 2 waypoints): how many waypoints (places marked by name) the file holds.
geo_contents_waypoints = { $count ->
        [one] { $count } waypoint
       *[other] { $count } waypoints
    }

# A notice under a map: the file stops making sense part way, and what was read before is drawn. why: what is wrong, in lower case (the file ends inside a record).
geo_damaged = The file is damaged ({ $why }); what came before is shown

# A notice under a map of a file with more waypoints and areas than are drawn. count: how many are not drawn.
geo_dropped_places = { $count ->
       *[other] { $count } more places than the map shows
    }

# Why a picture's location could not be written: ExifTool, the program that writes it, cannot write one into a file of its format. name: the picture's name.
geo_exiftool_cannot_write = ExifTool cannot write a location into { $name }

# Why a picture's location could not be written. name: the picture's name; why: what ExifTool, the program that writes it, said, in English.
geo_exiftool_could_not_write = ExifTool could not write { $name }: { $why }

# Why pictures cannot be geotagged or their location removed: ExifTool, the program that writes it, is not installed.
geo_exiftool_missing = Install ExifTool to write pictures' locations

# Why a picture's location could not be read, after its name (“a.jpg” could not be read: …): ExifTool, the program that reads it, said nothing.
geo_exiftool_unreadable = ExifTool could not read it

# A label in a map's Properties: how many areas (outlined shapes) the file holds.
geo_fact_areas = Areas

# A label in a map's Properties: how much the track climbed, every rise added up.
geo_fact_climb = Climb

# A label in a map's Properties: the device or the program that wrote the file.
geo_fact_created_by = Created by

# A label in a map's Properties: the altitude a flight plan cruises at.
geo_fact_cruise = Cruise

# A label in a map's Properties: where a flight plan or a drone mission starts.
geo_fact_departure = Departure

# A label in a map's Properties: how much the track went down, every descent added up.
geo_fact_descent = Descent

# A label in a map's Properties: what the file says of itself.
geo_fact_description = Description

# A label in a map's Properties: where a flight plan or a drone mission ends.
geo_fact_destination = Destination

# A label in a map's Properties: how long the track, the route or the plan is.
geo_fact_distance = Distance

# A label in a map's Properties: how long the track took, from its first point to its last.
geo_fact_duration = Duration

# A label in a map's Properties: when the track's last point was recorded.
geo_fact_ended = Ended

# A label in a map's Properties: the height a drone mission flies at.
geo_fact_flight_height = Flight height

# A label in a map's Properties: the highest altitude the track or the mission reaches.
geo_fact_highest_point = Highest point

# A label in a map's Properties: the altitude of a flight plan's highest waypoint.
geo_fact_highest_waypoint = Highest waypoint

# A label in a map's Properties: how many legs (stretches from one waypoint to the next) a flight plan has.
geo_fact_legs = Legs

# A label in a map's Properties: the lowest altitude the track reaches.
geo_fact_lowest_point = Lowest point

# A label in a map's Properties: the track's average speed while it was on the move.
geo_fact_moving_speed = Moving speed

# A label in a map's Properties: how long the track was on the move, its stops left out.
geo_fact_moving_time = Moving time

# A label in a map's Properties: the name the file gives itself.
geo_fact_name = Name

# A label in a map's Properties: how many points the file's tracks and routes have.
geo_fact_points = Points

# A label in a map's Properties: how long the file's routes (the ways planned) are, beside its tracks (the ways recorded).
geo_fact_route_distance = Route distance

# A label in a map's Properties: how many routes (ways planned) the file holds.
geo_fact_routes = Routes

# A label in a map's Properties: the latitude and longitude of the file's first point.
geo_fact_start = Start

# A label in a map's Properties: when the track's first point was recorded.
geo_fact_started = Started

# A label in a map's Properties: how many tracks (ways recorded) the file holds.
geo_fact_tracks = Tracks

# A label in a map's Properties: how many waypoints (places marked by name) the file holds.
geo_fact_waypoints = Waypoints

# Why a FIT file reads as damaged, inside the notice that says so: a message's numbers are written neither little- nor big-endian.
geo_fit_byte_order = a message has an unknown byte order

# Why a FIT file reads as damaged, inside the notice that says so.
geo_fit_checksum_mismatch = the file's checksum does not match its content

# Why a FIT file (a sports watch's recording) reads as damaged, inside the notice that says so: it is cut short.
geo_fit_ends_in_record = the file ends inside a record

# Why a FIT file reads as damaged, inside the notice that says so.
geo_fit_no_checksum = the file ends before its checksum

# Why a .fit file reads as damaged, inside the notice that says so: its header is not a FIT file's.
geo_fit_not_fit = this is not a FIT file

# Why a FIT file reads as damaged, inside the notice that says so.
geo_fit_record_past_end = a record runs past the end of the data

# Why a FIT file reads as damaged, inside the notice that says so: after its recordings comes something else.
geo_fit_trailing = something other than a FIT file follows the data

# Why a FIT file reads as damaged, inside the notice that says so: a message comes before the definition of its layout.
geo_fit_undefined_message = a message's type was never defined

# Why a FIT file reads as damaged, inside the notice that says so: the device stopped before it closed the recording.
geo_fit_unfinished = the recording was not finished

# A flight plan's distance in a map's Properties: in metres or kilometres, then in nautical miles. distance: the first, with its unit; nautical: the number of nautical miles.
geo_flight_distance = { $distance } · { $nautical } NM

# Why an X-Plane plan reads as damaged, inside the notice that says so: it holds fewer waypoints than its header counts. expected: that count; read: how many it holds.
geo_fms_fewer_waypoints = the plan lists { $expected ->
       *[other] { $expected } waypoints
    } and holds { $read }

# Why a .fms file is not shown on a map: its first lines are not an X-Plane flight plan's.
geo_fms_not_plan = not an X-Plane flight plan

# Why a .fms file is not shown on a map. version: the version its second line states, as written.
geo_fms_version = an X-Plane flight plan of version { $version }, which is not read

# A file format, in a map's title: a flight plan of Garmin's (.fpl).
geo_format_garmin_plan = Garmin flight plan

# A file format, in a map's title: a drone mission of Litchi, a drone app (.csv).
geo_format_litchi_mission = Litchi mission

# A file format, in a map's title: a flight plan of Little Navmap, a flight planning program (.lnmpln).
geo_format_little_navmap_plan = Little Navmap plan

# A file format, in a map's title: a drone mission in MAVLink's text (.waypoints, .mission).
geo_format_mavlink_mission = MAVLink mission

# A file format, in a map's title: a drone mission of QGroundControl, a mission planner (.plan).
geo_format_qgc_plan = QGroundControl plan

# A file format, in a map's title: a flight plan of Microsoft Flight Simulator (.pln).
geo_format_sim_plan = Flight Simulator plan

# A file format, in a map's title: a flight plan of X-Plane, a flight simulator (.fms).
geo_format_xplane_plan = X-Plane plan

# Why a .fpl file is not shown on a map: it is not a flight plan of Garmin's.
geo_fpl_not_plan = not a Garmin flight plan

# Why a GeoJSON file is not shown on a map. error: what the JSON reader says is wrong, in English.
geo_geojson_invalid = Not valid GeoJSON: { $error }

# Why a GeoJSON file is not shown on a map: it is larger than the preview reads. size: that bound (16 MB).
geo_geojson_too_large = This GeoJSON file exceeds { $size } preview limit

# An action: write into pictures where they were taken, read from a GPS track recorded at the same time.
geo_geotag = Geotag pictures from a track

# Read by agents and in F1's list of actions.
geo_geotag_description = Place the selected pictures along the one selected track (GPX, FIT, any track read): each picture taken while it was recorded gets its position at that moment; proposed for review. Works on a worker: the result arrives as a proposal or a notification

# A context menu item on pictures beside a GPS track: write into the pictures where they were taken, from the track.
geo_geotag_menu = Geotag from Track

# Why a step of a plan that writes a picture's location cannot run: the picture is gone.
geo_gone = { $name } no longer exists

# Why pictures cannot be geotagged: the file selected as their track holds none.
geo_holds_no_track = { $name } holds no track

# A notice under a map: points of the file whose latitude or longitude is not one are not drawn.
geo_invalid_points = { $count ->
        [one] { $count } point left out: not a position
       *[other] { $count } points left out: not a position
    }

# A distance in kilometres, in a map's Properties (Distance: 42.2 km) and in where a picture was taken (5.6 km NE of Example Town). km: the number.
geo_kilometres = { $km } km

# Why a KMZ file (a zipped Google Earth file) is not shown on a map: it holds more files than the preview reads (4,000).
geo_kmz_members = { $count ->
       *[other] This KMZ holds more than { $count } members
    }

# Why a KMZ file (a zipped Google Earth file) is not shown on a map: it holds no .kml file.
geo_kmz_no_kml = This KMZ holds no KML document

# Why a KMZ file (a zipped Google Earth file) is not shown on a map: the .kml file it holds cannot be unpacked.
geo_kmz_unreadable = This KMZ's document cannot be read

# Under a plan that writes pictures' locations, what was left out: selected files that are not pictures.
geo_left_others = { $count ->
        [one] { $count } other file left out
       *[other] { $count } other files left out
    }

# Under the title of a plan placing or removing pictures' locations: what was left out, as a list (1 picture outside the track's times and 1 other file left out).
geo_left_out = { $what }

# Under a geotag's plan, what was left out: pictures taken before the track began or after it ended.
geo_left_outside = { $count ->
        [one] { $count } picture outside the track's times
       *[other] { $count } pictures outside the track's times
    }

# Under a geotag's plan, what was left out: pictures that already say where they were taken.
geo_left_placed = { $count ->
        [one] { $count } picture already placed, left as it is
       *[other] { $count } pictures already placed, left as they are
    }

# Under a geotag's plan, what was left out: a camera's RAW pictures, whose location would be written in a file beside them (a sidecar), which is not done yet.
geo_left_raw = { $count ->
        [one] { $count } RAW picture left out: their sidecars are not written yet
       *[other] { $count } RAW pictures left out: their sidecars are not written yet
    }

# Under a geotag's plan, what was left out: pictures that do not say when they were taken.
geo_left_undated = { $count ->
        [one] { $count } picture without the time taken
       *[other] { $count } pictures without the time taken
    }

# Under a geotag's plan, what was left out: pictures ExifTool could not read.
geo_left_unreadable = { $count ->
        [one] { $count } picture that could not be read
       *[other] { $count } pictures that could not be read
    }

# Under a geotag's plan, what was left out: pictures of a format ExifTool, the program that writes the location, cannot write.
geo_left_unwritable = { $count ->
        [one] { $count } picture ExifTool cannot write
       *[other] { $count } pictures ExifTool cannot write
    }

# Why a .csv file is not read as a drone mission of Litchi, a drone app: its header names other columns.
geo_litchi_columns = Not a Litchi mission: the CSV's columns are not a mission's

# Why a .csv file is not read as a drone mission of Litchi, a drone app.
geo_litchi_empty = Not a Litchi mission: the file is empty

# The name on the map of a point a Litchi drone mission's camera turns towards.
geo_litchi_point_of_interest = Point of interest

# Why a .lnmpln file is not shown on a map: it is not a plan of Little Navmap, a flight planning program.
geo_lnmpln_not_plan = not a Little Navmap plan

geo_locate = Where is a place

geo_locate_description = The places of that name skwir knows offline, biggest first, with their positions: for scripts and agents

# What a step found done already, said once its plan ran: the picture already said where it was taken, as it did before. Begins in lower case.
geo_location_already_back = the location of { $name } was already back

# Why a drone mission file is not shown on a map.
geo_mavlink_empty = Not a MAVLink mission: the file is empty

# Why a drone mission file is not shown on a map. “QGC WPL 110” is the line such a file begins with, as written in the file: keep it as it is.
geo_mavlink_no_header = Not a MAVLink mission: its first line is not “QGC WPL 110”

# A length or a height in metres, in a map's Properties (Climb: 120 m). metres: the number.
geo_metres = { $metres } m

# The name a drone mission's area is given on the map: an area the drone must keep out of.
geo_mission_exclusion_zone = Exclusion zone

# The name a drone mission's area is given on the map: the area the drone must stay inside.
geo_mission_geofence = Geofence

# The name of a drone mission's home on the map, where the drone takes off and returns to; also what Destination says of a mission that returns there.
geo_mission_home = Home

# The name of an area on the map that a drone mission flies over in rows to photograph it.
geo_mission_survey = Survey

# An error to a script or an agent that asked the Map view of a pane that is not there. pane: the pane's number, as they name it.
geo_no_pane = no pane { $pane }

# Why no location was removed: the selection holds no picture.
geo_no_picture_selected = No picture selected

# Why no location was removed: the selection holds no picture. why: what it holds instead, as a list (1 RAW picture left out: their sidecars are not written yet and 2 other files left out).
geo_no_picture_selected_because = No picture selected: { $why }

# A notice in place of a map: the file holds no point with a latitude and a longitude.
geo_no_position = Nothing in this file has a position

# Why a file is not shown on a map: its name says a format of tracks or plans, and it holds none.
geo_not_a_map = Not a map file

# Why a geotag planned nothing. track: the name of the GPS track.
geo_nothing_along = No picture to place along { $track }

# Why a geotag planned nothing. track: the name of the GPS track; why: what was left out, as a list (3 pictures outside the track's times and 1 other file left out).
geo_nothing_along_because = No picture to place along { $track }: { $why }

# A plan's title, in its review and in what is said after it (Undid: …): pictures are given where they were taken, from a GPS track. name: the picture's name when there is one; track: the track's name. Begins in lower case.
geo_place_along = { $count ->
        [1] place { $name } along { $track }
       *[other] place { $count } pictures along { $track }
    }

# A step of a plan, in its review: a picture is given where it was taken. lat, lon: its latitude and longitude in degrees. Begins in lower case.
geo_place_at = place { $name } at { $lat }, { $lon }

# A row's label in a picture's or a video's Properties: the town it was taken in or near.
geo_place_label = Place

# Where a picture was taken, in its Properties beside Place, when it is near a town rather than in it: 5.6 km NE of Example Town. distance: how far, with its unit; direction: one of the eight abbreviated directions, from the town; place: the town's name, as the place names' data writes it.
geo_place_near = { $distance } { $direction } of { $place }

# What a step did, said once its plan ran: a picture was given where it was taken. lat, lon: its latitude and longitude in degrees. Begins in lower case.
geo_placed_at = placed { $name } at { $lat }, { $lon }

# Why a QGroundControl plan (a drone mission) is not shown on a map: it is larger than the preview reads. size: that bound (16 MB).
geo_plan_too_large = This plan exceeds { $size } preview limit

# Why a .pln file is not shown on a map: it is not a flight plan of Microsoft Flight Simulator.
geo_pln_not_plan = not a flight simulator plan

geo_plugin_description = Show GPS tracks and routes (GPX, KML, KMZ, GeoJSON, TCX, FIT, IGC, NMEA), flight plans (Garmin, Flight Simulator, X-Plane, Little Navmap) and drone missions (QGroundControl, MAVLink, Litchi, DJI) on a map, with their distance, time and climb, and the pictures and videos of a folder in the Map view. Nothing a file links to is fetched

geo_plugin_title = Maps

geo_points = Where the files are

# Read by agents and in F1's list of actions. `since` is the name of the command's argument: keep it as it is.
geo_points_description = The pictures and tracks of what a pane lists that were placed on the map, after the first `since`

# Why a .plan file of QGroundControl (a drone mission planner) is not shown on a map. error: what the JSON reader says is wrong, in English.
geo_qgc_invalid = Not a valid QGroundControl plan: { $error }

# The name of an area on the map around a building or a structure that a drone mission circles to photograph it.
geo_qgc_structure_scan = Structure scan

# The label on the map of a drone mission's rally point (a safe place to land instead of home), numbered from 1: R1, R2…
geo_rally_point = R{ $number }

# A notice under the map of a file larger than is read. size: how much of it was read (256 MB).
geo_read_first = Only the first { $size } were read

geo_release = Forget where the files are

geo_release_description = Stop finding where a pane's files are: its Map view closed

geo_remove_location = Remove pictures' GPS locations

geo_remove_location_description = Take the location out of the selected pictures (plan); those without one are left as they are

# A context menu item on pictures: take out of them where they were taken.
geo_remove_location_menu = Remove GPS Location

# A step of a plan, in its review: where a picture was taken is taken out of it. Begins in lower case.
geo_remove_location_of = remove the location of { $name }

# A plan's title, in its review and in what is said after it (Undid: …): where pictures were taken is taken out of them. name: the picture's name when there is one. Begins in lower case.
geo_remove_locations = { $count ->
        [1] remove the location of { $name }
       *[other] remove the location of { $count } pictures
    }

# What a step did, said once its plan ran: where a picture was taken was taken out of it. Begins in lower case.
geo_removed_location_of = removed the location of { $name }

# What ExifTool, a program the Maps plugin uses, is needed for, in the plugin's requirements.
geo_requirement = Writing and removing pictures' locations

# A step of a plan undoing another, in its review: where a picture was taken is written back as it was. Begins in lower case.
geo_restore_location_of = restore the location of { $name }

# What a step did, said once its plan ran: where a picture was taken was written back as it was. Begins in lower case.
geo_restored_location_of = restored the location of { $name }

geo_scan = Find where the files are

# Read by agents and in F1's list of actions.
geo_scan_description = Find the pictures and tracks of what a pane lists that say where they are, for the Map view

# An action of the Map view: find the pictures and videos made inside the area the map shows.
geo_search_area = Search this area

geo_search_area_description = Show the pictures and videos of the folder and its subfolders that were made inside an area

# An action of the Map view: put on the map the pictures and tracks of the folder's subfolders too.
geo_search_subfolders = Search subfolders for places

geo_search_subfolders_description = Show the pictures and tracks of the folder and all its subfolders

# Why pictures cannot be geotagged: the selection holds several GPS tracks. count: how many.
geo_select_one_track = Select one track: { $count ->
       *[other] { $count } are selected
    }

# Why no location was removed: nothing is selected.
geo_select_pictures = Select the pictures whose location to remove

# Why pictures cannot be geotagged: the selection holds no GPS track.
geo_select_track = Select a track and the pictures to place along it

# A speed in kilometres per hour, in a map's Properties. speed: the number.
geo_speed = { $speed } km/h

# How a stop of a flight plan is labelled on the map: its identifier (an airport's or a beacon's code, XAAA) with its name as the plan gives it (Example Field), both as written.
geo_stop_named = { $ident } ({ $name })

# The title of a flight plan's preview, when it does not say where from and to.
geo_summary_flight = Flight plan

# The title of a flight plan's preview. from, to: where it departs and where it lands, as the plan names them (an airport's code, XAAA).
geo_summary_flight_route = Flight plan · { $from } → { $to }

# The title of a drone mission's preview, when it has no waypoint.
geo_summary_mission = Drone mission

# The title of a drone mission's preview. count: how many waypoints it has.
geo_summary_mission_waypoints = Drone mission · { $count ->
        [one] { $count } waypoint
       *[other] { $count } waypoints
    }

# Why a plan's text reads as damaged, inside the notice that says so: the line holds bytes that are not UTF-8 text. line: the line's number, from 1.
geo_text_line_not_text = line { $line } is not text

# Why a plan's text reads as damaged, inside the notice that says so. line: the line's number, from 1; size: the longest line read (64 KB).
geo_text_line_too_long = line { $line } is longer than { $size }

# A notice under a map of a track with more points than are drawn: one point of every few is drawn. stride: how many points each drawn point stands for.
geo_thinned = The map shows one point in { $stride }

# The title of a map's preview: the file's format, then what it holds (GPX · 1 track, 1 route, 2 waypoints). contents: a list of counts.
geo_title = { $format } · { $contents }

# Why pictures cannot be geotagged: the track selected does not say when it passed each point (a route planned, not recorded).
geo_track_no_times = { $name } has no times: only a recorded track can place pictures

# Why a step of a plan that writes a picture's location cannot run: what the picture says of its location cannot be read. error: why.
geo_unreadable = { $name } could not be read: { $error }

# The view that places a folder's pictures, videos and tracks on a map, among the views (List, Grid…).
geo_view = Map

# Where a DJI drone mission ends, beside Destination in the preview's Properties: it flies back to its first waypoint.
geo_wpml_first_waypoint = First waypoint

# The name of a DJI drone mission's takeoff point on the map.
geo_wpml_takeoff = Takeoff

# Why a DJI drone mission (a KMZ file) is not shown on a map: the mission it holds cannot be unpacked.
geo_wpml_unreadable = This mission cannot be read

# Why an XML file reads as damaged, inside the notice that says so: an element is closed that was never opened.
geo_xml_end_closes_nothing = an end tag closes nothing

# Why an XML file reads as damaged, inside the notice that says so. error: the XML reader's own words (English); byte: where in the file, a number.
geo_xml_error_at = { $error } (at byte { $byte })

# An error reading an XML file (a track, a plan): one element's text is larger than is read. size: that bound (64 MB).
geo_xml_text_too_long = an element holds more than { $size } of text

# Why an XML file reads as damaged, inside the notice that says so. depth: how many elements may be open inside one another (256).
geo_xml_too_deep = elements nest deeper than { $depth }
