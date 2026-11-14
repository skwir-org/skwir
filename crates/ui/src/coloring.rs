//! Text colored where it is shown (ADR-051), never whole before it shows:
//! a text read where it is shown (`text_view`), and the code view of an
//! inspection held whole (`CodeColors`). A file's bytes are read the same
//! way, a chunk at a time around the view (`bytes_view`).
//!
//! Rows come in chunks of `STRIDE`; those around the view are wanted,
//! nearest first, and those far from it go past `KEPT`. A chunk is colored
//! from where the one before stopped (`Resume`): exactly, when that one
//! was; fresh, from the language's start, after a jump, and again once the
//! one before it has been colored exactly.
//!
//! A code view's chunks are wanted from the lines it drew, so a closed fold
//! between them asks for nothing inside it. Once those are colored, a pass
//! goes on in the background from the last chunk colored exactly to the
//! farthest wanted, keeping only where each chunk stops; then every chunk
//! wanted is colored exactly.
//!
//! **The first rows arrive colored** (ADR-051, amended): the worker that
//! makes a view colors its first chunk before handing it over, until
//! `COLORS_WITHIN` has passed, so its first frame has them. Rows whose
//! colors are late show as far as they got, and the view colors them as it
//! colors any other. Lines too long to color in that time are not waited
//! for (`waits_for_colors`).
//!
//! A text a tree-sitter grammar colors (ADR-110, `highlight::engine`) is
//! parsed whole in the helper (`parse`): by the worker making the view
//! when the helper runs and the text is short (`may_parse_soon`), else
//! once it shows, its chunks plain meanwhile rather than colored twice;
//! then each chunk is colored from the parse, exactly at once, wherever
//! it is (`tree_rows`): no state goes from one chunk to the next. A parse
//! that fails leaves the text to
//! bat's grammar, and so does a chunk the parse cannot color, colored
//! fresh (`parsed_rows`), in the code view as in the text view.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::Context;
use parking_lot::Mutex;
use skwir_core::cancel::CancelToken;
use skwir_core::inspection::{Block, Inspection};
use skwir_core::scheduler::{Lane, Priority};

use crate::code_preview::{Highlighter, Palette, Resume};
use crate::latest::Latest;
use crate::preview::PreviewContent;
use crate::shell::Shell;
use skwir_platform::highlight::tree::Tree;

/// Chunks kept around the view: a few screens either way.
pub(crate) const KEPT: usize = 24;
/// Rows colored at once.
pub(crate) const STRIDE: u64 = skwir_platform::text_rows::STRIDE;
/// How long rows wait for their colors before showing without them, looked
/// at between rows. A grammar ready colors a chunk in milliseconds. Below
/// what reads as a wait, and below `Shell::BUSY_AFTER`. A grammar being
/// loaded or compiled for its first file in a while (ADR-060) is not cut
/// short: tens of milliseconds where it was measured, more on a slower
/// machine, and its text waits that long. Tests are built unoptimized,
/// where a chunk takes longer, on machines under load: what they say must
/// not depend on the clock.
pub(crate) const COLORS_WITHIN: Duration = if cfg!(test) {
    Duration::from_secs(60)
} else {
    Duration::from_millis(100)
};

/// The longest text parsed before it shows (`may_parse_soon`). A parse is
/// of the whole text, so one too long to make `COLORS_WITHIN` would cost
/// that time on every look and be thrown away, as a chunk of long lines
/// would (`waits_for_colors`). Measured with a release helper and the JSON
/// grammar: 28 ms for 128 KiB, 57 for 256, 120 for 512, 246 for 1 MiB. A
/// heavier grammar or a slower machine has room here; a wrong guess costs
/// at most the limit, or a flash.
const PARSED_SOON: u64 = 128 * 1024;

/// Whether `rows` are worth holding back for their colors: not with a line
/// long enough to be near a row's limit, where a chunk is a megabyte to
/// color (ADR-051) and the time would be spent for nothing, on every look.
pub(crate) fn waits_for_colors(rows: &[Block]) -> bool {
    rows.iter()
        .all(|row| row.text.len() * 2 < skwir_platform::text_rows::ROW_BYTES)
}

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

