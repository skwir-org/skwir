//! HEIC and AVIF pictures read by libheif (ADR-148), a helper of their own
//! as Skwir's other decoders of untrusted files are: run isolated where the
//! system allows, one picture a run, nothing kept.
//!
//! `skwir-heif-worker <input> <output> size <most>` prints the primary
//! picture's width, height and whether it has an alpha channel, as it is
//! shown (turned, mirrored and cropped as the file says), without decoding
//! it, then the size of the thumbnail it carries (`0 0` for none, or one
//! `thumbnail` would not give). Every mode refuses a picture of more than
//! `most` pixels as shown or as stored (before it is cropped), before
//! decoding anything of it; libheif bounds the rest of what it reads, the
//! tiles included, by `most` too, a tile's sides no more than its
//! picture's where a libheif before 1.19 bounds sides rather than areas
//! (`Picture::open`).
//! `skwir-heif-worker <input> <output> decode <most>` decodes it, at most
//! `most` pixels, to `output`: its rows of 8-bit RGB or RGBA packed, and
//! prints the same line. `… region <most> <x> <y> <width> <height>`
//! decodes the part of it so placed as it is shown, from the tiles it
//! covers where the picture is stored in tiles (an iPhone's, of 512
//! pixels) and libheif decodes one alone (1.19 and later, looked up as it
//! runs), else from the whole; it prints the part's size and alpha, and
//! the tiles decoded (0 for the whole). The tiles are found and decoded
//! as stored, the part then turned and mirrored here as the file says:
//! libheif's tiling as shown places a turned picture's first tile by the
//! pixels of its last (`image % tile`, 1.19 to 1.23), not by what of it
//! lies past the picture, so a portrait photo's part came out shifted. `… thumbnail <most>` decodes the
//! thumbnail the file carries, turned as it is shown, and prints its size,
//! or `0 0 0` where it carries none, or one of other proportions. `--probe` and `--build` answer
//! Skwir's settings and its failures (ADR-040, ADR-041). It asks nothing
//! else of libheif newer than 1.13 (`heif_init`): Debian's, Ubuntu's and
//! Fedora's have it.
// SAFETY: every handle libheif gives is released once, by its owner's
// `Drop`, the context last; a plane is read only while its image holds
// it, row by row within the stride and width libheif says it has.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::Write;
use std::marker::PhantomData;
use std::path::Path;

#[repr(C)]
struct Context {
    _private: [u8; 0],
}
#[repr(C)]
struct Handle {
    _private: [u8; 0],
}
#[repr(C)]
struct Image {
    _private: [u8; 0],
}

/// `heif_error`: its code (0 when all went well), its detail, its message.
#[repr(C)]
struct Error {
    code: c_int,
    subcode: c_int,
    message: *const c_char,
}

/// The start of `heif_decoding_options`, whose later versions only add
/// fields: allocated by libheif at its own version, only these written.
#[repr(C)]
struct DecodingOptions {
    version: u8,
    ignore_transformations: u8,
    start_progress: *const c_void,
    on_progress: *const c_void,
    end_progress: *const c_void,
    progress_user_data: *mut c_void,
    convert_hdr_to_8bit: u8,
}

/// `heif_image_tiling` (libheif 1.19), which libheif fills at its own
/// version's size: room is left for what a later one adds.
#[repr(C)]
struct Tiling {
    version: c_int,
    columns: u32,
    rows: u32,
    tile_width: u32,
    tile_height: u32,
    image_width: u32,
    image_height: u32,
    /// How far above and left of the picture as shown its first tile
    /// begins, which libheif counts amiss for a turned picture: asked as
    /// stored, where they are 0.
    top_offset: u32,
    left_offset: u32,
    extra_dimensions: u8,
    extra_dimension_size: [u32; 8],
    _later: [u8; 256],
}

const COLORSPACE_RGB: c_int = 1;
const CHROMA_RGB: c_int = 10;
const CHROMA_RGBA: c_int = 11;
const CHANNEL_INTERLEAVED: c_int = 10;

