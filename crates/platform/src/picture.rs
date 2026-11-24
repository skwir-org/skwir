//! Pictures as they are meant to be seen. A camera held upright stores its
//! pixels as the sensor reads them, sideways, and says in the EXIF
//! Orientation tag how to turn them. Every picture shown, and every size
//! reported for one, goes through here, so a picture never stands one way
//! in its thumbnail and another in the preview. A picture in floating
//! point (`OpenEXR`) holds linear light, which a screen shows too dark unless
//! it is encoded as sRGB first; that happens here too.
use std::io::{BufRead, BufReader, Seek};

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, ImageResult};
use skwir_core::preview::{FileSource, ReadSeek};

/// A reader over the picture `file` (what a preview holds of it, or its
/// path, opened without blocking on a FIFO: `file::open_regular`), its
/// format guessed from the bytes.
pub fn open<S: FileSource + ?Sized>(
    file: &S,
) -> anyhow::Result<ImageReader<BufReader<Box<dyn ReadSeek + Send>>>> {
    formats();
    Ok(ImageReader::new(BufReader::new(file.open_reader()?)).with_guessed_format()?)
}

/// A reader over the picture `file` of type `mime`, read once
/// ([`Typed`]): what reads it once alone.
pub fn open_typed<S: FileSource + ?Sized>(
    file: &S,
    mime: Option<&str>,
) -> anyhow::Result<ImageReader<BufReader<Box<dyn ReadSeek + Send>>>> {
    Typed::new(file, mime)?.reader()
}

/// The width and height of the picture `file` of type `mime`, turned as
/// it is shown, read from its header: a camera's raw file's from its
/// JPEG's first bytes (`raw::shown_size`), its JPEG not read whole for it.
pub fn typed_size<S: FileSource + ?Sized>(
    file: &S,
    mime: Option<&str>,
) -> anyhow::Result<(u32, u32)> {
    if mime.is_some_and(crate::raw::is_raw) {
        return crate::raw::shown_size(&mut file.open_reader()?);
    }
    Ok(size(open(file)?)?)
}

/// The picture `file` of type `mime`, as a request reads it again and
/// again (its size, what it carries, its pixels): a camera's raw file as
/// the JPEG it carries, found and read once (`raw::embedded_jpeg`,
/// ADR-148), each reader over it in memory; any other as [`open`] reads
/// it. A raw file's JPEG is several megabytes, read anew for each reader
/// it was found for (on a remote, fetched anew).
pub enum Typed<'a, S: FileSource + ?Sized> {
    /// Read as it is.
    File(&'a S),
    /// A raw file's JPEG.
    Jpeg(std::sync::Arc<[u8]>),
}

impl<'a, S: FileSource + ?Sized> Typed<'a, S> {
    /// `file` of type `mime`, a raw file's JPEG read now.
    pub fn new(file: &'a S, mime: Option<&str>) -> anyhow::Result<Typed<'a, S>> {
        if mime.is_some_and(crate::raw::is_raw) {
            let jpeg = crate::raw::embedded_jpeg(&mut file.open_reader()?)?;
            return Ok(Typed::Jpeg(jpeg.into()));
        }
        Ok(Typed::File(file))
    }

    /// A reader over it, its format guessed, or a JPEG's.
    pub fn reader(&self) -> anyhow::Result<ImageReader<BufReader<Box<dyn ReadSeek + Send>>>> {
        match self {
            Typed::File(file) => open(*file),
            Typed::Jpeg(jpeg) => {
                let inner: Box<dyn ReadSeek + Send> =
                    Box::new(std::io::Cursor::new(std::sync::Arc::clone(jpeg)));
                Ok(ImageReader::with_format(
                    BufReader::new(inner),
                    ImageFormat::Jpeg,
                ))
            }
        }
    }
}

/// The formats Skwir decodes beside `image`'s own, taught to it once
/// (ADR-148): JPEG XL by jxl-oxide, in bands (`jxl`), HEIC and AVIF
/// through the libheif helper where it is installed (`heif`), so whatever reads
/// a picture through `image` (its size, a reduced decode, a region, a
/// thumbnail, a comic's page) reads one. Asked by every entry point that
/// guesses a picture's format; the first call registers, the rest return.
pub fn formats() {
    static TAUGHT: std::sync::Once = std::sync::Once::new();
    TAUGHT.call_once(|| {
        crate::jxl::register();
        crate::heif::register();
    });
}

/// The picture's width and height once turned, read from its header.
pub fn size<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(u32, u32)> {
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    Ok(turned(width, height, orientation(&mut decoder)))
}

/// The picture's width and height as stored, read from its header, and
/// its EXIF Orientation (1 to 8): what the print helper needs to draw it
/// turned as it is shown, from its own bytes.
pub fn stored<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(u32, u32, u8)> {
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    Ok((width, height, orientation(&mut decoder).to_exif()))
}

/// The picture decoded and turned, a floating-point one encoded as sRGB in
/// eight bits, within `limits` as `ImageReader::decode` would be: its
/// pixels counted too, which `ImageReader::into_decoder` leaves to each
/// decoder (a JPEG's checks only its sides).
pub fn decode<R: BufRead + Seek>(
    mut reader: ImageReader<R>,
    limits: image::Limits,
) -> ImageResult<DynamicImage> {
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder()?;
    let orientation = orientation(&mut decoder);
    let mut image = encoded(within(decoder, limits)?);
    image.apply_orientation(orientation);
    Ok(image)
}

/// The pixels `decoder` decodes, refused if they are more than `limits`
/// allow; what it holds besides within the rest.
fn within(mut decoder: impl ImageDecoder, mut limits: image::Limits) -> ImageResult<DynamicImage> {
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    DynamicImage::from_decoder(decoder)
}

/// Whether a decode may take threads of its own: said by whoever asks for
/// it, which knows who waits for it, never read from the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Threads {
    /// On the asking thread alone: one of many pictures decoded at once,
    /// each on a worker of its own (thumbnails, a comic's pages, a contact
    /// sheet), where a thread a component for each would make three or four
    /// times as many.
    Caller,
    /// A thread a component (a JPEG's, decoded at a reduced scale): the
    /// picture the user waits for, decoded alone (the preview's own, which
    /// take turns).
    Components,
}

/// The picture decoded, shrunk to at most `side` on its longer side, then
/// turned: what a thumbnail needs. Turned before shrinking, a portrait
/// photo of 24 megapixels held a second full copy of itself, 72 MB more
/// for each worker, and the turn cost the full picture's time. Within
/// `image`'s own limits, whatever `reader`'s: a caller with limits of its
/// own decodes through [`jpeg_fitted`] and [`decode`].
pub fn decode_fitted<R: BufRead + Seek>(
    reader: ImageReader<R>,
    side: u32,
    threads: Threads,
) -> ImageResult<DynamicImage> {
    decode_fitted_within(reader, side, side, threads)
}

/// As [`decode_fitted`], a JPEG at its least reduced scale at least `least`
/// on its longer side: a picture up to `side / least` short of `side` where
/// a smaller scale reaches it, quicker to decode than the next one up (a
/// whole decode shrunk), shown while stepping (ADR-146, 6).
pub fn decode_fitted_within<R: BufRead + Seek>(
    reader: ImageReader<R>,
    least: u32,
    side: u32,
    threads: Threads,
) -> ImageResult<DynamicImage> {
    if reader.format() == Some(ImageFormat::Jpeg) {
        let mut inner = reader.into_inner();
        if let Some(reduced) = jpeg_reduced(&mut inner, least.min(side), threads, REDUCED_BYTES)? {
            return Ok(fitted(reduced, side));
        }
        return Ok(fitted(
            whole(ImageReader::with_format(inner, ImageFormat::Jpeg))?,
            side,
        ));
    }
    Ok(fitted(whole(reader)?, side))
}

/// What a picture carries smaller than itself, turned as it is shown,
/// where `wanted` takes its longer side: a progressive JPEG's eighth (its
/// DC scans, ADR-146, 6), a JPEG XL's (its LF image, `jxl::eighth`), a
/// HEIF's or an AVIF's thumbnail, decoded by the helper (ADR-148 amended).
/// Decoded in a tenth of the whole's time (a JPEG XL's) to a hundredth (the
/// others'), it stands for the picture while it is decoded, where it is
/// smaller than asked, and is the picture, shrunk, where it covers what is
/// asked (a grid's thumbnail, a small preview). `None` for any other
/// picture, or where `wanted` refuses what it carries (a HEIF's thumbnail
/// is asked by the size the helper says of it, before it is decoded). A JPEG XL whose
/// eighth fails to decode has none: its whole says why.
pub fn carried<R: BufRead + Seek>(
    reader: ImageReader<R>,
    wanted: impl Fn(u32) -> bool,
    threads: Threads,
) -> ImageResult<Option<DynamicImage>> {
    if reader.format() == Some(ImageFormat::Jpeg) {
        return progressive_eighth(reader, wanted, threads);
    }
    if reader.format().is_some() {
        return Ok(None);
    }
    let mut inner = reader.into_inner();
    if crate::jxl::starts(inner.fill_buf()?) {
        return Ok(crate::jxl::eighth(&mut inner, wanted, threads).unwrap_or(None));
    }
    if crate::heif::starts(inner.fill_buf()?) {
        // Its size asked first: one refused is not decoded.
        return crate::heif::thumbnail(inner, &wanted);
    }
    Ok(None)
}

/// A progressive JPEG's eighth, turned (ADR-146, 6): its DC scans alone,
/// about a tenth of the file, decoded in a few milliseconds where the
/// whole picture's AC scans take hundreds at a quarter or a half. `None`
/// for any other picture, or where `wanted` refuses its longer side.
fn progressive_eighth<R: BufRead + Seek>(
    reader: ImageReader<R>,
    wanted: impl Fn(u32) -> bool,
    threads: Threads,
) -> ImageResult<Option<DynamicImage>> {
    if reader.format() != Some(ImageFormat::Jpeg) {
        return Ok(None);
    }
    let mut inner = reader.into_inner();
    let start = inner
        .stream_position()
        .map_err(image::ImageError::IoError)?;
    let read = {
        let mut jpeg = jpeg_decoder::Decoder::new(&mut inner);
        jpeg.read_info()
            .ok()
            .and_then(|()| jpeg.info())
            .map(|info| {
                (
                    info.coding_process == jpeg_decoder::CodingProcess::DctProgressive,
                    info.width.max(info.height),
                )
            })
    };
    inner
        .seek(std::io::SeekFrom::Start(start))
        .map_err(image::ImageError::IoError)?;
    let Some((true, longer)) = read else {
        return Ok(None);
    };
    let eighth = u32::from(longer).div_ceil(8);
    if !wanted(eighth) {
        return Ok(None);
    }
    jpeg_fitted(&mut inner, eighth, threads, REDUCED_BYTES)
}

