//! Fold boundaries come from parsers, never indentation guesses inside strings.
use skwir_core::CancelToken;
use skwir_platform::inspection::{check, limits};
use std::ops::Range;

pub fn parse(
    text: &str,
    language: &str,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<Range<usize>>> {
    let mut folds = match language {
        "toml" => toml(text, cancel)?,
        "yaml" | "yml" => yaml(text, cancel)?,
        _ => json(text, cancel)?,
    };
    folds.retain(|r| r.end > r.start + 1);
    folds.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
    folds.dedup_by_key(|r| r.start);
    // Keep only nested/disjoint ranges so a collapsed region cannot hide a
    // sibling's end marker. Parsers can emit coincident containers on a line.
    let mut ends = Vec::new();
    folds.retain(|r| {
        while ends.last().is_some_and(|&end| end <= r.start) {
            ends.pop();
        }
        if ends.last().is_some_and(|&end| r.end > end) {
            return false;
        }
        ends.push(r.end);
        true
    });
    Ok(folds)
}

/// Where the lexer is in JSON, JSONC or JSON5 text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lexed {
    Code,
    /// In a string opened by this quote (JSON5 also has `'`).
    Quoted(char),
    /// After a backslash in a string opened by this quote.
    Escaped(char),
    /// After a `/` that may open a comment.
    Slash,
    Line,
    Block,
    /// After a `*` in a block comment.
    Star,
}

impl Lexed {
    /// The state after `ch`, and whether `ch` is a token: code outside
    /// strings and comments, or the quote that opens a string. A `/` that
    /// opened no comment is neither.
    fn next(self, ch: char) -> (Self, bool) {
        match (self, ch) {
            (Self::Code | Self::Slash, '"' | '\'') => (Self::Quoted(ch), true),
            (Self::Code, '/') => (Self::Slash, false),
            (Self::Slash, '/') => (Self::Line, false),
            (Self::Slash, '*') => (Self::Block, false),
            (Self::Code | Self::Slash, _) => (Self::Code, true),
            (Self::Quoted(quote), '\\') => (Self::Escaped(quote), false),
            (Self::Quoted(quote), _) if ch == quote => (Self::Code, false),
            (Self::Quoted(quote) | Self::Escaped(quote), _) => (Self::Quoted(quote), false),
            (Self::Line, '\n') | (Self::Star, '/') => (Self::Code, false),
            (Self::Line, _) => (Self::Line, false),
            (Self::Block | Self::Star, '*') => (Self::Star, false),
            (Self::Block | Self::Star, _) => (Self::Block, false),
        }
    }
}

/// JSONC as JSON: comments blanked (their line breaks kept) and trailing
/// commas dropped, to check that what is shown as written is JSON else.
pub fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut state = Lexed::Code;
    for ch in text.chars() {
        let (next, _) = state.next(ch);
        match (state, next) {
            (Lexed::Slash, Lexed::Line | Lexed::Block) => out.push_str("  "),
            (Lexed::Slash, _) => {
                out.push('/');
                out.push(ch);
            }
            (_, Lexed::Slash) => {}
            (Lexed::Line | Lexed::Block | Lexed::Star, _) if ch != '\n' => out.push(' '),
            _ => out.push(ch),
        }
        state = next;
    }
    if state == Lexed::Slash {
        out.push('/');
    }
    // A comma whose next token closes its object or array is a trailing
    // comma.
    let mut kept = String::with_capacity(out.len());
    let mut comma = None;
    let mut state = Lexed::Code;
    for ch in out.chars() {
        let (next, token) = state.next(ch);
        state = next;
        if token && !ch.is_whitespace() {
            if let Some(at) = comma.take()
                && matches!(ch, '}' | ']')
            {
                kept.replace_range(at..=at, " ");
            }
            if ch == ',' {
                comma = Some(kept.len());
            }
        }
        kept.push(ch);
    }
    kept
}

