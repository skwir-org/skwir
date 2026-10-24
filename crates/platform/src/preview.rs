//! On-demand document pages and video frames using installed tools.
//! All methods below run on workers. No renderer process survives its request.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::Context;
use parking_lot::{Mutex, MutexGuard};
use skwir_core::cache::CacheSlot;
use skwir_core::preview::{Preview, PreviewInfo, PreviewProvider, PreviewRequest};
use skwir_core::{CancelToken, thumbs::Image};

use crate::inspection::{check, extension};
use crate::process;
use crate::sandbox::{self, Otherwise};

const MAX_SIDE: u32 = 2048;
const MAX_VIDEO_SIDE: u32 = 8192;
const MAX_ARTIFACT: u64 = 64 * 1024 * 1024;
const RENDER_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a converter (`LibreOffice`, Pandoc) may take over one file.
pub const CONVERT_TIMEOUT: Duration = Duration::from_secs(45);

/// Wait for a gate until it is free or the request is cancelled. There is no
/// deadline of its own: whoever holds a gate is bounded by the timeouts of
/// the tools it runs, so a waiter gives up only when its request does.
fn acquire<'a, T>(gate: &'a Mutex<T>, cancel: &CancelToken) -> anyhow::Result<MutexGuard<'a, T>> {
    loop {
        check(cancel)?;
        if let Some(guard) = gate.try_lock_for(Duration::from_millis(50)) {
            return Ok(guard);
        }
    }
}

/// What a renderer wrote, read as a file and no more than `MAX_ARTIFACT`:
/// a program working on an untrusted file may leave a pipe there, or a
/// link to an endless device.
fn read_artifact(path: &Path) -> anyhow::Result<Vec<u8>> {
    crate::file::read_regular(path, MAX_ARTIFACT).map_err(|error| {
        if error.kind() == std::io::ErrorKind::InvalidData {
            anyhow::anyhow!("preview output is too large")
        } else {
            error.into()
        }
    })
}

pub(crate) fn decode_preview(bytes: &[u8], max_side: u32) -> anyhow::Result<Image> {
    let reader = || image::ImageReader::new(Cursor::new(bytes)).with_guessed_format();
    let (w, h) = reader()?.into_dimensions()?;
    let side = max_side.clamp(64, MAX_VIDEO_SIDE);
    anyhow::ensure!(
        w > 0 && h > 0 && w <= side && h <= side,
        "preview dimensions exceed limit"
    );
    // Renderers already size their output, so the image is taken as it is:
    // nothing is resampled again, not even on cache hits.
    let image = reader()?.decode()?.to_rgba8();
    Ok(Image {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw().into(),
    })
}

fn encode_preview(image: &Image) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // Poppler's PNG compression dominates page latency on scanned PDFs.
        // Keep the same lossless pixels, using our fast encoder for disk cache.
        encoder.set_compression(png::Compression::Fast);
        encoder.write_header()?.write_image_data(&image.rgba)?;
    }
    Ok(bytes)
}

/// Include backend executable metadata so upgrading a tool invalidates its output.
fn tool_stamp(path: &Path) -> anyhow::Result<(PathBuf, u64, u128)> {
    let m = path.metadata()?;
    Ok((
        path.to_owned(),
        m.len(),
        m.modified()?.duration_since(UNIX_EPOCH)?.as_nanos(),
    ))
}

fn fingerprint(request: &PreviewRequest, path: &Path) -> anyhow::Result<Vec<u8>> {
    fingerprint_of(request.entry.id, path)
}

/// A file whose number a mount made up is kept by its path and version
/// alone (`FileId::lasts`).
fn fingerprint_of(id: skwir_core::FileId, path: &Path) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(&(
        id.lasts().then_some(id),
        path,
        crate::document::file_version(path)?,
    ))?)
}

fn key(base: &[u8], suffix: impl serde::Serialize) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(&("preview-v1", base, suffix))?)
}

/// Where what `tool` made of `path` (entry `id`) is cached, `what` telling
/// the products of one conversion apart: by the file's version and the
/// tool's, so either changing makes it again.
pub fn made_key(
    id: skwir_core::FileId,
    path: &Path,
    tool: &Path,
    what: impl serde::Serialize,
) -> anyhow::Result<Vec<u8>> {
    key(&fingerprint_of(id, path)?, (tool_stamp(tool)?, what))
}

// A tiny JSON header followed by the compressed picture (PNG, or JPEG for a
// film's frame), not an array of JSON pixels.
fn cached(cache: &CacheSlot, key: &[u8], max_side: u32) -> Option<Preview> {
    let bytes = cache.get(key)?;
    let n = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let end = 4usize.checked_add(n)?;
    let info = serde_json::from_slice(bytes.get(4..end)?).ok()?;
    let image = decode_preview(bytes.get(end..)?, max_side).ok()?;
    Some(Preview { image, info })
}

fn finish(
    cache: &CacheSlot,
    key: &[u8],
    image: Image,
    png: &[u8],
    info: PreviewInfo,
    cancel: &CancelToken,
) -> anyhow::Result<Preview> {
    check(cancel)?;
    let header = serde_json::to_vec(&info)?;
    let mut bytes = (u32::try_from(header.len())?).to_le_bytes().to_vec();
    bytes.extend(header);
    bytes.extend(png);
    check(cancel)?;
    // Cache failure must not prevent a preview; it is always disposable.
    if cache.put(key, &bytes).is_ok() {
        cache.trim_soon(bytes.len() as u64, 128 * 1024 * 1024);
    }
    Ok(Preview { image, info })
}

/// Playback tuning an owner can change while its provider stays registered:
/// each session reads it when it opens, so a changed preference applies from
/// the next track and never stops the one playing.
pub type SharedTuning = std::sync::Arc<parking_lot::Mutex<crate::media::Tuning>>;

/// The ids of the audio and the video provider (`PreviewProvider::id`).
pub const AUDIO_PROVIDER: &str = "media.audio";
pub const VIDEO_PROVIDER: &str = "media.video";

pub struct MediaPreviews {
    audio: bool,
    backend: String,
    tuning: SharedTuning,
    cache: CacheSlot,
    gate: Mutex<()>,
}
impl MediaPreviews {
    pub fn audio(mut self) -> Self {
        self.audio = true;
        self
    }
    pub fn with_backend(mut self, backend: String) -> Self {
        self.backend = backend;
        self
    }
    pub fn with_tuning(mut self, tuning: SharedTuning) -> Self {
        self.tuning = tuning;
        self
    }
    pub fn new(cache: CacheSlot) -> Self {
        Self {
            cache,
            backend: "auto".into(),
            tuning: SharedTuning::default(),
            audio: false,
            gate: Mutex::new(()),
        }
    }
}

