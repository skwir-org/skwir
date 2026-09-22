//! The query language: parser, printer, and matcher. Pure; no I/O except
//! content matching, which reads the file it is asked about.
//!
//! Grammar (whitespace-separated terms, implicit AND):
//!
//! ```text
//! query   := or_expr | options
//! or_expr := and_expr ("OR" and_expr)*
//! and_expr:= unary+
//! unary   := "NOT" unary | "-" unary | "(" or_expr ")" | term
//! term    := word | "name:" pat | "ext:" list | "type:" kind | "mime:" glob
//!          | "size" cmp | "modified" cmp | "created" cmp | "accessed" cmp
//!          | "tag:" word | "content:" text | "content:/" regex "/" flags?
//! options := "in:" path | "depth:" n | "hidden:" bool | "case:" ("sensitive"|"insensitive")
//! ```
//!
//! `cmp` is `>`, `<`, `>=`, `<=`, `=`/`:` a value, or `:` `a..b`. Sizes take
//! `k`, `M`, `G` (1024-based). Times take dates (`2026`, `2026-05`,
//! `2026-05-01`) or durations relative to now (`7d`, `2h30m`, `3w`).
//! `modified<7d` means within the last 7 days; `modified>7d` older than that.
//!
//! Quoting: a quote opens only at the start of a word, after a leading `-`,
//! or right after one of the keys above (`name:"a b"`), so `don't` is an
//! ordinary word. A quoted word is always literal: `"OR"`, `"-draft"` and
//! `"in:x"` are names, not an operator, a negation or an option. Inside
//! quotes `\` escapes the next character, and `\xNN` stands for one byte, so
//! `in:` can name a folder whose name is not UTF-8. [`quote`] and
//! [`quote_os`] write a literal back in this form.
//!
//! Regexes: `content:/re/` runs to the first `/` not escaped with `\` (so
//! it may hold spaces and parentheses), followed by flags: `i` ignores
//! case, `c` matches it; without a flag `case:` decides. The printer always
//! writes the flag, so a query prints and parses back to the same meaning.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bibli_core::entry::{EntryKind, Metadata, Value, keys};
use globset::{Glob, GlobMatcher};
use grep_regex::RegexMatcher;

/// A parsed query: the boolean expression plus walk options.
#[derive(Clone, Debug)]
pub struct Query {
    pub expr: Option<Expr>,
    pub root: PathBuf,
    /// `None` = unlimited.
    pub depth: Option<usize>,
    pub hidden: bool,
    pub case_sensitive: bool,
    /// Whether `in:` was given explicitly.
    pub root_given: bool,
}

#[derive(Clone, Debug)]
pub enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Term(Term),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeField {
    Modified,
    Created,
    Accessed,
}

impl TimeField {
    const ALL: [TimeField; 3] = [TimeField::Modified, TimeField::Created, TimeField::Accessed];

    fn key(self) -> &'static str {
        match self {
            TimeField::Modified => keys::MODIFIED,
            TimeField::Created => keys::CREATED,
            TimeField::Accessed => keys::ACCESSED,
        }
    }
    fn name(self) -> &'static str {
        match self {
            TimeField::Modified => "modified",
            TimeField::Created => "created",
            TimeField::Accessed => "accessed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindSel {
    File,
    Dir,
    Symlink,
}

/// A name pattern: substring (case-insensitive unless `case:sensitive`) or glob.
#[derive(Clone, Debug)]
pub struct NamePat {
    pub text: String,
    pub glob: Option<GlobMatcher>,
    pub case_sensitive: bool,
    lowered: String,
}

impl NamePat {
    /// A pattern with glob characters is a glob; one that does not compile
    /// as a glob (a name like `a[b`) is searched for as written.
    fn new(text: &str, case_sensitive: bool) -> Self {
        let glob = text
            .contains(['*', '?', '['])
            .then(|| {
                globset::GlobBuilder::new(text)
                    .case_insensitive(!case_sensitive)
                    .literal_separator(false)
                    .build()
                    .ok()
            })
            .flatten()
            .map(|g| g.compile_matcher());
        NamePat {
            text: text.to_owned(),
            glob,
            case_sensitive,
            lowered: text.to_lowercase(),
        }
    }

    fn matches(&self, c: &Candidate<'_>) -> bool {
        match &self.glob {
            Some(g) => g.is_match(&*c.text),
            None if self.case_sensitive => c.text.contains(&self.text),
            None => c.lowered().contains(&self.lowered),
        }
    }
}

/// Text or a regex to find in a file, compiled once when the query is parsed.
#[derive(Clone, Debug)]
pub struct ContentPat {
    /// The needle, or the regex's source.
    pub text: String,
    pub regex: bool,
    pub case_sensitive: bool,
    matcher: RegexMatcher,
}

impl ContentPat {
    /// A literal needle.
    pub fn text(needle: &str, case_sensitive: bool) -> Result<Self, ParseError> {
        Self::build(needle, false, case_sensitive)
    }

    /// A regular expression (the `regex` crate's syntax).
    pub fn regex(pattern: &str, case_sensitive: bool) -> Result<Self, ParseError> {
        Self::build(pattern, true, case_sensitive)
    }

    fn build(text: &str, regex: bool, case_sensitive: bool) -> Result<Self, ParseError> {
        let matcher = grep_regex::RegexMatcherBuilder::new()
            .case_insensitive(!case_sensitive)
            .fixed_strings(!regex)
            .build(text)
            .map_err(|e| ParseError(format!("bad regex `{text}`: {e}")))?;
        Ok(ContentPat {
            text: text.to_owned(),
            regex,
            case_sensitive,
            matcher,
        })
    }
}

