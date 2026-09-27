//! Thumbnail providers for `bibli_core::thumbs`: the freedesktop cache
//! (read, and write-back of what later providers make), an in-process image
//! decoder, the desktop's installed `*.thumbnailer` programs, and fallbacks
//! for what the desktop has no thumbnailer for: PDF first pages, Office
//! documents the preview has converted, and audio covers.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use bibli_core::cancel::CancelToken;
use bibli_core::thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider};
use image::{DynamicImage, ImageDecoder as _};

use crate::thumbnails::{ThumbCache, ThumbSize};

/// MIME of a request: the caller's, else guessed from the file name.
fn mime_of(req: &ThumbRequest) -> Option<Arc<str>> {
    if let Some(m) = &req.mime {
        return Some(Arc::clone(m));
    }
    crate::mime::by_name(req.entry.path.file_name()?).map(|m| m.0.into())
}

fn is_regular_file(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file())
}

/// Refuse what is not a file, following links: a FIFO named `x.pdf` would
/// hold a renderer until its timeout.
pub fn ensure_file(p: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        std::fs::metadata(p)?.is_file(),
        "{} is not a regular file",
        p.display()
    );
    Ok(())
}

/// Downscale so the longest side is at most `px`, then RGBA8.
fn fit(img: DynamicImage, px: u32) -> Image {
    let img = if img.width().max(img.height()) > px {
        img.thumbnail(px, px)
    } else {
        img
    };
    let rgba = img.into_rgba8();
    Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw().into(),
    }
}

// ------------------------------------------------------------- freedesktop cache

/// The freedesktop thumbnail cache. Its stage puts it first: it answers
/// instantly for anything the desktop has already thumbnailed and stores what
/// the other providers produce so every other application benefits too.
#[derive(Debug, Clone)]
pub struct FreedesktopCache {
    /// `None` when there is no cache directory at all.
    cache: Option<ThumbCache>,
    write_back: bool,
}

impl FreedesktopCache {
    /// The desktop's cache (`$XDG_CACHE_HOME/thumbnails`); `write_back`
    /// stores what later providers make.
    pub fn desktop(write_back: bool) -> Self {
        Self {
            cache: ThumbCache::desktop(),
            write_back,
        }
    }

    /// A cache at an explicit root, for tests and the bench.
    pub fn at(cache: ThumbCache, write_back: bool) -> Self {
        Self {
            cache: Some(cache),
            write_back,
        }
    }
}

impl ThumbnailProvider for FreedesktopCache {
    fn id(&self) -> &'static str {
        "freedesktop-cache"
    }

    fn stage(&self) -> Stage {
        Stage::Cache
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        self.cache.is_some() && req.entry.path.is_absolute() && is_regular_file(&req.entry.path)
    }

    fn thumbnail(
        &self,
        req: &ThumbRequest,
        _cancel: &CancelToken,
    ) -> anyhow::Result<Option<Image>> {
        Ok(self
            .cache
            .as_ref()
            .and_then(|cache| cache.load(&req.entry.path, ThumbSize::from_core(req.size))))
    }

    fn store(&self, req: &ThumbRequest, image: &Image) {
        let Some(cache) = self.cache.as_ref().filter(|_| self.write_back) else {
            return;
        };
        let mtime = req.mtime.or_else(|| {
            std::fs::symlink_metadata(&req.entry.path)
                .ok()?
                .modified()
                .ok()
        });
        let Some(mtime) = mtime else { return };
        if let Err(e) = cache.store(
            &req.entry.path,
            ThumbSize::from_core(req.size),
            image,
            mtime,
        ) {
            tracing::debug!(path = %req.entry.path.display(), "thumbnail cache write failed: {e:#}");
        }
    }
}

// ------------------------------------------------------------- image decoder

/// Decodes raster images in-process with the `image` crate.
#[derive(Debug, Clone, Default)]
pub struct ImageDecoder;

/// `image/<subtype>` values the crate can decode with our feature set.
const SUBTYPES: &[&str] = &[
    "png",
    "jpeg",
    "gif",
    "webp",
    "bmp",
    "x-ms-bmp",
    "tiff",
    "x-icon",
    "vnd.microsoft.icon",
    "qoi",
    "x-qoi",
    "x-tga",
    "x-targa",
    "x-portable-pixmap",
    "x-portable-graymap",
    "x-portable-bitmap",
    "x-portable-anymap",
];

