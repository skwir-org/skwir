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

use crate::process;

const MAX_SIDE: u32 = 2048;
const MAX_ARTIFACT: u64 = 64 * 1024 * 1024;
const RENDER_TIMEOUT: Duration = Duration::from_secs(15);
const CONVERT_TIMEOUT: Duration = Duration::from_secs(45);

fn check(cancel: &CancelToken) -> anyhow::Result<()> {
    anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
    Ok(())
}

fn acquire<'a>(gate: &'a Mutex<()>, cancel: &CancelToken) -> anyhow::Result<MutexGuard<'a, ()>> {
    let started = Instant::now();
    loop {
        check(cancel)?;
        if let Some(guard) = gate.try_lock() {
            return Ok(guard);
        }
        anyhow::ensure!(
            started.elapsed() < CONVERT_TIMEOUT,
            "preview queue timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read_artifact(path: &Path) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        path.metadata()?.len() <= MAX_ARTIFACT,
        "preview output is too large"
    );
    Ok(std::fs::read(path)?)
}

fn image_from_png(bytes: &[u8], max_side: u32) -> anyhow::Result<Image> {
    let reader = || image::ImageReader::new(Cursor::new(bytes)).with_guessed_format();
    let (w, h) = reader()?.into_dimensions()?;
    anyhow::ensure!(
        w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE,
        "preview dimensions exceed limit"
    );
    let image = reader()?
        .decode()?
        .thumbnail(max_side.clamp(64, MAX_SIDE), max_side.clamp(64, MAX_SIDE))
        .to_rgba8();
    Ok(Image {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw().into(),
    })
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
    let m = path.metadata()?;
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        (m.ctime(), m.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = (0i64, 0i64);
    Ok(serde_json::to_vec(&(
        request.entry.id,
        path,
        m.len(),
        m.modified()?,
        changed,
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
    let image = image_from_png(bytes.get(end..)?, max_side).ok()?;
    Some(Preview { image, info })
}

fn finish(
    cache: &CacheSlot,
    key: &[u8],
    png: &[u8],
    info: PreviewInfo,
    request: &PreviewRequest,
    cancel: &CancelToken,
) -> anyhow::Result<Preview> {
    check(cancel)?;
    let image = image_from_png(png, request.max_side)?;
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
    cache: CacheSlot,
    gate: Mutex<()>,
}
impl MediaPreviews {
    pub fn new(cache: CacheSlot) -> Self {
        Self {
            cache,
            gate: Mutex::new(()),
        }
    }
}

impl PreviewProvider for MediaPreviews {
    fn id(&self) -> &'static str {
        "preview-media"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        req.mime.as_deref().is_some_and(|m| m.starts_with("video/"))
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
        let seconds = req
            .seconds
            .filter(|t| t.is_finite())
            .unwrap_or_else(|| duration.map_or(0.0, |d| (d * 0.1).min(30.0)))
            .max(0.0)
            .min(duration.map_or(f64::MAX, |d| (d - 0.1).max(0.0)));
        let tmp = tempfile::tempdir()?;
        let output = tmp.path().join("frame.png");
        let side = req.max_side.clamp(64, MAX_SIDE);
        let mut command = Command::new(ffmpeg);
        command
            .args(["-nostdin", "-v", "error", "-threads", "1", "-ss"])
            .arg(format!("{seconds:.3}"))
            .arg("-i")
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
                "scale={side}:{side}:force_original_aspect_ratio=decrease,setsar=1"
            ))
            .args(["-threads", "1"])
            .arg(&output);
        process::run(&mut command, cancel, RENDER_TIMEOUT)?;
        anyhow::ensure!(
            fingerprint(req, &path)? == base,
            "file changed during preview"
        );
        finish(
            &self.cache,
            &key,
            &read_artifact(&output)?,
            PreviewInfo::Video { seconds, duration },
            req,
            cancel,
        )
    }
}

pub struct DocumentPreviews {
    cache: CacheSlot,
    gate: Mutex<()>,
}
impl DocumentPreviews {
    pub fn new(cache: CacheSlot) -> Self {
        Self {
            cache,
            gate: Mutex::new(()),
        }
    }
}

fn is_pdf(req: &PreviewRequest) -> bool {
    req.mime.as_deref() == Some("application/pdf") || extension(&req.entry.path) == "pdf"
}
fn extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
}

impl PreviewProvider for DocumentPreviews {
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
        let _guard = acquire(&self.gate, cancel)?;
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
            return Ok(preview);
        }
        let tmp = tempfile::tempdir()?;
        let pdf = if let Some(office) = office {
            let conversion_key = self::key(&base, ("pdf", office_stamp))?;
            let pdf = tmp.path().join("document.pdf");
            if let Some(bytes) = self
                .cache
                .get(&conversion_key)
                .filter(|b| b.starts_with(b"%PDF-"))
            {
                std::fs::write(&pdf, bytes)?;
            } else {
                convert_office(&office, &path, tmp.path(), "pdf", cancel)?;
                let bytes = read_artifact(&pdf).context(
                    "LibreOffice did not produce a PDF (the file may be encrypted or unsupported)",
                )?;
                anyhow::ensure!(
                    bytes.starts_with(b"%PDF-"),
                    "conversion did not produce a PDF"
                );
                check(cancel)?;
                anyhow::ensure!(
                    fingerprint(req, &path)? == base,
                    "file changed during conversion"
                );
                if self.cache.put(&conversion_key, &bytes).is_ok() {
                    // Conversion can succeed even if page rendering later fails.
                    let _ = self.cache.trim_to_bytes(128 * 1024 * 1024);
                }
            }
            pdf
        } else {
            path.clone()
        };
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
        let page = req.page.clamp(1, pages);
        let prefix = tmp.path().join("page");
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
                    "-png",
                ])
                .arg(&pdf)
                .arg(&prefix),
            cancel,
            RENDER_TIMEOUT,
        )?;
        anyhow::ensure!(
            fingerprint(req, &path)? == base,
            "file changed during preview"
        );
        finish(
            &self.cache,
            &key,
            &read_artifact(&prefix.with_extension("png"))?,
            PreviewInfo::Document { page, pages },
            req,
            cancel,
        )
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
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            writeln!(bytes, "{} 0 obj\n{object}\nendobj", i + 1).unwrap();
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
        assert!(image_from_png(png.get_ref(), 256).is_err());
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
            }
        }
    }
}
