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
    /// Folded as a booklet, two pages to a side of `paper` turned
    /// landscape.
    pub booklet: Option<Booklet>,
}

/// How a booklet folds (ADR-111): in signatures of `signature` pages (one
/// for all without), its pages' white margins trimmed, its backs turned
/// half a turn for a printer that turns the sheet over its long edge.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
pub struct Booklet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<u32>,
    #[serde(default)]
    pub trim: bool,
    #[serde(default)]
    pub turn_backs: bool,
}

/// What a page of a PDF keeps clear of a booklet sheet's edges, and of its
/// fold, in points (about 6 mm and 4 mm): a page Skwir lays out at half a
/// sheet keeps its own margins instead.
const EDGE: f32 = 18.0;
const FOLD: f32 = 12.0;

/// A PDF ready to print or keep.
pub(crate) struct Ready {
    pub pdf: PathBuf,
    pub pages: u32,
    /// What was left out (a text cut at its bound), to say.
    pub notice: Option<String>,
}

/// The PDF of `paths` as `layout` asks, made in `work` (a folder of its
/// own the caller removes): the file itself when one is printed whole as
/// it is, else a new one. Several files are printed as one document, each
/// whole, in their order (`joined`); what the preview shows of one
/// (`shown`) is only ever one file's.
pub(crate) fn ready(
    app: &App,
    paths: &[PathBuf],
    shown: &Shown,
    layout: &Layout,
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Ready> {
    // A booklet's page is half its sheet: what Skwir lays out is laid out
    // at that size, its text not shrunk.
    let paper = page_paper(layout);
    let (whole, shown) = match paths {
        [path] => (
            whole(app, path, shown, &paper, work, cancel)?,
            shown.clone(),
        ),
        _ => (joined(app, paths, &paper, work, cancel)?, Shown::default()),
    };
    picked(whole, &shown, layout, work, cancel)
}

/// A document Skwir makes of `parts` (a contact sheet, a folder's
/// listing), titled `title`, as `layout` asks.
pub(crate) fn made(
    app: &App,
    title: &str,
    parts: Vec<Part>,
    layout: &Layout,
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Ready> {
    let mut job = job(app, title, &page_paper(layout), parts);
    let made = print_job::make(&mut job, work, cancel)?;
    let whole = Whole {
        pdf: job.output,
        pages: made.pages,
        notice: None,
        made: true,
    };
    picked(whole, &Shown::default(), layout, work, cancel)
}

/// The paper a page is laid out on: a booklet's is half its sheet, so
/// what Skwir lays out is not shrunk.
fn page_paper(layout: &Layout) -> Paper {
    match layout.booklet {
        Some(_) => layout.paper.half(),
        None => layout.paper,
    }
}

/// The pages of `whole` that `layout` picks, folded as a booklet when it
/// says, on the sides and in the order it chose.
fn picked(
    whole: Whole,
    shown: &Shown,
    layout: &Layout,
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Ready> {
    let Whole {
        pdf,
        pages: count,
        notice,
        made,
    } = whole;
    // A page of a PDF keeps clear of a booklet sheet's edges and fold; one
    // Skwir laid out at half a sheet has its own margins.
    let (edge, fold) = if made { (0.0, 0.0) } else { (EDGE, FOLD) };
    let current = shown.page.and_then(|page| page.checked_sub(1));
    // What is selected was made into its own document: all of it.
    let picked = layout.pages.pick(count, current);
    anyhow::ensure!(
        !picked.pages.is_empty(),
        "No page of it is among those asked"
    );
    let notice = notice.or_else(|| {
        picked
            .clipped
            .then(|| format!("It has {count} pages; those past them were left out"))
    });
    // A booklet: the pages picked folded in their order, then the sides
    // and the order chosen applied to its sheets' sides.
    let (pdf, count, picked) = match layout.booklet {
        Some(booklet) => {
            let sheets = paper::booklet::sheets(&picked.pages, booklet.signature);
            let sheet = layout.paper.landscape();
            let folded = work.join("booklet.pdf");
            let sides = skwir_platform::document::compose(
                &pdf,
                Compose::Booklet {
                    sheets,
                    width: sheet.width,
                    height: sheet.height,
                    fold,
                    edge,
                    trim: booklet.trim,
                    turn_backs: booklet.turn_backs,
                },
                None,
                &folded,
                cancel,
            )?
            .context("Folding a booklet needs PDFium, which is not there")?;
            (folded, sides, (0..sides).collect())
        }
        None => (pdf, count, picked.pages),
    };
    let mut order = layout.page_set.keep(&picked);
    if layout.reverse {
        order.reverse();
    }
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

/// A file printed whole: its PDF and pages, what was left out of it, and
/// whether Skwir made it (laid out at the size asked, with its margins).
struct Whole {
    pdf: PathBuf,
    pages: u32,
    notice: Option<String>,
    made: bool,
}

/// `path` as its preview shows it, whole, made on `paper` in `work` when
/// Skwir makes it.
fn whole(
    app: &App,
    path: &Path,
    shown: &Shown,
    paper: &Paper,
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Whole> {
    Ok(match source(app, path, shown, work, cancel)? {
        Source::Pdf(pdf, pages) => Whole {
            pdf,
            pages,
            notice: None,
            made: false,
        },
        Source::Parts(parts, notice) => {
            let mut job = job(app, &display_name(path), paper, parts);
            let made = print_job::make(&mut job, work, cancel)?;
            Whole {
                pdf: job.output,
                pages: made.pages,
                notice,
                made: true,
            }
        }
    })
}

/// Several files as one document, each whole as its preview shows it, in
/// their order, each made in a folder of its own in `work`, then joined
/// by the PDF helper (`Compose::Append`). One that cannot be printed (a
/// song) is left out and said; none, and that is the error.
fn joined(
    app: &App,
    paths: &[PathBuf],
    paper: &Paper,
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Whole> {
    let mut wholes: Vec<Whole> = Vec::with_capacity(paths.len());
    let mut said = Vec::new();
    for (n, path) in paths.iter().enumerate() {
        anyhow::ensure!(!cancel.is_cancelled(), "Printing was cancelled");
        let own = work.join(n.to_string());
        std::fs::create_dir(&own)?;
        let name = display_name(path);
        match whole(app, path, &Shown::default(), paper, &own, cancel) {
            Ok(mut part) => {
                if let Some(notice) = part.notice.take() {
                    said.push(format!("“{name}”: {notice}"));
                }
                // The helper is told the documents' paths as text.
                if part.pdf.to_str().is_none() {
                    let named = own.join("named.pdf");
                    std::fs::hard_link(&part.pdf, &named)
                        .or_else(|_| std::fs::copy(&part.pdf, &named).map(drop))
                        .with_context(|| format!("Keeping “{name}” to print it"))?;
                    part.pdf = named;
                }
                wholes.push(part);
            }
            Err(error) if !cancel.is_cancelled() => {
                said.push(format!("“{name}” was left out: {error:#}"));
            }
            Err(error) => return Err(error),
        }
    }
    let notice = (!said.is_empty()).then(|| said.join(". "));
    let Some(first) = wholes.first() else {
        anyhow::bail!("None of them can be printed. {}", said.join(". "));
    };
    if wholes.len() == 1 {
        let only = wholes.remove(0);
        return Ok(Whole { notice, ..only });
    }
    let out = work.join("joined.pdf");
    let pages = skwir_platform::document::compose(
        &first.pdf,
        Compose::Append(wholes[1..].iter().map(|whole| whole.pdf.clone()).collect()),
        None,
        &out,
        cancel,
    )?
    .context("Printing several files as one needs PDFium, which is not there")?;
    Ok(Whole {
        pdf: out,
        pages,
        notice,
        made: wholes.iter().all(|whole| whole.made),
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

/// Pictures of a contact sheet are written this large on their longer
/// side: about 300 dpi at a third of A4's width, far less than a photo.
const SHEET_SIDE: u32 = 800;

/// A contact sheet of the pictures among `paths`, in their order, each
/// shrunk (`SHEET_SIDE`) and written in `work`, named under it; what is no
/// picture or cannot be read is left out, and said.
pub(crate) fn sheet(
    paths: &[PathBuf],
    work: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<(Part, Option<String>)> {
    let mut pictures = Vec::with_capacity(paths.len());
    let mut left_out = Vec::new();
    for (n, path) in paths.iter().enumerate() {
        anyhow::ensure!(!cancel.is_cancelled(), "Printing was cancelled");
        let name = display_name(path);
        let image = skwir_platform::mime::of_request(path, None).is_some_and(|mime| {
            skwir_platform::mime::is_kind(&mime, skwir_platform::mime::Kind::Image)
        });
        let read = image
            .then(|| sheet_picture(path, &work.join(format!("sheet-{n}")), &name))
            .transpose();
        match read {
            Ok(Some(picture)) => pictures.push(picture),
            Ok(None) => left_out.push(name),
            Err(error) => {
                tracing::debug!(%error, "a picture of a contact sheet");
                left_out.push(name);
            }
        }
    }
    anyhow::ensure!(!pictures.is_empty(), "None of them is a picture");
    let notice = match left_out.as_slice() {
        [] => None,
        [one] => Some(format!("“{one}” is not a picture and was left out")),
        many => Some(format!(
            "{} files are not pictures and were left out",
            many.len()
        )),
    };
    Ok((
        Part::Pictures {
            pictures,
            sheet: true,
        },
        notice,
    ))
}

/// A picture of a contact sheet: decoded, turned, shrunk, written as a
/// JPEG at `stem` (a PNG where it is see-through).
fn sheet_picture(path: &Path, stem: &Path, name: &str) -> anyhow::Result<paper::Picture> {
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let image = skwir_platform::picture::decode_fitted(reader, SHEET_SIDE)?;
    let (width, height) = (image.width(), image.height());
    let written = if image.color().has_alpha() {
        let png = stem.with_extension("png");
        image.save_with_format(&png, image::ImageFormat::Png)?;
        png
    } else {
        let jpeg = stem.with_extension("jpg");
        let file = std::io::BufWriter::new(std::fs::File::create(&jpeg)?);
        image
            .to_rgb8()
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(file, 85))?;
        jpeg
    };
    Ok(paper::Picture {
        path: written,
        width,
        height,
        alt: name.to_owned(),
        caption: Some(name.to_owned()),
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

/// The job of `parts`, titled `name`, laid out on `paper` in the fonts the
/// Reading page chooses (the desktop's where it says Automatic), in the
/// light look.
fn job(app: &App, name: &str, paper: &Paper, parts: Vec<Part>) -> Job {
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
    let name = name.to_owned();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A contact sheet takes the pictures among the files, shrunk and
    /// named, a see-through one kept see-through, and says what it left.
    #[test]
    fn a_contact_sheet_takes_the_pictures_shrunk_and_says_what_it_left() {
        let tmp = tempfile::tempdir().unwrap();
        let (big, clear, text) = (
            tmp.path().join("wide photo.png"),
            tmp.path().join("clear.png"),
            tmp.path().join("notes.txt"),
        );
        image::RgbImage::from_pixel(2000, 1000, image::Rgb([200, 30, 30]))
            .save(&big)
            .unwrap();
        image::RgbaImage::from_pixel(10, 20, image::Rgba([0, 0, 0, 0]))
            .save(&clear)
            .unwrap();
        std::fs::write(&text, "not a picture").unwrap();
        let work = tmp.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let cancel = CancelToken::new();
        let (part, notice) = sheet(&[big, text.clone(), clear], &work, &cancel).unwrap();
        let Part::Pictures {
            pictures,
            sheet: true,
        } = part
        else {
            panic!("a contact sheet");
        };
        assert_eq!(pictures.len(), 2);
        assert_eq!((pictures[0].width, pictures[0].height), (SHEET_SIDE, 400));
        assert_eq!(pictures[0].caption.as_deref(), Some("wide photo.png"));
        assert!(pictures[0].path.starts_with(&work));
        assert_eq!(
            pictures[0].path.extension().and_then(|e| e.to_str()),
            Some("jpg")
        );
        assert_eq!(
            pictures[1].path.extension().and_then(|e| e.to_str()),
            Some("png"),
            "see-through, kept so"
        );
        assert!(notice.unwrap().contains("notes.txt"));
        assert!(sheet(&[text], &work, &cancel).is_err(), "no picture");
    }
}
