//! One image of a PDF written anew, where it is stored (ADR-112): its
//! samples read (JPEG, Deflate with its predictor, none; a black-and-white
//! image from CCITT fax or Deflate), resized when it is drawn at more dots
//! per inch than asked, made gray when asked, and written as JPEG, as
//! Deflate with PNG's predictor, or as JBIG2, whichever is smaller and
//! keeps what was asked. Its colour space stays its own: resizing works on
//! samples, each a linear measure of it, as a `Decode` array maps them.
//!
//! An image of a palette (its samples indexes, not measures) is written
//! as the colours they stand for, in the palette's own space.
//!
//! What is left as it is: an image of 2, 4 or 16 bits, a stencil mask, one with a colour
//! key or a transparency matted on a colour, JPEG 2000, a JPEG of four
//! components (CMYK's inversion is read two ways), a JPEG whose colour
//! transform is set by hand, and filters other than these.

use std::io::Read;

use anyhow::{Context, Result, bail, ensure};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use skwir_core::shrink::PdfRule;

use crate::bitonal;
use crate::samples::{self, JpegExtras, Samples};

/// The most samples an image is decoded to: past it, it is left as it is.
const MOST_BYTES: u64 = 512 * 1024 * 1024;

/// A JPEG kept only when it is this much smaller than what it replaces: a
/// photograph written again at the same size loses a little each time.
const WORTH: f64 = 0.9;

/// The least JPEG quality a flat image (text, a drawing) is written at:
/// what keeps its edges free of the rings lower qualities leave.
const FLAT_QUALITY: u8 = 85;

/// What became of an image.
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    /// Written anew, at this many bytes.
    Written,
    /// Left as it is: nothing smaller, or nothing asked of it.
    Kept,
}

/// How an image is stored.
enum Stored {
    Jpeg,
    Deflate { params: Option<Dictionary> },
    Raw,
    Fax(bitonal::Fax),
}

/// The image `id` of `doc` written anew under `rule`, `dpi` the dots per
/// inch it is drawn at (none when no page draws it: then it is never
/// resized), `gray` asked; its soft mask, when it has one of its size,
/// resized with it. Left as it is when it is of a kind not written here,
/// or when nothing written is smaller.
pub fn rewrite(
    doc: &mut Document,
    id: ObjectId,
    rule: &PdfRule,
    dpi: Option<f64>,
    gray: bool,
) -> Result<Done> {
    let Ok(Object::Stream(stream)) = doc.get_object(id) else {
        return Ok(Done::Kept);
    };
    let stream = stream.clone();
    let dict = &stream.dict;
    if flag(dict, b"ImageMask") || dict.has(b"Mask") && !mask_is_stream(doc, dict) {
        return Ok(Done::Kept);
    }
    let width = int(dict, b"Width").context("an image without a width")?;
    let height = int(dict, b"Height").context("an image without a height")?;
    let bits = int(dict, b"BitsPerComponent").unwrap_or(8);
    let Some(stored) = stored(doc, dict) else {
        return Ok(Done::Kept);
    };
    let Some(space) = space(doc, dict) else {
        return Ok(Done::Kept);
    };
    match bits {
        1 if space.components == 1 && space.palette.is_none() && !dict.has(b"SMask") => {
            bitonal_written(doc, id, &stream, &stored, width, height)
        }
        8 => samples_written(
            doc, id, &stream, &stored, &space, width, height, rule, dpi, gray,
        ),
        _ => Ok(Done::Kept),
    }
}

