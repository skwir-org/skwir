//! Zip packages: a Word or `OpenDocument` file, an EPUB, a 3MF or a
//! `FreeCAD` document, a spreadsheet, a KMZ. One way to open one and read
//! its members:
//!
//! - opened only within `limits::CONTAINER_MEMBERS` members;
//! - members found by their normalized names ([`normalize`]: `.`, `..` and
//!   empty segments collapsed, a name climbing out of the root no member),
//!   never as a path on disk nor a name the zip library interprets;
//! - each read bounded by the bytes it inflates, never by the size a
//!   central directory declares, which can say anything.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use skwir_core::CancelToken;

use crate::inspection::{self, limits};

type Archive = zip::ZipArchive<BufReader<File>>;

pub struct Package {
    archive: Archive,
    /// Normalized member name (lowercased when `folded`) → its index.
    members: HashMap<String, usize>,
    /// Names compared without case, as a Word package's parts are.
    folded: bool,
}

impl Package {
    /// The package at `path`, `what` naming it in the message of a package
    /// with too many members ("The document"); its names compared without
    /// case when `folded`.
    ///
    /// # Errors
    /// When it cannot be read, is no zip, or holds too many members.
    pub fn open(
        path: &Path,
        what: &str,
        folded: bool,
        cancel: &CancelToken,
    ) -> anyhow::Result<Self> {
        let mut archive = zip::ZipArchive::new(BufReader::new(inspection::open(path)?))?;
        anyhow::ensure!(
            archive.len() <= limits::CONTAINER_MEMBERS,
            "{what} holds more than {} members",
            limits::CONTAINER_MEMBERS
        );
        let mut members = HashMap::new();
        for index in 0..archive.len() {
            inspection::check(cancel)?;
            // A name its archive calls UTF-8 that is not can never be asked
            // for: that member is left out, not the package.
            let member = archive.by_index_raw(index)?;
            if let Some(name) = member.name().ok().and_then(|name| normalize(&name)) {
                let name = if folded { name.to_lowercase() } else { name };
                members.entry(name).or_insert(index);
            }
        }
        Ok(Package {
            archive,
            members,
            folded,
        })
    }

    fn index(&self, name: &str) -> Option<usize> {
        let name = normalize(name)?;
        let name = if self.folded {
            name.to_lowercase()
        } else {
            name
        };
        self.members.get(&name).copied()
    }

    pub fn has(&self, name: &str) -> bool {
        self.index(name).is_some()
    }

    /// Every member's normalized name (lowercased when folded).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.members.keys().map(String::as_str)
    }

    /// Member `name`, whole within `bound` bytes inflated; `what` names it
    /// in the messages.
    ///
    /// # Errors
    /// When it is missing, damaged, longer than `bound` or cancelled.
    pub fn read(
        &mut self,
        name: &str,
        bound: usize,
        what: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<u8>> {
        let index = self
            .index(name)
            .ok_or_else(|| anyhow::anyhow!("{what} is missing"))?;
        let file = self.archive.by_index(index)?;
        inspection::copy_bounded(file, bound, what, cancel)
    }

    /// The start of member `name`, at most `limit` bytes of it: enough to
    /// learn a picture's size, or a model's metadata, without inflating
    /// the rest.
    pub fn head(&mut self, name: &str, limit: usize) -> Option<Vec<u8>> {
        let index = self.index(name)?;
        let mut head = Vec::new();
        self.archive
            .by_index(index)
            .ok()?
            .take(limit as u64)
            .read_to_end(&mut head)
            .ok()?;
        Some(head)
    }

    /// Member `name` to stream, for a reader that bounds what it takes of
    /// it itself (a track, a spreadsheet's parts counted together).
    pub fn reader(&mut self, name: &str) -> Option<impl Read + '_> {
        let index = self.index(name)?;
        self.archive.by_index(index).ok()
    }
}

/// Whether the file is a zip, from its first bytes: an encrypted Word file
/// is an OLE container under the same name.
pub fn is_zip(path: &Path) -> bool {
    let mut head = [0u8; 4];
    inspection::open(path)
        .and_then(|mut file| Ok(file.read_exact(&mut head)?))
        .is_ok()
        && head == *b"PK\x03\x04"
}

/// A member's name with `.`, `..` and empty segments collapsed (`/` and
/// `\` both separate); `None` for one that climbs out of the root.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn a_member_name_never_climbs_out() {
        assert_eq!(
            normalize("word/../media/a.png").as_deref(),
            Some("media/a.png")
        );
        assert_eq!(
            normalize("./OEBPS//ch1.xhtml").as_deref(),
            Some("OEBPS/ch1.xhtml")
        );
        assert_eq!(normalize("a\\b").as_deref(), Some("a/b"));
        assert_eq!(normalize("../etc/passwd"), None);
        assert_eq!(normalize(""), None);
    }
}
