//! `DjVu` read by djvu-rs, in Rust, behind the protocol every document worker
//! speaks (`skwir_core::document::worker`): a page drawn at the side asked,
//! its words, its links and the document's outline. The document is
//! untrusted: the worker runs isolated where the system allows (ADR-041,
//! ADR-061).
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Context;
use djvu_rs::annotation::{MapArea, Shape};
use djvu_rs::djvu_document::{DjVuBookmark, DjVuDocument, DjVuPage};
use djvu_rs::djvu_render::{RenderOptions, render_pixmap};
use djvu_rs::resource_limits::{ParseOptions, ResourceLimits};
use djvu_rs::text::{TextZone, TextZoneKind};
use skwir_core::document::{
    Destination, DocumentInfo, FIRST_PAGE, Link, MAX_HITS, MAX_PAGE_SIZE, MAX_SIDE, Outline,
    PIXELS_FILE, PageContent, PixelOrder, Rect, SearchHit, WorkerReply, WorkerRequest, words,
    worker,
};

/// The largest document read: djvu-rs holds the file in memory.
const MAX_FILE: u64 = 1 << 30;
/// The bytes of pages djvu-rs keeps decoded between two renders, the page
/// drawn last apart: drawn again sharper, it is not decoded again.
const KEPT_BYTES: usize = 64 << 20;
/// Headings an outline keeps, the deepest one, and its longest title.
const HEADINGS: usize = 4096;
const DEPTH: u32 = 16;
const TITLE: usize = 200;
/// The links a page keeps.
const LINKS: usize = 10_000;

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
    // (`platform::process::probe`): it links nothing it could miss.
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
        "expected DjVu path, scratch directory and maybe {FIRST_PAGE}"
    );
    let options = ParseOptions {
        limits: Some(ResourceLimits {
            max_file_bytes: Some(MAX_FILE),
            ..ResourceLimits::default()
        }),
    };
    let document = djvu_rs::Document::open_with_options(&args[0], &options)
        .map_err(|error| anyhow::anyhow!("{error}"))
        .context("open DjVu document")?;
    let document = Document::new(document.inner())?;
    // Its first page alone (a thumbnail): no outline.
    worker::greet(&WorkerReply::Ready(DocumentInfo {
        pages: document.pages,
        outlines: if first_page {
            Vec::new()
        } else {
            document.outline()
        },
        reflowable: false,
        // djvu-rs does not say what the document names its pages.
        labels: Vec::new(),
    }))?;
    let scratch = Path::new(&args[1]);
    worker::serve(|request, superseded| match request {
        WorkerRequest::Render { page, side } => document.render(page, side, scratch),
        WorkerRequest::Search { query } => document
            .search(&query, superseded)
            .map(WorkerReply::Matches),
        // A DjVu page never moves.
        WorkerRequest::Locate { .. } => Ok(WorkerReply::Located(None)),
        // `serve` answers it: nothing runs by the time it is read.
        WorkerRequest::Cancel => anyhow::bail!("nothing to cancel"),
    })
}

/// A `DjVu` document as the protocol reads it.
struct Document<'a> {
    inner: &'a DjVuDocument,
    pages: u32,
    /// Each component's identifier and the page it is, for links that name
    /// a page by its file (`#p0012.djvu`).
    ids: Vec<(String, u32)>,
}

impl<'a> Document<'a> {
    fn new(inner: &'a DjVuDocument) -> anyhow::Result<Self> {
        let pages = u32::try_from(inner.page_count())?;
        anyhow::ensure!(pages > 0, "document has no pages");
        let mut ids = Vec::new();
        if let Ok(components) = inner.component_directory() {
            let mut page = 0;
            for component in components {
                if component.kind == 'P' {
                    page += 1;
                    ids.push((component.id, page));
                }
            }
        }
        Ok(Self { inner, pages, ids })
    }

    fn page(&self, number: u32) -> anyhow::Result<&DjVuPage> {
        self.inner
            .page(number as usize - 1)
            .map_err(|error| anyhow::anyhow!("page {number}: {error}"))
    }

    /// The outline's headings in order, each at its depth.
    fn outline(&self) -> Vec<Outline> {
        fn walk(
            document: &Document,
            marks: &[DjVuBookmark],
            depth: u32,
            outline: &mut Vec<Outline>,
        ) {
            for mark in marks {
                if outline.len() >= HEADINGS {
                    return;
                }
                let title: String = mark.title.trim().chars().take(TITLE).collect();
                if !title.is_empty() {
                    outline.push(Outline {
                        title,
                        depth: depth.min(DEPTH),
                        page: document.page_of(&mark.url, 1),
                    });
                }
                walk(document, &mark.children, depth + 1, outline);
            }
        }
        let mut outline = Vec::new();
        walk(self, self.inner.bookmarks(), 0, &mut outline);
        outline
    }