#[link(name = "heif")]
unsafe extern "C" {
    fn heif_init(params: *const c_void) -> Error;
    fn heif_context_alloc() -> *mut Context;
    fn heif_context_free(context: *mut Context);
    fn heif_context_set_maximum_image_size_limit(context: *mut Context, width: c_int);
    fn heif_context_read_from_file(
        context: *mut Context,
        path: *const c_char,
        options: *const c_void,
    ) -> Error;
    fn heif_context_get_primary_image_handle(
        context: *mut Context,
        handle: *mut *mut Handle,
    ) -> Error;
    fn heif_image_handle_release(handle: *const Handle);
    fn heif_get_version_number() -> u32;
    fn heif_image_handle_get_width(handle: *const Handle) -> c_int;
    fn heif_image_handle_get_ispe_width(handle: *const Handle) -> c_int;
    fn heif_image_handle_get_ispe_height(handle: *const Handle) -> c_int;
    fn heif_image_handle_get_height(handle: *const Handle) -> c_int;
    fn heif_image_handle_has_alpha_channel(handle: *const Handle) -> c_int;
    fn heif_image_handle_get_number_of_thumbnails(handle: *const Handle) -> c_int;
    fn heif_image_handle_get_list_of_thumbnail_IDs(
        handle: *const Handle,
        ids: *mut u32,
        count: c_int,
    ) -> c_int;
    fn heif_image_handle_get_thumbnail(
        handle: *const Handle,
        id: u32,
        thumbnail: *mut *mut Handle,
    ) -> Error;
    fn heif_decoding_options_alloc() -> *mut DecodingOptions;
    fn heif_decoding_options_free(options: *mut DecodingOptions);
    fn heif_decode_image(
        handle: *const Handle,
        image: *mut *mut Image,
        colorspace: c_int,
        chroma: c_int,
        options: *const DecodingOptions,
    ) -> Error;
    fn heif_image_release(image: *const Image);
    fn heif_image_get_width(image: *const Image, channel: c_int) -> c_int;
    fn heif_image_get_height(image: *const Image, channel: c_int) -> c_int;
    fn heif_image_get_bits_per_pixel_range(image: *const Image, channel: c_int) -> c_int;
    // Its `…2` with a wider stride is libheif 1.20's: Debian's is older.
    fn heif_image_get_plane_readonly(
        image: *const Image,
        channel: c_int,
        stride: *mut c_int,
    ) -> *const u8;
}

/// The property types of the transformations a picture is shown with
/// (`heif_item_property_type`, codes of four letters).
const ROTATION: c_int = 0x6972_6f74; // irot
const MIRROR: c_int = 0x696d_6972; // imir

/// libheif's calls a part is decoded from tiles with, where its version
/// has them (1.19): looked up as it runs, so the helper still runs with an
/// older one (Ubuntu 24.04's is 1.17), which decodes the whole.
struct Calls {
    /// `heif_image_handle_get_image_tiling`.
    tiling: unsafe extern "C" fn(*const Handle, c_int, *mut Tiling) -> Error,
    /// `heif_image_handle_decode_image_tile`.
    tile: unsafe extern "C" fn(
        *const Handle,
        *mut *mut Image,
        c_int,
        c_int,
        *const DecodingOptions,
        u32,
        u32,
    ) -> Error,
    /// `heif_image_handle_get_item_id`.
    item: unsafe extern "C" fn(*const Handle) -> u32,
    /// `heif_item_get_transformation_properties`.
    transformations: unsafe extern "C" fn(*const Context, u32, *mut u32, c_int) -> c_int,
    /// `heif_item_get_property_type`.
    kind: unsafe extern "C" fn(*const Context, u32, u32) -> c_int,
    /// `heif_item_get_property_transform_rotation_ccw`.
    rotation: unsafe extern "C" fn(*const Context, u32, u32) -> c_int,
    /// `heif_item_get_property_transform_mirror`.
    mirror: unsafe extern "C" fn(*const Context, u32, u32) -> c_int,
}

/// The function `name`, of type `F`, where the libraries have it.
///
/// # Safety
///
/// `F` is the type of a pointer to the function `name` is.
unsafe fn function<F: Copy>(name: &CStr) -> Option<F> {
    let found = symbol(name);
    assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*mut c_void>());
    // SAFETY: a function pointer of the type the caller says.
    (!found.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, F>(&found) })
}

fn calls() -> Option<Calls> {
    // SAFETY: each is libheif's function of the type declared beside it
    // (`heif_tiling.h`, `heif_properties.h`, `heif_image_handle.h`).
    unsafe {
        Some(Calls {
            tiling: function(c"heif_image_handle_get_image_tiling")?,
            tile: function(c"heif_image_handle_decode_image_tile")?,
            item: function(c"heif_image_handle_get_item_id")?,
            transformations: function(c"heif_item_get_transformation_properties")?,
            kind: function(c"heif_item_get_property_type")?,
            rotation: function(c"heif_item_get_property_transform_rotation_ccw")?,
            mirror: function(c"heif_item_get_property_transform_mirror")?,
        })
    }
}

/// What a picture is shown with, as stored: turned counter-clockwise by
/// 90, 180 or 270 degrees, or mirrored, left and right (`across`) or top
/// and bottom.
#[derive(Clone, Copy, Debug)]
enum Turn {
    Rotate(u32),
    Mirror { across: bool },
}

