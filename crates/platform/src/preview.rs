//! On-demand document pages and video frames using installed tools.
//! All methods below run on workers. No renderer process survives its request.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::Context;
use bibli_core::cache::CacheSlot;
use bibli_core::preview::{Preview, PreviewInfo, PreviewProvider, PreviewRequest};
use bibli_core::{CancelToken, thumbs::Image};
use parking_lot::{Mutex, MutexGuard};

use crate::inspection::{check, extension};
use crate::process;

const MAX_SIDE: u32 = 2048;
const MAX_VIDEO_SIDE: u32 = 8192;
const MAX_ARTIFACT: u64 = 64 * 1024 * 1024;
const RENDER_TIMEOUT: Duration = Duration::from_secs(15);
const CONVERT_TIMEOUT: Duration = Duration::from_secs(45);

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

fn read_artifact(path: &Path) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        path.metadata()?.len() <= MAX_ARTIFACT,
        "preview output is too large"
    );
    Ok(std::fs::read(path)?)
}

fn decode_preview(bytes: &[u8], max_side: u32) -> anyhow::Result<Image> {
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
    Ok(serde_json::to_vec(&(
        request.entry.id,
        path,
        crate::document::file_version(path)?,
    ))?)
}

fn key(base: &[u8], suffix: impl serde::Serialize) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(&("preview-v1", base, suffix))?)
}

// A tiny JSON header followed by compressed PNG, not an array of JSON pixels.
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
        let _ = cache.trim_to_bytes(128 * 1024 * 1024);
    }
    Ok(Preview { image, info })
}

pub struct MediaPreviews {
    audio: bool,
    backend: String,
    tuning: crate::media::Tuning,
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
    pub fn with_replay_gain(mut self, replay_gain: crate::media::ReplayGain) -> Self {
        self.tuning.replay_gain = replay_gain;
        self
    }
    pub fn new(cache: CacheSlot) -> Self {
        Self {
            cache,
            backend: "auto".into(),
            tuning: crate::media::Tuning::default(),
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
        wake: bibli_core::location::Waker,
    ) -> anyhow::Result<Option<std::sync::Arc<dyn bibli_core::media::MediaSession>>> {
        if self.backend == "stills" {
            return Ok(None);
        }
        let session = crate::media::WorkerSession::open_tuned(
            req.entry.resolve()?,
            self.tuning,
            lifetime,
            wake,
        )?;
        anyhow::ensure!(
            session.is_some() || self.backend == "auto",
            "Install bibli-media-worker and GStreamer playback plugins, or select the stills backend"
        );
        Ok(session)
    }
    fn id(&self) -> &'static str {
        if self.audio {
            "preview-audio"
        } else {
            "preview-media"
        }
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        // A playlist has an `audio/` type and is not a recording: it is a list
        // of them, and the playlist preview's to read.
        let playlist = matches!(
            extension(&req.entry.path).as_str(),
            "m3u" | "m3u8" | "pls" | "xspf" | "cue"
        );
        (!playlist
            && req
                .mime
                .as_deref()
                .is_some_and(|m| m.starts_with(if self.audio { "audio/" } else { "video/" })))
            || (!self.audio
                && (matches!(
                    req.mime.as_deref(),
                    Some("application/vnd.ms-asf" | "application/x-ms-asf")
                ) || extension(&req.entry.path) == "asf"))
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
                "native-video-v2",
            ),
        )?;
        if let Some(preview) = cached(&self.cache, &key, req.max_side) {
            return Ok(preview);
        }
        let probe = process::run(
            Command::new(ffprobe)
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "json",
                ])
                .arg(&path),
            cancel,
            RENDER_TIMEOUT,
        )?;
        let value: serde_json::Value = serde_json::from_slice(&probe)?;
        let duration = value["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|d| d.is_finite() && *d > 0.0);
        let seconds = if self.audio {
            0.
        } else {
            req.seconds
                .filter(|t| t.is_finite())
                .unwrap_or_else(|| duration.map_or(0.0, |d| (d * 0.1).min(30.0)))
                .max(0.0)
                .min(duration.map_or(f64::MAX, |d| (d - 0.1).max(0.0)))
        };
        let tmp = tempfile::tempdir()?;
        let output = tmp.path().join("frame.png");
        let side = req.max_side.clamp(64, MAX_VIDEO_SIDE);
        let mut command = Command::new(ffmpeg);
        command.args(["-nostdin", "-v", "error", "-threads", "1"]);
        if !self.audio {
            command.arg("-ss").arg(format!("{seconds:.3}"));
        }
        command.arg("-i")
            .arg(&path)
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
            .arg(format!(
                "scale='min(iw,{side})':'min(ih,{side})':force_original_aspect_ratio=decrease,setsar=1"
            ))
            .args(["-threads", "1", "-compression_level", "1"])
            .arg(&output);
        process::run(&mut command, cancel, RENDER_TIMEOUT)?;
        anyhow::ensure!(
            fingerprint(req, &path)? == base,
            "file changed during preview"
        );
        let png = read_artifact(&output)?;
        finish(
            &self.cache,
            &key,
            decode_preview(&png, req.max_side)?,
            &png,
            PreviewInfo::Video { seconds, duration },
            cancel,
        )
    }
}

