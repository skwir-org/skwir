# Video playback

Use `just run` inside the development shell. Select a video and open Preview
(F11) or Peek (Space). Playback starts paused. The file thumbnail appears first,
then an optional FFmpeg job replaces it with a representative frame at source
resolution (up to 8192 pixels per side, without upscaling). It samples 10% into
the video, capped at 30 seconds; desktop thumbnailers do not expose a standard
frame timestamp, so the sharper poster may depict a different moment. Play and
Seek cancel extraction and prevent late posters from replacing playback. If
FFmpeg is absent or extraction fails, the thumbnail remains; without a thumbnail,
the initial decoded frame is used. The top toolbar provides play/pause,
seek dragging, elapsed/total time, mute, fullscreen, and an anchored options menu
for volume, playback speed, audio tracks and embedded subtitles. Narrow panels
move secondary controls into the options menu.

Click the picture to focus the player: Space plays/pauses, Left/Right seek five
seconds, M mutes, F toggles fullscreen, and Escape exits fullscreen or closes
the preview. Moving to another file stops the old session. Closing Peek pauses
playback if the side preview remains open; closing the final presentation stops
and reaps the worker.

Fullscreen fills the display with aspect-preserving video, scaling up smaller
sources and leaving black bars where needed. Its toolbar overlays the picture;
the controls hide after one second with a stationary pointer outside them and
reappear on mouse movement. Hover, seeking, open menus and keyboard focus keep
them visible. C remains a keyboard shortcut to toggle the controls; there is no
separate Hide/Show button. This uses one cancellable inactivity timer, not a polling loop. Space, arrows,
M, F and Escape still work with the panel collapsed. Exiting restores the prior
preview presentation and preserves an already-fullscreen application window.

Seeking updates the bar immediately. Status and frame messages carry the command
generation, so queued or late replies cannot restore an older position. During
GStreamer's asynchronous flushing seek the worker retains the requested position
until the pipeline finishes preroll, then resumes querying its clock.

A control the player cannot carry out, such as seeking or changing the speed
of a non-seekable stream, is answered with a `Rejected` reply: the status bar
shows the reason, the session continues, and the next state update restores
the actual position. The UI does not send seeks or rate changes while the
stream reports itself non-seekable, and a track change on such a stream
restarts it from the beginning instead of restoring the position. Only decoder
errors and a lost runtime end the session; the controls are then withdrawn
and the last picture stays.

The manager owns the single startup deadline (20 s from spawn to the first
preroll). The worker has no timer of its own: cancelling the session kills it.
The manager thread sleeps in one channel select over commands, replies and the
session's cancellation signal (`CancelToken::subscribe`), so closing a preview
reaps the worker immediately rather than on a polling interval. Inside the
worker, bus messages travel through the loop's channel from the bus sync
handler, so a wake and its payload are one object.

Settings → `preview-media.backend`:

- `auto` (default): GStreamer playback, falling back to FFmpeg stills when the
  optional player/runtime is unavailable.
- `gstreamer`: require the player; report missing dependencies explicitly.
- `stills`: use the previous FFmpeg frame browser.

A packaged installation needs `skwir-media-worker` next to `skwir` or on PATH,
and GStreamer with base/good playback plugins and suitable codec plugins.
`SKWIR_MEDIA_WORKER` overrides the executable. GStreamer loads only in the child,
not in the file manager. `cargo build -p skwir` does not link GStreamer; workspace
builds also build the optional worker and need its development libraries.

Continuous playback uses CPU BGRA transfer and GPU upload, capped at a
1920-pixel side even for 4K input. The frame slot lives on `/dev/shm` when
that is available and writable, and otherwise in the ordinary temporary
directory, which may be disk-backed. The manager reads each frame into a
buffer recycled from frames the UI did not take; a UI that keeps up still
receives one owned buffer per frame, and GPUI creates a new atlas texture per
frame. Reusing the atlas texture across frames is a follow-up that needs a
gpui patch (`RenderImage` does not return its pixel buffer). Decoding 4K
still costs more than decoding 1080p. The audio clock remains owned by
GStreamer; old video frames may be skipped if the UI cannot keep up. There is
no playback thread or native media library in the UI process when the preview
is closed.

## Reproduce the checks

```
just media-check
python3 tools/media/check.py --worker target/release/skwir-media-worker --size 3840x2160
```

