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
//!
//! A text a tree-sitter grammar colors (ADR-110) is parsed once the view
//! shows: the copy the preview holds whole (`Held::whole`, up to what it
//! keeps), UTF-8, never the file read again; a remote's file not held
//! whole is not fetched for it. Its chunks wait plain for the parse, then
//! each is colored from it, exactly wherever it is: a row that goes on
//! with a long line takes the spans of its line from where it starts in
//! it. A parse that fails, or a text it cannot have, is colored by bat's
//! grammar. The view does not fold.

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

use crate::code_preview::{Palette, Resume};
use crate::coloring::{
    Parsed, Shade, around, color_rows, let_go, next_to_color, parse, tree_rows, unread,
};
use crate::elements::rows::{Scroll, Seen};
use crate::latest::Latest;
use crate::shell::Shell;

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
    /// The tree-sitter grammar that colors it, when one does
    /// (`highlight::engine`): the text is parsed once the view shows.
    pub grammar: Option<String>,
    /// Whether that grammar says where definitions are (an outline query).
    pub outlined: bool,
}

/// A text parsed by its tree-sitter grammar, and the bytes it was parsed
/// from (the file's, its mark included): where a row starts in its line.
pub(crate) struct Text {
    parsed: Parsed,
    bytes: Arc<[u8]>,
}

/// How a chunk is read: plain, colored from where the chunk before
/// stopped (or fresh) in a palette, or colored from the text's parse.
enum Reading<'a> {
    Plain,
    Colored(Option<Resume>, &'a Palette),
    /// From the parse; by bat's grammar in the palette, fresh, where a
    /// line is too long for it or the helper fails.
    Tree(&'a Text, &'a Palette),
}

/// Where the row at byte `at` of `bytes` starts in its line: how far past
/// the line's start (the text's own, `start`, after its mark).
fn in_line(bytes: &[u8], start: usize, at: usize) -> usize {
    let (start, at) = (start.min(bytes.len()), at.min(bytes.len()));
    bytes[start..at]
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(at - start, |newline| at - start - newline - 1)
}

/// Whether the line that starts at byte `at` of `bytes` is longer than a
/// tree-sitter grammar colors (`tree::LONGEST_LINE`): looked for that far
/// and no further.
fn too_long(bytes: &[u8], at: usize) -> bool {
    let longest = skwir_platform::highlight::tree::LONGEST_LINE;
    !bytes
        .get(at..)
        .unwrap_or_default()
        .iter()
        .take(longest + 1)
        .any(|&byte| byte == b'\n')
        && bytes.len().saturating_sub(at) > longest
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
    let mut blocks = Vec::with_capacity(rows.len());
    let mut lines = Vec::with_capacity(rows.len());
    for row in rows {
        blocks.push(Block {
            text: row.text,
            code: true,
            language: file.language.clone(),
            ..Block::default()
        });
        lines.push((row.line, row.first));
    }
    let (stop, exact) = match reading {
        Reading::Plain => {
            for (n, block) in blocks.iter_mut().enumerate() {
                if n % 64 == 0 {
                    skwir_platform::inspection::check(cancel)?;
                }
                skwir_platform::links::link_block(block, true);
            }
            (None, false)
        }
        Reading::Colored(from, palette) => {
            let exact = chunk == 0 || from.is_some();
            let language = file.language.as_deref();
            let stop = color_rows(&mut blocks, |n| ends[n], (language, palette), from, cancel)?;
            // Colored rows are read after the plain ones showed: the paths
            // they write are looked up now.
            if let Some(paths) = paths {
                paths.lock().link(&mut blocks);
                skwir_platform::inspection::check(cancel)?;
            }
            (Some(stop), exact)
        }
        Reading::Tree(text, palette) => {
            // Where each row starts in its line: a row that begins one at
            // its start, one that goes on with it where the row before
            // ended, the chunk's first where its place says. A line cut
            // into rows is longer than a row: one longer than a grammar
            // colors leaves the chunk to bat's.
            let mut at: Vec<(u32, usize)> = Vec::with_capacity(blocks.len());
            let mut long = false;
            let mut offset = usize::try_from(point.offset).unwrap_or(usize::MAX);
            for (n, &(line, first)) in lines.iter().enumerate() {
                let from = match (first, n.checked_sub(1)) {
                    (true, _) => 0,
                    (false, Some(before)) => at[before].1 + blocks[before].text.len(),
                    (false, None) => in_line(
                        &text.bytes,
                        usize::try_from(form.start).unwrap_or(usize::MAX),
                        offset,
                    ),
                };
                if (from > 0 || !ends[n]) && (n == 0 || first) {
                    long |= too_long(&text.bytes, offset.saturating_sub(from));
                }
                at.push((u32::try_from(line).unwrap_or(u32::MAX), from));
                // The next row's place: past this one's text, and its
                // line's ending when it ends one.
                offset += blocks[n].text.len();
                if ends[n] {
                    offset = text.bytes[offset.min(text.bytes.len())..]
                        .iter()
                        .position(|&byte| byte == b'\n')
                        .map_or(text.bytes.len(), |newline| offset + newline + 1);
                }
            }
            let colored = if long {
                Err(anyhow::anyhow!("a line longer than a grammar colors"))
            } else {
                tree_rows(&mut blocks, |n| at[n], &text.parsed.tree, cancel)
            };
            // Colored fresh, as after a jump, and taken as exact: the parse
            // would fail it again.
            if let Err(error) = colored {
                skwir_platform::inspection::check(cancel)?;
                tracing::debug!(%error, chunk, "a text's rows colored by bat's grammar instead");
                let language = file.language.as_deref();
                color_rows(&mut blocks, |n| ends[n], (language, palette), None, cancel)?;
            }
            if let Some(paths) = paths {
                paths.lock().link(&mut blocks);
                skwir_platform::inspection::check(cancel)?;
            }
            // Exact wherever it is: no state goes on to the next.
            return Ok((
                Chunk {
                    rows: blocks,
                    lines,
                    colored: true,
                    exact: true,
                },
                None,
            ));
        }
    };
    Ok((
        Chunk {
            rows: blocks,
            lines,
            colored: stop.is_some(),
            exact,
        },
        stop,
    ))
}