pub struct DocumentPreviews {
    cache: CacheSlot,
    // One active document's metadata, guarded by the same lock as rendering.
    // Reuse it across page turns, and discard it on source/tool changes.
    gate: Mutex<Option<(Vec<u8>, u32)>>,
    backend: String,
    library: Option<PathBuf>,
}
impl DocumentPreviews {
    pub fn new(cache: CacheSlot) -> Self {
        Self {
            cache,
            gate: Mutex::new(None),
            backend: "auto".into(),
            library: None,
        }
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
        let conversion_key = key(&base, ("pdf", Some(tool_stamp(office)?)))?;
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
                let _ = self.cache.trim_to_bytes(128 * 1024 * 1024);
            }
        }
        Ok(pdf)
    }
}

fn is_pdf(req: &PreviewRequest) -> bool {
    req.mime.as_deref() == Some("application/pdf") || extension(&req.entry.path) == "pdf"
}

impl PreviewProvider for DocumentPreviews {
    fn open_document(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<std::sync::Arc<dyn bibli_core::document::DocumentSession>>> {
        use crate::document::{Engine, WorkerSession};
        anyhow::ensure!(
            matches!(self.backend.as_str(), "auto" | "pdfium" | "poppler"),
            "unknown PDF backend; choose auto, pdfium, or poppler"
        );
        let engines: &[Engine] = match self.backend.as_str() {
            "pdfium" => &[Engine::Pdfium],
            "poppler" => &[Engine::Poppler],
            _ => &[Engine::Pdfium, Engine::Poppler],
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
            let scratch = tempfile::tempdir()?;
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
            if let Some(session) = WorkerSession::open(
                engine,
                pdf,
                source.clone(),
                scratch,
                self.library.clone(),
                lifetime.clone(),
            )? {
                return Ok(Some(session));
            }
        }
        anyhow::ensure!(
            self.backend == "auto",
            "Install the {} document worker and its native library (see README)",
            self.backend
        );
        Ok(None)
    }

    fn id(&self) -> &'static str {
        "preview-documents"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        is_pdf(req)
            || matches!(
                extension(&req.entry.path).as_str(),
                "doc"
                    | "docx"
                    | "docm"
                    | "odt"
                    | "ott"
                    | "rtf"
                    | "fodt"
                    | "xls"
                    | "xlsx"
                    | "xlsm"
                    | "ods"
                    | "ots"
                    | "fods"
                    | "ppt"
                    | "pptx"
                    | "pptm"
                    | "odp"
                    | "otp"
                    | "fodp"
                    | "odg"
            )
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
                let info = process::run(
                    Command::new(pdfinfo).env("LC_ALL", "C").arg(&pdf),
                    cancel,
                    RENDER_TIMEOUT,
                )?;
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
        let prefix = tmp.path().join("page");
        let rendering = Instant::now();
        process::run(
            Command::new(renderer)
                .args([
                    "-f",
                    &page.to_string(),
                    "-l",
                    &page.to_string(),
                    "-singlefile",
                    "-scale-to",
                    &req.max_side.clamp(64, MAX_SIDE).to_string(),
                ])
                .arg(&pdf)
                .arg(&prefix),
            cancel,
            RENDER_TIMEOUT,
        )?;
        let render_ms = rendering.elapsed().as_millis();
        anyhow::ensure!(
            fingerprint(req, &path)? == base,
            "file changed during preview"
        );
        check(cancel)?;
        // PPM avoids Poppler's expensive PNG compression. Decode once, then
        // encode losslessly with the fast cache encoder (also used on revisits).
        let image = decode_preview(&read_artifact(&prefix.with_extension("ppm"))?, req.max_side)?;
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

/// Conversion is isolated from the user's `LibreOffice` session and files. Linux
/// requires bubblewrap: no network, only the input and system runtime are readable,
/// and only the disposable working directory is writable. Never silently retry
/// without isolation when the sandbox cannot start.
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
    #[cfg(target_os = "linux")]
    {
        let bwrap = process::required("bwrap")?;
        let mut command = Command::new(bwrap);
        command.args(["--unshare-all", "--die-with-parent", "--new-session"]);
        for system in [
            "/nix/store",
            "/usr",
            "/bin",
            "/lib",
            "/lib64",
            "/etc/fonts",
            "/etc/ld.so.cache",
            "/etc/localtime",
        ] {
            if Path::new(system).exists() {
                command.args(["--ro-bind", system, system]);
            }
        }
        let source = format!("/input/document.{}", extension(input));
        command
            .args([
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--tmpfs",
                "/tmp",
                "--dir",
                "/input",
                "--ro-bind",
            ])
            .arg(input)
            .arg(&source)
            .arg("--bind")
            .arg(work)
            .arg("/work")
            .args([
                "--chdir",
                "/work",
                "--setenv",
                "HOME",
                "/work/home",
                "--setenv",
                "XDG_CACHE_HOME",
                "/work/cache",
                "--setenv",
                "SAL_USE_VCLPLUGIN",
                "svp",
                "--unsetenv",
                "DISPLAY",
                "--unsetenv",
                "WAYLAND_DISPLAY",
                "--unsetenv",
                "DBUS_SESSION_BUS_ADDRESS",
                "--",
            ])
            .arg(office)
            .args([
                "-env:UserInstallation=file:///work/profile",
                "--headless",
                "--nologo",
                "--nodefault",
                "--norestore",
                "--convert-to",
                output_format,
                "--outdir",
                "/work",
            ])
            .arg(source);
        process::run(&mut command, cancel, CONVERT_TIMEOUT)?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (office, input, output_format, cancel);
        anyhow::bail!("isolated office preview is not implemented on this platform")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::{cache::Cache, op::EntryRef};
    use std::io::Write;

    fn request(path: &Path) -> PreviewRequest {
        PreviewRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: crate::mime::by_name(path.as_os_str()).map(|m| m.to_string()),
            page: 1,
            seconds: None,
            max_side: 256,
        }
    }

    #[test]
    fn asf_containers_are_media_even_with_application_mime() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::new(tmp.path().join("cache"));
        let video = MediaPreviews::new(cache.namespace("video"));
        let audio = MediaPreviews::new(cache.namespace("audio")).audio();
        let path = tmp.path().join("Synthetic clip.ASF");
        std::fs::write(&path, b"synthetic container marker").unwrap();
        let mut req = request(&path);
        for mime in [
            Some("application/vnd.ms-asf"),
            Some("application/x-ms-asf"),
            Some("video/x-ms-asf"),
            Some("application/octet-stream"),
            None,
        ] {
            req.mime = mime.map(str::to_owned);
            assert!(video.accepts(&req));
            assert!(!audio.accepts(&req));
        }
        req.entry.path = tmp.path().join("clip.data");
        req.mime = Some("application/vnd.ms-asf".into());
        assert!(video.accepts(&req));
        req.mime = Some("application/octet-stream".into());
        assert!(!video.accepts(&req));
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
    #[ignore = "requires PDFium worker/library and Poppler; run in release mode for timings"]
    fn real_pdfium_and_poppler_backend_comparison() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic scanned reader.pdf");
        scanned_pdf(&path);
        let mut req = request(&path);
        req.max_side = MAX_SIDE;
        let cancel = CancelToken::new();
        for engine in ["pdfium", "poppler"] {
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
            matches!(later.info, PreviewInfo::Video { seconds, duration: Some(d) } if (seconds - 2.0).abs() < 0.001 && d >= 3.0)
        );
        assert_eq!(
            backend.render(&req, &token).unwrap().image.rgba,
            later.image.rgba
        );
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
        token.cancel();
        assert!(backend.render(&req, &token).is_err());
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
                for engine in ["pdfium", "poppler"] {
                    if engine == "pdfium" && std::env::var_os("BIBLI_PDFIUM_LIBRARY").is_none() {
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