impl PreviewProvider for MediaPreviews {
    fn open_media(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
        wake: skwir_core::location::Waker,
    ) -> anyhow::Result<Option<std::sync::Arc<dyn skwir_core::media::MediaSession>>> {
        if self.backend == "stills" {
            return Ok(None);
        }
        let session = crate::media::WorkerSession::open_tuned(
            req.entry.resolve()?,
            *self.tuning.lock(),
            lifetime,
            wake,
        )?;
        if session.is_none() && self.backend != "auto" {
            // In the words of the Video page: audio has no still to fall
            // back to.
            let instead = if self.audio {
                ""
            } else {
                ", or choose Still frames as the Video engine"
            };
            anyhow::bail!(
                "Install {} and GStreamer's playback plugins to play this{instead}",
                crate::media::WORKER.executable
            );
        }
        Ok(session)
    }
    fn media(&self) -> bool {
        true
    }
    fn id(&self) -> &'static str {
        if self.audio {
            AUDIO_PROVIDER
        } else {
            VIDEO_PROVIDER
        }
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        let playlist = playlist(&req.entry.path, req.mime.as_deref()).is_some();
        (!playlist
            && req
                .mime
                .as_deref()
                .is_some_and(|m| m.starts_with(if self.audio { "audio/" } else { "video/" })))
            || (!self.audio && ASF.holds(&req.entry.path, req.mime.as_deref()))
    }
    fn render(&self, req: &PreviewRequest, cancel: &CancelToken) -> anyhow::Result<Preview> {
        let _guard = acquire(&self.gate, cancel)?;
        let path = req.entry.resolve()?;
        let base = fingerprint(req, &path)?;
        let ffmpeg = process::required("ffmpeg")?;
        let ffprobe = process::required("ffprobe")?;
        let key = key(
            &base,
            (
                tool_stamp(&ffmpeg)?,
                tool_stamp(&ffprobe)?,
                req.seconds,
                req.max_side,
                req.fit,
                "video-v5",
            ),
        )?;
        if let Some(preview) = cached(&self.cache, &key, req.max_side) {
            return Ok(preview);
        }
        let tmp = tempfile::tempdir()?;
        let mut probe = sandbox::untrusted(&ffprobe, &path, tmp.path(), Otherwise::Run)?;
        probe
            .command
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "format=duration:stream=width,height,sample_aspect_ratio:stream_side_data=rotation",
                "-of",
                "json",
            ])
            .arg(&probe.input);
        let probe = process::run(&mut probe.command, cancel, RENDER_TIMEOUT)?;
        let value: serde_json::Value = serde_json::from_slice(&probe)?;
        let duration = value["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|d| d.is_finite() && *d > 0.0);
        // The picture is asked at the size it is shown at; the film's own
        // size is what it is laid out by and can be had at.
        let stream = &value["streams"][0];
        let stored = stream["width"]
            .as_u64()
            .zip(stream["height"].as_u64())
            .and_then(|(w, h)| Some((u32::try_from(w).ok()?, u32::try_from(h).ok()?)))
            .filter(|&(w, h)| w > 0 && h > 0)
            .map(|size| square_pixels(size, stream["sample_aspect_ratio"].as_str()));
        // A phone's film is stored the other way and turned when decoded.
        let turned = stream["side_data_list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|data| data["rotation"].as_f64())
            .any(|degrees| (degrees / 90.).round() as i64 % 2 != 0);
        let shown = stored.map(|(w, h)| if turned { (h, w) } else { (w, h) });
        let asked = req.seconds.filter(|t| t.is_finite());
        let seconds = if self.audio {
            0.
        } else {
            asked
                .unwrap_or_else(|| duration.map_or(0.0, |d| (d * 0.1).min(30.0)))
                .max(0.0)
                .min(duration.map_or(f64::MAX, |d| (d - 0.1).max(0.0)))
        };
        let side = fitted_side(shown, req.fit, req.max_side).clamp(64, MAX_VIDEO_SIDE);
        let exact = |seconds| ffmpeg_picture(&ffmpeg, &path, seconds, side, tmp.path(), cancel);
        // A film's own moment, unasked, is the first keyframe from it on:
        // keyframes alone decoded, not every frame since the last one. A
        // moment asked for (a step) is that one.
        let (picture, seconds) = if self.audio {
            (exact(None)?, seconds)
        } else if asked.is_some() {
            (exact(Some(seconds))?, seconds)
        } else {
            match ffmpeg_keyframe(&ffmpeg, &path, seconds, side, tmp.path(), cancel)? {
                Some(found) => found,
                None => (exact(Some(seconds))?, seconds),
            }
        };
        anyhow::ensure!(
            fingerprint(req, &path)? == base,
            "file changed during preview"
        );
        let image = decode_preview(&picture, req.max_side)?;
        // Stored as it is decoded; turned, a phone's film is the other way.
        let own = stored.map(|(w, h)| {
            if (w >= h) == (image.width >= image.height) {
                (w, h)
            } else {
                (h, w)
            }
        });
        finish(
            &self.cache,
            &key,
            image,
            &picture,
            PreviewInfo::Video {
                seconds,
                duration,
                size: own,
            },
            cancel,
        )
    }
}

/// Told which entry an Office document was just converted for.
pub type Converted = std::sync::Arc<dyn Fn(skwir_core::FileId) + Send + Sync>;

pub struct DocumentPreviews {
    cache: CacheSlot,
    // One active document's metadata, guarded by the same lock as rendering.
    // Reuse it across page turns, and discard it on source/tool changes.
    gate: Mutex<Option<(Vec<u8>, u32)>>,
    backend: String,
    library: Option<PathBuf>,
    converted: Option<Converted>,
}
impl DocumentPreviews {
    pub fn new(cache: CacheSlot) -> Self {
        Self {
            cache,
            gate: Mutex::new(None),
            backend: "auto".into(),
            library: None,
            converted: None,
        }
    }

    /// Called when an Office document has been converted to PDF: its
    /// thumbnail (`OfficePages`) can now be drawn from the conversion.
    pub fn on_converted(mut self, converted: Converted) -> Self {
        self.converted = Some(converted);
        self
    }

    pub fn with_reader(mut self, backend: String, library: Option<PathBuf>) -> Self {
        self.backend = backend;
        self.library = library;
        self
    }

    fn office_pdf(
        &self,
        req: &PreviewRequest,
        source: &Path,
        office: &Path,
        scratch: &Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<PathBuf> {
        let base = fingerprint(req, source)?;
        let conversion_key = conversion_key(&base, office)?;
        let pdf = scratch.join("document.pdf");
        if let Some(bytes) = self
            .cache
            .get(&conversion_key)
            .filter(|b| b.starts_with(b"%PDF-"))
        {
            std::fs::write(&pdf, bytes)?;
        } else {
            convert_office(office, source, scratch, "pdf", cancel)?;
            let bytes = read_artifact(&pdf).context(
                "LibreOffice did not produce a PDF (the file may be encrypted or unsupported)",
            )?;
            anyhow::ensure!(
                bytes.starts_with(b"%PDF-"),
                "conversion did not produce a PDF"
            );
            check(cancel)?;
            anyhow::ensure!(
                fingerprint(req, source)? == base,
                "file changed during conversion"
            );
            if self.cache.put(&conversion_key, &bytes).is_ok() {
                self.cache.trim_soon(bytes.len() as u64, 128 * 1024 * 1024);
                if let Some(converted) = &self.converted {
                    converted(req.entry.id);
                }
            }
        }
        Ok(pdf)
    }
}

/// Where the PDF an Office document converts to is cached: by the file's
/// version and `LibreOffice`'s, so either changing converts again.
fn conversion_key(base: &[u8], office: &Path) -> anyhow::Result<Vec<u8>> {
    key(base, ("pdf", Some(tool_stamp(office)?)))
}

/// The PDF the document preview converted `source` (entry `id`) to, written
/// to `into`, if it is in `cache` for the file as it is now; `None` without
/// `LibreOffice` or a conversion. Never converts: a thumbnail must not start
/// an office suite for every document in view.
pub(crate) fn converted_pdf(
    cache: &CacheSlot,
    id: skwir_core::FileId,
    source: &Path,
    into: &Path,
) -> anyhow::Result<Option<PathBuf>> {
    use std::io::{Read as _, Seek as _};
    let Ok(office) = process::required("libreoffice") else {
        return Ok(None);
    };
    let key = conversion_key(&fingerprint_of(id, source)?, &office)?;
    let Some(mut kept) = cache.open(&key) else {
        return Ok(None);
    };
    let mut magic = [0u8; 5];
    if kept.read_exact(&mut magic).is_err() || &magic != b"%PDF-" {
        return Ok(None);
    }
    kept.rewind()?;
    // Copied file to file, which the kernel does: read whole, a conversion
    // of up to 64 MB was held in memory for every thumbnail.
    let pdf = into.join("document.pdf");
    std::io::copy(&mut kept, &mut std::fs::File::create(&pdf)?)?;
    Ok(Some(pdf))
}

/// Windows Media's container, a film whatever its tracks: its type is no
/// `video/` one (`video/x-ms-asf` is another name for it, and some
/// databases call it `application/x-ms-asf`).
static ASF: crate::mime::Types =
    crate::mime::Types::new(&["application/vnd.ms-asf", "application/x-ms-asf"]);

/// What a list of recordings is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaylistFormat {
    M3u,
    Pls,
    Xspf,
    Cue,
    Asx,
}

