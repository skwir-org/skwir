//! `PDFium` runs in this optional child process, never in the GPU/UI process.
//! A document stays open for the worker's lifetime; each request releases pages
//! and bitmaps. The parent enforces cancellation, time and output limits.

use std::path::Path;

use anyhow::Context;
use pdfium_render::prelude::*;
use skwir_core::document::{
    COMPOSED_FILE, CSS_PER_POINT, Compose, Destination, DocumentInfo, FIRST_PAGE, Link, MAX_HITS,
    MAX_PAGE_CHARS, MAX_SIDE, Outline, PIXELS_FILE, PageContent, PixelOrder, Rect, SearchHit,
    TextChar, WorkerReply, WorkerRequest, page_labels, worker,
};

mod print;

fn main() {
    // `--build`, skwir asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--build")
    {
        println!("{}", env!("SKWIR_BUILD"));
        return;
    }
    // `--probe`, skwir's settings asking whether this helper works here
    // (`platform::process::probe`): PDFium's library, which it opens
    // itself, is there.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--probe")
    {
        if let Err(error) = bindings() {
            eprintln!("PDFium library unavailable: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run() {
        worker::fail(&error);
    }
}

/// `PDFium`'s library: the one named (`SKWIR_PDFIUM_LIBRARY`: the
/// settings'), else the one a package carries for this helper (ADR-041),
/// else the system's.
fn bindings() -> Result<Box<dyn PdfiumLibraryBindings>, PdfiumError> {
    if let Some(path) = std::env::var_os("SKWIR_PDFIUM_LIBRARY") {
        return Pdfium::bind_to_library(path);
    }
    let carried = std::env::current_exe().ok().and_then(|program| {
        carried(program.parent()?, &Pdfium::pdfium_platform_library_name())
            .into_iter()
            .find(|path| path.is_file())
    });
    match carried {
        Some(path) => Pdfium::bind_to_library(path),
        None => Pdfium::bind_to_system_library(),
    }
}

/// Where a package puts `PDFium`'s library, named `library` as this system
/// names it (`libpdfium.so`, `libpdfium.dylib`, `pdfium.dll`), for a
/// helper in `folder`: beside it (Linux's /usr/lib/skwir, Windows'
/// program folder), and a Mac bundle's `Frameworks` beside its `MacOS`.
fn carried(folder: &Path, library: &std::ffi::OsStr) -> [std::path::PathBuf; 2] {
    let frameworks = folder.parent().map_or_else(
        || folder.join("Frameworks"),
        |bundle| bundle.join("Frameworks"),
    );
    [folder.join(library), frameworks.join(library)]
}

fn run() -> anyhow::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let first_page = args.get(2).is_some_and(|arg| arg == FIRST_PAGE);
    anyhow::ensure!(
        args.len() == 2 || (args.len() == 3 && first_page),
        "expected PDF path, scratch directory and maybe {FIRST_PAGE}"
    );
    let bindings = match bindings() {
        Ok(bindings) => bindings,
        Err(error) => {
            return worker::greet(&WorkerReply::Unavailable(format!(
                "PDFium library unavailable: {error}"
            )));
        }
    };
    // pdfium-render 0.9 keeps the bindings it is handed to itself (its
    // issue 279) and reads no page labels: they are asked through a second
    // binding to the same library, made before it takes the first.
    let labelling = if first_page {
        None
    } else {
        self::bindings().ok()
    };
    let pdfium = Pdfium::new(bindings);
    let document = pdfium
        .load_pdf_from_file(Path::new(&args[0]), None)
        .context("open PDF (encrypted files require an external reader)")?;
    let info = document_info(&document, first_page, labelling.as_deref());
    anyhow::ensure!(info.pages > 0, "document has no pages");
    worker::greet(&WorkerReply::Ready(info))?;
    let scratch = Path::new(&args[1]);
    worker::serve(|request, superseded| match request {
        WorkerRequest::Render { page, side } => render(&document, page, side, scratch),
        WorkerRequest::Search { query } => {
            search(&document, &query, superseded).map(WorkerReply::Matches)
        }
        // A PDF's pages never move.
        WorkerRequest::Locate { .. } => Ok(WorkerReply::Located(None)),
        WorkerRequest::Compose(compose) => {
            composed(&pdfium, &document, &compose, scratch, superseded)
        }
        WorkerRequest::Print {
            printer,
            devmode,
            title,
        } => {
            // A binding of its own: pdfium-render keeps its page handles.
            let bindings = self::bindings()?;
            let pages = print::print(&*bindings, Path::new(&args[0]), &printer, &devmode, &title)?;
            Ok(WorkerReply::Printed { pages })
        }
        // `serve` answers it: nothing runs by the time it is read.
        WorkerRequest::Cancel => anyhow::bail!("nothing to cancel"),
    })
}

/// A new document of `document`'s pages, as `compose` lists them, written
/// in `scratch` as `COMPOSED_FILE` (ADR-111).
fn composed<'a>(
    pdfium: &'a Pdfium,
    document: &PdfDocument<'a>,
    compose: &Compose,
    scratch: &Path,
    superseded: &dyn Fn() -> bool,
) -> anyhow::Result<WorkerReply> {
    let path = scratch.join(COMPOSED_FILE);
    let pages = match compose {
        Compose::Pages { pages, scale } => {
            let out = listed(pdfium, document, pages)?;
            scaled(&out, *scale, superseded)?;
            out.save_to_file(&path)
                .context("write the composed document")?;
            out.pages().len()
        }
        Compose::Booklet {
            sheets,
            width,
            height,
            fold,
            edge,
            trim,
            turn_backs,
        } => {
            let layout = Layout {
                width: *width,
                height: *height,
                fold: *fold,
                edge: *edge,
                trim: *trim,
                turn_backs: *turn_backs,
            };
            booklet(pdfium, document, sheets, &layout, superseded, &path)?
        }
        Compose::Append(others) => {
            let mut out = pdfium.create_new_pdf().context("make a document")?;
            out.pages_mut()
                .append(document)
                .context("copy the first document's pages")?;
            for other in others {
                anyhow::ensure!(!superseded(), "composing superseded");
                let read = pdfium
                    .load_pdf_from_file(other, None)
                    .with_context(|| format!("open {}", other.display()))?;
                out.pages_mut()
                    .append(&read)
                    .with_context(|| format!("copy the pages of {}", other.display()))?;
            }
            out.save_to_file(&path)
                .context("write the composed document")?;
            out.pages().len()
        }
    };
    Ok(WorkerReply::Composed {
        pages: u32::try_from(pages).unwrap_or(u32::MAX),
    })
}

