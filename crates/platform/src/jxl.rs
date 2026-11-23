//! JPEG XL, read by jxl-oxide (ADR-148, amended): taught to `image` as a
//! decoder of Skwir's own (`picture::formats`), which reads a picture in
//! bands of its rows, and a region of it alone.
//!
//! jxl-oxide draws a frame in 32-bit floats: decoded whole, a photo of 24
//! megapixels held 635 MiB and one of 45 megapixels 1.18 GiB, past what
//! `image` lets a decode hold (512 MiB), so neither was shown. Read in
//! bands as tall as that allows, a decode holds one band's floats beside
//! the picture's eight bits: 45 megapixels in 400 to 510 MiB, in about
//! the same time (each band decodes again the groups beside it, which its
//! filters read). A region (what a zoom shows past what is held whole)
//! decodes the groups of 256 × 256 pixels it covers and those beside
//! them: 2000 × 1600 of 45 megapixels in 150 to 320 ms, where the whole
//! took 1.8 s or more.
//!
//! jxl-oxide turns the pixels as the codestream says. A JPEG converted to
//! JPEG XL keeps its EXIF, which may say so too: it is not read again for
//! it, as `image` did, which turned such a picture twice.
use std::io::Read;

use image::error::{DecodingError, ImageFormatHint};
use image::metadata::Orientation;
use image::{
    ColorType, DynamicImage, ImageDecoder, ImageDecoderRect, ImageError, ImageResult, RgbImage,
};
use jxl_frame::data::TocGroupKind;
use jxl_frame::header::{Encoding, FrameType};
use jxl_oxide::color::{ColourEncoding, ColourSpace, EnumColourEncoding, RenderingIntent};
use jxl_oxide::integration::JxlDecoder;
use jxl_oxide::{InitializeResult, JxlImage, JxlThreadPool};

/// A codestream's first bytes.
const CODESTREAM: [u8; 2] = [0xFF, 0x0A];
/// A container's first box.
const CONTAINER: [u8; 12] = *b"\x00\x00\x00\x0cJXL \x0d\x0a\x87\x0a";

/// What a band holds a sample of it: jxl-oxide's planes in 32-bit floats,
/// the frame's and its filters', measured at 8 bytes, and half as much
/// again for what is not counted (a band decoded narrower than the
/// picture holds the groups around it too).
const BAND_SAMPLE_BYTES: u64 = 12;
/// A band's rows are a multiple of a group's side (256, as encoders write
/// one unless told otherwise): a group across two bands would be decoded
/// for each.
const GROUP: u32 = 256;
/// What a decode may hold where nothing says: `image`'s own default.
const HELD: u64 = 512 << 20;

/// Teach `image` the format. Called once, by `picture::formats`.
pub(crate) fn register() {
    image::hooks::register_decoding_hook(
        "jxl".into(),
        Box::new(|reader| Ok(Box::new(Decoder::new(reader)?))),
    );
    image::hooks::register_format_detection_hook("jxl".into(), &CODESTREAM, None);
    image::hooks::register_format_detection_hook("jxl".into(), &CONTAINER, None);
}

/// Whether `start`, a file's first bytes, begins a JPEG XL.
pub(crate) fn starts(start: &[u8]) -> bool {
    start.starts_with(&CODESTREAM) || start.starts_with(&CONTAINER)
}

/// What a JPEG XL's eighth may hold: its samples in floats, three to a
/// pixel, at most this many pixels, an eighth of the largest picture any
/// preview decodes (`WHOLE_PICTURE_PIXELS`, 128 Mi pixels: 2 Mi samples,
/// about 60 MiB with the coefficients beside them). One larger is left to
/// a whole decode, which its limits refuse: a header saying 1 Gi pixels,
/// which a file of a kilobyte can, cost half a gigabyte for a thumbnail.
const EIGHTH_PIXELS: u64 = crate::inspection::limits::WHOLE_PICTURE_PIXELS / 64;
/// What is read of a file at a time while looking for its eighth.
const CHUNK: usize = 64 << 10;

