//! What the address field asks of the query language while it is typed
//! in: the pieces of the text and what each is ([`tokens_at`], for its
//! colours), what the term at the caret can go on with ([`complete`]),
//! whether the text parses ([`check`], [`problem`]), and a reading of
//! plain words as terms ([`interpret`], the rule half of `query.ask`).
//!
//! Pure. The field draws what these answer and knows no syntax: a term is
//! split as the parser splits it, and checked by parsing it alone, so a
//! message is the parser's own. A term still being typed at the caret (a
//! key with no value yet, the start of a value offered, the digits of a
//! date) is not an error until the caret leaves it.

use std::ops::Range;

use serde::Serialize;
use skwir_core::app::App;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::metadata::{same_tag, tag_leaf};
use skwir_core::workspace::PaneId;
use skwir_platform::mime::Kind;

use crate::facets::{PERIODS, SIZE};
use crate::lang;

/// A key of the language: its name, the icon its values are drawn with,
/// what it takes (said beside it when offered), and whether it is an
/// option (where and how a search looks) rather than a test.
struct KeyInfo {
    name: &'static str,
    icon: &'static str,
    example: &'static str,
    option: bool,
}

/// Every key the parser knows, in the order they are offered: what is
/// asked most first.
static KEYS: [KeyInfo; 20] = [
    key("kind", "grid", "image, video, document…", false),
    key(
        "modified",
        "calendar",
        "today, last-week, 2026-05, <7d",
        false,
    ),
    key("size", "sliders", ">100M, <1M, 1M..100M", false),
    key("tag", "tag", "a tag, or * for any; #tag too", false),
    key("content", "contents", "\"words\" or /regex/", false),
    key(
        "taken",
        "calendar",
        "2023, last-year, by the camera's clock",
        false,
    ),
    key("name", "search", "*.pdf, report-??", false),
    key("ext", "search", "pdf,jpg", false),
    key("type", "search", "file, dir, symlink", false),
    key("created", "calendar", "this-month, >1y", false),
    key("accessed", "calendar", "yesterday, <3d", false),
    key("rating", "star", ">=3, 2..4", false),
    key("comment", "comment", "words in a file's comment", false),
    key("mime", "search", "image/*, application/pdf", false),
    key("near", "map", "45.0,6.0~5km", false),
    key("inside", "map", "south,west,north,east", false),
    key("in", "folder", "a folder, \"~/My Files\"", true),
    key("depth", "layers", "how deep: 2", true),
    key("hidden", "eye", "true, false", true),
    key("case", "code", "sensitive, insensitive", true),
];

const fn key(
    name: &'static str,
    icon: &'static str,
    example: &'static str,
    option: bool,
) -> KeyInfo {
    KeyInfo {
        name,
        icon,
        example,
        option,
    }
}

/// The terms a search takes, the most asked first: each key as written
/// before its value (`kind:`), its icon and examples of values.
/// What `?` in the address field lists.
pub fn terms() -> Vec<(String, &'static str, &'static str)> {
    KEYS.iter()
        .filter(|k| !k.option)
        .map(|k| (format!("{}:", k.name), k.icon, k.example))
        .collect()
}

fn key_info(name: &str) -> Option<&'static KeyInfo> {
    KEYS.iter().find(|k| k.name == name)
}

/// Keys written before a comparison with no colon (`size>10M`).
const COMPARED: [&str; 6] = ["size", "rating", "modified", "created", "accessed", "taken"];

/// The times a value is a date or a period of.
const TIMES: [&str; 4] = ["modified", "created", "accessed", "taken"];

/// How many stars, as the field offers them.
const RATINGS: [(&str, &str); 4] = [
    ("One star or more", ">=1"),
    ("Three stars or more", ">=3"),
    ("Five stars", "5"),
    ("Unrated", "0"),
];

/// Kinds by another of their words, offered when no kind's own name
/// starts with what is typed (`pic` for images).
const KIND_SYNONYMS: [(&str, Kind); 12] = [
    ("pictures", Kind::Image),
    ("photos", Kind::Image),
    ("music", Kind::Audio),
    ("songs", Kind::Audio),
    ("movies", Kind::Video),
    ("films", Kind::Video),
    ("docs", Kind::Document),
    ("slides", Kind::Presentation),
    ("code", Kind::Text),
    ("apps", Kind::Program),
    ("dirs", Kind::Folder),
    ("directories", Kind::Folder),
];

// ---------------------------------------------------------------- lexing

/// A piece of the text as the parser reads it.
#[derive(Clone, Debug)]
enum Lexeme {
    /// `(`, `)`, a `-` before a word or a group, `OR`, `NOT`.
    Operator(Range<usize>),
    Word(WordLex),
}

#[derive(Clone, Debug)]
struct WordLex {
    range: Range<usize>,
    /// Its key, and where its value starts: after the colon, or after the
    /// comparison of a key written without one (`size>`).
    key: Option<(&'static str, usize)>,
    quoted: bool,
    /// A quote opened and not closed.
    unterminated: bool,
}

impl WordLex {
    fn value<'a>(&self, text: &'a str) -> &'a str {
        let start = self.key.map_or(self.range.start, |(_, at)| at);
        &text[start..self.range.end]
    }
}