/// `document`'s pages as `pages` lists them, blanks as large as the page
/// before them.
fn listed<'a>(
    pdfium: &'a Pdfium,
    document: &PdfDocument<'a>,
    pages: &[Option<u32>],
) -> anyhow::Result<PdfDocument<'a>> {
    let count = document.pages().len();
    let mut out = pdfium.create_new_pdf().context("make a document")?;
    // A blank is as large as the page before it, the first page's first.
    let first = document.pages().get(0).context("read the first page")?;
    let mut size = (first.width(), first.height());
    for page in pages {
        let at = out.pages().len();
        match page {
            Some(index) => {
                let index = PdfPageIndex::try_from(*index).context("page out of range")?;
                anyhow::ensure!(index < count, "page {} is past the end", index + 1);
                out.pages_mut()
                    .copy_page_range_from_document(document, index..=index, at)
                    .with_context(|| format!("copy page {}", index + 1))?;
                let copied = document.pages().get(index)?;
                size = (copied.width(), copied.height());
            }
            None => {
                out.pages_mut()
                    .create_page_at_end(PdfPagePaperSize::Custom(size.0, size.1))
                    .context("add a blank page")?;
            }
        }
    }
    Ok(out)
}

/// The largest a print dialog's scale is taken: ten times (GTK's dialog
/// offers up to 1,000 %).
const LARGEST_SCALE: f32 = 10.0;

/// Each page of `document` drawn at `scale` of its size about the middle
/// of what it shows, clipped to that (its media box within its crop box),
/// its annotations drawn into it first, on a page of its own size: the
/// print dialog's scale, which the system's printing leaves to the
/// application. Stops when a newer request waits.
fn scaled(
    document: &PdfDocument<'_>,
    scale: f32,
    superseded: &dyn Fn() -> bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        scale.is_finite() && scale > 0.0 && scale <= LARGEST_SCALE,
        "no scale to print at: {scale}"
    );
    if (scale - 1.0).abs() < 1e-3 {
        return Ok(());
    }
    for (number, mut page) in document.pages().iter().enumerate() {
        if number % 64 == 0 {
            anyhow::ensure!(!superseded(), "composing superseded");
        }
        // Its notes, stamps and form fields drawn into it as they print
        // (`FPDFPage_Flatten`, `FLAT_PRINT`), so they scale with it rather
        // than print at their own size over it.
        page.flatten()?;
        // Its content is rewritten here: never again from its objects.
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        let shown = page.boundaries().bounding()?.bounds;
        let middle = (
            f32::midpoint(shown.left().value, shown.right().value),
            f32::midpoint(shown.bottom().value, shown.top().value),
        );
        page.apply_matrix_with_clip(scale_about(scale, middle), shown)?;
    }
    Ok(())
}

