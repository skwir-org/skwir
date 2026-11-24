//! A PNG's region read from its rows (ADR-146 amended): what a zoom past
//! what is held of a picture shows (`picture::decode_region`). A PNG not
//! interlaced is one zlib stream of rows, each unfiltered from the one
//! above it, so the rows above the region are inflated and let go one by
//! one, those below never read, and only the region's columns of its own
//! rows are kept: what is held is the region and a row, never the
//! picture. Its pixels are the ones `image`'s decoder draws (the same
//! `png`, expanded the same way: palettes, low depths and `tRNS` to eight
//! bits and alpha, sixteen bits kept).
use std::io::{BufRead, Seek};

use image::error::{DecodingError, ImageFormatHint, LimitError, LimitErrorKind};
use image::{ColorType, ImageDecoder, ImageError, ImageFormat, ImageResult};

/// A PNG picture, or the region of it `picture::Regional::cut` says, in
/// its pixels as stored: its EXIF Orientation (`eXIf`) is its caller's
/// to apply, as `image`'s decoder leaves it.
pub(crate) struct Decoder<R: BufRead + Seek> {
    reader: png::Reader<R>,
    color: ColorType,
    /// Its width and height.
    size: (u32, u32),
    /// What is read (left, top, width, height): the whole unless cut.
    region: [u32; 4],
    /// What the decode may hold besides the region, in bytes, as its
    /// limits say: an interlaced picture's whole.
    held: u64,
}