impl Turn {
    /// The size of a picture of `size` once turned.
    fn size(self, (width, height): (u32, u32)) -> (u32, u32) {
        match self {
            Turn::Rotate(90 | 270) => (height, width),
            _ => (width, height),
        }
    }

    /// Where `[x, y, width, height]` of a picture once turned lies in it
    /// before, of `size` (before).
    fn before(self, [x, y, width, height]: [u32; 4], (w, h): (u32, u32)) -> [u32; 4] {
        match self {
            // Counter-clockwise a quarter: the right comes to the top.
            Turn::Rotate(90) => [w - y - height, x, height, width],
            Turn::Rotate(180) => [w - x - width, h - y - height, width, height],
            Turn::Rotate(270) => [y, h - x - width, height, width],
            Turn::Mirror { across: true } => [w - x - width, y, width, height],
            Turn::Mirror { across: false } => [x, h - y - height, width, height],
            Turn::Rotate(_) => [x, y, width, height],
        }
    }

    /// `pixels` (of `width` × `height`, `channels` a pixel) turned, and
    /// their size so.
    fn apply(
        self,
        pixels: &[u8],
        (w, h): (usize, usize),
        channels: usize,
    ) -> (Vec<u8>, (usize, usize)) {
        let (new_width, new_height) = match self {
            Turn::Rotate(90 | 270) => (h, w),
            _ => (w, h),
        };
        // Where each pixel of the new comes from, in the old.
        let from = |x: usize, y: usize| match self {
            Turn::Rotate(90) => (w - 1 - y, x),
            Turn::Rotate(180) => (w - 1 - x, h - 1 - y),
            Turn::Rotate(270) => (y, h - 1 - x),
            Turn::Mirror { across: true } => (w - 1 - x, y),
            Turn::Mirror { across: false } => (x, h - 1 - y),
            Turn::Rotate(_) => (x, y),
        };
        let mut turned = Vec::with_capacity(pixels.len());
        for y in 0..new_height {
            for x in 0..new_width {
                let (old_x, old_y) = from(x, y);
                let at = (old_y * w + old_x) * channels;
                turned.extend_from_slice(&pixels[at..at + channels]);
            }
        }
        (turned, (new_width, new_height))
    }
}

/// A function of the libraries the program was linked with, or null.
#[cfg(unix)]
fn symbol(name: &CStr) -> *mut c_void {
    unsafe extern "C" {
        fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }
    const RTLD_LAZY: c_int = 1;
    // SAFETY: a null name opens the program itself, whose libraries
    // (libheif) are searched; the name is a C string.
    unsafe {
        let program = dlopen(std::ptr::null(), RTLD_LAZY);
        if program.is_null() {
            return std::ptr::null_mut();
        }
        dlsym(program, name.as_ptr())
    }
}

/// Not looked up elsewhere: the helper is shipped on Linux alone.
#[cfg(not(unix))]
fn symbol(_name: &CStr) -> *mut c_void {
    std::ptr::null_mut()
}

/// What libheif said went wrong, or nothing.
fn checked(error: &Error) -> anyhow::Result<()> {
    if error.code == 0 {
        return Ok(());
    }
    let message = if error.message.is_null() {
        String::new()
    } else {
        // SAFETY: libheif's message is always a valid C string it keeps.
        unsafe { CStr::from_ptr(error.message) }
            .to_string_lossy()
            .into_owned()
    };
    anyhow::bail!("libheif: {message} ({}.{})", error.code, error.subcode)
}

/// A picture's pixels, 8-bit RGB or RGBA rows packed, its width and height,
/// whether it has alpha.
type Pixels = (Vec<u8>, u32, u32, bool);

/// The side libheif bounds a picture by unless told otherwise (32 768,
/// `MAX_IMAGE_WIDTH` before 1.19, the square root of its pixels since).
const LIBHEIF_SIDE: c_int = 32_768;
/// The first libheif whose bound is an area (1.19), as
/// `heif_get_version_number` says it: before, it bounds each side.
const AREA_SINCE: u32 = 0x0113_0000;

/// Refused where `width` × `height` is more than `most` pixels.
fn within(width: u32, height: u32, most: u64) -> anyhow::Result<()> {
    anyhow::ensure!(
        u64::from(width) * u64::from(height) <= most,
        "the picture is larger than asked"
    );
    Ok(())
}

/// A file read by libheif, released with it.
struct Picture {
    context: *mut Context,
    handle: *mut Handle,
}