#[derive(Clone, Debug)]
pub enum Term {
    Name(NamePat),
    Ext(Vec<String>),
    Type(KindSel),
    Mime(GlobMatcher, String),
    Size {
        min: Option<u64>,
        max: Option<u64>,
        text: String,
    },
    Time {
        field: TimeField,
        after: Option<SystemTime>,
        before: Option<SystemTime>,
        text: String,
    },
    Tag(String),
    Content(ContentPat),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ParseError {}

// ---------------------------------------------------------------- tokenizer

/// The keys a word can start with. Only these open a quote after their
/// colon; any other word with a colon (`http://x`, `a:'b`) is a name.
const KEYS: [&str; 14] = [
    "name", "ext", "type", "mime", "tag", "content", "size", "modified", "created", "accessed",
    "in", "depth", "hidden", "case",
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(Word),
    /// `NOT`, or a `-` in front of a word or a group.
    Not,
    Or,
    LParen,
    RParen,
}

#[derive(Debug, Clone, PartialEq)]
struct Word {
    /// The key before the colon, as written outside quotes.
    key: Option<&'static str>,
    /// Bytes, because `\xNN` can name bytes that are not UTF-8.
    value: Vec<u8>,
    /// Some of the value was quoted: it is a literal, never an operator,
    /// an option or a comparison.
    quoted: bool,
}

impl Word {
    fn text(&self) -> Result<&str, ParseError> {
        std::str::from_utf8(&self.value)
            .map_err(|_| ParseError("only in: accepts bytes that are not UTF-8 (\\xNN)".into()))
    }
}

fn is_break(c: char) -> bool {
    c.is_whitespace() || c == '(' || c == ')'
}

fn push_char(out: &mut Vec<u8>, c: char) {
    out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
}

fn hex_byte(hi: Option<&char>, lo: Option<&char>) -> Option<u8> {
    let hi = hi?.to_digit(16)?;
    let lo = lo?.to_digit(16)?;
    u8::try_from(hi * 16 + lo).ok()
}

/// The index of the `/` closing a regex whose body starts at `from`: the
/// first one not escaped with `\`.
fn regex_end(cs: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while let Some(&c) = cs.get(i) {
        match c {
            '\\' => i += 2,
            '/' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

fn tokenize(s: &str) -> Result<Vec<Tok>, ParseError> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = cs.get(i) {
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '(' || c == ')' {
            out.push(if c == '(' { Tok::LParen } else { Tok::RParen });
            i += 1;
            continue;
        }
        if c == '-'
            && cs
                .get(i + 1)
                .is_some_and(|&n| !n.is_whitespace() && n != ')')
        {
            out.push(Tok::Not);
            i += 1;
            continue;
        }
        let mut word = Word {
            key: None,
            value: Vec::new(),
            quoted: false,
        };
        if let Some(key) = KEYS.iter().find(|k| {
            let n = k.len();
            cs.get(i..i + n)
                .is_some_and(|w| w.iter().copied().eq(k.chars()))
                && cs.get(i + n) == Some(&':')
        }) {
            word.key = Some(key);
            i += key.len() + 1;
        }
        let first = cs.get(i).copied();
        if word.key == Some("content")
            && first == Some('/')
            && let Some(end) = regex_end(&cs, i + 1)
        {
            // The regex, delimiters included, as written; flags follow.
            for &c in &cs[i..=end] {
                push_char(&mut word.value, c);
            }
            i = end + 1;
        } else if let Some(q @ ('"' | '\'')) = first {
            word.quoted = true;
            i += 1;
            loop {
                match cs.get(i) {
                    None => return Err(ParseError("unterminated quote".into())),
                    Some(&c) if c == q => {
                        i += 1;
                        break;
                    }
                    Some('\\') => {
                        if let Some(b) = (cs.get(i + 1) == Some(&'x'))
                            .then(|| hex_byte(cs.get(i + 2), cs.get(i + 3)))
                            .flatten()
                        {
                            word.value.push(b);
                            i += 4;
                        } else if let Some(&n) = cs.get(i + 1) {
                            push_char(&mut word.value, n);
                            i += 2;
                        } else {
                            return Err(ParseError("unterminated quote".into()));
                        }
                    }
                    Some(&c) => {
                        push_char(&mut word.value, c);
                        i += 1;
                    }
                }
            }
        }
        // The rest of the word is literal: quotes inside a word are characters.
        while let Some(&c) = cs.get(i).filter(|c| !is_break(**c)) {
            push_char(&mut word.value, c);
            i += 1;
        }
        let bare = !word.quoted && word.key.is_none();
        out.push(match &word.value[..] {
            b"OR" if bare => Tok::Or,
            b"NOT" if bare => Tok::Not,
            _ => Tok::Word(word),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- parser

struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
    case_sensitive: bool,
    now: SystemTime,
}

/// Parse with the current time as the reference for relative dates.
pub fn parse(text: &str) -> Result<Query, ParseError> {
    parse_at(text, SystemTime::now())
}

/// Parse with an explicit "now" (tests).
pub fn parse_at(text: &str, now: SystemTime) -> Result<Query, ParseError> {
    let toks = tokenize(text)?;
    // Options first, since `case:` changes how patterns compile.
    let mut root = None;
    let mut depth = None;
    let mut hidden = false;
    let mut case_sensitive = false;
    let mut rest: Vec<Tok> = Vec::with_capacity(toks.len());
    for t in toks {
        if let Tok::Word(w) = &t {
            match w.key {
                Some("in") => {
                    if w.value.is_empty() {
                        return Err(ParseError("in: expects a folder".into()));
                    }
                    root = Some(expand_root(w));
                    continue;
                }
                Some("depth") => {
                    let v = w.text()?;
                    depth =
                        Some(v.parse::<usize>().map_err(|_| {
                            ParseError(format!("depth: expects a number, got `{v}`"))
                        })?);
                    continue;
                }
                Some("hidden") => {
                    hidden = parse_bool(w.text()?)?;
                    continue;
                }
                Some("case") => {
                    case_sensitive = match w.text()? {
                        "sensitive" | "true" | "yes" => true,
                        "insensitive" | "false" | "no" => false,
                        v => {
                            return Err(ParseError(format!(
                                "case: expects sensitive or insensitive, got `{v}`"
                            )));
                        }
                    };
                    continue;
                }
                _ => {}
            }
        }
        rest.push(t);
    }
    let mut p = Parser {
        toks: &rest,
        pos: 0,
        case_sensitive,
        now,
    };
    let expr = if rest.is_empty() {
        None
    } else {
        Some(p.or_expr()?)
    };
    if p.pos < rest.len() {
        return Err(ParseError("unexpected `)`".into()));
    }
    let root_given = root.is_some();
    let root = root.unwrap_or_else(|| {
        std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
    });
    Ok(Query {
        expr,
        root,
        depth,
        hidden,
        case_sensitive,
        root_given,
    })
}

#[cfg(unix)]
fn os_from_bytes(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes)
}

#[cfg(not(unix))]
fn os_from_bytes(bytes: Vec<u8>) -> OsString {
    OsString::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// The `in:` folder; an unquoted `~` is the home folder.
fn expand_root(w: &Word) -> PathBuf {
    let v = &w.value[..];
    if !w.quoted
        && (v == b"~" || v.starts_with(b"~/"))
        && let Some(home) = std::env::var_os("HOME")
    {
        let home = PathBuf::from(home);
        return if v == b"~" {
            home
        } else {
            home.join(os_from_bytes(v[2..].to_vec()))
        };
    }
    PathBuf::from(os_from_bytes(v.to_vec()))
}

fn parse_bool(v: &str) -> Result<bool, ParseError> {
    match v {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        _ => Err(ParseError(format!("expected true or false, got `{v}`"))),
    }
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn or_expr(&mut self) -> Result<Expr, ParseError> {
        let mut alts = vec![self.and_expr()?];
        while matches!(self.peek(), Some(Tok::Or)) {
            self.pos += 1;
            alts.push(self.and_expr()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap()
        } else {
            Expr::Or(alts)
        })
    }

    fn and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut items = Vec::new();
        loop {
            match self.peek() {
                None | Some(Tok::RParen | Tok::Or) => break,
                _ => items.push(self.unary()?),
            }
        }
        if items.is_empty() {
            return Err(ParseError("expected a term".into()));
        }
        Ok(if items.len() == 1 {
            items.pop().unwrap()
        } else {
            Expr::And(items)
        })
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        let Some(tok) = self.peek() else {
            return Err(ParseError("expected a term".into()));
        };
        match tok {
            Tok::Not => {
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Tok::LParen => {
                self.pos += 1;
                let e = self.or_expr()?;
                match self.peek() {
                    Some(Tok::RParen) => {
                        self.pos += 1;
                        Ok(e)
                    }
                    _ => Err(ParseError("missing `)`".into())),
                }
            }
            Tok::RParen => Err(ParseError("unexpected `)`".into())),
            Tok::Or => Err(ParseError("expected a term".into())),
            Tok::Word(w) => {
                let term = self.term(w)?;
                self.pos += 1;
                Ok(Expr::Term(term))
            }
        }
    }

    fn term(&self, w: &Word) -> Result<Term, ParseError> {
        let cs = self.case_sensitive;
        let v = w.text()?;
        let Some(key) = w.key else {
            if !w.quoted {
                if let Some(rest) = v.strip_prefix("size")
                    && rest.starts_with(['<', '>', '='])
                {
                    return parse_size(rest);
                }
                for field in TimeField::ALL {
                    if let Some(rest) = v.strip_prefix(field.name())
                        && rest.starts_with(['<', '>', '='])
                    {
                        return parse_time(field, rest, self.now);
                    }
                }
            }
            return Ok(Term::Name(NamePat::new(v, cs)));
        };
        if v.is_empty() {
            return Err(ParseError(format!("{key}: expects a value")));
        }
        match key {
            "name" => Ok(Term::Name(NamePat::new(v, cs))),
            "ext" => {
                let exts: Vec<String> = v
                    .split(',')
                    .filter(|e| !e.is_empty())
                    .map(|e| e.trim_start_matches('.').to_lowercase())
                    .collect();
                if exts.is_empty() {
                    return Err(ParseError("ext: expects at least one extension".into()));
                }
                Ok(Term::Ext(exts))
            }
            "type" => match v {
                "file" | "f" => Ok(Term::Type(KindSel::File)),
                "dir" | "d" | "directory" | "folder" => Ok(Term::Type(KindSel::Dir)),
                "symlink" | "link" | "l" => Ok(Term::Type(KindSel::Symlink)),
                _ => Err(ParseError(format!(
                    "type: expects file, dir, or symlink, got `{v}`"
                ))),
            },
            "mime" => {
                let g =
                    Glob::new(v).map_err(|e| ParseError(format!("bad mime glob `{v}`: {e}")))?;
                Ok(Term::Mime(g.compile_matcher(), v.to_owned()))
            }
            "tag" => Ok(Term::Tag(v.to_owned())),
            "content" => Ok(Term::Content(parse_content(v, w.quoted, cs)?)),
            "size" => parse_size(&format!(":{v}")),
            _ => {
                let field = TimeField::ALL
                    .into_iter()
                    .find(|f| f.name() == key)
                    .ok_or_else(|| ParseError(format!("{key}: is not allowed here")))?;
                parse_time(field, &format!(":{v}"), self.now)
            }
        }
    }
}

fn parse_content(v: &str, quoted: bool, cs: bool) -> Result<ContentPat, ParseError> {
    let regex = (!quoted)
        .then(|| v.strip_prefix('/'))
        .flatten()
        .and_then(|rest| {
            let body: Vec<char> = rest.chars().collect();
            let end = regex_end(&body, 0)?;
            Some((
                body[..end].iter().collect::<String>(),
                body[end + 1..].iter().collect::<String>(),
            ))
        });
    let Some((pattern, flags)) = regex else {
        return ContentPat::text(v, cs);
    };
    if pattern.is_empty() {
        return Err(ParseError("content: empty regex".into()));
    }
    let mut case_sensitive = cs;
    for flag in flags.chars() {
        case_sensitive = match flag {
            'i' => false,
            'c' => true,
            _ => {
                return Err(ParseError(format!(
                    "content: unknown regex flag `{flag}` (i ignores case, c matches it; quote text that starts with /)"
                )));
            }
        };
    }
    ContentPat::regex(&pattern, case_sensitive)
}

fn split_cmp(rest: &str) -> Result<(&str, &str), ParseError> {
    for op in [">=", "<=", ">", "<", "=", ":"] {
        if let Some(v) = rest.strip_prefix(op) {
            return Ok((op, v));
        }
    }
    Err(ParseError(format!("expected a comparison after `{rest}`")))
}

/// `10M`, `1k`, `512`, `1.5G`.
pub fn parse_bytes(s: &str) -> Result<u64, ParseError> {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('k' | 'K') => (&s[..s.len() - 1], 1u64 << 10),
        Some('m' | 'M') => (&s[..s.len() - 1], 1u64 << 20),
        Some('g' | 'G') => (&s[..s.len() - 1], 1u64 << 30),
        Some('t' | 'T') => (&s[..s.len() - 1], 1u64 << 40),
        Some('b' | 'B') => (&s[..s.len() - 1], 1),
        _ => (s, 1),
    };
    let n: f64 = num
        .parse()
        .map_err(|_| ParseError(format!("bad size `{s}`")))?;
    // `inf` and `nan` parse as floats; they are not sizes.
    if !n.is_finite() || n < 0.0 {
        return Err(ParseError(format!("bad size `{s}`")));
    }
    // Saturating: a size beyond u64 is "as large as it gets".
    Ok((n * mult as f64) as u64)
}

/// `rest` is what follows `size`: an operator and a value.
fn parse_size(rest: &str) -> Result<Term, ParseError> {
    let (op, v) = split_cmp(rest)?;
    let (min, max) = match op {
        ">" => (Some(parse_bytes(v)?.saturating_add(1)), None),
        ">=" => (Some(parse_bytes(v)?), None),
        "<" => {
            let n = parse_bytes(v)?;
            if n == 0 {
                return Err(ParseError("size<0 matches nothing".into()));
            }
            (None, Some(n - 1))
        }
        "<=" => (None, Some(parse_bytes(v)?)),
        _ => {
            if let Some((a, b)) = v.split_once("..") {
                (Some(parse_bytes(a)?), Some(parse_bytes(b)?))
            } else {
                let n = parse_bytes(v)?;
                (Some(n), Some(n))
            }
        }
    };
    Ok(Term::Size {
        min,
        max,
        text: format!("size{rest}"),
    })
}

/// Days since the epoch for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_to_time(y: i64, m: u32, d: u32) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs((days_from_civil(y, m, d) * 86_400).max(0) as u64)
}

fn next_month(y: i64, m: u32) -> (i64, u32) {
    if m == 12 { (y + 1, 1) } else { (y, m + 1) }
}

/// A date literal and the exclusive end of the period it denotes.
fn parse_date(s: &str) -> Option<(SystemTime, SystemTime)> {
    let parts: Vec<&str> = s.split('-').collect();
    let y: i64 = parts.first()?.parse().ok()?;
    if !(1970..=9999).contains(&y) {
        return None;
    }
    match parts.len() {
        1 => Some((civil_to_time(y, 1, 1), civil_to_time(y + 1, 1, 1))),
        2 => {
            let m: u32 = parts[1].parse().ok()?;
            if !(1..=12).contains(&m) {
                return None;
            }
            let (ny, nm) = next_month(y, m);
            Some((civil_to_time(y, m, 1), civil_to_time(ny, nm, 1)))
        }
        3 => {
            let m: u32 = parts[1].parse().ok()?;
            let d: u32 = parts[2].parse().ok()?;
            if !(1..=12).contains(&m) || d == 0 {
                return None;
            }
            // `2026-02-31` is not a day; it must not roll into March.
            let (ny, nm) = next_month(y, m);
            let month_days = days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1);
            if i64::from(d) > month_days {
                return None;
            }
            let start = civil_to_time(y, m, d);
            Some((start, start + Duration::from_secs(86_400)))
        }
        _ => None,
    }
}

enum TimeLit {
    /// An instant in the past, `now - duration`.
    Ago(SystemTime),
    /// A calendar period `[start, end)`.
    Period(SystemTime, SystemTime),
}

fn parse_time_lit(s: &str, now: SystemTime) -> Result<TimeLit, ParseError> {
    if let Some((a, b)) = parse_date(s) {
        return Ok(TimeLit::Period(a, b));
    }
    if let Ok(d) = humantime::parse_duration(s) {
        return Ok(TimeLit::Ago(now.checked_sub(d).unwrap_or(UNIX_EPOCH)));
    }
    Err(ParseError(format!(
        "bad date or duration `{s}` (try 2026-05-01 or 7d)"
    )))
}

/// `rest` is what follows the field's name: an operator and a value.
fn parse_time(field: TimeField, rest: &str, now: SystemTime) -> Result<Term, ParseError> {
    let (op, v) = split_cmp(rest)?;
    let text = format!("{}{rest}", field.name());
    let mk = |after, before| Term::Time {
        field,
        after,
        before,
        text: text.clone(),
    };
    if let Some((a, b)) = v.split_once("..") {
        let start = match parse_time_lit(a, now)? {
            TimeLit::Ago(t) | TimeLit::Period(t, _) => t,
        };
        let end = match parse_time_lit(b, now)? {
            TimeLit::Ago(t) => t,
            TimeLit::Period(_, e) => e,
        };
        return Ok(mk(Some(start), Some(end)));
    }
    let lit = parse_time_lit(v, now)?;
    Ok(match (op, lit) {
        // "within the last 7d" / "before the date"
        ("<", TimeLit::Period(s, _)) => mk(None, Some(s)),
        ("<=", TimeLit::Period(_, e)) => mk(None, Some(e)),
        // "older than 7d" / "after the date"
        (">" | ">=", TimeLit::Ago(t)) => mk(None, Some(t)),
        (">", TimeLit::Period(_, e)) => mk(Some(e), None),
        (">=", TimeLit::Period(s, _)) => mk(Some(s), None),
        (_, TimeLit::Period(s, e)) => mk(Some(s), Some(e)),
        // "within the last 7d" (also the bare `modified:7d` form)
        (_, TimeLit::Ago(t)) => mk(Some(t), None),
    })
}

// ---------------------------------------------------------------- printing

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Term::Name(p) => write!(f, "name:{}", quote(&p.text)),
            Term::Ext(e) => write!(f, "ext:{}", quote(&e.join(","))),
            Term::Type(KindSel::File) => f.write_str("type:file"),
            Term::Type(KindSel::Dir) => f.write_str("type:dir"),
            Term::Type(KindSel::Symlink) => f.write_str("type:symlink"),
            Term::Mime(_, s) => write!(f, "mime:{}", quote(s)),
            Term::Size { text, .. } | Term::Time { text, .. } => f.write_str(text),
            Term::Tag(t) => write!(f, "tag:{}", quote(t)),
            Term::Content(p) if p.regex => {
                // Escape a `/` the source left bare, so the regex ends where it should.
                f.write_str("content:/")?;
                let mut chars = p.text.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            f.write_str("\\")?;
                            if let Some(n) = chars.next() {
                                write!(f, "{n}")?;
                            }
                        }
                        '/' => f.write_str("\\/")?,
                        c => write!(f, "{c}")?,
                    }
                }
                write!(f, "/{}", if p.case_sensitive { "c" } else { "i" })
            }
            // Unquoted, a needle starting with `/` would read as a regex.
            Term::Content(p) if p.text.starts_with('/') => {
                write!(f, "content:{}", quoted(p.text.as_bytes()))
            }
            Term::Content(p) => write!(f, "content:{}", quote(&p.text)),
        }
    }
}

