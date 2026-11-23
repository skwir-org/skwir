# ADR-146: A picture is held at the pixels it covers

Status: accepted, 2026-11-21, on the user's answers (asked why stepping
through a folder of JPEGs is slower than in Gwenview, and why a picture
about 2000 × 11 000 stays blurred when zoomed in the preview; chose
today's memory as the bound, counted in pixels; the region's tiles next;
two pictures kept; a picture a tenth short shown at once, then
sharpened; `fast_image_resize`). Decisions 1 to 4 built on Linux the
same day, 5 to 7 that night (2026-11-22); each checked in a window on
Xvfb. Amended the next night (2026-11-22), as the user chose for its
leftovers: a JPEG's region decoded from its rows alone (5), what a zoom
asks in a turn of its own (8), a picture read ahead keeping its eighth
(6). Amended by the review before 0.6.1 (2026-11-23): the room is the
one laid out for the picture's own preview (1); the region drawn round,
never over, a picture that lets its background through, and let go once
zoomed out (5).

## Context

Measured in release on synthetic photos of about three bits a pixel
(`docs/STATUS-LOG.md`, 2026-11-21), the load 1.4 to 3.6:

- **The panel decoded every picture at 2048 pixels or more** on its
  longer side (`IMAGE_MAX_SIDE`), whatever its size. A phone's 12
  megapixels (4000 or 4032 wide) then missed a JPEG's half scale by a
  few pixels: decoded whole (65 ms) and shrunk from 12 megapixels
  (99 ms), 176 ms in all; 24 megapixels 195 ms (113 at a half, 71 to
  shrink). On a 4K screen a 24-megapixel photo took 457 ms (whole 135,
  shrunk 290, made RGBA 28).
- **Shrinking cost more than decoding.** `image`'s `thumbnail` took
  71 to 306 ms; `fast_image_resize` (Lanczos3) 7 to 62 ms for the same,
  and it weighs each pixel by its alpha, where `thumbnail` averaged a
  transparent pixel's colour into its neighbours.
- **Gwenview** reads the file whole, has libjpeg-turbo decode it at a
  power of two through `QImageReader::setScaledSize` (whole above a zoom
  of 0.25) and lets Qt scale as it paints; it preloads the next file
  forward and keeps three decoded documents. libjpeg-turbo is 1.2 to 1.4
  times as fast as jpeg-decoder at a reduced scale, where decoding the
  entropy dominates: the gap was not the decoder's.
- **A side bounded what was held**: 4096 (`limits::PICTURE_SIDE`), for
  memory and because gpui drew an image as one texture, at most as large
  as the device allows. A tall picture of 2000 × 11 500 was held at
  712 × 4096, 36 % of its width, blurred already at Fit width, and a
  zoom asked no more (`preview::sharper`).
- **A 2048 picture drawn in a 600 px panel** was shrunk more than three
  times by the GPU, which has no mipmaps there: fine detail shimmered.

## Decision

1. **One rule wherever a picture is shown** (the panel, peek, the whole
   screen, zoomed): it is held at the device pixels it covers, in both
   directions, never more than its own. Fitted, that is what its room
   gives it (`preview::Room`: the content's area as laid out for its
   own preview, in the place it is shown, measured with the window's
   scale; `Room::side`, as Fit lays it out, never past 100 %), worked
   out on the worker once its header says its size (`Cover::Fit`). The
   job waits for that frame (`Area::room_for`, 200 ms at most, as a
   first page does): what the area held before is no measure of it (the
   line saying why the last file was not shown made a photo stepped to
   from it decode at 80 pixels, then again as a zoom), nor is the
   panel's once the whole screen or peek shows the preview
   (`Area::moved`). Zoomed or in a larger room, the side the zoom
   asks (`Cover::Side`, `Sharpen`, as before). It is decoded at the
   cheapest decoder scale covering that (a JPEG's eighth, quarter, half
   or whole, `picture::decode_fitted`), then shrunk to it once. A
   drawing (SVG) is drawn at that side, past its own pixels when zoomed.
   `IMAGE_MAX_SIDE` is only what a picture is asked at while no room
   was ever laid out. What is read ahead is fitted in the same room,
   waited for the same way: the screen's on the whole screen, laid out
   there first, the panel's in the panel (`screen_side` is gone).
