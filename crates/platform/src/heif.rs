//! HEIC and AVIF, read by libheif in a helper of its own (ADR-148):
//! `skwir-heif-worker`, run isolated where the system allows, as Skwir's
//! other decoders of untrusted files are, where it is installed (Linux
//! packages carry it apart, `skwir-heif`).
//!
//! Taught to `image` as a decoder (`picture::formats`), so a HEIC is read
//! wherever a picture is (its size, a decode shrunk, a region, a
//! thumbnail, a comic's page), and counted as read here only where the
//! helper is found (`reads`): elsewhere the desktop's thumbnailer draws
//! it, as before. A region is decoded from the tiles it covers
//! (`Decoder`'s `picture::Regional`, ADR-148 amended): an iPhone's photo
//! is stored in tiles of 512 pixels, and a part of 2000 × 1600 of one of
//! 45 megapixels decodes a tenth of them. The thumbnail the file carries
//! stands for it while it decodes (`thumbnail`).
//!
//! A run is asked for by whoever decodes (`asking`, ADR-148 amended):
//! cancelled with its request, in a turn of the preview's own for the
//! picture the user waits for, and, for a grid's thumbnail, never waited
//! for (`Asking::busy`): the thumbnails take it again once a turn is free
//! (`when_free`), their worker going on with other files meanwhile.
use std::cell::{Cell, RefCell};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use image::error::{DecodingError, ImageFormatHint};
use image::{ColorType, DynamicImage, ImageDecoder, ImageError, ImageResult};
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
/// The preview's own run, for the picture the user waits for: never
/// behind thumbnails or what is read ahead.
static SHOWN: crate::process::Turns = crate::process::Turns::new(1);

/// Who asks for the pictures decoded on a thread (`asking`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asker {
    /// The picture the user waits for (the preview's own, a zoom's part):
    /// a turn of its own.
    Shown,
    /// What is read ahead, or one of many decoded together (a comic's
    /// pages, a contact sheet): one of the shared turns, waited for.
    Background,
    /// A grid's thumbnail: one of the shared turns, never waited for. With
    /// none free, the run is not made and the request is busy
    /// (`Asking::busy`).
    Thumbnail,
}

/// What a thread's decodes are asked with (`asking`).
struct Asked {
    cancel: CancelToken,
    asker: Asker,
    /// Whether a run found no turn free (`Asker::Thumbnail`).
    busy: Cell<bool>,
}

thread_local! {
    /// The innermost `asking` of this thread, read by the decoder
    /// `image`'s hook makes, which no argument of the caller's reaches.
    static ASKED: RefCell<Option<Asked>> = const { RefCell::new(None) };
}

/// The HEIF and AVIF pictures this thread decodes until the guard is
/// dropped are asked by `asker`, and their runs cancelled with `cancel`.
/// Unasked (a picture in a document), a run waits for one of the shared
/// turns and ends with its time alone.
pub fn asking(cancel: &CancelToken, asker: Asker) -> Asking {
    let now = Asked {
        cancel: cancel.clone(),
        asker,
        busy: Cell::new(false),
    };
    Asking {
        previous: ASKED.with(|slot| slot.replace(Some(now))),
    }
}

/// While held, what this thread's decodes are asked with (`asking`).
#[must_use = "the asking ends when the guard is dropped"]
pub struct Asking {
    previous: Option<Asked>,
}

impl Asking {
    /// Whether a run asked by a thumbnail found no turn free: the request
    /// is to be asked again once one is (`when_free`).
    pub fn busy(&self) -> bool {
        ASKED.with(|slot| slot.borrow().as_ref().is_some_and(|now| now.busy.get()))
    }
}

