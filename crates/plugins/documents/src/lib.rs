//! Optional documents previews, registered independently of the UI toolkit,
//! and thumbnails for a desktop that has no thumbnailer for them: PDFs, and
//! Office documents from the picture they keep or once the preview has
//! converted them.
//!
//! A text document (Word, `OpenDocument`, RTF) is shown as pages by
//! `LibreOffice`, or read as text by skwir or by Pandoc, as the Text
//! documents setting says and what is installed allows (ADR-030).

mod blocks;
mod docx;
mod odt;
mod package;
mod pandoc;
mod stored;

use pandoc::Pandoc;
use skwir_core::{
    App, CancelToken,
    cache::CacheSlot,
    inspection::{InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{Held, PreviewProvider, PreviewRequest},
    words,
};
use skwir_platform::inspection::{self, limits, limits::Bounds};
use skwir_platform::preview::DocumentPreviews;
use skwir_platform::rich_preview;
use skwir_platform::thumb_providers::{OfficePages, PdfPages};
use std::sync::Arc;

const ID: &str = "documents";
/// How Word, `OpenDocument` and RTF files are shown.
const TEXT_DOCUMENTS: &str = "documents.text_documents";
/// Which engine reads PDFs.
const BACKEND: &str = "documents.backend";
/// Where the `PDFium` library is, when not in the usual places.
const PDFIUM_LIBRARY: &str = "documents.pdfium_library";
/// The provider that reads text documents as text.
const READING_PROVIDER: &str = "documents.reading";
/// The document reader (`DocumentPreviews`), which shows pages.
const READER_PROVIDER: &str = "documents";

pub struct PreviewPlugin {
    /// `PDFium`'s library as the settings name it: the one the platform
    /// keeps for every `PDFium` helper (`document::pdfium_choice`: printing
    /// opens it too), which the requirement's probe and the PDF thumbnails
    /// tell the helper, as the reader's launch does.
    pdfium_library: skwir_platform::document::PdfiumChoice,
}

impl Default for PreviewPlugin {
    fn default() -> Self {
        Self {
            pdfium_library: skwir_platform::document::pdfium_choice(),
        }
    }
}

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(documents_plugin_title = "Documents")
    }
    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            documents_plugin_description = "Read PDF and office documents: continuous scrolling, search, text selection, links and contents; Word and OpenDocument text also as text that fits the panel"
        )
    }
    fn for_viewer(&self) -> bool {
        true
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    /// The library its setting named is the program's no more: turned
    /// off, it leaves no trace.
    fn on_disable(&self, _app: &App) {
        *self.pdfium_library.write() = None;
    }
    /// Only the provider a key is about starts again: changing how text
    /// documents are shown leaves a PDF open in the reader as it is.
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key == TEXT_DOCUMENTS) {
            app.previews.unregister(ID, READING_PROVIDER);
            register_reading(app);
        }
        if keys
            .iter()
            .any(|key| key == BACKEND || key == PDFIUM_LIBRARY)
        {
            *self.pdfium_library.write() = pdfium_library(app);
            app.previews.unregister(ID, READER_PROVIDER);
            register_reader(app);
        }
    }

    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        use skwir_core::plugin::Requirement;
        use skwir_platform::document::{Engine, worker};
        let library = Arc::clone(&self.pdfium_library);
        let pdfium = worker(Engine::Pdfium);
        let pdf_reading = words!(
            documents_requirement_pdfium = "PDF reading with PDFium",
            note = "What stops working without PDFium's helper, in the settings' list of what a plugin needs."
        );
        vec![
            // Its library is the one the settings name, else the system's.
            Requirement::new(pdfium.executable, pdf_reading, move || {
                let library = library.read().clone();
                let envs: Vec<(&str, &std::ffi::OsStr)> = library
                    .iter()
                    .map(|path| ("SKWIR_PDFIUM_LIBRARY", path.as_os_str()))
                    .collect();
                pdfium.works_with(&envs)
            }),
            worker(Engine::Poppler).requirement(words!(
                documents_requirement_poppler = "PDF reading with Poppler",
                note = "What stops working without Poppler's helper, in the settings' list of what a plugin needs."
            )),
            worker(Engine::Mupdf).requirement(words!(
                documents_requirement_mupdf = "PDF reading with MuPDF",
                note = "What stops working without MuPDF's helper, in the settings' list of what a plugin needs."
            )),
            // Where it installs itself too, off Linux (`process::required`).
            Requirement::new(
                "libreoffice",
                words!(
                    documents_requirement_libreoffice = "Office documents shown as pages",
                    note = "What stops working without LibreOffice, in the settings' list of what a plugin needs."
                ),
                || skwir_platform::process::required("libreoffice").is_ok(),
            ),
            Requirement::program(
                "pandoc",
                words!(
                    documents_requirement_pandoc = "Text documents read by Pandoc, and RTF files read as text",
                    note = "What stops working without Pandoc, in the settings' list of what a plugin needs."
                ),
            ),
        ]
        .into_iter()
        .chain(skwir_platform::sandbox::PROGRAM.map(|program| {
            Requirement::program(
                program,
                words!(
                    documents_requirement_sandbox = "Isolation of LibreOffice, which needs it, and of Pandoc",
                    note = "What stops working without the sandbox program (bubblewrap), in the settings' list of what a plugin needs: LibreOffice is not run without it, Pandoc runs in its own sandbox where it has one."
                ),
            )
        }))
        .collect()
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
    }
    fn register(&self, app: &App) {
        use skwir_core::settings::SettingSpec;
        use skwir_platform::document::{AUTO, Engine, worker};
        // What Automatic gives, once the settings probed each engine.
        let tries = AUTO.map(|engine| (engine.key(), worker(engine).executable));
        app.settings.declare(
            SettingSpec::choice_words(
                BACKEND,
                words!(
                    documents_engine = "Document engine",
                    note = "A setting's title: which program reads PDF documents."
                ),
                words!(
                    documents_engine_description = "Automatic prefers PDFium and falls back to Poppler; MuPDF is used only when chosen.",
                    note = "Automatic is the setting's first choice; PDFium, Poppler and MuPDF are the programs' names."
                ),
                "auto",
                vec![
                    (
                        "auto",
                        words!(
                            documents_engine_auto = "Automatic",
                            note = "A choice of the Document engine setting: the first engine installed."
                        ),
                    ),
                    ("pdfium", words::Words::as_written("PDFium")),
                    ("poppler", words::Words::as_written("Poppler")),
                    ("mupdf", words::Words::as_written("MuPDF")),
                ],
            )
            .tries(&tries)
            // Used only when chosen, it needs its helper all the same.
            .needs(&[("mupdf", worker(Engine::Mupdf).executable)])
            .page(skwir_core::settings::page::READING)
            .section(skwir_core::settings::section::DOCUMENTS),
        );
        app.settings.declare(
            SettingSpec::path(
                PDFIUM_LIBRARY,
                words!(
                    documents_pdfium_library = "PDFium library",
                    note = "A setting's title: where the PDFium library is."
                ),
                (
                    words!(
                        documents_pdfium_library_description = "Path to the PDFium shared library."
                    ),
                    words!(
                        documents_pdfium_library_description_more = "Leave empty to search the usual locations.",
                        note = "What a setting's description says after its first sentence, in its tooltip."
                    ),
                ),
                "",
            )
            .page(skwir_core::settings::page::READING)
            .section(skwir_core::settings::section::DOCUMENTS),
        );
        app.settings.declare(SettingSpec::choice_words(
            TEXT_DOCUMENTS,
            words!(
                documents_text_documents = "Text documents",
                note = "A setting's title: how Word, OpenDocument and RTF documents are shown in the preview."
            ),
            (
                words!(
                    documents_text_documents_description = "How Word, OpenDocument and RTF documents are shown.",
                    note = "Reading, Pages and Pandoc are the setting's three choices; Pandoc and LibreOffice are programs' names."
                ),
                words!(
                    documents_text_documents_description_more = "Reading shows their text, headings, lists, tables, notes and pictures at once, to fit the panel: Skwir reads Word and OpenDocument files, Pandoc RTF. Pages lays them out as LibreOffice prints them, which takes a few seconds each. Pandoc has Pandoc read them all. When what a choice needs is not installed, another is used.",
                    note = "Reading, Pages and Pandoc are the setting's three choices; Pandoc and LibreOffice are programs' names."
                ),
            ),
            "reading",
            vec![
                (
                    "reading",
                    words!(
                        documents_text_documents_reading = "Reading",
                        note = "A choice of the Text documents setting: the document's text read to fit the panel (a reading mode)."
                    ),
                ),
                (
                    "pages",
                    words!(
                        documents_text_documents_pages = "Pages",
                        note = "A choice of the Text documents setting: the document laid out in pages, as LibreOffice prints it."
                    ),
                ),
                ("pandoc", words::Words::as_written("Pandoc")),
            ],
        )
        .page(skwir_core::settings::page::READING)
        .section(skwir_core::settings::section::DOCUMENTS));
        register_reading(app);
        register_reader(app);
        *self.pdfium_library.write() = pdfium_library(app);
        let cache = app.cache.namespace(ID);
        // A fallback stage: the desktop's cache and thumbnailers come first
        // whichever plugin registers first. The picture a document keeps
        // comes before the first page of one converted.
        app.thumbnails.register(
            ID,
            Arc::new(PdfPages {
                pdfium: Arc::clone(&self.pdfium_library),
            }),
        );
        app.thumbnails
            .register(ID, Arc::new(stored::StoredPictures));
        app.thumbnails.register(
            ID,
            Arc::new(OfficePages::new(cache, Arc::clone(&self.pdfium_library))),
        );
    }
}