/// JSON's delimiters (JSONC's and JSON5's too), read lexically: a stray
/// bracket can only be in a string or a comment, which the lexer steps
/// over, so no parse is needed to trust them.
fn json(text: &str, cancel: &CancelToken) -> anyhow::Result<Vec<Range<usize>>> {
    let mut folds = Vec::new();
    let mut stack = Vec::new();
    let mut state = Lexed::Code;
    let mut line = 0;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            check(cancel)?;
        }
        let (next, token) = state.next(ch);
        state = next;
        if token {
            match ch {
                '{' | '[' => {
                    anyhow::ensure!(
                        stack.len() < limits::NESTING,
                        "Code preview exceeds {} levels",
                        limits::NESTING
                    );
                    stack.push(line);
                }
                '}' | ']' => {
                    if let Some(start) = stack.pop() {
                        // Leave the closing delimiter visible.
                        folds.push(start..line);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(folds)
}

fn toml(text: &str, cancel: &CancelToken) -> anyhow::Result<Vec<Range<usize>>> {
    let document = toml_edit::Document::parse(text)?;
    let starts: Vec<_> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line = |offset: usize| starts.partition_point(|&i| i <= offset).saturating_sub(1);
    let mut folds = Vec::new();
    let mut items = vec![(document.as_item(), 0)];
    let mut values = Vec::new();
    let mut headers = Vec::new();
    while let Some((item, depth)) = items.pop() {
        check(cancel)?;
        anyhow::ensure!(
            depth <= limits::NESTING,
            "Code preview exceeds {} levels",
            limits::NESTING
        );
        match item {
            toml_edit::Item::Table(table) => {
                if depth > 0
                    && !table.is_implicit()
                    && let Some(span) = table.span()
                {
                    headers.push(line(span.start));
                }
                items.extend(table.iter().map(|(_, item)| (item, depth + 1)));
            }
            toml_edit::Item::ArrayOfTables(tables) => {
                items.extend(tables.iter().flat_map(|table| {
                    if let Some(span) = table.span() {
                        headers.push(line(span.start));
                    }
                    table.iter().map(move |(_, item)| (item, depth + 1))
                }));
            }
            toml_edit::Item::Value(value) => values.push((value, depth)),
            toml_edit::Item::None => {}
        }
    }
    while let Some((value, depth)) = values.pop() {
        check(cancel)?;
        anyhow::ensure!(
            depth <= limits::NESTING,
            "Code preview exceeds {} levels",
            limits::NESTING
        );
        if let Some(span) = value.span() {
            folds.push(line(span.start)..line(span.end.saturating_sub(1)));
        }
        match value {
            toml_edit::Value::Array(array) => values.extend(array.iter().map(|v| (v, depth + 1))),
            toml_edit::Value::InlineTable(table) => {
                values.extend(table.iter().map(|(_, v)| (v, depth + 1)));
            }
            _ => {}
        }
    }
    headers.sort_unstable();
    headers.dedup();
    headers.push(starts.len());
    folds.extend(headers.windows(2).map(|pair| pair[0]..pair[1]));
    Ok(folds)
}

fn yaml(text: &str, cancel: &CancelToken) -> anyhow::Result<Vec<Range<usize>>> {
    use yaml_rust2::parser::{Event, Parser};
    let mut parser = Parser::new_from_str(text);
    let mut folds = Vec::new();
    let mut stack = Vec::new();
    let mut previous_scalar = None;
    loop {
        check(cancel)?;
        let (event, mark) = parser.next_token()?;
        let line = mark.line().saturating_sub(1);
        match event {
            Event::StreamEnd => break,
            Event::MappingStart(..) | Event::SequenceStart(..) => {
                anyhow::ensure!(
                    stack.len() < limits::NESTING,
                    "Code preview exceeds {} levels",
                    limits::NESTING
                );
                let start = previous_scalar
                    .take()
                    .filter(|&(row, col)| row < line && col < mark.col())
                    .map_or(line, |(row, _)| row);
                stack.push(start);
            }
            Event::MappingEnd | Event::SequenceEnd => {
                if let Some(start) = stack.pop() {
                    // Root collection has no separate header to collapse.
                    if !stack.is_empty() {
                        folds.push(start..line);
                    }
                }
                previous_scalar = None;
            }
            Event::Scalar(..) => previous_scalar = Some((line, mark.col())),
            _ => previous_scalar = None,
        }
    }
    Ok(folds)
}
