//! Pictures made smaller (ADR-112): a JPEG or a PNG resized and written
//! again in its own format, turned upright as it is shown, with its colour
//! profile, and its EXIF told what was done (Orientation 1, the copy's
//! size) or cut down to its date when location and camera are left out.
//! Lossless re-encodes nothing at a loss: a JPEG keeps its compressed
//! pixels byte for byte, only its metadata rewritten; a PNG's pixels are
//! filtered and compressed again at their own depth and colour type.

use crate::samples::{self, JpegExtras, Samples};
use anyhow::{Context, Result, bail, ensure};
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{
    DynamicImage, ImageBuffer, ImageDecoder, ImageFormat, ImageReader, Limits, LumaA, Rgba,
};
use skwir_core::print::Picture;
use skwir_core::shrink::Quality;
use std::borrow::Cow;
use std::io::{Cursor, Read, Write};
use std::path::Path;

/// What a picture's copy is made with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub quality: Quality,
    /// Made gray; ignored with [`Quality::Lossless`], which changes no pixel.
    pub gray: bool,
    /// Location and camera details left out: the date and time it was
    /// taken stay.
    pub strip: bool,
    /// At most this many bytes: smaller steps are tried until it fits
    /// ([`Quality::picture_steps`]), the smallest kept when none does.
    /// Lossless has none: it re-encodes nothing at a loss.
    pub at_most: Option<u64>,
}

/// A copy written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// Its size, in bytes.
    pub bytes: u64,
    /// Whether it fits `at_most`, when asked.
    pub met: Option<bool>,
    /// What was left as it was, and why: said to the user.
    pub notes: Vec<String>,
}

/// The most bytes a picture file is read at: past what a picture within
/// [`Picture::MOST_PIXELS`] takes, even at 16 bits a sample.
const MOST_BYTES: u64 = 2 << 30;

const UNREADABLE: &str = "The picture could not be read.";
const DAMAGED_JPEG: &str = "The JPEG is damaged.";
const DAMAGED_PNG: &str = "The PNG is damaged.";

/// A smaller copy of the picture at `input`, written at `output` (JPEG
/// stays JPEG, PNG stays PNG); `Ok(None)` and nothing at `output` when the
/// copy would not be smaller than the input.
pub fn shrink(input: &Path, output: &Path, options: &Options) -> Result<Option<Written>> {
    let bytes = skwir_core::fs::file::read_regular(input, MOST_BYTES).context(UNREADABLE)?;
    let source = Source::read(&bytes)?;
    let rule = options.quality.picture();
    let gray = options.gray && rule.jpeg.is_some();
    let mut best = None;
    let mut original = None;
    match rule.jpeg {
        None => keep(&mut best, source.lossless(options.strip)?),
        Some(quality) => {
            let decoded = source.decode()?;
            // A PNG whose pixels this rule leaves as they are is only
            // packed tighter, below, keeping all that describes them.
            let as_stored = source.as_stored(&decoded, rule.longest, gray);
            if !as_stored {
                let copy = source.encoded(&decoded, rule.longest, quality, gray, options.strip)?;
                keep(&mut best, Some(copy));
            }
            // Pixels that need no change are also tried as they are: an
            // already small picture written again may only grow, and lose.
            let side = source.width.max(source.height);
            let fits = rule.longest.is_none_or(|longest| side <= longest);
            if fits
                && !(gray && source.colour)
                && let Some(mut kept) = source.lossless(options.strip)?
            {
                if !as_stored {
                    kept.notes.insert(
                        0,
                        "Its pixels were kept as they were: written again, they came out larger."
                            .to_owned(),
                    );
                }
                keep(&mut best, Some(kept));
            }
            original = Some(decoded);
        }
    }
    let before = bytes.len() as u64;
    let size = |best: &Option<Candidate>| best.as_ref().map_or(before, Candidate::len);
    let met = match options.at_most {
        None => None,
        Some(limit) => {
            // None under Lossless, which loses nothing to fit a size.
            let steps = options.quality.picture_steps();
            if size(&best) > limit && !steps.is_empty() {
                // Each step from the original pixels, never from a copy
                // already made smaller.
                let decoded = match original.take() {
                    Some(decoded) => decoded,
                    None => source.decode()?,
                };
                for step in steps {
                    let Some(quality) = step.jpeg else { continue };
                    if source.as_stored(&decoded, step.longest, gray) {
                        // Its copy as stored was tried above.
                        continue;
                    }
                    let copy =
                        source.encoded(&decoded, step.longest, quality, gray, options.strip)?;
                    keep(&mut best, Some(copy));
                    if size(&best) <= limit {
                        break;
                    }
                }
            }
            Some(size(&best) <= limit)
        }
    };
    let Some(best) = best.filter(|best| best.len() < before) else {
        return Ok(None);
    };
    std::fs::write(output, &best.bytes).context("The smaller copy could not be written.")?;
    Ok(Some(Written {
        bytes: best.len(),
        met,
        notes: best.notes,
    }))
}

/// A copy made, and what to say of it.
struct Candidate {
    bytes: Vec<u8>,
    notes: Vec<String>,
}

impl Candidate {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
}

/// `made` kept in `best` when it is smaller.
fn keep(best: &mut Option<Candidate>, made: Option<Candidate>) {
    if let Some(made) = made
        && best.as_ref().is_none_or(|best| made.len() < best.len())
    {
        *best = Some(made);
    }
}

/// How the decoder is bounded: room for the largest picture made smaller,
/// at 16 bits a sample with alpha.
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_alloc = Some(Picture::MOST_PIXELS * 8);
    limits
}

/// A picture read: its bytes, how they are stored, and the metadata a
/// copy keeps.
struct Source<'a> {
    bytes: &'a [u8],
    container: Container<'a>,
    /// Its width and height as stored, before it is turned.
    width: u32,
    height: u32,
    orientation: Orientation,
    /// Stored in colour (a palette counts), not gray.
    colour: bool,
    icc: Option<Vec<u8>>,
    /// Its EXIF, as a TIFF block (after a JPEG's `Exif\0\0`).
    exif: Option<Vec<u8>>,
}

