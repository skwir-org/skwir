//! HEIC and AVIF, read by libheif in a helper of its own (ADR-148):
//! `skwir-heif-worker`, run isolated where the system allows, as Skwir's
//! other decoders of untrusted files are, where it is installed (Linux
//! packages carry it apart, `skwir-heif`).
//!
//! Taught to `image` as a decoder (`picture::formats`), so a HEIC is read
//! wherever a picture is (its size, a decode shrunk, a region, a
//! thumbnail, a comic's page), and counted as read here only where the
//! helper is found (`reads`): elsewhere the desktop's thumbnailer draws
//! it, as before.
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use image::error::{DecodingError, ImageFormatHint};
use image::{ColorType, ImageDecoder, ImageError, ImageResult};
use skwir_core::cancel::CancelToken;

use crate::inspection::limits;
use crate::sandbox::{self, Otherwise};

/// The helper.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "SKWIR_HEIF_WORKER",
    executable: "skwir-heif-worker",
};

/// The types it reads.
static TYPES: crate::mime::Types = crate::mime::Types::new(&[
    "image/heic",
    "image/heif",
    "image/heic-sequence",
    "image/heif-sequence",
    "image/avif",
]);

/// A file's first bytes the helper takes, after its box's size: the
/// brands of HEIF (HEVC in `heic`, `heix`, `hevc`, `heim`, `heis`, `hevm`,
/// `hevs`, a generic `mif1` or `msf1`) and AVIF (`avif`, `avis`).
const BRANDS: [&[u8; 12]; 11] = [
    b"\0\0\0\0ftypheic",
    b"\0\0\0\0ftypheix",
    b"\0\0\0\0ftyphevc",
    b"\0\0\0\0ftypheim",
    b"\0\0\0\0ftypheis",
    b"\0\0\0\0ftyphevm",
    b"\0\0\0\0ftyphevs",
    b"\0\0\0\0ftypmif1",
    b"\0\0\0\0ftypmsf1",
    b"\0\0\0\0ftypavif",
    b"\0\0\0\0ftypavis",
];
/// The box's size is any.
const MASK: &[u8; 12] = &[0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

/// The most read of a file: a photo is a few megabytes.
const FILE_BYTES: u64 = 256 << 20;
/// A run's time: a large picture decodes in a second or two.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Runs at once, the others waiting: each holds a picture whole.
static TURNS: crate::process::Turns = crate::process::Turns::new(2);

/// Where the helper is, once asked; `None` where it is not installed.
fn program() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND.get_or_init(|| WORKER.find()).as_deref()
}

/// Whether a file of type `mime` is read here: its type is HEIF's or
/// AVIF's, and the helper is installed.
pub fn reads(mime: &str) -> bool {
    TYPES.contains(mime) && program().is_some()
}

/// Teach `image` the formats. Called once, by `picture::formats`, which
/// looks for no helper: where none is installed, a decode fails as it did
/// before (HEIC was no format of `image`'s, AVIF one it was not built to
/// decode), and the types are not counted as read here (`reads`).
pub(crate) fn register() {
    image::hooks::register_decoding_hook(
        "heif".into(),
        Box::new(|reader| Ok(Box::new(Decoder::new(reader)?))),
    );
    for brand in BRANDS {
        image::hooks::register_format_detection_hook("heif".into(), brand, Some(MASK));
    }
}

/// A HEIF or AVIF picture handed to the helper: its size asked as it is
/// made, its pixels as they are read.
struct Decoder {
    program: &'static Path,
    /// The file, written here for the helper to read.
    work: tempfile::TempDir,
    width: u32,
    height: u32,
    alpha: bool,
}

/// What the file is named in the helper's folder.
const INPUT: &str = "input";
/// What the helper writes its pixels to.
const OUTPUT: &str = "pixels";

fn failed(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Name("HEIF".into()),
        error,
    ))
}

