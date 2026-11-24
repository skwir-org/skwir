//! A TIFF's region read from the strips or tiles it meets (ADR-146
//! amended): what a zoom past what is held of a picture shows
//! (`picture::decode_region`). A TIFF stores its pixels in chunks, each
//! compressed alone and found by its offset, so only those the region
//! meets are read, one at a time, and the region's part of each kept: a
//! region of a scan in strips holds the region and a strip, never the
//! picture. Its pixels are the ones `image`'s decoder draws (the same
//! `tiff`, each chunk expanded as it expands them for the whole).
//!
//! Only what `image` takes as `tiff` gives it is read so: one plane,
//! eight or sixteen bits unsigned, or 32-bit floats in colour. What
//! `image` draws otherwise than it is stored (a bit a pixel spread to
//! eight, CMYK made RGB, planes stored apart and interleaved) is decoded
//! whole, as is what it refuses, which then says why ([`by_chunks`]).
use std::io::{BufRead, Read, Seek, SeekFrom};

use image::error::{
    DecodingError, ImageFormatHint, LimitError, LimitErrorKind, UnsupportedError,
    UnsupportedErrorKind,
};
use image::metadata::Orientation;
use image::{ColorType, ImageDecoder, ImageError, ImageFormat, ImageResult};
use tiff::decoder::Decoder as Tiff;
use tiff::tags::{SampleFormat, Tag};

/// Whether the TIFF `reader` reads (its first picture, as `image` reads)
/// is laid out as [`Decoder`] reads it, `reader` back where it was: what
/// is not is decoded whole by `image`.
pub(crate) fn by_chunks<R: BufRead + Seek>(reader: &mut R) -> ImageResult<bool> {
    let start = reader.stream_position().map_err(ImageError::IoError)?;
    let read = Tiff::new(&mut *reader)
        .ok()
        .and_then(|mut tiff| layout(&mut tiff))
        .is_some();
    reader
        .seek(SeekFrom::Start(start))
        .map_err(ImageError::IoError)?;
    Ok(read)
}

/// The colours `tiff` hands for each pixel of a chunk, where they are
/// those `image` draws: one plane of unsigned eight or sixteen bits, or
/// of 32-bit floats in colour, its sample format said.
fn layout<R: Read + Seek>(tiff: &mut Tiff<R>) -> Option<ColorType> {
    // Planes stored apart are interleaved by `image`.
    if tiff
        .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
        .ok()?
        .unwrap_or(1)
        != 1
    {
        return None;
    }
    let (color, float) = match tiff.colortype().ok()? {
        tiff::ColorType::Gray(8) => (ColorType::L8, false),
        tiff::ColorType::Gray(16) => (ColorType::L16, false),
        tiff::ColorType::GrayA(8) => (ColorType::La8, false),
        tiff::ColorType::GrayA(16) => (ColorType::La16, false),
        tiff::ColorType::RGB(8) => (ColorType::Rgb8, false),
        tiff::ColorType::RGB(16) => (ColorType::Rgb16, false),
        tiff::ColorType::RGBA(8) => (ColorType::Rgba8, false),
        tiff::ColorType::RGBA(16) => (ColorType::Rgba16, false),
        tiff::ColorType::RGB(32) => (ColorType::Rgb32F, true),
        tiff::ColorType::RGBA(32) => (ColorType::Rgba32F, true),
        // A bit a pixel, CMYK: drawn by `image` from what is stored.
        // Palettes, YCbCr, other depths: refused by it.
        _ => return None,
    };
    let wanted = if float {
        SampleFormat::IEEEFP
    } else {
        SampleFormat::Uint
    };
    match tiff.find_tag_unsigned_vec::<u16>(Tag::SampleFormat).ok()? {
        Some(formats)
            if formats
                .iter()
                .all(|&format| SampleFormat::from_u16(format) == Some(wanted)) => {}
        // Unsigned where it says nothing.
        None if !float => {}
        _ => return None,
    }
    let (across, down) = tiff.chunk_dimensions();
    (across > 0 && down > 0).then_some(color)
}

/// A TIFF picture laid out as [`by_chunks`] says, or the region of it
/// `picture::Regional::cut` says, in its pixels as stored: its
/// Orientation tag is its caller's to apply, as `image`'s decoder leaves
/// it.
pub(crate) struct Decoder<R: BufRead + Seek> {
    tiff: Tiff<R>,
    color: ColorType,
    /// Its width and height.
    size: (u32, u32),
    /// What is read (left, top, width, height): the whole unless cut.
    region: [u32; 4],
    /// What the decode may hold besides the region, in bytes, as its
    /// limits say: a chunk, and what is read of it compressed.
    held: u64,
}

impl<R: BufRead + Seek> Decoder<R> {
    /// The picture `reader` reads, its directory read: an error where it
    /// is not laid out as [`by_chunks`] says.
    pub(crate) fn new(reader: R) -> ImageResult<Decoder<R>> {
        let mut tiff = Tiff::new(reader).map_err(failed)?;
        let color = layout(&mut tiff).ok_or_else(|| {
            ImageError::Unsupported(UnsupportedError::from_format_and_kind(
                ImageFormatHint::Exact(ImageFormat::Tiff),
                UnsupportedErrorKind::GenericFeature(
                    "a TIFF laid out otherwise than its chunks are read".into(),
                ),
            ))
        })?;
        let size = tiff.dimensions().map_err(failed)?;
        Ok(Decoder {
            tiff,
            color,
            size,
            region: [0, 0, size.0, size.1],
            held: image::Limits::default().max_alloc.unwrap_or(u64::MAX),
        })
    }
}