/// Lists of recordings, by type: their types say `audio/` or `video/`
/// (an `.m3u`, a `.pls`), yet they are no recording, so the player never
/// takes one; the playlist preview reads those it parses.
const PLAYLISTS: [(&str, PlaylistFormat); 7] = [
    ("audio/x-mpegurl", PlaylistFormat::M3u),
    ("application/vnd.apple.mpegurl", PlaylistFormat::M3u),
    ("video/vnd.mpegurl", PlaylistFormat::M3u),
    ("audio/x-scpls", PlaylistFormat::Pls),
    ("application/xspf+xml", PlaylistFormat::Xspf),
    ("application/x-cue", PlaylistFormat::Cue),
    ("audio/x-ms-asx", PlaylistFormat::Asx),
];

/// The list of recordings a file of type `mime` (else named `path`) is,
/// if it is one. No I/O once the MIME database is loaded.
pub fn playlist(path: &Path, mime: Option<&str>) -> Option<PlaylistFormat> {
    let mime = crate::mime::unalias(&crate::mime::of_request(path, mime)?);
    PLAYLISTS
        .iter()
        .find(|(name, _)| {
            crate::mime::unalias(&crate::mime::Mime((*name).to_owned()))
                .as_str()
                .eq_ignore_ascii_case(mime.as_str())
        })
        .map(|&(_, format)| format)
}

fn is_pdf(req: &PreviewRequest) -> bool {
    req.mime.as_deref() == Some("application/pdf") || extension(&req.entry.path) == "pdf"
}

/// Office documents, by type: what `LibreOffice` converts to PDF for a
/// preview, and an office package's stored picture is looked for in. Not
/// all it opens: its desktop entries also claim plain text, CSV and PDF,
/// which skwir reads itself.
static OFFICE: crate::mime::Types = crate::mime::Types::new(&[
    "application/msword",
    "application/msword-template",
    "application/rtf",
    "application/vnd.ms-word.document.macroEnabled.12",
    "application/vnd.ms-word.template.macroEnabled.12",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.text-template",
    "application/vnd.oasis.opendocument.text-flat-xml",
    "application/vnd.ms-excel",
    "application/vnd.ms-excel.sheet.macroEnabled.12",
    "application/vnd.ms-excel.sheet.binary.macroEnabled.12",
    "application/vnd.ms-excel.template.macroEnabled.12",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.template",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.spreadsheet-template",
    "application/vnd.oasis.opendocument.spreadsheet-flat-xml",
    "application/vnd.ms-powerpoint",
    "application/vnd.ms-powerpoint.presentation.macroEnabled.12",
    "application/vnd.ms-powerpoint.slideshow.macroEnabled.12",
    "application/vnd.ms-powerpoint.template.macroEnabled.12",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
    "application/vnd.openxmlformats-officedocument.presentationml.template",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.oasis.opendocument.presentation-template",
    "application/vnd.oasis.opendocument.presentation-flat-xml",
    "application/vnd.oasis.opendocument.graphics",
    "application/vnd.oasis.opendocument.graphics-template",
    "application/vnd.oasis.opendocument.graphics-flat-xml",
]);

/// Whether a file asked about (`mime::of_request`) is an office document.
/// The one list every reader of office files asks: the previews, the
/// thumbnails and the stored pictures cannot disagree. No I/O once the
/// MIME database is loaded.
pub fn is_office(path: &Path, mime: Option<&str>) -> bool {
    OFFICE.holds(path, mime)
}

/// Whether `path` is shown as `LibreOffice` prints it here: an office
/// type (`is_office`), `LibreOffice` installed and the converters' sandbox
/// there. Every reader that would show it otherwise asks this one
/// question, so two never both cede a file, nor both claim it. Looks on
/// `PATH`: call from a worker.
pub fn shown_as_pages(path: &Path, mime: Option<&str>) -> bool {
    is_office(path, mime) && sandbox::available() && process::required("libreoffice").is_ok()
}

impl PreviewProvider for DocumentPreviews {
    fn open_document(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<std::sync::Arc<dyn skwir_core::document::DocumentSession>>> {
        use crate::document::{Engine, WorkerSession};
        anyhow::ensure!(
            matches!(
                self.backend.as_str(),
                "auto" | "pdfium" | "poppler" | "mupdf"
            ),
            "unknown PDF backend; choose auto, pdfium, poppler or mupdf"
        );
        let engines: &[Engine] = match self.backend.as_str() {
            "pdfium" => &[Engine::Pdfium],
            "poppler" => &[Engine::Poppler],
            "mupdf" => &[Engine::Mupdf],
            _ => &crate::document::AUTO,
        };
        let source = req.entry.resolve()?;
        let base = fingerprint(req, &source)?;
        let available: Vec<Engine> = engines
            .iter()
            .copied()
            .filter(|engine| crate::document::worker_path(*engine).is_some())
            .collect();
        // An Office document converts once, however many engines `auto` tries
        // before one greets ready. Each session still owns its own scratch
        // directory, so the converted PDF is linked or copied into it. The
        // gate is held for the conversion only, as `render` holds it: never
        // through a worker's startup, which is bounded on its own.
        let converted = if is_pdf(req) || available.is_empty() {
            None
        } else {
            let office = process::required("libreoffice")?;
            let dir = tempfile::tempdir()?;
            let _gate = acquire(&self.gate, lifetime)?;
            let pdf = self.office_pdf(req, &source, &office, dir.path(), lifetime)?;
            Some((dir, pdf))
        };
        for engine in available {
            // Its pages pass through memory, as every other session's: a
            // page written to a disk's temporary folder was written out to
            // the disk each time (a converted document's PDF, a few
            // megabytes, is copied in with them).
            let scratch = crate::file::memory_dir("skwir-pages-")?;
            let pdf = match &converted {
                None => source.clone(),
                Some((_, converted)) => {
                    let pdf = scratch.path().join("document.pdf");
                    if std::fs::hard_link(converted, &pdf).is_err() {
                        std::fs::copy(converted, &pdf)?;
                    }
                    pdf
                }
            };
            anyhow::ensure!(
                fingerprint(req, &source)? == base,
                "file changed during document open"
            );
            let setup = crate::document::Setup {
                library: self.library.clone(),
                layout: None,
                first_page: false,
            };
            if let Some(session) = WorkerSession::open(
                engine,
                pdf,
                source.clone(),
                scratch,
                setup,
                lifetime.clone(),
            )? {
                return Ok(Some(session));
            }
        }
        // The engine chosen is missing, or would not start: what it needs,
        // in the words every worker's absence is said in. A library the
        // settings name that is not there is said so: installing would
        // not help.
        if let ([crate::document::Engine::Pdfium], Some(library)) = (engines, &self.library)
            && !library.is_file()
        {
            anyhow::bail!(
                "PDFium's library is not at “{}”, where the settings say it is \
                 (Settings › Reading › Documents › PDFium library)",
                library.display()
            );
        }
        if let [engine] = engines {
            anyhow::bail!(
                "Install {} and its library to read documents with {}",
                crate::document::worker(*engine).executable,
                engine.label()
            );
        }
        Ok(None)
    }

    fn id(&self) -> &'static str {
        "documents"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        is_pdf(req) || is_office(&req.entry.path, req.mime.as_deref())
    }
    fn render(&self, req: &PreviewRequest, cancel: &CancelToken) -> anyhow::Result<Preview> {
        let started = Instant::now();
        let mut document = acquire(&self.gate, cancel)?;
        let path = req.entry.resolve()?;
        let base = fingerprint(req, &path)?;
        let renderer = process::required("pdftoppm")?;
        let pdfinfo = process::required("pdfinfo")?;
        let office = if is_pdf(req) {
            None
        } else {
            Some(process::required("libreoffice")?)
        };
        let office_stamp = office.as_deref().map(tool_stamp).transpose()?;
        let key = key(
            &base,
            (
                tool_stamp(&renderer)?,
                tool_stamp(&pdfinfo)?,
                &office_stamp,
                req.page,
                req.max_side,
            ),
        )?;
        if let Some(preview) = cached(&self.cache, &key, req.max_side) {
            tracing::debug!(
                page = req.page,
                cache_hit = true,
                ms = started.elapsed().as_millis(),
                "document preview"
            );
            return Ok(preview);
        }
        let tmp = tempfile::tempdir()?;
        let pdf = if let Some(office) = office {
            self.office_pdf(req, &path, &office, tmp.path(), cancel)?
        } else {
            path.clone()
        };
        let metadata_key = self::key(&base, ("pages", tool_stamp(&pdfinfo)?, &office_stamp))?;
        let pages = match document.as_ref() {
            Some((key, pages)) if key == &metadata_key => *pages,
            _ => {
                let mut info = sandbox::untrusted(&pdfinfo, &pdf, tmp.path(), Otherwise::Run)?;
                info.command.env("LC_ALL", "C").arg(&info.input);
                let info = process::run(&mut info.command, cancel, RENDER_TIMEOUT)?;
                let pages = String::from_utf8_lossy(&info)
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("Pages:")
                            .and_then(|n| n.trim().parse::<u32>().ok())
                    })
                    .filter(|n| *n > 0)
                    .context("PDF has no readable pages")?;
                *document = Some((metadata_key, pages));
                pages
            }
        };
        let page = req.page.clamp(1, pages);
        let rendering = Instant::now();
        let image = render_pdf_page(&renderer, &pdf, page, req.max_side, tmp.path(), cancel)?;
        let render_ms = rendering.elapsed().as_millis();
        anyhow::ensure!(
            fingerprint(req, &path)? == base,
            "file changed during preview"
        );
        check(cancel)?;
        let png = encode_preview(&image)?;
        let preview = finish(
            &self.cache,
            &key,
            image,
            &png,
            PreviewInfo::Document { page, pages },
            cancel,
        )?;
        tracing::debug!(
            page,
            cache_hit = false,
            render_ms,
            ms = started.elapsed().as_millis(),
            "document preview"
        );
        Ok(preview)
    }
}

