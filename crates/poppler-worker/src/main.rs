//! Persistent Poppler reader. Native handles never leave this process.
use std::io::{BufRead, BufWriter, Read, Write};
use std::path::Path;

use anyhow::Context;
use bibli_core::document::{
    DocumentInfo, MAX_HITS, MAX_REPLY, MAX_SIDE, PIXELS_FILE, PageContent, Rect, SearchHit,
    WorkerReply, WorkerRequest,
};
use cairo::{Context as CairoContext, Format, ImageSurface};
use poppler::{Document, Page};

mod navigation;

fn main() {
    if let Err(error) = run() {
        let _ = reply(&WorkerReply::Error(format!("{error:#}")));
    }
}

fn reply(value: &WorkerReply) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    anyhow::ensure!(bytes.len() <= MAX_REPLY, "document metadata exceeds limit");
    let mut out = std::io::stdout().lock();
    out.write_all(&bytes)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn run() -> anyhow::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    anyhow::ensure!(args.len() == 2, "expected PDF path and scratch directory");
    let uri = glib::filename_to_uri(std::fs::canonicalize(&args[0])?, None)?;
    let document = Document::from_file(&uri, None)
        .context("open PDF (encrypted files require an external reader)")?;
    let pages = u32::try_from(document.n_pages())?;
    anyhow::ensure!(pages > 0, "document has no pages");
    reply(&WorkerReply::Ready(DocumentInfo {
        pages,
        outlines: navigation::outlines(&document),
    }))?;
    let mut input = std::io::stdin().lock();
    loop {
        let mut line = String::new();
        let n = Read::by_ref(&mut input)
            .take(16 * 1024)
            .read_line(&mut line)?;
        if n == 0 {
            break;
        }
        anyhow::ensure!(line.ends_with('\n'), "request exceeds limit");
        let request: WorkerRequest = serde_json::from_str(&line)?;
        let result = match request {
            WorkerRequest::Render { page, side } => {
                render(&document, page, side, Path::new(&args[1]))
            }
            WorkerRequest::Search { query } => search(&document, &query).map(WorkerReply::Matches),
        };
        reply(&result.unwrap_or_else(|e| WorkerReply::Error(format!("{e:#}"))))?;
    }
    Ok(())
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
    scratch: &Path,
) -> anyhow::Result<WorkerReply> {
    let number = number.clamp(1, u32::try_from(document.n_pages())?);
    let page = document
        .page(i32::try_from(number - 1)?)
        .context("missing page")?;
    let size = page_size(&page)?;
    let limit = f64::from(max_side.clamp(64, MAX_SIDE));
    let scale = limit / size.0.max(size.1);
    // Rounding must not push a dimension above the protocol pixel limit.
    let width = (size.0 * scale).ceil().clamp(1., limit) as i32;
    let height = (size.1 * scale).ceil().clamp(1., limit) as i32;
    let mut surface = ImageSurface::create(Format::ARgb32, width, height)?;
    {
        let context = CairoContext::new(&surface)?;
        context.set_source_rgb(1., 1., 1.);
        context.paint()?;
        context.scale(scale, scale);
        page.render(&context);
        context.status()?;
    }
    // Cairo ARGB32 is native-endian and premultiplied. Painting opaque white
    // first makes alpha 255, so this becomes straight RGBA without unpremultiply.
    surface.flush();
    let stride = usize::try_from(surface.stride())?;
    let mut output = BufWriter::new(std::fs::File::create(scratch.join(PIXELS_FILE))?);
    for row in surface.data()?.chunks_exact_mut(stride) {
        let row = &mut row[..usize::try_from(width)? * 4];
        for pixel in row.as_chunks_mut::<4>().0 {
            let argb = u32::from_ne_bytes(*pixel);
            *pixel = [
                (argb >> 16) as u8,
                (argb >> 8) as u8,
                argb as u8,
                (argb >> 24) as u8,
            ];
        }
        output.write_all(row)?;
    }
    output.flush()?;
    let chars = navigation::chars(&page, size)?;
    let links = navigation::links(document, &page, size);
    let width = u32::try_from(width)?;
    let height = u32::try_from(height)?;
    Ok(WorkerReply::Page {
        content: PageContent {
            page: number,
            width: width as f32,
            height: height as f32,
            chars,
            links,
        },
        width,
        height,
    })
}

fn search(document: &Document, query: &str) -> anyhow::Result<Vec<SearchHit>> {
    anyhow::ensure!(
        query.len() <= 4096 && !query.contains('\0'),
        "invalid search query"
    );
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for index in 0..document.n_pages() {
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