impl Picture {
    /// The primary picture of `path`, refused past `most` pixels as it is
    /// shown and as it is stored (before it is turned and cropped: a file
    /// may store 30 000 × 30 000 pixels, in a few kilobytes of tiles, and
    /// show 100 × 100 of them), before anything of it is decoded.
    ///
    /// libheif is told what it refuses besides, before it reads anything
    /// (`side`): since 1.19 the area of every picture the file holds, its
    /// tiles included, so `most`'s square; before, each side of each, so
    /// the larger of `most`'s square's and the picture's own sides, read
    /// once at libheif's own bound (32 768) and the file read again: a
    /// panorama 16 000 pixels wide, within `most`, is read, and a tile is
    /// never wider or taller than its picture or that square.
    fn open(path: &Path, most: u64) -> anyhow::Result<Picture> {
        let path = CString::new(path.as_os_str().as_encoded_bytes())?;
        // The side of the least square of at least `most` pixels: libheif
        // since 1.19 bounds the area by its square, which is then never
        // less than `most` (its own bound, `within`, is exact).
        let side = most.isqrt() + u64::from(most.isqrt() * most.isqrt() < most);
        let square = c_int::try_from(side).unwrap_or(c_int::MAX);
        // SAFETY: a plain number, asked of the library linked.
        if unsafe { heif_get_version_number() } >= AREA_SINCE {
            return Picture::read(&path, square, most);
        }
        let first = Picture::read(&path, LIBHEIF_SIDE, most)?;
        let (width, height) = first.stored();
        let side = c_int::try_from(width.max(height))
            .unwrap_or(c_int::MAX)
            .max(square);
        if side >= LIBHEIF_SIDE {
            return Ok(first);
        }
        drop(first);
        Picture::read(&path, side, most)
    }

    /// The primary picture of `path`, libheif bounding what it reads by
    /// `side`, refused past `most` pixels as shown and as stored.
    fn read(path: &CString, side: c_int, most: u64) -> anyhow::Result<Picture> {
        // SAFETY: a fresh context, owned by the `Picture` from here.
        let context = unsafe { heif_context_alloc() };
        anyhow::ensure!(!context.is_null(), "libheif made no context");
        let mut picture = Picture {
            context,
            handle: std::ptr::null_mut(),
        };
        // SAFETY: the context is valid; the path a C string for the call.
        unsafe {
            heif_context_set_maximum_image_size_limit(picture.context, side);
            checked(&heif_context_read_from_file(
                picture.context,
                path.as_ptr(),
                std::ptr::null(),
            ))?;
            checked(&heif_context_get_primary_image_handle(
                picture.context,
                &raw mut picture.handle,
            ))?;
        }
        let (width, height, _) = picture.size();
        within(width, height, most)?;
        let (width, height) = picture.stored();
        within(width, height, most)?;
        Ok(picture)
    }

    /// Its size as stored, before it is turned or cropped (its `ispe`).
    fn stored(&self) -> (u32, u32) {
        // SAFETY: the handle is valid while `self` is.
        unsafe {
            (
                u32::try_from(heif_image_handle_get_ispe_width(self.handle)).unwrap_or(0),
                u32::try_from(heif_image_handle_get_ispe_height(self.handle)).unwrap_or(0),
            )
        }
    }

    /// Its size as shown, and whether it has an alpha channel.
    fn size(&self) -> (u32, u32, bool) {
        size_of(self.handle)
    }

    /// Its pixels.
    fn decode(&self) -> anyhow::Result<Pixels> {
        let (_, _, alpha) = self.size();
        let decoded = decode(self.handle, alpha)?;
        let plane = decoded.plane(channels(alpha))?;
        let mut pixels = Vec::with_capacity(plane.row_bytes * plane.height);
        for y in 0..plane.height {
            pixels.extend_from_slice(plane.row(y));
        }
        Ok((pixels, plane.width_u32()?, plane.height_u32()?, alpha))
    }