/// The longest side a picture of `own` pixels (as shown, turned) is asked
/// at to fit `fit`, the box it is shown in, as `Contain` lays it out:
/// as wide as a tall box for a landscape film. Within `max_side`, which is
/// all there is without either size.
fn fitted_side(own: Option<(u32, u32)>, fit: Option<(u32, u32)>, max_side: u32) -> u32 {
    let (Some((width, height)), Some((across, down))) = (own, fit) else {
        return max_side;
    };
    if width == 0 || height == 0 || across == 0 || down == 0 {
        return max_side;
    }
    let scale = (f64::from(across) / f64::from(width)).min(f64::from(down) / f64::from(height));
    ((f64::from(width.max(height)) * scale).ceil() as u32).min(max_side)
}

/// One picture of `path` made by `ffmpeg`, through `scratch`: the frame at
/// `seconds` of a film, as JPEG, or with `None` the first video stream of a
/// recording, its cover, as PNG. Its longest side is at most `side`, never
/// enlarged. Only local files are read: thumbnails are made for whatever a
/// folder holds, and a playlist must not send `ffmpeg` to the network.
/// Recent `ffmpeg` refuses already (a playlist is recognised only by its
/// extension, and what it names only through `file,crypto,data`); the
/// whitelist says so for any version.
pub(crate) fn ffmpeg_picture(
    ffmpeg: &Path,
    path: &Path,
    seconds: Option<f64>,
    side: u32,
    scratch: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<u8>> {
    let frame = seconds.map_or(Frame::First, Frame::At);
    read_artifact(&ffmpeg_frame(ffmpeg, path, frame, side, scratch, cancel)?)
}

/// The first keyframe of `path` from `seconds` on, made as `ffmpeg_picture`
/// makes a frame, and its time. Not the one before: in a short clip with
/// keyframes far apart it was the first frame, black where a film fades in.
/// `None` when there is none from there to the end.
fn ffmpeg_keyframe(
    ffmpeg: &Path,
    path: &Path,
    seconds: f64,
    side: u32,
    scratch: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Option<(Vec<u8>, f64)>> {
    // With no frame to write, `ffmpeg` fails ("Could not open encoder
    // before EOF"): what it printed says whether one came. Any other
    // failure is met again by the exact frame read instead.
    let picture = ffmpeg_frame(
        ffmpeg,
        path,
        Frame::Keyframe(seconds),
        side,
        scratch,
        cancel,
    );
    // `frame:0    pts:128000  pts_time:8.333333`, empty when no frame was
    // decoded.
    let printed = read_artifact(&scratch.join("frame.txt")).unwrap_or_default();
    let Some(time) = String::from_utf8_lossy(&printed)
        .lines()
        .next()
        .and_then(|line| {
            line.split_whitespace()
                .find_map(|word| word.strip_prefix("pts_time:"))
        })
        .and_then(|time| time.parse::<f64>().ok())
        .filter(|time| time.is_finite())
    else {
        return Ok(None);
    };
    Ok(Some((read_artifact(&picture?)?, time.max(0.0))))
}

/// Which frame `ffmpeg_frame` asks `ffmpeg` for.
#[derive(Clone, Copy)]
enum Frame {
    /// The first of the first video stream: a recording's cover.
    First,
    /// The one shown at this time, decoded from the keyframe before it.
    At(f64),
    /// The first keyframe from this time on, only keyframes decoded. Its
    /// time from the file's start is printed beside it (`frame.txt`).
    Keyframe(f64),
}

/// A film's size as it is shown, its pixels made square: its stored size
/// stretched by its `sample_aspect_ratio` (`64:45`, a widescreen DVD's), as
/// ffprobe says it. One unknown (`0:1`, `N/A`) is square.
fn square_pixels((width, height): (u32, u32), ratio: Option<&str>) -> (u32, u32) {
    let ratio = ratio
        .and_then(|ratio| ratio.split_once(':'))
        .and_then(|(num, den)| Some((num.parse::<u64>().ok()?, den.parse::<u64>().ok()?)))
        .filter(|&(num, den)| num > 0 && den > 0);
    let Some((num, den)) = ratio else {
        return (width, height);
    };
    let shown = (u64::from(width) * num + den / 2) / den;
    (u32::try_from(shown).unwrap_or(u32::MAX).max(1), height)
}

/// `frame` of `path`, written in `scratch` (`ffmpeg_picture`): where.
fn ffmpeg_frame(
    ffmpeg: &Path,
    path: &Path,
    frame: Frame,
    side: u32,
    scratch: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<std::path::PathBuf> {
    let mut run = sandbox::untrusted(ffmpeg, path, scratch, Otherwise::Run)?;
    let command = &mut run.command;
    command.args(["-nostdin", "-y", "-v", "error", "-threads", "1"]);
    let mut filters = String::new();
    match frame {
        Frame::First => {}
        Frame::At(seconds) => {
            command.arg("-ss").arg(format!("{seconds:.3}"));
        }
        Frame::Keyframe(seconds) => {
            // Only keyframes are decoded, from the one before `seconds`;
            // those before it are dropped. Timestamps stay the file's,
            // from its start, so the time printed is the frame's own.
            command
                .args(["-copyts", "-start_at_zero", "-skip_frame", "nokey", "-ss"])
                .arg(format!("{seconds:.3}"));
            // The metadata filter prints only a frame that carries some.
            // The file is named from the working folder, `work`.
            filters.push_str(
                "metadata=mode=add:key=skwir:value=1,metadata=mode=print:file=frame.txt,",
            );
        }
    }
    command
        .args(["-protocol_whitelist", "file", "-i"])
        .arg(&run.input)
        .args([
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-an",
            "-sn",
            "-filter_threads",
            "1",
            "-vf",
        ])
        // Its pixels made square first, as the player shows them (a
        // widescreen DVD's 720 × 576 is 1024 × 576), then within `side`,
        // never enlarged: the poster is the film's shape.
        .arg(format!(
            "{filters}scale=w='max(1,round(iw*sar*min(1,{side}/max(iw*sar,ih))))':h='max(1,round(ih*min(1,{side}/max(iw*sar,ih))))',setsar=1"
        ))
        .args(["-threads", "1"]);
    // A film's frame, already a lossy decode, is kept as JPEG at high
    // quality: a quarter of the PNG at the same size, and quicker to make
    // and read (`docs/invariants/media.md`, Posters). A cover may have
    // transparency.
    let name = if matches!(frame, Frame::First) {
        command.args(["-compression_level", "1"]);
        "frame.png"
    } else {
        command.args(["-q:v", "2"]);
        "frame.jpg"
    };
    command.arg(run.work.join(name));
    process::run(command, cancel, RENDER_TIMEOUT)?;
    Ok(scratch.join(name))
}

/// One page of `pdf` rendered by Poppler's `pdftoppm` (`renderer`), its
/// longest side `side` pixels, through `scratch`: the document preview's
/// pages and the grid's thumbnails alike. PPM avoids Poppler's expensive PNG
/// compression; the preview encodes it again with the fast cache encoder.
pub(crate) fn render_pdf_page(
    renderer: &Path,
    pdf: &Path,
    page: u32,
    side: u32,
    scratch: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Image> {
    let side = side.clamp(64, MAX_SIDE);
    let mut run = sandbox::untrusted(renderer, pdf, scratch, Otherwise::Run)?;
    run.command
        .args([
            "-f",
            &page.to_string(),
            "-l",
            &page.to_string(),
            "-singlefile",
            "-scale-to",
            &side.to_string(),
        ])
        .arg(&run.input)
        .arg(run.work.join("page"));
    process::run(&mut run.command, cancel, RENDER_TIMEOUT)?;
    check(cancel)?;
    decode_preview(&read_artifact(&scratch.join("page.ppm"))?, side)
}

/// Conversion is isolated from the user's `LibreOffice` session and files
/// (`sandbox`): a fresh profile in the working directory, macros and link
/// updates off, no display.
fn convert_office(
    office: &Path,
    input: &Path,
    work: &Path,
    output_format: &str,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let profile = work.join("profile/user");
    std::fs::create_dir_all(&profile)?;
    std::fs::write(
        profile.join("registrymodifications.xcu"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<oor:items xmlns:oor="http://openoffice.org/2001/registry">
<item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop><prop oor:name="DisableMacrosExecution" oor:op="fuse"><value>true</value></prop></item>
<item oor:path="/org.openoffice.Office.Writer/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>2</value></prop></item>
<item oor:path="/org.openoffice.Office.Calc/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>2</value></prop></item>
</oor:items>"#,
    )?;
    let mut run = crate::sandbox::command(office, input, work)?;
    // The sandbox passes its environment on: no display, `LibreOffice`'s
    // headless backend (Linux's; elsewhere `--headless` alone).
    if cfg!(target_os = "linux") {
        run.command.env("SAL_USE_VCLPLUGIN", "svp");
    }
    run.command
        .arg(format!(
            "-env:UserInstallation={}",
            crate::uri::file_uri(&run.work.join("profile"))
        ))
        .args([
            "--headless",
            "--nologo",
            "--nodefault",
            "--norestore",
            "--convert-to",
            output_format,
            "--outdir",
        ])
        .arg(&run.work)
        .arg(&run.input);
    process::run(&mut run.command, cancel, CONVERT_TIMEOUT)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::{cache::Cache, op::EntryRef};
    use std::io::Write;
    use std::process::Command;

    fn request(path: &Path) -> PreviewRequest {
        PreviewRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: crate::mime::by_name(path.as_os_str()).map(|m| m.to_string()),
            page: 1,
            seconds: None,
            max_side: 256,
            text_size: None,
            fit: None,
        }
    }

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn asf_containers_are_media_even_with_application_mime() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::new(tmp.path().join("cache"));
        let video = MediaPreviews::new(cache.namespace("video"));
        let audio = MediaPreviews::new(cache.namespace("audio")).audio();
        let path = tmp.path().join("Synthetic clip.ASF");
        std::fs::write(&path, b"synthetic container marker").unwrap();
        let mut req = request(&path);
        // Its type by any of its names, or, untyped, its name's.
        for mime in [
            Some("application/vnd.ms-asf"),
            Some("application/x-ms-asf"),
            Some("video/x-ms-asf"),
            None,
        ] {
            req.mime = mime.map(str::to_owned);
            assert!(video.accepts(&req));
            assert!(!audio.accepts(&req));
        }
        // Bytes that said nothing are not a film by their name.
        req.mime = Some("application/octet-stream".into());
        assert!(!video.accepts(&req));
        req.entry.path = tmp.path().join("clip.data");
        req.mime = Some("application/vnd.ms-asf".into());
        assert!(video.accepts(&req));
    }

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn office_documents_are_known_by_type() {
        let office = |name: &str| super::is_office(Path::new(name), None);
        for name in [
            "a.doc", "a.docx", "a.rtf", "a.fodt", "a.xlsb", "a.ods", "a.ppsx", "a.odg",
        ] {
            assert!(office(name), "{name} is an office document");
        }
        // `LibreOffice` opens these too; skwir reads them itself.
        for name in ["a.txt", "a.csv", "a.pdf", "a.epub", "a.png"] {
            assert!(!office(name), "{name} is no office document");
        }
        // The type a request was asked with, not its name.
        assert!(!super::is_office(Path::new("a.docx"), Some("text/plain")));
    }

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn pdf_pages_is_a_fallback_for_pdfs_only() {
        use crate::thumb_providers::PdfPages;
        use skwir_core::thumbs::{Stage, ThumbRequest, ThumbSize, ThumbnailProvider};
        let tmp = tempfile::tempdir().unwrap();
        let req = |name: &str, mime: Option<&str>| {
            let path = tmp.path().join(name);
            std::fs::write(&path, b"").unwrap();
            ThumbRequest {
                entry: EntryRef::from_path(&path).unwrap(),
                mime: mime.map(Into::into),
                mtime: None,
                size: ThumbSize::Normal,
            }
        };
        assert_eq!(PdfPages::default().stage(), Stage::Fallback);
        assert!(PdfPages::default().accepts(&req("Report \u{2019}24.PDF", None)));
        assert!(PdfPages::default().accepts(&req("scan", Some("application/pdf"))));
        assert!(!PdfPages::default().accepts(&req("notes.txt", None)));
        assert!(!PdfPages::default().accepts(&req("book.epub", None)));
    }

    /// A PDF's thumbnail drawn by the reader's engine, `PDFium`, with no
    /// Poppler tool to be found (macOS, Windows, a Linux without them).
    /// Needs the helper built in the target folder the test runs from and
    /// a `PDFium` (the `pdf` dev shell names one); without them the test
    /// says so and passes.
    #[test]
    fn pdf_thumbnails_are_drawn_by_the_readers_engine() {
        use crate::thumb_providers::PdfPages;
        use skwir_core::thumbs::{ThumbRequest, ThumbSize, ThumbnailProvider};
        let helper = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("skwir-pdf-worker")))
            .filter(|path| path.is_file());
        let (Some(helper), Some(_)) = (helper, std::env::var_os("SKWIR_PDFIUM_LIBRARY")) else {
            eprintln!("skipped: needs skwir-pdf-worker built and a PDFium library");
            return;
        };
        // SAFETY: nextest runs each test in a process of its own, and
        // nothing else reads the environment before this.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("SKWIR_PDF_WORKER", &helper);
            // No `pdftoppm` to fall back on.
            std::env::set_var("PATH", "");
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic \u{201c}report\u{201d}.pdf");
        pdf(&path);
        let req = ThumbRequest {
            entry: EntryRef::from_path(&path).unwrap(),
            mime: None,
            mtime: None,
            size: ThumbSize::Normal,
        };
        let img = PdfPages::default()
            .thumbnail(&req, &CancelToken::new())
            .unwrap()
            .expect("PDFium draws it");
        assert_eq!(
            img.width,
            ThumbSize::Normal.pixels(),
            "the page is wider than tall"
        );
        let middle = ((img.height / 2 * img.width + img.width / 2) * 4) as usize;
        assert_eq!(
            &img.rgba[middle..middle + 4],
            &[255, 0, 0, 255],
            "page 1 is red"
        );
    }

    #[test]
    #[ignore = "requires installed Poppler tools; run explicitly for preview changes"]
    fn pdf_thumbnails_are_the_first_page_at_the_size_asked() {
        use crate::thumb_providers::PdfPages;
        use skwir_core::thumbs::{ThumbRequest, ThumbSize, ThumbnailProvider};
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp
            .path()
            .join("Synthetic \u{201c}report\u{201d} n\u{b0} 2.pdf");
        pdf(&path);
        for size in [ThumbSize::Normal, ThumbSize::Large] {
            let req = ThumbRequest {
                entry: EntryRef::from_path(&path).unwrap(),
                mime: None,
                mtime: None,
                size,
            };
            let img = PdfPages::default()
                .thumbnail(&req, &CancelToken::new())
                .unwrap()
                .expect("a thumbnail");
            assert_eq!(img.width, size.pixels(), "the page is wider than tall");
            assert!(img.height < img.width);
            let middle = ((img.height / 2 * img.width + img.width / 2) * 4) as usize;
            assert_eq!(
                &img.rgba[middle..middle + 4],
                &[255, 0, 0, 255],
                "page 1 is red"
            );
        }
        let broken = tmp.path().join("broken.pdf");
        std::fs::write(&broken, b"%PDF-1.4 truncated").unwrap();
        let req = ThumbRequest {
            entry: EntryRef::from_path(&broken).unwrap(),
            mime: None,
            mtime: None,
            size: ThumbSize::Normal,
        };
        assert!(
            PdfPages::default()
                .thumbnail(&req, &CancelToken::new())
                .is_err()
        );
    }

    #[test]
    #[ignore = "requires installed FFmpeg; run explicitly for preview changes"]
    fn audio_covers_are_the_attached_picture_at_its_size() {
        use crate::thumb_providers::AudioCovers;
        use skwir_core::thumbs::{ThumbRequest, ThumbSize, ThumbnailProvider};
        let tmp = tempfile::tempdir().unwrap();
        let ffmpeg = |args: &[&str], out: &Path| {
            let status = Command::new(process::required("ffmpeg").unwrap())
                .args(["-nostdin", "-v", "error", "-y"])
                .args(args)
                .arg(out)
                .status()
                .unwrap();
            assert!(status.success());
        };
        let tone = ["-f", "lavfi", "-i", "sine=frequency=440:duration=0.2"];
        let request = |path: &Path, size| ThumbRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: crate::mime::by_name(path.as_os_str()).map(|m| m.0.into()),
            mtime: None,
            size,
        };
        for (name, codec) in [
            ("Synthetic \u{2019}track\u{2019}.flac", "flac"),
            ("t.mp3", "libmp3lame"),
        ] {
            let path = tmp.path().join(name);
            ffmpeg(
                &[
                    &tone[..],
                    &["-f", "lavfi", "-i", "color=c=0x00ff00:s=300x200:d=1"],
                    &[
                        "-map",
                        "0:a",
                        "-map",
                        "1:v",
                        "-frames:v",
                        "1",
                        "-c:a",
                        codec,
                    ],
                    &["-c:v", "png", "-disposition:v", "attached_pic"],
                ]
                .concat(),
                &path,
            );
            for (size, want) in [
                (ThumbSize::Normal, (128, 85)),
                (ThumbSize::Large, (256, 171)),
            ] {
                let req = request(&path, size);
                assert!(AudioCovers.accepts(&req), "{name}");
                let cover = AudioCovers
                    .thumbnail(&req, &CancelToken::new())
                    .unwrap()
                    .expect("a cover");
                assert_eq!((cover.width, cover.height), want, "{name}: aspect kept");
                // Through YUV and back: green, give or take rounding.
                let [r, g, b, _] = cover.rgba[..4] else {
                    unreachable!()
                };
                assert!(r < 8 && g > 247 && b < 8, "{name}: {:?}", &cover.rgba[..4]);
            }
        }
        let bare = tmp.path().join("bare.flac");
        ffmpeg(&tone, &bare);
        let req = request(&bare, ThumbSize::Normal);
        assert!(AudioCovers.thumbnail(&req, &CancelToken::new()).is_err());
        // Playlists are `audio/` too, and never handed to ffmpeg.
        for name in ["list.m3u", "list.m3u8", "list.pls"] {
            let path = tmp.path().join(name);
            std::fs::write(&path, "#EXTM3U\n").unwrap();
            assert!(
                !AudioCovers.accepts(&request(&path, ThumbSize::Normal)),
                "{name}"
            );
        }
    }

    /// A valid, deterministic two-page PDF with visibly different pages.
    fn pdf(path: &Path) {
        let mut objects: Vec<String> = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 5 0 R >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 6 0 R >>",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for color in ["1 0 0", "0 0 1"] {
            let stream = format!("{color} rg 0 0 300 200 re f\n");
            objects.push(format!(
                "<< /Length {} >>\nstream\n{stream}endstream",
                stream.len()
            ));
        }
        write_pdf(
            path,
            &objects
                .into_iter()
                .map(String::into_bytes)
                .collect::<Vec<_>>(),
        );
    }

    fn write_pdf(path: &Path, objects: &[Vec<u8>]) {
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            writeln!(bytes, "{} 0 obj", i + 1).unwrap();
            bytes.extend(object);
            bytes.extend(b"\nendobj\n");
        }
        let xref = bytes.len();
        writeln!(bytes, "xref\n0 {}\n0000000000 65535 f ", offsets.len()).unwrap();
        for offset in &offsets[1..] {
            writeln!(bytes, "{offset:010} 00000 n ").unwrap();
        }
        writeln!(
            bytes,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF",
            offsets.len()
        )
        .unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// Paper noise and dark text-like strokes expose expensive PNG compression
    /// that the tiny, solid-color page fixture cannot exercise.
    fn scanned_pdf(path: &Path) {
        let (width, height) = (1600, 2200);
        let mut state = 1u32;
        let mut pixels = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let ink = (100..1500).contains(&x)
                    && (100..2100).contains(&y)
                    && y % 32 < 5
                    && x % 19 < 13;
                pixels.push((if ink { 35 } else { 230 }) + ((state >> 24) % 25) as u8);
            }
        }
        let mut raster = format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
            pixels.len()
        ).into_bytes();
        raster.extend(pixels);
        raster.extend(b"\nendstream");
        let content = b"q 600 0 0 825 0 0 cm /Im0 Do Q\n";
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend(content);
        stream.extend(b"endstream");
        let page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 825] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 6 0 R >>";
        write_pdf(
            path,
            &[
                b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
                b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
                page.to_vec(),
                page.to_vec(),
                raster,
                stream,
            ],
        );
    }

    #[test]
    fn malformed_cache_and_oversized_images_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::new(tmp.path()).namespace("preview");
        cache.put(b"bad", &[255; 8]).unwrap();
        assert!(cached(&cache, b"bad", 256).is_none());
        let mut png = Cursor::new(Vec::new());
        image::RgbaImage::new(MAX_SIDE + 1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        assert!(decode_preview(png.get_ref(), 256).is_err());
    }

    #[test]
    #[ignore = "requires Poppler; run in release mode with --nocapture for timings"]
    fn real_scanned_pdf_fast_path_preserves_pixels() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic scanned pages.pdf");
        scanned_pdf(&path);
        let cancel = CancelToken::new();
        let prefix = tmp.path().join("legacy");
        let started = Instant::now();
        process::run(
            Command::new(process::required("pdftoppm").unwrap())
                .args([
                    "-f",
                    "2",
                    "-l",
                    "2",
                    "-singlefile",
                    "-scale-to",
                    "2048",
                    "-png",
                ])
                .arg(&path)
                .arg(&prefix),
            &cancel,
            RENDER_TIMEOUT,
        )
        .unwrap();
        let old = decode_preview(
            &read_artifact(&prefix.with_extension("png")).unwrap(),
            MAX_SIDE,
        )
        .unwrap();
        let legacy_ms = started.elapsed().as_millis();
        let backend = DocumentPreviews::new(Cache::new(tmp.path()).namespace("preview"));
        let mut req = request(&path);
        req.page = 2;
        req.max_side = MAX_SIDE;
        let started = Instant::now();
        let page = backend.render(&req, &cancel).unwrap();
        let cold_ms = started.elapsed().as_millis();
        assert_eq!(old.rgba, page.image.rgba, "fast encoding must be lossless");
        assert_eq!(page.info, PreviewInfo::Document { page: 2, pages: 2 });
        let started = Instant::now();
        let cached = backend.render(&req, &cancel).unwrap();
        let cached_ms = started.elapsed().as_millis();
        assert_eq!(page.image.rgba, cached.image.rgba);
        eprintln!(
            "scanned PDF at 2048 px: Poppler PNG {legacy_ms} ms; fast path {cold_ms} ms; disk cache {cached_ms} ms"
        );
    }

    #[test]
    #[ignore = "requires the PDFium worker/library, Poppler and MuPDF; run in release mode for timings"]
    fn real_pdfium_and_poppler_backend_comparison() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic scanned reader.pdf");
        scanned_pdf(&path);
        let mut req = request(&path);
        req.max_side = MAX_SIDE;
        let cancel = CancelToken::new();
        for engine in ["pdfium", "poppler", "mupdf"] {
            let backend = DocumentPreviews::new(Cache::new(tmp.path()).namespace("preview"))
                .with_reader(engine.into(), None);
            let start = Instant::now();
            let session = backend.open_document(&req, &cancel).unwrap().unwrap();
            let open_ms = start.elapsed().as_secs_f64() * 1000.;
            assert_eq!(session.info().pages, 2);
            assert!(session.info().outlines.is_empty());
            let mut timings = Vec::new();
            for page in [1, 2, 1] {
                let start = Instant::now();
                let result = session.render(page, MAX_SIDE, &cancel).unwrap();
                timings.push(start.elapsed().as_secs_f64() * 1000.);
                assert_eq!(result.content.page, page);
                assert_eq!(result.height, MAX_SIDE);
                assert!(result.content.chars.is_empty());
            }
            assert!(session.search("absent", &cancel).unwrap().is_empty());
            eprintln!(
                "2048px scanned {engine}: open {open_ms:.2} ms; first/next/revisit {timings:.2?} ms"
            );
            // Source changes invalidate either engine, regardless of cached native handles.
            std::fs::write(&path, b"changed PDF").unwrap();
            assert!(session.render(1, MAX_SIDE, &cancel).is_err());
            drop(session);
            scanned_pdf(&path);
        }
        // Auto retains full reader features when PDFium's runtime is unavailable.
        let backend = DocumentPreviews::new(Cache::new(tmp.path()).namespace("preview"))
            .with_reader("auto".into(), Some(tmp.path().join("missing-library")));
        assert!(backend.open_document(&req, &cancel).unwrap().is_some());
    }

    #[test]
    #[ignore = "requires installed Poppler tools; run explicitly for preview changes"]
    fn real_pdf_pages_cache_and_file_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic d'Example (two pages).pdf");
        pdf(&path);
        let cache = Cache::new(tmp.path()).namespace("preview");
        let backend = DocumentPreviews::new(cache);
        let mut req = request(&path);
        let token = CancelToken::new();
        let first = backend.render(&req, &token).unwrap();
        assert_eq!(first.info, PreviewInfo::Document { page: 1, pages: 2 });
        assert_eq!(first.image.width, 256);
        req.page = 2;
        let second = backend.render(&req, &token).unwrap();
        assert_eq!(second.info, PreviewInfo::Document { page: 2, pages: 2 });
        assert_ne!(first.image.rgba, second.image.rgba);
        assert_eq!(
            backend.render(&req, &token).unwrap().image.rgba,
            second.image.rgba
        );
        req.page = u32::MAX;
        assert_eq!(backend.render(&req, &token).unwrap().info, second.info);
        // Changing the document must invalidate the remembered page count too.
        write_pdf(
            &path,
            &[
                b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>".to_vec(),
            ],
        );
        assert_eq!(
            backend.render(&req, &token).unwrap().info,
            PreviewInfo::Document { page: 1, pages: 1 }
        );
        std::fs::write(&path, b"broken PDF").unwrap();
        assert!(
            backend.render(&req, &token).is_err(),
            "must invalidate cached output"
        );
    }

    #[test]
    fn a_film_is_laid_out_with_square_pixels() {
        assert_eq!(square_pixels((720, 576), Some("64:45")), (1024, 576));
        assert_eq!(square_pixels((1440, 1080), Some("4:3")), (1920, 1080));
        for unknown in [None, Some("0:1"), Some("N/A"), Some("1:0"), Some("1:1")] {
            assert_eq!(square_pixels((720, 576), unknown), (720, 576));
        }
    }

    #[test]
    #[ignore = "requires installed FFmpeg and ffprobe; run explicitly for preview changes"]
    fn an_anamorphic_film_s_poster_has_its_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic d'Example DVD.mkv");
        let token = CancelToken::new();
        // A widescreen DVD's frames: 720 × 576, each pixel 64:45 wide.
        process::run(
            Command::new(process::required("ffmpeg").unwrap())
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=720x576:rate=4",
                    "-t",
                    "1",
                    "-vf",
                    "setsar=64/45",
                    "-c:v",
                    "ffv1",
                ])
                .arg(&path),
            &token,
            RENDER_TIMEOUT,
        )
        .unwrap();
        let backend = MediaPreviews::new(Cache::new(tmp.path()).namespace("preview"));
        let mut req = request(&path);
        req.mime = Some("video/x-matroska".into());
        req.seconds = Some(0.0);
        let poster = backend.render(&req, &token).unwrap();
        let PreviewInfo::Video { size, .. } = poster.info else {
            panic!("a film");
        };
        assert_eq!(size, Some((1024, 576)));
        let (width, height) = (poster.image.width, poster.image.height);
        assert!(
            (f64::from(width) / f64::from(height) - 16. / 9.).abs() < 0.01,
            "{width} × {height}"
        );
    }

    #[test]
    #[ignore = "requires installed FFmpeg and ffprobe; run explicitly for preview changes"]
    fn real_video_can_seek() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic d'Example clip.mkv");
        let token = CancelToken::new();
        process::run(
            Command::new(process::required("ffmpeg").unwrap())
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=160x90:rate=4",
                    "-t",
                    "3",
                    "-c:v",
                    "ffv1",
                ])
                .arg(&path),
            &token,
            RENDER_TIMEOUT,
        )
        .unwrap();
        let backend = MediaPreviews::new(Cache::new(tmp.path()).namespace("preview"));
        let mut req = request(&path);
        req.mime = Some("video/x-matroska".into());
        req.seconds = Some(0.0);
        let start = backend.render(&req, &token).unwrap();
        assert_eq!(
            (start.image.width, start.image.height),
            (160, 90),
            "do not upscale the source"
        );
        req.seconds = Some(2.0);
        let later = backend.render(&req, &token).unwrap();
        assert_ne!(start.image.rgba, later.image.rgba);
        assert!(
            matches!(later.info, PreviewInfo::Video { seconds, duration: Some(d), .. } if (seconds - 2.0).abs() < 0.001 && d >= 3.0)
        );
        assert_eq!(
            backend.render(&req, &token).unwrap().image.rgba,
            later.image.rgba
        );
    }

    #[test]
    #[ignore = "requires installed FFmpeg and ffprobe; run explicitly for preview changes"]
    fn a_film_s_own_moment_is_a_keyframe_shown_at_its_time() {
        let tmp = tempfile::tempdir().unwrap();
        let token = CancelToken::new();
        let ffmpeg = process::required("ffmpeg").unwrap();
        let film = tmp.path().join("Synthetic d'Example film.mkv");
        // The same frames where time does not start at 0 and the seek
        // has no index to land on.
        let stream = tmp.path().join("Synthetic d'Example film.ts");
        for path in [&film, &stream] {
            // 25 s, a keyframe every 2 s: its own moment, 10 %, is 2.5 s.
            process::run(
                Command::new(&ffmpeg)
                    .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i"])
                    .args(["testsrc2=size=160x90:rate=10", "-t", "25"])
                    .args(["-c:v", "mpeg4", "-g", "20"])
                    .arg(path),
                &token,
                RENDER_TIMEOUT,
            )
            .unwrap();
        }
        let backend = MediaPreviews::new(Cache::new(tmp.path()).namespace("preview"));
        for (path, mime) in [(&film, "video/x-matroska"), (&stream, "video/mp2t")] {
            let mut req = request(path);
            req.mime = Some(mime.into());
            let own = backend.render(&req, &token).unwrap();
            let PreviewInfo::Video { seconds, .. } = own.info else {
                panic!("not a film's picture: {:?}", own.info);
            };
            assert!(
                (seconds - 4.0).abs() < 0.001,
                "{}: the first keyframe from 2.5 s, said {seconds}",
                path.display()
            );
            req.seconds = Some(seconds);
            let there = backend.render(&req, &token).unwrap();
            assert_eq!(
                own.image.rgba,
                there.image.rgba,
                "{}: the frame at the time it says, {seconds}",
                path.display()
            );
        }
        // A clip whose one keyframe is its first: none from its tenth on,
        // so the frame there, decoded exactly.
        let clip = tmp.path().join("Synthetic one keyframe.mp4");
        process::run(
            Command::new(&ffmpeg)
                .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i"])
                .args(["testsrc2=size=160x90:rate=10", "-t", "3"])
                .args(["-c:v", "mpeg4", "-g", "100"])
                .arg(&clip),
            &token,
            RENDER_TIMEOUT,
        )
        .unwrap();
        let mut req = request(&clip);
        req.mime = Some("video/mp4".into());
        let shown = backend.render(&req, &token).unwrap();
        let PreviewInfo::Video { seconds, .. } = shown.info else {
            panic!("not a film's picture: {:?}", shown.info);
        };
        assert!((seconds - 0.3).abs() < 0.001, "its tenth, said {seconds}");
    }

    #[test]
    #[ignore = "requires installed FFmpeg and ffprobe"]
    fn real_video_poster_preserves_source_resolution() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic 4K poster.mkv");
        let token = CancelToken::new();
        process::run(
            Command::new(process::required("ffmpeg").unwrap())
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=blue:size=3840x2160:rate=1",
                    "-t",
                    "2",
                    "-c:v",
                    "ffv1",
                    "-threads",
                    "1",
                ])
                .arg(&path),
            &token,
            RENDER_TIMEOUT,
        )
        .unwrap();
        let backend = MediaPreviews::new(Cache::new(tmp.path()).namespace("preview"));
        let mut req = request(&path);
        req.mime = Some("video/x-matroska".into());
        req.max_side = u32::MAX;
        let poster = backend.render(&req, &token).unwrap();
        assert_eq!((poster.image.width, poster.image.height), (3840, 2160));
        assert_eq!(
            backend.render(&req, &token).unwrap().image.rgba,
            poster.image.rgba
        );
        // Asked at the size it is shown at, it says its own.
        req.max_side = 1024;
        let shown = backend.render(&req, &token).unwrap();
        assert_eq!((shown.image.width, shown.image.height), (1024, 576));
        assert!(
            matches!(
                shown.info,
                PreviewInfo::Video {
                    size: Some((3840, 2160)),
                    ..
                }
            ),
            "{:?}",
            shown.info
        );
        // In a tall box, a landscape film is as wide as the box.
        req.max_side = 1536;
        req.fit = Some((1024, 1536));
        let fitted = backend.render(&req, &token).unwrap();
        assert_eq!((fitted.image.width, fitted.image.height), (1024, 576));
        // A phone's film, stored the other way, fits as it is shown.
        let turned = tmp.path().join("Synthetic turned.mp4");
        for (args, out) in [
            (
                vec![
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=red:size=1920x1080:rate=1",
                    "-t",
                    "2",
                    "-c:v",
                    "mpeg4",
                ],
                tmp.path().join("plain.mp4"),
            ),
            (
                vec!["-display_rotation", "90", "-i", "plain.mp4", "-c", "copy"],
                turned.clone(),
            ),
        ] {
            process::run(
                Command::new(process::required("ffmpeg").unwrap())
                    .current_dir(tmp.path())
                    .args(["-nostdin", "-v", "error", "-threads", "1"])
                    .args(args)
                    .arg(&out),
                &token,
                RENDER_TIMEOUT,
            )
            .unwrap();
        }
        let mut phone = request(&turned);
        phone.mime = Some("video/mp4".into());
        phone.max_side = 1536;
        phone.fit = Some((1024, 1536));
        let upright = backend.render(&phone, &token).unwrap();
        assert_eq!((upright.image.width, upright.image.height), (864, 1536));
        assert!(
            matches!(
                upright.info,
                PreviewInfo::Video {
                    size: Some((1080, 1920)),
                    ..
                }
            ),
            "{:?}",
            upright.info
        );
        token.cancel();
        assert!(backend.render(&req, &token).is_err());
    }

    #[test]
    fn a_film_is_asked_at_the_side_it_covers_in_its_box() {
        let tall = Some((1024, 1536));
        assert_eq!(fitted_side(Some((1920, 1080)), tall, 1536), 1024);
        assert_eq!(fitted_side(Some((1080, 1920)), tall, 1536), 1536);
        assert_eq!(fitted_side(Some((1440, 1080)), tall, 1536), 1024);
        // Bounded still, and told nothing, the side asked.
        assert_eq!(
            fitted_side(Some((1080, 1920)), Some((4096, 4096)), 2048),
            2048
        );
        assert_eq!(fitted_side(None, tall, 1536), 1536);
        assert_eq!(fitted_side(Some((1920, 1080)), None, 1536), 1536);
        assert_eq!(fitted_side(Some((0, 1080)), tall, 1536), 1536);
    }

    #[test]
    #[ignore = "requires LibreOffice, bubblewrap and Poppler; run explicitly for preview changes"]
    fn real_office_formats_convert_and_render() {
        let tmp = tempfile::tempdir().unwrap();
        let backend = DocumentPreviews::new(Cache::new(tmp.path()).namespace("cache"));
        let office = process::required("libreoffice").unwrap();
        let token = CancelToken::new();
        for (ext, mimetype, body, outputs) in [
            (
                "fodt",
                "text",
                "<office:text><text:p>Synthetic document preview</text:p></office:text>",
                ["odt", "docx", "doc"],
            ),
            (
                "fods",
                "spreadsheet",
                "<office:spreadsheet><table:table table:name=\"Sample\"><table:table-row><table:table-cell office:value-type=\"string\"><text:p>Synthetic cell</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet>",
                ["ods", "xlsx", "xls"],
            ),
            (
                "fodp",
                "presentation",
                "<office:presentation><draw:page draw:name=\"Sample\"><draw:frame svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"12cm\" svg:height=\"3cm\"><draw:text-box><text:p>Synthetic slide</text:p></draw:text-box></draw:frame></draw:page></office:presentation>",
                ["odp", "pptx", "ppt"],
            ),
        ] {
            let path = tmp.path().join(format!("Synthetic d'Example.{ext}"));
            std::fs::write(&path, format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.2" office:mimetype="application/vnd.oasis.opendocument.{mimetype}"><office:body>{body}</office:body></office:document>"#)).unwrap();
            for output in outputs {
                let work = tempfile::tempdir().unwrap();
                convert_office(&office, &path, work.path(), output, &token).unwrap();
                let converted = work.path().join(format!("document.{output}"));
                assert!(
                    converted.exists(),
                    "conversion to {output} produced no file"
                );
                let req = request(&converted);
                assert!(backend.accepts(&req));
                let preview = backend
                    .render(&req, &token)
                    .unwrap_or_else(|e| panic!("{output}: {e:#}"));
                assert!(
                    matches!(preview.info, PreviewInfo::Document { page: 1, pages } if pages >= 1)
                );
                assert!(preview.image.width > 0 && preview.image.height > 0);
                // The optional reader consumes the same isolated conversion.
                for engine in ["pdfium", "poppler", "mupdf"] {
                    if engine == "pdfium" && std::env::var_os("SKWIR_PDFIUM_LIBRARY").is_none() {
                        continue;
                    }
                    let reader = DocumentPreviews::new(Cache::new(tmp.path()).namespace("preview"))
                        .with_reader(engine.into(), None);
                    let session = reader
                        .open_document(&req, &token)
                        .unwrap()
                        .expect("native worker available");
                    let page = session.render(1, 256, &token).unwrap();
                    assert!(!page.content.chars.is_empty(), "{output}: selectable text");
                    assert!(
                        !session.search("Synthetic", &token).unwrap().is_empty(),
                        "{output}: searchable text"
                    );
                }
            }
        }
    }
}
