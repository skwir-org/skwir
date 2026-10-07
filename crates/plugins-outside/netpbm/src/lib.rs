//! A thumbnailer from outside bibli (ADR-045, ADR-046): a binary Netpbm
//! picture (`P6`, colour; `P5`, grey; at most 255 a channel) read through
//! bibli, drawn to fit the size asked (nearest pixel), and answered as a
//! PNG this module writes itself: a module has no libraries but its own.

use bibli_plugin_api::guest::{Host, Plugin};
use bibli_plugin_api::protocol::ThumbnailParams;

/// The largest picture read: its header and its pixels.
const LIMIT: u64 = 64 * 1024 * 1024;
const PAGE: u32 = 1024 * 1024;

#[derive(Default)]
pub struct Netpbm;

impl Plugin for Netpbm {
    fn thumbnail(
        &self,
        host: &dyn Host,
        params: ThumbnailParams,
    ) -> Result<Option<Vec<u8>>, String> {
        let mut bytes = Vec::new();
        while (bytes.len() as u64) < LIMIT {
            let page = host.read(params.file.handle, bytes.len() as u64, PAGE)?;
            if page.is_empty() {
                break;
            }
            bytes.extend(page);
        }
        let Some(picture) = Picture::parse(&bytes) else {
            return Ok(None);
        };
        Ok(Some(png(&picture.fit(params.size))))
    }
}

/// Pixels in rows, three bytes each.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

impl Picture {
    /// A `P6` or `P5` picture, its header's comments skipped; `None` for
    /// anything else.
    pub fn parse(bytes: &[u8]) -> Option<Picture> {
        let mut at = 0;
        let mut field = || -> Option<&[u8]> {
            loop {
                while bytes.get(at)?.is_ascii_whitespace() {
                    at += 1;
                }
                if bytes.get(at) == Some(&b'#') {
                    while *bytes.get(at)? != b'\n' {
                        at += 1;
                    }
                    continue;
                }
                let start = at;
                while !bytes.get(at)?.is_ascii_whitespace() {
                    at += 1;
                }
                return Some(&bytes[start..at]);
            }
        };
        let channels = match field()? {
            b"P6" => 3,
            b"P5" => 1,
            _ => return None,
        };
        let mut number = || -> Option<usize> { std::str::from_utf8(field()?).ok()?.parse().ok() };
        let (width, height, max) = (number()?, number()?, number()?);
        if width == 0 || height == 0 || max == 0 || max > 255 {
            return None;
        }
        // One white space after the maximum, then the pixels.
        let pixels =
            bytes.get(at + 1..at + 1 + width.checked_mul(height)?.checked_mul(channels)?)?;
        let rgb = if channels == 3 {
            pixels.to_vec()
        } else {
            pixels.iter().flat_map(|g| [*g, *g, *g]).collect()
        };
        Some(Picture { width, height, rgb })
    }

    /// The picture fit in a square of `side`, the nearest pixel taken;
    /// one already smaller is kept.
    pub fn fit(&self, side: u32) -> Picture {
        let side = side.max(1) as usize;
        let scale = (self.width.max(self.height) as f64 / side as f64).max(1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Sides of a picture.
        let (width, height) = (
            ((self.width as f64 / scale) as usize).max(1),
            ((self.height as f64 / scale) as usize).max(1),
        );
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            let from_y = y * self.height / height;
            for x in 0..width {
                let from = (from_y * self.width + x * self.width / width) * 3;
                rgb.extend_from_slice(&self.rgb[from..from + 3]);
            }
        }
        Picture { width, height, rgb }
    }
}

/// `picture` as a PNG: 8-bit RGB, its rows unfiltered, kept in stored
/// deflate blocks.
pub fn png(picture: &Picture) -> Vec<u8> {
    let mut raw = Vec::with_capacity(picture.height * (picture.width * 3 + 1));
    for row in picture.rgb.chunks(picture.width * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(i + 1 == blocks.len()));
        #[allow(clippy::cast_possible_truncation)] // A block is at most 65 535 bytes.
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut header = Vec::with_capacity(13);
    #[allow(clippy::cast_possible_truncation)] // A thumbnail's side.
    {
        header.extend_from_slice(&(picture.width as u32).to_be_bytes());
        header.extend_from_slice(&(picture.height as u32).to_be_bytes());
    }
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [
        (b"IHDR", &header[..]),
        (b"IDAT", &zlib[..]),
        (b"IEND", &[][..]),
    ] {
        #[allow(clippy::cast_possible_truncation)] // A chunk of a thumbnail.
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = png.len();
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        let crc = crc32(&png[start..]);
        png.extend_from_slice(&crc.to_be_bytes());
    }
    png
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    b << 16 | a
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

bibli_plugin_api::export!(Netpbm);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_or_grey_picture_is_read_and_fit() {
        let mut ppm = b"P6\n# made here\n4 2\n255\n".to_vec();
        ppm.extend(std::iter::repeat_n([255u8, 0, 0], 8).flatten());
        let picture = Picture::parse(&ppm).unwrap();
        assert_eq!((picture.width, picture.height), (4, 2));
        let small = picture.fit(2);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(&small.rgb[..3], &[255, 0, 0]);
        let mut pgm = b"P5 2 2 255 ".to_vec();
        pgm.extend([0, 64, 128, 255]);
        let grey = Picture::parse(&pgm).unwrap();
        assert_eq!(&grey.rgb[9..], &[255, 255, 255]);
        assert!(
            Picture::parse(b"P3 1 1 255 0 0 0").is_none(),
            "text pixels are not read"
        );
        assert!(Picture::parse(b"P6 2 2 255 ").is_none(), "pixels missing");
    }

    #[test]
    fn a_png_is_signed_and_its_checksums_are_the_known_ones() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        let png = png(&Picture {
            width: 1,
            height: 1,
            rgb: vec![1, 2, 3],
        });
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png.ends_with(&[0xAE, 0x42, 0x60, 0x82]));
    }
}
