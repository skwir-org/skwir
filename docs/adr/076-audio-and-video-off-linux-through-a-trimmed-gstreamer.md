# ADR-076: Audio and video off Linux through a trimmed GStreamer

Status: accepted, 2026-10-20, the user's choice among the alternatives
below; built the same day: Windows' package plays under Wine, macOS's is
looked over, never run. Amends ADR-041, point 4.

## Context

- **ADR-041 chose a helper per system** for audio and video off Linux,
  AVFoundation's on macOS and Media Foundation's on Windows, "not a bundled
  GStreamer (100-200 MB, codec licensing)", to be written on the machines.
  None was written; Windows' and macOS's builds to try (ADR-074, ADR-075)
  had no player.
- **The helper Skwir has is GStreamer's** (`crates/media-worker`), with
  its rules (`docs/invariants/media.md`): tracks chosen again through a
  return to READY behind a fence, subtitles drawn into the frame,
  ReplayGain, the tempo kept at another rate, the spectrum, BGRA frames
  through one slot.
- **The alternatives, researched on 2026-10-20** (`docs/STATUS-LOG.md`),
  each a size added to a package, zipped:
  - **A helper per system** (1–2 MB) would build those rules again,
    twice. AVFoundation opens no Matroska, WebM or Ogg in any macOS, and
    Apple offers no Vorbis decoder to apps; Media Foundation's formats
    depend on the Store extensions a machine has. Neither can be run here.
  - **A player of our own** on a trimmed FFmpeg or in Rust, with the
    system's decoders, as Firefox has (3–7 MB): months of work, an engine
    Linux would not share.
  - **libmpv** (8–9 MB) hands out no samples for the spectrum; **libVLC**
    is not lighter.
  - **GStreamer built by us** (gst-full, 7–9 MB, estimated): a build to
    keep for each release, its macOS build from Linux never done by
    anyone.
  - **GStreamer's own builds, trimmed, without FFmpeg** (14–17 MB,
    measured): no H.264, HEVC or AAC but the system's, so no 10-bit H.264,
    no H.264 on a Windows machine without a GPU decoder, no MPEG-2, DivX,
    AC-3, DTS, WMV.
  - **GStreamer's own builds with FFmpeg's decoders** (gst-libav), as
    Linux's distributions and Skwir's AppImage carry them: every format
    Linux plays.
- **Skwir already ships FFmpeg's H.264, HEVC and AAC decoders**: its
  AppImage carries Ubuntu's gst-libav. Keeping them out of Windows and
  macOS would not change what Skwir distributes, only where formats play.
- **What was tried here** with GStreamer 1.28.8's own builds: under Wine,
  the media check (`tools/media/check.py`: play, seek both ways, change
  audio track, subtitles on and off, rate, end, again) passed with the
  whole of it on VP9 and Opus, AV1 and Vorbis, H.264 and AAC; a set
  without FFmpeg played no H.264 or AAC there, Wine having no Direct3D
  and no Media Foundation decoder.

## Decision

1. **Windows' and macOS's packages carry GStreamer** for the media helper
   Linux has: one engine, one set of rules, on every system. The helpers
   per system of ADR-041 are not written.
2. **GStreamer's own builds, aligned with the AppImage**: every decoder
   FFmpeg has (gst-libav), H.264, HEVC and AAC in software among them, so
   a file plays where it plays on Linux. GStreamer ranks the system's
   decoders above FFmpeg's (Direct3D 11's on Windows, VideoToolbox's on
   macOS), so the GPU decodes where it can. No GPL plugin: what they
   decode (AC-3, MPEG-2), FFmpeg does too. This reverses ADR-041's
   "codec licensing": H.264 has US patents to about 2030 and HEVC and AAC
   pools are active; the project ships those decoders knowingly, as its
   AppImage and Linux's distributions do.