/// Text documents read as text, as the Text documents setting says.
fn register_reading(app: &App) {
    let choice = match app.settings.get::<String>(TEXT_DOCUMENTS).as_deref() {
        Some("pages") => Choice::Pages,
        Some("pandoc") => Choice::Pandoc,
        _ => Choice::Reading,
    };
    app.previews.register(
        ID,
        Arc::new(Reading {
            choice,
            cache: app.cache.namespace(ID),
        }),
    );
}

/// `PDFium`'s library as the settings name it, if they do.
fn pdfium_library(app: &App) -> Option<std::path::PathBuf> {
    app.settings
        .get::<String>(PDFIUM_LIBRARY)
        .filter(|s| !s.is_empty())
        .map(Into::into)
}

/// The document reader, with the engine the settings choose.
fn register_reader(app: &App) {
    let backend = app
        .settings
        .get::<String>(BACKEND)
        .unwrap_or_else(|| "auto".into());
    let library = pdfium_library(app);
    // A document converted for its preview has a thumbnail from then on:
    // a tile that had none asks again.
    let thumbnails = Arc::clone(&app.thumbnails);
    app.previews.register(
        ID,
        Arc::new(
            DocumentPreviews::new(app.cache.namespace(ID))
                .with_reader(backend, library)
                .on_converted(Arc::new(move |id| thumbnails.retry(id))),
        ),
    );
}

