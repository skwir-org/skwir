//! An image's samples, 8 bits each, as stored, and what is done to them:
//! resized (never enlarged), made gray, told flat (a screenshot, a
//! drawing) from a photograph, and written as JPEG or as Deflate. Resizing
//! works on samples, whatever colour space they are in: a PDF's image is
//! written back in its own (ADR-112), a sample a linear measure of it.

use anyhow::{Context, Result, bail, ensure};
use image::imageops::FilterType;
use std::io::Write;

/// Pixels row by row, `components` bytes each (1 gray, 3 RGB or any
/// three-component space, 4 CMYK), no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Samples {
    pub width: u32,
    pub height: u32,
    pub components: u8,
    pub data: Vec<u8>,
}

impl Samples {
    /// Samples of `data`, refused when their count is not its size.
    pub fn new(width: u32, height: u32, components: u8, data: Vec<u8>) -> Result<Samples> {
        ensure!(
            matches!(components, 1 | 3 | 4),
            "{components} components a pixel"
        );
        let expected = u64::from(width) * u64::from(height) * u64::from(components);
        ensure!(
            data.len() as u64 == expected,
            "{} bytes for {width}×{height}×{components}",
            data.len()
        );
        Ok(Samples {
            width,
            height,
            components,
            data,
        })
    }

    /// The size it is brought to so its longer side is at most `longest`:
    /// its own when it already is.
    pub fn fitted_size(&self, longest: u32) -> (u32, u32) {
        fitted(self.width, self.height, longest)
    }

    /// Resized to `width` × `height`, each component alone (a filter whose
    /// reach grows as the image shrinks, so every source pixel counts).
    /// Itself when the size is its own.
    pub fn resized(&self, width: u32, height: u32) -> Result<Samples> {
        if (width, height) == (self.width, self.height) {
            return Ok(self.clone());
        }
        ensure!(width > 0 && height > 0, "an empty size");
        let filter = FilterType::Triangle;
        let data = match self.components {
            1 => image::imageops::resize(&self.luma()?, width, height, filter).into_raw(),
            3 => image::imageops::resize(&self.rgb()?, width, height, filter).into_raw(),
            4 => image::imageops::resize(&self.rgba()?, width, height, filter).into_raw(),
            n => bail!("{n} components a pixel"),
        };
        Samples::new(width, height, self.components, data)
    }