fn is_break(c: char) -> bool {
    c.is_whitespace() || c == '(' || c == ')'
}

/// The pieces of `text`, with their byte ranges, split as `lang`'s
/// tokenizer splits them.
fn lex(text: &str) -> Vec<Lexeme> {
    let cs: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| cs.get(i).map(|&(_, c)| c);
    let off = |i: usize| cs.get(i).map_or(text.len(), |&(o, _)| o);
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(c) = at(i) {
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '(' || c == ')' {
            out.push(Lexeme::Operator(off(i)..off(i + 1)));
            i += 1;
            continue;
        }
        if c == '-' && at(i + 1).is_some_and(|n| !n.is_whitespace() && n != ')') {
            out.push(Lexeme::Operator(off(i)..off(i + 1)));
            i += 1;
            continue;
        }
        let start = i;
        let mut key = None;
        if let Some(k) = KEYS.iter().find(|k| {
            let n = k.name.len();
            (0..n).all(|j| at(i + j) == k.name[j..].chars().next()) && at(i + n) == Some(':')
        }) {
            i += k.name.len() + 1;
            key = Some((k.name, off(i)));
        } else if c == '#' {
            i += 1;
            key = Some(("tag", off(i)));
        }
        let mut quoted = false;
        let mut unterminated = false;
        let first = at(i);
        if key.is_some_and(|(k, _)| k == "content")
            && first == Some('/')
            && let Some(end) = regex_end(&cs, i + 1)
        {
            i = end + 1;
        } else if let Some(q @ ('"' | '\'')) = first {
            quoted = true;
            i += 1;
            loop {
                match at(i) {
                    None => {
                        unterminated = true;
                        break;
                    }
                    Some(c) if c == q => {
                        i += 1;
                        break;
                    }
                    Some('\\') => {
                        if at(i + 1).is_none() {
                            unterminated = true;
                            i += 1;
                            break;
                        }
                        i += 2;
                    }
                    Some(_) => i += 1,
                }
            }
        }
        while at(i).is_some_and(|c| !is_break(c)) {
            i += 1;
        }
        let range = off(start)..off(i);
        let word = &text[range.clone()];
        if key.is_none() && !quoted {
            if word == "OR" || word == "NOT" {
                out.push(Lexeme::Operator(range));
                continue;
            }
            key = compared(word).map(|(k, op)| (k, range.start + k.len() + op.len()));
        }
        out.push(Lexeme::Word(WordLex {
            range,
            key,
            quoted,
            unterminated,
        }));
    }
    out
}