    /// The part `[x, y, width, height]` of it as shown, from the tiles it
    /// covers where it has some, else from the whole; and the tiles
    /// decoded, 0 for the whole.
    fn region(&self, region: [u32; 4]) -> anyhow::Result<(Pixels, u32)> {
        let [x, y, width, height] = region;
        let (own_width, own_height, alpha) = self.size();
        anyhow::ensure!(
            width > 0
                && height > 0
                && x.checked_add(width).is_some_and(|end| end <= own_width)
                && y.checked_add(height).is_some_and(|end| end <= own_height),
            "the region is not within the picture"
        );
        let channels = channels(alpha);
        // Where libheif cannot say how it is tiled, the whole is decoded.
        if let Some((calls, tiling, turns)) = self.tiled((own_width, own_height)).ok().flatten() {
            // The part as stored: back through each turn, the last first.
            let mut sizes = vec![(tiling.image_width, tiling.image_height)];
            for turn in &turns {
                sizes.push(turn.size(sizes[sizes.len() - 1]));
            }
            let mut stored = region;
            for (turn, size) in turns.iter().zip(&sizes).rev() {
                stored = turn.before(stored, *size);
            }
            let (mut pixels, tiles) = self.stored_part(&calls, &tiling, stored, alpha)?;
            // Then turned as it is shown.
            let mut size = (stored[2] as usize, stored[3] as usize);
            for turn in turns {
                (pixels, size) = turn.apply(&pixels, size, channels);
            }
            anyhow::ensure!(
                size == (width as usize, height as usize),
                "the part turned is {size:?}"
            );
            return Ok(((pixels, width, height, alpha), tiles));
        }
        let decoded = decode(self.handle, alpha)?;
        let mut part = Part::new(region, channels);
        part.take(&decoded.plane(channels)?, (0, 0));
        Ok(((part.done()?, width, height, alpha), 0))
    }