impl<R: BufRead + Seek> Decoder<R> {
    /// The picture `reader` reads, read to its first rows, within what
    /// `image` allows by default.
    pub(crate) fn new(reader: R) -> ImageResult<Decoder<R>> {
        let held = image::Limits::default().max_alloc.unwrap_or(u64::MAX);
        let bytes = usize::try_from(held).unwrap_or(usize::MAX);
        let mut decoder = png::Decoder::new_with_limits(reader, png::Limits { bytes });
        // As `image` reads it: eight bits from fewer, a palette's colours,
        // `tRNS` as alpha; sixteen bits kept.
        decoder.set_transformations(png::Transformations::EXPAND);
        let reader = decoder.read_info().map_err(failed)?;
        let color = match reader.output_color_type() {
            (png::ColorType::Grayscale, png::BitDepth::Eight) => ColorType::L8,
            (png::ColorType::Grayscale, png::BitDepth::Sixteen) => ColorType::L16,
            (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => ColorType::La8,
            (png::ColorType::GrayscaleAlpha, png::BitDepth::Sixteen) => ColorType::La16,
            (png::ColorType::Rgb, png::BitDepth::Eight) => ColorType::Rgb8,
            (png::ColorType::Rgb, png::BitDepth::Sixteen) => ColorType::Rgb16,
            (png::ColorType::Rgba, png::BitDepth::Eight) => ColorType::Rgba8,
            (png::ColorType::Rgba, png::BitDepth::Sixteen) => ColorType::Rgba16,
            // None once expanded: a palette or fewer bits are eight.
            (color, depth) => {
                return Err(broken(format!(
                    "a PNG expanded to {color:?} in {depth:?} bits"
                )));
            }
        };
        let size = reader.info().size();
        Ok(Decoder {
            reader,
            color,
            size,
            region: [0, 0, size.0, size.1],
            held,
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
        Ok(self.reader.info().icc_profile.as_ref().map(|x| x.to_vec()))
    }

    /// Its `eXIf` chunk, read before its rows: where its Orientation is.
    fn exif_metadata(&mut self) -> ImageResult<Option<Vec<u8>>> {
        Ok(self
            .reader
            .info()
            .exif_metadata
            .as_ref()
            .map(|x| x.to_vec()))
    }

    fn set_limits(&mut self, limits: image::Limits) -> ImageResult<()> {
        self.held = limits.max_alloc.unwrap_or(u64::MAX);
        Ok(())
    }

    fn read_image(mut self, buf: &mut [u8]) -> ImageResult<()> {
        let [left, top, width, height] = self.region;
        let pixel = usize::from(self.color.bytes_per_pixel());
        let (from, across) = (left as usize * pixel, width as usize * pixel);
        let line = self
            .reader
            .output_line_size(self.size.0)
            .ok_or_else(too_large)?;
        // Three rows are held whatever the region is (this one, and
        // `png`'s row being unfiltered and the one above it), within what
        // the decode may hold besides it: `png` measured them against
        // `image`'s own limits as the picture was opened, never the
        // caller's.
        if (line as u64).saturating_mul(3) > self.held {
            return Err(too_large());
        }
        let rows = buf.chunks_exact_mut(across);
        if self.reader.info().interlaced {
            // Decoded whole, then cut: Adam7 spreads every row over seven
            // passes, the last of which (every other row) is the stream's
            // second half, so no row is whole before the stream's end and
            // reading it all takes the whole's time. Only memory could be
            // spared, by keeping the region's pixels of each pass; a
            // picture large enough to be zoomed past what is held of it is
            // seldom interlaced.
            let whole = self.reader.output_buffer_size().ok_or_else(too_large)?;
            if whole as u64 > self.held {
                return Err(too_large());
            }
            let mut pixels = vec![0; whole];
            self.reader.next_frame(&mut pixels).map_err(failed)?;
            let kept = pixels.chunks_exact(line).skip(top as usize);
            for (row, out) in kept.zip(rows) {
                out.copy_from_slice(&row[from..from + across]);
            }
        } else {
            // Every row to the region's bottom, each unfiltered from the
            // one before; those within it keep their columns alone.
            let mut row = vec![0; line];
            let mut out = rows;
            for at in 0..top + height {
                if self.reader.read_row(&mut row).map_err(failed)?.is_none() {
                    return Err(broken("fewer rows than the PNG says"));
                }
                if at >= top
                    && let Some(out) = out.next()
                {
                    out.copy_from_slice(&row[from..from + across]);
                }
            }
        }
        // A PNG's samples are big-endian; `read_image` hands them in the
        // machine's order, as `image`'s decoder does.
        if self.color.bytes_per_pixel() / self.color.channel_count() == 2 {
            for sample in buf.as_chunks_mut::<2>().0 {
                *sample = u16::from_be_bytes(*sample).to_ne_bytes();
            }
        }
        Ok(())
    }

    fn read_image_boxed(self: Box<Self>, buf: &mut [u8]) -> ImageResult<()> {
        (*self).read_image(buf)
    }
}

/// `png`'s error as `image` says it.
fn failed(error: png::DecodingError) -> ImageError {
    match error {
        png::DecodingError::IoError(error) => ImageError::IoError(error),
        png::DecodingError::LimitsExceeded => too_large(),
        error => broken(error),
    }
}

/// A PNG that cannot be read as it says.
fn broken(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Exact(ImageFormat::Png),
        error,
    ))
}

/// More than the decode may hold.
fn too_large() -> ImageError {
    ImageError::Limits(LimitError::from_kind(LimitErrorKind::InsufficientMemory))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::Regional;

    /// What a decode holds besides the region is within the caller's
    /// limits, the row each row is read into too: a picture a million
    /// pixels wide holds a row of 4 MB to read a pixel of it.
    #[test]
    fn a_row_is_held_within_the_limits() {
        let width = 1_000_000;
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, width, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&vec![0; width as usize * 4])
                .unwrap();
        }
        let read = |max_alloc: u64| {
            let mut decoder = Decoder::new(std::io::Cursor::new(&png)).unwrap();
            decoder.cut([0, 0, 1, 1]);
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(max_alloc);
            decoder.set_limits(limits).unwrap();
            let mut pixel = [0; 4];
            decoder.read_image(&mut pixel)
        };
        assert!(read(64 << 20).is_ok(), "a row of 4 MB within 64 MB");
        assert!(
            matches!(read(8 << 20), Err(ImageError::Limits(_))),
            "three rows of 4 MB beyond 8 MB"
        );
        assert!(
            matches!(read(1 << 20), Err(ImageError::Limits(_))),
            "a row of 4 MB beyond 1 MB"
        );
    }
}
