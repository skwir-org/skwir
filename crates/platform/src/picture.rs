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
/// eight bits. The reader's limits bound the decode as they would
/// `ImageReader::decode`.
pub fn decode<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<DynamicImage> {
    let mut decoder = reader.into_decoder()?;
    let orientation = orientation(&mut decoder);
    let mut image = encoded(DynamicImage::from_decoder(decoder)?);
    image.apply_orientation(orientation);
    Ok(image)
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
/// for each worker, and the turn cost the full picture's time.
pub fn decode_fitted<R: BufRead + Seek>(
    reader: ImageReader<R>,
    side: u32,
    threads: Threads,
) -> ImageResult<DynamicImage> {
    let (mut image, orientation) = if reader.format() == Some(ImageFormat::Jpeg) {
        let mut inner = reader.into_inner();
        match jpeg_reduced(&mut inner, side, threads)? {
            Some(reduced) => reduced,
            None => whole(ImageReader::with_format(inner, ImageFormat::Jpeg))?,
        }
    } else {
        whole(reader)?
    };
    if image.width().max(image.height()) > side {
        image = image.thumbnail(side, side);
    }
    image.apply_orientation(orientation);
    Ok(image)
}

/// The picture decoded whole, and how to turn it.
fn whole<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(DynamicImage, Orientation)> {
    let mut decoder = reader.into_decoder()?;
    let orientation = orientation(&mut decoder);
    Ok((encoded(DynamicImage::from_decoder(decoder)?), orientation))
}

/// What a JPEG's reduced decode may hold, as `image`'s own limit.
const REDUCED_BYTES: usize = 512 << 20;

/// A JPEG decoded at the least of its reduced scales (an eighth, a quarter,
/// a half) still at least `side` on its longer side, and how to turn it:
/// a photo eight times the side has an eighth of its pixels computed and
/// held. `None`, `reader` back where it was, when no smaller scale fits (a
/// lossless JPEG has none), or for a JPEG this decoder reads otherwise than
/// `image` (CMYK, 16 bits, arithmetic coding): it is decoded whole.
fn jpeg_reduced<R: BufRead + Seek>(
    reader: &mut R,
    side: u32,
    threads: Threads,
) -> ImageResult<Option<(DynamicImage, Orientation)>> {
    let start = reader
        .stream_position()
        .map_err(image::ImageError::IoError)?;
    let reduced = (|| {
        let mut jpeg = jpeg_decoder::Decoder::new(&mut *reader);
        jpeg.set_max_decoding_buffer_size(REDUCED_BYTES);
        jpeg.set_worker_threads(threads == Threads::Components);
        jpeg.read_info().ok()?;
        let full = jpeg.info()?;
        // Told before a pixel is decoded: one `image` reads otherwise is
        // decoded once, whole.
        if !matches!(
            full.pixel_format,
            jpeg_decoder::PixelFormat::L8 | jpeg_decoder::PixelFormat::RGB24
        ) {
            return None;
        }
        let asked = u16::try_from(side).unwrap_or(u16::MAX);
        let (width, height) = jpeg.scale(asked, asked).ok()?;
        if (width, height) == (full.width, full.height) {
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
        let channels = if full.pixel_format == jpeg_decoder::PixelFormat::L8 {
            1
        } else {
            3
        };
        if pixels.len() as u64 != u64::from(width) * u64::from(height) * channels {
            return None;
        }
        let image = match full.pixel_format {
            jpeg_decoder::PixelFormat::L8 => {
                DynamicImage::ImageLuma8(image::GrayImage::from_raw(width, height, pixels)?)
            }
            jpeg_decoder::PixelFormat::RGB24 => {
                DynamicImage::ImageRgb8(image::RgbImage::from_raw(width, height, pixels)?)
            }
            _ => return None,
        };
        Some((image, orientation))
    })();
    if reduced.is_none() {
        reader
            .seek(std::io::SeekFrom::Start(start))
            .map_err(image::ImageError::IoError)?;
    }
    Ok(reduced)
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
        let image = decode(reader(&bytes)).unwrap().to_rgb8();
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
        let (reduced, orientation) = jpeg_reduced(&mut cursor, 100, Threads::Caller)
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
        let whole = decode(reader(&bytes))
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
            jpeg_reduced(&mut cursor, 128, Threads::Caller)
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
            let (image, _) = jpeg_reduced(&mut Cursor::new(bytes), 100, Threads::Caller)
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
        let whole = decode(reader(PROGRESSIVE))
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
            let (image, _) = jpeg_reduced(&mut Cursor::new(bytes), 100, Threads::Caller)
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
                jpeg_reduced(&mut cursor, 16, Threads::Caller)
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
        let image = decode(reader(&bytes)).unwrap();
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
            assert_eq!(decode(reader(&bytes)).unwrap().width(), 3);
        }
    }
}
