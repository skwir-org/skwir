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
//! wanted is colored exactly. Its first paint waits for none of it: the
//! lines show plain, their links found, and their colors follow.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::Context;
use parking_lot::Mutex;
use skwir_core::cancel::CancelToken;
use skwir_core::inspection::{Block, Inspection};
use skwir_core::scheduler::{Lane, Priority};

use crate::code_preview::{Highlighter, Palette, Resume};
use crate::latest::Latest;
use crate::preview::PreviewContent;
use crate::shell::Shell;

/// Chunks kept around the view: a few screens either way.
pub(crate) const KEPT: usize = 24;
/// Rows colored at once.
pub(crate) const STRIDE: u64 = skwir_platform::text_rows::STRIDE;

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
/// (`link_block`). Where the colors stopped, to go on from.
pub(crate) fn color_rows(
    rows: &mut [Block],
    ends: impl Fn(usize) -> bool,
    (language, palette): (Option<&str>, &Palette),
    from: Option<Resume>,
    cancel: &CancelToken,
) -> anyhow::Result<Resume> {
    let mut colors = Highlighter::resume(language, palette, from);
    for (n, row) in rows.iter_mut().enumerate() {
        if n % 64 == 0 {
            skwir_platform::inspection::check(cancel)?;
        }
        row.runs = colors.row(&row.text, ends(n));
        skwir_platform::links::link_block(row, true);
    }
    Ok(colors.stop())
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
}

/// The rows of a chunk of a code view, colored, and how.
pub(crate) struct Colored {
    pub rows: Vec<Block>,
    exact: bool,
}

enum Reply {
    /// A chunk colored, and where its colors stopped.
    Chunk {
        chunk: u64,
        rows: Vec<Block>,
        exact: bool,
        stop: Resume,
    },
    /// A pass forward ended, with the rows of the chunks it was asked to
    /// keep; where each chunk stopped went in `progress` as it went.
    Forward { kept: Vec<(u64, Vec<Block>)> },
}

/// What is on its way.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Asked {
    Chunk(u64),
    Forward,
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
            };
            if !given_way {
                return;
            }
            self.asked.forget();
            self.asking = None;
            self.drain();
        }
        let Some(next) = self.next() else {
            return;
        };
        let (data, with) = (Arc::clone(data), with.clone());
        match next {
            Next::Chunk(chunk) => {
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
                        let Ok(stop) = color_rows(&mut rows, |_| true, palette, from, cancel)
                        else {
                            return Reply::Forward { kept: Vec::new() };
                        };
                        Reply::Chunk {
                            chunk,
                            rows,
                            exact,
                            stop,
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
                            let Ok(stop) = color_rows(&mut rows, |_| true, palette, from, cancel)
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
                start = exact.then_some((chunk + 1, stop));
                true
            }
            Reply::Forward { kept } => {
                let changed = !kept.is_empty();
                for (chunk, rows) in kept {
                    chunks.insert(chunk, Rc::new(Colored { rows, exact: true }));
                }
                changed
            }
        };
        let_go(chunks, &self.wanted, self.middle);
        if let Some((chunk, stop)) = start {
            self.started(chunk, stop);
        }
        changed
    }
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
        let changed = colors.take();
        if let Some(PreviewContent::Inspection(display)) = &preview.content
            && let Some(with) = display.colors.as_ref()
        {
            colors.request(&self.app, &display.data, with);
        }
        if changed {
            cx.notify();
        }
    }
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
        crate::code_preview::highlight(&mut whole, &palette);
        let runs = |block: &Block| format!("{:?}", block.runs);
        let cancel = CancelToken::new();
        let mut from = None;
        for chunk in 0..4 {
            let mut rows = rows_of(&data, chunk);
            let stop = color_rows(&mut rows, |_| true, (Some("toml"), &palette), from, &cancel);
            from = Some(stop.unwrap());
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
            &cancel,
        )
        .unwrap();
        assert_ne!(runs(&fresh[258 - 256]), runs(&whole.blocks[258]));
        assert_eq!(rows_of(&data, 3).len(), 40, "the last chunk, short");
        assert!(rows_of(&data, 9).is_empty(), "past the end: none");
    }
}
