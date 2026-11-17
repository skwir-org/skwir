# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A column's header in a playlist's preview: who performs the track.
playlists_column_artist = Artist

# A column's header in a playlist's preview: how long the track plays.
playlists_column_length = Length

# A column's header in a playlist's preview: the track's number in the playlist.
playlists_column_number = #

# A column's header in a playlist's preview: the track's title.
playlists_column_title = Title

# A label in the preview's Properties: how long the playlist's tracks play, together.
playlists_detail_length = Total length

# A label in the preview's Properties: how many of the playlist's tracks are files that are not there.
playlists_detail_missing = Missing

# A label in the preview's Properties: how many of the playlist's tracks are somewhere else (a server, a stream).
playlists_detail_remote = Remote

# A label in the preview's Properties: how many tracks the playlist has.
playlists_detail_tracks = Tracks

# A note under a playlist's preview that shows only its first tracks. Source is the preview's button that shows the file as text.
playlists_first_tracks = { $count ->
        [1] Only the first track is shown · Source shows the rest
       *[other] Only the first { $count } tracks are shown · Source shows the rest
    }

# An error: the track a row of a playlist's preview named is not in the playlist.
playlists_no_such_track = No such track

playlists_plugin_description = List the tracks of M3U, PLS, XSPF and CUE files, and play the ones that are on this machine. Remote entries are named, never fetched

playlists_plugin_title = Playlists

# A note under a playlist's preview: its tracks that are somewhere else (a server, a stream) are listed, not fetched.
playlists_remote_named = Remote entries are named, never fetched

# The last cell of a playlist's track whose file is not there. In lower case.
playlists_row_missing = missing

# The last cell of a playlist's track that is somewhere else (a server, a stream), never fetched. In lower case.
playlists_row_remote = remote

# The title of a playlist's preview, with how many tracks it lists.
playlists_title = { $count ->
        [one] Playlist · { $count } track
       *[other] Playlist · { $count } tracks
    }

# An error: the file a playlist names is not there. The path is the file's whole path.
playlists_track_missing = { $path } is missing

# An error: a track of a playlist is somewhere else (a server, a stream), which the preview does not fetch.
playlists_track_remote = This track is not on this machine; previews fetch nothing

# The value beside Tracks in the preview's Properties, for a playlist read only as far as its first tracks: more than that many.
playlists_tracks_more = { $count ->
       *[other] more than { $count }
    }
