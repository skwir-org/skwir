//! `ImageMagick`, when installed, draws the pictures nothing else draws
//! (ADR-031): the last resort for thumbnails, and for the preview of a
//! picture bibli does not decode. It runs only isolated
//! (`crate::sandbox`), under a policy written for each run: it reads the
//! one format named and writes PAM, runs no other program, and holds
//! bounded memory, on one thread.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Context;
use bibli_core::CancelToken;
use bibli_core::thumbs::Image;
use parking_lot::{Condvar, Mutex};

use crate::inspection::limits;
use crate::{process, sandbox};

/// Formats never asked for, whatever a file's type says: they read more
/// than the file's pixels. `PostScript` and PDF are programs run by
/// Ghostscript; drawings and scripts read other files; text and web pages
/// are typeset; video goes through another program; the rest read the
/// network, the screen, or what their name says.
pub const REFUSED: &[&str] = &[
    "AI",
    "EPDF",
    "EPI",
    "EPS",
    "EPS2",
    "EPS3",
    "EPSF",
    "EPSI",
    "EPT",
    "EPT2",
    "EPT3",
    "PCL",
    "PDF",
    "PDFA",
    "POCKETMOD",
    "PS",
    "PS2",
    "PS3",
    "XPS",
    "MSL",
    "MVG",
    "MSVG",
    "RSVG",
    "SVG",
    "SVGZ",
    "CAPTION",
    "FTXT",
    "HTM",
    "HTML",
    "LABEL",
    "PANGO",
    "SHTML",
    "TEXT",
    "TXT",
    "3G2",
    "3GP",
    "APNG",
    "AVI",
    "FLV",
    "M2V",
    "M4V",
    "MKV",
    "MOV",
    "MP4",
    "MPEG",
    "MPG",
    "VID",
    "WEBM",
    "WMV",
    "FILE",
    "FTP",
    "HTTP",
    "HTTPS",
    "INLINE",
    "SCREENSHOT",
    "STEGANO",
    "TILE",
    "URL",
    "X",
];

/// At most this many run at once, whatever the number of workers: each
/// holds a picture's pixels in floating point, about 200 MB for 12
/// megapixels.
pub const RUNNING: usize = 2;

/// How long one may take, decoding a large raw file included.
const TIMEOUT: Duration = Duration::from_secs(30);

/// What a run writes, inside the sandbox.
const OUTPUT: &str = "picture.pam";

/// The installed `ImageMagick` and the formats it reads.
#[derive(Clone, Debug)]
pub struct Magick {
    path: PathBuf,
    /// Readable formats, by `ImageMagick`'s own names (`HEIC`, `CR2`).
    formats: Arc<HashSet<String>>,
}

impl Magick {
    /// `ImageMagick` 7's `magick`, else `ImageMagick` 6's `convert` when it
    /// says it is `ImageMagick`, if it can run isolated and reads the policy
    /// it is given. Runs it two or three times: call on a worker, once.
    pub fn find() -> Option<Magick> {
        if !sandbox::available() {
            return None;
        }
        let path = ["magick", "convert"].into_iter().find_map(|name| {
            let path = process::required(name).ok()?;
            let version = ask(Command::new(&path).arg("-version"))?;
            version.contains("ImageMagick").then_some(path)
        })?;
        let listing = ask(Command::new(&path).args(["-list", "format"]))?;
        let magick = Magick::listing(path, &listing);
        if magick.reads_its_policy() {
            Some(magick)
        } else {
            tracing::debug!("ImageMagick ignores the policy it is given: not used");
            None
        }
    }

    /// `path`, reading what its `-list format` said.
    pub fn listing(path: PathBuf, listing: &str) -> Magick {
        Magick {
            path,
            formats: Arc::new(readable(listing)),
        }
    }

    /// Whether it draws files of type `mime`: a picture bibli does not
    /// decode itself, in a format it reads.
    pub fn draws(&self, mime: &str) -> bool {
        self.format(mime, None).is_some()
    }

