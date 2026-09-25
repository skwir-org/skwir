//! CBZ/CBR comic books in the document reader, their pages read through
//! libarchive one at a time, without extracting the archive.
use bibli_core::{
    App, CancelToken,
    document::{DocumentInfo, DocumentPage, DocumentSession, PageContent, SearchHit},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::{archive, inspection};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-comics"
    }
    fn title(&self) -> &'static str {
        "Preview: comics"
    }
    fn description(&self) -> &'static str {
        "Read CBZ and CBR comic books, continuously or a page at a time"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        vec![
            bibli_core::plugin::Requirement::new(
                "bibli-archive-worker",
                "Reading the pages of a comic book",
                || archive::worker_path().is_some(),
            )
            .required(),
        ]
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-comics"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        matches!(
            inspection::extension(&req.entry.path).as_str(),
            "cbz" | "cbr" | "cb7" | "cbt"
        )
    }
    /// A comic is a document of pages: read continuously or a page at a
    /// time in the reader, each page decoded at the side it is shown at.
    fn open_document(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let path = req.entry.resolve()?;
        let mut pages: Vec<_> = archive::list(&path, lifetime)?
            .members
            .into_iter()
            .filter(is_page)
            .collect();
        pages.sort_by_cached_key(|m| bibli_core::collate::sort_key(m.name.as_ref()));
        anyhow::ensure!(!pages.is_empty(), "Comic contains no supported images");
        let count = u32::try_from(pages.len())?;
        Ok(Some(Arc::new(Comic {
            path,
            pages,
            info: DocumentInfo {
                pages: count,
                outlines: Vec::new(),
                reflowable: false,
                labels: Vec::new(),
            },
        })))
    }
}

/// A comic's pages, read from the archive when the reader asks for one.
struct Comic {
    path: std::path::PathBuf,
    pages: Vec<archive::Member>,
    info: DocumentInfo,
}

impl DocumentSession for Comic {
    fn info(&self) -> &DocumentInfo {
        &self.info
    }

    /// A page decoded with at most `side` pixels on its longer side; its own
    /// size is what 100% is.
    fn render(&self, page: u32, side: u32, cancel: &CancelToken) -> anyhow::Result<DocumentPage> {
        let number = page.clamp(1, self.info.pages);
        let member = &self.pages[number as usize - 1];
        let decoded = inspection::picture(&archive::read(&self.path, member, cancel)?, side)?;
        Ok(DocumentPage {
            width: decoded.width,
            height: decoded.height,
            rgba: decoded.rgba,
            content: Arc::new(PageContent {
                page: number,
                width: decoded.own.0 as f32,
                height: decoded.own.1 as f32,
                chars: Vec::new(),
                links: Vec::new(),
            }),
        })
    }

    /// Pictures have no text to find.
    fn search(&self, _query: &str, _cancel: &CancelToken) -> anyhow::Result<Vec<SearchHit>> {
        Ok(Vec::new())
    }
}

/// A picture the book shows: a regular, readable image, and not something a
/// tool left beside the pages. Archives made on a Mac carry an `AppleDouble`
/// twin of every page (`__MACOSX/._page01.jpg`) that sorts first and is no
/// picture at all; other dotfiles are thumbnails and caches.
fn is_page(member: &archive::Member) -> bool {
    member.regular
        && !member.encrypted
        && !member
            .name
            .split('/')
            .any(|part| part.starts_with('.') || part == "__MACOSX")
        && matches!(
            inspection::extension(std::path::Path::new(&member.name)).as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
        )
}
#[cfg(test)]
mod tests {
    use super::*;

    fn member(index: u64, name: &str, encrypted: bool) -> archive::Member {
        archive::Member {
            index,
            name: name.into(),
            size: 1,
            directory: false,
            regular: true,
            encrypted,
        }
    }

    #[test]
    fn pages_sort_numerically_without_what_tools_left_beside_them() {
        let mut pages: Vec<_> = [
            member(0, "__MACOSX/._page01.jpg", false),
            member(1, "Book/page10.png", false),
            member(2, "Book/page2.png", false),
            member(3, "Book/.thumbs/page1.png", false),
            member(4, "Book/._page1.png", false),
            member(5, "Book/page1.png", false),
            member(6, "Book/secret.png", true),
            member(7, "Book/notes.txt", false),
        ]
        .into_iter()
        .filter(is_page)
        .collect();
        pages.sort_by_cached_key(|m| bibli_core::collate::sort_key(m.name.as_ref()));
        let names: Vec<_> = pages.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["Book/page1.png", "Book/page2.png", "Book/page10.png"]
        );
    }
}