/// A JPEG XL's eighth, turned as it is shown, where `wanted` takes its
/// longer side (ADR-148 amended): the LF image a lossy picture stores
/// before its details, each sample the mean of 8 × 8 of its pixels, read
/// alone (the file's first part, about a tenth of it) and in the colours
/// its whole decodes to. As a JPEG's DC scans do, it stands for the
/// picture while the rest is decoded, and is the picture shown, shrunk,
/// where it covers what is asked (a thumbnail, a small preview): tens of
/// milliseconds of decoding, a few megabytes held, where the whole of 24
/// megapixels takes a second and 400 MiB. `None` where the picture has
/// none to read alone (a lossless
/// one, one with alpha or other extra channels, an animation, one drawn
/// from several frames, patches or splines, which its LF lacks), or
/// where `wanted` refuses it.
pub(crate) fn eighth<R: Read>(
    reader: &mut R,
    wanted: impl FnOnce(u32) -> bool,
    threads: crate::picture::Threads,
) -> ImageResult<Option<DynamicImage>> {
    let mut read = Feed::new(reader);
    let mut uninit = JxlImage::builder()
        .pool(JxlThreadPool::none())
        .build_uninit();
    let mut image = loop {
        if !read.more(|bytes| uninit.feed_bytes(bytes))? {
            return Ok(None);
        }
        match uninit.try_init().map_err(failed)? {
            InitializeResult::NeedMoreData(more) => uninit = more,
            InitializeResult::Initialized(image) => break image,
        }
    };
    let header = image.image_header();
    let (width, height) = (header.size.width, header.size.height);
    let metadata = &header.metadata;
    let (lf_width, lf_height) = (width.div_ceil(8), height.div_ceil(8));
    if !metadata.ec_info.is_empty()
        || metadata.animation.is_some()
        || u64::from(lf_width) * u64::from(lf_height) > EIGHTH_PIXELS
        || !wanted(lf_width.max(lf_height))
    {
        return Ok(None);
    }
    // The first frame's header, then its LF: read on until it is.
    loop {
        if let Some(frame) = image.frame(0) {
            if !lf_alone(frame, (width, height)) {
                return Ok(None);
            }
            if lf_read(frame) {
                break;
            }
        }
        if !read.more(|bytes| image.feed_bytes(bytes))? {
            return Ok(None);
        }
    }
    let header = image.image_header();
    let Some(frame) = image.frame(0) else {
        return Ok(None);
    };
    let Some(mut planes) = lf_planes(frame, (lf_width, lf_height), threads)? else {
        return Ok(None);
    };
    let metadata = &header.metadata;
    let samples = (lf_width * lf_height) as usize;
    let [x, y, b] = &mut planes;
    let channels = if metadata.xyb_encoded {
        // What its whole decodes to: the colours it names, or sRGB where
        // it names them by an ICC profile.
        let target = match &metadata.colour_encoding {
            ColourEncoding::Enum(own)
                if !matches!(own.colour_space, ColourSpace::Xyb | ColourSpace::Unknown) =>
            {
                own.clone()
            }
            _ => EnumColourEncoding::srgb(RenderingIntent::Relative),
        };
        jxl_color::ColorTransform::xyb_to_enum(
            RenderingIntent::Perceptual,
            &target,
            &metadata.opsin_inverse_matrix,
            &metadata.tone_mapping,
        )
        .and_then(|transform| transform.run(&mut [&mut x[..], &mut y[..], &mut b[..]]))
        .map_err(failed)?
    } else {
        // A JPEG recompressed: its YCbCr, as Cb, Y, Cr.
        jxl_color::ycbcr_to_rgb([&mut x[..], &mut y[..], &mut b[..]]);
        3
    };
    let eight = |sample: f32| (sample.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut picture = if channels == 1 {
        DynamicImage::ImageLuma8(
            image::GrayImage::from_raw(lf_width, lf_height, x.iter().map(|&s| eight(s)).collect())
                .ok_or_else(|| failed("the eighth's size"))?,
        )
    } else {
        let mut pixels = Vec::with_capacity(samples * 3);
        for ((&r, &g), &b) in x.iter().zip(y.iter()).zip(b.iter()) {
            pixels.extend([eight(r), eight(g), eight(b)]);
        }
        DynamicImage::ImageRgb8(
            RgbImage::from_raw(lf_width, lf_height, pixels)
                .ok_or_else(|| failed("the eighth's size"))?,
        )
    };
    let turn = u8::try_from(metadata.orientation).ok();
    picture.apply_orientation(
        turn.and_then(Orientation::from_exif)
            .unwrap_or(Orientation::NoTransforms),
    );
    Ok(Some(picture))
}

/// A file read a chunk at a time, what a decoder took of it let go.
struct Feed<'r, R: Read> {
    reader: &'r mut R,
    buffer: Vec<u8>,
    held: usize,
}

