//! What Skwir says, as data (ADR-139, `docs/invariants/words.md`): a
//! crate's message and its typed arguments, said where it is drawn or sent,
//! in the language of who reads it. Written as English beside an id
//! (`words!`), which the macro makes a static [`Message`] of; translated in
//! each crate's Fluent catalogs ([`Catalog`]), said by Skwir's own
//! resolver; English last, always whole.
//!
//! As data that leaves the process (JSON for agents, scripts and the CLI),
//! `Words` is its English text: agents read English (ADR-139, 7).

mod catalog;
mod pattern;

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

pub use catalog::{Catalog, find, languages_of, register, withdraw};
pub use pattern::{Element, Key, Variant, from_fluent};

/// English, the language the code writes its words in.
pub const ENGLISH: &str = crate::locale::ENGLISH;

/// The pseudo-language (CLDR's `en-XA`): English accented, longer, each
/// argument marked, to find in a window what is not a message, what is
/// cut and what is placed wrong (`docs/invariants/words.md`, "Tests").
pub const PSEUDO: &str = "en-XA";

/// A crate's message: who says it, which, its English as the code writes
/// it, and what a translator needs to know of it.
#[derive(Debug, PartialEq, Eq)]
pub struct Message {
    /// The crate's package name (`skwir-plugin-trash`).
    pub domain: &'static str,
    pub id: &'static str,
    pub english: &'static [Element],
    pub note: &'static str,
}

/// What Skwir says: a message with its arguments, or text as written (an
/// agent's, the system's), shown as it is in every language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Words(Inner);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Inner {
    Message(&'static Message, Arc<[(&'static str, Arg)]>),
    Written(Cow<'static, str>),
    /// An error, each link of its chain said in the reader's language
    /// where it is words (`Words::of_error`).
    Error(Failure),
    /// Words that begin what is shown (a toast, a row), their first letter
    /// cased as the reader's language begins a sentence (`Words::heading`).
    Heading(Box<Words>),
}

/// An error held as words: its chain, said link by link where it is said
/// (`Words::of_error`, `Arg::Error`), from the link it begins at
/// (`Words::of_error_from`).
#[derive(Clone, Debug)]
pub struct Failure(Arc<anyhow::Error>, usize);

impl PartialEq for Failure {
    /// The same error from the same link, not two alike.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) && self.1 == other.1
    }
}

impl Eq for Failure {}

/// An argument of a message, typed: each said as its type says it in the
/// reader's language (ADR-139, 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    /// Text that is not Skwir's to translate (a path, a command, what the
    /// system said), as it is.
    Text(Cow<'static, str>),
    /// A file's, a place's or a tag's name: quoted as the language quotes,
    /// its bidirectional controls shown, never obeyed.
    Name(String),
    /// A folder's name where it is a destination ("to Docs"): unquoted,
    /// its bidirectional controls shown, never obeyed.
    Folder(String),
    /// A count: chooses the plural, written with its groups.
    Count(i64),
    /// A size, as the size setting counts it (`fs::size_said`).
    Size(u64),
    /// What something is (`Kind`): a selector chooses by it, so a
    /// language makes its words agree.
    Kind(&'static str),
    /// Words in words: a plan's title in a toast.
    Words(Box<Words>),
    /// An error, said link by link in the reader's language (what a step
    /// could not do, then why).
    Error(Failure),
    /// Items joined as the reader's language joins a list ("a, b and c",
    /// "a, b et c"): `List::and`, `List::or`.
    List(Join, Vec<Arg>),
}

/// How a list's items are joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// All of them: "a, b and c".
    And,
    /// One of them: "a, b or c".
    Or,
}

/// A list of items, for `Arg::List`: each item an argument of its own (a
/// `Name`, a count, words), joined in the reader's language.
pub struct List(Join, Vec<Arg>);

impl List {
    /// "a, b and c".
    pub fn and<A: Into<Arg>>(items: impl IntoIterator<Item = A>) -> Self {
        List(Join::And, items.into_iter().map(Into::into).collect())
    }