impl Decoder {
    fn new<R: Read + Seek>(mut reader: R) -> ImageResult<Decoder> {
        let program = program().ok_or_else(|| failed("no HEIF helper"))?;
        let work = crate::file::memory_dir("skwir-heif-")?;
        let mut file = std::fs::File::create(work.path().join(INPUT))?;
        reader.rewind()?;
        let copied = std::io::copy(&mut reader.by_ref().take(FILE_BYTES + 1), &mut file)?;
        if copied > FILE_BYTES {
            return Err(failed("the picture's file is too large"));
        }
        drop(file);
        let mut decoder = Decoder {
            program,
            work,
            width: 0,
            height: 0,
            alpha: false,
        };
        (decoder.width, decoder.height, decoder.alpha) = decoder
            .ask("size")
            .map_err(|error| failed(format!("{error:#}")))?;
        Ok(decoder)
    }

    /// The helper run in `mode`: the size it says.
    fn ask(&self, mode: &str) -> anyhow::Result<(u32, u32, bool)> {
        let cancel = CancelToken::new();
        let _turn = TURNS.take(&cancel);
        let input = self.work.path().join(INPUT);
        let mut run = sandbox::untrusted(self.program, &input, self.work.path(), Otherwise::Run)?;
        run.command
            .arg(&run.input)
            .arg(run.work.join(OUTPUT))
            .arg(mode)
            .arg(limits::WHOLE_PICTURE_PIXELS.to_string());
        let said = crate::process::run(&mut run.command, &cancel, TIMEOUT)
            .map_err(|error| crate::process::explain(self.program, &cancel, error))?;
        let said = String::from_utf8_lossy(&said);
        let mut words = said.split_whitespace().map(str::parse::<u32>);
        let (Some(Ok(width)), Some(Ok(height)), Some(Ok(alpha))) =
            (words.next(), words.next(), words.next())
        else {
            anyhow::bail!("the HEIF helper said no size");
        };
        anyhow::ensure!(
            width > 0
                && height > 0
                && u64::from(width) * u64::from(height) <= limits::WHOLE_PICTURE_PIXELS,
            "the picture is {width} × {height}"
        );
        Ok((width, height, alpha == 1))
    }
}

impl ImageDecoder for Decoder {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn color_type(&self) -> ColorType {
        if self.alpha {
            ColorType::Rgba8
        } else {
            ColorType::Rgb8
        }
    }

    fn read_image(self, buf: &mut [u8]) -> ImageResult<()> {
        let said = self
            .ask("decode")
            .map_err(|error| failed(format!("{error:#}")))?;
        if said != (self.width, self.height, self.alpha) {
            return Err(failed("the picture decoded at another size than it says"));
        }
        let pixels =
            crate::file::read_regular(&self.work.path().join(OUTPUT), buf.len() as u64 + 1)?;
        if pixels.len() != buf.len() {
            return Err(failed("the HEIF helper wrote another size"));
        }
        buf.copy_from_slice(&pixels);
        Ok(())
    }

    fn read_image_boxed(self: Box<Self>, buf: &mut [u8]) -> ImageResult<()> {
        (*self).read_image(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The brands told apart by their first bytes, whatever the box's size.
    #[test]
    fn heif_and_avif_are_told_by_their_brand() {
        let matches = |start: &[u8]| {
            BRANDS.iter().any(|brand| {
                start
                    .iter()
                    .zip(brand.iter().zip(MASK))
                    .all(|(&byte, (&sig, &mask))| byte & mask == sig)
            })
        };
        assert!(matches(b"\0\0\0\x18ftypheic\0\0\0\0"));
        assert!(matches(b"\0\0\0\x1cftypavif\0\0\0\0"));
        assert!(matches(b"\0\0\0\x20ftypmif1\0\0\0\0"));
        assert!(!matches(b"\0\0\0\x18ftypisom\0\0\0\0"));
        assert!(!matches(b"\0\0\0\x18ftypcrx \0\0\0\0"));
    }
}