impl<'r, R: Read> Feed<'r, R> {
    fn new(reader: &'r mut R) -> Feed<'r, R> {
        Feed {
            reader,
            buffer: vec![0; CHUNK],
            held: 0,
        }
    }

    /// Read on, and hand what is held to `feed`, which says how much it
    /// took; false at the end of the file.
    fn more(&mut self, feed: impl FnOnce(&[u8]) -> jxl_oxide::Result<usize>) -> ImageResult<bool> {
        if self.held == self.buffer.len() {
            self.buffer.resize(self.buffer.len() + CHUNK, 0);
        }
        let count = self.reader.read(&mut self.buffer[self.held..])?;
        if count == 0 {
            return Ok(false);
        }
        self.held += count;
        let taken = feed(&self.buffer[..self.held]).map_err(failed)?;
        self.buffer.copy_within(taken..self.held, 0);
        self.held -= taken;
        Ok(true)
    }
}

/// Whether `frame`, a picture's first, is all of it and its LF image
/// what it shows, scaled: a lossy (`VarDCT`) frame of the picture's size,
/// alone, in XYB or a JPEG's YCbCr, with no LF frame of its own, no
/// patches or splines drawn over it.
fn lf_alone(frame: &jxl_oxide::Frame, (width, height): (u32, u32)) -> bool {
    let header = frame.header();
    header.encoding == Encoding::VarDct
        && header.frame_type == FrameType::RegularFrame
        && header.is_last
        && !header.flags.use_lf_frame()
        && !header.flags.patches()
        && !header.flags.splines()
        && header.upsampling == 1
        && header.x0 == 0
        && header.y0 == 0
        && (header.width, header.height) == (width, height)
        && (frame.image_header().metadata.xyb_encoded || header.do_ycbcr)
        && !frame.toc().is_single_entry()
}

/// Whether `frame`'s LF global and LF groups are all read.
fn lf_read(frame: &jxl_oxide::Frame) -> bool {
    frame.toc().iter_bitstream_order().all(|group| {
        !matches!(
            group.kind,
            TocGroupKind::LfGlobal | TocGroupKind::LfGroup(_)
        ) || frame
            .data(group.kind)
            .is_some_and(|bytes| bytes.len() == group.size as usize)
    })
}

/// `frame`'s LF image, `size` samples of three planes (X, Y, B, or a
/// JPEG's Cb, Y, Cr), dequantized as jxl-render does, chroma from luma
/// applied, its subsampled chroma (a JPEG's) spread over what it covers.
/// Its adaptive smoothing is left out: it smooths what is enlarged from
/// the LF, and at its own scale each sample is its block's mean already.
/// Its LF groups (2048 × 2048 pixels each) are decoded on a thread each,
/// four at most, where `threads` allows.
fn lf_planes(
    frame: &jxl_oxide::Frame,
    (lf_width, lf_height): (u32, u32),
    threads: crate::picture::Threads,
) -> ImageResult<Option<[Vec<f32>; 3]>> {
    let header = frame.header();
    let Some(global) = frame.try_parse_lf_global::<i32>() else {
        return Ok(None);
    };
    let global = global.map_err(failed)?;
    let Some(vardct) = global.vardct.as_ref() else {
        return Ok(None);
    };
    let Some(coefficients) = lf_coefficients(frame, &global, threads)? else {
        return Ok(None);
    };
    let quantizer = &vardct.quantizer;
    let dequant = &global.lf_dequant;
    // The scale of each plane's samples, as `copy_lf_dequant` has it.
    let scale = |multiplier: f32, extra_precision: u8| {
        let precision = f64::from(1u32 << (9 - u32::from(extra_precision.min(3))));
        let inverse = f64::from(quantizer.global_scale) * f64::from(quantizer.quant_lf);
        (f64::from(multiplier) * precision / inverse) as f32
    };
    let upsampling = header.jpeg_upsampling;
    // Each plane's subsampling, across and down, as jxl-modular reads
    // `jpeg_upsampling` (X, Y, B).
    let across = upsampling.iter().any(|&u| u == 1 || u == 2);
    let down = upsampling.iter().any(|&u| u == 1 || u == 3);
    let shifts = upsampling.map(|u| match u {
        0 => (u32::from(across), u32::from(down)),
        2 => (0, u32::from(down)),
        3 => (u32::from(across), 0),
        _ => (0, 0),
    });
    let subsampled = upsampling.iter().any(|&u| u != 0);
    let (width, height) = (lf_width as usize, lf_height as usize);
    // Each plane at its own size: a subsampled one at half the LF's.
    let sizes = shifts.map(|(x, y)| (width.div_ceil(1 << x), height.div_ceil(1 << y)));
    let mut planes = sizes.map(|(across, down)| vec![0f32; across * down]);
    let group_dim = header.group_dim() as usize;
    let per_row = header.lf_groups_per_row() as usize;
    for (index, coeff) in coefficients.iter().enumerate() {
        let Some(lf_quant) = coeff.lf_quant.image() else {
            return Ok(None);
        };
        // Stored Y, X, B; laid out X, Y, B.
        let stored = lf_quant.image_channels();
        let (left, top) = (index % per_row * group_dim, index / per_row * group_dim);
        for (plane, (channel, multiplier)) in [
            (1, dequant.m_x_lf),
            (0, dequant.m_y_lf),
            (2, dequant.m_b_lf),
        ]
        .into_iter()
        .enumerate()
        {
            let Some(grid) = stored.get(channel) else {
                return Ok(None);
            };
            let factor = scale(multiplier, coeff.extra_precision);
            let (shift_x, shift_y) = shifts[plane];
            let (across, down) = sizes[plane];
            let (left, top) = (left >> shift_x, top >> shift_y);
            let rows = grid.height().min(down.saturating_sub(top));
            let columns = grid.width().min(across.saturating_sub(left));
            let samples = grid.buf();
            for row in 0..rows {
                let from = &samples[row * grid.width()..][..columns];
                let to = &mut planes[plane][(top + row) * across + left..][..columns];
                for (sample, &quantized) in to.iter_mut().zip(from) {
                    *sample = quantized as f32 * factor;
                }
            }
        }
    }
    // A subsampled plane spread over the LF as a JPEG's decoder spreads
    // chroma: each sample three quarters its own, a quarter its
    // neighbour's on the side it lies.
    for (plane, &(shift_x, shift_y)) in planes.iter_mut().zip(&shifts) {
        if (shift_x, shift_y) != (0, 0) {
            *plane = spread(
                plane,
                (width.div_ceil(1 << shift_x), shift_x),
                (width, height),
                shift_y,
            );
        }
    }
    if !subsampled {
        // Chroma from luma, as `chroma_from_luma_lf` has it.
        let correlation = &vardct.lf_chan_corr;
        let colour = correlation.colour_factor as f32;
        let factor = |stored: u32| (f64::from(stored) - 128.0) as f32 / colour;
        let kx = correlation.base_correlation_x + factor(correlation.x_factor_lf);
        let kb = correlation.base_correlation_b + factor(correlation.b_factor_lf);
        let [x, y, b] = &mut planes;
        for ((x, &y), b) in x.iter_mut().zip(y.iter()).zip(b.iter_mut()) {
            *x += kx * y;
            *b += kb * y;
        }
    }
    Ok(Some(planes))
}

/// The quantized LF of each of `frame`'s LF groups, in their order, and
/// nothing else of them (what the details need beside it, read by
/// `try_parse_lf_group` too, took a third of the time). `None` where one is
/// not all there.
fn lf_coefficients(
    frame: &jxl_oxide::Frame,
    global: &jxl_frame::data::LfGlobal<i32>,
    threads: crate::picture::Threads,
) -> ImageResult<Option<Vec<jxl_vardct::LfCoeff<i32>>>> {
    use jxl_oxide_common::Bundle;
    let header = frame.header();
    let ma = global.gmodular.ma_config();
    let count = header.num_lf_groups();
    let pool = JxlThreadPool::none();
    let decode = |index: u32| -> ImageResult<Option<jxl_vardct::LfCoeff<i32>>> {
        let Some(bytes) = frame.data(TocGroupKind::LfGroup(index)) else {
            return Ok(None);
        };
        let (lf_width, lf_height) = header.lf_group_size_for(index);
        let params = jxl_vardct::LfCoeffParams {
            lf_group_idx: index,
            lf_width,
            lf_height,
            jpeg_upsampling: header.jpeg_upsampling,
            bits_per_sample: header.bit_depth.bits_per_sample(),
            global_ma_config: ma,
            allow_partial: false,
            tracker: None,
            pool: &pool,
        };
        let coeff = jxl_vardct::LfCoeff::parse(&mut jxl_bitstream::Bitstream::new(bytes), params)
            .map_err(failed)?;
        Ok((!coeff.partial).then_some(coeff))
    };
    let workers = match threads {
        crate::picture::Threads::Caller => 1,
        crate::picture::Threads::Components => std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(4)
            .min(count as usize),
    };
    if workers <= 1 {
        return (0..count)
            .map(decode)
            .collect::<ImageResult<Option<Vec<_>>>>();
    }
    let next = std::sync::atomic::AtomicU32::new(0);
    let mut decoded = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut own = Vec::new();
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if index >= count {
                            return own;
                        }
                        own.push((index, decode(index)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_default())
            .collect::<Vec<_>>()
    });
    if decoded.len() != count as usize {
        return Ok(None);
    }
    decoded.sort_unstable_by_key(|(index, _)| *index);
    decoded
        .into_iter()
        .map(|(_, coeff)| coeff)
        .collect::<ImageResult<Option<Vec<_>>>>()
}

