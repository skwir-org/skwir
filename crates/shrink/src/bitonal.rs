//! Black-and-white images: read from CCITT fax, written as JBIG2.
//!
//! JBIG2 is written lossless only: one generic region covering the page,
//! coded with the MQ arithmetic coder (ITU-T T.88, 6.2 and Annex E), so
//! decoding gives every pixel back. Symbol matching (text regions) is never
//! used: a lossy encoder that takes one glyph for another changes what a
//! scan says (the 6 read as an 8), which a smaller copy must never do.
//!
//! What `JBIG2Decode` gives a PDF is the page's bitmap with its colours
//! swapped: JBIG2 calls black 1, and the filter hands the image 0 for it,
//! which `DeviceGray` with the default `/Decode` draws black (as `MuPDF`,
//! Poppler, `PDFium` and Ghostscript draw it). So [`Bits`] (1 black) are
//! coded as they are, for an image with no `/Decode`.

use anyhow::{Result, anyhow, ensure};

/// A black-and-white image: rows of `(width + 7) / 8` bytes, most
/// significant bit first, 1 black. `data` holds `height` rows exactly, as
/// the constructors make sure and [`jbig2`] relies on; the bits past
/// `width` in a row's last byte are 0 in what they give.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bits {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Bits {
    /// Bytes a row.
    pub fn stride(&self) -> usize {
        stride(self.width)
    }

    /// The image `data` holds, refused when its length is not
    /// `stride * height`; the bits past the width are cleared.
    pub fn new(width: u32, height: u32, mut data: Vec<u8>) -> Result<Bits> {
        let len = size(width, height)?;
        ensure!(
            data.len() == len,
            "{} bytes for {width}×{height} pixels of one bit",
            data.len()
        );
        clear_padding(&mut data, width);
        Ok(Bits {
            width,
            height,
            data,
        })
    }

    /// From a PDF image's 1-bit samples (rows padded to a byte, most
    /// significant bit first), where `zero_is_black` says which value is
    /// black: true for `DeviceGray` with the default `/Decode [0 1]`. A
    /// stream longer than the image is read to its size (a filter may
    /// leave bytes after it), a shorter one refused.
    pub fn from_samples(
        width: u32,
        height: u32,
        samples: &[u8],
        zero_is_black: bool,
    ) -> Result<Bits> {
        let len = size(width, height)?;
        ensure!(
            samples.len() >= len,
            "{} bytes for {width}×{height} pixels of one bit",
            samples.len()
        );
        let mut data = samples[..len].to_vec();
        if zero_is_black {
            invert(&mut data);
        }
        clear_padding(&mut data, width);
        Ok(Bits {
            width,
            height,
            data,
        })
    }

    /// Back to PDF 1-bit samples where 0 is black (`DeviceGray`, the
    /// default `/Decode`), the bits past the width 0.
    pub fn to_samples(&self) -> Vec<u8> {
        let mut samples = self.data.clone();
        invert(&mut samples);
        clear_padding(&mut samples, self.width);
        samples
    }

    /// Black made white and white black: for an image whose `/Decode` is
    /// `[1 0]`, which draws its samples' 0 white.
    pub fn invert(&mut self) {
        invert(&mut self.data);
        clear_padding(&mut self.data, self.width);
    }
}

/// Bytes a row of `width` pixels.
fn stride(width: u32) -> usize {
    (width as usize).div_ceil(8)
}

/// Bytes an image of `width` × `height` pixels holds, refused when empty
/// or past what memory addresses.
fn size(width: u32, height: u32) -> Result<usize> {
    ensure!(width > 0 && height > 0, "an empty image ({width}×{height})");
    stride(width)
        .checked_mul(height as usize)
        .ok_or_else(|| anyhow!("an image of {width}×{height} pixels is too large"))
}

fn invert(data: &mut [u8]) {
    for byte in data {
        *byte = !*byte;
    }
}

/// The bits past `width` in each row's last byte set to 0, so rows compare
/// and code alike whatever a producer left there.
fn clear_padding(data: &mut [u8], width: u32) {
    let used = width % 8;
    if used == 0 {
        return;
    }
    let mask = 0xFF_u8 << (8 - used);
    for row in data.chunks_exact_mut(stride(width)) {
        if let Some(last) = row.last_mut() {
            *last &= mask;
        }
    }
}

/// A PDF `CCITTFaxDecode` filter's `/DecodeParms`; `Default` holds the
/// PDF's defaults (`K` 0, `Columns` 1728, `Rows` 0, `BlackIs1` false,
/// `EncodedByteAlign` false, `EndOfLine` false, `EndOfBlock` true).
#[allow(clippy::struct_excessive_bools)] // The filter's parameters, one each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fax {
    /// Negative for Group 4, 0 for Group 3 one-dimensional, positive for
    /// Group 3 two-dimensional.
    pub k: i32,
    pub columns: u32,
    /// 0 when the stream does not say: the image's height is taken.
    pub rows: u32,
    pub black_is_1: bool,
    pub byte_align: bool,
    pub end_of_line: bool,
    pub end_of_block: bool,
}

