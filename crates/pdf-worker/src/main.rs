//! `PDFium` runs in this optional child process, never in the GPU/UI process.
//! A document stays open for the worker's lifetime; each request releases pages
//! and bitmaps. The parent enforces cancellation, time and output limits.

use std::path::Path;

use anyhow::Context;
use pdfium_render::prelude::*;
use skwir_core::document::{
    CSS_PER_POINT, Destination, DocumentInfo, FIRST_PAGE, Link, MAX_HITS, MAX_PAGE_CHARS, MAX_SIDE,
    Outline, PIXELS_FILE, PageContent, PixelOrder, Rect, SearchHit, TextChar, WorkerReply,
    WorkerRequest, page_labels, worker,
};

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
    let pdfium = Pdfium::new(bindings);
    let document = pdfium
        .load_pdf_from_file(Path::new(&args[0]), None)
        .context("open PDF (encrypted files require an external reader)")?;
    let info = document_info(&document, first_page);
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
        // `serve` answers it: nothing runs by the time it is read.
        WorkerRequest::Cancel => anyhow::bail!("nothing to cancel"),
    })
}

/// What the document is: its pages, and unless only its first page is
/// drawn (`first_page`), its outline and its pages' labels.
fn document_info(document: &PdfDocument<'_>, first_page: bool) -> DocumentInfo {
    if first_page {
        return DocumentInfo {
            pages: u32::from(document.pages().len()),
            outlines: Vec::new(),
            reflowable: false,
            labels: Vec::new(),
        };
    }
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
        reflowable: false,
        labels: labels(document),
    }
}

/// Each page's label, read from the document without loading its pages.
fn labels(document: &PdfDocument<'_>) -> Vec<String> {
    let bindings = document.bindings();
    let handle = bindings.get_handle_from_document(document);
    let count = i32::from(document.pages().len());
    page_labels((0..count).map(|index| {
        // UTF-16LE with its terminating NUL, the length asked first.
        let length = bindings.FPDF_GetPageLabel(handle, index, std::ptr::null_mut(), 0);
        if length == 0 {
            return String::new();
        }
        let mut buffer = vec![0u8; length as usize];
        bindings.FPDF_GetPageLabel(handle, index, buffer.as_mut_ptr().cast(), length);
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
    let number = number.clamp(1, u32::from(document.pages().len()));
    let page = document.pages().get(u16::try_from(number - 1)?)?;
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
