//! HEIC and AVIF pictures read by libheif (ADR-148), a helper of their own
//! as Skwir's other decoders of untrusted files are: run isolated where the
//! system allows, one picture a run, nothing kept.
//!
//! `skwir-heif-worker <input> <output> size` prints the primary picture's
//! width, height and whether it has an alpha channel, as it is shown
//! (turned, mirrored and cropped as the file says), without decoding it.
//! `skwir-heif-worker <input> <output> decode <most>` decodes it, at most
//! `most` pixels, to `output`: its rows of 8-bit RGB or RGBA packed, and
//! prints the same line. `--probe` and `--build` answer Skwir's settings
//! and its failures (ADR-040, ADR-041). It asks nothing of libheif newer
//! than 1.13 (`heif_init`): Debian's, Ubuntu's and Fedora's have it.
// SAFETY: every handle libheif gives is released once, by its owner's
// `Drop`, the context last; a plane is read only while its image holds
// it, row by row within the stride and width libheif says it has.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::Write;
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
    fn heif_image_handle_get_width(handle: *const Handle) -> c_int;
    fn heif_image_handle_get_height(handle: *const Handle) -> c_int;
    fn heif_image_handle_has_alpha_channel(handle: *const Handle) -> c_int;
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

/// A file read by libheif, released with it.
struct Picture {
    context: *mut Context,
    handle: *mut Handle,
}

impl Picture {
    /// The primary picture of `path`, refused past `most` pixels.
    fn open(path: &Path, most: u64) -> anyhow::Result<Picture> {
        let path = CString::new(path.as_os_str().as_encoded_bytes())?;
        // SAFETY: a fresh context, owned by the `Picture` from here.
        let context = unsafe { heif_context_alloc() };
        anyhow::ensure!(!context.is_null(), "libheif made no context");
        let mut picture = Picture {
            context,
            handle: std::ptr::null_mut(),
        };
        // Its square's side: libheif bounds the area by it.
        let side = c_int::try_from(most.isqrt()).unwrap_or(c_int::MAX);
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
        Ok(picture)
    }

    /// Its size as shown, and whether it has an alpha channel.
    fn size(&self) -> (u32, u32, bool) {
        // SAFETY: the handle is valid while `self` is.
        unsafe {
            (
                u32::try_from(heif_image_handle_get_width(self.handle)).unwrap_or(0),
                u32::try_from(heif_image_handle_get_height(self.handle)).unwrap_or(0),
                heif_image_handle_has_alpha_channel(self.handle) != 0,
            )
        }
    }

    /// Its pixels, 8-bit RGB or RGBA rows packed, and its size.
    fn decode(&self) -> anyhow::Result<(Vec<u8>, u32, u32, bool)> {
        let (_, _, alpha) = self.size();
        let channels = if alpha { 4 } else { 3 };
        // SAFETY: options allocated by libheif at its version, only the
        // first fields (in every version) written, freed below; the image
        // released by `Decoded`'s drop.
        let decoded = unsafe {
            let options = heif_decoding_options_alloc();
            anyhow::ensure!(!options.is_null(), "libheif made no options");
            if (*options).version >= 2 {
                (*options).convert_hdr_to_8bit = 1;
            }
            let mut image = std::ptr::null_mut();
            let error = heif_decode_image(
                self.handle,
                &raw mut image,
                COLORSPACE_RGB,
                if alpha { CHROMA_RGBA } else { CHROMA_RGB },
                options,
            );
            heif_decoding_options_free(options);
            checked(&error)?;
            Decoded(image)
        };
        // SAFETY: the image is valid while `decoded` is; each row read
        // within its stride, which holds at least `width × channels`.
        unsafe {
            let image = decoded.0;
            let width = usize::try_from(heif_image_get_width(image, CHANNEL_INTERLEAVED))?;
            let height = usize::try_from(heif_image_get_height(image, CHANNEL_INTERLEAVED))?;
            anyhow::ensure!(
                heif_image_get_bits_per_pixel_range(image, CHANNEL_INTERLEAVED) == 8,
                "libheif decoded more than 8 bits a channel"
            );
            let mut stride: c_int = 0;
            let plane = heif_image_get_plane_readonly(image, CHANNEL_INTERLEAVED, &raw mut stride);
            let stride = usize::try_from(stride)?;
            let row = width * channels;
            anyhow::ensure!(!plane.is_null() && stride >= row, "libheif gave no picture");
            let mut pixels = Vec::with_capacity(row * height);
            for y in 0..height {
                pixels.extend_from_slice(std::slice::from_raw_parts(plane.add(y * stride), row));
            }
            Ok((pixels, u32::try_from(width)?, u32::try_from(height)?, alpha))
        }
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

/// A decoded image, released when dropped.
struct Decoded(*mut Image);

impl Drop for Decoded {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: released once, by its one owner.
            unsafe { heif_image_release(self.0) };
        }
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
        anyhow::bail!("expected the input, the output and size or decode");
    };
    let most = match rest {
        [most] => most.to_str().and_then(|most| most.parse().ok()),
        _ => None,
    }
    .unwrap_or(1 << 28);
    // Its decoders, where a distribution ships them as plugins (Debian's
    // libde265 and dav1d), are loaded by this alone.
    // SAFETY: called once, before anything else of libheif's.
    checked(&unsafe { heif_init(std::ptr::null()) })?;
    let picture = Picture::open(Path::new(input), most)?;
    let (width, height, alpha) = if mode == "size" {
        picture.size()
    } else if mode == "decode" {
        let (pixels, width, height, alpha) = picture.decode()?;
        std::fs::write(output, pixels)?;
        (width, height, alpha)
    } else {
        anyhow::bail!("expected size or decode");
    };
    anyhow::ensure!(
        u64::from(width) * u64::from(height) <= most,
        "the picture is larger than asked"
    );
    let mut out = std::io::stdout().lock();
    writeln!(out, "{width} {height} {}", u8::from(alpha))?;
    Ok(())
}
