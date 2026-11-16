# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A setting's title: whether audio and video start as soon as they are selected.
media_autoplay = Play on selection

# Play is the player's button.
media_autoplay_description = Start audio and video files as soon as they are selected, instead of waiting for Play.

# A setting's title: what the player does once what it plays ends.
media_continue = When a track ends

# The setting's three choices: Stop, Play next, Shuffle.
media_continue_description = Stop, go on with the next file of the same kind in the folder (or the playlist), or with one of them at random.

# A choice of the When a track ends setting: the next file of the same kind plays.
media_continue_next = Play next

# A choice of the When a track ends setting: a file of the same kind, chosen at random, plays.
media_continue_shuffle = Shuffle

# A choice of the When a track ends setting: the player stops.
media_continue_stop = Stop

# A setting's title: whether what plays goes on when another file is selected.
media_keep_playing = Keep playing while you browse

media_keep_playing_description = What you play goes on when you select another file, in a player at the foot of the preview. Off, it stops when the selection moves.

# A context menu's item: plays the music selected, or the video, in the preview.
media_play = Play

media_plugin_description = Play audio and video files in the preview panel: audio with its cover and a spectrum, video with a still frame when playback is unavailable

media_plugin_title = Audio and video

# A setting's title: whether tracks are played louder or quieter so that they sound equally loud (ReplayGain).
media_replay_gain = Even out loudness

# A choice of the Even out loudness setting: each album evened out as a whole.
media_replay_gain_album = Per album

# ReplayGain is a standard's name; Per album is one of the setting's choices.
media_replay_gain_description = Use the ReplayGain values in a track's tags so that songs mastered at different levels sound equally loud. Per album keeps an album's own quiet and loud songs as they are. Tracks without the tags play unchanged; it applies from the next track.

# A choice of the Even out loudness setting: tracks play as they are.
media_replay_gain_off = Off

# A choice of the Even out loudness setting: each track evened out on its own.
media_replay_gain_track = Per track

# What stops working without FFmpeg, in the settings' list of what a plugin needs.
media_requirement_ffmpeg = Still frames when playback is unavailable

# What stops working without the media helper (GStreamer), in the settings' list of what a plugin needs.
media_requirement_worker = Audio and video playback

# A setting's title: what plays videos, or shows a still frame of them.
media_video_engine = Video engine

# A choice of the Video engine setting: GStreamer where it works, else still frames.
media_video_engine_auto = Automatic

# Automatic is the setting's first choice; GStreamer and FFmpeg are programs' names; the media worker is Skwir's helper program that plays.
media_video_engine_description = Automatic plays with GStreamer and falls back to a still frame from FFmpeg. Playback needs the media worker and GStreamer plugins.

# A choice of the Video engine setting: a still frame of the video, made by FFmpeg, instead of playing it.
media_video_engine_stills = Still frames
