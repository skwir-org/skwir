//! Formulas a text writes between delimiters, found as the program that
//! typesets them on its page finds them, and taken out of it, each replaced
//! by a placeholder (`MARK`, its number, `END`) found again where the text
//! is read (`Taken`, `split`). A formula's delimiters match and its braces
//! balance. Its rules (`Rules`) are Jupyter's for a notebook's Markdown
//! (`JUPYTER`, JupyterLab's `removeMath` in
//! `packages/rendermime/src/latex.ts`: the backslash of `\\(` doubled, as
//! Markdown keeps one; no formula across a blank line; no `$` of code), or
//! MathJax's and KaTeX's for an HTML page that loads them (`mathjax`): its
//! text's own `\(…\)` and `\[…\]`, `$…$` where the page asks for it.
//! `$$…$$` and `\begin{…}…\end{…}` are formulas under both.

/// Which delimiters a text writes its formulas between, and what ends one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rules {
    /// A notebook's Markdown cell, as Jupyter reads it.
    Jupyter,
    /// An HTML page's text, as MathJax and KaTeX's auto-render read it:
    /// `$…$` only where the page asks for it (`dollars`).
    Page { dollars: bool },
}

/// A notebook's Markdown cell, as Jupyter reads it.
pub const JUPYTER: Rules = Rules::Jupyter;

/// An HTML page's text, as MathJax and KaTeX's auto-render read it.
pub fn mathjax(dollars: bool) -> Rules {
    Rules::Page { dollars }
}

impl Rules {
    /// `$…$` is a formula (`$$…$$` always is).
    fn dollars(self) -> bool {
        match self {
            Self::Jupyter => true,
            Self::Page { dollars } => dollars,
        }
    }

    /// `\(` and `\[` are written with their backslash doubled, as Markdown
    /// keeps one; otherwise with one, as HTML's text has it.
    fn doubled(self) -> bool {
        self == Self::Jupyter
    }

    /// A blank line ends a formula open (a paragraph), and a `$` in a code
    /// span or a fenced block is not one: Markdown.
    fn markdown(self) -> bool {
        self == Self::Jupyter
    }
}

/// What opens a placeholder: a private character, which no text writes
/// as one (one it holds is taken as written, `Taken::Written`).
pub const MARK: char = '\u{e000}';
/// What closes a placeholder.
pub const END: char = '\u{e001}';

/// A piece of a text taken out before it is read.
#[derive(Clone, Debug, PartialEq)]
pub enum Taken {
    /// A formula, and how it was written (what it reads as, and is shown
    /// as where it cannot be drawn).
    Formula {
        tex: String,
        display: bool,
        written: String,
    },
    /// Text that is the text's own: a private character it wrote.
    Written(String),
}

impl Taken {
    /// What it reads as in the text.
    pub fn written(&self) -> &str {
        match self {
            Self::Formula { written, .. } | Self::Written(written) => written,
        }
    }
}

/// A delimiter or a sign the rules look for, and where it is in the text.
#[derive(Clone, Debug, PartialEq)]
enum Sign {
    Dollar,
    Dollars,
    /// `\begin{name}` or `\end{name}`, as written.
    Environment(String),
    /// `\(`, `\)`, `\[` or `\]` (`\\(`… where `Rules::doubled`).
    Bracket(char),
    Open,
    Close,
    /// Line breaks and the spaces between them: two or more end a
    /// paragraph, and any formula open.
    Breaks(usize),
    /// What stands for itself: an escaped `{`, `}` or `$`, a backslash
    /// escaped (a TeX line break) where a bracket has one.
    Escaped,
    Private,
}