/// Whether `s` reads back as itself, unquoted, as a word or after a key.
fn is_plain(s: &str) -> bool {
    let Some(first) = s.chars().next() else {
        return false;
    };
    let comparison = |prefix: &str| {
        s.strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(['<', '>', '=']))
    };
    !s.contains(is_break)
        && !matches!(first, '"' | '\'' | '-' | '~')
        && s != "OR"
        && s != "NOT"
        && !KEYS
            .iter()
            .any(|k| s.strip_prefix(k).is_some_and(|rest| rest.starts_with(':')))
        && !comparison("size")
        && !TimeField::ALL.iter().any(|f| comparison(f.name()))
}

/// `bytes` in double quotes: `\` and `"` escaped, bytes that are not UTF-8
/// written `\xNN`.
fn quoted(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if matches!(c, '\\' | '"') {
                out.push('\\');
            }
            out.push(c);
        }
        for b in chunk.invalid() {
            let _ = write!(out, "\\x{b:02x}");
        }
    }
    out.push('"');
    out
}

/// Encode a literal using the tokenizer's quoting and escape rules. All query
/// builders must use this so paths and values survive parsing unchanged:
/// `quote(x)` always parses back to the word `x`, whatever it contains.
pub fn quote(s: &str) -> String {
    if is_plain(s) {
        s.to_owned()
    } else {
        quoted(s.as_bytes())
    }
}