/// Refuse to decode anything larger than this many pixels.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

impl ThumbnailProvider for ImageDecoder {
    fn id(&self) -> &'static str {
        "image-decoder"
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        mime_of(req).is_some_and(|m| {
            m.strip_prefix("image/")
                .is_some_and(|sub| SUBTYPES.contains(&sub))
        })
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = &req.entry.path;
        let open = || -> anyhow::Result<image::ImageReader<BufReader<std::fs::File>>> {
            let f = crate::file::open_regular(path)
                .with_context(|| format!("open {}", path.display()))?;
            Ok(image::ImageReader::new(BufReader::new(f)).with_guessed_format()?)
        };
        let (w, h) = open()?.into_dimensions()?;
        anyhow::ensure!(
            u64::from(w) * u64::from(h) <= MAX_PIXELS,
            "{w}x{h} is too large to thumbnail"
        );
        let mut decoder = open()?.into_decoder()?;
        let orientation = decoder
            .orientation()
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let mut img = DynamicImage::from_decoder(decoder)?;
        if cancel.is_cancelled() {
            return Ok(None);
        }
        img.apply_orientation(orientation);
        Ok(Some(fit(img, req.size.pixels())))
    }
}

// ------------------------------------------------------------- thumbnailers

/// One parsed `*.thumbnailer` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailerEntry {
    pub name: String,
    pub try_exec: Option<String>,
    pub exec: String,
    pub mime_types: Vec<String>,
}

/// Parse the `[Thumbnailer Entry]` group of a `.thumbnailer` file.
pub fn parse_thumbnailer(name: &str, text: &str) -> Option<ThumbnailerEntry> {
    let mut in_group = false;
    let mut exec = None;
    let mut try_exec = None;
    let mut mime_types = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(group) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_group = group == "Thumbnailer Entry";
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Exec" => exec = Some(v.trim().to_owned()),
            "TryExec" => try_exec = Some(v.trim().to_owned()),
            "MimeType" => {
                mime_types = v
                    .split(';')
                    .map(str::trim)
                    .filter(|m| !m.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            _ => {}
        }
    }
    Some(ThumbnailerEntry {
        name: name.to_owned(),
        try_exec,
        exec: exec?,
        mime_types,
    })
}

/// The directories the desktop keeps thumbnailers in, most specific first.
pub fn default_thumbnailer_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = crate::xdg::data_home() {
        dirs.push(d.join("thumbnailers"));
    }
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join(".nix-profile/share/thumbnailers"));
    }
    for d in crate::xdg::data_dirs() {
        dirs.push(d.join("thumbnailers"));
    }
    dirs.push(PathBuf::from("/run/current-system/sw/share/thumbnailers"));
    dirs.dedup();
    dirs
}

/// Read every `*.thumbnailer` in `dirs`, keeping the first definition of a
/// name and dropping those whose `TryExec` is not installed.
pub fn discover(dirs: &[PathBuf]) -> Vec<ThumbnailerEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "thumbnailer"))
            .collect();
        files.sort();
        for f in files {
            let name = f
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !seen.insert(name.clone()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&f) else {
                continue;
            };
            let Some(entry) = parse_thumbnailer(&name, &text) else {
                continue;
            };
            if entry
                .try_exec
                .as_deref()
                .is_some_and(|t| bibli_core::os_path::find_program(t).is_none())
            {
                continue;
            }
            out.push(entry);
        }
    }
    out
}

/// Split a command line into words, honoring single and double quotes and
/// backslash escapes, which is what desktop files expect.
fn shell_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    in_word = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            (None, c) => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    words
}

/// Substitute `%i`, `%o`, `%s`, `%u`, `%%` in an `Exec` line, per word.
/// Paths go in as they are, bytes and all: a name that is not UTF-8 must
/// reach the thumbnailer as the file that exists.
pub fn substitute_exec(exec: &str, input: &Path, output: &Path, size: u32) -> Vec<OsString> {
    let uri = crate::uri::file_uri(input);
    shell_words(exec)
        .into_iter()
        .map(|w| {
            let mut out = OsString::with_capacity(w.len());
            let mut literal = String::new();
            let mut it = w.chars();
            while let Some(c) = it.next() {
                if c != '%' {
                    literal.push(c);
                    continue;
                }
                match it.next() {
                    Some('i') => {
                        out.push(std::mem::take(&mut literal));
                        out.push(input);
                    }
                    Some('o') => {
                        out.push(std::mem::take(&mut literal));
                        out.push(output);
                    }
                    Some('s') => literal.push_str(&size.to_string()),
                    Some('u') => literal.push_str(&uri),
                    Some('%') | None => literal.push('%'),
                    Some(other) => {
                        literal.push('%');
                        literal.push(other);
                    }
                }
            }
            out.push(literal);
            out
        })
        .collect()
}