enum Container<'a> {
    Jpeg(Jpeg<'a>),
    Png(Png<'a>),
}

impl<'a> Source<'a> {
    /// `bytes` read as a JPEG or a PNG, typed by their content; any other
    /// picture, an animated PNG, or one past [`Picture::MOST_PIXELS`]
    /// refused.
    fn read(bytes: &'a [u8]) -> Result<Source<'a>> {
        let mut reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .context(UNREADABLE)?;
        let container = match reader.format() {
            Some(ImageFormat::Jpeg) => Container::Jpeg(Jpeg::parse(bytes)?),
            Some(ImageFormat::Png) => {
                let png = Png::parse(bytes)?;
                ensure!(
                    !png.chunks.iter().any(|chunk| &chunk.kind == b"acTL"),
                    "An animated PNG is not made smaller."
                );
                Container::Png(png)
            }
            _ => bail!("Only JPEG and PNG pictures are made smaller."),
        };
        reader.limits(limits());
        let mut decoder = reader.into_decoder().context(UNREADABLE)?;
        let (width, height) = decoder.dimensions();
        ensure!(
            u64::from(width) * u64::from(height) <= Picture::MOST_PIXELS,
            "A picture of more than {} megapixels is not made smaller.",
            Picture::MOST_PIXELS / 1_000_000
        );
        let colour = decoder.color_type().has_color();
        let icc = decoder.icc_profile().ok().flatten();
        // A PNG's eXIf holds the TIFF block alone; some writers put the
        // JPEG's header before it.
        let exif = decoder.exif_metadata().ok().flatten().map(|block| {
            match block.strip_prefix(EXIF_HEADER) {
                Some(tiff) => tiff.to_vec(),
                None => block,
            }
        });
        // Read from the block the copy keeps, so its pixels are turned as
        // that block says, whatever stood before it.
        let orientation = exif
            .as_deref()
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms);
        Ok(Source {
            bytes,
            container,
            width,
            height,
            orientation,
            colour,
            icc,
            exif,
        })
    }

    /// Its pixels as stored, not yet turned.
    fn decode(&self) -> Result<Pixels> {
        let format = match self.container {
            Container::Jpeg(_) => ImageFormat::Jpeg,
            Container::Png(_) => ImageFormat::Png,
        };
        let mut reader = ImageReader::with_format(Cursor::new(self.bytes), format);
        reader.limits(limits());
        Ok(Pixels::of(reader.decode().context(UNREADABLE)?))
    }

    /// The copy that loses nothing: a PNG's pixels packed tighter, a JPEG's
    /// left byte for byte with its metadata cut down (none when `strip` is
    /// not asked: there is nothing else to change in it).
    fn lossless(&self, strip: bool) -> Result<Option<Candidate>> {
        match &self.container {
            Container::Png(png) => self.png_lossless(png, strip).map(Some),
            Container::Jpeg(jpeg) if strip => self.jpeg_stripped(jpeg).map(Some),
            Container::Jpeg(_) => Ok(None),
        }
    }

    /// Whether `original` brought to `longest`, gray when asked, holds a
    /// PNG's pixels as stored: the same size, 8 bits a sample and colour
    /// type (no palette expanded, no transparency added or taken), which
    /// the copy that loses nothing ([`Source::lossless`]) keeps with all
    /// that describes them.
    fn as_stored(&self, original: &Pixels, longest: Option<u32>, gray: bool) -> bool {
        let Container::Png(png) = &self.container else {
            return false;
        };
        let size = longest.map_or((original.width, original.height), |longest| {
            samples::fitted(original.width, original.height, longest)
        });
        let channels = if gray && original.colour() {
            original.channels - 2
        } else {
            original.channels
        };
        size == (png.header.width, png.header.height)
            && png.header.depth == 8
            && png.header.colour == colour_type(channels)
    }

    /// A copy re-encoded from `original`: its longer side at most
    /// `longest`, turned upright, gray when asked, a JPEG at `quality`.
    fn encoded(
        &self,
        original: &Pixels,
        longest: Option<u32>,
        quality: u8,
        gray: bool,
        strip: bool,
    ) -> Result<Candidate> {
        let (width, height) = match longest {
            Some(longest) => samples::fitted(original.width, original.height, longest),
            None => (original.width, original.height),
        };
        // Resized as stored, then turned: the turn works on the smaller
        // picture, and fitting a size is the same either way.
        let scale = (
            f64::from(width) / f64::from(original.width),
            f64::from(height) / f64::from(original.height),
        );
        let mut pixels = original.resized(width, height)?;
        let jpeg = matches!(self.container, Container::Jpeg(_));
        if jpeg && pixels.alpha() {
            pixels = pixels.without_alpha();
        }
        let made_gray = gray && pixels.colour();
        if made_gray {
            pixels = pixels.gray()?;
        }
        let pixels = pixels.turned(self.orientation)?;
        let size = (pixels.width, pixels.height);
        let mut notes = Vec::new();
        let profile = self.profile(pixels.colour());
        if self.icc.is_some() && profile.is_none() && !made_gray {
            notes.push(
                "Its colour profile was left out: it is not for the colours the copy is written in, \
                 which may look different."
                    .to_owned(),
            );
        }
        let exif = self.exif_for(strip, true, size, &mut notes);
        let bytes = match &self.container {
            Container::Jpeg(_) => {
                let samples =
                    Samples::new(pixels.width, pixels.height, pixels.channels, pixels.data)?;
                let extras = JpegExtras {
                    icc: profile,
                    exif: exif.as_deref(),
                };
                let written = samples::jpeg(&samples, quality, &extras)?;
                if strip {
                    written
                } else {
                    with_segments(written, APP1, &self.xmp())?
                }
            }
            Container::Png(png) => {
                let header = Header {
                    width: pixels.width,
                    height: pixels.height,
                    depth: 8,
                    colour: colour_type(pixels.channels),
                    interlaced: false,
                };
                let raster = Raster {
                    width: pixels.width,
                    height: pixels.height,
                    bits: pixels.channels * 8,
                    data: pixels.data,
                };
                let kept = Kept::Resampled {
                    profile: profile.is_some(),
                    scale,
                    quarter_turn: quarter_turn(self.orientation),
                };
                png.written(&header, &raster, &kept, strip, exif.as_deref())?
            }
        };
        Ok(Candidate { bytes, notes })
    }

    /// A PNG's pixels unfiltered and turned upright, then filtered and
    /// compressed again at their own depth and colour type, its palette
    /// and transparency kept.
    fn png_lossless(&self, png: &Png<'_>, strip: bool) -> Result<Candidate> {
        let raster = png.raster()?.turned(self.orientation);
        let header = Header {
            width: raster.width,
            height: raster.height,
            interlaced: false,
            ..png.header
        };
        let mut notes = Vec::new();
        let exif = self.exif_for(strip, true, (raster.width, raster.height), &mut notes);
        let kept = Kept::Lossless {
            quarter_turn: quarter_turn(self.orientation),
        };
        let bytes = png.written(&header, &raster, &kept, strip, exif.as_deref())?;
        Ok(Candidate { bytes, notes })
    }

    /// A JPEG's compressed pixels byte for byte, with its metadata cut
    /// down to its date, colour profile and how to turn it: what is not a
    /// picture's own (other applications' segments, comments, a second
    /// picture after its end) left out.
    fn jpeg_stripped(&self, jpeg: &Jpeg<'_>) -> Result<Candidate> {
        let mut notes = Vec::new();
        let mut exif = self.exif_for(true, false, (self.width, self.height), &mut notes);
        if self.orientation != Orientation::NoTransforms {
            notes.push(
                "Its pixels were kept as stored, turned upright by its EXIF as before.".to_owned(),
            );
        }
        let mut out = Vec::with_capacity(self.bytes.len());
        out.extend_from_slice(&[0xFF, 0xD8]);
        for segment in &jpeg.header {
            if segment.is_exif() {
                // Where its EXIF was, the first time.
                if let Some(block) = exif.take() {
                    write_segment(&mut out, APP1, &[EXIF_HEADER, &block].concat())?;
                }
            } else if let Some(body) = segment.stripped() {
                write_segment(&mut out, segment.marker, &body)?;
            }
        }
        jpeg.scans_stripped(&mut out)?;
        Ok(Candidate { bytes: out, notes })
    }

    /// Its colour profile, where it is for the copy's colours (`colour`,
    /// or gray): a CMYK JPEG's is not for the RGB its pixels are read as,
    /// nor an RGB one for a copy made gray.
    fn profile(&self, colour: bool) -> Option<&[u8]> {
        let icc = self.icc.as_deref()?;
        let space: &[u8] = if colour { b"RGB " } else { b"GRAY" };
        (icc.get(16..20) == Some(space)).then_some(icc)
    }

    /// The EXIF a copy is written with: its own, told the pixels now stand
    /// upright (`upright`) at `size`; or, with `strip`, its date alone, and
    /// how to turn pixels kept as stored.
    fn exif_for(
        &self,
        strip: bool,
        upright: bool,
        size: (u32, u32),
        notes: &mut Vec<String>,
    ) -> Option<Vec<u8>> {
        let block = self.exif.as_deref()?;
        if strip {
            let orientation = if upright {
                1
            } else {
                u16::from(self.orientation.to_exif())
            };
            dated(block, orientation, notes)
        } else {
            let turned = upright && self.orientation != Orientation::NoTransforms;
            Some(told(block, turned, size))
        }
    }

    /// A JPEG's XMP segments, its packet saying its pixels stand upright.
    fn xmp(&self) -> Vec<Vec<u8>> {
        let Container::Jpeg(jpeg) = &self.container else {
            return Vec::new();
        };
        jpeg.header
            .iter()
            .filter(|segment| segment.marker == APP1)
            .filter_map(|segment| {
                if segment.body.starts_with(XMP_HEADER) {
                    let mut body = segment.body.to_vec();
                    upright_xmp(&mut body);
                    Some(body)
                } else if segment.body.starts_with(XMP_EXTENSION) {
                    Some(segment.body.to_vec())
                } else {
                    None
                }
            })
            .collect()
    }
}

/// A PNG's colour type for pixels of `channels` 8-bit samples.
fn colour_type(channels: u8) -> u8 {
    match channels {
        1 => 0,
        2 => 4,
        3 => 2,
        _ => 6,
    }
}

/// Whether `orientation` turns a quarter, swapping width and height.
fn quarter_turn(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// Pixels 8 bits a sample, row by row: gray (1 channel), gray and alpha
/// (2), RGB (3) or RGBA (4).
struct Pixels {
    width: u32,
    height: u32,
    channels: u8,
    data: Vec<u8>,
}

impl Pixels {
    /// `image` in 8 bits a sample, gray kept gray, alpha kept only where
    /// some pixel is not opaque (it would only take room).
    fn of(image: DynamicImage) -> Pixels {
        let (width, height) = (image.width(), image.height());
        let colour = image.color().has_color();
        let (channels, data) = match (colour, image.color().has_alpha()) {
            (false, false) => (1, image.into_luma8().into_raw()),
            (false, true) => (2, image.into_luma_alpha8().into_raw()),
            (true, false) => (3, image.into_rgb8().into_raw()),
            (true, true) => (4, image.into_rgba8().into_raw()),
        };
        let pixels = Pixels {
            width,
            height,
            channels,
            data,
        };
        if pixels.alpha() && pixels.opaque() {
            pixels.without_alpha()
        } else {
            pixels
        }
    }

    fn alpha(&self) -> bool {
        self.channels.is_multiple_of(2)
    }

    fn colour(&self) -> bool {
        self.channels >= 3
    }

    fn opaque(&self) -> bool {
        let channels = usize::from(self.channels);
        self.data
            .chunks_exact(channels)
            .all(|pixel| pixel[channels - 1] == u8::MAX)
    }

    fn without_alpha(self) -> Pixels {
        if !self.alpha() {
            return self;
        }
        let channels = usize::from(self.channels);
        let data = self
            .data
            .chunks_exact(channels)
            .flat_map(|pixel| &pixel[..channels - 1])
            .copied()
            .collect();
        Pixels {
            channels: self.channels - 1,
            data,
            ..self
        }
    }

    /// Resized to `width` × `height` as [`samples::resize`] does, read
    /// where they are; with alpha, premultiplied at 16 bits a sample, so
    /// the colour of a transparent pixel, whatever it is, does not bleed
    /// into its neighbours.
    fn resized(&self, width: u32, height: u32) -> Result<Pixels> {
        if (width, height) == (self.width, self.height) {
            return Ok(Pixels {
                data: self.data.clone(),
                ..*self
            });
        }
        if !self.alpha() {
            let data = samples::resize(
                &self.data,
                (self.width, self.height),
                self.channels,
                (width, height),
            )?;
            return Ok(Pixels {
                width,
                height,
                channels: self.channels,
                data,
            });
        }
        let channels = usize::from(self.channels);
        let mut premultiplied = Vec::with_capacity(self.data.len());
        for pixel in self.data.chunks_exact(channels) {
            let alpha = u32::from(pixel[channels - 1]) * 257;
            for &c in &pixel[..channels - 1] {
                premultiplied.push(((u32::from(c) * alpha + 127) / 255) as u16);
            }
            premultiplied.push(alpha as u16);
        }
        let filter = FilterType::Triangle;
        let resized = if channels == 4 {
            let buffer =
                ImageBuffer::<Rgba<u16>, _>::from_raw(self.width, self.height, premultiplied)
                    .context("pixels of the wrong size")?;
            image::imageops::resize(&buffer, width, height, filter).into_raw()
        } else {
            let buffer =
                ImageBuffer::<LumaA<u16>, _>::from_raw(self.width, self.height, premultiplied)
                    .context("pixels of the wrong size")?;
            image::imageops::resize(&buffer, width, height, filter).into_raw()
        };
        let mut data = Vec::with_capacity(resized.len());
        for pixel in resized.chunks_exact(channels) {
            let alpha = u32::from(pixel[channels - 1]);
            for &c in &pixel[..channels - 1] {
                let c = (u32::from(c) * 255 + alpha / 2)
                    .checked_div(alpha)
                    .map_or(0, |c| c.min(255));
                data.push(c as u8);
            }
            data.push(((alpha + 128) / 257) as u8);
        }
        Ok(Pixels {
            width,
            height,
            channels: self.channels,
            data,
        })
    }

    /// Gray, by [`Samples::gray`], its alpha kept.
    fn gray(self) -> Result<Pixels> {
        match self.channels {
            3 => {
                let gray = Samples::new(self.width, self.height, 3, self.data)?.gray();
                Ok(Pixels {
                    channels: 1,
                    data: gray.data,
                    ..self
                })
            }
            4 => {
                let count = self.data.len() / 4;
                let (mut rgb, mut alpha) =
                    (Vec::with_capacity(count * 3), Vec::with_capacity(count));
                for pixel in self.data.as_chunks::<4>().0 {
                    rgb.extend_from_slice(&pixel[..3]);
                    alpha.push(pixel[3]);
                }
                let gray = Samples::new(self.width, self.height, 3, rgb)?.gray();
                let data = gray
                    .data
                    .iter()
                    .zip(&alpha)
                    .flat_map(|(&luma, &alpha)| [luma, alpha])
                    .collect();
                Ok(Pixels {
                    channels: 2,
                    data,
                    ..self
                })
            }
            _ => Ok(self),
        }
    }

    /// Turned as `orientation` says, by the picture library's own turn.
    fn turned(self, orientation: Orientation) -> Result<Pixels> {
        if orientation == Orientation::NoTransforms {
            return Ok(self);
        }
        let (width, height, channels) = (self.width, self.height, self.channels);
        let wrong = || anyhow::anyhow!("pixels of the wrong size");
        let mut image = match channels {
            1 => DynamicImage::ImageLuma8(
                ImageBuffer::from_raw(width, height, self.data).ok_or_else(wrong)?,
            ),
            2 => DynamicImage::ImageLumaA8(
                ImageBuffer::from_raw(width, height, self.data).ok_or_else(wrong)?,
            ),
            3 => DynamicImage::ImageRgb8(
                ImageBuffer::from_raw(width, height, self.data).ok_or_else(wrong)?,
            ),
            _ => DynamicImage::ImageRgba8(
                ImageBuffer::from_raw(width, height, self.data).ok_or_else(wrong)?,
            ),
        };
        image.apply_orientation(orientation);
        Ok(Pixels {
            width: image.width(),
            height: image.height(),
            channels,
            data: image.into_bytes(),
        })
    }
}

// JPEG: its segments read up to its first scan, so metadata can be cut
// or added while its compressed pixels stay byte for byte.

const APP1: u8 = 0xE1;
const EXIF_HEADER: &[u8] = b"Exif\0\0";
const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const XMP_EXTENSION: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
const ICC_HEADER: &[u8] = b"ICC_PROFILE\0";

/// A JPEG's segments before its first scan, and where that scan starts.
struct Jpeg<'a> {
    bytes: &'a [u8],
    header: Vec<Segment<'a>>,
    scans_at: usize,
}

/// A marker segment: its marker, where it starts (its `0xFF`), and its
/// body after the length.
struct Segment<'a> {
    marker: u8,
    at: usize,
    body: &'a [u8],
}

impl<'a> Segment<'a> {
    fn is_exif(&self) -> bool {
        self.marker == APP1 && self.body.starts_with(EXIF_HEADER)
    }

