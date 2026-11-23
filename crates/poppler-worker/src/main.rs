//! Persistent Poppler reader. Native handles never leave this process.
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Context;
use cairo::{Context as CairoContext, Format, ImageSurface};
use poppler::{Document, Page};
use skwir_core::document::{
    CSS_PER_POINT, DocumentInfo, FIRST_PAGE, MAX_HITS, MAX_PART_SIDE, PIXELS_FILE, PageContent,
    Part, PixelOrder, Rect, SearchHit, WorkerReply, WorkerRequest, held_side, page_labels, worker,
};

mod navigation;

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
    // (`platform::process::probe`): being started at all says its
    // libraries are there.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--probe")
    {
        return;
    }
    if let Err(error) = run() {
        worker::fail(&error);
    }
}

fn run() -> anyhow::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let first_page = args.get(2).is_some_and(|arg| arg == FIRST_PAGE);
    anyhow::ensure!(
        args.len() == 2 || (args.len() == 3 && first_page),
        "expected PDF path, scratch directory and maybe {FIRST_PAGE}"
    );
    let uri = glib::filename_to_uri(skwir_core::os_path::canonical(&args[0])?, None)?;
    let document = Document::from_file(&uri, None)
        .context("open PDF (encrypted files require an external reader)")?;
    let pages = u32::try_from(document.n_pages())?;
    anyhow::ensure!(pages > 0, "document has no pages");
    // Its first page alone (a thumbnail): no outline, and no labels, which
    // Poppler reads by loading every page.
    worker::greet(&WorkerReply::Ready(DocumentInfo {
        pages,
        outlines: if first_page {
            Vec::new()
        } else {
            navigation::outlines(&document)
        },
        reflowable: false,
        labels: if first_page {
            Vec::new()
        } else {
            page_labels((0..document.n_pages()).map(|index| {
                document
                    .page(index)
                    .and_then(|page| page.label())
                    .map(|label| label.to_string())
                    .unwrap_or_default()
            }))
        },
        // Never the way it says it reads (`/ViewerPreferences
        // /Direction`): poppler-glib has a flag for it (`DIRECTION_RTL`)
        // that it never sets, a catalog's `/R2L` read as no preference at
        // all (poppler-glib 26.10), and no other way to the catalog.
        // PDFium and MuPDF read it.
        direction: None,
    }))?;
    let scratch = Path::new(&args[1]);
    worker::serve(|request, superseded| match request {
        WorkerRequest::Render { page, side, part } => render(&document, page, side, part, scratch),
        WorkerRequest::Search { query } => {
            search(&document, &query, superseded).map(WorkerReply::Matches)
        }
        // A PDF's pages never move.
        WorkerRequest::Locate { .. } => Ok(WorkerReply::Located(None)),
        // Printing composes with PDFium (ADR-111).
        WorkerRequest::Compose(_) | WorkerRequest::Print { .. } => {
            anyhow::bail!("Poppler writes no PDF here")
        }
        // `serve` answers it: nothing runs by the time it is read.
        WorkerRequest::Cancel => anyhow::bail!("nothing to cancel"),
    })
}

/// Poppler text layout uses displayed, top-left coordinates; links and search
/// use displayed, bottom-left coordinates. Both already include crop/rotation.
fn rect(size: (f64, f64), x1: f64, y1: f64, x2: f64, y2: f64, bottom: bool) -> Rect {
    Rect {
        x: (x1.min(x2) / size.0) as f32,
        y: ((if bottom {
            size.1 - y1.max(y2)
        } else {
            y1.min(y2)
        }) / size.1) as f32,
        width: ((x2 - x1).abs() / size.0) as f32,
        height: ((y2 - y1).abs() / size.1) as f32,
    }
}

fn page_size(page: &Page) -> anyhow::Result<(f64, f64)> {
    let (width, height) = page.size();
    anyhow::ensure!(
        width.is_finite() && height.is_finite() && width > 0. && height > 0.,
        "invalid page dimensions"
    );
    Ok((width, height))
}

