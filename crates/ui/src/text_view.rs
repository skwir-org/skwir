//! A text file shown whole, read where it is shown (ADR-051).
//!
//! The provider names the file (`Inspection::text`) and its first rows. A
//! worker counts its rows into an index (`text_rows::count`) while the
//! view draws; the rows it lays out (`elements::rows`) are read a chunk of
//! `STRIDE` at a time around them, nearest first, one chunk on its way at a
//! time, as a document's pictures are (`inspector_pictures`). A chunk is
//! read plain first, which takes a few milliseconds whatever its grammar
//! or its lines, then colored: from where the one before it stopped, or
//! fresh after a jump, and again once the one before it has been.
//! Chunks far from the view go past `KEPT`. Find, copy and a line a link
//! asks for (`file:12`) read the file on a worker too. The paths the text
//! writes are found as a chunk is colored, never before its text shows
//! (`links::WrittenPaths`: each a lookup on the disk).
//!
//! A remote's file the preview does not hold whole (ADR-101) is counted as
//! far as it is shown and a little past (`text_rows::count_until`), its
//! rows beyond estimated from its length: a log of a gigabyte on a server
//! is not downloaded whole to size a scrollbar. Scrolling on counts on; a
//! find, a line asked for and a copy count to the end first.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::Context;
use parking_lot::{Mutex, RwLock};
use skwir_core::cancel::CancelToken;
use skwir_core::inspection::{Block, TextFile};
use skwir_core::scheduler::Priority;
use skwir_platform::links::WrittenPaths;
use skwir_platform::text_rows::{self, Form, Index, Point, STRIDE};

use crate::code_preview::{Highlighter, Palette, Resume};
use crate::elements::rows::{Scroll, Seen};
use crate::latest::Latest;
use crate::shell::Shell;

/// Chunks kept around the view: a few screens either way.
const KEPT: usize = 24;
/// The most Copy reads from the file.
pub const COPY_BYTES: usize = 16 * 1024 * 1024;
/// How often counting shows how far it got.
const TELL: Duration = Duration::from_millis(100);

/// Rows `chunk × STRIDE` on, read and their links found, colored or not
/// yet.
#[derive(Clone)]
pub(crate) struct Chunk {
    pub rows: Vec<Block>,
    /// Each row's line (from 0), and whether the row begins it.
    pub lines: Vec<(u64, bool)>,
    colored: bool,
    /// Colored from where the chunk before stopped (or the file's start),
    /// not from a fresh start after a jump.
    exact: bool,
}

/// The first rows, read plain with the inspection on its worker
/// (`inspector::Display::new`), and what colors them.
pub(crate) struct Start {
    pub form: Form,
    pub chunk: Chunk,
    pub palette: Arc<Palette>,
}

/// How a chunk is read: plain, or colored from where the chunk before
/// stopped (or fresh) in a palette.
enum Reading<'a> {
    Plain,
    Colored(Option<Resume>, &'a Palette),
}

/// Read chunk `chunk`, which starts at `point`, plain or colored. One row
/// more is read to say whether the last ends its line. Where the colors
/// stopped, when they were applied.
fn read_chunk(
    file: &TextFile,
    form: Form,
    (chunk, point): (u64, Point),
    reading: Reading<'_>,
    paths: Option<&Mutex<WrittenPaths>>,
    cancel: &CancelToken,
) -> anyhow::Result<(Chunk, Option<Resume>)> {
    let at = chunk * STRIDE;
    let mut rows = text_rows::read(file, form, (at, point), at..at + STRIDE + 1, cancel)?;
    let after = (rows.len() as u64 > STRIDE)
        .then(|| rows.pop())
        .flatten()
        .map(|row| row.first);
    let ends: Vec<bool> = (0..rows.len())
        .map(|n| {
            rows.get(n + 1)
                .map_or(after.unwrap_or(true), |next| next.first)
        })
        .collect();
    let (mut colors, exact) = match reading {
        Reading::Plain => (None, false),
        Reading::Colored(from, palette) => {
            let exact = chunk == 0 || from.is_some();
            let colors = Highlighter::resume(file.language.as_deref(), palette, from);
            (Some(colors), exact)
        }
    };
    let mut blocks = Vec::with_capacity(rows.len());
    let mut lines = Vec::with_capacity(rows.len());
    for (n, row) in rows.into_iter().enumerate() {
        if n % 64 == 0 {
            skwir_platform::inspection::check(cancel)?;
        }
        let mut block = Block {
            runs: colors
                .as_mut()
                .map(|colors| colors.row(&row.text, ends[n]))
                .unwrap_or_default(),
            text: row.text,
            code: true,
            language: file.language.clone(),
            ..Block::default()
        };
        skwir_platform::links::link_block(&mut block, true);
        blocks.push(block);
        lines.push((row.line, row.first));
    }
    // Colored rows are read after the plain ones showed: the paths they
    // write are looked up now.
    if let (Some(paths), true) = (paths, colors.is_some()) {
        paths.lock().link(&mut blocks);
        skwir_platform::inspection::check(cancel)?;
    }
    Ok((
        Chunk {
            rows: blocks,
            lines,
            colored: colors.is_some(),
            exact,
        },
        colors.map(Highlighter::stop),
    ))
}

