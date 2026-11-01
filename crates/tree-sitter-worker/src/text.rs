//! The texts open: each colored whole when it is opened, its spans kept by
//! line, its folds and outline found then too, and the text and its tree
//! let go of.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Context;
use skwir_core::syntax::{Grammar, MAX_LINES, MAX_TEXT, Reply, Request, Span, Symbol};
use tree_sitter::{Node, ParseOptions, Parser, Query, QueryCursor, StreamingIterator, Tree};
use tree_sitter_highlight::{Highlight, HighlightEvent, Highlighter};

use crate::grammars::{Grammars, Loaded};

/// Definitions an outline lists at most.
const SYMBOLS: usize = 100_000;
/// Characters of a definition's name kept.
const NAME: usize = 200;

pub struct Worker {
    grammars: Grammars,
    highlighter: Highlighter,
    /// Whether the highlighter's parser has a store to run WebAssembly
    /// grammars in.
    stored: bool,
    documents: HashMap<u32, Document>,
    /// Cancels waiting to be answered (`serve`): what runs gives up while
    /// one does.
    cancels: Arc<AtomicUsize>,
}

/// What is kept of a text opened.
struct Document {
    /// Where each line's spans begin in `spans`, and where the last ends.
    first: Vec<u32>,
    spans: Vec<Span>,
    folds: Vec<(u32, u32)>,
    outline: Vec<Symbol>,
}

impl Worker {
    pub fn new(cancels: Arc<AtomicUsize>) -> Self {
        Worker {
            grammars: Grammars::new(),
            highlighter: Highlighter::new(),
            stored: false,
            documents: HashMap::new(),
            cancels,
        }
    }

    /// After a panic: a parser that may be in the middle of something.
    pub fn recover(&mut self) {
        self.highlighter = Highlighter::new();
        self.stored = false;
    }

    pub fn handle(&mut self, request: Request) -> anyhow::Result<Reply> {
        match request {
            Request::Grammars(list) => {
                self.set_grammars(list);
                Ok(Reply::Done)
            }
            Request::Check { name } => {
                let index = self.grammars.find(&name)?;
                let loaded = self.grammars.load(index)?;
                Ok(Reply::Checked {
                    abi: loaded.abi,
                    problems: loaded
                        .problems
                        .iter()
                        .chain(&loaded.colorless)
                        .cloned()
                        .collect(),
                })
            }
            Request::Open { doc, name, text } => {
                // In place of what it held: let go of first.
                self.documents.remove(&doc);
                let (document, lines, captures) = self.open(&name, &text)?;
                self.documents.insert(doc, document);
                Ok(Reply::Opened { lines, captures })
            }
            Request::Colors { doc, from, to } => {
                anyhow::ensure!(
                    to.saturating_sub(from) <= MAX_LINES,
                    "more than {MAX_LINES} lines asked"
                );
                let document = self.document(doc)?;
                Ok(Reply::Colors {
                    lines: document.colors(from, to),
                })
            }
            Request::Folds { doc } => Ok(Reply::Folds(self.document(doc)?.folds.clone())),
            Request::Outline { doc } => Ok(Reply::Outline(self.document(doc)?.outline.clone())),
            Request::Close { doc } => {
                self.documents.remove(&doc);
                Ok(Reply::Done)
            }
            // Answered by `serve`, which counts it.
            Request::Cancel => Ok(Reply::Error("cancelled".into())),
        }
    }

    fn set_grammars(&mut self, list: Vec<Grammar>) {
        self.grammars.set(list);
    }

    fn document(&self, doc: u32) -> anyhow::Result<&Document> {
        self.documents
            .get(&doc)
            .with_context(|| format!("no document {doc}"))
    }

    fn cancelled(&self) -> bool {
        self.cancels.load(Ordering::SeqCst) != 0
    }

