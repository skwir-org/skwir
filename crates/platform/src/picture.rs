//! Pictures as they are meant to be seen. A camera held upright stores its
//! pixels as the sensor reads them, sideways, and says in the EXIF
//! Orientation tag how to turn them. Every picture shown, and every size
//! reported for one, goes through here, so a picture never stands one way
//! in its thumbnail and another in the preview.
use std::io::{BufRead, BufReader, Seek};
use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, ImageResult};

/// A reader over the picture file at `path`, its format guessed from the
/// bytes. Opened without blocking on a FIFO (`file::open_regular`).
pub fn open(path: &Path) -> anyhow::Result<ImageReader<BufReader<std::fs::File>>> {
    let file = crate::file::open_regular(path)?;
    Ok(ImageReader::new(BufReader::new(file)).with_guessed_format()?)
}

/// The picture's width and height once turned, read from its header.
pub fn size<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(u32, u32)> {
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    Ok(turned(width, height, orientation(&mut decoder)))
}

/// The picture decoded and turned. The reader's limits bound the decode as
/// they would `ImageReader::decode`.
pub fn decode<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<DynamicImage> {
    let mut decoder = reader.into_decoder()?;
    let orientation = orientation(&mut decoder);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
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
        let pixels = image::RgbImage::from_fn(3, 2, |x, _| {
            if x == 0 {
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
    fn upright_and_half_turned_pictures_keep_their_size() {
        for bytes in [jpeg(1), jpeg(3)] {
            assert_eq!(size(reader(&bytes)).unwrap(), (3, 2));
            assert_eq!(decode(reader(&bytes)).unwrap().width(), 3);
        }
    }
}