    /// The format to read a file of type `mime` as: the first of the
    /// extensions the MIME database names the type by (`path`'s own first,
    /// when it is one of them) that `ImageMagick` reads and is not refused.
    pub fn format(&self, mime: &str, path: Option<&Path>) -> Option<String> {
        if !mime.starts_with("image/") || crate::thumb_providers::decodes(mime) {
            return None;
        }
        let extensions = crate::mime::extensions(&crate::mime::Mime(mime.to_owned()));
        let own = path
            .map(crate::inspection::extension)
            .filter(|own| extensions.contains(own));
        own.into_iter()
            .chain(extensions)
            .map(|extension| extension.to_ascii_uppercase())
            .find(|format| {
                format.chars().all(|c| c.is_ascii_alphanumeric())
                    && self.formats.contains(format)
                    && !REFUSED.contains(&format.as_str())
            })
    }

    /// The picture at `path`, of type `mime`, at most `pixels` on its
    /// longer side. `None` when cancelled, waiting its turn or running.
    pub fn draw(
        &self,
        path: &Path,
        mime: &str,
        pixels: u32,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Image>> {
        let format = self
            .format(mime, Some(path))
            .context("ImageMagick does not read this picture")?;
        crate::thumb_providers::ensure_file(path)?;
        let Some(_turn) = Turn::take(cancel) else {
            return Ok(None);
        };
        let work = tempfile::tempdir()?;
        prepare(work.path(), &format)?;
        let mut run = sandbox::command(&self.path, path, work.path())?;
        let (Some(input), Some(folder)) = (run.input.to_str(), run.work.to_str()) else {
            anyhow::bail!("the picture's path inside the sandbox is not text");
        };
        let arguments = arguments(&format, input, folder, pixels);
        run.command.envs(environment(&run.work)).args(arguments);
        let result = process::run(&mut run.command, cancel, TIMEOUT);
        if cancel.is_cancelled() {
            return Ok(None);
        }
        result?;
        crate::thumb_providers::drawn(&work.path().join(OUTPUT), pixels).map(Some)
    }

    /// Whether `ImageMagick` reads the policy it is pointed at: a build that
    /// did not would read any format it was handed.
    fn reads_its_policy(&self) -> bool {
        let check = || -> anyhow::Result<bool> {
            let work = tempfile::tempdir()?;
            let policy = prepare(work.path(), "PAM")?;
            let mut run = sandbox::command(&self.path, &policy, work.path())?;
            let expected = run.work.join("policy").join("policy.xml");
            let listed = process::run(
                run.command
                    .envs(environment(&run.work))
                    .args(["-list", "policy"]),
                &CancelToken::new(),
                limits::HELPER_TIMEOUT,
            )?;
            Ok(String::from_utf8_lossy(&listed).contains(&*expected.to_string_lossy()))
        };
        check().unwrap_or_else(|error| {
            tracing::debug!("ImageMagick's policy check failed: {error:#}");
            false
        })
    }
}

/// What a program said, run outside the sandbox: it reads no file of
/// anyone's.
fn ask(command: &mut Command) -> Option<String> {
    let said = process::run(command, &CancelToken::new(), limits::HELPER_TIMEOUT).ok()?;
    Some(String::from_utf8_lossy(&said).into_owned())
}

/// The formats a `-list format` listing says can be read: lines of a
/// name (marked `*` when read from memory), a mode (`rw+`, `r--`) and a
/// description, among headers and descriptions running onto more lines.
fn readable(listing: &str) -> HashSet<String> {
    listing
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?.trim_end_matches('*');
            let mode = words.next()?.as_bytes();
            let is_mode = mode.len() == 3
                && matches!(mode[0], b'r' | b'-')
                && matches!(mode[1], b'w' | b'-')
                && matches!(mode[2], b'+' | b'-');
            (is_mode && mode[0] == b'r' && !name.is_empty()).then(|| name.to_ascii_uppercase())
        })
        .collect()
}

/// The private folder made ready for a run reading `format`: its policy,
/// and the folder `ImageMagick` keeps pixels in beyond its memory. Returns
/// the policy's path.
fn prepare(work: &Path, format: &str) -> anyhow::Result<PathBuf> {
    let folder = work.join("policy");
    std::fs::create_dir(&folder)?;
    std::fs::create_dir(work.join("tmp"))?;
    let policy = folder.join("policy.xml");
    std::fs::write(&policy, policy_for(format))?;
    Ok(policy)
}

