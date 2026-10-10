//! A Word or `OpenDocument` file is a zip of parts, read as every package
//! is (`bibli_platform::package`: bounded, members by normalized name),
//! its part names compared without case as Word's are.
use bibli_core::CancelToken;
use bibli_platform::inspection::{self, limits};
pub(crate) use bibli_platform::package::{is_zip, normalize};
use std::path::Path;

pub(crate) struct Package(bibli_platform::package::Package);

impl Package {
    pub(crate) fn open(path: &Path, cancel: &CancelToken) -> anyhow::Result<Self> {
        bibli_platform::package::Package::open(path, "The document", true, cancel).map(Self)
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.0.has(name)
    }

    /// Member `name`, read whole within `limits::BINARY_INPUT`.
    pub(crate) fn read(
        &mut self,
        name: &str,
        what: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<u8>> {
        self.0.read(name, limits::BINARY_INPUT, what, cancel)
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

    /// A picture the document names by member in a part not shown: in
    /// its place, unread. Each turn to another part read every picture of
    /// the document for its size, a 256 KB head each, a drawing whole.
    pub(crate) fn unread_picture(name: &str) -> Option<bibli_core::inspection::Picture> {
        Some(bibli_core::inspection::Picture::Later {
            key: normalize(name)?,
            width: 1,
            height: 1,
        })
    }

    /// A picture the document names by member, sized for later from its
    /// header (`Picture::Later`, read again by `picture`).
    pub(crate) fn picture(
        &mut self,
        name: &str,
        cancel: &CancelToken,
    ) -> Option<bibli_core::inspection::Picture> {
        let key = normalize(name)?;
        let head = self.0.head(&key, limits::PICTURE_HEADER)?;
        let (width, height) = inspection::picture_size(&head).or_else(|| {
            let whole = self.read(&key, "Embedded image", cancel).ok()?;
            inspection::picture_size(&whole)
        })?;
        Some(bibli_core::inspection::Picture::Later { key, width, height })
    }
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
    inspection::xml_depth(text.as_bytes())?;
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
