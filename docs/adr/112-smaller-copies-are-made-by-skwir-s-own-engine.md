# ADR-112: Smaller copies are made by Skwir's own engine

Status: accepted 2026-11-01 with the user; built the same day, as
`docs/PLAN.md` ("Smaller copies") records: checked on Linux, compiled for
Windows and macOS (`just lint-all`), not run there.

## Context

- **The user asked** for a way to make a PDF smaller, from the Compress
  submenu of a PDF and the PDF preview's More menu, with a dialog of
  choices like Print's, and whether BentoPDF's Hyper Compress could do it,
  or do it in the print pipeline.
- **Hyper Compress** is a C++ engine built into a patched PDFium
  (jpegli, jbig2enc, Leptonica, AFDKO), finished by a qpdf pass, AGPL-3.0
  or commercial. Its Linux worker is 14.7 MB (5.7 MB in a zstd package,
  qpdf 1.5 MB more), needs glibc 2.38 and a recent libstdc++, has no
  build for Intel Macs or ARM Linux, and builds only from a 10 GB PDFium
  checkout with Google's tools; no distribution packages it.
- **A trial** (2026-11-01, Hyper 1.0.3 isolated without network, MuPDF
  1.28, Ghostscript 10.08, qpdf 12.4; SSIM of sampled pages in gray at
  150 dpi; the reference machine loaded, times rough) on public PDFs:
  - photographs: at equal quality Hyper and MuPDF were even (Hyper's
    low 8.2 % of the size at SSIM 0.981, MuPDF's medium 9.5 % at
    0.982); Hyper's smaller files came from harsher presets (its medium
    0.93), JPEG 2000 buying it some 15 %;
  - a LaTeX paper without loss: Hyper 66 %, MuPDF 78 %, qpdf 80 % (its
    font work);
  - a tax form of 199 fields carrying rights its maker signed (`/Perms`
    `/UR3`, Adobe's Reader extensions): Hyper kept it as it was, as a
    signed PDF; MuPDF saved 29 % by keeping its 289 streams as 42 (each
    field's same appearance stored once), breaking those rights;
  - black-and-white scans (CCITT): Hyper 24 % on one by JBIG2's symbols
    matched (a lossy coding that can swap characters), nothing on a 45 MB
    one after 10 minutes per preset; MuPDF 6 to 10 %;
  - Ghostscript removed the form's 199 fields and its tags, and lost
    6 % of the paper's words and 37 % of a scan's text layer.
- **PDFium cannot edit safely as shipped.** Its writer of a page's
  drawing expresses RGB and gray only (a page drawn again loses CMYK,
  spot colours, patterns), its JPEG encoder is Windows' alone, and an
  image replaced through a page's objects copies one several pages share:
  Hyper patched about fifteen of its files for these.
- **MuPDF's helper** (AGPL, ADR-014) is optional on Linux and not in the
  macOS and Windows packages; it holds a whole document in memory (2 GB
  for the 45 MB scan).
- **Pictures need the same core**: shrinking a photograph to send it is
  decode, resize and encode, as a PDF's images are.
- **The print path** already keeps a PDF's pages vector (PDFium composes
  a page range, a booklet, a scale, ADR-111). Pages without a PDF behind
  them (a DjVu, a comic) were drawn and written as four-byte-a-pixel
  PNG at about 350 dpi.

## Decision

1. **One Reduce Size…** (`shrink.reduce_ask`, the dialog, the user's
   alone) in the Compress submenu of a selection of JPEG, PNG and PDF
   files, and in the preview's menu of one; `shrink.reduce` makes the plan
   for scripts and agents. Each file gets a copy beside it (or in
   `dest`), `name (reduced).ext`, a step of a plan undone to the trash.
   A copy that would not be smaller is not made: the step says so
   (`op::Unneeded`, new in core: the executor counts it done and the plan
   goes on), and what the steps say is told in the plan's toast after
   what was made (`StepContext::say`; first a notification of the plugin's
   own, which covered the toast and its Undo, changed the same evening).
2. **Four qualities** (`skwir_core::shrink::Quality`), each saying what
   it does to what is chosen: Lossless (nothing re-encoded at a loss),
   Print (pictures 3840 pixels across, a PDF's images 300 dpi from 450,
   JPEG 85), Screen (1920, 150 dpi from 225, JPEG 75 for PDFs and 80 for
   pictures), Small (1280, 96 dpi from 144, JPEG 60 and 70); in gray; a
   picture's location and camera details left out (its date stays); at
   most a size, smaller steps tried below the quality chosen, the
   smallest kept when none fits.
3. **The engine is Skwir's** (`skwir-shrink`, pure Rust: image,
   jpeg-encoder, lopdf, hayro-ccitt), run by the print helper's
   `--shrink` mode, isolated as the helper is: no program of its own, the
   same on every system. Neither Hyper, nor MuPDF, nor Ghostscript.
