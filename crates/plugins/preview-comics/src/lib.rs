//! CBZ/CBR comic books in the document reader, their pages read through
//! libarchive one at a time, without extracting the archive; and their
//! covers as thumbnails.
use skwir_core::{
    App, CancelToken,
    document::{DocumentInfo, DocumentPage, DocumentSession, PageContent, PixelOrder, SearchHit},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
};
use skwir_platform::{archive, inspection, mime, thumb_providers};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-comics"
    }
    fn title(&self) -> &'static str {
        "Comics"
    }
    fn description(&self) -> &'static str {
        "Read CBZ and CBR comic books, continuously or a page at a time, and show their covers"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![
            archive::WORKER
                .requirement("Reading the pages of a comic book")
                .required(),
        ]
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
        app.thumbnails.register(self.id(), Arc::new(Cover));
    }
}
/// Comic book archives, by type.
static COMICS: mime::Types = mime::Types::new(&[
    "application/vnd.comicbook+zip",
    "application/vnd.comicbook-rar",
    "application/x-cb7",
    "application/x-cbt",
]);
/// A comic's pictures in reading order.
fn pages(path: &std::path::Path, cancel: &CancelToken) -> anyhow::Result<Vec<archive::Member>> {
    let mut pages: Vec<_> = archive::list(path, cancel)?
        .members
        .iter()
        .filter(|member| is_page(member))
        .cloned()
        .collect();
    pages.sort_by_cached_key(|m| skwir_core::collate::sort_key(m.name.as_ref()));
    anyhow::ensure!(!pages.is_empty(), "Comic contains no supported images");
    Ok(pages)
}
/// A comic's cover, its first page, for the grid. A fallback: a thumbnailer
/// the desktop installed for comics comes first.
struct Cover;
impl ThumbnailProvider for Cover {
    fn id(&self) -> &'static str {
        "comic-covers"
    }
    fn stage(&self) -> Stage {
        Stage::Fallback
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        COMICS.holds(&req.entry.path, req.mime.as_deref())
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = req.entry.resolve()?;
        skwir_platform::thumb_providers::ensure_file(&path)?;
        let first = pages(&path, cancel)?.swap_remove(0);
        let cover = inspection::picture(&archive::read(&path, &first, cancel)?, req.size.pixels())?;
        Ok(Some(Image {
            width: cover.width,
            height: cover.height,
            rgba: cover.rgba.into(),
        }))
    }
}
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-comics"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        COMICS.holds(&req.entry.path, req.mime.as_deref())
    }
    /// A comic is a document of pages: read continuously or a page at a
    /// time in the reader, each page decoded at the side it is shown at.
    fn open_document(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let path = req.entry.resolve()?;
        let pages = pages(&path, lifetime)?;
        let count = u32::try_from(pages.len())?;
        Ok(Some(Arc::new(Comic {
            path,
            pages,
            read: std::sync::Mutex::default(),
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
    /// The pages read last, as the archive holds them, within `KEPT_BYTES`:
    /// drawn sharper once the reader settles, or asked for beside the one
    /// shown, a page was read from the archive again, which for a solid
    /// one decompresses all the pages before it.
    read: std::sync::Mutex<std::collections::VecDeque<(u32, Arc<Vec<u8>>)>>,
    info: DocumentInfo,
}

/// The bytes of pages a comic keeps read.
const KEPT_BYTES: usize = 64 << 20;

impl Comic {
    /// Page `number` as the archive holds it: kept, or read and kept.
    fn bytes(&self, number: u32, cancel: &CancelToken) -> anyhow::Result<Arc<Vec<u8>>> {
        let kept = || {
            self.read
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        };
        if let Some((_, bytes)) = kept().iter().find(|(page, _)| *page == number) {
            return Ok(Arc::clone(bytes));
        }
        let member = &self.pages[number as usize - 1];
        let bytes = Arc::new(archive::read(&self.path, member, cancel)?);
        let mut read = kept();
        read.push_back((number, Arc::clone(&bytes)));
        while read.iter().map(|(_, b)| b.len()).sum::<usize>() > KEPT_BYTES && read.len() > 1 {
            read.pop_front();
        }
        Ok(bytes)
    }
}

impl DocumentSession for Comic {
    fn info(&self) -> &DocumentInfo {
        &self.info
    }

    /// A page decoded with at most `side` pixels on its longer side; its own
    /// size is what 100% is.
    fn render(&self, page: u32, side: u32, cancel: &CancelToken) -> anyhow::Result<DocumentPage> {
        let number = page.clamp(1, self.info.pages);
        let decoded = inspection::picture(&self.bytes(number, cancel)?, side)?;
        Ok(DocumentPage {
            width: decoded.width,
            height: decoded.height,
            pixels: decoded.rgba,
            order: PixelOrder::Rgba,
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
        // A picture the decoder reads, as a thumbnail's is.
        && std::path::Path::new(&member.name)
            .file_name()
            .and_then(mime::by_name)
            .is_some_and(|mime| thumb_providers::decodes(mime.as_str()))
}
#[cfg(test)]
// Helpers of the tests that run on Linux alone (ADR-035).
#[cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]
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

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
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
            // Any picture the decoder reads, as a thumbnail's.
            member(8, "Book/page3.tif", false),
        ]
        .into_iter()
        .filter(is_page)
        .collect();
        pages.sort_by_cached_key(|m| skwir_core::collate::sort_key(m.name.as_ref()));
        let names: Vec<_> = pages.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Book/page1.png",
                "Book/page2.png",
                "Book/page3.tif",
                "Book/page10.png"
            ]
        );
    }
}
