//! A page a document's reader drew, to be printed (ADR-111: a `DjVu`, a
//! comic, a book with no PDF behind it), written as small as it reads
//! (ADR-112): in gray when it has no colour (a scan, a page of text), as
//! JPEG when it has (a comic, a magazine). The print helper is handed it
//! as the reader drew it, four bytes a pixel at about 350 dpi, which
//! written as it is weighs megabytes a page.

use anyhow::{Context, Result};

use crate::samples::{self, Samples};

/// The JPEG quality a coloured page is written at: what reads as the page
/// on paper.
const QUALITY: u8 = 90;

/// How far apart a gray pixel's components may be: a reader's
/// anti-aliasing leaves one or two levels between them.
const GRAY: u8 = 3;

/// A drawn page, written.
#[derive(Debug)]
pub enum Drawn {
    Jpeg(Vec<u8>),
    Png(Vec<u8>),
}

/// The page whose picture `png` holds (any PNG the image crate reads,
/// transparency drawn over white), written in gray as PNG when it has no
/// colour, else as JPEG.
pub fn drawn(png: &[u8]) -> Result<Drawn> {
    let picture = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .context("the page drawn could not be read")?
        .into_rgba8();
    let (width, height) = picture.dimensions();
    let rgb: Vec<u8> = picture
        .pixels()
        .flat_map(|p| {
            let [r, g, b, a] = p.0;
            let over = |c: u8| {
                let c = (u32::from(c) * u32::from(a) + 255 * (255 - u32::from(a))) / 255;
                u8::try_from(c).unwrap_or(u8::MAX)
            };
            [over(r), over(g), over(b)]
        })
        .collect();
    let samples = Samples::new(width, height, 3, rgb)?;
    if samples.looks_gray(GRAY) {
        let gray = samples.gray();
        let mut out = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new_with_quality(
            &mut out,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Adaptive,
        );
        image::ImageEncoder::write_image(
            encoder,
            &gray.data,
            width,
            height,
            image::ExtendedColorType::L8,
        )
        .context("the page could not be written")?;
        return Ok(Drawn::Png(out));
    }
    Ok(Drawn::Jpeg(samples::jpeg(
        &samples,
        QUALITY,
        &samples::JpegExtras::default(),
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let picture = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(pixel(x, y)));
        let mut out = std::io::Cursor::new(Vec::new());
        picture.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_gray_page_stays_png_and_a_coloured_one_is_jpeg() {
        let text = png(400, 600, |x, y| {
            let ink = if (x / 7 + y / 11) % 5 == 0 { 20 } else { 250 };
            [ink, ink, ink, 255]
        });
        let Drawn::Png(gray) = drawn(&text).unwrap() else {
            panic!("a page of text is gray");
        };
        assert!(gray.len() < text.len());
        let back = image::load_from_memory(&gray).unwrap();
        assert_eq!(back.color(), image::ColorType::L8);
        let comic = png(400, 600, |x, y| {
            [
                u8::try_from(x % 256).unwrap(),
                u8::try_from(y % 256).unwrap(),
                u8::try_from((x * y) % 256).unwrap(),
                255,
            ]
        });
        let Drawn::Jpeg(jpeg) = drawn(&comic).unwrap() else {
            panic!("a page in colour is JPEG");
        };
        assert!(jpeg.len() < comic.len());
    }
}
