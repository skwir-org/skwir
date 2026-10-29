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

/// The picture decoded, shrunk to at most `side` on its longer side, then
/// turned: what a thumbnail needs. Turned before shrinking, a portrait
/// photo of 24 megapixels held a second full copy of itself, 72 MB more
/// for each worker, and the turn cost the full picture's time.
pub fn decode_fitted<R: BufRead + Seek>(
    reader: ImageReader<R>,
    side: u32,
) -> ImageResult<DynamicImage> {
    let (mut image, orientation) = if reader.format() == Some(ImageFormat::Jpeg) {
        let mut inner = reader.into_inner();
        match jpeg_reduced(&mut inner, side)? {
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
/// held. `None`, `reader` back where it was, when no smaller scale fits, or
/// for a JPEG this decoder reads otherwise than `image` (CMYK, 16 bits,
/// arithmetic coding): it is decoded whole.
fn jpeg_reduced<R: BufRead + Seek>(
    reader: &mut R,
    side: u32,
) -> ImageResult<Option<(DynamicImage, Orientation)>> {
    let start = reader
        .stream_position()
        .map_err(image::ImageError::IoError)?;
    let reduced = (|| {
        let mut jpeg = jpeg_decoder::Decoder::new(&mut *reader);
        jpeg.set_max_decoding_buffer_size(REDUCED_BYTES);
        jpeg.read_info().ok()?;
        let full = jpeg.info()?;
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
        let (reduced, orientation) = jpeg_reduced(&mut cursor, 100).unwrap().unwrap();
        assert_eq!((reduced.width(), reduced.height()), (150, 100));
        assert_eq!(orientation, Orientation::Rotate90);
        let fitted = decode_fitted(reader(&bytes), 100).unwrap().to_rgb8();
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
        assert!(jpeg_reduced(&mut cursor, 128).unwrap().is_none());
        assert_eq!(cursor.position(), 0);
        let fitted = decode_fitted(reader(&small), 128).unwrap();
        assert_eq!((fitted.width(), fitted.height()), (2, 3));
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
