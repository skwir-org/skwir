# ADR-016: One plugin for audio and video

Status: accepted, 2026-09-25. Built. Amends ADR-008 (video playback) and
ADR-009 (the structured preview plugins, `preview-audio` among them).

## Context

Audio and video were two plugins, `preview-audio` and `preview-media`, with
no decision behind the split. Both registered the same provider
(`MediaPreviews`) over the same GStreamer worker, and one player in the
shell plays both (panel controls, pin, dock, footer strip, `NowPlaying`).

The player gained preferences of its own (keep playing while browsing, play
on selection, what follows a track). In `preview-audio`, films would obey a
music plugin, and disabling it would withdraw settings video still used.
Declared by the shell, they landed on the Appearance page, where they do not
belong. The split bought only disabling video apart from audio, which the
Video engine's Still frames choice already gives.

## Decision

- One plugin, `preview-media` ("Audio and video"), registers both
  providers (`preview-media.audio`, `preview-media.video`) and declares
  every playback preference: Playback (`keep_playing`, `autoplay`,
  `continue`), Audio (`replay_gain`), Video (`backend`). `preview-audio` is
  removed.
- The shell's player reads the Playback keys through `skwir_ui::settings`
  constants, checked against the plugin's names and choices by
  `crates/skwir/tests/settings.rs`. With the plugin off there is no player.
- Changing the engine re-registers the video provider alone
  (`PreviewRegistry::unregister(owner, id)`): audio playing, pinned or not,
  goes on. ReplayGain is read when a track opens.
- The desktop's media controls stay a separate plugin (`mpris`), mirroring
  `App::player` for whatever plays.

## Consequences

- One Plugins toggle for playback, its settings on one page.
- `preview-audio.replay_gain` and the shell's `ui.media_*` became
  `preview-media.*`; stored values move through `Settings::rename`.
- Video can be disabled apart from audio only through Still frames.
