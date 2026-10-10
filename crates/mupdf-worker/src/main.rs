//! `MuPDF` runs in this optional child process, never in the GPU/UI process.
//! A document stays open for the worker's lifetime; each request loads its
//! page and drops it. The parent enforces cancellation, time and output limits.
//!
//! A reflowable document (a book) is laid out into pages of the size and
//! text size the parent gives (`--layout=width,height,em`, in points), and
//! says so (`DocumentInfo::reflowable`): the parent opens it again at
//! another text size. A PDF's pages are fixed and ignore it.
//!
//! `MuPDF` gives page geometry in points with the origin at the top left of
//! the page's bounds, crop and rotation already applied: pixels, characters,
//! links and search hits all share that space, and are normalized against the
//! page's bounds here, as the protocol wants.

use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Context;
use bibli_core::document::{
    CSS_PER_POINT, Destination, DocumentInfo, FIRST_PAGE, Link, MAX_HITS, MAX_PAGE_CHARS, MAX_SIDE,
    Outline, PIXELS_FILE, PageContent, PixelOrder, Rect, SearchHit, TextChar, WorkerReply,
    WorkerRequest, page_labels, worker,
};
use mupdf::{Colorspace, Device, Document, Matrix, Page, Pixmap, Quad, TextPageFlags};

mod fonts;

/// Entries of a document's outline, and links on one page.
const ENTRIES: usize = 4096;
/// Outline nesting kept.
const DEPTH: u32 = 32;

fn main() {
    // `--build`, bibli asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--build")
    {
        println!("{}", env!("BIBLI_BUILD"));
        return;
    }
    // `--probe`, bibli's settings asking whether this helper works here
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
    let first_page = args.iter().skip(2).any(|arg| arg == FIRST_PAGE);
    let options: Vec<_> = args
        .iter()
        .skip(2)
        .filter(|arg| *arg != FIRST_PAGE)
        .collect();
    anyhow::ensure!(
        args.len() >= 2 && options.len() <= 1,
        "expected document path, scratch directory, an optional layout and maybe {FIRST_PAGE}"
    );
    let path = args[0]
        .to_str()
        .context("document path is not valid UTF-8")?;
    let layout = match options.first() {
        Some(arg) => Some(layout(arg.to_str().context("invalid layout")?)?),
        None => None,
    };
    // Text in a script no font of the document covers asks the desktop.
    mupdf::set_font_loader(fonts::Fallback::default());
    let mut document = Document::open(path)
        .context("open document (encrypted files require an external reader)")?;
    anyhow::ensure!(
        !document.needs_password()?,
        "document is encrypted; open it in an external reader"
    );
    let reflowable = document.is_reflowable()?;
    if reflowable {
        let [width, height, em] = layout.unwrap_or([420., 595., 12.]);
        document.layout(width, height, em)?;
    }
    let pages = u32::try_from(document.page_count()?)?;
    anyhow::ensure!(pages > 0, "document has no pages");
    // A PDF names its pages (`/PageLabels`); only MuPDF's PDF document
    // reads them, and it is still the document every request reads.
    let opened = if document.is_pdf() {
        Opened::Pdf(mupdf::pdf::PdfDocument::try_from(document)?)
    } else {
        Opened::Other(document)
    };
    let (document, labels): (&Document, _) = match &opened {
        Opened::Pdf(pdf) if !first_page => (
            pdf,
            page_labels((0..pages as usize).map(|index| pdf.page_label(index).unwrap_or_default())),
        ),
        Opened::Pdf(pdf) => (pdf, Vec::new()),
        Opened::Other(document) => (document, Vec::new()),
    };
    worker::greet(&WorkerReply::Ready(DocumentInfo {
        pages,
        // Its first page alone (a thumbnail): no outline.
        outlines: if first_page {
            Vec::new()
        } else {
            outlines(document)
        },
        reflowable,
        labels,
    }))?;
    let scratch = Path::new(&args[1]);
    worker::serve(|request, superseded| match request {
        WorkerRequest::Render { page, side } => render(document, pages, page, side, scratch),
        WorkerRequest::Search { query } => {
            search(document, pages, &query, superseded).map(WorkerReply::Matches)
        }
        // A PDF's pages never move.
        WorkerRequest::Locate { .. } if !reflowable => Ok(WorkerReply::Located(None)),
        WorkerRequest::Locate { text, near } => {
            locate(document, pages, &text, near, superseded).map(WorkerReply::Located)
        }
        // `serve` answers it: nothing runs by the time it is read.
        WorkerRequest::Cancel => anyhow::bail!("nothing to cancel"),
    })
}