/// Runs the desktop's `*.thumbnailer` programs for the MIME types they claim.
#[derive(Debug)]
pub struct Thumbnailers {
    entries: Vec<ThumbnailerEntry>,
    by_mime: HashMap<String, usize>,
    timeout: Duration,
}

impl Thumbnailers {
    /// Discover from the desktop's standard directories.
    pub fn from_system() -> Self {
        Self::new(discover(&default_thumbnailer_dirs()))
    }

    pub fn new(entries: Vec<ThumbnailerEntry>) -> Self {
        let mut by_mime = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            for m in &e.mime_types {
                by_mime.entry(m.clone()).or_insert(i);
            }
        }
        Thumbnailers {
            entries,
            by_mime,
            timeout: Duration::from_secs(10),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[ThumbnailerEntry] {
        &self.entries
    }

    fn run(
        &self,
        entry: &ThumbnailerEntry,
        req: &ThumbRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Image>> {
        // A FIFO named `x.mp4` would hold the thumbnailer until its timeout.
        anyhow::ensure!(
            std::fs::metadata(&req.entry.path)?.is_file(),
            "{} is not a regular file",
            req.entry.path.display()
        );
        let scratch = tempfile::tempdir()?;
        let output = scratch.path().join("thumbnail.png");
        let argv = substitute_exec(&entry.exec, &req.entry.path, &output, req.size.pixels());
        let Some((exe, exe_args)) = argv.split_first() else {
            anyhow::bail!("empty Exec in {}", entry.name)
        };
        let result = crate::process::run(
            std::process::Command::new(exe).args(exe_args),
            cancel,
            self.timeout,
        );
        if cancel.is_cancelled() {
            return Ok(None);
        }
        result?;
        let (w, h) = image::ImageReader::open(&output)?
            .with_guessed_format()?
            .into_dimensions()?;
        anyhow::ensure!(
            u64::from(w) * u64::from(h) <= MAX_PIXELS,
            "thumbnailer output is too large"
        );
        image::open(&output)
            .map(|img| Some(fit(img, req.size.pixels())))
            .with_context(|| format!("{}: decode its output", entry.name))
    }
}

impl ThumbnailProvider for Thumbnailers {
    fn id(&self) -> &'static str {
        "thumbnailers"
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        !self.entries.is_empty() && mime_of(req).is_some_and(|m| self.by_mime.contains_key(&*m))
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let Some(m) = mime_of(req) else {
            return Ok(None);
        };
        let Some(&i) = self.by_mime.get(&*m) else {
            return Ok(None);
        };
        self.run(&self.entries[i], req, cancel)
    }
}

// ------------------------------------------------------------- PDF pages

/// A PDF's first page, rendered by Poppler's `pdftoppm` the way the document
/// preview renders its pages. A fallback: a thumbnailer the desktop installed
/// for PDFs runs first, so bibli draws what other applications draw, and the
/// cache stores what this makes for them.
#[derive(Debug, Clone, Default)]
pub struct PdfPages;

impl ThumbnailProvider for PdfPages {
    fn id(&self) -> &'static str {
        "pdf-pages"
    }

    fn stage(&self) -> Stage {
        Stage::Fallback
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        mime_of(req).is_some_and(|m| &*m == "application/pdf")
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let Ok(renderer) = crate::process::required("pdftoppm") else {
            return Ok(None);
        };
        ensure_file(&req.entry.path)?;
        let scratch = tempfile::tempdir()?;
        let page = crate::preview::render_pdf_page(
            &renderer,
            &req.entry.path,
            1,
            req.size.pixels(),
            scratch.path(),
            cancel,
        );
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let page = page?;
        anyhow::ensure!(
            page.width.max(page.height) <= req.size.pixels(),
            "pdftoppm made a page larger than asked"
        );
        Ok(Some(page))
    }
}