fn render(
    document: &Document,
    number: u32,
    max_side: u32,
    part: Option<Part>,
    scratch: &Path,
) -> anyhow::Result<WorkerReply> {
    let number = number.clamp(1, u32::try_from(document.n_pages())?);
    let page = document
        .page(i32::try_from(number - 1)?)
        .context("missing page")?;
    let size = page_size(&page)?;
    // Whole, within what a page may hold; a part, the page at the side
    // asked however large (ADR-147).
    let longest = match part {
        None => held_side(size.0 as f32, size.1 as f32, max_side),
        Some(_) => max_side.clamp(64, MAX_PART_SIDE),
    };
    let limit = f64::from(longest);
    let scale = limit / size.0.max(size.1);
    // Rounding must not push a dimension above the protocol pixel limit.
    let whole_width = (size.0 * scale).ceil().clamp(1., limit) as u32;
    let whole_height = (size.1 * scale).ceil().clamp(1., limit) as u32;
    // The whole, or the part alone: the page laid at that scale with its
    // corner off the surface's.
    let [left, top, w, h] = match part {
        None => [0, 0, whole_width, whole_height],
        Some(part) => part.drawn(whole_width, whole_height)?,
    };
    let (width, height) = (i32::try_from(w)?, i32::try_from(h)?);
    let mut surface = ImageSurface::create(Format::ARgb32, width, height)?;
    {
        let context = CairoContext::new(&surface)?;
        context.set_source_rgb(1., 1., 1.);
        context.paint()?;
        context.translate(-f64::from(left), -f64::from(top));
        context.scale(scale, scale);
        page.render(&context);
        context.status()?;
    }
    // Cairo ARGB32 is native-endian and premultiplied. Painting opaque white
    // first makes alpha 255, so it is straight BGRA on a little-endian
    // machine, the window's order, written as it is.
    surface.flush();
    let stride = usize::try_from(surface.stride())?;
    let mut output = BufWriter::new(std::fs::File::create(scratch.join(PIXELS_FILE))?);
    for row in surface.data()?.chunks_exact_mut(stride) {
        let row = &mut row[..usize::try_from(width)? * 4];
        if cfg!(target_endian = "big") {
            for pixel in row.as_chunks_mut::<4>().0 {
                *pixel = u32::from_ne_bytes(*pixel).to_le_bytes();
            }
        }
        output.write_all(row)?;
    }
    output.flush()?;
    // A part's page carried its text whole: none again.
    let (chars, links) = if part.is_some() {
        (Vec::new(), Vec::new())
    } else {
        (
            navigation::chars(&page, size),
            navigation::links(document, &page, size),
        )
    };
    Ok(WorkerReply::Page {
        content: PageContent {
            page: number,
            width: size.0 as f32 * CSS_PER_POINT,
            height: size.1 as f32 * CSS_PER_POINT,
            chars,
            links,
        },
        width: w,
        height: h,
        order: PixelOrder::Bgra,
        part: part.map(|_| Part::of_pixels([left, top, w, h], whole_width, whole_height)),
    })
}

/// Stops, with an error nobody reads, as soon as a newer request waits.
fn search(
    document: &Document,
    query: &str,
    superseded: &dyn Fn() -> bool,
) -> anyhow::Result<Vec<SearchHit>> {
    anyhow::ensure!(
        query.len() <= 4096 && !query.contains('\0'),
        "invalid search query"
    );
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for index in 0..document.n_pages() {
        anyhow::ensure!(!superseded(), "search superseded");
        let page = document.page(index).context("missing page")?;
        let size = page_size(&page)?;
        for bounds in page.find_text(query) {
            hits.push(SearchHit {
                page: u32::try_from(index)? + 1,
                bounds: vec![rect(
                    size,
                    bounds.x1(),
                    bounds.y1(),
                    bounds.x2(),
                    bounds.y2(),
                    true,
                )],
            });
            if hits.len() == MAX_HITS {
                return Ok(hits);
            }
        }
    }
    Ok(hits)
}