/// How the user asked text documents to be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Pages,
    Reading,
    Pandoc,
}

/// What reads a text document as text.
enum Engine {
    Word,
    OpenDocument,
    Pandoc(Pandoc, &'static str),
}

/// Text documents read as text, before the document reader that shows
/// them as pages (`DocumentPreviews`): it takes a file only when the
/// setting and what is installed say so, and decides on the worker.
struct Reading {
    choice: Choice,
    /// The plugin's cache, where Pandoc's pictures are kept.
    cache: CacheSlot,
}

/// skwir's own reader for the file `held`, if it reads its format: by its
/// name, and its first bytes.
fn builtin(held: &Arc<Held>) -> Option<Engine> {
    let engine = match inspection::extension(held.path()).as_str() {
        "docx" | "docm" | "dotx" | "dotm" => Engine::Word,
        "odt" | "ott" => Engine::OpenDocument,
        _ => return None,
    };
    package::is_zip(held).then_some(engine)
}

/// Pandoc for the file `held`, if it is installed and reads its format.
fn by_pandoc(held: &Arc<Held>) -> Option<Engine> {
    let format = pandoc::format_of(&inspection::extension(held.path()))?;
    let readable = if format == "rtf" {
        held.head().starts_with(b"{\\rtf")
    } else {
        package::is_zip(held)
    };
    if !readable {
        return None;
    }
    let tool = Pandoc::find()?;
    tool.reads(format).then_some(Engine::Pandoc(tool, format))
}

impl Reading {
    /// What reads the file `held`: the choice when what it needs is there,
    /// else the next that can; `None` leaves it to the pages.
    fn engine(&self, held: &Arc<Held>) -> Option<Engine> {
        match self.choice {
            Choice::Pages if skwir_platform::preview::shown_as_pages(held.path(), None) => None,
            Choice::Pages | Choice::Reading => builtin(held).or_else(|| by_pandoc(held)),
            Choice::Pandoc => by_pandoc(held).or_else(|| builtin(held)),
        }
    }