    /// What stays of it in a copy left without location and camera: the
    /// tables and frame that decode it, JFIF (without the thumbnail it may
    /// hold), its colour profile, Adobe's colour transform; not other
    /// applications' segments (XMP, IPTC, a maker's, a second picture's
    /// index, a thumbnail) or comments.
    fn stripped(&self) -> Option<Cow<'a, [u8]>> {
        match self.marker {
            0xE0 if self.body.starts_with(b"JFIF\0") => {
                // Its first 14 bytes, the thumbnail's size after them 0 × 0.
                match self.body.get(..14) {
                    Some(fixed) if self.body.len() > 14 || fixed[12..] != [0, 0] => {
                        let mut fixed = fixed.to_vec();
                        fixed[12..].fill(0);
                        Some(Cow::Owned(fixed))
                    }
                    _ => Some(Cow::Borrowed(self.body)),
                }
            }
            0xE2 if self.body.starts_with(ICC_HEADER) => Some(Cow::Borrowed(self.body)),
            0xEE if self.body.starts_with(b"Adobe") => Some(Cow::Borrowed(self.body)),
            0xE0..=0xEF | 0xFE => None,
            _ => Some(Cow::Borrowed(self.body)),
        }
    }
}

impl<'a> Jpeg<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Jpeg<'a>> {
        ensure!(bytes.starts_with(&[0xFF, 0xD8]), DAMAGED_JPEG);
        let mut header = Vec::new();
        let mut at = 2;
        loop {
            // Bytes that are no marker, as some writers leave between
            // segments, are passed over, as libjpeg does; fill bytes may
            // stand before a marker.
            while bytes.get(at).is_some_and(|&byte| byte != 0xFF) {
                at += 1;
            }
            while bytes.get(at + 1) == Some(&0xFF) {
                at += 1;
            }
            let marker = *bytes.get(at + 1).context(DAMAGED_JPEG)?;
            match marker {
                0xDA => {
                    return Ok(Jpeg {
                        bytes,
                        header,
                        scans_at: at,
                    });
                }
                0xD8 | 0xD9 => bail!(DAMAGED_JPEG),
                // Markers without a length.
                0x01 | 0xD0..=0xD7 => at += 2,
                _ => {
                    let length = usize::from(u16::from_be_bytes(
                        bytes
                            .get(at + 2..at + 4)
                            .context(DAMAGED_JPEG)?
                            .try_into()?,
                    ));
                    ensure!(length >= 2, DAMAGED_JPEG);
                    let body = bytes.get(at + 4..at + 2 + length).context(DAMAGED_JPEG)?;
                    header.push(Segment { marker, at, body });
                    at += 2 + length;
                }
            }
        }
    }

    /// Its scans to its end, byte for byte, written to `out`: other
    /// applications' segments and comments between them left out, and
    /// what follows its end (a second picture, a video) too. One that
    /// stops short is ended.
    fn scans_stripped(&self, out: &mut Vec<u8>) -> Result<()> {
        let bytes = self.bytes;
        let mut at = self.scans_at;
        loop {
            while bytes.get(at).is_some_and(|&byte| byte != 0xFF) {
                at += 1;
            }
            while bytes.get(at + 1) == Some(&0xFF) {
                at += 1;
            }
            let Some(&marker) = bytes.get(at + 1) else {
                out.extend_from_slice(&[0xFF, 0xD9]);
                return Ok(());
            };
            match marker {
                0xD9 => {
                    out.extend_from_slice(&[0xFF, 0xD9]);
                    return Ok(());
                }
                0x01 | 0xD0..=0xD7 => {
                    out.extend_from_slice(&bytes[at..at + 2]);
                    at += 2;
                    continue;
                }
                _ => {}
            }
            let length = usize::from(u16::from_be_bytes(
                bytes
                    .get(at + 2..at + 4)
                    .context(DAMAGED_JPEG)?
                    .try_into()?,
            ));
            let end = at + 2 + length;
            ensure!(length >= 2 && end <= bytes.len(), DAMAGED_JPEG);
            if !matches!(marker, 0xE0..=0xEF | 0xFE) {
                out.extend_from_slice(&bytes[at..end]);
            }
            at = end;
            if marker == 0xDA {
                // Its compressed data, to the next marker: a 0xFF there is
                // followed by 0 (a byte of data) or a restart marker.
                let start = at;
                loop {
                    match bytes.get(at) {
                        None => break,
                        Some(&0xFF) => match bytes.get(at + 1) {
                            Some(&(0x00 | 0xD0..=0xD7)) => at += 2,
                            _ => break,
                        },
                        Some(_) => at += 1,
                    }
                }
                out.extend_from_slice(&bytes[start..at]);
            }
        }
    }
}

/// `body` written as a segment of `marker`.
fn write_segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) -> Result<()> {
    let length = u16::try_from(body.len() + 2).context("a JPEG segment of more than 64 KiB")?;
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(body);
    Ok(())
}

/// `jpeg` with segments of `marker` holding `bodies`, after its other
/// application segments, before its tables.
fn with_segments(jpeg: Vec<u8>, marker: u8, bodies: &[Vec<u8>]) -> Result<Vec<u8>> {
    if bodies.is_empty() {
        return Ok(jpeg);
    }
    let parsed = Jpeg::parse(&jpeg)?;
    let at = parsed
        .header
        .iter()
        .find(|segment| !(0xE0..=0xEF).contains(&segment.marker))
        .map_or(parsed.scans_at, |segment| segment.at);
    let mut out =
        Vec::with_capacity(jpeg.len() + bodies.iter().map(|body| body.len() + 4).sum::<usize>());
    out.extend_from_slice(&jpeg[..at]);
    for body in bodies {
        write_segment(&mut out, marker, body)?;
    }
    out.extend_from_slice(&jpeg[at..]);
    Ok(out)
}

/// An XMP packet's `tiff:Orientation` set to 1, one digit for another in
/// place: its pixels now stand as they are shown.
fn upright_xmp(xmp: &mut [u8]) {
    const NAME: &[u8] = b"tiff:Orientation";
    let mut at = 0;
    while let Some(found) = xmp[at..]
        .windows(NAME.len())
        .position(|window| window == NAME)
    {
        // `="6"`, `='6'` or `>6<`, spaces allowed between.
        let mut i = at + found + NAME.len();
        while matches!(
            xmp.get(i),
            Some(b' ' | b'\t' | b'\r' | b'\n' | b'=' | b'"' | b'\'' | b'>')
        ) {
            i += 1;
        }
        if matches!(xmp.get(i), Some(b'2'..=b'8'))
            && matches!(xmp.get(i + 1), Some(b'"' | b'\'' | b'<' | b' '))
        {
            xmp[i] = b'1';
        }
        at = i;
    }
}

// EXIF: patched where its values are stored, so every other tag (a maker
// note's offsets among them) stays as it was; or rebuilt from its date.

const SHORT: u16 = 3;
const LONG: u16 = 4;
const IMAGE_WIDTH: u16 = 0x0100;
const IMAGE_LENGTH: u16 = 0x0101;
const ORIENTATION: u16 = 0x0112;
const THUMBNAIL_OFFSET: u16 = 0x0201;
const THUMBNAIL_LENGTH: u16 = 0x0202;
const EXIF_IFD: u16 = 0x8769;
const PIXEL_X: u16 = 0xA002;
const PIXEL_Y: u16 = 0xA003;

/// The EXIF block `block` told what was done to the pixels: turned
/// upright (`turned`: Orientation 1, and the thumbnail it holds unlinked,
/// which would now stand sideways) and brought to `width` × `height` (its
/// pixel sizes, where it gives them).
fn told(block: &[u8], turned: bool, (width, height): (u32, u32)) -> Vec<u8> {
    let Some(mut tiff) = Tiff::new(block.to_vec()) else {
        return block.to_vec();
    };
    let Some(ifd0) = tiff.u32_at(4).map(|offset| offset as usize) else {
        return tiff.data;
    };
    if turned {
        tiff.set(ifd0, ORIENTATION, 1);
    }
    tiff.set(ifd0, IMAGE_WIDTH, width);
    tiff.set(ifd0, IMAGE_LENGTH, height);
    if let Some(exif_ifd) = tiff
        .entry(ifd0, EXIF_IFD)
        .and_then(|entry| tiff.number(entry))
    {
        let exif_ifd = exif_ifd as usize;
        tiff.set(exif_ifd, PIXEL_X, width);
        tiff.set(exif_ifd, PIXEL_Y, height);
    }
    if turned {
        tiff.unlink_thumbnail(ifd0);
    }
    tiff.data
}

/// A TIFF block, read and patched in its own byte order.
struct Tiff {
    data: Vec<u8>,
    little: bool,
}

impl Tiff {
    fn new(data: Vec<u8>) -> Option<Tiff> {
        let little = match data.get(..4)? {
            [b'I', b'I', 42, 0] => true,
            [b'M', b'M', 0, 42] => false,
            _ => return None,
        };
        Some(Tiff { data, little })
    }

    fn u16_at(&self, at: usize) -> Option<u16> {
        let bytes: [u8; 2] = self.data.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    }

    fn u32_at(&self, at: usize) -> Option<u32> {
        let bytes: [u8; 4] = self.data.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }

    fn put(&mut self, at: usize, bytes: &[u8]) {
        if let Some(slot) = self.data.get_mut(at..at + bytes.len()) {
            slot.copy_from_slice(bytes);
        }
    }

    fn put_u16(&mut self, at: usize, value: u16) {
        let bytes = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.put(at, &bytes);
    }

    fn put_u32(&mut self, at: usize, value: u32) {
        let bytes = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.put(at, &bytes);
    }

    /// Where `ifd`'s entry for `tag` is.
    fn entry(&self, ifd: usize, tag: u16) -> Option<usize> {
        let count = usize::from(self.u16_at(ifd)?);
        (0..count)
            .map(|index| ifd + 2 + 12 * index)
            .take_while(|&entry| entry + 12 <= self.data.len())
            .find(|&entry| self.u16_at(entry) == Some(tag))
    }

    /// The number an entry holds, one short or long stored in it.
    fn number(&self, entry: usize) -> Option<u32> {
        match (self.u16_at(entry + 2)?, self.u32_at(entry + 4)?) {
            (SHORT, 1) => self.u16_at(entry + 8).map(u32::from),
            (LONG, 1) => self.u32_at(entry + 8),
            _ => None,
        }
    }

    /// `ifd`'s `tag` set to `value`, where it is there as one short or long.
    fn set(&mut self, ifd: usize, tag: u16, value: u32) {
        let Some(entry) = self.entry(ifd, tag) else {
            return;
        };
        match (self.u16_at(entry + 2), self.u32_at(entry + 4)) {
            (Some(SHORT), Some(1)) => {
                if let Ok(value) = u16::try_from(value) {
                    self.put_u16(entry + 8, value);
                }
            }
            (Some(LONG), Some(1)) => self.put_u32(entry + 8, value),
            _ => {}
        }
    }

    /// The thumbnail's directory unlinked from the first; its bytes cut
    /// too where they end the block, as cameras write them.
    fn unlink_thumbnail(&mut self, ifd0: usize) {
        let Some(count) = self.u16_at(ifd0) else {
            return;
        };
        let next = ifd0 + 2 + 12 * usize::from(count);
        let Some(ifd1) = self.u32_at(next).filter(|&offset| offset != 0) else {
            return;
        };
        let ifd1 = ifd1 as usize;
        let field = |tag| self.entry(ifd1, tag).and_then(|entry| self.number(entry));
        let thumbnail = field(THUMBNAIL_OFFSET).zip(field(THUMBNAIL_LENGTH));
        self.put_u32(next, 0);
        if let Some((offset, length)) = thumbnail {
            let (offset, end) = (offset as usize, offset as usize + length as usize);
            let len = self.data.len();
            // Up to 3 bytes of padding may follow it.
            if offset > next + 4 && offset < end && end <= len && end + 3 >= len {
                self.data.truncate(offset);
            }
        }
    }
}