/// The document opened, as a PDF when it is one.
enum Opened {
    Pdf(mupdf::pdf::PdfDocument),
    Other(Document),
}

/// `--layout=width,height,em`: page size and body text size, in points,
/// each bounded.
fn layout(arg: &str) -> anyhow::Result<[f32; 3]> {
    let values: Vec<f32> = arg
        .strip_prefix("--layout=")
        .context("unknown argument")?
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    let [width, height, em] = values[..] else {
        anyhow::bail!("layout needs a width, a height and a text size");
    };
    anyhow::ensure!(
        [width, height]
            .iter()
            .all(|side| (72. ..=2000.).contains(side))
            && (4. ..=72.).contains(&em),
        "layout out of bounds"
    );
    Ok([width, height, em])
}

/// The outline flattened in reading order, with its depth. An outline that
/// cannot be read is none: the document still opens.
fn outlines(document: &Document) -> Vec<Outline> {
    fn walk(entries: &[mupdf::Outline], depth: u32, out: &mut Vec<Outline>) {
        for entry in entries {
            if out.len() >= ENTRIES {
                return;
            }
            out.push(Outline {
                title: entry.title.clone(),
                depth,
                page: entry.dest.map(|dest| dest.loc.page_number + 1),
            });
            if depth + 1 < DEPTH {
                walk(&entry.down, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&document.outlines().unwrap_or_default(), 0, &mut out);
    out
}

/// A page's bounds, `x0 y0 width height` in points.
fn bounds(page: &Page) -> anyhow::Result<[f32; 4]> {
    let bounds = page.bounds()?;
    let (width, height) = (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
    anyhow::ensure!(
        width.is_finite() && height.is_finite() && width > 0. && height > 0.,
        "invalid page dimensions"
    );
    Ok([bounds.x0, bounds.y0, width, height])
}

/// A rectangle in page space as fractions of the page.
fn normalized(page: [f32; 4], x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    let [px, py, width, height] = page;
    Rect {
        x: (x0.min(x1) - px) / width,
        y: (y0.min(y1) - py) / height,
        width: (x1 - x0).abs() / width,
        height: (y1 - y0).abs() / height,
    }
}
fn quad(page: [f32; 4], quad: &Quad) -> Rect {
    let xs = [quad.ul.x, quad.ur.x, quad.ll.x, quad.lr.x];
    let ys = [quad.ul.y, quad.ur.y, quad.ll.y, quad.lr.y];
    let min = |values: [f32; 4]| values.into_iter().fold(f32::INFINITY, f32::min);
    let max = |values: [f32; 4]| values.into_iter().fold(f32::NEG_INFINITY, f32::max);
    normalized(page, min(xs), min(ys), max(xs), max(ys))
}

fn render(
    document: &Document,
    pages: u32,
    number: u32,
    max_side: u32,
    scratch: &Path,
) -> anyhow::Result<WorkerReply> {
    let number = number.clamp(1, pages);
    let page = document.load_page(i32::try_from(number - 1)?)?;
    let space = bounds(&page)?;
    let [x0, y0, page_width, page_height] = space;
    let limit = max_side.clamp(64, MAX_SIDE) as f32;
    let scale = limit / page_width.max(page_height);
    // The pixmap is exactly the size asked for: MuPDF would round the page's
    // bounds itself, and a cropped page's fractional origin can add a pixel.
    let width = (page_width * scale).ceil().clamp(1., limit) as u32;
    let height = (page_height * scale).ceil().clamp(1., limit) as u32;
    let mut pixmap = Pixmap::new_with_w_h(
        &Colorspace::device_rgb(),
        i32::try_from(width)?,
        i32::try_from(height)?,
        false,
    )?;
    pixmap.clear_with(255)?;
    {
        let device = Device::from_pixmap(&pixmap)?;
        let mut ctm = Matrix::new_translate(-x0, -y0);
        ctm.concat(Matrix::new_scale(scale, scale));
        page.run(&device, &ctm)?;
    }
    let (columns, stride) = (width as usize * 3, usize::try_from(pixmap.stride())?);
    let mut output = BufWriter::new(std::fs::File::create(scratch.join(PIXELS_FILE))?);
    let mut row = Vec::with_capacity(width as usize * 4);
    for samples in pixmap.samples().chunks(stride).take(height as usize) {
        row.clear();
        // In the window's order.
        for rgb in samples[..columns].as_chunks::<3>().0 {
            row.extend_from_slice(&[rgb[2], rgb[1], rgb[0], 255]);
        }
        output.write_all(&row)?;
    }
    output.flush()?;
    Ok(WorkerReply::Page {
        content: PageContent {
            page: number,
            width: page_width * CSS_PER_POINT,
            height: page_height * CSS_PER_POINT,
            chars: chars(&page, space),
            links: links(&page, space),
        },
        width,
        height,
        order: PixelOrder::Bgra,
    })
}

/// The page's text in reading order, a character each with its bounds, a
/// line break after each line so a selection copies as it reads. A page
/// whose text cannot be read, or holds more than the protocol carries, is
/// shown without selectable text rather than lost.
fn chars(page: &Page, space: [f32; 4]) -> Vec<TextChar> {
    let Ok(text) = page.to_text_page(TextPageFlags::empty()) else {
        return Vec::new();
    };
    let mut chars = Vec::new();
    for block in text.blocks() {
        for line in block.lines() {
            let mut last = None;
            for ch in line.chars() {
                let Some(c) = ch.char() else {
                    continue;
                };
                let bounds = quad(space, &ch.quad());
                last = Some(bounds);
                chars.push(TextChar {
                    text: c.to_string(),
                    bounds,
                });
            }
            if let Some(bounds) = last {
                chars.push(TextChar {
                    text: "\n".into(),
                    bounds: Rect {
                        x: bounds.x + bounds.width,
                        width: 0.,
                        ..bounds
                    },
                });
            }
            if chars.len() > MAX_PAGE_CHARS {
                return Vec::new();
            }
        }
    }
    chars
}

/// Page links and web links. The binding resolves each link's destination
/// as it lists them and gives up on the page at the first it cannot: a page
/// with such a link is shown without links rather than failing.
fn links(page: &Page, space: [f32; 4]) -> Vec<Link> {
    let listed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        page.links()
            .map(|links| links.take(ENTRIES).collect::<Vec<_>>())
    }));
    let Ok(Ok(listed)) = listed else {
        return Vec::new();
    };
    listed
        .into_iter()
        .filter_map(|link| {
            let destination = match link.dest {
                Some(dest) => Destination::Page(dest.loc.page_number + 1),
                None if !link.uri.is_empty() && !link.uri.starts_with('#') => {
                    Destination::Uri(link.uri)
                }
                None => return None,
            };
            let b = link.bounds;
            Some(Link {
                bounds: normalized(space, b.x0, b.y0, b.x1, b.y1),
                destination,
            })
        })
        .collect()
}

/// The page nearest `near` holding `text`, looking at most `LOCATE` pages
/// either side, nearest first: after a book is laid out again, what was read
/// is a page or two from where its share of the book says. A text the book
/// repeats (a refrain, a running phrase) is found where it is nearest.
fn locate(
    document: &Document,
    pages: u32,
    text: &str,
    near: u32,
    superseded: &dyn Fn() -> bool,
) -> anyhow::Result<Option<u32>> {
    const LOCATE: u32 = 24;
    anyhow::ensure!(
        text.len() <= 4096 && !text.contains('\0'),
        "invalid text to locate"
    );
    if text.trim().is_empty() {
        return Ok(None);
    }
    let near = near.clamp(1, pages);
    for distance in 0..=LOCATE {
        for page in [near.checked_sub(distance), near.checked_add(distance)] {
            let Some(page) = page.filter(|page| (1..=pages).contains(page)) else {
                continue;
            };
            if distance == 0 && page != near {
                continue;
            }
            anyhow::ensure!(!superseded(), "locate superseded");
            let loaded = document.load_page(i32::try_from(page - 1)?)?;
            if !loaded.search(text, 1)?.is_empty() {
                return Ok(Some(page));
            }
        }
    }
    Ok(None)
}

/// Stops, with an error nobody reads, as soon as a newer request waits.
fn search(
    document: &Document,
    pages: u32,
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
    for index in 0..pages {
        anyhow::ensure!(!superseded(), "search superseded");
        let page = document.load_page(i32::try_from(index)?)?;
        let space = bounds(&page)?;
        let left = u32::try_from(MAX_HITS - hits.len())?;
        for found in page.search(query, left)?.iter() {
            hits.push(SearchHit {
                page: index + 1,
                bounds: vec![quad(space, found)],
            });
            if hits.len() >= MAX_HITS {
                return Ok(hits);
            }
        }
    }
    Ok(hits)
}