    /// The part `[x, y, width, height]` of it as stored, from the stored
    /// tiles it covers, untransformed, decoded at once by as many threads
    /// as libheif decodes a whole's (its default, 4) where there are as
    /// many cores; and how many.
    fn stored_part(
        &self,
        calls: &Calls,
        tiling: &Tiling,
        region: [u32; 4],
        alpha: bool,
    ) -> anyhow::Result<(Vec<u8>, u32)> {
        let [x, y, width, height] = region;
        let (tile_width, tile_height) = (tiling.tile_width, tiling.tile_height);
        let columns = x / tile_width..=((x + width - 1) / tile_width).min(tiling.columns - 1);
        let rows = y / tile_height..=((y + height - 1) / tile_height).min(tiling.rows - 1);
        let tiles = rows
            .flat_map(|tile_y| columns.clone().map(move |tile_x| (tile_x, tile_y)))
            .collect::<Vec<_>>();
        let threads = std::thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .min(4)
            .min(tiles.len());
        let part = std::sync::Mutex::new(Part::new(region, channels(alpha)));
        let next = std::sync::atomic::AtomicUsize::new(0);
        let handle = Shared(self.handle);
        let decode_tiles = || -> anyhow::Result<()> {
            let options = Options::new(true)?;
            loop {
                let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&(tile_x, tile_y)) = tiles.get(at) else {
                    return Ok(());
                };
                let mut image = std::ptr::null_mut();
                // SAFETY: the handle is valid while `self` is, which
                // outlives the threads; libheif decodes tiles of one
                // handle on several threads itself (a whole grid's). The
                // options are this thread's; the image released by
                // `Decoded`'s drop.
                let tile = unsafe {
                    let error = (calls.tile)(
                        handle.handle(),
                        &raw mut image,
                        COLORSPACE_RGB,
                        chroma(alpha),
                        options.0,
                        tile_x,
                        tile_y,
                    );
                    let tile = Decoded(image);
                    checked(&error)?;
                    tile
                };
                let plane = tile.plane(channels(alpha))?;
                let origin = (
                    i64::from(tile_x) * i64::from(tile_width),
                    i64::from(tile_y) * i64::from(tile_height),
                );
                part.lock()
                    .map_err(|_| anyhow::anyhow!("a tile's thread failed"))?
                    .take(&plane, origin);
            }
        };
        std::thread::scope(|scope| {
            let workers = (0..threads)
                .map(|_| scope.spawn(decode_tiles))
                .collect::<Vec<_>>();
            workers.into_iter().try_for_each(|worker| {
                worker
                    .join()
                    .map_err(|_| anyhow::anyhow!("a tile's thread failed"))?
            })
        })?;
        let pixels = part
            .into_inner()
            .map_err(|_| anyhow::anyhow!("a tile's thread failed"))?
            .done()?;
        Ok((pixels, u32::try_from(tiles.len())?))
    }

    /// How it is stored in tiles libheif decodes one by one (as stored),
    /// what it is shown with, and the calls for them: `None` for a
    /// picture of one tile, where libheif has no such calls, or where it
    /// is shown otherwise than turned and mirrored (cropped, by `clap`):
    /// it is then decoded whole.
    fn tiled(&self, shown: (u32, u32)) -> anyhow::Result<Option<(Calls, Tiling, Vec<Turn>)>> {
        let Some(calls) = calls() else {
            return Ok(None);
        };
        // SAFETY: every field is a number, for which zero is a value.
        let mut tiling: Tiling = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is valid while `self` is; the tiling has room
        // for libheif's, at its version.
        checked(&unsafe { (calls.tiling)(self.handle, 0, &raw mut tiling) })?;
        if tiling.columns == 0
            || tiling.rows == 0
            || u64::from(tiling.columns) * u64::from(tiling.rows) <= 1
            || tiling.tile_width == 0
            || tiling.tile_height == 0
        {
            return Ok(None);
        }
        // SAFETY: the context and handle are valid while `self` is; the
        // list has room for what is asked.
        let turns = unsafe {
            let item = (calls.item)(self.handle);
            let count = (calls.transformations)(self.context, item, std::ptr::null_mut(), 0);
            let mut ids = vec![0; usize::try_from(count)?];
            let listed = (calls.transformations)(self.context, item, ids.as_mut_ptr(), count);
            anyhow::ensure!(listed == count, "libheif listed {listed} of {count}");
            let mut turns = Vec::new();
            for id in ids {
                let turn = match (calls.kind)(self.context, item, id) {
                    ROTATION => match (calls.rotation)(self.context, item, id) {
                        0 => continue,
                        angle @ (90 | 180 | 270) => Turn::Rotate(angle.unsigned_abs()),
                        _ => return Ok(None),
                    },
                    MIRROR => match (calls.mirror)(self.context, item, id) {
                        0 => Turn::Mirror { across: false },
                        1 => Turn::Mirror { across: true },
                        _ => return Ok(None),
                    },
                    _ => return Ok(None),
                };
                turns.push(turn);
            }
            turns
        };
        // What the turns make of it is what is shown, or it is decoded
        // whole.
        let size = turns
            .iter()
            .fold((tiling.image_width, tiling.image_height), |size, turn| {
                turn.size(size)
            });
        Ok((size == shown).then_some((calls, tiling, turns)))
    }

    /// The thumbnail it carries, turned as it is shown, if any: none where
    /// its proportions are not the picture's, within a pixel of rounding
    /// (another picture's, or the picture's before it was cropped).
    fn thumbnail(&self) -> anyhow::Result<Option<Pixels>> {
        let Some((thumbnail, _, _, alpha)) = self.carried()? else {
            return Ok(None);
        };
        let decoded = decode(thumbnail.0, alpha)?;
        let plane = decoded.plane(channels(alpha))?;
        let mut pixels = Vec::with_capacity(plane.row_bytes * plane.height);
        for y in 0..plane.height {
            pixels.extend_from_slice(plane.row(y));
        }
        Ok(Some((
            pixels,
            plane.width_u32()?,
            plane.height_u32()?,
            alpha,
        )))
    }

    /// The thumbnail it carries, its size as shown and its alpha, read
    /// without decoding it: none where its proportions are not the
    /// picture's, within a pixel of rounding, or it is larger.
    fn carried(&self) -> anyhow::Result<Option<(Thumbnail, u32, u32, bool)>> {
        let mut id = 0;
        // SAFETY: the handle is valid while `self` is; one ID asked, room
        // for one given.
        let listed = unsafe {
            heif_image_handle_get_number_of_thumbnails(self.handle) > 0
                && heif_image_handle_get_list_of_thumbnail_IDs(self.handle, &raw mut id, 1) == 1
        };
        if !listed {
            return Ok(None);
        }
        let mut thumbnail = Thumbnail(std::ptr::null_mut());
        // SAFETY: as above; the thumbnail's handle released by its drop,
        // before the context.
        checked(&unsafe {
            heif_image_handle_get_thumbnail(self.handle, id, &raw mut thumbnail.0)
        })?;
        let (width, height, alpha) = size_of(thumbnail.0);
        let (own_width, own_height, _) = self.size();
        let (across, down) = (
            u64::from(width) * u64::from(own_height),
            u64::from(height) * u64::from(own_width),
        );
        if width == 0
            || height == 0
            || across.abs_diff(down) > u64::from(own_width) + u64::from(own_height)
            || u64::from(width) * u64::from(height) > u64::from(own_width) * u64::from(own_height)
        {
            return Ok(None);
        }
        Ok(Some((thumbnail, width, height, alpha)))
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: each released once, the handle before its context.
        unsafe {
            if !self.handle.is_null() {
                heif_image_handle_release(self.handle);
            }
            heif_context_free(self.context);
        }
    }
}

/// A thumbnail's handle, released when dropped.
struct Thumbnail(*mut Handle);

impl Drop for Thumbnail {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: released once, by its one owner.
            unsafe { heif_image_handle_release(self.0) };
        }
    }
}

fn channels(alpha: bool) -> usize {
    if alpha { 4 } else { 3 }
}

fn chroma(alpha: bool) -> c_int {
    if alpha { CHROMA_RGBA } else { CHROMA_RGB }
}