impl<R: BufRead + Seek> crate::picture::Regional for Decoder<R> {
    fn cut(&mut self, region: [u32; 4]) {
        self.region = crate::picture::inside(region, self.size);
    }

    fn region(&self) -> [u32; 4] {
        self.region
    }
}

impl<R: BufRead + Seek> ImageDecoder for Decoder<R> {
    fn dimensions(&self) -> (u32, u32) {
        (self.region[2], self.region[3])
    }

    fn color_type(&self) -> ColorType {
        self.color
    }

    fn icc_profile(&mut self) -> ImageResult<Option<Vec<u8>>> {
        Ok(self.tiff.get_tag_u8_vec(Tag::IccProfile).ok())
    }

    /// As `image` reads it: its Orientation tag.
    fn orientation(&mut self) -> ImageResult<Orientation> {
        Ok(self
            .tiff
            .find_tag(Tag::Orientation)
            .map_err(failed)?
            .and_then(|value| Orientation::from_exif(u8::try_from(value.into_u16().ok()?).ok()?))
            .unwrap_or(Orientation::NoTransforms))
    }

    fn set_limits(&mut self, limits: image::Limits) -> ImageResult<()> {
        self.held = limits.max_alloc.unwrap_or(u64::MAX);
        Ok(())
    }

    fn read_image(self, buf: &mut [u8]) -> ImageResult<()> {
        let Decoder {
            tiff,
            color,
            size,
            region: [left, top, width, height],
            held,
        } = self;
        // A chunk, and what is read of it compressed, within what may be
        // held.
        let bound = usize::try_from(held).unwrap_or(usize::MAX);
        let mut limits = tiff::decoder::Limits::default();
        limits.decoding_buffer_size = bound;
        limits.intermediate_buffer_size = bound;
        limits.ifd_value_size = bound;
        let mut tiff = tiff.with_limits(limits);
        let pixel = usize::from(color.bytes_per_pixel());
        let pitch = width as usize * pixel;
        // A strip is the picture's width, its last cut at its bottom; tiles
        // on the right and the bottom are cut where it ends.
        let (chunk_width, chunk_height) = tiff.chunk_dimensions();
        let chunks_across = size.0.div_ceil(chunk_width);
        let (right, bottom) = (left + width, top + height);
        let mut chunk = Vec::new();
        for down in top / chunk_height..=(bottom - 1) / chunk_height {
            for across in left / chunk_width..=(right - 1) / chunk_width {
                let index = down * chunks_across + across;
                let layout = tiff.image_chunk_buffer_layout(index).map_err(failed)?;
                if layout.len as u64 > held {
                    return Err(too_large());
                }
                let stride = layout.row_stride.map_or(0, std::num::NonZeroUsize::get);
                chunk.resize(layout.len, 0);
                tiff.read_chunk_bytes(index, &mut chunk).map_err(failed)?;
                // Where it lies (within the region's last row and column),
                // and its part within the region.
                let (x, y) = (across * chunk_width, down * chunk_height);
                let (from_x, to_x) = (left.max(x), right.min(x.saturating_add(chunk_width)));
                let (from_y, to_y) = (top.max(y), bottom.min(y.saturating_add(chunk_height)));
                let length = (to_x - from_x) as usize * pixel;
                for row in from_y..to_y {
                    let source = (row - y) as usize * stride + (from_x - x) as usize * pixel;
                    let target = (row - top) as usize * pitch + (from_x - left) as usize * pixel;
                    let part = chunk
                        .get(source..source + length)
                        .ok_or_else(|| broken("a chunk smaller than the TIFF says"))?;
                    buf[target..target + length].copy_from_slice(part);
                }
            }
        }
        Ok(())
    }

    fn read_image_boxed(self: Box<Self>, buf: &mut [u8]) -> ImageResult<()> {
        (*self).read_image(buf)
    }
}

/// `tiff`'s error as `image` says it.
fn failed(error: tiff::TiffError) -> ImageError {
    match error {
        tiff::TiffError::IoError(error) => ImageError::IoError(error),
        tiff::TiffError::LimitsExceeded => too_large(),
        tiff::TiffError::UnsupportedError(error) => {
            ImageError::Unsupported(UnsupportedError::from_format_and_kind(
                ImageFormatHint::Exact(ImageFormat::Tiff),
                UnsupportedErrorKind::GenericFeature(error.to_string()),
            ))
        }
        error => broken(error),
    }
}

/// A TIFF that cannot be read as it says.
fn broken(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Exact(ImageFormat::Tiff),
        error,
    ))
}

/// More than the decode may hold.
fn too_large() -> ImageError {
    ImageError::Limits(LimitError::from_kind(LimitErrorKind::InsufficientMemory))
}