/// The `/` closing a regex whose body starts at `from`.
fn regex_end(cs: &[(usize, char)], from: usize) -> Option<usize> {
    let mut i = from;
    while let Some(&(_, c)) = cs.get(i) {
        match c {
            '\\' => i += 2,
            '/' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// A key written before a comparison (`size>=`), and the comparison.
fn compared(word: &str) -> Option<(&'static str, &'static str)> {
    COMPARED.iter().find_map(|k| {
        let rest = word.strip_prefix(k)?;
        [">=", "<=", ">", "<", "="]
            .into_iter()
            .find(|op| rest.starts_with(op))
            .map(|op| (*k, op))
    })
}

/// The word that ends at `caret`, if one does.
fn word_at(lexemes: &[Lexeme], caret: usize) -> Option<&WordLex> {
    lexemes.iter().find_map(|l| match l {
        Lexeme::Word(w) if w.range.end == caret && w.range.start < caret => Some(w),
        _ => None,
    })
}

/// Whether `w` parses on its own, else the parser's message.
fn verdict(text: &str, w: &WordLex) -> Result<(), String> {
    if w.unterminated {
        return Err("unterminated quote".into());
    }
    lang::parse(&text[w.range.clone()])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Whether `w`, at the caret, is a term still being typed: a key with no
/// value yet, a value that starts one offered, the digits of a date, a
/// quote not closed yet.
fn typing(text: &str, w: &WordLex, caret: usize) -> bool {
    if w.range.end != caret {
        return false;
    }
    if w.unterminated {
        return true;
    }
    let Some((key, _)) = w.key else {
        return false;
    };
    let value = w.value(text);
    value.is_empty()
        || !values(text, w, key, &[]).is_empty()
        || (TIMES.contains(&key)
            && value
                .chars()
                .all(|c| c.is_ascii_digit() || c == '-' || c == '.'))
}

// ---------------------------------------------------------------- tokens

/// What a piece of the text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// A word matched against names.
    Word,
    /// A term's key with its colon or comparison (`kind:`, `size>`).
    Key,
    /// What follows a key.
    Value,
    /// An option's key (`in:`, `depth:`, `hidden:`, `case:`): where and
    /// how a search looks, not a test.
    Option,
    /// `(`, `)`, `-`, `OR`, `NOT`.
    Operator,
    /// A term that does not parse; `message` says why.
    Error,
}

/// A piece of the text: its byte range, what it is, the key it belongs
/// to, and why it is an error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: SpanKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Span {
    fn new(range: Range<usize>, kind: SpanKind, facet: Option<&str>) -> Self {
        Span {
            range,
            kind,
            facet: facet.map(str::to_owned),
            message: None,
        }
    }
}

/// The pieces of `text`, the caret at its end.
pub fn tokens(text: &str) -> Vec<Span> {
    tokens_at(text, text.len())
}

/// The pieces of `text`: words, keys and values, operators, and terms
/// that do not parse, each with the parser's message. The term the caret
/// ends while it is still being typed is a key and a value, not an error.
pub fn tokens_at(text: &str, caret: usize) -> Vec<Span> {
    let mut out = Vec::new();
    for lexeme in lex(text) {
        let w = match lexeme {
            Lexeme::Operator(range) => {
                out.push(Span::new(range, SpanKind::Operator, None));
                continue;
            }
            Lexeme::Word(w) => w,
        };
        if let Err(message) = verdict(text, &w)
            && !typing(text, &w, caret)
        {
            out.push(Span {
                message: Some(message),
                ..Span::new(w.range, SpanKind::Error, w.key.map(|(k, _)| k))
            });
            continue;
        }
        match w.key {
            None => out.push(Span::new(w.range, SpanKind::Word, None)),
            Some((key, at)) => {
                let option = key_info(key).is_some_and(|k| k.option);
                let kind = if option {
                    SpanKind::Option
                } else {
                    SpanKind::Key
                };
                out.push(Span::new(w.range.start..at, kind, Some(key)));
                if at < w.range.end {
                    out.push(Span::new(at..w.range.end, SpanKind::Value, Some(key)));
                }
            }
        }
    }
    out
}

/// Why `text` does not parse, as the parser says it; `None` when it does.
pub fn check(text: &str) -> Option<String> {
    lang::parse(text).err().map(|e| e.to_string())
}

/// Whether `text` is words alone: no key, comparison or operator.
pub fn only_words(text: &str) -> bool {
    lex(text)
        .iter()
        .all(|l| matches!(l, Lexeme::Word(w) if w.key.is_none()))
}

/// `text` with each tag written `#tag` searched as a name instead
/// (`"#3"`), for a name that has one; `None` when it has none.
pub fn hashes_as_names(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len() + 2);
    let mut at = 0;
    for lexeme in lex(text) {
        if let Lexeme::Word(w) = lexeme
            && !w.quoted
            && w.range.len() > 1
            && text[w.range.clone()].starts_with('#')
        {
            out.push_str(&text[at..w.range.start]);
            out.push_str(&lang::quote(&text[w.range.clone()]));
            at = w.range.end;
        }
    }
    (at > 0).then(|| out + &text[at..])
}

/// Why `text` cannot be searched yet, as the field shows it with the
/// caret at `caret`: the first term that does not parse, else what the
/// parser says of the whole. A term still being typed at the caret, an
/// operator or an open parenthesis just typed, is not a problem yet.
pub fn problem(text: &str, caret: usize) -> Option<String> {
    let spans = tokens_at(text, caret);
    if let Some(span) = spans.iter().find(|s| s.kind == SpanKind::Error) {
        return span.message.clone();
    }
    let error = check(text)?;
    let at_end = caret >= text.trim_end().len();
    let last = lex(text).pop();
    let still_typing = match &last {
        Some(Lexeme::Operator(r)) => {
            at_end && r.end == text.trim_end().len() && &text[r.clone()] != ")"
        }
        Some(Lexeme::Word(w)) => typing(text, w, caret) || (at_end && error == "missing `)`"),
        None => false,
    };
    (!still_typing).then_some(error)
}

// ---------------------------------------------------------------- complete

/// Whether the last term of `text` opens a quote it does not close: what
/// is still typed then is the quoted text, not a value.
pub fn quote_open(text: &str) -> bool {
    matches!(lex(text).last(), Some(Lexeme::Word(w)) if w.unterminated)
}

/// What the term ending at the caret can go on with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Completed {
    /// The bytes of the text an item's `text` replaces.
    pub range: Range<usize>,
    pub items: Vec<Item>,
    /// The term there is still being typed: not an error yet.
    pub typing: bool,
}