    /// "a, b or c".
    pub fn or<A: Into<Arg>>(items: impl IntoIterator<Item = A>) -> Self {
        List(Join::Or, items.into_iter().map(Into::into).collect())
    }
}

impl From<List> for Arg {
    fn from(list: List) -> Self {
        Arg::List(list.0, list.1)
    }
}

/// A name, for `Arg::Name`.
pub struct Name(pub String);

impl Name {
    /// The name `path` goes by: its last part, else the whole path.
    pub fn of(path: &std::path::Path) -> Self {
        Name(path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ))
    }
}

/// A folder named as a destination, for `Arg::Folder`.
pub struct Folder(pub String);

impl Folder {
    /// The folder `path` is in, by its name: `Docs` for `/a/Docs/b.txt`;
    /// nothing for a path with no folder.
    pub fn of(path: &std::path::Path) -> Self {
        Folder(
            path.parent()
                .map_or_else(String::new, |folder| Name::of(folder).0),
        )
    }
}

/// A size, for `Arg::Size`.
pub struct Size(pub u64);

/// What a message's subject is, for a language to make its words agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Folder,
    Link,
    Item,
}

impl Kind {
    /// Its selector's word.
    pub fn word(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Folder => "folder",
            Kind::Link => "link",
            Kind::Item => "item",
        }
    }
}

impl From<&str> for Arg {
    fn from(text: &str) -> Self {
        Arg::Text(Cow::Owned(text.to_owned()))
    }
}

impl From<String> for Arg {
    fn from(text: String) -> Self {
        Arg::Text(Cow::Owned(text))
    }
}

impl From<Name> for Arg {
    fn from(name: Name) -> Self {
        Arg::Name(name.0)
    }
}

impl From<Folder> for Arg {
    fn from(folder: Folder) -> Self {
        Arg::Folder(folder.0)
    }
}

impl From<Size> for Arg {
    fn from(size: Size) -> Self {
        Arg::Size(size.0)
    }
}

impl From<Kind> for Arg {
    fn from(kind: Kind) -> Self {
        Arg::Kind(kind.word())
    }
}

impl From<Words> for Arg {
    fn from(words: Words) -> Self {
        Arg::Words(Box::new(words))
    }
}

impl From<anyhow::Error> for Arg {
    fn from(error: anyhow::Error) -> Self {
        Arg::Error(Failure(Arc::new(error), 0))
    }
}

impl From<&Arc<anyhow::Error>> for Arg {
    fn from(error: &Arc<anyhow::Error>) -> Self {
        Arg::Error(Failure(Arc::clone(error), 0))
    }
}

macro_rules! count_from {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            fn from(n: $t) -> Self {
                Arg::Count(i64::try_from(n).unwrap_or(i64::MAX))
            }
        }
    )*};
}
count_from!(u8, u16, u32, u64, usize, i8, i16, i32, i64);

