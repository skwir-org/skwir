# ADR-031: Pictures drawn by ImageMagick, and thumbnailers isolated

Status: accepted, 2026-10-03. Built. Extended by ADR-039 (isolation on
macOS and Windows).

## Context

skwir decodes common pictures in process, in pure Rust. Others (HEIC,
AVIF, JPEG XL, camera raw, PSD, XCF) are drawn by the desktop's
thumbnailer for their type; where none is installed the file has no
picture, though the machine may read it.

Options (reference machine, synthetic 12-megapixel pictures):

| Option | Measured | Verdict |
|---|---|---|
| AVIF in process (rav1d + own container reader) | 0.17 s one thread with assembly, 0.92 s without; 2.7 MB of code in the UI process | Rejected: one format, and a video decoder's assembly where only pure Rust decodes. HEIC has no such decoder. |
| KDE's thumbnailers | Qt plugins in KIO's worker, no command | Rejected: needs a C++ helper linked to Qt and KDE Frameworks, for one desktop. Dolphin's results already reach skwir through the freedesktop cache. |
| ImageMagick | 259 formats here (AVIF, HEIC, JPEG XL, JPEG 2000, raw via LibRaw, PSD, XCF, DjVu); AVIF → 256 px in 0.72 s at ~213 MB (`heif-thumbnailer` 0.57 s, `vipsthumbnail` 0.73 s) | Chosen, isolated: its vulnerability history (ImageTragick) is long. |

Separately, the desktop's thumbnailers ran with skwir's rights (every
file, the network, the session). GNOME has sandboxed them with
bubblewrap since 3.26; skwir has had that sandbox since ADR-030.

## Decision

### ImageMagick, the last resort

`Stage::LastResort` runs after the cache, skwir's decoders, the
desktop's thumbnailers (`Generate`) and plugins' renderers (`Fallback`).
A picture's preview asks the desktop's thumbnailer first, then
ImageMagick.

- **Takes** an `image/*` file skwir does not decode, if ImageMagick
  reads its format. The format comes from the MIME database's globs,
  the file's extension first, named explicitly
  (`CR2:/input/document.cr2[0]`) so the bytes never choose another
  reader. Readable formats are ImageMagick's own answer (`-list format`,
  once per binary). `magick` (7), else `convert` (6) when it is
  ImageMagick.
- **Never asks for** formats that read more than the file's pixels
  (`magick::REFUSED`: PostScript, PDF, vector and scripted drawings,
  text, web pages, video, network, screen).
- **Runs only isolated** (`skwir_platform::sandbox`), or takes nothing.
  Each run writes its own policy: every format refused but the one read
  and PAM; every delegate and indirect read (`@file`) refused; 256 MiB
  memory, 512 MiB mapped, 1 GiB disk, one thread, 30 s.
- **At most `magick::RUNNING` (two) at once**: each holds about 200 MB
  for 12 megapixels in floating point. Waiting requests give way when
  cancelled.
- **Output:** first frame or page (GIMP layers are frames), upright,
  sRGB, `-scale` (0.3 s cheaper than a filtered resize at 1024 px),
  8-bit PAM, read back within a thumbnailer output's bounds.
- `thumbnails.imagemagick` turns it off (on by default; inert until
  ImageMagick is installed).

### The desktop's thumbnailers, isolated

They run in the converters' sandbox: no network, display or session
bus; the file read-only at `/input/document.<ext>` (`%i`, `%u`); output,
home and caches in a private folder (`%o`); system programs, libraries
and fonts read-only; a thumbnailer outside the system's directories
bound at its own path. What else it needs it does not get: it fails and
the next provider is asked.

Where bubblewrap is missing or cannot make namespaces (a container),
thumbnailers run unisolated, as in other file managers. A sandbox that
starts and then fails is not retried outside it. `sandbox::available`
asks bubblewrap once whether it can isolate, so a converter that needs
the sandbox says so where it cannot work.

### The sandbox

The input is named `document` plus its extension kept to ASCII letters
and digits, so a name's brackets, percent signs or colons reach no
program as syntax. Also bound read-only: `/etc/alternatives`, the
fontconfig cache, `/lib32`, `/sbin`, NixOS's `/run/current-system`.

## Consequences

- With ImageMagick, pictures no thumbnailer draws get a thumbnail and a
  preview on every desktop, at ~0.7 s per 12 megapixels in another
  process; it reads what its build links (no libheif, no HEIC).
- Isolation costs little: ffmpegthumbnailer, 20-second synthetic video,
  59–63 ms alone, 63–68 ms sandboxed, warm.
- A raw photo is decoded whole by LibRaw, not from its embedded
  preview: slow. HDR is not tone-mapped, wide gamuts not converted.
- A thumbnailer relying on what the sandbox hides stops drawing; one
  with a cache (GStreamer's registry) rebuilds it per run.
- KDE's thumbnailers stay unused; missing formats reported by KDE users
  would need a helper, in a decision of its own.
- Not covered: isolation outside Linux (ADR-039 since); vips as another
  last resort (no faster here, less often installed).
