//! Optional documents previews, registered independently of the UI toolkit,
//! and thumbnails for a desktop that has no thumbnailer for them: PDFs, and
//! Office documents from the picture they keep or once the preview has
//! converted them.
//!
//! A text document (Word, `OpenDocument`, RTF) is shown as pages by
//! `LibreOffice`, or read as text by bibli or by Pandoc, as the Text
//! documents setting says and what is installed allows (ADR-030).

mod blocks;
mod docx;
mod odt;
mod package;
mod pandoc;
mod stored;

use bibli_core::{
    App, CancelToken,
    cache::CacheSlot,
    inspection::{InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::inspection::{self, limits};
use bibli_platform::preview::DocumentPreviews;
use bibli_platform::thumb_providers::{OfficePages, PdfPages};
use bibli_platform::{process, rich_preview, sandbox};
use pandoc::Pandoc;
use std::path::Path;
use std::sync::Arc;

const ID: &str = "preview-documents";
/// How Word, `OpenDocument` and RTF files are shown.
const TEXT_DOCUMENTS: &str = "preview-documents.text_documents";

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Preview: documents"
    }
    fn description(&self) -> &'static str {
        "Read PDF and office documents: continuous scrolling, search, text selection, links and contents; Word and OpenDocument text also as text that fits the panel"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn on_settings_changed(&self, app: &App, _keys: &[String]) {
        app.previews.unregister_owner(self.id());
        app.thumbnails.unregister_owner(self.id());
        self.register(app);
    }

    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        use bibli_core::plugin::Requirement;
        use bibli_platform::document::{Engine, worker_path};
        vec![
            Requirement::new("bibli-pdf-worker", "PDF reading with PDFium", || {
                worker_path(Engine::Pdfium).is_some()
            }),
            Requirement::new("bibli-poppler-worker", "PDF reading with Poppler", || {
                worker_path(Engine::Poppler).is_some()
            }),
            Requirement::new("bibli-mupdf-worker", "PDF reading with MuPDF", || {
                worker_path(Engine::Mupdf).is_some()
            }),
            Requirement::program("libreoffice", "Office documents shown as pages"),
            Requirement::program(
                "pandoc",
                "Text documents read by Pandoc, and RTF files read as text",
            ),
            Requirement::program(
                "bwrap",
                "Isolation of LibreOffice and Pandoc, which both need it",
            ),
        ]
    }

    fn register(&self, app: &App) {
        use bibli_core::settings::SettingSpec;
        app.settings.declare(SettingSpec::choice(
            "preview-documents.backend",
            "Document engine",
            "Automatic prefers PDFium and falls back to Poppler; MuPDF is used only when chosen.",
            "auto",
            &[
                ("auto", "Automatic"),
                ("pdfium", "PDFium"),
                ("poppler", "Poppler"),
                ("mupdf", "MuPDF"),
            ],
        ));
        app.settings.declare(SettingSpec::path(
            "preview-documents.pdfium_library",
            "PDFium library",
            "Path to the PDFium shared library. Leave empty to search the usual locations.",
            "",
        ));
        app.settings.declare(SettingSpec::choice(
            TEXT_DOCUMENTS,
            "Text documents",
            "How Word, OpenDocument and RTF documents are shown. Reading shows their text, headings, lists, tables, notes and pictures at once, to fit the panel: bibli reads Word and OpenDocument files, Pandoc RTF. Pages lays them out as LibreOffice prints them, which takes a few seconds each. Pandoc has Pandoc read them all. When what a choice needs is not installed, another is used.",
            "reading",
            &[
                ("reading", "Reading"),
                ("pages", "Pages"),
                ("pandoc", "Pandoc"),
            ],
        ));
        let backend = app
            .settings
            .get::<String>("preview-documents.backend")
            .unwrap_or_else(|| "auto".into());
        let library = app
            .settings
            .get::<String>("preview-documents.pdfium_library")
            .filter(|s| !s.is_empty())
            .map(Into::into);
        let choice = match app.settings.get::<String>(TEXT_DOCUMENTS).as_deref() {
            Some("pages") => Choice::Pages,
            Some("pandoc") => Choice::Pandoc,
            _ => Choice::Reading,
        };
        let cache = app.cache.namespace(self.id());
        app.previews.register(
            self.id(),
            Arc::new(Reading {
                choice,
                cache: cache.clone(),
            }),
        );
        // A document converted for its preview has a thumbnail from then on:
        // a tile that had none asks again.
        let thumbnails = Arc::clone(&app.thumbnails);
        app.previews.register(
            self.id(),
            Arc::new(
                DocumentPreviews::new(cache.clone())
                    .with_reader(backend, library)
                    .on_converted(Arc::new(move |id| thumbnails.retry(id))),
            ),
        );
        // A fallback stage: the desktop's cache and thumbnailers come first
        // whichever plugin registers first. The picture a document keeps
        // comes before the first page of one converted.
        app.thumbnails.register(self.id(), Arc::new(PdfPages));
        app.thumbnails
            .register(self.id(), Arc::new(stored::StoredPictures));
        app.thumbnails
            .register(self.id(), Arc::new(OfficePages::new(cache)));
    }
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

