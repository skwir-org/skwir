//! bibli plugin: XMP (ISO 16684-1), the one standard for a file's metadata
//! beside it or inside it (ADR-027). A sidecar is a notes store: read with
//! the others, updated where it is, created only when the user asks
//! (Notes page); it is a companion of its file (ADR-028), which it follows.
//! XMP inside a file (a picture's, a PDF's) is read on demand for Details,
//! never written: that would change the file's content.
//!
//! Sidecars are named two ways: darktable, digiKam and others add `.xmp`
//! to the file's name (`x.jpg.xmp`); Adobe's replace the extension
//! (`x.xmp`), which a camera's pair (`x.CR3` and `x.JPG`) both claim, so
//! that one follows its file only while no other file shares its stem, and
//! is read for a file only when it does not say it is another's
//! (`photoshop:SidecarForExtension`).

#[cfg(test)]
mod any_input;
pub mod packet;

use std::ffi::{OsStr, OsString};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::companions::Companion;
use bibli_core::entry::Value;
use bibli_core::fields::FieldProvider;
use bibli_core::metadata::Listed;
use bibli_core::notes::raw::{self, RawKey};
use bibli_core::notes::{Holds, Kept, Notes, NotesStore, Probe, Tag, Write, Wrote};
use bibli_core::plugin::{Category, Plugin};

pub(crate) const ID: &str = "xmp";

fn with_suffix(name: &OsStr, suffix: &str) -> OsString {
    let mut out = name.to_os_string();
    out.push(suffix);
    out
}

/// A sidecar's text: a regular file of a sidecar's size at most, so a pipe
/// or a giant named `x.xmp` holds nothing up.
fn read(path: &Path) -> std::io::Result<String> {
    bibli_platform::file::read_regular_text(path, bibli_platform::file::SIDE_FILE_LIMIT)
}

/// `x.jpg.xmp`, the name everyone but Adobe gives a sidecar.
fn appended(name: &OsStr) -> OsString {
    with_suffix(name, ".xmp")
}

/// `x.xmp`, Adobe's, for a name with an extension.
fn replaced(name: &OsStr) -> Option<OsString> {
    let path = Path::new(name);
    path.extension()?;
    let stem = path.file_stem()?;
    Some(with_suffix(stem, ".xmp"))
}

/// Whether `name` ends in `.xmp` (any case). Compared as bytes: four
/// bytes from the end of a name may fall inside a character (`crème`).
fn is_sidecar(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    bytes.len() > 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".xmp")
}

/// Whether another file of `dir` than `path` has `path`'s stem: then
/// Adobe's `x.xmp` is not `path`'s alone.
fn stem_shared(path: &Path) -> bool {
    let (Some(dir), Some(stem), Some(name)) = (path.parent(), path.file_stem(), path.file_name())
    else {
        return true;
    };
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let other = e.file_name();
            other != name && !is_sidecar(&other) && Path::new(&other).file_stem() == Some(stem)
        })
    })
}

/// XMP sidecars as a notes store.
pub struct XmpStore {
    /// The `.xmp` files of a folder.
    names: bibli_core::companions::FolderNames,
}

impl Default for XmpStore {
    fn default() -> Self {
        XmpStore {
            names: bibli_core::companions::FolderNames::new(|name| {
                is_sidecar(name).then(|| name.to_owned())
            }),
        }
    }
}

impl XmpStore {
    /// The sidecar of the file named `name` in `dir`, if it has one:
    /// `x.jpg.xmp`, else Adobe's `x.xmp` unless it says it is another's.
    fn sidecar(dir: &Path, name: &OsStr, has: &dyn Fn(&OsStr) -> bool) -> Option<PathBuf> {
        let appended = appended(name);
        if has(&appended) {
            return Some(dir.join(appended));
        }
        let replaced = replaced(name).filter(|r| has(r))?;
        let path = dir.join(&replaced);
        let text = read(&path).ok()?;
        let ext = Path::new(name).extension()?.to_string_lossy().into_owned();
        packet::sidecar_for(&text)
            .is_none_or(|theirs| theirs.eq_ignore_ascii_case(&ext))
            .then_some(path)
    }

    fn existing(dir: &Path, name: &OsStr) -> Option<PathBuf> {
        Self::sidecar(dir, name, &|n: &OsStr| dir.join(n).exists())
    }
}