/// A JPEG decoded at a reduced scale, shrunk to at most `side` on its
/// longer side and turned, as [`decode_fitted`] decodes it, where that
/// holds at most `bytes` (a progressive JPEG's coefficients at a quarter or
/// a half can be more than its pixels whole). `None`, `reader` back where
/// it was, where it is decoded whole: the caller decodes it, within its
/// own limits.
pub fn jpeg_fitted<R: BufRead + Seek>(
    reader: &mut R,
    side: u32,
    threads: Threads,
    bytes: u64,
) -> ImageResult<Option<DynamicImage>> {
    Ok(jpeg_reduced(reader, side, threads, bytes)?.map(|reduced| fitted(reduced, side)))
}

/// `image` shrunk to at most `side` on its longer side, then turned.
fn fitted((image, orientation): (DynamicImage, Orientation), side: u32) -> DynamicImage {
    let mut image = shrink(image, side);
    image.apply_orientation(orientation);
    image
}

/// The part `region` of the picture `reader` reads (left, top, width and
/// height in its own pixels, turned as it is shown, `own` its size so), at
/// the scale where its longer side is `side` (ADR-146, 5): what a zoom
/// shows of a picture past what is held of it whole. Decoded as
/// [`decode_fitted`] decodes it, at the least reduced scale covering
/// `side`, then the region alone cut, turned and shrunk to its size at
/// that scale: the whole is never turned or shrunk. Within `image`'s own
/// limits.
pub fn decode_region<R: BufRead + Seek>(
    reader: ImageReader<R>,
    own: (u32, u32),
    region: [u32; 4],
    side: u32,
    threads: Threads,
) -> ImageResult<DynamicImage> {
    // A JPEG's rows around the region alone, a PNG's rows down to it, a
    // TIFF's strips or tiles it meets, a JPEG XL's or a HEIF's region
    // alone; any other picture whole (a WebP: `image-webp` decodes a frame
    // only whole).
    let format = reader.format();
    let mut inner = reader.into_inner();
    let band = if format == Some(ImageFormat::Jpeg) {
        match jpeg_band(&mut inner, own, region, side, threads, REDUCED_BYTES)? {
            Some(band) => band,
            None => Band::whole(whole(ImageReader::with_format(inner, ImageFormat::Jpeg))?),
        }
    } else if format == Some(ImageFormat::Png) {
        region_alone(crate::png_rows::Decoder::new(inner)?, own, region)?
    } else if format == Some(ImageFormat::Tiff) && crate::tiff_chunks::by_chunks(&mut inner)? {
        region_alone(crate::tiff_chunks::Decoder::new(inner)?, own, region)?
    } else if format.is_none() && crate::jxl::starts(inner.fill_buf()?) {
        region_alone(crate::jxl::Decoder::new(inner)?, own, region)?
    } else if format.is_none() && crate::heif::starts(inner.fill_buf()?) {
        region_alone(crate::heif::Decoder::new(inner)?, own, region)?
    } else {
        let reader = match format {
            Some(format) => ImageReader::with_format(inner, format),
            None => ImageReader::new(inner).with_guessed_format()?,
        };
        Band::whole(whole(reader)?)
    };
    let (decoded, orientation) = (band.image, band.orientation);
    let longer = own.0.max(own.1).max(1);
    // Where the region lies in the pixels as stored, then in those decoded
    // (the whole picture at the scale decoded, `band.size`), then in the
    // band held.
    let [left, top, width, height] = stored_region(region, own, orientation);
    let stored = turned(own.0, own.1, orientation);
    let (across, down) = (
        f64::from(band.size.0) / f64::from(stored.0.max(1)),
        f64::from(band.size.1) / f64::from(stored.1.max(1)),
    );
    let from = |at: u32, scale: f64, most: u32| ((f64::from(at) * scale).floor() as u32).min(most);
    let to = |at: u32, scale: f64, most: u32| ((f64::from(at) * scale).ceil() as u32).min(most);
    let columns = band.left..band.left + decoded.width();
    let rows = band.top..band.top + decoded.height();
    let (x0, y0) = (
        from(left, across, columns.end).clamp(columns.start, columns.end.saturating_sub(1)),
        from(top, down, rows.end).clamp(rows.start, rows.end.saturating_sub(1)),
    );
    let (x1, y1) = (
        to(left + width, across, columns.end).max(x0 + 1),
        to(top + height, down, rows.end).max(y0 + 1),
    );
    let mut cut = decoded.crop_imm(x0 - columns.start, y0 - rows.start, x1 - x0, y1 - y0);
    cut.apply_orientation(orientation);
    // Its size where the whole's longer side is `side`, never larger than
    // what was cut.
    let scale = f64::from(side.min(longer)) / f64::from(longer);
    let at = |length: u32, most: u32| ((f64::from(length) * scale).round() as u32).clamp(1, most);
    let (to_width, to_height) = (at(region[2], cut.width()), at(region[3], cut.height()));
    Ok(resized(cut, to_width, to_height))
}

/// Where `region` of a picture shown `own` (left, top, width, height,
/// turned as `orientation` turns it) lies in its pixels as stored.
fn stored_region(region: [u32; 4], own: (u32, u32), orientation: Orientation) -> [u32; 4] {
    let [x, y, width, height] = region;
    let (shown_width, shown_height) = own;
    // From the right or the bottom of what is shown.
    let right = shown_width.saturating_sub(x + width);
    let bottom = shown_height.saturating_sub(y + height);
    match orientation {
        Orientation::NoTransforms => [x, y, width, height],
        Orientation::FlipHorizontal => [right, y, width, height],
        Orientation::FlipVertical => [x, bottom, width, height],
        Orientation::Rotate180 => [right, bottom, width, height],
        // A quarter turn: what is shown across was stored down.
        Orientation::Rotate90 => [y, right, height, width],
        Orientation::Rotate270 => [bottom, x, height, width],
        Orientation::Rotate90FlipH => [y, x, height, width],
        Orientation::Rotate270FlipH => [bottom, right, height, width],
    }
}

/// `image` resized to `width` × `height` as [`shrink`] does.
fn resized(image: DynamicImage, width: u32, height: u32) -> DynamicImage {
    if (image.width(), image.height()) == (width, height) {
        return image;
    }
    let mut out = DynamicImage::new(width, height, image.color());
    let done = fast_image_resize::Resizer::new().resize(
        &image,
        &mut out,
        &fast_image_resize::ResizeOptions::new(),
    );
    match done {
        Ok(()) => out,
        // A layout it does not take (none `image` decodes to): `image`'s
        // own filter.
        Err(_) => image.resize_exact(width, height, image::imageops::FilterType::Triangle),
    }
}

/// `image` shrunk to at most `side` on its longer side, its proportions
/// kept, by a Lanczos filter weighted by each pixel's alpha
/// (`fast_image_resize`, ADR-146): a picture decoded larger than it is
/// shown (a JPEG at the reduced scale just above, any other whole). One no
/// larger is given back as it is. `image`'s own filter took 98 ms to shrink
/// 12 megapixels to 2048 pixels, where this takes 14, and averaged the
/// colour of transparent pixels into their neighbours.
pub fn shrink(image: DynamicImage, side: u32) -> DynamicImage {
    let (width, height) = (image.width(), image.height());
    let longer = width.max(height);
    if side == 0 || longer <= side {
        return image;
    }
    let scale = f64::from(side) / f64::from(longer);
    let fit = |length: u32| ((f64::from(length) * scale).round() as u32).clamp(1, side);
    let (width, height) = (fit(width), fit(height));
    resized(image, width, height)
}

/// The longer side a picture of `width` × `height` pixels is held at with
/// at most `pixels` of them, its proportions kept: past its own side where
/// it has fewer (a drawing, drawn at any size), else as many as allowed. A
/// side alone bounded what a picture was held at, and a tall or wide one
/// was held at a fraction of what a square one was (ADR-146).
pub fn side_within(width: u32, height: u32, pixels: u64) -> u32 {
    let (width, height) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let scale = (pixels as f64 / (width * height)).sqrt();
    (width.max(height) * scale)
        .floor()
        .clamp(1., f64::from(u32::MAX)) as u32
}

/// The picture decoded whole, within `image`'s own limits, and how to turn
/// it.
fn whole<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(DynamicImage, Orientation)> {
    let mut decoder = reader.into_decoder()?;
    let orientation = orientation(&mut decoder);
    let image = within(decoder, image::Limits::default())?;
    Ok((encoded(image), orientation))
}

/// What a JPEG's reduced decode may hold, as `image`'s own limit.
const REDUCED_BYTES: u64 = 512 << 20;

/// A JPEG's decode at a reduced scale, as its header tells it.
struct Reduced {
    width: u16,
    height: u16,
    /// Of each pixel: 1 in grey, else 3.
    channels: u64,
    /// What the decode holds at most, in bytes.
    holds: u64,
}

/// How the JPEG `jpeg` reads is decoded at the least of its reduced scales
/// (an eighth, a quarter, a half) still at least `side` on its longer
/// side, read from its header. `None` when no smaller scale fits (a
/// lossless JPEG has none), or for a JPEG this decoder reads otherwise than
/// `image` (CMYK, 16 bits, arithmetic coding): it is decoded whole.
fn reduced<R: std::io::Read>(jpeg: &mut jpeg_decoder::Decoder<R>, side: u32) -> Option<Reduced> {
    jpeg.read_info().ok()?;
    let full = jpeg.info()?;
    let channels = match full.pixel_format {
        jpeg_decoder::PixelFormat::L8 => 1,
        jpeg_decoder::PixelFormat::RGB24 => 3,
        _ => return None,
    };
    let asked = u16::try_from(side).unwrap_or(u16::MAX);
    let (width, height) = jpeg.scale(asked, asked).ok()?;
    if (width, height) == (full.width, full.height) {
        return None;
    }
    // Its pixels, as each component's plane and as the picture.
    let pixels = u64::from(width) * u64::from(height) * channels * 2;
    // A progressive JPEG keeps coefficients for the whole picture: two
    // bytes a sample of each component (every one at its fullest, as in
    // 4:4:4), or at an eighth one a block, its DC (`vendor/jpeg-decoder`).
    let samples = u64::from(u32::from(full.width).next_multiple_of(16))
        * u64::from(u32::from(full.height).next_multiple_of(16))
        * channels;
    let eighth = (width, height) == (full.width.div_ceil(8), full.height.div_ceil(8));
    let coefficients = match full.coding_process {
        jpeg_decoder::CodingProcess::DctProgressive if eighth => samples / 64 * 2,
        jpeg_decoder::CodingProcess::DctProgressive => samples * 2,
        _ => 0,
    };
    Some(Reduced {
        width,
        height,
        channels,
        holds: pixels + coefficients,
    })
}