impl Default for Fax {
    fn default() -> Fax {
        Fax {
            k: 0,
            columns: 1728,
            rows: 0,
            black_is_1: false,
            byte_align: false,
            end_of_line: false,
            end_of_block: true,
        }
    }
}

/// The image a CCITT fax stream holds, `height` rows (the image's
/// `/Height`, also taken when the parameters' `Rows` is 0). Black is what
/// the decoded samples draw black under `DeviceGray`'s default `/Decode`, a
/// 0 sample, as [`Bits::from_samples`] reads it: a coded black pixel unless
/// `BlackIs1`. An image whose `/Decode` is `[1 0]` is then inverted
/// ([`Bits::invert`]).
///
/// Refused when `Columns` is not the image's width (what readers show of
/// such a stream is each its own), and when the stream ends or breaks
/// before `height` rows: a copy never shows less than the original.
pub fn from_fax(data: &[u8], fax: &Fax, width: u32, height: u32) -> Result<Bits> {
    let len = size(width, height)?;
    ensure!(
        fax.columns == width,
        "a fax {} columns wide for an image {width} wide",
        fax.columns
    );
    let encoding = match fax.k {
        k if k < 0 => hayro_ccitt::EncodingMode::Group4,
        0 => hayro_ccitt::EncodingMode::Group3_1D,
        k => hayro_ccitt::EncodingMode::Group3_2D {
            k: k.unsigned_abs(),
        },
    };
    let rows = if fax.rows > 0 {
        fax.rows.min(height)
    } else {
        height
    };
    let settings = hayro_ccitt::DecodeSettings {
        columns: width,
        rows,
        end_of_block: fax.end_of_block,
        end_of_line: fax.end_of_line,
        rows_are_byte_aligned: fax.byte_align,
        encoding,
        // The decoder says white for a coded white pixel, or for a coded
        // black one when told to invert: with `BlackIs1`, a coded white
        // pixel is a 0 sample, which is black here.
        invert_black: fax.black_is_1,
    };
    let mut sink = FaxSink {
        data: vec![0; len],
        stride: stride(width),
        width: width as usize,
        rows: height as usize,
        row: 0,
        x: 0,
    };
    let mut context = hayro_ccitt::DecoderContext::new(settings);
    hayro_ccitt::decode(data, &mut sink, &mut context)
        .map_err(|e| anyhow!("CCITT fax, row {}: {e}", sink.row + 1))?;
    ensure!(
        sink.row >= sink.rows,
        "the CCITT fax holds {} rows of {height}",
        sink.row
    );
    Ok(Bits {
        width,
        height,
        data: sink.data,
    })
}

/// Where the fax decoder's runs land: black runs set in the rows of
/// [`Bits`].
struct FaxSink {
    data: Vec<u8>,
    stride: usize,
    width: usize,
    rows: usize,
    row: usize,
    x: usize,
}

impl hayro_ccitt::Decoder for FaxSink {
    fn push_pixels(&mut self, white: bool, count: u32) {
        let end = self.x.saturating_add(count as usize).min(self.width);
        if !white && self.row < self.rows {
            let start = self.row * self.stride;
            set_run(&mut self.data[start..start + self.stride], self.x, end);
        }
        self.x = end;
    }

    fn next_line(&mut self) {
        self.row += 1;
        self.x = 0;
    }
}

/// Pixels `from..to` of a row set to 1.
fn set_run(row: &mut [u8], from: usize, to: usize) {
    if from >= to {
        return;
    }
    let (first, last) = (from / 8, (to - 1) / 8);
    let head = 0xFF_u8 >> (from % 8);
    let tail = 0xFF_u8 << (7 - (to - 1) % 8);
    if first == last {
        row[first] |= head & tail;
    } else {
        row[first] |= head;
        row[first + 1..last].fill(0xFF);
        row[last] |= tail;
    }
}

