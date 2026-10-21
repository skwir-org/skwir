//! Thumbnail providers for `skwir_core::thumbs`: the freedesktop cache
//! (read, and write-back of what later providers make), an in-process image
//! decoder, the desktop's installed `*.thumbnailer` programs, and fallbacks
//! for what the desktop has no thumbnailer for: PDF first pages, Office
//! documents the preview has converted, and audio covers.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use image::DynamicImage;
use skwir_core::cancel::CancelToken;
use skwir_core::thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider};

use crate::thumbnails::{ThumbCache, ThumbSize};

/// MIME of a request: the caller's, else guessed from the file name.
fn mime_of(req: &ThumbRequest) -> Option<Arc<str>> {
    if let Some(m) = &req.mime {
        return Some(Arc::clone(m));
    }
    crate::mime::by_name(req.entry.path.file_name()?).map(|m| m.0.into())
}

/// What a picture another program drew may weigh.
const DRAWN_BYTES: u64 = 64 << 20;

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

/// The picture another program drew into `output`, at most `pixels` on
/// its longer side: its size checked before it is decoded. Read as a file
/// and no more than a picture holds: what an untrusted program leaves
/// there may be a pipe, which would hold the worker, or a link to an
/// endless device.
pub(crate) fn drawn(output: &Path, pixels: u32) -> anyhow::Result<Image> {
    let bytes = crate::file::read_regular(output, DRAWN_BYTES)?;
    let open = || -> anyhow::Result<_> {
        Ok(image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?)
    };
    let (w, h) = open()?.into_dimensions()?;
    anyhow::ensure!(
        u64::from(w) * u64::from(h) <= MAX_PIXELS,
        "what it drew is too large"
    );
    Ok(fit(open()?.decode()?, pixels))
}

/// Downscale so the longest side is at most `px`, then RGBA8.
pub(crate) fn fit(img: DynamicImage, px: u32) -> Image {
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

    /// Asked on the window's thread for every picture drawn: nothing is
    /// read here, the file is looked at on the worker that asks the cache.
    fn accepts(&self, req: &ThumbRequest) -> bool {
        self.cache.is_some() && req.entry.path.is_absolute()
    }

    fn failed_before(&self, req: &ThumbRequest) -> bool {
        self.cache
            .as_ref()
            .is_some_and(|cache| cache.failed(&req.entry.path, tools_stamp()))
    }

    fn store_failure(&self, req: &ThumbRequest) {
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
        if let Err(e) = cache.store_failed(&req.entry.path, mtime, tools_stamp()) {
            tracing::debug!(path = %req.entry.path.display(), "thumbnail failure not kept: {e:#}");
        }
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

/// What could draw thumbnails here, stamped: skwir's version, the
/// desktop's thumbnailers (each by its name and when it was changed), and
/// the programs skwir asks itself. A failure remembered with another
/// stamp is tried again: a tool installed since may draw the file. Read
/// once a process.
pub fn tools_stamp() -> &'static str {
    static STAMP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    STAMP.get_or_init(|| {
        let mut parts = vec![env!("CARGO_PKG_VERSION").to_owned()];
        for dir in crate::xdg::data_search() {
            let Ok(read) = std::fs::read_dir(dir.join("thumbnailers")) else {
                continue;
            };
            for entry in read.flatten() {
                let changed = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                parts.push(format!("{}:{changed}", entry.file_name().to_string_lossy()));
            }
        }
        for program in [
            "magick",
            "convert",
            "ffmpeg",
            "ffmpegthumbnailer",
            "pdftoppm",
            "soffice",
            "libreoffice",
        ] {
            if skwir_core::os_path::find_program(program).is_some() {
                parts.push(program.to_owned());
            }
        }
        parts.sort();
        format!("{:x}", md5::compute(parts.join("\n")))
    })
}

// ------------------------------------------------------------- image decoder

/// Decodes in-process what `decodes` lists: rasters with the `image`
/// crate, drawings with resvg.
#[derive(Debug, Clone, Default)]
pub struct ImageDecoder;

/// The types the crate can decode with our feature set, each by one of
/// its names (`mime::Types` knows the others).
static RASTERS: crate::mime::Types = crate::mime::Types::new(&[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/tiff",
    "image/vnd.microsoft.icon",
    "image/qoi",
    "image/x-tga",
    "image/x-portable-pixmap",
    "image/x-portable-graymap",
    "image/x-portable-bitmap",
    "image/x-portable-anymap",
    "image/x-exr",
    "image/x-dds",
]);

/// The drawings resvg draws: SVG, plain or compressed.
static DRAWINGS: crate::mime::Types =
    crate::mime::Types::new(&["image/svg+xml", "image/svg+xml-compressed"]);

/// Whether skwir decodes pictures of type `mime` itself: a raster the
/// `image` crate reads, or a drawing (`is_drawing`). What it lists,
/// `ImageDecoder` thumbnails and the fallbacks leave alone.
pub fn decodes(mime: &str) -> bool {
    raster(mime) || is_drawing(mime)
}

/// Whether a file of type `mime` is a drawing, read by
/// `inspection::read_drawing` and drawn by `inspection::drawing`.
pub fn is_drawing(mime: &str) -> bool {
    DRAWINGS.contains(mime)
}

fn raster(mime: &str) -> bool {
    mime.starts_with("image/") && RASTERS.contains(mime)
}

/// Refuse to decode anything larger than this many pixels.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

impl ThumbnailProvider for ImageDecoder {
    fn id(&self) -> &'static str {
        "image-decoder"
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        mime_of(req).is_some_and(|m| decodes(&m))
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = &req.entry.path;
        if mime_of(req).is_some_and(|m| is_drawing(&m)) {
            return drawn_to(path, req.size.pixels(), cancel);
        }
        let open =
            || crate::picture::open(path).with_context(|| format!("open {}", path.display()));
        let (w, h) = open()?.into_dimensions()?;
        anyhow::ensure!(
            u64::from(w) * u64::from(h) <= MAX_PIXELS,
            "{w}x{h} is too large to thumbnail"
        );
        let img = crate::picture::decode_fitted(open()?, req.size.pixels())?;
        if cancel.is_cancelled() {
            return Ok(None);
        }
        Ok(Some(fit(img, req.size.pixels())))
    }
}