/// [`quote`] for a path or a name that may not be UTF-8 (`in:` roots).
pub fn quote_os(s: &OsStr) -> String {
    match s.to_str() {
        Some(text) => quote(text),
        None => quoted(os_bytes(s)),
    }
}

#[cfg(unix)]
fn os_bytes(s: &OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes()
}

#[cfg(not(unix))]
fn os_bytes(s: &OsStr) -> &[u8] {
    s.as_encoded_bytes()
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Term(t) => write!(f, "{t}"),
            Expr::Not(e) => match &**e {
                Expr::Term(t) => write!(f, "NOT {t}"),
                other => write!(f, "NOT ({other})"),
            },
            Expr::And(items) => {
                for (i, e) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    match e {
                        Expr::Or(_) => write!(f, "({e})")?,
                        _ => write!(f, "{e}")?,
                    }
                }
                Ok(())
            }
            Expr::Or(alts) => {
                for (i, e) in alts.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" OR ")?;
                    }
                    write!(f, "{e}")?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if let Some(e) = &self.expr {
            parts.push(e.to_string());
        }
        if self.root_given {
            parts.push(format!("in:{}", quote_os(self.root.as_os_str())));
        }
        if let Some(d) = self.depth {
            parts.push(format!("depth:{d}"));
        }
        if self.hidden {
            parts.push("hidden:true".into());
        }
        if self.case_sensitive {
            parts.push("case:sensitive".into());
        }
        f.write_str(&parts.join(" "))
    }
}

