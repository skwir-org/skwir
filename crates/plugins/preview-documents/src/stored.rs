//! The picture of its first page an Office or `OpenDocument` file keeps:
//! `OpenDocument`'s `Thumbnails/thumbnail.png`, and the thumbnail an Office
//! package's relationships name (`docProps/thumbnail.jpeg`), when it was
//! saved with one. A fallback: a desktop thumbnailer comes first, and this
//! comes before a converted document's first page, which needs a preview
//! to have been made.
use crate::package::{self, Package};
use bibli_core::CancelToken;
use bibli_core::thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider};
use bibli_platform::inspection;
use std::path::Path;

/// Where `OpenDocument` keeps its picture.
const OPEN_DOCUMENT: &str = "Thumbnails/thumbnail.png";

/// Where the package keeps its picture: `OpenDocument`'s place, else the
/// one an Office package's relationships name. The package says which it
/// is, not its name.
fn stored(package: &mut Package, cancel: &CancelToken) -> Option<String> {
    if package.has(OPEN_DOCUMENT) {
        return Some(OPEN_DOCUMENT.to_owned());
    }
    let rels = package
        .text("_rels/.rels", "Relationships", cancel)
        .ok()??;
    let doc = package::xml(&rels).ok()?;
    doc.root_element()
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
        .find(|rel| {
            rel.attribute("Type")
                .is_some_and(|t| t.ends_with("/metadata/thumbnail"))
                && rel.attribute("TargetMode") != Some("External")
        })
        .and_then(|rel| package::resolve("", rel.attribute("Target")?))
}

/// The picture `path` keeps of itself, at most `side` on its longer side;
/// `None` when it keeps none this process decodes (WMF, EMF).
pub(crate) fn picture(
    path: &Path,
    side: u32,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Image>> {
    if !package::is_zip(path) {
        return Ok(None);
    }
    let mut package = Package::open(path, cancel)?;
    let Some(name) = stored(&mut package, cancel) else {
        return Ok(None);
    };
    let bytes = package.read(&name, "Thumbnail", cancel)?;
    Ok(inspection::picture(&bytes, side).ok().map(|picture| Image {
        width: picture.width,
        height: picture.height,
        rgba: picture.rgba.into(),
    }))
}

pub(crate) struct StoredPictures;

impl ThumbnailProvider for StoredPictures {
    fn id(&self) -> &'static str {
        "office-stored-pictures"
    }
    fn stage(&self) -> Stage {
        Stage::Fallback
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        bibli_platform::preview::is_office(&req.entry.path, req.mime.as_deref())
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = req.entry.resolve()?;
        picture(&path, req.size.pixels(), cancel)
    }
}