2. **What is held of a picture is bounded by its pixels, not by a side**
   (`limits::PICTURE_HELD`, 4096 × 4096, 64 MiB as RGBA: the memory the
   side allowed a square picture). A tall or wide picture is held at as
   many pixels as a square one (`picture::side_within`): the
   2000 × 11 500 picture at 1708 × 9821, 85 % of its own. The user's
   choice: no more memory than before; past the bound, decision 5.
3. **A picture is shrunk by `fast_image_resize`** (Lanczos3, weighted by
   alpha, the processor's vector instructions chosen as it runs):
   `picture::shrink`, for every picture Skwir decodes (the preview,
   thumbnails, a comic's pages, contact sheets, what a thumbnailer drew).
   An RGB picture is made BGRA in one pass (`thumbs::picture_image`).
4. **An image larger than the GPU's largest texture is drawn in pieces**
   (vendored gpui 0090): each piece holds the pixel beside its own
   where another lies, so no seam shows, and only the pieces that show
   are uploaded. Every image gains, not only pictures.
5. **Past the bound, only what shows is held at the side asked**: the
   part in view and a margin round it of half as much each way, as many
   pixels as the bound allows, decoded at the side it covers and drawn
   over the whole held within the bound (`preview::Detail`,
   `region_past_held`, `picture::decode_region`); over a picture that
   lets what is behind it through, the whole is drawn round it, never
   under it (amended, 2026-11-23: it was drawn twice over there). One
   region at a time; one within it at no larger side is not asked again;
   it goes once the picture is shown no larger than what is held of it
   whole (amended, 2026-11-23). Its decode reads
   the picture at the least reduced scale covering the side, then cuts,
   turns and shrinks the region alone. A JPEG reads only the rows the
   region covers as stored (`picture::jpeg_band`, `vendor/jpeg-decoder`'s
   0006): those above are entropy-decoded and never transformed, those
   below never read, so a region at the top of a 45-megapixel photo holds
   its rows, never the 134 MB of the whole. Any other picture is decoded
   whole for it.
6. **Something sooner**: a progressive JPEG shows its eighth first (its
   DC scans, 9 to 17 ms for 24 to 45 megapixels), as the stand-in a
   thumbnail is; while stepping, a picture whose cheaper scale is at most
   a tenth short of its room's side is decoded at that scale
   (`Cover::Quick`), and at its side once the walk has rested on it
   600 ms (`settle_picture`). A picture read ahead keeps its eighth too
   (`AheadSlot::Decoding`): a step that takes it while it decodes shows
   the eighth at once, or as it comes.
7. **Read ahead and kept**: the next picture is read ahead from the first
   picture shown, the way the last step went (forward before any), not
   after two steps alike; the last two pictures shown fitted in their
   room are kept as shown, let go after a minute unseen, so a step back
   shows at once.
8. **A zoom never holds a step back**: what a zoom asks, the picture
   again at the zoom's side and a region past the bound, decodes in a
   turn of its own (`preview::Turn::Zoomed`), beside the turn the
   preview's next picture waits for and the one read ahead: a decode
   started cannot be stopped, and a step taken while a large picture
   decodes for a zoom drew the next picture only once it ended (2.5 s
   in a debug build). At most three pictures decode at once.

## Alternatives

- **A larger side** (8192): a square picture would hold four times the
  memory, and a strip would still be cut.
- **A larger bound in pixels** (32 megapixels, 128 MiB, held twice with
  the GPU's copy): the user kept today's memory; decision 5 makes every
  zoom sharp with less.
- **libjpeg-turbo**: a C library for 1.2 to 1.4 times, on three systems
  built from Linux. **zune-jpeg**: as fast as libjpeg-turbo whole, but
  it decodes no reduced scale (`vendor/jpeg-decoder/README.skwir.md`).
- **Mipmaps on the GPU**: gpui's atlas has none; a picture held at its
  covered size needs none.

## Consequences

- A small panel holds and uploads less: a 24-megapixel photo in a room
  1200 pixels wide is a quarter-scale decode shrunk to 1200, not a half
  shrunk to 2048.
- A picture is decoded for the room laid out for it: a panel made larger
  afterwards, or the whole screen entered, asks for more as a zoom does,
  in place. A room made smaller keeps the larger picture, shrunk by the
  GPU.
- A tall picture may need a texture longer than 4096: gpui draws it in
  pieces where the device's limit is shorter.
- Comic and PDF pages keep their side (`document::MAX_SIDE`), and HEIC,
  AVIF, JPEG XL and raw files their thumbnailer's 1024 pixels, until
  the follow-ups in PLAN's "What is left".
