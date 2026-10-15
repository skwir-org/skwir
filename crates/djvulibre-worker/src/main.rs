//! `DjVu` read by `DjVuLibre`, the reference library, behind the protocol every
//! document worker speaks (`skwir_core::document::worker`): a page drawn at
//! the side asked, its words, its links and the document's outline. Native
//! handles never leave this process, and the document is untrusted: the
//! worker runs isolated where the system allows (ADR-041, ADR-061).
use std::io::{BufWriter, Write};
use std::path::Path;

use skwir_core::document::{
    Destination, DocumentInfo, FIRST_PAGE, Link, MAX_HITS, MAX_PAGE_SIZE, MAX_SIDE, Outline,
    PIXELS_FILE, PageContent, PixelOrder, Rect, SearchHit, WorkerReply, WorkerRequest, page_labels,
    words, worker,
};

mod native;

use native::{Sexp, Shown};

/// Headings an outline keeps, the deepest one, and its longest title.
const HEADINGS: usize = 4096;
const DEPTH: u32 = 16;
const TITLE: usize = 200;

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
    // library is there.
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
    let context = native::Context::new()?;
    let document = Document::new(native::Document::open(&context, Path::new(&args[0]))?)?;
    // Its first page alone (a thumbnail): no outline, no labels.
    worker::greet(&WorkerReply::Ready(DocumentInfo {
        pages: document.pages,
        outlines: if first_page {
            Vec::new()
        } else {
            document.outline()
        },
        reflowable: false,
        labels: if first_page {
            Vec::new()
        } else {
            document.labels.clone()
        },
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
    native: native::Document<'a>,
    pages: u32,
    /// Each page's file name and title, for links that name a page so
    /// (`#p0012.djvu`).
    names: Vec<(String, u32)>,
    /// What the document calls its pages, where it names them otherwise
    /// than their files and numbers (`page_labels`).
    labels: Vec<String>,
}

impl<'a> Document<'a> {
    fn new(native: native::Document<'a>) -> anyhow::Result<Self> {
        let pages = native.pages();
        anyhow::ensure!(pages > 0, "document has no pages");
        let mut names = Vec::new();
        let mut labels = vec![String::new(); pages as usize];
        for component in native.components() {
            let Some(page) = component.page.filter(|page| *page <= pages) else {
                continue;
            };
            // A page titled is titled otherwise than its file.
            if !component.title.is_empty() && component.title != component.id {
                labels[page as usize - 1].clone_from(&component.title);
                names.push((component.title, page));
            }
            names.push((component.id, page));
        }
        Ok(Self {
            native,
            pages,
            names,
            labels: page_labels(labels),
        })
    }

    /// The outline's headings in order, each at its depth.
    fn outline(&self) -> Vec<Outline> {
        fn walk(document: &Document, entries: &[Sexp], depth: u32, outline: &mut Vec<Outline>) {
            for entry in entries {
                if outline.len() >= HEADINGS {
                    return;
                }
                let Sexp::List(items) = entry else { continue };
                let (Some(Sexp::Text(title)), url) = (items.first(), items.get(1)) else {
                    continue;
                };
                let title: String = title.trim().chars().take(TITLE).collect();
                if !title.is_empty() {
                    outline.push(Outline {
                        title,
                        depth: depth.min(DEPTH),
                        page: match url {
                            Some(Sexp::Text(url)) => document.page_of(url, 1),
                            _ => None,
                        },
                    });
                }
                walk(
                    document,
                    items.get(2..).unwrap_or_default(),
                    depth + 1,
                    outline,
                );
            }
        }
        let mut outline = Vec::new();
        // `(bookmarks entry…)`.
        if let Some(Sexp::List(items)) = self.native.outline()
            && matches!(items.first(), Some(Sexp::Symbol(head)) if head == "bookmarks")
        {
            walk(self, &items[1..], 0, &mut outline);
        }
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
            self.names.iter().find(|(name, _)| name == target)?.1
        };
        (1..=self.pages).contains(&page).then_some(page)
    }

    /// Page `number` drawn with at most `side` pixels on its longer side.
    fn render(&self, number: u32, side: u32, scratch: &Path) -> anyhow::Result<WorkerReply> {
        let number = number.clamp(1, self.pages);
        let shown = self.native.shown(number)?;
        let limit = f64::from(side.clamp(64, MAX_SIDE));
        let scale = limit / f64::from(shown.width.max(shown.height));
        // Rounding must not push a side above the protocol's limit.
        let width = (f64::from(shown.width) * scale).round().clamp(1., limit) as u32;
        let height = (f64::from(shown.height) * scale).round().clamp(1., limit) as u32;
        let pixels = self.native.render(number, width, height)?;
        let mut output = BufWriter::new(std::fs::File::create(scratch.join(PIXELS_FILE))?);
        output.write_all(&pixels)?;
        output.flush()?;
        let content = PageContent {
            page: number,
            width: css(&shown, shown.width),
            height: css(&shown, shown.height),
            chars: words::chars(&self.runs(number, &shown)),
            links: self.links(number, &shown),
        };
        Ok(WorkerReply::Page {
            content,
            width,
            height,
            order: PixelOrder::Bgra,
        })
    }

    /// Page `number`'s words in reading order, each in its box as the page
    /// is shown.
    fn runs(&self, number: u32, shown: &Shown) -> Vec<words::Run> {
        let Some(text) = self.native.text(number) else {
            return Vec::new();
        };
        // The text's boxes are in the page's pixels as stored, from its
        // bottom left.
        let stored = if shown.quarters % 2 == 1 {
            (shown.height as f32, shown.width as f32)
        } else {
            (shown.width as f32, shown.height as f32)
        };
        let mut runs = Vec::new();
        zone_runs(&text, stored, shown.quarters, &mut runs);
        runs
    }

    /// Page `number`'s links: a part of it that leads to another page or
    /// out.
    fn links(&self, number: u32, shown: &Shown) -> Vec<Link> {
        self.native
            .links(number)
            .iter()
            .filter_map(|area| {
                // `(maparea url comment shape …)`, the url a string or
                // `(url "href" "target")`.
                let Sexp::List(items) = area else { return None };
                let url = match items.get(1)? {
                    Sexp::Text(url) => url.as_str(),
                    Sexp::List(parts) => match parts.get(1)? {
                        Sexp::Text(url) => url.as_str(),
                        _ => return None,
                    },
                    _ => return None,
                };
                let destination = match self.page_of(url, number) {
                    Some(page) => Destination::Page(page),
                    None if url.starts_with('#') || url.trim().is_empty() => return None,
                    None => Destination::Uri(url.to_owned()),
                };
                Some(Link {
                    bounds: area_bounds(items.get(3)?, shown)?,
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
            let shown = self.native.shown(number)?;
            let chars = words::chars(&self.runs(number, &shown));
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

/// `pixels` of the page as CSS pixels (1/96 in), what 100% shows. A
/// resolution out of the format's range is its default.
fn css(shown: &Shown, pixels: u32) -> f32 {
    let dpi = Some(shown.dpi)
        .filter(|dpi| (25..=6000).contains(dpi))
        .unwrap_or(300);
    (f64::from(pixels) * 96. / f64::from(dpi)).min(f64::from(MAX_PAGE_SIZE)) as f32
}

/// The words of a text zone, `(kind xmin ymin xmax ymax text-or-zones…)`
/// in the page's pixels as stored (`stored`), from its bottom left: each
/// word, or each finest zone that holds text, a run.
fn zone_runs(zone: &Sexp, stored: (f32, f32), quarters: u8, runs: &mut Vec<words::Run>) {
    let Sexp::List(items) = zone else { return };
    let number = |at: usize| match items.get(at) {
        Some(Sexp::Number(n)) => Some(*n as f32),
        _ => None,
    };
    let (Some(Sexp::Symbol(_)), Some(left), Some(bottom), Some(right), Some(top)) =
        (items.first(), number(1), number(2), number(3), number(4))
    else {
        return;
    };
    let rest = items.get(5..).unwrap_or_default();
    if let [Sexp::Text(text)] = rest {
        let text = text.trim();
        if !text.is_empty() {
            let (w, h) = stored;
            runs.push(words::Run {
                text: text.to_owned(),
                bounds: Rect {
                    x: left.min(right) / w,
                    y: 1. - top.max(bottom) / h,
                    width: (right - left).abs() / w,
                    height: (top - bottom).abs() / h,
                }
                .turned(quarters),
                edges: Vec::new(),
            });
        }
        return;
    }
    for child in rest {
        zone_runs(child, stored, quarters, runs);
    }
}

/// Where a link's area lies on the page as shown: the box around its shape
/// (`(rect x y w h)`, `(oval …)`, `(text …)`, `(poly x y x y …)`), in the
/// page's pixels as shown, from its bottom left. A line leads nowhere one
/// can point.
fn area_bounds(shape: &Sexp, shown: &Shown) -> Option<Rect> {
    let Sexp::List(items) = shape else {
        return None;
    };
    let Some(Sexp::Symbol(kind)) = items.first() else {
        return None;
    };
    let numbers: Vec<f32> = items[1..]
        .iter()
        .map_while(|item| match item {
            Sexp::Number(n) => Some(*n as f32),
            _ => None,
        })
        .collect();
    let (left, bottom, right, top) = match (kind.as_str(), numbers.as_slice()) {
        ("rect" | "oval" | "text", [x, y, w, h, ..]) => (*x, *y, x + w, y + h),
        ("poly", points) if points.len() >= 4 => {
            let xs = points.iter().step_by(2);
            let ys = points.iter().skip(1).step_by(2);
            (
                xs.clone().copied().fold(f32::INFINITY, f32::min),
                ys.clone().copied().fold(f32::INFINITY, f32::min),
                xs.copied().fold(f32::NEG_INFINITY, f32::max),
                ys.copied().fold(f32::NEG_INFINITY, f32::max),
            )
        }
        _ => return None,
    };
    let (w, h) = (shown.width as f32, shown.height as f32);
    let bounds = Rect {
        x: left / w,
        y: 1. - top / h,
        width: (right - left) / w,
        height: (top - bottom) / h,
    };
    (bounds.width > 0. && bounds.height > 0.).then_some(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, box_: [i32; 4]) -> Sexp {
        let mut items = vec![Sexp::Symbol("word".into())];
        items.extend(box_.iter().map(|n| Sexp::Number(*n)));
        items.push(Sexp::Text(text.into()));
        Sexp::List(items)
    }

    fn zone(kind: &str, box_: [i32; 4], children: Vec<Sexp>) -> Sexp {
        let mut items = vec![Sexp::Symbol(kind.into())];
        items.extend(box_.iter().map(|n| Sexp::Number(*n)));
        items.extend(children);
        Sexp::List(items)
    }

    #[test]
    fn words_are_read_from_the_bottom_left_and_turned_as_the_page() {
        // A page 1000 wide and 2000 high, as stored.
        let page = zone(
            "page",
            [0, 0, 1000, 2000],
            vec![zone(
                "line",
                [100, 1800, 900, 1900],
                vec![
                    word("Hello", [100, 1800, 400, 1900]),
                    word("you", [500, 1800, 900, 1900]),
                ],
            )],
        );
        let mut runs = Vec::new();
        zone_runs(&page, (1000., 2000.), 0, &mut runs);
        let texts: Vec<&str> = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["Hello", "you"]);
        let hello = runs[0].bounds;
        assert!((hello.x - 0.1).abs() < 1e-6 && (hello.y - 0.05).abs() < 1e-6);
        assert!((hello.width - 0.3).abs() < 1e-6 && (hello.height - 0.05).abs() < 1e-6);
        // Turned a quarter counterclockwise, the top of the page is its left.
        let mut turned = Vec::new();
        zone_runs(&page, (1000., 2000.), 1, &mut turned);
        let hello = turned[0].bounds;
        assert!(hello.x < 0.1 && hello.y > 0.5, "{hello:?}");
    }

    #[test]
    fn a_link_is_the_box_around_its_shape() {
        let shown = Shown {
            width: 1000,
            height: 2000,
            dpi: 300,
            quarters: 0,
        };
        let numbers = |kind: &str, n: &[i32]| {
            let mut items = vec![Sexp::Symbol(kind.into())];
            items.extend(n.iter().map(|n| Sexp::Number(*n)));
            Sexp::List(items)
        };
        let rect = area_bounds(&numbers("rect", &[100, 1800, 200, 100]), &shown).unwrap();
        assert!((rect.x - 0.1).abs() < 1e-6 && (rect.y - 0.05).abs() < 1e-6);
        let poly = area_bounds(&numbers("poly", &[100, 100, 300, 100, 200, 300]), &shown).unwrap();
        assert!((poly.width - 0.2).abs() < 1e-6 && (poly.height - 0.1).abs() < 1e-6);
        assert!(area_bounds(&numbers("line", &[0, 0, 10, 10]), &shown).is_none());
    }
}