/// What draws a page at `scale` of its size, `middle` staying where it is.
fn scale_about(scale: f32, middle: (f32, f32)) -> PdfMatrix {
    PdfMatrix::new(
        scale,
        0.0,
        0.0,
        scale,
        middle.0 * (1.0 - scale),
        middle.1 * (1.0 - scale),
    )
}

/// How a booklet's sheets are laid out (`Compose::Booklet`).
struct Layout {
    width: f32,
    height: f32,
    fold: f32,
    edge: f32,
    trim: bool,
    turn_backs: bool,
}

/// The most sheets a booklet is made of, four pages each: a request
/// listing more is refused (its line's bound holds fewer anyway).
const MOST_SHEETS: usize = 50_000;
/// The largest a sheet may be, in points: PDF's own limit (200 inches).
const LARGEST_SHEET: f32 = 14_400.0;

/// A box on a page, in points: `x` and `y` its left and bottom (PDF's
/// way, upwards) or its left and top (a drawing's, downwards), as said.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Area {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// The two halves of a sheet of `layout`, left then right, upwards: within
/// `edge` of the sheet's edges and `fold` of its middle, both bounded so
/// each half keeps some room.
fn halves(layout: &Layout) -> [Area; 2] {
    let (width, height) = (layout.width, layout.height);
    let edge = layout.edge.clamp(0.0, width.min(height) / 4.0);
    let fold = layout.fold.clamp(0.0, width / 8.0);
    let half = (width / 2.0 - fold - edge).max(1.0);
    let tall = (height - 2.0 * edge).max(1.0);
    [
        Area {
            x: edge,
            y: edge,
            width: half,
            height: tall,
        },
        Area {
            x: width / 2.0 + fold,
            y: edge,
            width: half,
            height: tall,
        },
    ]
}

/// What draws a page of `width` × `height` (its form's own space, upwards
/// from its bottom left) fitted in `half`, keeping its proportions,
/// centred; turned half a turn about the sheet's centre when `turned`.
fn placed(width: f32, height: f32, half: Area, turned: bool, sheet: (f32, f32)) -> PdfMatrix {
    let scale = (half.width / width.max(0.01)).min(half.height / height.max(0.01));
    let x = half.x + (half.width - width * scale) / 2.0;
    let y = half.y + (half.height - height * scale) / 2.0;
    if turned {
        PdfMatrix::new(-scale, 0.0, 0.0, -scale, sheet.0 - x, sheet.1 - y)
    } else {
        PdfMatrix::new(scale, 0.0, 0.0, scale, x, y)
    }
}

