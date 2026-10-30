//! Books (ADR-106): EPUB 2/3, `FictionBook` 2 and Mobipocket (MOBI 6, KF8,
//! `PalmDOC` texts) read in reading mode, or laid out into pages by `MuPDF`;
//! and books' covers as thumbnails.
//!
//! The built-in engine reads every book: one reading path for all of them
//! (`book`), each format saying how its chapters and what they name are
//! read (`epub`, `mobi`). `MuPDF`, when chosen, reads the kinds of book it
//! reads well, told from the file before it opens.
use skwir_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection},
    plugin::Plugin,
    preview::{FileSource, Held, PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
};
use skwir_platform::{inspection, mime::Types};
use std::{path::Path, sync::Arc};

mod book;
mod epub;
mod fb2;
mod mobi;
mod pages;

const ID: &str = "books";

pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    /// The EPUB plugin's, before it read other books (ADR-106).
    fn former_ids(&self) -> &'static [&'static str] {
        &["epub"]
    }
    fn title(&self) -> &'static str {
        "Books"
    }
    fn description(&self) -> &'static str {
        "Read EPUB, FB2 and MOBI books with their table of contents"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    /// A new engine reads books from now on; covers are the same either way.
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key == BACKEND) {
            app.previews.unregister(self.id(), self.id());
            register_books(app);
        }
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        use skwir_platform::document::{Engine, worker};
        vec![worker(Engine::Mupdf).requirement("Books laid out into pages by MuPDF")]
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
    }
    fn register(&self, app: &App) {
        app.settings.rename("epub.backend", BACKEND);
        app.settings.declare(skwir_core::settings::SettingSpec::choice(
            BACKEND,
            "Book engine",
            "Automatic reads books with the built-in engine: text that reflows in the theme, fixed-layout pages drawn as their book lays them out. MuPDF lays books out into pages, with their own styles: every EPUB, FB2 and MOBI book it reads well; the built-in engine reads the others.",
            "auto",
            &[
                ("auto", "Automatic"),
                ("builtin", "Built-in"),
                ("mupdf", "MuPDF"),
            ],
        )
        .page(skwir_core::settings::page::READING)
        .section("Books"));
        register_books(app);
        app.thumbnails.register(self.id(), Arc::new(Cover));
    }
}

/// Which engine reads books.
const BACKEND: &str = "books.backend";

/// The books provider, with the engine the setting chooses.
fn register_books(app: &App) {
    let engine = match app.settings.get::<String>(BACKEND).as_deref() {
        Some("mupdf") => Engine::Mupdf,
        _ => Engine::BuiltIn,
    };
    app.previews.register(
        ID,
        Arc::new(Provider {
            engine,
            ..Provider::default()
        }),
    );
}

/// An EPUB, by type (AGENTS.md, "Types").
static EPUB: Types = Types::new(&["application/epub+zip"]);
/// A `FictionBook` 2 document, plain or zipped.
static FB2: Types = Types::new(&["application/x-fictionbook+xml"]);
static FB2_ZIP: Types = Types::new(&["application/x-zip-compressed-fb2"]);
/// A Mobipocket book: MOBI 6, KF8 (AZW3), and the KFX the database names
/// alike, refused when read.
static MOBI: Types = Types::new(&[
    "application/x-mobipocket-ebook",
    "application/vnd.amazon.mobi8-ebook",
]);
/// What a Palm database may be typed as, a book only when its type and
/// creator say so (`mobi::is_book`): `.pdb` is a Palm database, a `PalmDOC`
/// text (`AportisDoc`), a protein's structure and a Windows program
/// database alike, and the database types it by its heaviest globs (the
/// last two) before its bytes are read.
static PALM: Types = Types::new(&[
    "application/vnd.palm",
    "application/x-aportisdoc",
    "application/x-ms-pdb",
    "chemical/x-pdb",
]);

/// The format of a book, by its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Epub,
    Fb2,
    Fb2Zip,
    /// A Palm database: a Mobipocket book or a `PalmDOC` text.
    Palm,
}

impl Format {
    /// The format of the file asked about at `path` (typed `mime`), whose
    /// first bytes `file` gives when its type alone cannot say.
    fn of<S: FileSource>(
        path: &Path,
        mime: Option<&str>,
        file: impl FnOnce() -> Option<S>,
    ) -> Option<Self> {
        let mime = skwir_platform::mime::of_request(path, mime)?;
        Self::of_type(mime.as_str()).or_else(|| {
            let file = PALM.contains(mime.as_str()).then(file)??;
            mobi::is_book(&file.first_bytes().ok()?).then_some(Self::Palm)
        })
    }

