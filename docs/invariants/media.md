# Media playback invariants

Read this before touching `crates/media-worker`, `crates/platform/src/media.rs`,
`crates/ui/src/media.rs`, the player (`player.rs`, `dock.rs`,
`elements/halo.rs`), the `mpris` plugin or gpui's atlas (vendored). It is
binding, like `AGENTS.md`. Measurements and checks: `docs/video-playback.md`.

## Process and plugin

- GStreamer loads only in `skwir-media-worker`. The media provider owns the
  runtime setting; the session interface stays independent of GPUI.
- Linux's packages use the system's GStreamer, all but the AppImage and
  the tarball, which carry one built for them (ADR-052). Windows' and macOS's
  carry GStreamer's own build, cut down (ADR-076,
  `tools/package/gstreamer.py`). The worker, finding one beside itself,
  plays with it alone
  (`carried_gstreamer`): its plugins, its scanner, its registry in
  skwir's cache, never the plugins or registry of a GStreamer the user
  installed. A plugin the worker needs is added to the recipe's set,
  whose check plays clips under Wine (`tools/package/windows-check.sh`).
- One plugin, `media`, registers the audio and video providers and
  declares every playback preference. Audio and video share one player, so
  no player preference belongs to one kind; with the plugin off there is no
  player (ADR-016).
- A changed engine (`media.backend`) re-registers only the video
  provider (`PreviewRegistry::unregister`), so audio playing goes on.

## Sessions and threads

- Playback starts paused unless `media.autoplay` is on, and follows
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
- The volume control is the stream's own (playbin's volume is the audio
  sink's): the worker reports what the sound server holds, the volume and
  mute it gives the application's next stream back, or its mixer moved
  (`Player::follow_sound`): on `notify::volume` and `notify::mute`, which
  pulsesink emits only while it renders, and at every preroll's
  `AsyncDone`, where reading the property asks the server. The control
  then never says 100 % over a stream at 0 %. Tests with the real audio
  sink use a sound server of their own: one on the session's leaves the
  application's stream at what the test set.
- GStreamer owns audio/video timing: decoder events wake the worker, new
  output wakes the UI. Never add a UI frame timer for playback.

## Posters

- A film shows one picture before it plays, a frame of its own: the
  poster, or, when none comes (`Media::NoPoster`, or the request reading
  it was cancelled), the player's first frame, held aside until then.
  Never one picture then another: the second is a flicker.
- The picture and the player are asked at once (`PreviewProvider::media`
  says a file is a film or a track), and each is shown as it comes: a
  poster may come before its player or after. What did not come is said
  after what did: `Media::NoPoster` after the player, `Media::NoPlayer`
  after the picture (Still frames, no GStreamer: the picture is then the
  preview, the still and the poster one request). A wish to play the
  track stepped to ends with the player's outcome, never with a picture.
- The picture is asked at the size it is shown at: to fit the content's
  area, its width and height in device pixels (`media_box`, measured
  where it is used, each in 512 px steps), so a resize within a step
  reuses it. Only the provider knows the film's shape before decoding
  it (`ffprobe`, turned as a phone's film is shown), so it fits the box
  (`PreviewRequest::fit`): a landscape film in a tall panel is as wide
  as the panel, not as tall. It carries the film's own size
  (`PreviewInfo::Video::size`), which it is laid out by. A film's frame
  is kept as JPEG at high quality, a cover as PNG. At 1080p and 4K a
  poster went from 4 and 16 MB of PNG at the film's size to 60 to 400
  KB, so the media cache's 128 MB holds hundreds of films rather than
  eight to thirty.
- A poster drawn larger than its pixels (a panel made wider, the whole
  screen) is read again at the next step, as a picture is (`Sharpen`,
  in fullscreen too), but alone (`Shell::sharpen_poster`): the frame at
  the time it says, never the whole preview, which would open a second
  player. The one shown stays until the sharper comes, so only its
  sharpness changes. A film playing shows its frames as they are.
- A film's desktop thumbnail is a frame its thumbnailer chose (another
  moment, often a film strip drawn on it), so it never stands in for
  the wait (`preview::placeholder`). It is the picture only when no
  frame comes (`Shell::film_without_frame`). Until a picture comes the
  panel is loading; the frame or the poster ends the wait
  (`Shell::film_waited`).