    /// The page a link inside the document leads to (`#12`, `#+1`, `#-1`,
    /// `#p0012.djvu`), from page `here`; `None` for one that leads out.
    fn page_of(&self, url: &str, here: u32) -> Option<u32> {
        let target = url.strip_prefix('#')?;
        let page = if let Some(ahead) = target.strip_prefix('+') {
            here.checked_add(ahead.parse().ok()?)?
        } else if let Some(back) = target.strip_prefix('-') {
            here.checked_sub(back.parse().ok()?)?
        } else if let Ok(number) = target.parse::<u32>() {
            number
        } else {
            self.ids.iter().find(|(id, _)| id == target)?.1
        };
        (1..=self.pages).contains(&page).then_some(page)
    }

    /// Page `number` drawn with at most `side` pixels on its longer side.
    fn render(&self, number: u32, side: u32, scratch: &Path) -> anyhow::Result<WorkerReply> {
        let number = number.clamp(1, self.pages);
        let page = self.page(number)?;
        let geometry = Geometry::of(page);
        let limit = f64::from(side.clamp(64, MAX_SIDE));
        let scale = limit / f64::from(geometry.width.max(geometry.height));
        // Rounding must not push a side above the protocol's limit.
        let width = (f64::from(geometry.width) * scale).round().clamp(1., limit) as u32;
        let height = (f64::from(geometry.height) * scale)
            .round()
            .clamp(1., limit) as u32;
        // Shrunk to half or less, the page is put together at twice the
        // side and each two by two pixels averaged: djvu-rs's own
        // smoothing, at a quarter of the cost of a filter over the whole
        // page (ADR-061). Never above the protocol's side.
        let smooth = width * 2 <= geometry.width
            && height * 2 <= geometry.height
            && width.max(height) * 2 <= MAX_SIDE;
        let times = if smooth { 2 } else { 1 };
        // The options are in the page's own orientation; the picture is
        // turned as the page says.
        let quarter = geometry.quarters % 2 == 1;
        let stored = if quarter {
            (geometry.height, geometry.width)
        } else {
            (geometry.width, geometry.height)
        };
        let wanted = if quarter {
            (height * times, width * times)
        } else {
            (width * times, height * times)
        };
        let (across, down) = exact_size(stored, wanted);
        let options = RenderOptions {
            width: across,
            height: down,
            aa: smooth,
            ..RenderOptions::default()
        };
        let pixmap = render_pixmap(page, &options)
            .map_err(|error| anyhow::anyhow!("page {number}: {error}"))?;
        self.inner
            .enforce_cache_budget(KEPT_BYTES, &[number as usize - 1]);
        let (width, height) = if quarter {
            (down / times, across / times)
        } else {
            (across / times, down / times)
        };
        anyhow::ensure!(
            pixmap.width == width
                && pixmap.height == height
                && pixmap.data.len() == width as usize * height as usize * 4,
            "page {number} was drawn {}×{}, not {width}×{height}",
            pixmap.width,
            pixmap.height
        );
        let mut output = BufWriter::new(std::fs::File::create(scratch.join(PIXELS_FILE))?);
        output.write_all(&pixmap.data)?;
        output.flush()?;
        let content = PageContent {
            page: number,
            width: geometry.css(geometry.width),
            height: geometry.css(geometry.height),
            chars: words::chars(&runs(page, &geometry)),
            links: self.links(page, &geometry, number),
        };
        Ok(WorkerReply::Page {
            content,
            width,
            height,
            order: PixelOrder::Rgba,
        })
    }

    /// The page's links: a part of it that leads to another page or out.
    fn links(&self, page: &DjVuPage, geometry: &Geometry, here: u32) -> Vec<Link> {
        page.hyperlinks()
            .unwrap_or_default()
            .iter()
            .take(LINKS)
            .filter_map(|area| {
                let destination = match self.page_of(&area.url, here) {
                    Some(page) => Destination::Page(page),
                    None if area.url.starts_with('#') || area.url.trim().is_empty() => {
                        return None;
                    }
                    None => Destination::Uri(area.url.clone()),
                };
                Some(Link {
                    bounds: area_bounds(area, geometry)?,
                    destination,
                })
            })
            .collect()
    }

