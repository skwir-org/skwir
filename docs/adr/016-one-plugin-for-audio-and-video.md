# ADR-016: One plugin for audio and video

Status: accepted, 2026-09-25. Amends ADR-008 (video playback) and ADR-009
(the structured preview plugins, of which `preview-audio` was one).

## Context

Audio and video were two plugins, `preview-audio` and `preview-media`,
added in one batch without a decision of their own. Both registered the
same provider (`MediaPreviews`) over the same GStreamer worker; audio added
its cover, spectrum and ReplayGain, video its engine choice and FFmpeg
stills. What plays either of them is one player in the shell: the panel's
controls, the pin, its dock, the status bar's strip, `NowPlaying`.

That player gained preferences of its own (keep playing while you browse,
play on selection, what follows a track that ends). They are neither
kind's: put in `preview-audio`, films would behave according to a plugin
about music, and switching it off would withdraw settings the video player
still followed. Declared by the shell, they landed on the Settings dialog's
Appearance page, the page of every `ui.*` key, which they are not. The
split itself bought one thing, switching video off while keeping audio,
which the Video engine's Still frames choice already gives.

## Decision

- One plugin, `preview-media`, titled "Preview: audio and video",
  registers both providers (`preview-media.audio`, `preview-media.video`)
  and declares every preference of playback, in sections: Playback
  (`keep_playing`, `autoplay`, `continue`), Audio (`replay_gain`), Video
  (`backend`). `preview-audio` is removed.
- The shell's player reads the Playback keys (`bibli_ui::settings`
  constants, held to the plugin's names and choices by
  `crates/bibli/tests/settings.rs`). With the plugin off there is no
  player, so nothing follows a withdrawn preference.
- A changed engine re-registers the video provider alone
  (`PreviewRegistry::unregister(owner, id)`, new in core): audio playing,
  pinned or not, goes on. ReplayGain stays a tuning read when a track opens.
- The desktop's media controls stay a plugin of their own (`mpris`): they
  mirror `App::player` for whatever plays, and are switched off for reasons
  of their own.

## Consequences

- The Plugins page lists one toggle for playback, and its settings are on
  one page named after it.
- Keys renamed without migration: `preview-audio.replay_gain` and the
  shell's `ui.media_*` (a day old) become `preview-media.*`; a stored value
  under an old name is ignored and returns to its default.
- Video cannot be switched off apart from audio except through Still
  frames, which keeps a picture of each film.