// ---------------------------------------------------------------- needs

impl Expr {
    fn any_term(&self, pred: &dyn Fn(&Term) -> bool) -> bool {
        match self {
            Expr::Term(t) => pred(t),
            Expr::Not(e) => e.any_term(pred),
            Expr::And(v) | Expr::Or(v) => v.iter().any(|e| e.any_term(pred)),
        }
    }
}

impl Query {
    fn any(&self, pred: &dyn Fn(&Term) -> bool) -> bool {
        self.expr.as_ref().is_some_and(|e| e.any_term(pred))
    }

    /// Size or time terms: a stat is needed to evaluate.
    pub fn needs_stat(&self) -> bool {
        self.any(&|t| matches!(t, Term::Size { .. } | Term::Time { .. }))
    }

    /// Tag terms: metadata providers must run.
    pub fn needs_metadata(&self) -> bool {
        self.any(&|t| matches!(t, Term::Tag(_)))
    }

    /// Content terms: files must be read.
    pub fn needs_content(&self) -> bool {
        self.any(&|t| matches!(t, Term::Content(_)))
    }

    /// The substrings every match's name contains, when the expression is
    /// exactly that: one substring, or an AND of them. This is what a
    /// locate database answers (it ANDs its patterns); an OR, a NOT, a
    /// glob or any other term needs the walk.
    pub fn name_substrings(&self) -> Option<Vec<String>> {
        let substring = |e: &Expr| match e {
            Expr::Term(Term::Name(p)) if p.glob.is_none() && !p.text.is_empty() => {
                Some(p.text.clone())
            }
            _ => None,
        };
        match self.expr.as_ref()? {
            Expr::And(items) => items.iter().map(substring).collect(),
            e => substring(e).map(|s| vec![s]),
        }
    }
}

// ---------------------------------------------------------------- matching

/// What the matcher looks at. `metadata` is `None` before stat.
pub struct Candidate<'a> {
    pub name: &'a OsStr,
    pub kind: EntryKind,
    pub path: &'a Path,
    pub metadata: Option<&'a Metadata>,
    /// The name as text, and lowercased, computed once for all the terms.
    text: Cow<'a, str>,
    lowered: OnceCell<String>,
}

impl<'a> Candidate<'a> {
    pub fn new(
        name: &'a OsStr,
        kind: EntryKind,
        path: &'a Path,
        metadata: Option<&'a Metadata>,
    ) -> Self {
        Candidate {
            name,
            kind,
            path,
            metadata,
            text: name.to_string_lossy(),
            lowered: OnceCell::new(),
        }
    }

    fn lowered(&self) -> &str {
        self.lowered.get_or_init(|| self.text.to_lowercase())
    }
}

/// Three-valued: some terms cannot be decided before stat/content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Yes,
    No,
    Unknown,
}

impl Verdict {
    fn not(self) -> Self {
        match self {
            Verdict::Yes => Verdict::No,
            Verdict::No => Verdict::Yes,
            Verdict::Unknown => Verdict::Unknown,
        }
    }
}

/// The lowercase extension of a lowercased name.
fn ext_of(lowered: &str) -> Option<&str> {
    let (stem, ext) = lowered.rsplit_once('.')?;
    (!stem.is_empty()).then_some(ext)
}

