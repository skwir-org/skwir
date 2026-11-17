//! A Word or `OpenDocument` file is a zip of parts, read as every package
//! is (`skwir_platform::package`: bounded, members by normalized name),
//! its part names compared without case as Word's are.
use skwir_core::CancelToken;
use skwir_core::preview::FileSource;
use skwir_core::words;
use skwir_core::words::Words;
use skwir_platform::inspection::{self, limits};
pub(crate) use skwir_platform::package::{is_zip, normalize};

pub(crate) struct Package(skwir_platform::package::Package);

/// What a read of a member is for: what its refusals say is missing, or
/// larger than is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Picture,
    Relationships,
    Thumbnail,
    /// The document's own text (`word/document.xml`, `content.xml`).
    Text,
    Styles,
    Numbering,
    Notes,
    Properties,
    Manifest,
}

impl Part {
    /// The refusal of a member the package does not hold.
    fn missing(self) -> Words {
        match self {
            Part::Picture => words!(
                documents_missing_picture = "An embedded image is missing",
                note = "Why a picture of a Word or OpenDocument file is not shown: the file names one it does not hold."
            ),
            Part::Relationships => words!(
                documents_missing_relationships = "The document's relationships are missing",
                note = "Why a Word file is not shown: the part that says where its other parts are (its relationships, _rels/*.rels) is not there."
            ),
            Part::Thumbnail => words!(
                documents_missing_thumbnail = "The thumbnail is missing",
                note = "Why a document's stored picture is not shown: the picture it names is not there."
            ),
            Part::Text => words!(
                documents_missing_text = "The document's text is missing",
                note = "Why a Word or OpenDocument file is not shown: the part holding its text is not there."
            ),
            Part::Styles => words!(
                documents_missing_styles = "The document's styles are missing",
                note = "Why a Word or OpenDocument file is not shown: the part holding its styles is not there."
            ),
            Part::Numbering => words!(
                documents_missing_numbering = "The document's numbering is missing",
                note = "Why a Word file is not shown: the part saying how its lists are numbered is not there."
            ),
            Part::Notes => words!(
                documents_missing_notes = "The document's notes are missing",
                note = "Why a Word file's footnotes or endnotes are not shown: the part holding them is not there."
            ),
            Part::Properties => words!(
                documents_missing_properties = "The document's properties are missing",
                note = "Why what a document says of itself (its title, author) is not shown: the part holding it is not there."
            ),
            Part::Manifest => words!(
                documents_missing_manifest = "The document's manifest is missing",
                note = "Why an OpenDocument file is not shown: the list of its parts (META-INF/manifest.xml) is not there."
            ),
        }
    }

    /// The refusal of a member that inflates past `limits::BINARY_INPUT`.
    fn too_large(self) -> Words {
        let size = words::Size(limits::BINARY_INPUT as u64);
        match self {
            Part::Picture => words!(
                documents_too_large_picture =
                    "An embedded image exceeds the { $size } preview limit",
                size = size,
                note = "Why a picture of a Word or OpenDocument file is not shown: it unpacks past what the preview reads. size: that bound (16 MB)."
            ),
            Part::Relationships => words!(
                documents_too_large_relationships =
                    "The document's relationships exceed the { $size } preview limit",
                size = size,
                note = "Why a Word file is not shown: the part that says where its other parts are (its relationships) unpacks past what the preview reads. size: that bound (16 MB)."
            ),
            Part::Thumbnail => words!(
                documents_too_large_thumbnail = "The thumbnail exceeds the { $size } preview limit",
                size = size,
                note = "Why a document's stored picture is not shown: it unpacks past what the preview reads. size: that bound (16 MB)."
            ),
            Part::Text => words!(
                documents_too_large_text =
                    "The document's text exceeds the { $size } preview limit",
                size = size,
                note = "Why a Word or OpenDocument file is not shown: the part holding its text unpacks past what the preview reads. size: that bound (16 MB)."
            ),
            Part::Styles => words!(
                documents_too_large_styles =
                    "The document's styles exceed the { $size } preview limit",
                size = size,
                note = "Why a Word or OpenDocument file is not shown: the part holding its styles unpacks past what the preview reads. size: that bound (16 MB)."
            ),
            Part::Numbering => words!(
                documents_too_large_numbering =
                    "The document's numbering exceeds the { $size } preview limit",
                size = size,
                note = "Why a Word file is not shown: the part saying how its lists are numbered unpacks past what the preview reads. size: that bound (16 MB)."
            ),
            Part::Notes => words!(
                documents_too_large_notes =
                    "The document's notes exceed the { $size } preview limit",
                size = size,
                note = "Why a Word file's footnotes or endnotes are not shown: the part holding them unpacks past what the preview reads. size: that bound (16 MB)."
            ),
            Part::Properties => words!(
                documents_too_large_properties =
                    "The document's properties exceed the { $size } preview limit",
                size = size,
                note = "Why what a document says of itself (its title, author) is not shown: the part holding it unpacks past what the preview reads. size: that bound (16 MB)."
            ),
            Part::Manifest => words!(
                documents_too_large_manifest =
                    "The document's manifest exceeds the { $size } preview limit",
                size = size,
                note = "Why an OpenDocument file is not shown: the list of its parts (META-INF/manifest.xml) unpacks past what the preview reads. size: that bound (16 MB)."
            ),
        }
    }
}

impl Package {
    /// The package `file`: what a preview holds of it, or its path (a
    /// thumbnail's).
    pub(crate) fn open<S: FileSource + ?Sized>(
        file: &S,
        cancel: &CancelToken,
    ) -> anyhow::Result<Self> {
        let too_many = words!(
            documents_members = "{ $count ->
               *[other] The document holds more than { $count } members
            }",
            count = limits::CONTAINER_MEMBERS,
            note = "Why a Word or OpenDocument file is not shown: its package holds more parts than the preview reads (4,000)."
        );
        skwir_platform::package::Package::open(file, too_many, true, cancel).map(Self)
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.0.has(name)
    }

    /// Member `name`, read whole within `limits::BINARY_INPUT`, for `what`.
    pub(crate) fn read(
        &mut self,
        name: &str,
        what: Part,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<u8>> {
        self.0.read(
            name,
            limits::BINARY_INPUT,
            what.missing(),
            what.too_large(),
            cancel,
        )
    }

    /// Member `name` as text, if it is there: a part that is missing is
    /// one the document does not use.
    pub(crate) fn text(
        &mut self,
        name: &str,
        what: Part,
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
    pub(crate) fn unread_picture(name: &str) -> Option<skwir_core::inspection::Picture> {
        Some(skwir_core::inspection::Picture::Later {
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
    ) -> Option<skwir_core::inspection::Picture> {
        let key = normalize(name)?;
        let head = self.0.head(&key, limits::PICTURE_HEADER)?;
        let (width, height) = inspection::picture_size(&head).or_else(|| {
            let whole = self.read(&key, Part::Picture, cancel).ok()?;
            inspection::picture_size(&whole)
        })?;
        Some(skwir_core::inspection::Picture::Later { key, width, height })
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
