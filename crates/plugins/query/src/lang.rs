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
//!          | "kind:" kinds | "size" cmp | "modified" cmp | "created" cmp | "accessed" cmp
//!          | "tag:" word | "comment:" text | "rating" cmp
//!          | "content:" text | "content:/" regex "/" flags?
//!          | "near:" lat "," lon ("~" distance)? | "inside:" south "," west "," north "," east
//! options := "in:" path | "depth:" n | "hidden:" bool | "case:" ("sensitive"|"insensitive")
//! ```
//!
//! `cmp` is `>`, `<`, `>=`, `<=`, `=`/`:` a value, or `:` `a..b`. Sizes take
//! `k`, `M`, `G` (1024-based). Ratings are stars, halves allowed
//! (`rating>=4`, `rating:3.5`, `rating:2..4`); `rating:0` is unrated.
//! `comment:` holds when the file's comment contains the text, whatever
//! its case. Times take dates (`2026`, `2026-05`,
//! `2026-05-01`), periods named from now (`today`, `yesterday`,
//! `this-week`, `last-week`, `this-month`, `last-month`, `this-year`,
//! `last-year`), or durations relative to now
//! (`7d`, `2h30m`, `3w`, `1mo`). A date or a named period is a period of
//! the user's calendar, in their time zone with its clock changes, weeks
//! from their locale's first day (ADR-017; the timeline's bars are the
//! same days): `modified:this-week` is within it, `modified<today` before
//! it. Durations are counted back on the calendar too: `modified<7d` is
//! within the last seven days, this time of day; `modified>7d` older.
//!
//! `kind:` takes what the desktop's MIME database groups files into
//! (`image`, `video`, `audio`, `document`, `spreadsheet`, `presentation`,
//! `text`, `archive`, `font`, `program`, `folder`; plurals and synonyms
//! such as `music` or `photos` too), several separated by commas:
//! `kind:image,video`. See [`bibli_platform::mime::Kind`].
//!
//! Links: a link is matched as what it opens, as it is shown. `type:`,
//! `kind:`, `mime:`, the sizes and times, `content:` and the places are
//! what it leads to (a link to a folder is `type:dir`, and `type:symlink`
//! still), while `name:`, `ext:` and bare words match its own name. A link
//! that leads nowhere is `mime:inode/symlink` and of no kind; a pipe, a
//! socket or a device (or a link to one) is `mime:inode/fifo` and so on,
//! of no kind and never read, whatever its name says. A search
//! does not go down into a folder through a link: no loops, and no file
//! found twice.
//!
//! Quoting: a quote opens only at the start of a word, after a leading `-`,
//! or right after one of the keys above (`name:"a b"`), so `don't` is an
//! ordinary word. A quoted word is always literal: `"OR"`, `"-draft"` and
//! `"in:x"` are names, not an operator, a negation or an option. Inside
//! quotes `\` escapes the next character, and `\xNN` stands for one byte, so
//! `in:` can name a folder whose name is not UTF-8. [`quote`] and
//! [`quote_os`] write a literal back in this form.
//!
//! Places (ADR-022, ADR-023): `near:10.12,20.12~5km` holds the files that
//! say they were made within 5 km of that point (1 km without a
//! distance; `m` or `km`), `inside:9.9,19.9,10.2,20.3` those inside the
//! box of those edges in degrees (south, west, north, east; west past
//! east crosses ±180°). A file says where it was made through the field
//! `position`, read on demand from its content (a picture's EXIF), so
//! these terms are decided last, as `content:` is.
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
use std::time::{SystemTime, UNIX_EPOCH};

use bibli_core::entry::{EntryKind, Metadata, Value, keys};
use bibli_platform::calendar::{Calendar, civil_from_days, days_from_civil};
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
    /// Any of these kinds.
    Kind(Vec<bibli_platform::mime::Kind>),
    Size {
        min: Option<u64>,
        max: Option<u64>,
        text: String,
    },
    Time {
        field: TimeField,
        after: Option<SystemTime>,
        before: Option<SystemTime>,
        /// When the term means another period: a named one (`today`)
        /// changes at its next start.
        changes: Option<SystemTime>,
        text: String,
    },
    Tag(String),
    /// The comment contains this, whatever its case (held lowercase).
    Comment(String),
    /// A rating between these, inclusive, in half stars (`keys::RATING`);
    /// 0 is unrated.
    Rating {
        min: u64,
        max: u64,
        text: String,
    },
    Content(ContentPat),
    /// Within `radius` metres of `at`.
    Near {
        at: bibli_core::geo::LatLon,
        radius: f64,
        text: String,
    },
    /// Inside the box of these edges, in degrees; `west > east` crosses
    /// ±180°.
    Inside {
        south: f64,
        west: f64,
        north: f64,
        east: f64,
        text: String,
    },
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
const KEYS: [&str; 19] = [
    "name", "ext", "type", "mime", "kind", "tag", "comment", "rating", "content", "size",
    "modified", "created", "accessed", "in", "depth", "hidden", "case", "near", "inside",
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
    calendar: &'a Calendar,
}

/// Parse with the current time as the reference for relative dates.
pub fn parse(text: &str) -> Result<Query, ParseError> {
    parse_in(text, SystemTime::now(), &Calendar::system())
}

