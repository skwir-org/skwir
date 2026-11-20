//! Pictures as they are meant to be seen. A camera held upright stores its
//! pixels as the sensor reads them, sideways, and says in the EXIF
//! Orientation tag how to turn them. Every picture shown, and every size
//! reported for one, goes through here, so a picture never stands one way
//! in its thumbnail and another in the preview. A picture in floating
//! point (`OpenEXR`) holds linear light, which a screen shows too dark unless
//! it is encoded as sRGB first; that happens here too.
use std::io::{BufRead, BufReader, Seek};

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, ImageResult};
use skwir_core::preview::{FileSource, ReadSeek};

/// A reader over the picture `file` (what a preview holds of it, or its
/// path, opened without blocking on a FIFO: `file::open_regular`), its
/// format guessed from the bytes.
pub fn open<S: FileSource + ?Sized>(
    file: &S,
) -> anyhow::Result<ImageReader<BufReader<Box<dyn ReadSeek + Send>>>> {
    Ok(ImageReader::new(BufReader::new(file.open_reader()?)).with_guessed_format()?)
}

/// The picture's width and height once turned, read from its header.
pub fn size<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(u32, u32)> {
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    Ok(turned(width, height, orientation(&mut decoder)))
}

/// The picture's width and height as stored, read from its header, and
/// its EXIF Orientation (1 to 8): what the print helper needs to draw it
/// turned as it is shown, from its own bytes.
pub fn stored<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(u32, u32, u8)> {
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    Ok((width, height, orientation(&mut decoder).to_exif()))
}

/// The picture decoded and turned, a floating-point one encoded as sRGB in
/// eight bits, within `limits` as `ImageReader::decode` would be: its
/// pixels counted too, which `ImageReader::into_decoder` leaves to each
/// decoder (a JPEG's checks only its sides).
pub fn decode<R: BufRead + Seek>(
    mut reader: ImageReader<R>,
    limits: image::Limits,
) -> ImageResult<DynamicImage> {
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder()?;
    let orientation = orientation(&mut decoder);
    let mut image = encoded(within(decoder, limits)?);
    image.apply_orientation(orientation);
    Ok(image)
}

/// The pixels `decoder` decodes, refused if they are more than `limits`
/// allow; what it holds besides within the rest.
fn within(mut decoder: impl ImageDecoder, mut limits: image::Limits) -> ImageResult<DynamicImage> {
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    DynamicImage::from_decoder(decoder)
}

/// Whether a decode may take threads of its own: said by whoever asks for
/// it, which knows who waits for it, never read from the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Threads {
    /// On the asking thread alone: one of many pictures decoded at once,
    /// each on a worker of its own (thumbnails, a comic's pages, a contact
    /// sheet), where a thread a component for each would make three or four
    /// times as many.
    Caller,
    /// A thread a component (a JPEG's, decoded at a reduced scale): the
    /// picture the user waits for, decoded alone (the preview's own, which
    /// take turns).
    Components,
}

/// The picture decoded, shrunk to at most `side` on its longer side, then
/// turned: what a thumbnail needs. Turned before shrinking, a portrait
/// photo of 24 megapixels held a second full copy of itself, 72 MB more
/// for each worker, and the turn cost the full picture's time. Within
/// `image`'s own limits, whatever `reader`'s: a caller with limits of its
/// own decodes through [`jpeg_fitted`] and [`decode`].
pub fn decode_fitted<R: BufRead + Seek>(
    reader: ImageReader<R>,
    side: u32,
    threads: Threads,
) -> ImageResult<DynamicImage> {
    if reader.format() == Some(ImageFormat::Jpeg) {
        let mut inner = reader.into_inner();
        if let Some(image) = jpeg_fitted(&mut inner, side, threads, REDUCED_BYTES)? {
            return Ok(image);
        }
        return Ok(fitted(
            whole(ImageReader::with_format(inner, ImageFormat::Jpeg))?,
            side,
        ));
    }
    Ok(fitted(whole(reader)?, side))
}