    /// Where Pandoc's picture `name` of the file `held` is kept.
    fn media_key(
        req: &PreviewRequest,
        held: &Held,
        tool: &Pandoc,
        name: &str,
    ) -> anyhow::Result<Vec<u8>> {
        skwir_platform::preview::made_key(req.entry.id, held, &tool.path, ("pandoc-media", name))
    }

    fn pandoc(
        &self,
        req: &PreviewRequest,
        held: &Arc<Held>,
        tool: &Pandoc,
        format: &str,
        bounds: Bounds,
        cancel: &CancelToken,
    ) -> anyhow::Result<Inspection> {
        // What Pandoc made of this very file before, with its pictures'
        // sizes, within these bounds: kept, it is shown again without
        // running Pandoc.
        let html_key = skwir_platform::preview::made_key(
            req.entry.id,
            held,
            &tool.path,
            (bounds.choose("pandoc-html", "pandoc-html-whole"), format),
        )?;
        let (page, cut) = if let Some((html, cut, sizes)) = self
            .cache
            .get(&html_key)
            .and_then(|bytes| converted_from(&bytes))
        {
            (
                rich_preview::html(
                    &html,
                    rich_preview::Markup::Html,
                    bounds,
                    cancel,
                    pandoc::Sized(&sizes),
                )?,
                cut,
            )
        } else {
            let work = tempfile::tempdir()?;
            let source = held.path_for_program(cancel)?;
            let (html, cut) = tool.convert(&source, format, work.path(), bounds, cancel)?;
            let mut kept = 0u64;
            let mut sizes = std::collections::HashMap::new();
            let keep = |name: &str, bytes: &[u8]| {
                if let Some(size) = inspection::picture_size(bytes) {
                    sizes.insert(name.to_owned(), size);
                }
                if let Ok(key) = Self::media_key(req, held, tool, name)
                    && self.cache.put(&key, bytes).is_ok()
                {
                    kept += bytes.len() as u64;
                }
            };
            let media = pandoc::Media {
                work: work.path(),
                keep,
                cancel,
            };
            let page =
                rich_preview::html(&html, rich_preview::Markup::Html, bounds, cancel, media)?;
            let made = converted_to(&html, cut, &sizes);
            if self.cache.put(&html_key, &made).is_ok() {
                kept += made.len() as u64;
            }
            if kept > 0 {
                self.cache.trim_soon(kept, CACHE_BYTES);
            }
            (page, cut)
        };
        let words = page
            .blocks
            .iter()
            .filter(|b| b.note.is_none())
            .map(|b| b.text.split_whitespace().count())
            .sum();
        let facts = about(held, cancel);
        let notices = [
            Some(words!(
                documents_read_by_pandoc = "Read by Pandoc · layout, headers and comments are not shown",
                note = "A note under a text document's preview read by Pandoc, a program."
            )),
            cut.then(|| {
                words!(
                    documents_start_only = "Only the start is shown",
                    note = "A note under a document's preview: Pandoc's reading of it was longer than the preview shows."
                )
            }),
            page.truncated.then(|| {
                words!(
                    documents_first_blocks = "{ $count ->
                        [1] Only the first block is shown
                       *[other] Only the first { $count } blocks are shown
                    }",
                    count = bounds.blocks(),
                    note = "A note under a document's preview: only its first paragraphs, headings, tables and pictures (blocks) are shown."
                )
            }),
        ];
        Ok(Inspection {
            kind: InspectionKind::Markdown,
            whole_available: (cut || page.truncated) && bounds.cut(),
            blocks: page.blocks,
            notices: notices.into_iter().flatten().collect(),
            details: details(&facts, words),
            ..Inspection::default()
        })
    }
}

/// The error for a picture a document does not name.
fn not_its_picture() -> words::Words {
    words!(
        documents_not_its_picture = "not a picture of this document",
        note = "An error in place of a picture in a document's preview: the picture asked for is none the document holds."
    )
}

/// The plugin's cache namespace is bounded as the document preview's is.
const CACHE_BYTES: u64 = 128 * 1024 * 1024;