/// A drawing at `pixels` on its longer side, enlarged too, as a drawing
/// has as many pixels as it is asked for: as inert as the preview's,
/// nothing outside it read.
fn drawn_to(path: &Path, pixels: u32, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
    let bytes =
        crate::inspection::read_drawing(path, crate::inspection::limits::Bounds::QUICK, cancel)?;
    let drawn = crate::inspection::drawing_within(&bytes, pixels)?;
    if cancel.is_cancelled() {
        return Ok(None);
    }
    Ok(Some(Image {
        width: drawn.width,
        height: drawn.height,
        rgba: drawn.rgba.into(),
    }))
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
    let mut exec = None;
    let mut try_exec = None;
    let mut mime_types = Vec::new();
    for (group, key, value) in crate::keyfile::entries(text) {
        if group != "Thumbnailer Entry" {
            continue;
        }
        match key {
            "Exec" => exec = Some(crate::keyfile::unescape(value.trim_end())),
            "TryExec" => try_exec = Some(crate::keyfile::unescape(value.trim_end())),
            "MimeType" => mime_types = crate::keyfile::list(value),
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

/// The directories the desktop keeps thumbnailers in, most specific first:
/// where `XDG_DATA_DIRS` says the system's data is, as for every other
/// file the desktop shares (a distribution that keeps them elsewhere, as
/// NixOS does, names its folders there).
pub fn default_thumbnailer_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<_> = crate::xdg::data_search()
        .into_iter()
        .map(|d| d.join("thumbnailers"))
        .collect();
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
                .is_some_and(|t| skwir_core::os_path::find_program(t).is_none())
            {
                continue;
            }
            out.push(entry);
        }
    }
    out
}