// ------------------------------------------------------------- Office pages

/// An Office document's first page, from the PDF the document preview
/// converted it to (`preview::converted_pdf`), rendered like a PDF's. It
/// never converts: `LibreOffice` takes seconds and hundreds of megabytes per
/// document, too much for each tile in view, so a document has a thumbnail
/// once it has been previewed (`DocumentPreviews::on_converted` asks again).
#[derive(Debug, Clone)]
pub struct OfficePages {
    /// The document preview's cache, where its conversions are.
    cache: bibli_core::cache::CacheSlot,
}

impl OfficePages {
    pub fn new(cache: bibli_core::cache::CacheSlot) -> Self {
        Self { cache }
    }
}

impl ThumbnailProvider for OfficePages {
    fn id(&self) -> &'static str {
        "office-pages"
    }

    fn stage(&self) -> Stage {
        Stage::Fallback
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        crate::preview::is_office(&req.entry.path)
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let Ok(renderer) = crate::process::required("pdftoppm") else {
            return Ok(None);
        };
        let source = req.entry.resolve()?;
        let scratch = tempfile::tempdir()?;
        let Some(pdf) =
            crate::preview::converted_pdf(&self.cache, req.entry.id, &source, scratch.path())?
        else {
            return Ok(None);
        };
        let page = crate::preview::render_pdf_page(
            &renderer,
            &pdf,
            1,
            req.size.pixels(),
            scratch.path(),
            cancel,
        );
        if cancel.is_cancelled() {
            return Ok(None);
        }
        Ok(Some(page?))
    }
}

// ------------------------------------------------------------- audio covers

/// Audio containers that carry a cover picture.
const COVERED_AUDIO: &[&str] = &[
    "audio/mpeg",
    "audio/flac",
    "audio/x-flac",
    "audio/mp4",
    "audio/m4a",
    "audio/x-m4a",
    "audio/aac",
    "audio/ogg",
    "audio/x-vorbis+ogg",
    "audio/x-opus+ogg",
    "audio/opus",
    "audio/x-ms-wma",
    "audio/x-ape",
    "audio/x-wavpack",
    "audio/x-aiff",
    "audio/x-matroska",
    "audio/webm",
];

/// A recording's cover, as `ffmpeg` reads it for the audio preview. A
/// fallback: the desktop's audio thumbnailer, when installed, comes first.
/// Only containers that carry covers: never a playlist, whose type is
/// `audio/` too.
#[derive(Debug, Clone, Default)]
pub struct AudioCovers;