/// The first rows of `file`, plain, for the inspection's worker: shown at
/// once, colored by the view.
pub(crate) fn start(file: &TextFile, palette: Palette) -> anyhow::Result<Start> {
    let cancel = CancelToken::new();
    let form = Form::read(file, &cancel)?;
    let (chunk, _) = read_chunk(
        file,
        form,
        (0, text_rows::origin(form)),
        Reading::Plain,
        None,
        &cancel,
    )?;
    Ok(Start {
        form,
        chunk,
        palette: Arc::new(palette),
    })
}

struct Reply {
    chunk: u64,
    read: anyhow::Result<(Chunk, Option<Resume>)>,
}

/// What a search or a line asked for found: the match and its row, the
/// row to show, or what to say instead.
pub(crate) enum Found {
    Match(u64, Range<usize>),
    Row(u64),
    /// What a search came to instead, said in its field: no match, or a
    /// failure.
    Missed {
        said: String,
        error: bool,
    },
    /// What a line a link names came to instead.
    Nothing(String),
}

pub(crate) struct Copied {
    text: String,
    cut: bool,
}

pub struct TextView {
    pub(crate) file: TextFile,
    index: Arc<RwLock<Index>>,
    counting: CancelToken,
    /// One count at a time on the index: the view's, and a find's, a
    /// line's or a copy's that counts to the end first.
    gate: Arc<Mutex<()>>,
    /// How a remote's file not held whole is counted, as far as it is
    /// shown; `None` for any other, counted whole at once.
    paced: Option<Paced>,
    palette: Arc<Palette>,
    /// What finds the paths the text writes, kept for the file: each word
    /// is looked up once. None for a member written out of its container.
    paths: Option<Arc<Mutex<WrittenPaths>>>,
    chunks: HashMap<u64, Rc<Chunk>>,
    /// Where a chunk's colors start: where the one before stopped.
    starts: HashMap<u64, Resume>,
    /// The chunks to have, nearest the view first.
    wanted: Vec<u64>,
    laid: Range<u64>,
    asked: Latest<Reply>,
    /// The chunk asked for, while it is.
    asking: Option<u64>,
    /// The chunks a read failed for (`unread`).
    failed: HashSet<u64>,
    pub(crate) scroll: Scroll,
    /// Where the match a find landed on was drawn, for the rows to bring
    /// it into view (`RowScroll::reveal_spot`).
    pub(crate) spot: Seen,
    found: Latest<Found>,
    copied: Latest<anyhow::Result<Copied>>,
    /// A line (from 1) a link asked for, until it is counted.
    line: Option<u64>,
    /// The rows and lines of the first chunk, before any is counted.
    first: (u64, u64),
    /// The rows known at the last wake.
    seen: u64,
}

/// A count that goes as far as the view needs (`TextView::count_more`).
struct Paced {
    /// The file's length, which the rows not yet counted are estimated by.
    len: u64,
    /// Whether a count is on its way.
    going: Arc<std::sync::atomic::AtomicBool>,
    /// Whether a count failed: none is asked again while the view lasts,
    /// as a chunk's read is not (`unread`).
    failed: Arc<std::sync::atomic::AtomicBool>,
}

/// How far a remote's file is counted at first: a few screens.
const PACED_FIRST: u64 = 4 * STRIDE;

