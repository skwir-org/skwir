# Italian catalog of skwir-plugin-playlists, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
playlists_column_artist = Artista

# draft
playlists_column_length = Durata

# draft
playlists_column_number = #

# draft
playlists_column_title = Titolo

# draft
playlists_detail_length = Durata totale

# draft
playlists_detail_missing = Mancanti

# draft
playlists_detail_remote = Remote

# draft
playlists_detail_tracks = Tracce

# draft
playlists_first_tracks = { $count ->
        [1] È mostrata solo la prima traccia · Sorgente mostra le altre
       *[other] Sono mostrate solo le prime { $count } tracce · Sorgente mostra le altre
    }

# draft
playlists_no_such_track = Traccia inesistente

# draft
playlists_plugin_description = Elenca le tracce dei file M3U, PLS, XSPF e CUE e riproduce quelle presenti su questo computer. Le voci remote sono indicate per nome, mai scaricate

# draft
playlists_plugin_title = Scalette

# draft
playlists_remote_named = Le voci remote sono indicate per nome, mai scaricate

# draft
playlists_row_missing = mancante

# draft
playlists_row_remote = remota

# draft
playlists_title = { $count ->
        [one] Scaletta · { $count } traccia
       *[other] Scaletta · { $count } tracce
    }

# draft
playlists_track_missing = { $path } manca

# draft
playlists_track_remote = Questa traccia non si trova su questo computer; le anteprime non scaricano nulla

# draft
playlists_tracks_more = { $count ->
       *[other] più di { $count }
    }