/// What a region is cut from (`decode_region`): part of the picture
/// decoded at a scale, from `left` and `top`, of the whole's `size` at that
/// scale, and how to turn it.
struct Band {
    image: DynamicImage,
    left: u32,
    top: u32,
    size: (u32, u32),
    orientation: Orientation,
}

impl Band {
    /// A picture decoded whole.
    fn whole((image, orientation): (DynamicImage, Orientation)) -> Band {
        Band {
            size: (image.width(), image.height()),
            image,
            left: 0,
            top: 0,
            orientation,
        }
    }
}

/// A decoder that reads a region of its picture alone: a JPEG XL's groups
/// (`jxl`), a HEIF's tiles (`heif`, ADR-148 amended), a PNG's rows down to
/// it (`png_rows`), a TIFF's strips or tiles (`tiff_chunks`, ADR-146
/// amended).
pub(crate) trait Regional: ImageDecoder {
    /// Read `region` alone (left, top, width, height, in its pixels as it
    /// decodes them: as shown where it turns them itself, a JPEG XL's or a
    /// HEIF's; as stored where its orientation says how to turn them, a
    /// PNG's or a TIFF's), within the picture (`inside`): what
    /// [`Regional::region`] says.
    fn cut(&mut self, region: [u32; 4]);
    /// What is read: the whole, or what [`Regional::cut`] said.
    fn region(&self) -> [u32; 4];
}

/// `[left, top, width, height]` within a picture of `size`, at least a
/// pixel.
pub(crate) fn inside([left, top, width, height]: [u32; 4], size: (u32, u32)) -> [u32; 4] {
    let left = left.min(size.0.saturating_sub(1));
    let top = top.min(size.1.saturating_sub(1));
    [
        left,
        top,
        width.clamp(1, size.0.saturating_sub(left).max(1)),
        height.clamp(1, size.1.saturating_sub(top).max(1)),
    ]
}

/// The region of a picture a zoom shows (`region`, of the picture as it is
/// shown, `own`), decoded alone by `decoder` at its full size, the only one
/// it is decoded at: where the region lies as stored, turned as its
/// orientation says (none where the decoder turns it itself, a JPEG XL's,
/// a HEIF's).
fn region_alone(
    mut decoder: impl Regional,
    own: (u32, u32),
    region: [u32; 4],
) -> ImageResult<Band> {
    let orientation = orientation(&mut decoder);
    let size = decoder.dimensions();
    let stored = turned(own.0, own.1, orientation);
    // In its pixels, should what was shown be of another size.
    let (across, down) = (
        f64::from(size.0) / f64::from(stored.0.max(1)),
        f64::from(size.1) / f64::from(stored.1.max(1)),
    );
    let [x, y, width, height] = stored_region(region, own, orientation);
    let at = |at: u32, scale: f64| (f64::from(at) * scale).floor() as u32;
    let end = |at: u32, scale: f64| (f64::from(at) * scale).ceil() as u32;
    let (left, top) = (at(x, across), at(y, down));
    decoder.cut([
        left,
        top,
        end(x + width, across).saturating_sub(left),
        end(y + height, down).saturating_sub(top),
    ]);
    let [left, top, ..] = decoder.region();
    Ok(Band {
        image: encoded(within(decoder, image::Limits::default())?),
        left,
        top,
        size,
        orientation,
    })
}

/// The rows of a JPEG a region of it shows, decoded alone (ADR-146
/// amended, `vendor/jpeg-decoder`'s 0006): at the least reduced scale
/// whose longer side is at least `side` (its full size where none is
/// smaller), the band of rows of blocks around the region's rows as
/// stored. Those above are read and never transformed, those below not
/// read: a region at the top of a photo of 45 megapixels holds its rows
/// alone, where the whole was 134 MB. `None`, `reader` back where it was,
/// for a JPEG this decoder reads otherwise than `image` (CMYK, 16 bits),
/// or one whose band holds more than `bytes` (a progressive JPEG keeps its
/// coefficients for the whole picture).
fn jpeg_band<R: BufRead + Seek>(
    reader: &mut R,
    own: (u32, u32),
    region: [u32; 4],
    side: u32,
    threads: Threads,
    bytes: u64,
) -> ImageResult<Option<Band>> {
    let start = reader
        .stream_position()
        .map_err(image::ImageError::IoError)?;
    let band = (|| {
        let mut jpeg = jpeg_decoder::Decoder::new(&mut *reader);
        jpeg.set_max_decoding_buffer_size(usize::try_from(bytes).ok()?);
        jpeg.set_worker_threads(threads == Threads::Components);
        jpeg.read_info().ok()?;
        let full = jpeg.info()?;
        let channels: u64 = match full.pixel_format {
            jpeg_decoder::PixelFormat::L8 => 1,
            jpeg_decoder::PixelFormat::RGB24 => 3,
            _ => return None,
        };
        let asked = u16::try_from(side).unwrap_or(u16::MAX);
        let (width, height) = jpeg.scale(asked, asked).ok()?;
        let orientation = jpeg
            .exif_data()
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms);
        // The region's rows as stored, at the scale decoded.
        let [_, top, _, rows] = stored_region(region, own, orientation);
        let stored = turned(own.0, own.1, orientation).1.max(1);
        let down = f64::from(height) / f64::from(stored);
        let first = (f64::from(top) * down).floor() as u32;
        let end = (f64::from(top + rows) * down).ceil() as u32;
        jpeg.set_rows(
            u16::try_from(first).ok()?,
            u16::try_from(end.min(u32::from(height))).ok()?,
        );
        let (band_top, band_rows) = jpeg.rows_decoded()?;
        // What it holds: the band's pixels, as planes and as the picture,
        // and a progressive JPEG's coefficients for the whole picture.
        let pixels = u64::from(width) * u64::from(band_rows) * channels * 2;
        let coefficients = match full.coding_process {
            jpeg_decoder::CodingProcess::DctProgressive => {
                u64::from(u32::from(full.width).next_multiple_of(16))
                    * u64::from(u32::from(full.height).next_multiple_of(16))
                    * channels
                    * 2
            }
            _ => 0,
        };
        if pixels + coefficients > bytes {
            return None;
        }
        let pixels = jpeg.decode().ok()?;
        let (width, band_rows) = (u32::from(width), u32::from(band_rows));
        // The pixels of the size it said, or none: see `jpeg_reduced`.
        if pixels.len() as u64 != u64::from(width) * u64::from(band_rows) * channels {
            return None;
        }
        let image = if channels == 1 {
            DynamicImage::ImageLuma8(image::GrayImage::from_raw(width, band_rows, pixels)?)
        } else {
            DynamicImage::ImageRgb8(image::RgbImage::from_raw(width, band_rows, pixels)?)
        };
        Some(Band {
            image,
            left: 0,
            top: u32::from(band_top),
            size: (width, u32::from(height)),
            orientation,
        })
    })();
    if band.is_none() {
        reader
            .seek(std::io::SeekFrom::Start(start))
            .map_err(image::ImageError::IoError)?;
    }
    Ok(band)
}

/// A JPEG decoded as [`reduced`] says, and how to turn it: a photo eight
/// times the side has an eighth of its pixels computed and held. `None`,
/// `reader` back where it was, where it is decoded whole: no smaller scale,
/// or more held than `bytes`.
fn jpeg_reduced<R: BufRead + Seek>(
    reader: &mut R,
    side: u32,
    threads: Threads,
    bytes: u64,
) -> ImageResult<Option<(DynamicImage, Orientation)>> {
    let start = reader
        .stream_position()
        .map_err(image::ImageError::IoError)?;
    let decoded = (|| {
        let mut jpeg = jpeg_decoder::Decoder::new(&mut *reader);
        // The caller's bound, as `holds` is weighed against it below.
        jpeg.set_max_decoding_buffer_size(usize::try_from(bytes).ok()?);
        jpeg.set_worker_threads(threads == Threads::Components);
        // Told before a pixel is decoded: one `image` reads otherwise, or
        // too large, is decoded once, whole.
        let Reduced {
            width,
            height,
            channels,
            holds,
        } = reduced(&mut jpeg, side)?;
        if holds > bytes {
            return None;
        }
        let orientation = jpeg
            .exif_data()
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms);
        let pixels = jpeg.decode().ok()?;
        let (width, height) = (u32::from(width), u32::from(height));
        // The pixels of the size it said, or no picture: `from_raw` takes a
        // buffer longer than it needs, and pixels of another size would be
        // drawn as stripes of the picture's first rows.
        if pixels.len() as u64 != u64::from(width) * u64::from(height) * channels {
            return None;
        }
        let image = if channels == 1 {
            DynamicImage::ImageLuma8(image::GrayImage::from_raw(width, height, pixels)?)
        } else {
            DynamicImage::ImageRgb8(image::RgbImage::from_raw(width, height, pixels)?)
        };
        Some((image, orientation))
    })();
    if decoded.is_none() {
        reader
            .seek(std::io::SeekFrom::Start(start))
            .map_err(image::ImageError::IoError)?;
    }
    Ok(decoded)
}

/// Linear light encoded as sRGB, what a screen expects: an `OpenEXR`
/// picture's floats, clipped at white. Alpha is not light, and stays
/// linear. Any other picture is already encoded.
fn encoded(image: DynamicImage) -> DynamicImage {
    fn srgb(linear: f32) -> u8 {
        let linear = linear.clamp(0., 1.);
        let encoded = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1. / 2.4) - 0.055
        };
        (encoded * 255.).round() as u8
    }
    match image {
        DynamicImage::ImageRgb32F(linear) => DynamicImage::ImageRgb8(image::RgbImage::from_fn(
            linear.width(),
            linear.height(),
            |x, y| image::Rgb(linear.get_pixel(x, y).0.map(srgb)),
        )),
        DynamicImage::ImageRgba32F(linear) => DynamicImage::ImageRgba8(image::RgbaImage::from_fn(
            linear.width(),
            linear.height(),
            |x, y| {
                let [r, g, b, a] = linear.get_pixel(x, y).0;
                image::Rgba([
                    srgb(r),
                    srgb(g),
                    srgb(b),
                    (a.clamp(0., 1.) * 255.).round() as u8,
                ])
            },
        )),
        other => other,
    }
}