impl Term {
    /// Decide the term for `c`. `content` says whether reading files is allowed now.
    pub fn eval(&self, c: &Candidate<'_>, content: bool) -> Verdict {
        match self {
            Term::Name(p) => yes_no(p.matches(c)),
            Term::Ext(exts) => {
                yes_no(ext_of(c.lowered()).is_some_and(|e| exts.iter().any(|x| x == e)))
            }
            Term::Type(sel) => yes_no(match sel {
                KindSel::File => c.kind == EntryKind::File,
                KindSel::Dir => c.kind == EntryKind::Directory,
                KindSel::Symlink => c.kind == EntryKind::Symlink,
            }),
            Term::Mime(g, _) => {
                if c.kind == EntryKind::Directory {
                    return yes_no(g.is_match("inode/directory"));
                }
                yes_no(
                    bibli_platform::mime::by_name(c.name).is_some_and(|m| g.is_match(m.as_str())),
                )
            }
            Term::Size { min, max, .. } => {
                let Some(m) = c.metadata else {
                    return Verdict::Unknown;
                };
                let Some(size) = m.value(keys::SIZE).and_then(Value::as_u64) else {
                    return Verdict::Unknown;
                };
                yes_no(min.is_none_or(|lo| size >= lo) && max.is_none_or(|hi| size <= hi))
            }
            Term::Time {
                field,
                after,
                before,
                ..
            } => {
                let Some(m) = c.metadata else {
                    return Verdict::Unknown;
                };
                let Some(t) = m.value(field.key()).and_then(Value::as_time) else {
                    return Verdict::No;
                };
                yes_no(after.is_none_or(|a| t >= a) && before.is_none_or(|b| t < b))
            }
            Term::Tag(tag) => {
                let Some(m) = c.metadata else {
                    return Verdict::Unknown;
                };
                let Some(Value::List(tags)) = m.value(keys::TAGS) else {
                    return Verdict::No;
                };
                yes_no(tags.iter().any(|v| {
                    v.as_str()
                        .is_some_and(|s| bibli_core::metadata::same_tag(s, tag))
                }))
            }
            Term::Content(p) => {
                if c.kind != EntryKind::File {
                    return Verdict::No;
                }
                if !content {
                    return Verdict::Unknown;
                }
                yes_no(content_matches(c.path, p))
            }
        }
    }
}

fn yes_no(b: bool) -> Verdict {
    if b { Verdict::Yes } else { Verdict::No }
}

impl Expr {
    pub fn eval(&self, c: &Candidate<'_>, content: bool) -> Verdict {
        match self {
            Expr::Term(t) => t.eval(c, content),
            Expr::Not(e) => e.eval(c, content).not(),
            Expr::And(items) => {
                let mut unknown = false;
                for e in items {
                    match e.eval(c, content) {
                        Verdict::No => return Verdict::No,
                        Verdict::Unknown => unknown = true,
                        Verdict::Yes => {}
                    }
                }
                if unknown {
                    Verdict::Unknown
                } else {
                    Verdict::Yes
                }
            }
            Expr::Or(alts) => {
                let mut unknown = false;
                for e in alts {
                    match e.eval(c, content) {
                        Verdict::Yes => return Verdict::Yes,
                        Verdict::Unknown => unknown = true,
                        Verdict::No => {}
                    }
                }
                if unknown {
                    Verdict::Unknown
                } else {
                    Verdict::No
                }
            }
        }
    }
}

/// Files larger than this are not content-searched.
pub const CONTENT_LIMIT: u64 = 8 << 20;