- A track's cover is its thumbnail at once, then the poster's: the same
  picture, sharper, glowing in the same colour. The thumbnail is drawn in
  the player's figure, with the player's bar (`Shell::shows_player`),
  from the player's opening (`sync_media`) or, standing in once the wait
  has lasted, before it: the worker marks a track's stand-in as its
  picture when the provider shows the file as media
  (`preview::placeholder`). Drawn as a plain picture under "Loading…",
  it was replaced by the figure (the first track a window opens waits
  that long: GStreamer reads its registry).
- Unasked, a film's own moment is the first keyframe from a tenth of it
  on (at most 30 s in), only keyframes decoded (`ffmpeg_keyframe`), and
  its time is the keyframe's: a picture always says the time of the frame
  it shows. Not the keyframe before: in a short clip with keyframes far
  apart it was the first frame, black where a film fades in. With none
  from there to the end (a short clip whose one keyframe is its first),
  the frame at the tenth, decoded exactly: `ffmpeg` fails with no frame
  to write, so what it printed tells, not how it ended (taken for a
  failure, such a clip waited for its player's frame, and with Still
  frames had no preview). A moment asked for (a still's step) is decoded
  exactly. Timestamps stay the file's, from its start (`-copyts
  -start_at_zero`). The time is read in the filter graph, before the
  output: a frame the output drops (one with a negative timestamp, as
  `-noaccurate_seek` left the keyframe before the seek) leaves the time
  of another. The opt-in test
  `a_film_s_own_moment_is_a_keyframe_shown_at_its_time` compares the
  picture with the frame at the time it says.
- Play and Seek cancel extraction and reject late posters; poster work
  never moves the position or delays playback.

## Generations, seeking and errors

- Position updates carry command generations. A control makes stale only
  what it changes of what the worker reports (`Asked`): a seek its frames
  and its state, queued and late; a pause, a rate or a track chosen its
  state; a volume or a mute neither, a state from before it given the
  volume asked (a slider dragged sends one with every move, and must not
  freeze the picture or the time); asking for frames or none nothing.
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
- Atlas eviction drops pending uploads before recycling texture slots, and
  a texture is destroyed only after in-flight GPU work finishes: GPUI can
  draw several scenes before Wayland presents one, so keeping only the
  latest preview is not enough. gpui's wgpu atlas does both (wgpu keeps a
  texture until the GPU is done with it); its tests, an upload to a removed
  texture among them, run in `just gpui-check`.
- Call `drop_image` in update or render code, never in prepaint or paint:
  the scene being drawn may already use the texture, whose sprites then
  draw nothing, or another image that took its slot.
- While the window is not shown (`Window::is_visible`: Wayland's
  suspended state, X11's fully obscured or unmapped window, Windows' and
  macOS' occlusion) a player sends no frames (`Control::Frames(false)`,
  from `Shell::media_shown`); it decodes on, its sound and position with
  it. Shown again, frames come at once; paused, the film is sought where
  it is, so its frame of now is drawn. A player opened meanwhile starts
  without frames.

## Track changes and subtitles

- A track change re-prerolls from READY, then restores position, rate and
  play state.
- Returning to READY fences the loop's inbox (`Player::fence`), dropping bus
  messages from the abandoned preroll. Otherwise a second change before the
  first prerolled selects streams decodebin3 no longer knows, and the video
  stops for good. `tools/media/check.py` switches twice while playing.
- A preroll's stream collection is answered on the thread that posts it,
  in the bus's sync handler (`answer`, from `Wanted`): decodebin3 lets go
  of its lock while it posts so that such a handler can ask, and asked for
  nothing by then it takes one stream of each kind, subtitles included.
  An answer from the loop's thread comes after that now and then, and
  takes a stream away from a paused pipeline: the preroll waits for good.
  A track change holds `Wanted` while it returns to READY, and the handler
  only tries it: answering the abandoned preroll would wait on that change,
  which waits for the posting thread. Each input's first collection is
  answered, a next file's too (`hand_over`, in `about-to-finish`, only
  tried): its first audio track, and its first subtitle where it has one,
  as its state then says; its answer holds back no frame of the file
  still playing. Once it starts, a track change takes that subtitle away
  where the file before showed none (`next_tracks`). Never asked of it,
  the switch stops for good: playbin3, choosing first for the collection,
  asks playsink for a subtitle output, and the stream synchronizer holds
  the new file's picture and sound until a subtitle starts there (GStreamer
  1.28.7). Left to decodebin3, the next file's subtitles were taken while
  the state said none.
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

- Fullscreen is the preview's, for every kind (previews.md, "The whole
  screen"): a film is drawn there as in the panel, its Fit filling the
  screen (previews.md, "What Fit makes of what is shown"); its bar is
  the panel's, over the picture, and hides when the pointer rests
  (`fullscreen_bar_floats`: a picture's, a film's and music's), the
  pointer with it, scrubbing keeping it.
- A film playing, in the panel or the dock, keeps the screen awake
  (`Shell::sync_awake`); music does not.
- Pointer movement must not take focus from an open playback popup.
- The seek bar, the panel's and the dock's, is the shared slider
  (`media::seek_bar_of`, `controls::slider`): its knob follows a press,
  then the pointer over the whole window while held, and the release,
  wherever it is, seeks there (`media::scrub`). Its keys are the
  player's (Left and Right seek), not its own.
- The volume is the speaker's popup, the panel's and the dock's alike
  (`media::volume_control`): its level, an upright slider
  (`controls::upright_slider`, nothing at its foot) and Mute at its foot,
  which leaves the popup open. A click on the speaker opens it, the
  slider given the keyboard (the `volume` place: Up and Down, Right and
  Left step it, Home and End take it to its ends, `ui.volume_step`; Tab
  goes between it and Mute); the wheel over the speaker or the slider steps it
  (`VOLUME_STEP`: a wheel's notch, a high-resolution wheel's parts once
  they make one, a touchpad's 60 px, `reader_scroll::Wheeled`),
  and over a speaker whose popup is closed opens it to show the level,
  passing (`toolbar::Passing`): it holds no keyboard, what had it keeps
  it (a field being typed in), and it closes once the pointer has been
  off the speaker and the popup `VOLUME_LINGER`. A press in it, or the
  keyboard reaching it, uses it: its slider takes the keyboard, given
  back when it closes, as a click's popup does. Passing is the popup's
  own, its close on its way too: whatever closes the popup ends both, and
  that close closes that popup alone, never one opened since. M mutes from
  anywhere in the player. Every move of a volume goes through
  `Shell::set_volume` (a step through `step_volume`, from the level
  shown, which it changes at once): the slider, its keys, the wheel, the
  player's Up and Down, the desktop's media controls. A volume moved is
  heard: it unmutes, as GNOME's, KDE's and Windows' own volume controls
  do. The speaker
  says how loud (`speaker_icon`): crossed out when muted, else no wave at
  nothing, one below half, two from half. Where the bar or the dock leaves the
  speaker out, More's Volume row opens it, above More. More lists no
  volume steps, and says the time only where the bar left the duration
  out (`preview_bar::Item::in_more`).
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
- It is read at the same places in the picture at any size: the middles
  of a grid over its width and its height, so a track's thumbnail and its
  own cover glow alike. Every n-th pixel in a row's order read only a few
  columns when the width was a multiple of n (four of a 1024 px cover):
  the test pattern's thumbnail glowed magenta, its cover green.

## Sound

- Volume is perceptual: the worker applies the cube of the control's
  position (`media::amplitude`), so half the control sounds half as loud.
- The filter is `scaletempo` (pitch kept at other speeds), then `spectrum`
  for audio. Either may be missing; playback does without.
- ReplayGain (`media.replay_gain`: off, track, album) reaches the
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
  (`Shell::neighbours`, `media.rs`, bounded), never while painting. Previous and
  Next show only when there are any.
- Stepping (`ui.media_step`) moves the pane's selection and the preview
  follows, so the list shows what plays; it keeps playing if it was
  (`PreviewState::autoplay`).
- At a track's end, `media.continue` (`player::Continue`) stops (the
  default), plays the next, or one at random. The pin draws a shuffle once,
  from the track playing, and walks it, so Previous retraces Next; a step
  within the same folder keeps it.
- Playlists (`playlists`) are read, not obeyed: nothing is fetched,
  remote entries are only named, missing ones marked, nothing written or
  removed. A track is a member that is a file of its own
  (`Member::own_file`).
- A track opened from a playlist keeps its rows as shown (`preview::Queue`):
  Previous, Next and the hand-over walk them until the playlist is left.

## Keep playing (the pin)

- A preview's session ends with the selection; a pinned session belongs to
  the window (`player.rs`).
- With `media.keep_playing` on (the default), a track is pinned when
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
- A track started from the files (Play, `ui.media_play`) is pinned once
  it opens, Keep playing on or off: the pin is where it plays, the
  preview not shown for it (`Shell::pin_started`). It takes the place of
  any pin; what the preview plays pauses, and a pin the preview showed
  is the preview's again. The queue is what was played (the selection,
  a playlist's tracks), so Next stops at its end; a track alone walks
  its folder's.
- What the pin opens (a track, a playlist's, its cover) is read on a
  worker in the lane of its remote (`fs::reach::lane`; a selection
  across remotes in its first file's), never in the lane every folder
  shares. It is typed there as a preview types it (`player::request_for`,
  `mime::identify`: the bytes where the name is not enough), and the pin
  is a film's (its picture, a dock tall enough) by that type
  (`Opened::video`), never by the name alone: a Theora `.ogg` the
  folder's names called music is shown as the film it is.
- While the panel shows the pinned session, every step (panel, dock,
  desktop) moves the selection in the pin's order, and the selection follows
  when the session advances by itself (`Shell::select_track`). The panel
  never holds a session the pin released.
- One reader pulls a session's updates: the panel while it shows it, else the
  pin, which keeps a film's last frame for the dock.
- The pin walks a queue of paths (same-kind folder files in pane order, or a
  playlist's tracks), resolved and opened on a worker.
- **The pin follows its tracks** (ADR-136). Each window observes the
  plans that end from the moment it is made (`Shell::observe_plans`, a
  cheap look while nothing is pinned): on the executor's thread, the
  pin's track and queue (`Watched`, the queue shared as `Arc<[PathBuf]>`,
  never copied per wake) are followed through the plan's changes
  (`op::Follow`) and what moved or went is sent (`Followed`); the window
  follows it (`Pinned::follow`, `Shell::pin_follows`). Moved or renamed,
  the track plays on where it went, the queue and the mark with it, the
  next track told again by its path. Taken away, tracks leave the queue,
  a shuffle keeping its order of those left, and the track played ends as
  at its end: the next of those left plays where Continue says one does,
  else the player closes; the panel showing that very session keeps it
  as its preview, stopped. Undo brings the file back, never the player.
- **A plan takes the file played** before it removes it (on Windows,
  moves or replaces it too): the session's manager keeps the file played
  and the one queued once its worker opened it (`fs::kept_open`,
  `Reply::Handing`), and when a plan lets go of them kills its worker,
  drops them once reaped, and says so
  (`Update::taken`), never as an error. The pin then waits where it stood
  (`Taken`) for that plan's end (`Shell::pin_taken`): gone, it ends
  as above; left or moved, it is opened again there, at its position,
  playing or paused as it was (`Opened::resume`). The session's word and
  the plan's end arrive in either order: each end followed is kept with
  what it did to the pin (`PlayerState::finished`, `after_taken`), so a
  word that comes after an end that took the track away takes nothing
  up (the pin went on past it already). A track that then fails to open
  leaves no pin of a dead session (`Pinned::ended`). The track queued and
  not opened yet is not taken this way: a plan that takes it has the
  worker forget it (`Request::Next` with none), the track playing on,
  and the pin tells its queue again after every plan's end.
- **What another program does** is followed only where a pane shows
  the pin's folder (`Shell::follow_outside`), looked at once a pane's
  names change: a rename the pane followed (`Pane::renames_since`) of the
  entry the pin's name was last listed as moves the pin (the name, never
  another name of its file: a hard link renamed beside it leaves the
  pin, `renamed_to`); a name listed, then gone, with no plan of Skwir's
  under way in the folder (`Executor::under_way_in`, asked before the
  plans' ends are read: a plan is under way until its observers were
  told) takes it away. Under way, it is asked once, and again only once
  a plan has started or ended (`Executor::history_version`), never at
  each sync. A name
  not listed yet says nothing: a plan's end is often followed before the
  folder lists where the file went. Elsewhere the track plays on, and a
  queued track found gone is passed: the helper does not hand over a
  file no longer there (`about-to-finish`), and the pin, opening a track
  that is not there (`Opened::gone`), drops it and goes on past it the
  way it last went (`Pinned::pass`, `PlayerState::stepping`).
- **The panel plays on through a rename** of the file it plays (not a
  playlist's member): the same file renamed is the same preview, its key
  moved to the new name (`preview.rs`, `renamed_playing`), never a new
  preview that would stop it or open a second player. Where a plan takes
  the file (a delete, on Windows a rename), its session ends; the window's
  plans' observer follows the panel's file too (`Watched::panel`), and the
  player is taken up again where it stood once the plan has ended
  (`PanelTaken`, `Shell::panel_resume` from `show_media`): left, the
  panel asks for the file again; moved, it follows it to its new name;
  gone, it shows what follows.
- Gapless: `MediaSession::play_next` names the next file; the worker hands it
  to playbin3 in `about-to-finish` on the streaming thread, using only what
  the loop left for it, and reports `Advanced` when it starts (clearing
  position and tags). The pin queues the next track as soon as one starts
  and only follows the switch (`Pinned::advance`). A session that cannot
  refuses `play_next`, and the pin opens the next track at the end, with a
  gap. Verified: one worker carried three tracks. A film gone on with
  gaplessly after one with a subtitle stream, into one with none, may
  never start: uridecodebin3 frees the old source from a streaming thread
  holding its play items' lock, while another thread of that source waits
  for the lock (`switch_and_activate_input_locked` against
  `uri_src_probe`, GStreamer 1.28.7, sampled with gdb; the helper's loop
  goes on reporting the old file's end). Music goes on, and the other
  film switches tried: from a plain film to either, from one with
  subtitles to another, shown or not, across codecs and containers.

## The dock

- `dock.rs` is the pinned player at the foot of the side panel, under the
  preview or under Properties alone: cover or film, spectrum meter,
  transport, seek bar, More and close.
- Its height is dragged on its top edge and remembered with the window's
  shape: decided in render, never measured. Below `TALL_FROM` it is a row,
  above it the halo figure. A film takes the height it gets (`VIDEO_HEIGHT`
  in a compact dock), as large as the dock lets it be, a small one
  enlarged, as a film's Fit is everywhere; its click area is the frame
  as drawn (`OnPicture` with the same `ObjectFit::Contain`).
- The pin is drawn in one place, always one (`player::PinPlace`, from the
  panel's switches and what the preview shows): the preview while it shows
  the pinned session or is opening a track that will; else the dock,
  wherever the panel is open, Properties alone included, which plays
  nothing, the track selected or not; with the panel closed, a strip of the
  controls in the last pane's footer. Whatever is switched, the pin never
  goes from the window while it plays.
- Only its picture answers clicks as a player (`Flash::Dock`), and stops the
  click there: a film's frame, not its letterbox (`picture::OnPicture`, from
  `ObjectFit::get_bounds` in prepaint); the cover or file icon; in the tall
  dock only the cover's disc (`halo::OnDisc`, tested against the circle).
  Each occludes the hover beneath it (the letterbox's, the figure's).
- Any other click outside the transport, and the footer strip's title, shows
  the track (`Shell::player_reveal`): Preview on, the one place it is seen as
  it plays (a press of the user's, which may turn the switch: ADR-058,
  decision 8), and its folder in the pane, the file selected, still playing.
  The desktop's Raise does the same.
- Full screen is the preview's: a double click on a film, its Fullscreen
  button and F (`PlayerRequest::Fullscreen`, `Shell::player_fullscreen`)
  reveal the track there, Preview off or on, which goes fullscreen once it
  shows the session (`fullscreen_wanted`, forgotten when the pin changes).
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
  announced. Without a session bus it logs once and does without. The
  session's bus started again, its objects and name are served again on the
  next connection (`bus::keep`), with nothing else said. Test it on a
  private bus (a `dbus-daemon` of the test's), never the user's.
- `player.request` and `player.now` are the same surface for scripts and
  agents.
