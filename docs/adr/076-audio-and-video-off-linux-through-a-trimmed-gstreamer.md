# ADR-076: Audio and video off Linux through a trimmed GStreamer

Status: proposed, 2026-10-20, after a trial on Linux: GStreamer 1.28.8's
own builds for Windows and macOS, the media helper built against them,
Windows' tried under Wine (`docs/STATUS-LOG.md`). It awaits the user's
decision: it grows each download by about 15 MB and carries LGPL
libraries. Amends ADR-041, point 4.

## Context

- **ADR-041 chose a helper per system** for audio and video off Linux,
  AVFoundation's on macOS and Media Foundation's on Windows, "not a bundled
  GStreamer (100-200 MB, codec licensing)", to be written on the machines.
  Nothing of them is written; Windows' and macOS's builds to try (ADR-074,
  ADR-075) have no player.
- **The helper Skwir has is GStreamer's** (`crates/media-worker`), with
  its rules (`docs/invariants/media.md`): playbin3's tracks chosen again
  through a return to READY behind a fence, subtitles drawn into the
  frame, ReplayGain (`rgvolume`, `rglimiter`), the tempo kept at another
  rate (`scaletempo`), the spectrum (`spectrum`), BGRA frames through one
  slot. A helper per system would build each of these again, twice:
  - AVFoundation opens no Matroska, WebM or Ogg, and its frame output
    (`AVPlayerItemVideoOutput`) leaves subtitles out.
  - Media Foundation has no simple way to the audio for the spectrum
    (an effect transform of its own) nor to subtitles.
  - Neither can be tried here: Wine's Media Foundation is itself built on
    GStreamer.
- **GStreamer can decode through the system**, which answers both of
  ADR-041's objections where they bite: on Windows, Direct3D 11's
  decoders (the GPU's: H.264, HEVC, VP8, VP9, AV1, MPEG-2) and Media
  Foundation's (AAC, MP3); on macOS, VideoToolbox's (H.264, HEVC, MPEG-2,
  ProRes) and AudioToolbox's (AAC, MP3). What remains in software is free
  of royalties: VP8, VP9 (libvpx), AV1 (dav1d), Opus, Vorbis, FLAC,
  Theora, MP3 (mpg123; its patents ran out in 2017), WavPack.
- **The trial, 2026-10-20**, with GStreamer's own builds of 1.28.8, each
  checked against its published SHA-256:
  - Windows (MinGW, x86-64): the helper, unchanged, built against it in a
    minute. Under Wine, with the whole of it, `tools/media/check.py`
    (play, seek both ways, change audio track, subtitles on and off, rate,
    end, again) passed on VP9 and Opus, AV1 and Vorbis, H.264 and AAC
    (the last by gst-libav, which the set below leaves out): 30 frames a
    second, none dropped.
  - The set below, stripped: 43.7 MB, 14.3 MB zipped (DirectWrite's
    subtitles), or 49.7 MB, 16.7 MB zipped (pango's). Under Wine it played
    VP9, AV1 and MP3; AAC and H.264 found no decoder, since Wine registers
    no Direct3D decoder and no Media Foundation audio decoder. Pango drew
    the subtitles; DirectWrite's overlay has no rank, and given one it
    stalled under Wine, whose DirectWrite lacks what it needs.
  - Turning subtitles off while paused stalled in 3 of about 10 runs
    under Wine (once with the whole set), never on Linux (5 of 5): a
    preroll after a return to READY that does not come.
  - macOS (the framework, universal): the helper links against its
    libraries by `@rpath`. The set below, arm64 alone: 46.9 MB, 14.6 MB
    zipped; MoltenVK, which VideoToolbox's plugin loads through
    GStreamer's Vulkan library, is 7.4 MB of it. Nothing run.
  - The packages today: Windows' zip 40.2 MB, macOS's disk image 36.7 MB.

## Decision

1. **Windows' and macOS's packages carry GStreamer**, trimmed, for the
   media helper Linux has: one engine, one set of rules, on every system.
   The helpers per system of ADR-041 are not written.
2. **GStreamer's own builds**, one version pinned with each file's SHA-256
   (as `tools/package/pdfium.lock` pins PDFium), updated with Skwir's
   releases: its security updates are ours. Built on Linux as the
   packages are: Windows' installer (Inno Setup 6.7, which innoextract
   cannot open) run under Wine on a display of its own, macOS's package
   opened with xar, pbzx and cpio; the libraries stripped of debug
   information (MinGW's `strip`: GStreamer's keep theirs, libstdc++ 26.6
   MB of it, 4.7 without) and macOS's thinned to arm64.
3. **The set:**
   - Core: `coreelements`, `typefindfunctions`, `playback`, `app`,
     `videoconvertscale`, `audioconvert`, `audioresample`, `volume`,
     `autodetect`, `pbtypes`, `subparse`; the helper's own `replaygain`,
     `audiofx`, `spectrum`.
   - Containers: `matroska`, `isomp4`, `ogg`, `avi`, `flv`, `wavparse`,
     `mpegtsdemux`, `mpegpsdemux`, `audioparsers`, `videoparsersbad`,
     `id3demux`, `apetag`.
   - Software decoders: `vpx`, `dav1d`, `opus`, `vorbis`, `flac`,
     `theora`, `mpg123`, `wavpack`.
   - The system's: `d3d11`, `mediafoundation`, `wasapi2` on Windows;
     `applemedia`, `osxaudio` on macOS.
   - Subtitles: `pango`, until DirectWrite's (`dwrite`, smaller, the
     system's fonts) is tried on Windows.
   - Never gst-libav, openh264, nor a GPL or "restricted" plugin: no
     software decoder of H.264, HEVC or AAC is carried.
4. **Kept apart from any GStreamer the user installed:** its plugins
   found where the package puts them (`GST_PLUGIN_SYSTEM_PATH_1_0`), its
   registry in Skwir's cache, its scanner its own; on Windows its DLLs in
   a folder of their own, on macOS in `Contents/Frameworks`, reached by
   the helper's rpath.
5. **Its licences are carried**, those of each library in the set
   (GStreamer's tree holds them, `share/licenses`), with where GStreamer's
   sources for that version are; the libraries stay separate files, as the
   LGPL asks.

## Consequences

- **The downloads grow by about 15 MB**: Windows' zip from 40 to about
  55 MB, macOS's disk image from 37 to about 51 MB.
- **H.264 and HEVC play only through the GPU**: not on a Windows machine
  without a hardware decoder (a virtual one, an old GPU); AAC not on
  Windows' N editions without the Media Feature Pack. There, a still frame
  from FFmpeg, if installed, stands in (`media.backend`, automatic).
- **What decodes differs by system**, though the helper does not: a file
  may play on one and not another. The decoder is GStreamer's choice by
  rank, as on Linux (`docs/invariants/media.md`).
- **Not yet known:** whether anything plays on Windows or a Mac; the
  stall when subtitles are turned off, under Wine; DirectWrite's
  subtitles; the helper's memory there.
