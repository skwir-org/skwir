//! `PDFium` runs in this optional child process, never in the GPU/UI process.
//! A document stays open for the worker's lifetime; each request releases pages
//! and bitmaps. The parent enforces cancellation, time and output limits.

use std::io::{BufRead, Read, Write};
use std::path::Path;

use anyhow::Context;
use bibli_core::document::{
    Destination, DocumentInfo, Link, MAX_HITS, MAX_REPLY, MAX_SIDE, Outline, PIXELS_FILE,
    PageContent, Rect, SearchHit, TextChar, WorkerReply, WorkerRequest,
};
use pdfium_render::prelude::*;

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
    let bindings = match std::env::var_os("BIBLI_PDFIUM_LIBRARY") {
        Some(path) => Pdfium::bind_to_library(path),
        None => Pdfium::bind_to_system_library(),
    };
    let bindings = match bindings {
        Ok(bindings) => bindings,
        Err(error) => {
            reply(&WorkerReply::Unavailable(format!(
                "PDFium library unavailable: {error}"
            )))?;
            return Ok(());
        }
    };
    let pdfium = Pdfium::new(bindings);
    let document = pdfium
        .load_pdf_from_file(Path::new(&args[0]), None)
        .context("open PDF (encrypted files require an external reader)")?;
    let info = document_info(&document);
    anyhow::ensure!(info.pages > 0, "document has no pages");
    reply(&WorkerReply::Ready(info))?;
    let mut input = std::io::stdin().lock();
    loop {
        let mut line = String::new();
        let n = std::io::Read::by_ref(&mut input)
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

fn document_info(document: &PdfDocument<'_>) -> DocumentInfo {
    let outlines = document
        .bookmarks()
        .iter()
        .take(4096)
        .map(|bookmark| {
            let mut depth = 0;
            let mut parent = bookmark.parent();
            while let Some(p) = parent {
                depth += 1;
                if depth >= 32 {
                    break;
                }
                parent = p.parent();
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
            Outline {
                title: bookmark.title().unwrap_or_default(),
                depth,
                page: page.map(|p| u32::from(p) + 1),
            }
        })
        .collect();
    DocumentInfo {
        pages: u32::from(document.pages().len()),
        outlines,
    }
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
    let number = number.clamp(1, u32::from(document.pages().len()));
    let page = document.pages().get(u16::try_from(number - 1)?)?;
    let side = i32::try_from(side.clamp(64, MAX_SIDE))?;
    let config = PdfRenderConfig::new()
        .set_target_width(side)
        .set_maximum_height(side);
    let bitmap = page.render_with_config(&config)?;
    let width = u32::try_from(bitmap.width())?;
    let height = u32::try_from(bitmap.height())?;
    std::fs::write(scratch.join(PIXELS_FILE), bitmap.as_rgba_bytes())?;
    let text = page.text()?;
    anyhow::ensure!(text.chars().len() <= 200_000, "page text exceeds limit");
    let chars = text
        .chars()
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
        .collect();
    let links = page
        .links()
        .iter()
        .take(4096)
        .filter_map(|link| {
            let destination = link
                .destination()
                .and_then(|d| d.page_index().ok())
                .map(|p| Destination::Page(u32::from(p) + 1))
                .or_else(|| {
                    let action = link.action()?;
                    if let Some(local) = action.as_local_destination_action() {
                        return local
                            .destination()
                            .ok()?
                            .page_index()
                            .ok()
                            .map(|p| Destination::Page(u32::from(p) + 1));
                    }
                    action.as_uri_action()?.uri().ok().map(Destination::Uri)
                })?;
            Some(Link {
                bounds: rect(&page, link.rect().ok()?).ok()?,
                destination,
            })
        })
        .collect();
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

fn search(document: &PdfDocument<'_>, query: &str) -> anyhow::Result<Vec<SearchHit>> {
    anyhow::ensure!(query.len() <= 4096, "search query exceeds limit");
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for (index, page) in document.pages().iter().enumerate() {
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
