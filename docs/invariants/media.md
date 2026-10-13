# Media playback invariants

Read this before touching `crates/media-worker`, `crates/platform/src/media.rs`,
`crates/ui/src/media.rs`, the player (`player.rs`, `dock.rs`,
`elements/halo.rs`), the `mpris` plugin or the vendored Blade atlas. It is
binding, like `AGENTS.md`. Measurements and checks: `docs/video-playback.md`.

## Process and plugin

- GStreamer loads only in `skwir-media-worker`. The media provider owns the
  runtime setting; the session interface stays independent of GPUI.
- One plugin, `preview-media`, registers the audio and video providers and
  declares every playback preference. Audio and video share one player, so
  no player preference belongs to one kind; with the plugin off there is no
  player (ADR-016).
- A changed engine (`preview-media.backend`) re-registers only the video
  provider (`PreviewRegistry::unregister`), so audio playing goes on.

## Sessions and threads

- Playback starts paused unless `preview-media.autoplay` is on, and follows
  the selected preview and the provider's lifetime.
- UI controls only enqueue. The manager owns child cleanup and pixel I/O.
- The manager owns the one startup deadline (20 s to the first preroll) and
  sleeps in one select over commands, replies and
  `CancelToken::subscribe()`. Never poll `is_cancelled` on a timer from a
  blocked thread. The worker has no timer; cancelling kills it.
- Worker bus messages go from the sync handler through the loop's channel,
  so wake and payload are one object. Install bus and sink handlers before
  the first state change; never drop or block on that channel from a
  streaming thread.
- GStreamer owns audio/video timing: decoder events wake the worker, new
  output wakes the UI. Never add a UI frame timer for playback.

## Posters

- Show the shared thumbnail at once, then an optional native-resolution
  poster. Play and Seek cancel extraction and reject late posters; poster
  work never moves the position or delays playback.

## Generations, seeking and errors

- Position updates carry command generations. After enqueueing a control,
  discard queued and late older state; reject old generations.
- Seek intent stays visible until GStreamer finishes its asynchronous
  preroll. A timeout or position tolerance is not a seek acknowledgment.
- A control the worker cannot carry out gets `Rejected` with its
  generation, never `Error`: a transient notice, and the session goes on.
- Only decoder errors and a lost runtime end a session; the UI then
  withdraws the controls and returns focus.
- The UI gates seek and rate on reported seekability, and skips the
  position restore after a track change on a non-seekable stream.

## Frames and the atlas

- Keep appsink, transfer storage and UI delivery bounded. A frame slot is
  never overwritten before it is acknowledged. Release replaced atlas
  entries; never write per-frame image files.
- Atlas eviction cancels pending uploads before recycling texture indices,
  and destroys textures only after in-flight GPU work finishes: GPUI can draw
  several scenes before Wayland presents one, so keeping only the latest
  preview is not enough. See `vendor/patches/0003-blade-atlas-retirement.patch`
  and its GPU tests (`just atlas-check`).
- Call `drop_image` in update or render code, never in prepaint or paint: the
  patch waits only for the previous submission before freeing
  (`vendor/gpui/README.skwir.md`).

## Track changes and subtitles

- A track change re-prerolls from READY, then restores position, rate and
  play state.
- Returning to READY fences the loop's inbox (`Player::fence`), dropping bus
  messages from the abandoned preroll. Otherwise a second change before the
  first prerolled selects streams decodebin3 no longer knows, and the video
  stops for good. `tools/media/check.py` switches twice while playing.
- Do not race dynamic subtitle output replacement with paused flushing
  seeks. Keep playbin flags intact; test subtitle pixels, not metadata only.
- The manager bounds glibc arenas in the child to limit frame-buffer
  retention; native benchmarks use the same policy (`tools/media/check.py`).

## Decoder choice

- The decoder is GStreamer's choice, by its feature ranks. Skwir never picks
  one per machine: the ranks stay as installed.
- Measured 2026-09-24 on the reference machine:

  | What | Cost |
  | --- | --- |
  | NVIDIA `nvcodec` (ranked above `avdec_*`) | ~150 MB of CUDA mappings per video worker |
  | Decoder frame buffers, either decoder | ~60 MB at 1080p, ~400 MB at 4K H.265 |
  | Audio worker (never loads a video decoder) | ~12 MB PSS |

  There, software decoding was lighter and faster to a first frame at 1080p,
  and cost 1 to 1.5 more cores at 4K. One machine does not settle the choice.
