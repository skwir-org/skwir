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
}

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
    /// A single owned buffer transfers from the manager to the UI worker.
    /// Avoid an Arc conversion and another full-page copy for channel swizzling.
    pub rgba: Vec<u8>,
    pub content: Arc<PageContent>,
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
    fn search(&self, query: &str, cancel: &CancelToken) -> anyhow::Result<Vec<SearchHit>>;
    /// The page nearest `near` holding `text`, for a book laid out again:
    /// the page where what was read now lies. `None` when no page near
    /// enough holds it, or the document's pages never move (a PDF).
    fn locate(&self, text: &str, near: u32, cancel: &CancelToken) -> anyhow::Result<Option<u32>> {
        let _ = (text, near, cancel);
        Ok(None)
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
    /// Stop what runs now: a search notices at its next page. Answered,
    /// like any request, by an `Error`.
    Cancel,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WorkerReply {
    Ready(DocumentInfo),
    Page {
        content: PageContent,
        width: u32,
        height: u32,
    },
    Matches(Vec<SearchHit>),
    Located(Option<u32>),
    Unavailable(String),
    Error(String),
}

/// The longest side a page is rendered at. The reader asks for the side a
/// page covers on screen (`reader::page_side`), up to this.
pub const MAX_SIDE: u32 = 4096;
/// Bytes of one reply line, its newline included.
pub const MAX_REPLY: usize = 32 * 1024 * 1024;
/// Bytes of one request line, its newline included.
pub const MAX_REQUEST: usize = 16 * 1024;
pub const MAX_HITS: usize = 500;
/// Characters of selectable text on one page. A page with more is shown
/// without selectable text; its pixels are not lost for it.
pub const MAX_PAGE_CHARS: usize = 200_000;
pub const PIXELS_FILE: &str = "page.rgba";

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