/// The chunks of `per` rows to have for rows `laid` of `rows`: those laid
/// out, nearest their middle first, then the one after and the one before.
/// And their middle, to tell what is far.
pub(crate) fn around(laid: &Range<u64>, rows: u64, per: u64) -> (Vec<u64>, u64) {
    let last = rows.saturating_sub(1) / per;
    let (first, end) = (laid.start / per, laid.end.saturating_sub(1) / per);
    let middle = u64::midpoint(first, end);
    let mut wanted: Vec<u64> = (first..=end.min(last)).collect();
    wanted.sort_by_key(|chunk| chunk.abs_diff(middle));
    wanted.extend((end < last).then_some(end + 1));
    wanted.extend(first.checked_sub(1));
    (wanted, middle)
}

/// Whether `chunk` is still to be read: not read, and no read of it
/// failed. A failed one is not asked again while the view lasts: asked at
/// once, it fails at once, and each answer wakes the window to draw, a
/// loop with nothing new (a file made unreadable while shown, a bad
/// sector). A change to the file replaces the view.
pub(crate) fn unread<T>(chunk: u64, chunks: &HashMap<u64, T>, failed: &HashSet<u64>) -> bool {
    !chunks.contains_key(&chunk) && !failed.contains(&chunk)
}

/// Past `KEPT`, the chunks farthest from `middle` that are not wanted go.
/// Which went.
pub(crate) fn let_go<T>(chunks: &mut HashMap<u64, T>, wanted: &[u64], middle: u64) -> Vec<u64> {
    if chunks.len() <= KEPT {
        return Vec::new();
    }
    let mut far: Vec<u64> = chunks
        .keys()
        .copied()
        .filter(|chunk| !wanted.contains(chunk))
        .collect();
    far.sort_by_key(|chunk| std::cmp::Reverse(chunk.abs_diff(middle)));
    far.truncate(chunks.len() - KEPT);
    for chunk in &far {
        chunks.remove(chunk);
    }
    far
}

impl Drop for TextView {
    fn drop(&mut self) {
        self.counting.cancel();
    }
}

impl TextView {
    /// The view of `file`, from its first rows, its rows counted from now.
    /// `scroll` is the place to keep, when it is the same file read again.
    /// `paths` finds the paths it writes, when they can be read from its
    /// folder.
    fn new(
        app: &Arc<skwir_core::App>,
        file: TextFile,
        start: Start,
        scroll: Option<Scroll>,
        paths: bool,
    ) -> Self {
        let Start {
            form,
            chunk,
            palette,
        } = start;
        let first = (
            chunk.rows.len() as u64,
            chunk.lines.iter().filter(|(_, first)| *first).count() as u64,
        );
        let index = Arc::new(RwLock::new(Index::new(form)));
        let counting = app.lifetime().child();
        // A remote's file not held whole is counted as far as it is shown.
        let paced = file
            .held
            .as_ref()
            .filter(|held| held.is_remote() && held.held_whole().is_none())
            .map(|held| Paced {
                len: held.len(),
                going: Arc::default(),
                failed: Arc::default(),
            });
        let paths = paths
            .then(|| WrittenPaths::new(&file.path))
            .flatten()
            .map(|paths| Arc::new(Mutex::new(paths)));
        let view = TextView {
            file,
            index,
            counting,
            gate: Arc::default(),
            paced,
            palette,
            paths,
            chunks: HashMap::from([(0, Rc::new(chunk))]),
            starts: HashMap::new(),
            wanted: Vec::new(),
            laid: 0..0,
            asked: Latest::default(),
            asking: None,
            failed: HashSet::new(),
            scroll: scroll.unwrap_or_default(),
            spot: Seen::default(),
            found: Latest::default(),
            copied: Latest::default(),
            line: None,
            first,
            seen: 0,
        };
        view.count_more(app, view.paced.as_ref().map(|_| PACED_FIRST));
        view
    }