/// The chunks of `per` lines to have for the lines `drawn` of `lines`, in
/// any order and with folds between them: those holding a line drawn,
/// nearest their middle first, then the one after the last and the one
/// before the first. And their middle, to tell what is far.
pub(crate) fn chunks_of(drawn: &[usize], lines: usize, per: u64) -> (Vec<u64>, u64) {
    let last = (lines as u64).saturating_sub(1) / per;
    let mut chunks: Vec<u64> = drawn
        .iter()
        .map(|&line| line as u64 / per)
        .filter(|&chunk| chunk <= last)
        .collect();
    chunks.sort_unstable();
    chunks.dedup();
    let (Some(&first), Some(&end)) = (chunks.first(), chunks.last()) else {
        return (Vec::new(), 0);
    };
    let middle = u64::midpoint(first, end);
    chunks.sort_by_key(|chunk| chunk.abs_diff(middle));
    chunks.extend((end < last).then_some(end + 1));
    chunks.extend(first.checked_sub(1));
    (chunks, middle)
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

/// Rows `rows`, their runs replaced: colored in `language` from `from`
/// (fresh from the language's start without), `ends` saying whether row
/// `n` ends its line, then given the links their text writes
/// (`link_block`). Where the colors stopped, to go on from; `None` when
/// `until` came first: the rows reached keep their colors, the others are
/// plain, their links found all the same.
pub(crate) fn color_rows(
    rows: &mut [Block],
    ends: impl Fn(usize) -> bool,
    (language, palette): (Option<&str>, &Palette),
    from: Option<Resume>,
    until: Option<Instant>,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Resume>> {
    let mut colors = Highlighter::resume(language, palette, from);
    for n in 0..rows.len() {
        if n % 64 == 0 {
            skwir_platform::inspection::check(cancel)?;
        }
        if until.is_some_and(|until| Instant::now() >= until) {
            for row in &mut rows[n..] {
                skwir_platform::links::link_block(row, true);
            }
            return Ok(None);
        }
        rows[n].runs = colors.row(&rows[n].text, ends(n));
        skwir_platform::links::link_block(&mut rows[n], true);
    }
    Ok(Some(colors.stop()))
}

/// Rows `rows`, their runs replaced: colored by `tree`, row `n` being the
/// bytes from `at(n).1` of line `at(n).0`, then given the links their text
/// writes (`link_block`). Exact wherever the rows are: nothing goes from
/// one chunk to the next.
pub(crate) fn tree_rows(
    rows: &mut [Block],
    at: impl Fn(usize) -> (u32, usize),
    tree: &Tree<'_>,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let (Some(first), Some(last)) = (
        rows.first().map(|_| at(0).0),
        rows.len().checked_sub(1).map(|last| at(last).0),
    ) else {
        return Ok(());
    };
    skwir_platform::inspection::check(cancel)?;
    let spans = tree.spans(first..last.saturating_add(1), cancel)?;
    for (n, row) in rows.iter_mut().enumerate() {
        if n % 64 == 0 {
            skwir_platform::inspection::check(cancel)?;
        }
        let (line, from) = at(n);
        let spans = spans
            .get(line.saturating_sub(first) as usize)
            .map_or(&[][..], Vec::as_slice);
        row.runs = tree.runs(spans, from, &row.text);
        skwir_platform::links::link_block(row, true);
    }
    Ok(())
}

/// Rows `rows` colored from their parse (`tree_rows`, `at` saying where
/// each is), else by bat's grammar in `colors`, fresh from the rows'
/// start (`ends(n)` saying whether row `n` ends its line), where the parse
/// cannot color them: `tree` an error (a line longer than a grammar
/// colors), or the helper failing, timing out or gone. What tree-sitter
/// cannot color bat colors, in every view alike; either way the rows are
/// as exact as they will be, since the parse would fail them again.
pub(crate) fn parsed_rows(
    rows: &mut [Block],
    (at, ends): (impl Fn(usize) -> (u32, usize), impl Fn(usize) -> bool),
    tree: anyhow::Result<&Tree<'_>>,
    colors: (Option<&str>, &Palette),
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let colored = tree.and_then(|tree| tree_rows(rows, at, tree, cancel));
    if let Err(error) = colored {
        skwir_platform::inspection::check(cancel)?;
        tracing::debug!(%error, "rows colored by bat's grammar, not their parse");
        color_rows(rows, ends, colors, None, None, cancel)?;
    }
    Ok(())
}

/// A text a tree-sitter grammar parsed for a view (ADR-110), and what the
/// grammar says of it. Held by the view, and dropped with it: the helper
/// then forgets the text.
pub(crate) struct Parsed {
    pub tree: Tree<'static>,
    /// Where it folds, as the code view folds (`Inspection::folds`): asked
    /// only of a code view that has no folds of its own.
    pub folds: Arc<[Range<usize>]>,
    /// Its definitions, for the contents; `None` without any.
    pub outline: Option<Arc<crate::outline::Symbols>>,
}

/// `text` (UTF-8) parsed by the tree-sitter grammar named `grammar` in the
/// shared helper, its folds asked when `folds` says, and its outline: on a
/// worker. `None` when the helper cannot (not installed, the grammar
/// failing, a timeout): bat's grammar colors it then. What the grammar
/// cannot say beside its colors (no folds query, no outline) is left out.
pub(crate) fn parse(
    grammar: &str,
    text: skwir_platform::tree_sitter::Text,
    palette: &Palette,
    folds: bool,
    cancel: &CancelToken,
) -> Option<Parsed> {
    let tree = Tree::open(grammar, text, palette, cancel)
        .inspect_err(|error| {
            if !cancel.is_cancelled() {
                tracing::debug!(%error, grammar, "coloring with a tree-sitter grammar");
            }
        })
        .ok()?;
    let folds = if folds {
        tree.folds(cancel)
            .inspect_err(|error| tracing::debug!(%error, grammar, "a tree-sitter grammar's folds"))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let outline = tree
        .outline(cancel)
        .inspect_err(|error| tracing::debug!(%error, grammar, "a tree-sitter grammar's outline"))
        .ok()
        .and_then(|symbols| crate::outline::Symbols::of(&symbols))
        .map(Arc::new);
    Some(Parsed {
        tree,
        folds: folds.into(),
        outline,
    })
}

/// A token of `cancel`'s that is cancelled `COLORS_WITHIN` from now, and
/// what keeps that from happening once dropped: the bound on a wait for
/// the helper, which unlike coloring cannot look at a clock between rows.
/// One thread cancels every such token at its time (`Limits`), not one a
/// wait.
pub(crate) fn soon(cancel: &CancelToken) -> (CancelToken, Soon) {
    limited(cancel, COLORS_WITHIN)
}

/// A token of `cancel`'s cancelled `within` from now (`soon`).
fn limited(cancel: &CancelToken, within: Duration) -> (CancelToken, Soon) {
    let limit = cancel.child();
    let kept = Arc::new(());
    let asked = Limits::get().is_some_and(|limits| {
        limits
            .send((
                Instant::now() + within,
                limit.clone(),
                Arc::downgrade(&kept),
            ))
            .is_ok()
    });
    // Without the thread nothing could bound the wait: none is waited for.
    if !asked {
        limit.cancel();
    }
    (limit, Soon { _kept: kept })
}

/// What keeps a token of `soon` from being cancelled at its time, until it
/// is dropped: the wait it bounds is over.
pub(crate) struct Soon {
    _kept: Arc<()>,
}

/// The tokens to cancel at their time (`soon`), and the thread that does:
/// started with the first, asleep until the next time is due, and while
/// none is waiting. A token whose wait ended before its time (its `Soon`
/// dropped) is passed over.
struct Limits;

type Limit = (Instant, CancelToken, std::sync::Weak<()>);

impl Limits {
    /// Where to send a token to cancel at its time; `None` where the thread
    /// could not be started.
    fn get() -> Option<&'static flume::Sender<Limit>> {
        static LIMITS: std::sync::OnceLock<Option<flume::Sender<Limit>>> =
            std::sync::OnceLock::new();
        LIMITS
            .get_or_init(|| {
                let (send, asked) = flume::unbounded::<Limit>();
                std::thread::Builder::new()
                    .name("skwir-colors-limits".into())
                    .spawn(move || Self::run(&asked))
                    .ok()
                    .map(|_| send)
            })
            .as_ref()
    }

    fn run(asked: &flume::Receiver<Limit>) {
        use std::cmp::Reverse;
        // By time, then by arrival: a heap of the times, the tokens by
        // their arrival beside it.
        let mut due: std::collections::BinaryHeap<Reverse<(Instant, u64)>> =
            std::collections::BinaryHeap::new();
        let mut waiting: HashMap<u64, (CancelToken, std::sync::Weak<()>)> = HashMap::new();
        let mut next = 0u64;
        loop {
            let arrived = match due.peek() {
                Some(Reverse((at, _))) => asked.recv_deadline(*at),
                None => asked
                    .recv()
                    .map_err(|_| flume::RecvTimeoutError::Disconnected),
            };
            match arrived {
                Ok((at, token, kept)) => {
                    due.push(Reverse((at, next)));
                    waiting.insert(next, (token, kept));
                    next += 1;
                }
                Err(flume::RecvTimeoutError::Timeout) => {}
                Err(flume::RecvTimeoutError::Disconnected) => return,
            }
            let now = Instant::now();
            while let Some(Reverse((at, key))) = due.peek().copied()
                && at <= now
            {
                due.pop();
                if let Some((token, kept)) = waiting.remove(&key)
                    && kept.strong_count() > 0
                {
                    token.cancel();
                }
            }
        }
    }
}

/// Whether a parse of `bytes` could make it in `COLORS_WITHIN`: starting
/// the helper takes longer than rows may wait, and one given up on would
/// have started it for nothing.
pub(crate) fn may_parse_soon(bytes: u64) -> bool {
    short_enough_to_parse_soon(bytes) && skwir_platform::tree_sitter::shared().is_running()
}

fn short_enough_to_parse_soon(bytes: u64) -> bool {
    bytes <= PARSED_SOON
}

/// `parse`, by `limit` (`soon`). `None` when it did not make it, what the
/// grammar says beside its colors included.
pub(crate) fn parse_soon(
    grammar: &str,
    text: skwir_platform::tree_sitter::Text,
    palette: &Palette,
    folds: bool,
    limit: &CancelToken,
) -> Option<Parsed> {
    parse(grammar, text, palette, folds, limit).filter(|_| !limit.is_cancelled())
}

/// The first chunk of a code view, colored on the worker making it, if
/// that took no longer than `COLORS_WITHIN`: or only its parse, where
/// coloring the chunk from it did not make it, which the view then colors
/// from the parse rather than parse the text again.
pub(crate) struct FirstColors {
    rows: Option<Vec<Block>>,
    /// Where bat's colors stopped; none where a tree-sitter grammar's.
    stop: Option<Resume>,
    parsed: Option<Arc<Parsed>>,
}

/// The first chunk of `data` colored as `with` says, for its first frame:
/// on the worker making the view. `None` when the view is to color it, as
/// when its colors are late.
pub(crate) fn first_colors(data: &Inspection, with: &Colorer) -> Option<FirstColors> {
    let mut rows = rows_of(data, 0);
    if !waits_for_colors(&rows) {
        return None;
    }
    let cancel = skwir_core::scheduler::job_token().unwrap_or_default();
    if let Some(grammar) = &with.grammar {
        let bytes = data.blocks.iter().map(|b| b.text.len() as u64 + 1).sum();
        if !may_parse_soon(bytes) {
            return None;
        }
        let (limit, _timer) = soon(&cancel);
        let text = joined(data);
        let parsed = parse_soon(grammar, text, &with.palette, data.folds.is_empty(), &limit)?;
        let at = |n: usize| (u32::try_from(n).unwrap_or(u32::MAX), 0);
        let colored = tree_rows(&mut rows, at, &parsed.tree, &limit).is_ok();
        return Some(FirstColors {
            rows: colored.then_some(rows),
            stop: None,
            parsed: Some(Arc::new(parsed)),
        });
    }
    let until = Instant::now() + COLORS_WITHIN;
    let colors = (with.language.as_deref(), &*with.palette);
    let stop = color_rows(&mut rows, |_| true, colors, None, Some(until), &cancel).ok()??;
    Some(FirstColors {
        rows: Some(rows),
        stop: Some(stop),
        parsed: None,
    })
}

/// How far a chunk is colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shade {
    /// Not colored: its rows plain.
    Plain,
    /// Colored fresh, after a jump: from the language's start.
    Fresh,
    /// Colored from where the chunk before stopped, or the text's start.
    Exact,
}