/// A black-and-white image written as JBIG2 when that is smaller: without
/// loss, whatever is asked.
fn bitonal_written(
    doc: &mut Document,
    id: ObjectId,
    stream: &Stream,
    stored: &Stored,
    width: u32,
    height: u32,
) -> Result<Done> {
    let dict = &stream.dict;
    // Which sample is black, as its `Decode` reads it: 0 unless inverted.
    let zero_is_black = match dict.get(b"Decode").and_then(Object::as_array) {
        Ok(decode) => decode.first().and_then(number).unwrap_or(0.0) < 0.5,
        Err(_) => true,
    };
    let bits = match stored {
        Stored::Fax(fax) => {
            // Black is a 0 sample, `BlackIs1` applied; a `Decode` of
            // [1 0] turns it again.
            let mut image = bitonal::from_fax(&stream.content, fax, width, height)?;
            if !zero_is_black {
                image.invert();
            }
            image
        }
        Stored::Deflate { params } => {
            let data = predicted_back(&inflated(&stream.content)?, params.as_ref(), 1, 1, width)?;
            bitonal::Bits::from_samples(width, height, &data, zero_is_black)?
        }
        Stored::Raw => bitonal::Bits::from_samples(width, height, &stream.content, zero_is_black)?,
        Stored::Jpeg => return Ok(Done::Kept),
    };
    let jbig2 = bitonal::jbig2(&bits);
    if jbig2.len() >= stream.content.len() {
        return Ok(Done::Kept);
    }
    let mut dict = stream.dict.clone();
    dict.set("Filter", Object::Name(b"JBIG2Decode".to_vec()));
    dict.remove(b"DecodeParms");
    // JBIG2Decode reads black as 0, as DeviceGray's default `Decode` does.
    dict.remove(b"Decode");
    replace(doc, id, dict, jbig2);
    Ok(Done::Written)
}

/// An image of 8-bit samples written anew.
#[allow(clippy::too_many_arguments)]
fn samples_written(
    doc: &mut Document,
    id: ObjectId,
    stream: &Stream,
    stored: &Stored,
    space: &Space,
    width: u32,
    height: u32,
    rule: &PdfRule,
    dpi: Option<f64>,
    gray: bool,
) -> Result<Done> {
    let bytes = u64::from(width) * u64::from(height) * u64::from(space.components);
    if bytes > MOST_BYTES {
        return Ok(Done::Kept);
    }
    let was_jpeg = matches!(stored, Stored::Jpeg);
    // A JPEG is written again only at a loss, which Lossless never asks;
    // one of a palette is no JPEG a reader reads.
    if was_jpeg && (rule.jpeg.is_none() || space.palette.is_some()) {
        return Ok(Done::Kept);
    }
    let samples = match stored {
        Stored::Jpeg => {
            if space.components == 4 {
                return Ok(Done::Kept);
            }
            let decoded =
                image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg)
                    .context("its JPEG could not be read")?;
            let (data, components) = match space.components {
                1 => (decoded.into_luma8().into_raw(), 1),
                _ => (decoded.into_rgb8().into_raw(), 3),
            };
            Samples::new(width, height, components, data)
                .context("its JPEG is not the size it says")?
        }
        Stored::Deflate { params } => {
            let data = predicted_back(
                &inflated(&stream.content)?,
                params.as_ref(),
                space.components,
                8,
                width,
            )?;
            Samples::new(width, height, space.components, data)?
        }
        Stored::Raw => Samples::new(width, height, space.components, stream.content.clone())?,
        Stored::Fax(_) => return Ok(Done::Kept),
    };
    // A palette's indexes looked up: the colours they stand for, in the
    // palette's own space, which the image is written in.
    let (samples, grayable) = match &space.palette {
        Some(palette) => (palette.looked_up(&samples)?, palette.grayable),
        None => (samples, space.grayable),
    };
    // Resized when drawn at more dots per inch than the rule keeps.
    let (new_width, new_height) = match (rule.dpi, rule.from, dpi) {
        (Some(target), Some(from), Some(dpi)) if dpi > f64::from(from) => {
            let scale = f64::from(target) / dpi;
            let side = |n: u32| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = (f64::from(n) * scale).round() as u32;
                n.clamp(1, u32::MAX)
            };
            (side(width), side(height))
        }
        _ => (width, height),
    };
    let resized = (new_width, new_height) != (width, height);
    let made_gray = gray && grayable && !space.decoded;
    if was_jpeg && !resized && !made_gray && rule.jpeg.is_none() {
        return Ok(Done::Kept);
    }
    let mut samples = samples.resized(new_width, new_height)?;
    if made_gray {
        samples = samples.gray();
    }
    // What it may be written as: JPEG where the rule writes it (a flat
    // image, text or a drawing, at a quality that keeps its edges, and only
    // when that saves a quarter), or where it was a JPEG; else Deflate with
    // PNG's predictor.
    let flat = !was_jpeg && samples.looks_flat();
    let jpeg_allowed =
        rule.jpeg.is_some() && samples.components != 4 && (was_jpeg || rule.lossy_flat);
    let jpeg = match rule.jpeg {
        Some(quality) if jpeg_allowed => {
            let quality = if flat {
                quality.max(FLAT_QUALITY)
            } else {
                quality
            };
            samples::jpeg(&samples, quality, &JpegExtras::default()).ok()
        }
        _ => None,
    };
    let deflated = (!was_jpeg).then(|| samples::predicted(&samples));
    let original = stream.content.len();
    let candidate = match (jpeg, deflated) {
        (Some(jpeg), Some(deflated)) => {
            let wins = if flat {
                jpeg.len() * 4 < deflated.len() * 3
            } else {
                jpeg.len() < deflated.len()
            };
            if wins {
                Written::Jpeg(jpeg)
            } else {
                Written::Deflate(deflated)
            }
        }
        (Some(jpeg), None) => Written::Jpeg(jpeg),
        (None, Some(deflated)) => Written::Deflate(deflated),
        (None, None) => return Ok(Done::Kept),
    };
    #[allow(clippy::cast_precision_loss)]
    let worth = match &candidate {
        // A JPEG written again at its size must save enough to lose a little.
        Written::Jpeg(data) if was_jpeg && !resized && !made_gray => {
            (data.len() as f64) < original as f64 * WORTH
        }
        written => written.len() < original,
    };
    if !worth {
        return Ok(Done::Kept);
    }
    let mut dict = stream.dict.clone();
    dict.set("Width", Object::Integer(i64::from(new_width)));
    dict.set("Height", Object::Integer(i64::from(new_height)));
    dict.set("BitsPerComponent", Object::Integer(8));
    if made_gray {
        dict.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
    } else if let Some(palette) = &space.palette {
        dict.set("ColorSpace", palette.base.clone());
    }
    dict.remove(b"DecodeParms");
    let data = match candidate {
        Written::Jpeg(data) => {
            dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
            data
        }
        Written::Deflate(data) => {
            dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
            dict.set("DecodeParms", predictor(&samples));
            data
        }
    };
    // Its soft mask, drawn over the same square, follows its size.
    if resized {
        resize_mask(doc, &dict, width, height, new_width, new_height);
    }
    replace(doc, id, dict, data);
    Ok(Done::Written)
}