/// One way to go on: what it reads as, the term it writes in place of
/// `Completed::range`, a word on it, its icon, and the tag it names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    pub label: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub icon: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// What the term ending at `caret` (a byte offset) can go on with: a word
/// of two letters or more that starts a key's name offers the key; a key
/// offers its values (kinds, `tags`, periods, sizes, stars, the words an
/// option takes) that go on with what is typed, after the last comma of a
/// list. A term already whole offers only what goes on from it.
pub fn complete(text: &str, caret: usize, tags: &[String]) -> Completed {
    let caret = floor_boundary(text, caret);
    let lexemes = lex(text);
    let Some(w) = word_at(&lexemes, caret) else {
        return Completed {
            range: caret..caret,
            ..Completed::default()
        };
    };
    let range = w.range.clone();
    let items = match w.key {
        _ if w.unterminated => Vec::new(),
        None if w.quoted => Vec::new(),
        None => keys(&text[range.clone()]),
        Some((key, _)) => values(text, w, key, tags),
    };
    // Any tag parses, so a tag is still being typed while it only starts
    // the names of the tags offered (`#pr`): Enter takes the first.
    let unnamed = w.key.is_some_and(|(k, _)| k == "tag")
        && !items.is_empty()
        && !tags.iter().any(|t| same_tag(t, w.value(text)));
    let typing = (verdict(text, w).is_err() && typing(text, w, caret)) || unnamed;
    Completed {
        range,
        items,
        typing,
    }
}

/// The keys a word starts: `ki` offers `kind:`.
fn keys(word: &str) -> Vec<Item> {
    let word = word.to_ascii_lowercase();
    if word.chars().count() < 2 {
        return Vec::new();
    }
    KEYS.iter()
        .filter(|k| k.name.starts_with(&word))
        .map(|k| Item {
            label: format!("{}:", k.name),
            text: format!("{}:", k.name),
            detail: Some(k.example.to_owned()),
            icon: k.icon.to_owned(),
            tag: None,
        })
        .collect()
}

/// The values of `key` that go on with what `w` holds.
fn values(text: &str, w: &WordLex, key: &'static str, tags: &[String]) -> Vec<Item> {
    if w.quoted || w.unterminated {
        return Vec::new();
    }
    let icon = key_info(key).map_or("search", |k| k.icon);
    let whole = &text[w.range.clone()];
    let value = w.value(text);
    let presets: &[(&str, &str)] = match key {
        "size" => &SIZE,
        "rating" => &RATINGS,
        k if TIMES.contains(&k) => &PERIODS,
        _ => &[],
    };
    if !presets.is_empty() {
        // What follows the key's name, written whole for these (never
        // `#`): the colon or comparison, then the value.
        let after = &whole[key.len()..];
        return presets
            .iter()
            .filter_map(|(label, preset)| {
                let form = if preset.starts_with(['<', '>', '=']) {
                    (*preset).to_owned()
                } else {
                    format!(":{preset}")
                };
                let goes_on = value.is_empty() || (form.starts_with(after) && form != after);
                goes_on.then(|| {
                    let term = format!("{key}{form}");
                    Item {
                        label: (*label).to_owned(),
                        text: term.clone(),
                        detail: Some(term),
                        icon: icon.to_owned(),
                        tag: None,
                    }
                })
            })
            .collect();
    }
    let prefix = &whole[..whole.len() - value.len()];
    let (head, typed) = match key {
        "kind" => value.rsplit_once(',').map_or(("", value), |(h, t)| (h, t)),
        _ => ("", value),
    };
    let head = if head.is_empty() {
        String::new()
    } else {
        format!("{head},")
    };
    let typed_lower = typed.to_lowercase();
    let item = |label: String, written: &str, tag: Option<String>| {
        let term = format!("{prefix}{head}{written}");
        Item {
            label,
            detail: Some(term.clone()),
            text: term,
            icon: icon.to_owned(),
            tag,
        }
    };
    let words = |names: &[&str]| -> Vec<Item> {
        names
            .iter()
            .filter(|n| n.starts_with(&typed_lower) && **n != typed_lower)
            .map(|n| item(capitalized(n), n, None))
            .collect()
    };
    match key {
        "kind" => {
            let listed: Vec<&str> = head.split(',').filter(|h| !h.is_empty()).collect();
            let mut found: Vec<Kind> = Kind::ALL
                .into_iter()
                .filter(|k| k.name().starts_with(&typed_lower) && k.name() != typed_lower)
                .collect();
            if found.is_empty() && !typed_lower.is_empty() {
                for (word, kind) in KIND_SYNONYMS {
                    if word.starts_with(&typed_lower) && !found.contains(&kind) {
                        found.push(kind);
                    }
                }
            }
            found
                .into_iter()
                .filter(|k| !listed.contains(&k.name()))
                .map(|k| item(k.title().to_owned(), k.name(), None))
                .collect()
        }
        "type" => words(&["file", "dir", "symlink"]),
        "hidden" => words(&["true", "false"]),
        "case" => words(&["sensitive", "insensitive"]),
        "tag" => tags
            .iter()
            .filter(|t| {
                let lower = t.to_lowercase();
                lower != typed_lower
                    && (lower.starts_with(&typed_lower)
                        || tag_leaf(t).to_lowercase().starts_with(&typed_lower))
            })
            .map(|t| item(t.clone(), &lang::quote(t), Some(t.clone())))
            .collect(),
        _ => Vec::new(),
    }
}

fn capitalized(word: &str) -> String {
    let mut cs = word.chars();
    cs.next()
        .map(|c| c.to_uppercase().chain(cs).collect())
        .unwrap_or_default()
}