/// `plane`, `across` samples wide, its columns halved where `shift_x` is 1
/// and its rows where `shift_y` is, spread over `size` samples: each one
/// three quarters the nearest of `plane`'s, a quarter the next on its side.
fn spread(
    plane: &[f32],
    (across, shift_x): (usize, u32),
    (width, height): (usize, usize),
    shift_y: u32,
) -> Vec<f32> {
    let down = plane.len() / across.max(1);
    // The two samples of `plane` along a side of `length` sample `at`
    // takes, the nearest first.
    let pair = |at: usize, shift: u32, length: usize| {
        let own = (at >> shift).min(length - 1);
        if shift == 0 {
            return (own, own);
        }
        let other = if at.is_multiple_of(2) {
            own.saturating_sub(1)
        } else {
            (own + 1).min(length - 1)
        };
        (own, other)
    };
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        let (near_row, far_row) = pair(y, shift_y, down);
        for x in 0..width {
            let (near, far) = pair(x, shift_x, across);
            let row = |r: usize| 0.75 * plane[r * across + near] + 0.25 * plane[r * across + far];
            out.push(0.75 * row(near_row) + 0.25 * row(far_row));
        }
    }
    out
}

/// A decoding error of jxl-oxide's.
fn failed(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Name("JPEG XL".into()),
        error,
    ))
}