enum Written {
    Jpeg(Vec<u8>),
    Deflate(Vec<u8>),
}

impl Written {
    fn len(&self) -> usize {
        match self {
            Written::Jpeg(data) | Written::Deflate(data) => data.len(),
        }
    }
}

/// The `DecodeParms` that read [`samples::predicted`] back.
fn predictor(samples: &Samples) -> Object {
    let mut params = Dictionary::new();
    params.set("Predictor", Object::Integer(15));
    params.set("Colors", Object::Integer(i64::from(samples.components)));
    params.set("BitsPerComponent", Object::Integer(8));
    params.set("Columns", Object::Integer(i64::from(samples.width)));
    Object::Dictionary(params)
}

/// The soft mask the image `dict` names, resized from `width` × `height`
/// to the image's new size when it was the image's size; left as it is
/// otherwise (it is drawn over the same square at its own resolution).
fn resize_mask(
    doc: &mut Document,
    dict: &Dictionary,
    width: u32,
    height: u32,
    new_width: u32,
    new_height: u32,
) {
    let Ok(mask_id) = dict.get(b"SMask").and_then(Object::as_reference) else {
        return;
    };
    let Ok(Object::Stream(mask)) = doc.get_object(mask_id) else {
        return;
    };
    let mask = mask.clone();
    let md = &mask.dict;
    if int(md, b"Width") != Some(width)
        || int(md, b"Height") != Some(height)
        || int(md, b"BitsPerComponent").unwrap_or(8) != 8
        || md.has(b"Matte")
    {
        return;
    }
    let data = match stored(doc, md) {
        Some(Stored::Deflate { params }) => inflated(&mask.content)
            .and_then(|data| predicted_back(&data, params.as_ref(), 1, 8, width)),
        Some(Stored::Raw) => Ok(mask.content.clone()),
        _ => return,
    };
    let Ok(resized) = data
        .and_then(|data| Samples::new(width, height, 1, data))
        .and_then(|samples| samples.resized(new_width, new_height))
    else {
        return;
    };
    let mut md = mask.dict.clone();
    md.set("Width", Object::Integer(i64::from(new_width)));
    md.set("Height", Object::Integer(i64::from(new_height)));
    md.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    md.set("DecodeParms", predictor(&resized));
    let data = samples::predicted(&resized);
    replace(doc, mask_id, md, data);
}

