# French catalog of skwir-plugin-playlists, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
playlists_column_artist = Artiste

# draft
playlists_column_length = Durée

# draft
playlists_column_number = N°

# draft
playlists_column_title = Titre

# draft
playlists_detail_length = Durée totale

# draft
playlists_detail_missing = Manquantes

# draft
playlists_detail_remote = Distantes

# draft
playlists_detail_tracks = Pistes

# draft
playlists_first_tracks = { $count ->
        [one] { $count } première piste · Source montre la suite
       *[other] { $count } premières pistes · Source montre la suite
    }

# draft
playlists_no_such_track = Piste introuvable

# draft
playlists_plugin_description = Lister les pistes des fichiers M3U, PLS, XSPF et CUE, et lire celles qui sont sur cette machine. Les éléments distants sont nommés, jamais récupérés

# draft
playlists_plugin_title = Listes de lecture

# draft
playlists_remote_named = Les éléments distants sont nommés, jamais récupérés

# draft
playlists_row_missing = manquante

# draft
playlists_row_remote = distante

# draft
playlists_title = { $count ->
        [one] Liste de lecture · { $count } piste
       *[other] Liste de lecture · { $count } pistes
    }

# draft
playlists_track_missing = { $path } est introuvable

# draft
playlists_track_remote = Cette piste n’est pas sur cette machine ; les aperçus ne récupèrent rien

# draft
playlists_tracks_more = { $count ->
       *[other] plus de { $count }
    }
