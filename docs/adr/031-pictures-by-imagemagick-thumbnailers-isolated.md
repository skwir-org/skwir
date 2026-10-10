# ADR-031: Pictures drawn by ImageMagick, and thumbnailers isolated

Status: accepted, 2026-10-03.

## Context

skwir decodes the common pictures in the process, in pure Rust (PNG,
JPEG, GIF, WebP, TIFF, OpenEXR…). Any other picture (HEIC, AVIF, JPEG
XL, a camera's raw file, a Photoshop or GIMP image) is drawn by the
desktop's thumbnailer for its type, for the grid and for the preview
(`thumbnails` plugin). Where the desktop installed none, the file has no
picture at all: on a desktop without libheif's thumbnailer an AVIF shows
nothing, though the machine may well be able to read it.

Three ways of reading more were weighed:

- **Decoding AVIF in the process.** rav1d, the Rust port of dav1d, with
  a container reader of our own: 0.17 s for a 12-megapixel picture on
  one thread with its assembly, 0.92 s without, and 2.7 MB of code in
  the UI process, assembly included (one machine, a synthetic picture).
  Declined: it reads one format, and puts a video decoder's assembly
  where nothing but pure Rust decodes today. HEIC has no such decoder.
- **KDE's thumbnailers.** They are Qt plugins loaded by KIO's thumbnail
  worker, with no command to call: using them means a helper in C++
  linked to Qt and KDE Frameworks, for one desktop. What Dolphin draws
  is already shared: it writes the freedesktop cache skwir reads first.
- **ImageMagick.** Often installed where no matching thumbnailer is, and
  it reads what its build links: on the reference machine 259 formats,
  AVIF, HEIC, JPEG XL, JPEG 2000, raw files (LibRaw), PSD, XCF and DjVu
  among them. A synthetic 12-megapixel AVIF became a 256-pixel
  thumbnail in 0.72 s (libheif's `heif-thumbnailer` 0.57 s, libvips'
  `vipsthumbnail` 0.73 s: decoding dominates), at about 213 MB of
  resident memory. Its history of vulnerabilities (ImageTragick) is
  long: formats that run scripts, read other files or addresses, or call
  other programs.

Separately, the desktop's thumbnailers ran with skwir's own rights:
every file, the network, the session. GNOME has run them inside
bubblewrap since 3.26; skwir has had the same sandbox since ADR-030.

## Decision

### ImageMagick, the last resort

A provider of a stage of its own, `Stage::LastResort`, after every
other: the cache, skwir's decoders and the desktop's thumbnailers
(`Generate`), and the renderers plugins keep for their formats
(`Fallback`, a DXF drawing's among them). The preview of a picture skwir
does not decode asks the desktop's thumbnailer first, then ImageMagick.

It takes an `image/*` file skwir does not decode itself when ImageMagick
reads its format. The format is the MIME database's answer, not a list
kept beside it: the type's globs (`image/x-canon-cr2` is `*.cr2`, the
format `CR2`), the file's own extension first when it is one of them,
named to ImageMagick explicitly (`CR2:/input/document.cr2[0]`), so the
bytes never choose another reader. Which formats an installed ImageMagick
reads is its own answer (`-list format`, asked once per binary and
date). ImageMagick 7's `magick` is used, else ImageMagick 6's `convert`
when its version says it is ImageMagick. Formats that read more than
the file's pixels are never asked for, whatever the type says
(`magick::REFUSED`: PostScript and PDF, vector and scripted drawings,
text, web pages, video, the network, the screen).

It runs only isolated (`skwir_platform::sandbox`): without bubblewrap
it takes nothing. Each run writes its own security policy into its
private folder and points ImageMagick at it:

- every format refused but the one read and PAM, the one written;
- every delegate refused, so no other program is run, and indirect
  reads (`@file`) too;
- 256 MiB of memory, 512 MiB mapped, 1 GiB of disk in the private
  folder, one thread, 30 seconds.

At most `magick::RUNNING` (two) run at once, whatever the number of
workers: each holds a picture's pixels in floating point, a
12-megapixel one about 200 MB. A request waiting its turn gives way
when it is cancelled. The first frame or page is drawn (a GIMP image
flattened, its layers being frames), turned upright, in sRGB, shrunk by
averaging to the size asked (`-scale`, 0.3 s cheaper than a filtered
resize at 1024 pixels) and written as 8-bit PAM, which skwir reads
back within the bounds it reads a thumbnailer's output.

A switch turns it off (`thumbnails.imagemagick`, Thumbnails, on by
default: it does nothing until ImageMagick is installed).

### The desktop's thumbnailers, isolated

A thumbnailer runs in the same sandbox as the converters: no network,
no display, no session bus; the file read-only at
`/input/document.<its extension>` (`%i`, and `%u` as its URI), its
output written in the private folder (`%o`); the system's programs,
libraries and fonts read-only; its home and caches in the private
folder. A thumbnailer installed outside the system's directories is
made visible at its own path. What it needs beyond that (a program in
`/opt`, a cache in the user's home) it does not get: it fails, and the
next provider is asked.

Where bubblewrap is missing, or cannot make namespaces (a container),
thumbnailers run as before, as other file managers run them: they are
what the desktop installed to draw its files. A sandbox that starts and
then fails is not retried outside it. `sandbox::available` now asks
bubblewrap once whether it can isolate, instead of looking for it on
the `PATH`, so a converter that needs the sandbox says so where it
cannot work.

### The sandbox

Its file name is `document`, then the input's extension kept to ASCII
letters and digits (none at all without one), so a name's brackets,
percent signs or colons reach no program as syntax. It binds read-only,
besides the system's programs and libraries: `/etc/alternatives`
(Debian's links to programs), the fontconfig cache, `/lib32`, `/sbin`
and NixOS's `/run/current-system`. A program outside them is bound at
its own path.

## Consequences

- With ImageMagick installed, pictures no thumbnailer draws have a
  thumbnail and a preview, on every desktop, at a cost of decoding a
  whole picture in another process (about 0.7 s for 12 megapixels).
  What it reads is what its build links: one without libheif reads no
  HEIC.
- Isolating a thumbnailer costs little: ffmpegthumbnailer drew a
  synthetic 20-second video in 59–63 ms alone and 63–68 ms in the
  sandbox, once warm (the reference machine).
- The preview says the size it drew (1024 pixels at most) for a picture
  whose file states none in EXIF (an AVIF), as it already did for a
  thumbnailer's.
- A raw photo is decoded whole by LibRaw, not from the preview its
  camera stored: slow for large files. HDR pictures are not tone-mapped
  and wide gamuts not converted, as for every other picture.
- A thumbnailer that relied on what the sandbox hides stops drawing.
  One that keeps a cache (GStreamer's registry) builds it again in each
  private folder.
- KDE's thumbnailers stay unused. If KDE users report formats missing,
  a helper is the way, in a decision of its own.
- Not covered: isolation outside Linux; vips as another last resort
  (no faster here, less often installed).