/// The chunk of `wanted` (nearest the view first) to color next: the first
/// plain, or colored fresh while where it starts is now `known`. `shade`
/// is `None` for one that cannot be colored yet (not read, or a read of it
/// failed).
pub(crate) fn next_to_color(
    wanted: &[u64],
    shade: impl Fn(u64) -> Option<Shade>,
    known: impl Fn(u64) -> bool,
) -> Option<u64> {
    wanted.iter().copied().find(|&chunk| match shade(chunk) {
        Some(Shade::Plain) => true,
        Some(Shade::Fresh) => known(chunk),
        Some(Shade::Exact) | None => false,
    })
}

/// What colors an inspection's code view where it is shown: its language
/// and the palette, settled on the worker with the inspection
/// (`inspector::Display`). `None` there for a view colored whole.
#[derive(Clone)]
pub(crate) struct Colorer {
    pub language: Option<String>,
    pub palette: Arc<Palette>,
    /// The tree-sitter grammar that colors it, when one does
    /// (`highlight::engine`); bat's grammar for `language` without, or
    /// when the grammar fails.
    pub grammar: Option<String>,
    /// Whether that grammar says where definitions are (an outline query).
    pub outlined: bool,
}

/// The rows of a chunk of a code view, colored, and how.
pub(crate) struct Colored {
    pub rows: Vec<Block>,
    exact: bool,
}

enum Reply {
    /// A chunk colored, and where bat's colors stopped (none where a
    /// tree-sitter grammar colored it).
    Chunk {
        chunk: u64,
        rows: Vec<Block>,
        exact: bool,
        stop: Option<Resume>,
    },
    /// A pass forward ended, with the rows of the chunks it was asked to
    /// keep; where each chunk stopped went in `progress` as it went.
    Forward { kept: Vec<(u64, Vec<Block>)> },
    /// The text parsed by its tree-sitter grammar; `None` when it could
    /// not be.
    Parsed(Option<Arc<Parsed>>),
}