/// The signs of `text` under `rules`, with their byte ranges. A `$` within
/// `code` (spans and fenced blocks, in order) is none.
fn signs(
    text: &str,
    rules: Rules,
    code: &[std::ops::Range<usize>],
) -> Vec<(Sign, std::ops::Range<usize>)> {
    let bytes = text.as_bytes();
    // The code ranges come in order, as the signs do: one walks with them.
    let mut code = code.iter().peekable();
    let mut in_code = |at: usize| {
        while code.next_if(|range| range.end <= at).is_some() {}
        code.peek().is_some_and(|range| range.contains(&at))
    };
    let bracket = |at: usize| match bytes.get(at).copied() {
        Some(bracket @ (b'(' | b')' | b'[' | b']')) => Some(char::from(bracket)),
        _ => None,
    };
    let mut signs = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let rest = &text[at..];
        let found = match bytes[at] {
            b'$' if in_code(at) => None,
            b'$' if rest.starts_with("$$") => Some((Sign::Dollars, 2)),
            b'$' => Some((Sign::Dollar, 1)),
            b'\\' => {
                let next = bytes.get(at + 1).copied();
                if matches!(next, Some(b'{' | b'}' | b'$')) {
                    Some((Sign::Escaped, 2))
                } else if rules.doubled() && next == Some(b'\\') {
                    bracket(at + 2).map(|bracket| (Sign::Bracket(bracket), 3))
                } else if !rules.doubled() && next == Some(b'\\') {
                    Some((Sign::Escaped, 2))
                } else if !rules.doubled()
                    && let Some(bracket) = bracket(at + 1)
                {
                    Some((Sign::Bracket(bracket), 2))
                } else {
                    environment(rest).map(|len| (Sign::Environment(rest[..len].to_owned()), len))
                }
            }
            b'{' => Some((Sign::Open, 1)),
            b'}' => Some((Sign::Close, 1)),
            b'\n' if rules.markdown() => {
                let len = rest.len() - rest.trim_start().len();
                Some((Sign::Breaks(rest[..len].matches('\n').count()), len))
            }
            _ => rest
                .chars()
                .next()
                .filter(|&letter| letter == MARK || letter == END)
                .map(|letter| (Sign::Private, letter.len_utf8())),
        };
        match found {
            Some((sign, len)) => {
                signs.push((sign, at..at + len));
                at += len;
            }
            None => at += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    signs
}

/// What `text`, outside its formulas, shows of what it escaped under
/// `rules`: a page MathJax reads with `$` (its `processEscapes`) shows
/// `\$` as a dollar. A notebook's Markdown is Markdown's to unescape.
pub fn unescaped(text: &str, rules: Rules) -> std::borrow::Cow<'_, str> {
    if rules == (Rules::Page { dollars: true }) && text.contains("\\$") {
        std::borrow::Cow::Owned(text.replace("\\$", "$"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// The length of the `\begin{name}` or `\end{name}` `text` starts with: a
/// name of letters, which a `*` may end.
fn environment(text: &str) -> Option<usize> {
    let after = text
        .strip_prefix("\\begin{")
        .or_else(|| text.strip_prefix("\\end{"))?;
    let name = after.bytes().take_while(u8::is_ascii_alphabetic).count();
    let star = usize::from(after[name..].starts_with('*'));
    after[name + star..]
        .starts_with('}')
        .then(|| text.len() - after.len() + name + star + 1)
}

/// The byte ranges of `text` Markdown reads as code: fenced blocks (three
/// or more backticks or tildes, closed by the same) and code spans (a run of
/// backticks closed by an equal run on the same line).
fn code(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence: Option<(String, usize)> = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        if let Some((open, start)) = &fence {
            if bare.starts_with(open.as_str()) && bare[open.len()..].bytes().all(|b| b == b'`') {
                ranges.push(*start..at + bare.len());
                fence = None;
            }
        } else {
            let mark = bare.bytes().next().filter(|b| matches!(b, b'`' | b'~'));
            let run = mark.map_or(0, |mark| bare.bytes().take_while(|&b| b == mark).count());
            if run >= 3 && !(mark == Some(b'`') && bare[run..].contains('`')) {
                fence = Some((bare[..run].to_owned(), at));
            } else {
                spans(bare, at, &mut ranges);
            }
        }
        at += line.len();
    }
    if let Some((_, start)) = fence {
        ranges.push(start..text.len());
    }
    ranges
}

/// The code spans of one line, `offset` into the cell.
fn spans(line: &str, offset: usize, ranges: &mut Vec<std::ops::Range<usize>>) {
    let bytes = line.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'`' || (at > 0 && bytes[at - 1] == b'\\') {
            at += 1;
            continue;
        }
        let run = bytes[at..].iter().take_while(|&&b| b == b'`').count();
        let content = at + run;
        // The first run of the same length after it closes it.
        let mut close = None;
        let mut from = content;
        while from < bytes.len() {
            if bytes[from] == b'`' {
                let other = bytes[from..].iter().take_while(|&&b| b == b'`').count();
                if other == run && from > content {
                    close = Some(from);
                    break;
                }
                from += other;
            } else {
                from += 1;
            }
        }
        match close {
            Some(close) => {
                ranges.push(offset + at..offset + close + run);
                at = close + run;
            }
            None => at = content,
        }
    }
}

/// `text` with its formulas under `rules` taken out, each piece a
/// placeholder, and the pieces in order.
pub fn take(text: &str, rules: Rules) -> (String, Vec<Taken>) {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let code = if rules.markdown() {
        code(&text)
    } else {
        Vec::new()
    };
    let signs = signs(&text, rules, &code);
    // Each piece taken: the signs it spans, first and last.
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    let mut end: Option<Sign> = None;
    let mut last: Option<usize> = None;
    let mut braces = 0usize;
    let mut i = 0;
    while i < signs.len() {
        let sign = &signs[i].0;
        if *sign == Sign::Private {
            pieces.push((i, i));
        } else if let Some(open) = start {
            if Some(sign) == end.as_ref() {
                if braces > 0 {
                    last = Some(i);
                } else {
                    pieces.push((open, i));
                    (start, end, last) = (None, None, None);
                }
            } else if matches!(sign, Sign::Breaks(lines) if *lines >= 2) {
                // A paragraph ends whatever is open, taken up to the last
                // close met while its braces were open, if any.
                if let Some(close) = last {
                    pieces.push((open, close));
                    i = close;
                }
                (start, end, last) = (None, None, None);
                braces = 0;
            } else if *sign == Sign::Open {
                braces += 1;
            } else if *sign == Sign::Close && braces > 0 {
                braces -= 1;
            }
        } else {
            let closing = match sign {
                Sign::Dollar if rules.dollars() => Some(Sign::Dollar),
                Sign::Dollars => Some(Sign::Dollars),
                Sign::Bracket('(') => Some(Sign::Bracket(')')),
                Sign::Bracket('[') => Some(Sign::Bracket(']')),
                Sign::Environment(begin) => begin
                    .strip_prefix("\\begin")
                    .map(|name| Sign::Environment(format!("\\end{name}"))),
                _ => None,
            };
            if closing.is_some() {
                (start, end, braces) = (Some(i), closing, 0);
            }
        }
        i += 1;
    }
    if let (Some(open), Some(close)) = (start, last) {
        pieces.push((open, close));
    }
    // A private character met inside a formula was taken first: the
    // formula holds it.
    pieces.sort_by_key(|(first, _)| *first);
    let mut out = String::with_capacity(text.len());
    let mut taken = Vec::with_capacity(pieces.len());
    let mut from = 0;
    for (first, last) in pieces {
        let range = signs[first].1.start..signs[last].1.end;
        if range.start < from {
            continue;
        }
        out.push_str(&text[from..range.start]);
        out.push(MARK);
        out.push_str(&taken.len().to_string());
        out.push(END);
        let written = &text[range.clone()];
        taken.push(match signs[first].0 {
            Sign::Private => Taken::Written(written.to_owned()),
            _ => formula(written),
        });
        from = range.end;
    }
    out.push_str(&text[from..]);
    (out, taken)
}

/// A piece of math as the page's typesetter is handed it: what its
/// delimiters say.
fn formula(written: &str) -> Taken {
    let between = |open: &str, close: &str| {
        written
            .strip_prefix(open)
            .and_then(|rest| rest.strip_suffix(close))
            .map(str::to_owned)
    };
    let (tex, display) = if let Some(tex) = between("$$", "$$").filter(|_| written.len() >= 4) {
        (tex, true)
    } else if let Some(tex) = between("\\\\[", "\\\\]").or_else(|| between("\\[", "\\]")) {
        (tex, true)
    } else if let Some(tex) = between("\\\\(", "\\\\)").or_else(|| between("\\(", "\\)")) {
        (tex, false)
    } else if let Some(tex) = between("$", "$") {
        (tex, false)
    } else {
        // An environment is TeX whole, on a line of its own.
        (written.to_owned(), true)
    };
    Taken::Formula {
        tex,
        display,
        written: written.to_owned(),
    }
}

/// The pieces `text` holds placeholders of, in order, with the text
/// between: `Ok` for text, `Err` for a piece's number.
pub fn split(text: &str) -> Vec<Result<&str, usize>> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(MARK) {
        let after = &rest[open + MARK.len_utf8()..];
        let Some(close) = after.find(END) else {
            break;
        };
        let Ok(number) = after[..close].parse() else {
            break;
        };
        if open > 0 {
            parts.push(Ok(&rest[..open]));
        }
        parts.push(Err(number));
        rest = &after[close + END.len_utf8()..];
    }
    if !rest.is_empty() {
        parts.push(Ok(rest));
    }
    parts
}

/// `text` with each placeholder as what it was written as.
pub fn restore(text: &str, taken: &[Taken]) -> String {
    split(text)
        .into_iter()
        .map(|part| match part {
            Ok(text) => text,
            Err(number) => taken.get(number).map_or("", Taken::written),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formulas(text: &str) -> Vec<(String, bool)> {
        take(text, JUPYTER)
            .1
            .into_iter()
            .filter_map(|taken| match taken {
                Taken::Formula { tex, display, .. } => Some((tex, display)),
                Taken::Written(_) => None,
            })
            .collect()
    }

    #[test]
    fn math_is_what_jupyter_takes_out() {
        let found = |tex: &str, display| (tex.to_owned(), display);
        assert_eq!(
            formulas("Let $x_1$ and $$\\sum_i x_i$$ be."),
            [found("x_1", false), found("\\sum_i x_i", true)]
        );
        assert_eq!(
            formulas("\\\\(a\\\\) or \\\\[b\\\\]"),
            [found("a", false), found("b", true)]
        );
        assert_eq!(
            formulas("\\begin{align*}a &= b\\\\ c\\end{align*} then"),
            [found("\\begin{align*}a &= b\\\\ c\\end{align*}", true)]
        );
        // Braces balance: a `$` inside them closes nothing.
        assert_eq!(formulas("${a$b}$"), [found("{a$b}", false)]);
        // As Jupyter: two prices are a formula; an escaped one is not.
        assert_eq!(formulas("costs $5 and $10"), [found("5 and ", false)]);
        assert!(formulas("costs \\$5").is_empty());
    }

    #[test]
    fn no_formula_crosses_a_paragraph_or_code() {
        assert!(formulas("$a\n\nb$").is_empty());
        assert_eq!(
            formulas("$a\nb$").len(),
            1,
            "a line break is not a paragraph"
        );
        assert!(formulas("`$x$` is code").is_empty());
        assert!(formulas("``a `$x$` b``").is_empty());
        assert!(formulas("```\n$x$\n```\n").is_empty());
        assert!(formulas("~~~python\nprice = '$'\nother = '$'\n~~~\n").is_empty());
        assert_eq!(formulas("`code` and $y$"), [("y".to_owned(), false)]);
    }

    #[test]
    fn a_page_s_math_is_what_mathjax_finds() {
        let found = |text: &str, dollars| -> Vec<(String, bool)> {
            take(text, mathjax(dollars))
                .1
                .into_iter()
                .filter_map(|taken| match taken {
                    Taken::Formula { tex, display, .. } => Some((tex, display)),
                    Taken::Written(_) => None,
                })
                .collect()
        };
        let formula = |tex: &str, display| (tex.to_owned(), display);
        assert_eq!(
            found("Let \\(x_1\\) and \\[\\sum x\\] and $$y$$.", false),
            [
                formula("x_1", false),
                formula("\\sum x", true),
                formula("y", true)
            ]
        );
        // A price is one only where the page asks for `$`.
        assert!(found("costs $5 and $10", false).is_empty());
        assert_eq!(found("costs $5 and $10", true).len(), 1);
        // A line break of TeX is no delimiter, nor is a paragraph an end.
        assert_eq!(
            found("\\[a \\\\ (b)\\]", false),
            [formula("a \\\\ (b)", true)]
        );
        assert_eq!(found("\\(a\n\nb\\)", false).len(), 1);
        assert_eq!(
            found("`$x$` $y$", true).len(),
            2,
            "HTML's text has no Markdown code"
        );
    }

    #[test]
    fn placeholders_split_and_restore_as_written() {
        let cell = "A \u{e000} and $x$, $$y$$.";
        let (text, taken) = take(cell, JUPYTER);
        assert_eq!(taken.len(), 3, "{taken:?}");
        assert_eq!(taken[0], Taken::Written("\u{e000}".into()));
        assert_eq!(restore(&text, &taken), cell);
        let parts = split(&text);
        assert_eq!(parts[0], Ok("A "));
        assert_eq!(parts[1], Err(0));
        assert_eq!(parts.iter().filter(|part| part.is_err()).count(), 3);
    }
}