/// A tag that cannot be read turns nothing: the pixels are still a picture.
fn orientation(decoder: &mut impl ImageDecoder) -> Orientation {
    decoder.orientation().unwrap_or(Orientation::NoTransforms)
}

/// `width` × `height` after `orientation`: a quarter turn swaps them.
fn turned(width: u32, height: u32, orientation: Orientation) -> (u32, u32) {
    match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        Orientation::NoTransforms
        | Orientation::Rotate180
        | Orientation::FlipHorizontal
        | Orientation::FlipVertical => (width, height),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// A 3 × 2 JPEG, left column red, whose EXIF says `tag`.
    fn jpeg(tag: u16) -> Vec<u8> {
        jpeg_of(3, 2, tag)
    }

    /// A `width` × `height` JPEG, its left third red, the rest blue, whose
    /// EXIF says `tag`.
    fn jpeg_of(width: u32, height: u32, tag: u16) -> Vec<u8> {
        let pixels = image::RgbImage::from_fn(width, height, |x, _| {
            if x < width.div_ceil(3) {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 100)
            .encode_image(&pixels)
            .unwrap();
        // An APP1 segment right after SOI: "Exif\0\0", then the EXIF.
        let mut segment = b"Exif\0\0".to_vec();
        segment.extend_from_slice(&exif(tag));
        let mut out = plain[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&u16::try_from(segment.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(&segment);
        out.extend_from_slice(&plain[2..]);
        out
    }

    /// EXIF saying Orientation `tag`: a little-endian TIFF header and one
    /// directory holding it.
    fn exif(tag: u16) -> Vec<u8> {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&tag.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes()); // no next directory
        tiff
    }

    fn reader(bytes: &[u8]) -> ImageReader<Cursor<&[u8]>> {
        ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .unwrap()
    }

    #[test]
    fn a_picture_as_stored_says_how_to_turn_it() {
        assert_eq!(stored(reader(&jpeg(6))).unwrap(), (3, 2, 6));
        assert_eq!(stored(reader(&jpeg(1))).unwrap(), (3, 2, 1));
    }

    #[test]
    fn a_quarter_turn_stands_the_picture_up() {
        let bytes = jpeg(6); // turn 90° clockwise to view
        assert_eq!(size(reader(&bytes)).unwrap(), (2, 3));
        let image = decode(reader(&bytes), image::Limits::default())
            .unwrap()
            .to_rgb8();
        assert_eq!(image.dimensions(), (2, 3));
        // The red left column is now the top row.
        let [r, _, b] = image.get_pixel(1, 0).0;
        assert!(r > 200 && b < 60, "top row should be red");
        let [r, _, b] = image.get_pixel(1, 2).0;
        assert!(r < 60 && b > 200, "bottom row should be blue");
    }

    #[test]
    fn a_large_jpeg_is_decoded_reduced_and_still_turned() {
        // An eighth of 1200 × 800 is 150 × 100: what a side of 100 needs.
        let bytes = jpeg_of(1200, 800, 6);
        let mut cursor = Cursor::new(bytes.as_slice());
        let (reduced, orientation) = jpeg_reduced(&mut cursor, 100, Threads::Caller, REDUCED_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!((reduced.width(), reduced.height()), (150, 100));
        assert_eq!(orientation, Orientation::Rotate90);
        let fitted = decode_fitted(reader(&bytes), 100, Threads::Caller)
            .unwrap()
            .to_rgb8();
        assert_eq!(fitted.dimensions(), (67, 100));
        // The red left third is now the top.
        let [r, _, b] = fitted.get_pixel(33, 5).0;
        assert!(r > 200 && b < 60, "top should be red");
        let [r, _, b] = fitted.get_pixel(33, 95).0;
        assert!(r < 60 && b > 200, "bottom should be blue");
        // As the whole decode shrunk would have it.
        let whole = decode(reader(&bytes), image::Limits::default())
            .unwrap()
            .thumbnail(100, 100)
            .to_rgb8();
        assert_eq!(whole.dimensions(), fitted.dimensions());
        let differ = whole
            .as_raw()
            .iter()
            .zip(fitted.as_raw())
            .filter(|(a, b)| a.abs_diff(**b) > 40)
            .count();
        assert!(differ * 50 < whole.as_raw().len(), "{differ} bytes differ");
        // A small one has no smaller scale: decoded whole, from where it was.
        let small = jpeg(6);
        let mut cursor = Cursor::new(small.as_slice());
        assert!(
            jpeg_reduced(&mut cursor, 128, Threads::Caller, REDUCED_BYTES)
                .unwrap()
                .is_none()
        );
        assert_eq!(cursor.position(), 0);
        let fitted = decode_fitted(reader(&small), 128, Threads::Caller).unwrap();
        assert_eq!((fitted.width(), fitted.height()), (2, 3));
    }

    /// 1200 × 800, its left third red and the rest a ramp of blue,
    /// progressive as libjpeg writes it: its first scan brings every
    /// component's DC coefficients, less their lowest bit.
    const PROGRESSIVE: &[u8] = include_bytes!("../testdata/synthetic-progressive.jpg");

    #[test]
    fn a_progressive_jpeg_at_an_eighth_is_read_to_its_first_scan() {
        // Cut after the first scan: the next one's marker and nothing of it.
        let next = PROGRESSIVE
            .windows(2)
            .enumerate()
            .filter(|(_, w)| *w == [0xFF, 0xDA])
            .nth(1)
            .map(|(at, _)| at)
            .expect("a second scan");
        let cut = &PROGRESSIVE[..next + 2];
        assert!(
            jpeg_decoder::Decoder::new(Cursor::new(cut))
                .decode()
                .is_err(),
            "the cut file is no picture at its full size"
        );
        let reduced = |bytes: &[u8]| {
            let (image, _) =
                jpeg_reduced(&mut Cursor::new(bytes), 100, Threads::Caller, REDUCED_BYTES)
                    .unwrap()
                    .unwrap();
            image.to_rgb8()
        };
        let read = reduced(PROGRESSIVE);
        assert_eq!(read.dimensions(), (150, 100));
        assert_eq!(reduced(cut), read, "nothing after the first scan is read");
        let fitted = decode_fitted(reader(cut), 100, Threads::Caller).unwrap();
        assert_eq!((fitted.width(), fitted.height()), (100, 67));
        // As the whole decode shrunk has it, give or take the bit the first
        // scan leaves out and the filters' edges.
        let whole = decode(reader(PROGRESSIVE), image::Limits::default())
            .unwrap()
            .thumbnail(150, 100)
            .to_rgb8();
        let differ = whole
            .as_raw()
            .iter()
            .zip(read.as_raw())
            .filter(|(a, b)| a.abs_diff(**b) > 40)
            .count();
        assert!(differ * 50 < whole.as_raw().len(), "{differ} bytes differ");
        let [r, _, b] = read.get_pixel(20, 50).0;
        assert!(r > 200 && b < 60, "the left is red");
        let [r, _, b] = read.get_pixel(100, 5).0;
        assert!(r < 60 && b > 200, "the top of the right is blue");
    }

    /// 800 × 600, a plasma (`magick -seed 3 -size 800x600 plasma:fractal
    /// -blur 0x1 -quality 30 -sampling-factor 2x2`), its coefficients
    /// written four ways by libjpeg-turbo's jpegtran, without a loss
    /// (`-copy none`): sequential; progressive as libjpeg writes it, every
    /// DC less its lowest bit first (`-progressive`); and by the scripts
    /// beside the two others (`-scans`).
    const PLASMA: &[u8] = include_bytes!("../testdata/synthetic-plasma-sequential.jpg");
    const PLASMA_PROGRESSIVE: &[u8] =
        include_bytes!("../testdata/synthetic-plasma-progressive.jpg");
    /// `0,1,2: 0-0, 0, 0;`, then libjpeg's AC scans: every DC whole first.
    const PLASMA_DC_WHOLE: &[u8] = include_bytes!("../testdata/synthetic-plasma-dc-whole.jpg");
    /// `0: 0-0, 0, 1; 0: 1-63, 0, 1; 0: 1-63, 1, 0; 0: 0-0, 1, 0; 1: 0-0, 0,
    /// 0; 2: 0-0, 0, 0; 1: 1-63, 0, 0; 2: 1-63, 0, 0;`: the luma's AC,
    /// refined, and its DC's last bit, before the colours' DC.
    const PLASMA_DC_APART: &[u8] = include_bytes!("../testdata/synthetic-plasma-dc-apart.jpg");

    /// A progressive JPEG at an eighth draws what a decoder reading every
    /// scan draws, as a sequential one, where its DC came whole. Where its
    /// first scan left out a bit, a pixel moves a few levels either way, and
    /// none on average: drawn as kept, the DC made it 4 levels darker on
    /// average, 10 at most.
    #[test]
    fn a_progressive_jpeg_at_an_eighth_is_drawn_as_a_sequential_one() {
        let reduced = |bytes: &[u8]| {
            let (image, _) =
                jpeg_reduced(&mut Cursor::new(bytes), 100, Threads::Caller, REDUCED_BYTES)
                    .unwrap()
                    .unwrap();
            image.to_rgb8()
        };
        let sequential = reduced(PLASMA);
        assert_eq!(sequential.dimensions(), (100, 75));
        assert_eq!(reduced(PLASMA_DC_WHOLE), sequential, "a DC whole first");
        assert_eq!(
            reduced(PLASMA_DC_APART),
            sequential,
            "AC scans before the last DC"
        );
        let read = reduced(PLASMA_PROGRESSIVE);
        for channel in 0..3 {
            let differences: Vec<i32> = read
                .pixels()
                .zip(sequential.pixels())
                .map(|(a, b)| i32::from(a.0[channel]) - i32::from(b.0[channel]))
                .collect();
            let most = differences.iter().map(|d| d.abs()).max().unwrap();
            let mean = f64::from(differences.iter().sum::<i32>()) / differences.len() as f64;
            assert!(most <= 6, "channel {channel}: a pixel moved {most} levels");
            assert!(
                mean.abs() < 0.5,
                "channel {channel}: {mean} levels on average"
            );
        }
    }

    /// Whether a decode takes threads of its own changes nothing of what it
    /// draws: at an eighth, and at a half, where the threads start.
    #[test]
    fn threads_draw_the_same_picture() {
        for bytes in [PLASMA, PLASMA_PROGRESSIVE] {
            for side in [100, 300] {
                let fitted = |threads| {
                    decode_fitted(reader(bytes), side, threads)
                        .unwrap()
                        .to_rgb8()
                };
                assert_eq!(fitted(Threads::Caller), fitted(Threads::Components));
            }
        }
    }

    /// The plasma at 400 × 300, encoded again at quality 30 in 4:4:4, 4:2:2
    /// and grey (`cjpeg -sample 1x1`, `-sample 2x1`, `-grayscale`), each
    /// sequential (`jpegtran -optimize`) and progressive with a restart
    /// marker every row of blocks (`-restart 1`, `-scans`): in 4:4:4 by
    /// `PLASMA_DC_APART`'s script, the luma's AC before the colours' DC; in
    /// 4:2:2 every DC whole first, then each component's AC; in grey its DC,
    /// then its AC in two bands.
    const PLASMA_444: &[u8] = include_bytes!("../testdata/synthetic-plasma-444.jpg");
    const PLASMA_444_RESTART: &[u8] =
        include_bytes!("../testdata/synthetic-plasma-444-restart.jpg");
    const PLASMA_422: &[u8] = include_bytes!("../testdata/synthetic-plasma-422.jpg");
    const PLASMA_422_RESTART: &[u8] =
        include_bytes!("../testdata/synthetic-plasma-422-restart.jpg");
    const PLASMA_GREY: &[u8] = include_bytes!("../testdata/synthetic-plasma-grey.jpg");
    const PLASMA_GREY_RESTART: &[u8] =
        include_bytes!("../testdata/synthetic-plasma-grey-restart.jpg");

    /// At an eighth, a progressive JPEG whose DC comes whole draws exactly
    /// its sequential one in every layout: 4:4:4, 4:2:2 and grey, its AC
    /// scans passed over across their restart markers.
    #[test]
    fn every_layout_at_an_eighth_is_drawn_as_its_sequential_one() {
        let reduced = |bytes: &[u8]| {
            let mut cursor = Cursor::new(bytes);
            let (image, _) = jpeg_reduced(&mut cursor, 50, Threads::Caller, REDUCED_BYTES)
                .unwrap()
                .unwrap();
            image
        };
        for (layout, sequential, progressive) in [
            ("4:4:4", PLASMA_444, PLASMA_444_RESTART),
            ("4:2:2", PLASMA_422, PLASMA_422_RESTART),
            ("grey", PLASMA_GREY, PLASMA_GREY_RESTART),
        ] {
            let drawn = reduced(sequential);
            assert_eq!((drawn.width(), drawn.height()), (50, 38), "{layout}");
            assert_eq!(drawn, reduced(progressive), "{layout}");
        }
        assert!(reduced(PLASMA_GREY).as_luma8().is_some(), "grey stays grey");
    }

    /// A reduced decode holds what its caller allows, or the picture is
    /// decoded whole: a progressive JPEG at a half keeps two bytes a sample
    /// of each component for the whole picture (800 × 608 × 3 × 2, 2.9 MB,
    /// with 0.7 MB of pixels), at an eighth one coefficient a block; a
    /// sequential one only its pixels.
    #[test]
    fn a_reduced_decode_holds_at_most_what_its_caller_allows() {
        const MOST: u64 = 1 << 20;
        let fitted = |bytes: &[u8], side| {
            let mut cursor = Cursor::new(bytes);
            let image = jpeg_fitted(&mut cursor, side, Threads::Caller, MOST).unwrap();
            if image.is_none() {
                assert_eq!(cursor.position(), 0, "decoded whole from where it was");
            }
            image.map(|image| (image.width(), image.height()))
        };
        assert_eq!(
            fitted(PLASMA, 300),
            Some((300, 225)),
            "sequential at a half"
        );
        assert_eq!(
            fitted(PLASMA_PROGRESSIVE, 100),
            Some((100, 75)),
            "at an eighth"
        );
        assert_eq!(
            fitted(PLASMA_PROGRESSIVE, 300),
            None,
            "progressive at a half"
        );
        let mut cursor = Cursor::new(PLASMA_PROGRESSIVE);
        let image = jpeg_fitted(&mut cursor, 300, Threads::Caller, 4 << 20).unwrap();
        assert_eq!(image.map(|i| (i.width(), i.height())), Some((300, 225)));
    }

    /// Allowed short of its side, a JPEG is decoded at the smaller scale
    /// that reaches what it may be: 1200 × 800 asked at 640, at least 576,
    /// is its half (600 × 400), not its whole shrunk; any other picture
    /// is its side.
    #[test]
    fn a_picture_may_be_decoded_a_little_short() {
        let bytes = jpeg_of(1200, 800, 1);
        let quick = decode_fitted_within(reader(&bytes), 576, 640, Threads::Caller).unwrap();
        assert_eq!((quick.width(), quick.height()), (600, 400));
        let exact = decode_fitted_within(reader(&bytes), 640, 640, Threads::Caller).unwrap();
        assert_eq!((exact.width(), exact.height()), (640, 427));
        // Short by more than allowed: the scale above, shrunk.
        let further = decode_fitted_within(reader(&bytes), 620, 640, Threads::Caller).unwrap();
        assert_eq!(further.width(), 640);
        let png = {
            let mut bytes = Vec::new();
            DynamicImage::new_rgb8(1200, 800)
                .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
                .unwrap();
            bytes
        };
        let other = decode_fitted_within(reader(&png), 576, 640, Threads::Caller).unwrap();
        assert_eq!(other.width(), 640);
    }

    /// A progressive JPEG carries its eighth, read to its DC scans, where
    /// it is wanted; a sequential one carries none.
    #[test]
    fn a_progressive_jpeg_has_its_eighth_first() {
        let below = |side: u32| move |longer: u32| longer < side;
        let eighth = carried(reader(PLASMA_PROGRESSIVE), below(400), Threads::Caller)
            .unwrap()
            .expect("an eighth");
        assert_eq!((eighth.width(), eighth.height()), (100, 75));
        // As the eighth decoded for itself (`a_progressive_jpeg_at_an_eighth_…`).
        let fitted = decode_fitted(reader(PLASMA_PROGRESSIVE), 100, Threads::Caller).unwrap();
        assert_eq!(eighth.to_rgb8(), fitted.to_rgb8());
        assert!(
            carried(reader(PLASMA_PROGRESSIVE), below(100), Threads::Caller)
                .unwrap()
                .is_none(),
            "not wanted"
        );
        assert!(
            carried(reader(PLASMA), |_| true, Threads::Caller)
                .unwrap()
                .is_none(),
            "sequential"
        );
        let png = {
            let mut bytes = Vec::new();
            DynamicImage::new_rgb8(800, 600)
                .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
                .unwrap();
            bytes
        };
        assert!(
            carried(reader(&png), |_| true, Threads::Caller)
                .unwrap()
                .is_none()
        );
    }

    /// 64 × 48, the plasma in eight bits, lossless (`cjpeg -lossless 1`), in
    /// RGB (`-rgb`) and in grey.
    const LOSSLESS_RGB: &[u8] = include_bytes!("../testdata/synthetic-lossless-rgb.jpg");
    const LOSSLESS_GREY: &[u8] = include_bytes!("../testdata/synthetic-lossless-grey.jpg");

    /// A lossless JPEG has no reduced scale, and is never decoded at one:
    /// the decoder drew its first rows at the smaller size, the desktop's
    /// cache kept them, and grey ones came with every pixel of the whole.
    #[test]
    fn a_lossless_jpeg_is_never_decoded_reduced() {
        for bytes in [LOSSLESS_RGB, LOSSLESS_GREY] {
            let mut jpeg = jpeg_decoder::Decoder::new(Cursor::new(bytes));
            assert_eq!(jpeg.scale(16, 16).unwrap(), (64, 48), "its full size");
            let mut cursor = Cursor::new(bytes);
            assert!(
                jpeg_reduced(&mut cursor, 16, Threads::Caller, REDUCED_BYTES)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(cursor.position(), 0, "decoded whole from where it was");
            // What the whole decode makes of it, if anything (zune-jpeg
            // reads no lossless JPEG): the picture, shrunk.
            if let Ok(fitted) = decode_fitted(reader(bytes), 16, Threads::Caller) {
                let whole = jpeg_decoder::Decoder::new(Cursor::new(bytes))
                    .decode()
                    .unwrap();
                let whole = if whole.len() == 64 * 48 {
                    DynamicImage::ImageLuma8(image::GrayImage::from_raw(64, 48, whole).unwrap())
                } else {
                    DynamicImage::ImageRgb8(image::RgbImage::from_raw(64, 48, whole).unwrap())
                };
                assert_eq!(fitted.to_rgb8(), shrink(whole, 16).to_rgb8());
            }
        }
    }

    /// A picture is shrunk by its pixels' alpha: a transparent pixel's
    /// colour bleeds into no visible one (`image`'s own filter made a
    /// transparent red beside an opaque blue purple).
    #[test]
    fn a_picture_shrinks_by_its_alpha() {
        let pixels = image::RgbaImage::from_fn(8, 2, |x, _| {
            if x < 4 {
                image::Rgba([255, 0, 0, 0])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let shrunk = shrink(DynamicImage::ImageRgba8(pixels), 2).to_rgba8();
        assert_eq!(shrunk.dimensions(), (2, 1));
        for pixel in shrunk.pixels() {
            let [r, _, b, a] = pixel.0;
            assert!(a == 0 || (r < 10 && b > 240), "{:?}", pixel.0);
        }
        // Never enlarged, and its proportions kept.
        let small = DynamicImage::new_rgb8(30, 20);
        assert_eq!(shrink(small.clone(), 100).width(), 30);
        let fitted = shrink(DynamicImage::new_rgb8(1200, 800), 100);
        assert_eq!((fitted.width(), fitted.height()), (100, 67));
    }

    /// A region cut from a picture as it is shown is the region of the
    /// whole turned, whatever its EXIF Orientation: each of the eight read
    /// back from where it was stored.
    #[test]
    fn a_region_is_cut_where_the_picture_is_shown() {
        // 7 × 5 as stored, each pixel its own colour.
        let stored = DynamicImage::ImageRgb8(image::RgbImage::from_fn(7, 5, |x, y| {
            image::Rgb([
                u8::try_from(x * 30).unwrap(),
                u8::try_from(y * 50).unwrap(),
                9,
            ])
        }));
        for exif in 1..=8 {
            let orientation = Orientation::from_exif(exif).unwrap();
            let mut shown = stored.clone();
            shown.apply_orientation(orientation);
            let own = (shown.width(), shown.height());
            let region = [1, 2, 2, 3];
            let [x, y, width, height] = stored_region(region, own, orientation);
            let mut cut = stored.crop_imm(x, y, width, height);
            cut.apply_orientation(orientation);
            assert_eq!(
                cut.to_rgb8(),
                shown.crop_imm(1, 2, 2, 3).to_rgb8(),
                "orientation {exif}"
            );
        }
    }

    /// A region decoded at a scale is the whole decoded at that scale, cut:
    /// a JPEG at its reduced scale, turned as it is shown.
    #[test]
    fn a_region_is_the_whole_at_its_scale_cut() {
        let bytes = jpeg_of(1200, 800, 6);
        // Shown 800 × 1200: its top third red, the rest blue.
        let region = decode_region(
            reader(&bytes),
            (800, 1200),
            [100, 300, 400, 600],
            600,
            Threads::Caller,
        )
        .unwrap()
        .to_rgb8();
        // At half its size: 200 × 300, from 50, 150 of the whole's 400 × 600.
        assert_eq!(region.dimensions(), (200, 300));
        // Shown rows 300 to 400 are red (the top third is 0 to 400), the
        // rest blue.
        let [r, _, b] = region.get_pixel(100, 10).0;
        assert!(r > 200 && b < 60, "the top is red");
        let [r, _, b] = region.get_pixel(100, 250).0;
        assert!(r < 60 && b > 200, "the bottom is blue");
        // Asked past the picture's own pixels, it is cut at its own.
        let whole = decode_region(
            reader(&bytes),
            (800, 1200),
            [0, 0, 800, 1200],
            5000,
            Threads::Caller,
        )
        .unwrap();
        assert_eq!((whole.width(), whole.height()), (800, 1200));
    }

    /// A JPEG's rows decoded alone (`vendor/jpeg-decoder`'s 0006) are the
    /// rows the whole decode draws, to the bit: sequential and progressive,
    /// every sampling, grey, restart intervals, at full and reduced scales,
    /// rows at the top, within and at the bottom.
    #[test]
    fn a_jpeg_s_rows_decoded_alone_are_the_whole_s() {
        let decode = |bytes: &[u8], side: u16, rows: Option<(u16, u16)>| {
            let mut jpeg = jpeg_decoder::Decoder::new(Cursor::new(bytes));
            jpeg.set_worker_threads(false);
            jpeg.scale(side, side).unwrap();
            if let Some((top, bottom)) = rows {
                jpeg.set_rows(top, bottom);
            }
            let pixels = jpeg.decode().unwrap();
            let info = jpeg.info().unwrap();
            let channels = if info.pixel_format == jpeg_decoder::PixelFormat::L8 {
                1
            } else {
                3
            };
            (pixels, info.width, jpeg.rows_decoded().unwrap(), channels)
        };
        for name in [
            "synthetic-plasma-sequential.jpg",
            "synthetic-plasma-progressive.jpg",
            "synthetic-plasma-dc-apart.jpg",
            "synthetic-plasma-422.jpg",
            "synthetic-plasma-422-restart.jpg",
            "synthetic-plasma-444.jpg",
            "synthetic-plasma-444-restart.jpg",
            "synthetic-plasma-grey.jpg",
            "synthetic-plasma-grey-restart.jpg",
        ] {
            let bytes = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("testdata")
                    .join(name),
            )
            .unwrap();
            // Full size, a half and a quarter.
            for side in [800, 400, 200] {
                let (whole, width, (_, height), channels) = decode(&bytes, side, None);
                let row = usize::from(width) * channels;
                for (top, bottom) in [(0, 9), (height / 3, height / 2), (height - 5, height)] {
                    let (band, _, (first, count), _) = decode(&bytes, side, Some((top, bottom)));
                    assert!(
                        first <= top && first + count >= bottom && count < height,
                        "{name} at {side}: rows {top}..{bottom} in {first}+{count} of {height}"
                    );
                    let start = usize::from(first) * row;
                    assert!(
                        band == whole[start..start + usize::from(count) * row],
                        "{name} at {side}: rows {first}+{count} differ"
                    );
                }
            }
        }
    }

    /// A region of a JPEG past its bound is cut from the rows it needs: as
    /// the whole decoded and cut, wherever it lies (a turned one's, from
    /// its rows as stored: `a_region_is_the_whole_at_its_scale_cut`).
    #[test]
    fn a_jpeg_region_is_cut_from_its_rows_alone() {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("testdata/synthetic-plasma-sequential.jpg"),
        )
        .unwrap();
        let whole = decode(reader(&bytes), image::Limits::default())
            .unwrap()
            .to_rgb8();
        for region in [[0, 0, 100, 40], [300, 250, 200, 90], [700, 560, 100, 40]] {
            let cut = decode_region(reader(&bytes), (800, 600), region, 800, Threads::Caller)
                .unwrap()
                .to_rgb8();
            let [x, y, width, height] = region;
            let expected = image::imageops::crop_imm(&whole, x, y, width, height).to_image();
            assert_eq!(cut.dimensions(), expected.dimensions());
            // `image` decodes the whole through zune-jpeg, the region
            // through jpeg-decoder: within rounding of each other.
            let differs = cut
                .pixels()
                .zip(expected.pixels())
                .filter(|(a, b)| a.0.iter().zip(b.0).any(|(a, b)| a.abs_diff(b) > 3))
                .count();
            assert!(
                differs * 200 < cut.pixels().len(),
                "{region:?}: {differs} pixels differ"
            );
        }
    }

    /// `length` bytes of noise, the same at each call: no filter or
    /// compression makes a picture of it trivial, and every byte is a
    /// sample (or, packed, a few).
    fn noise(length: usize) -> Vec<u8> {
        let mut state = 0x2545_F491_u32;
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state.to_le_bytes()[1]
            })
            .collect()
    }

    /// `samples` as stored in a PNG of `color` in `depth` (big-endian at
    /// sixteen bits, packed below eight), not interlaced, each row filtered
    /// by `filter`; with four colours, the last two partly transparent,
    /// where it is a palette's, and EXIF saying Orientation `turn` where
    /// given.
    fn png_of(
        (width, height): (u32, u32),
        (color, depth): (png::ColorType, png::BitDepth),
        filter: png::Filter,
        samples: &[u8],
        turn: Option<u16>,
    ) -> Vec<u8> {
        let mut info = png::Info::with_size(width, height);
        info.color_type = color;
        info.bit_depth = depth;
        info.exif_metadata = turn.map(|tag| exif(tag).into());
        let mut out = Vec::new();
        let mut encoder = png::Encoder::with_info(&mut out, info).unwrap();
        encoder.set_filter(filter);
        if color == png::ColorType::Indexed {
            encoder.set_palette(vec![200, 30, 30, 30, 200, 30, 30, 30, 200, 250, 250, 250]);
            encoder.set_trns(vec![255, 255, 128, 0]);
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(samples).unwrap();
        writer.finish().unwrap();
        out
    }

    /// `samples` (eight bits, `channels` a pixel) as an interlaced PNG of
    /// `color`, each of Adam7's seven passes in its rows, unfiltered: the
    /// encoder writes none.
    fn interlaced_png(
        (width, height): (u32, u32),
        color: png::ColorType,
        channels: u32,
        samples: &[u8],
    ) -> Vec<u8> {
        // Where each pass starts, and its steps across and down.
        const PASSES: [(u32, u32, u32, u32); 7] = [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ];
        let mut rows = Vec::new();
        for (left, top, across, down) in PASSES {
            for y in (top..height).step_by(down as usize) {
                if left >= width {
                    break;
                }
                rows.push(0);
                for x in (left..width).step_by(across as usize) {
                    let at = ((y * width + x) * channels) as usize;
                    rows.extend_from_slice(&samples[at..at + channels as usize]);
                }
            }
        }
        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut zlib, &rows).unwrap();
        let data = zlib.finish().unwrap();
        let mut info = png::Info::with_size(width, height);
        info.color_type = color;
        info.interlaced = true;
        let mut out = Vec::new();
        let mut writer = png::Encoder::with_info(&mut out, info)
            .unwrap()
            .write_header()
            .unwrap();
        writer.write_chunk(png::chunk::IDAT, &data).unwrap();
        writer.finish().unwrap();
        out
    }

    /// `samples` as a TIFF of `C`, in strips of `rows`, compressed and
    /// predicted as said, its Orientation tag `turn`.
    fn tiff_strips<C: tiff::encoder::colortype::ColorType>(
        (width, height): (u32, u32),
        samples: &[C::Inner],
        rows: u32,
        (compression, predictor): (tiff::encoder::Compression, tiff::tags::Predictor),
        turn: u16,
    ) -> Vec<u8>
    where
        [C::Inner]: tiff::encoder::TiffValue,
    {
        let mut out = Cursor::new(Vec::new());
        let mut tiff = tiff::encoder::TiffEncoder::new(&mut out)
            .unwrap()
            .with_compression(compression)
            .with_predictor(predictor);
        let mut image = tiff.new_image::<C>(width, height).unwrap();
        image.rows_per_strip(rows).unwrap();
        image
            .encoder()
            .write_tag(tiff::tags::Tag::Orientation, turn)
            .unwrap();
        image.write_data(samples).unwrap();
        out.into_inner()
    }

    /// `samples` (eight bits, three a pixel in RGB, four in RGBA) as a TIFF
    /// in tiles of `tile` × `tile` (a multiple of 16), each deflated, its
    /// Orientation tag `turn`: the encoder writes no tiles. Those on the
    /// right and the bottom run past the picture, padded.
    fn tiff_tiles(
        (width, height): (u32, u32),
        channels: u16,
        samples: &[u8],
        (tile, turn): (u32, u16),
    ) -> Vec<u8> {
        use tiff::tags::Tag;
        let pixel = usize::from(channels);
        let mut out = Cursor::new(Vec::new());
        let mut tiff = tiff::encoder::TiffEncoder::new(&mut out).unwrap();
        let mut directory = tiff.image_directory().unwrap();
        let (mut offsets, mut counts) = (Vec::new(), Vec::<u32>::new());
        for down in 0..height.div_ceil(tile) {
            for across in 0..width.div_ceil(tile) {
                let mut part = Vec::new();
                for y in down * tile..(down + 1) * tile {
                    for x in across * tile..(across + 1) * tile {
                        if x < width && y < height {
                            let at = (y * width + x) as usize * pixel;
                            part.extend_from_slice(&samples[at..at + pixel]);
                        } else {
                            part.extend(std::iter::repeat_n(0, pixel));
                        }
                    }
                }
                let mut zlib =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
                std::io::Write::write_all(&mut zlib, &part).unwrap();
                let part = zlib.finish().unwrap();
                let at = directory.write_data(part.as_slice()).unwrap();
                offsets.push(u32::try_from(at).unwrap());
                counts.push(u32::try_from(part.len()).unwrap());
            }
        }
        directory.write_tag(Tag::ImageWidth, width).unwrap();
        directory.write_tag(Tag::ImageLength, height).unwrap();
        directory
            .write_tag(Tag::BitsPerSample, &vec![8u16; pixel][..])
            .unwrap();
        directory.write_tag(Tag::Compression, 8u16).unwrap(); // deflate
        directory
            .write_tag(Tag::PhotometricInterpretation, 2u16)
            .unwrap(); // RGB
        directory.write_tag(Tag::SamplesPerPixel, channels).unwrap();
        if channels == 4 {
            directory.write_tag(Tag::ExtraSamples, 2u16).unwrap(); // alpha
        }
        directory.write_tag(Tag::TileWidth, tile).unwrap();
        directory.write_tag(Tag::TileLength, tile).unwrap();
        directory
            .write_tag(Tag::TileOffsets, offsets.as_slice())
            .unwrap();
        directory
            .write_tag(Tag::TileByteCounts, counts.as_slice())
            .unwrap();
        directory.write_tag(Tag::Orientation, turn).unwrap();
        directory.finish().unwrap();
        out.into_inner()
    }

    /// A region of the picture `bytes` holds, anywhere (its corners, a
    /// pixel, a band across, inside, the whole), is the whole decoded and
    /// cut, to the bit: as it is shown, turned as it says.
    fn regions_are_the_whole_cut(bytes: &[u8], what: &str) {
        let whole = decode(reader(bytes), image::Limits::default()).unwrap();
        let (width, height) = (whole.width(), whole.height());
        for region in [
            [0, 0, 13, 9],
            [width - 17, height - 11, 17, 11],
            [width - 1, 0, 1, 1],
            [0, height - 1, 1, 1],
            [width / 3, height / 4, width / 2, height / 3],
            [0, height / 2, width, 5],
            [width / 2, 0, 7, height],
            [0, 0, width, height],
        ] {
            let cut = decode_region(
                reader(bytes),
                (width, height),
                region,
                width.max(height),
                Threads::Caller,
            )
            .unwrap();
            let [x, y, across, down] = region;
            assert!(
                cut == whole.crop_imm(x, y, across, down),
                "{what}: {region:?} differs"
            );
        }
    }

    /// A PNG's region is read from its rows down to it, as the whole
    /// decoded and cut: every filter, every depth and colour `image` expands
    /// (a palette with transparency, two bits of grey, sixteen bits), turned
    /// by its EXIF; an interlaced one decoded whole and cut.
    #[test]
    fn a_png_region_is_read_from_its_rows() {
        use png::{BitDepth, ColorType, Filter};
        let sides = (97, 61);
        let pixels = 97 * 61;
        let rgb = noise(pixels * 3);
        for filter in [
            Filter::NoFilter,
            Filter::Sub,
            Filter::Up,
            Filter::Avg,
            Filter::Paeth,
            Filter::Adaptive,
        ] {
            let png = png_of(sides, (ColorType::Rgb, BitDepth::Eight), filter, &rgb, None);
            regions_are_the_whole_cut(&png, &format!("RGB, {filter:?}"));
        }
        for (what, color, samples) in [
            ("RGBA, 16 bits", ColorType::Rgba, noise(pixels * 8)),
            ("grey, 16 bits", ColorType::Grayscale, noise(pixels * 2)),
            (
                "grey and alpha, 16 bits",
                ColorType::GrayscaleAlpha,
                noise(pixels * 4),
            ),
        ] {
            let png = png_of(
                sides,
                (color, BitDepth::Sixteen),
                Filter::Paeth,
                &samples,
                None,
            );
            regions_are_the_whole_cut(&png, what);
        }
        // Rows of 97 samples of two bits: 25 bytes.
        let packed = noise(25 * 61);
        let png = png_of(
            sides,
            (ColorType::Indexed, BitDepth::Two),
            Filter::Up,
            &packed,
            None,
        );
        assert!(
            decode(reader(&png), image::Limits::default())
                .unwrap()
                .as_rgba8()
                .is_some(),
            "a palette with transparency is RGBA"
        );
        regions_are_the_whole_cut(&png, "a palette of two bits");
        let png = png_of(
            sides,
            (ColorType::Grayscale, BitDepth::Two),
            Filter::Sub,
            &packed,
            None,
        );
        regions_are_the_whole_cut(&png, "grey, two bits");
        for turn in [3, 6, 8] {
            let png = png_of(
                sides,
                (ColorType::Rgb, BitDepth::Eight),
                Filter::Adaptive,
                &rgb,
                Some(turn),
            );
            assert_eq!(
                size(reader(&png)).unwrap().0,
                if turn == 3 { 97 } else { 61 }
            );
            regions_are_the_whole_cut(&png, &format!("turned by {turn}"));
        }
        let png = interlaced_png(sides, ColorType::Rgb, 3, &rgb);
        assert_eq!(
            decode(reader(&png), image::Limits::default())
                .unwrap()
                .into_rgb8()
                .into_raw(),
            rgb,
            "an interlaced PNG as written"
        );
        regions_are_the_whole_cut(&png, "interlaced");
        let png = interlaced_png(sides, ColorType::Rgba, 4, &noise(pixels * 4));
        regions_are_the_whole_cut(&png, "interlaced, RGBA");
    }

    /// A TIFF's region is read from the strips or tiles it meets, as the
    /// whole decoded and cut: strips of a row to the whole picture,
    /// compressed three ways and predicted, eight and sixteen bits, floats,
    /// tiles cut where the picture ends, turned by its Orientation tag; a
    /// layout `image` draws otherwise than it is stored (CMYK) decoded whole.
    #[test]
    fn a_tiff_region_is_read_from_the_chunks_it_meets() {
        use tiff::encoder::Compression;
        use tiff::encoder::colortype::{CMYK8, Gray8, RGB8, RGB32Float, RGBA16};
        use tiff::encoder::compression::DeflateLevel;
        use tiff::tags::Predictor;
        let sides = (97, 61);
        let pixels = 97 * 61;
        let wide = |bytes: Vec<u8>| -> Vec<u16> {
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect()
        };
        let chunked =
            |bytes: &[u8]| crate::tiff_chunks::by_chunks(&mut Cursor::new(bytes)).unwrap();
        let rgb = noise(pixels * 3);
        let lzw = (Compression::Lzw, Predictor::Horizontal);
        for (rows, turn) in [(1, 1), (7, 6), (61, 1), (1000, 8)] {
            let tiff = tiff_strips::<RGB8>(sides, &rgb, rows, lzw, turn);
            assert!(
                chunked(&tiff),
                "RGB in strips of {rows} is read by its chunks"
            );
            regions_are_the_whole_cut(&tiff, &format!("RGB in strips of {rows}, turned by {turn}"));
        }
        let deflated = (
            Compression::Deflate(DeflateLevel::Fast),
            Predictor::Horizontal,
        );
        let tiff = tiff_strips::<RGBA16>(sides, &wide(noise(pixels * 8)), 5, deflated, 3);
        assert!(chunked(&tiff));
        regions_are_the_whole_cut(&tiff, "RGBA, 16 bits");
        let grey = noise(pixels);
        let tiff =
            tiff_strips::<Gray8>(sides, &grey, 9, (Compression::Packbits, Predictor::None), 1);
        assert!(chunked(&tiff));
        regions_are_the_whole_cut(&tiff, "grey, packed bits");
        let floats: Vec<f32> = noise(pixels * 3)
            .into_iter()
            .map(|byte| f32::from(byte) / 200.)
            .collect();
        let deflated = (Compression::Deflate(DeflateLevel::Fast), Predictor::None);
        let tiff = tiff_strips::<RGB32Float>(sides, &floats, 4, deflated, 1);
        assert!(chunked(&tiff));
        regions_are_the_whole_cut(&tiff, "floats");
        for (channels, turn) in [(3, 1), (4, 5), (3, 6)] {
            let samples = noise(pixels * usize::from(channels));
            let tiff = tiff_tiles(sides, channels, &samples, (16, turn));
            assert!(chunked(&tiff), "tiles are read by their chunks");
            regions_are_the_whole_cut(
                &tiff,
                &format!("tiles of {channels} samples, turned by {turn}"),
            );
        }
        let tiff = tiff_strips::<CMYK8>(sides, &noise(pixels * 4), 7, lzw, 6);
        assert!(!chunked(&tiff), "CMYK is decoded whole");
        regions_are_the_whole_cut(&tiff, "CMYK");
    }

    /// A region reads what it needs of a file: a PNG's rows down to it, a
    /// TIFF's strips or tiles it meets, a fraction of the file, counted as
    /// it is read.
    #[test]
    fn a_region_reads_what_it_needs() {
        use std::cell::Cell;
        use std::io::Read;
        struct Counted<'a> {
            bytes: Cursor<&'a [u8]>,
            read: &'a Cell<u64>,
        }
        impl Read for Counted<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let read = self.bytes.read(buf)?;
                self.read.set(self.read.get() + read as u64);
                Ok(read)
            }
        }
        impl Seek for Counted<'_> {
            fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
                self.bytes.seek(to)
            }
        }
        let sides = (300, 2000);
        let reads = |bytes: &[u8], format, region| {
            let count = Cell::new(0);
            let counted = Counted {
                bytes: Cursor::new(bytes),
                read: &count,
            };
            let reader = ImageReader::with_format(BufReader::new(counted), format);
            let cut = decode_region(reader, sides, region, 2000, Threads::Caller).unwrap();
            assert_eq!((cut.width(), cut.height()), (region[2], region[3]));
            let read = count.get();
            assert!(
                read * 4 < bytes.len() as u64,
                "{format:?}, {region:?}: {read} of {} bytes read",
                bytes.len()
            );
        };
        let rgb = noise(300 * 2000 * 3);
        let png = png_of(
            sides,
            (png::ColorType::Rgb, png::BitDepth::Eight),
            png::Filter::Adaptive,
            &rgb,
            None,
        );
        reads(&png, ImageFormat::Png, [100, 50, 100, 100]);
        let lzw = (tiff::encoder::Compression::Lzw, tiff::tags::Predictor::None);
        let tiff = tiff_strips::<tiff::encoder::colortype::RGB8>(sides, &rgb, 16, lzw, 1);
        reads(&tiff, ImageFormat::Tiff, [100, 1000, 100, 100]);
        // Tiles of 64: `tiff` inflates each through a buffer of 32 KiB,
        // read whole past a smaller tile's end.
        let tiff = tiff_tiles(sides, 3, &rgb, (64, 1));
        reads(&tiff, ImageFormat::Tiff, [100, 1000, 50, 50]);
    }

    /// What a region of an 8000 × 6000 PNG or TIFF takes, against the
    /// whole: its time, and what the decode held at most beyond what the
    /// process held before it (Linux's peak resident memory, set back
    /// before it). One case a process, as `SKWIR_REGION_CASE` says
    /// (`png`, `strips` or `tiles`, then `whole`, `top`, `middle` or
    /// `bottom`: `tiles:middle`): memory a decode let go of is the next
    /// one's, unseen.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "measures 48-megapixel pictures; run alone with --run-ignored only --no-capture"]
    fn a_large_region_measured() {
        fn kib(key: &str) -> u64 {
            let status = std::fs::read_to_string("/proc/self/status").unwrap();
            status
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .and_then(|value| value.trim().strip_suffix(" kB"))
                .and_then(|kb| kb.trim().parse::<u64>().ok())
                .unwrap()
        }
        let case = std::env::var("SKWIR_REGION_CASE").unwrap_or_else(|_| "png:middle".into());
        let (format, part) = case.split_once(':').expect("a format and a part");
        let (width, height) = (8000, 6000);
        let bytes = {
            // A photo's smoothness, and some grain.
            let grain = noise(width as usize * height as usize);
            let pixels = image::RgbImage::from_fn(width, height, |x, y| {
                let grain = grain[(y * width + x) as usize] & 15;
                image::Rgb([
                    (x / 32).to_le_bytes()[0].wrapping_add(grain),
                    (y / 24).to_le_bytes()[0].wrapping_add(grain),
                    ((x + y) / 56).to_le_bytes()[0].wrapping_add(grain),
                ])
            });
            let deflated = (
                tiff::encoder::Compression::Deflate(tiff::encoder::compression::DeflateLevel::Fast),
                tiff::tags::Predictor::Horizontal,
            );
            match format {
                "png" => png_of(
                    (width, height),
                    (png::ColorType::Rgb, png::BitDepth::Eight),
                    png::Filter::Adaptive,
                    pixels.as_raw(),
                    None,
                ),
                "strips" => tiff_strips::<tiff::encoder::colortype::RGB8>(
                    (width, height),
                    pixels.as_raw(),
                    16,
                    deflated,
                    1,
                ),
                "tiles" => tiff_tiles((width, height), 3, pixels.as_raw(), (256, 1)),
                other => panic!("no such format: {other}"),
            }
        };
        let region = match part {
            "whole" => None,
            "top" => Some([3000, 0, 2000, 1600]),
            "middle" => Some([3000, 2200, 2000, 1600]),
            "bottom" => Some([3000, 4400, 2000, 1600]),
            other => panic!("no such part: {other}"),
        };
        std::fs::write("/proc/self/clear_refs", "5").unwrap();
        let before = kib("VmRSS:");
        let started = std::time::Instant::now();
        let decoded = match region {
            None => decode(reader(&bytes), image::Limits::default()),
            Some(region) => decode_region(
                reader(&bytes),
                (width, height),
                region,
                width,
                Threads::Caller,
            ),
        }
        .unwrap();
        let took = started.elapsed().as_millis();
        let held = kib("VmHWM:").saturating_sub(before) / 1024;
        eprintln!(
            "{format} of {} MB, {part} ({} × {}): {took} ms, {held} MiB held at most",
            bytes.len() / 1_000_000,
            decoded.width(),
            decoded.height()
        );
    }

    /// A JPEG XL picture is read where any picture is (ADR-148): its size
    /// from its header, decoded and shrunk as a thumbnail is, its colours
    /// where they were (a gradient from red at the top to blue).
    #[test]
    fn a_jpeg_xl_picture_is_read_as_any_picture() {
        const JXL: &[u8] = include_bytes!("../testdata/synthetic-gradient.jxl");
        formats();
        let reader = || {
            ImageReader::new(Cursor::new(JXL))
                .with_guessed_format()
                .unwrap()
        };
        assert_eq!(size(reader()).unwrap(), (96, 64));
        let fitted = decode_fitted(reader(), 48, Threads::Caller)
            .unwrap()
            .to_rgb8();
        assert_eq!(fitted.dimensions(), (48, 32));
        let [r, _, b] = fitted.get_pixel(24, 1).0;
        assert!(r > 180 && b < 80, "the top is red: {r} {b}");
        let [r, _, b] = fitted.get_pixel(24, 30).0;
        assert!(r < 80 && b > 180, "the bottom is blue: {r} {b}");
    }

    /// A JPEG XL turned by its codestream, whose EXIF says so too (a JPEG
    /// converted): turned once, its size as it is shown.
    #[test]
    fn a_turned_jpeg_xl_is_turned_once() {
        const JXL: &[u8] = include_bytes!("../testdata/synthetic-turned.jxl");
        formats();
        let reader = || {
            ImageReader::new(Cursor::new(JXL))
                .with_guessed_format()
                .unwrap()
        };
        // 300 × 200 as stored, red at the top and blue at the bottom,
        // turned a quarter clockwise (EXIF Orientation 6): red on the right.
        assert_eq!(size(reader()).unwrap(), (200, 300));
        let shown = decode(reader(), image::Limits::default())
            .unwrap()
            .to_rgb8();
        assert_eq!(shown.dimensions(), (200, 300));
        let [r, _, b] = shown.get_pixel(190, 150).0;
        assert!(r > 200 && b < 60, "the right is red: {r} {b}");
        let [r, _, b] = shown.get_pixel(10, 150).0;
        assert!(r < 60 && b > 200, "the left is blue: {r} {b}");
        // A region of it, as it is shown.
        let right = decode_region(
            reader(),
            (200, 300),
            [150, 0, 50, 300],
            300,
            Threads::Caller,
        )
        .unwrap()
        .to_rgb8();
        assert_eq!(right.dimensions(), (50, 300));
        let [r, _, b] = right.get_pixel(40, 150).0;
        assert!(r > 200 && b < 60, "the right is red: {r} {b}");
    }

    /// A JPEG XL's region is decoded alone (`jxl::Decoder::cut`), as the
    /// whole decoded and cut, wherever it lies.
    #[test]
    fn a_jpeg_xl_region_is_decoded_alone() {
        const JXL: &[u8] = include_bytes!("../testdata/synthetic-plasma.jxl");
        formats();
        let reader = || {
            ImageReader::new(Cursor::new(JXL))
                .with_guessed_format()
                .unwrap()
        };
        let whole = decode(reader(), image::Limits::default())
            .unwrap()
            .to_rgb8();
        assert_eq!(whole.dimensions(), (640, 1300));
        for region in [[0, 0, 100, 40], [300, 250, 200, 600], [520, 1100, 120, 200]] {
            let cut = decode_region(reader(), (640, 1300), region, 1300, Threads::Caller)
                .unwrap()
                .to_rgb8();
            let [x, y, width, height] = region;
            let expected = image::imageops::crop_imm(&whole, x, y, width, height).to_image();
            assert!(cut == expected, "{region:?} differs");
        }
    }

    /// A JPEG XL carries its eighth, which stands for it while a larger
    /// picture is decoded and is shown, shrunk, where it covers what is
    /// asked; a lossless one, which has none, is read whole from its start.
    #[test]
    fn a_jpeg_xl_carries_its_eighth() {
        const PLASMA: &[u8] = include_bytes!("../testdata/synthetic-plasma.jxl");
        const LOSSLESS: &[u8] = include_bytes!("../testdata/synthetic-lossless.jxl");
        formats();
        let reader = |bytes| {
            ImageReader::new(Cursor::new(bytes))
                .with_guessed_format()
                .unwrap()
        };
        let eighth = crate::jxl::eighth(&mut Cursor::new(PLASMA), |_| true, Threads::Caller)
            .unwrap()
            .unwrap();
        // 640 × 1300: an eighth of 80 × 163.
        let below = |bytes, side: u32| {
            carried(reader(bytes), |longer| longer < side, Threads::Caller).unwrap()
        };
        assert!(below(PLASMA, 400).unwrap() == eighth, "its eighth");
        assert!(below(PLASMA, 163).is_none(), "not wanted");
        assert!(below(LOSSLESS, 400).is_none(), "lossless");
        let whole = decode_fitted(reader(LOSSLESS), 30, Threads::Caller).unwrap();
        assert_eq!((whole.width(), whole.height()), (30, 20));
    }

    /// What may be held of a picture is counted in pixels: a square one at
    /// the side, a tall or wide one as many pixels, one with fewer whole
    /// (a drawing, past its own size).
    #[test]
    fn a_picture_is_held_by_its_pixels_not_its_side() {
        const HELD: u64 = 4096 * 4096;
        assert_eq!(side_within(8192, 8192, HELD), 4096);
        assert_eq!(side_within(2000, 11_500, HELD), 9821);
        assert_eq!(side_within(40_000, 400, HELD), 40_960);
        assert_eq!(side_within(100, 100, HELD), 4096);
        assert_eq!(side_within(0, 0, HELD), 4096);
    }

    #[test]
    fn linear_light_is_encoded_as_srgb() {
        // Middle grey in linear light, and a highlight past white.
        let linear = image::Rgb32FImage::from_fn(2, 1, |x, _| {
            image::Rgb(if x == 0 {
                [0.214, 0.214, 0.214]
            } else {
                [4., 1., 0.]
            })
        });
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb32F(linear)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::OpenExr)
            .unwrap();
        let image = decode(reader(&bytes), image::Limits::default()).unwrap();
        let image = image.as_rgb8().expect("eight bits, not floats");
        // Half of white on screen, within the file's half-float precision.
        let [grey, ..] = image.get_pixel(0, 0).0;
        assert!((127..=128).contains(&grey), "{grey}");
        assert_eq!(image.get_pixel(1, 0).0, [255, 255, 0]);
    }

    #[test]
    fn upright_and_half_turned_pictures_keep_their_size() {
        for bytes in [jpeg(1), jpeg(3)] {
            assert_eq!(size(reader(&bytes)).unwrap(), (3, 2));
            assert_eq!(
                decode(reader(&bytes), image::Limits::default())
                    .unwrap()
                    .width(),
                3
            );
        }
    }
}