/// What stays of an EXIF left without location and camera: when the
/// picture was taken, and its time zone and fractions of a second.
const DATES: [exif::Tag; 9] = [
    exif::Tag::DateTime,
    exif::Tag::DateTimeOriginal,
    exif::Tag::DateTimeDigitized,
    exif::Tag::OffsetTime,
    exif::Tag::OffsetTimeOriginal,
    exif::Tag::OffsetTimeDigitized,
    exif::Tag::SubSecTime,
    exif::Tag::SubSecTimeOriginal,
    exif::Tag::SubSecTimeDigitized,
];

/// The date and time a picture was taken, from `block`, in an EXIF of its
/// own, with its `orientation` where the pixels are kept as stored (other
/// than 1): no location, camera, maker note or thumbnail. None when there
/// is nothing to keep.
fn dated(block: &[u8], orientation: u16, notes: &mut Vec<String>) -> Option<Vec<u8>> {
    let read = exif::Reader::new()
        .continue_on_error(true)
        .read_raw(block.to_vec());
    let read = match read {
        Ok(exif) => Some(exif),
        // What could be read of a block partly damaged.
        Err(exif::Error::PartialResult(partial)) => Some(partial.into_inner().0),
        Err(_) => {
            notes.push("Its EXIF could not be read, so its date was left out.".to_owned());
            None
        }
    };
    let little = read.as_ref().is_some_and(exif::Exif::little_endian);
    let (dates, unknown): (Vec<&exif::Field>, Vec<&exif::Field>) = read
        .iter()
        .flat_map(exif::Exif::fields)
        .filter(|field| field.ifd_num == exif::In::PRIMARY && DATES.contains(&field.tag))
        // A value of a type EXIF does not have is one no writer writes.
        .partition(|field| !matches!(field.value, exif::Value::Unknown(..)));
    let turn = (orientation != 1).then(|| exif::Field {
        tag: exif::Tag::Orientation,
        ifd_num: exif::In::PRIMARY,
        value: exif::Value::Short(vec![orientation]),
    });
    let written = |dates: &[&exif::Field]| -> Option<Vec<u8>> {
        if dates.is_empty() && turn.is_none() {
            return None;
        }
        let mut writer = exif::experimental::Writer::new();
        for field in dates.iter().copied().chain(turn.as_ref()) {
            writer.push_field(field);
        }
        let mut out = Cursor::new(Vec::new());
        writer.write(&mut out, little).ok()?;
        Some(out.into_inner())
    };
    if !unknown.is_empty() {
        notes.push("Part of its date could not be read, so it was left out.".to_owned());
    }
    match written(&dates) {
        Some(block) => Some(block),
        None if dates.is_empty() => None,
        // How to turn it kept whatever its date holds.
        None => {
            notes.push("Its date could not be kept, so it was left out.".to_owned());
            written(&[])
        }
    }
}

// PNG: read chunk by chunk, so a copy keeps what it is told to, and its
// pixels can be unfiltered and filtered again at any depth.

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// The most bytes in one IDAT chunk written.
const IDAT_SIZE: usize = 1 << 20;

/// Where each pass of an interlaced PNG (Adam7) starts, and its step.
const ADAM7: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

struct Png<'a> {
    header: Header,
    chunks: Vec<Chunk<'a>>,
}

struct Chunk<'a> {
    kind: [u8; 4],
    data: &'a [u8],
}

/// A PNG's IHDR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Header {
    width: u32,
    height: u32,
    /// Bits a sample (an index, for a palette).
    depth: u8,
    /// 0 gray, 2 RGB, 3 palette, 4 gray and alpha, 6 RGBA.
    colour: u8,
    interlaced: bool,
}

impl Header {
    fn parse(data: &[u8]) -> Result<Header> {
        ensure!(data.len() == 13, DAMAGED_PNG);
        let header = Header {
            width: u32::from_be_bytes(data[0..4].try_into()?),
            height: u32::from_be_bytes(data[4..8].try_into()?),
            depth: data[8],
            colour: data[9],
            interlaced: data[12] == 1,
        };
        let depths: &[u8] = match header.colour {
            0 => &[1, 2, 4, 8, 16],
            3 => &[1, 2, 4, 8],
            2 | 4 | 6 => &[8, 16],
            _ => &[],
        };
        ensure!(
            depths.contains(&header.depth) && data[10] == 0 && data[11] == 0 && data[12] <= 1,
            DAMAGED_PNG
        );
        Ok(header)
    }

    fn bytes(self) -> [u8; 13] {
        let mut bytes = [0; 13];
        bytes[0..4].copy_from_slice(&self.width.to_be_bytes());
        bytes[4..8].copy_from_slice(&self.height.to_be_bytes());
        bytes[8] = self.depth;
        bytes[9] = self.colour;
        bytes[12] = u8::from(self.interlaced);
        bytes
    }

    /// Bits a pixel.
    fn bits(self) -> u8 {
        let channels = match self.colour {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        };
        channels * self.depth
    }
}

/// What a PNG copy keeps of the original's chunks: all that describes its
/// pixels when they are the same (a palette, its transparency), only what
/// describes their colours when they are resampled, its profile where it
/// still fits; and its pixels' physical size, scaled with them and swapped
/// by a quarter turn.
enum Kept {
    Lossless {
        quarter_turn: bool,
    },
    Resampled {
        profile: bool,
        /// How much each side, as stored, was brought down.
        scale: (f64, f64),
        quarter_turn: bool,
    },
}

impl<'a> Png<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Png<'a>> {
        ensure!(bytes.starts_with(PNG_SIGNATURE), DAMAGED_PNG);
        let mut chunks = Vec::new();
        let mut at = PNG_SIGNATURE.len();
        // One without its end is read as far as it goes, as viewers do.
        while at < bytes.len() {
            let length = u32::from_be_bytes(bytes.get(at..at + 4).context(DAMAGED_PNG)?.try_into()?)
                as usize;
            let kind: [u8; 4] = bytes.get(at + 4..at + 8).context(DAMAGED_PNG)?.try_into()?;
            let data = bytes.get(at + 8..at + 8 + length).context(DAMAGED_PNG)?;
            let crc = bytes
                .get(at + 8 + length..at + 12 + length)
                .context(DAMAGED_PNG)?;
            let mut sum = flate2::Crc::new();
            sum.update(&kind);
            sum.update(data);
            ensure!(crc == sum.sum().to_be_bytes(), DAMAGED_PNG);
            // A critical chunk not known is one a copy cannot keep.
            ensure!(
                kind[0] & 0x20 != 0 || matches!(&kind, b"IHDR" | b"PLTE" | b"IDAT" | b"IEND"),
                "The PNG holds a part that is not understood."
            );
            at += 12 + length;
            chunks.push(Chunk { kind, data });
            if &kind == b"IEND" {
                break;
            }
        }
        let first = chunks
            .first()
            .filter(|chunk| &chunk.kind == b"IHDR")
            .context(DAMAGED_PNG)?;
        let header = Header::parse(first.data)?;
        Ok(Png { header, chunks })
    }

    /// Its pixels, unfiltered and, when interlaced, put in place: its
    /// compressed data read where it is, each row unfiltered as it comes.
    fn raster(&self) -> Result<Raster> {
        let header = self.header;
        let bits = header.bits();
        let bpp = usize::from(bits.div_ceil(8));
        let mut inflated = flate2::read::ZlibDecoder::new(Idats {
            chunks: self.chunks.iter(),
            current: &[],
        });
        let mut rows = |width: u32, height: u32| -> Result<Raster> {
            let mut data = Vec::new();
            samples::unfilter_rows(
                &mut inflated,
                stride(width, bits),
                bpp,
                height as usize,
                &mut data,
            )
            .context(DAMAGED_PNG)?;
            Ok(Raster {
                width,
                height,
                bits,
                data,
            })
        };
        if !header.interlaced {
            return rows(header.width, header.height);
        }
        let mut full = Raster::new(header.width, header.height, bits);
        for (x0, y0, dx, dy) in ADAM7 {
            let across = |size: u32, start: u32, step: u32| {
                if size > start {
                    (size - start).div_ceil(step)
                } else {
                    0
                }
            };
            let (width, height) = (across(header.width, x0, dx), across(header.height, y0, dy));
            if width == 0 || height == 0 {
                continue;
            }
            let pass = rows(width, height)?;
            for y in 0..height {
                for x in 0..width {
                    full.set(x0 + x * dx, y0 + y * dy, pass.get(x, y));
                }
            }
        }
        Ok(full)
    }

    /// A PNG of `raster` as `header` says, keeping what `kept` and `strip`
    /// allow of this one's chunks, in their order, with `exif` for its
    /// eXIf (before the pixels, where readers look for it).
    fn written(
        &self,
        header: &Header,
        raster: &Raster,
        kept: &Kept,
        strip: bool,
        exif: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let (lossless, profile) = match *kept {
            Kept::Lossless { .. } => (true, true),
            Kept::Resampled { profile, .. } => (false, profile),
        };
        let mut out = PNG_SIGNATURE.to_vec();
        chunk(&mut out, *b"IHDR", &header.bytes());
        let first_idat = self
            .chunks
            .iter()
            .position(|chunk| &chunk.kind == b"IDAT")
            .unwrap_or(self.chunks.len());
        let (before, after) = self.chunks.split_at(first_idat);
        let mut exif = exif;
        for original in before {
            if &original.kind == b"eXIf" {
                if let Some(block) = exif.take() {
                    chunk(&mut out, *b"eXIf", block);
                }
            } else if kept_chunk(original.kind, lossless, strip, profile) {
                chunk(&mut out, original.kind, &kept_data(original, kept));
            }
        }
        if let Some(block) = exif {
            chunk(&mut out, *b"eXIf", block);
        }
        // Its rows filtered, compressed and cut into chunks as they come,
        // never held whole. A palette or fewer than 8 bits a pixel compress
        // best unfiltered, as libpng has it.
        let adaptive = header.colour != 3 && header.depth >= 8;
        let mut compressed = flate2::write::ZlibEncoder::new(
            Idat {
                out: &mut out,
                pending: Vec::with_capacity(IDAT_SIZE),
            },
            flate2::Compression::best(),
        );
        samples::filter_rows(
            &raster.data,
            stride(raster.width, raster.bits),
            usize::from(raster.bits.div_ceil(8)),
            adaptive,
            &mut compressed,
        )?;
        compressed.finish()?.end();
        for original in after {
            if kept_chunk(original.kind, lossless, strip, profile) {
                chunk(&mut out, original.kind, &kept_data(original, kept));
            }
        }
        chunk(&mut out, *b"IEND", &[]);
        Ok(out)
    }
}

/// Whether a PNG copy keeps a chunk of `kind`: see [`Kept`]. Left without
/// location and camera (`strip`), no text (XMP, comments, EXIF as text)
/// or chunk not known; never one not known that depends on the pixels
/// (its fifth bit unset in its last letter), whose meaning their
/// rewriting breaks: Apple's `iDOT` gives offsets into them.
fn kept_chunk(kind: [u8; 4], lossless: bool, strip: bool, profile: bool) -> bool {
    match &kind {
        b"IHDR" | b"IDAT" | b"IEND" | b"eXIf" => false,
        b"PLTE" | b"tRNS" | b"sBIT" | b"bKGD" | b"hIST" | b"sPLT" => lossless,
        b"pHYs" | b"gAMA" | b"cHRM" | b"sRGB" | b"cICP" | b"mDCv" | b"cLLi" => true,
        b"iCCP" => profile,
        b"tIME" => lossless && !strip,
        b"tEXt" | b"zTXt" | b"iTXt" => !strip,
        _ => !strip && kind[0] & 0x20 != 0 && kind[3] & 0x20 != 0,
    }
}