/// Parse with an explicit "now" and calendar: dates and periods are days
/// of `cal`, durations are counted back from `now` on it.
pub fn parse_in(text: &str, now: SystemTime, cal: &Calendar) -> Result<Query, ParseError> {
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
        calendar: cal,
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
                if let Some(rest) = v.strip_prefix("rating")
                    && rest.starts_with(['<', '>', '='])
                {
                    return parse_rating(rest);
                }
                for field in TimeField::ALL {
                    if let Some(rest) = v.strip_prefix(field.name())
                        && rest.starts_with(['<', '>', '='])
                    {
                        return parse_time(field, rest, self.now, self.calendar);
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
            "kind" => {
                let mut kinds = Vec::new();
                for word in v.split(',').filter(|w| !w.is_empty()) {
                    let kind = bibli_platform::mime::Kind::from_name(word).ok_or_else(|| {
                        ParseError(format!("kind: expects {}, got `{word}`", kind_words()))
                    })?;
                    if !kinds.contains(&kind) {
                        kinds.push(kind);
                    }
                }
                if kinds.is_empty() {
                    return Err(ParseError(format!("kind: expects {}", kind_words())));
                }
                Ok(Term::Kind(kinds))
            }
            "tag" => Ok(Term::Tag(v.to_owned())),
            "comment" => Ok(Term::Comment(v.to_lowercase())),
            "rating" => parse_rating(&format!(":{v}")),
            "content" => Ok(Term::Content(parse_content(v, w.quoted, cs)?)),
            "near" => parse_near(v),
            "inside" => parse_inside(v),
            "size" => parse_size(&format!(":{v}")),
            _ => {
                let field = TimeField::ALL
                    .into_iter()
                    .find(|f| f.name() == key)
                    .ok_or_else(|| ParseError(format!("{key}: is not allowed here")))?;
                parse_time(field, &format!(":{v}"), self.now, self.calendar)
            }
        }
    }
}