/// `caret` moved back to the start of the character it falls in.
fn floor_boundary(text: &str, caret: usize) -> usize {
    let mut at = caret.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

// ---------------------------------------------------------------- interpret

/// Words read as terms: what the field offers below a search of the
/// words themselves ("Did you mean"), applied only when picked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reading {
    /// What it finds, in words: "PDFs modified last week".
    pub label: String,
    /// The search, in the language.
    pub expr: String,
}

/// Extensions a word may name (`pdf`, and `pdfs`).
const EXTENSIONS: [&str; 37] = [
    "pdf", "jpg", "jpeg", "png", "gif", "svg", "webp", "heic", "tiff", "mp3", "flac", "ogg", "wav",
    "mp4", "mkv", "mov", "avi", "webm", "docx", "odt", "xlsx", "ods", "pptx", "odp", "txt", "md",
    "csv", "json", "xml", "zip", "tar", "gz", "7z", "iso", "epub", "gpx", "html",
];

/// Words that say nothing once something else was read.
const FILLERS: [&str; 15] = [
    "from", "of", "the", "in", "with", "tagged", "modified", "taken", "files", "on", "my", "all",
    "and", "that", "are",
];

/// Sizes by a word, with how a reading says them.
const SIZE_WORDS: [(&str, &str, &str); 6] = [
    ("big", ">100M", "over 100 MB"),
    ("large", ">100M", "over 100 MB"),
    ("huge", ">1G", "over 1 GB"),
    ("small", "<100k", "under 100 KB"),
    ("tiny", "<100k", "under 100 KB"),
    ("empty", ":0", "that are empty"),
];

/// Plain words read as terms, by rules: periods (`last week`, `today`)
/// and years as when files were modified (taken, for pictures and
/// videos), kinds by their words (`photos`), extensions (`pdf`), sizes
/// (`big`), and the tags of `tags`; what is left searches names. `None`
/// when the text holds anything but words, or nothing was read.
pub fn interpret(text: &str, tags: &[String]) -> Option<Reading> {
    let mut words: Vec<(String, bool)> = Vec::new();
    for lexeme in lex(text) {
        match lexeme {
            Lexeme::Word(w) if w.key.is_none() && !w.unterminated => {
                words.push((text[w.range].to_owned(), w.quoted));
            }
            _ => return None,
        }
    }
    let mut exts: Vec<String> = Vec::new();
    let mut kinds: Vec<Kind> = Vec::new();
    let mut when: Option<(&str, &str)> = None;
    let mut year: Option<String> = None;
    let mut size: Option<(&str, &str)> = None;
    let mut tagged: Vec<String> = Vec::new();
    let mut names: Vec<(String, bool)> = Vec::new();
    let mut read = 0;
    let mut i = 0;
    while i < words.len() {
        let (word, quoted) = &words[i];
        i += 1;
        if *quoted {
            names.push((word.clone(), true));
            continue;
        }
        let lower = word.to_lowercase();
        let single = lower.strip_suffix('s').unwrap_or(&lower);
        if let ("last" | "this", Some((next, false))) = (lower.as_str(), words.get(i))
            && let Some(period) = PERIODS
                .iter()
                .find(|(_, v)| *v == format!("{lower}-{}", next.to_lowercase()))
        {
            when = Some(*period);
            read += 1;
            i += 1;
            continue;
        }
        if let Some(period) = PERIODS
            .iter()
            .find(|(_, v)| *v == lower && (lower == "today" || lower == "yesterday"))
        {
            when = Some(*period);
        } else if lower.len() == 4
            && lower
                .parse::<u32>()
                .is_ok_and(|y| (1970..=2099).contains(&y))
        {
            year = Some(lower.clone());
        } else if let Some(kind) =
            Kind::from_name(&lower).filter(|_| lower != "text" && lower != "code")
        {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        } else if let Some(ext) = EXTENSIONS.iter().find(|e| **e == lower || **e == single) {
            if !exts.iter().any(|e| e == ext) {
                exts.push((*ext).to_owned());
            }
        } else if let Some(&(_, form, says)) = SIZE_WORDS.iter().find(|(w, ..)| *w == lower) {
            size = Some((form, says));
        } else if let Some(tag) = tags.iter().find(|t| {
            let full = t.to_lowercase();
            let leaf = tag_leaf(t).to_lowercase();
            [full.as_str(), leaf.as_str()].contains(&lower.as_str())
                || [full.as_str(), leaf.as_str()].contains(&single)
        }) {
            if !tagged.contains(tag) {
                tagged.push(tag.clone());
            }
        } else {
            names.push((word.clone(), false));
            continue;
        }
        read += 1;
    }
    if read == 0 {
        return None;
    }
    names.retain(|(w, quoted)| *quoted || !FILLERS.contains(&w.to_lowercase().as_str()));
    let pictures =
        !kinds.is_empty() && kinds.iter().all(|k| matches!(k, Kind::Image | Kind::Video));

    let mut terms: Vec<String> = names
        .iter()
        .map(|(w, quoted)| if *quoted { w.clone() } else { lang::quote(w) })
        .collect();
    if !exts.is_empty() {
        terms.push(format!("ext:{}", exts.join(",")));
    }
    if !kinds.is_empty() {
        let words: Vec<&str> = kinds.iter().map(|k| k.name()).collect();
        terms.push(format!("kind:{}", words.join(",")));
    }
    if let Some(y) = &year {
        terms.push(format!(
            "{}:{y}",
            if pictures { "taken" } else { "modified" }
        ));
    }
    if let Some((_, period)) = when {
        terms.push(if period.starts_with(['<', '>']) {
            format!("modified{period}")
        } else {
            format!("modified:{period}")
        });
    }
    if let Some((form, _)) = size {
        terms.push(format!("size{form}"));
    }
    for tag in &tagged {
        terms.push(format!("tag:{}", lang::quote(tag)));
    }

    let subject = if !exts.is_empty() {
        joined(
            exts.iter()
                .map(|e| format!("{}s", e.to_uppercase()))
                .collect(),
        )
    } else if !kinds.is_empty() {
        let titles: Vec<String> = kinds
            .iter()
            .enumerate()
            .map(|(i, k)| {
                if i == 0 {
                    k.title().to_owned()
                } else {
                    k.title().to_lowercase()
                }
            })
            .collect();
        joined(titles)
    } else {
        "Files".to_owned()
    };
    let mut label = vec![subject];
    if !names.is_empty() {
        let said: Vec<String> = names
            .iter()
            .map(|(w, quoted)| {
                if *quoted {
                    w.trim_matches(['"', '\'']).to_owned()
                } else {
                    w.clone()
                }
            })
            .collect();
        label.push(format!("named “{}”", said.join(" ")));
    }
    if !tagged.is_empty() {
        label.push(format!("tagged {}", joined(tagged.clone())));
    }
    if let Some(y) = &year {
        label.push(format!(
            "{} in {y}",
            if pictures { "taken" } else { "modified" }
        ));
    }
    if let Some((title, _)) = when {
        label.push(format!("modified {}", title.to_lowercase()));
    }
    if let Some((_, says)) = size {
        label.push(says.to_owned());
    }
    Some(Reading {
        label: label.join(" "),
        expr: terms.join(" "),
    })
}

