# ADR-148: Pictures Skwir reads itself

Status: accepted, 2026-11-22, on the user's answers (asked, after
ADR-146, that HEIC, AVIF, JPEG XL and camera raw files be shown at the
pixels they cover too; chose a libheif helper for HEIC and AVIF, the
embedded JPEG for raw files, jxl-oxide for JPEG XL). Built on Linux
that night; the helper in Debian's, Fedora's, Arch's and Nix's packages,
not yet in the AppImage (decision 3).

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
   Skwir decodes (`thumb_providers::decodes`). It is decoded whole: no
   reduced scale yet.
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
   are asked.

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
- A large JPEG XL decodes whole each time it is asked smaller (no reduced
  scale in jxl-oxide's decoder through `image`), and so does a HEIC or an
  AVIF, in a helper started for its size and again for its pixels: an
  iPhone's photo of 12 megapixels is some 50 MB written and read in
  memory.
- A picture of these formats past the preview's bound decodes whole for
  its region, as a PNG does.