/// A JPEG XL picture, or the region of it `picture::Regional::cut` says,
/// read in bands.
pub(crate) struct Decoder<R: Read> {
    inner: JxlDecoder<R>,
    /// What is read (left, top, width, height, as shown): the whole
    /// unless cut.
    region: [u32; 4],
    /// What the decode may hold, in bytes, as its limits say.
    held: u64,
}

impl<R: Read> Decoder<R> {
    /// The picture `reader` reads, its header read.
    pub(crate) fn new(reader: R) -> ImageResult<Decoder<R>> {
        let inner = JxlDecoder::new(reader)?;
        let (width, height) = inner.dimensions();
        Ok(Decoder {
            inner,
            region: [0, 0, width, height],
            held: HELD,
        })
    }

    /// A band's rows: as many as the decode may hold of its width, a
    /// multiple of a group's side, at least one group.
    fn rows(&self) -> u32 {
        let width = u64::from(self.region[2].max(1));
        let samples = u64::from(self.inner.color_type().channel_count());
        let rows = self.held / (width * samples * BAND_SAMPLE_BYTES);
        (u32::try_from(rows).unwrap_or(u32::MAX) / GROUP * GROUP).max(GROUP)
    }
}

impl<R: Read> crate::picture::Regional for Decoder<R> {
    fn cut(&mut self, region: [u32; 4]) {
        self.region = crate::picture::inside(region, self.inner.dimensions());
    }

