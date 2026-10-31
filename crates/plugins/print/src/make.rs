//! What a file is printed as (ADR-111): the PDF it is, or the one its
//! preview's reader reads (an office document converted), else a PDF the
//! print helper makes of what its preview shows: its provider's
//! inspection, its pages drawn, a picture, a drawing, its text. Then the
//! pages chosen, in the order and on the sides chosen.
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use skwir_core::App;
use skwir_core::cancel::CancelToken;
use skwir_core::document::Compose;
use skwir_core::inspection::{InspectRequest, InspectionKind, Picture};
use skwir_core::op::EntryRef;
use skwir_core::preview::{HeldSlot, PreviewRequest};
use skwir_core::print::{self as paper, Face, Job, PageSet, Pages, Paper, Part};
use skwir_platform::print_job::{self, Roles};

/// The pages of a document drawn as pictures are drawn this large on
/// their longer side: the most the readers draw (`document::MAX_SIDE`),
/// about 350 dpi on A4.
const PAGE_SIDE: u32 = skwir_core::document::MAX_SIDE;

/// What the preview shows of the file besides the file: the page it is on
/// (from 1), what is selected in it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Shown {
    pub page: Option<u32>,
    pub selection: Option<String>,
}

/// How what is printed is laid out and which of it.
#[derive(Clone, Debug)]
pub(crate) struct Layout {
    pub paper: Paper,
    pub pages: Pages,
    pub page_set: PageSet,
    pub reverse: bool,
}

/// A PDF ready to print or keep.
pub(crate) struct Ready {
    pub pdf: PathBuf,
    pub pages: u32,
    /// What was left out (a text cut at its bound), to say.
    pub notice: Option<String>,
}

