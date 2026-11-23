# ADR-148: Pictures Skwir reads itself

Status: accepted, 2026-11-22, on the user's answers (asked, after
ADR-146, that HEIC, AVIF, JPEG XL and camera raw files be shown at the
pixels they cover too; chose a libheif helper for HEIC and AVIF, the
embedded JPEG for raw files, jxl-oxide for JPEG XL). Built on Linux
that night; the helper in Debian's, Fedora's, Arch's and Nix's packages,
in the AppImage once it carried a libheif of its own, the next day
(decision 3). Amended the next day (2026-11-22),
as the user asked once it was measured: JPEG XL read in bands and a
region alone, by a decoder of Skwir's own around jxl-oxide (decision 1);
a photo of 24 megapixels or more had not been shown at all. Then, as the
user chose once HEIC was measured: a HEIC's or an AVIF's region decoded
from its tiles, and its thumbnail shown while it decodes and made into
the grid's thumbnails (decision 3). Then, as the user asked the same
day: a JPEG XL's eighth read alone (decision 1), and what a picture
carries smaller than itself made the picture wherever it covers what is
asked, in the preview as in the grid (decision 3).

## Context

- **These formats were the desktop thumbnailer's**, or ImageMagick's,
  drawn at 1024 pixels (`PICTURE_SIZE = XXLarge`, the `thumbnails`
  plugin) and final (`Pixels::Drawn`): never sharper, never read ahead,
  never kept. An iPhone's photo on a 4K screen, or zoomed, showed 1024
  pixels. Where no thumbnailer for the type was installed (HEIC on the
  machine this was built on), only ImageMagick drew it, when it could.
- **ADR-146's rule needs a decoder that takes a size**, in process or
  in a helper it can ask: the picture decoded at the pixels it covers,
  read ahead, kept, a region decoded past the bound.

## Decision