    /// Count the rows on, to `until` (to the end with `None`), on a worker,
    /// one count at a time (`gate`). A paced view asks again as it needs
    /// more, never while a count is on its way or after one failed.
    fn count_more(&self, app: &Arc<skwir_core::App>, until: Option<u64>) {
        use std::sync::atomic::Ordering;
        if let Some(paced) = &self.paced
            && (paced.failed.load(Ordering::Relaxed) || paced.going.swap(true, Ordering::AcqRel))
        {
            return;
        }
        let (index, file, cancel) = (
            Arc::clone(&self.index),
            self.file.clone(),
            self.counting.clone(),
        );
        let gate = Arc::clone(&self.gate);
        let paced = self
            .paced
            .as_ref()
            .map(|paced| (Arc::clone(&paced.going), Arc::clone(&paced.failed)));
        let wake = app.waker_handle();
        app.scheduler.spawn_in(
            skwir_core::fs::reach::lane(&self.file.path),
            Priority::Normal,
            "text-rows-count",
            self.counting.clone(),
            move || {
                let mut told = Instant::now();
                let counted = {
                    let _gate = gate.lock();
                    text_rows::count_until(&file, &index, until, &cancel, || {
                        if told.elapsed() >= TELL {
                            told = Instant::now();
                            wake();
                        }
                    })
                };
                if let Err(error) = &counted
                    && !cancel.is_cancelled()
                {
                    tracing::debug!(%error, "counting a text's rows");
                }
                if let Some((going, failed)) = paced {
                    failed.fetch_or(counted.is_err(), Ordering::Relaxed);
                    going.store(false, Ordering::Release);
                }
                wake();
            },
        );
    }

    /// The rows known: counted, or the first chunk's before any is; for a
    /// paced view, those not counted yet estimated from how far the count
    /// reached into the file.
    pub(crate) fn rows(&self) -> u64 {
        let index = self.index.read();
        let counted = index.rows().max(self.first.0);
        match &self.paced {
            Some(paced) if !index.done() && index.reached() > 0 => {
                let per_byte = counted as f64 / index.reached() as f64;
                ((per_byte * paced.len as f64).ceil() as u64).max(counted)
            }
            _ => counted,
        }
    }

    /// The lines known, and whether all are counted.
    pub(crate) fn lines(&self) -> (u64, bool) {
        let index = self.index.read();
        let done = index.done() || self.file.whole;
        (index.lines().max(self.first.1), done)
    }

    /// The chunks read, for a frame's rows.
    pub(crate) fn chunks(&self) -> HashMap<u64, Rc<Chunk>> {
        self.chunks.clone()
    }

    /// Row `row`, if it is read.
    pub(crate) fn block(&self, row: usize) -> Option<&Block> {
        let row = row as u64;
        self.chunks
            .get(&(row / STRIDE))?
            .rows
            .get((row % STRIDE) as usize)
    }

    /// The text of rows `from..=to` (row, byte), when all are read: rows of
    /// a line joined, lines by a newline.
    fn text_read(&self, from: (usize, usize), to: (usize, usize)) -> Option<String> {
        let mut out = String::new();
        for row in from.0..=to.0 {
            let chunk = self.chunks.get(&(row as u64 / STRIDE))?;
            let n = row % STRIDE as usize;
            let (block, (_, first)) = (chunk.rows.get(n)?, chunk.lines.get(n)?);
            if *first && row > from.0 {
                out.push('\n');
            }
            let within = |mut at: usize| {
                at = at.min(block.text.len());
                while !block.text.is_char_boundary(at) {
                    at -= 1;
                }
                at
            };
            let end = if row == to.0 {
                within(to.1)
            } else {
                block.text.len()
            };
            let start = if row == from.0 {
                within(from.1).min(end)
            } else {
                0
            };
            out.push_str(&block.text[start..end]);
        }
        Some(out)
    }

    /// The rows just laid out: what to read is around them. Whether that
    /// changed.
    fn laid(&mut self, laid: Range<u64>) -> bool {
        if laid == self.laid {
            return false;
        }
        self.laid = laid.clone();
        let (wanted, middle) = around(&laid, self.rows(), STRIDE);
        self.wanted = wanted;
        for chunk in let_go(&mut self.chunks, &self.wanted, middle) {
            self.starts.remove(&(chunk + 1));
        }
        true
    }

