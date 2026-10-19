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
    preview::{PreviewProvider, PreviewRequest},
};
use skwir_platform::inspection::{self, limits, limits::Bounds};
use skwir_platform::preview::DocumentPreviews;
use skwir_platform::rich_preview;
use skwir_platform::thumb_providers::{OfficePages, PdfPages};
use std::path::Path;
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

#[derive(Default)]
pub struct PreviewPlugin {
    /// `PDFium`'s library as the settings name it: what the `PDFium`
    /// requirement's probe and the PDF thumbnails tell the helper, as the
    /// reader's launch does.
    pdfium_library: skwir_platform::document::PdfiumChoice,
}

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Documents"
    }
    fn description(&self) -> &'static str {
        "Read PDF and office documents: continuous scrolling, search, text selection, links and contents; Word and OpenDocument text also as text that fits the panel"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
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
        vec![
            // Its library is the one the settings name, else the system's.
            Requirement::new(pdfium.executable, "PDF reading with PDFium", move || {
                let library = library.read().clone();
                let envs: Vec<(&str, &std::ffi::OsStr)> = library
                    .iter()
                    .map(|path| ("SKWIR_PDFIUM_LIBRARY", path.as_os_str()))
                    .collect();
                pdfium.works_with(&envs)
            }),
            worker(Engine::Poppler).requirement("PDF reading with Poppler"),
            worker(Engine::Mupdf).requirement("PDF reading with MuPDF"),
            // Where it installs itself too, off Linux (`process::required`).
            Requirement::new("libreoffice", "Office documents shown as pages", || {
                skwir_platform::process::required("libreoffice").is_ok()
            }),
            Requirement::program(
                "pandoc",
                "Text documents read by Pandoc, and RTF files read as text",
            ),
        ]
        .into_iter()
        .chain(skwir_platform::sandbox::PROGRAM.map(|program| {
            Requirement::program(
                program,
                "Isolation of LibreOffice, which needs it, and of Pandoc",
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
        app.settings.declare(SettingSpec::choice(
            BACKEND,
            "Document engine",
            "Automatic prefers PDFium and falls back to Poppler; MuPDF is used only when chosen.",
            "auto",
            &[
                ("auto", "Automatic"),
                ("pdfium", "PDFium"),
                ("poppler", "Poppler"),
                ("mupdf", "MuPDF"),
            ],
        )
        .page(skwir_core::settings::page::READING)
        .section("Documents"));
        app.settings.declare(
            SettingSpec::path(
                PDFIUM_LIBRARY,
                "PDFium library",
                "Path to the PDFium shared library. Leave empty to search the usual locations.",
                "",
            )
            .page(skwir_core::settings::page::READING)
            .section("Documents"),
        );
        app.settings.declare(SettingSpec::choice(
            TEXT_DOCUMENTS,
            "Text documents",
            "How Word, OpenDocument and RTF documents are shown. Reading shows their text, headings, lists, tables, notes and pictures at once, to fit the panel: skwir reads Word and OpenDocument files, Pandoc RTF. Pages lays them out as LibreOffice prints them, which takes a few seconds each. Pandoc has Pandoc read them all. When what a choice needs is not installed, another is used.",
            "reading",
            &[
                ("reading", "Reading"),
                ("pages", "Pages"),
                ("pandoc", "Pandoc"),
            ],
        )
        .page(skwir_core::settings::page::READING)
        .section("Documents"));
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

/// skwir's own reader for `path`, if it reads its format.
fn builtin(path: &Path) -> Option<Engine> {
    let engine = match inspection::extension(path).as_str() {
        "docx" | "docm" | "dotx" | "dotm" => Engine::Word,
        "odt" | "ott" => Engine::OpenDocument,
        _ => return None,
    };
    package::is_zip(path).then_some(engine)
}

/// Pandoc for `path`, if it is installed and reads its format.
fn by_pandoc(path: &Path) -> Option<Engine> {
    let format = pandoc::format_of(&inspection::extension(path))?;
    let readable = if format == "rtf" {
        inspection::read(path, 0, 5, &CancelToken::new()).is_ok_and(|head| head == b"{\\rtf")
    } else {
        package::is_zip(path)
    };
    if !readable {
        return None;
    }
    let tool = Pandoc::find()?;
    tool.reads(format).then_some(Engine::Pandoc(tool, format))
}

impl Reading {
    /// What reads `path`: the choice when what it needs is there, else the
    /// next that can; `None` leaves it to the pages.
    fn engine(&self, path: &Path) -> Option<Engine> {
        match self.choice {
            Choice::Pages if skwir_platform::preview::shown_as_pages(path, None) => None,
            Choice::Pages | Choice::Reading => builtin(path).or_else(|| by_pandoc(path)),
            Choice::Pandoc => by_pandoc(path).or_else(|| builtin(path)),
        }
    }

    /// Where Pandoc's picture `name` of `path` is kept.
    fn media_key(
        req: &PreviewRequest,
        path: &Path,
        tool: &Pandoc,
        name: &str,
    ) -> anyhow::Result<Vec<u8>> {
        skwir_platform::preview::made_key(req.entry.id, path, &tool.path, ("pandoc-media", name))
    }

    fn pandoc(
        &self,
        req: &PreviewRequest,
        path: &Path,
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
            path,
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
            let (html, cut) = tool.convert(path, format, work.path(), bounds, cancel)?;
            let mut kept = 0u64;
            let mut sizes = std::collections::HashMap::new();
            let keep = |name: &str, bytes: &[u8]| {
                if let Some(size) = inspection::picture_size(bytes) {
                    sizes.insert(name.to_owned(), size);
                }
                if let Ok(key) = Self::media_key(req, path, tool, name)
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
        let facts = about(path, cancel);
        let notice = [
            Some("Read by Pandoc · layout, headers and comments are not shown".to_owned()),
            cut.then(|| "Only the start is shown".to_owned()),
            page.truncated
                .then(|| format!("First {} blocks", bounds.blocks())),
        ];
        Ok(Inspection {
            kind: InspectionKind::Markdown,
            whole_available: (cut || page.truncated) && bounds.cut(),
            blocks: page.blocks,
            notice: notice.into_iter().flatten().collect::<Vec<_>>().join(" · "),
            details: details(&facts, words),
            ..Inspection::default()
        })
    }
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
fn about(path: &Path, cancel: &CancelToken) -> Vec<(&'static str, String)> {
    let read = match builtin(path) {
        Some(Engine::Word) => docx::about(path, cancel),
        Some(Engine::OpenDocument) => odt::about(path, cancel),
        _ => return Vec::new(),
    };
    read.unwrap_or_default()
}

/// Properties in their order: what the document states, and its words.
fn details(facts: &[(&'static str, String)], words: usize) -> Vec<skwir_core::inspection::Detail> {
    let fact = |label: &str| {
        facts
            .iter()
            .find(|(l, _)| *l == label)
            .map(|(_, value)| value.clone())
    };
    inspection::details([
        ("Title", fact("Title")),
        ("Author", fact("Author")),
        ("Subject", fact("Subject")),
        ("Keywords", fact("Keywords")),
        ("Pages", fact("Pages")),
        ("Words", inspection::count(words)),
        // `LibreOffice/24.2.1.2$Linux_X86_64 LibreOffice_project/…`: the
        // application and its version.
        (
            "Application",
            fact("Application")
                .map(|app| app.split('$').next().unwrap_or_default().replace('/', " ")),
        ),
    ])
}

/// A document skwir read, as the reading mode shows it: one part of it,
/// `[` and `]` going to the others.
fn shown(read: blocks::Read) -> Inspection {
    let paged = read.parts > 1;
    let notice = [
        Some("Reading mode · layout, headers and comments are not shown".to_owned()),
        paged.then(|| "[ and ] move through its parts".to_owned()),
        read.truncated
            .then(|| format!("First {} blocks of this part", limits::BLOCKS)),
    ];
    Inspection {
        kind: InspectionKind::Markdown,
        title: if paged {
            format!("Part {} of {}", read.part + 1, read.parts)
        } else {
            String::new()
        },
        blocks: read.blocks,
        previous: read.part.checked_sub(1),
        next: Some(read.part + 1).filter(|next| *next < read.parts),
        position: read.part,
        notice: notice.into_iter().flatten().collect::<Vec<_>>().join(" · "),
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
    fn accepts(&self, req: &PreviewRequest) -> bool {
        self.engine(&req.entry.path).is_some()
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
        let path = req.entry.resolve()?;
        let engine = self
            .engine(&path)
            .ok_or_else(|| anyhow::anyhow!("Nothing installed reads this document as text"))?;
        Ok(Some(match engine {
            Engine::Word => shown(docx::read(&path, view.offset, cancel)?),
            Engine::OpenDocument => shown(odt::read(&path, view.offset, cancel)?),
            Engine::Pandoc(tool, format) => {
                self.pandoc(req, &path, &tool, format, Bounds::of(view), cancel)?
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
        let path = req.entry.resolve()?;
        let bytes = match self.engine(&path) {
            Some(Engine::Word | Engine::OpenDocument) => {
                package::Package::open(&path, cancel)?.read(key, "Embedded image", cancel)?
            }
            Some(Engine::Pandoc(tool, format)) => {
                let name = pandoc::media_name(key)
                    .ok_or_else(|| anyhow::anyhow!("not a picture of this document"))?;
                let cache_key = Self::media_key(req, &path, &tool, &name)?;
                if let Some(bytes) = self.cache.get(&cache_key) {
                    bytes
                } else {
                    // Converted again: every picture is kept again, not
                    // only this one, so the others do not each convert.
                    let work = tempfile::tempdir()?;
                    tool.convert(&path, format, work.path(), Bounds::of(view), cancel)?;
                    let mut wanted = None;
                    let mut kept = 0u64;
                    for (each, file) in pandoc::extracted(work.path()) {
                        let Ok(bytes) = inspection::read_bounded(
                            &file,
                            limits::BINARY_INPUT,
                            "Embedded image",
                            cancel,
                        ) else {
                            continue;
                        };
                        if let Ok(key) = Self::media_key(req, &path, &tool, &each)
                            && self.cache.put(&key, &bytes).is_ok()
                        {
                            kept += bytes.len() as u64;
                        }
                        if each == name {
                            wanted = Some(bytes);
                        }
                    }
                    if kept > 0 {
                        self.cache.trim_soon(kept, CACHE_BYTES);
                    }
                    wanted.ok_or_else(|| anyhow::anyhow!("not a picture of this document"))?
                }
            }
            None => return Ok(None),
        };
        Ok(Some(inspection::image(&bytes)?))
    }
}

#[cfg(test)]
mod tests;