    /// Every page's words searched, whatever the case; stops as soon as a
    /// newer request waits.
    fn search(&self, query: &str, superseded: &dyn Fn() -> bool) -> anyhow::Result<Vec<SearchHit>> {
        anyhow::ensure!(
            query.len() <= 4096 && !query.contains('\0'),
            "invalid search query"
        );
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut hits = Vec::new();
        for number in 1..=self.pages {
            anyhow::ensure!(!superseded(), "search superseded");
            let page = self.page(number)?;
            let chars = words::chars(&runs(page, &Geometry::of(page)));
            for bounds in words::matches(&chars, query) {
                hits.push(SearchHit {
                    page: number,
                    bounds,
                });
                if hits.len() == MAX_HITS {
                    return Ok(hits);
                }
            }
        }
        Ok(hits)
    }
}

/// djvu-rs's step through a page, in its pixels, is counted in this many
/// parts of one, and truncated (its `FRAC`, 0.42).
const STEP_PARTS: u64 = 16;

/// How much larger than the page djvu-rs may draw it: two thousandths, a
/// pixel at 500, what rounding alone costs `DjVuLibre`.
const TRUNCATED: f64 = 0.002;

/// The size, at most `wanted`, a page of `stored` pixels is drawn at so
/// that it lies where its text says. djvu-rs steps through the page by
/// `stored × 16 / size` sixteenths of a pixel, truncated: at a size whose
/// step is no whole number of sixteenths, its drawing is larger than the
/// page (by 2% for a page shrunk to two thirds). The largest size below
/// `wanted` drawn within `TRUNCATED` across and down, else the one drawn
/// nearest; the reader shows any size at the page's own (ADR-061).
fn exact_size(stored: (u32, u32), wanted: (u32, u32)) -> (u32, u32) {
    let (stored_w, stored_h) = (u64::from(stored.0.max(1)), u64::from(stored.1.max(1)));
    let wanted_w = u64::from(wanted.0.max(1));
    // How much larger than the page a step truncated draws it.
    let error = |size: u64, page: u64| {
        let exact = (page * STEP_PARTS) as f64 / size as f64;
        (exact - exact.floor()) / exact
    };
    let first = (stored_w * STEP_PARTS).div_ceil(wanted_w).max(1);
    let sizes: Vec<(f64, u64, u64)> = (first..first + STEP_PARTS)
        .filter_map(|step| {
            let across = (stored_w * STEP_PARTS / step).max(1);
            let down = (stored_h * STEP_PARTS / step).max(1);
            (across <= u64::from(wanted.0) && down <= u64::from(wanted.1.max(1))).then(|| {
                (
                    error(across, stored_w).max(error(down, stored_h)),
                    across,
                    down,
                )
            })
        })
        .collect();
    sizes
        .iter()
        .find(|(error, ..)| *error <= TRUNCATED)
        .or_else(|| sizes.iter().min_by(|a, b| a.0.total_cmp(&b.0)))
        .and_then(|(_, across, down)| {
            Some((u32::try_from(*across).ok()?, u32::try_from(*down).ok()?))
        })
        .unwrap_or(wanted)
}

/// A page's sides as shown (turned as it says), in its pixels, and how it
/// turns.
struct Geometry {
    width: u32,
    height: u32,
    /// Quarter turns counterclockwise from the page as stored.
    quarters: u8,
    dpi: u16,
}

impl Geometry {
    fn of(page: &DjVuPage) -> Self {
        let quarters = match page.rotation() {
            djvu_rs::Rotation::None => 0,
            djvu_rs::Rotation::Ccw90 => 1,
            djvu_rs::Rotation::Rot180 => 2,
            djvu_rs::Rotation::Cw90 => 3,
        };
        let (stored_width, stored_height) = (
            u32::from(page.width()).max(1),
            u32::from(page.height()).max(1),
        );
        let (width, height) = if quarters % 2 == 1 {
            (stored_height, stored_width)
        } else {
            (stored_width, stored_height)
        };
        Self {
            width,
            height,
            quarters,
            // A resolution out of the format's range is its default.
            dpi: Some(page.dpi())
                .filter(|dpi| (25..=6000).contains(dpi))
                .unwrap_or(300),
        }
    }

    /// `pixels` of the page as CSS pixels (1/96 in), what 100% shows.
    fn css(&self, pixels: u32) -> f32 {
        (f64::from(pixels) * 96. / f64::from(self.dpi)).min(f64::from(MAX_PAGE_SIZE)) as f32
    }
}