    /// The format of a file its type alone calls a book, nothing read: a
    /// Palm database is not one.
    fn named(path: &Path, mime: Option<&str>) -> Option<Self> {
        Self::of_type(skwir_platform::mime::of_request(path, mime)?.as_str())
    }

    fn of_type(mime: &str) -> Option<Self> {
        if EPUB.contains(mime) {
            Some(Self::Epub)
        } else if FB2.contains(mime) {
            Some(Self::Fb2)
        } else if FB2_ZIP.contains(mime) {
            Some(Self::Fb2Zip)
        } else if MOBI.contains(mime) {
            Some(Self::Palm)
        } else {
            None
        }
    }

    /// The format of the book a preview asks about.
    fn of_request(req: &PreviewRequest) -> Option<Self> {
        Self::of(&req.entry.path, req.mime.as_deref(), || req.held().ok())
    }
}

/// A book's cover for the grid: the picture its book names, for a file its
/// type calls a book. The grid asks on its own thread before the file is
/// read, so a Palm database (`.pdb`), a book only when its bytes say so,
/// has none; its preview reads it. A fallback: a thumbnailer the desktop
/// installed for books comes first.
struct Cover;
impl ThumbnailProvider for Cover {
    fn id(&self) -> &'static str {
        "book-covers"
    }
    fn stage(&self) -> Stage {
        Stage::Fallback
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        Format::named(&req.entry.path, req.mime.as_deref()).is_some()
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let Some(format) = Format::named(&req.entry.path, req.mime.as_deref()) else {
            return Ok(None);
        };
        let path = req.entry.resolve()?;
        // Nothing for a cover the book encrypts, or names none.
        let cover = match format {
            Format::Epub => epub::cover_picture(&path, cancel)?,
            Format::Fb2 | Format::Fb2Zip => fb2::cover(&path, format == Format::Fb2Zip, cancel)?,
            Format::Palm => mobi::cover_picture(&path, cancel)?,
        };
        let Some(bytes) = cover else {
            return Ok(None);
        };
        let cover = inspection::picture(&bytes, req.size.pixels())?;
        Ok(Some(Image {
            width: cover.width,
            height: cover.height,
            rgba: cover.rgba.into(),
        }))
    }
}

/// Which engine reads a book (`books.backend`). Either one sizes
/// what is read with the reader's text size, and zooms what is looked at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Engine {
    /// Reading mode for reflowing books, drawn pages for fixed ones.
    #[default]
    BuiltIn,
    /// The books it reads laid out into pages by `MuPDF`, in the document
    /// reader; the built-in engine reads the others.
    Mupdf,
}
#[derive(Default)]
struct Provider {
    engine: Engine,
    /// The FB2 shown, read once.
    fb2: book::Kept<fb2::Fb2>,
    /// The Mobipocket book shown, its text decompressed once.
    mobi: book::Kept<mobi::Mobi>,
}

impl Provider {
    /// The FB2 the preview `req` shows, read once while it is shown.
    fn fb2(
        &self,
        req: &PreviewRequest,
        format: Format,
        cancel: &CancelToken,
    ) -> anyhow::Result<Arc<fb2::Fb2>> {
        let held = req.held()?;
        self.fb2.get(&held, req.shown(), || {
            fb2::Fb2::open(&held, format == Format::Fb2Zip, cancel)
        })
    }

    /// The Mobipocket book the preview `req` shows, read once while it is
    /// shown.
    fn mobi(&self, req: &PreviewRequest, cancel: &CancelToken) -> anyhow::Result<Arc<mobi::Mobi>> {
        let held = req.held()?;
        self.mobi
            .get(&held, req.shown(), || mobi::Mobi::open(&held, cancel))
    }

    /// Whether `MuPDF` reads the book `format` names in `file`: when it is
    /// the engine chosen, and reads that kind of book (ADR-106). Told
    /// before the book opens, never from `MuPDF` failing on it.
    fn mupdf_reads<S: FileSource + ?Sized>(
        &self,
        format: Format,
        file: &S,
        cancel: &CancelToken,
    ) -> bool {
        self.engine == Engine::Mupdf
            && match format {
                Format::Epub | Format::Fb2 => true,
                // It finds no document in a zipped FB2.
                Format::Fb2Zip => false,
                Format::Palm => mobi::mupdf_reads(file, cancel),
            }
    }
}

impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        ID
    }
    /// A book, by type: EPUB, FB2 and Mobipocket, whichever engine reads
    /// it.
    fn accepts(&self, req: &PreviewRequest) -> bool {
        Format::of_request(req).is_some()
    }
    /// A picture a chapter named, by its key: an EPUB's member, read
    /// through the same bounded reader and never outside the container; an
    /// FB2's binary; a Mobipocket book's record, after its text.
    fn picture(
        &self,
        req: &PreviewRequest,
        _view: &InspectRequest,
        key: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<skwir_core::thumbs::Image>> {
        let bytes = match Format::of_request(req) {
            Some(format @ (Format::Fb2 | Format::Fb2Zip)) => self
                .fb2(req, format, cancel)?
                .binary(key, skwir_platform::inspection::limits::BINARY_INPUT)?,
            Some(Format::Palm) => self.mobi(req, cancel)?.picture(
                &req.held()?,
                key,
                skwir_platform::inspection::limits::BINARY_INPUT,
                cancel,
            )?,
            _ => epub::picture(&req.held()?, key, cancel)?,
        };
        Ok(Some(inspection::image(&bytes)?))
    }
    /// A book of pages opens in the document reader: with `MuPDF`, every
    /// book it reads, laid out into pages at the reader's text size and
    /// again when it changes; with the built-in engine, a fixed-layout
    /// book, its pages drawn as it lays them out (`pages::FixedBook`), from
    /// what is held. `MuPDF`'s helper reads it by its path: the one what is
    /// held gives a program (a copy of a remote's file held whole).
    fn open_document(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn skwir_core::document::DocumentSession>>> {
        let held = req.held()?;
        let Some(format) = Format::of_request(req) else {
            return Ok(None);
        };
        if !self.mupdf_reads(format, &held, lifetime) {
            if format != Format::Epub {
                return Ok(None);
            }
            let book = pages::FixedBook::open(held, lifetime)?;
            return Ok(
                book.map(|book| Arc::new(book) as Arc<dyn skwir_core::document::DocumentSession>)
            );
        }
        let source = held.path_for_program(lifetime)?.into_owned();
        let session = skwir_platform::document::open_book(source, req.text_size, lifetime.clone())?;
        let Some(session) = session else {
            anyhow::bail!(
                "Reading books with MuPDF needs skwir-mupdf-worker; install it, or choose the built-in book engine"
            );
        };
        Ok(Some(Arc::new(Holding {
            session,
            _held: held,
        })))
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let held = req.held()?;
        let Some(format) = Format::of_request(req) else {
            return Ok(None);
        };
        // MuPDF's books are documents, opened by `open_document`.
        if self.mupdf_reads(format, &held, cancel) {
            return Ok(None);
        }
        match format {
            Format::Epub => {
                // A book of pages is a document (`open_document`).
                let Some(mut book) = epub::EpubBook::open(&held, cancel)? else {
                    return Ok(None);
                };
                book::inspect(&mut book, view, cancel).map(Some)
            }
            Format::Fb2 | Format::Fb2Zip => {
                let book = self.fb2(req, format, cancel)?;
                book::inspect(&mut fb2::Reading(&book), view, cancel).map(Some)
            }
            Format::Palm => {
                let book = self.mobi(req, cancel)?;
                let mut reading = mobi::Reading {
                    book: &book,
                    file: &held,
                };
                book::inspect(&mut reading, view, cancel).map(Some)
            }
        }
    }
}
/// `MuPDF`'s session over the path what is held gave it: the copy there
/// lives as long as the session, which may start its helper again.
struct Holding {
    session: Arc<dyn skwir_core::document::DocumentSession>,
    _held: Arc<Held>,
}

impl skwir_core::document::DocumentSession for Holding {
    fn info(&self) -> &skwir_core::document::DocumentInfo {
        self.session.info()
    }
    fn render(
        &self,
        page: u32,
        side: u32,
        cancel: &CancelToken,
    ) -> anyhow::Result<skwir_core::document::DocumentPage> {
        self.session.render(page, side, cancel)
    }
    fn search(
        &self,
        query: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<skwir_core::document::SearchHit>> {
        self.session.search(query, cancel)
    }
    fn locate(&self, text: &str, near: u32, cancel: &CancelToken) -> anyhow::Result<Option<u32>> {
        self.session.locate(text, near, cancel)
    }
    fn closed_links(&self) -> Option<&str> {
        self.session.closed_links()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::{op::EntryRef, preview::HeldSlot};

    /// A book made by `tools/books/fixtures.py`.
    pub(crate) fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name)
    }

    pub(crate) fn request(path: &Path) -> PreviewRequest {
        PreviewRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
            fit: None,
            held: HeldSlot::default(),
        }
    }

    /// `MuPDF`, chosen, reads the kinds of book it reads well; the others
    /// are the built-in engine's, told from the file before it opens.
    #[test]
    fn mupdf_reads_the_kinds_it_reads_well() {
        let mupdf = Provider {
            engine: Engine::Mupdf,
            ..Provider::default()
        };
        let cancel = CancelToken::new();
        for (name, format, reads) in [
            ("synthetic.epub", Format::Epub, true),
            ("synthetic.fb2", Format::Fb2, true),
            ("synthetic-1251.fb2", Format::Fb2, true),
            ("synthetic.fb2.zip", Format::Fb2Zip, false),
            ("synthetic.mobi", Format::Palm, true),
            ("synthetic-uncompressed.mobi", Format::Palm, true),
            // Read through its MOBI 6 part.
            ("synthetic-both.mobi", Format::Palm, true),
            // MuPDF draws nothing of a KF8 book.
            ("synthetic.azw3", Format::Palm, false),
        ] {
            let request = request(&fixture(name));
            assert_eq!(Format::of_request(&request), Some(format), "{name}");
            let held = request.held().unwrap();
            assert_eq!(mupdf.mupdf_reads(format, &held, &cancel), reads, "{name}");
            assert!(
                !Provider::default().mupdf_reads(format, &held, &cancel),
                "{name}: the built-in engine reads it unless MuPDF is chosen"
            );
        }
    }

    /// A cover is asked for by the file's type alone, on the window's
    /// thread before the file is read (a name that leads nowhere is
    /// enough): a Palm database (`.pdb`), a book only when its bytes say
    /// so, has none, and its preview still reads it.
    #[test]
    fn a_cover_is_a_book_s_by_its_type_alone() {
        use skwir_core::thumbs::ThumbSize;
        let asked = |path: &Path, mime: Option<&str>| ThumbRequest {
            entry: EntryRef {
                id: skwir_core::FileId::new(0, 0),
                path: path.to_owned(),
                only_name: false,
            },
            mime: mime.map(Arc::from),
            mtime: None,
            size: ThumbSize::Normal,
        };
        for name in [
            "Synthetic.epub",
            "Synthetic.fb2",
            "Synthetic.fb2.zip",
            "Synthetic.mobi",
            "Synthetic.azw3",
        ] {
            let nowhere = Path::new("/nowhere").join(name);
            assert!(Cover.accepts(&asked(&nowhere, None)), "{name}");
        }
        let text = fixture("synthetic.pdb");
        for mime in ["application/vnd.palm", "application/x-aportisdoc"] {
            let palm = ThumbRequest {
                entry: EntryRef::from_path(&text).unwrap(),
                ..asked(&text, Some(mime))
            };
            assert!(!Cover.accepts(&palm), "{mime}");
        }
        let mut preview = request(&text);
        preview.mime = Some("application/vnd.palm".into());
        assert_eq!(Format::of_request(&preview), Some(Format::Palm));
    }

    /// A Palm database is a book only when it says it is one; a KFX book is
    /// not one `MuPDF` reads.
    #[test]
    fn a_palm_database_is_a_book_by_its_type_and_creator() {
        let tmp = tempfile::tempdir().unwrap();
        let text = tmp.path().join("Synthetic text.pdb");
        std::fs::copy(fixture("synthetic.pdb"), &text).unwrap();
        let mut request = request(&text);
        request.mime = Some("application/vnd.palm".into());
        assert_eq!(Format::of_request(&request), Some(Format::Palm));
        let other = tmp.path().join("Synthetic other.pdb");
        let mut bytes = std::fs::read(&text).unwrap();
        bytes[60..68].copy_from_slice(b"DATAappl");
        std::fs::write(&other, &bytes).unwrap();
        let mut request = super::tests::request(&other);
        request.mime = Some("application/vnd.palm".into());
        assert_eq!(Format::of_request(&request), None);
        let kfx = tmp.path().join("Synthetic book.kfx");
        std::fs::write(&kfx, b"CONT\x02\x00\x12\x00\x00\x00 synthetic").unwrap();
        let request = super::tests::request(&kfx);
        assert_eq!(Format::of_request(&request), Some(Format::Palm));
        let mupdf = Provider {
            engine: Engine::Mupdf,
            ..Provider::default()
        };
        assert!(!mupdf.mupdf_reads(Format::Palm, &request.held().unwrap(), &CancelToken::new()));
    }
}