/// A booklet of `document`'s pages, `sheets` in their order, each its
/// front then its back, written at `path`; how many sides it has.
///
/// The pages are copied in the order their halves print in (front left,
/// front right, back left, back right, a sheet after another), blanks
/// where a half has none, in one import, so what they share (fonts,
/// pictures) is copied once; with `trim`, each copy is cut to the box of
/// every page's ink (its crop box set to it). `PDFium` then draws them two
/// to a side as forms of themselves (`FPDF_ImportNPagesToOne`, which
/// copies what they share once again, turns each as its `/Rotate` says
/// and clips it to its crop box), and each form is placed again where
/// `layout` says: in its half, within the edges and off the fold, the
/// back turned half a turn for a printer turning sheets over their long
/// edge. Stops when a newer request waits.
fn booklet(
    pdfium: &Pdfium,
    document: &PdfDocument<'_>,
    sheets: &[skwir_core::print::booklet::Sheet],
    layout: &Layout,
    superseded: &dyn Fn() -> bool,
    path: &Path,
) -> anyhow::Result<PdfPageIndex> {
    anyhow::ensure!(
        layout.width.is_finite()
            && layout.height.is_finite()
            && layout.width >= 72.0
            && layout.height >= 72.0
            && layout.width <= LARGEST_SHEET
            && layout.height <= LARGEST_SHEET,
        "the sheet is no size to print on"
    );
    anyhow::ensure!(!sheets.is_empty(), "a booklet of nothing");
    anyhow::ensure!(sheets.len() <= MOST_SHEETS, "too many sheets");
    let count = document.pages().len();
    // The halves in the order they print.
    let mut order: Vec<Option<PdfPageIndex>> = Vec::with_capacity(sheets.len() * 4);
    for page in sheets
        .iter()
        .flat_map(|sheet| sheet.front.into_iter().chain(sheet.back))
    {
        order.push(match page {
            Some(page) => {
                let index = PdfPageIndex::try_from(page).context("page out of range")?;
                anyhow::ensure!(index < count, "page {} is past the end", index + 1);
                Some(index)
            }
            None => None,
        });
    }
    let ink = if layout.trim {
        let used: std::collections::BTreeSet<PdfPageIndex> =
            order.iter().flatten().copied().collect();
        ink(document, &used, superseded)?
    } else {
        None
    };
    let mut ordered = pdfium.create_new_pdf().context("make a document")?;
    let listed: Vec<String> = order
        .iter()
        .flatten()
        .map(|index| (index + 1).to_string())
        .collect();
    ordered
        .pages_mut()
        .copy_pages_from_document(document, &listed.join(","), 0)
        .context("copy the pages")?;
    let first = document.pages().get(0).context("read the first page")?;
    let blank = PdfPagePaperSize::Custom(first.width(), first.height());
    for (at, page) in order.iter().enumerate() {
        if page.is_none() {
            let at = PdfPageIndex::try_from(at).context("too many pages")?;
            ordered
                .pages_mut()
                .create_page_at_index(blank, at)
                .context("add a blank half")?;
        }
    }
    // Each half's size as it draws: its page as it shows, or cut.
    let mut sizes = Vec::with_capacity(order.len());
    for (at, page) in order.iter().enumerate() {
        if at % 64 == 0 {
            anyhow::ensure!(!superseded(), "composing superseded");
        }
        if page.is_none() {
            sizes.push(None);
            continue;
        }
        let at = PdfPageIndex::try_from(at).context("too many pages")?;
        let copy = ordered.pages().get(at).context("read a page copied")?;
        sizes.push(Some(cut(copy, ink)?));
    }
    anyhow::ensure!(!superseded(), "composing superseded");
    let sheet = (layout.width, layout.height);
    let tiled = ordered
        .pages()
        .tile_into_new_document(
            1,
            2,
            PdfPagePaperSize::Custom(PdfPoints::new(sheet.0), PdfPoints::new(sheet.1)),
        )
        .context("lay the pages two to a side")?;
    let halves = halves(layout);
    for (number, mut side) in tiled.pages().iter().enumerate() {
        if number % 16 == 0 {
            anyhow::ensure!(!superseded(), "composing superseded");
        }
        let turned = layout.turn_backs && number % 2 == 1;
        side.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        {
            let mut forms: Vec<PdfPageObject<'_>> = side
                .objects()
                .iter()
                .filter(|object| object.object_type() == PdfPageObjectType::XObjectForm)
                .collect();
            anyhow::ensure!(
                forms.len() == 2,
                "a side holds {} pages, not two",
                forms.len()
            );
            for (k, form) in forms.iter_mut().enumerate() {
                let Some(Some((width, height))) = sizes.get(number * 2 + k) else {
                    continue;
                };
                let matrix = placed(*width, *height, halves[k], turned, sheet);
                form.as_x_object_form_object_mut()
                    .context("a page's form")?
                    .reset_matrix(matrix)?;
            }
            drop(forms);
        }
        side.regenerate_content()?;
    }
    tiled
        .save_to_file(path)
        .context("write the composed document")?;
    Ok(tiled.pages().len())
}

