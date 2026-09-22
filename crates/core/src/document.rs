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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageContent {
    pub page: u32,
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
    Unavailable(String),
    Error(String),
}

pub const MAX_SIDE: u32 = 2048;
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
    use std::io::{BufRead, BufReader, Read, Write};

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
                loop {
                    let mut line = Vec::new();
                    let read = Read::by_ref(&mut input)
                        .take(MAX_REQUEST as u64)
                        .read_until(b'\n', &mut line);
                    if matches!(read, Ok(0)) {
                        break;
                    }
                    let request = (|| -> anyhow::Result<Tagged<WorkerRequest>> {
                        read?;
                        anyhow::ensure!(line.last() == Some(&b'\n'), "request exceeds limit");
                        Ok(serde_json::from_slice(&line)?)
                    })();
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
        let mut line = serde_json::to_vec(&Tagged { id, body: reply })?;
        if line.len() >= MAX_REPLY {
            line = serde_json::to_vec(&Tagged {
                id,
                body: WorkerReply::Error("page content exceeds the reply limit".into()),
            })?;
        }
        line.push(b'\n');
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