/// Does the file's content match? Binary files (NUL byte), large files and
/// anything that is not a regular file once opened (a FIFO would block) never do.
pub fn content_matches(path: &Path, pat: &ContentPat) -> bool {
    use grep_searcher::{BinaryDetection, SearcherBuilder, sinks};
    let Ok(file) = bibli_platform::file::open_regular(path) else {
        return false;
    };
    if file.metadata().map_or(true, |m| m.len() > CONTENT_LIMIT) {
        return false;
    }
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .build();
    let mut found = false;
    let r = searcher.search_file(
        &pat.matcher,
        &file,
        sinks::Bytes(|_, _| {
            found = true;
            Ok(false)
        }),
    );
    r.is_ok() && found
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::entry::Provenance;

    fn now() -> SystemTime {
        // 2026-09-18T12:00:00Z
        civil_to_time(2026, 9, 18) + Duration::from_secs(12 * 3600)
    }

    fn q(s: &str) -> Query {
        parse_at(s, now()).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    fn cand<'a>(name: &'a str, kind: EntryKind, md: Option<&'a Metadata>) -> Candidate<'a> {
        Candidate::new(OsStr::new(name), kind, Path::new(name), md)
    }

    fn md(size: u64, mtime: SystemTime, tags: &[&str]) -> Metadata {
        let mut m = Metadata::default();
        m.set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
        m.set(keys::MODIFIED, Value::Time(mtime), Provenance::Filesystem);
        if !tags.is_empty() {
            m.set(
                keys::TAGS,
                Value::List(tags.iter().map(|t| Value::Text((*t).into())).collect()),
                Provenance::User,
            );
        }
        m
    }

    fn word(key: Option<&'static str>, value: &str, quoted: bool) -> Tok {
        Tok::Word(Word {
            key,
            value: value.as_bytes().to_vec(),
            quoted,
        })
    }

    #[test]
    fn tokenizer_handles_quotes_and_parens() {
        assert_eq!(
            tokenize(r#"a "b c" (d OR 'e f') -x -(y) name:"OR" don't"#).unwrap(),
            vec![
                word(None, "a", false),
                word(None, "b c", true),
                Tok::LParen,
                word(None, "d", false),
                Tok::Or,
                word(None, "e f", true),
                Tok::RParen,
                Tok::Not,
                word(None, "x", false),
                Tok::Not,
                Tok::LParen,
                word(None, "y", false),
                Tok::RParen,
                word(Some("name"), "OR", true),
                word(None, "don't", false),
            ]
        );
        assert!(tokenize("\"open").is_err());
        // A regex runs to its closing slash, spaces and parentheses included.
        assert_eq!(
            tokenize(r"content:/(a b)\/c/i x").unwrap(),
            vec![
                word(Some("content"), r"/(a b)\/c/i", false),
                word(None, "x", false)
            ]
        );
    }

    #[test]
    fn ordinary_words_are_names() {
        for (s, name) in [
            ("sizes", "sizes"),
            ("size_chart.png", "size_chart.png"),
            ("size", "size"),
            ("don't", "don't"),
            ("modified", "modified"),
            ("http://x", "http://x"),
            ("a:'b", "a:'b"),
            ("\"OR\"", "OR"),
            ("\"NOT\"", "NOT"),
            ("\"-draft\"", "-draft"),
            ("\"in:x\"", "in:x"),
            ("\"size>1\"", "size>1"),
            ("name:\"a b\"", "a b"),
            ("a[b", "a[b"),
        ] {
            let Some(Expr::Term(Term::Name(p))) = q(s).expr else {
                panic!("`{s}` is not a name");
            };
            assert_eq!(p.text, name, "{s}");
        }
        // Mid-word quotes are characters: nothing is dropped.
        let Some(Expr::And(items)) = q("it's bob's").expr else {
            panic!()
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], Expr::Term(Term::Name(p)) if p.text == "it's"));
        assert!(matches!(&items[1], Expr::Term(Term::Name(p)) if p.text == "bob's"));
    }

    #[test]
    fn negation_applies_to_groups() {
        let p = q("-(a OR b)");
        let Some(Expr::Not(inner)) = &p.expr else {
            panic!("{p:?}")
        };
        assert!(matches!(&**inner, Expr::Or(v) if v.len() == 2));
        assert_eq!(p.to_string(), "NOT (name:a OR name:b)");
        let f = cand("a.txt", EntryKind::File, None);
        assert_eq!(p.expr.unwrap().eval(&f, false), Verdict::No);
    }

    #[test]
    fn regexes_keep_their_text_and_case() {
        for (s, printed) in [
            ("content:/a b/", "content:/a b/i"),
            ("content:/(x|y)/c", "content:/(x|y)/c"),
            ("content:/a\\/b/i", "content:/a\\/b/i"),
            ("content:/x/ case:sensitive", "content:/x/c case:sensitive"),
            ("content:/x/i case:sensitive", "content:/x/i case:sensitive"),
            ("content:\"/etc/\"", "content:\"/etc/\""),
        ] {
            let parsed = q(s);
            assert_eq!(parsed.to_string(), printed, "{s}");
            let again = q(printed);
            assert_eq!(again.to_string(), printed, "{s}");
        }
        let Some(Expr::Term(Term::Content(p))) = q("content:/x/c").expr else {
            panic!()
        };
        assert!(p.regex && p.case_sensitive);
        let Some(Expr::Term(Term::Content(p))) = q("content:\"/etc/\"").expr else {
            panic!()
        };
        assert!(!p.regex && p.text == "/etc/");
        assert!(
            parse_at("content:/usr/bin", now()).is_err(),
            "unknown flags"
        );
    }

    #[test]
    fn parses_and_prints_round_trip() {
        for s in [
            "name:report ext:pdf,jpg",
            "*.png OR *.jpg",
            "NOT type:dir size>10M",
            "(a OR b) c",
            "modified<7d",
            "modified:2026-05",
            "created:2026-01-01..2026-02-01",
            "tag:work content:hello",
            "content:/foo.*bar/i",
            "mime:image/*",
            "report in:/tmp/x depth:2 hidden:true case:sensitive",
            "name:\"two words\"",
        ] {
            let parsed = q(s);
            let printed = parsed.to_string();
            let again = parse_at(&printed, now()).unwrap_or_else(|e| panic!("{printed}: {e}"));
            assert_eq!(again.to_string(), printed, "round trip of `{s}`");
        }
        assert_eq!(q("*.png").to_string(), "name:*.png");
        assert_eq!(q("-ext:tmp").to_string(), "NOT ext:tmp");
    }

    #[test]
    fn options_are_extracted() {
        let p = q("foo in:~ depth:3 hidden:yes case:sensitive");
        assert!(p.root_given);
        assert_eq!(p.root, PathBuf::from(std::env::var("HOME").unwrap()));
        assert_eq!(p.depth, Some(3));
        assert!(p.hidden && p.case_sensitive);
        let p = q("foo");
        assert!(!p.root_given && p.depth.is_none() && !p.hidden);
        let p = q("in:/tmp");
        assert!(p.expr.is_none());
    }

    #[test]
    fn errors_are_helpful() {
        for (s, needle) in [
            ("(a", "missing"),
            ("a)", "unexpected"),
            ("depth:x", "number"),
            ("size>1x", "bad size"),
            ("modified<yesterday", "bad date"),
            ("type:blob", "type:"),
            ("content:/[/", "bad regex"),
            ("OR a", "term"),
            ("a OR", "term"),
            ("size<0", "nothing"),
            ("size>inf", "bad size"),
            ("size>nan", "bad size"),
            ("modified:2026-02-31", "bad date"),
            ("tag:", "expects"),
            ("in:", "folder"),
        ] {
            let e = parse_at(s, now()).unwrap_err().to_string();
            assert!(
                e.contains(needle),
                "`{s}` -> `{e}` should mention `{needle}`"
            );
        }
    }

    #[test]
    fn sizes_and_needs() {
        assert_eq!(parse_bytes("10M").unwrap(), 10 << 20);
        assert_eq!(parse_bytes("1.5k").unwrap(), 1536);
        assert_eq!(parse_bytes("512").unwrap(), 512);
        let p = q("size>10M");
        assert!(p.needs_stat() && !p.needs_metadata() && !p.needs_content());
        assert!(q("tag:x").needs_metadata());
        assert!(q("content:x").needs_content());
        assert_eq!(q("foo").name_substrings().unwrap(), ["foo"]);
        assert_eq!(q("foo bar").name_substrings().unwrap(), ["foo", "bar"]);
        // A locate database ANDs its patterns: anything else needs the walk.
        for s in ["foo OR bar", "foo *.png", "foo ext:png", "NOT foo", "-foo"] {
            assert!(q(s).name_substrings().is_none(), "{s}");
        }
    }

    #[test]
    fn name_ext_type_mime_match() {
        let f = cand("Report-Final.PDF", EntryKind::File, None);
        assert_eq!(q("report").expr.unwrap().eval(&f, false), Verdict::Yes);
        assert_eq!(
            q("report case:sensitive").expr.unwrap().eval(&f, false),
            Verdict::No
        );
        assert_eq!(q("*.pdf").expr.unwrap().eval(&f, false), Verdict::Yes);
        assert_eq!(q("ext:pdf,jpg").expr.unwrap().eval(&f, false), Verdict::Yes);
        assert_eq!(q("ext:txt").expr.unwrap().eval(&f, false), Verdict::No);
        assert_eq!(q("type:file").expr.unwrap().eval(&f, false), Verdict::Yes);
        assert_eq!(q("type:dir").expr.unwrap().eval(&f, false), Verdict::No);
        assert_eq!(
            q("mime:application/pdf").expr.unwrap().eval(&f, false),
            Verdict::Yes
        );
        assert_eq!(q("mime:image/*").expr.unwrap().eval(&f, false), Verdict::No);
        let d = cand("photos", EntryKind::Directory, None);
        assert_eq!(
            q("mime:inode/directory").expr.unwrap().eval(&d, false),
            Verdict::Yes
        );
        assert_eq!(q("NOT report").expr.unwrap().eval(&f, false), Verdict::No);
        assert_eq!(
            q("nope OR report").expr.unwrap().eval(&f, false),
            Verdict::Yes
        );
    }

    #[test]
    fn stat_terms_are_unknown_before_stat_then_decided() {
        let f = cand("a.txt", EntryKind::File, None);
        let e = q("size>1k").expr.unwrap();
        assert_eq!(e.eval(&f, false), Verdict::Unknown);
        let m = md(2048, now() - Duration::from_secs(3600), &["work"]);
        let f = cand("a.txt", EntryKind::File, Some(&m));
        assert_eq!(e.eval(&f, false), Verdict::Yes);
        assert_eq!(q("size<1k").expr.unwrap().eval(&f, false), Verdict::No);
        assert_eq!(q("size:2k").expr.unwrap().eval(&f, false), Verdict::Yes);
        assert_eq!(q("size:1k..3k").expr.unwrap().eval(&f, false), Verdict::Yes);
        assert_eq!(q("modified<7d").expr.unwrap().eval(&f, false), Verdict::Yes);
        assert_eq!(q("modified>7d").expr.unwrap().eval(&f, false), Verdict::No);
        assert_eq!(
            q("modified:2026-09").expr.unwrap().eval(&f, false),
            Verdict::Yes
        );
        assert_eq!(
            q("modified:2026-08").expr.unwrap().eval(&f, false),
            Verdict::No
        );
        assert_eq!(
            q("modified>2026-09-01").expr.unwrap().eval(&f, false),
            Verdict::Yes
        );
        assert_eq!(
            q("modified<2026-09-01").expr.unwrap().eval(&f, false),
            Verdict::No
        );
        assert_eq!(
            q("modified:2026-09-18..2026-09-19")
                .expr
                .unwrap()
                .eval(&f, false),
            Verdict::Yes
        );
        assert_eq!(q("tag:WORK").expr.unwrap().eval(&f, false), Verdict::Yes);
        assert_eq!(q("size<=2k").expr.unwrap().eval(&f, false), Verdict::Yes);
        assert_eq!(q("tag:home").expr.unwrap().eval(&f, false), Verdict::No);
        // AND with an unknown stays unknown; a definite No short-circuits.
        assert_eq!(
            q("a size>1k")
                .expr
                .unwrap()
                .eval(&cand("a.txt", EntryKind::File, None), false),
            Verdict::Unknown
        );
        assert_eq!(
            q("zzz size>1k")
                .expr
                .unwrap()
                .eval(&cand("a.txt", EntryKind::File, None), false),
            Verdict::No
        );
    }

    #[test]
    fn content_matching_reads_files() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("t.txt");
        std::fs::write(&p, "Hello World\nsecond line\n").unwrap();
        assert!(content_matches(
            &p,
            &ContentPat::text("world", false).unwrap()
        ));
        assert!(!content_matches(
            &p,
            &ContentPat::text("world", true).unwrap()
        ));
        assert!(content_matches(
            &p,
            &ContentPat::regex("^second", true).unwrap()
        ));
        let b = tmp.path().join("b.bin");
        std::fs::write(&b, b"Hello\x00World").unwrap();
        assert!(!content_matches(
            &b,
            &ContentPat::text("Hello", false).unwrap()
        ));
    }

    #[test]
    fn civil_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        let (a, b) = parse_date("2026-02").unwrap();
        assert_eq!(b.duration_since(a).unwrap().as_secs(), 28 * 86_400);
        let (a, b) = parse_date("2024").unwrap();
        assert_eq!(b.duration_since(a).unwrap().as_secs(), 366 * 86_400);
        assert!(parse_date("2026-13").is_none());
        assert!(parse_date("2026-02-31").is_none());
        assert!(parse_date("2026-02-29").is_none());
        assert!(parse_date("2024-02-29").is_some());
        assert!(parse_date("2026-04-31").is_none());
        assert!(parse_date("2026-12-31").is_some());
    }

    #[test]
    fn non_utf8_roots_round_trip() {
        use std::os::unix::ffi::OsStrExt;
        let root = Path::new(OsStr::from_bytes(b"/data/caf\xe9 \"x\"\\y"));
        let text = format!("type:file in:{}", quote_os(root.as_os_str()));
        let parsed = q(&text);
        assert_eq!(parsed.root, root);
        assert_eq!(q(&parsed.to_string()).root, root);
        // A name term cannot hold such bytes; the error says where they can go.
        assert!(parse_at("\"\\xff\"", now()).is_err());
        // `\x41` is a byte in quotes, and a literal backslash survives quoting.
        assert_eq!(q("in:\"/\\x41\"").root, Path::new("/A"));
        assert_eq!(
            q(&format!("in:{}", quote("/\\x41"))).root,
            Path::new("/\\x41")
        );
        // Only an unquoted `~` is home.
        assert_eq!(q("in:\"~\"").root, Path::new("~"));
    }

    proptest::proptest! {
        /// Whatever a name contains, quoting it and parsing it back yields
        /// the same name: keywords, dashes, keys, quotes, backslashes.
        #[test]
        fn quote_round_trips(
            s in "(OR|NOT|-|in:|name:|content:/|size>|modified<|~|\"|'|\\\\|\\\\x41|\\\\xff|\\(|\\)| |\t|\n|a|é|\\*|/){1,8}"
        ) {
            let parsed = parse_at(&quote(&s), now()).unwrap();
            let Some(Expr::Term(Term::Name(p))) = parsed.expr else {
                panic!("`{s}` quoted as `{}` is not a name", quote(&s));
            };
            proptest::prop_assert_eq!(&p.text, &s);
            // After a key too, and as an `in:` root.
            let parsed = parse_at(&format!("name:{}", quote(&s)), now()).unwrap();
            let Some(Expr::Term(Term::Name(p))) = parsed.expr else { panic!() };
            proptest::prop_assert_eq!(&p.text, &s);
            let root = format!("/{s}");
            let parsed = parse_at(&format!("x in:{}", quote(&root)), now()).unwrap();
            proptest::prop_assert_eq!(parsed.root, PathBuf::from(&root));
        }

        #[test]
        fn quote_round_trips_any_text(s in "\\PC{1,12}") {
            let parsed = parse_at(&quote(&s), now()).unwrap();
            let Some(Expr::Term(Term::Name(p))) = parsed.expr else {
                panic!("`{s}` quoted as `{}` is not a name", quote(&s));
            };
            proptest::prop_assert_eq!(&p.text, &s);
        }
    }
}