/// What is on its way.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Asked {
    Chunk(u64),
    Forward,
    Parse,
}

/// Where a code view a tree-sitter grammar colors is with its parse.
#[derive(Default)]
enum Parse {
    /// Not asked: bat's grammar colors it, or nothing was drawn yet.
    #[default]
    Unasked,
    /// On its way, the chunks plain until it comes.
    Asked,
    Done(Arc<Parsed>),
    /// It could not be parsed: bat's grammar colors it.
    Failed,
}

/// An inspection's code view colored where it is shown (`inspector`): the
/// chunks around the lines drawn, the others plain until they are.
pub(crate) struct CodeColors {
    /// The chunks colored, kept around the view (`KEPT`), shared with each
    /// frame's rows.
    pub(crate) chunks: Rc<HashMap<u64, Rc<Colored>>>,
    /// Where each chunk's colors start exactly: where the one before
    /// stopped. Kept for every chunk reached, a state per 256 lines.
    starts: HashMap<u64, Resume>,
    /// The first chunk past the first whose start is not known.
    frontier: u64,
    /// Where a pass forward put the starts it reached, kept when it is
    /// let go of for a chunk the view wants.
    progress: Arc<Mutex<Vec<(u64, Resume)>>>,
    /// The lines drawn last, and the chunks they want, nearest first.
    drawn: Vec<usize>,
    wanted: Vec<u64>,
    middle: u64,
    asked: Latest<Reply>,
    asking: Option<Asked>,
    /// The parse of a view a tree-sitter grammar colors (`Colorer::grammar`).
    parse: Parse,
}

impl Default for CodeColors {
    fn default() -> Self {
        CodeColors {
            chunks: Rc::default(),
            starts: HashMap::new(),
            frontier: 1,
            progress: Arc::default(),
            drawn: Vec::new(),
            wanted: Vec::new(),
            middle: 0,
            asked: Latest::default(),
            asking: None,
            parse: Parse::default(),
        }
    }
}

/// What to color next: a chunk the view wants, and where its colors start;
/// or a pass forward over chunks `first..=last`, keeping those wanted.
#[derive(Debug, PartialEq)]
enum Next {
    Chunk(u64),
    Forward { first: u64, last: u64 },
}

impl CodeColors {
    /// The colored rows of line `line`, if its chunk is colored.
    pub(crate) fn row(chunks: &HashMap<u64, Rc<Colored>>, line: usize) -> Option<&Block> {
        let line = line as u64;
        chunks
            .get(&(line / STRIDE))?
            .rows
            .get((line % STRIDE) as usize)
    }

    /// The folds its tree-sitter grammar found, for a view whose
    /// inspection has none of its own: none until it is parsed.
    pub(crate) fn folds(&self) -> Option<&Arc<[Range<usize>]>> {
        match &self.parse {
            Parse::Done(parsed) if !parsed.folds.is_empty() => Some(&parsed.folds),
            _ => None,
        }
    }

    /// The first chunk, colored on the worker that made the view. Whether
    /// its parse came with it, for what waits for one.
    pub(crate) fn begin_with(&mut self, first: FirstColors) -> bool {
        let FirstColors { rows, stop, parsed } = first;
        if let Some(rows) = rows {
            Rc::make_mut(&mut self.chunks).insert(0, Rc::new(Colored { rows, exact: true }));
        }
        if let Some(stop) = stop {
            self.started(1, stop);
        }
        let came = parsed.is_some();
        if let Some(parsed) = parsed {
            self.parse = Parse::Done(parsed);
        }
        came
    }

    /// Its parse failed, as when the helper cannot have it.
    #[cfg(test)]
    pub(crate) fn parse_failed(&mut self) {
        self.parse = Parse::Failed;
    }

    /// Its definitions, as its tree-sitter grammar found them.
    pub(crate) fn outline(&self) -> Option<&Arc<crate::outline::Symbols>> {
        match &self.parse {
            Parse::Done(parsed) => parsed.outline.as_ref(),
            _ => None,
        }
    }

    fn shade(&self, chunk: u64) -> Shade {
        match self.chunks.get(&chunk) {
            Some(colored) if colored.exact => Shade::Exact,
            Some(_) => Shade::Fresh,
            None => Shade::Plain,
        }
    }

    fn known(&self, chunk: u64) -> bool {
        chunk == 0 || self.starts.contains_key(&chunk)
    }

    fn started(&mut self, chunk: u64, start: Resume) {
        self.starts.insert(chunk, start);
        while self.starts.contains_key(&self.frontier) {
            self.frontier += 1;
        }
    }

    /// What a pass forward reached, kept.
    fn drain(&mut self) {
        let reached = std::mem::take(&mut *self.progress.lock());
        for (chunk, start) in reached {
            self.started(chunk, start);
        }
    }

    /// The lines just drawn, of `lines`: the chunks to have are theirs.
    /// Whether that changed.
    fn laid(&mut self, drawn: Vec<usize>, lines: usize) -> bool {
        if drawn == self.drawn {
            return false;
        }
        let (wanted, middle) = chunks_of(&drawn, lines, STRIDE);
        self.drawn = drawn;
        if wanted == self.wanted {
            return false;
        }
        self.wanted = wanted;
        self.middle = middle;
        true
    }

    /// What to color next: a chunk the view wants, plain or colored fresh
    /// while its start is now known; else, while a chunk it wants starts
    /// where nothing is known yet, a pass forward from the last chunk
    /// colored exactly to it.
    fn next(&self) -> Option<Next> {
        if matches!(self.parse, Parse::Done(_)) {
            // Colored from the parse, a chunk is exact wherever it is.
            return next_to_color(&self.wanted, |chunk| Some(self.shade(chunk)), |_| true)
                .map(Next::Chunk);
        }
        if let Some(chunk) = next_to_color(
            &self.wanted,
            |chunk| Some(self.shade(chunk)),
            |chunk| self.known(chunk),
        ) {
            return Some(Next::Chunk(chunk));
        }
        let last = self
            .wanted
            .iter()
            .copied()
            .filter(|&chunk| !self.known(chunk))
            .max()?;
        Some(Next::Forward {
            first: self.frontier - 1,
            last,
        })
    }