- Compare worker memory with the same decoder in both runs.

## Measuring

- Generate media with `tools/media/check.py`. Report worker-only latency,
  full UI latency and combined PSS separately.
- The clocked fake audio sink checks timing, not audible synchronization or
  real audio devices.

## Clicks and fullscreen controls

- Fullscreen controls hide on one cancellable inactivity timer, never a loop.
  Stationary pointer events from layout must not reset it; hover, scrubbing,
  menus and keyboard focus keep the controls shown.
- Pointer movement must not take focus from an open playback popup.
- A click on the picture (`media::clickable`: film, audio figure, fullscreen
  canvas) toggles playback. Overlays on the film, such as the controls bar,
  must occlude it, or a click on their buttons also plays or pauses.
- The click flashes the new state (`media::flash`) at the centre of the film,
  or of the cover's disc for music, never of the figure with its text.
- The flash (`FLASH_MS`, whole for `FLASH_HOLD`, then fading) is cleared by
  one timer. It is the one exception to the 200 ms rule: a glyph must be read.
- A double click on a film toggles fullscreen; its second click undoes the
  first, so playback is unchanged.

## The audio picture

- `elements/halo.rs` draws the cover in a disc, the spectrum as rays of dots
  around it and a glow in the cover's colour.
- No timer: a published spectrum is a frame; paused, it is still (1 CPU
  tick in 4 s, measured).
- Rays are dots, quads in one batch: a circle looks the same at any angle,
  and gpui's path pass would cost a frame of its own.
- The cover's colour is computed once per cover (`halo::tint`, weighted by
  how colourful a pixel is so a border does not wash it out). Everything is
  sized in `paint`.

## Sound

- Volume is perceptual: the worker applies the cube of the control's
  position (`media::amplitude`), so half the control sounds half as loud.
- The filter is `scaletempo` (pitch kept at other speeds), then `spectrum`
  for audio. Either may be missing; playback does without.
- ReplayGain (`preview-media.replay_gain`: off, track, album) reaches the
  worker as `Tuning` when a session opens, so it applies from the next track,
  never mid-stream. `rgvolume` and `rglimiter` then go first; `rgvolume`
  reads the stream's tags itself; untagged tracks pass unchanged.
- `SPECTRUM_BINS` linear bins fold into `SPECTRUM_BANDS` logarithmic ones
  (`media::log_bands`), with the usual analyser tilt: music is heard in
  octaves, and linear bands put nearly all of it in the first five.
- Stream facts beyond title, artist and album reach Properties through
  `Playback::details`, bounded by the worker.

## Tracks and playlists

- Neighbours (same-kind files before and after the one shown, in pane
  order) are found when the listing or entry changes
  (`Shell::media_neighbours`, bounded), never while painting. Previous and
  Next show only when there are any.
- Stepping (`ui.media_step`) moves the pane's selection and the preview
  follows, so the list shows what plays; it keeps playing if it was
  (`PreviewState::autoplay`).
- At a track's end, `preview-media.continue` (`player::Continue`) stops (the
  default), plays the next, or one at random. The pin draws a shuffle once,
  from the track playing, and walks it, so Previous retraces Next; a step
  within the same folder keeps it.
- Playlists (`preview-playlists`) are read, not obeyed: nothing is fetched,
  remote entries are only named, missing ones marked, nothing written or
  removed. A track is a member that is a file of its own
  (`Member::own_file`).
- A track opened from a playlist keeps its rows as shown (`preview::Queue`):
  Previous, Next and the hand-over walk them until the playlist is left.

## Keep playing (the pin)

- A preview's session ends with the selection; a pinned session belongs to
  the window (`player.rs`).
- With `preview-media.keep_playing` on (the default), a track is pinned when
  it starts to play (`Shell::media_started`, from `media_send`, the one path
  every start takes). `ui.media_pin` pins or releases by hand.
- A media session has its own lifetime (`MediaState`). The pin takes it and
  gives it back when released, so neither ends the other's session.
- Selecting the pinned file shows the pinned session live in the panel
  (`MediaState::show_live`, no poster). The preview worker opens no player
  for it: never two players for one file.