/// A JPEG decoded at a reduced scale, shrunk to at most `side` on its
/// longer side and turned, as [`decode_fitted`] decodes it, where that
/// holds at most `bytes` (a progressive JPEG's coefficients at a quarter or
/// a half can be more than its pixels whole). `None`, `reader` back where
/// it was, where it is decoded whole: the caller decodes it, within its
/// own limits.
pub fn jpeg_fitted<R: BufRead + Seek>(
    reader: &mut R,
    side: u32,
    threads: Threads,
    bytes: u64,
) -> ImageResult<Option<DynamicImage>> {
    Ok(jpeg_reduced(reader, side, threads, bytes)?.map(|reduced| fitted(reduced, side)))
}

/// `image` shrunk to at most `side` on its longer side, then turned.
fn fitted((mut image, orientation): (DynamicImage, Orientation), side: u32) -> DynamicImage {
    if image.width().max(image.height()) > side {
        image = image.thumbnail(side, side);
    }
    image.apply_orientation(orientation);
    image
}

/// The picture decoded whole, within `image`'s own limits, and how to turn
/// it.
fn whole<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(DynamicImage, Orientation)> {
    let mut decoder = reader.into_decoder()?;
    let orientation = orientation(&mut decoder);
    let image = within(decoder, image::Limits::default())?;
    Ok((encoded(image), orientation))
}

/// What a JPEG's reduced decode may hold, as `image`'s own limit.
const REDUCED_BYTES: u64 = 512 << 20;

/// A JPEG's decode at a reduced scale, as its header tells it.
struct Reduced {
    width: u16,
    height: u16,
    /// Of each pixel: 1 in grey, else 3.
    channels: u64,
    /// What the decode holds at most, in bytes.
    holds: u64,
}

/// How the JPEG `jpeg` reads is decoded at the least of its reduced scales
/// (an eighth, a quarter, a half) still at least `side` on its longer
/// side, read from its header. `None` when no smaller scale fits (a
/// lossless JPEG has none), or for a JPEG this decoder reads otherwise than
/// `image` (CMYK, 16 bits, arithmetic coding): it is decoded whole.
fn reduced<R: std::io::Read>(jpeg: &mut jpeg_decoder::Decoder<R>, side: u32) -> Option<Reduced> {
    jpeg.read_info().ok()?;
    let full = jpeg.info()?;
    let channels = match full.pixel_format {
        jpeg_decoder::PixelFormat::L8 => 1,
        jpeg_decoder::PixelFormat::RGB24 => 3,
        _ => return None,
    };
    let asked = u16::try_from(side).unwrap_or(u16::MAX);
    let (width, height) = jpeg.scale(asked, asked).ok()?;
    if (width, height) == (full.width, full.height) {
        return None;
    }
    // Its pixels, as each component's plane and as the picture.
    let pixels = u64::from(width) * u64::from(height) * channels * 2;
    // A progressive JPEG keeps coefficients for the whole picture: two
    // bytes a sample of each component (every one at its fullest, as in
    // 4:4:4), or at an eighth one a block, its DC (`vendor/jpeg-decoder`).
    let samples = u64::from(u32::from(full.width).next_multiple_of(16))
        * u64::from(u32::from(full.height).next_multiple_of(16))
        * channels;
    let eighth = (width, height) == (full.width.div_ceil(8), full.height.div_ceil(8));
    let coefficients = match full.coding_process {
        jpeg_decoder::CodingProcess::DctProgressive if eighth => samples / 64 * 2,
        jpeg_decoder::CodingProcess::DctProgressive => samples * 2,
        _ => 0,
    };
    Some(Reduced {
        width,
        height,
        channels,
        holds: pixels + coefficients,
    })
}