impl Drop for Asking {
    fn drop(&mut self) {
        let previous = self.previous.take();
        ASKED.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// Call `then` once one of the shared turns is free, on the thread giving
/// it back: a busy thumbnail asked again.
pub fn when_free(then: Box<dyn FnOnce() + Send>) {
    TURNS.when_free(then);
}

/// A turn for a run asked as this thread asks (`asking`), and the token
/// it is cancelled with; an error once cancelled, or where a thumbnail's
/// finds none free (noted busy).
fn turn() -> anyhow::Result<(crate::process::Turn<'static>, CancelToken)> {
    ASKED.with(|slot| {
        let slot = slot.borrow();
        let (cancel, by) = slot
            .as_ref()
            .map_or((CancelToken::new(), Asker::Background), |now| {
                (now.cancel.clone(), now.asker)
            });
        let turn = match by {
            Asker::Shown => SHOWN.take(&cancel),
            Asker::Background => TURNS.take(&cancel),
            Asker::Thumbnail => {
                let Some(turn) = TURNS.try_take() else {
                    if let Some(now) = slot.as_ref() {
                        now.busy.set(true);
                    }
                    anyhow::bail!("the HEIF helper is busy");
                };
                Some(turn)
            }
        };
        let turn = turn.ok_or_else(|| anyhow::anyhow!("cancelled"))?;
        Ok((turn, cancel))
    })
}

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

/// Whether `start`, a file's first bytes, begins a HEIF or an AVIF.
pub(crate) fn starts(start: &[u8]) -> bool {
    BRANDS.iter().any(|brand| {
        start.len() >= brand.len()
            && start
                .iter()
                .zip(brand.iter().zip(MASK))
                .all(|(&byte, (&sig, &mask))| byte & mask == sig)
    })
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

/// What is named in the helper's folder: the file, and what the helper
/// writes its pixels to.
const INPUT: &str = "input";
const OUTPUT: &str = "pixels";

fn failed(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Name("HEIF".into()),
        error,
    ))
}

/// What the helper said of a file's picture (`size`): its size as shown,
/// whether it has alpha, and the thumbnail it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Facts {
    width: u32,
    height: u32,
    alpha: bool,
    thumbnail: Carried,
}

/// The thumbnail a file carries, as the helper's `size` says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Carried {
    /// None it gives (none, or one of other proportions).
    None,
    /// One of this size, as shown.
    Sized(u32, u32),
    /// Not said, by a helper of before it was: asked of `thumbnail`.
    Unknown,
}

/// What the helper said of the files asked last, by their length and
/// their bytes' hash (`Work::key`): a request asks a picture's size, then
/// what it carries, then its pixels, each through a decoder of its own,
/// and the helper is run for its size once.
static KNOWN: parking_lot::Mutex<std::collections::VecDeque<((u64, u64), Facts)>> =
    parking_lot::const_mutex(std::collections::VecDeque::new());
/// How many are known at most: a grid's thumbnails asked at once.
const KNOWN_MOST: usize = 64;
/// What a file is hashed by at a time, whatever its reader gives.
const BLOCK: usize = 64 << 10;

/// A file handed to the helper: written to a folder in memory for it to
/// read, and what it writes there.
struct Work {
    program: &'static Path,
    folder: tempfile::TempDir,
    /// Its length and its bytes' hash.
    key: (u64, u64),
}

