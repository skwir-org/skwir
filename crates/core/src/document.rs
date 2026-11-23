//! Toolkit-independent reader data and the bounded document-worker protocol.
//! Rectangles use normalized coordinates, with the origin at the displayed
//! page's top left; selection geometry is independent of zoom and resolution.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::CancelToken;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }

    /// Where this part of a page lies once the page is turned `quarters`
    /// quarter turns counterclockwise: a box of a page as stored, on the
    /// page as shown.
    #[must_use]
    pub fn turned(self, quarters: u8) -> Self {
        let turn = |u: f32, v: f32| match quarters % 4 {
            1 => (v, 1. - u),
            2 => (1. - u, 1. - v),
            3 => (1. - v, u),
            _ => (u, v),
        };
        let (a, b) = (
            turn(self.x, self.y),
            turn(self.x + self.width, self.y + self.height),
        );
        Self {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            width: (a.0 - b.0).abs(),
            height: (a.1 - b.1).abs(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextChar {
    pub text: String,
    pub bounds: Rect,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Destination {
    /// One-based page number.
    Page(u32),
    Uri(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Link {
    pub bounds: Rect,
    pub destination: Destination,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outline {
    pub title: String,
    pub depth: u32,
    pub page: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentInfo {
    pub pages: u32,
    pub outlines: Vec<Outline>,
    /// The engine laid the document out into these pages itself (a book):
    /// a text size of its own gives other pages. PDF pages are fixed.
    #[serde(default)]
    pub reflowable: bool,
    /// Each page's label as printed ("iv", "47", "A-3"), when the document
    /// names its pages otherwise than by their number (`page_labels`);
    /// empty when it does not. A page without one has an empty label.
    #[serde(default)]
    pub labels: Vec<String>,
    /// The way the document says it reads (its pages turned, its text):
    /// a PDF's `/ViewerPreferences /Direction`, a comic's `ComicInfo`
    /// manga read right to left, a book's page progression. `None` where
    /// it says nothing: what is printed of it is told by its text, else
    /// left to right (ADR-111).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<crate::direction::Direction>,
}

/// The most pages of one document that are read (`DocumentInfo::pages`):
/// the reader lays out an item for each, on the window's thread, and a
/// search reads each in turn, so a count past it (a damaged file's, a
/// plugin's four billion) is held to it where it is read, and a longer
/// document shows its first pages. No document a person reads comes near.
pub const MAX_PAGES: u32 = 100_000;

/// The longest page label kept, in characters.
const LABEL_CHARS: usize = 64;

/// A document's page labels as the protocol carries them: trimmed and
/// bounded, and none at all when every labelled page is its own number, or
/// none is labelled: a label is worth showing only where it differs.
pub fn page_labels(labels: impl IntoIterator<Item = String>) -> Vec<String> {
    let labels: Vec<String> = labels
        .into_iter()
        .map(|label| label.trim().chars().take(LABEL_CHARS).collect())
        .collect();
    let plain = labels
        .iter()
        .enumerate()
        .all(|(index, label)| label.is_empty() || *label == (index + 1).to_string());
    if plain { Vec::new() } else { labels }
}

/// CSS pixels (1/96 in) in a point (1/72 in): a page's own size is given
/// in CSS pixels, so a PDF, a book's page and a picture share one 100%.
pub const CSS_PER_POINT: f32 = 96. / 72.;
/// The largest a page's own side may be, in CSS pixels (a PDF's largest
/// page is 200 inches).
pub const MAX_PAGE_SIZE: f32 = 20_000.;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageContent {
    pub page: u32,
    /// The page's own size in CSS pixels, what 100% zoom shows, whatever
    /// side its pixels were rendered at (`DocumentPage::width`).
    pub width: f32,
    pub height: f32,
    pub chars: Vec<TextChar>,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug)]
pub struct DocumentPage {
    pub width: u32,
    pub height: u32,
    /// Four bytes a pixel, in `order`. A single owned buffer transfers
    /// from the manager to the UI worker: no Arc conversion, and no
    /// full-page copy to swizzle.
    pub pixels: Vec<u8>,
    pub order: PixelOrder,
    pub content: Arc<PageContent>,
    /// The part of the page the pixels are, exactly as drawn
    /// (`DocumentSession::render_part`, ADR-147); `None` for the whole.
    pub part: Option<Part>,
}

/// A part of a page, as shares of its width and height (0 to 1, from its
/// top left): what a page zoomed past what is held of it whole draws of
/// itself, where it shows (ADR-147). Shares, never pixels: each engine
/// lays its own pixels on the page at a side, and says back the part it
/// drew exactly ([`Part::of_pixels`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Part {
    /// The pixels of a page drawn `width` × `height` this part covers:
    /// left, top, width and height, whole pixels round it, at least one.
    pub fn pixels(&self, width: u32, height: u32) -> [u32; 4] {
        // A share is a float: a pixel's edge it names may come a hair past
        // it, which is that edge still.
        let at = |share: f32, length: u32, up: bool| {
            let at = f64::from(share.clamp(0., 1.)) * f64::from(length);
            let at = if up {
                (at - 1e-3).ceil()
            } else {
                (at + 1e-3).floor()
            };
            (at.max(0.) as u32).min(length)
        };
        let (left, top) = (at(self.left, width, false), at(self.top, height, false));
        let right = at(self.right, width, true).max(left + 1).min(width.max(1));
        let bottom = at(self.bottom, height, true)
            .max(top + 1)
            .min(height.max(1));
        let (left, top) = (left.min(right - 1), top.min(bottom - 1));
        [left, top, right - left, bottom - top]
    }

    /// The shares `pixels` (left, top, width, height) are of a page drawn
    /// `width` × `height`: what an engine says it drew.
    pub fn of_pixels([left, top, w, h]: [u32; 4], width: u32, height: u32) -> Self {
        let share = |at: u32, length: u32| (f64::from(at) / f64::from(length.max(1))) as f32;
        Part {
            left: share(left, width),
            top: share(top, height),
            right: share(left + w, width),
            bottom: share(top + h, height),
        }
    }

    /// The pixels this part is drawn at on a page laid `width` × `height`
    /// ([`Part::pixels`]), refused past what a page's pixels may be
    /// (`MAX_PAGE_PIXELS`, the manager's bound): what every engine draws.
    /// The reader sizes a part by [`Part::most_pixels`], never fewer than
    /// these, to `PAGE_PIXELS`, well within (ADR-147).
    pub fn drawn(&self, width: u32, height: u32) -> anyhow::Result<[u32; 4]> {
        let drawn = self.pixels(width, height);
        anyhow::ensure!(
            u64::from(drawn[2]) * u64::from(drawn[3]) <= MAX_PAGE_PIXELS,
            "the part asked is larger than a page may be"
        );
        Ok(drawn)
    }

    /// The most pixels this part is drawn at on a page of `own` size (in
    /// any unit) laid with `side` on its longer side, however an engine
    /// lays the page's pixels there (rounded, rounded up, or a little
    /// smaller, as djvu-rs draws) and the part's whole pixels round it:
    /// what the reader counts a part by, never fewer than an engine's
    /// [`Part::drawn`] (ADR-147).
    pub fn most_pixels(&self, own: (f32, f32), side: u32) -> u64 {
        let (width, height) = (f64::from(own.0.max(1.)), f64::from(own.1.max(1.)));
        let scale = f64::from(side) / width.max(height);
        // The page laid at most a pixel past its size at that scale, and
        // the part's whole pixels a pixel and a half more each way at most.
        let along = |from: f32, to: f32, length: f64| {
            let laid = (length * scale).ceil() + 1.;
            (f64::from((to - from).clamp(0., 1.)) * laid + 3.).min(laid)
        };
        (along(self.left, self.right, width) * along(self.top, self.bottom, height)).ceil() as u64
    }

    /// Whether this part covers all of `other`.
    pub fn holds(&self, other: &Part) -> bool {
        self.left <= other.left
            && self.top <= other.top
            && self.right >= other.right
            && self.bottom >= other.bottom
    }
}

/// The longest side a page drawn whole is asked at: `side`, at most
/// `MAX_SIDE`, or, for a page longer than an A4 one, as many pixels as an
/// A4 page has there (`PAGE_PIXELS`, ADR-147). `width` × `height` is its
/// own size, in any unit. A tall page (a webtoon's, 800 × 12 000) is held
/// whole at its own width, where `MAX_SIDE` alone held it at 273 pixels
/// across; no page is held at fewer pixels than `MAX_SIDE` held it.
pub fn held_side(width: f32, height: f32, side: u32) -> u32 {
    let (width, height) = (f64::from(width.max(1.)), f64::from(height.max(1.)));
    let counted = (width.max(height) * (PAGE_PIXELS as f64 / (width * height)).sqrt()).floor();
    let most = counted.max(f64::from(MAX_SIDE));
    (f64::from(side).min(most).max(64.)).min(f64::from(MAX_PART_SIDE)) as u32
}

impl DocumentPage {
    /// The part `part` of this page, cut from its pixels: what an engine
    /// that draws no part gives (`DocumentSession::render_part`), the part
    /// it says exactly the pixels cut.
    pub fn cut(self, part: Part) -> Self {
        let [left, top, w, h] = part.pixels(self.width, self.height);
        let stride = self.width as usize * 4;
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for row in top..top + h {
            let start = row as usize * stride + left as usize * 4;
            pixels.extend_from_slice(&self.pixels[start..start + w as usize * 4]);
        }
        DocumentPage {
            part: Some(Part::of_pixels([left, top, w, h], self.width, self.height)),
            width: w,
            height: h,
            pixels,
            order: self.order,
            content: self.content,
        }
    }

    /// The page's pixels in RGBA order, for what is not the window.
    pub fn into_rgba(self) -> Vec<u8> {
        let mut pixels = self.pixels;
        if self.order == PixelOrder::Bgra {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        pixels
    }
}

/// The order of a page's four bytes a pixel. An engine says the one it
/// draws in (`PDFium` and Cairo draw BGRA, the order the window shows), so
/// no page is turned into the other order on the way and back again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PixelOrder {
    #[default]
    Rgba,
    Bgra,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub page: u32,
    pub bounds: Vec<Rect>,
}

/// A document is owned by an open preview. Methods block only on workers;
/// dropping the last handle must never wait for a renderer on the UI thread.
pub trait DocumentSession: Send + Sync {
    fn info(&self) -> &DocumentInfo;
    fn render(&self, page: u32, side: u32, cancel: &CancelToken) -> anyhow::Result<DocumentPage>;
    /// `part` of page `page`, drawn as the page would be with `side` on its
    /// longer side, though the whole would be past what a page may hold
    /// (ADR-147): what shows of a page zoomed in. An engine that draws at
    /// the scale asked draws the part alone (and says so, `draws_parts`);
    /// by default the whole is drawn at `side` (as large as it may be) and
    /// the part cut from it, which the reader never asks.
    fn render_part(
        &self,
        page: u32,
        side: u32,
        part: Part,
        cancel: &CancelToken,
    ) -> anyhow::Result<DocumentPage> {
        Ok(self.render(page, side, cancel)?.cut(part))
    }
    fn search(&self, query: &str, cancel: &CancelToken) -> anyhow::Result<Vec<SearchHit>>;
    /// The page nearest `near` holding `text`, for a book laid out again:
    /// the page where what was read now lies. `None` when no page near
    /// enough holds it, or the document's pages never move (a PDF).
    fn locate(&self, text: &str, near: u32, cancel: &CancelToken) -> anyhow::Result<Option<u32>> {
        let _ = (text, near, cancel);
        Ok(None)
    }
    /// Why its links are not followed, when they are not: a plugin from
    /// outside skwir that may not use the network drew its pages, and a
    /// link the user clicks would carry what it read (ADR-047). Said where
    /// a click on one is refused.
    fn closed_links(&self) -> Option<&crate::words::Words> {
        None
    }
    /// The PDF it reads, when it reads one (a PDF, an office document
    /// converted to one): what is printed as it is (ADR-111). Valid while
    /// the session lives; `None` for pages drawn otherwise.
    fn pdf(&self) -> Option<&std::path::Path> {
        None
    }
    /// Whether a page drawn small costs much less than drawn large: an
    /// engine that draws at the scale asked (a PDF's, a `DjVu`'s, which
    /// subsample what they hold). Such a page opening is drawn small first,
    /// to stand in for it while it is drawn sharp; a page that costs about
    /// as much either way (a picture decoded whole, a book laid out) is not
    /// drawn twice.
    fn draws_small_quickly(&self) -> bool {
        false
    }
    /// Whether this session draws a part of a page alone, sharper than the
    /// page whole holds it (`render_part`, ADR-147): the reader asks parts
    /// only of one that does. By default, no: the whole drawn and cut
    /// holds no more than the whole shown (a book laid out, a plugin from
    /// outside Skwir).
    fn draws_parts(&self) -> bool {
        false
    }
}

/// Internal protocol: JSON lines for small requests/metadata, a private RGBA
/// file for pixels. Neither filenames nor document text are written to logs.
///
/// Every line is `Tagged`: the greeting (`Ready`, `Unavailable` or `Error`)
/// is reply 0, and request `n` is answered by reply `n`, once and in order,
/// so a manager drains the answer to a request it gave up on instead of
/// restarting the worker to be rid of it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Tagged<T> {
    pub id: u64,
    pub body: T,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WorkerRequest {
    Render {
        page: u32,
        side: u32,
        /// Only this part of it, the page at `side` however large
        /// (`DocumentSession::render_part`); absent for the whole.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Option<Part>,
    },
    Search {
        query: String,
    },
    /// The page nearest `near` holding `text` (`DocumentSession::locate`).
    /// An engine whose pages are fixed answers `Located(None)`.
    Locate {
        text: String,
        near: u32,
    },
    /// Write a new document of this one's pages (`Compose`), in the
    /// worker's folder as [`COMPOSED_FILE`]: what is printed (ADR-111).
    /// Answered `Composed`; an engine that writes no PDF answers an
    /// `Error`.
    Compose(Compose),
    /// Print every page on Windows' printer `printer`, with its settings
    /// as the print dialog left them (`devmode`, a `DEVMODEW`), the job
    /// named `title` (ADR-111). Answered `Printed`; elsewhere an `Error`.
    Print {
        printer: String,
        devmode: Vec<u8>,
        title: String,
    },
    /// Stop what runs now: a search notices at its next page. Answered,
    /// like any request, by an `Error`.
    Cancel,
}

/// A new document made of a PDF's pages, for printing (ADR-111).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Compose {
    /// These pages (from 0), in this order, one listed twice written
    /// twice; `None` a blank page the size of the page before it (of the
    /// document's first, first). Each drawn at `scale` of its size about
    /// its middle, on a page of its own size (1 as it is): the print
    /// dialog's scale.
    Pages { pages: Vec<Option<u32>>, scale: f32 },
    /// A booklet: `sheets` in the order they print (`print::booklet::sheets`),
    /// each its front then its back, `per_side` pages to a side in its grid
    /// (each upside down where its cell says), on sheets `width` × `height`
    /// points (as `Paper::sheet_of` turns them); each page fitted in its
    /// place within `edge` of the sheet's edges and `fold` of each fold;
    /// `trim` fits the pages' ink rather than their whole page (one box for
    /// every page, so they scale alike); `turn_backs` turns each back half a
    /// turn, for a printer turning the sheet over the edge the backs were
    /// not laid for. Four or eight to a side, the folds are marked in the
    /// front's margins.
    Booklet {
        sheets: Vec<crate::print::booklet::Sheet>,
        #[serde(default)]
        per_side: crate::print::booklet::PerSide,
        width: f32,
        height: f32,
        fold: f32,
        edge: f32,
        trim: bool,
        turn_backs: bool,
    },
    /// Its pages, then those of the documents at these paths, in their
    /// order: several files printed as one (ADR-111). The worker reads
    /// them where it is told it may (`Setup`'s readable files).
    Append(Vec<std::path::PathBuf>),
}

/// What a worker writes a composed document as, in its folder.
pub const COMPOSED_FILE: &str = "composed.pdf";

#[derive(Debug, Serialize, Deserialize)]
pub enum WorkerReply {
    Ready(DocumentInfo),
    Page {
        content: PageContent,
        width: u32,
        height: u32,
        /// What a helper that says nothing writes is RGBA.
        #[serde(default)]
        order: PixelOrder,
        /// The part drawn, exactly, where one was asked; its `content` may
        /// then leave out the page's text, which the whole carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Option<Part>,
    },
    Matches(Vec<SearchHit>),
    Located(Option<u32>),
    /// `Compose` written, of this many pages.
    Composed {
        pages: u32,
    },
    /// `Print` sent to the printer, this many pages.
    Printed {
        pages: u32,
    },
    Unavailable(String),
    Error(String),
}

/// The longest side a page is rendered at. The reader asks for the side a
/// page covers on screen (`reader::page_side`), up to this.
pub const MAX_SIDE: u32 = 4096;
/// The most pixels a page drawn whole holds: an A4 page's at `MAX_SIDE`
/// (11.9 megapixels, 47 MB), counted rather than a side, so a tall page
/// is held as long as an A4 one is wide (`held_side`, ADR-147).
pub const PAGE_PIXELS: u64 = (MAX_SIDE as u64) * (MAX_SIDE as u64) * 1000 / 1414;
/// The most pixels a page's drawing may be, whole or a part of one: a
/// square page's at `MAX_SIDE`. The manager takes no more, and an engine
/// refuses no part within it (`Part::drawn`): the reader asks at most
/// `PAGE_PIXELS` (`Part::most_pixels`), so rounding never meets it.
pub const MAX_PAGE_PIXELS: u64 = (MAX_SIDE as u64) * (MAX_SIDE as u64);
/// The longest side a page is drawn at for a part of it: past it, a zoom
/// enlarges what is drawn (about 30 times an A4 page at 96 per inch).
pub const MAX_PART_SIDE: u32 = 32_768;
/// Bytes of one reply line, its newline included.
pub const MAX_REPLY: usize = 32 * 1024 * 1024;
/// Bytes of one request line, its newline included: a document composed
/// of tens of thousands of pages lists them all.
pub const MAX_REQUEST: usize = 1024 * 1024;
pub const MAX_HITS: usize = 500;
/// Characters of selectable text on one page. A page with more is shown
/// without selectable text; its pixels are not lost for it.
pub const MAX_PAGE_CHARS: usize = 200_000;
pub const PIXELS_FILE: &str = "page.rgba";
/// The argument that starts a worker for its first page alone (a
/// thumbnail): it reads how many pages there are, never the outline nor
/// the pages' labels, which a PDF's Poppler reads by loading every page.
pub const FIRST_PAGE: &str = "--first-page";

pub mod words;

/// The worker's side of the protocol, the same for every engine: a worker
/// greets once, then `serve` answers each request in order.
pub mod worker {
    use std::io::{BufReader, Read, Write};

    use super::{MAX_REPLY, MAX_REQUEST, Tagged, WorkerReply, WorkerRequest};

    /// Reply 0: `Ready`, or `Unavailable` when the engine's library is missing.
    pub fn greet(reply: &WorkerReply) -> anyhow::Result<()> {
        send(&mut std::io::stdout().lock(), 0, reply)
    }

    /// Report the error that ended the worker, before or instead of its
    /// greeting. A manager that already had one sees a broken protocol and
    /// starts another worker.
    pub fn fail(error: &anyhow::Error) {
        let reply = WorkerReply::Error(format!("{error:#}"));
        let _ = send(&mut std::io::stdout().lock(), 0, &reply);
    }

    /// Answer requests from standard input until the manager closes it.
    /// `handle` gets each request with a test of whether a newer line is
    /// already waiting: a long request (a search) checks it between pages and
    /// gives up, since its answer would only be drained. `Cancel` is answered
    /// here. A failed request, or a reply too large for the protocol, is that
    /// request's `Error`; the worker goes on.
    pub fn serve(
        handle: impl FnMut(WorkerRequest, &dyn Fn() -> bool) -> anyhow::Result<WorkerReply>,
    ) -> anyhow::Result<()> {
        serve_on(std::io::stdin(), &mut std::io::stdout(), handle)
    }

    fn serve_on(
        input: impl Read + Send + 'static,
        output: &mut impl Write,
        mut handle: impl FnMut(WorkerRequest, &dyn Fn() -> bool) -> anyhow::Result<WorkerReply>,
    ) -> anyhow::Result<()> {
        let (lines, requests) = crossbeam_channel::unbounded();
        // Requests are read ahead on a thread of their own, so the one
        // being handled can see that another is waiting.
        std::thread::Builder::new()
            .name("requests".into())
            .spawn(move || {
                let mut input = BufReader::new(input);
                while let Some(request) =
                    crate::jsonl::read::<Tagged<WorkerRequest>>(&mut input, MAX_REQUEST)
                {
                    let failed = request.is_err();
                    if lines.send(request).is_err() || failed {
                        break;
                    }
                }
            })?;
        while let Ok(request) = requests.recv() {
            let Tagged { id, body } = request?;
            let superseded = || !requests.is_empty();
            let reply = match body {
                WorkerRequest::Cancel => WorkerReply::Error("cancelled".into()),
                request => handle(request, &superseded)
                    .unwrap_or_else(|error| WorkerReply::Error(format!("{error:#}"))),
            };
            send(output, id, &reply)?;
        }
        Ok(())
    }

    /// One reply line, newline included, within `MAX_REPLY`.
    fn send(output: &mut impl Write, id: u64, reply: &WorkerReply) -> anyhow::Result<()> {
        let mut line = crate::jsonl::encode(&Tagged { id, body: reply })?;
        if line.len() > MAX_REPLY {
            line = crate::jsonl::encode(&Tagged {
                id,
                body: WorkerReply::Error("page content exceeds the reply limit".into()),
            })?;
        }
        output.write_all(&line)?;
        output.flush()?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn each_request_gets_one_tagged_reply_in_order() {
            let input = [
                r#"{"id":1,"body":{"Search":{"query":"needle"}}}"#,
                r#"{"id":2,"body":"Cancel"}"#,
                r#"{"id":3,"body":{"Search":{"query":"huge"}}}"#,
                r#"{"id":4,"body":{"Render":{"page":9,"side":64}}}"#,
                r#"{"id":5,"body":{"Search":{"query":"after"}}}"#,
            ]
            .map(|line| format!("{line}\n"))
            .concat();
            let mut output = Vec::new();
            serve_on(
                std::io::Cursor::new(input.into_bytes()),
                &mut output,
                |request, _| match request {
                    WorkerRequest::Search { query } if query == "huge" => {
                        Ok(WorkerReply::Error("x".repeat(MAX_REPLY)))
                    }
                    WorkerRequest::Search { .. } => Ok(WorkerReply::Matches(Vec::new())),
                    WorkerRequest::Locate { .. } => Ok(WorkerReply::Located(None)),
                    WorkerRequest::Render { page, .. } => anyhow::bail!("page {page} is broken"),
                    WorkerRequest::Compose(_) | WorkerRequest::Print { .. } => {
                        anyhow::bail!("writes no PDF")
                    }
                    WorkerRequest::Cancel => anyhow::bail!("answered by serve"),
                },
            )
            .unwrap();
            let replies: Vec<Tagged<WorkerReply>> = output
                .split(|b| *b == b'\n')
                .filter(|line| !line.is_empty())
                .map(|line| {
                    assert!(line.len() < MAX_REPLY);
                    serde_json::from_slice(line).unwrap()
                })
                .collect();
            let ids: Vec<u64> = replies.iter().map(|r| r.id).collect();
            assert_eq!(ids, [1, 2, 3, 4, 5]);
            let error = |i: usize| match &replies[i].body {
                WorkerReply::Error(message) => message.clone(),
                other => panic!("{other:?}"),
            };
            assert!(matches!(replies[0].body, WorkerReply::Matches(_)));
            assert_eq!(error(1), "cancelled");
            assert!(error(2).contains("exceeds the reply limit"), "oversized");
            assert_eq!(error(3), "page 9 is broken", "the worker goes on");
            assert!(matches!(replies[4].body, WorkerReply::Matches(_)));
        }

        #[test]
        fn a_newer_request_supersedes_a_long_one() {
            let input = concat!(
                r#"{"id":1,"body":{"Search":{"query":"long"}}}"#,
                "\n",
                r#"{"id":2,"body":"Cancel"}"#,
                "\n"
            );
            let mut output = Vec::new();
            let mut saw = Vec::new();
            serve_on(
                std::io::Cursor::new(input.as_bytes().to_vec()),
                &mut output,
                |_, superseded| {
                    // The reader thread gets to the second line soon.
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    while !superseded() && std::time::Instant::now() < deadline {
                        std::thread::yield_now();
                    }
                    saw.push(superseded());
                    anyhow::bail!("search superseded")
                },
            )
            .unwrap();
            assert_eq!(saw, [true]);
        }
    }
}

#[cfg(test)]
mod label_tests {
    use super::page_labels;

    fn labels(names: &[&str]) -> Vec<String> {
        page_labels(names.iter().map(|name| (*name).to_owned()))
    }

    #[test]
    fn labels_are_kept_only_where_a_page_is_named_otherwise() {
        assert_eq!(labels(&["i", "ii", "1", "2"]), ["i", "ii", "1", "2"]);
        assert_eq!(
            labels(&["", " 1 ", "2"]),
            ["", "1", "2"].map(String::from).to_vec()
        );
        // Each page its own number, or nothing named: nothing to show.
        assert!(labels(&["1", "2", "3"]).is_empty());
        assert!(labels(&["", "", ""]).is_empty());
        assert!(labels(&["1", "", "3"]).is_empty());
        // Bounded.
        let long = "x".repeat(200);
        assert_eq!(labels(&[&long])[0].chars().count(), 64);
    }
}

#[cfg(test)]
mod part_tests {
    use std::sync::Arc;

    use super::{DocumentPage, MAX_SIDE, PAGE_PIXELS, PageContent, Part, PixelOrder, held_side};

    /// A part in shares covers whole pixels round it, and says back the
    /// shares those pixels are exactly.
    #[test]
    fn a_part_lies_on_whole_pixels() {
        let part = Part {
            left: 0.25,
            top: 0.1,
            right: 0.6,
            bottom: 0.55,
        };
        assert_eq!(part.pixels(100, 50), [25, 5, 35, 23]);
        let back = Part::of_pixels(part.pixels(100, 50), 100, 50);
        assert_eq!(back.pixels(100, 50), [25, 5, 35, 23]);
        assert!(back.holds(&part), "what is drawn holds what was asked");
        // Never empty, never past the page.
        let thin = Part {
            left: 0.999,
            top: 1.,
            right: 1.,
            bottom: 1.2,
        };
        assert_eq!(thin.pixels(100, 50), [99, 49, 1, 1]);
    }

    /// A part cut from a page drawn whole is its pixels there.
    #[test]
    fn a_part_is_cut_from_the_whole() {
        let page = DocumentPage {
            width: 4,
            height: 3,
            pixels: (0..48).collect(),
            order: PixelOrder::Rgba,
            content: Arc::new(PageContent {
                page: 1,
                width: 4.,
                height: 3.,
                chars: Vec::new(),
                links: Vec::new(),
            }),
            part: None,
        };
        let cut = page.cut(Part {
            left: 0.5,
            top: 1. / 3.,
            right: 1.,
            bottom: 1.,
        });
        assert_eq!((cut.width, cut.height), (2, 2));
        // Rows 1 and 2, columns 2 and 3: bytes from 4 × (4 × row + column).
        assert_eq!(
            cut.pixels,
            [
                24, 25, 26, 27, 28, 29, 30, 31, 40, 41, 42, 43, 44, 45, 46, 47
            ]
        );
        assert_eq!(cut.part.unwrap().pixels(4, 3), [2, 1, 2, 2]);
    }

    /// A page drawn whole holds as many pixels as an A4 page at
    /// `MAX_SIDE`, counted: a tall page as long as an A4 page is wide.
    #[test]
    fn a_page_drawn_whole_is_bounded_by_its_pixels() {
        let a4 = (595., 842.);
        assert_eq!(
            held_side(a4.0, a4.1, 1000),
            1000,
            "as asked, below the bound"
        );
        let side = held_side(a4.0, a4.1, 100_000);
        assert!((MAX_SIDE - 8..=MAX_SIDE + 8).contains(&side), "{side}");
        // A webtoon's page, 800 × 12 000, whole at its own size.
        assert_eq!(held_side(800., 12_000., 12_000), 12_000);
        let tall = held_side(800., 12_000., 100_000);
        let pixels = u64::from(tall) * u64::from(tall) * 800 / 12_000;
        assert!(pixels <= PAGE_PIXELS, "{tall}");
        assert!(tall > 12_000);
        // A squarer page keeps `MAX_SIDE`, as before.
        assert_eq!(held_side(600., 800., 100_000), MAX_SIDE);
        assert_eq!(held_side(10., 10., 1), 64, "at least the least side");
    }

    /// What the reader counts a part by is never fewer pixels than an
    /// engine draws it at, however the engine lays the page's pixels
    /// (rounded as `PDFium` and `DjVuLibre` do, rounded up as Poppler and
    /// `MuPDF` do, a little smaller as djvu-rs may): a part the reader sizes
    /// to `PAGE_PIXELS` is drawn by every engine. Counted by its shares
    /// alone, a part sized to the bound was refused once its whole pixels
    /// rounded it out.
    #[test]
    fn a_part_is_counted_alike_by_the_reader_and_every_engine() {
        let pages: [(f32, f32); 4] = [
            (793.7, 1122.5),
            (816., 1056.),
            (1122.5, 793.7),
            (533., 8000.),
        ];
        let parts = [
            Part {
                left: 0.3,
                top: 0.4,
                right: 0.703,
                bottom: 0.578,
            },
            Part {
                left: 0.,
                top: 0.123_457,
                right: 1.,
                bottom: 0.3,
            },
            Part {
                left: 0.499_99,
                top: 0.5,
                right: 0.500_01,
                bottom: 0.5,
            },
        ];
        for own in pages {
            for side in [513, 4608, 8704, 17_000, 32_768] {
                let scale = f64::from(side) / f64::from(own.0.max(own.1));
                let laid = |length: f32| f64::from(length) * scale;
                let wholes = [
                    (laid(own.0).round(), laid(own.1).round()),
                    (laid(own.0).ceil(), laid(own.1).ceil()),
                    (laid(own.0).floor() - 1., laid(own.1).floor() - 1.),
                ];
                for part in parts {
                    let most = part.most_pixels(own, side);
                    for (width, height) in wholes {
                        let [_, _, w, h] = part.pixels(width as u32, height as u32);
                        assert!(
                            u64::from(w) * u64::from(h) <= most,
                            "{own:?} at {side}: {part:?} drawn {w}×{h}, counted {most}"
                        );
                    }
                }
            }
        }
        // A part counted at the reader's bound is drawn by every engine;
        // one past the manager's is refused.
        let side = 8704;
        let scale = f64::from(side) / 1122.5;
        let (width, height) = (
            (793.7 * scale).ceil() as u32,
            (1122.5 * scale).ceil() as u32,
        );
        let across = 0.5;
        let down = (PAGE_PIXELS as f64 / (f64::from(width) * f64::from(height) * across)) as f32;
        let at_bound = Part {
            left: 0.2,
            top: 0.1,
            right: 0.2 + across as f32,
            bottom: 0.1 + down,
        };
        let [_, _, w, h] = at_bound.pixels(width, height);
        assert!(
            u64::from(w) * u64::from(h) > PAGE_PIXELS,
            "rounded out past the bound"
        );
        assert!(at_bound.drawn(width, height).is_ok());
        let whole = Part {
            left: 0.,
            top: 0.,
            right: 1.,
            bottom: 1.,
        };
        assert!(whole.drawn(MAX_SIDE, MAX_SIDE).is_ok());
        assert!(whole.drawn(MAX_SIDE + 1, MAX_SIDE).is_err());
    }
}

#[cfg(test)]
mod turn_tests {
    use super::Rect;

    fn near(a: Rect, b: Rect) -> bool {
        [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height]
            .iter()
            .all(|d| d.abs() < 1e-6)
    }

    #[test]
    fn a_box_follows_its_page_turned_counterclockwise() {
        // The top right corner of a page as stored.
        let corner = Rect {
            x: 0.8,
            y: 0.,
            width: 0.2,
            height: 0.1,
        };
        assert!(near(corner.turned(0), corner));
        // A quarter turn counterclockwise takes it to the top left, its
        // sides swapped.
        let quarter = Rect {
            x: 0.,
            y: 0.,
            width: 0.1,
            height: 0.2,
        };
        assert!(near(corner.turned(1), quarter), "{:?}", corner.turned(1));
        let half = Rect {
            x: 0.,
            y: 0.9,
            width: 0.2,
            height: 0.1,
        };
        assert!(near(corner.turned(2), half), "{:?}", corner.turned(2));
        let three = Rect {
            x: 0.9,
            y: 0.8,
            width: 0.1,
            height: 0.2,
        };
        assert!(near(corner.turned(3), three), "{:?}", corner.turned(3));
        assert!(near(corner.turned(4), corner));
    }
}