/// `a`, `a and b`, `a, b and c`.
fn joined(mut items: Vec<String>) -> String {
    match items.len() {
        0 => String::new(),
        1 => items.remove(0),
        _ => {
            let last = items.pop().unwrap_or_default();
            format!("{} and {last}", items.join(", "))
        }
    }
}

// ---------------------------------------------------------------- commands

/// `query.tokens`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct TokensArgs {
    /// The text as typed.
    pub text: String,
    /// Where the caret is, in bytes; the end when omitted. A term still
    /// being typed there is not an error.
    #[serde(default)]
    pub caret: Option<usize>,
}

/// `query.complete`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct CompleteArgs {
    /// The text as typed.
    pub text: String,
    /// Where the caret is, in bytes; the end when omitted.
    #[serde(default)]
    pub caret: Option<usize>,
    /// The pane whose files' tags `tag:` offers; the active one when
    /// omitted.
    #[serde(default)]
    pub pane: Option<PaneId>,
}

/// `query.interpret`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct InterpretArgs {
    /// Plain words, as typed.
    pub text: String,
    /// The pane whose files' tags words may name; the active one when
    /// omitted.
    #[serde(default)]
    pub pane: Option<PaneId>,
}

/// The tags of the pane's files the listing has read, as the tags plugin
/// counts them (`tags.list`, never reading a file); none without it.
pub(crate) fn pane_tags(app: &App, pane: Option<PaneId>) -> Vec<String> {
    if app.commands.get("tags.list").is_none() {
        return Vec::new();
    }
    let list = Invocation::with_args("tags.list", &serde_json::json!({ "pane": pane }));
    match app.dispatch(&list) {
        Ok(Outcome::Value(v)) => {
            serde_json::from_value::<std::collections::BTreeMap<String, usize>>(v)
                .map(|counts| counts.into_keys().collect())
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(spans: &[Span]) -> Vec<(Range<usize>, SpanKind)> {
        spans.iter().map(|s| (s.range.clone(), s.kind)).collect()
    }

    /// `#` is the tag key: drawn as one, offering the tags, which it
    /// writes after itself; the same words as names for the field's row.
    #[test]
    fn a_hash_is_the_tag_key() {
        assert_eq!(
            kinds(&tokens("#print dunes")),
            vec![
                (0..1, SpanKind::Key),
                (1..6, SpanKind::Value),
                (7..12, SpanKind::Word)
            ]
        );
        let tags = vec!["print".to_owned(), "Places/France".to_owned()];
        let texts = |c: Completed| c.items.into_iter().map(|i| i.text).collect::<Vec<_>>();
        assert_eq!(texts(complete("#pr", 3, &tags)), ["#print"]);
        // Still typed while it only starts a tag's name: Enter takes it.
        assert!(complete("#pr", 3, &tags).typing);
        assert!(!complete("#print", 6, &tags).typing);
        assert!(!complete("#elsewhere", 10, &tags).typing);
        // Alone, every tag, and no error while it is typed.
        assert_eq!(complete("#", 1, &tags).items.len(), 2);
        assert!(problem("#", 1).is_none());
        assert!(problem("# dunes", 7).is_some());
        assert_eq!(
            hashes_as_names("#3 dunes -#x").as_deref(),
            Some(r##""#3" dunes -"#x""##)
        );
        assert_eq!(hashes_as_names("C# tag:x"), None);
        assert!(!only_words("#print") && !only_words("size>1M"));
        assert!(only_words("water filter"));
    }

    #[test]
    fn tokens_split_as_the_parser_does() {
        let text = r#"dunes kind:image -tag:draft content:"water filter""#;
        let spans = tokens(text);
        assert_eq!(
            kinds(&spans),
            vec![
                (0..5, SpanKind::Word),
                (6..11, SpanKind::Key),
                (11..16, SpanKind::Value),
                (17..18, SpanKind::Operator),
                (18..22, SpanKind::Key),
                (22..27, SpanKind::Value),
                (28..36, SpanKind::Key),
                (36..50, SpanKind::Value),
            ]
        );
        assert_eq!(spans[1].facet.as_deref(), Some("kind"));
        assert_eq!(spans[7].facet.as_deref(), Some("content"));
        assert!(spans.iter().all(|s| s.message.is_none()));
    }

    #[test]
    fn comparisons_options_and_operators_are_told_apart() {
        let text = "(size>=10M OR rating:3) NOT in:~/x";
        let spans = tokens(text);
        assert_eq!(
            kinds(&spans),
            vec![
                (0..1, SpanKind::Operator),
                (1..7, SpanKind::Key),
                (7..10, SpanKind::Value),
                (11..13, SpanKind::Operator),
                (14..21, SpanKind::Key),
                (21..22, SpanKind::Value),
                (22..23, SpanKind::Operator),
                (24..27, SpanKind::Operator),
                (28..31, SpanKind::Option),
                (31..34, SpanKind::Value),
            ]
        );
        // A word with a colon that is no key is a word; quotes in a word's
        // middle are characters.
        assert_eq!(
            kinds(&tokens("http://x don't")),
            vec![(0..8, SpanKind::Word), (9..14, SpanKind::Word)]
        );
        // A regex runs to its closing slash, spaces and all.
        assert_eq!(
            kinds(&tokens("content:/a b/i x")),
            vec![
                (0..8, SpanKind::Key),
                (8..14, SpanKind::Value),
                (15..16, SpanKind::Word)
            ]
        );
    }

    #[test]
    fn a_term_that_does_not_parse_says_why_in_the_parsers_words() {
        let spans = tokens("kind:pix");
        assert_eq!(kinds(&spans), vec![(0..8, SpanKind::Error)]);
        let message = spans[0].message.as_deref().unwrap();
        assert!(message.starts_with("kind: expects "), "{message}");
        assert!(message.ends_with("got `pix`"), "{message}");
        assert_eq!(Some(message.to_owned()), check("kind:pix"));

        let spans = tokens("dunes size>10X");
        assert_eq!(
            kinds(&spans),
            vec![(0..5, SpanKind::Word), (6..14, SpanKind::Error)]
        );
        assert_eq!(spans[1].message.as_deref(), Some("bad size `10X`"));
        assert_eq!(spans[1].facet.as_deref(), Some("size"));
        // Away from the caret, an empty value is an error too.
        let spans = tokens_at("kind: x", 7);
        assert_eq!(spans[0].kind, SpanKind::Error);
        assert_eq!(
            spans[0].message.as_deref(),
            check("kind:").as_deref(),
            "the parser's own message"
        );
    }

    #[test]
    fn a_term_typed_at_the_caret_is_no_error_yet() {
        assert_eq!(
            kinds(&tokens("kind:im")),
            vec![(0..5, SpanKind::Key), (5..7, SpanKind::Value)]
        );
        assert_eq!(
            kinds(&tokens("modified:2026-0")),
            vec![(0..9, SpanKind::Key), (9..15, SpanKind::Value)]
        );
        assert_eq!(
            kinds(&tokens("x tag:")),
            vec![(0..1, SpanKind::Word), (2..6, SpanKind::Key)]
        );
        assert_eq!(kinds(&tokens("size>")), vec![(0..5, SpanKind::Key)]);
        assert_eq!(kinds(&tokens("\"water f")), vec![(0..8, SpanKind::Word)]);
        // The caret elsewhere: the same text is an error.
        assert_eq!(tokens_at("kind:im x", 9)[0].kind, SpanKind::Error);
        // And a value that starts nothing offered is an error at once.
        assert_eq!(tokens("kind:pix")[0].kind, SpanKind::Error);
    }

    #[test]
    fn a_problem_waits_for_what_is_still_typed() {
        assert_eq!(problem("kind:im", 7), None);
        assert_eq!(problem("a OR", 4), None);
        assert_eq!(problem("(a", 2), None);
        assert_eq!(problem("(a b", 2), Some("missing `)`".to_owned()));
        assert_eq!(problem("a b)", 4), Some("unexpected `)`".to_owned()));
        let message = problem("kind:pix dunes", 14).unwrap();
        assert!(message.ends_with("got `pix`"), "{message}");
        assert_eq!(check("report kind:image"), None);
        assert_eq!(check("(a"), Some("missing `)`".to_owned()));
    }

    fn texts(c: &Completed) -> Vec<&str> {
        c.items.iter().map(|i| i.text.as_str()).collect()
    }

    #[test]
    fn keys_complete_from_a_word() {
        let c = complete("ki", 2, &[]);
        assert_eq!(c.range, 0..2);
        assert_eq!(texts(&c), ["kind:"]);
        assert!(!c.typing);
        assert_eq!(texts(&complete("dunes ta", 8, &[])), ["tag:", "taken:"]);
        assert!(complete("k", 1, &[]).items.is_empty());
        // The caret inside a word completes nothing.
        assert!(complete("kind", 2, &[]).items.is_empty());
    }

    #[test]
    fn values_complete_after_their_key() {
        let c = complete("dunes kind:im", 13, &[]);
        assert_eq!(c.range, 6..13);
        assert_eq!(texts(&c), ["kind:image"]);
        assert_eq!(c.items[0].label, "Images");
        assert!(c.typing);
        let c = complete("kind:image,vi", 13, &[]);
        assert_eq!(texts(&c), ["kind:image,video"]);
        assert!(complete("kind:image", 10, &[]).items.is_empty());
        assert_eq!(texts(&complete("kind:pic", 8, &[])), ["kind:image"]);

        let tags = [
            "draft".to_owned(),
            "favorite".to_owned(),
            "Places/France".to_owned(),
        ];
        let c = complete("tag:", 4, &tags);
        assert_eq!(
            texts(&c),
            ["tag:draft", "tag:favorite", "tag:Places/France"]
        );
        assert_eq!(c.items[0].tag.as_deref(), Some("draft"));
        assert!(c.typing);
        assert_eq!(texts(&complete("tag:fr", 6, &tags)), ["tag:Places/France"]);

        let c = complete("modified:l", 10, &[]);
        assert_eq!(
            texts(&c),
            [
                "modified:last-week",
                "modified:last-month",
                "modified:last-year"
            ]
        );
        assert_eq!(c.items[0].label, "Last week");
        assert_eq!(texts(&complete("size>1", 6, &[])), ["size>100M", "size>1G"]);
        // An operator alone offers every preset, each with its own.
        let c = complete("taken>", 6, &[]);
        assert_eq!(c.items.len(), PERIODS.len());
        assert!(texts(&c).contains(&"taken>1y"));
        assert!(texts(&c).contains(&"taken:today"));
        assert_eq!(texts(&complete("type:d", 6, &[])), ["type:dir"]);
        for item in complete("size:", 5, &[]).items {
            assert!(lang::parse(&item.text).is_ok(), "{}", item.text);
        }
    }

    #[test]
    fn plain_words_are_read_as_terms() {
        let tags = ["print".to_owned(), "Places/France".to_owned()];
        let cases = [
            (
                "pdf last week",
                "ext:pdf modified:last-week",
                "PDFs modified last week",
            ),
            (
                "photos 2023",
                "kind:image taken:2023",
                "Images taken in 2023",
            ),
            (
                "report print",
                "report tag:print",
                "Files named “report” tagged print",
            ),
            (
                "big downloads",
                "downloads size>100M",
                "Files named “downloads” over 100 MB",
            ),
            (
                "videos from france",
                "kind:video tag:Places/France",
                "Videos tagged Places/France",
            ),
            (
                "invoices 2025 pdfs",
                "invoices ext:pdf modified:2025",
                "PDFs named “invoices” modified in 2025",
            ),
            (
                "photos videos today",
                "kind:image,video modified:today",
                "Images and videos modified today",
            ),
        ];
        for (text, expr, label) in cases {
            let reading = interpret(text, &tags).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(reading.expr, expr, "{text}");
            assert_eq!(reading.label, label, "{text}");
            assert!(lang::parse(&reading.expr).is_ok(), "{}", reading.expr);
        }
        assert_eq!(interpret("dunes", &tags), None);
        assert_eq!(interpret("dunes kind:image", &tags), None);
        assert_eq!(interpret("a OR b", &tags), None);
        // A year the parser takes no date of is a name.
        assert_eq!(interpret("1950", &tags), None);
        let quoted = interpret("\"last week\" pdf", &tags).unwrap();
        assert_eq!(quoted.expr, "\"last week\" ext:pdf");
        assert!(lang::parse(&quoted.expr).is_ok());
    }
}
