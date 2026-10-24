//! A file's bytes read where they are shown (ADR-051), not a page at a
//! time: rows of `HEX_ROW` bytes, every one of them in the view.
//!
//! A row is always `HEX_ROW` bytes, so row `r` starts at `r × HEX_ROW`:
//! no index is needed, the file's length (read with the inspection,
//! `Display::bytes`) says how many rows there are. The rows are still the
//! bytes view's own (`PreviewProvider::shows_bytes`, a plugin from outside
//! too): the view asks it for the page at the offset of each chunk it shows,
//! nearest first, one at a time, as the text view asks for its chunks
//! (`text_view`), and a page holds as many rows as its first answer did.
//! Find asks the same provider to search the file from a byte, and goes to
//! the row of the match; Jump to offset goes to its row.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::Context;
use skwir_core::inspection::{InspectRequest, Inspection, Row};
use skwir_core::preview::PreviewRequest;
use skwir_core::scheduler::Priority;
use skwir_platform::inspection::HEX_ROW;

use crate::elements::rows::Scroll;
use crate::latest::Latest;
use crate::shell::Shell;
use crate::text_view::{around, let_go, unread};

const ROW: u64 = HEX_ROW as u64;

struct Reply {
    chunk: u64,
    rows: anyhow::Result<Vec<Row>>,
}

/// What a search answered: where it landed (the match's exact byte, or
/// where it started when there was none), and what the bytes view said.
struct Found {
    query: String,
    answer: anyhow::Result<(u64, String)>,
}

pub struct BytesView {
    /// How the bytes were asked for, which each page is asked with.
    request: PreviewRequest,
    view: InspectRequest,
    rows: u64,
    /// Rows a page holds.
    per: u64,
    chunks: HashMap<u64, Rc<Vec<Row>>>,
    wanted: Vec<u64>,
    laid: Range<u64>,
    asked: Latest<Reply>,
    /// The chunk asked for, while it is.
    asking: Option<u64>,
    /// The pages a read failed for: not asked again (`text_view::unread`).
    failed: HashSet<u64>,
    pub(crate) scroll: Scroll,
    found: Latest<Found>,
    /// The last match: a search for the same goes on after it.
    last: Option<(String, u64)>,
}

impl BytesView {
    /// The view of the `len` bytes the inspection `data` began to show,
    /// asked with `origin`, its pages asked of `provider`, the bytes view.
    /// Its first page is kept when it is the first chunk. `scroll` is the
    /// place to keep, when it is the file read again.
    fn new(
        data: &Inspection,
        origin: (PreviewRequest, InspectRequest),
        provider: &str,
        len: u64,
        scroll: Option<Scroll>,
    ) -> Option<Self> {
        let (request, mut view) = origin;
        // Asked of the bytes view by name: chosen again for each page, the
        // providers before it would try the file first.
        view.provider = Some(provider.to_owned());
        let per = data.rows.len() as u64;
        if per == 0 {
            return None;
        }
        let first = view.offset == 0 && view.query.is_none();
        view.offset = 0;
        view.query = None;
        let mut chunks = HashMap::new();
        if first {
            chunks.insert(0, Rc::new(data.rows.clone()));
        }
        Some(BytesView {
            request,
            view,
            rows: len.div_ceil(ROW),
            per,
            chunks,
            wanted: Vec::new(),
            laid: 0..0,
            asked: Latest::default(),
            asking: None,
            failed: HashSet::new(),
            scroll: scroll.unwrap_or_default(),
            found: Latest::default(),
            last: None,
        })
    }

    /// Row `row`, if its page is read.
    pub(crate) fn row(&self, row: usize) -> Option<&Row> {
        let row = row as u64;
        self.chunks
            .get(&(row / self.per))?
            .get((row % self.per) as usize)
    }

    fn laid(&mut self, laid: Range<u64>) -> bool {
        if laid == self.laid {
            return false;
        }
        self.laid = laid.clone();
        let (wanted, middle) = around(&laid, self.rows, self.per);
        self.wanted = wanted;
        let_go(&mut self.chunks, &self.wanted, middle);
        true
    }

