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

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bibli_core::entry::{EntryKind, Metadata, Value, keys};
use globset::{Glob, GlobMatcher};

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
    fn new(text: &str, case_sensitive: bool) -> Result<Self, ParseError> {
        let is_glob = text.contains(['*', '?', '[']);
        let glob = if is_glob {
            let g = globset::GlobBuilder::new(text)
                .case_insensitive(!case_sensitive)
                .literal_separator(false)
                .build()
                .map_err(|e| ParseError(format!("bad glob `{text}`: {e}")))?;
            Some(g.compile_matcher())
        } else {
            None
        };
        Ok(NamePat {
            text: text.to_owned(),
            glob,
            case_sensitive,
            lowered: text.to_lowercase(),
        })
    }

    fn matches(&self, name: &str) -> bool {
        match &self.glob {
            Some(g) => g.is_match(name),
            None if self.case_sensitive => name.contains(&self.text),
            None => name.to_lowercase().contains(&self.lowered),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ContentPat {
    Text {
        needle: String,
        case_sensitive: bool,
    },
    Regex {
        pattern: String,
        case_insensitive: bool,
    },
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

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    LParen,
    RParen,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, ParseError> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '(' {
            chars.next();
            out.push(Tok::LParen);
        } else if c == ')' {
            chars.next();
            out.push(Tok::RParen);
        } else {
            let mut word = String::new();
            let mut quote: Option<char> = None;
            while let Some(&c) = chars.peek() {
                if let Some(q) = quote {
                    chars.next();
                    if c == q {
                        quote = None;
                    } else if c == '\\' {
                        if let Some(n) = chars.next() {
                            word.push(n);
                        }
                    } else {
                        word.push(c);
                    }
                } else if c == '"' || c == '\'' {
                    chars.next();
                    quote = Some(c);
                } else if c.is_whitespace() || c == '(' || c == ')' {
                    break;
                } else {
                    chars.next();
                    word.push(c);
                }
            }
            if quote.is_some() {
                return Err(ParseError("unterminated quote".into()));
            }
            out.push(Tok::Word(word));
        }
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
            if let Some(v) = w.strip_prefix("in:") {
                root = Some(expand_root(v));
                continue;
            }
            if let Some(v) = w.strip_prefix("depth:") {
                depth = Some(
                    v.parse::<usize>()
                        .map_err(|_| ParseError(format!("depth: expects a number, got `{v}`")))?,
                );
                continue;
            }
            if let Some(v) = w.strip_prefix("hidden:") {
                hidden = parse_bool(v)?;
                continue;
            }
            if let Some(v) = w.strip_prefix("case:") {
                case_sensitive = match v {
                    "sensitive" | "true" | "yes" => true,
                    "insensitive" | "false" | "no" => false,
                    _ => {
                        return Err(ParseError(format!(
                            "case: expects sensitive or insensitive, got `{v}`"
                        )));
                    }
                };
                continue;
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

fn expand_root(v: &str) -> PathBuf {
    if (v == "~" || v.starts_with("~/"))
        && let Some(home) = std::env::var_os("HOME")
    {
        let home = PathBuf::from(home);
        return if v == "~" { home } else { home.join(&v[2..]) };
    }
    PathBuf::from(v)
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
        while matches!(self.peek(), Some(Tok::Word(w)) if w == "OR") {
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
                None | Some(Tok::RParen) => break,
                Some(Tok::Word(w)) if w == "OR" => break,
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
        match self.peek().cloned() {
            Some(Tok::Word(w)) if w == "NOT" => {
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Some(Tok::Word(w)) if w.len() > 1 && w.starts_with('-') => {
                self.pos += 1;
                let inner = self.term(&w[1..])?;
                Ok(Expr::Not(Box::new(Expr::Term(inner))))
            }
            Some(Tok::LParen) => {
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
            Some(Tok::RParen) => Err(ParseError("unexpected `)`".into())),
            Some(Tok::Word(w)) => {
                self.pos += 1;
                Ok(Expr::Term(self.term(&w)?))
            }
            None => Err(ParseError("expected a term".into())),
        }
    }

    fn term(&self, w: &str) -> Result<Term, ParseError> {
        let cs = self.case_sensitive;
        if let Some(v) = w.strip_prefix("name:") {
            return Ok(Term::Name(NamePat::new(v, cs)?));
        }
        if let Some(v) = w.strip_prefix("ext:") {
            let exts: Vec<String> = v
                .split(',')
                .filter(|e| !e.is_empty())
                .map(|e| e.trim_start_matches('.').to_lowercase())
                .collect();
            if exts.is_empty() {
                return Err(ParseError("ext: expects at least one extension".into()));
            }
            return Ok(Term::Ext(exts));
        }
        if let Some(v) = w.strip_prefix("type:") {
            return match v {
                "file" | "f" => Ok(Term::Type(KindSel::File)),
                "dir" | "d" | "directory" | "folder" => Ok(Term::Type(KindSel::Dir)),
                "symlink" | "link" | "l" => Ok(Term::Type(KindSel::Symlink)),
                _ => Err(ParseError(format!(
                    "type: expects file, dir, or symlink, got `{v}`"
                ))),
            };
        }
        if let Some(v) = w.strip_prefix("mime:") {
            let g = Glob::new(v).map_err(|e| ParseError(format!("bad mime glob `{v}`: {e}")))?;
            return Ok(Term::Mime(g.compile_matcher(), v.to_owned()));
        }
        if let Some(v) = w.strip_prefix("tag:") {
            if v.is_empty() {
                return Err(ParseError("tag: expects a tag".into()));
            }
            return Ok(Term::Tag(v.to_owned()));
        }
        if let Some(v) = w.strip_prefix("content:") {
            return Ok(Term::Content(parse_content(v, cs)?));
        }
        if let Some(rest) = w.strip_prefix("size") {
            return parse_size(rest, w);
        }
        for field in [TimeField::Modified, TimeField::Created, TimeField::Accessed] {
            if let Some(rest) = w.strip_prefix(field.name()) {
                if rest.is_empty() || !rest.starts_with(['<', '>', '=', ':']) {
                    continue;
                }
                return parse_time(field, rest, w, self.now);
            }
        }
        Ok(Term::Name(NamePat::new(w, cs)?))
    }
}

fn parse_content(v: &str, cs: bool) -> Result<ContentPat, ParseError> {
    if v.is_empty() {
        return Err(ParseError("content: expects text or /regex/".into()));
    }
    if let Some(rest) = v.strip_prefix('/')
        && let Some(end) = rest.rfind('/')
    {
        let pattern = &rest[..end];
        let flags = &rest[end + 1..];
        if pattern.is_empty() {
            return Err(ParseError("content: empty regex".into()));
        }
        let case_insensitive = flags.contains('i') || (!cs && !flags.contains('c'));
        grep_regex::RegexMatcherBuilder::new()
            .case_insensitive(case_insensitive)
            .build(pattern)
            .map_err(|e| ParseError(format!("bad regex `{pattern}`: {e}")))?;
        return Ok(ContentPat::Regex {
            pattern: pattern.to_owned(),
            case_insensitive,
        });
    }
    Ok(ContentPat::Text {
        needle: v.to_owned(),
        case_sensitive: cs,
    })
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
    if n < 0.0 {
        return Err(ParseError(format!("bad size `{s}`")));
    }
    Ok((n * mult as f64) as u64)
}

fn parse_size(rest: &str, whole: &str) -> Result<Term, ParseError> {
    let (op, v) = split_cmp(rest)?;
    let text = whole.to_owned();
    let term = match op {
        ">" => Term::Size {
            min: Some(parse_bytes(v)?.saturating_add(1)),
            max: None,
            text,
        },
        ">=" => Term::Size {
            min: Some(parse_bytes(v)?),
            max: None,
            text,
        },
        "<" => Term::Size {
            min: None,
            max: Some(parse_bytes(v)?.saturating_sub(1)),
            text,
        },
        "<=" => Term::Size {
            min: None,
            max: Some(parse_bytes(v)?),
            text,
        },
        _ => {
            if let Some((a, b)) = v.split_once("..") {
                Term::Size {
                    min: Some(parse_bytes(a)?),
                    max: Some(parse_bytes(b)?),
                    text,
                }
            } else {
                let n = parse_bytes(v)?;
                Term::Size {
                    min: Some(n),
                    max: Some(n),
                    text,
                }
            }
        }
    };
    Ok(term)
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
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            Some((civil_to_time(y, m, 1), civil_to_time(ny, nm, 1)))
        }
        3 => {
            let m: u32 = parts[1].parse().ok()?;
            let d: u32 = parts[2].parse().ok()?;
            if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
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

fn parse_time(
    field: TimeField,
    rest: &str,
    whole: &str,
    now: SystemTime,
) -> Result<Term, ParseError> {
    let (op, v) = split_cmp(rest)?;
    let text = whole.to_owned();
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
            Term::Ext(e) => write!(f, "ext:{}", e.join(",")),
            Term::Type(KindSel::File) => f.write_str("type:file"),
            Term::Type(KindSel::Dir) => f.write_str("type:dir"),
            Term::Type(KindSel::Symlink) => f.write_str("type:symlink"),
            Term::Mime(_, s) => write!(f, "mime:{s}"),
            Term::Size { text, .. } | Term::Time { text, .. } => f.write_str(text),
            Term::Tag(t) => write!(f, "tag:{}", quote(t)),
            Term::Content(ContentPat::Text { needle, .. }) => {
                write!(f, "content:{}", quote(needle))
            }
            Term::Content(ContentPat::Regex {
                pattern,
                case_insensitive,
            }) => {
                write!(
                    f,
                    "content:/{pattern}/{}",
                    if *case_insensitive { "i" } else { "" }
                )
            }
        }
    }
}

/// Encode a literal using the tokenizer's quoting and escape rules. All query
/// builders must use this so paths and values survive parsing unchanged.
pub(crate) fn quote(s: &str) -> String {
    if s.is_empty()
        || s.contains(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '"' | '\''))
    {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_owned()
    }
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
            parts.push(format!("in:{}", quote(&self.root.to_string_lossy())));
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

    /// Only name terms (and boolean structure): a locate database can answer it.
    pub fn is_name_only(&self) -> bool {
        self.expr.is_some() && !self.any(&|t| !matches!(t, Term::Name(_)))
    }

    /// Substring name patterns, for handing to `plocate`.
    pub fn name_substrings(&self) -> Vec<String> {
        fn walk(e: &Expr, out: &mut Vec<String>) {
            match e {
                Expr::Term(Term::Name(p)) if p.glob.is_none() => out.push(p.text.clone()),
                Expr::Term(_) | Expr::Not(_) => {}
                Expr::And(v) | Expr::Or(v) => v.iter().for_each(|e| walk(e, out)),
            }
        }
        let mut out = Vec::new();
        if let Some(e) = &self.expr {
            walk(e, &mut out);
        }
        out
    }
}

// ---------------------------------------------------------------- matching

/// What the matcher looks at. `metadata` is `None` before stat.
pub struct Candidate<'a> {
    pub name: &'a OsStr,
    pub kind: EntryKind,
    pub path: &'a Path,
    pub metadata: Option<&'a Metadata>,
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

fn ext_of(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() {
        None
    } else {
        Some(ext.to_lowercase())
    }
}

impl Term {
    /// Decide the term for `c`. `content` says whether reading files is allowed now.
    pub fn eval(&self, c: &Candidate<'_>, content: bool) -> Verdict {
        let name = c.name.to_string_lossy();
        match self {
            Term::Name(p) => yes_no(p.matches(&name)),
            Term::Ext(exts) => yes_no(ext_of(&name).is_some_and(|e| exts.contains(&e))),
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
                let Some(Value::List(tags)) = m.value("tags") else {
                    return Verdict::No;
                };
                yes_no(
                    tags.iter()
                        .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(tag))),
                )
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

/// Does the file's content match? Binary files (NUL byte) and large files never do.
pub fn content_matches(path: &Path, pat: &ContentPat) -> bool {
    use grep_searcher::{BinaryDetection, SearcherBuilder, sinks};
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() > CONTENT_LIMIT {
        return false;
    }
    let matcher = match pat {
        ContentPat::Text {
            needle,
            case_sensitive,
        } => grep_regex::RegexMatcherBuilder::new()
            .case_insensitive(!case_sensitive)
            .fixed_strings(true)
            .build(needle),
        ContentPat::Regex {
            pattern,
            case_insensitive,
        } => grep_regex::RegexMatcherBuilder::new()
            .case_insensitive(*case_insensitive)
            .build(pattern),
    };
    let Ok(matcher) = matcher else { return false };
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .build();
    let mut found = false;
    let r = searcher.search_path(
        &matcher,
        path,
        sinks::UTF8(|_, _| {
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
        Candidate {
            name: OsStr::new(name),
            kind,
            path: Path::new(name),
            metadata: md,
        }
    }

    fn md(size: u64, mtime: SystemTime, tags: &[&str]) -> Metadata {
        let mut m = Metadata::default();
        m.set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
        m.set(keys::MODIFIED, Value::Time(mtime), Provenance::Filesystem);
        if !tags.is_empty() {
            m.set(
                "tags",
                Value::List(tags.iter().map(|t| Value::Text((*t).into())).collect()),
                Provenance::User,
            );
        }
        m
    }

    #[test]
    fn tokenizer_handles_quotes_and_parens() {
        assert_eq!(
            tokenize(r#"a "b c" (d OR 'e f')"#).unwrap(),
            vec![
                Tok::Word("a".into()),
                Tok::Word("b c".into()),
                Tok::LParen,
                Tok::Word("d".into()),
                Tok::Word("OR".into()),
                Tok::Word("e f".into()),
                Tok::RParen
            ]
        );
        assert!(tokenize("\"open").is_err());
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
        assert!(q("foo *.png").is_name_only());
        assert!(!q("foo ext:png").is_name_only());
        assert_eq!(q("foo bar *.png").name_substrings(), vec!["foo", "bar"]);
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
            &ContentPat::Text {
                needle: "world".into(),
                case_sensitive: false
            }
        ));
        assert!(!content_matches(
            &p,
            &ContentPat::Text {
                needle: "world".into(),
                case_sensitive: true
            }
        ));
        assert!(content_matches(
            &p,
            &ContentPat::Regex {
                pattern: "^second".into(),
                case_insensitive: false
            }
        ));
        let b = tmp.path().join("b.bin");
        std::fs::write(&b, b"Hello\x00World").unwrap();
        assert!(!content_matches(
            &b,
            &ContentPat::Text {
                needle: "Hello".into(),
                case_sensitive: false
            }
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
    }
}