/// A number of degrees as the printer writes it: at most 5 decimals (a
/// metre), no trailing zeros.
fn degrees(value: f64) -> String {
    let text = format!("{value:.5}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

/// `5km`, `500m`, `2.5km`; a bare number is kilometres. Metres, and the
/// text the printer writes back.
fn parse_distance(text: &str) -> Result<(f64, String), ParseError> {
    let text = text.trim();
    let (number, metres_per) = if let Some(n) = text.strip_suffix("km") {
        (n, 1000.)
    } else if let Some(n) = text.strip_suffix('m') {
        (n, 1.)
    } else {
        (text, 1000.)
    };
    let value: f64 = number.trim().parse().map_err(|_| {
        ParseError(format!(
            "near: expects a distance such as 5km or 500m, got `{text}`"
        ))
    })?;
    let metres = value * metres_per;
    if !(metres > 0. && metres <= 20_037_509.) {
        return Err(ParseError(format!(
            "near: a distance of `{text}` is not one on the Earth"
        )));
    }
    let written = if metres >= 1000. {
        format!("{}km", degrees(metres / 1000.))
    } else {
        format!("{}m", degrees(metres))
    };
    Ok((metres, written))
}

/// Numbers separated by commas, as many as asked.
fn numbers(text: &str, count: usize, key: &str, expected: &str) -> Result<Vec<f64>, ParseError> {
    let values: Vec<f64> = text
        .split(',')
        .map(|n| n.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| ParseError(format!("{key}: expects {expected}, got `{text}`")))?;
    if values.len() != count || values.iter().any(|v| !v.is_finite()) {
        return Err(ParseError(format!(
            "{key}: expects {expected}, got `{text}`"
        )));
    }
    Ok(values)
}

/// `near:10.12,20.12~5km`: a point, and how far from it (1 km unsaid).
fn parse_near(v: &str) -> Result<Term, ParseError> {
    let (point, distance) = v.split_once('~').map_or((v, None), |(p, d)| (p, Some(d)));
    let values = numbers(
        point,
        2,
        "near",
        "a latitude and a longitude, such as 45.1,6.2",
    )?;
    let at = bibli_core::geo::LatLon::new(values[0], values[1])
        .filter(|_| (-90. ..=90.).contains(&values[0]))
        .ok_or_else(|| ParseError(format!("near: `{point}` is not a place")))?;
    let (radius, written) = match distance {
        Some(d) => parse_distance(d)?,
        None => (1000., "1km".into()),
    };
    Ok(Term::Near {
        at,
        radius,
        text: format!("near:{},{}~{written}", degrees(at.lat), degrees(at.lon)),
    })
}

/// `inside:9.9,19.9,10.2,20.3`: south, west, north, east.
fn parse_inside(v: &str) -> Result<Term, ParseError> {
    let expected = "south, west, north and east edges, such as 9.9,19.9,10.2,20.3";
    let values = numbers(v, 4, "inside", expected)?;
    let (south, west, north, east) = (values[0], values[1], values[2], values[3]);
    let latitude = |x: f64| (-90. ..=90.).contains(&x);
    let longitude = |x: f64| (-180. ..=180.).contains(&x);
    if !(latitude(south) && latitude(north) && south <= north && longitude(west) && longitude(east))
    {
        return Err(ParseError(format!("inside: expects {expected}, got `{v}`")));
    }
    Ok(Term::Inside {
        south,
        west,
        north,
        east,
        text: format!(
            "inside:{},{},{},{}",
            degrees(south),
            degrees(west),
            degrees(north),
            degrees(east)
        ),
    })
}

/// The words `kind:` takes, for an error message.
fn kind_words() -> String {
    let words: Vec<&str> = bibli_platform::mime::Kind::ALL
        .iter()
        .map(|k| k.name())
        .collect();
    words.join(", ")
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

/// A number of stars (`4`, `3.5`) in half stars.
fn half_stars(v: &str) -> Result<u64, ParseError> {
    let stars: f64 = v
        .trim()
        .parse()
        .ok()
        .filter(|s: &f64| (0.0..=5.0).contains(s) && (s * 2.).fract() == 0.)
        .ok_or_else(|| {
            ParseError(format!(
                "rating expects stars from 0 to 5 (halves allowed), got `{v}`"
            ))
        })?;
    Ok((stars * 2.) as u64)
}

/// `rest` is what follows `rating`: an operator and a number of stars.
fn parse_rating(rest: &str) -> Result<Term, ParseError> {
    const MAX: u64 = 10;
    let (op, v) = split_cmp(rest)?;
    let (min, max) = match op {
        ">" => (half_stars(v)? + 1, MAX),
        ">=" => (half_stars(v)?, MAX),
        "<" => match half_stars(v)? {
            0 => return Err(ParseError("rating<0 matches nothing".into())),
            n => (0, n - 1),
        },
        "<=" => (0, half_stars(v)?),
        _ => {
            if let Some((a, b)) = v.split_once("..") {
                (half_stars(a)?, half_stars(b)?)
            } else {
                let n = half_stars(v)?;
                (n, n)
            }
        }
    };
    Ok(Term::Rating {
        min,
        max,
        text: format!("rating{rest}"),
    })
}

/// A calendar period `[start, end)` in civil days.
type Days = (i64, i64);

/// A date literal (`2026`, `2026-05`, `2026-05-01`) as civil days.
fn parse_date(s: &str) -> Option<Days> {
    let parts: Vec<&str> = s.split('-').collect();
    let y: i64 = parts.first()?.parse().ok()?;
    if !(1970..=9999).contains(&y) {
        return None;
    }
    let month = |m: u32| (days_from_civil(y, m, 1), next_month_start(y, m));
    match parts.len() {
        1 => Some((days_from_civil(y, 1, 1), days_from_civil(y + 1, 1, 1))),
        2 => {
            let m: u32 = parts[1].parse().ok()?;
            (1..=12).contains(&m).then(|| month(m))
        }
        3 => {
            let m: u32 = parts[1].parse().ok()?;
            let d: u32 = parts[2].parse().ok()?;
            if !(1..=12).contains(&m) || d == 0 {
                return None;
            }
            // `2026-02-31` is not a day; it must not roll into March.
            let (first, next) = month(m);
            let day = first + i64::from(d) - 1;
            (day < next).then_some((day, day + 1))
        }
        _ => None,
    }
}

fn next_month_start(y: i64, m: u32) -> i64 {
    if m == 12 {
        days_from_civil(y + 1, 1, 1)
    } else {
        days_from_civil(y, m + 1, 1)
    }
}

/// A period named from `today` (`today`, `this-week`, `last-month`), and
/// the day it next means another period: at its unit's next start.
fn named_period(s: &str, today: i64, cal: &Calendar) -> Option<(Days, i64)> {
    let week = cal.week_floor(today);
    let (y, m, _) = civil_from_days(today);
    let month = days_from_civil(y, m, 1);
    let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    let year = days_from_civil(y, 1, 1);
    let next_year = days_from_civil(y + 1, 1, 1);
    Some(match s.to_ascii_lowercase().as_str() {
        "today" => ((today, today + 1), today + 1),
        "yesterday" => ((today - 1, today), today + 1),
        "this-week" => ((week, week + 7), week + 7),
        "last-week" => ((week - 7, week), week + 7),
        "this-month" => ((month, next_month_start(y, m)), next_month_start(y, m)),
        "last-month" => ((days_from_civil(py, pm, 1), month), next_month_start(y, m)),
        "this-year" => ((year, next_year), next_year),
        "last-year" => ((days_from_civil(y - 1, 1, 1), year), next_year),
        _ => return None,
    })
}

enum TimeLit {
    /// An instant in the past, `now - duration`.
    Ago(SystemTime),
    /// A calendar period `[start, end)`, and when it changes (named
    /// periods only).
    Period(SystemTime, SystemTime, Option<SystemTime>),
}

fn parse_time_lit(s: &str, now: SystemTime, cal: &Calendar) -> Result<TimeLit, ParseError> {
    let period = |(a, b): Days, changes: Option<i64>| {
        TimeLit::Period(
            cal.day_start(a),
            cal.day_start(b),
            changes.map(|day| cal.day_start(day)),
        )
    };
    if let Some(days) = parse_date(s) {
        return Ok(period(days, None));
    }
    if let Some((days, changes)) = named_period(s, cal.day_of(now), cal) {
        return Ok(period(days, Some(changes)));
    }
    // On the calendar (`7d` across a clock change is seven days), else as
    // humantime reads it (`1month`), an exact length.
    if let Some(t) = cal.ago(now, s) {
        return Ok(TimeLit::Ago(t));
    }
    if let Ok(d) = humantime::parse_duration(s) {
        return Ok(TimeLit::Ago(now.checked_sub(d).unwrap_or(UNIX_EPOCH)));
    }
    Err(ParseError(format!(
        "bad date or duration `{s}` (try 2026-05-01, this-week or 7d)"
    )))
}

/// `rest` is what follows the field's name: an operator and a value.
fn parse_time(
    field: TimeField,
    rest: &str,
    now: SystemTime,
    cal: &Calendar,
) -> Result<Term, ParseError> {
    let (op, v) = split_cmp(rest)?;
    let text = format!("{}{rest}", field.name());
    let mk = |after, before, changes| Term::Time {
        field,
        after,
        before,
        changes,
        text: text.clone(),
    };
    let changes = |lit: &TimeLit| match lit {
        TimeLit::Period(_, _, changes) => *changes,
        TimeLit::Ago(_) => None,
    };
    if let Some((a, b)) = v.split_once("..") {
        let (a, b) = (parse_time_lit(a, now, cal)?, parse_time_lit(b, now, cal)?);
        let first = [changes(&a), changes(&b)].into_iter().flatten().min();
        let start = match a {
            TimeLit::Ago(t) | TimeLit::Period(t, _, _) => t,
        };
        let end = match b {
            TimeLit::Ago(t) => t,
            TimeLit::Period(_, e, _) => e,
        };
        return Ok(mk(Some(start), Some(end), first));
    }
    let lit = parse_time_lit(v, now, cal)?;
    let changes = changes(&lit);
    Ok(match (op, lit) {
        // "within the last 7d" / "before the date"
        ("<", TimeLit::Period(s, _, _)) => mk(None, Some(s), changes),
        ("<=", TimeLit::Period(_, e, _)) => mk(None, Some(e), changes),
        // "older than 7d" / "after the date"
        (">" | ">=", TimeLit::Ago(t)) => mk(None, Some(t), None),
        (">", TimeLit::Period(_, e, _)) => mk(Some(e), None, changes),
        (">=", TimeLit::Period(s, _, _)) => mk(Some(s), None, changes),
        (_, TimeLit::Period(s, e, _)) => mk(Some(s), Some(e), changes),
        // "within the last 7d" (also the bare `modified:7d` form)
        (_, TimeLit::Ago(t)) => mk(Some(t), None, None),
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
            Term::Kind(kinds) => {
                let words: Vec<&str> = kinds.iter().map(|k| k.name()).collect();
                write!(f, "kind:{}", words.join(","))
            }
            Term::Size { text, .. }
            | Term::Rating { text, .. }
            | Term::Time { text, .. }
            | Term::Near { text, .. }
            | Term::Inside { text, .. } => f.write_str(text),
            Term::Tag(t) => write!(f, "tag:{}", quote(t)),
            Term::Comment(t) => write!(f, "comment:{}", quote(t)),
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
        && !comparison("rating")
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

impl Expr {
    /// The expression for a label (a saved search's name): as printed, but
    /// a name as it was written, without its key or quotes. Never parsed
    /// back.
    pub fn label(&self) -> String {
        let term = |t: &Term| match t {
            Term::Name(p) => p.text.clone(),
            other => other.to_string(),
        };
        let join = |items: &[Expr], sep: &str| {
            items
                .iter()
                .map(|e| match e {
                    Expr::Or(_) if sep == " " => format!("({})", e.label()),
                    _ => e.label(),
                })
                .collect::<Vec<_>>()
                .join(sep)
        };
        match self {
            Expr::Term(t) => term(t),
            Expr::Not(e) => match &**e {
                Expr::Term(t) => format!("NOT {}", term(t)),
                other => format!("NOT ({})", other.label()),
            },
            Expr::And(items) => join(items, " "),
            Expr::Or(alts) => join(alts, " OR "),
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

    /// When the query next means other periods (`today`, at midnight):
    /// what it listed is to be listed again then. `None` when none of its
    /// terms moves on the calendar.
    pub fn next_change(&self) -> Option<SystemTime> {
        fn visit(e: &Expr, first: &mut Option<SystemTime>) {
            match e {
                Expr::Term(Term::Time {
                    changes: Some(t), ..
                }) => *first = Some(first.map_or(*t, |f| f.min(*t))),
                Expr::Term(_) => {}
                Expr::Not(e) => visit(e, first),
                Expr::And(v) | Expr::Or(v) => v.iter().for_each(|e| visit(e, first)),
            }
        }
        let mut first = None;
        visit(self.expr.as_ref()?, &mut first);
        first
    }

    /// Size or time terms: a stat is needed to evaluate.
    pub fn needs_stat(&self) -> bool {
        self.any(&|t| matches!(t, Term::Size { .. } | Term::Time { .. }))
    }

    /// Tag, comment and rating terms: metadata providers must run.
    pub fn needs_metadata(&self) -> bool {
        self.any(&|t| matches!(t, Term::Tag(_) | Term::Comment(_) | Term::Rating { .. }))
    }

    /// Content terms: files must be read.
    pub fn needs_content(&self) -> bool {
        self.any(&|t| matches!(t, Term::Content(_)))
    }

    /// The substrings every match's name contains, when the expression is
    /// exactly that: one substring, or an AND of them. This is what a
    /// locate database answers (it ANDs its patterns); an OR, a NOT, a
    /// glob or any other term needs the walk.
    /// A tag every match has: the query is a `tag:` term, or an AND with
    /// one. What an index of tags can answer first (ADR-026).
    pub fn required_tag(&self) -> Option<&str> {
        fn tag(e: &Expr) -> Option<&str> {
            match e {
                Expr::Term(Term::Tag(t)) => Some(t.as_str()),
                _ => None,
            }
        }
        match self.expr.as_ref()? {
            Expr::And(items) => items.iter().find_map(tag),
            e => tag(e),
        }
    }

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

/// What the late terms read with: the fields a file gives when asked
/// (`near:`, `inside:`), on the search's worker, and the search's token.
#[derive(Clone, Copy)]
pub struct Late<'a> {
    pub fields: &'a bibli_core::fields::Fields,
    pub cancel: &'a bibli_core::CancelToken,
}

/// What the matcher looks at. `metadata` is `None` before stat.
pub struct Candidate<'a> {
    /// The entry's own name, which name terms (`name:`, `ext:`) match.
    pub name: &'a OsStr,
    /// The entry's own kind: a link is `Symlink`.
    pub kind: EntryKind,
    pub path: &'a Path,
    pub metadata: Option<&'a Metadata>,
    /// What it opens, which type terms (`type:`, `kind:`, `mime:`) and the
    /// terms that read the file match: its kind and the name that says
    /// its type. A link's is where it leads (`Entry::target`), and it
    /// stays `Symlink` for a link that leads nowhere. `None` for a link
    /// not yet stat-ed: those terms wait for the stat.
    opens: Option<(EntryKind, &'a OsStr)>,
    /// Without it, a place term is never decided.
    late: Option<Late<'a>>,
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
            opens: (kind != EntryKind::Symlink).then_some((kind, name)),
            late: None,
            text: name.to_string_lossy(),
            lowered: OnceCell::new(),
        }
    }

    /// A stat-ed entry: a link is matched as what it leads to.
    pub fn of_entry(entry: &'a bibli_core::Entry, path: &'a Path) -> Self {
        Candidate {
            opens: Some((entry.opens(), entry.type_name())),
            ..Candidate::new(&entry.name, entry.kind, path, Some(&entry.metadata))
        }
    }

    /// The same candidate, able to read fields for the late terms.
    #[must_use]
    pub fn with_late(mut self, late: Late<'a>) -> Self {
        self.late = Some(late);
        self
    }

    /// Where the file says it was made, read now.
    fn position(&self) -> Option<bibli_core::geo::LatLon> {
        let late = self.late?;
        late.fields
            .get(keys::POSITION, self.path, late.cancel)
            .as_ref()
            .and_then(bibli_core::geo::LatLon::from_value)
    }

    /// The verdict of a term that reads a file, for what is not one (or
    /// not known yet to lead to one): `None` for a file, read through a
    /// link.
    fn unless_file(&self) -> Option<Verdict> {
        match self.opens {
            Some((EntryKind::File, _)) => None,
            Some(_) => Some(Verdict::No),
            None => Some(Verdict::Unknown),
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
            // A link is one whatever it leads to; a link to a folder is a
            // folder too, as it is shown and opened.
            Term::Type(sel) => match (sel, c.opens) {
                (KindSel::Symlink, _) => yes_no(c.kind == EntryKind::Symlink),
                (_, None) => Verdict::Unknown,
                (KindSel::File, Some((kind, _))) => yes_no(kind == EntryKind::File),
                (KindSel::Dir, Some((kind, _))) => yes_no(kind == EntryKind::Directory),
            },
            Term::Mime(g, _) => match c.opens {
                None => Verdict::Unknown,
                Some((EntryKind::Symlink, _)) => yes_no(g.is_match("inode/symlink")),
                // A folder, a pipe, a device: what it is, whatever its name.
                Some((kind, name)) => yes_no(match kind.inode_type() {
                    Some(inode) => g.is_match(inode),
                    None => {
                        bibli_platform::mime::by_name(name).is_some_and(|m| g.is_match(m.as_str()))
                    }
                }),
            },
            Term::Kind(kinds) => {
                use bibli_platform::mime::{Kind, is_kind};
                match c.opens {
                    None => Verdict::Unknown,
                    Some((EntryKind::Directory, _)) => yes_no(kinds.contains(&Kind::Folder)),
                    // A link that leads nowhere, a pipe or a device is of
                    // no kind, whatever its name.
                    Some((kind, _)) if kind == EntryKind::Symlink || kind.is_special() => {
                        Verdict::No
                    }
                    Some((_, name)) => yes_no(
                        bibli_platform::mime::by_name(name)
                            .is_some_and(|m| kinds.iter().any(|k| is_kind(&m, *k))),
                    ),
                }
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
            Term::Comment(text) => {
                let Some(m) = c.metadata else {
                    return Verdict::Unknown;
                };
                yes_no(
                    m.value(keys::COMMENT)
                        .and_then(Value::as_str)
                        .is_some_and(|comment| comment.to_lowercase().contains(text.as_str())),
                )
            }
            Term::Rating { min, max, .. } => {
                let Some(m) = c.metadata else {
                    return Verdict::Unknown;
                };
                let rating = m.value(keys::RATING).and_then(Value::as_u64).unwrap_or(0);
                yes_no((*min..=*max).contains(&rating))
            }
            Term::Content(p) => {
                if let Some(verdict) = c.unless_file() {
                    return verdict;
                }
                if !content {
                    return Verdict::Unknown;
                }
                yes_no(content_matches(c.path, p))
            }
            Term::Near { .. } | Term::Inside { .. } => {
                if let Some(verdict) = c.unless_file() {
                    return verdict;
                }
                if !content {
                    return Verdict::Unknown;
                }
                yes_no(c.position().is_some_and(|at| self.holds(at)))
            }
        }
    }
}

impl Term {
    /// Whether a place term holds a position; false for any other term.
    fn holds(&self, at: bibli_core::geo::LatLon) -> bool {
        match self {
            Term::Near {
                at: center, radius, ..
            } => center.distance(at) <= *radius,
            Term::Inside {
                south,
                west,
                north,
                east,
                ..
            } => {
                (*south..=*north).contains(&at.lat)
                    && if west <= east {
                        (*west..=*east).contains(&at.lon)
                    } else {
                        at.lon >= *west || at.lon <= *east
                    }
            }
            _ => false,
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
    use bibli_platform::calendar::Weekday;
    use std::time::Duration;

    /// Parse on a UTC calendar with weeks from Monday: what these tests
    /// are about is not the zone, which the ones below vary.
    fn parse_at(s: &str, now: SystemTime) -> Result<Query, ParseError> {
        parse_in(s, now, &Calendar::utc())
    }

    fn civil_to_time(y: i64, m: u32, d: u32) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(days_from_civil(y, m, d).unsigned_abs() * 86_400)
    }

    fn now() -> SystemTime {
        // 2026-09-18T12:00:00Z
        civil_to_time(2026, 9, 18) + Duration::from_secs(12 * 3600)
    }

    fn q(s: &str) -> Query {
        parse_at(s, now()).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    /// Answers `position` from a text file holding `lat,lon`.
    struct Written;
    impl bibli_core::fields::FieldProvider for Written {
        fn id(&self) -> &'static str {
            "written"
        }
        fn keys(&self) -> &'static [&'static str] {
            &[keys::POSITION]
        }
        fn accepts(&self, _: &str, _: &OsStr) -> bool {
            true
        }
        fn read(
            &self,
            _: &str,
            path: &Path,
            _: &bibli_core::CancelToken,
        ) -> anyhow::Result<Option<Value>> {
            let text = std::fs::read_to_string(path)?;
            let (lat, lon) = text.trim().split_once(',').unwrap();
            Ok(bibli_core::geo::LatLon::new(lat.parse()?, lon.parse()?)
                .map(bibli_core::geo::LatLon::to_value))
        }
    }

    #[test]
    fn places_parse_and_print_back_as_one_meaning() {
        assert_eq!(
            q("near:10.120000,20.12~5km").to_string(),
            "near:10.12,20.12~5km"
        );
        assert_eq!(q("near:10.12,20.12").to_string(), "near:10.12,20.12~1km");
        assert_eq!(
            q("near:10.12,20.12~500m").to_string(),
            "near:10.12,20.12~500m"
        );
        assert_eq!(
            q("near:10.12,20.12~2.5").to_string(),
            "near:10.12,20.12~2.5km"
        );
        assert_eq!(
            q("inside:9.9,19.9,10.2,20.3 kind:image").to_string(),
            "inside:9.9,19.9,10.2,20.3 kind:image"
        );
        for bad in [
            "near:95,20",
            "near:10",
            "near:10,20~-3km",
            "near:10,20~far",
            "inside:10.2,19.9,9.9,20.3",
            "inside:9.9,19.9,10.2",
        ] {
            assert!(parse_at(bad, now()).is_err(), "{bad}");
        }
    }

    #[test]
    fn places_hold_the_files_that_say_they_were_made_there() {
        let tmp = tempfile::tempdir().unwrap();
        let fields = bibli_core::fields::Fields::default();
        fields.register("test", std::sync::Arc::new(Written));
        let cancel = bibli_core::CancelToken::new();
        let late = Late {
            fields: &fields,
            cancel: &cancel,
        };
        let file = |name: &str, at: &str| {
            let path = tmp.path().join(name);
            std::fs::write(&path, at).unwrap();
            path
        };
        // 1.1 km apart north to south; across ±180° for the box.
        let here = file("here.jpg", "10.12,20.12");
        let north = file("north.jpg", "10.13,20.12");
        let over = file("over the line.jpg", "60.0,-179.9");
        let decide = |query: &str, path: &Path| {
            let c = Candidate::new(path.file_name().unwrap(), EntryKind::File, path, None)
                .with_late(late);
            let expr = q(query).expr.unwrap();
            // Nothing is read before the late pass.
            assert_eq!(expr.eval(&c, false), Verdict::Unknown, "{query}");
            expr.eval(&c, true)
        };
        assert_eq!(decide("near:10.12,20.12~500m", &here), Verdict::Yes);
        assert_eq!(decide("near:10.12,20.12~500m", &north), Verdict::No);
        assert_eq!(decide("near:10.12,20.12~2km", &north), Verdict::Yes);
        assert_eq!(decide("inside:10,20,10.125,20.2", &here), Verdict::Yes);
        assert_eq!(decide("inside:10,20,10.125,20.2", &north), Verdict::No);
        assert_eq!(decide("inside:59,179.5,61,-179.5", &over), Verdict::Yes);
        assert_eq!(decide("inside:59,-179.5,61,179.5", &over), Verdict::No);
        // A folder, or a candidate that cannot read fields, is never near.
        let dir = Candidate::new(OsStr::new("d"), EntryKind::Directory, tmp.path(), None);
        assert_eq!(
            q("near:10.12,20.12").expr.unwrap().eval(&dir, true),
            Verdict::No
        );
        let blind = Candidate::new(OsStr::new("here.jpg"), EntryKind::File, &here, None);
        assert_eq!(
            q("near:10.12,20.12").expr.unwrap().eval(&blind, true),
            Verdict::No
        );
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
            "kind:image,video",
            "modified:this-week",
            "modified>=last-month",
            "report in:/tmp/x depth:2 hidden:true case:sensitive",
            "name:\"two words\"",
            "rating>=4 comment:\"from the trip\"",
            "rating:2.5..4 NOT rating:0",
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
            ("modified<someday", "bad date"),
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
        assert_eq!(q("tag:x").required_tag(), Some("x"));
        assert_eq!(q("ext:pdf tag:\"to do\"").required_tag(), Some("to do"));
        assert_eq!(q("tag:x OR tag:y").required_tag(), None);
        assert_eq!(q("-tag:x").required_tag(), None);
        assert_eq!(q("ext:pdf").required_tag(), None);
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
    #[cfg(target_os = "linux")]
    fn kinds_come_from_the_mime_database() {
        let yes = |expr: &str, name: &str, kind: EntryKind| {
            q(expr).expr.unwrap().eval(&cand(name, kind, None), false) == Verdict::Yes
        };
        assert!(yes("kind:document", "Report-Final.PDF", EntryKind::File));
        assert!(
            yes("kind:documents", "notes.md", EntryKind::File),
            "a plural"
        );
        assert!(
            yes("kind:text", "notes.md", EntryKind::File),
            "and text too"
        );
        assert!(yes("kind:image,video", "clip.mp4", EntryKind::File));
        assert!(yes("kind:photos", "cat.JPG", EntryKind::File));
        assert!(yes("kind:music", "song.flac", EntryKind::File));
        assert!(yes("kind:archive", "backup.tar.gz", EntryKind::File));
        assert!(yes("kind:folder", "photos", EntryKind::Directory));
        assert!(
            !yes("kind:image", "photos", EntryKind::Directory),
            "a folder is a folder"
        );
        assert!(!yes("kind:image", "Report-Final.PDF", EntryKind::File));
        assert!(!yes("kind:document", "no-extension", EntryKind::File));
        assert!(yes("-kind:image", "Report-Final.PDF", EntryKind::File));
        assert_eq!(
            q("kind:Photos,images,video").to_string(),
            "kind:image,video"
        );
        for bad in ["kind:pdf", "kind:", "kind:,"] {
            let error = parse_at(bad, now()).unwrap_err().to_string();
            assert!(error.starts_with("kind:"), "{bad}: {error}");
        }
    }

    /// The instants a named period covers, `[start, end)`, from `now`.
    fn period(expr: &str, now: SystemTime) -> (SystemTime, SystemTime) {
        match parse_at(expr, now).unwrap().expr {
            Some(Expr::Term(Term::Time {
                after: Some(a),
                before: Some(b),
                ..
            })) => (a, b),
            other => panic!("{expr}: {other:?}"),
        }
    }

    #[test]
    fn named_periods_are_calendar_periods_from_now() {
        let at = |y, m, d| civil_to_time(y, m, d);
        // Friday 2026-09-25, 23:59:59 UTC.
        let friday = at(2026, 9, 26) - Duration::from_secs(1);
        let p = |name: &str| period(&format!("modified:{name}"), friday);
        assert_eq!(p("today"), (at(2026, 9, 25), at(2026, 9, 26)));
        assert_eq!(p("yesterday"), (at(2026, 9, 24), at(2026, 9, 25)));
        assert_eq!(
            p("this-week"),
            (at(2026, 9, 21), at(2026, 9, 28)),
            "from Monday"
        );
        assert_eq!(p("last-week"), (at(2026, 9, 14), at(2026, 9, 21)));
        assert_eq!(p("this-month"), (at(2026, 9, 1), at(2026, 10, 1)));
        assert_eq!(p("last-month"), (at(2026, 8, 1), at(2026, 9, 1)));
        assert_eq!(p("this-year"), (at(2026, 1, 1), at(2027, 1, 1)));
        assert_eq!(p("Last-Year"), (at(2025, 1, 1), at(2026, 1, 1)), "any case");
        // On a Monday at midnight the week has just begun.
        let monday = at(2026, 9, 21);
        assert_eq!(
            period("modified:this-week", monday),
            (at(2026, 9, 21), at(2026, 9, 28))
        );
        assert_eq!(
            period("modified:this-week", monday - Duration::from_secs(1)),
            (at(2026, 9, 14), at(2026, 9, 21)),
            "one second before, the week before"
        );
        // A week across the new year, and last month in January.
        let new_year = at(2027, 1, 1);
        assert_eq!(
            period("modified:this-week", new_year),
            (at(2026, 12, 28), at(2027, 1, 4))
        );
        assert_eq!(
            period("modified:last-month", new_year),
            (at(2026, 12, 1), at(2027, 1, 1))
        );
        // The comparisons read them as they read dates.
        let t = |expr: &str, instant: SystemTime| {
            let m = md(1, instant, &[]);
            parse_at(expr, friday)
                .unwrap()
                .expr
                .unwrap()
                .eval(&cand("f", EntryKind::File, Some(&m)), false)
        };
        let monday_noon = at(2026, 9, 21) + Duration::from_secs(12 * 3600);
        assert_eq!(t("modified:this-week", monday_noon), Verdict::Yes);
        assert_eq!(t("modified<this-week", monday_noon), Verdict::No);
        assert_eq!(t("modified<today", monday_noon), Verdict::Yes);
        assert_eq!(t("modified>=this-month", at(2026, 8, 31)), Verdict::No);
        assert_eq!(
            t("modified:last-month..yesterday", at(2026, 9, 24)),
            Verdict::Yes
        );
        assert!(parse_at("modified:next-week", friday).is_err());
    }

    fn paris(week_start: Weekday) -> Calendar {
        Calendar::named("Europe/Paris", week_start).expect("the tz database has Paris")
    }

    /// The user's day, not UTC's: late on the 25th in UTC is already the
    /// 26th in Paris, and a day of the clocks changing is 23 or 25 hours.
    #[test]
    fn dates_and_periods_are_days_of_the_users_zone() {
        let cal = paris(Weekday::Monday);
        let hour = |y, m, d, h: u64| civil_to_time(y, m, d) + Duration::from_secs(h * 3600);
        let span = |expr: &str, now: SystemTime| match parse_in(expr, now, &cal).unwrap().expr {
            Some(Expr::Term(Term::Time {
                after: Some(a),
                before: Some(b),
                ..
            })) => (a, b),
            other => panic!("{expr}: {other:?}"),
        };
        // 23:30 UTC on the 25th: 01:30 on the 26th in Paris.
        let late = hour(2026, 9, 25, 23) + Duration::from_secs(1800);
        assert_eq!(
            span("modified:today", late),
            (hour(2026, 9, 25, 22), hour(2026, 9, 26, 22))
        );
        assert_eq!(
            span("modified:2026-09-26", late),
            span("modified:today", late),
            "a date is the same day"
        );
        // The day the clocks go back is 25 hours long.
        let (a, b) = span("modified:2026-10-25", late);
        assert_eq!(b.duration_since(a).unwrap().as_secs(), 25 * 3600);
        // A month begins at its local midnight, in winter time here.
        assert_eq!(
            span("modified:2026-12", late),
            (hour(2026, 11, 30, 23), hour(2026, 12, 31, 23))
        );
        // Weeks start on the locale's day.
        let sunday = paris(Weekday::Sunday);
        let friday = hour(2026, 9, 25, 10);
        let week = |cal: &Calendar| match parse_in("modified:this-week", friday, cal).unwrap().expr
        {
            Some(Expr::Term(Term::Time { after: Some(a), .. })) => a,
            other => panic!("{other:?}"),
        };
        assert_eq!(week(&cal), hour(2026, 9, 20, 22), "Monday the 21st");
        assert_eq!(week(&sunday), hour(2026, 9, 19, 22), "Sunday the 20th");
    }

    #[test]
    fn a_named_period_says_when_it_moves_on() {
        let cal = paris(Weekday::Monday);
        let hour = |y, m, d, h: u64| civil_to_time(y, m, d) + Duration::from_secs(h * 3600);
        let friday = hour(2026, 9, 25, 10);
        let next = |expr: &str| parse_in(expr, friday, &cal).unwrap().next_change();
        // The next local midnight, week, month.
        assert_eq!(next("modified:today"), Some(hour(2026, 9, 25, 22)));
        assert_eq!(next("modified:yesterday"), Some(hour(2026, 9, 25, 22)));
        assert_eq!(next("modified>=this-week"), Some(hour(2026, 9, 27, 22)));
        assert_eq!(next("modified:last-month"), Some(hour(2026, 9, 30, 22)));
        // The first of several; nothing for dates and durations.
        assert_eq!(
            next("kind:image (modified:this-year OR created:today)"),
            Some(hour(2026, 9, 25, 22))
        );
        assert_eq!(next("modified:2026-09 size>1M"), None);
        assert_eq!(next("modified<7d"), None);
        assert_eq!(next("NOT modified:this-month"), Some(hour(2026, 9, 30, 22)));
    }

    #[test]
    fn ratings_and_comments_are_what_the_file_says() {
        let rated = |rating: Option<u64>, comment: Option<&str>| {
            let mut m = md(1, now(), &[]);
            if let Some(r) = rating {
                m.set(keys::RATING, Value::UInt(r), Provenance::User);
            }
            if let Some(c) = comment {
                m.set(keys::COMMENT, Value::Text(c.into()), Provenance::User);
            }
            m
        };
        let holds = |expr: &str, m: &Metadata| {
            q(expr)
                .expr
                .unwrap()
                .eval(&cand("a.jpg", EntryKind::File, Some(m)), false)
        };
        let four = rated(Some(8), Some("From the Trip, day 2"));
        let half = rated(Some(7), None);
        let none = rated(None, None);
        assert_eq!(holds("rating>=4", &four), Verdict::Yes);
        assert_eq!(holds("rating>=4", &half), Verdict::No);
        assert_eq!(holds("rating:3.5", &half), Verdict::Yes);
        assert_eq!(holds("rating>3", &half), Verdict::Yes, "3½ is more than 3");
        assert_eq!(holds("rating:3..4", &half), Verdict::Yes);
        assert_eq!(holds("rating:0", &none), Verdict::Yes, "unrated");
        assert_eq!(holds("rating<1", &four), Verdict::No);
        assert_eq!(holds("comment:trip", &four), Verdict::Yes, "any case");
        assert_eq!(holds("comment:\"day 3\"", &four), Verdict::No);
        assert_eq!(holds("comment:trip", &none), Verdict::No);
        for bad in ["rating:6", "rating>=x", "rating:2.2", "rating<0"] {
            assert!(parse_at(bad, now()).is_err(), "{bad}");
        }
        assert!(q("rating>=4").needs_metadata() && q("comment:x").needs_metadata());
        let unread = cand("a.jpg", EntryKind::File, None);
        assert_eq!(
            q("rating>=4").expr.unwrap().eval(&unread, false),
            Verdict::Unknown
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
        assert_eq!(b - a, 28, "days");
        let (a, b) = parse_date("2024").unwrap();
        assert_eq!(b - a, 366);
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