/// Substitute `%i`, `%o`, `%s`, `%u`, `%%` in an `Exec` line, per word.
/// Paths go in as they are, bytes and all: a name that is not UTF-8 must
/// reach the thumbnailer as the file that exists.
pub fn substitute_exec(exec: &str, input: &Path, output: &Path, size: u32) -> Vec<OsString> {
    let uri = crate::uri::file_uri(input);
    // Every word is expanded, quoted or not: GNOME's thumbnailers expand
    // codes wherever they are, and a thumbnailer is written for them.
    crate::keyfile::exec_words(exec)
        .into_iter()
        .map(|(w, _)| {
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

/// What a thumbnailer writes, in its private folder.
const THUMBNAIL: &str = "thumbnail.png";

/// `entry`'s command for `path`, isolated where bubblewrap can (ADR-031):
/// the file read at its name inside the sandbox (`%i`, and `%u` as its
/// URI), the picture written in `work` (`%o`), the program found as the
/// desktop would find it and run from where it really is. Where nothing
/// isolates, it runs as the desktop would run it.
fn thumbnailer(
    entry: &ThumbnailerEntry,
    path: &Path,
    work: &Path,
    pixels: u32,
) -> anyhow::Result<std::process::Command> {
    let words = substitute_exec(&entry.exec, path, &work.join(THUMBNAIL), pixels);
    let Some(exe) = words.first() else {
        anyhow::bail!("empty Exec in {}", entry.name)
    };
    let program = crate::process::required(&exe.to_string_lossy())?;
    let mut run = crate::sandbox::untrusted(&program, path, work, crate::sandbox::Otherwise::Run)?;
    let argv = substitute_exec(&entry.exec, &run.input, &run.work.join(THUMBNAIL), pixels);
    run.command.args(&argv[1..]);
    Ok(run.command)
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
                // A file's type is the database's own name for it: a
                // thumbnailer that claims an alias claims that type too.
                let canonical = crate::mime::unalias(&crate::mime::Mime(m.clone())).0;
                by_mime.entry(canonical).or_insert(i);
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

    /// Whether a thumbnailer claims files of type `mime`.
    pub fn claims(&self, mime: &str) -> bool {
        self.by_mime.contains_key(mime)
    }

    /// The file at `path`, of type `mime`, drawn by the thumbnailer that
    /// claims it, at most `pixels` on its longer side: a thumbnail, or, for
    /// a picture skwir does not decode, its preview. `None` when none
    /// claims it or the request was cancelled.
    pub fn draw(
        &self,
        path: &Path,
        mime: &str,
        pixels: u32,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Image>> {
        match self.by_mime.get(mime) {
            Some(&i) => self.run(&self.entries[i], path, pixels, cancel),
            None => Ok(None),
        }
    }

    fn run(
        &self,
        entry: &ThumbnailerEntry,
        path: &Path,
        pixels: u32,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Image>> {
        // A FIFO named `x.mp4` would hold the thumbnailer until its timeout.
        ensure_file(path)?;
        let scratch = tempfile::tempdir()?;
        let mut command = thumbnailer(entry, path, scratch.path(), pixels)?;
        let result = crate::process::run(&mut command, cancel, self.timeout);
        if cancel.is_cancelled() {
            return Ok(None);
        }
        result?;
        drawn(&scratch.path().join(THUMBNAIL), pixels)
            .map(Some)
            .with_context(|| format!("{}: read what it drew", entry.name))
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
        self.draw(&req.entry.path, &m, req.size.pixels(), cancel)
    }
}

// ------------------------------------------------------------- PDF pages

/// A PDF's first page, drawn as the reader draws it (`document::first_page`:
/// `PDFium`, which skwir's packages carry, then Poppler), else by Poppler's
/// `pdftoppm`. A fallback: a thumbnailer the desktop installed for PDFs
/// runs first, so skwir draws what other applications draw, and the cache
/// stores what this makes for them.
#[derive(Debug, Clone, Default)]
pub struct PdfPages {
    /// `PDFium`'s library as the settings name it.
    pub pdfium: crate::document::PdfiumChoice,
}

/// The first page of the PDF at `pdf`, at most `side` pixels: by a reader
/// engine, else by `pdftoppm`, else none.
fn first_pdf_page(
    pdf: &Path,
    side: u32,
    pdfium: &crate::document::PdfiumChoice,
    scratch: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Image>> {
    let library = pdfium.read().clone();
    if let Some(page) = crate::document::first_page(
        pdf,
        side,
        &crate::document::AUTO,
        library.as_deref(),
        cancel,
    )? {
        return Ok(Some(page));
    }
    let Ok(renderer) = crate::process::required("pdftoppm") else {
        return Ok(None);
    };
    crate::preview::render_pdf_page(&renderer, pdf, 1, side, scratch, cancel).map(Some)
}

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
        ensure_file(&req.entry.path)?;
        let scratch = tempfile::tempdir()?;
        let page = first_pdf_page(
            &req.entry.path,
            req.size.pixels(),
            &self.pdfium,
            scratch.path(),
            cancel,
        );
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let Some(page) = page? else {
            return Ok(None);
        };
        anyhow::ensure!(
            page.width.max(page.height) <= req.size.pixels(),
            "a page larger than asked"
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
    cache: skwir_core::cache::CacheSlot,
    /// `PDFium`'s library as the settings name it.
    pdfium: crate::document::PdfiumChoice,
}

impl OfficePages {
    pub fn new(cache: skwir_core::cache::CacheSlot, pdfium: crate::document::PdfiumChoice) -> Self {
        Self { cache, pdfium }
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
        crate::preview::is_office(&req.entry.path, req.mime.as_deref())
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let source = req.entry.resolve()?;
        let scratch = tempfile::tempdir()?;
        let Some(pdf) =
            crate::preview::converted_pdf(&self.cache, req.entry.id, &source, scratch.path())?
        else {
            return Ok(None);
        };
        let page = first_pdf_page(
            &pdf,
            req.size.pixels(),
            &self.pdfium,
            scratch.path(),
            cancel,
        );
        if cancel.is_cancelled() {
            return Ok(None);
        }
        page
    }
}

// ------------------------------------------------------------- audio covers

/// Audio containers that carry a cover picture.
static COVERED_AUDIO: crate::mime::Types = crate::mime::Types::new(&[
    "audio/mpeg",
    "audio/x-flac",
    "audio/mp4",
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
]);

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
        mime_of(req).is_some_and(|m| COVERED_AUDIO.contains(&m))
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
    use skwir_core::op::EntryRef;
    use skwir_core::thumbs::ThumbSize as CoreSize;

    const SAMPLE: &str = "[Thumbnailer Entry]\nTryExec=totem-video-thumbnailer\nExec=totem-video-thumbnailer -s %s %u %o\nMimeType=video/mp4;video/x-matroska;\n";

    // The freedesktop thumbnailers and cache (step G, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_thumbnailer_that_claims_an_alias_claims_the_type() {
        let t = Thumbnailers::new(vec![ThumbnailerEntry {
            name: "models".into(),
            try_exec: None,
            exec: "draw %i %o".into(),
            mime_types: vec!["model/x.stl-binary".into()],
        }]);
        assert!(t.claims("model/x.stl-binary"));
        assert!(t.claims("model/stl"), "the database's name for it");
        assert!(!t.claims("model/obj"));
    }

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

    // The freedesktop thumbnailers and cache (step G, ADR-035).
    #[cfg(target_os = "linux")]
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

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn image_decoder_accepts_by_mime_or_extension_what_skwir_decodes() {
        let r = |p: &str, mime: Option<&str>| ThumbRequest {
            entry: EntryRef::new(skwir_core::FileId::new(0, 0), p),
            mime: mime.map(Into::into),
            mtime: None,
            size: CoreSize::Normal,
        };
        assert!(ImageDecoder.accepts(&r("/x/a.png", None)));
        assert!(ImageDecoder.accepts(&r("/x/a.bin", Some("image/jpeg"))));
        assert!(ImageDecoder.accepts(&r("/x/a.svg", None)), "a drawing");
        assert!(ImageDecoder.accepts(&r("/x/a.svgz", None)), "compressed");
        assert!(!ImageDecoder.accepts(&r("/x/a.txt", None)));
        assert!(
            !ImageDecoder.accepts(&r("/x/a.heic", None)),
            "left to the thumbnailers"
        );
    }

    #[test]
    fn image_decoder_draws_a_drawing_to_the_thumbnail_side() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        // Small, and wider than tall: a drawing is enlarged to the side.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8">
            <rect width="16" height="8" fill="#ff0000"/></svg>"##;
        let plain = tmp.path().join("l'été en vrac.svg");
        std::fs::write(&plain, svg).unwrap();
        let compressed = tmp.path().join("l'été en vrac.svgz");
        let mut gzip = flate2::write::GzEncoder::new(
            std::fs::File::create(&compressed).unwrap(),
            flate2::Compression::default(),
        );
        gzip.write_all(svg).unwrap();
        gzip.finish().unwrap();
        for (path, mime, size, side) in [
            (&plain, "image/svg+xml", CoreSize::Normal, 128),
            (&plain, "image/svg+xml", CoreSize::Large, 256),
            (
                &compressed,
                "image/svg+xml-compressed",
                CoreSize::Normal,
                128,
            ),
        ] {
            let req = ThumbRequest {
                entry: EntryRef::from_path(path).unwrap(),
                mime: Some(mime.into()),
                mtime: None,
                size,
            };
            assert!(ImageDecoder.accepts(&req));
            let img = ImageDecoder
                .thumbnail(&req, &CancelToken::new())
                .unwrap()
                .unwrap();
            assert_eq!((img.width, img.height), (side, side / 2), "{path:?}");
            assert!(img.is_whole());
            assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
        }
    }

    // The freedesktop thumbnailers and cache (step G, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_fake_thumbnailer_script_produces_a_thumbnail() {
        // A script that "renders" a 4x4 red picture, with nothing but the
        // shell: isolated, it sees only the system and its own folder.
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("fake-thumb.sh");
        let red = "255 0 0 ".repeat(16);
        // Where it can be isolated, a file beside it is out of its sight.
        let secret = tmp.path().join("secret");
        std::fs::write(&secret, "").unwrap();
        let unseen = if crate::sandbox::available() {
            format!("[ -e '{}' ] && exit 4\n", secret.display())
        } else {
            String::new()
        };
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ \"$1\" = 128 ] || exit 3\n{unseen}printf 'P3 4 4 255 {red}' > \"$3\"\n"
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