/// The image `id` replaced by `dict` and `data`, written as they are.
fn replace(doc: &mut Document, id: ObjectId, mut dict: Dictionary, data: Vec<u8>) {
    dict.remove(b"Length");
    let mut stream = Stream::new(dict, data);
    stream.allows_compression = false;
    doc.objects.insert(id, Object::Stream(stream));
}

/// What an image's colour space is, as resizing needs it.
struct Space {
    /// Components of its samples as stored: 1 for a palette's indexes.
    components: u8,
    /// Gray is asked of it and it can be: RGB of some kind, or CMYK.
    grayable: bool,
    /// It has a `Decode` array: gray would read its samples another way.
    decoded: bool,
    /// Its samples are a palette's indexes (`Indexed`).
    palette: Option<Palette>,
}

/// An `Indexed` colour space: what its indexes stand for.
struct Palette {
    /// The space its colours are in, as the image names it.
    base: Object,
    components: u8,
    grayable: bool,
    /// The highest index, and each index's colour, `components` bytes each.
    hival: usize,
    table: Vec<u8>,
}

impl Palette {
    /// The colours `indexes` stand for; an index past the palette takes its
    /// last colour, as readers do.
    fn looked_up(&self, indexes: &Samples) -> Result<Samples> {
        let size = usize::from(self.components);
        ensure!(
            self.table.len() >= (self.hival + 1) * size,
            "its palette is shorter than it says"
        );
        let mut data = Vec::with_capacity(indexes.data.len() * size);
        for &index in &indexes.data {
            let at = usize::from(index).min(self.hival) * size;
            data.extend_from_slice(&self.table[at..at + size]);
        }
        Samples::new(indexes.width, indexes.height, self.components, data)
    }
}

/// The colour space of the image `dict`, when its samples are measures of
/// 1, 3 or 4 components, or indexes of a palette of such colours.
fn space(doc: &Document, dict: &Dictionary) -> Option<Space> {
    let decoded = dict.has(b"Decode");
    let object = dict.get(b"ColorSpace").ok()?;
    let resolved = doc.dereference(object).ok()?.1;
    if let Object::Array(items) = resolved
        && items.first().and_then(|first| first.as_name().ok()) == Some(b"Indexed".as_slice())
    {
        // A palette's indexes read through a `Decode` are not read here.
        if decoded {
            return None;
        }
        let base = items.get(1)?.clone();
        let (components, grayable) = base_space(doc, &base)?;
        let hival = usize::try_from(items.get(2)?.as_i64().ok()?).ok()?.min(255);
        let table = match doc.dereference(items.get(3)?).ok()?.1 {
            Object::String(bytes, _) => bytes.clone(),
            Object::Stream(stream) => stream
                .decompressed_content()
                .unwrap_or_else(|_| stream.content.clone()),
            _ => return None,
        };
        return Some(Space {
            components: 1,
            grayable: false,
            decoded,
            palette: Some(Palette {
                base,
                components,
                grayable,
                hival,
                table,
            }),
        });
    }
    let (components, grayable) = base_space(doc, object)?;
    Some(Space {
        components,
        grayable,
        decoded,
        palette: None,
    })
}

/// A colour space that is no palette: its components (1, 3 or 4) and
/// whether it can be made gray.
fn base_space(doc: &Document, object: &Object) -> Option<(u8, bool)> {
    let space = doc.dereference(object).ok()?.1;
    let (name, rest): (&[u8], &[Object]) = match space {
        Object::Name(name) => (name, &[]),
        Object::Array(items) => (items.first()?.as_name().ok()?, &items[1..]),
        _ => return None,
    };
    let (components, grayable) = match name {
        b"DeviceGray" | b"CalGray" | b"G" | b"Separation" => (1, false),
        b"DeviceRGB" | b"CalRGB" | b"RGB" => (3, true),
        b"Lab" => (3, false),
        b"DeviceCMYK" | b"CMYK" => (4, true),
        b"ICCBased" => {
            let profile = rest.first()?;
            let n = match doc.dereference(profile).ok()?.1 {
                Object::Stream(stream) => int(&stream.dict, b"N")?,
                _ => return None,
            };
            let n = u8::try_from(n).ok()?;
            (n, n == 3 || n == 4)
        }
        b"DeviceN" => {
            let names = doc.dereference(rest.first()?).ok()?.1.as_array().ok()?;
            (u8::try_from(names.len()).ok()?, false)
        }
        _ => return None,
    };
    matches!(components, 1 | 3 | 4).then_some((components, grayable))
}

