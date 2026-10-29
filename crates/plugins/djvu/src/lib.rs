//! `DjVu` documents, scanned books mostly, in the document reader, and their
//! first pages as thumbnails: read by `DjVuLibre` where it is installed, else
//! by djvu-rs, whose helper every package carries; or by the one the `DjVu`
//! engine setting names (ADR-061).
use skwir_core::{
    App, CancelToken,
    document::{DocumentInfo, DocumentPage, DocumentSession, SearchHit},
    plugin::Plugin,
    preview::{Held, PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
};
use skwir_platform::document::{self, Engine};
use skwir_platform::mime;
use std::sync::Arc;

const ID: &str = "djvu";
/// Which engine reads `DjVu` documents.
const ENGINE: &str = "djvu.engine";
/// The reader, which shows a document's pages.
const READER: &str = "djvu";

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "DjVu"
    }
    fn description(&self) -> &'static str {
        "Read DjVu documents, scanned books mostly: continuous scrolling, search, text selection, links and contents; and show their first pages as thumbnails"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    /// A document open in the reader starts again with the engine chosen.
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key == ENGINE) {
            app.previews.unregister(ID, READER);
            register_reader(app);
        }
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![
            document::worker(Engine::Djvulibre).requirement("DjVu reading with DjVuLibre"),
            document::worker(Engine::DjvuRs).requirement("DjVu reading with djvu-rs"),
        ]
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
    }
    fn register(&self, app: &App) {
        app.settings.declare(skwir_core::settings::SettingSpec::choice(
            ENGINE,
            "DjVu engine",
            "Automatic prefers DjVuLibre, where it is installed, and falls back to djvu-rs, which skwir carries.",
            "auto",
            &[
                ("auto", "Automatic"),
                ("djvulibre", "DjVuLibre"),
                ("djvu-rs", "djvu-rs"),
            ],
        )
        .page(skwir_core::settings::page::READING)
        .section("Documents"));
        register_reader(app);
        // A fallback stage: the desktop's cache and thumbnailers come first.
        app.thumbnails.register(ID, Arc::new(FirstPages));
    }
}

/// `DjVu` documents, by type: one page ("`DjVu` image") or several.
static DJVU: mime::Types = mime::Types::new(&["image/vnd.djvu", "image/vnd.djvu+multipage"]);

/// The engines the setting `choice` names, in the order they are tried.
fn engines(choice: Option<&str>) -> &'static [Engine] {
    match choice {
        Some("djvulibre") => &[Engine::Djvulibre],
        Some("djvu-rs") => &[Engine::DjvuRs],
        _ => &document::DJVU_AUTO,
    }
}

/// The reader, with the engines the settings choose.
fn register_reader(app: &App) {
    let choice = app.settings.get::<String>(ENGINE);
    app.previews.register(
        ID,
        Arc::new(Reader {
            engines: engines(choice.as_deref()),
        }),
    );
}

struct Reader {
    engines: &'static [Engine],
}

impl PreviewProvider for Reader {
    fn id(&self) -> &'static str {
        READER
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        DJVU.holds(&req.entry.path, req.mime.as_deref())
    }
    /// A document of pages: read continuously or a page at a time, each
    /// page drawn at the side it is shown at. The engine's helper reads it
    /// by its path: the one what is held gives a program (a copy of a
    /// remote's file held whole).
    fn open_document(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let held = req.held()?;
        let path = held.path_for_program(lifetime)?;
        if let Some(session) = document::open(&path, self.engines, lifetime)? {
            return Ok(Some(Arc::new(Holding {
                session,
                _held: held,
            })));
        }
        // What is missing, in the words every helper's absence is said in.
        let helpers: Vec<&str> = self
            .engines
            .iter()
            .map(|engine| document::worker(*engine).executable)
            .collect();
        match self.engines {
            [engine] => anyhow::bail!(
                "Install {} to read DjVu documents with {}",
                helpers[0],
                engine.label()
            ),
            _ => anyhow::bail!("Install {} to read DjVu documents", helpers.join(" or ")),
        }
    }
}

/// A helper's session over the path what is held gave it: the copy there
/// lives as long as the session, which may start its helper again.
struct Holding {
    session: Arc<dyn DocumentSession>,
    _held: Arc<Held>,
}

impl DocumentSession for Holding {
    fn info(&self) -> &DocumentInfo {
        self.session.info()
    }
    fn render(&self, page: u32, side: u32, cancel: &CancelToken) -> anyhow::Result<DocumentPage> {
        self.session.render(page, side, cancel)
    }
    fn search(&self, query: &str, cancel: &CancelToken) -> anyhow::Result<Vec<SearchHit>> {
        self.session.search(query, cancel)
    }
    fn locate(&self, text: &str, near: u32, cancel: &CancelToken) -> anyhow::Result<Option<u32>> {
        self.session.locate(text, near, cancel)
    }
    fn closed_links(&self) -> Option<&str> {
        self.session.closed_links()
    }
}