/// What a chunk kept holds in the copy: its pixels a unit (pHYs) as many
/// fewer as its sides have, so the picture keeps its size on paper, and
/// swapped by a quarter turn; an XMP packet saying
/// its pixels stand upright (one stored as it is: its keyword, then a 0
/// for no compression).
fn kept_data<'d>(original: &Chunk<'d>, kept: &Kept) -> Cow<'d, [u8]> {
    match &original.kind {
        b"pHYs" if original.data.len() == 9 => {
            let ((across, down), quarter_turn) = match *kept {
                Kept::Lossless { quarter_turn } => ((1.0, 1.0), quarter_turn),
                Kept::Resampled {
                    scale,
                    quarter_turn,
                    ..
                } => (scale, quarter_turn),
            };
            let at = |range: std::ops::Range<usize>| {
                u32::from_be_bytes(original.data[range].try_into().unwrap_or_default())
            };
            // A count only ever brought down (a picture is never enlarged),
            // never to none.
            let scaled = |count: u32, scale: f64| {
                if count == 0 || scale >= 1.0 {
                    count
                } else {
                    (f64::from(count) * scale).round().max(1.0) as u32
                }
            };
            let (x, y) = (scaled(at(0..4), across), scaled(at(4..8), down));
            let (x, y) = if quarter_turn { (y, x) } else { (x, y) };
            let mut data = original.data.to_vec();
            data[0..4].copy_from_slice(&x.to_be_bytes());
            data[4..8].copy_from_slice(&y.to_be_bytes());
            Cow::Owned(data)
        }
        b"iTXt" if original.data.starts_with(b"XML:com.adobe.xmp\0\0") => {
            let mut xmp = original.data.to_vec();
            upright_xmp(&mut xmp);
            Cow::Owned(xmp)
        }
        _ => Cow::Borrowed(original.data),
    }
}

/// A PNG chunk of `kind` holding `data`, written to `out`.
fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let mut sum = flate2::Crc::new();
    sum.update(&kind);
    sum.update(data);
    out.extend_from_slice(&sum.sum().to_be_bytes());
}

/// Bytes a row of `width` pixels of `bits` each takes.
fn stride(width: u32, bits: u8) -> usize {
    (width as usize * usize::from(bits)).div_ceil(8)
}

/// Pixels of any depth, row by row, each row starting on a byte: a PNG's
/// as stored, palette indices and 1 to 64 bits a pixel alike.
#[derive(Debug, PartialEq, Eq)]
struct Raster {
    width: u32,
    height: u32,
    bits: u8,
    data: Vec<u8>,
}

impl Raster {
    fn new(width: u32, height: u32, bits: u8) -> Raster {
        Raster {
            width,
            height,
            bits,
            data: vec![0; stride(width, bits) * height as usize],
        }
    }

    fn get(&self, x: u32, y: u32) -> u64 {
        let row = y as usize * stride(self.width, self.bits);
        let bits = usize::from(self.bits);
        if bits >= 8 {
            let start = row + x as usize * bits / 8;
            self.data[start..start + bits / 8]
                .iter()
                .fold(0, |value, &byte| (value << 8) | u64::from(byte))
        } else {
            let bit = x as usize * bits;
            let shift = 8 - bits - bit % 8;
            u64::from(self.data[row + bit / 8] >> shift) & ((1 << bits) - 1)
        }
    }

    fn set(&mut self, x: u32, y: u32, value: u64) {
        let row = y as usize * stride(self.width, self.bits);
        let bits = usize::from(self.bits);
        if bits >= 8 {
            let start = row + x as usize * bits / 8;
            let bytes = value.to_be_bytes();
            self.data[start..start + bits / 8].copy_from_slice(&bytes[8 - bits / 8..]);
        } else {
            let bit = x as usize * bits;
            let shift = 8 - bits - bit % 8;
            let mask = (((1u16 << bits) - 1) as u8) << shift;
            let byte = &mut self.data[row + bit / 8];
            *byte = (*byte & !mask) | (((value as u8) << shift) & mask);
        }
    }

    /// Turned as `orientation` says (EXIF's eight), pixel by pixel at any
    /// depth: the same turns as the picture library's.
    fn turned(self, orientation: Orientation) -> Raster {
        if orientation == Orientation::NoTransforms {
            return self;
        }
        let (width, height) = (self.width, self.height);
        let mut out = if quarter_turn(orientation) {
            Raster::new(height, width, self.bits)
        } else {
            Raster::new(width, height, self.bits)
        };
        // Where each pixel of the turned picture comes from.
        let from = |x: u32, y: u32| match orientation {
            Orientation::NoTransforms => (x, y),
            Orientation::FlipHorizontal => (width - 1 - x, y),
            Orientation::Rotate180 => (width - 1 - x, height - 1 - y),
            Orientation::FlipVertical => (x, height - 1 - y),
            Orientation::Rotate90FlipH => (y, x),
            Orientation::Rotate90 => (y, height - 1 - x),
            Orientation::Rotate270FlipH => (width - 1 - y, height - 1 - x),
            Orientation::Rotate270 => (width - 1 - y, x),
        };
        for y in 0..out.height {
            for x in 0..out.width {
                let (sx, sy) = from(x, y);
                out.set(x, y, self.get(sx, sy));
            }
        }
        out
    }
}

/// The data of a PNG's IDAT chunks, read one after another where they
/// are.
struct Idats<'c, 'a> {
    chunks: std::slice::Iter<'c, Chunk<'a>>,
    current: &'a [u8],
}

impl Read for Idats<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.current.is_empty() {
            match self.chunks.find(|chunk| &chunk.kind == b"IDAT") {
                Some(chunk) => self.current = chunk.data,
                None => return Ok(0),
            }
        }
        self.current.read(buf)
    }
}

/// What is written to it put in `out` as IDAT chunks of [`IDAT_SIZE`]
/// bytes, what remains in a last one when it ends.
struct Idat<'o> {
    out: &'o mut Vec<u8>,
    pending: Vec<u8>,
}