    /// Ask for the first page wanted that is not read, one at a time.
    fn request(&mut self, app: &Arc<skwir_core::App>) {
        // One the view moved away from is let go for what it shows now.
        if self
            .asking
            .is_some_and(|chunk| !self.wanted.contains(&chunk))
        {
            self.asked.forget();
            self.asking = None;
        }
        if self.asked.is_pending() {
            return;
        }
        let Some(chunk) = self
            .wanted
            .iter()
            .copied()
            .find(|chunk| unread(*chunk, &self.chunks, &self.failed))
        else {
            return;
        };
        let previews = Arc::clone(&app.previews);
        let request = self.request.clone();
        let view = InspectRequest {
            offset: chunk * self.per * ROW,
            ..self.view.clone()
        };
        self.asking = Some(chunk);
        self.asked.ask(
            app,
            skwir_core::fs::reach::lane(&self.request.entry.path),
            app.lifetime(),
            Priority::Visible,
            "bytes-read",
            move |cancel| Reply {
                chunk,
                rows: previews
                    .inspect(&request, &view, cancel)
                    .and_then(|page| page.ok_or_else(|| anyhow::anyhow!("no bytes view")))
                    .map(|page| page.rows),
            },
        );
    }

    fn take(&mut self) -> bool {
        let Some(Reply { chunk, rows }) = self.asked.take() else {
            return false;
        };
        self.asking = None;
        match rows {
            Ok(rows) => {
                self.chunks.insert(chunk, Rc::new(rows));
            }
            Err(error) => {
                tracing::debug!(%error, chunk, "reading a page of bytes");
                self.failed.insert(chunk);
            }
        }
        true
    }

    /// Search the file for `query` from byte `from`, through the bytes
    /// view, which says where it found it.
    fn find(&mut self, app: &Arc<skwir_core::App>, query: String, from: u64) {
        let previews = Arc::clone(&app.previews);
        let request = self.request.clone();
        let view = InspectRequest {
            offset: from,
            query: Some(query.clone()),
            ..self.view.clone()
        };
        self.found.ask(
            app,
            skwir_core::fs::reach::lane(&self.request.entry.path),
            app.lifetime(),
            Priority::Visible,
            "bytes-find",
            move |cancel| Found {
                query,
                answer: previews
                    .inspect(&request, &view, cancel)
                    .and_then(|page| page.ok_or_else(|| anyhow::anyhow!("no bytes view")))
                    .map(|page| (page.position, page.notice)),
            },
        );
    }
}

/// The rows of `view`, those of a page read drawn as the byte view draws a
/// row (`inspector::hex_row`), the others their offset alone until it is.
pub(crate) fn element(
    view: &BytesView,
    theme: crate::theme::Theme,
    font: gpui::Font,
    selected: Option<usize>,
    pane: skwir_core::workspace::PaneId,
    entity: gpui::WeakEntity<Shell>,
    a11y: bool,
) -> gpui::AnyElement {
    use gpui::IntoElement;
    let (chunks, per) = (view.chunks.clone(), view.per);
    let laid = entity.clone();
    crate::elements::rows::rows(
        "bytes-rows",
        Rc::clone(&view.scroll),
        view.rows,
        true,
        move |row, _, _, _| {
            let read = chunks
                .get(&(row / per))
                .and_then(|chunk| chunk.get((row % per) as usize));
            let waiting;
            let shown = if let Some(read) = read {
                read
            } else {
                waiting = Row {
                    cells: vec![format!("{:016X}", row * ROW), String::new(), String::new()],
                    ..Row::default()
                };
                &waiting
            };
            let index = row as usize;
            let chosen = selected == Some(index);
            crate::inspector::said_hex(
                crate::inspector::hex_row(
                    shown,
                    index,
                    chosen,
                    &font,
                    &theme,
                    pane,
                    entity.clone(),
                ),
                shown,
                chosen,
                a11y,
            )
        },
    )
    .on_laid(move |range, _, cx| {
        if let Some(shell) = laid.upgrade() {
            shell.update(cx, |shell, cx| shell.bytes_laid(range, cx));
        }
    })
    .into_any_element()
}

/// The offset `typed` names: decimal, or hexadecimal after `0x`.
pub(crate) fn offset(typed: &str) -> Option<u64> {
    let typed = typed.trim();
    match typed
        .strip_prefix("0x")
        .or_else(|| typed.strip_prefix("0X"))
    {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => typed.parse().ok(),
    }
}