impl Words {
    /// `message`, said with `args` (what `words!` makes).
    pub fn message(message: &'static Message, args: Vec<(&'static str, Arg)>) -> Self {
        Words(Inner::Message(message, Arc::from(args)))
    }

    /// Text Skwir did not write, shown as it is in every language.
    pub fn as_written(text: impl Into<Cow<'static, str>>) -> Self {
        Words(Inner::Written(text.into()))
    }

    /// An error as words: its chain, outermost first, joined by `: `, each
    /// link in the reader's language where it is words (a context written
    /// with `words!`, a system's error by its kind, `op::io_words`), else
    /// as its own text, the error's number left out.
    pub fn of_error(error: anyhow::Error) -> Self {
        Words(Inner::Error(Failure(Arc::new(error), 0)))
    }

    /// An error kept elsewhere too, as words from the link `from` of its
    /// chain on (0 for the whole), said as [`Words::of_error`] says it: what
    /// stopped a step said alone, after the step's own words naming its
    /// file (`op::reason_after`).
    pub fn of_error_from(error: &Arc<anyhow::Error>, from: usize) -> Self {
        Words(Inner::Error(Failure(Arc::clone(error), from)))
    }

    /// Whether it says nothing: no words were given (text as written, and
    /// empty), as a field that may say nothing holds then (a form's help,
    /// a choice's detail). A message or an error always says something.
    pub fn is_empty(&self) -> bool {
        matches!(&self.0, Inner::Written(text) if text.is_empty())
    }

    /// Its message, `None` for text as written or an error.
    pub fn message_of(&self) -> Option<&'static Message> {
        match &self.0 {
            Inner::Message(message, _) => Some(message),
            Inner::Heading(words) => words.message_of(),
            Inner::Written(_) | Inner::Error(_) => None,
        }
    }

    /// These words where they begin what is shown (a toast's text, a row's
    /// first words): their first letter cased as the reader's language
    /// begins a sentence. Words written to be said inside sentences (a
    /// plan's title, "copy 3 items to Docs") begin in lower case; this is
    /// the one casing done in code (ADR-139, the user's choice of
    /// 2026-11-17), by the language's own rules (ICU4X: a Turkish i takes
    /// its dot, a script without case is left as it is).
    #[must_use]
    pub fn heading(&self) -> Words {
        Words(Inner::Heading(Box::new(self.clone())))
    }

    /// Said in the reader's languages, cut around its argument `name`: what
    /// comes before it and what comes after, the language choosing where it
    /// goes (a link inside a sentence: the caller draws the argument
    /// itself). `None` where these words are no message placing `name`
    /// once.
    pub fn say_around(&self, name: &str) -> Option<(String, String)> {
        const MARK: &str = "\u{F8FF}";
        let Inner::Message(message, args) = &self.0 else {
            return None;
        };
        let marked: Arc<[(&'static str, Arg)]> = args
            .iter()
            .map(|(n, a)| {
                if *n == name {
                    (*n, Arg::Text(Cow::Borrowed(MARK)))
                } else {
                    (*n, a.clone())
                }
            })
            .collect();
        let said = Words(Inner::Message(message, marked)).say();
        let mut parts = said.split(MARK);
        let (before, after) = (parts.next()?, parts.next()?);
        parts
            .next()
            .is_none()
            .then(|| (before.to_owned(), after.to_owned()))
    }

    /// Said in the languages Skwir speaks to the user (`locale::languages`):
    /// what the window draws. The languages are kept on each thread, read
    /// again only when they move (`revision`): a frame says many words.
    pub fn say(&self) -> String {
        thread_local! {
            static SAID_IN: std::cell::RefCell<(u64, Vec<String>)> =
                const { std::cell::RefCell::new((u64::MAX, Vec::new())) };
        }
        let now = revision();
        SAID_IN.with_borrow_mut(|(at, languages)| {
            if *at != now {
                *languages = crate::locale::languages();
                *at = now;
            }
            self.say_in(languages)
        })
    }

    /// Said in English: what agents, scripts and logs read.
    pub fn english(&self) -> String {
        self.say_in(&[])
    }

    /// Said in the first of `languages` that has its message, English
    /// last.
    pub fn say_in(&self, languages: &[String]) -> String {
        let mut out = String::new();
        self.say_into(&mut out, languages);
        out
    }

    fn say_into(&self, out: &mut String, languages: &[String]) {
        let (message, args) = match &self.0 {
            Inner::Written(text) => return out.push_str(text),
            Inner::Error(failure) => return say_error(failure, languages, out),
            Inner::Heading(words) => {
                let mut said = String::new();
                words.say_into(&mut said, languages);
                let language = languages.first().map_or(ENGLISH, String::as_str);
                return out.push_str(&casing::heading(language, &said));
            }
            Inner::Message(message, args) => (*message, args),
        };
        if languages.first().is_some_and(|first| first == PSEUDO) {
            // Its English accented, nested words said in the
            // pseudo-language too (bracketed apart from what is as written).
            let mut english = String::new();
            say(
                message.english,
                args,
                ENGLISH,
                languages,
                &mut english,
                true,
            );
            return out.push_str(&pseudo(&english));
        }
        match find(message.domain, message.id, languages) {
            Some((said, language)) => say(&said, args, &language, languages, out, false),
            None => say(message.english, args, ENGLISH, languages, out, false),
        }
    }
}

/// Say `elements` with `args` in `language` (its plurals, its quotes);
/// nested words in `languages`. In the pseudo-language each argument is
/// marked, so what is placed wrong shows.
fn say(
    elements: &[Element],
    args: &[(&'static str, Arg)],
    language: &str,
    languages: &[String],
    out: &mut String,
    marked: bool,
) {
    let arg = |name: &str| args.iter().find(|(n, _)| *n == name).map(|(_, a)| a);
    for element in elements {
        match element {
            Element::Text(text) => out.push_str(text),
            Element::Arg(name) => {
                if let Some(value) = arg(name) {
                    if marked {
                        out.push('‹');
                    }
                    said_arg(value, language, languages, out);
                    if marked {
                        out.push('›');
                    }
                } else {
                    // A translation naming an argument the code gives not:
                    // the checks refuse it; said so rather than dropped.
                    out.push_str("{$");
                    out.push_str(name);
                    out.push('}');
                }
            }
            Element::Select {
                on,
                variants,
                default,
            } => {
                let chosen = arg(on);
                let count = match chosen {
                    Some(Arg::Count(n)) => Some(*n),
                    _ => None,
                };
                let word = match chosen {
                    Some(Arg::Kind(word)) => Some(*word),
                    Some(Arg::Text(text)) => Some(text.as_ref()),
                    _ => None,
                };
                let value = pattern::choose(variants, *default, count, word, |n| {
                    plurals::category(language, n)
                });
                say(value, args, language, languages, out, marked);
            }
        }
    }
}

/// An argument said in `language`.
fn said_arg(arg: &Arg, language: &str, languages: &[String], out: &mut String) {
    match arg {
        Arg::Text(text) => out.push_str(text),
        Arg::Name(name) => quoted(language, &shown_controls(name), out),
        Arg::Folder(name) => out.push_str(&shown_controls(name)),
        Arg::Count(n) => out.push_str(&grouped(*n)),
        Arg::Size(bytes) => out.push_str(&crate::fs::size_said(*bytes)),
        Arg::Kind(word) => out.push_str(word),
        Arg::Words(words) => words.say_into(out, languages),
        Arg::Error(failure) => say_error(failure, languages, out),
        Arg::List(join, items) => {
            let said: Vec<String> = items
                .iter()
                .map(|item| {
                    let mut one = String::new();
                    said_arg(item, language, languages, &mut one);
                    one
                })
                .collect();
            out.push_str(&lists::joined(language, *join, &said));
        }
    }
}

thread_local! {
    /// The languages words are said in while an error's chain is written
    /// out (`say_error`): a link that is words says itself in them through
    /// `Display`. Outside, `Display` is English.
    static SAYING_IN: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// `failure`'s chain, outermost first from the link it begins at, joined
/// by `: `, in `languages`.
fn say_error(failure: &Failure, languages: &[String], out: &mut String) {
    let before = SAYING_IN.with_borrow_mut(|saying| saying.replace(languages.to_vec()));
    let mut first = true;
    for link in failure.0.chain().skip(failure.1) {
        if !first {
            out.push_str(": ");
        }
        first = false;
        match link.downcast_ref::<std::io::Error>() {
            Some(io) => crate::op::io_words(io).say_into(out, languages),
            None => out.push_str(&link.to_string()),
        }
    }
    SAYING_IN.with_borrow_mut(|saying| *saying = before);
}

/// `name` quoted as `language` quotes: “…” in English, « … » in French
/// (narrow spaces inside), „…“ in German, «…» in Spanish, Italian and
/// Russian, 「…」 in Japanese and Chinese.
fn quoted(language: &str, name: &str, out: &mut String) {
    let base = language.split('-').next().unwrap_or(language);
    let (open, close) = match base {
        "fr" => ("«\u{202f}", "\u{202f}»"),
        "de" | "cs" | "sk" | "pl" | "ro" | "hu" | "bg" | "lt" => ("„", "“"),
        "es" | "it" | "pt" | "ru" | "uk" | "ca" | "el" | "no" | "nb" | "nn" => ("«", "»"),
        "ja" | "zh" => ("「", "」"),
        _ => ("“", "”"),
    };
    out.push_str(open);
    out.push_str(name);
    out.push_str(close);
}

/// A name's bidirectional controls shown as tags, never obeyed (ADR-139,
/// 5): `Hello_` + U+202E + `gnp.js` reads `Hello_⟨RLO⟩gnp.js`, not as
/// `Hello_sj.png`. The marks (U+200E, U+200F, U+061C) are left as they are.
pub fn shown_controls(name: &str) -> Cow<'_, str> {
    let tag = |c: char| match c {
        '\u{202A}' => Some("LRE"),
        '\u{202B}' => Some("RLE"),
        '\u{202C}' => Some("PDF"),
        '\u{202D}' => Some("LRO"),
        '\u{202E}' => Some("RLO"),
        '\u{2066}' => Some("LRI"),
        '\u{2067}' => Some("RLI"),
        '\u{2068}' => Some("FSI"),
        '\u{2069}' => Some("PDI"),
        _ => None,
    };
    if !name.chars().any(|c| tag(c).is_some()) {
        return Cow::Borrowed(name);
    }
    let mut out = String::with_capacity(name.len() + 8);
    for c in name.chars() {
        match tag(c) {
            Some(tag) => {
                out.push('⟨');
                out.push_str(tag);
                out.push('⟩');
            }
            None => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// A count with its thousands grouped, as English groups them (`1,234`):
/// each language's groups arrive with its numbers (PLAN.md, step F).
fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// English made strange but readable: each letter accented, its vowels
/// doubled (a third longer, as other languages often are), in brackets.
fn pseudo(english: &str) -> String {
    let mut out = String::with_capacity(english.len() * 2 + 2);
    out.push('[');
    let mut marked = false;
    for c in english.chars() {
        // An argument, between its marks, as it is.
        if c == '‹' || c == '›' {
            marked = c == '‹';
            out.push(c);
            continue;
        }
        if marked {
            out.push(c);
            continue;
        }
        let accented = match c {
            'a' => 'á',
            'e' => 'é',
            'i' => 'í',
            'o' => 'ö',
            'u' => 'ü',
            'y' => 'ý',
            'A' => 'Å',
            'E' => 'É',
            'I' => 'Î',
            'O' => 'Ø',
            'U' => 'Û',
            'Y' => 'Ý',
            'c' => 'ç',
            'n' => 'ñ',
            's' => 'š',
            'z' => 'ž',
            'C' => 'Ç',
            'N' => 'Ñ',
            'S' => 'Š',
            'Z' => 'Ž',
            'g' => 'ĝ',
            'h' => 'ĥ',
            'k' => 'ķ',
            'l' => 'ļ',
            other => other,
        };
        out.push(accented);
        if "aeiouyAEIOUY".contains(c) {
            out.push(accented);
        }
    }
    out.push(']');
    out
}

impl fmt::Display for Words {
    /// In English: what a log and `{}` read. While an error's chain is said
    /// (`Words::of_error`, `Arg::Error`), in the languages it is said in:
    /// words that are a link of it say themselves there.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The languages taken out before saying: words holding an error
        // say its chain, which sets them again.
        let languages = SAYING_IN.with_borrow(Clone::clone);
        let said = languages.map_or_else(|| self.english(), |languages| self.say_in(&languages));
        f.write_str(&said)
    }
}

/// Words are an error: written with `words!`, a step's failure is said in
/// the reader's language (`Err(words!(…).into())`, `.context(words!(…))`).
impl std::error::Error for Words {}

impl Default for Words {
    /// No words: what a field that may say nothing holds until it is
    /// given some (`Words::is_empty`).
    fn default() -> Self {
        Words::as_written("")
    }
}

impl From<&str> for Words {
    /// Transitional (PLAN.md, step E): text not yet a message, shown as it
    /// is. Goes once every crate is converted.
    fn from(text: &str) -> Self {
        Words::as_written(text.to_owned())
    }
}

impl From<String> for Words {
    /// Transitional, as `From<&str>`.
    fn from(text: String) -> Self {
        Words::as_written(text)
    }
}

impl serde::Serialize for Words {
    /// Its English text: what leaves the process is read by agents and
    /// scripts, in English (ADR-139, 7).
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.english())
    }
}

impl<'de> serde::Deserialize<'de> for Words {
    /// Text that comes in (an agent's plan, a file kept) is as written.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Words::as_written)
    }
}

impl schemars::JsonSchema for Words {
    fn schema_name() -> Cow<'static, str> {
        "Words".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string" })
    }
}

/// What moves whenever what is said would change: the languages, or the
/// catalogs registered. What keeps what it said keys on it.
pub fn revision() -> u64 {
    crate::locale::revision().wrapping_add(catalog::revision())
}

/// The plural categories of each language, from ICU4X (CLDR's current
/// rules), made once a language on each thread that asks: ICU4X's rules
/// are not to be sent between threads, and made from the program's own
/// data they cost little.
mod plurals {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use icu_plurals::{PluralCategory, PluralRules};

    thread_local! {
        static RULES: RefCell<HashMap<String, Option<PluralRules>>> = RefCell::new(HashMap::new());
    }

    /// The category `n` is in, in `language`: `other` where its rules are
    /// unknown.
    pub fn category(language: &str, n: i64) -> &'static str {
        let category = RULES.with_borrow_mut(|rules| {
            rules
                .entry(language.to_owned())
                .or_insert_with(|| {
                    let locale: icu_locale_core::Locale = language.parse().ok()?;
                    PluralRules::try_new_cardinal(locale.into()).ok()
                })
                .as_ref()
                .map_or(PluralCategory::Other, |rules| rules.category_for(n))
        });
        match category {
            PluralCategory::Zero => "zero",
            PluralCategory::One => "one",
            PluralCategory::Two => "two",
            PluralCategory::Few => "few",
            PluralCategory::Many => "many",
            PluralCategory::Other => "other",
        }
    }
}