/// A JPEG decoded as [`reduced`] says, and how to turn it: a photo eight
/// times the side has an eighth of its pixels computed and held. `None`,
/// `reader` back where it was, where it is decoded whole: no smaller scale,
/// or more held than `bytes`.
fn jpeg_reduced<R: BufRead + Seek>(
    reader: &mut R,
    side: u32,
    threads: Threads,
    bytes: u64,
) -> ImageResult<Option<(DynamicImage, Orientation)>> {
    let start = reader
        .stream_position()
        .map_err(image::ImageError::IoError)?;
    let decoded = (|| {
        let mut jpeg = jpeg_decoder::Decoder::new(&mut *reader);
        jpeg.set_max_decoding_buffer_size(usize::try_from(REDUCED_BYTES).ok()?);
        jpeg.set_worker_threads(threads == Threads::Components);
        // Told before a pixel is decoded: one `image` reads otherwise, or
        // too large, is decoded once, whole.
        let Reduced {
            width,
            height,
            channels,
            holds,
        } = reduced(&mut jpeg, side)?;
        if holds > bytes {
            return None;
        }
        let orientation = jpeg
            .exif_data()
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms);
        let pixels = jpeg.decode().ok()?;
        let (width, height) = (u32::from(width), u32::from(height));
        // The pixels of the size it said, or no picture: `from_raw` takes a
        // buffer longer than it needs, and pixels of another size would be
        // drawn as stripes of the picture's first rows.
        if pixels.len() as u64 != u64::from(width) * u64::from(height) * channels {
            return None;
        }
        let image = if channels == 1 {
            DynamicImage::ImageLuma8(image::GrayImage::from_raw(width, height, pixels)?)
        } else {
            DynamicImage::ImageRgb8(image::RgbImage::from_raw(width, height, pixels)?)
        };
        Some((image, orientation))
    })();
    if decoded.is_none() {
        reader
            .seek(std::io::SeekFrom::Start(start))
            .map_err(image::ImageError::IoError)?;
    }
    Ok(decoded)
}

/// Linear light encoded as sRGB, what a screen expects: an `OpenEXR`
/// picture's floats, clipped at white. Alpha is not light, and stays
/// linear. Any other picture is already encoded.
fn encoded(image: DynamicImage) -> DynamicImage {
    fn srgb(linear: f32) -> u8 {
        let linear = linear.clamp(0., 1.);
        let encoded = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1. / 2.4) - 0.055
        };
        (encoded * 255.).round() as u8
    }
    match image {
        DynamicImage::ImageRgb32F(linear) => DynamicImage::ImageRgb8(image::RgbImage::from_fn(
            linear.width(),
            linear.height(),
            |x, y| image::Rgb(linear.get_pixel(x, y).0.map(srgb)),
        )),
        DynamicImage::ImageRgba32F(linear) => DynamicImage::ImageRgba8(image::RgbaImage::from_fn(
            linear.width(),
            linear.height(),
            |x, y| {
                let [r, g, b, a] = linear.get_pixel(x, y).0;
                image::Rgba([
                    srgb(r),
                    srgb(g),
                    srgb(b),
                    (a.clamp(0., 1.) * 255.).round() as u8,
                ])
            },
        )),
        other => other,
    }
}

/// A tag that cannot be read turns nothing: the pixels are still a picture.
fn orientation(decoder: &mut impl ImageDecoder) -> Orientation {
    decoder.orientation().unwrap_or(Orientation::NoTransforms)
}