/// The size `page` draws at, as it shows (from its crop box, turned as its
/// `/Rotate` says), or cut to `ink` (a box in the page as it shows, from
/// its top left) when it holds some of it: its crop box set to that box,
/// which `PDFium` clips its form to.
fn cut(mut page: PdfPage<'_>, ink: Option<Area>) -> anyhow::Result<(f32, f32)> {
    let (width, height) = (page.width().value, page.height().value);
    // The ink's box on this page, within it.
    let within = ink.and_then(|ink| {
        let left = ink.x.max(0.0);
        let top = ink.y.max(0.0);
        let right = (ink.x + ink.width).min(width);
        let bottom = (ink.y + ink.height).min(height);
        (right - left >= 1.0 && bottom - top >= 1.0).then_some(Area {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    });
    let Some(within) = within else {
        return Ok((width, height));
    };
    // The box in the page's own space, through `PDFium`'s mapping of a
    // drawing of it (four pixels a point), which knows its crop box and
    // its turn.
    let config = PdfRenderConfig::new().scale_page_by_factor(4.0);
    let corner = |x: f32, y: f32| {
        page.pixels_to_points((x * 4.0).round() as i32, (y * 4.0).round() as i32, &config)
    };
    let (x0, y0) = corner(within.x, within.y)?;
    let (x1, y1) = corner(within.x + within.width, within.y + within.height)?;
    let (x0, y0, x1, y1) = (x0.value, y0.value, x1.value, y1.value);
    let user = PdfRect::new_from_values(y0.min(y1), x0.min(x1), y0.max(y1), x0.max(x1));
    page.boundaries_mut().set_crop(user)?;
    Ok((within.width, within.height))
}

/// The longest side a page is drawn at to find its ink.
const INK_SIDE: f32 = 1024.0;
/// What a channel under is ink, of 255: paper's off-white is not.
const INK_LEVEL: u8 = 240;
/// White kept around the ink found, in points: what a small drawing of
/// the page rounds away.
const INK_MARGIN: f32 = 2.0;

/// The box holding every one of `pages`' ink, in points from each page's
/// top left as it shows, a little white around it: each page drawn small
/// and its pixels not white bounded. `None` when no page has ink. Stops
/// when a newer request waits, between pages.
fn ink(
    document: &PdfDocument<'_>,
    pages: &std::collections::BTreeSet<PdfPageIndex>,
    superseded: &dyn Fn() -> bool,
) -> anyhow::Result<Option<Area>> {
    let mut union: Option<(f32, f32, f32, f32)> = None;
    for &index in pages {
        anyhow::ensure!(!superseded(), "composing superseded");
        let page = document.pages().get(index)?;
        let (width, height) = (page.width().value, page.height().value);
        let scale = (INK_SIDE / width.max(height).max(1.0)).min(1.0);
        let config = PdfRenderConfig::new()
            .set_target_width(((width * scale).ceil() as i32).max(1))
            .set_maximum_height(((height * scale).ceil() as i32).max(1))
            .set_reverse_byte_order(false)
            .render_annotations(false)
            .render_form_data(false);
        let bitmap = page.render_with_config(&config)?;
        let (columns, rows) = (
            usize::try_from(bitmap.width())?,
            usize::try_from(bitmap.height())?,
        );
        let raw = bitmap.as_raw_bytes();
        let pixels = if raw.len() == columns * rows * 4 {
            raw
        } else {
            bitmap.as_rgba_bytes()
        };
        let Some((left, top, right, bottom)) = inked(&pixels, columns, rows) else {
            continue;
        };
        let (x, y) = (width / columns as f32, height / rows as f32);
        let found = (
            left as f32 * x - INK_MARGIN,
            top as f32 * y - INK_MARGIN,
            right as f32 * x + INK_MARGIN,
            bottom as f32 * y + INK_MARGIN,
        );
        union = Some(union.map_or(found, |(l, t, r, b)| {
            (
                l.min(found.0),
                t.min(found.1),
                r.max(found.2),
                b.max(found.3),
            )
        }));
    }
    Ok(union.map(|(left, top, right, bottom)| Area {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }))
}

/// The bounds of the pixels of a drawing `columns` × `rows`, four bytes
/// each, any of whose colour channels is darker than paper: left, top,
/// right and bottom, the last two past the last such pixel.
fn inked(pixels: &[u8], columns: usize, rows: usize) -> Option<(usize, usize, usize, usize)> {
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for (row, line) in pixels.chunks_exact(columns * 4).take(rows).enumerate() {
        let mut first = None;
        let mut last = 0;
        for (column, pixel) in line.as_chunks::<4>().0.iter().enumerate() {
            if pixel[..3].iter().any(|&channel| channel < INK_LEVEL) {
                first.get_or_insert(column);
                last = column;
            }
        }
        if let Some(first) = first {
            bounds = Some(
                bounds.map_or((first, row, last + 1, row + 1), |(l, t, r, _)| {
                    (l.min(first), t, r.max(last + 1), row + 1)
                }),
            );
        }
    }
    bounds
}

/// What the document is: its pages, and unless only its first page is
/// drawn (`first_page`), its outline and its pages' labels, read through
/// `labelling`.
fn document_info(
    document: &PdfDocument<'_>,
    first_page: bool,
    labelling: Option<&dyn PdfiumLibraryBindings>,
) -> DocumentInfo {
    let pages = u32::try_from(document.pages().len()).unwrap_or_default();
    if first_page {
        return DocumentInfo {
            pages,
            outlines: Vec::new(),
            reflowable: false,
            labels: Vec::new(),
        };
    }
    DocumentInfo {
        pages,
        outlines: outlines(document),
        reflowable: false,
        labels: labelling.map_or_else(Vec::new, |bindings| labels(document, bindings)),
    }
}

/// A page's number, from its index as `PDFium` counts it (a C `int`).
fn page_number(index: PdfPageIndex) -> Option<u32> {
    u32::try_from(index).ok()?.checked_add(1)
}

/// The outline in reading order, each entry at its depth, walked as the
/// tree it is: a bookmark's `parent` from pdfium-render knows no parent of
/// its own, so depth counted through parents stopped at one.
fn outlines(document: &PdfDocument<'_>) -> Vec<Outline> {
    /// Entries kept, and nesting.
    const ENTRIES: usize = 4096;
    const DEPTH: u32 = 32;
    fn walk(first: Option<PdfBookmark<'_>>, depth: u32, out: &mut Vec<Outline>) {
        let mut next = first;
        while let Some(bookmark) = next {
            // Each turn keeps an entry, so an outline whose links go round
            // ends here too.
            if out.len() >= ENTRIES {
                return;
            }
            let page = bookmark
                .destination()
                .and_then(|d| d.page_index().ok())
                .or_else(|| {
                    bookmark
                        .action()?
                        .as_local_destination_action()?
                        .destination()
                        .ok()?
                        .page_index()
                        .ok()
                });
            out.push(Outline {
                title: bookmark.title().unwrap_or_default(),
                depth,
                page: page.and_then(page_number),
            });
            if depth + 1 < DEPTH {
                walk(bookmark.first_child(), depth + 1, out);
            }
            next = bookmark.next_sibling();
        }
    }
    let mut out = Vec::new();
    walk(document.bookmarks().root(), 0, &mut out);
    out
}

/// Each page's label, read from the document without loading its pages,
/// through `bindings` to the library that opened it.
fn labels(document: &PdfDocument<'_>, bindings: &dyn PdfiumLibraryBindings) -> Vec<String> {
    let handle = bindings.get_handle_from_document(document);
    page_labels((0..document.pages().len()).map(|index| {
        // UTF-16LE with its terminating NUL, the length asked first.
        // SAFETY: the handle is of a document open in this library, and
        // the index one of its pages; no buffer: the call only measures.
        #[allow(unsafe_code)]
        let length = unsafe { bindings.FPDF_GetPageLabel(handle, index, std::ptr::null_mut(), 0) };
        if length == 0 {
            return String::new();
        }
        let mut buffer = vec![0u8; length as usize];
        // SAFETY: as above, with a buffer of the `length` bytes the call
        // writes at most.
        #[allow(unsafe_code)]
        unsafe {
            bindings.FPDF_GetPageLabel(handle, index, buffer.as_mut_ptr().cast(), length);
        }
        let units: Vec<u16> = buffer
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|unit| *unit != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }))
}

/// `PDFium` maps crop offsets and intrinsic rotation. Normalized coordinates keep
/// text/search/link geometry independent of rendering resolution and zoom.
fn rect(page: &PdfPage<'_>, rect: PdfRect) -> anyhow::Result<Rect> {
    let config = PdfRenderConfig::new().set_target_size(10_000, 10_000);
    let (x0, y0) = page.points_to_pixels(rect.left(), rect.top(), &config)?;
    let (x1, y1) = page.points_to_pixels(rect.right(), rect.bottom(), &config)?;
    Ok(Rect {
        x: x0.min(x1) as f32 / 10_000.,
        y: y0.min(y1) as f32 / 10_000.,
        width: (x1 - x0).unsigned_abs() as f32 / 10_000.,
        height: (y1 - y0).unsigned_abs() as f32 / 10_000.,
    })
}

fn render(
    document: &PdfDocument<'_>,
    number: u32,
    side: u32,
    scratch: &Path,
) -> anyhow::Result<WorkerReply> {
    let number = number.clamp(1, u32::try_from(document.pages().len())?);
    let page = document.pages().get(PdfPageIndex::try_from(number - 1)?)?;
    let side = i32::try_from(side.clamp(64, MAX_SIDE))?;
    // BGRA, PDFium's own order and the window's: pdfium-render asks for
    // the reverse unless told.
    let config = PdfRenderConfig::new()
        .set_target_width(side)
        .set_maximum_height(side)
        .set_reverse_byte_order(false);
    let bitmap = page.render_with_config(&config)?;
    let width = u32::try_from(bitmap.width())?;
    let height = u32::try_from(bitmap.height())?;
    // Written as it was drawn, but for a bitmap of another layout.
    let raw = bitmap.as_raw_bytes();
    let four = matches!(
        bitmap.format(),
        Ok(PdfBitmapFormat::BGRA | PdfBitmapFormat::BGRx)
    );
    let (pixels, order) = if four && raw.len() as u64 == u64::from(width) * u64::from(height) * 4 {
        (raw, PixelOrder::Bgra)
    } else {
        (bitmap.as_rgba_bytes(), PixelOrder::Rgba)
    };
    std::fs::write(scratch.join(PIXELS_FILE), pixels)?;
    // Text is what can be selected on the page; the pixels are already
    // rendered. A page whose text cannot be read, or holds more than the
    // protocol carries, is shown without selectable text rather than lost.
    let chars = page
        .text()
        .ok()
        .filter(|text| text.chars().len() <= MAX_PAGE_CHARS)
        .map(|text| {
            text.chars()
                .iter()
                .filter_map(|ch| {
                    let text = ch.unicode_string()?;
                    let bounds = ch
                        .loose_bounds()
                        .or_else(|_| ch.tight_bounds())
                        .ok()
                        .and_then(|b| rect(&page, b).ok())
                        .unwrap_or_default();
                    Some(TextChar { text, bounds })
                })
                .collect()
        })
        .unwrap_or_default();
    let links = page
        .links()
        .iter()
        .take(4096)
        .filter_map(|link| {
            let destination = link
                .destination()
                .and_then(|d| d.page_index().ok())
                .and_then(page_number)
                .map(Destination::Page)
                .or_else(|| {
                    let action = link.action()?;
                    if let Some(local) = action.as_local_destination_action() {
                        return local
                            .destination()
                            .ok()?
                            .page_index()
                            .ok()
                            .and_then(page_number)
                            .map(Destination::Page);
                    }
                    action.as_uri_action()?.uri().ok().map(Destination::Uri)
                })?;
            Some(Link {
                bounds: rect(&page, link.rect().ok()?).ok()?,
                destination,
            })
        })
        .collect();
    // The page's own size: the rendered pixels scaled to its longer side
    // in points, which a rotation does not change.
    let long = page.width().value.max(page.height().value) * CSS_PER_POINT;
    let own = long / width.max(height).max(1) as f32;
    Ok(WorkerReply::Page {
        content: PageContent {
            page: number,
            width: width as f32 * own,
            height: height as f32 * own,
            chars,
            links,
        },
        width,
        height,
        order,
    })
}

/// Stops, with an error nobody reads, as soon as a newer request waits.
fn search(
    document: &PdfDocument<'_>,
    query: &str,
    superseded: &dyn Fn() -> bool,
) -> anyhow::Result<Vec<SearchHit>> {
    anyhow::ensure!(query.len() <= 4096, "search query exceeds limit");
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for (index, page) in document.pages().iter().enumerate() {
        anyhow::ensure!(!superseded(), "search superseded");
        let text = page.text()?;
        let options = PdfSearchOptions::new();
        let search = text.search(query, &options)?;
        for result in search.iter(PdfSearchDirection::SearchForward) {
            let bounds = result
                .iter()
                .map(|s| rect(&page, s.bounds()))
                .collect::<anyhow::Result<Vec<_>>>()?;
            hits.push(SearchHit {
                page: u32::try_from(index)? + 1,
                bounds,
            });
            if hits.len() >= MAX_HITS {
                return Ok(hits);
            }
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    fn layout(turn_backs: bool) -> super::Layout {
        super::Layout {
            width: 842.0,
            height: 595.0,
            fold: 10.0,
            edge: 20.0,
            trim: false,
            turn_backs,
        }
    }

    #[test]
    fn a_sheet_s_halves_keep_off_its_edges_and_its_fold() {
        let [left, right] = super::halves(&layout(false));
        assert_eq!((left.x, left.y), (20.0, 20.0));
        assert_eq!(left.width, 421.0 - 10.0 - 20.0);
        assert_eq!(right.x, 431.0);
        assert_eq!(right.x + right.width, 842.0 - 20.0);
        assert_eq!(left.height, 595.0 - 40.0);
    }

    #[test]
    fn a_page_is_fitted_centred_in_its_half_and_turned_with_the_back() {
        let [left, _] = super::halves(&layout(false));
        // A page half as wide as tall, in a half 391 × 555.
        let matrix = super::placed(200.0, 400.0, left, false, (842.0, 595.0));
        let point = |m: &pdfium_render::prelude::PdfMatrix, x: f32, y: f32| {
            let (a, b, c, d, e, f) = (m.a(), m.b(), m.c(), m.d(), m.e(), m.f());
            (a * x + c * y + e, b * x + d * y + f)
        };
        let scale = 555.0 / 400.0;
        let (x0, y0) = point(&matrix, 0.0, 0.0);
        let (x1, y1) = point(&matrix, 200.0, 400.0);
        assert!((y0 - 20.0).abs() < 0.01 && (y1 - 575.0).abs() < 0.01);
        assert!(((x1 - x0) - 200.0 * scale).abs() < 0.01);
        assert!(
            (f32::midpoint(x0, x1) - (20.0 + 391.0 / 2.0)).abs() < 0.01,
            "centred"
        );
        // Turned, its corners land where the sheet turned half a turn
        // puts them: the left half's page in the right half, upside down.
        let turned = super::placed(200.0, 400.0, left, true, (842.0, 595.0));
        let (tx0, ty0) = point(&turned, 0.0, 0.0);
        assert!((tx0 - (842.0 - x0)).abs() < 0.01 && (ty0 - (595.0 - y0)).abs() < 0.01);
        let (tx1, _) = point(&turned, 200.0, 400.0);
        assert!(tx1 < tx0, "upside down");
    }

    /// A page drawn at half its size keeps its middle where it is: its
    /// corners come halfway in.
    #[test]
    fn a_page_scaled_keeps_its_middle() {
        let m = super::scale_about(0.5, (300.0, 400.0));
        let point = |x: f32, y: f32| (m.a() * x + m.c() * y + m.e(), m.b() * x + m.d() * y + m.f());
        assert_eq!(point(300.0, 400.0), (300.0, 400.0));
        assert_eq!(point(0.0, 0.0), (150.0, 200.0));
        assert_eq!(point(600.0, 800.0), (450.0, 600.0));
    }

    #[test]
    fn ink_is_bounded_by_what_is_darker_than_paper() {
        // A 4 × 3 drawing, white but for two pixels.
        let mut pixels = vec![255u8; 4 * 3 * 4];
        let mut dark = |column: usize, row: usize| {
            pixels[(row * 4 + column) * 4..][..3].copy_from_slice(&[10, 10, 10]);
        };
        dark(1, 0);
        dark(2, 2);
        assert_eq!(super::inked(&pixels, 4, 3), Some((1, 0, 3, 3)));
        assert_eq!(super::inked(&[255; 48], 4, 3), None);
        // Paper's off-white is no ink.
        assert_eq!(super::inked(&[245; 48], 4, 3), None);
    }

    #[test]
    fn a_packages_pdfium_is_beside_the_helper_or_in_a_macs_frameworks() {
        let at = super::carried(Path::new("/usr/lib/skwir"), OsStr::new("libpdfium.so"));
        assert_eq!(at[0], Path::new("/usr/lib/skwir/libpdfium.so"));
        let at = super::carried(
            Path::new("/Applications/skwir.app/Contents/MacOS"),
            OsStr::new("libpdfium.dylib"),
        );
        assert_eq!(
            at[1],
            Path::new("/Applications/skwir.app/Contents/Frameworks/libpdfium.dylib")
        );
    }
}