The generator creates only synthetic material (moving test pattern, two sine
tracks, embedded text subtitles). Default checks use a clocked fake audio sink.
Use `--audio` for the real sound backend. Reported frame latency and drops are
worker-side delivery measurements, not compositor presentation or audible A/V
synchronization. Full application measurements must include both processes.

## Verification and resource measurements

Measurements use synthetic H.264 at 30 fps, two audio tracks, embedded subtitles,
software decode and CPU BGRA transfer. Output is capped at a 1920-pixel side.
Short samples on an active desktop are not portable hardware guarantees. CPU
percentages count one core as 100%. Frame timestamp gaps exclude manager, UI,
GPU and compositor latency; sink drop counters exclude upstream decoder skips.

Release-worker samples on a warm filesystem, with the clocked fake audio sink
and subtitles enabled after repeated track changes:

| Synthetic input | First frame | Delivered | Worker PSS | Worker CPU | p95 timestamp gap |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1920×1080 | 205 ms | 30.0 fps | 270.7 MiB | 52.7% | 3.7 ms |
| 3840×2160 | 197 ms | 30.0 fps | 405.1 MiB | 75.2% | 4.0 ms |

Both samples reported zero sink drops over the three-second measurement.
These first-frame times cover worker startup and preroll, not full UI latency.

The worker caps glibc allocator arenas at two to limit fragmentation when track
changes recreate native threads. In a synthetic 1080p stress check, 60 subtitle
switches kept PSS between 213 and 248 MiB at five-pair sampling points, with 25
threads throughout. Without the limit, PSS grew to 414 MiB after 40 switches.
Closing the preview reaps the process and releases its allocations.

Initial release-app samples in Peek, before the atlas fix below, using fresh
sessions with subtitles off and the clocked fake audio sink (five-second CPU
windows):

| State | Combined PSS | UI CPU | Worker CPU |
| --- | ---: | ---: | ---: |
| Fresh, preview closed | 102.7 MiB | 0.8% | none |
| 1080p playing | 362.6 MiB | 27.4% | 41.4% |
| 4K playing | 460.3 MiB | 30.8% | 72.8% |
| Closed after both clips | 123.1 MiB | 0.8% | none |

Paused sessions used 335.8 MiB combined for 1080p and 428.5 MiB for 4K, with
about 0.6–0.8% UI CPU and 1.2% worker CPU. No GStreamer library was mapped in the
UI process, and neither worker survived closing its final preview presentation.

The native check verifies paused subtitle pixels: enabling a track must change
the image, and disabling it must restore the clean frame. It also exercises
paused seek, track selection, volume/mute, clock progression at 1× and 1.5×,
end-of-stream and replay. Timing begins after asynchronous preroll. Track
changes re-preroll the pipeline and restore the current position and play state;
the UI keeps its last frame during the switch. Selection does not change
playbin's audio/video/text flags. Real-backend checks run with output muted;
audible lip synchronization has not been assessed.

Automated tests exercise provider cancellation, idle worker cleanup, unavailable
runtime versus malformed-media errors, bounded updates, stale generations, and
a wedged worker with a full command pipe. Manual UI checks exercised seek
dragging, keyboard playback, fullscreen, anchored options, fallback stills,
and scrolling the 10k-image grid and 100k-entry list. No native GStreamer library
was mapped in the UI process. The historical 100 MB UI memory gate remains open.

Lint and all 148 default tests pass (five optional native tests remain skipped
by the default suite). The default filesystem/thumbnail benchmark gate passes.
Its first image-stream sample was 15.003 ms against 12 ms; an identical 15-run
comparison passed for both the pre-change binary (2.596 ms) and current binary
(3.236 ms). The final default check passed at 3.246 ms, following another
12.216 ms outlier. Budgets were not changed; this measurement was already
variable before the playback work.

## Rapid selection regression

Rapid changes can replace a UI scene before Wayland presents it.
The Blade atlas patch cancels pending initialization/uploads before recycling
texture indices, then frees retired textures after prior GPU work completes.
This fixes the `flush_initializations` vacant-slot panic and prevents old uploads
from targeting a smaller replacement texture. Run `just atlas-check` for the two
GPU regression cases. The release application also passed 320 alternating video
selections at 35–550 ms intervals, followed by preview close and worker cleanup.
The unpatched application crashed after 49 changes with the same synthetic clips.
The patched application also scrolled 50 pages in each of the synthetic
10k-image grid and 100k-entry list without a panic; the final five-second idle
CPU sample was 1.4% of one core.