- Setting, moving or releasing the pin starts a new generation; a track still
  opening for the old one is dropped.
- Starting another track moves the pin, or with Keep playing off stops the
  pinned one. Never two playing at once.
- While the panel shows the pinned session, every step (panel, dock,
  desktop) moves the selection in the pin's order, and the selection follows
  when the session advances by itself (`Shell::select_track`). The panel
  never holds a session the pin released.
- One reader pulls a session's updates: the panel while it shows it, else the
  pin, which keeps a film's last frame for the dock.
- The pin walks a queue of paths (same-kind folder files in pane order, or a
  playlist's tracks), resolved and opened on a worker.
- Gapless: `MediaSession::play_next` names the next file; the worker hands it
  to playbin3 in `about-to-finish` on the streaming thread, using only what
  the loop left for it, and reports `Advanced` when it starts (clearing
  position and tags). The pin queues the next track as soon as one starts
  and only follows the switch (`Pinned::advance`). A session that cannot
  refuses `play_next`, and the pin opens the next track at the end, with a
  gap. Verified: one worker carried three tracks.

## The dock

- `dock.rs` is the pinned player at the foot of the preview panel: cover or
  film, spectrum meter, transport, seek bar, More and close.
- Its height is dragged on its top edge and remembered with the window's
  shape: decided in render, never measured. Below `TALL_FROM` it is a row,
  above it the halo figure. A film takes the height it gets (`VIDEO_HEIGHT`
  in a compact dock).
- It steps aside while the panel shows the pinned session or is opening a
  track that will (`Shell::dock_shown`). With the panel closed, the last
  pane's footer keeps a strip of the controls.
- Only its picture answers clicks as a player (`Flash::Dock`), and stops the
  click there: a film's frame, not its letterbox (`picture::OnPicture`, from
  `ObjectFit::get_bounds` in prepaint); the cover or file icon; in the tall
  dock only the cover's disc (`halo::OnDisc`, tested against the circle).
  Each occludes the hover beneath it (the letterbox's, the figure's).
- Any other click outside the transport shows the track in the panel, still
  playing.
- Full screen is the panel's: a double click on a film, its Fullscreen button
  and F (`PlayerRequest::Fullscreen`, `Shell::player_fullscreen`) reveal the
  track in the panel, which goes fullscreen once it shows the session
  (`fullscreen_wanted`, forgotten when the pin changes).
- The transport has four `AdaptiveRow` forms, each moving one more item to
  More in the panel bar's order: Mute, a film's Fullscreen, then Previous,
  Next and the times. The narrowest keeps Play, seek bar, More and close.
- More (`dock::MENU`, a popup above its button) holds what the form left
  out, then the panel's rows for continuation, volume, speed and a film's
  tracks (`media::playback_rows`), each a `PlayerRequest` to the pin. It
  closes when the dock steps aside.
- The compact row has three `AdaptiveRow` forms: meter beside the whole
  text; beside text at least `TEXT_LEAST` wide; under the text. The meter
  never cuts the title's end; beside the text it is two lines tall.
- Its keys (`Player` context) send the same `PlayerRequest`s as the desktop.

## What plays is core data

- `App::player` holds `playing::NowPlaying`, published once per wake for the
  pin or, without one, the audio preview. Listeners hear of changes, not
  time passing, and compute the position.
- Others leave a `PlayerRequest` there (`ui.player` from the UI's own
  surfaces). The wake itself takes it (`Shell::sync`), not the frame it asks
  for: an off-screen window is never drawn, and a media key or a track ending
  must act at once. Nothing calls into the UI from another thread.
- Selecting a song is not playing it. The preview's track is announced when
  it starts to play, and no longer after Stop or when the selection moves on.
  A pin is announced while it lasts.
- The announced cover is written on a worker under the cache, per process
  (`player::COVERS`, swept at start), one file per track so a widget caching
  by location sees it change.
- The `mpris` plugin mirrors `App::player` onto the session bus and knows
  nothing else: properties come from the record, calls become
  `PlayerRequest`s, the per-process bus name is held only while something is
  announced. Without a session bus it logs once and does without. Test it on
  a private bus (`dbus-run-session`), never the user's.
- `player.request` and `player.now` are the same surface for scripts and
  agents.