1. **JPEG XL is read in process by jxl-oxide** (pure Rust, MIT or
   Apache-2.0), through `image`'s decoding hook
   (`picture::formats`, registered once by every entry point that
   guesses a picture's format, and as the program starts): whatever reads
   a picture through `image` reads one, its size, a decode shrunk, a
   region, a thumbnail, a comic's page. `image/jxl` is among the types
   Skwir decodes (`thumb_providers::decodes`). The hook is Skwir's own
   decoder (`jxl::Decoder`, amended), around jxl-oxide's:
   - **Read in bands of rows**, as tall as what the decode may hold
     allows (`image`'s limits: 512 MiB by default, less what the picture
     holds in eight bits), in multiples of a group's 256 rows. jxl-oxide
     draws a frame in 32-bit floats, about 24 bytes a pixel: decoded
     whole, a photo of 24 megapixels held 635 MiB and failed within
     `image`'s limits, so it was not shown at all. In bands it holds
     400 MiB at 24 megapixels, 500 at 45, in about the same time: a
     tenth more, at most a fifth but once (each band decodes again the
     groups beside it, which its filters read).
   - **A region is read alone** (`Decoder::cut`, `picture::decode_region`):
     the groups it covers and those beside them, at full size, the only
     scale it is decoded at.
   - **Turned once**: jxl-oxide turns the pixels as the codestream says,
     and the EXIF Orientation a JPEG converted keeps beside it is not
     read again (`image` read it and turned such a picture twice).
   - **Its eighth is read alone** (`jxl::eighth`, through
     `picture::carried`): the LF image a lossy picture stores before its
     details, each sample the mean of 8 × 8 pixels, read from the file's
     first part (4 to 12 % of the photos measured) and decoded in the
     colours its whole decodes to (XYB to the colours it names, a
     recompressed JPEG's YCbCr, its subsampled chroma spread as a JPEG
     decoder spreads it). jxl-oxide reads it only to enlarge it into a
     frame of full size, so Skwir reads it with jxl-oxide's own crates
     (`jxl-frame`, `jxl-vardct`, `jxl-bitstream`, `jxl-color`,
     `jxl-oxide-common`, at the versions it pins): the LF global and
     each LF group's coefficients, nothing of the details beside them
     (which `try_parse_lf_group` reads too, a third of the time). Its
     adaptive smoothing is left out: it smooths what is enlarged, and at
     its own scale a sample is its block's mean already. Its light is
     the whole's means within a level; a recompressed JPEG's colour a few
     levels off in 4:2:0, where a chroma sample covers 16 × 16 pixels.
     None is read where the LF is not all the picture is: a lossless
     picture (Modular), one with alpha or other extra channels, an
     animation, a frame with an LF frame, patches or splines of its own,
     or not the picture's size. 24 and 45 megapixels: 31 to 137 ms on
     the preview's threads (an LF group of 2048 × 2048 pixels each), 61
     to 315 ms on one, at 11 to 22 MiB, where the whole took 0.74 to
     1.9 s at 366 to 500 MiB (release, a machine under load).
2. **A camera's raw file is shown by the largest JPEG it carries**
   (`raw::embedded_jpeg`): the camera's own picture of its sensor, full
   size in most raw files, as file managers and image viewers show it.
   Read from the container without decoding: the TIFF structure most raw
   files are (its IFDs, SubIFDs and EXIF's, a JPEG by its
   `JPEGInterchangeFormat`, by a strip compressed as one, or Panasonic's
   `JpgFromRaw`), Fuji's RAF header, Canon CR3's first track; the largest
   by its frame header's size. The raw file's Orientation is written into
   the JPEG where it says none (amended: a CR3's read from the `CMT1` box
   Canon keeps its IFD0 in, its JPEG saying nothing of it; a JPEG whose
   EXIF names no Orientation is turned too, its EXIF given way to one
   that says it). The JPEG then takes the JPEG path
   (`picture::Typed`, found and read once for a request, whatever
   readers it opens: the preview's size, what it carries and its pixels
   had each read it anew, three or four times a picture): its reduced
   scales, a region, read ahead and kept. The types read are listed (`raw::RAW`); an Olympus ORF (its
   preview in its maker note), a Canon CRW, a Sigma X3F stay with the
   desktop's thumbnailer. A raw file's sensor data is never developed.
   Amended (review before 0.6.1): a raw file is Skwir's file by file, not
   by its type. One whose directories, read in its first bytes, name no
   JPEG (a phone's DNG holds only uncompressed pictures) is left to the
   desktop's thumbnailer and ImageMagick (`raw::may_carry_jpeg`,
   `thumb_providers::decodes_file`), as before Skwir read raw files; the
   grid's decoder tries every raw file, and the next provider draws one
   it finds no JPEG in.
3. **HEIC and AVIF are read by a helper of their own**, libheif (with
   libde265 and dav1d, LGPL and BSD) in `skwir-heif-worker`, as the
   Poppler and DjVuLibre helpers are: isolated where the system allows,
   one picture a run, linking the system's libheif (1.13 or later, for
   `heif_init`, which loads the decoders Debian ships as plugins). It is
   taught to `image` as a decoder (`heif::register`, by the brands of a
   file's first box): its size asked of the helper when the decoder is
   made, without decoding, its pixels when they are read, the file written
   to a folder in memory for it. Amended: its size, and its thumbnail's,
   are asked once of a file's bytes (`size` says both; what it said is
   kept by the bytes' hash), so a request's decoders, each made anew by
   `image`, do not run the helper for it again, and a thumbnail too small
   for what is asked is not decoded; the pixels it writes are read
   straight into the decode's buffer, not held twice. Its types count as read here only where
   it works (`heif::reads`; amended: by its `--probe`, asked once on a
   worker, as found is not enough where `libheif` is optional, as on
   Arch), so elsewhere the desktop's thumbnailer
   draws them as before. A package of its own, `skwir-heif`, recommended
   by `skwir` (Debian's recommending libheif's decoder plugins); in Arch's
   `skwir`, `libheif` an optional dependency; a flag of Nix's package, on
   by default. In the AppImage and the tarball made of it, with a libheif
   built for it in its Ubuntu 22.04 root
   (`tools/package/appimage/heif.py`: Jammy's is 1.12, without `heif_init`
   or the tiles' calls): libheif 1.23.7 and libde265 1.1.3, the newest,
   the decoders built into libheif rather than loaded as plugins, AV1
   through Jammy's libaom, which the AppImage carries for GStreamer
   already; no encoder, tool or gdk-pixbuf loader. Fedora's libheif
   decodes AVIF but no HEVC: HEIC there needs RPM Fusion's
   `libheif-freeworld`. Windows and macOS keep the thumbnailer's picture
   until their own decoders (WIC, ImageIO) are asked. Amended:
   - **A region is decoded from the tiles it covers** (the helper's
     `region`, `heif::Decoder` cut by `picture::decode_region`): an
     iPhone's photo is a grid of 512 px tiles, which libheif 1.19 and
     later decodes one by one; the calls are looked up as the helper runs,
     so an older libheif (Ubuntu 24.04's 1.17) still runs it and decodes
     the whole, cut in the helper, only the part crossing to Skwir. The
     tiles are found and decoded as stored, the part then turned and
     mirrored by the helper as the file says (`irot`, `imir`, in its
     order): libheif's tiling as shown places a turned picture's first
     tile by the pixels of its last (`image % tile`, 1.19 to 1.23), not by
     what of it lies past the picture, so a portrait photo's part would
     come out shifted. A picture its file crops (`clap`) is decoded whole.
     The tiles decode on as many threads as libheif decodes a whole on
     (its default, 4). A part of 2000 × 1600 of 45 megapixels: about
     0.2 to 0.4 s at 25 to 35 MiB, where the whole took 0.9 to 1.6 s at
     279 MiB in the helper, and as much again in Skwir.
   - **Its thumbnail is the picture where it covers what is asked**,
     and stands for it while it decodes where it is smaller
     (`picture::carried`, as a progressive JPEG's eighth and a JPEG XL's
     are): the one the file carries (320 px in an iPhone's), turned as
     the picture, decoded by the helper in 10 to 25 ms. The grid's
     thumbnails of a side it covers are made from it, as the desktop's
     HEIF thumbnailers do, and so is a preview no larger (an iPhone's
     photo in a pane 320 pixels wide: 16 ms, where its whole took 1.6
     s in a debug build): the whole is not decoded for them. The helper
     gives none whose proportions are not the picture's, within a pixel
     of rounding (another picture's, or the picture's before it was
     cropped): the whole is decoded then.
   - **A run is asked for by whoever decodes** (review before 0.6.1,
     `heif::asking`): `image`'s hook reaches no argument of the caller's,
     so the caller names itself for its thread while it decodes. The run
     is cancelled with its request; the picture the user waits for (the
     preview's own, a zoom's part) has a turn of its own, never behind
     thumbnails or what is read ahead (two shared turns); a grid's
     thumbnail never waits for one: with none free it answers `Busy`, as
     ImageMagick's does, and is asked again once one is, its worker going
     on with other files. A thumbnail waiting held a scheduler worker
     each, all but one of them in a folder of AVIFs.

## Alternatives

- **ImageMagick at the room's size**: no new dependency, but one process
  and a whole decode per picture, and only where its build reads the
  format.
- **libheif linked into Skwir**: every package and the builds for
  Windows and macOS would carry it, and a crash in a decoder of untrusted
  files would take the window down.
- **A raw file developed** (libraw, or a Rust developer): seconds a photo,
  colours unlike the camera's.

## Consequences

- A JPEG XL or a raw file is shown, read ahead, kept and zoomed as a JPEG
  is, thumbnails included; the thumbnailer no longer draws them.
- A raw file without a JPEG of its own (rare: a DNG written without a
  preview) shows nothing of Skwir's; its type is listed only where its
  container carries one.
- A lossy JPEG XL asked at its eighth or less is shown from its eighth;
  asked larger, it decodes all its bands (no other reduced scale in
  jxl-oxide), holding the picture's eight bits whole, its eighth shown
  meanwhile. A lossless one, or one with alpha, has no eighth to read
  alone: it decodes whole however small it is asked. A HEIC or an AVIF
  asked larger than its thumbnail decodes whole (HEVC and AV1 have no
  smaller scale), in a helper started for its size and again for its
  pixels, its thumbnail shown meanwhile: an iPhone's photo of 12
  megapixels is some 50 MB written and read in memory.
- Skwir reads parts of a JPEG XL frame with jxl-oxide's own crates,
  which jxl-oxide moves with it: an update of jxl-oxide moves them by
  hand, and the test of the eighth against the whole's means
  (`jxl::tests`) says whether their reading still holds.
- Past the preview's bound, a JPEG XL decodes its region alone (2000 ×
  1600 of 45 megapixels in 150 to 320 ms, where the whole took 1.8 s or
  more), a HEIC or an AVIF the tiles its region covers; an AVIF is
  seldom stored in tiles, and decodes whole for its region, as a PNG
  does.