/// A picture's size as shown, and whether it has an alpha channel.
fn size_of(handle: *const Handle) -> (u32, u32, bool) {
    // SAFETY: the handle is valid while its owner is.
    unsafe {
        (
            u32::try_from(heif_image_handle_get_width(handle)).unwrap_or(0),
            u32::try_from(heif_image_handle_get_height(handle)).unwrap_or(0),
            heif_image_handle_has_alpha_channel(handle) != 0,
        )
    }
}

/// Decoding options, allocated by libheif at its version: HDR decoded to
/// 8 bits.
struct Options(*mut DecodingOptions);

impl Options {
    /// As shown, or as stored (`untransformed`: not turned, mirrored nor
    /// cropped).
    fn new(untransformed: bool) -> anyhow::Result<Options> {
        // SAFETY: allocated by libheif, only the first fields (in every
        // version) written, freed by the drop.
        unsafe {
            let options = heif_decoding_options_alloc();
            anyhow::ensure!(!options.is_null(), "libheif made no options");
            (*options).ignore_transformations = u8::from(untransformed);
            if (*options).version >= 2 {
                (*options).convert_hdr_to_8bit = 1;
            }
            Ok(Options(options))
        }
    }
}

impl Drop for Options {
    fn drop(&mut self) {
        // SAFETY: freed once, by its one owner.
        unsafe { heif_decoding_options_free(self.0) };
    }
}

/// The picture `handle` is, decoded to 8-bit RGB, or RGBA with `alpha`.
fn decode(handle: *const Handle, alpha: bool) -> anyhow::Result<Decoded> {
    let options = Options::new(false)?;
    let mut image = std::ptr::null_mut();
    // SAFETY: the handle is valid while its owner is, the options while
    // `options` is; the image released by `Decoded`'s drop.
    unsafe {
        let error = heif_decode_image(
            handle,
            &raw mut image,
            COLORSPACE_RGB,
            chroma(alpha),
            options.0,
        );
        let decoded = Decoded(image);
        checked(&error)?;
        Ok(decoded)
    }
}

/// The handle a picture's tiles are decoded from on several threads.
struct Shared(*const Handle);

// SAFETY: libheif decodes the tiles of one handle on several threads
// itself (a whole grid's, `grid.cc`); nothing else is done with it
// meanwhile.
unsafe impl Sync for Shared {}

impl Shared {
    /// The handle, through the wrapper: a closure taking the field alone
    /// would take the pointer, which is not `Sync`.
    fn handle(&self) -> *const Handle {
        self.0
    }
}

/// The part `[x, y, width, height]` of a picture, made of the planes placed
/// in it: refused unless they cover it.
struct Part {
    region: [u32; 4],
    channels: usize,
    pixels: Vec<u8>,
    covered: i64,
}

impl Part {
    fn new(region: [u32; 4], channels: usize) -> Part {
        Part {
            region,
            channels,
            pixels: vec![0; region[2] as usize * region[3] as usize * channels],
            covered: 0,
        }
    }

    /// What `plane`, placed at `at`, has of the part.
    fn take(&mut self, plane: &Plane<'_>, at: (i64, i64)) {
        let [x, y, width, height] = self.region;
        let channels = self.channels;
        let row = width as usize * channels;
        let span = |start: u32, length: u32, at: i64, has: usize| {
            let from = i64::from(start).max(at);
            let to = (i64::from(start) + i64::from(length))
                .min(at + i64::try_from(has).unwrap_or(i64::MAX));
            (from, to)
        };
        let (x0, x1) = span(x, width, at.0, plane.width);
        let (y0, y1) = span(y, height, at.1, plane.height);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let length = (x1 - x0) as usize * channels;
        for line in y0..y1 {
            let source = &plane.row((line - at.1) as usize)[(x0 - at.0) as usize * channels..];
            let start =
                (line - i64::from(y)) as usize * row + (x0 - i64::from(x)) as usize * channels;
            self.pixels[start..start + length].copy_from_slice(&source[..length]);
        }
        self.covered += (x1 - x0) * (y1 - y0);
    }

    /// Its pixels, once covered.
    fn done(self) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            self.covered == i64::from(self.region[2]) * i64::from(self.region[3]),
            "the tiles did not cover the part"
        );
        Ok(self.pixels)
    }
}

/// A decoded image, released when dropped.
struct Decoded(*mut Image);

