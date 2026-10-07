//! Netpbm pictures from outside bibli (ADR-045, ADR-046, ADR-047): a
//! binary one (`P6`, colour; `P5`, grey; at most 255 a channel) read
//! through bibli, drawn to fit the size asked (nearest pixel) for a
//! thumbnail, and converted whole for the preview, which bibli shows as
//! any picture; both a PNG this module writes itself: a module has no
//! libraries but its own.

use bibli_plugin_api::guest::{Host, Plugin};
use bibli_plugin_api::protocol::{ConvertParams, ThumbnailParams};

/// The largest picture read: its header and its pixels.
const LIMIT: u64 = 64 * 1024 * 1024;
const PAGE: u32 = 1024 * 1024;
/// The longest side of a picture previewed: its PNG (stored, not
/// compressed) stays within what a message holds.
const PREVIEW_SIDE: u32 = 3600;

#[derive(Default)]
pub struct Netpbm;

impl Plugin for Netpbm {
    fn thumbnail(
        &self,
        host: &dyn Host,
        params: ThumbnailParams,
    ) -> Result<Option<Vec<u8>>, String> {
        let Some(picture) = Picture::parse(&read(host, params.file.handle)?) else {
            return Ok(None);
        };
        Ok(Some(png(&picture.fit(params.size))))
    }

    fn convert(
        &self,
        host: &dyn Host,
        params: ConvertParams,
    ) -> Result<Option<(String, Vec<u8>)>, String> {
        let Some(picture) = Picture::parse(&read(host, params.file.handle)?) else {
            return Ok(None);
        };
        let stem = params
            .file
            .name
            .rsplit_once('.')
            .map_or(params.file.name.as_str(), |(stem, _)| stem);
        // Whole, unless its PNG would not fit a message.
        Ok(Some((
            format!("{stem}.png"),
            png(&picture.fit(PREVIEW_SIDE)),
        )))
    }
}

/// A file of the call, whole, up to [`LIMIT`].
fn read(host: &dyn Host, handle: u32) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    while (bytes.len() as u64) < LIMIT {
        let page = host.read(handle, bytes.len() as u64, PAGE)?;
        if page.is_empty() {
            break;
        }
        bytes.extend(page);
    }
    Ok(bytes)
}

/// Pixels in rows, three bytes each.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

impl Picture {
    /// A Netpbm picture, whichever of its six kinds (`P1` to `P6`: a
    /// bitmap, grey or colour, as text or bytes, samples up to 16 bits
    /// made 8), its comments skipped; `None` for anything else or a
    /// raster cut short.
    pub fn parse(bytes: &[u8]) -> Option<Picture> {
        let mut at = 0;
        let (text, channels, bitmap) = match field(bytes, &mut at)? {
            b"P1" => (true, 1, true),
            b"P2" => (true, 1, false),
            b"P3" => (true, 3, false),
            b"P4" => (false, 1, true),
            b"P5" => (false, 1, false),
            b"P6" => (false, 3, false),
            _ => return None,
        };
        let width = number(bytes, &mut at)?;
        let height = number(bytes, &mut at)?;
        let max = if bitmap { 1 } else { number(bytes, &mut at)? };
        if width == 0 || height == 0 || max == 0 || max > 65_535 {
            return None;
        }
        let pixels = width.checked_mul(height)?;
        let samples = pixels.checked_mul(channels)?;
        #[allow(clippy::cast_possible_truncation)] // At most 255.
        let eight = |sample: usize| (sample.min(max) * 255 / max) as u8;
        // Grown as the samples are read, never to what a header claims.
        let mut values = Vec::new();
        // One white space after the header, then the raster's bytes.
        let raster = at + 1;
        match (text, bitmap) {
            // Digits, white space between them or not; 1 is black.
            (true, true) => {
                while values.len() < pixels {
                    match bytes.get(at)? {
                        b'0' => values.push(255),
                        b'1' => values.push(0),
                        // A comment, to the end of its line.
                        b'#' => {
                            while *bytes.get(at)? != b'\n' {
                                at += 1;
                            }
                            continue;
                        }
                        space if space.is_ascii_whitespace() => {}
                        _ => return None,
                    }
                    at += 1;
                }
            }
            (true, false) => {
                while values.len() < samples {
                    values.push(eight(number(bytes, &mut at)?));
                }
            }
            // Rows of bits, each row whole bytes; 1 is black.
            (false, true) => {
                let row = width.div_ceil(8);
                let bits = bytes.get(raster..raster + row.checked_mul(height)?)?;
                for y in 0..height {
                    for x in 0..width {
                        let bit = (bits[y * row + x / 8] >> (7 - x % 8)) & 1;
                        values.push(if bit == 1 { 0 } else { 255 });
                    }
                }
            }
            (false, false) if max < 256 => {
                let raster = bytes.get(raster..raster + samples)?;
                values.extend(raster.iter().map(|sample| eight(usize::from(*sample))));
            }
            (false, false) => {
                let raster = bytes.get(raster..raster + samples.checked_mul(2)?)?;
                values.extend(
                    raster
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|two| eight(usize::from(u16::from_be_bytes(*two)))),
                );
            }
        }
        let rgb = if channels == 3 {
            values
        } else {
            values.iter().flat_map(|g| [*g, *g, *g]).collect()
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

/// The field at `at` (a word of the header, a sample written as text),
/// white space and comments before it skipped; `at` left after it.
fn field<'a>(bytes: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    loop {
        while bytes.get(*at)?.is_ascii_whitespace() {
            *at += 1;
        }
        if bytes.get(*at) == Some(&b'#') {
            while *bytes.get(*at)? != b'\n' {
                *at += 1;
            }
            continue;
        }
        let start = *at;
        while bytes.get(*at).is_some_and(|b| !b.is_ascii_whitespace()) {
            *at += 1;
        }
        return Some(&bytes[start..*at]);
    }
}

/// The number at `at`, written as text.
fn number(bytes: &[u8], at: &mut usize) -> Option<usize> {
    std::str::from_utf8(field(bytes, at)?).ok()?.parse().ok()
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
        assert!(Picture::parse(b"P6 2 2 255 ").is_none(), "pixels missing");
        assert!(Picture::parse(b"P7 1 1 255 x").is_none());
    }

    #[test]
    fn every_kind_of_netpbm_picture_is_read() {
        let colours = |picture: Picture| -> Vec<[u8; 3]> {
            picture.rgb.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
        };
        let (black, white) = ([0, 0, 0], [255, 255, 255]);
        // Text: a bitmap whose digits touch, grey, colour of 15 a channel.
        let p1 = Picture::parse(b"P1\n# a comment\n3 1\n01# and one 1111\n0").unwrap();
        assert_eq!(colours(p1), [white, black, white]);
        let p2 = Picture::parse(b"P2 2 1 4\n0 4").unwrap();
        assert_eq!(colours(p2), [black, white]);
        let p3 = Picture::parse(b"P3 1 1 15 15 0 5").unwrap();
        assert_eq!(colours(p3), [[255, 0, 85]]);
        // Bytes: a bitmap of 10 by 1 (two bytes a row), 16-bit samples.
        let p4 = Picture::parse(b"P4 10 1 \x80\x40").unwrap();
        let p4 = colours(p4);
        assert_eq!((p4[0], p4[1], p4[9]), (black, white, black));
        let p5 = Picture::parse(b"P5 2 1 65535 \x00\x00\xff\xff").unwrap();
        assert_eq!(colours(p5), [black, white]);
        assert!(Picture::parse(b"P2 2 1 4\n0").is_none(), "a sample missing");
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