/// The first rows of `file`, plain, for the inspection's worker: shown at
/// once, colored by the view, in the colors and with the grammars `code`
/// chooses.
pub(crate) fn start(
    file: &TextFile,
    code: &crate::code_preview::CodeTheme,
) -> anyhow::Result<Start> {
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
    // A grammar parses UTF-8 the preview holds.
    let grammar = (form.encoding == skwir_platform::inspection::Encoding::Utf8
        && file.held.is_some())
    .then(|| {
        let name = file.path.file_name().map(|name| name.to_string_lossy());
        let language = file.language.as_deref();
        let first_line = chunk.rows.first().map(|row| row.text.as_str());
        let name = name.as_deref().or(language).unwrap_or_default();
        match skwir_platform::highlight::engine(name, language, first_line, code.grammars) {
            skwir_platform::highlight::Engine::TreeSitter(grammar) => Some(grammar),
            skwir_platform::highlight::Engine::Bat => None,
        }
    })
    .flatten();
    Ok(Start {
        form,
        chunk,
        palette: Arc::new(crate::code_preview::Palette::of(code)),
        outlined: grammar.as_deref().is_some_and(crate::inspector::outlines),
        grammar,
    })
}

/// The bytes a grammar parses of `file`: what the preview holds of it
/// whole, up to what it keeps, UTF-8 from its mark on. `None` for a file
/// it does not hold (a member written out), a remote's file it does not
/// hold whole yet (never fetched for colors), or one that is not UTF-8
/// throughout.
fn held_text(file: &TextFile, form: Form, cancel: &CancelToken) -> Option<Arc<[u8]>> {
    let held = file.held.as_ref()?;
    if held.is_remote() && held.held_whole().is_none() {
        return None;
    }
    let bytes = held
        .whole(held.keeps(), cancel)
        .inspect_err(|error| tracing::debug!(%error, "a text's bytes, to parse"))
        .ok()
        .flatten()?;
    let start = usize::try_from(form.start).ok()?;
    std::str::from_utf8(bytes.get(start..)?).ok()?;
    Some(bytes)
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
    /// Its parse, when a tree-sitter grammar colors it.
    parse: Parse,
}