/// A page information segment (T.88, 7.4.8).
const PAGE_INFORMATION: u8 = 48;
/// An immediate lossless generic region segment (T.88, 7.4.6).
const IMMEDIATE_LOSSLESS_GENERIC_REGION: u8 = 39;
/// The page flags: eventually lossless; no refinements, white by default,
/// regions combined by OR (T.88, 7.4.8.5).
const PAGE_LOSSLESS: u8 = 0x01;
/// The generic region's flags: arithmetic coding (no MMR), template 0,
/// typical prediction on (TPGDON, T.88 7.4.6.2).
const GENERIC_TEMPLATE_0_TPGDON: u8 = 0x08;
/// Template 0's adaptive pixels where the standard puts them (T.88,
/// 6.2.5.3, Table 5), as (x, y) bytes: A1 (3, −1), A2 (−3, −1), A3 (2, −2),
/// A4 (−2, −2). Decoders have a faster path for exactly these.
const NOMINAL_AT: [u8; 8] = [3, 0xFF, 0xFD, 0xFF, 2, 0xFE, 0xFE, 0xFE];
/// The context of typical prediction's bit, template 0 (T.88, 6.2.5.7,
/// Figure 8).
const SLTP_CONTEXT: usize = 0x9B25;
/// The most pixels a generic region holds: a larger image is written as
/// regions of rows below each other, so each segment's four-byte length
/// holds its coded data. A pixel, or a row's prediction bit, codes in at
/// most 15 bits (one renormalisation from the smallest estimate), so a
/// region codes in less than 2.5 GB, whatever its shape.
const REGION_PIXELS: u64 = 1 << 29;

/// The stream a PDF image's `/Filter /JBIG2Decode` reads, in the embedded
/// organisation (T.88, Annex D.3) and without `/JBIG2Globals`: a page
/// information segment and an immediate lossless generic region covering
/// the page, without the end-of-page segment a PDF leaves out. Lossless:
/// decoding gives `bits` back exactly. The image is written with
/// `DeviceGray`, `BitsPerComponent` 1 and no `/Decode`.
pub fn jbig2(bits: &Bits) -> Vec<u8> {
    jbig2_in_regions(bits, REGION_PIXELS)
}

/// [`jbig2`] with regions of at most `most` pixels (whole rows, one at
/// least).
fn jbig2_in_regions(bits: &Bits, most: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(bits.data.len() / 8 + 64);
    segment_header(&mut out, 0, PAGE_INFORMATION, 19);
    out.extend_from_slice(&bits.width.to_be_bytes());
    out.extend_from_slice(&bits.height.to_be_bytes());
    // The resolution, unknown: the PDF page says where the image goes.
    out.extend_from_slice(&[0; 8]);
    out.push(PAGE_LOSSLESS);
    // Not striped.
    out.extend_from_slice(&[0; 2]);

    let rows = (most / u64::from(bits.width.max(1))).clamp(1, u64::from(u32::MAX)) as u32;
    let mut top = 0;
    let mut number = 1;
    while top < bits.height {
        let height = rows.min(bits.height - top);
        segment_header(&mut out, number, IMMEDIATE_LOSSLESS_GENERIC_REGION, 0);
        let start = out.len();
        // The region's information (T.88, 7.4.1): its size, where it is,
        // combined by OR onto the white page.
        out.extend_from_slice(&bits.width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&0_u32.to_be_bytes());
        out.extend_from_slice(&top.to_be_bytes());
        out.push(0);
        out.push(GENERIC_TEMPLATE_0_TPGDON);
        out.extend_from_slice(&NOMINAL_AT);
        code_region(bits, top, height, &mut out);
        let length = u32::try_from(out.len() - start)
            .expect("a region of at most 2^29 pixels codes in less than 4 GiB");
        out[start - 4..start].copy_from_slice(&length.to_be_bytes());
        top += height;
        number += 1;
    }
    out
}

/// A segment's header (T.88, 7.2): its number and type, retained by
/// nothing, referring to no other, on page 1, `length` bytes of data.
fn segment_header(out: &mut Vec<u8>, number: u32, kind: u8, length: u32) {
    out.extend_from_slice(&number.to_be_bytes());
    out.push(kind);
    out.push(0);
    out.push(1);
    out.extend_from_slice(&length.to_be_bytes());
}

/// Rows `top..top + height` coded as a generic region with template 0
/// and typical prediction (T.88, 6.2.5), its data appended to `out`.
fn code_region(bits: &Bits, top: u32, height: u32, out: &mut Vec<u8>) {
    let stride = bits.stride();
    let width = bits.width as usize;
    // The row coded and the two above it, each two bytes longer than a row
    // so the pixels right of the image read white, as those above the
    // region's first row do.
    let mut above2 = vec![0_u8; stride + 2];
    let mut above1 = vec![0_u8; stride + 2];
    let mut row = vec![0_u8; stride + 2];
    let mut states: Box<[u8; 1 << 16]> = vec![0; 1 << 16]
        .into_boxed_slice()
        .try_into()
        .expect("a slice of the array's length");
    let mut coder = Coder::new(out);
    let mut ltp = false;
    for y in top..top + height {
        let start = y as usize * stride;
        row[..stride].copy_from_slice(&bits.data[start..start + stride]);
        clear_padding(&mut row[..stride], bits.width);
        // Typical prediction: a row the same as the one above is said so
        // in one bit and not coded (T.88, 6.2.5.7). The bit says whether
        // that changed since the last row.
        let same = row == above1;
        coder.encode(&mut states[SLTP_CONTEXT], u8::from(same != ltp));
        ltp = same;
        if !same {
            code_row(&mut coder, &mut states, &row, &above1, &above2, width);
        }
        std::mem::swap(&mut above2, &mut above1);
        std::mem::swap(&mut above1, &mut row);
    }
    coder.flush();
}