    /// Ask for what is to be colored next of `data`, one thing at a time:
    /// a chunk the view wants goes before a pass forward, which gives way
    /// to it, what it reached kept.
    fn request(&mut self, app: &Arc<skwir_core::App>, data: &Arc<Inspection>, with: &Colorer) {
        self.drain();
        if let Some(asked) = self.asking {
            let given_way = match asked {
                Asked::Chunk(chunk) => !self.wanted.contains(&chunk),
                Asked::Forward => matches!(self.next(), Some(Next::Chunk(_))),
                // A parse is never asked again: it waits for it.
                Asked::Parse => false,
            };
            if !given_way {
                return;
            }
            self.asked.forget();
            self.asking = None;
            self.drain();
        }
        if let Some(grammar) = with.grammar.clone()
            && matches!(self.parse, Parse::Unasked)
            && !self.wanted.is_empty()
        {
            return self.ask_parse(app, data, with, grammar);
        }
        let Some(next) = self.next() else {
            return;
        };
        let (data, with) = (Arc::clone(data), with.clone());
        match next {
            Next::Chunk(chunk) => {
                if let Parse::Done(parsed) = &self.parse {
                    let parsed = Arc::clone(parsed);
                    return self.ask_tree_chunk(app, (data, with), chunk, parsed);
                }
                let exact = self.known(chunk);
                let from = self.starts.get(&chunk).cloned();
                self.asking = Some(Asked::Chunk(chunk));
                self.asked.ask(
                    app,
                    Lane::SHARED,
                    app.lifetime(),
                    Priority::Visible,
                    "code-colors",
                    move |cancel| {
                        let mut rows = rows_of(&data, chunk);
                        let palette = (with.language.as_deref(), &*with.palette);
                        // It fails only when let go of: never taken.
                        let Ok(Some(stop)) =
                            color_rows(&mut rows, |_| true, palette, from, None, cancel)
                        else {
                            return Reply::Forward { kept: Vec::new() };
                        };
                        Reply::Chunk {
                            chunk,
                            rows,
                            exact,
                            stop: Some(stop),
                        }
                    },
                );
            }
            Next::Forward { first, last } => {
                let from = self.starts.get(&first).cloned();
                let keep = self.wanted.clone();
                let progress = Arc::clone(&self.progress);
                self.asking = Some(Asked::Forward);
                self.asked.ask(
                    app,
                    Lane::SHARED,
                    app.lifetime(),
                    Priority::Background,
                    "code-colors-forward",
                    move |cancel| {
                        let palette = (with.language.as_deref(), &*with.palette);
                        let mut from = from;
                        let mut kept = Vec::new();
                        for chunk in first..=last {
                            let mut rows = rows_of(&data, chunk);
                            let Ok(Some(stop)) =
                                color_rows(&mut rows, |_| true, palette, from, None, cancel)
                            else {
                                break;
                            };
                            progress.lock().push((chunk + 1, stop.clone()));
                            from = Some(stop);
                            if keep.contains(&chunk) {
                                kept.push((chunk, rows));
                            }
                        }
                        Reply::Forward { kept }
                    },
                );
            }
        }
    }

    /// Color chunk `chunk` of `data` from its parse, else by bat's grammar
    /// as `with` says (`parsed_rows`).
    fn ask_tree_chunk(
        &mut self,
        app: &Arc<skwir_core::App>,
        (data, with): (Arc<Inspection>, Colorer),
        chunk: u64,
        parsed: Arc<Parsed>,
    ) {
        self.asking = Some(Asked::Chunk(chunk));
        self.asked.ask(
            app,
            Lane::SHARED,
            app.lifetime(),
            Priority::Visible,
            "code-colors-tree",
            move |cancel| {
                let mut rows = rows_of(&data, chunk);
                // A block is a whole line: its spans from its start.
                let first = chunk * STRIDE;
                let at = |n: usize| (u32::try_from(first + n as u64).unwrap_or(u32::MAX), 0);
                let colors = (with.language.as_deref(), &*with.palette);
                // It fails only when let go of: never taken.
                if parsed_rows(&mut rows, (at, |_| true), Ok(&parsed.tree), colors, cancel).is_err()
                {
                    return Reply::Forward { kept: Vec::new() };
                }
                Reply::Chunk {
                    chunk,
                    rows,
                    exact: true,
                    stop: None,
                }
            },
        );
    }

    /// Parse the text with `grammar`: its lines joined, as the code view
    /// holds them (a block a line), its folds asked when the inspection
    /// has none of its own.
    fn ask_parse(
        &mut self,
        app: &Arc<skwir_core::App>,
        data: &Arc<Inspection>,
        with: &Colorer,
        grammar: String,
    ) {
        let (data, palette) = (Arc::clone(data), Arc::clone(&with.palette));
        self.parse = Parse::Asked;
        self.asking = Some(Asked::Parse);
        self.asked.ask(
            app,
            Lane::SHARED,
            app.lifetime(),
            Priority::Visible,
            "code-colors-parse",
            move |cancel| {
                let folds = data.folds.is_empty();
                let text = joined(&data);
                Reply::Parsed(parse(&grammar, text, &palette, folds, cancel).map(Arc::new))
            },
        );
    }

    /// Take what was colored. Whether rows the view draws changed.
    fn take(&mut self) -> bool {
        let Some(reply) = self.asked.take() else {
            return false;
        };
        self.asking = None;
        self.drain();
        let chunks = Rc::make_mut(&mut self.chunks);
        let mut start = None;
        let changed = match reply {
            Reply::Chunk {
                chunk,
                rows,
                exact,
                stop,
            } => {
                // Colored fresh never replaces colored exactly.
                let had = chunks.get(&chunk).is_some_and(|colored| colored.exact);
                if exact || !had {
                    chunks.insert(chunk, Rc::new(Colored { rows, exact }));
                }
                start = stop.filter(|_| exact).map(|stop| (chunk + 1, stop));
                true
            }
            Reply::Forward { kept } => {
                let changed = !kept.is_empty();
                for (chunk, rows) in kept {
                    chunks.insert(chunk, Rc::new(Colored { rows, exact: true }));
                }
                changed
            }
            Reply::Parsed(parsed) => {
                // Its folds and contents come with it.
                self.parse = parsed.map_or(Parse::Failed, Parse::Done);
                true
            }
        };
        let_go(chunks, &self.wanted, self.middle);
        if let Some((chunk, stop)) = start {
            self.started(chunk, stop);
        }
        changed
    }
}