/// A sentence's first letter, cased by its language's rules (ICU4X).
mod casing {
    use icu_casemap::TitlecaseMapper;
    use icu_casemap::options::{TitlecaseOptions, TrailingCase};
    use icu_locale_core::LanguageIdentifier;

    /// `said` with its first letter (past what is not one: a quote, a
    /// bracket) in its title case in `language`, the rest as it is.
    pub fn heading(language: &str, said: &str) -> String {
        let language: LanguageIdentifier = language.parse().unwrap_or(LanguageIdentifier::UNKNOWN);
        let mut options = TitlecaseOptions::default();
        options.trailing_case = Some(TrailingCase::Unchanged);
        TitlecaseMapper::new()
            .titlecase_segment_to_string(said, &language, options)
            .into_owned()
    }
}

/// Lists joined by each language's patterns (ICU4X, CLDR's data), made
/// once a language on each thread that asks, as the plurals are.
mod lists {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use icu_list::ListFormatter;
    use icu_list::options::{ListFormatterOptions, ListLength};

    use super::Join;

    thread_local! {
        static FORMATTERS: RefCell<HashMap<(String, bool), Option<ListFormatter>>> =
            RefCell::new(HashMap::new());
    }

    /// `items` joined as `language` joins them; with ", " where its
    /// patterns are unknown.
    pub fn joined(language: &str, join: Join, items: &[String]) -> String {
        FORMATTERS.with_borrow_mut(|formatters| {
            let formatter = formatters
                .entry((language.to_owned(), join == Join::And))
                .or_insert_with(|| {
                    let locale: icu_locale_core::Locale = language.parse().ok()?;
                    let options = ListFormatterOptions::default().with_length(ListLength::Wide);
                    match join {
                        Join::And => ListFormatter::try_new_and(locale.into(), options).ok(),
                        Join::Or => ListFormatter::try_new_or(locale.into(), options).ok(),
                    }
                });
            match formatter {
                Some(formatter) => formatter.format_to_string(items.iter().map(String::as_str)),
                None => items.join(", "),
            }
        })
    }
}

#[cfg(test)]
mod tests;
