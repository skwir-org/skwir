//! The thumbnails the system itself draws for a file (ADR-041): Quick
//! Look's on macOS, the shell's thumbnail handlers' on Windows, for every
//! format one is installed for (Office documents, RAW photos, HEIC,
//! films, fonts…), as the Finder and Explorer show them. A provider of
//! the Generate stage, after skwir's own decoder, where a Linux desktop's
//! thumbnailers are (`thumb_providers::Thumbnailers`). Only a thumbnail:
//! never the file's icon, which skwir draws from its type. Compiled on
//! every system; on Linux it answers nothing.

use std::path::Path;

use skwir_core::cancel::CancelToken;
use skwir_core::thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider};

/// The system's thumbnails.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemThumbnails;

/// Whether this system has thumbnails of its own to ask for.
pub const AVAILABLE: bool = cfg!(any(target_os = "macos", windows));

impl ThumbnailProvider for SystemThumbnails {
    fn id(&self) -> &'static str {
        "system"
    }

    fn stage(&self) -> Stage {
        Stage::Generate
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        AVAILABLE && !req.mime.as_deref().is_some_and(|m| m.starts_with("inode/"))
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        crate::thumb_providers::ensure_file(&req.entry.path)?;
        let side = req.size.pixels();
        Ok(imp::thumbnail(&req.entry.path, side, cancel)?
            .map(|picture| crate::thumb_providers::fit(picture, side)))
    }
}

/// Straight alpha from premultiplied, as a system draws thumbnails: a
/// colour over a partly transparent pixel divided back out.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn unpremultiply(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel =
                    u8::try_from((u16::from(*channel) * 255 + alpha / 2) / alpha).unwrap_or(255);
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use super::{CancelToken, Path};

    #[allow(clippy::unnecessary_wraps)]
    pub fn thumbnail(
        _path: &Path,
        _side: u32,
        _cancel: &CancelToken,
    ) -> anyhow::Result<Option<image::DynamicImage>> {
        Ok(None)
    }
}

/// Quick Look (`QLThumbnailGenerator`), asked for a thumbnail, not an
/// icon, drawn into RGBA by Core Graphics. Compiled, never run (5b).
// The system's calls take raw pointers: each `unsafe` block says why its
// call is sound, and nothing unsafe leaves the module.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod imp {
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::AnyThread as _;
    use objc2::rc::autoreleasepool;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
        CGImageByteOrderInfo,
    };
    use objc2_foundation::{NSError, NSURL};
    use objc2_quick_look_thumbnailing::{
        QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes,
        QLThumbnailGenerator, QLThumbnailRepresentation,
    };

    use super::{CancelToken, Path};

    /// How long Quick Look may take with one file.
    const PATIENCE: Duration = Duration::from_secs(30);

    pub fn thumbnail(
        path: &Path,
        side: u32,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<image::DynamicImage>> {
        let Some(url) = NSURL::from_file_path(path) else {
            return Ok(None);
        };
        let (sent, reply) = mpsc::sync_channel(1);
        let wanted = CGSize {
            width: f64::from(side),
            height: f64::from(side),
        };
        // SAFETY: a request for a file URL, a size and the one
        // representation asked; the shared generator answers it once, on a
        // queue of its own, with a representation or nil, which the block
        // only reads.
        let (generator, request) = autoreleasepool(|_| unsafe {
            let request =
                QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(
                    QLThumbnailGenerationRequest::alloc(),
                    &url,
                    wanted,
                    1.0,
                    QLThumbnailGenerationRequestRepresentationTypes::Thumbnail,
                );
            let generator = QLThumbnailGenerator::sharedGenerator();
            let handler = RcBlock::new(
                move |representation: *mut QLThumbnailRepresentation, _error: *mut NSError| {
                    let picture = autoreleasepool(|_| {
                        representation
                            .as_ref()
                            .and_then(|representation| pixels(&representation.CGImage()))
                    });
                    let _ = sent.try_send(picture);
                },
            );
            generator.generateBestRepresentationForRequest_completionHandler(&request, &handler);
            (generator, request)
        });
        let deadline = Instant::now() + PATIENCE;
        loop {
            match reply.recv_timeout(Duration::from_millis(50)) {
                Ok(picture) => return Ok(picture),
                Err(RecvTimeoutError::Timeout)
                    if cancel.is_cancelled() || Instant::now() > deadline =>
                {
                    // SAFETY: the request this generator was asked.
                    unsafe { generator.cancelRequest(&request) };
                    return Ok(None);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(None),
            }
        }
    }

    /// The picture as straight RGBA, drawn into a bitmap of its size: a
    /// thumbnail's, and a system icon's (`system_icons`).
    pub(crate) fn pixels(picture: &CGImage) -> Option<image::DynamicImage> {
        let (width, height) = (
            CGImage::width(Some(picture)),
            CGImage::height(Some(picture)),
        );
        if width == 0 || height == 0 || width * height > 64 << 20 {
            return None;
        }
        let mut rgba = vec![0u8; width * height * 4];
        let space = CGColorSpace::new_device_rgb()?;
        let layout = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
        // SAFETY: a buffer of `width × height` RGBA pixels, rows of
        // `width × 4` bytes, alive while the context draws into it.
        let context = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                Some(&space),
                layout,
            )
        }?;
        let whole = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: width as f64,
                height: height as f64,
            },
        };
        CGContext::draw_image(Some(&context), whole, Some(picture));
        drop(context);
        super::unpremultiply(&mut rgba);
        image::RgbaImage::from_raw(
            u32::try_from(width).ok()?,
            u32::try_from(height).ok()?,
            rgba,
        )
        .map(image::DynamicImage::ImageRgba8)
    }
}