    /// Gray, from RGB by its luma (Rec. 601) or from CMYK by its ink;
    /// itself when gray already.
    pub fn gray(&self) -> Samples {
        let data = match self.components {
            3 => self
                .data
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| luma(p[0], p[1], p[2]))
                .collect(),
            4 => self
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| {
                    let ink = |c: u8| 255 - u16::from(c);
                    let (r, g, b) = (ink(p[0]), ink(p[1]), ink(p[2]));
                    let k = 255 - u16::from(p[3]);
                    let scale = |c: u16| u8::try_from(c * k / 255).unwrap_or(u8::MAX);
                    luma(scale(r), scale(g), scale(b))
                })
                .collect(),
            _ => return self.clone(),
        };
        Samples {
            width: self.width,
            height: self.height,
            components: 1,
            data,
        }
    }

    /// Whether every pixel is gray already (each of its three components
    /// within `tolerance` of the others): a page scanned or drawn in gray,
    /// written as one component.
    pub fn looks_gray(&self, tolerance: u8) -> bool {
        match self.components {
            1 => true,
            3 => self.data.as_chunks::<3>().0.iter().all(|p| {
                let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
                hi - lo <= tolerance
            }),
            _ => false,
        }
    }

    /// Whether it is flat rather than a photograph: few distinct colours
    /// among pixels taken across it, or one colour (its background) over a
    /// good part of it (a screenshot, a chart, a page of text), which JPEG
    /// rings around and Deflate keeps small. Gray has too few levels for
    /// their count to tell: its background alone says.
    pub fn looks_flat(&self) -> bool {
        const TAKEN: u64 = 64 * 1024;
        const FLAT: usize = 512;
        const BACKGROUND: f64 = 0.35;
        let pixels = u64::from(self.width) * u64::from(self.height);
        if pixels == 0 {
            return true;
        }
        let step = (pixels / TAKEN).max(1);
        let size = usize::from(self.components);
        let mut seen: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut taken = 0u32;
        let mut at = 0u64;
        while at < pixels {
            let start = usize::try_from(at).unwrap_or(usize::MAX) * size;
            if let Some(pixel) = self.data.get(start..start + size) {
                // Five bits a component: noise in a photograph's smooth
                // areas still counts as one colour.
                let key = pixel
                    .iter()
                    .fold(0u32, |key, &c| (key << 5) | u32::from(c >> 3));
                *seen.entry(key).or_default() += 1;
                taken += 1;
            }
            at += step;
        }
        let top = seen.values().copied().max().unwrap_or(0);
        let background = f64::from(top) / f64::from(taken.max(1)) >= BACKGROUND;
        background || (self.components > 1 && seen.len() <= FLAT)
    }

    fn luma(&self) -> Result<image::GrayImage> {
        image::GrayImage::from_raw(self.width, self.height, self.data.clone())
            .context("gray samples of the wrong size")
    }

    fn rgb(&self) -> Result<image::RgbImage> {
        image::RgbImage::from_raw(self.width, self.height, self.data.clone())
            .context("three-component samples of the wrong size")
    }

    fn rgba(&self) -> Result<image::RgbaImage> {
        image::RgbaImage::from_raw(self.width, self.height, self.data.clone())
            .context("four-component samples of the wrong size")
    }
}

/// `width` × `height` brought so the longer side is at most `longest`,
/// its proportions kept, never enlarged, never to nothing.
pub fn fitted(width: u32, height: u32, longest: u32) -> (u32, u32) {
    let side = width.max(height);
    if side <= longest || side == 0 {
        return (width, height);
    }
    let scale = f64::from(longest) / f64::from(side);
    let shrunk = |n: u32| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = (f64::from(n) * scale).round() as u32;
        n.max(1)
    };
    (shrunk(width), shrunk(height))
}

fn luma(r: u8, g: u8, b: u8) -> u8 {
    let y = (299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b) + 500) / 1000;
    u8::try_from(y).unwrap_or(u8::MAX)
}

/// What a JPEG is written with besides its samples.
#[derive(Clone, Debug, Default)]
pub struct JpegExtras<'a> {
    pub icc: Option<&'a [u8]>,
    /// The EXIF block, as in an APP1 segment after `Exif\0\0`.
    pub exif: Option<&'a [u8]>,
}

/// `samples` as a JPEG at `quality` (1 to 100): gray or three components
/// (colour subsampled 4:2:0), optimized Huffman tables, progressive, and
/// the quantization tables that keep more for their size than libjpeg's
/// (`ImageMagick`'s, as mozjpeg uses). Four components are refused: a
/// CMYK JPEG's inversion is read two ways.
pub fn jpeg(samples: &Samples, quality: u8, extras: &JpegExtras<'_>) -> Result<Vec<u8>> {
    let color = match samples.components {
        1 => jpeg_encoder::ColorType::Luma,
        3 => jpeg_encoder::ColorType::Rgb,
        n => bail!("a JPEG of {n} components is not written"),
    };
    let (Ok(width), Ok(height)) = (u16::try_from(samples.width), u16::try_from(samples.height))
    else {
        bail!("a JPEG is at most 65,535 pixels across");
    };
    let mut out = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100));
    encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
    encoder.set_optimized_huffman_tables(true);
    encoder.set_progressive(true);
    encoder.set_quantization_tables(
        jpeg_encoder::QuantizationTableType::ImageMagick,
        jpeg_encoder::QuantizationTableType::ImageMagick,
    );
    if let Some(icc) = extras.icc {
        encoder.add_icc_profile(icc).context("the colour profile")?;
    }
    if let Some(exif) = extras.exif {
        encoder.add_exif_metadata(exif).context("the EXIF")?;
    }
    encoder
        .encode(&samples.data, width, height, color)
        .context("the JPEG could not be written")?;
    Ok(out)
}

