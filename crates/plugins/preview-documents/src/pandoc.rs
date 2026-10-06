//! Pandoc, when installed, reads text documents bibli does not (RTF), or
//! all of them when asked (ADR-030). It runs in its own sandbox
//! (`--sandbox`: it reads the file it is named and nothing else, and
//! fetches nothing), and in the converters' where the system has one; a
//! Pandoc too old for its own runs only in the system's (ADR-039). It
//! writes HTML, which the reading mode reads like any page; the pictures
//! it extracts are kept in the cache, so they can be shown when scrolled
//! to, after its folder is gone.
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

/// The installed Pandoc, the formats it reads, and whether it has its own
/// sandbox.
#[derive(Clone, Debug)]
pub(crate) struct Pandoc {
    pub path: PathBuf,
    formats: Vec<String>,
    sandboxes: bool,
}

/// What each installed Pandoc said of itself, by its path and date: asked
/// once, not for every file.
type Said = (Vec<String>, bool);

fn known() -> &'static Mutex<HashMap<(PathBuf, SystemTime), Said>> {
    static KNOWN: OnceLock<Mutex<HashMap<(PathBuf, SystemTime), Said>>> = OnceLock::new();
    KNOWN.get_or_init(Mutex::default)
}

/// Whether the Pandoc whose `--version` says `version` has its own
/// sandbox (`--sandbox`, from Pandoc 2.15): its first line names its
/// version, `pandoc 3.7.0.2`.
fn has_sandbox(version: &str) -> bool {
    let number = version.lines().next().and_then(|line| {
        line.split_whitespace()
            .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
    });
    let mut parts = number.into_iter().flat_map(|n| n.split('.'));
    let major = parts.next().and_then(|p| p.parse::<u32>().ok());
    let minor = parts
        .next()
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    major.is_some_and(|major| (major, minor) >= (2, 15))
}

/// What the Pandoc at `path` says of itself: the formats it reads, and
/// whether it has its own sandbox.
fn ask_about(path: &Path) -> Option<Said> {
    let ask = |argument: &str| {
        process::run(
            Command::new(path).arg(argument),
            &CancelToken::new(),
            limits::HELPER_TIMEOUT,
        )
        .ok()
        .map(|said| String::from_utf8_lossy(&said).into_owned())
    };
    let formats: Vec<String> = ask("--list-input-formats")?
        .lines()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect();
    let sandboxes = ask("--version").is_some_and(|v| has_sandbox(&v));
    Some((formats, sandboxes))
}

impl Pandoc {
    /// Pandoc, if it is installed and can run: in its own sandbox, or in
    /// the system's. Asks it, the first time, its version and which
    /// formats it reads.
    pub(crate) fn find() -> Option<Pandoc> {
        let path = process::required("pandoc").ok()?;
        let modified = path.metadata().ok()?.modified().ok()?;
        let key = (path.clone(), modified);
        let known_now = known().lock().get(&key).cloned();
        let (formats, sandboxes) = if let Some(said) = known_now {
            said
        } else {
            let said = ask_about(&path)?;
            known().lock().insert(key, said.clone());
            said
        };
        (sandboxes || sandbox::available()).then_some(Pandoc {
            path,
            formats,
            sandboxes,
        })
    }

    pub(crate) fn reads(&self, format: &str) -> bool {
        self.formats.iter().any(|f| f == format)
    }

    /// Pandoc run as `run` is set (isolated or not), in its own sandbox
    /// when it has one: `run.input` read as `format`, written as HTML in
    /// its working folder, its pictures under `media/`, both named from
    /// that folder, so the page names them as `media/…` wherever it ran.
    fn run_in(
        &self,
        mut run: sandbox::Untrusted,
        format: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        if self.sandboxes {
            run.command.arg("--sandbox");
        }
        run.command
            .args(["--from", format, "--to", "html5"])
            .arg(format!("--extract-media={MEDIA}"))
            .args(["--output", "document.html"])
            .arg(&run.input);
        process::run(
            &mut run.command,
            cancel,
            bibli_platform::preview::CONVERT_TIMEOUT,
        )?;
        Ok(())
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
        // Its own sandbox lets it run where the system has none.
        let otherwise = if self.sandboxes {
            sandbox::Otherwise::Run
        } else {
            sandbox::Otherwise::Refuse
        };
        self.run_in(
            sandbox::untrusted(&self.path, source, work, otherwise)?,
            format,
            cancel,
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
/// (`media/…`, from its working folder): within its pictures' folder, or
/// none.
pub(crate) fn media_name(src: &str) -> Option<String> {
    let name = crate::package::normalize(src)?;
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
            media_name("media/media/rId11.png").as_deref(),
            Some("media/media/rId11.png")
        );
        assert_eq!(media_name("media/../document.html"), None);
        assert_eq!(media_name("../etc/passwd"), None);
        assert_eq!(media_name("/etc/passwd"), None);
        assert_eq!(media_name("document.html"), None);
        assert_eq!(media_name("https://example.invalid/a.png"), None);
        assert_eq!(media_name("file:///media/a.png"), None);
    }

    #[test]
    fn its_own_sandbox_comes_with_pandoc_2_15() {
        assert!(has_sandbox("pandoc 3.7.0.2\nFeatures: +server +lua\n"));
        assert!(has_sandbox("pandoc.exe 2.15\n"));
        assert!(has_sandbox("pandoc 2.19.2\n"));
        assert!(!has_sandbox("pandoc 2.9.2.1\n"));
        assert!(!has_sandbox("pandoc 2.14.2\n"));
        assert!(!has_sandbox("not pandoc at all"));
        assert!(!has_sandbox(""));
    }

    /// Pandoc as it runs where the system isolates nothing (Windows, a
    /// Linux without bubblewrap): in its own sandbox alone. Needs Pandoc,
    /// which the dev shell has.
    #[test]
    #[ignore = "runs Pandoc"]
    fn without_the_systems_sandbox_pandoc_keeps_to_its_own() {
        let pandoc = Pandoc::find().expect("Pandoc installed");
        assert!(pandoc.sandboxes, "a Pandoc of 2.15 or later");
        let dir = tempfile::tempdir().unwrap();
        let rtf = dir.path().join("a note [draft] été.rtf");
        std::fs::write(&rtf, br"{\rtf1\ansi{\b Bold} text\par}").unwrap();
        let work = tempfile::tempdir().unwrap();
        let run = sandbox::plain(&pandoc.path, &rtf, work.path());
        pandoc.run_in(run, "rtf", &CancelToken::new()).unwrap();
        let html = std::fs::read_to_string(work.path().join("document.html")).unwrap();
        assert!(html.contains("<strong>Bold</strong>"), "{html}");
    }

    #[test]
    fn formats_by_extension() {
        assert_eq!(format_of("dotm"), Some("docx"));
        assert_eq!(format_of("ott"), Some("odt"));
        assert_eq!(format_of("rtf"), Some("rtf"));
        assert_eq!(format_of("doc"), None);
    }
}