/// What Pandoc made of a file, as it is kept: whether it was cut, the size
/// of each picture (`name`, tab, width, tab, height), a blank line, then
/// the HTML.
fn converted_to(
    html: &str,
    cut: bool,
    sizes: &std::collections::HashMap<String, (u32, u32)>,
) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut made = String::from(if cut { "cut\n" } else { "whole\n" });
    for (name, (width, height)) in sizes {
        // A name holding a tab or a line could not be read back.
        if !name.contains(['\t', '\n']) {
            let _ = writeln!(made, "{name}\t{width}\t{height}");
        }
    }
    made.push('\n');
    made.push_str(html);
    made.into_bytes()
}

/// What `converted_to` kept, read back.
fn converted_from(
    bytes: &[u8],
) -> Option<(String, bool, std::collections::HashMap<String, (u32, u32)>)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let (head, html) = text.split_once("\n\n")?;
    let mut lines = head.lines();
    let cut = match lines.next()? {
        "cut" => true,
        "whole" => false,
        _ => return None,
    };
    let sizes = lines
        .map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?.to_owned();
            let width = parts.next()?.parse().ok()?;
            let height = parts.next()?.parse().ok()?;
            Some((name, (width, height)))
        })
        .collect::<Option<_>>()?;
    Some((html.to_owned(), cut, sizes))
}

/// What a Word or `OpenDocument` file says about itself, for Properties; none
/// for another format.
fn about(held: &Arc<Held>, cancel: &CancelToken) -> Vec<(&'static str, String)> {
    let read = match builtin(held) {
        Some(Engine::Word) => docx::about(held, cancel),
        Some(Engine::OpenDocument) => odt::about(held, cancel),
        _ => return Vec::new(),
    };
    read.unwrap_or_default()
}

/// Properties in their order: what the document states, and its words.
/// A fact is keyed by its English label (`docx::about`, `odt::about`), an
/// identifier: the label shown is a message.
fn details(facts: &[(&'static str, String)], words: usize) -> Vec<skwir_core::inspection::Detail> {
    let fact = |key: &str| {
        facts
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, value)| value.clone())
    };
    inspection::details([
        (
            words!(
                documents_detail_title = "Title",
                note = "A label in the preview's Properties: the title the document gives itself."
            ),
            fact("Title"),
        ),
        (
            words!(
                documents_detail_author = "Author",
                note = "A label in the preview's Properties: who wrote the document, as it says."
            ),
            fact("Author"),
        ),
        (
            words!(
                documents_detail_subject = "Subject",
                note =
                    "A label in the preview's Properties: the subject the document gives itself."
            ),
            fact("Subject"),
        ),
        (
            words!(
                documents_detail_keywords = "Keywords",
                note =
                    "A label in the preview's Properties: the keywords the document gives itself."
            ),
            fact("Keywords"),
        ),
        (
            words!(
                documents_detail_pages = "Pages",
                note = "A label in the preview's Properties: how many pages the document had when it was saved."
            ),
            fact("Pages"),
        ),
        (
            words!(
                documents_detail_words = "Words",
                note = "A label in the preview's Properties: how many words the document holds."
            ),
            inspection::count(words),
        ),
        // `LibreOffice/24.2.1.2$Linux_X86_64 LibreOffice_project/…`: the
        // application and its version.
        (
            words!(
                documents_detail_application = "Application",
                note = "A label in the preview's Properties: the program that saved the document, with its version."
            ),
            fact("Application")
                .map(|app| app.split('$').next().unwrap_or_default().replace('/', " ")),
        ),
    ])
}