/// How the image `dict` is stored, when it is one way read here.
fn stored(doc: &Document, dict: &Dictionary) -> Option<Stored> {
    let filter = match dict.get(b"Filter") {
        Err(_) => return Some(Stored::Raw),
        Ok(filter) => doc.dereference(filter).ok()?.1,
    };
    let name: &[u8] = match filter {
        Object::Name(name) => name,
        Object::Array(items) if items.len() == 1 => items[0].as_name().ok()?,
        Object::Array(items) if items.is_empty() => return Some(Stored::Raw),
        _ => return None,
    };
    let params = dict.get(b"DecodeParms").ok().and_then(|params| {
        let params = doc.dereference(params).ok()?.1;
        match params {
            Object::Dictionary(params) => Some(params.clone()),
            Object::Array(items) => items
                .first()
                .and_then(|first| first.as_dict().ok())
                .cloned(),
            _ => None,
        }
    });
    match name {
        b"DCTDecode" | b"DCT" => {
            // A colour transform set by hand reads the JPEG another way.
            if params
                .as_ref()
                .is_some_and(|params| params.has(b"ColorTransform"))
            {
                return None;
            }
            Some(Stored::Jpeg)
        }
        b"FlateDecode" | b"Fl" => Some(Stored::Deflate { params }),
        b"CCITTFaxDecode" | b"CCF" => {
            let params = params.unwrap_or_default();
            let mut fax = bitonal::Fax::default();
            if let Some(k) = params.get(b"K").ok().and_then(|k| k.as_i64().ok()) {
                fax.k = i32::try_from(k).ok()?;
            }
            if let Some(columns) = int(&params, b"Columns") {
                fax.columns = columns;
            }
            if let Some(rows) = int(&params, b"Rows") {
                fax.rows = rows;
            }
            fax.black_is_1 = flag(&params, b"BlackIs1");
            fax.byte_align = flag(&params, b"EncodedByteAlign");
            fax.end_of_line = flag(&params, b"EndOfLine");
            if let Ok(end) = params.get(b"EndOfBlock").and_then(Object::as_bool) {
                fax.end_of_block = end;
            }
            Some(Stored::Fax(fax))
        }
        _ => None,
    }
}

/// Whether the image `dict`'s `Mask` is an image (a stencil, drawn as it
/// is whatever the image becomes) rather than colours keyed out (which a
/// JPEG's loss would no longer match).
fn mask_is_stream(doc: &Document, dict: &Dictionary) -> bool {
    dict.get(b"Mask")
        .ok()
        .and_then(|mask| doc.dereference(mask).ok())
        .is_some_and(|(_, mask)| matches!(mask, Object::Stream(_)))
}

/// `data` inflated (zlib), bounded.
fn inflated(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .take(MOST_BYTES)
        .read_to_end(&mut out)
        .context("its Deflate data could not be read")?;
    Ok(out)
}