/// The text of `data`'s code view, its lines joined, as it holds them (a
/// block a line): joined once, then kept by the tree alone (to parse again
/// in a helper started later), never copied.
fn joined(data: &Inspection) -> skwir_platform::tree_sitter::Text {
    let mut text = String::new();
    for (n, block) in data.blocks.iter().enumerate() {
        if n > 0 {
            text.push('\n');
        }
        text.push_str(&block.text);
    }
    Arc::<[u8]>::from(text.into_bytes()).into()
}

/// The lines of chunk `chunk` of `data`'s code view, their runs to be
/// colored.
fn rows_of(data: &Inspection, chunk: u64) -> Vec<Block> {
    let lines = data.blocks.len();
    let start = usize::try_from(chunk * STRIDE).unwrap_or(lines).min(lines);
    let end = start.saturating_add(STRIDE as usize).min(lines);
    data.blocks[start..end]
        .iter()
        .map(|block| Block {
            runs: Vec::new(),
            ..block.clone()
        })
        .collect()
}

impl Shell {
    /// The code view drew lines `drawn` (`inspector::content`), after
    /// layout: color what is around them. Never while painting.
    pub(crate) fn inspector_colors(&mut self, drawn: Vec<usize>) {
        let preview = &mut self.preview;
        let Some(PreviewContent::Inspection(display)) = &preview.content else {
            return;
        };
        let Some(with) = display.colors.as_ref() else {
            return;
        };
        let colors = &mut preview.inspection.colors;
        if colors.laid(drawn, display.data.blocks.len()) {
            let middle = colors.middle;
            let_go(Rc::make_mut(&mut colors.chunks), &colors.wanted, middle);
            colors.request(&self.app, &display.data, with);
        }
    }

    /// Take what was colored, as the core wakes the UI, and ask for what
    /// comes next.
    pub(crate) fn sync_colors(&mut self, cx: &mut Context<Self>) {
        let preview = &mut self.preview;
        let colors = &mut preview.inspection.colors;
        if !colors.asked.is_pending() {
            return;
        }
        let waited = matches!(colors.parse, Parse::Asked);
        let changed = colors.take();
        let parsed = waited && !matches!(colors.parse, Parse::Asked);
        if let Some(PreviewContent::Inspection(display)) = &preview.content
            && let Some(with) = display.colors.as_ref()
        {
            preview
                .inspection
                .colors
                .request(&self.app, &display.data, with);
            if parsed {
                // Its folds came, or none will: those kept closed through
                // a refresh close again.
                preview.inspection.folds_arrived(&display.data);
            }
        }
        if changed {
            cx.notify();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Rows the parse cannot color (the helper gone, a line too long for
    /// it) are colored by bat's grammar, fresh, and their links found:
    /// never left plain, whichever view asks.
    #[test]
    fn rows_the_parse_cannot_color_are_colored_by_bat_s_grammar() {
        let palette = Palette::of(&crate::code_preview::CodeTheme::own(false));
        let mut rows: Vec<Block> = [
            "fn main() {",
            "    let answer = 42; // see https://example.invalid/answer",
            "}",
        ]
        .iter()
        .map(|text| Block {
            text: (*text).to_owned(),
            code: true,
            ..Block::default()
        })
        .collect();
        let at = |n: usize| (u32::try_from(n).unwrap(), 0);
        parsed_rows(
            &mut rows,
            (at, |_| true),
            Err(anyhow::anyhow!("the helper is gone")),
            (Some("rs"), &palette),
            &CancelToken::new(),
        )
        .unwrap();
        let colors: HashSet<[u8; 3]> = rows
            .iter()
            .flat_map(|row| row.runs.iter().filter_map(|run| run.color))
            .collect();
        assert!(colors.len() > 2, "{colors:?}");
        assert!(rows[1].runs.iter().any(|run| run.link.is_some()));
    }

    /// The tree-sitter helper built beside the tests, told the JSON grammar
    /// it is tested with (`crates/tree-sitter-worker/tests/fixtures/json`,
    /// as WebAssembly). `cargo nextest run -p skwir-tree-sitter-worker -p
    /// skwir-ui` builds it; without it a test says so and passes, as the
    /// helper's own do. Kept for the test's process: a `Parsed` holds the
    /// helper it was parsed in.
    #[cfg(target_os = "linux")]
    pub(crate) fn json_helper() -> Option<&'static skwir_platform::tree_sitter::Helper> {
        let Some(program) = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("skwir-tree-sitter-worker")))
            .filter(|path| path.is_file())
        else {
            eprintln!("skipped: needs skwir-tree-sitter-worker built beside the tests");
            return None;
        };
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tree-sitter-worker/tests/fixtures/json");
        let helper = skwir_platform::tree_sitter::with_program(program);
        helper.set_grammars(vec![skwir_core::syntax::Grammar {
            name: "json".into(),
            library: fixtures.join("tree-sitter-json.wasm"),
            symbol: "tree_sitter_json".into(),
            highlights: vec![fixtures.join("queries/highlights.scm")],
            ..skwir_core::syntax::Grammar::default()
        }]);
        Some(Box::leak(Box::new(helper)))
    }

