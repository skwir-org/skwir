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

use image::metadata::Orientation;
use image::{ColorType, ImageDecoder, ImageDecoderRect, ImageResult};
use jxl_oxide::integration::JxlDecoder;

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

/// A JPEG XL picture, or the region of it [`Decoder::cut`] says, read in
/// bands.
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

    /// Read `region` alone (left, top, width, height, as shown), within
    /// the picture, at least a pixel: what [`Decoder::region`] says.
    pub(crate) fn cut(&mut self, [left, top, width, height]: [u32; 4]) {
        let (own_width, own_height) = self.inner.dimensions();
        let left = left.min(own_width.saturating_sub(1));
        let top = top.min(own_height.saturating_sub(1));
        self.region = [
            left,
            top,
            width.clamp(1, own_width - left),
            height.clamp(1, own_height - top),
        ];
    }

    /// What is read: the whole, or what [`Decoder::cut`] said.
    pub(crate) fn region(&self) -> [u32; 4] {
        self.region
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

    /// A band is as tall as what may be held allows, in groups.
    #[test]
    fn a_band_is_as_tall_as_what_may_be_held() {
        const JXL: &[u8] = include_bytes!("../testdata/synthetic-plasma.jxl");
        let mut decoder = Decoder::new(std::io::Cursor::new(JXL)).unwrap();
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
        const JXL: &[u8] = include_bytes!("../testdata/synthetic-plasma.jxl");
        let read = |held: u64, region: Option<[u32; 4]>| {
            let mut decoder = Decoder::new(std::io::Cursor::new(JXL)).unwrap();
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