/// `bytes` compressed by Deflate at its best (zlib, as `FlateDecode` reads).
pub fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    // Writing to a vector does not fail.
    let _ = encoder.write_all(bytes);
    encoder.finish().unwrap_or_default()
}

/// `samples` filtered row by row as PNG does (each row the filter that
/// leaves its smallest sum, `Predictor 15`), then compressed by Deflate:
/// what a flat image is written as, with the `DecodeParms` that read it.
pub fn predicted(samples: &Samples) -> Vec<u8> {
    let bpp = usize::from(samples.components);
    let stride = samples.width as usize * bpp;
    let mut out = Vec::with_capacity(samples.data.len() + samples.height as usize);
    let zero = vec![0u8; stride];
    let mut previous: &[u8] = &zero;
    let mut candidates = vec![vec![0u8; stride]; 5];
    for row in samples.data.chunks_exact(stride.max(1)) {
        for (kind, filtered) in candidates.iter_mut().enumerate() {
            for x in 0..stride {
                let a = if x >= bpp { row[x - bpp] } else { 0 };
                let b = previous[x];
                let c = if x >= bpp { previous[x - bpp] } else { 0 };
                let predicted = match kind {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => a.midpoint(b),
                    _ => paeth(a, b, c),
                };
                filtered[x] = row[x].wrapping_sub(predicted);
            }
        }
        let cost = |row: &[u8]| -> u64 {
            row.iter()
                .map(|&v| u64::from(v.cast_signed().unsigned_abs()))
                .sum()
        };
        let (kind, best) = candidates
            .iter()
            .enumerate()
            .min_by_key(|(_, row)| cost(row))
            .unwrap_or((0, &candidates[0]));
        out.push(u8::try_from(kind).unwrap_or(0));
        out.extend_from_slice(best);
        previous = row;
    }
    deflate(&out)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Samples {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[
                    u8::try_from(x * 255 / width.max(1)).unwrap(),
                    u8::try_from(y * 255 / height.max(1)).unwrap(),
                    u8::try_from((x + y) % 256).unwrap(),
                ]);
            }
        }
        Samples::new(width, height, 3, data).unwrap()
    }

    #[test]
    fn a_size_is_fitted_never_enlarged() {
        assert_eq!(fitted(4000, 3000, 1920), (1920, 1440));
        assert_eq!(fitted(3000, 4000, 1920), (1440, 1920));
        assert_eq!(fitted(800, 600, 1920), (800, 600));
        assert_eq!(fitted(10_000, 1, 100), (100, 1));
    }

    #[test]
    fn samples_are_resized_and_made_gray() {
        let photo = gradient(300, 200);
        let small = photo.resized(150, 100).unwrap();
        assert_eq!((small.width, small.height, small.components), (150, 100, 3));
        assert_eq!(small.data.len(), 150 * 100 * 3);
        let gray = small.gray();
        assert_eq!((gray.components, gray.data.len()), (1, 150 * 100));
        assert!(gray.looks_gray(0));
        assert!(!photo.looks_gray(4));
    }

    #[test]
    fn a_photograph_is_told_from_a_flat_picture() {
        assert!(!gradient(512, 512).looks_flat());
        let flat = Samples::new(64, 64, 3, [250, 250, 250].repeat(64 * 64)).unwrap();
        assert!(flat.looks_flat());
    }

    #[test]
    fn a_jpeg_and_a_deflate_read_back() {
        let photo = gradient(120, 80);
        let jpeg = jpeg(&photo, 80, &JpegExtras::default()).unwrap();
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (120, 80));
        let data = predicted(&photo);
        let mut raw = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(&data[..]), &mut raw)
            .unwrap();
        assert_eq!(raw.len(), (120 * 3 + 1) * 80);
    }
}