    /// The text at `path`, read and colored whole with the grammar `name`:
    /// what is kept of it, its lines and its capture names.
    fn open(&mut self, name: &str, path: &Path) -> anyhow::Result<(Document, u32, Vec<String>)> {
        let index = self.grammars.find(name)?;
        let loaded = self.grammars.load(index)?;
        // Colored by nothing, the text is bat's grammar's to color.
        if let Some(why) = &loaded.colorless {
            anyhow::bail!("{name} colors nothing: {why}");
        }
        let text = read_text(path)?;
        anyhow::ensure!(!self.cancelled(), "cancelled");
        if !self.stored && self.grammars.any_wasm() {
            let store = tree_sitter::WasmStore::new(self.grammars.engine())
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            self.highlighter
                .parser()
                .set_wasm_store(store)
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            self.stored = true;
        }
        let lines = Lines::of(text.as_bytes());
        let mut spans = Colored::new(&lines);
        let mut highlights = Vec::new();
        {
            let Worker {
                grammars,
                highlighter,
                cancels,
                ..
            } = self;
            let events = highlighter
                .highlight(
                    &loaded.highlight,
                    text.as_bytes(),
                    None,
                    Some(cancels),
                    |injected| grammars.injected(injected),
                )
                .map_err(highlight_error)?;
            // Each highlight's index into the document's captures.
            let mut local: Vec<Option<u16>> = Vec::new();
            let mut open: Vec<Option<u16>> = Vec::new();
            for event in events {
                match event.map_err(highlight_error)? {
                    HighlightEvent::HighlightStart(Highlight(highlight)) => {
                        if local.len() <= highlight {
                            local.resize(highlight + 1, None);
                        }
                        if local[highlight].is_none() {
                            local[highlight] = u16::try_from(highlights.len()).ok();
                            highlights.push(highlight);
                        }
                        open.push(local[highlight]);
                    }
                    HighlightEvent::HighlightEnd => {
                        open.pop();
                    }
                    HighlightEvent::Source { start, end } => {
                        if let Some(Some(capture)) = open.last() {
                            spans.add(start, end, *capture);
                        }
                    }
                }
            }
        }
        let captures = highlights
            .into_iter()
            .take(usize::from(u16::MAX) + 1)
            .map(|highlight| self.grammars.capture(highlight).to_owned())
            .collect();
        let (first, spans) = spans.finish();
        let (folds, outline) = if loaded.folds.is_some() || loaded.outline.is_some() {
            let tree = self.parse(loaded, &text)?;
            let folds = loaded.folds.as_ref().map_or_else(Vec::new, |query| {
                folds(query, &tree, text.as_bytes(), &self.cancels)
            });
            let outline = loaded.outline.as_ref().map_or_else(Vec::new, |query| {
                outline(query, &tree, text.as_bytes(), &self.cancels)
            });
            anyhow::ensure!(!self.cancelled(), "cancelled");
            (folds, outline)
        } else {
            (Vec::new(), Vec::new())
        };
        let count = u32::try_from(lines.count()).context("too many lines")?;
        Ok((
            Document {
                first,
                spans,
                folds,
                outline,
            },
            count,
            captures,
        ))
    }

    /// The text's tree, for its folds and outline: highlighting kept none.
    fn parse(&mut self, loaded: &Loaded, text: &str) -> anyhow::Result<Tree> {
        let parser: &mut Parser = self.highlighter.parser();
        // The highlighter left the ranges of the last language it parsed.
        parser
            .set_included_ranges(&[])
            .map_err(|_| anyhow::anyhow!("the parser keeps its ranges"))?;
        parser
            .set_language(&loaded.language)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let cancels = &self.cancels;
        let mut progress = |_: &tree_sitter::ParseState| {
            if cancels.load(Ordering::SeqCst) == 0 {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        };
        let bytes = text.as_bytes();
        parser
            .parse_with_options(
                &mut |at, _| bytes.get(at..).unwrap_or_default(),
                None,
                Some(ParseOptions::new().progress_callback(&mut progress)),
            )
            .context("cancelled")
    }
}

fn highlight_error(error: tree_sitter_highlight::Error) -> anyhow::Error {
    match error {
        tree_sitter_highlight::Error::Cancelled => anyhow::anyhow!("cancelled"),
        error => anyhow::anyhow!("{error}"),
    }
}

/// The text skwir wrote: a regular file, UTF-8, no larger than `MAX_TEXT`.
fn read_text(path: &Path) -> anyhow::Result<String> {
    let bytes = skwir_core::fs::file::read_regular(path, MAX_TEXT).map_err(|error| {
        if error.kind() == std::io::ErrorKind::InvalidData {
            anyhow::anyhow!("the text exceeds {} MiB", MAX_TEXT >> 20)
        } else {
            anyhow::anyhow!("read the text: {error}")
        }
    })?;
    String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("the text is not UTF-8"))
}

impl Document {
    /// The spans of lines `from..to`, those past the last left out.
    fn colors(&self, from: u32, to: u32) -> Vec<Vec<Span>> {
        let lines = self.first.len().saturating_sub(1);
        let to = (to as usize).min(lines);
        let from = (from as usize).min(to);
        (from..to)
            .map(|line| {
                self.spans[self.first[line] as usize..self.first[line + 1] as usize].to_vec()
            })
            .collect()
    }
}

/// Where each line of a text begins, as `str::lines` counts them: a last
/// line ending, or none, makes no line of its own.
struct Lines<'t> {
    text: &'t [u8],
    starts: Vec<usize>,
}

