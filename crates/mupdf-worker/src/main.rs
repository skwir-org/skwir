//! `MuPDF` runs in this optional child process, never in the GPU/UI process.
//! A document stays open for the worker's lifetime; each request loads its
//! page and drops it. The parent enforces cancellation, time and output limits.
//!
//! `MuPDF` gives page geometry in points with the origin at the top left of
//! the page's bounds, crop and rotation already applied: pixels, characters,
//! links and search hits all share that space, and are normalized against the
//! page's bounds here, as the protocol wants.

use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Context;
use bibli_core::document::{
    Destination, DocumentInfo, Link, MAX_HITS, MAX_PAGE_CHARS, MAX_SIDE, Outline, PIXELS_FILE,
    PageContent, Rect, SearchHit, TextChar, WorkerReply, WorkerRequest, worker,
};
use mupdf::{Colorspace, Device, Document, Matrix, Page, Pixmap, Quad, TextPageFlags};

/// Entries of a document's outline, and links on one page.
const ENTRIES: usize = 4096;
/// Outline nesting kept.
const DEPTH: u32 = 32;

fn main() {
    if let Err(error) = run() {
        worker::fail(&error);
    }
}

fn run() -> anyhow::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    anyhow::ensure!(args.len() == 2, "expected PDF path and scratch directory");
    let path = args[0]
        .to_str()
        .context("document path is not valid UTF-8")?;
    let document =
        Document::open(path).context("open PDF (encrypted files require an external reader)")?;
    anyhow::ensure!(
        !document.needs_password()?,
        "PDF is encrypted; open it in an external reader"
    );
    let pages = u32::try_from(document.page_count()?)?;
    anyhow::ensure!(pages > 0, "document has no pages");
    worker::greet(&WorkerReply::Ready(DocumentInfo {
        pages,
        outlines: outlines(&document),
    }))?;
    let scratch = Path::new(&args[1]);
    worker::serve(|request, superseded| match request {
        WorkerRequest::Render { page, side } => render(&document, pages, page, side, scratch),
        WorkerRequest::Search { query } => {
            search(&document, pages, &query, superseded).map(WorkerReply::Matches)
        }
        // `serve` answers it: nothing runs by the time it is read.
        WorkerRequest::Cancel => anyhow::bail!("nothing to cancel"),
    })
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
        for rgb in samples[..columns].as_chunks::<3>().0 {
            row.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        output.write_all(&row)?;
    }
    output.flush()?;
    Ok(WorkerReply::Page {
        content: PageContent {
            page: number,
            width: width as f32,
            height: height as f32,
            chars: chars(&page, space),
            links: links(&page, space),
        },
        width,
        height,
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