    /// Ask for what the chunks wanted lack, nearest first, one at a time:
    /// one not read is read plain, then one not colored is colored, or
    /// colored again when it started fresh and the one before has since
    /// been colored.
    fn request(&mut self, app: &Arc<skwir_core::App>) {
        // A paced view counts on to just past the chunks it wants that the
        // count has not reached.
        if self.paced.is_some() {
            let index = self.index.read();
            let beyond = self
                .wanted
                .iter()
                .copied()
                .filter(|&chunk| chunk > 0 && index.point(chunk).is_none())
                .max();
            let more = beyond.filter(|_| !index.done());
            drop(index);
            if let Some(chunk) = more {
                self.count_more(app, Some((chunk + 2) * STRIDE));
            }
        }
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
        let next = {
            let index = self.index.read();
            let form = index.form;
            // Read once the count has reached it: its place is known.
            let place = |chunk: u64| {
                if chunk == 0 {
                    Some(text_rows::origin(form))
                } else {
                    index.point(chunk)
                }
            };
            let missing = self
                .wanted
                .iter()
                .copied()
                .filter(|chunk| unread(*chunk, &self.chunks, &self.failed))
                .find_map(|chunk| Some((chunk, place(chunk)?, false)));
            missing
                .or_else(|| {
                    self.wanted.iter().copied().find_map(|chunk| {
                        if self.failed.contains(&chunk) {
                            return None;
                        }
                        let read = self.chunks.get(&chunk)?;
                        let stale =
                            !read.colored || (!read.exact && self.starts.contains_key(&chunk));
                        stale.then(|| Some((chunk, place(chunk)?, true))).flatten()
                    })
                })
                .map(|(chunk, point, colored)| (chunk, point, colored, form))
        };
        let Some((chunk, point, colored, form)) = next else {
            return;
        };
        let from = self.starts.get(&chunk).cloned();
        let (file, palette) = (self.file.clone(), Arc::clone(&self.palette));
        let paths = self.paths.clone();
        self.asking = Some(chunk);
        self.asked.ask(
            app,
            skwir_core::fs::reach::lane(&self.file.path),
            &self.counting,
            Priority::Visible,
            "text-rows-read",
            move |cancel| {
                let reading = if colored {
                    Reading::Colored(from, &palette)
                } else {
                    Reading::Plain
                };
                Reply {
                    chunk,
                    read: read_chunk(
                        &file,
                        form,
                        (chunk, point),
                        reading,
                        paths.as_deref(),
                        cancel,
                    ),
                }
            },
        );
    }

    /// Take a chunk that arrived. Whether one did.
    fn take(&mut self) -> bool {
        let Some(Reply { chunk, read }) = self.asked.take() else {
            return false;
        };
        self.asking = None;
        match read {
            Ok((read, stop)) => {
                if let Some(stop) = stop.filter(|_| read.exact) {
                    self.starts.insert(chunk + 1, stop);
                }
                // Plain rows never replace colored ones.
                let colored = self.chunks.get(&chunk).is_some_and(|had| had.colored);
                if read.colored || !colored {
                    self.chunks.insert(chunk, Rc::new(read));
                }
            }
            Err(error) => {
                tracing::debug!(%error, chunk, "reading a text's rows");
                self.failed.insert(chunk);
            }
        }
        true
    }

    /// Look for `query` from `from` (a row and a byte in it), forward or
    /// `back`.
    fn find(&mut self, app: &Arc<skwir_core::App>, query: String, from: (u64, usize), back: bool) {
        let (file, index) = (self.file.clone(), Arc::clone(&self.index));
        let to_the_end = self.to_the_end();
        self.found.ask(
            app,
            skwir_core::fs::reach::lane(&file.path),
            &self.counting,
            Priority::Visible,
            "text-rows-find",
            move |cancel| {
                let found = to_the_end(&file, &index, cancel)
                    .and_then(|()| text_rows::find(&file, &index, &query, from, back, cancel));
                let (said, error) = match found {
                    Ok(Some((row, found))) => return Found::Match(row, found),
                    Ok(None) if index.read().done() => ("No match in the file".into(), false),
                    Ok(None) => ("No match in what is counted so far".into(), false),
                    Err(error) => (format!("{error:#}"), true),
                };
                Found::Missed { said, error }
            },
        );
    }