4. **A PDF is rewritten object by object.** Its pages' drawing, text,
   fonts, links, forms, outline, tags and metadata are never rewritten.
   Each image is replaced where it is stored, in its own colour space:
   samples of 8 bits and 1, 3 or 4 components (gray, RGB, ICC, CMYK,
   Lab, one spot colour) resized when drawn at more dots per inch than
   the quality keeps (its largest placement, read from the pages'
   drawing and the forms it draws), made gray when asked, written as JPEG
   (4:2:0, optimized tables, progressive, `ImageMagick`'s quantization
   tables, mozjpeg's) where the quality loses, else as Deflate with PNG's
   predictor, whichever is smaller; a flat image (text, a drawing: one
   colour over a good part of it) becomes JPEG only at 85 or more and
   when that saves a quarter. Its soft mask is resized with it. An image
   of a palette is written as the colours its indexes stand for, in the
   palette's own space (a scan's gray photographs often are). A
   black-and-white image is written as JBIG2 without loss (one generic
   region, never symbols matched, which swap characters). Left as it is:
   2, 4 or 16 bits, a stencil mask, a colour key, a matted mask, JPEG
   2000, a CMYK JPEG (its inversion is read two ways), a JPEG whose
   colour transform is set, and other filters. An image no page's drawing
   reaches (an annotation's, a pattern's) is never resized and written
   again only without loss, as the check would not look at it. Streams
   stored twice are kept once, the pages' thumbnails let go of, Deflate
   written at its best (a stream holding more than 1 GiB left as it is),
   the rest saved in object streams. A PDF protected by a password (an
   owner's alone included, which opens without one), or signed, is not
   made smaller (a copy would drop the protection, break the signature),
   nor one carrying rights its maker signed (`UR3`), said apart, nor a
   damaged one (its table of cross-references rebuilt, or an object it
   reaches not read): a copy is written from what was read, and would
   lose what a reader repairs as it shows it.
5. **A PDF's copy is checked** before it is kept
   (`skwir_platform::shrink_job::made`): its first and last pages, and
   each image written anew on the first page showing it, drawn from the
   file and from the copy by the reader the settings choose, at 600
   pixels in gray. A page differing by more than detail lost (an average
   of 6 levels, 3 % of its pixels by more than 64, or a square of 8
   pixels whose average moved by more than 40: resizing and a JPEG's
   blocks keep each square's average, an image gone or turned does not,
   however small beside its page) has its images put back, the copy made
   again under the same rule and checked again, three times at most. A
   copy with another count of pages, or one differing where no image was
   written anew, is not kept, and said. No reader on, the copy is kept
   unchecked. What the helper wrote is opened once, a regular file
   itself and never through a link, and what is kept is read from it: a
   helper that a file took over could leave a link to another file there.
   The helper is stopped when it says nothing of how far it went for 10
   minutes (it tells each page, image and stream it goes through), or
   past 8 hours: how long a file takes is the machine's and the file's
   (73 s for 10 MB of pages drawn, without loss, on the reference
   machine), and a limit by its size, measured on one machine, stopped a
   slower one's work that was going. Its longest silence is one image of
   the largest size the engine reads, a minute or two on a slow machine;
   the margin is a slower one's, and a helper that hangs meanwhile is the
   user's to cancel.
6. **A picture** (JPEG, PNG; not an animated PNG) keeps its format, is
   turned upright as its EXIF said, keeps its colour profile, its EXIF
   (orientation set to 1) and XMP; with location and camera details left
   out, only its date and time stay. Lossless writes a PNG again without
   loss and changes a JPEG's metadata alone, its image data byte for
   byte.
7. **Printing uses it twice.** Pages a reader drew are written as gray
   PNG when they have no colour, JPEG 90 when they have
   (`skwir_shrink::page`); Save as PDF offers "Smaller file" (Screen,
   checked as Reduce Size checks), `print.save_pdf`'s `smaller`.
8. **jpeg-encoder's licence** adds IJG to those accepted (its DCT and
   tables come from libjpeg); the notices carry it.

## Measured

The trial's files again, Skwir's engine from a release build (the
debug build is 10 to 30 times slower), the same checks: every copy passes
qpdf's check and keeps all its text, links, fields, outline and tags.

| File | Lossless | Print | Screen | Small |
| --- | --- | --- | --- | --- |
| photographs, 17.5 MB | 100 % | 26 % (0.992) | 5.7 % (0.976), 2.9 s | 2.0 % (0.937) |
| LaTeX paper, 2.2 MB | 78 % | 78 % | 73 % (0.9996) | 72 % (0.999) |
| scan, 195 pages, 6.4 MB | 80 % | 80 % | 80 % | 64 % (0.991) |
| scan, 239 pages, 45.5 MB | 85 %, 33 s | 85 % | 43 % (0.979), 28 s | 37 % (0.960) |
| 15 pages drawn, 10.3 MB | 94 %, 73 s | 94 % | 35 % (0.987), 12 s | 17 % (0.959) |

(Size against the file, SSIM in brackets.) Against the others at about
the same quality: on the photographs Screen is smaller than Hyper's low (8.2 % at
0.981) and MuPDF's medium (9.5 % at 0.982), Small than Hyper's medium
(3.1 % at 0.931); on the 239-page scan Screen beats MuPDF's harshest
(48 % at 0.949), the scans' JBIG2 saves what Hyper and MuPDF did not
without loss (15 and 20 %, Hyper's 24 % on the smaller one by symbols
matched); on the pages drawn Screen is MuPDF's medium (34 % at 0.994),
Hyper's being smaller (26 %). The form's signed rights left it as it was.

## Consequences

- No new program and no new system library: a few hundred kilobytes in
  the print helper, on every system the helper runs.
- What Hyper does beyond this is left: fonts subset or merged, JPEG 2000
  written, a colour scan split into text and background (MRC), CMYK
  JPEGs. A text-heavy PDF saves less than Hyper's lossless (the paper's
  66 % against 78 %), a PDF of photographs or scans as much or more.
- Lossless over large pictures stored without loss is slow: every row's
  five predictors tried and Deflate at its best, 73 s for 15 pages of
  4096 pixels.
- A PDF's copy costs a second reading of the pages checked; a scan of
  every page an image, every page. A copy whose images were put back
  costs a second pass through the whole file, announced as more work.
- A copy is made in Skwir's cache (`shrink/work`, on a disk), not in a
  temporary folder that may be memory, and the folder goes once it is
  kept or given up.
- The print helper reads a file nobody vouched for in its `--shrink`
  mode, as it reads pictures to print: isolated where the system can.