impl ThumbnailProvider for AudioCovers {
    fn id(&self) -> &'static str {
        "audio-covers"
    }

    fn stage(&self) -> Stage {
        Stage::Fallback
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        mime_of(req).is_some_and(|m| COVERED_AUDIO.contains(&&*m))
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let Ok(ffmpeg) = crate::process::required("ffmpeg") else {
            return Ok(None);
        };
        ensure_file(&req.entry.path)?;
        let scratch = tempfile::tempdir()?;
        let side = req.size.pixels();
        let png = crate::preview::ffmpeg_picture(
            &ffmpeg,
            &req.entry.path,
            None,
            side,
            scratch.path(),
            cancel,
        );
        if cancel.is_cancelled() {
            return Ok(None);
        }
        Ok(Some(crate::preview::decode_preview(&png?, side)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::op::EntryRef;
    use bibli_core::thumbs::ThumbSize as CoreSize;

    const SAMPLE: &str = "[Thumbnailer Entry]\nTryExec=totem-video-thumbnailer\nExec=totem-video-thumbnailer -s %s %u %o\nMimeType=video/mp4;video/x-matroska;\n";

    #[test]
    fn parses_a_thumbnailer_file() {
        let e = parse_thumbnailer("totem", SAMPLE).unwrap();
        assert_eq!(e.try_exec.as_deref(), Some("totem-video-thumbnailer"));
        assert_eq!(e.mime_types, ["video/mp4", "video/x-matroska"]);
        assert!(
            parse_thumbnailer("x", "[Other]\nExec=foo\n").is_none(),
            "Exec outside the group is ignored"
        );
    }

    #[test]
    fn substitutes_exec_placeholders_per_word() {
        let argv = substitute_exec(
            "tool -s %s \"%i\" %o 'a b' %% %u",
            Path::new("/p/a b.mp4"),
            Path::new("/t/out.png"),
            128,
        );
        assert_eq!(
            argv,
            [
                "tool",
                "-s",
                "128",
                "/p/a b.mp4",
                "/t/out.png",
                "a b",
                "%",
                "file:///p/a%20b.mp4"
            ]
            .map(OsString::from)
        );
    }

    #[cfg(unix)]
    #[test]
    fn paths_that_are_not_utf8_reach_the_thumbnailer_unchanged() {
        use std::os::unix::ffi::OsStrExt;
        let input = Path::new(std::ffi::OsStr::from_bytes(b"/p/caf\xe9.mp4"));
        let argv = substitute_exec("tool --in=%i %o", input, Path::new("/t/o.png"), 128);
        assert_eq!(argv[1].as_bytes(), b"--in=/p/caf\xe9.mp4");
        assert_eq!(argv[2], "/t/o.png");
    }

    #[test]
    fn image_decoder_accepts_by_mime_or_extension_only_for_rasters() {
        let r = |p: &str, mime: Option<&str>| ThumbRequest {
            entry: EntryRef::new(bibli_core::EntryId { dev: 0, ino: 0 }, p),
            mime: mime.map(Into::into),
            mtime: None,
            size: CoreSize::Normal,
        };
        assert!(ImageDecoder.accepts(&r("/x/a.png", None)));
        assert!(ImageDecoder.accepts(&r("/x/a.bin", Some("image/jpeg"))));
        assert!(
            !ImageDecoder.accepts(&r("/x/a.svg", None)),
            "svg is not decodable here"
        );
        assert!(!ImageDecoder.accepts(&r("/x/a.txt", None)));
    }

    #[test]
    fn a_fake_thumbnailer_script_produces_a_thumbnail() {
        // A 4x4 red PNG fixture the script "renders".
        let tmp = tempfile::tempdir().unwrap();
        let fixture = tmp.path().join("fixture.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .save(&fixture)
            .unwrap();
        let script = tmp.path().join("fake-thumb.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ \"$1\" = 128 ] || exit 3\ncp {} \"$3\"\n",
                fixture.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let dir = tmp.path().join("thumbnailers");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("fake.thumbnailer"), format!("[Thumbnailer Entry]\nTryExec={s}\nExec={s} %s %i %o\nMimeType=application/x-fake;\n", s = script.display())).unwrap();
        std::fs::write(dir.join("missing.thumbnailer"), "[Thumbnailer Entry]\nTryExec=/nonexistent/bin\nExec=/nonexistent/bin %i %o\nMimeType=application/x-missing;\n").unwrap();
        let found = discover(&[dir]);
        assert_eq!(
            found.len(),
            1,
            "the thumbnailer whose TryExec is missing is dropped"
        );
        let t = Thumbnailers::new(found);
        let input = tmp.path().join("doc.fake");
        std::fs::write(&input, b"").unwrap();
        let req = ThumbRequest {
            entry: EntryRef::from_path(&input).unwrap(),
            mime: Some("application/x-fake".into()),
            mtime: None,
            size: CoreSize::Normal,
        };
        assert!(t.accepts(&req));
        let img = t
            .thumbnail(&req, &CancelToken::new())
            .unwrap()
            .expect("thumbnail");
        assert_eq!((img.width, img.height), (4, 4));
        assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
        let other = ThumbRequest {
            mime: Some("video/mp4".into()),
            ..req
        };
        assert!(!t.accepts(&other));
    }

    #[test]
    fn image_decoder_thumbnails_and_downscales() {
        let tmp = tempfile::tempdir().unwrap();
        let big = tmp.path().join("big.png");
        image::RgbaImage::from_fn(300, 150, |x, _| image::Rgba([(x % 256) as u8, 0, 0, 255]))
            .save(&big)
            .unwrap();
        let req = ThumbRequest {
            entry: EntryRef::from_path(&big).unwrap(),
            mime: None,
            mtime: None,
            size: CoreSize::Normal,
        };
        let img = ImageDecoder
            .thumbnail(&req, &CancelToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(
            (img.width, img.height),
            (128, 64),
            "aspect kept, longest side 128"
        );
        assert_eq!(img.rgba.len(), 128 * 64 * 4);
    }
}