    /// Ask for the line a link wants, once it is counted.
    fn find_line(&mut self, app: &Arc<skwir_core::App>) {
        let Some(line) = self.line else {
            return;
        };
        // The index finds the line: what it counted, not the first rows a
        // file small enough to be whole showed before counting began.
        let (lines, done) = {
            let index = self.index.read();
            (index.lines(), index.done())
        };
        if lines < line && !done {
            // A paced view counts on: the line may be past where it is.
            if self.paced.is_some() {
                self.count_more(app, None);
            }
            return;
        }
        self.line = None;
        let (file, index) = (self.file.clone(), Arc::clone(&self.index));
        self.found.ask(
            app,
            skwir_core::fs::reach::lane(&file.path),
            &self.counting,
            Priority::Visible,
            "text-rows-line",
            move |cancel| match text_rows::row_of_line(&file, &index, line - 1, cancel) {
                Ok(Some(row)) => Found::Row(row),
                Ok(None) => Found::Nothing(format!("The file has no line {line}")),
                Err(error) => Found::Nothing(format!("{error:#}")),
            },
        );
    }

    /// Show line `line` (from 1) once it is counted, as a link to it asks.
    pub(crate) fn show_line(&mut self, line: u64) {
        self.line = Some(line.max(1));
    }

    fn copy(&mut self, app: &Arc<skwir_core::App>, from: (u64, usize), to: (u64, usize)) {
        let (file, index) = (self.file.clone(), Arc::clone(&self.index));
        let to_the_end = self.to_the_end();
        self.copied.ask(
            app,
            skwir_core::fs::reach::lane(&file.path),
            &self.counting,
            Priority::Visible,
            "text-rows-copy",
            move |cancel| {
                to_the_end(&file, &index, cancel)?;
                text_rows::text(&file, &index, from, to, COPY_BYTES, cancel)
                    .map(|(text, cut)| Copied { text, cut })
            },
        );
    }

    /// What a find or a copy does first, on its worker: a paced view's
    /// rows counted to the end, one count at a time; nothing for another.
    fn to_the_end(
        &self,
    ) -> impl Fn(&TextFile, &RwLock<Index>, &CancelToken) -> anyhow::Result<()> + Send + 'static
    {
        let gate = self.paced.as_ref().map(|_| Arc::clone(&self.gate));
        move |file, index, cancel| {
            let Some(gate) = &gate else {
                return Ok(());
            };
            if index.read().done() {
                return Ok(());
            }
            let _gate = gate.lock();
            text_rows::count_until(file, index, None, cancel, || {})
        }
    }
}

/// How the rows are drawn this frame: the code view's look.
pub(crate) struct Look {
    pub theme: crate::theme::Theme,
    pub style: crate::markdown::MarkdownStyle,
    pub mono: gpui::SharedString,
    pub font_size: f32,
    pub selected: Option<usize>,
    pub selection: Option<crate::inspector_text::Selection>,
    pub pane: skwir_core::workspace::PaneId,
    pub entity: gpui::WeakEntity<Shell>,
    /// Whether lines wrap to the width (`reading.wrap_lines`).
    pub wrap: bool,
    /// What the find field's popup hides of the rows, while it is open.
    pub covered: Option<crate::elements::rows::Covered>,
}

/// The rows of `view`, those read drawn as the code view draws a line
/// (`code_preview::CodeRow`), the others empty until they are.
pub(crate) fn element(view: &TextView, look: Look) -> gpui::AnyElement {
    use gpui::IntoElement;
    let chunks = view.chunks();
    let digits = crate::code_preview::digits(view.lines().0);
    let empty = Block {
        code: true,
        ..Block::default()
    };
    let Look {
        theme,
        style,
        mono,
        font_size,
        selected,
        selection,
        pane,
        entity,
        wrap,
        covered,
    } = look;
    let laid = entity.clone();
    let spot = Rc::clone(&view.spot);
    crate::elements::rows::rows(
        "text-rows",
        Rc::clone(&view.scroll),
        view.rows(),
        wrap,
        move |row, shift, _, _| {
            let n = (row % STRIDE) as usize;
            let read = chunks
                .get(&(row / STRIDE))
                .and_then(|chunk| Some((chunk.rows.get(n)?, chunk.lines.get(n)?)));
            let (block, number) = match read {
                Some((block, &(line, first))) => (block, first.then_some(line + 1)),
                None => (&empty, None),
            };
            crate::code_preview::CodeRow {
                block,
                index: row as usize,
                number,
                digits,
                fold: None,
                shift,
                selected: selected == Some(row as usize),
                selection,
                font_size,
                mono: &mono,
                theme: &theme,
                style: &style,
                pane,
                entity: entity.clone(),
            }
            .render()
        },
    )
    .on_laid(move |range, _, cx| {
        if let Some(shell) = laid.upgrade() {
            shell.update(cx, |shell, cx| shell.text_laid(range, cx));
        }
    })
    .seen(spot)
    .covered(covered)
    .into_any_element()
}