impl NotesStore for XmpStore {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "XMP sidecars (darktable, digiKam, Lightroom)"
    }
    fn kept(&self) -> Kept {
        Kept::Beside
    }
    fn holds(&self) -> Holds {
        Holds {
            tags: true,
            tag_colors: false,
            rating: true,
            comment: true,
            origin: false,
            limits: "whole stars; a tag's color only as a color label",
        }
    }
    fn default_write(&self) -> Write {
        Write::Update
    }
    fn fit(&self, notes: &Notes) -> Notes {
        Notes {
            tags: notes
                .tags
                .iter()
                .map(|t| Tag::named(t.name.clone()))
                .collect(),
            rating: notes.rating.map(|r| packet::stars(r) * 2),
            comment: notes.comment.clone(),
            origin: None,
            referrer: None,
        }
    }
    fn in_use(&self, dir: &Path) -> bool {
        !self.names.of(dir).is_empty()
    }
    fn read(
        &self,
        dir: &Path,
        files: &[Probe<'_>],
        listed: Option<&Listed<'_>>,
    ) -> Vec<Option<Notes>> {
        let scanned = if listed.is_none() {
            self.names.of(dir)
        } else {
            Arc::default()
        };
        if listed.is_none() && scanned.is_empty() {
            return files.iter().map(|_| None).collect();
        }
        let has = |n: &OsStr| match listed {
            Some(listed) => listed(n),
            None => scanned.contains(n),
        };
        files
            .iter()
            .map(|f| {
                if is_sidecar(f.name) {
                    return None;
                }
                let path = Self::sidecar(dir, f.name, &has)?;
                packet::notes_of(&read(&path).ok()?).filter(|n| !n.is_empty())
            })
            .collect()
    }
    fn keys(&self, file: &Probe<'_>, create: bool) -> Vec<RawKey> {
        let Some(dir) = file.path.parent() else {
            return Vec::new();
        };
        match Self::existing(dir, file.name) {
            Some(path) => vec![RawKey::Beside {
                path: path.file_name().map(PathBuf::from).unwrap_or_default(),
            }],
            None if create => vec![RawKey::Beside {
                path: appended(file.name).into(),
            }],
            None => Vec::new(),
        }
    }
    fn write(&self, file: &Probe<'_>, notes: &Notes, create: bool) -> std::io::Result<Wrote> {
        let Some(dir) = file.path.parent() else {
            return Ok(Wrote::NotHere);
        };
        let has_notes = notes.has_user_notes();
        let (path, text) = match Self::existing(dir, file.name) {
            Some(path) => {
                let text = read(&path)?;
                let Some(edited) = packet::edit(&text, notes) else {
                    // Not XMP bibli can edit: left alone, the others kept.
                    tracing::debug!(path = %path.display(), "xmp: a sidecar left as it is");
                    return Ok(Wrote::NotHere);
                };
                if edited == text {
                    return Ok(Wrote::Unchanged);
                }
                (path, edited)
            }
            None if create && has_notes => {
                (dir.join(appended(file.name)), packet::new_document(notes))
            }
            None => return Ok(Wrote::NotHere),
        };
        raw::replace(&path, text.as_bytes())?;
        if let Some(name) = path.file_name() {
            self.names.wrote(dir, name);
        }
        Ok(Wrote::Written)
    }
}

/// A sidecar as its file's companion: it goes where the file goes.
pub struct XmpCompanion;

impl Companion for XmpCompanion {
    fn id(&self) -> &'static str {
        ID
    }
    fn of(&self, name: &OsStr) -> Vec<PathBuf> {
        let mut out = vec![PathBuf::from(appended(name))];
        if let Some(replaced) = replaced(name) {
            out.push(replaced.into());
        }
        out
    }
    fn owner(&self, name: &OsStr) -> Option<OsString> {
        // Only `x.jpg.xmp` names its file; Adobe's `x.xmp` does not say
        // which of its stem's files it is, and stays in view.
        let s = name.to_str()?;
        if !is_sidecar(name) {
            return None;
        }
        // The last four bytes are `.xmp`: cutting them is cutting a name.
        let file = &s[..s.len() - 4];
        Path::new(file).extension()?;
        (!file.starts_with('.')).then(|| file.into())
    }
    fn follows(&self, path: &Path, index: usize) -> bool {
        index == 0 || !stem_shared(path)
    }
}

/// XMP inside a file, read for Details: what it says of its keywords,
/// rating and description.
pub struct Embedded;

/// Extensions of the formats that carry an XMP packet.
const EMBEDDING: [&str; 16] = [
    "jpg", "jpeg", "png", "tif", "tiff", "webp", "heic", "heif", "avif", "dng", "psd", "pdf",
    "mp4", "mov", "svg", "ai",
];

/// How far into a file its packet is looked for.
const SCANNED: u64 = 16 << 20;

impl FieldProvider for Embedded {
    fn id(&self) -> &'static str {
        "xmp.embedded"
    }
    fn keys(&self) -> &'static [&'static str] {
        &["xmp.embedded"]
    }
    fn label(&self, _key: &str) -> Option<&'static str> {
        Some("XMP in the file")
    }
    fn accepts(&self, _key: &str, name: &OsStr) -> bool {
        Path::new(name)
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|e| EMBEDDING.iter().any(|x| x.eq_ignore_ascii_case(e)))
    }
    fn read(&self, _key: &str, path: &Path, cancel: &CancelToken) -> anyhow::Result<Option<Value>> {
        let file = bibli_platform::file::open_regular(path)?;
        let mut bytes = Vec::new();
        file.take(SCANNED).read_to_end(&mut bytes)?;
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let Some(notes) = packet::packet_in(&bytes).and_then(packet::notes_of) else {
            return Ok(None);
        };
        Ok(summary(&notes).map(|s| Value::Text(s.into())))
    }
}

/// XMP's notes in words: "Keywords a, b · 4 stars · “the description”".
pub fn summary(notes: &Notes) -> Option<String> {
    let mut parts = Vec::new();
    if !notes.tags.is_empty() {
        parts.push(format!("Keywords {}", notes.tag_names().join(", ")));
    }
    if let Some(r) = notes.rating {
        parts.push(bibli_core::notes::stars(r));
    }
    if let Some(c) = &notes.comment {
        parts.push(format!("“{c}”"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The XMP plugin.
#[derive(Debug, Default)]
pub struct XmpPlugin;

impl Plugin for XmpPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "XMP"
    }
    fn description(&self) -> &'static str {
        "Tags, ratings and descriptions in XMP sidecars, as photo applications keep them; XMP inside pictures and PDFs shown in Details"
    }
    fn category(&self) -> Category {
        Category::Files
    }
    fn capabilities(&self) -> bibli_core::permissions::Capabilities {
        use bibli_core::permissions::Capability;
        bibli_core::permissions::Capabilities::of(&[Capability::Read, Capability::Write])
    }
    fn register(&self, app: &App) {
        app.register_notes_store(ID, Arc::new(XmpStore::default()));
        app.companions.register(ID, Arc::new(XmpCompanion));
        app.fields.register(ID, Arc::new(Embedded));
    }
}

#[cfg(test)]
mod tests;
