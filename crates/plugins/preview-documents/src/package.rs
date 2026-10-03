//! A Word or `OpenDocument` file is a zip of parts. Every read is bounded by
//! the bytes it actually inflates (a central directory can claim anything)
//! and every name is a key into the package, never a path on disk.
use bibli_core::CancelToken;
use bibli_platform::inspection::{self, limits};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Whether the file is a zip, from its first bytes: an encrypted Word file
/// is an OLE container under the same name, and only a zip is read here.
pub(crate) fn is_zip(path: &Path) -> bool {
    let mut head = [0u8; 4];
    inspection::open(path)
        .and_then(|mut file| Ok(file.read_exact(&mut head)?))
        .is_ok()
        && head == *b"PK\x03\x04"
}

pub(crate) struct Package {
    archive: zip::ZipArchive<std::fs::File>,
    /// Normalized, lowercased member name → index: a Word package's part
    /// names are case-insensitive, and a name that climbs out of the root
    /// is no member.
    members: HashMap<String, usize>,
}

impl Package {
    pub(crate) fn open(path: &Path, cancel: &CancelToken) -> anyhow::Result<Self> {
        let mut archive = zip::ZipArchive::new(inspection::open(path)?)?;
        anyhow::ensure!(
            archive.len() <= limits::CONTAINER_MEMBERS,
            "The document holds more than {} parts",
            limits::CONTAINER_MEMBERS
        );
        let mut members = HashMap::new();
        for index in 0..archive.len() {
            inspection::check(cancel)?;
            if let Some(name) = normalize(archive.by_index_raw(index)?.name()) {
                members.entry(name.to_lowercase()).or_insert(index);
            }
        }
        Ok(Self { archive, members })
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.members.get(&normalize(name)?.to_lowercase()).copied()
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.index(name).is_some()
    }

    /// Member `name`, read whole within `limits::BINARY_INPUT`.
    pub(crate) fn read(
        &mut self,
        name: &str,
        what: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<u8>> {
        let index = self
            .index(name)
            .ok_or_else(|| anyhow::anyhow!("{what} is missing from the document"))?;
        let file = self.archive.by_index(index)?;
        inspection::copy_bounded(file, limits::BINARY_INPUT, what, cancel)
    }

    /// Member `name` as text, if it is there: a part that is missing is
    /// one the document does not use.
    pub(crate) fn text(
        &mut self,
        name: &str,
        what: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<String>> {
        if !self.has(name) {
            return Ok(None);
        }
        let bytes = self.read(name, what, cancel)?;
        Ok(Some(inspection::decode(&bytes).0.into_owned()))
    }

    /// The start of member `name`, enough to learn a picture's size
    /// without inflating the rest.
    pub(crate) fn head(&mut self, name: &str, limit: usize) -> Option<Vec<u8>> {
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

    /// A picture the document names by member, sized for later from its
    /// header (`Picture::Later`, read again by `picture`).
    pub(crate) fn picture(
        &mut self,
        name: &str,
        cancel: &CancelToken,
    ) -> Option<bibli_core::inspection::Picture> {
        let key = normalize(name)?;
        let head = self.head(&key, limits::PICTURE_HEADER)?;
        let (width, height) = inspection::picture_size(&head).or_else(|| {
            let whole = self.read(&key, "Embedded image", cancel).ok()?;
            inspection::picture_size(&whole)
        })?;
        Some(bibli_core::inspection::Picture::Later { key, width, height })
    }
}

/// Collapse `.`, `..` and empty segments; a path climbing out of the root
/// is not a member.
pub(crate) fn normalize(path: &str) -> Option<String> {
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

/// `target` named from the part in folder `base` (`word`): relative to it,
/// or from the package's root when it starts with `/`.
pub(crate) fn resolve(base: &str, target: &str) -> Option<String> {
    if let Some(absolute) = target.strip_prefix('/') {
        normalize(absolute)
    } else if base.is_empty() {
        normalize(target)
    } else {
        normalize(&format!("{base}/{target}"))
    }
}

/// The XML of a part, parsed within the node limit every document shares.
pub(crate) fn xml(text: &str) -> anyhow::Result<roxmltree::Document<'_>> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: NODES,
        ..roxmltree::ParsingOptions::default()
    };
    Ok(roxmltree::Document::parse_with_options(text, options)?)
}

/// Nodes one part may have: a long report is a few hundred thousand.
const NODES: u32 = 1 << 21;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_never_leave_the_package() {
        assert_eq!(
            resolve("word", "media/a.png").as_deref(),
            Some("word/media/a.png")
        );
        assert_eq!(
            resolve("word", "/word/media/a.png").as_deref(),
            Some("word/media/a.png")
        );
        assert_eq!(
            resolve("word", "../docProps/t.jpeg").as_deref(),
            Some("docProps/t.jpeg")
        );
        assert_eq!(resolve("word", "../../etc/passwd"), None);
        assert_eq!(
            resolve("", "Pictures/x.png").as_deref(),
            Some("Pictures/x.png")
        );
    }
}