impl<'t> Lines<'t> {
    fn of(text: &'t [u8]) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.iter()
                .enumerate()
                .filter(|(_, byte)| **byte == b'\n')
                .map(|(at, _)| at + 1),
        );
        if starts.last() == Some(&text.len()) {
            starts.pop();
        }
        Lines { text, starts }
    }

    fn count(&self) -> usize {
        self.starts.len()
    }

    /// Where line `line` begins and its text ends, its ending (`\n` or
    /// `\r\n`) left out, and where the next begins.
    fn bounds(&self, line: usize) -> (usize, usize, usize) {
        let start = self.starts[line];
        let next = self
            .starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len());
        let mut end = next;
        if end > start && self.text[end - 1] == b'\n' {
            end -= 1;
            if end > start && self.text[end - 1] == b'\r' {
                end -= 1;
            }
        }
        (start, end, next)
    }
}

/// Spans built line by line from the highlighter's runs, which come in
/// the text's order.
struct Colored<'l, 't> {
    lines: &'l Lines<'t>,
    /// The line the last run began on.
    line: usize,
    first: Vec<u32>,
    spans: Vec<Span>,
}

impl<'l, 't> Colored<'l, 't> {
    fn new(lines: &'l Lines<'t>) -> Self {
        Colored {
            lines,
            line: 0,
            first: Vec::with_capacity(lines.count() + 1),
            spans: Vec::new(),
        }
    }

    /// Bytes `start..end` of the text colored by `capture`, split at line
    /// ends, its line endings left out.
    fn add(&mut self, start: usize, end: usize, capture: u16) {
        let count = self.lines.count();
        while self.line + 1 < count && self.lines.starts[self.line + 1] <= start {
            self.line += 1;
        }
        let mut line = self.line;
        while line < count {
            let (line_start, line_end, next) = self.lines.bounds(line);
            let (from, to) = (start.max(line_start), end.min(line_end));
            if from < to {
                self.push(
                    line,
                    (from - line_start) as u32,
                    (to - line_start) as u32,
                    capture,
                );
            }
            if end <= next {
                break;
            }
            line += 1;
        }
    }

    fn push(&mut self, line: usize, start: u32, end: u32, capture: u16) {
        // Lines before it, and it, begin here.
        while self.first.len() <= line {
            self.first.push(self.spans.len() as u32);
        }
        if self.first.len() == line + 1
            && self.spans.len() > self.first[line] as usize
            && let Some(last) = self.spans.last_mut()
            && last.end == start
            && last.capture == capture
        {
            last.end = end;
            return;
        }
        self.spans.push(Span {
            start,
            end,
            capture,
        });
    }

    /// Each line's first span, and the spans.
    fn finish(mut self) -> (Vec<u32>, Vec<Span>) {
        while self.first.len() <= self.lines.count() {
            self.first.push(self.spans.len() as u32);
        }
        self.spans.shrink_to_fit();
        (self.first, self.spans)
    }
}

/// Every capture named `name` of `query`'s matches in `tree`, a test of
/// `cancels` between matches.
fn each_match(
    query: &Query,
    tree: &Tree,
    text: &[u8],
    cancels: &AtomicUsize,
    mut each: impl FnMut(&tree_sitter::QueryMatch<'_, '_>) -> ControlFlow<()>,
) {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), text);
    let mut seen = 0usize;
    while let Some(found) = matches.next() {
        seen += 1;
        if seen.is_multiple_of(256) && cancels.load(Ordering::SeqCst) != 0 {
            return;
        }
        if each(found).is_break() {
            return;
        }
    }
}

/// The line a node ends on: a node that ends at a line's start (one that
/// takes its line ending) ends on the line before.
fn last_line(node: &Node<'_>) -> usize {
    let end = node.end_position();
    if end.column == 0 && end.row > node.start_position().row {
        end.row - 1
    } else {
        end.row
    }
}

/// The folds of a folds query (`@fold`): from a node's first line to its
/// last, kept when that is two lines at least; one for each line it
/// begins on (the widest), nested or apart, in order: one that crosses
/// another is left out.
fn folds(query: &Query, tree: &Tree, text: &[u8], cancels: &AtomicUsize) -> Vec<(u32, u32)> {
    let Some(fold) = query.capture_index_for_name("fold") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    each_match(query, tree, text, cancels, |matched| {
        for capture in matched.captures().iter().filter(|c| c.index == fold) {
            let (start, end) = (capture.node.start_position().row, last_line(&capture.node));
            if end > start
                && let (Ok(start), Ok(end)) = (u32::try_from(start), u32::try_from(end))
            {
                found.push((start, end));
            }
        }
        ControlFlow::Continue(())
    });
    nest(found)
}