/// Whether `LibreOffice` can show `path` as pages here.
fn as_pages(path: &Path) -> bool {
    bibli_platform::preview::is_office(path)
        && sandbox::available()
        && process::required("libreoffice").is_ok()
}

/// bibli's own reader for `path`, if it reads its format.
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
            Choice::Pages if as_pages(path) => None,
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
        bibli_platform::preview::made_key(req.entry.id, path, &tool.path, ("pandoc-media", name))
    }

    fn pandoc(
        &self,
        req: &PreviewRequest,
        path: &Path,
        tool: &Pandoc,
        format: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Inspection> {
        let work = tempfile::tempdir()?;
        let (html, cut) = tool.convert(path, format, work.path(), cancel)?;
        let mut kept = false;
        let keep = |name: &str, bytes: &[u8]| {
            if let Ok(key) = Self::media_key(req, path, tool, name)
                && self.cache.put(&key, bytes).is_ok()
            {
                kept = true;
            }
        };
        let media = pandoc::Media {
            work: work.path(),
            keep,
            cancel,
        };
        let page = rich_preview::html(&html, rich_preview::Markup::Html, cancel, media)?;
        if kept {
            let _ = self.cache.trim_to_bytes(CACHE_BYTES);
        }
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
                .then(|| format!("First {} blocks", limits::BLOCKS)),
        ];
        Ok(Inspection {
            kind: InspectionKind::Markdown,
            blocks: page.blocks,
            notice: notice.into_iter().flatten().collect::<Vec<_>>().join(" · "),
            details: details(&facts, words),
            ..Inspection::default()
        })
    }
}

/// The plugin's cache namespace is bounded as the document preview's is.
const CACHE_BYTES: u64 = 128 * 1024 * 1024;

/// What a Word or `OpenDocument` file says about itself, for Details; none
/// for another format.
fn about(path: &Path, cancel: &CancelToken) -> Vec<(&'static str, String)> {
    let read = match builtin(path) {
        Some(Engine::Word) => docx::about(path, cancel),
        Some(Engine::OpenDocument) => odt::about(path, cancel),
        _ => return Vec::new(),
    };
    read.unwrap_or_default()
}

/// Details in their order: what the document states, and its words.
fn details(facts: &[(&'static str, String)], words: usize) -> Vec<bibli_core::inspection::Detail> {
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

/// A document bibli read, as the reading mode shows it: one part of it,
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
        "preview-documents.reading"
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
            Engine::Pandoc(tool, format) => self.pandoc(req, &path, &tool, format, cancel)?,
        }))
    }
    /// A picture the document named: a member of its package, read through
    /// the same bounded reader; or one Pandoc extracted, from the cache, or
    /// extracted again when the cache let it go.
    fn picture(
        &self,
        req: &PreviewRequest,
        _view: &InspectRequest,
        key: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<bibli_core::thumbs::Image>> {
        let path = req.entry.resolve()?;
        let bytes = match self.engine(&path) {
            Some(Engine::Word | Engine::OpenDocument) => {
                package::Package::open(&path, cancel)?.read(key, "Embedded image", cancel)?
            }
            Some(Engine::Pandoc(tool, format)) => {
                let name = pandoc::media_name(&format!("/work/{key}"))
                    .ok_or_else(|| anyhow::anyhow!("not a picture of this document"))?;
                let cache_key = Self::media_key(req, &path, &tool, &name)?;
                if let Some(bytes) = self.cache.get(&cache_key) {
                    bytes
                } else {
                    let work = tempfile::tempdir()?;
                    tool.convert(&path, format, work.path(), cancel)?;
                    let bytes = inspection::read_bounded(
                        &work.path().join(&name),
                        limits::BINARY_INPUT,
                        "Embedded image",
                        cancel,
                    )?;
                    if self.cache.put(&cache_key, &bytes).is_ok() {
                        let _ = self.cache.trim_to_bytes(CACHE_BYTES);
                    }
                    bytes
                }
            }
            None => return Ok(None),
        };
        Ok(Some(inspection::image(&bytes)?))
    }
}

#[cfg(test)]
mod tests;
