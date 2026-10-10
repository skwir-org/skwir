# ADR-008: Optional GStreamer video playback

Status: accepted, 2026-09-19

## Decision

Extend `preview-media` with a persistent, initially paused playback session.
`skwir-media-worker` links GStreamer; the UI and platform adapter do not. The
worker uses `playbin3` for decoding, audio/video synchronization, stream selection,
subtitles, seeking and speed. Track changes re-preroll from READY with the desired
streams, then restore position, rate and play state. This avoids racing dynamic
subtitle sink replacement with paused flushing seeks; the UI retains its last
frame during the switch. Playbin's flags remain intact. Commands and small state
updates use JSON lines.
The core owns the toolkit-independent session interface and protocol.

Use one acknowledged BGRA frame slot in a private temporary directory on
`/dev/shm` on Linux, falling back to the ordinary temporary directory when
that is unavailable. It is reused throughout the session, read/written through
ordinary file APIs, and removed after the child is reaped. Other platforms use
their temporary directory. This avoids unsafe cross-process mappings and
per-frame files, but still involves CPU copies and a GPU upload. The worker's
appsink holds at most one frame and drops older frames. The manager retains only
the latest UI frame and rejects generations preceding the latest command.
GPUI atlas entries are explicitly evicted when replaced or closed.

Continuous playback output is capped at 1920 pixels on either axis, with square pixels.
The initial poster separately uses the installed FFmpeg renderer at source
resolution, bounded to 8192 pixels per side. Its cancellable extraction is optional,
cached, and never delays controls or changes the playback clock; the desktop
thumbnail remains available as immediate feedback and on extraction failure.
Software decoder parallelism is capped at four threads where the decoder exposes
`max-threads`, avoiding CPU-count-dependent reference-frame pools. Conversion
and scaling also use up to four threads where supported. On glibc, the manager
caps child allocator arenas at two: repeated decoder/overlay thread replacement
otherwise retains freed frame buffers in separate arenas. This applies only to
the player process. QoS is enabled with a
20 ms sink lateness budget so slow video can yield to the audio clock. Native output
playback resolution, zero-copy GPU surfaces and a second engine are deferred until
measurements justify their added complexity. Linux GPUI currently has no native
video-surface import path.

The session follows the selected file, pane and provider registration. Closing,
replacing or disabling a preview cancels it. Only the manager kills/reaps children
and removes temporary storage; UI destruction never waits. Decoder events wake
the worker, and output wakes the UI. There is no UI animation or polling loop,
and the manager thread does not poll either: it selects on the session's
cancellation signal and owns the only startup deadline.

A declined control (for example a seek on a non-seekable stream) is a
`Rejected` reply that surfaces as a transient status while the session
continues; only decoder errors and a lost runtime end a session, after which
the UI withdraws the controls.

`preview-media.backend` offers `auto`, `gstreamer`, and `stills`. Auto retains the
existing FFmpeg still renderer when the player/runtime is absent. Media errors
are reported rather than concealed by trying another decoder. `just run` builds
the optional worker; the Nix development shell supplies GStreamer and plugins.
Packagers may omit the worker and its native dependencies entirely.

## Licensing and alternatives

GStreamer is LGPL-2.1; its codec/plugin dependencies need separate consideration
when distributing them. This is less restrictive than MuPDF's AGPL, but is not
a permissive license. libVLC is another LGPL option. Supporting both immediately
would duplicate packaging, integration and test work without a demonstrated
benefit; the session interface leaves room for another provider later.

References: [GStreamer licensing](https://gstreamer.freedesktop.org/documentation/frequently-asked-questions/legal.html),
[playbin3](https://gstreamer.freedesktop.org/documentation/playback/playbin3.html),
[appsink](https://gstreamer.freedesktop.org/documentation/app/appsink.html).

## Verification

`tools/media/check.py` generates synthetic H.264 clips with two audio streams
and subtitles. It exercises paused seek, stream selection, volume/mute, rate,
clock progression, end-of-stream and replay, and reports worker-only first-frame
latency, delivered frame rate, CPU and PSS. Use a clocked fake audio sink for
automated checks, and `--audio` to exercise the installed sound backend.
Full UI measurements must include the child process and remain separate from
worker-only measurements and filesystem microbenchmarks.