/// A document's first page, for the grid. A fallback: a thumbnailer the
/// desktop installed for `DjVu` comes first; drawn by an engine in
/// `auto`'s order, whatever the reader's, as a PDF's first page is.
struct FirstPages;

impl ThumbnailProvider for FirstPages {
    fn id(&self) -> &'static str {
        "djvu-pages"
    }
    fn stage(&self) -> Stage {
        Stage::Fallback
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        DJVU.holds(&req.entry.path, req.mime.as_deref())
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = req.entry.resolve()?;
        skwir_platform::thumb_providers::ensure_file(&path)?;
        document::first_page(&path, req.size.pixels(), &document::DJVU_AUTO, None, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of `width` × `height` pixels at `dpi` with nothing on it: a
    /// `DjVu` file's least, its `INFO` chunk alone.
    fn blank_page(width: u16, height: u16, dpi: u16) -> Vec<u8> {
        let mut info = Vec::new();
        info.extend_from_slice(&width.to_be_bytes());
        info.extend_from_slice(&height.to_be_bytes());
        // Its version (minor, major), resolution, gamma 2.2, upright.
        info.extend_from_slice(&[24, 0]);
        info.extend_from_slice(&dpi.to_le_bytes());
        info.extend_from_slice(&[22, 1]);
        let mut form = b"DJVUINFO".to_vec();
        form.extend_from_slice(&u32::try_from(info.len()).unwrap().to_be_bytes());
        form.extend(info);
        let mut file = b"AT&TFORM".to_vec();
        file.extend_from_slice(&u32::try_from(form.len()).unwrap().to_be_bytes());
        file.extend(form);
        file
    }

    /// A preview reads its file once (ADR-101): a remote's document held
    /// whole is handed to djvu-rs's helper as a copy written from what is
    /// held, which lives as long as the reader; its own path, where no
    /// file ever is, is never read. Needs the helper built in the target
    /// folder the test runs from; without it the test says so and passes,
    /// unless `SKWIR_REQUIRE_WORKERS` is set.
    #[test]
    fn a_remote_document_held_is_read_from_its_copy() {
        use skwir_core::preview::HeldSlot;
        let helper = document::worker(Engine::DjvuRs);
        let built = std::env::current_exe()
            .ok()
            .and_then(|exe| {
                let name = format!("{}{}", helper.executable, std::env::consts::EXE_SUFFIX);
                Some(exe.parent()?.parent()?.join(name))
            })
            .filter(|path| path.is_file());
        let Some(worker) = std::env::var_os(helper.variable)
            .map(std::path::PathBuf::from)
            .or(built)
        else {
            assert!(
                std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
                "skwir-djvu-worker is not built"
            );
            eprintln!("skipped: skwir-djvu-worker is not built");
            return;
        };
        // SAFETY: nextest runs each test in a process of its own, and nothing
        // else reads the environment before this.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var(helper.variable, &worker);
        }
        let dir = skwir_platform::file::memory_dir("skwir-djvu-test-").unwrap();
        let nowhere = dir.path().join("gone").join("l’été scan #1.djvu");
        let scratch = dir.path().join("scratch");
        let bytes = blank_page(850, 1100, 100);
        let held = Arc::new(Held::of_bytes_on_remote(
            &nowhere,
            bytes.clone(),
            scratch.clone(),
        ));
        let entry = skwir_core::op::EntryRef::new(skwir_core::FileId::new(0, 0), &nowhere);
        let request = PreviewRequest {
            entry: entry.clone(),
            mime: Some("image/vnd.djvu".into()),
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
            fit: None,
            held: HeldSlot::holding(entry, Arc::clone(&held)),
        };
        let reader = Reader {
            engines: &[Engine::DjvuRs],
        };
        assert!(reader.accepts(&request));
        let cancel = CancelToken::new();
        let session = reader
            .open_document(&request, &cancel)
            .unwrap()
            .expect("a document of pages");
        // The copy the helper was handed: in the preview's folder, what is
        // held, byte for byte.
        let copy = held.path_for_program(&cancel).unwrap().into_owned();
        assert!(copy.starts_with(&scratch), "{}", copy.display());
        assert_eq!(std::fs::read(&copy).unwrap(), bytes);
        // The request let go of, the reader draws from the copy, which
        // lives with it: 8.5 × 11 in, as the page held says.
        drop((request, held));
        assert_eq!(session.info().pages, 1);
        let page = session.render(1, 400, &cancel).unwrap();
        assert!(
            (page.content.width - 816.).abs() < 1.,
            "{}",
            page.content.width
        );
        assert!(copy.is_file());
        assert!(!nowhere.parent().unwrap().exists(), "the path never was");
    }

    #[test]
    fn the_setting_names_the_engines_tried() {
        assert!(matches!(engines(Some("djvulibre")), [Engine::Djvulibre]));
        assert!(matches!(engines(Some("djvu-rs")), [Engine::DjvuRs]));
        // Automatic, and anything the setting should not hold.
        for choice in [None, Some("auto"), Some("other")] {
            assert!(matches!(
                engines(choice),
                [Engine::Djvulibre, Engine::DjvuRs]
            ));
        }
    }
}
