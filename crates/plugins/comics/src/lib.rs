//! CBZ/CBR comic books in the document reader, their pages read through
//! libarchive one at a time, without extracting the archive; and their
//! covers as thumbnails.
use skwir_core::{
    App, CancelToken,
    direction::Direction,
    document::{
        DocumentInfo, DocumentPage, DocumentSession, PageContent, Part, PixelOrder, SearchHit,
    },
    plugin::Plugin,
    preview::{Held, PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
    words,
};
use skwir_platform::{archive, inspection, mime, thumb_providers};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "comics"
    }
    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(comics_plugin_title = "Comics")
    }
    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            comics_plugin_description = "Read CBZ and CBR comic books, continuously or a page at a time, and show their covers"
        )
    }
    fn for_viewer(&self) -> bool {
        true
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![
            archive::WORKER
                .requirement(words!(
                    comics_requirement_pages = "Reading the pages of a comic book",
                    note = "What stops working without the archive helper, in the settings' list of what a plugin needs."
                ))
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
/// A comic's pictures in reading order, of the archive's `listing`.
fn pages(listing: &archive::Listing) -> anyhow::Result<Vec<archive::Member>> {
    let mut pages: Vec<_> = listing
        .members
        .iter()
        .filter(|member| is_page(member))
        .cloned()
        .collect();
    pages.sort_by_cached_key(|m| skwir_core::collate::sort_key(m.name.as_ref()));
    anyhow::ensure!(
        !pages.is_empty(),
        words!(
            comics_no_pages = "Comic contains no supported images",
            note = "An error: the comic book's archive holds no picture the reader can show."
        )
    );
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
        // One run of the helper lists the pages and reads the first, and
        // ends with the session.
        let session = archive::Session::open(&path, cancel)?;
        let listing = session.list(inspection::limits::Bounds::QUICK, cancel)?;
        let first = pages(&listing)?.swap_remove(0);
        let bytes = session.read_one(&first, inspection::limits::Bounds::QUICK, cancel)?;
        let cover = inspection::picture(&bytes, req.size.pixels())?;
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
        "comics"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        COMICS.holds(&req.entry.path, req.mime.as_deref())
    }
    /// A comic is a document of pages: read continuously or a page at a
    /// time in the reader, each page decoded at the side it is shown at.
    /// libarchive's helper reads it by its path: the one what is held gives
    /// a program (a copy of a remote's file held whole). One helper, kept
    /// for as long as the reader shows it (`lifetime`), lists the pages and
    /// reads them (`archive::Session`). What the book says of itself
    /// (`ComicInfo.xml`: the way it reads) is read with the first pages
    /// the reader asks for, in one pass over the archive, those pages
    /// kept for it.
    fn open_document(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let held = req.held()?;
        let path = held.path_for_program(lifetime)?.into_owned();
        let session = archive::Session::open(&path, lifetime)?;
        let listing = session.list(inspection::limits::Bounds::QUICK, lifetime)?;
        let pages = pages(&listing)?;
        let count = u32::try_from(pages.len())?;
        let mut comic = Comic {
            session,
            _held: held,
            pages,
            read: std::sync::Mutex::default(),
            info: DocumentInfo {
                pages: count,
                outlines: Vec::new(),
                reflowable: false,
                labels: Vec::new(),
                direction: None,
            },
        };
        if let Some(info) = comic_info(&listing.members) {
            // A page that cannot be read is the reader's to say, when it
            // asks for it.
            let (_, said) = comic.bytes_with(req.page.clamp(1, count), Some(info), lifetime);
            comic.info.direction = said.as_deref().and_then(declared);
        }
        Ok(Some(Arc::new(comic)))
    }
}

/// A comic's pages, read from the archive when the reader asks for one.
struct Comic {
    /// The archive's helper, kept while the reader shows it: a page after
    /// the last read is read on from it.
    session: archive::Session,
    /// What the preview holds of it: the copy the session reads lives as
    /// long.
    _held: Arc<Held>,
    pages: Vec<archive::Member>,
    /// The pages read last, as the archive holds them, within `KEPT_BYTES`:
    /// drawn sharper once the reader settles, or asked for beside the one
    /// shown, a page was read from the archive again, which for a solid
    /// one, read again from its start, decompresses all the pages before.
    read: std::sync::Mutex<std::collections::VecDeque<(u32, Arc<Vec<u8>>)>>,
    info: DocumentInfo,
}

/// The pages read with the one asked for, after it in the book: the
/// reader asks for them next.
const AHEAD: u32 = 2;

/// The bytes of pages a comic keeps read.
const KEPT_BYTES: usize = 64 << 20;

impl Comic {
    /// Page `number` as the archive holds it: kept, or read and kept, with
    /// the `AHEAD` pages after it not kept yet, in one pass over the
    /// archive in its own order (`archive::Session::read`).
    fn bytes(&self, number: u32, cancel: &CancelToken) -> anyhow::Result<Arc<Vec<u8>>> {
        self.bytes_with(number, None, cancel).0
    }

    /// [`Comic::bytes`], and `beside`, a member that is no page (the
    /// book's `ComicInfo.xml`), read in the same pass: its bytes, when
    /// they were read before the pass ended.
    fn bytes_with(
        &self,
        number: u32,
        beside: Option<&archive::Member>,
        cancel: &CancelToken,
    ) -> (anyhow::Result<Arc<Vec<u8>>>, Option<Vec<u8>>) {
        let kept = || {
            self.read
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        };
        if beside.is_none()
            && let Some((_, bytes)) = kept().iter().find(|(page, _)| *page == number)
        {
            return (Ok(Arc::clone(bytes)), None);
        }
        let wanted: Vec<archive::Member> = {
            let read = kept();
            (number..=number.saturating_add(AHEAD).min(self.info.pages))
                .filter(|&page| page == number || !read.iter().any(|(kept, _)| *kept == page))
                .map(|page| self.pages[page as usize - 1].clone())
                .chain(beside.cloned())
                .collect()
        };
        let mut asked = None;
        let mut told = None;
        let done = self.session.read(
            &wanted,
            inspection::limits::Bounds::QUICK,
            cancel,
            |member, bytes| {
                if beside.is_some_and(|beside| beside.index == member.index) {
                    told = Some(bytes);
                    return;
                }
                let Some(page) = self.pages.iter().position(|p| p.index == member.index) else {
                    return;
                };
                let (page, bytes) = (page as u32 + 1, Arc::new(bytes));
                if page == number {
                    asked = Some(Arc::clone(&bytes));
                }
                let mut read = kept();
                read.push_back((page, bytes));
                while read.iter().map(|(_, b)| b.len()).sum::<usize>() > KEPT_BYTES
                    && read.len() > 1
                {
                    read.pop_front();
                }
            },
        );
        // A page read is shown though one after it failed.
        let page = match (asked, done) {
            (Some(bytes), _) => Ok(bytes),
            (None, Err(error)) => Err(error),
            (None, Ok(())) => Err(anyhow::anyhow!(words!(
                comics_page_not_read = "the page was not read",
                note = "An error in place of a comic book's page: the archive was read to its end without finding it."
            ))),
        };
        (page, told)
    }
}

/// The most a book's `ComicInfo.xml` is read to: more than any tool
/// writes.
const INFO_BYTES: i64 = 1 << 20;

/// What the book says of itself, as the tools that tag comics write it
/// (`ComicRack`'s `ComicInfo.xml`): the one nearest the archive's root,
/// never one a tool left beside the pages.
fn comic_info(members: &[archive::Member]) -> Option<&archive::Member> {
    members
        .iter()
        .filter(|member| {
            member.regular
                && !member.encrypted
                && member.size <= INFO_BYTES
                && !left_beside(&member.name)
                && member.file_name().eq_ignore_ascii_case("ComicInfo.xml")
        })
        .min_by_key(|member| member.name.matches('/').count())
}

/// The way a comic says it reads, in its `ComicInfo.xml` (`info`): its
/// `Manga` is `YesAndRightToLeft`. `Yes` (a manga, its direction unsaid),
/// `No`, `Unknown`, nothing, and a text that is not one, say nothing.
fn declared(info: &[u8]) -> Option<Direction> {
    let text = inspection::decode(info).0;
    let doc = inspection::xml_document(&text).ok()?;
    let root = doc.root_element();
    if !root.has_tag_name("ComicInfo") {
        return None;
    }
    root.children()
        .find(|node| node.has_tag_name("Manga"))?
        .text()?
        .trim()
        .eq_ignore_ascii_case("YesAndRightToLeft")
        .then_some(Direction::RightToLeft)
}

impl DocumentSession for Comic {
    fn info(&self) -> &DocumentInfo {
        &self.info
    }

    /// A picture's region decodes alone (`render_part`).
    fn draws_parts(&self) -> bool {
        true
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
            part: None,
        })
    }

    /// A part of a page past what is held of it whole (ADR-147): only that
    /// region of its picture, decoded at the side asked
    /// (`picture::decode_region`), never the whole shrunk and cut. A
    /// drawing's page has no part decoded alone, and its whole cut would
    /// be no sharper than the whole shown: it is refused, as a picture
    /// that cannot be read is, and the reader asks it no more.
    fn render_part(
        &self,
        page: u32,
        side: u32,
        part: Part,
        cancel: &CancelToken,
    ) -> anyhow::Result<DocumentPage> {
        let number = page.clamp(1, self.info.pages);
        let bytes = self.bytes(number, cancel)?;
        let (decoded, drawn) = inspection::picture_part(&bytes, part, side)?;
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
            part: Some(drawn),
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
        && !left_beside(&member.name)
        // A picture the decoder reads, as a thumbnail's is (on a worker: a
        // HEIC page once the HEIF helper's probe has answered).
        && std::path::Path::new(&member.name)
            .file_name()
            .and_then(mime::by_name)
            .is_some_and(|mime| thumb_providers::decodes_on_worker(mime.as_str()))
}
/// Whether the member `name` is what a tool left beside a book: in a
/// Mac's `__MACOSX`, or hidden (an `AppleDouble` twin, a thumbnail, a
/// cache).
fn left_beside(name: &str) -> bool {
    name.split('/')
        .any(|part| part.starts_with('.') || part == "__MACOSX")
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
            ..Default::default()
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

    /// A 2 × 3 picture.
    const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x02\x00\x00\x00\x03\x08\x06\x00\x00\x00\xb9\xea\xde\x81\x00\x00\x00\x11\x49\x44\x41\x54\x78\x9c\x63\xf8\xcf\xc0\xf0\x1f\x84\x19\x30\x18\x00\xa1\x79\x0b\xf5\x4d\xc4\x9a\x07\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

    /// A tar of `members`, as a `.cbt` comic is: each a header of 512
    /// bytes (its name, mode and size in octal, the header's checksum),
    /// its bytes padded to 512, two empty blocks at the end.
    fn tar(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, bytes) in members {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[100..107].copy_from_slice(b"0000644");
            header[108..115].copy_from_slice(b"0000000");
            header[116..123].copy_from_slice(b"0000000");
            header[124..135].copy_from_slice(format!("{:011o}", bytes.len()).as_bytes());
            header[136..147].copy_from_slice(b"00000000000");
            // Counted as spaces, then written over.
            header[148..156].copy_from_slice(b"        ");
            header[156] = b'0';
            header[257..263].copy_from_slice(b"ustar\0");
            header[263..265].copy_from_slice(b"00");
            let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
            header[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
            out.extend_from_slice(&header);
            out.extend_from_slice(bytes);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.resize(out.len() + 1024, 0);
        out
    }

    /// A preview reads its file once (ADR-101): a remote's comic held
    /// whole is handed to libarchive's helper as a copy written from what
    /// is held, which lives as long as the reader; its own path, where no
    /// file ever is, is never read. Needs the helper built in the target
    /// folder the test runs from; without it the test says so and passes,
    /// unless `SKWIR_REQUIRE_WORKERS` is set.
    /// The helper built beside the tests, named for the plugin: false, said,
    /// where it is not built.
    #[cfg(target_os = "linux")]
    fn with_worker() -> bool {
        let built = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join(archive::WORKER.executable)))
            .filter(|path| path.is_file());
        let Some(worker) = std::env::var_os(archive::WORKER.variable)
            .map(std::path::PathBuf::from)
            .or(built)
        else {
            assert!(
                std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
                "skwir-archive-worker is not built"
            );
            eprintln!("skipped: skwir-archive-worker is not built");
            return false;
        };
        // SAFETY: nextest runs each test in a process of its own, and nothing
        // else reads the environment before this.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var(archive::WORKER.variable, &worker);
        }
        true
    }

    /// A page is read with the two after it, in one pass over the archive:
    /// turning to them reads nothing more, and the last page reads only
    /// what is left of the book.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_page_is_read_with_the_two_after_it() {
        if !with_worker() {
            return;
        }
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("comic-")
            .tempdir_in(base)
            .unwrap();
        let path = dir.path().join("l’été comic #2.cbt");
        let names: Vec<String> = (1..=5).map(|n| format!("Book/page{n}.png")).collect();
        let members: Vec<(&str, &[u8])> = names.iter().map(|name| (name.as_str(), PNG)).collect();
        std::fs::write(&path, tar(&members)).unwrap();
        let cancel = CancelToken::new();
        let session = archive::Session::open(&path, &cancel).unwrap();
        let listing = session
            .list(inspection::limits::Bounds::QUICK, &cancel)
            .unwrap();
        let pages = pages(&listing).unwrap();
        let comic = Comic {
            session,
            _held: Arc::new(Held::of_bytes(&path, Vec::new())),
            info: DocumentInfo {
                pages: u32::try_from(pages.len()).unwrap(),
                outlines: Vec::new(),
                reflowable: false,
                labels: Vec::new(),
                direction: None,
            },
            pages,
            read: std::sync::Mutex::default(),
        };
        let kept = |comic: &Comic| {
            let mut kept: Vec<u32> = comic
                .read
                .lock()
                .unwrap()
                .iter()
                .map(|(page, _)| *page)
                .collect();
            kept.sort_unstable();
            kept
        };
        assert_eq!(comic.render(1, 64, &cancel).unwrap().content.page, 1);
        assert_eq!(kept(&comic), [1, 2, 3]);
        comic.render(2, 64, &cancel).unwrap();
        assert_eq!(kept(&comic), [1, 2, 3], "read already");
        comic.render(5, 64, &cancel).unwrap();
        assert_eq!(kept(&comic), [1, 2, 3, 5], "the last, alone");
        comic.render(4, 64, &cancel).unwrap();
        assert_eq!(kept(&comic), [1, 2, 3, 4, 5]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_remote_comic_held_is_read_from_its_copy() {
        use skwir_core::preview::HeldSlot;
        if !with_worker() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let nowhere = dir.path().join("gone").join("l’été comic #1.cbt");
        let scratch = dir.path().join("scratch");
        let bytes = tar(&[("Book/page2.png", PNG), ("Book/page1.png", PNG)]);
        let held = Arc::new(Held::of_bytes_on_remote(
            &nowhere,
            bytes.clone(),
            scratch.clone(),
        ));
        let entry = skwir_core::op::EntryRef::new(skwir_core::FileId::new(0, 0), &nowhere);
        let request = PreviewRequest {
            entry: entry.clone(),
            mime: Some("application/x-cbt".into()),
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
            fit: None,
            held: HeldSlot::holding(entry, Arc::clone(&held)),
        };
        assert!(Provider.accepts(&request));
        let cancel = CancelToken::new();
        let comic = Provider
            .open_document(&request, &cancel)
            .unwrap()
            .expect("a comic");
        assert_eq!(comic.info().pages, 2);
        // The copy the helper was handed: in the preview's folder, what is
        // held, byte for byte.
        let copy = held.path_for_program(&cancel).unwrap().into_owned();
        assert!(copy.starts_with(&scratch), "{}", copy.display());
        assert_eq!(std::fs::read(&copy).unwrap(), bytes);
        // The request let go of, the reader reads a page it had not: from
        // the copy, which lives with it.
        drop((request, held));
        let page = comic.render(2, 64, &cancel).unwrap();
        assert_eq!((page.content.width, page.content.height), (2., 3.));
        assert!(copy.is_file());
        assert!(!nowhere.parent().unwrap().exists(), "the path never was");
    }

    /// A book's `ComicInfo.xml` saying it reads as a manga does, right to
    /// left.
    const RIGHT_TO_LEFT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Title>Synthetic Pages</Title>
  <Manga>YesAndRightToLeft</Manga>
</ComicInfo>"#;

    /// `ComicInfo.xml`'s `Manga` read right to left only where it says so:
    /// a manga whose direction is unsaid, or a comic that is none, says
    /// nothing, and so does a text that is not a comic's information.
    #[test]
    fn a_comic_reads_right_to_left_only_where_it_says_so() {
        assert_eq!(
            declared(RIGHT_TO_LEFT.as_bytes()),
            Some(Direction::RightToLeft)
        );
        // Written with a byte order mark, as some tools do.
        let marked = [b"\xEF\xBB\xBF", RIGHT_TO_LEFT.as_bytes()].concat();
        assert_eq!(declared(&marked), Some(Direction::RightToLeft));
        for manga in ["Yes", "No", "Unknown", ""] {
            let said = RIGHT_TO_LEFT.replace("YesAndRightToLeft", manga);
            assert_eq!(declared(said.as_bytes()), None, "{manga:?}");
        }
        let unsaid = RIGHT_TO_LEFT.replace("<Manga>YesAndRightToLeft</Manga>", "");
        assert_eq!(declared(unsaid.as_bytes()), None);
        let other = RIGHT_TO_LEFT.replace("ComicInfo", "Other");
        assert_eq!(declared(other.as_bytes()), None, "not a comic's");
        assert_eq!(declared(b"<ComicInfo><Manga>"), None, "not well-formed");
    }

    /// The book's information is the `ComicInfo.xml` nearest its root,
    /// whatever its case, never a twin a tool left beside it, nor one
    /// larger than any tool writes.
    #[test]
    fn a_comic_s_information_is_the_one_nearest_its_root() {
        let info = |index: u64, name: &str, size: i64| archive::Member {
            size,
            ..member(index, name, false)
        };
        let members = [
            info(0, "__MACOSX/ComicInfo.xml", 10),
            info(1, "Book/._ComicInfo.xml", 10),
            info(2, "Book/Extras/ComicInfo.xml", 10),
            info(3, "Book/comicinfo.XML", 10),
            info(4, "Book/page1.png", 10),
        ];
        assert_eq!(comic_info(&members).map(|m| m.index), Some(3));
        let larger = [info(0, "ComicInfo.xml", INFO_BYTES + 1)];
        assert!(comic_info(&larger).is_none());
        let none = [info(0, "Book/page1.png", 10)];
        assert!(comic_info(&none).is_none());
    }

    /// CRC-32 (ISO 3309), as a zip stores it beside each member.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// A zip of `members`, stored as they are, as a `.cbz` comic is: each
    /// a local header and its bytes, then the central directory naming
    /// them, then its end.
    fn zip(members: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut directory) = (Vec::new(), Vec::new());
        let u16_of = |n: usize| u16::try_from(n).unwrap().to_le_bytes();
        let u32_of = |n: usize| u32::try_from(n).unwrap().to_le_bytes();
        for (name, bytes) in members {
            let offset = out.len();
            // Version 2.0, no flags, stored, 1980-01-01 00:00.
            let common = [
                &20u16.to_le_bytes()[..],
                &0u16.to_le_bytes(),
                &0u16.to_le_bytes(),
                &0u16.to_le_bytes(),
                &0x21u16.to_le_bytes(),
                &crc32(bytes).to_le_bytes(),
                &u32_of(bytes.len()),
                &u32_of(bytes.len()),
                &u16_of(name.len()),
                &0u16.to_le_bytes(),
            ]
            .concat();
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&common);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(bytes);
            directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            directory.extend_from_slice(&20u16.to_le_bytes());
            directory.extend_from_slice(&common);
            // No comment, disk 0, no attributes, where its header is.
            directory.extend_from_slice(&[0; 10]);
            directory.extend_from_slice(&u32_of(offset));
            directory.extend_from_slice(name.as_bytes());
        }
        let at = out.len();
        out.extend_from_slice(&directory);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&u16_of(members.len()));
        out.extend_from_slice(&u16_of(members.len()));
        out.extend_from_slice(&u32_of(directory.len()));
        out.extend_from_slice(&u32_of(at));
        out.extend_from_slice(&[0; 2]);
        out
    }

    /// A comic says it reads right to left in its `ComicInfo.xml`, read
    /// with the pages the reader asks for first, in one pass over the
    /// archive (a tagger writes it last); a manga whose direction is
    /// unsaid, and a comic with no information, say nothing. Needs the
    /// archive helper (`with_worker`).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_cbz_says_which_way_it_reads_in_its_comic_info() {
        use skwir_core::preview::HeldSlot;
        if !with_worker() {
            return;
        }
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("comic-")
            .tempdir_in(base)
            .unwrap();
        let unsaid = RIGHT_TO_LEFT.replace("YesAndRightToLeft", "Yes");
        let names: Vec<String> = (1..=5).map(|n| format!("Book/page{n}.png")).collect();
        for (name, info, direction) in [
            (
                "l’été manga.cbz",
                Some(RIGHT_TO_LEFT),
                Some(Direction::RightToLeft),
            ),
            ("l’été manga unsaid.cbz", Some(unsaid.as_str()), None),
            ("l’été comic.cbz", None, None),
        ] {
            let mut members: Vec<(&str, &[u8])> =
                names.iter().map(|name| (name.as_str(), PNG)).collect();
            members.extend(info.map(|info| ("ComicInfo.xml", info.as_bytes())));
            let path = dir.path().join(name);
            std::fs::write(&path, zip(&members)).unwrap();
            let entry = skwir_core::op::EntryRef::from_path(&path).unwrap();
            let request = PreviewRequest {
                entry,
                mime: Some("application/vnd.comicbook+zip".into()),
                page: 2,
                seconds: None,
                max_side: 64,
                text_size: None,
                fit: None,
                held: HeldSlot::default(),
            };
            assert!(Provider.accepts(&request), "{name}");
            let cancel = CancelToken::new();
            let comic = Provider
                .open_document(&request, &cancel)
                .unwrap()
                .expect("a comic");
            assert_eq!(comic.info().pages, 5, "{name}");
            assert_eq!(comic.info().direction, direction, "{name}");
            let page = comic.render(2, 64, &cancel).unwrap();
            assert_eq!((page.content.width, page.content.height), (2., 3.));
        }
    }

    /// What the book says of itself is read in the pass that reads the
    /// first page and the two after it, wherever it lies: those pages are
    /// kept, and turning to them reads nothing more.
    #[cfg(target_os = "linux")]
    #[test]
    fn comic_info_is_read_in_the_pass_of_the_first_pages() {
        if !with_worker() {
            return;
        }
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("comic-")
            .tempdir_in(base)
            .unwrap();
        let path = dir.path().join("l’été manga #3.cbz");
        let names: Vec<String> = (1..=5).map(|n| format!("Book/page{n}.png")).collect();
        let mut members: Vec<(&str, &[u8])> =
            names.iter().map(|name| (name.as_str(), PNG)).collect();
        members.push(("ComicInfo.xml", RIGHT_TO_LEFT.as_bytes()));
        std::fs::write(&path, zip(&members)).unwrap();
        let cancel = CancelToken::new();
        let session = archive::Session::open(&path, &cancel).unwrap();
        let listing = session
            .list(inspection::limits::Bounds::QUICK, &cancel)
            .unwrap();
        let pages = pages(&listing).unwrap();
        let info = comic_info(&listing.members).unwrap().clone();
        let comic = Comic {
            session,
            _held: Arc::new(Held::of_bytes(&path, Vec::new())),
            info: DocumentInfo {
                pages: u32::try_from(pages.len()).unwrap(),
                outlines: Vec::new(),
                reflowable: false,
                labels: Vec::new(),
                direction: None,
            },
            pages,
            read: std::sync::Mutex::default(),
        };
        let (page, said) = comic.bytes_with(1, Some(&info), &cancel);
        assert_eq!(page.unwrap().as_slice(), PNG);
        assert_eq!(said.as_deref(), Some(RIGHT_TO_LEFT.as_bytes()));
        let mut kept: Vec<u32> = comic
            .read
            .lock()
            .unwrap()
            .iter()
            .map(|(page, _)| *page)
            .collect();
        kept.sort_unstable();
        assert_eq!(kept, [1, 2, 3], "read with it, and kept");
    }
}