3. **One release pinned** (`tools/package/gstreamer.lock`, each file's
   SHA-256 as GStreamer publishes it), updated with Skwir's releases: its
   security updates are ours. `tools/package/gstreamer.py` makes it on
   Linux, kept in the build cache:
   - Windows' installer (Inno Setup 6.7, which innoextract cannot open)
     runs under a WoW64 Wine on an Xvfb display of its own; macOS's
     packages are opened with xar, pbzx and cpio.
   - The set: the core and playback; the containers (Matroska, MP4,
     Ogg, AVI, FLV, ASF, WAV, AIFF, MPEG-TS and PS); gst-libav, dav1d,
     Opus, Vorbis, FLAC; the subtitle renderers (pango, libass, DVD and
     DVB bitmaps); the helper's own (ReplayGain, tempo, spectrum); the
     system's decoders and audio output (Direct3D 11, WASAPI;
     VideoToolbox, AudioToolbox, Core Audio). The libraries they load
     follow from their imports.
   - Windows' libraries stripped of their debug information (MinGW's
     strip: GStreamer's keep it, libstdc++ 26.6 MB of it, 4.7 without),
     macOS's thinned to Apple silicon.
4. **Kept apart from any GStreamer the user installed**: the helper,
   finding GStreamer beside it, plays with its plugins alone, its own
   scanner and a registry in Skwir's cache (`carried_gstreamer`). On
   Windows it sits among GStreamer's libraries (`gstreamer\bin`), as
   Windows finds a program's libraries beside it; on macOS GStreamer is
   a framework bundle, `Contents/Frameworks/GStreamer.framework`, as a
   bundle's Frameworks holds code to be signed, its main binary
   libgstreamer (GStreamer's own re-exports libraries the set leaves
   out), found by the helper's rpath, its libraries by their own relative
   ones. Linux's packages keep the system's GStreamer.
5. **The licences carried**: each component's notice from GStreamer's
   tree (a library no component claims stops the recipe), the versions
   of those carried, where their sources are; the libraries stay
   GStreamer's files, as built, which a user may replace.

## Consequences

- **Every format the AppImage plays, on both systems** (gst-libav's 211
  decoders in GStreamer's build, read from the registry): 10-bit H.264,
  H.264 and HEVC without a GPU decoder, AAC on Windows' N editions,
  MPEG-2, DivX, AC-3, E-AC-3, DTS, WMV and WMA, styled ASS subtitles.
- **The downloads grow**: the GStreamer carried is 67.1 MB on Windows
  (24.4 MB zipped) and 62.4 MB on macOS (21.5 MB zipped); Windows' zip
  went from 40.2 to 65.2 MB and its installer from 42.6 to 68.6 MB,
  macOS's disk image from 36.7 to 58.6 MB and its zip from 38.1 to 60.5
  MB. Of it,
  FFmpeg's libavcodec is 14.8 and 12.9 MB; MoltenVK, which VideoToolbox's
  plugin loads through GStreamer's Vulkan library, 7.4 MB on macOS; the
  dav1d plugin, with its own copy of Rust's runtime, 5.1 MB on Windows.
  Lighter, later and apart: applemedia built without Vulkan, dav1d
  linked into the helper, or GStreamer built by us.
- **The patent stance is now one**: Windows, macOS and the AppImage ship
  the same decoders. The AppImage still carries Ubuntu's GPL plugins
  (ugly's); these packages do not.
- **Wine plays what software decodes**: the package's check plays H.264
  and AAC, VP9 and Opus through the helper; Direct3D's and
  VideoToolbox's decoders, DirectWrite, Core Audio and anything on a Mac
  are for people with the machines (`docs/HELP-WANTED.md`).
- **Under Wine, turning subtitles off while paused stalled** in some runs
  (3 of about 10 in the trial), never on Linux: a preroll after a return
  to READY that does not come. To look into on Windows.
- **Building the packages needs** about 2 GB of GStreamer's downloads,
  kept in the build cache, and WoW64 Wine in the `windows` shell.