/// A document skwir read, as the reading mode shows it: one part of it,
/// `[` and `]` going to the others.
fn shown(read: blocks::Read) -> Inspection {
    let paged = read.parts > 1;
    let notices = [
        Some(words!(
            documents_reading_mode = "Reading mode · layout, headers and comments are not shown",
            note = "A note under a Word or OpenDocument file's preview, read by skwir as text that fits the panel."
        )),
        paged.then(|| {
            words!(
                documents_parts_keys = "[ and ] move through its parts",
                note = "A note under a long document's preview, shown a part at a time: the [ and ] keys show the part before and the part after."
            )
        }),
        read.truncated.then(|| {
            words!(
                documents_first_blocks_of_part = "{ $count ->
                    [1] Only the first block of this part is shown
                   *[other] Only the first { $count } blocks of this part are shown
                }",
                count = limits::BLOCKS,
                note = "A note under a document's preview: only the first paragraphs, headings, tables and pictures (blocks) of the part shown are shown."
            )
        }),
    ];
    Inspection {
        kind: InspectionKind::Markdown,
        title: if paged {
            words!(
                documents_part_of = "Part { $part } of { $parts }",
                part = read.part + 1,
                parts = read.parts,
                note = "The title of a long document's preview, shown a part at a time: which part, of how many."
            )
        } else {
            skwir_core::words::Words::default()
        },
        blocks: read.blocks,
        previous: read.part.checked_sub(1),
        next: Some(read.part + 1).filter(|next| *next < read.parts),
        position: read.part,
        notices: notices.into_iter().flatten().collect(),
        details: details(&read.details, read.words),
        ..Inspection::default()
    }
}

impl PreviewProvider for Reading {
    fn id(&self) -> &'static str {
        READING_PROVIDER
    }
    /// Before the document reader, which shows the same files as pages.
    fn priority(&self) -> i32 {
        10
    }
    /// By its name, then its first bytes: what is held is read only for a
    /// file named as one it may read.
    fn accepts(&self, req: &PreviewRequest) -> bool {
        pandoc::format_of(&inspection::extension(&req.entry.path)).is_some()
            && req.held().is_ok_and(|held| self.engine(&held).is_some())
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        if view.source {
            return Ok(None);
        }
        let held = req.held()?;
        let engine = self
            .engine(&held)
            .ok_or_else(|| {
                anyhow::anyhow!(words!(
                    documents_no_reader = "Nothing installed reads this document as text",
                    note = "An error in place of a text document when neither skwir nor Pandoc reads its format."
                ))
            })?;
        Ok(Some(match engine {
            Engine::Word => shown(docx::read(&held, view.offset, cancel)?),
            Engine::OpenDocument => shown(odt::read(&held, view.offset, cancel)?),
            Engine::Pandoc(tool, format) => {
                self.pandoc(req, &held, &tool, format, Bounds::of(view), cancel)?
            }
        }))
    }
    /// A picture the document named: a member of its package, read through
    /// the same bounded reader; or one Pandoc extracted, from the cache, or
    /// extracted again when the cache let it go.
    fn picture(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        key: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<skwir_core::thumbs::Image>> {
        let held = req.held()?;
        let bytes = match self.engine(&held) {
            Some(Engine::Word | Engine::OpenDocument) => {
                package::Package::open(&held, cancel)?.read(key, package::Part::Picture, cancel)?
            }
            Some(Engine::Pandoc(tool, format)) => {
                let name =
                    pandoc::media_name(key).ok_or_else(|| anyhow::anyhow!(not_its_picture()))?;
                let cache_key = Self::media_key(req, &held, &tool, &name)?;
                if let Some(bytes) = self.cache.get(&cache_key) {
                    bytes
                } else {
                    // Converted again: every picture is kept again, not
                    // only this one, so the others do not each convert.
                    let work = tempfile::tempdir()?;
                    let source = held.path_for_program(cancel)?;
                    tool.convert(&source, format, work.path(), Bounds::of(view), cancel)?;
                    let mut wanted = None;
                    let mut kept = 0u64;
                    for (each, file) in pandoc::extracted(work.path()) {
                        let Ok(bytes) = inspection::read_bounded(
                            &file,
                            limits::BINARY_INPUT,
                            inspection::image_too_large(limits::BINARY_INPUT),
                            cancel,
                        ) else {
                            continue;
                        };
                        if let Ok(key) = Self::media_key(req, &held, &tool, &each)
                            && self.cache.put(&key, &bytes).is_ok()
                        {
                            kept += bytes.len() as u64;
                        }
                        if each == name {
                            wanted = Some(bytes.into_owned());
                        }
                    }
                    if kept > 0 {
                        self.cache.trim_soon(kept, CACHE_BYTES);
                    }
                    wanted.ok_or_else(|| anyhow::anyhow!(not_its_picture()))?
                }
            }
            None => return Ok(None),
        };
        Ok(Some(inspection::image(&bytes)?))
    }
}

#[cfg(test)]
mod tests;