impl Shell {
    /// An inspection arrived: the view of the file it names, when it names
    /// one. The same file read again (it changed) keeps its place.
    pub(crate) fn start_text(
        &mut self,
        content: &mut Option<crate::preview::PreviewContent>,
        refresh: bool,
    ) {
        let kept = self
            .preview
            .inspection
            .text
            .take()
            .filter(|_| refresh)
            .map(|view| Rc::clone(&view.scroll));
        let Some(crate::preview::PreviewContent::Inspection(display)) = content else {
            return;
        };
        let (Some(file), Some(start)) = (display.data.text.clone(), display.text.take()) else {
            return;
        };
        // A member written out of its container has nothing of it beside
        // it on the disk.
        let paths = self
            .preview
            .nested
            .last()
            .is_none_or(|member| !member.extracted());
        let mut view = TextView::new(&self.app, file, *start, kept, paths);
        view.request(&self.app);
        self.preview.inspection.text = Some(view);
    }

    /// Ask for what the view lacks: a chunk around it, a line a link wants.
    pub(crate) fn ask_text(&mut self) {
        let app = Arc::clone(&self.app);
        if let Some(view) = self.preview.inspection.text.as_mut() {
            view.request(&app);
            view.find_line(&app);
        }
    }

    /// The rows just laid out (`elements::rows`), after layout: read what
    /// is missing around them. Never while painting.
    pub(crate) fn text_laid(&mut self, laid: Range<u64>, cx: &mut Context<Self>) {
        let app = Arc::clone(&self.app);
        let Some(view) = self.preview.inspection.text.as_mut() else {
            return;
        };
        if view.laid(laid) {
            view.request(&app);
            cx.notify();
        }
    }

