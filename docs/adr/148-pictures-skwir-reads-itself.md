# ADR-148: Pictures Skwir reads itself

Status: accepted, 2026-11-22, on the user's answers (asked, after
ADR-146, that HEIC, AVIF, JPEG XL and camera raw files be shown at the
pixels they cover too; chose a libheif helper for HEIC and AVIF, the
embedded JPEG for raw files, jxl-oxide for JPEG XL). Built on Linux
that night; the helper in Debian's, Fedora's, Arch's and Nix's packages,
not yet in the AppImage (decision 3). Amended the next day (2026-11-22),
as the user asked once it was measured: JPEG XL read in bands and a
region alone, by a decoder of Skwir's own around jxl-oxide (decision 1);
a photo of 24 megapixels or more had not been shown at all. Then, as the
user chose once HEIC was measured: a HEIC's or an AVIF's region decoded
from its tiles, and its thumbnail shown while it decodes (decision 3).

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
2. **A camera's raw file is shown by the largest JPEG it carries**
   (`raw::embedded_jpeg`): the camera's own picture of its sensor, full
   size in most raw files, as file managers and image viewers show it.
   Read from the container without decoding: the TIFF structure most raw
   files are (its IFDs, SubIFDs and EXIF's, a JPEG by its
   `JPEGInterchangeFormat`, by a strip compressed as one, or Panasonic's
   `JpgFromRaw`), Fuji's RAF header, Canon CR3's first track; the largest
   by its frame header's size. The raw file's Orientation is written into
   the JPEG where it says none. The JPEG then takes the JPEG path
   (`picture::open_typed`): its reduced scales, a region, read ahead and
   kept. The types read are listed (`raw::RAW`); an Olympus ORF (its
   preview in its maker note), a Canon CRW, a Sigma X3F stay with the
   desktop's thumbnailer. A raw file's sensor data is never developed.
3. **HEIC and AVIF are read by a helper of their own**, libheif (with
   libde265 and dav1d, LGPL and BSD) in `skwir-heif-worker`, as the
   Poppler and DjVuLibre helpers are: isolated where the system allows,
   one picture a run, linking the system's libheif (1.13 or later, for
   `heif_init`, which loads the decoders Debian ships as plugins). It is
   taught to `image` as a decoder (`heif::register`, by the brands of a
   file's first box): its size asked of the helper when the decoder is
   made, without decoding, its pixels when they are read, the file
   written to a folder in memory for it. Its types count as read here
   only where it is installed (`heif::reads`), so elsewhere the
   desktop's thumbnailer draws them as before. A package of its own,
   `skwir-heif`, recommended by `skwir` (Debian's recommending libheif's
   decoder plugins); in Arch's `skwir`, `libheif` an optional dependency;
   a flag of Nix's package, on by default. Left out of the AppImage and
   the tarball made of it (`tools/package/appimage/left-out`): Ubuntu
   22.04's libheif is 1.12. Fedora's libheif decodes AVIF but no HEVC:
   HEIC there needs RPM Fusion's `libheif-freeworld`. Windows and macOS
   keep the thumbnailer's picture until their own decoders (WIC, ImageIO)
   are asked. Amended:
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
   - **Its thumbnail stands for it** while it decodes
     (`picture::stand_in`, as a progressive JPEG's eighth does, ADR-146,
     6): the one the file carries (320 px in the files measured), turned
     as the picture, decoded by the helper in 10 to 25 ms; none where it
     carries none, or where it is no smaller than what is asked.

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
- A large JPEG XL decodes all its bands each time it is asked smaller
  (no reduced scale in jxl-oxide's decoder through `image`, its eighth
  not asked apart), holding the picture's eight bits whole; a HEIC or an
  AVIF decodes whole (HEVC and AV1 have no smaller scale), in a helper
  started for its size and again for its pixels, its thumbnail shown
  meanwhile: an iPhone's photo of 12 megapixels is some 50 MB written and
  read in memory.
- Past the preview's bound, a JPEG XL decodes its region alone (2000 ×
  1600 of 45 megapixels in 150 to 320 ms, where the whole took 1.8 s or
  more), a HEIC or an AVIF the tiles its region covers; an AVIF is
  seldom stored in tiles, and decodes whole for its region, as a PNG
  does.