/// Where a text a tree-sitter grammar colors is with its parse.
enum Parse {
    /// bat's grammar colors it: none chose a tree-sitter grammar, or the
    /// parse could not be had.
    None,
    /// On its way: the chunks stay plain until it comes, rather than be
    /// colored twice.
    Asked(Latest<Option<Arc<Text>>>),
    Done(Arc<Text>),
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
            grammar,
            outlined: _,
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
        let mut view = TextView {
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
            parse: Parse::None,
        };
        view.count_more(app, view.paced.as_ref().map(|_| PACED_FIRST));
        if let Some(grammar) = grammar {
            view.parse_with(app, form, grammar);
        }
        view
    }

    /// Parse the text with `grammar` on a worker, in the helper every
    /// view shares: the bytes the preview holds, not the file again.
    fn parse_with(&mut self, app: &Arc<skwir_core::App>, form: Form, grammar: String) {
        let (file, palette) = (self.file.clone(), Arc::clone(&self.palette));
        let mut asked = Latest::default();
        // What it reads is held: a remote's file not held whole is never
        // parsed, so its lane is not taken for it.
        asked.ask(
            app,
            skwir_core::scheduler::Lane::SHARED,
            &self.counting,
            Priority::Visible,
            "text-rows-parse",
            move |cancel| {
                let bytes = held_text(&file, form, cancel)?;
                let start = usize::try_from(form.start).ok()?;
                let parsed = parse(&grammar, &bytes[start..], &palette, false, cancel)?;
                Some(Arc::new(Text { parsed, bytes }))
            },
        );
        self.parse = Parse::Asked(asked);
    }

    /// Its definitions, as its tree-sitter grammar found them.
    pub(crate) fn outline(&self) -> Option<&Arc<crate::outline::Symbols>> {
        match &self.parse {
            Parse::Done(text) => text.parsed.outline.as_ref(),
            _ => None,
        }
    }

    /// The line row `row` is of, once it is read.
    pub(crate) fn line_of(&self, row: u64) -> Option<u64> {
        let chunk = self.chunks.get(&(row / STRIDE))?;
        chunk
            .lines
            .get((row % STRIDE) as usize)
            .map(|&(line, _)| line)
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
            // Then one to color: plain, or colored fresh while where it
            // starts is now known.
            let shade = |chunk: u64| {
                if self.failed.contains(&chunk) {
                    return None;
                }
                let read = self.chunks.get(&chunk)?;
                place(chunk)?;
                Some(match (read.colored, read.exact) {
                    (false, _) => Shade::Plain,
                    (true, false) => Shade::Fresh,
                    (true, true) => Shade::Exact,
                })
            };
            // Colored from its parse, a chunk is exact wherever it is;
            // while the parse is on its way, none is colored.
            let (coloring, known) = match &self.parse {
                Parse::None => (true, false),
                Parse::Asked(_) => (false, false),
                Parse::Done(_) => (true, true),
            };
            missing
                .or_else(|| {
                    let chunk = next_to_color(&self.wanted, shade, |chunk| {
                        known || self.starts.contains_key(&chunk)
                    })
                    .filter(|_| coloring)?;
                    Some((chunk, place(chunk)?, true))
                })
                .map(|(chunk, point, colored)| (chunk, point, colored, form))
        };
        let Some((chunk, point, colored, form)) = next else {
            return;
        };
        let from = self.starts.get(&chunk).cloned();
        let (file, palette) = (self.file.clone(), Arc::clone(&self.palette));
        let tree = match &self.parse {
            Parse::Done(text) => Some(Arc::clone(text)),
            _ => None,
        };
        let paths = self.paths.clone();
        self.asking = Some(chunk);
        self.asked.ask(
            app,
            skwir_core::fs::reach::lane(&self.file.path),
            &self.counting,
            Priority::Visible,
            "text-rows-read",
            move |cancel| {
                let reading = match (colored, &tree) {
                    (false, _) => Reading::Plain,
                    (true, Some(text)) => Reading::Tree(text.as_ref(), &palette),
                    (true, None) => Reading::Colored(from, &palette),
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

    /// Take a chunk that arrived, or the parse. Whether one did.
    fn take(&mut self) -> bool {
        let parsed = match &mut self.parse {
            Parse::Asked(asked) => asked.take(),
            _ => None,
        };
        let took = parsed.is_some();
        if let Some(parsed) = parsed {
            // Its chunks are colored from it now; its contents show.
            self.parse = parsed.map_or(Parse::None, Parse::Done);
        }
        let Some(Reply { chunk, read }) = self.asked.take() else {
            return took;
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
        // A code view held whole is colored where it is shown too.
        self.sync_colors(cx);
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
    fn a_row_starts_in_its_line_after_the_line_s_newline() {
        assert_eq!(in_line(b"ab\ncdef", 0, 5), 2);
        assert_eq!(in_line(b"abcdef", 0, 4), 4, "the first line");
        assert_eq!(in_line(b"\xEF\xBB\xBFabcd", 3, 5), 2, "after its mark");
        assert_eq!(in_line(b"ab\n", 0, 3), 0, "a line's start");
        assert_eq!(in_line(b"ab", 0, 9), 2, "past the end: its end");
    }

    #[test]
    fn a_line_past_the_longest_a_grammar_colors_is_left_to_bat() {
        let longest = skwir_platform::highlight::tree::LONGEST_LINE;
        let mut bytes = vec![b'a'; longest];
        assert!(
            !too_long(&bytes, 0),
            "the last line, as long as the longest"
        );
        bytes.push(b'\n');
        bytes.extend_from_slice(b"next");
        assert!(!too_long(&bytes, 0));
        assert!(!too_long(&bytes, longest + 1), "the line after");
        bytes.insert(0, b'a');
        assert!(too_long(&bytes, 0));
        assert!(!too_long(b"ab", 0));
    }

    /// A line long enough to be cut into rows across a chunk's end: each
    /// row, the chunk's first too, takes the spans of its line from where
    /// it starts in it, so its numbers, and only they, wear the number's
    /// color, a number cut in two included.
    #[cfg(target_os = "linux")]
    #[test]
    fn rows_of_a_long_line_take_its_spans_from_where_they_start() {
        use skwir_platform::highlight::tree::Tree;
        use std::fmt::Write as _;
        let Some(helper) = crate::coloring::tests::json_helper() else {
            return;
        };
        let mut text = String::from("[\n");
        for n in 1..250 {
            writeln!(text, "{n},").unwrap();
        }
        let mut n = 0u64;
        while text.len() < 13 * text_rows::ROW_BYTES {
            write!(text, "{}, ", n * 7919 % 100_003).unwrap();
            n += 1;
        }
        text.push_str("0]\n");
        let bytes: Arc<[u8]> = text.as_bytes().into();
        let path = std::path::PathBuf::from("/nowhere/numbers.json");
        let file = TextFile {
            path: path.clone(),
            language: Some("json".into()),
            whole: false,
            held: Some(Arc::new(skwir_core::preview::Held::of_bytes(
                path,
                Arc::clone(&bytes),
            ))),
        };
        let cancel = CancelToken::new();
        let form = Form::read(&file, &cancel).unwrap();
        let index = RwLock::new(Index::new(form));
        text_rows::count_until(&file, &index, None, &cancel, || {}).unwrap();
        let start = index.read().point(1).unwrap();
        assert!(!start.first, "the second chunk starts inside the long line");
        assert_eq!(held_text(&file, form, &cancel).as_deref(), Some(&bytes[..]));
        let palette = Palette::of(&crate::code_preview::CodeTheme::own(false));
        let tree = Tree::open_in(helper, "json", &bytes, &palette, &cancel).unwrap();
        let parsed = Text {
            parsed: Parsed {
                tree,
                folds: Arc::default(),
                outline: None,
            },
            bytes,
        };
        let number = palette.look_of("number").color;
        let mut long_rows = 0;
        for chunk in 0..2 {
            let point = index.read().point(chunk).unwrap();
            let (read, stop) = read_chunk(
                &file,
                form,
                (chunk, point),
                Reading::Tree(&parsed, &palette),
                None,
                &cancel,
            )
            .unwrap();
            assert!(read.colored && read.exact && stop.is_none());
            for (row, &(line, _)) in read.rows.iter().zip(&read.lines) {
                if line != 250 {
                    continue;
                }
                long_rows += 1;
                for (at, ch) in row.text.char_indices() {
                    let color = row
                        .runs
                        .iter()
                        .find(|run| run.range.contains(&at))
                        .and_then(|run| run.color)
                        .unwrap();
                    assert_eq!(
                        ch.is_ascii_digit(),
                        color == number,
                        "{ch:?} at {at} of a row of line 250, chunk {chunk}"
                    );
                }
            }
        }
        assert!(long_rows > 10, "{long_rows} rows");
    }
}