/// The security policy of a run reading `format`.
fn policy_for(format: &str) -> String {
    format!(
        r#"<policymap>
  <policy domain="resource" name="memory" value="256MiB"/>
  <policy domain="resource" name="map" value="512MiB"/>
  <policy domain="resource" name="disk" value="1GiB"/>
  <policy domain="resource" name="thread" value="1"/>
  <policy domain="resource" name="time" value="30"/>
  <policy domain="delegate" rights="none" pattern="*"/>
  <policy domain="path" rights="none" pattern="@*"/>
  <policy domain="coder" rights="none" pattern="*"/>
  <policy domain="coder" rights="read" pattern="{format}"/>
  <policy domain="coder" rights="write" pattern="PAM"/>
</policymap>
"#
    )
}

/// Where `ImageMagick` finds the policy, and keeps what does not fit in
/// memory: in the private folder, `work` as the run sees it.
fn environment(work: &Path) -> [(&'static str, PathBuf); 2] {
    [
        ("MAGICK_CONFIGURE_PATH", work.join("policy")),
        ("MAGICK_TEMPORARY_PATH", work.join("tmp")),
    ]
}

/// The arguments that draw `inside`, read as `format`: its first frame
/// (a GIMP image's layers flattened, as they are its frames), upright, in
/// sRGB, shrunk by averaging to at most `pixels`, as 8-bit PAM, written
/// in `work` as the run sees it.
fn arguments(format: &str, inside: &str, work: &str, pixels: u32) -> Vec<String> {
    let mut arguments = if format == "XCF" {
        let flattened = ["-background", "none", "-layers", "flatten"].map(String::from);
        std::iter::once(format!("{format}:{inside}"))
            .chain(flattened)
            .collect()
    } else {
        vec![format!("{format}:{inside}[0]")]
    };
    arguments.extend(["-auto-orient", "-colorspace", "sRGB", "-scale"].map(String::from));
    arguments.push(format!("{pixels}x{pixels}>"));
    arguments.extend(["-depth", "8"].map(String::from));
    arguments.push(format!("PAM:{work}/{OUTPUT}"));
    arguments
}

/// A turn to run: at most `RUNNING` at once.
struct Turn;

fn running() -> &'static (Mutex<usize>, Condvar) {
    static RUNNING_NOW: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    RUNNING_NOW.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

impl Turn {
    /// Waits for a turn; `None` once `cancel` is cancelled.
    fn take(cancel: &CancelToken) -> Option<Turn> {
        let (count, freed) = running();
        let mut count = count.lock();
        while *count >= RUNNING {
            if cancel.is_cancelled() {
                return None;
            }
            freed.wait_for(&mut count, Duration::from_millis(50));
        }
        if cancel.is_cancelled() {
            return None;
        }
        *count += 1;
        Some(Turn)
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let (count, freed) = running();
        *count.lock() -= 1;
        freed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "   Format  Mode  Description
-------------------------------------------------------------------------------
      ARW  r--   Sony Alpha Raw Format (0.22.1-Release)
     AVIF  rw+   AV1 Image File Format (1.23.5)
     DJVU* r--   Deja vu
             DJVU 1.2 specification is available there and at
      EPS  rw-   Encapsulated PostScript
     HEIC  rw+   High Efficiency Image Format (1.23.5)
      ORA  ---   OpenRaster format
      PAM* rw+   Common 2-dimensional bitmap format
      PNG* rw-   Portable Network Graphics (libpng 1.6.58)
      SVG  rw+   Scalable Vector Graphics (RSVG 2.62.3)
      XCF* r--   GIMP image
";

    #[test]
    fn the_listing_says_what_it_reads() {
        let formats = readable(LISTING);
        for format in [
            "ARW", "AVIF", "DJVU", "EPS", "HEIC", "PAM", "PNG", "SVG", "XCF",
        ] {
            assert!(formats.contains(format), "{format}");
        }
        assert!(!formats.contains("ORA"), "listed, not readable");
        assert!(!formats.contains("FORMAT") && !formats.contains("DJVU 1.2"));
        assert_eq!(formats.len(), 9);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_picture_is_read_as_the_format_its_type_names() {
        let magick = Magick::listing(PathBuf::from("/x/magick"), LISTING);
        assert_eq!(magick.format("image/avif", None).as_deref(), Some("AVIF"));
        assert_eq!(
            magick
                .format("image/heif", Some(Path::new("/p/IMG_1.HEIC")))
                .as_deref(),
            Some("HEIC")
        );
        assert_eq!(
            magick.format("image/x-sony-arw", None).as_deref(),
            Some("ARW")
        );
        assert!(magick.draws("image/x-xcf"));
        assert!(!magick.draws("image/png"), "bibli decodes it");
        assert!(!magick.draws("image/svg+xml"), "bibli draws it");
        assert!(!magick.draws("image/x-eps"), "PostScript is refused");
        assert!(!magick.draws("image/jxl"), "this one does not read it");
        assert!(!magick.draws("application/pdf"), "not a picture");
    }

    #[test]
    fn a_run_reads_one_format_and_writes_pam_on_one_thread() {
        let policy = policy_for("AVIF");
        assert!(policy.contains(r#"<policy domain="coder" rights="none" pattern="*"/>"#));
        assert!(policy.contains(r#"rights="read" pattern="AVIF""#));
        assert!(policy.contains(r#"<policy domain="delegate" rights="none" pattern="*"/>"#));
        assert!(policy.contains(r#"name="thread" value="1""#));
        assert_eq!(
            arguments("AVIF", "/input/document.avif", "/work", 256),
            [
                "AVIF:/input/document.avif[0]",
                "-auto-orient",
                "-colorspace",
                "sRGB",
                "-scale",
                "256x256>",
                "-depth",
                "8",
                "PAM:/work/picture.pam"
            ]
        );
        assert_eq!(
            arguments("XCF", "/input/document.xcf", "/work", 64)[..4],
            ["XCF:/input/document.xcf", "-background", "none", "-layers"]
        );
    }

    #[test]
    fn a_turn_waits_for_one_to_end_and_gives_way_when_cancelled() {
        let cancel = CancelToken::new();
        let taken: Vec<Turn> = (0..RUNNING)
            .map(|_| Turn::take(&cancel).expect("free"))
            .collect();
        let waiting = CancelToken::new();
        let waiter = {
            let waiting = waiting.clone();
            std::thread::spawn(move || Turn::take(&waiting).is_some())
        };
        std::thread::sleep(Duration::from_millis(120));
        waiting.cancel();
        assert!(!waiter.join().unwrap(), "a cancelled wait takes no turn");
        drop(taken);
        assert!(Turn::take(&cancel).is_some(), "turns come back");
    }

    /// `ImageMagick` and bubblewrap as installed: run by hand
    /// (`cargo nextest run -p bibli-platform --run-ignored only magick`).
    #[test]
    #[ignore = "needs ImageMagick with libheif, and bubblewrap"]
    fn magick_draws_an_avif_isolated_and_refuses_what_it_was_not_asked() {
        let magick = Magick::find().expect("ImageMagick, isolated");
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("made.png");
        image::RgbaImage::from_fn(300, 200, |x, _| {
            image::Rgba([(x % 256) as u8, 90, 200, 128])
        })
        .save(&png)
        .unwrap();
        // Written by ImageMagick itself, outside the sandbox: a test fixture.
        let avif = dir.path().join("a photo’s [name].avif");
        let made = Command::new(&magick.path)
            .arg(&png)
            .arg(format!("AVIF:{}", avif.display()))
            .status()
            .unwrap();
        assert!(made.success());
        let image = magick
            .draw(&avif, "image/avif", 128, &CancelToken::new())
            .unwrap()
            .expect("drawn");
        assert_eq!((image.width, image.height), (128, 85));
        let alpha = image.rgba[3];
        assert!((120..=136).contains(&alpha), "transparency kept: {alpha}");
        // Bytes that say something else are still read as the type said.
        let lying = dir.path().join("text.avif");
        std::fs::write(&lying, "not a picture").unwrap();
        assert!(
            magick
                .draw(&lying, "image/avif", 128, &CancelToken::new())
                .is_err()
        );
    }
}