/// The PDF of `path` as `layout` asks, made in `work` (a folder of its
/// own the caller removes): the file itself when it is printed whole as
/// it is, else a new one.
pub(crate) fn ready(
    app: &App,
    path: &Path,
    shown: &Shown,
    layout: &Layout,
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Ready> {
    let (pdf, count, notice) = match source(app, path, shown, work, cancel)? {
        Source::Pdf(pdf, count) => (pdf, count, None),
        Source::Parts(parts, notice) => {
            let mut job = job(app, path, &layout.paper, parts);
            let made = print_job::make(&mut job, work, cancel)?;
            (job.output, made.pages, notice)
        }
    };
    let current = shown.page.and_then(|page| page.checked_sub(1));
    // What is selected was made into its own document: all of it.
    let picked = layout.pages.pick(count, current);
    anyhow::ensure!(
        !picked.pages.is_empty(),
        "No page of it is among those asked"
    );
    let mut order = layout.page_set.keep(&picked.pages);
    if layout.reverse {
        order.reverse();
    }
    let notice = notice.or_else(|| {
        picked
            .clipped
            .then(|| format!("It has {count} pages; those past them were left out"))
    });
    // Printed whole, in order: the file as it is.
    if order.len() == count as usize
        && order
            .iter()
            .enumerate()
            .all(|(at, &page)| at as u32 == page)
    {
        return Ok(Ready {
            pdf,
            pages: count,
            notice,
        });
    }
    let out = work.join("pages.pdf");
    let pages = skwir_platform::document::compose(
        &pdf,
        Compose::Pages(order.iter().copied().map(Some).collect()),
        None,
        &out,
        cancel,
    )?
    .context("Printing its pages needs PDFium, which is not there")?;
    Ok(Ready {
        pdf: out,
        pages,
        notice,
    })
}

/// What is printed before its pages are picked.
enum Source {
    /// A PDF, and how many pages it has.
    Pdf(PathBuf, u32),
    /// Parts the helper makes a PDF of, and what was left out of them.
    Parts(Vec<Part>, Option<String>),
}

/// What `path` is printed as, as its preview shows it: the order the
/// window asks providers in (`converted`, `inspect`, a reader's pages),
/// then what the window shows itself (a drawing, a picture, a text).
fn source(
    app: &App,
    path: &Path,
    shown: &Shown,
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Source> {
    let mime = skwir_platform::mime::of_request(path, None).map(|mime| mime.as_str().to_owned());
    let request = request(path, mime.clone())?;
    if let Some(selection) = &shown.selection {
        return Ok(Source::Parts(vec![selected(path, selection)], None));
    }
    if mime.as_deref() == Some("application/pdf") {
        let count = page_count(path, cancel)?;
        return Ok(Source::Pdf(path.to_owned(), count));
    }
    if let Some(converted) = app.previews.converted(&request, cancel)? {
        // A plugin's own file in its place: printed as that file is.
        return source(app, &converted.path, shown, work, cancel);
    }
    let view = InspectRequest::default();
    if let Some(inspection) = app.previews.inspect(&request, &view, cancel)? {
        if inspection.kind == InspectionKind::Font {
            return Ok(Source::Parts(vec![specimen(path)], None));
        }
        let palette = print_job::light_palette(app);
        let mut written = 0usize;
        let mut pictures = |picture: &Picture| -> Option<PathBuf> {
            let image = match picture {
                Picture::Ready(image) => image.clone(),
                Picture::Later { key, .. } => app
                    .previews
                    .picture(&request, &view, key, cancel)
                    .ok()
                    .flatten()?,
            };
            written += 1;
            let png = work.join(format!("picture-{written}.png"));
            write_png(&image, &png).ok()?;
            Some(png)
        };
        let parts = print_job::parts(&inspection, &palette, &mut pictures, cancel)?;
        if !parts.parts.is_empty() {
            return Ok(Source::Parts(parts.parts, parts.notice));
        }
    }
    if let Some(session) = app.previews.open_document(&request, cancel)? {
        if let Some(pdf) = session.pdf() {
            // Kept while the session lives: copied for printing.
            let copy = work.join("document.pdf");
            std::fs::copy(pdf, &copy).context("Keeping the document's PDF to print it")?;
            return Ok(Source::Pdf(copy, session.info().pages));
        }
        return Ok(Source::Parts(vec![drawn(&*session, work, cancel)?], None));
    }
    builtin(path, mime.as_deref(), &request, work, cancel)
}

/// What the window shows itself: a drawing, a picture, a text.
fn builtin(
    path: &Path,
    mime: Option<&str>,
    request: &PreviewRequest,
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Source> {
    let name = display_name(path);
    if matches!(mime, Some("image/svg+xml" | "image/svg+xml-compressed")) {
        return Ok(Source::Parts(
            vec![Part::Drawing {
                svg: path.to_owned(),
                alt: name,
            }],
            None,
        ));
    }
    let image = mime
        .map(|mime| skwir_platform::mime::Mime(mime.to_owned()))
        .is_some_and(|mime| {
            skwir_platform::mime::is_kind(&mime, skwir_platform::mime::Kind::Image)
        });
    if image {
        return Ok(Source::Parts(vec![picture(path, mime, work)?], None));
    }
    let held = request.held()?;
    if skwir_platform::inspection::is_text(held.head()) {
        let inspection = skwir_platform::inspection::plain(&held, cancel)?;
        let palette = skwir_platform::highlight::Palette::of(
            &skwir_platform::highlight::CodeTheme::own(false),
        );
        let parts = print_job::parts(&inspection, &palette, &mut |_| None, cancel)?;
        return Ok(Source::Parts(parts.parts, parts.notice));
    }
    anyhow::bail!("“{name}” shows nothing that can be printed")
}

/// A request about `path`, holding it whole as a Full preview does.
fn request(path: &Path, mime: Option<String>) -> anyhow::Result<PreviewRequest> {
    Ok(PreviewRequest {
        entry: EntryRef::from_path(path)
            .with_context(|| format!("“{}” is not there", display_name(path)))?,
        mime,
        page: 1,
        seconds: None,
        max_side: PAGE_SIDE,
        text_size: None,
        fit: None,
        held: HeldSlot::keeping(skwir_core::preview::WHOLE_FULL),
    })
}

/// How many pages the PDF at `path` has, as `PDFium` reads it.
fn page_count(path: &Path, cancel: &CancelToken) -> anyhow::Result<u32> {
    let session = skwir_platform::document::open(
        path,
        &[skwir_platform::document::Engine::Pdfium],
        &cancel.child(),
    )?
    .context("Printing a PDF needs PDFium, which is not there")?;
    Ok(session.info().pages)
}

/// What is selected, as its own document: lines in the code font, colored
/// as the file's language.
fn selected(path: &Path, selection: &str) -> Part {
    let language = path
        .extension()
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().into_owned());
    let palette =
        skwir_platform::highlight::Palette::of(&skwir_platform::highlight::CodeTheme::own(false));
    let mut colors = skwir_platform::highlight::Highlighter::new(language.as_deref(), &palette);
    let lines = selection
        .split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let runs = colors
                .line(line)
                .into_iter()
                .map(|run| paper::Run {
                    start: u32::try_from(run.range.start).unwrap_or(u32::MAX),
                    end: u32::try_from(run.range.end).unwrap_or(u32::MAX),
                    color: run.color,
                    bold: run.bold,
                    italic: run.italic,
                    ..paper::Run::default()
                })
                .collect();
            paper::Line {
                text: line.to_owned(),
                runs,
            }
        })
        .collect();
    Part::Code {
        lines,
        numbers: false,
        first: 1,
    }
}