/// One row's pixels, each coded in the context of the 16 around it that
/// came before (template 0, T.88 6.2.5.3): four to its left, seven of the
/// row above (x − 3 to x + 3), five of the one above that (x − 2 to
/// x + 2). The context's bits, from 0: (x − 1 .. x − 4, y), then
/// (x + 3 .. x − 3, y − 1), then (x + 2 .. x − 2, y − 2), the order the
/// standard numbers them, so a new pixel shifts it along.
fn code_row(
    coder: &mut Coder<'_>,
    states: &mut [u8; 1 << 16],
    row: &[u8],
    above1: &[u8],
    above2: &[u8],
    width: usize,
) {
    // At x = 0, pixels 0 to 3 of the row above and 0 to 2 of the one above
    // that; those left of the image are white.
    let mut context = usize::from(above1[0] & 0xF0) | (usize::from(above2[0] >> 5) << 11);
    for (i, &byte) in row[..width.div_ceil(8)].iter().enumerate() {
        // Sixteen pixels from x = 8i of each row above: the ones a new
        // pixel brings in are x + 4 of the first and x + 3 of the second.
        let next1 = (usize::from(above1[i]) << 8) | usize::from(above1[i + 1]);
        let next2 = (usize::from(above2[i]) << 8) | usize::from(above2[i + 1]);
        for k in 0..(width - 8 * i).min(8) {
            let pixel = (byte >> (7 - k)) & 1;
            coder.encode(&mut states[context & 0xFFFF], pixel);
            context = ((context << 1) & 0xF7EE)
                | usize::from(pixel)
                | (((next1 >> (11 - k)) & 1) << 4)
                | (((next2 >> (12 - k)) & 1) << 11);
        }
    }
}

/// The MQ coder's probability states (T.88, Table E.1): the less probable
/// symbol's estimate, the state after the more probable one, after the
/// less probable one, and whether that swaps which is which.
#[rustfmt::skip]
const QE: [(u16, u8, u8, u8); 47] = [
    (0x5601, 1, 1, 1), (0x3401, 2, 6, 0), (0x1801, 3, 9, 0), (0x0AC1, 4, 12, 0),
    (0x0521, 5, 29, 0), (0x0221, 38, 33, 0), (0x5601, 7, 6, 1), (0x5401, 8, 14, 0),
    (0x4801, 9, 14, 0), (0x3801, 10, 14, 0), (0x3001, 11, 17, 0), (0x2401, 12, 18, 0),
    (0x1C01, 13, 20, 0), (0x1601, 29, 21, 0), (0x5601, 15, 14, 1), (0x5401, 16, 14, 0),
    (0x5101, 17, 15, 0), (0x4801, 18, 16, 0), (0x3801, 19, 17, 0), (0x3401, 20, 18, 0),
    (0x3001, 21, 19, 0), (0x2801, 22, 19, 0), (0x2401, 23, 20, 0), (0x2201, 24, 21, 0),
    (0x1C01, 25, 22, 0), (0x1801, 26, 23, 0), (0x1601, 27, 24, 0), (0x1401, 28, 25, 0),
    (0x1201, 29, 26, 0), (0x1101, 30, 27, 0), (0x0AC1, 31, 28, 0), (0x09C1, 32, 29, 0),
    (0x08A1, 33, 30, 0), (0x0521, 34, 31, 0), (0x0441, 35, 32, 0), (0x02A1, 36, 33, 0),
    (0x0221, 37, 34, 0), (0x0141, 38, 35, 0), (0x0111, 39, 36, 0), (0x0085, 40, 37, 0),
    (0x0049, 41, 38, 0), (0x0025, 42, 39, 0), (0x0015, 43, 40, 0), (0x0009, 44, 41, 0),
    (0x0005, 45, 42, 0), (0x0001, 45, 43, 0), (0x5601, 46, 46, 0),
];

/// The MQ arithmetic encoder (T.88, Annex E.2), appending to a buffer.
/// A context's state is one byte: its index in [`QE`] shifted left once,
/// its more probable symbol in bit 0.
struct Coder<'a> {
    out: &'a mut Vec<u8>,
    a: u32,
    c: u32,
    /// Shifts left before the next byte goes out.
    ct: u32,
    /// The byte being made, which a carry may still raise.
    b: u8,
    /// Whether `b` is a byte of the output: the first is the one before
    /// it, which the standard starts from and never writes.
    started: bool,
}