/// `data` with its predictor (`params`: PNG's, 10 to 15, or TIFF's, 2)
/// taken off: samples of `colors` components of `bits` each, rows of
/// `width`.
fn predicted_back(
    data: &[u8],
    params: Option<&Dictionary>,
    colors: u8,
    bits: u32,
    width: u32,
) -> Result<Vec<u8>> {
    let Some(params) = params else {
        return Ok(data.to_vec());
    };
    let predictor = params
        .get(b"Predictor")
        .and_then(Object::as_i64)
        .unwrap_or(1);
    if predictor <= 1 {
        return Ok(data.to_vec());
    }
    let colors = int(params, b"Colors").unwrap_or(u32::from(colors)) as usize;
    let bits = int(params, b"BitsPerComponent").unwrap_or(bits) as usize;
    let columns = int(params, b"Columns").unwrap_or(width) as usize;
    let row = (colors * bits * columns).div_ceil(8);
    let bpp = (colors * bits).div_ceil(8).max(1);
    ensure!(row > 0, "an image of no width");
    if predictor == 2 {
        ensure!(
            bits == 8,
            "TIFF's predictor on {bits}-bit samples is not read here"
        );
        let mut out = data.to_vec();
        for line in out.chunks_exact_mut(row) {
            for x in bpp..row {
                line[x] = line[x].wrapping_add(line[x - bpp]);
            }
        }
        return Ok(out);
    }
    ensure!(predictor >= 10, "predictor {predictor} is not read here");
    let mut out = Vec::with_capacity(data.len());
    let mut previous = vec![0u8; row];
    for line in data.chunks(row + 1) {
        if line.len() < row + 1 {
            break;
        }
        let (kind, filtered) = (line[0], &line[1..]);
        let mut current = vec![0u8; row];
        for x in 0..row {
            let a = if x >= bpp { current[x - bpp] } else { 0 };
            let b = previous[x];
            let c = if x >= bpp { previous[x - bpp] } else { 0 };
            let predicted = match kind {
                0 => 0,
                1 => a,
                2 => b,
                3 => a.midpoint(b),
                4 => paeth(a, b, c),
                other => bail!("PNG filter {other} is none"),
            };
            current[x] = filtered[x].wrapping_add(predicted);
        }
        out.extend_from_slice(&current);
        previous = current;
    }
    Ok(out)
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

fn int(dict: &Dictionary, key: &[u8]) -> Option<u32> {
    dict.get(key)
        .ok()
        .and_then(|value| value.as_i64().ok())
        .and_then(|value| u32::try_from(value).ok())
}

fn flag(dict: &Dictionary, key: &[u8]) -> bool {
    dict.get(key).and_then(Object::as_bool).unwrap_or(false)
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(n) => {
            #[allow(clippy::cast_precision_loss)]
            let n = *n as f64;
            Some(n)
        }
        Object::Real(n) => Some(f64::from(*n)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_predictor_is_taken_off() {
        let samples = Samples::new(
            7,
            3,
            3,
            (0..63)
                .map(|n| u8::try_from(n * 4 % 256).unwrap())
                .collect(),
        )
        .unwrap();
        let predicted = samples::predicted(&samples);
        let Object::Dictionary(params) = predictor(&samples) else {
            unreachable!()
        };
        let back = predicted_back(&inflated(&predicted).unwrap(), Some(&params), 3, 8, 7).unwrap();
        assert_eq!(back, samples.data);
    }

    /// A gray palette's image drawn at 600 dpi, brought to 150 dpi at
    /// Screen and written in the palette's own space.
    #[test]
    fn a_palette_image_is_written_as_its_colours() {
        let mut doc = Document::with_version("1.5");
        let (width, height) = (400u32, 300u32);
        let table: Vec<u8> = (0..=255u8).collect();
        let indexes: Vec<u8> = (0..width * height)
            .map(|n| u8::try_from((n % width) * 255 / width).unwrap())
            .collect();
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", Object::Name(b"Image".to_vec()));
        dict.set("Width", Object::Integer(i64::from(width)));
        dict.set("Height", Object::Integer(i64::from(height)));
        dict.set("BitsPerComponent", Object::Integer(8));
        dict.set(
            "ColorSpace",
            Object::Array(vec![
                Object::Name(b"Indexed".to_vec()),
                Object::Name(b"DeviceGray".to_vec()),
                Object::Integer(255),
                Object::String(table, lopdf::StringFormat::Hexadecimal),
            ]),
        );
        dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
        let id = doc.add_object(Stream::new(dict, samples::deflate(&indexes)));
        let rule = skwir_core::shrink::Quality::Screen.pdf();
        assert_eq!(
            rewrite(&mut doc, id, &rule, Some(600.0), false).unwrap(),
            Done::Written
        );
        let Ok(Object::Stream(stream)) = doc.get_object(id) else {
            panic!("an image");
        };
        assert_eq!(
            stream
                .dict
                .get(b"ColorSpace")
                .and_then(Object::as_name)
                .unwrap(),
            b"DeviceGray"
        );
        assert_eq!(int(&stream.dict, b"Width"), Some(100));
        assert_eq!(int(&stream.dict, b"Height"), Some(75));
    }
}
