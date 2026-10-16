# ADR-008: Optional GStreamer video playback

Status: accepted, 2026-09-19. Amended by ADR-016 (one plugin for audio and
video) and 2026-10-16 (no thumbnail before a film's frame). Area rules:
`docs/invariants/media.md`.

## Context

ADR-003 left playback for later: `media` showed FFmpeg stills with
five-second seeking. A player needs a persistent session (decoding, sync,
streams, subtitles, seeking, speed) without a media framework in the UI
process, without a polling or animation loop, and without unsafe
cross-process mappings.

## Decision

- **Worker.** `media` gains a persistent session that starts paused.
  `skwir-media-worker` links GStreamer; the UI and platform adapter do not.
  `playbin3` does decoding, sync, stream selection, subtitles, seeking and
  speed, its flags left intact. Commands and small state updates are JSON
  lines; core owns the toolkit-independent session interface and protocol.
- **Track changes** re-preroll from READY with the wanted streams, then
  restore position, rate and play state, rather than racing dynamic
  subtitle-sink replacement with paused flushing seeks. The UI keeps its last
  frame meanwhile.
- **Frames.** One acknowledged BGRA frame slot, reused for the session, in a
  private temporary directory (on Linux under `/dev/shm`, else the ordinary
  one), read and written with ordinary file APIs, removed after the child is
  reaped. No unsafe mappings or per-frame files; the cost is CPU copies and a
  GPU upload. The appsink holds at most one frame and drops older ones; the
  manager keeps only the latest and rejects generations older than the latest
  command; GPUI atlas entries are evicted explicitly when replaced or closed.
- **Resources.**
  - Playback output at most 1920 px per axis, square pixels.
  - The poster comes from installed FFmpeg at source resolution (at most
    8192 px per side): cancellable, optional, cached, never delaying controls
    or moving the clock. Without one, the player's first frame is the
    picture; the desktop thumbnail shows only when no frame comes (shown
    meanwhile until 2026-10-16, it was a second picture the frame
    replaced, a flicker).
  - Software decoders at most 4 threads where `max-threads` exists (no
    reference-frame pools that grow with the CPU count); conversion and
    scaling at most 4 where supported.
  - On glibc, the player process gets at most two allocator arenas: replaced
    decoder and overlay threads otherwise kept freed frames in separate
    arenas.
  - QoS on, with 20 ms sink lateness, so slow video yields to the audio clock.
  - Deferred until measured to be worth it: native-resolution output,
    zero-copy GPU surfaces, a second engine. Linux GPUI cannot import a
    native video surface.
- **Lifetime.** The session follows the selected file, the pane and the
  provider's registration; closing, replacing or disabling the preview
  cancels it. Only the manager kills and reaps children and removes temporary
  storage; UI destruction never waits. Decoder events wake the worker, output
  wakes the UI. Nothing polls: the manager selects on the cancellation signal
  and owns the only startup deadline.
- **Errors.** A declined control (a seek on a non-seekable stream) is a
  `Rejected` reply, shown as a transient status; the session goes on. Only
  decoder errors and a lost runtime end it, and the UI withdraws the
  controls. Errors are reported, never hidden by trying another decoder.
- **Choice.** `media.backend` is `auto`, `gstreamer` or `stills`;
  auto keeps FFmpeg stills when the player or its runtime is absent.

Licensing and alternatives: GStreamer is LGPL-2.1 (less restrictive than
MuPDF's AGPL, not permissive); its codec and plugin dependencies need their
own review when distributed. libVLC (also LGPL) was not added: two engines
would duplicate packaging, integration and tests with no shown benefit; the
session interface leaves room for one. See
[licensing](https://gstreamer.freedesktop.org/documentation/frequently-asked-questions/legal.html),
[playbin3](https://gstreamer.freedesktop.org/documentation/playback/playbin3.html),
[appsink](https://gstreamer.freedesktop.org/documentation/app/appsink.html).

## Consequences

- Playback is optional: `just run` builds the worker and the Nix shell
  supplies GStreamer; packagers may omit the worker and its dependencies,
  leaving stills.
- Every frame costs CPU copies and a GPU upload until a zero-copy path is
  justified.
- `tools/media/check.py` generates synthetic H.264 clips with two audio
  streams and subtitles and exercises paused seek, stream selection,
  volume/mute, rate, clock progression, end of stream and replay. It reports
  worker-only first-frame latency, delivered frame rate, CPU and PSS, with a
  clocked fake audio sink (`--audio` uses the installed sound backend).
- Full UI measurements include the child and stay apart from worker-only
  numbers and filesystem microbenchmarks.
