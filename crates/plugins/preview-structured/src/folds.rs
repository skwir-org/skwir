//! Fold boundaries come from parsers, never indentation guesses inside strings.
use bibli_core::CancelToken;
use bibli_platform::inspection::{check, limits};
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

/// JSON's delimiters, read from text the caller has already parsed as JSON
/// (or produced by printing a parsed value): its only place for a stray
/// bracket is a string, which the lexer below steps over, so no second
/// parse is needed to trust them.
fn json(text: &str, cancel: &CancelToken) -> anyhow::Result<Vec<Range<usize>>> {
    let mut folds = Vec::new();
    let mut stack = Vec::new();
    let (mut quoted, mut escaped) = (false, false);
    let mut line = 0;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            check(cancel)?;
        }
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else {
            match ch {
                '"' => quoted = true,
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