impl Write for Idat<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let taken = buf.len().min(IDAT_SIZE - self.pending.len());
        self.pending.extend_from_slice(&buf[..taken]);
        if self.pending.len() == IDAT_SIZE {
            chunk(self.out, *b"IDAT", &self.pending);
            self.pending.clear();
        }
        Ok(taken)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Idat<'_> {
    fn end(self) {
        if !self.pending.is_empty() {
            chunk(self.out, *b"IDAT", &self.pending);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exif::{Field, In, Rational, Tag, Value};
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, PngEncoder};
    use std::io::Write;

    /// A synthetic photograph: gradients, squares and noise.
    fn photo(width: u32, height: u32) -> Vec<u8> {
        let mut seed = 0x2545_f491_u32;
        let mut data = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let noise = (seed >> 27) as u8;
                let square = if (x / 48 + y / 48) % 2 == 0 { 40 } else { 0 };
                data.extend_from_slice(&[
                    ((x * 180 / width) as u8)
                        .saturating_add(noise)
                        .saturating_add(square),
                    ((y * 180 / height) as u8).saturating_add(noise),
                    (((x + y) * 120 / (width + height)) as u8)
                        .saturating_add(noise / 2)
                        .saturating_add(square),
                ]);
            }
        }
        data
    }

    /// An EXIF block as a camera writes it: turned by `orientation`, with
    /// a place, a camera, a date, a size and a thumbnail.
    fn exif_block(little: bool, orientation: u16, (width, height): (u32, u32)) -> Vec<u8> {
        let ascii = |text: &str| Value::Ascii(vec![text.as_bytes().to_vec()]);
        let field = |tag, value| Field {
            tag,
            ifd_num: In::PRIMARY,
            value,
        };
        let fields = [
            field(Tag::Orientation, Value::Short(vec![orientation])),
            field(Tag::Make, ascii("Example Cameras")),
            field(Tag::Model, ascii("Model 1")),
            field(Tag::DateTimeOriginal, ascii("2024:05:06 07:08:09")),
            field(Tag::OffsetTimeOriginal, ascii("+02:00")),
            field(Tag::PixelXDimension, Value::Long(vec![width])),
            field(Tag::PixelYDimension, Value::Long(vec![height])),
            field(Tag::GPSLatitudeRef, ascii("N")),
            field(
                Tag::GPSLatitude,
                Value::Rational(vec![
                    Rational { num: 12, denom: 1 },
                    Rational { num: 34, denom: 1 },
                    Rational { num: 56, denom: 1 },
                ]),
            ),
        ];
        let small = Samples::new(16, 16, 3, photo(16, 16)).unwrap();
        let thumbnail = samples::jpeg(&small, 80, &JpegExtras::default()).unwrap();
        let mut writer = exif::experimental::Writer::new();
        for field in &fields {
            writer.push_field(field);
        }
        writer.set_jpeg(&thumbnail, In::THUMBNAIL);
        let mut out = Cursor::new(Vec::new());
        writer.write(&mut out, little).unwrap();
        out.into_inner()
    }

    /// A colour profile's header saying `space`, and some bytes after it.
    fn profile(space: [u8; 4]) -> Vec<u8> {
        let mut icc = vec![0u8; 200];
        icc[0..4].copy_from_slice(&200u32.to_be_bytes());
        icc[12..16].copy_from_slice(b"mntr");
        icc[16..20].copy_from_slice(&space);
        icc[36..40].copy_from_slice(b"acsp");
        for (index, byte) in icc[128..].iter_mut().enumerate() {
            *byte = index as u8;
        }
        icc
    }

    fn xmp_body() -> Vec<u8> {
        [
            XMP_HEADER,
            br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description tiff:Orientation="6" exif:GPSLatitude="12,34.5N"/></rdf:RDF></x:xmpmeta>"#,
        ]
        .concat()
    }

    /// A JPEG of `data` at `quality`, with what a camera adds.
    fn jpeg_of(
        (width, height): (u32, u32),
        data: Vec<u8>,
        quality: u8,
        exif: Option<&[u8]>,
        icc: Option<&[u8]>,
        xmp: bool,
    ) -> Vec<u8> {
        let samples = Samples::new(width, height, 3, data).unwrap();
        let jpeg = samples::jpeg(&samples, quality, &JpegExtras { icc, exif }).unwrap();
        if xmp {
            with_segments(jpeg, APP1, &[xmp_body()]).unwrap()
        } else {
            jpeg
        }
    }

    fn plain_jpeg(width: u32, height: u32, quality: u8) -> Vec<u8> {
        jpeg_of(
            (width, height),
            photo(width, height),
            quality,
            None,
            None,
            false,
        )
    }

    /// A PNG of `raster`, stored without compression, with `extra` chunks
    /// before its pixels.
    fn png_of(header: Header, raster: &Raster, extra: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut filtered = Vec::new();
        samples::filter_rows(
            &raster.data,
            stride(raster.width, raster.bits),
            usize::from(raster.bits.div_ceil(8)),
            false,
            &mut filtered,
        )
        .unwrap();
        png_from(header, &filtered, extra)
    }

    fn png_from(header: Header, filtered: &[u8], extra: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        chunk(&mut out, *b"IHDR", &header.bytes());
        for (kind, data) in extra {
            chunk(&mut out, **kind, data);
        }
        let mut stored = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
        stored.write_all(filtered).unwrap();
        chunk(&mut out, *b"IDAT", &stored.finish().unwrap());
        chunk(&mut out, *b"IEND", &[]);
        out
    }

    /// A PNG of `data` written by the picture library, quickly (so a copy
    /// packs it tighter), with a profile and EXIF when given.
    fn library_png(
        (width, height): (u32, u32),
        data: &[u8],
        color: image::ExtendedColorType,
        icc: Option<Vec<u8>>,
        exif: Option<Vec<u8>>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = PngEncoder::new_with_quality(
            &mut out,
            CompressionType::Fast,
            image::codecs::png::FilterType::NoFilter,
        );
        if let Some(icc) = icc {
            encoder.set_icc_profile(icc).unwrap();
        }
        if let Some(exif) = exif {
            encoder.set_exif_metadata(exif).unwrap();
        }
        encoder.write_image(data, width, height, color).unwrap();
        out
    }

    fn options(quality: Quality) -> Options {
        Options {
            quality,
            ..Options::default()
        }
    }

    /// What `shrink` makes of `input`: what it says, and the copy written.
    fn shrunk(input: &[u8], options: Options) -> (Option<Written>, Option<Vec<u8>>) {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("in"), dir.path().join("out"));
        std::fs::write(&from, input).unwrap();
        let written = shrink(&from, &to, &options).unwrap();
        let copy = std::fs::read(&to).ok();
        assert_eq!(written.is_some(), copy.is_some());
        if let (Some(written), Some(copy)) = (&written, &copy) {
            assert_eq!(written.bytes, copy.len() as u64);
            assert!(copy.len() < input.len());
        }
        (written, copy)
    }

    fn refused(input: &[u8]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("in"), dir.path().join("out"));
        std::fs::write(&from, input).unwrap();
        let error = shrink(&from, &to, &options(Quality::Screen)).unwrap_err();
        assert!(!to.exists());
        format!("{error:#}")
    }

    fn exif_of(copy: &[u8]) -> Option<exif::Exif> {
        exif::Reader::new()
            .read_from_container(&mut Cursor::new(copy))
            .ok()
    }

    fn field_text(exif: &exif::Exif, tag: Tag) -> Option<String> {
        exif.get_field(tag, In::PRIMARY)
            .map(|field| field.display_value().to_string())
    }

    fn number(exif: &exif::Exif, tag: Tag) -> Option<u32> {
        exif.get_field(tag, In::PRIMARY)
            .and_then(|field| field.value.get_uint(0))
    }

    fn icc_of(copy: &[u8]) -> Option<Vec<u8>> {
        ImageReader::new(Cursor::new(copy))
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap()
            .icc_profile()
            .unwrap()
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    /// The mean of the first channel over a square of `side` at `(x, y)`.
    fn mean(image: &image::RgbImage, (x, y): (u32, u32), side: u32) -> u32 {
        let mut sum = 0;
        for dy in 0..side {
            for dx in 0..side {
                sum += u32::from(image.get_pixel(x + dx, y + dy)[0]);
            }
        }
        sum / (side * side)
    }

    #[test]
    fn picture_qualities_bring_the_longer_side_down() {
        let input = plain_jpeg(4000, 100, 95);
        for (quality, size) in [
            (Quality::Print, (3840, 96)),
            (Quality::Screen, (1920, 48)),
            (Quality::Small, (1280, 32)),
        ] {
            let (written, copy) = shrunk(&input, options(quality));
            let copy = copy.expect("a smaller copy");
            assert_eq!(written.unwrap().met, None);
            assert_eq!(image::guess_format(&copy).unwrap(), ImageFormat::Jpeg);
            let back = image::load_from_memory(&copy).unwrap();
            assert_eq!((back.width(), back.height()), size, "{quality:?}");
        }
        // Without its details left out, there is nothing in a JPEG to
        // change at no loss.
        assert_eq!(shrunk(&input, options(Quality::Lossless)).0, None);
    }

    #[test]
    fn picture_turned_is_made_upright_with_its_details() {
        let (width, height) = (600, 400);
        let mut data = photo(width, height);
        for y in 0..height {
            for x in 0..width {
                let at = (y * width + x) as usize * 3;
                // Its bottom left white, its top right black.
                if x < 60 && y >= height - 60 {
                    data[at..at + 3].copy_from_slice(&[255; 3]);
                } else if x >= width - 60 && y < 60 {
                    data[at..at + 3].copy_from_slice(&[0; 3]);
                }
            }
        }
        let exif = exif_block(false, 6, (width, height));
        let icc = profile(*b"RGB ");
        let input = jpeg_of((width, height), data, 95, Some(&exif), Some(&icc), true);
        let (written, copy) = shrunk(&input, options(Quality::Screen));
        assert_eq!(written.unwrap().notes, Vec::<String>::new());
        let copy = copy.unwrap();
        let back = image::load_from_memory(&copy).unwrap().to_rgb8();
        // Turned a quarter clockwise: its bottom left now its top left.
        assert_eq!(back.dimensions(), (400, 600));
        assert!(mean(&back, (5, 5), 40) > 220);
        assert!(mean(&back, (355, 555), 40) < 35);
        let exif = exif_of(&copy).unwrap();
        assert_eq!(number(&exif, Tag::Orientation), Some(1));
        assert_eq!(
            field_text(&exif, Tag::Make).as_deref(),
            Some("\"Example Cameras\"")
        );
        assert!(exif.get_field(Tag::GPSLatitude, In::PRIMARY).is_some());
        assert!(exif.get_field(Tag::DateTimeOriginal, In::PRIMARY).is_some());
        assert_eq!(number(&exif, Tag::PixelXDimension), Some(400));
        assert_eq!(number(&exif, Tag::PixelYDimension), Some(600));
        // Its thumbnail, sideways now, is gone.
        assert!(exif.fields().all(|field| field.ifd_num == In::PRIMARY));
        assert_eq!(icc_of(&copy), Some(icc));
        assert!(contains(&copy, br#"tiff:Orientation="1""#));
        assert!(contains(&copy, b"exif:GPSLatitude"));
    }

    #[test]
    fn picture_stripped_keeps_only_when_it_was_taken() {
        let (width, height) = (600, 400);
        let exif = exif_block(true, 6, (width, height));
        let icc = profile(*b"RGB ");
        let input = jpeg_of(
            (width, height),
            photo(width, height),
            95,
            Some(&exif),
            Some(&icc),
            true,
        );
        let (_, copy) = shrunk(
            &input,
            Options {
                strip: true,
                ..options(Quality::Screen)
            },
        );
        let copy = copy.unwrap();
        let back = image::load_from_memory(&copy).unwrap();
        assert_eq!((back.width(), back.height()), (400, 600));
        let exif = exif_of(&copy).unwrap();
        for tag in [
            Tag::Make,
            Tag::Model,
            Tag::GPSLatitude,
            Tag::GPSLatitudeRef,
            Tag::Orientation,
        ] {
            assert!(exif.get_field(tag, In::PRIMARY).is_none(), "{tag}");
        }
        assert_eq!(
            field_text(&exif, Tag::DateTimeOriginal).as_deref(),
            Some("2024-05-06 07:08:09")
        );
        assert!(
            exif.get_field(Tag::OffsetTimeOriginal, In::PRIMARY)
                .is_some()
        );
        assert!(exif.fields().all(|field| field.ifd_num == In::PRIMARY));
        assert_eq!(icc_of(&copy), Some(icc));
        assert!(!contains(&copy, b"http://ns.adobe.com/xap"));
        assert!(!contains(&copy, b"Example Cameras"));
    }

    #[test]
    fn picture_lossless_jpeg_keeps_its_scans_byte_for_byte() {
        let (width, height) = (600, 400);
        let exif = exif_block(true, 6, (width, height));
        let icc = profile(*b"RGB ");
        let jpeg = jpeg_of(
            (width, height),
            photo(width, height),
            90,
            Some(&exif),
            Some(&icc),
            true,
        );
        let input = with_segments(jpeg, 0xFE, &[b"a camera's comment".to_vec()]).unwrap();
        let lossless = Options {
            strip: true,
            ..options(Quality::Lossless)
        };
        let (written, copy) = shrunk(&input, lossless);
        let copy = copy.unwrap();
        let scans = |bytes: &[u8]| Jpeg::parse(bytes).unwrap().scans_at;
        assert_eq!(&input[scans(&input)..], &copy[scans(&copy)..]);
        assert!(
            written
                .unwrap()
                .notes
                .iter()
                .any(|note| note.contains("as stored"))
        );
        let exif = exif_of(&copy).unwrap();
        // Its pixels still stored sideways, still turned by its EXIF.
        assert_eq!(number(&exif, Tag::Orientation), Some(6));
        assert!(exif.get_field(Tag::DateTimeOriginal, In::PRIMARY).is_some());
        assert!(exif.get_field(Tag::GPSLatitude, In::PRIMARY).is_none());
        assert!(exif.get_field(Tag::Make, In::PRIMARY).is_none());
        assert_eq!(icc_of(&copy), Some(icc));
        assert!(!contains(&copy, b"http://ns.adobe.com/xap"));
        assert!(!contains(&copy, b"a camera's comment"));
        let back = image::load_from_memory(&copy).unwrap();
        assert_eq!(back, image::load_from_memory(&input).unwrap());
        // A second picture or a video after its end is left out.
        let trailed = [&input[..], b"a video after its end"].concat();
        let (_, copy) = shrunk(&trailed, lossless);
        let copy = copy.unwrap();
        assert!(copy.ends_with(&[0xFF, 0xD9]));
        assert!(!contains(&copy, b"a video"));
        // Nothing to leave out, nothing to write.
        assert_eq!(shrunk(&input, options(Quality::Lossless)).0, None);
        assert_eq!(shrunk(&plain_jpeg(300, 200, 90), lossless).0, None);
    }

    #[test]
    fn picture_kept_as_it_was_when_written_again_it_grows() {
        let (width, height) = (800, 600);
        let exif = exif_block(true, 1, (width, height));
        let input = jpeg_of(
            (width, height),
            photo(width, height),
            50,
            Some(&exif),
            None,
            true,
        );
        let (written, copy) = shrunk(
            &input,
            Options {
                strip: true,
                ..options(Quality::Screen)
            },
        );
        let copy = copy.expect("its details left out");
        let scans = |bytes: &[u8]| Jpeg::parse(bytes).unwrap().scans_at;
        assert_eq!(&input[scans(&input)..], &copy[scans(&copy)..]);
        assert!(written.unwrap().notes[0].contains("kept as they were"));
        // Nothing to leave out: no copy rather than a larger one.
        assert_eq!(shrunk(&input, options(Quality::Screen)).0, None);
    }

    #[test]
    fn picture_cmyk_profile_is_left_out_and_said() {
        let (width, height) = (300, 200);
        let cmyk: Vec<u8> = photo(width, height)
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [255 - pixel[0], 255 - pixel[1], 255 - pixel[2], 0])
            .collect();
        let mut input = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut input, 95);
        encoder.add_icc_profile(&profile(*b"CMYK")).unwrap();
        encoder
            .encode(
                &cmyk,
                width as u16,
                height as u16,
                jpeg_encoder::ColorType::Cmyk,
            )
            .unwrap();
        let (written, copy) = shrunk(&input, options(Quality::Screen));
        let notes = written.unwrap().notes;
        assert!(
            notes.iter().any(|note| note.contains("colour profile")),
            "{notes:?}"
        );
        let copy = copy.unwrap();
        assert_eq!(icc_of(&copy), None);
        let back = image::load_from_memory(&copy).unwrap();
        assert_eq!(back.color(), image::ColorType::Rgb8);
        // Its colours read as its inks stand for (Adobe's inverted CMYK,
        // as the encoder writes it), not as their negative: squares of the
        // copy against the same squares of the picture.
        let back = back.into_rgb8();
        let original = image::RgbImage::from_raw(width, height, photo(width, height)).unwrap();
        let mut differences = Vec::new();
        for y in (0..height).step_by(20) {
            for x in (0..width).step_by(20) {
                differences.push(mean(&back, (x, y), 20).abs_diff(mean(&original, (x, y), 20)));
            }
        }
        assert!(
            differences.iter().all(|&difference| difference < 16),
            "{differences:?}"
        );
    }

    #[test]
    fn picture_jfif_thumbnail_is_left_out() {
        let (width, height) = (300, 200);
        let exif = exif_block(true, 1, (width, height));
        let jpeg = jpeg_of(
            (width, height),
            photo(width, height),
            90,
            Some(&exif),
            None,
            false,
        );
        let parsed = Jpeg::parse(&jpeg).unwrap();
        let jfif = &parsed.header[0];
        assert!(jfif.marker == 0xE0 && jfif.body.starts_with(b"JFIF\0"));
        // JFIF's fixed part, then a thumbnail of 2 × 2 pixels.
        let mut body = jfif.body[..14].to_vec();
        body[12..].copy_from_slice(&[2, 2]);
        body.extend_from_slice(&[0xAB; 12]);
        let mut input = vec![0xFF, 0xD8];
        write_segment(&mut input, 0xE0, &body).unwrap();
        input.extend_from_slice(&jpeg[jfif.at + 4 + jfif.body.len()..]);
        let (_, copy) = shrunk(
            &input,
            Options {
                strip: true,
                ..options(Quality::Lossless)
            },
        );
        let copy = copy.unwrap();
        let kept = Jpeg::parse(&copy).unwrap();
        assert_eq!(
            kept.header[0].body,
            &body[..12]
                .iter()
                .chain(&[0, 0])
                .copied()
                .collect::<Vec<_>>()[..]
        );
        assert!(!contains(&copy, &[0xAB; 12]));
    }

    #[test]
    fn picture_jpeg_with_stray_bytes_between_segments_is_read() {
        let (width, height) = (300, 200);
        let exif = exif_block(true, 1, (width, height));
        let jpeg = jpeg_of(
            (width, height),
            photo(width, height),
            90,
            Some(&exif),
            None,
            false,
        );
        let parsed = Jpeg::parse(&jpeg).unwrap();
        let tables = parsed
            .header
            .iter()
            .find(|segment| segment.marker == 0xDB)
            .unwrap()
            .at;
        let input = [&jpeg[..tables], b"\0\0stray", &jpeg[tables..]].concat();
        let (_, copy) = shrunk(
            &input,
            Options {
                strip: true,
                ..options(Quality::Lossless)
            },
        );
        let copy = copy.unwrap();
        assert!(!contains(&copy, b"stray"));
        assert_eq!(
            image::load_from_memory(&copy).unwrap(),
            image::load_from_memory(&jpeg).unwrap()
        );
    }

    #[test]
    fn picture_not_smaller_is_not_written() {
        let input = plain_jpeg(300, 200, 40);
        assert_eq!(shrunk(&input, options(Quality::Print)), (None, None));
    }

    #[test]
    fn picture_at_most_is_met_or_said_not_to_be() {
        let input = plain_jpeg(1400, 300, 95);
        let small = shrunk(&input, options(Quality::Small)).0.unwrap().bytes;
        let (written, copy) = shrunk(
            &input,
            Options {
                at_most: Some(small - 1),
                ..options(Quality::Screen)
            },
        );
        let written = written.unwrap();
        assert_eq!(written.met, Some(true));
        assert!(written.bytes < small);
        let back = image::load_from_memory(&copy.unwrap()).unwrap();
        assert!(back.width() <= 1024);
        let (written, copy) = shrunk(
            &input,
            Options {
                at_most: Some(100),
                ..options(Quality::Screen)
            },
        );
        assert_eq!(written.unwrap().met, Some(false));
        // The smallest step kept.
        let back = image::load_from_memory(&copy.unwrap()).unwrap();
        assert_eq!((back.width(), back.height()), (640, 137));
    }

    #[test]
    fn picture_at_most_tries_the_qualities_below_first() {
        // Print's copy a little too large: the JPEG quality below it, at
        // its own size, fits, the picture keeping its pixels across.
        let input = plain_jpeg(1400, 300, 95);
        let print = shrunk(&input, options(Quality::Print)).0.unwrap().bytes;
        let (written, copy) = shrunk(
            &input,
            Options {
                at_most: Some(print - 1),
                ..options(Quality::Print)
            },
        );
        assert_eq!(written.unwrap().met, Some(true));
        let back = image::load_from_memory(&copy.unwrap()).unwrap();
        assert_eq!((back.width(), back.height()), (1400, 300));
    }

    #[test]
    fn picture_lossless_loses_nothing_to_fit_a_size() {
        // Wider than the smallest step, which would resize it.
        let (width, height) = (800, 100);
        let data = photo(width, height);
        let input = library_png(
            (width, height),
            &data,
            image::ExtendedColorType::Rgb8,
            None,
            None,
        );
        let (written, copy) = shrunk(
            &input,
            Options {
                at_most: Some(100),
                ..options(Quality::Lossless)
            },
        );
        assert_eq!(written.unwrap().met, Some(false));
        let back = image::load_from_memory(&copy.unwrap()).unwrap();
        assert_eq!(back.into_rgb8().into_raw(), data);
    }

    #[test]
    fn picture_png_keeps_its_transparency() {
        let (width, height) = (1600, 100);
        let rgb = photo(width, height);
        let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
        for (index, pixel) in rgb.as_chunks::<3>().0.iter().enumerate() {
            let x = index as u32 % width;
            rgba.extend_from_slice(pixel);
            rgba.push((x * 255 / (width - 1)) as u8);
        }
        let input = library_png(
            (width, height),
            &rgba,
            image::ExtendedColorType::Rgba8,
            None,
            None,
        );
        let (_, copy) = shrunk(&input, options(Quality::Small));
        let copy = copy.unwrap();
        assert_eq!(image::guess_format(&copy).unwrap(), ImageFormat::Png);
        let back = image::load_from_memory(&copy).unwrap();
        assert_eq!(back.color(), image::ColorType::Rgba8);
        let back = back.to_rgba8();
        assert_eq!(back.dimensions(), (1280, 80));
        assert!(back.get_pixel(0, 40)[3] < 5);
        assert!(back.get_pixel(1279, 40)[3] > 250);
        assert!(back.get_pixel(640, 40)[3].abs_diff(128) < 4);
        let (_, copy) = shrunk(
            &input,
            Options {
                gray: true,
                ..options(Quality::Small)
            },
        );
        let back = image::load_from_memory(&copy.unwrap()).unwrap();
        assert_eq!(back.color(), image::ColorType::La8);
    }

    #[test]
    fn picture_png_keeps_its_profile_and_exif_turned() {
        let (width, height) = (500, 300);
        let icc = profile(*b"RGB ");
        let exif = exif_block(false, 8, (width, height));
        let input = library_png(
            (width, height),
            &photo(width, height),
            image::ExtendedColorType::Rgb8,
            Some(icc.clone()),
            Some(exif),
        );
        let (_, copy) = shrunk(&input, options(Quality::Screen));
        let copy = copy.unwrap();
        let back = image::load_from_memory(&copy).unwrap();
        assert_eq!((back.width(), back.height()), (300, 500));
        let read = exif_of(&copy).unwrap();
        assert_eq!(number(&read, Tag::Orientation), Some(1));
        assert!(read.get_field(Tag::GPSLatitude, In::PRIMARY).is_some());
        assert_eq!(icc_of(&copy), Some(icc.clone()));
        let (_, copy) = shrunk(
            &input,
            Options {
                strip: true,
                gray: true,
                ..options(Quality::Screen)
            },
        );
        let copy = copy.unwrap();
        let back = image::load_from_memory(&copy).unwrap();
        assert_eq!(back.color(), image::ColorType::L8);
        let read = exif_of(&copy).unwrap();
        assert!(read.get_field(Tag::GPSLatitude, In::PRIMARY).is_none());
        assert!(read.get_field(Tag::DateTimeOriginal, In::PRIMARY).is_some());
        // An RGB profile is not a gray picture's.
        assert_eq!(icc_of(&copy), None);
    }

    #[test]
    fn picture_lossless_png_keeps_its_depth_palette_and_pixels() {
        // Four colours, two bits a pixel, the first transparent.
        let (width, height) = (300, 200);
        let mut raster = Raster::new(width, height, 2);
        for y in 0..height {
            for x in 0..width {
                raster.set(x, y, u64::from((x / 10 + y / 7) % 4));
            }
        }
        let header = Header {
            width,
            height,
            depth: 2,
            colour: 3,
            interlaced: false,
        };
        let palette = vec![0, 0, 0, 200, 30, 30, 30, 200, 30, 30, 30, 200];
        let input = png_of(
            header,
            &raster,
            &[
                (b"PLTE", palette.clone()),
                (b"tRNS", vec![0, 255, 255, 255]),
            ],
        );
        let (_, copy) = shrunk(&input, options(Quality::Lossless));
        let copy = copy.unwrap();
        let png = Png::parse(&copy).unwrap();
        assert_eq!(png.header, header);
        let plte = png
            .chunks
            .iter()
            .find(|chunk| &chunk.kind == b"PLTE")
            .unwrap();
        assert_eq!(plte.data, palette);
        assert!(png.chunks.iter().any(|chunk| &chunk.kind == b"tRNS"));
        assert_eq!(png.raster().unwrap(), raster);
        assert_eq!(
            image::load_from_memory(&copy).unwrap(),
            image::load_from_memory(&input).unwrap()
        );

        // Sixteen bits a sample stay sixteen.
        let (width, height) = (200, 150);
        let deep: Vec<u8> = photo(width, height)
            .iter()
            .flat_map(|&sample| [sample, sample / 3])
            .collect();
        let input = library_png(
            (width, height),
            &deep,
            image::ExtendedColorType::Rgb16,
            None,
            None,
        );
        let (_, copy) = shrunk(&input, options(Quality::Lossless));
        let copy = copy.unwrap();
        assert_eq!(Png::parse(&copy).unwrap().header.depth, 16);
        let back = image::load_from_memory(&copy).unwrap();
        assert_eq!(back.color(), image::ColorType::Rgb16);
        assert_eq!(back, image::load_from_memory(&input).unwrap());
        // Re-encoded, they are written in eight.
        let (_, copy) = shrunk(&input, options(Quality::Screen));
        let back = image::load_from_memory(&copy.unwrap()).unwrap();
        assert_eq!(back.color(), image::ColorType::Rgb8);
    }

    #[test]
    fn picture_lossless_png_is_put_together_from_its_passes() {
        let (width, height) = (97, 61);
        let mut raster = Raster::new(width, height, 8);
        for y in 0..height {
            for x in 0..width {
                raster.set(x, y, u64::from((x * 2 + y * 3) % 256));
            }
        }
        // Interlaced (Adam7): each pass a picture of its own, unfiltered.
        let mut passes = Vec::new();
        for (x0, y0, dx, dy) in ADAM7 {
            let mut pass = Vec::new();
            for y in (y0..height).step_by(dy as usize) {
                pass.push(0);
                for x in (x0..width).step_by(dx as usize) {
                    pass.push(raster.get(x, y) as u8);
                }
            }
            if x0 < width {
                passes.extend_from_slice(&pass);
            }
        }
        let header = Header {
            width,
            height,
            depth: 8,
            colour: 0,
            interlaced: true,
        };
        // Its data cut into many small chunks, as some writers do.
        let single = png_from(header, &passes, &[]);
        let png = Png::parse(&single).unwrap();
        let idat = png
            .chunks
            .iter()
            .find(|chunk| &chunk.kind == b"IDAT")
            .unwrap();
        let mut input = PNG_SIGNATURE.to_vec();
        chunk(&mut input, *b"IHDR", &header.bytes());
        for part in idat.data.chunks(7) {
            chunk(&mut input, *b"IDAT", part);
        }
        chunk(&mut input, *b"IEND", &[]);
        let (_, copy) = shrunk(&input, options(Quality::Lossless));
        let copy = copy.unwrap();
        let png = Png::parse(&copy).unwrap();
        assert!(!png.header.interlaced);
        assert_eq!(png.raster().unwrap(), raster);
        assert_eq!(
            image::load_from_memory(&copy).unwrap(),
            image::load_from_memory(&input).unwrap()
        );
    }

    #[test]
    fn picture_lossless_png_turns_pixels_of_any_depth() {
        let (width, height) = (37, 23);
        let mut raster = Raster::new(width, height, 4);
        for y in 0..height {
            for x in 0..width {
                raster.set(x, y, u64::from((x / 3 + (y / 2) * 3) % 16));
            }
        }
        let header = Header {
            width,
            height,
            depth: 4,
            colour: 0,
            interlaced: false,
        };
        for exif_orientation in 2..=8u8 {
            let exif = exif_block(true, u16::from(exif_orientation), (width, height));
            let input = png_of(header, &raster, &[(b"eXIf", exif)]);
            let (_, copy) = shrunk(&input, options(Quality::Lossless));
            let copy = copy.unwrap();
            let orientation = Orientation::from_exif(exif_orientation).unwrap();
            let mut expected = image::load_from_memory(&input).unwrap();
            expected.apply_orientation(orientation);
            assert_eq!(
                image::load_from_memory(&copy).unwrap(),
                expected,
                "{orientation:?}"
            );
            assert_eq!(Png::parse(&copy).unwrap().header.depth, 4);
            assert_eq!(number(&exif_of(&copy).unwrap(), Tag::Orientation), Some(1));
        }
    }

    #[test]
    fn picture_lossless_png_leaves_out_what_it_is_told() {
        let (width, height) = (120, 80);
        let mut raster = Raster::new(width, height, 24);
        for (index, sample) in raster.data.iter_mut().enumerate() {
            *sample = (index / 7 % 200) as u8;
        }
        let header = Header {
            width,
            height,
            depth: 8,
            colour: 2,
            interlaced: false,
        };
        let mut iccp = b"test\0\0".to_vec();
        iccp.extend_from_slice(&samples::deflate(&profile(*b"RGB ")));
        let input = png_of(
            header,
            &raster,
            &[
                (b"iCCP", iccp),
                (b"eXIf", exif_block(false, 1, (width, height))),
                (b"tEXt", b"Comment\0taken at home".to_vec()),
                (
                    b"iTXt",
                    [&b"XML:com.adobe.xmp\0\0\0\0\0"[..], &xmp_body()].concat(),
                ),
                (b"prVt", b"a private chunk".to_vec()),
                (b"iDOT", vec![0; 28]),
            ],
        );
        let kinds = |copy: &[u8]| -> Vec<[u8; 4]> {
            Png::parse(copy)
                .unwrap()
                .chunks
                .iter()
                .map(|chunk| chunk.kind)
                .collect()
        };
        let (_, copy) = shrunk(&input, options(Quality::Lossless));
        let copy = copy.unwrap();
        let kept = kinds(&copy);
        for kind in [b"iCCP", b"eXIf", b"tEXt", b"prVt"] {
            assert!(kept.contains(kind));
        }
        // Offsets into pixels rewritten would lead astray.
        assert!(!kept.contains(b"iDOT"));
        assert!(contains(&copy, br#"tiff:Orientation="1""#));
        assert!(
            exif_of(&copy)
                .unwrap()
                .get_field(Tag::Make, In::PRIMARY)
                .is_some()
        );
        let (_, copy) = shrunk(
            &input,
            Options {
                strip: true,
                ..options(Quality::Lossless)
            },
        );
        let copy = copy.unwrap();
        let kept = kinds(&copy);
        assert!(kept.contains(b"iCCP") && kept.contains(b"eXIf"));
        for kind in [b"tEXt", b"iTXt", b"prVt", b"iDOT"] {
            assert!(!kept.contains(kind));
        }
        let read = exif_of(&copy).unwrap();
        assert!(read.get_field(Tag::Make, In::PRIMARY).is_none());
        assert!(read.get_field(Tag::DateTimeOriginal, In::PRIMARY).is_some());
        assert_eq!(Png::parse(&copy).unwrap().raster().unwrap(), raster);
    }

    /// A pHYs chunk of `x` and `y` pixels a metre.
    fn phys(x: u32, y: u32) -> Vec<u8> {
        [&x.to_be_bytes()[..], &y.to_be_bytes(), &[1]].concat()
    }

    fn chunk_of(copy: &[u8], kind: [u8; 4]) -> Option<&[u8]> {
        Png::parse(copy)
            .unwrap()
            .chunks
            .into_iter()
            .find(|chunk| chunk.kind == kind)
            .map(|chunk| chunk.data)
    }

    #[test]
    fn picture_png_keeps_its_size_on_paper() {
        // Pixels a rule leaves as they are: packed tighter, all that
        // describes them kept.
        let (width, height) = (600, 400);
        let raster = Raster {
            width,
            height,
            bits: 24,
            data: photo(width, height),
        };
        let header = Header {
            width,
            height,
            depth: 8,
            colour: 2,
            interlaced: false,
        };
        let input = png_of(
            header,
            &raster,
            &[(b"pHYs", phys(11_811, 11_811)), (b"sBIT", vec![5, 6, 5])],
        );
        let (written, copy) = shrunk(&input, options(Quality::Screen));
        assert_eq!(written.unwrap().notes, Vec::<String>::new());
        let copy = copy.unwrap();
        assert_eq!(chunk_of(&copy, *b"pHYs"), Some(&phys(11_811, 11_811)[..]));
        assert_eq!(chunk_of(&copy, *b"sBIT"), Some(&[5, 6, 5][..]));
        assert_eq!(Png::parse(&copy).unwrap().raster().unwrap(), raster);

        // Resampled and turned: as many fewer pixels a metre as each side
        // has, the two swapped.
        let (width, height) = (1600, 200);
        let raster = Raster {
            width,
            height,
            bits: 24,
            data: photo(width, height),
        };
        let header = Header {
            width,
            height,
            ..header
        };
        let input = png_of(
            header,
            &raster,
            &[
                (b"pHYs", phys(11_811, 5_906)),
                (b"eXIf", exif_block(true, 6, (width, height))),
            ],
        );
        let (_, copy) = shrunk(&input, options(Quality::Small));
        let copy = copy.unwrap();
        let back = image::load_from_memory(&copy).unwrap();
        assert_eq!((back.width(), back.height()), (160, 1280));
        assert_eq!(chunk_of(&copy, *b"pHYs"), Some(&phys(4_725, 9_449)[..]));
    }

    #[test]
    fn picture_png_exif_after_a_jpeg_header_turns_it() {
        let (width, height) = (60, 40);
        let mut raster = Raster::new(width, height, 8);
        for y in 0..height {
            for x in 0..width {
                raster.set(x, y, u64::from((x * 4 + y) % 256));
            }
        }
        let header = Header {
            width,
            height,
            depth: 8,
            colour: 0,
            interlaced: false,
        };
        let exif = [EXIF_HEADER, &exif_block(true, 6, (width, height))].concat();
        let input = png_of(header, &raster, &[(b"eXIf", exif)]);
        let (_, copy) = shrunk(&input, options(Quality::Lossless));
        let copy = copy.unwrap();
        let back = image::load_from_memory(&copy).unwrap();
        // Turned as the block it keeps says, which says so no more.
        assert_eq!((back.width(), back.height()), (40, 60));
        assert_eq!(number(&exif_of(&copy).unwrap(), Tag::Orientation), Some(1));
    }

    #[test]
    fn picture_date_not_read_leaves_its_turn() {
        // Its date stored as a type EXIF does not have.
        let (width, height) = (300, 200);
        let mut tiff = Tiff::new(exif_block(true, 6, (width, height))).unwrap();
        let ifd0 = tiff.u32_at(4).unwrap() as usize;
        let exif_ifd = tiff
            .entry(ifd0, EXIF_IFD)
            .and_then(|entry| tiff.number(entry))
            .unwrap() as usize;
        for tag in [0x9003, 0x9011] {
            let entry = tiff.entry(exif_ifd, tag).unwrap();
            tiff.put_u16(entry + 2, 99);
        }
        let input = jpeg_of(
            (width, height),
            photo(width, height),
            90,
            Some(&tiff.data),
            None,
            false,
        );
        let (written, copy) = shrunk(
            &input,
            Options {
                strip: true,
                ..options(Quality::Lossless)
            },
        );
        let notes = written.unwrap().notes;
        assert!(notes.iter().any(|note| note.contains("date")), "{notes:?}");
        let exif = exif_of(&copy.unwrap()).unwrap();
        // Its pixels kept as stored, still turned by its EXIF.
        assert_eq!(number(&exif, Tag::Orientation), Some(6));
        assert!(exif.get_field(Tag::DateTimeOriginal, In::PRIMARY).is_none());
    }

    #[test]
    fn picture_png_data_is_read_and_written_across_chunks() {
        let data: Vec<u8> = (0..IDAT_SIZE * 5 / 2).map(|i| (i % 251) as u8).collect();
        let mut out = Vec::new();
        let mut idat = Idat {
            out: &mut out,
            pending: Vec::new(),
        };
        for part in data.chunks(100_003) {
            idat.write_all(part).unwrap();
        }
        idat.end();
        let png_header = Header {
            width: 1,
            height: 1,
            depth: 8,
            colour: 0,
            interlaced: false,
        };
        let mut file = PNG_SIGNATURE.to_vec();
        chunk(&mut file, *b"IHDR", &png_header.bytes());
        file.extend_from_slice(&out);
        let png = Png::parse(&file).unwrap();
        let sizes: Vec<usize> = png.chunks[1..]
            .iter()
            .map(|chunk| chunk.data.len())
            .collect();
        assert_eq!(sizes, [IDAT_SIZE, IDAT_SIZE, IDAT_SIZE / 2]);
        let mut read = Vec::new();
        Idats {
            chunks: png.chunks.iter(),
            current: &[],
        }
        .read_to_end(&mut read)
        .unwrap();
        assert_eq!(read, data);
    }

    #[test]
    fn picture_exif_is_patched_in_either_byte_order() {
        for little in [true, false] {
            let block = exif_block(little, 6, (600, 400));
            let patched = told(&block, true, (400, 600));
            assert!(patched.len() < block.len(), "its thumbnail cut");
            let read = exif::Reader::new().read_raw(patched).unwrap();
            assert_eq!(read.little_endian(), little);
            assert_eq!(number(&read, Tag::Orientation), Some(1));
            assert_eq!(number(&read, Tag::PixelXDimension), Some(400));
            assert_eq!(number(&read, Tag::PixelYDimension), Some(600));
            assert!(read.get_field(Tag::GPSLatitude, In::PRIMARY).is_some());
            assert!(read.fields().all(|field| field.ifd_num == In::PRIMARY));
            // Not turned, its thumbnail stays.
            let kept = told(&block, false, (600, 400));
            assert_eq!(kept, block);
        }
    }

    #[test]
    fn picture_not_jpeg_or_png_is_refused() {
        assert!(refused(b"GIF89a\x01\x00\x01\x00\x00\x00\x00;").contains("Only JPEG and PNG"));
        let header = Header {
            width: 4,
            height: 4,
            depth: 8,
            colour: 0,
            interlaced: false,
        };
        let raster = Raster::new(4, 4, 8);
        let animated = png_of(header, &raster, &[(b"acTL", vec![0, 0, 0, 2, 0, 0, 0, 0])]);
        assert!(refused(&animated).contains("animated"));
        // Its header alone says how large it is.
        let huge = Header {
            width: 20_000,
            height: 20_000,
            ..header
        };
        assert!(refused(&png_from(huge, &[0; 16], &[])).contains("megapixels"));
    }
}