/// A font shown in itself.
fn specimen(path: &Path) -> Part {
    Part::Specimen {
        face: Face {
            path: path.to_owned(),
            index: 0,
        },
        name: display_name(path),
        sample: "The quick brown fox jumps over the lazy dog 0123456789".to_owned(),
        sizes: vec![10.0, 14.0, 20.0, 32.0, 48.0],
    }
}

/// A picture: a JPEG, PNG, GIF or WebP as it is; another decoded and
/// written as a PNG.
fn picture(path: &Path, mime: Option<&str>, work: &Path) -> anyhow::Result<Part> {
    let alt = display_name(path);
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let embedded = matches!(
        mime,
        Some("image/jpeg" | "image/png" | "image/gif" | "image/webp")
    );
    let (path, (width, height)) = if embedded {
        (path.to_owned(), reader.into_dimensions()?)
    } else {
        let decoded = skwir_platform::picture::decode(reader)?;
        let png = work.join("picture.png");
        decoded.save_with_format(&png, image::ImageFormat::Png)?;
        (png, (decoded.width(), decoded.height()))
    };
    Ok(Part::Pictures {
        pictures: vec![paper::Picture {
            path,
            width,
            height,
            alt,
            caption: None,
        }],
        sheet: false,
    })
}

/// A document's pages drawn as pictures, each written as a PNG.
fn drawn(
    session: &dyn skwir_core::document::DocumentSession,
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Part> {
    let mut pages = Vec::new();
    for page in 1..=session.info().pages {
        anyhow::ensure!(!cancel.is_cancelled(), "Printing was cancelled");
        let drawn = session.render(page, PAGE_SIDE, cancel)?;
        let (width, height) = (drawn.width, drawn.height);
        let image = skwir_core::thumbs::Image {
            width,
            height,
            rgba: drawn.into_rgba().into(),
        };
        let png = work.join(format!("page-{page}.png"));
        write_png(&image, &png)?;
        pages.push(paper::Picture {
            path: png,
            width,
            height,
            alt: format!("Page {page}"),
            caption: None,
        });
    }
    Ok(Part::Pages { pages })
}

fn write_png(image: &skwir_core::thumbs::Image, path: &Path) -> anyhow::Result<()> {
    let buffer = image::RgbaImage::from_raw(image.width, image.height, image.rgba.to_vec())
        .context("a picture of the wrong size")?;
    buffer.save_with_format(path, image::ImageFormat::Png)?;
    Ok(())
}

/// The job of `parts`, laid out on `paper` in the fonts the Reading page
/// chooses (the desktop's where it says Automatic), in the light look.
fn job(app: &App, path: &Path, paper: &Paper, parts: Vec<Part>) -> Job {
    let style = skwir_platform::desktop_style::read();
    let chosen = |key: &str, desktop: Option<String>, generic: &str| {
        app.settings
            .get::<String>(key)
            .filter(|family| !family.is_empty() && family != "auto")
            .or(desktop)
            .unwrap_or_else(|| generic.to_owned())
    };
    let roles = Roles {
        text: chosen("reading.font", style.document_font.clone(), "serif"),
        code: chosen("ui.code_font", style.monospace_font.clone(), "monospace"),
    };
    let (fonts, fallback) = print_job::faces(&roles);
    let name = display_name(path);
    Job {
        paper: *paper,
        output: PathBuf::new(),
        title: name.clone(),
        author: None,
        language: None,
        header: Some(name),
        numbers: true,
        fonts,
        fallback,
        pages: None,
        parts,
    }
}

pub(crate) fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