/// A page's words in reading order, each in its box as the page is shown:
/// the text layer's words (their characters' boxes as edges), or its
/// finest zones that hold text when it has no words.
fn runs(page: &DjVuPage, geometry: &Geometry) -> Vec<words::Run> {
    fn walk(zone: &TextZone, stored: (f32, f32), geometry: &Geometry, runs: &mut Vec<words::Run>) {
        let finest = zone.children.is_empty()
            || zone.kind == TextZoneKind::Word
            || zone
                .children
                .iter()
                .all(|child| child.kind == TextZoneKind::Character);
        if !finest {
            for child in &zone.children {
                walk(child, stored, geometry, runs);
            }
            return;
        }
        let text = zone.text.trim();
        if text.is_empty() {
            return;
        }
        let (w, h) = stored;
        let rect = |r: &djvu_rs::text::Rect| Rect {
            x: r.x as f32 / w,
            y: r.y as f32 / h,
            width: r.width as f32 / w,
            height: r.height as f32 / h,
        };
        // Characters' boxes say where each starts, on a page shown as
        // stored: turned, a word runs across another way.
        let edges = if geometry.quarters == 0
            && zone.children.len() == text.chars().count()
            && zone
                .children
                .iter()
                .all(|child| child.kind == TextZoneKind::Character)
        {
            zone.children
                .iter()
                .map(|child| rect(&child.rect).x)
                .collect()
        } else {
            Vec::new()
        };
        runs.push(words::Run {
            text: text.to_owned(),
            bounds: rect(&zone.rect).turned(geometry.quarters),
            edges,
        });
    }
    let Ok(Some(layer)) = page.text_layer() else {
        return Vec::new();
    };
    // The text layer's boxes are in the page's pixels as stored, from its
    // top left.
    let stored = if geometry.quarters % 2 == 1 {
        (geometry.height as f32, geometry.width as f32)
    } else {
        (geometry.width as f32, geometry.height as f32)
    };
    let mut runs = Vec::new();
    for zone in &layer.zones {
        walk(zone, stored, geometry, &mut runs);
    }
    runs
}

/// Where a link's area lies on the page as shown: the box around its shape
/// (in the page's pixels as shown, from its bottom left). A line leads
/// nowhere one can point.
fn area_bounds(area: &MapArea, geometry: &Geometry) -> Option<Rect> {
    let (left, bottom, right, top) = match &area.shape {
        Shape::Rect(r) | Shape::Oval(r) | Shape::Text(r) => {
            (r.x, r.y, r.x + r.width, r.y + r.height)
        }
        Shape::Poly(points) => {
            let xs = points.iter().map(|p| p.0);
            let ys = points.iter().map(|p| p.1);
            (xs.clone().min()?, ys.clone().min()?, xs.max()?, ys.max()?)
        }
        Shape::Line(..) => return None,
    };
    let (w, h) = (geometry.width as f32, geometry.height as f32);
    let bounds = Rect {
        x: left as f32 / w,
        y: 1. - top as f32 / h,
        width: right.saturating_sub(left) as f32 / w,
        height: top.saturating_sub(bottom) as f32 / h,
    };
    (bounds.width > 0. && bounds.height > 0.).then_some(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_shown_at_its_resolution_is_its_size_in_css_pixels() {
        let geometry = Geometry {
            width: 2480,
            height: 3508,
            quarters: 0,
            dpi: 300,
        };
        // A4 at 300 dpi: 8.27 in, 793.6 CSS pixels.
        assert!((geometry.css(2480) - 793.6).abs() < 0.01);
        let huge = Geometry {
            dpi: 25,
            ..geometry
        };
        assert!((huge.css(60_000) - MAX_PAGE_SIZE).abs() < f32::EPSILON);
    }

    #[test]
    fn a_page_is_drawn_at_a_size_djvu_rs_steps_through_whole() {
        // How much larger than the page djvu-rs draws it at `size`.
        let truncated = |size: u32, page: u32| {
            let exact = f64::from(page) * 16. / f64::from(size);
            (exact - exact.floor()) / exact
        };
        for (stored, wanted) in [
            // Shrunk to two thirds, where djvu-rs drew 2% too large.
            ((1700, 2200), (1159, 1500)),
            ((2480, 3508), (724, 1024)),
            ((1700, 2200), (464, 600)),
            // Enlarged twice, and a thumbnail.
            ((850, 1100), (1700, 2200)),
            ((2480, 3508), (91, 128)),
        ] {
            let (across, down) = exact_size(stored, wanted);
            assert!(across <= wanted.0 && down <= wanted.1, "{across}×{down}");
            assert!(
                f64::from(down) >= f64::from(wanted.1) * 0.93,
                "{across}×{down}"
            );
            assert!(
                truncated(across, stored.0) <= TRUNCATED && truncated(down, stored.1) <= TRUNCATED,
                "{stored:?} at {across}×{down}"
            );
        }
    }
}