    /// A code view colored from its parse a chunk at a time is exact
    /// wherever the chunk is: a comment opened in the chunk before colors
    /// the lines of this one it holds, which bat's grammar, colored fresh
    /// from the chunk's start, cannot know. Links are found as before.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_chunk_is_colored_from_its_parse_exactly_wherever_it_is() {
        use skwir_core::inspection::InspectionKind;
        use skwir_platform::highlight::tree::Tree;
        let Some(helper) = json_helper() else {
            return;
        };
        let lines = 2 * STRIDE as usize + 10;
        let text = |n: usize| match n {
            0 => "[".to_owned(),
            250 => "/* a comment".to_owned(),
            262 => "still one */".to_owned(),
            300 => "\"see https://example.invalid/300\",".to_owned(),
            n if n == lines - 1 => "0]".to_owned(),
            n => format!("{n},"),
        };
        let data = Inspection {
            kind: InspectionKind::Code,
            blocks: (0..lines)
                .map(|n| Block {
                    text: text(n),
                    code: true,
                    language: Some("json".into()),
                    ..Block::default()
                })
                .collect(),
            ..Inspection::default()
        };
        let joined = (0..lines).map(text).collect::<Vec<_>>().join("\n");
        let palette = Palette::of(&crate::code_preview::CodeTheme::own(false));
        let cancel = CancelToken::new();
        let tree = Tree::open_in(helper, "json", joined.as_bytes(), &palette, &cancel).unwrap();
        let mut rows = rows_of(&data, 1);
        let first = STRIDE as usize;
        tree_rows(
            &mut rows,
            |n| (u32::try_from(first + n).unwrap(), 0),
            &tree,
            &cancel,
        )
        .unwrap();
        let color = |row: &Block| row.runs.first().and_then(|run| run.color);
        let (comment, number) = (
            palette.look_of("comment").color,
            palette.look_of("number").color,
        );
        for line in 256..=262 {
            assert_eq!(color(&rows[line - first]), Some(comment), "line {line}");
        }
        assert_eq!(color(&rows[263 - first]), Some(number), "past the comment");
        assert!(rows[300 - first].runs.iter().any(|run| run.link.is_some()));
        let colors: std::collections::BTreeSet<_> = rows
            .iter()
            .flat_map(|row| row.runs.iter().filter_map(|run| run.color))
            .collect();
        assert!(colors.len() > 2, "{colors:?}");
        // Every byte of every row in a run, none past its text.
        for row in &rows {
            assert_eq!(
                row.runs.iter().map(|run| run.range.len()).sum::<usize>(),
                row.text.len()
            );
        }
        // bat's grammar, fresh from the chunk's start, colors a number.
        let mut fresh = rows_of(&data, 1);
        color_rows(
            &mut fresh,
            |_| true,
            (Some("json"), &palette),
            None,
            None,
            &cancel,
        )
        .unwrap();
        assert_ne!(color(&fresh[0]), Some(comment));
    }

    fn code_blocks(lines: usize, text: impl Fn(usize) -> String) -> Vec<Block> {
        (0..lines)
            .map(|n| Block {
                text: text(n),
                code: true,
                language: Some("toml".into()),
                ..Block::default()
            })
            .collect()
    }

    /// Rows whose time has passed show without colors, their links found
    /// all the same; with time left they are colored, and say where.
    #[test]
    fn rows_past_their_time_stay_plain_with_their_links() {
        let palette = Palette::of(&crate::code_preview::CodeTheme::own(false));
        let line = |_| "# see https://example.invalid/page".to_owned();
        let colors = (Some("toml"), &palette);
        let cancel = CancelToken::new();
        let mut late = code_blocks(3, line);
        let past = Some(Instant::now());
        let stop = color_rows(&mut late, |_| true, colors, None, past, &cancel).unwrap();
        assert!(stop.is_none());
        for row in &late {
            assert!(row.runs.iter().all(|run| run.color.is_none()), "{row:?}");
            assert!(row.runs.iter().any(|run| run.link.is_some()), "{row:?}");
        }
        let mut quick = code_blocks(3, line);
        let ahead = Some(Instant::now() + Duration::from_secs(60));
        let stop = color_rows(&mut quick, |_| true, colors, None, ahead, &cancel).unwrap();
        assert!(stop.is_some());
        assert!(quick[0].runs.iter().any(|run| run.color.is_some()));
    }

    /// A text is parsed before it shows only where that can make it: the
    /// parse is of all of it.
    #[test]
    fn only_a_short_text_is_parsed_before_it_shows() {
        assert!(short_enough_to_parse_soon(PARSED_SOON));
        assert!(!short_enough_to_parse_soon(PARSED_SOON + 1));
    }

    /// A chunk with a line near a row's limit is a megabyte to color: its
    /// rows are not held back for it.
    #[test]
    fn long_lines_are_not_waited_for() {
        let row = |len: usize| {
            vec![Block {
                text: "x".repeat(len),
                ..Block::default()
            }]
        };
        let half = skwir_platform::text_rows::ROW_BYTES / 2;
        assert!(waits_for_colors(&row(half - 1)));
        assert!(!waits_for_colors(&row(half)));
    }

    /// A code view's first chunk comes colored from the worker making it,
    /// where the view goes on from: it is not colored again.
    /// A wait bounded by `soon` is given up on at its time, by the one
    /// thread every such wait shares; one over before it is not.
    #[test]
    fn a_bounded_wait_is_cancelled_at_its_time_unless_it_ended() {
        let cancel = CancelToken::new();
        let (late, _waiting) = limited(&cancel, Duration::from_millis(30));
        let (ended, done) = limited(&cancel, Duration::from_millis(30));
        let (later, _later) = limited(&cancel, Duration::from_secs(600));
        drop(done);
        let deadline = Instant::now() + Duration::from_secs(30);
        while !late.is_cancelled() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(late.is_cancelled(), "cancelled at its time");
        std::thread::sleep(Duration::from_millis(60));
        assert!(!ended.is_cancelled(), "its wait was over first");
        assert!(!later.is_cancelled(), "not before its time");
        assert!(!cancel.is_cancelled(), "only the limit, not what it bounds");
    }

    #[test]
    fn a_code_view_begins_with_its_first_chunk_colored() {
        use skwir_core::inspection::InspectionKind;
        let data = Inspection {
            kind: InspectionKind::Code,
            blocks: code_blocks(2 * STRIDE as usize, |n| format!("key{n} = {n}")),
            ..Inspection::default()
        };
        let with = Colorer {
            language: Some("toml".into()),
            palette: Arc::new(Palette::of(&crate::code_preview::CodeTheme::own(false))),
            grammar: None,
            outlined: false,
        };
        let first = first_colors(&data, &with).expect("colored well within its time");
        let mut colors = CodeColors::default();
        assert!(!colors.begin_with(first), "no parse came with it");
        assert_eq!(colors.shade(0), Shade::Exact);
        assert!(colors.known(1), "the next chunk starts where it stopped");
        assert!(colors.laid(vec![0, 1], data.blocks.len()));
        assert_eq!(colors.next(), Some(Next::Chunk(1)), "the next, not it");
        let row = CodeColors::row(&colors.chunks, 3).unwrap();
        assert!(row.runs.iter().any(|run| run.color.is_some()));
    }

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

    /// The lines a code view drew want their own chunks: a closed fold
    /// between them wants none of what it hides.
    #[test]
    fn a_closed_fold_between_the_lines_drawn_asks_for_nothing_in_it() {
        // Lines 10 to 20 drawn, a fold hiding 21 to 5000, then 5001 to 5010.
        let drawn: Vec<usize> = (10..=20).chain(5001..=5010).collect();
        let (wanted, middle) = chunks_of(&drawn, 10_000, 256);
        // Chunks 0 and 19 drawn, nearest their middle (9) first, then the
        // one after the last; none before the first.
        assert_eq!((wanted, middle), (vec![0, 19, 20], 9));
        assert_eq!(chunks_of(&[300, 301], 10_000, 256).0, [1, 2, 0]);
        assert_eq!(
            chunks_of(&[300], 400, 256).0,
            [1, 0],
            "the last: none after"
        );
        assert!(chunks_of(&[], 400, 256).0.is_empty());
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

    /// Nearest the view first: a plain chunk, or one colored fresh once
    /// where it starts is known; never one colored exactly, nor one that
    /// cannot be colored yet.
    #[test]
    fn the_next_chunk_colored_is_the_nearest_still_to_be() {
        let shades = |chunk| match chunk {
            1 => Some(Shade::Exact),
            2 => Some(Shade::Fresh),
            3 => None,
            _ => Some(Shade::Plain),
        };
        assert_eq!(next_to_color(&[1, 3, 2, 4], shades, |_| true), Some(2));
        assert_eq!(next_to_color(&[1, 3, 2, 4], shades, |_| false), Some(4));
        assert_eq!(next_to_color(&[1, 3], shades, |_| true), None);
    }

    /// The code view wants what is around the lines drawn: a chunk at once,
    /// then, once those are colored, a pass forward from the last chunk
    /// colored exactly to the farthest wanted whose start is not known.
    #[test]
    fn what_a_code_view_colors_next() {
        let mut colors = CodeColors::default();
        assert!(colors.laid((5000..5040).collect(), 100_000));
        assert_eq!(colors.wanted, [19, 20, 18]);
        assert_eq!(colors.next(), Some(Next::Chunk(19)));
        assert!(!colors.laid((5000..5040).collect(), 100_000), "the same");
        // Colored fresh, the three wait for the pass forward.
        let fresh = || {
            Rc::new(Colored {
                rows: Vec::new(),
                exact: false,
            })
        };
        let chunks = Rc::make_mut(&mut colors.chunks);
        for chunk in [18, 19, 20] {
            chunks.insert(chunk, fresh());
        }
        assert_eq!(colors.next(), Some(Next::Forward { first: 0, last: 20 }));
        // The pass reached chunk 21, a start at a time: each chunk wanted
        // is colored again from where it starts, nearest first.
        let palette = Palette::of(&crate::code_preview::CodeTheme::own(false));
        let start = || Highlighter::new(Some("toml"), &palette).stop();
        colors
            .progress
            .lock()
            .extend((1..=21).map(|chunk| (chunk, start())));
        colors.drain();
        assert_eq!(colors.frontier, 22);
        assert_eq!(colors.next(), Some(Next::Chunk(19)));
        let chunks = Rc::make_mut(&mut colors.chunks);
        for chunk in [18, 19, 20] {
            chunks.insert(
                chunk,
                Rc::new(Colored {
                    rows: Vec::new(),
                    exact: true,
                }),
            );
        }
        assert_eq!(colors.next(), None, "all colored exactly");
    }

    /// Colored a chunk at a time, each from where the one before stopped,
    /// a code view is colored as it was whole, links and all, a TOML string
    /// of several lines across the boundaries between chunks included;
    /// colored fresh, the lines inside it are not.
    #[test]
    fn chunks_colored_one_after_another_are_the_text_colored_whole() {
        use skwir_core::inspection::InspectionKind;
        let lines = 3 * STRIDE as usize + 40;
        let text = |n: usize| match n {
            250 | 510 => "text = \"\"\"".to_owned(),
            262 | 520 => "\"\"\"".to_owned(),
            n if n.is_multiple_of(5) => format!("# a comment, see https://example.invalid/{n}"),
            n => format!("key{n} = {n}"),
        };
        let data = Inspection {
            kind: InspectionKind::Code,
            blocks: (0..lines)
                .map(|n| Block {
                    text: text(n),
                    code: true,
                    language: Some("toml".into()),
                    ..Block::default()
                })
                .collect(),
            ..Inspection::default()
        };
        let palette = Palette::of(&crate::code_preview::CodeTheme::own(false));
        let mut whole = data.clone();
        skwir_platform::highlight::highlight(&mut whole, &palette);
        let runs = |block: &Block| format!("{:?}", block.runs);
        let cancel = CancelToken::new();
        let mut from = None;
        for chunk in 0..4 {
            let mut rows = rows_of(&data, chunk);
            let colors = (Some("toml"), &palette);
            from = color_rows(&mut rows, |_| true, colors, from, None, &cancel).unwrap();
            assert!(from.is_some());
            for (n, row) in rows.iter().enumerate() {
                let line = chunk as usize * STRIDE as usize + n;
                assert_eq!(runs(row), runs(&whole.blocks[line]), "line {line}");
            }
        }
        assert!(whole.blocks[255].runs.iter().any(|run| run.link.is_some()));
        // Line 258 is inside the string the line 250 opened: colored fresh
        // from chunk 1's start, it is a key again.
        let mut fresh = rows_of(&data, 1);
        color_rows(
            &mut fresh,
            |_| true,
            (Some("toml"), &palette),
            None,
            None,
            &cancel,
        )
        .unwrap();
        assert_ne!(runs(&fresh[258 - 256]), runs(&whole.blocks[258]));
        assert_eq!(rows_of(&data, 3).len(), 40, "the last chunk, short");
        assert!(rows_of(&data, 9).is_empty(), "past the end: none");
    }
}