impl Work {
    fn new<R: Read + Seek>(mut reader: R) -> ImageResult<Work> {
        use std::hash::{BuildHasher, Hasher};
        static SEED: OnceLock<skwir_core::hashing::State> = OnceLock::new();
        let program = program().ok_or_else(|| failed("no HEIF helper"))?;
        // One too large is refused before anything of it is copied.
        if reader.seek(std::io::SeekFrom::End(0))? > FILE_BYTES {
            return Err(failed("the picture's file is too large"));
        }
        reader.rewind()?;
        let folder = crate::file::memory_dir("skwir-heif-")?;
        let mut file = std::fs::File::create(folder.path().join(INPUT))?;
        let mut hasher = SEED
            .get_or_init(skwir_core::hashing::State::default)
            .build_hasher();
        let mut block = vec![0u8; BLOCK];
        let mut length = 0u64;
        loop {
            // Blocks filled whole, so the hash is the bytes', not the reads'.
            let mut held = 0;
            while held < BLOCK {
                match reader.read(&mut block[held..]) {
                    Ok(0) => break,
                    Ok(count) => held += count,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if held == 0 {
                break;
            }
            hasher.write(&block[..held]);
            std::io::Write::write_all(&mut file, &block[..held])?;
            length += held as u64;
            if length > FILE_BYTES {
                return Err(failed("the picture's file is too large"));
            }
        }
        Ok(Work {
            program,
            folder,
            key: (length, hasher.finish()),
        })
    }

    /// What the helper says of the picture: known, or asked now.
    fn facts(&self) -> ImageResult<Facts> {
        if let Some(&(_, facts)) = KNOWN.lock().iter().find(|(key, _)| *key == self.key) {
            return Ok(facts);
        }
        let said = self
            .ask("size", &[])
            .map_err(|error| failed(format!("{error:#}")))?;
        let (width, height, alpha) =
            picture(&said)?.ok_or_else(|| failed("the HEIF helper said no size"))?;
        let thumbnail = match said.get(3..5) {
            Some(&[0, 0]) => Carried::None,
            Some(&[width, height]) => Carried::Sized(width, height),
            _ => Carried::Unknown,
        };
        let facts = Facts {
            width,
            height,
            alpha,
            thumbnail,
        };
        let mut known = KNOWN.lock();
        if known.len() >= KNOWN_MOST {
            known.pop_front();
        }
        known.push_back((self.key, facts));
        Ok(facts)
    }

    /// The helper run in `mode`, `args` after the most pixels it may
    /// decode: the numbers it says.
    fn ask(&self, mode: &str, args: &[u32]) -> anyhow::Result<Vec<u32>> {
        let (_turn, cancel) = turn()?;
        let input = self.folder.path().join(INPUT);
        let mut run = sandbox::untrusted(self.program, &input, self.folder.path(), Otherwise::Run)?;
        run.command
            .arg(&run.input)
            .arg(run.work.join(OUTPUT))
            .arg(mode)
            .arg(limits::WHOLE_PICTURE_PIXELS.to_string())
            .args(args.iter().map(u32::to_string));
        let said = crate::process::run(&mut run.command, &cancel, TIMEOUT)
            .map_err(|error| crate::process::explain(self.program, &cancel, error))?;
        String::from_utf8_lossy(&said)
            .split_whitespace()
            .map(|word| word.parse::<u32>().map_err(anyhow::Error::from))
            .collect()
    }

    /// The pixels the helper wrote, read into `buf`, which holds as many.
    fn pixels(&self, buf: &mut [u8]) -> ImageResult<()> {
        let mut file = crate::file::open_regular(&self.folder.path().join(OUTPUT))?;
        if file.metadata()?.len() != buf.len() as u64 {
            return Err(failed("the HEIF helper wrote another size"));
        }
        file.read_exact(buf)?;
        Ok(())
    }
}

/// The picture the helper said (its first three numbers: width, height,
/// alpha), `None` for `0 0 0` (no thumbnail), refused past what any
/// picture is decoded at.
fn picture(said: &[u32]) -> ImageResult<Option<(u32, u32, bool)>> {
    let &[width, height, alpha, ..] = said else {
        return Err(failed("the HEIF helper said no size"));
    };
    if (width, height) == (0, 0) {
        return Ok(None);
    }
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > limits::WHOLE_PICTURE_PIXELS
    {
        return Err(failed(format!("the picture is {width} × {height}")));
    }
    Ok(Some((width, height, alpha == 1)))
}

/// The thumbnail a HEIF or an AVIF carries (ADR-148 amended), turned as
/// the picture is shown, decoded by the helper where `wanted` takes its
/// longer side, which the helper says with the picture's size: what stands
/// for it while it decodes, or is it where it covers what is asked
/// (`picture::carried`). `None` where it carries none, or one refused.
pub(crate) fn thumbnail<R: Read + Seek>(
    reader: R,
    wanted: &impl Fn(u32) -> bool,
) -> ImageResult<Option<DynamicImage>> {
    let work = Work::new(reader)?;
    match work.facts()?.thumbnail {
        Carried::None => return Ok(None),
        Carried::Sized(width, height) if !wanted(width.max(height)) => return Ok(None),
        Carried::Sized(..) | Carried::Unknown => {}
    }
    let said = work
        .ask("thumbnail", &[])
        .map_err(|error| failed(format!("{error:#}")))?;
    let Some((width, height, alpha)) = picture(&said)? else {
        return Ok(None);
    };
    if !wanted(width.max(height)) {
        return Ok(None);
    }
    let channels = if alpha { 4 } else { 3 };
    let mut pixels = vec![0; width as usize * height as usize * channels];
    work.pixels(&mut pixels)?;
    let image = if alpha {
        image::RgbaImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgba8)
    } else {
        image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
    };
    image
        .map(Some)
        .ok_or_else(|| failed("the HEIF helper wrote another size"))
}

/// A HEIF or AVIF picture handed to the helper: its size asked as it is
/// made (or known, `Work::facts`), its pixels as they are read, the whole
/// or a region of it.
pub(crate) struct Decoder {
    work: Work,
    width: u32,
    height: u32,
    alpha: bool,
    /// What is read (left, top, width, height, as shown): the whole
    /// unless cut.
    region: [u32; 4],
}

impl Decoder {
    pub(crate) fn new<R: Read + Seek>(reader: R) -> ImageResult<Decoder> {
        let work = Work::new(reader)?;
        let Facts {
            width,
            height,
            alpha,
            ..
        } = work.facts()?;
        Ok(Decoder {
            work,
            width,
            height,
            alpha,
            region: [0, 0, width, height],
        })
    }