/// `folds` sorted by start, the widest of each start kept, those crossing
/// one before them left out.
fn nest(mut folds: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    folds.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    folds.dedup_by_key(|fold| fold.0);
    let mut open: Vec<u32> = Vec::new();
    folds.retain(|&(start, end)| {
        while open.last().is_some_and(|&last| last <= start) {
            open.pop();
        }
        if open.last().is_some_and(|&last| end > last) {
            return false;
        }
        open.push(end);
        true
    });
    folds
}

/// The definitions of an outline query: `tags.scm`'s (`@definition.<kind>`
/// with its `@name`) or Zed's `outline.scm` (`@item` with its `@name`s),
/// in the text's order, each with how many hold it.
fn outline(query: &Query, tree: &Tree, text: &[u8], cancels: &AtomicUsize) -> Vec<Symbol> {
    let names = query.capture_names();
    let kind_of = |index: u32| -> Option<&str> {
        let capture = names[index as usize];
        match capture.strip_prefix("definition.") {
            Some(kind) => Some(kind.rsplit('.').next().unwrap_or(kind)),
            None => (capture == "item").then_some("item"),
        }
    };
    let name = query.capture_index_for_name("name");
    // Each definition's node, kind and name; one node once.
    let mut found: Vec<(std::ops::Range<usize>, usize, String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    each_match(query, tree, text, cancels, |matched| {
        let Some((node, kind)) = matched
            .captures()
            .iter()
            .find_map(|capture| Some((capture.node, kind_of(capture.index)?)))
        else {
            return ControlFlow::Continue(());
        };
        let said: Vec<&str> = matched
            .captures()
            .iter()
            .filter(|capture| Some(capture.index) == name)
            .filter_map(|capture| capture.node.utf8_text(text).ok())
            .collect();
        if said.is_empty() || !seen.insert(node.byte_range()) {
            return ControlFlow::Continue(());
        }
        let said = said.join(" ");
        let said: String = said
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(NAME)
            .collect();
        found.push((
            node.byte_range(),
            node.start_position().row,
            kind.to_owned(),
            said,
        ));
        if found.len() >= SYMBOLS {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    found.sort_by(|a, b| a.0.start.cmp(&b.0.start).then(b.0.end.cmp(&a.0.end)));
    let mut holding: Vec<usize> = Vec::new();
    found
        .into_iter()
        .map(|(range, line, kind, name)| {
            while holding.last().is_some_and(|&end| end <= range.start) {
                holding.pop();
            }
            let depth = holding.len() as u32;
            holding.push(range.end);
            Symbol {
                name,
                kind,
                line: u32::try_from(line).unwrap_or(u32::MAX),
                depth,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_of(text: &str, runs: &[(usize, usize, u16)]) -> Vec<Vec<(u32, u32, u16)>> {
        let lines = Lines::of(text.as_bytes());
        let mut spans = Colored::new(&lines);
        for &(start, end, capture) in runs {
            spans.add(start, end, capture);
        }
        let (first, spans) = spans.finish();
        let document = Document {
            first,
            spans,
            folds: Vec::new(),
            outline: Vec::new(),
        };
        document
            .colors(0, u32::MAX)
            .into_iter()
            .map(|line| line.iter().map(|s| (s.start, s.end, s.capture)).collect())
            .collect()
    }

    #[test]
    fn runs_are_split_at_line_ends_their_endings_left_out() {
        // "ab\r\n", "cd\n", "", "ef"
        let text = "ab\r\ncd\n\nef";
        assert_eq!(
            spans_of(text, &[(1, 6, 3), (6, 7, 3), (8, 10, 1)]),
            vec![vec![(1, 2, 3)], vec![(0, 2, 3)], vec![], vec![(0, 2, 1)]]
        );
        // Runs that meet with one capture are one span.
        assert_eq!(
            spans_of("abcd", &[(0, 1, 2), (1, 2, 2), (2, 3, 4)]),
            vec![vec![(0, 2, 2), (2, 3, 4)]]
        );
        // As `str::lines` counts them.
        for text in ["", "a", "a\n", "a\n\n", "\n", "a\r\nb"] {
            assert_eq!(
                Lines::of(text.as_bytes()).count(),
                text.lines().count(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn folds_are_nested_or_apart() {
        assert_eq!(
            nest(vec![(4, 6), (0, 9), (0, 3), (2, 5), (9, 12), (10, 11)]),
            // (0, 3) is narrower than (0, 9) on its line; (4, 6) crosses (2, 5).
            vec![(0, 9), (2, 5), (9, 12), (10, 11)]
        );
    }
}