impl Decoded {
    /// Its interleaved plane of `channels` 8-bit samples a pixel.
    fn plane(&self, channels: usize) -> anyhow::Result<Plane<'_>> {
        anyhow::ensure!(!self.0.is_null(), "libheif gave no picture");
        // SAFETY: the image is valid while `self` is.
        unsafe {
            let width = usize::try_from(heif_image_get_width(self.0, CHANNEL_INTERLEAVED))?;
            let height = usize::try_from(heif_image_get_height(self.0, CHANNEL_INTERLEAVED))?;
            anyhow::ensure!(
                heif_image_get_bits_per_pixel_range(self.0, CHANNEL_INTERLEAVED) == 8,
                "libheif decoded more than 8 bits a channel"
            );
            let mut stride: c_int = 0;
            let start = heif_image_get_plane_readonly(self.0, CHANNEL_INTERLEAVED, &raw mut stride);
            let stride = usize::try_from(stride)?;
            let row_bytes = width * channels;
            anyhow::ensure!(
                !start.is_null() && stride >= row_bytes,
                "libheif gave no picture"
            );
            Ok(Plane {
                start,
                stride,
                row_bytes,
                width,
                height,
                _image: PhantomData,
            })
        }
    }
}

impl Drop for Decoded {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: released once, by its one owner.
            unsafe { heif_image_release(self.0) };
        }
    }
}

/// A decoded image's rows, read while it is held.
struct Plane<'a> {
    start: *const u8,
    stride: usize,
    row_bytes: usize,
    width: usize,
    height: usize,
    _image: PhantomData<&'a Decoded>,
}

impl Plane<'_> {
    /// Row `y`'s pixels.
    fn row(&self, y: usize) -> &[u8] {
        assert!(y < self.height, "row {y} of {}", self.height);
        // SAFETY: within the plane: `y` below its height, `row_bytes`
        // within its stride, while its image is held.
        unsafe { std::slice::from_raw_parts(self.start.add(y * self.stride), self.row_bytes) }
    }

    fn width_u32(&self) -> anyhow::Result<u32> {
        Ok(u32::try_from(self.width)?)
    }

    fn height_u32(&self) -> anyhow::Result<u32> {
        Ok(u32::try_from(self.height)?)
    }
}

fn main() {
    let first = std::env::args_os().nth(1);
    // `--build`, skwir asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if first.as_deref().is_some_and(|arg| arg == "--build") {
        println!("{}", env!("SKWIR_BUILD"));
        return;
    }
    // `--probe`, skwir asking whether this helper works here: being
    // started at all says its library is there.
    if first.as_deref().is_some_and(|arg| arg == "--probe") {
        return;
    }
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let [input, output, mode, rest @ ..] = args.as_slice() else {
        anyhow::bail!("expected the input, the output and a mode");
    };
    let numbers = rest
        .iter()
        .map(|arg| arg.to_str().and_then(|arg| arg.parse::<u64>().ok()))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow::anyhow!("expected numbers after the mode"))?;
    let most = numbers.first().copied().unwrap_or(1 << 28);
    // Its decoders, where a distribution ships them as plugins (Debian's
    // libde265 and dav1d), are loaded by this alone.
    // SAFETY: called once, before anything else of libheif's.
    checked(&unsafe { heif_init(std::ptr::null()) })?;
    let picture = Picture::open(Path::new(input), most)?;
    let mut out = std::io::stdout().lock();
    let written = |(pixels, width, height, alpha): Pixels| -> anyhow::Result<(u32, u32, bool)> {
        within(width, height, most)?;
        std::fs::write(output, pixels)?;
        Ok((width, height, alpha))
    };
    if mode == "size" {
        let (width, height, alpha) = picture.size();
        // The thumbnail it carries, which is asked only where it is wanted.
        let (across, down) = picture
            .carried()
            .ok()
            .flatten()
            .map_or((0, 0), |(_, width, height, _)| (width, height));
        writeln!(out, "{width} {height} {} {across} {down}", u8::from(alpha))?;
    } else if mode == "decode" {
        let (width, height, alpha) = written(picture.decode()?)?;
        writeln!(out, "{width} {height} {}", u8::from(alpha))?;
    } else if mode == "region" {
        let [_, x, y, width, height] = numbers.as_slice() else {
            anyhow::bail!("expected the most pixels, then the region's x, y, width and height");
        };
        let region = [*x, *y, *width, *height].map(|n| u32::try_from(n).unwrap_or(u32::MAX));
        let (pixels, tiles) = picture.region(region)?;
        let (width, height, alpha) = written(pixels)?;
        writeln!(out, "{width} {height} {} {tiles}", u8::from(alpha))?;
    } else if mode == "thumbnail" {
        match picture.thumbnail()? {
            Some(pixels) => {
                let (width, height, alpha) = written(pixels)?;
                writeln!(out, "{width} {height} {}", u8::from(alpha))?;
            }
            None => writeln!(out, "0 0 0")?,
        }
    } else {
        anyhow::bail!("expected size, decode, region or thumbnail");
    }
    Ok(())
}