    fn whole(&self) -> bool {
        self.region == [0, 0, self.width, self.height]
    }
}

impl ImageDecoder for Decoder {
    fn dimensions(&self) -> (u32, u32) {
        (self.region[2], self.region[3])
    }

    fn color_type(&self) -> ColorType {
        if self.alpha {
            ColorType::Rgba8
        } else {
            ColorType::Rgb8
        }
    }

    /// Its pixels, read straight into `buf` from what the helper wrote.
    fn read_image(self, buf: &mut [u8]) -> ImageResult<()> {
        let said = if self.whole() {
            self.work.ask("decode", &[])
        } else {
            self.work.ask("region", &self.region)
        }
        .map_err(|error| failed(format!("{error:#}")))?;
        if picture(&said)? != Some((self.region[2], self.region[3], self.alpha)) {
            return Err(failed("the picture decoded at another size than it says"));
        }
        self.work.pixels(buf)
    }

    fn read_image_boxed(self: Box<Self>, buf: &mut [u8]) -> ImageResult<()> {
        (*self).read_image(buf)
    }
}

impl crate::picture::Regional for Decoder {
    fn cut(&mut self, region: [u32; 4]) {
        self.region = crate::picture::inside(region, (self.width, self.height));
    }

    fn region(&self) -> [u32; 4] {
        self.region
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thumbnail's run never waits for a turn: with none free it is not
    /// made and the request is busy; a run cancelled with its request is
    /// not made either; the preview's own has a turn of its own.
    #[test]
    fn a_run_is_asked_as_its_caller_says() {
        let held = [TURNS.try_take().unwrap(), TURNS.try_take().unwrap()];
        let thumbnail = asking(&CancelToken::new(), Asker::Thumbnail);
        assert!(turn().is_err());
        assert!(thumbnail.busy(), "busy, asked again once a turn is free");
        drop(thumbnail);
        {
            let _shown = asking(&CancelToken::new(), Asker::Shown);
            assert!(turn().is_ok(), "the shown one's turn is its own");
        }
        drop(held);
        let thumbnail = asking(&CancelToken::new(), Asker::Thumbnail);
        assert!(turn().is_ok());
        assert!(!thumbnail.busy());
        drop(thumbnail);
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let _background = asking(&cancelled, Asker::Background);
        assert!(turn().is_err(), "cancelled with its request");
    }

    /// The brands told apart by their first bytes, whatever the box's size.
    #[test]
    fn heif_and_avif_are_told_by_their_brand() {
        assert!(starts(b"\0\0\0\x18ftypheic\0\0\0\0"));
        assert!(starts(b"\0\0\0\x1cftypavif\0\0\0\0"));
        assert!(starts(b"\0\0\0\x20ftypmif1\0\0\0\0"));
        assert!(!starts(b"\0\0\0\x18ftypisom\0\0\0\0"));
        assert!(!starts(b"\0\0\0\x18ftypcrx \0\0\0\0"));
        assert!(!starts(b"\0\0\0\x18ftyphe"), "too short");
    }
}