impl<'a> Coder<'a> {
    /// INITENC.
    fn new(out: &'a mut Vec<u8>) -> Coder<'a> {
        Coder {
            out,
            a: 0x8000,
            c: 0,
            ct: 12,
            b: 0,
            started: false,
        }
    }

    /// ENCODE, CODEMPS and CODELPS: `bit` coded in the context `state`.
    #[inline]
    fn encode(&mut self, state: &mut u8, bit: u8) {
        let mps = *state & 1;
        let (qe, more_next, less_next, switch) = QE[usize::from(*state >> 1)];
        let qe = u32::from(qe);
        self.a -= qe;
        if bit == mps {
            if self.a & 0x8000 != 0 {
                self.c += qe;
                return;
            }
            if self.a < qe {
                self.a = qe;
            } else {
                self.c += qe;
            }
            *state = (more_next << 1) | mps;
        } else {
            if self.a < qe {
                self.c += qe;
            } else {
                self.a = qe;
            }
            *state = (less_next << 1) | (mps ^ switch);
        }
        self.renormalize();
    }

    /// RENORME.
    fn renormalize(&mut self) {
        loop {
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.ct == 0 {
                self.byte_out();
            }
            if self.a & 0x8000 != 0 {
                break;
            }
        }
    }

    /// BYTEOUT: a byte moved from C to the output, with a carry into the
    /// byte before, and a bit stuffed after a 0xFF so no carry goes
    /// further.
    fn byte_out(&mut self) {
        if self.b == 0xFF {
            self.next_byte(7);
        } else if self.c < 0x800_0000 {
            self.next_byte(8);
        } else {
            self.b += 1;
            if self.b == 0xFF {
                self.c &= 0x7FF_FFFF;
                self.next_byte(7);
            } else {
                self.next_byte(8);
            }
        }
    }

    /// The byte made written, the next one `bits` bits from C.
    fn next_byte(&mut self, bits: u32) {
        if self.started {
            self.out.push(self.b);
        }
        self.started = true;
        self.b = (self.c >> (27 - bits)) as u8;
        self.c &= (1 << (27 - bits)) - 1;
        self.ct = bits;
    }

    /// FLUSH, ended by the marker 0xFF 0xAC as JBIG2 encoders write it.
    fn flush(mut self) {
        let top = self.c + self.a;
        self.c |= 0xFFFF;
        if self.c >= top {
            self.c -= 0x8000;
        }
        self.c <<= self.ct;
        self.byte_out();
        self.c <<= self.ct;
        self.byte_out();
        self.out.push(self.b);
        if self.b != 0xFF {
            self.out.push(0xFF);
        }
        self.out.push(0xAC);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A repeatable sequence of numbers (xorshift64).
    struct Noise(u64);

    impl Noise {
        fn step(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u32) -> u32 {
            (self.step() % u64::from(n)) as u32
        }
    }

    fn blank(width: u32, height: u32) -> Bits {
        Bits::new(width, height, vec![0; size(width, height).unwrap()]).unwrap()
    }

    fn set(bits: &mut Bits, x: u32, y: u32) {
        let stride = bits.stride();
        bits.data[y as usize * stride + x as usize / 8] |= 0x80 >> (x % 8);
    }

    fn get(bits: &Bits, x: u32, y: u32) -> bool {
        bits.data[y as usize * bits.stride() + x as usize / 8] & (0x80 >> (x % 8)) != 0
    }

    fn rect(bits: &mut Bits, x: u32, y: u32, w: u32, h: u32) {
        for yy in y..(y + h).min(bits.height) {
            for xx in x..(x + w).min(bits.width) {
                set(bits, xx, yy);
            }
        }
    }

    /// Lines of made-up glyphs, each a few strokes in its cell, words
    /// apart, lines apart, as a scanned page of text is.
    fn text(width: u32, height: u32, seed: u64) -> Bits {
        let mut bits = blank(width, height);
        let mut noise = Noise(seed);
        let mut y = 6;
        while y + 16 < height {
            let mut x = 5;
            while x + 10 < width {
                if noise.below(6) == 0 {
                    x += 8;
                    continue;
                }
                for _ in 0..2 + noise.below(3) {
                    if noise.below(2) == 0 {
                        rect(&mut bits, x + noise.below(8), y + noise.below(4), 2, 8);
                    } else {
                        rect(&mut bits, x, y + noise.below(14), 2 + noise.below(7), 2);
                    }
                }
                x += 11;
            }
            y += 24;
        }
        bits
    }

    fn noise(width: u32, height: u32, seed: u64, one_in: u32) -> Bits {
        let mut bits = blank(width, height);
        let mut noise = Noise(seed);
        for y in 0..height {
            for x in 0..width {
                if noise.below(one_in) == 0 {
                    set(&mut bits, x, y);
                }
            }
        }
        bits
    }

    /// What hayro-jbig2 reads of a stream as a PDF embeds it.
    fn read_jbig2(data: &[u8]) -> Bits {
        struct Collect {
            data: Vec<u8>,
            stride: usize,
            row: usize,
            x: usize,
        }
        impl hayro_jbig2::Decoder for Collect {
            fn push_pixel(&mut self, black: bool) {
                if black {
                    self.data[self.row * self.stride + self.x / 8] |= 0x80 >> (self.x % 8);
                }
                self.x += 1;
            }
            fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
                let start = self.row * self.stride + self.x / 8;
                let count = chunk_count as usize;
                if black {
                    self.data[start..start + count].fill(0xFF);
                }
                self.x += 8 * count;
            }
            fn next_line(&mut self) {
                self.row += 1;
                self.x = 0;
            }
        }
        let image = hayro_jbig2::Image::new_embedded(data, None).unwrap();
        let (width, height) = (image.width(), image.height());
        let stride = stride(width);
        let mut collect = Collect {
            data: vec![0; stride * height as usize],
            stride,
            row: 0,
            x: 0,
        };
        image.decode(&mut collect).unwrap();
        assert_eq!(collect.row, height as usize);
        Bits::new(width, height, collect.data).unwrap()
    }

    fn assert_round_trip(bits: &Bits) {
        let coded = jbig2(bits);
        let read = read_jbig2(&coded);
        assert_eq!((read.width, read.height), (bits.width, bits.height), "size");
        assert!(
            read == *bits,
            "{}×{} read back differs",
            bits.width,
            bits.height
        );
    }

    #[test]
    fn jbig2_reads_back_as_written() {
        assert_round_trip(&text(203, 97, 1));
        assert_round_trip(&text(640, 200, 2));
        assert_round_trip(&noise(1001, 37, 3, 2));
        assert_round_trip(&noise(77, 300, 4, 17));
        assert_round_trip(&noise(9, 9, 5, 3));
    }

    #[test]
    fn jbig2_reads_back_plain_and_tiny_images() {
        assert_round_trip(&blank(64, 64));
        assert_round_trip(&blank(13, 7));
        assert_round_trip(&blank(1, 1));
        for (width, height) in [(1, 1), (65, 33), (8, 3), (3, 8)] {
            let mut black = blank(width, height);
            rect(&mut black, 0, 0, width, height);
            assert_round_trip(&black);
        }
        assert_round_trip(&noise(1, 50, 6, 2));
        assert_round_trip(&noise(5000, 1, 7, 2));
    }

    #[test]
    fn jbig2_reads_back_rows_repeated_and_patterns() {
        // Repeated rows, the first ones black: typical prediction turns on
        // and off, at the top and below.
        let mut bits = blank(150, 60);
        rect(&mut bits, 0, 0, 150, 3);
        rect(&mut bits, 20, 10, 7, 30);
        rect(&mut bits, 0, 45, 149, 1);
        assert_round_trip(&bits);
        let mut checks = blank(31, 17);
        let mut stripes = blank(31, 17);
        for y in 0..17 {
            for x in 0..31 {
                if (x + y) % 2 == 0 {
                    set(&mut checks, x, y);
                }
                if x % 3 == 0 {
                    set(&mut stripes, x, y);
                }
            }
        }
        assert_round_trip(&checks);
        assert_round_trip(&stripes);
    }

    #[test]
    fn jbig2_ignores_what_padding_holds() {
        let clean = noise(13, 20, 8, 2);
        let mut dirty = clean.clone();
        for [_, last] in dirty.data.as_chunks_mut::<2>().0 {
            *last |= 0x07;
        }
        assert_eq!(jbig2(&dirty), jbig2(&clean));
    }

    #[test]
    fn jbig2_in_regions_reads_back() {
        let bits = text(203, 97, 9);
        for most in [203 * 10, 203 * 96, 1] {
            let coded = jbig2_in_regions(&bits, most);
            assert!(read_jbig2(&coded) == bits, "regions of {most} pixels");
        }
    }

    #[test]
    fn jbig2_is_two_segments_as_a_pdf_embeds_them() {
        let bits = text(100, 40, 10);
        let coded = jbig2(&bits);
        // The page's information: segment 0, type 48, page 1, 19 bytes.
        assert_eq!(coded[..11], [0, 0, 0, 0, 48, 0, 1, 0, 0, 0, 19]);
        assert_eq!(coded[11..15], 100_u32.to_be_bytes());
        assert_eq!(coded[15..19], 40_u32.to_be_bytes());
        assert_eq!(coded[27], PAGE_LOSSLESS);
        // The region: segment 1, type 39, page 1, the rest of the stream.
        let region = &coded[30..];
        assert_eq!(region[..7], [0, 0, 0, 1, 39, 0, 1]);
        let length = u32::from_be_bytes(region[7..11].try_into().unwrap()) as usize;
        assert_eq!(length, region.len() - 11, "no segment after the region");
        // The whole page, at its top left, combined by OR.
        let data = &region[11..];
        assert_eq!(data[..4], 100_u32.to_be_bytes());
        assert_eq!(data[4..8], 40_u32.to_be_bytes());
        assert_eq!(data[8..17], [0; 9]);
        assert_eq!(data[17], GENERIC_TEMPLATE_0_TPGDON);
        assert_eq!(data[18..26], NOMINAL_AT);
        assert!(data.ends_with(&[0xFF, 0xAC]));
    }

    #[test]
    fn jbig2_is_smaller_than_deflate_on_text() {
        let bits = text(1275, 1650, 11);
        let mut deflate = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        deflate.write_all(&bits.data).unwrap();
        let deflated = deflate.finish().unwrap().len();
        let coded = jbig2(&bits).len();
        assert!(
            coded * 3 < deflated * 2,
            "JBIG2 {coded} bytes, Deflate {deflated}"
        );
    }

    #[test]
    fn samples_are_read_and_written_in_either_polarity() {
        // 10 pixels wide: two bytes a row, six bits of padding.
        let samples = [0b0011_1111, 0b1100_0000, 0b1111_1111, 0b0111_1111];
        let bits = Bits::from_samples(10, 2, &samples, true).unwrap();
        assert_eq!(
            bits.data,
            [0b1100_0000, 0b0000_0000, 0b0000_0000, 0b1000_0000]
        );
        assert!(get(&bits, 0, 0) && get(&bits, 1, 0) && !get(&bits, 2, 0));
        assert!(get(&bits, 8, 1) && !get(&bits, 9, 1));
        assert_eq!(
            bits.to_samples(),
            [0b0011_1111, 0b1100_0000, 0b1111_1111, 0b0100_0000]
        );
        let same = Bits::from_samples(10, 2, &samples, false).unwrap();
        assert_eq!(
            same.data,
            [0b0011_1111, 0b1100_0000, 0b1111_1111, 0b0100_0000]
        );
        let mut inverted = same.clone();
        inverted.invert();
        assert_eq!(inverted, bits);
        // A longer stream is read to the image's size, a shorter refused.
        let longer = [samples.as_slice(), &[0xAA, 0x55]].concat();
        assert_eq!(Bits::from_samples(10, 2, &longer, true).unwrap(), bits);
        assert!(Bits::from_samples(10, 2, &samples[..3], true).is_err());
        assert!(Bits::from_samples(0, 2, &samples, true).is_err());
    }

    #[test]
    fn new_refuses_a_wrong_length_and_clears_padding() {
        assert!(Bits::new(9, 3, vec![0; 5]).is_err());
        assert!(Bits::new(9, 3, vec![0; 7]).is_err());
        assert!(Bits::new(9, 0, vec![]).is_err());
        let bits = Bits::new(9, 1, vec![0xFF, 0xFF]).unwrap();
        assert_eq!(bits.data, [0xFF, 0x80]);
        assert_eq!(bits.stride(), 2);
    }

    /// A Group 4 stream of `bits` (coded black where they are 1), each
    /// row starting on a byte when `byte_align`.
    fn group4(bits: &Bits, byte_align: bool) -> Vec<u8> {
        use std::cell::RefCell;
        use std::rc::Rc;
        struct Shared(Rc<RefCell<fax::VecWriter>>);
        impl fax::BitWriter for Shared {
            type Error = std::convert::Infallible;
            fn write(&mut self, bits: fax::Bits) -> Result<(), Self::Error> {
                self.0.borrow_mut().write(bits)
            }
        }
        let writer = Rc::new(RefCell::new(fax::VecWriter::new()));
        let mut encoder = fax::encoder::Encoder::new(Shared(writer.clone()));
        let width = u16::try_from(bits.width).unwrap();
        for y in 0..bits.height {
            let colors = (0..bits.width).map(|x| {
                if get(bits, x, y) {
                    fax::Color::Black
                } else {
                    fax::Color::White
                }
            });
            encoder.encode_line(colors, width).unwrap();
            if byte_align {
                writer.borrow_mut().pad();
            }
        }
        drop(encoder.finish().unwrap());
        Rc::try_unwrap(writer).ok().unwrap().into_inner().finish()
    }

    fn group4_fax(width: u32) -> Fax {
        Fax {
            k: -1,
            columns: width,
            ..Fax::default()
        }
    }

    #[test]
    fn group4_fax_reads_as_coded() {
        for bits in [text(203, 97, 12), text(1700, 120, 13), noise(61, 40, 14, 5)] {
            let fax = group4_fax(bits.width);
            let read = from_fax(&group4(&bits, false), &fax, bits.width, bits.height).unwrap();
            assert!(read == bits, "{}×{}", bits.width, bits.height);
            let aligned = Fax {
                byte_align: true,
                ..fax
            };
            let read = from_fax(&group4(&bits, true), &aligned, bits.width, bits.height).unwrap();
            assert!(read == bits, "{}×{} aligned", bits.width, bits.height);
            // Rows said, the same as the image's height.
            let rows = Fax {
                rows: bits.height,
                ..fax
            };
            let read = from_fax(&group4(&bits, false), &rows, bits.width, bits.height).unwrap();
            assert!(read == bits, "{}×{} with its rows", bits.width, bits.height);
        }
    }

    #[test]
    fn group4_fax_black_is_1_swaps_colours() {
        let bits = text(150, 50, 15);
        let fax = Fax {
            black_is_1: true,
            ..group4_fax(150)
        };
        let mut read = from_fax(&group4(&bits, false), &fax, 150, 50).unwrap();
        read.invert();
        assert!(read == bits, "inverted");
    }

    #[test]
    fn a_fax_short_of_rows_or_of_another_width_is_refused() {
        let bits = text(120, 60, 16);
        let coded = group4(&bits, false);
        assert!(from_fax(&coded, &group4_fax(120), 120, 61).is_err());
        let fewer = Fax {
            rows: 30,
            ..group4_fax(120)
        };
        assert!(from_fax(&coded, &fewer, 120, 60).is_err());
        assert!(from_fax(&coded, &group4_fax(121), 120, 60).is_err());
        assert!(from_fax(&coded[..coded.len() / 2], &group4_fax(120), 120, 60).is_err());
        // More rows said than the image has: only its own are read.
        let more = Fax {
            rows: 100,
            ..group4_fax(120)
        };
        let read = from_fax(&coded, &more, 120, 50).unwrap();
        assert_eq!(read.data, bits.data[..50 * 15]);
    }

    /// Bits written most significant first from a string of 0 and 1
    /// (spaces between codes), each `row` starting on a byte when
    /// `byte_align`.
    fn code_bits(rows: &[&str], byte_align: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut used = 0_usize;
        for row in rows {
            for c in row.chars().filter(|c| !c.is_whitespace()) {
                if used.is_multiple_of(8) {
                    out.push(0);
                }
                if c == '1' {
                    *out.last_mut().unwrap() |= 0x80 >> (used % 8);
                }
                used += 1;
            }
            if byte_align {
                used = used.div_ceil(8) * 8;
            }
        }
        out
    }

    #[test]
    fn group3_fax_reads_as_coded() {
        // Eight pixels wide, two rows: ____XXXX then XX______ (X coded
        // black). One-dimensional, white runs first: white 4 is 1011, black
        // 4 011, white 0 00110101, black 2 11, white 6 1110 (T.4, Table 2).
        let rows = ["1011 011", "00110101 11 1110"];
        let expected = Bits::new(8, 2, vec![0b0000_1111, 0b1100_0000]).unwrap();
        let mh = Fax {
            columns: 8,
            ..Fax::default()
        };
        assert_eq!(
            from_fax(&code_bits(&rows, false), &mh, 8, 2).unwrap(),
            expected
        );
        let aligned = Fax {
            byte_align: true,
            ..mh
        };
        assert_eq!(
            from_fax(&code_bits(&rows, true), &aligned, 8, 2).unwrap(),
            expected
        );
        // Each row after an end of line, 000000000001.
        let eol = ["000000000001 1011 011", "000000000001 00110101 11 1110"];
        let with_eol = Fax {
            end_of_line: true,
            ..mh
        };
        assert_eq!(
            from_fax(&code_bits(&eol, false), &with_eol, 8, 2).unwrap(),
            expected
        );

        // Two-dimensional (K 2): a tag bit before each row, 1 for a row
        // coded alone, 0 for one coded against the row above. The second
        // row, the same as the first, is two vertical modes V0 (1); the
        // third, ______XX, is VR2 (000011) then V0.
        let rows = ["1 1011 011", "0 1 1", "0 000011 1"];
        let expected = Bits::new(8, 3, vec![0b0000_1111, 0b0000_1111, 0b0000_0011]).unwrap();
        let mr = Fax {
            k: 2,
            columns: 8,
            ..Fax::default()
        };
        assert_eq!(
            from_fax(&code_bits(&rows, false), &mr, 8, 3).unwrap(),
            expected
        );
    }
}
