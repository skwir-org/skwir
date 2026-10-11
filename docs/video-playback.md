# Video playback

How video playback behaves, how to check it, and what it measured. The
binding rules are in `docs/invariants/media.md`; the decision is ADR-008.

## Behaviour

- Open a video in Preview (F11) or Peek (Space). Playback starts paused
  unless Play on selection is on.
- The file's thumbnail shows first; an optional FFmpeg job replaces it with
  a frame 10% in (at most 30 s), at source resolution up to 8192 px a side,
  never upscaled. Thumbnailers expose no timestamp, so the two may show
  different moments. If FFmpeg is missing or fails, the thumbnail stays;
  without a thumbnail, the first decoded frame shows.
- The toolbar has play/pause, seek bar, times, mute, fullscreen and an
  options menu (volume, speed, audio tracks, embedded subtitles), which takes
  the secondary controls in narrow panels.
- A click on the picture plays or pauses; a double click toggles
  fullscreen. With the player focused: Space plays or pauses, Left/Right
  seek 5 s, Shift+Left/Right step to the previous/next file, Up/Down change
  the volume, M mutes, F toggles fullscreen, C shows or hides the fullscreen
  controls, Escape leaves fullscreen or closes the preview. Space, the
  arrows, M, F and Escape also work with the panel collapsed.
- Selecting another file ends the session unless it is pinned (Keep
  playing). Closing Peek pauses if the side preview stays open; closing the
  last presentation stops and reaps the worker.
- Fullscreen fills the display at the source's aspect ratio (scaled up,
  black bars as needed). The toolbar overlays the picture, hides after 1 s
  with a still pointer outside it and returns when the pointer moves.
  Leaving fullscreen restores the previous presentation and keeps an
  already-fullscreen window.
- A control the stream refuses (seek or speed on a non-seekable stream)
  shows its reason; the next state update restores the real position. A
  session ended by an error withdraws the controls and keeps the last
  picture.

## Engine and installation

`preview-media.backend`:

| Value | Behaviour |
| --- | --- |
| `auto` (default) | GStreamer playback; FFmpeg stills when the player or runtime is missing |
| `gstreamer` | require the player; report missing dependencies |
| `stills` | the FFmpeg frame browser only |

- Playback needs `skwir-media-worker` next to `skwir` or on PATH
  (`SKWIR_MEDIA_WORKER` overrides), and GStreamer with the base and good
  playback plugins plus codec plugins.
- `cargo build -p skwir` does not link GStreamer. Workspace builds also
  build the worker and need its development libraries.

## Frame path

- Frames cross as CPU BGRA and are uploaded to the GPU, capped at a 1920 px
  side (`MAX_SIDE`) even for 4K input; decoding 4K still costs more than
  1080p.
- The frame slot lives on `/dev/shm` when it exists and is writable,
  otherwise in the temporary directory, which may be on disk.
- The manager reads each frame into a buffer recycled from frames the UI did
  not take. A UI that keeps up still gets one owned buffer per frame, and
  GPUI makes a new atlas texture per frame. Reusing the texture needs a gpui
  patch (`RenderImage` does not return its pixel buffer): not done.
- If the UI cannot keep up, old video frames are skipped; GStreamer keeps the
  audio clock.
- With the preview closed, the UI process has no playback thread and no
  native media library.

## Reproduce

```
just media-check
python3 tools/media/check.py --worker target/release/skwir-media-worker --size 3840x2160
```

- The generator makes only synthetic material: a moving test pattern, two
  sine audio tracks, embedded text subtitles.
- The default checks use a clocked fake audio sink; `--audio` uses the real
  sound backend.
- Reported frame latency and drops are worker-side delivery, not compositor
  presentation or audible A/V sync. Full-application measurements must
  include both processes.

## What the checks cover

- Native check (`tools/media/check.py`): paused subtitle pixels (enabling a
  track changes the image, disabling it restores the clean frame), paused
  seek, track selection, volume and mute, clock progression at 1× and 1.5×,
  end of stream and replay. Timing starts after asynchronous preroll. Track
  changes keep the last frame shown and leave playbin's audio/video/text
  flags alone.
- Real-backend checks run with output muted. **Audible lip sync has not been
  assessed.**
- Automated tests: provider cancellation, idle worker cleanup, unavailable
  runtime versus malformed media, bounded updates, stale generations, a
  wedged worker with a full command pipe.
- Manual UI checks: seek dragging, keyboard playback, fullscreen, the options
  menu, fallback stills, scrolling the 10k-image grid and the 100k-entry
  list. No GStreamer library was mapped in the UI process.

## Measurements

Synthetic H.264 at 30 fps, two audio tracks, embedded subtitles, software
decoding, CPU BGRA transfer capped at a 1920 px side. Short samples on an
active desktop, not portable guarantees. CPU: one core = 100%. Timestamp
gaps exclude manager, UI, GPU and compositor latency; sink drop counters
exclude upstream decoder skips.

### Worker alone

Release worker, warm filesystem, clocked fake sink, subtitles on after
repeated track changes:

| Input | First frame | Delivered | Worker PSS | Worker CPU | p95 timestamp gap |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1920×1080 | 205 ms | 30.0 fps | 270.7 MiB | 52.7% | 3.7 ms |
| 3840×2160 | 197 ms | 30.0 fps | 405.1 MiB | 75.2% | 4.0 ms |

Zero sink drops over the 3 s measurement. First frame covers worker start
and preroll, not UI latency.

### Allocator arenas

The worker caps glibc arenas at two, because track changes recreate native
threads. Synthetic 1080p, 60 subtitle switches:

| Arena cap | PSS | Threads |
| --- | --- | --- |
| 2 | 213–248 MiB at every sample | 25 throughout |
| none | 414 MiB after 40 switches | — |

Closing the preview reaps the process and frees its memory.

### Whole application

Release app in Peek, before the atlas fix below, fresh sessions, subtitles
off, clocked fake sink, 5 s CPU windows:

| State | Combined PSS | UI CPU | Worker CPU |
| --- | ---: | ---: | ---: |
| Fresh, preview closed | 102.7 MiB | 0.8% | none |
| 1080p playing | 362.6 MiB | 27.4% | 41.4% |
| 4K playing | 460.3 MiB | 30.8% | 72.8% |
| 1080p paused | 335.8 MiB | 0.6–0.8% | 1.2% |
| 4K paused | 428.5 MiB | 0.6–0.8% | 1.2% |
| Closed after both clips | 123.1 MiB | 0.8% | none |

No GStreamer library was mapped in the UI process, and no worker outlived
its last presentation. The 100 MB UI memory gate remains open.

The filesystem/thumbnail bench gate passed at 3.246 ms, after two
image-stream outliers (15.003 ms and 12.216 ms against a 12 ms budget). An
identical 15-run comparison passed for the binaries before (2.596 ms) and
after (3.236 ms) the change: the measurement was already variable. Budgets
unchanged.

## Rapid selection (atlas regression)

Rapid changes can replace a UI scene before Wayland presents it. The Blade
atlas patch cancels pending initialization and uploads before recycling
texture indices, and frees retired textures after earlier GPU work
completes. This fixed the `flush_initializations` vacant-slot panic and
stopped old uploads from targeting a smaller replacement texture.

- `just atlas-check` runs the two GPU regression cases.
- Patched release app: 320 alternating video selections at 35–550 ms
  intervals, then preview close and worker cleanup, without a panic.
  Unpatched: crashed after 49 changes with the same clips.
- Patched: 50 pages scrolled in each of the 10k-image grid and 100k-entry
  list without a panic; final 5 s idle CPU 1.4% of one core.