    fn region(&self) -> [u32; 4] {
        self.region
    }
}

impl<R: Read> ImageDecoder for Decoder<R> {
    fn dimensions(&self) -> (u32, u32) {
        (self.region[2], self.region[3])
    }

    fn color_type(&self) -> ColorType {
        self.inner.color_type()
    }

    fn icc_profile(&mut self) -> ImageResult<Option<Vec<u8>>> {
        self.inner.icc_profile()
    }

    fn exif_metadata(&mut self) -> ImageResult<Option<Vec<u8>>> {
        self.inner.exif_metadata()
    }

    /// None: jxl-oxide has turned the pixels as the codestream says, which
    /// is what counts (an EXIF Orientation beside it said it again).
    fn orientation(&mut self) -> ImageResult<Orientation> {
        Ok(Orientation::NoTransforms)
    }

    fn set_limits(&mut self, limits: image::Limits) -> ImageResult<()> {
        self.held = limits.max_alloc.unwrap_or(HELD);
        self.inner.set_limits(limits)
    }

    fn read_image(mut self, buf: &mut [u8]) -> ImageResult<()> {
        let [left, top, width, height] = self.region;
        let pitch = width as usize * usize::from(self.color_type().bytes_per_pixel());
        let rows = self.rows();
        let bottom = top + height;
        // Bands from the group the region begins in, each but the first
        // and the last a band's rows.
        let mut band_end = top / GROUP * GROUP;
        let mut from = top;
        while from < bottom {
            band_end += rows;
            let to = band_end.min(bottom);
            let (start, end) = ((from - top) as usize * pitch, (to - top) as usize * pitch);
            self.inner
                .read_rect(left, from, width, to - from, &mut buf[start..end], pitch)?;
            from = to;
        }
        Ok(())
    }

