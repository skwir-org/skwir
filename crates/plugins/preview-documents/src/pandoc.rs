//! Pandoc, when installed, reads text documents bibli does not (RTF), or
//! all of them when asked (ADR-030). It runs in the converters' sandbox
//! and writes HTML, which the reading mode reads like any page; the
//! pictures it extracts are kept in the cache, so they can be shown when
//! scrolled to, after its folder is gone.
use bibli_core::CancelToken;
use bibli_core::inspection::Picture;
use bibli_platform::inspection::{self, limits};
use bibli_platform::{process, rich_preview, sandbox};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::SystemTime;

/// The Pandoc input format for a file of extension `extension`.
pub(crate) fn format_of(extension: &str) -> Option<&'static str> {
    match extension {
        "docx" | "docm" | "dotx" | "dotm" => Some("docx"),
        "odt" | "ott" => Some("odt"),
        "rtf" => Some("rtf"),
        _ => None,
    }
}

/// Where a picture Pandoc extracted is, under the folder it wrote in.
const MEDIA: &str = "media";

/// The installed Pandoc and the formats it reads.
#[derive(Clone, Debug)]
pub(crate) struct Pandoc {
    pub path: PathBuf,
    formats: Vec<String>,
}

/// The formats each installed Pandoc said it reads, by its path and date:
/// asked once, not for every file.
fn known() -> &'static Mutex<HashMap<(PathBuf, SystemTime), Vec<String>>> {
    static KNOWN: OnceLock<Mutex<HashMap<(PathBuf, SystemTime), Vec<String>>>> = OnceLock::new();
    KNOWN.get_or_init(Mutex::default)
}

impl Pandoc {
    /// Pandoc, if it is installed and can run isolated. Asks it, the first
    /// time, which formats it reads.
    pub(crate) fn find() -> Option<Pandoc> {
        let path = process::required("pandoc").ok()?;
        if !sandbox::available() {
            return None;
        }
        let modified = path.metadata().ok()?.modified().ok()?;
        let key = (path.clone(), modified);
        if let Some(formats) = known().lock().get(&key) {
            return Some(Pandoc {
                path,
                formats: formats.clone(),
            });
        }
        let listed = process::run(
            Command::new(&path).arg("--list-input-formats"),
            &CancelToken::new(),
            limits::HELPER_TIMEOUT,
        )
        .ok()?;
        let formats: Vec<String> = String::from_utf8_lossy(&listed)
            .lines()
            .map(|line| line.trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect();
        known().lock().insert(key, formats.clone());
        Some(Pandoc { path, formats })
    }

    pub(crate) fn reads(&self, format: &str) -> bool {
        self.formats.iter().any(|f| f == format)
    }

    /// `source` converted to HTML in `work`, read within
    /// `limits::TEXT_INPUT`: the page, and whether it was cut.
    pub(crate) fn convert(
        &self,
        source: &Path,
        format: &str,
        work: &Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<(String, bool)> {
        let (mut command, input) = sandbox::command(&self.path, source, work)?;
        command
            .args(["--from", format, "--to", "html5"])
            .arg(format!("--extract-media=/work/{MEDIA}"))
            .args(["--output", "/work/document.html"])
            .arg(input);
        process::run(
            &mut command,
            cancel,
            bibli_platform::preview::CONVERT_TIMEOUT,
        )?;
        let bytes = inspection::read(
            &work.join("document.html"),
            0,
            limits::TEXT_INPUT + 1,
            cancel,
        )?;
        let cut = bytes.len() > limits::TEXT_INPUT;
        let bytes = &bytes[..bytes.len().min(limits::TEXT_INPUT)];
        Ok((String::from_utf8_lossy(bytes).into_owned(), cut))
    }
}

/// The name a picture Pandoc wrote has, from where its page says it is
/// (`/work/media/…`): within its folder, or none.
pub(crate) fn media_name(src: &str) -> Option<String> {
    let name = crate::package::normalize(src.strip_prefix("/work/")?)?;
    name.starts_with(&format!("{MEDIA}/")).then_some(name)
}

/// The pictures of a page Pandoc wrote in `work`: each named for later,
/// handed to `keep` to be read again by its name once `work` is gone.
pub(crate) struct Media<'a, K: FnMut(&str, &[u8])> {
    pub work: &'a Path,
    pub keep: K,
    pub cancel: &'a CancelToken,
}

impl<K: FnMut(&str, &[u8])> rich_preview::Resources for Media<'_, K> {
    fn picture(&mut self, src: &str) -> Option<Picture> {
        let name = media_name(src)?;
        let bytes = inspection::read_bounded(
            &self.work.join(&name),
            limits::BINARY_INPUT,
            "Embedded image",
            self.cancel,
        )
        .ok()?;
        let (width, height) = inspection::picture_size(&bytes)?;
        (self.keep)(&name, &bytes);
        Some(Picture::Later {
            key: name,
            width,
            height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_only_those_pandoc_extracted() {
        assert_eq!(
            media_name("/work/media/media/rId11.png").as_deref(),
            Some("media/media/rId11.png")
        );
        assert_eq!(media_name("/work/media/../document.html"), None);
        assert_eq!(media_name("/work/../etc/passwd"), None);
        assert_eq!(media_name("media/rId11.png"), None);
        assert_eq!(media_name("https://example.invalid/a.png"), None);
    }

    #[test]
    fn formats_by_extension() {
        assert_eq!(format_of("dotm"), Some("docx"));
        assert_eq!(format_of("ott"), Some("odt"));
        assert_eq!(format_of("rtf"), Some("rtf"));
        assert_eq!(format_of("doc"), None);
    }
}