/// `width` × `height` after `orientation`: a quarter turn swaps them.
fn turned(width: u32, height: u32, orientation: Orientation) -> (u32, u32) {
    match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        Orientation::NoTransforms
        | Orientation::Rotate180
        | Orientation::FlipHorizontal
        | Orientation::FlipVertical => (width, height),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// A 3 × 2 JPEG, left column red, whose EXIF says `tag`.
    fn jpeg(tag: u16) -> Vec<u8> {
        jpeg_of(3, 2, tag)
    }

    /// A `width` × `height` JPEG, its left third red, the rest blue, whose
    /// EXIF says `tag`.
    fn jpeg_of(width: u32, height: u32, tag: u16) -> Vec<u8> {
        let pixels = image::RgbImage::from_fn(width, height, |x, _| {
            if x < width.div_ceil(3) {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 100)
            .encode_image(&pixels)
            .unwrap();
        // An APP1 segment right after SOI: "Exif\0\0", then a little-endian
        // TIFF header and one directory holding Orientation.
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&tag.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes()); // no next directory
        let mut segment = b"Exif\0\0".to_vec();
        segment.extend_from_slice(&tiff);
        let mut out = plain[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&u16::try_from(segment.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(&segment);
        out.extend_from_slice(&plain[2..]);
        out
    }

    fn reader(bytes: &[u8]) -> ImageReader<Cursor<&[u8]>> {
        ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .unwrap()
    }

    #[test]
    fn a_picture_as_stored_says_how_to_turn_it() {
        assert_eq!(stored(reader(&jpeg(6))).unwrap(), (3, 2, 6));
        assert_eq!(stored(reader(&jpeg(1))).unwrap(), (3, 2, 1));
    }

    #[test]
    fn a_quarter_turn_stands_the_picture_up() {
        let bytes = jpeg(6); // turn 90° clockwise to view
        assert_eq!(size(reader(&bytes)).unwrap(), (2, 3));
        let image = decode(reader(&bytes), image::Limits::default())
            .unwrap()
            .to_rgb8();
        assert_eq!(image.dimensions(), (2, 3));
        // The red left column is now the top row.
        let [r, _, b] = image.get_pixel(1, 0).0;
        assert!(r > 200 && b < 60, "top row should be red");
        let [r, _, b] = image.get_pixel(1, 2).0;
        assert!(r < 60 && b > 200, "bottom row should be blue");
    }

    #[test]
    fn a_large_jpeg_is_decoded_reduced_and_still_turned() {
        // An eighth of 1200 × 800 is 150 × 100: what a side of 100 needs.
        let bytes = jpeg_of(1200, 800, 6);
        let mut cursor = Cursor::new(bytes.as_slice());
        let (reduced, orientation) = jpeg_reduced(&mut cursor, 100, Threads::Caller, REDUCED_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!((reduced.width(), reduced.height()), (150, 100));
        assert_eq!(orientation, Orientation::Rotate90);
        let fitted = decode_fitted(reader(&bytes), 100, Threads::Caller)
            .unwrap()
            .to_rgb8();
        assert_eq!(fitted.dimensions(), (67, 100));
        // The red left third is now the top.
        let [r, _, b] = fitted.get_pixel(33, 5).0;
        assert!(r > 200 && b < 60, "top should be red");
        let [r, _, b] = fitted.get_pixel(33, 95).0;
        assert!(r < 60 && b > 200, "bottom should be blue");
        // As the whole decode shrunk would have it.
        let whole = decode(reader(&bytes), image::Limits::default())
            .unwrap()
            .thumbnail(100, 100)
            .to_rgb8();
        assert_eq!(whole.dimensions(), fitted.dimensions());
        let differ = whole
            .as_raw()
            .iter()
            .zip(fitted.as_raw())
            .filter(|(a, b)| a.abs_diff(**b) > 40)
            .count();
        assert!(differ * 50 < whole.as_raw().len(), "{differ} bytes differ");
        // A small one has no smaller scale: decoded whole, from where it was.
        let small = jpeg(6);
        let mut cursor = Cursor::new(small.as_slice());
        assert!(
            jpeg_reduced(&mut cursor, 128, Threads::Caller, REDUCED_BYTES)
                .unwrap()
                .is_none()
        );
        assert_eq!(cursor.position(), 0);
        let fitted = decode_fitted(reader(&small), 128, Threads::Caller).unwrap();
        assert_eq!((fitted.width(), fitted.height()), (2, 3));
    }

    /// 1200 × 800, its left third red and the rest a ramp of blue,
    /// progressive as libjpeg writes it: its first scan brings every
    /// component's DC coefficients, less their lowest bit.
    const PROGRESSIVE: &[u8] = include_bytes!("../testdata/synthetic-progressive.jpg");

    #[test]
    fn a_progressive_jpeg_at_an_eighth_is_read_to_its_first_scan() {
        // Cut after the first scan: the next one's marker and nothing of it.
        let next = PROGRESSIVE
            .windows(2)
            .enumerate()
            .filter(|(_, w)| *w == [0xFF, 0xDA])
            .nth(1)
            .map(|(at, _)| at)
            .expect("a second scan");
        let cut = &PROGRESSIVE[..next + 2];
        assert!(
            jpeg_decoder::Decoder::new(Cursor::new(cut))
                .decode()
                .is_err(),
            "the cut file is no picture at its full size"
        );
        let reduced = |bytes: &[u8]| {
            let (image, _) =
                jpeg_reduced(&mut Cursor::new(bytes), 100, Threads::Caller, REDUCED_BYTES)
                    .unwrap()
                    .unwrap();
            image.to_rgb8()
        };
        let read = reduced(PROGRESSIVE);
        assert_eq!(read.dimensions(), (150, 100));
        assert_eq!(reduced(cut), read, "nothing after the first scan is read");
        let fitted = decode_fitted(reader(cut), 100, Threads::Caller).unwrap();
        assert_eq!((fitted.width(), fitted.height()), (100, 67));
        // As the whole decode shrunk has it, give or take the bit the first
        // scan leaves out and the filters' edges.
        let whole = decode(reader(PROGRESSIVE), image::Limits::default())
            .unwrap()
            .thumbnail(150, 100)
            .to_rgb8();
        let differ = whole
            .as_raw()
            .iter()
            .zip(read.as_raw())
            .filter(|(a, b)| a.abs_diff(**b) > 40)
            .count();
        assert!(differ * 50 < whole.as_raw().len(), "{differ} bytes differ");
        let [r, _, b] = read.get_pixel(20, 50).0;
        assert!(r > 200 && b < 60, "the left is red");
        let [r, _, b] = read.get_pixel(100, 5).0;
        assert!(r < 60 && b > 200, "the top of the right is blue");
    }

    /// 800 × 600, a plasma (`magick -seed 3 -size 800x600 plasma:fractal
    /// -blur 0x1 -quality 30 -sampling-factor 2x2`), its coefficients
    /// written four ways by libjpeg-turbo's jpegtran, without a loss
    /// (`-copy none`): sequential; progressive as libjpeg writes it, every
    /// DC less its lowest bit first (`-progressive`); and by the scripts
    /// beside the two others (`-scans`).
    const PLASMA: &[u8] = include_bytes!("../testdata/synthetic-plasma-sequential.jpg");
    const PLASMA_PROGRESSIVE: &[u8] =
        include_bytes!("../testdata/synthetic-plasma-progressive.jpg");
    /// `0,1,2: 0-0, 0, 0;`, then libjpeg's AC scans: every DC whole first.
    const PLASMA_DC_WHOLE: &[u8] = include_bytes!("../testdata/synthetic-plasma-dc-whole.jpg");
    /// `0: 0-0, 0, 1; 0: 1-63, 0, 1; 0: 1-63, 1, 0; 0: 0-0, 1, 0; 1: 0-0, 0,
    /// 0; 2: 0-0, 0, 0; 1: 1-63, 0, 0; 2: 1-63, 0, 0;`: the luma's AC,
    /// refined, and its DC's last bit, before the colours' DC.
    const PLASMA_DC_APART: &[u8] = include_bytes!("../testdata/synthetic-plasma-dc-apart.jpg");

    /// A progressive JPEG at an eighth draws what a decoder reading every
    /// scan draws, as a sequential one, where its DC came whole. Where its
    /// first scan left out a bit, a pixel moves a few levels either way, and
    /// none on average: drawn as kept, the DC made it 4 levels darker on
    /// average, 10 at most.
    #[test]
    fn a_progressive_jpeg_at_an_eighth_is_drawn_as_a_sequential_one() {
        let reduced = |bytes: &[u8]| {
            let (image, _) =
                jpeg_reduced(&mut Cursor::new(bytes), 100, Threads::Caller, REDUCED_BYTES)
                    .unwrap()
                    .unwrap();
            image.to_rgb8()
        };
        let sequential = reduced(PLASMA);
        assert_eq!(sequential.dimensions(), (100, 75));
        assert_eq!(reduced(PLASMA_DC_WHOLE), sequential, "a DC whole first");
        assert_eq!(
            reduced(PLASMA_DC_APART),
            sequential,
            "AC scans before the last DC"
        );
        let read = reduced(PLASMA_PROGRESSIVE);
        for channel in 0..3 {
            let differences: Vec<i32> = read
                .pixels()
                .zip(sequential.pixels())
                .map(|(a, b)| i32::from(a.0[channel]) - i32::from(b.0[channel]))
                .collect();
            let most = differences.iter().map(|d| d.abs()).max().unwrap();
            let mean = f64::from(differences.iter().sum::<i32>()) / differences.len() as f64;
            assert!(most <= 6, "channel {channel}: a pixel moved {most} levels");
            assert!(
                mean.abs() < 0.5,
                "channel {channel}: {mean} levels on average"
            );
        }
    }

    /// Whether a decode takes threads of its own changes nothing of what it
    /// draws: at an eighth, and at a half, where the threads start.
    #[test]
    fn threads_draw_the_same_picture() {
        for bytes in [PLASMA, PLASMA_PROGRESSIVE] {
            for side in [100, 300] {
                let fitted = |threads| {
                    decode_fitted(reader(bytes), side, threads)
                        .unwrap()
                        .to_rgb8()
                };
                assert_eq!(fitted(Threads::Caller), fitted(Threads::Components));
            }
        }
    }

    /// The plasma at 400 × 300, encoded again at quality 30 in 4:4:4, 4:2:2
    /// and grey (`cjpeg -sample 1x1`, `-sample 2x1`, `-grayscale`), each
    /// sequential (`jpegtran -optimize`) and progressive with a restart
    /// marker every row of blocks (`-restart 1`, `-scans`): in 4:4:4 by
    /// `PLASMA_DC_APART`'s script, the luma's AC before the colours' DC; in
    /// 4:2:2 every DC whole first, then each component's AC; in grey its DC,
    /// then its AC in two bands.
    const PLASMA_444: &[u8] = include_bytes!("../testdata/synthetic-plasma-444.jpg");
    const PLASMA_444_RESTART: &[u8] =
        include_bytes!("../testdata/synthetic-plasma-444-restart.jpg");
    const PLASMA_422: &[u8] = include_bytes!("../testdata/synthetic-plasma-422.jpg");
    const PLASMA_422_RESTART: &[u8] =
        include_bytes!("../testdata/synthetic-plasma-422-restart.jpg");
    const PLASMA_GREY: &[u8] = include_bytes!("../testdata/synthetic-plasma-grey.jpg");
    const PLASMA_GREY_RESTART: &[u8] =
        include_bytes!("../testdata/synthetic-plasma-grey-restart.jpg");

    /// At an eighth, a progressive JPEG whose DC comes whole draws exactly
    /// its sequential one in every layout: 4:4:4, 4:2:2 and grey, its AC
    /// scans passed over across their restart markers.
    #[test]
    fn every_layout_at_an_eighth_is_drawn_as_its_sequential_one() {
        let reduced = |bytes: &[u8]| {
            let mut cursor = Cursor::new(bytes);
            let (image, _) = jpeg_reduced(&mut cursor, 50, Threads::Caller, REDUCED_BYTES)
                .unwrap()
                .unwrap();
            image
        };
        for (layout, sequential, progressive) in [
            ("4:4:4", PLASMA_444, PLASMA_444_RESTART),
            ("4:2:2", PLASMA_422, PLASMA_422_RESTART),
            ("grey", PLASMA_GREY, PLASMA_GREY_RESTART),
        ] {
            let drawn = reduced(sequential);
            assert_eq!((drawn.width(), drawn.height()), (50, 38), "{layout}");
            assert_eq!(drawn, reduced(progressive), "{layout}");
        }
        assert!(reduced(PLASMA_GREY).as_luma8().is_some(), "grey stays grey");
    }

    /// A reduced decode holds what its caller allows, or the picture is
    /// decoded whole: a progressive JPEG at a half keeps two bytes a sample
    /// of each component for the whole picture (800 × 608 × 3 × 2, 2.9 MB,
    /// with 0.7 MB of pixels), at an eighth one coefficient a block; a
    /// sequential one only its pixels.
    #[test]
    fn a_reduced_decode_holds_at_most_what_its_caller_allows() {
        const MOST: u64 = 1 << 20;
        let fitted = |bytes: &[u8], side| {
            let mut cursor = Cursor::new(bytes);
            let image = jpeg_fitted(&mut cursor, side, Threads::Caller, MOST).unwrap();
            if image.is_none() {
                assert_eq!(cursor.position(), 0, "decoded whole from where it was");
            }
            image.map(|image| (image.width(), image.height()))
        };
        assert_eq!(
            fitted(PLASMA, 300),
            Some((300, 225)),
            "sequential at a half"
        );
        assert_eq!(
            fitted(PLASMA_PROGRESSIVE, 100),
            Some((100, 75)),
            "at an eighth"
        );
        assert_eq!(
            fitted(PLASMA_PROGRESSIVE, 300),
            None,
            "progressive at a half"
        );
        let mut cursor = Cursor::new(PLASMA_PROGRESSIVE);
        let image = jpeg_fitted(&mut cursor, 300, Threads::Caller, 4 << 20).unwrap();
        assert_eq!(image.map(|i| (i.width(), i.height())), Some((300, 225)));
    }

    /// 64 × 48, the plasma in eight bits, lossless (`cjpeg -lossless 1`), in
    /// RGB (`-rgb`) and in grey.
    const LOSSLESS_RGB: &[u8] = include_bytes!("../testdata/synthetic-lossless-rgb.jpg");
    const LOSSLESS_GREY: &[u8] = include_bytes!("../testdata/synthetic-lossless-grey.jpg");

    /// A lossless JPEG has no reduced scale, and is never decoded at one:
    /// the decoder drew its first rows at the smaller size, the desktop's
    /// cache kept them, and grey ones came with every pixel of the whole.
    #[test]
    fn a_lossless_jpeg_is_never_decoded_reduced() {
        for bytes in [LOSSLESS_RGB, LOSSLESS_GREY] {
            let mut jpeg = jpeg_decoder::Decoder::new(Cursor::new(bytes));
            assert_eq!(jpeg.scale(16, 16).unwrap(), (64, 48), "its full size");
            let mut cursor = Cursor::new(bytes);
            assert!(
                jpeg_reduced(&mut cursor, 16, Threads::Caller, REDUCED_BYTES)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(cursor.position(), 0, "decoded whole from where it was");
            // What the whole decode makes of it, if anything (zune-jpeg
            // reads no lossless JPEG): the picture, shrunk.
            if let Ok(fitted) = decode_fitted(reader(bytes), 16, Threads::Caller) {
                let whole = jpeg_decoder::Decoder::new(Cursor::new(bytes))
                    .decode()
                    .unwrap();
                let whole = if whole.len() == 64 * 48 {
                    DynamicImage::ImageLuma8(image::GrayImage::from_raw(64, 48, whole).unwrap())
                } else {
                    DynamicImage::ImageRgb8(image::RgbImage::from_raw(64, 48, whole).unwrap())
                };
                assert_eq!(fitted.to_rgb8(), whole.thumbnail(16, 16).to_rgb8());
            }
        }
    }

    #[test]
    fn linear_light_is_encoded_as_srgb() {
        // Middle grey in linear light, and a highlight past white.
        let linear = image::Rgb32FImage::from_fn(2, 1, |x, _| {
            image::Rgb(if x == 0 {
                [0.214, 0.214, 0.214]
            } else {
                [4., 1., 0.]
            })
        });
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb32F(linear)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::OpenExr)
            .unwrap();
        let image = decode(reader(&bytes), image::Limits::default()).unwrap();
        let image = image.as_rgb8().expect("eight bits, not floats");
        // Half of white on screen, within the file's half-float precision.
        let [grey, ..] = image.get_pixel(0, 0).0;
        assert!((127..=128).contains(&grey), "{grey}");
        assert_eq!(image.get_pixel(1, 0).0, [255, 255, 0]);
    }

    #[test]
    fn upright_and_half_turned_pictures_keep_their_size() {
        for bytes in [jpeg(1), jpeg(3)] {
            assert_eq!(size(reader(&bytes)).unwrap(), (3, 2));
            assert_eq!(
                decode(reader(&bytes), image::Limits::default())
                    .unwrap()
                    .width(),
                3
            );
        }
    }
}