    fn read_image_boxed(self: Box<Self>, buf: &mut [u8]) -> ImageResult<()> {
        (*self).read_image(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::{Regional, Threads};

    const PLASMA: &[u8] = include_bytes!("../testdata/synthetic-plasma.jxl");
    /// A JPEG of 520 × 300 in 4:2:0 recompressed, turned a quarter
    /// clockwise (EXIF Orientation 6): its YCbCr, its chroma subsampled.
    const RECOMPRESSED: &[u8] = include_bytes!("../testdata/synthetic-plasma-420-turned.jxl");
    const LOSSLESS: &[u8] = include_bytes!("../testdata/synthetic-lossless.jxl");

    /// The mean of each 8 × 8 block of `picture`, its last ones what is
    /// left past the last eighth.
    fn by_eight(picture: &image::RgbImage) -> image::RgbImage {
        let (width, height) = picture.dimensions();
        image::RgbImage::from_fn(width.div_ceil(8), height.div_ceil(8), |x, y| {
            let mut sum = [0u32; 3];
            let mut count = 0;
            for row in y * 8..(y * 8 + 8).min(height) {
                for column in x * 8..(x * 8 + 8).min(width) {
                    let pixel = picture.get_pixel(column, row).0;
                    for channel in 0..3 {
                        sum[channel] += u32::from(pixel[channel]);
                    }
                    count += 1;
                }
            }
            image::Rgb(sum.map(|total| ((total + count / 2) / count) as u8))
        })
    }

    /// A lossy picture's eighth is its whole by eight, each sample its
    /// block's mean, in the colours its whole decodes to: one in XYB, a
    /// JPEG recompressed with its chroma subsampled, turned as it is
    /// shown. Its light is the mean's within a level; its colour a few
    /// levels off where a JPEG's chroma covers 16 × 16 pixels a sample (in
    /// 4:4:4, the same JPEG's eighth is the mean's within 0.13).
    #[test]
    fn a_jpeg_xl_s_eighth_is_its_whole_by_eight() {
        for (name, bytes, size) in [
            ("XYB", PLASMA, (80, 163)),
            ("a JPEG recompressed, turned", RECOMPRESSED, (38, 65)),
        ] {
            let eighth = eighth(
                &mut std::io::Cursor::new(bytes),
                |_| true,
                Threads::Components,
            )
            .unwrap()
            .unwrap_or_else(|| panic!("{name}: an eighth"))
            .to_rgb8();
            assert_eq!(eighth.dimensions(), size, "{name}");
            let whole = image::DynamicImage::from_decoder(
                Decoder::new(std::io::Cursor::new(bytes)).unwrap(),
            )
            .unwrap();
            // Blocks as stored: the recompressed one is shown turned a
            // quarter clockwise, its eighths counted from its stored top.
            let expected = if bytes == RECOMPRESSED {
                image::imageops::rotate90(&by_eight(&whole.rotate270().to_rgb8()))
            } else {
                by_eight(&whole.to_rgb8())
            };
            let light = |pixel: &image::Rgb<u8>| {
                let [r, g, b] = pixel.0.map(f64::from);
                0.299 * r + 0.587 * g + 0.114 * b
            };
            let (mut colour, mut lights, mut most) = (0u64, 0f64, 0u8);
            for (got, wanted) in eighth.pixels().zip(expected.pixels()) {
                lights += (light(got) - light(wanted)).abs();
                for channel in 0..3 {
                    let gap = got.0[channel].abs_diff(wanted.0[channel]);
                    colour += u64::from(gap);
                    most = most.max(gap);
                }
            }
            let samples = f64::from(size.0 * size.1);
            let (colour, lights) = (colour as f64 / samples / 3.0, lights / samples);
            assert!(
                lights < 1.0 && colour < 3.5 && most < 32,
                "{name}: light {lights:.2}, colour {colour:.2} on average, {most} at most"
            );
        }
    }

    /// What has no eighth to read alone, or one `wanted` refuses, has
    /// none.
    #[test]
    fn a_jpeg_xl_without_an_eighth_has_none() {
        let read =
            |bytes| eighth(&mut std::io::Cursor::new(bytes), |_| true, Threads::Caller).unwrap();
        assert!(read(LOSSLESS).is_none(), "lossless");
        assert!(read(PLASMA).is_some());
        let refused = eighth(
            &mut std::io::Cursor::new(PLASMA),
            |longer| {
                assert_eq!(longer, 163);
                false
            },
            Threads::Caller,
        );
        assert!(refused.unwrap().is_none(), "refused");
        // A file cut short of its LF.
        assert!(read(&PLASMA[..2000]).is_none(), "cut short");
    }

    /// A band is as tall as what may be held allows, in groups.
    #[test]
    fn a_band_is_as_tall_as_what_may_be_held() {
        let mut decoder = Decoder::new(std::io::Cursor::new(PLASMA)).unwrap();
        assert_eq!(decoder.dimensions(), (640, 1300));
        // 640 pixels of 3 samples, 12 bytes each: 23 040 bytes a row.
        decoder.held = 23_040 * 600;
        assert_eq!(decoder.rows(), 512);
        decoder.held = 1000;
        assert_eq!(decoder.rows(), GROUP, "never less than a group");
        decoder.cut([600, 1200, 500, 500]);
        assert_eq!(decoder.region(), [600, 1200, 40, 100], "within the picture");
    }

    /// A picture read band by band is the picture read whole, to the bit,
    /// and a region of it read alone is the whole's, cut.
    #[test]
    fn a_jpeg_xl_in_bands_is_its_whole() {
        let read = |held: u64, region: Option<[u32; 4]>| {
            let mut decoder = Decoder::new(std::io::Cursor::new(PLASMA)).unwrap();
            decoder.held = held;
            if let Some(region) = region {
                decoder.cut(region);
            }
            image::DynamicImage::from_decoder(decoder)
                .unwrap()
                .to_rgb8()
        };
        let whole = read(HELD, None);
        assert_eq!(whole.dimensions(), (640, 1300));
        // Bands of a group: six of them, the last short.
        assert!(read(1, None) == whole, "bands differ from the whole");
        for region in [[0, 0, 100, 40], [300, 250, 200, 600], [520, 1100, 120, 200]] {
            let [x, y, width, height] = region;
            let cut = image::imageops::crop_imm(&whole, x, y, width, height).to_image();
            assert!(read(1, Some(region)) == cut, "{region:?} differs");
        }
    }
}