/// The shell (`IShellItemImageFactory`), asked for a thumbnail only, its
/// bitmap read back as straight RGBA. Run under Wine, whose shell draws
/// none; compiled for the rest (5b).
// The system's calls take raw pointers: each `unsafe` block says why its
// call is sound, and nothing unsafe leaves the module.
#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use ::windows::Win32::Foundation::SIZE;
    use ::windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
    };
    use ::windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, IBindCtx};
    use ::windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
        SIIGBF_THUMBNAILONLY,
    };
    use ::windows::core::HSTRING;

    use super::{CancelToken, Path};

    // The signature every system's shares: Windows has no error to give.
    #[allow(clippy::unnecessary_wraps)]
    pub fn thumbnail(
        path: &Path,
        side: u32,
        _cancel: &CancelToken,
    ) -> anyhow::Result<Option<image::DynamicImage>> {
        // SAFETY: COM for this thread, as the shell needs it; a thread
        // already in it gets S_FALSE, which is no failure.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let name = HSTRING::from(path.as_os_str());
        // SAFETY: a NUL-terminated path, and no bind context.
        let Ok(factory) = (unsafe {
            SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(&name, None::<&IBindCtx>)
        }) else {
            return Ok(None);
        };
        let side = i32::try_from(side).unwrap_or(256);
        // SAFETY: a size, and flags asking a thumbnail only (never an icon).
        let Ok(bitmap) = (unsafe {
            factory.GetImage(
                SIZE { cx: side, cy: side },
                SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK,
            )
        }) else {
            return Ok(None);
        };
        let picture = pixels(bitmap).map(|(picture, _)| picture);
        // SAFETY: the bitmap the shell handed over, ours to delete.
        let _ = unsafe { DeleteObject(HGDIOBJ(bitmap.0)) };
        Ok(picture)
    }

    /// The bitmap's pixels, top row first, as straight RGBA, and whether
    /// it had an alpha channel: the shell's are BGRA, premultiplied, with
    /// no alpha at all (every byte 0) when the picture has none, which
    /// leaves them opaque (an icon's mask says what is not: `system_icons`).
    pub(crate) fn pixels(bitmap: HBITMAP) -> Option<(image::DynamicImage, bool)> {
        let mut info = BITMAP::default();
        // SAFETY: a BITMAP the size of which is given, filled by GDI.
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(bitmap.0),
                i32::try_from(size_of::<BITMAP>()).ok()?,
                Some((&raw mut info).cast()),
            )
        };
        let (width, height) = (info.bmWidth, info.bmHeight.abs());
        if got == 0 || width <= 0 || height <= 0 {
            return None;
        }
        let (w, h) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
        if w * h > 64 << 20 {
            return None;
        }
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).ok()?,
                biWidth: width,
                // Negative: the top row first.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; w * h * 4];
        // SAFETY: a memory DC for GetDIBits, and a buffer of the size its
        // header describes.
        let lines = unsafe {
            let dc = CreateCompatibleDC(None);
            let lines = GetDIBits(
                dc,
                bitmap,
                0,
                u32::try_from(height).ok()?,
                Some(bgra.as_mut_ptr().cast()),
                &raw mut header,
                DIB_RGB_COLORS,
            );
            let _ = DeleteDC(dc);
            lines
        };
        if lines == 0 {
            return None;
        }
        let mut rgba = bgra;
        let opaque = rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0);
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            if opaque {
                pixel[3] = 255;
            }
        }
        if !opaque {
            super::unpremultiply(&mut rgba);
        }
        image::RgbaImage::from_raw(width.unsigned_abs(), height.unsigned_abs(), rgba)
            .map(|picture| (image::DynamicImage::ImageRgba8(picture), !opaque))
    }
}

#[cfg(any(target_os = "macos", windows))]
pub(crate) use imp::pixels;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_premultiplied_pixel_gets_its_colour_back() {
        let mut rgba = [128, 64, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0];
        unpremultiply(&mut rgba);
        assert_eq!(rgba, [255, 128, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
    }

    /// The shell asked for a picture's thumbnail: whatever it answers
    /// (Wine's draws none), never a failure, and never larger than asked.
    #[cfg(windows)]
    #[test]
    fn the_shell_is_asked_for_a_pictures_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a picture \u{e9}t\u{e9}.png");
        image::RgbaImage::from_pixel(300, 200, image::Rgba([200, 30, 30, 255]))
            .save(&path)
            .unwrap();
        let req = ThumbRequest {
            entry: skwir_core::op::EntryRef::from_path(&path).unwrap(),
            mime: Some("image/png".into()),
            mtime: None,
            size: skwir_core::thumbs::ThumbSize::Normal,
        };
        let drawn = SystemThumbnails
            .thumbnail(&req, &CancelToken::new())
            .unwrap();
        if let Some(picture) = drawn {
            assert!(picture.width.max(picture.height) <= req.size.pixels());
        }
    }

    #[test]
    fn folders_are_never_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let req = |mime: Option<&str>| ThumbRequest {
            entry: skwir_core::op::EntryRef::from_path(dir.path()).unwrap(),
            mime: mime.map(Into::into),
            mtime: None,
            size: skwir_core::thumbs::ThumbSize::Normal,
        };
        assert!(!SystemThumbnails.accepts(&req(Some("inode/directory"))));
        assert_eq!(
            SystemThumbnails.accepts(&req(Some("image/heic"))),
            AVAILABLE
        );
    }
}