impl Shell {
    /// An inspection of bytes arrived: the view of the whole file, when its
    /// length is known. The same file read again keeps its place.
    pub(crate) fn start_bytes(
        &mut self,
        content: Option<&crate::preview::PreviewContent>,
        refresh: bool,
    ) {
        let kept = self
            .preview
            .inspection
            .bytes
            .take()
            .filter(|_| refresh)
            .map(|view| Rc::clone(&view.scroll));
        let Some(crate::preview::PreviewContent::Inspection(display)) = content else {
            return;
        };
        let (Some(len), Some(origin), Some(provider)) = (
            display.bytes,
            display.origin.as_deref().cloned(),
            self.app.previews.bytes_view(),
        ) else {
            return;
        };
        self.preview.inspection.bytes = BytesView::new(&display.data, origin, provider, len, kept);
    }

    /// The rows just laid out: read the pages around them.
    pub(crate) fn bytes_laid(&mut self, laid: Range<u64>, cx: &mut Context<Self>) {
        let app = Arc::clone(&self.app);
        let Some(view) = self.preview.inspection.bytes.as_mut() else {
            return;
        };
        if view.laid(laid) {
            view.request(&app);
            cx.notify();
        }
    }

    /// Take a page or a match that arrived, as the core wakes the UI.
    pub(crate) fn sync_bytes(&mut self, cx: &mut Context<Self>) {
        let app = Arc::clone(&self.app);
        let state = &mut self.preview.inspection;
        let Some(view) = state.bytes.as_mut() else {
            return;
        };
        let mut changed = view.take();
        view.request(&app);
        if let Some(Found { query, answer }) = view.found.take() {
            changed = true;
            match answer {
                Ok((byte, said)) => {
                    view.last = Some((query, byte));
                    view.scroll.borrow_mut().reveal(byte / ROW);
                    state.selected = Some((byte / ROW) as usize);
                    // "Match at 0x…", or that there is none further.
                    if !said.is_empty() {
                        self.set_status(said, false, cx);
                    }
                }
                Err(error) => self.set_status(format!("{error:#}"), true, cx),
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// `ui.inspect` actions on the bytes of a file read where they are
    /// shown: `true` when it was one, done.
    pub(crate) fn bytes_action(
        &mut self,
        action: &str,
        args: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let app = Arc::clone(&self.app);
        let state = &mut self.preview.inspection;
        let selected = state.selected;
        let Some(view) = state.bytes.as_mut() else {
            return false;
        };
        match action {
            "find" if args["back"].as_bool() == Some(true) => {
                self.set_status("The hex view searches forward only", false, cx);
            }
            "find" => {
                let query = args["value"].as_str().unwrap_or("").to_owned();
                // The same again goes on after its match; another starts at
                // the row selected, or the top.
                let from = match &view.last {
                    Some((last, byte)) if *last == query => byte + 1,
                    _ => selected.map_or(view.scroll.borrow().top, |row| row as u64) * ROW,
                };
                view.find(&app, query, from);
            }
            "jump" => {
                let Some(byte) = args["value"].as_str().and_then(offset) else {
                    self.set_status("Enter a decimal or hexadecimal offset", true, cx);
                    return true;
                };
                let row = (byte / ROW).min(view.rows.saturating_sub(1));
                view.scroll.borrow_mut().reveal(row);
                state.selected = Some(row as usize);
                cx.notify();
            }
            "copy" => {
                // The row selected, else the rows on screen.
                let rows: Vec<usize> = if let Some(row) = selected {
                    vec![row]
                } else {
                    let shown = view.scroll.borrow().shown();
                    (shown.start as usize..shown.end as usize).collect()
                };
                let text = rows
                    .into_iter()
                    .filter_map(|row| view.row(row))
                    .map(|row| row.cells.join("\t"))
                    .collect::<Vec<_>>()
                    .join("\n");
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offset_is_decimal_or_hexadecimal() {
        assert_eq!(offset("4096"), Some(4096));
        assert_eq!(offset(" 0x1F0 "), Some(0x1F0));
        assert_eq!(offset("0XFF"), Some(255));
        assert_eq!(offset("1F0"), None, "hexadecimal says so");
        assert_eq!(offset(""), None);
    }
}