    /// Take what the workers read, as the core wakes the UI: a chunk, how
    /// far counting got, a match, a copy.
    pub(crate) fn sync_text(&mut self, cx: &mut Context<Self>) {
        let app = Arc::clone(&self.app);
        let state = &mut self.preview.inspection;
        let Some(view) = state.text.as_mut() else {
            return;
        };
        // Counting moved on: what was not counted yet may be read now.
        let mut changed = view.take();
        view.request(&app);
        view.find_line(&app);
        if let Some(found) = view.found.take() {
            changed = true;
            match found {
                // The match in view, however far along its row.
                Found::Match(row, found) => {
                    view.scroll.borrow_mut().reveal_spot(row);
                    state.selected = Some(row as usize);
                    state.selection = None;
                    if let Some(sought) = state.sought.as_mut() {
                        sought.current = Some((row as usize, found));
                    }
                }
                Found::Row(row) => {
                    view.scroll.borrow_mut().reveal(row);
                    state.selected = Some(row as usize);
                    state.selection = None;
                }
                Found::Missed { said, error } => self.find_said(said, error, cx),
                Found::Nothing(said) => self.set_status(said, false, cx),
            }
        }
        let state = &mut self.preview.inspection;
        if let Some(view) = state.text.as_mut()
            && let Some(copied) = view.copied.take()
        {
            match copied {
                Ok(Copied { text, cut }) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    if cut {
                        self.set_status(
                            format!(
                                "Copied the first {}",
                                skwir_platform::limits::describe(COPY_BYTES)
                            ),
                            false,
                            cx,
                        );
                    }
                }
                Err(error) => self.set_status(format!("{error:#}"), true, cx),
            }
        }
        // The count grew: the scrollbar and the caption say so.
        if let Some(view) = self.preview.inspection.text.as_mut() {
            let rows = view.rows();
            changed |= std::mem::replace(&mut view.seen, rows) != rows;
        }
        if changed {
            cx.notify();
        }
    }

    /// `ui.inspect` actions on a text read where it is shown: `true` when
    /// it was one, done.
    pub(crate) fn text_action(
        &mut self,
        action: &str,
        args: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let app = Arc::clone(&self.app);
        let state = &mut self.preview.inspection;
        let selected = state.selected;
        let selection = state.selection;
        let Some(view) = state.text.as_mut() else {
            return false;
        };
        match action {
            "find" => {
                let query = args["value"].as_str().unwrap_or("").to_owned();
                let back = args["back"].as_bool() == Some(true);
                let top = view.scroll.borrow().top as usize;
                // From the match the find is on, else after the row
                // selected or before it, else from the top.
                let (row, at) = state.find_from(&query, back, (top, 0));
                if let Some(view) = state.text.as_mut() {
                    view.find(&app, query, (row as u64, at), back);
                }
                cx.notify();
            }
            "select_all" => {
                let last = view.rows().saturating_sub(1) as usize;
                state.selection = Some(crate::inspector_text::Selection {
                    start: (0, 0),
                    end: (last, usize::MAX),
                });
                cx.notify();
            }
            "copy" => {
                let (from, to) = match (selection, selected) {
                    (Some(selection), _) if selection.start != selection.end => {
                        let (from, to) = if selection.start <= selection.end {
                            (selection.start, selection.end)
                        } else {
                            (selection.end, selection.start)
                        };
                        (from, to)
                    }
                    (_, Some(row)) => ((row, 0), (row, usize::MAX)),
                    _ => ((0, 0), (view.rows().saturating_sub(1) as usize, usize::MAX)),
                };
                match view.text_read(from, to) {
                    Some(text) if text.len() <= COPY_BYTES => {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    }
                    _ => view.copy(&app, (from.0 as u64, from.1), (to.0 as u64, to.1)),
                }
            }
            "scroll" => {
                scroll(&view.scroll, args);
                cx.notify();
            }
            _ => return false,
        }
        true
    }
}

/// `ui.inspect`'s `scroll` on rows read where they are shown (a text, a
/// file's bytes): `to` a row up or down, a screen, either end, or a step
/// sideways along lines that do not wrap.
pub(crate) fn scroll(scroll: &Scroll, args: &serde_json::Value) {
    let mut scroll = scroll.borrow_mut();
    let by = match args["to"].as_str().unwrap_or("") {
        "up" => -scroll.row(),
        "down" => scroll.row(),
        "page_up" => -scroll.page(),
        "page_down" => scroll.page(),
        "start" => return scroll.to_edge(false),
        "end" => return scroll.to_edge(true),
        to @ ("left" | "right") => {
            let (x, most, view) = scroll.horizontal();
            let step = crate::preview::arrow_step(view);
            let x = if to == "left" { x - step } else { x + step };
            return scroll.scroll_horizontally_to(x.min(most));
        }
        _ => return,
    };
    scroll.scroll_by(by);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chunks_laid_out_come_first_then_their_neighbours() {
        // Rows 600 to 900 of 2000, chunks of 256: chunks 2 and 3 laid out,
        // 3 then 2 nearest their middle (2.5 rounds down), then 4 and 1.
        let (wanted, middle) = around(&(600..900), 2000, 256);
        assert_eq!((wanted, middle), (vec![2, 3, 4, 1], 2));
        // At the end: no chunk after the last; at the start, none before.
        assert_eq!(around(&(1900..2000), 2000, 256).0, [7, 6]);
        assert_eq!(around(&(0..30), 2000, 256).0, [0, 1]);
        assert_eq!(around(&(0..0), 0, 256).0, [0], "nothing yet: the first");
    }

    #[test]
    fn past_the_budget_the_farthest_chunks_go() {
        let mut chunks: HashMap<u64, u64> =
            (0..KEPT as u64 + 3).map(|chunk| (chunk, chunk)).collect();
        let gone = let_go(&mut chunks, &[0, 1], 0);
        assert_eq!(gone.len(), 3);
        assert!(gone.iter().all(|chunk| *chunk >= KEPT as u64), "{gone:?}");
        assert_eq!(chunks.len(), KEPT);
        assert!(let_go(&mut chunks, &[], 0).is_empty(), "within it: none");
    }
}
