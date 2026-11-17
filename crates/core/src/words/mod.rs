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

pub use catalog::{
    Catalog, Shipped, find, languages_of, register, register_shipped, register_shipped_of_program,
    ship, shipped_languages, withdraw,
};
pub use pattern::{Element, Key, Variant, from_fluent};

/// English, the language the code writes its words in.
pub const ENGLISH: &str = crate::locale::ENGLISH;

/// The pseudo-language (CLDR's `en-XA`): English accented, longer, each
/// argument marked, to find in a window what is not a message, what is
/// cut and what is placed wrong (`docs/invariants/words.md`, "Tests").
pub const PSEUDO: &str = "en-XA";

/// A crate's message: who says it, which, its English as the code writes
/// it, and what a translator needs to know of it.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Message {
    /// The crate's package name (`skwir-plugin-trash`).
    pub domain: &'static str,
    pub id: &'static str,
    pub english: &'static [Element],
    pub note: &'static str,
}

/// What Skwir says: a message with its arguments, or text as written (an
/// agent's, the system's), shown as it is in every language.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Words(Inner);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Inner {
    Message(&'static Message, Arc<[(&'static str, Arg)]>),
    Written(Cow<'static, str>),
    /// An error, each link of its chain said in the reader's language
    /// where it is words (`Words::of_error`).
    Error(Failure),
    /// Words that begin what is shown (a toast, a row), their first letter
    /// cased as the reader's language begins a sentence (`Words::heading`).
    Heading(Box<Words>),
    /// A message come back from its data (`Words::to_data`): said from the
    /// catalogs by its domain and id, else as its English was carried.
    Carried(Arc<Carried>),
}

/// A message known by its domain and id, as its data carries it.
#[derive(Debug, PartialEq, Eq, Hash)]
struct Carried {
    domain: String,
    id: String,
    english: String,
    args: Vec<(String, Arg)>,
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

impl std::hash::Hash for Failure {
    /// As it equals: the same error from the same link.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
        self.1.hash(state);
    }
}

/// An argument of a message, typed: each said as its type says it in the
/// reader's language (ADR-139, 5).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
    /// A count: chooses the plural, written with its groups as the
    /// reader's language writes them ("1,234", « 1 234 »).
    Count(i64),
    /// A number with digits after the point (`Decimal`): the reader's
    /// decimal sign and groups ("1.5", « 1,5 »). It chooses no plural: a
    /// selector on it says its default.
    Decimal(i64, u8),
    /// A moment (`Moment`): written as the reader's dates and times are
    /// ("14:02", "Tuesday 18:45", "12 Mar 2024", « 12 mars 2024 »), in the
    /// system's time zone.
    Moment(Moment),
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// A number with `digits` after the point, for `Arg::Decimal`: `value` is
/// the number times ten to the `digits` (`Decimal::of(1.5, 1)`).
pub struct Decimal {
    pub value: i64,
    pub digits: u8,
}

impl Decimal {
    /// `number` rounded to `digits` after the point.
    pub fn of(number: f64, digits: u8) -> Self {
        let scaled = (number * 10f64.powi(i32::from(digits))).round();
        // Rounded first: a size or a ratio, far inside i64.
        #[allow(clippy::cast_possible_truncation)]
        let value = scaled as i64;
        Decimal { value, digits }
    }
}

impl From<Decimal> for Arg {
    fn from(decimal: Decimal) -> Self {
        Arg::Decimal(decimal.value, decimal.digits)
    }
}

/// A moment, for `Arg::Moment`: when (seconds since 1970, UTC), and what of
/// it is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Moment {
    pub at: i64,
    pub shape: Shape,
}

/// What of a moment is written, each as the reader's locale writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    /// The time of day: "14:02" (or "2:02 PM" where the locale says so).
    Time,
    /// The weekday: "Tuesday" (a message places it beside a time: CLDR
    /// writes no weekday and time together well).
    Weekday,
    /// The day and the month: "12 Mar", "Mar 12".
    DayMonth,
    /// The date: "12 Mar 2024".
    Date,
    /// The date and the time: "12 Mar 2024, 14:02".
    DateTime,
    /// The date and the time to the second.
    DateTimeSeconds,
    /// The month and the year: "March 2024".
    MonthYear,
    /// The month: "March".
    Month,
}

impl Shape {
    fn word(self) -> &'static str {
        match self {
            Shape::Time => "time",
            Shape::Weekday => "weekday",
            Shape::DayMonth => "day_month",
            Shape::Date => "date",
            Shape::DateTime => "date_time",
            Shape::DateTimeSeconds => "date_time_seconds",
            Shape::MonthYear => "month_year",
            Shape::Month => "month",
        }
    }

    const ALL: [Shape; 8] = [
        Shape::Time,
        Shape::Weekday,
        Shape::DayMonth,
        Shape::Date,
        Shape::DateTime,
        Shape::DateTimeSeconds,
        Shape::MonthYear,
        Shape::Month,
    ];
}

impl Moment {
    /// `t`, written as `shape` says; `None` before 1970 or past what a
    /// date can say.
    pub fn of(t: std::time::SystemTime, shape: Shape) -> Option<Self> {
        let at = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Some(Moment {
            at: i64::try_from(at).ok()?,
            shape,
        })
    }
}

impl From<Moment> for Arg {
    fn from(moment: Moment) -> Self {
        Arg::Moment(moment)
    }
}

impl Moment {
    /// Written for the window, as the system's locale writes dates.
    pub fn said(self) -> String {
        dates::written(&crate::locale::dates(), self)
    }
}

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
            Inner::Written(_) | Inner::Error(_) | Inner::Carried(_) => None,
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

    /// Said in the reader's languages, cut around its arguments `names`:
    /// the text before the first one placed, then each placed, in the
    /// order the language places them, by its index in `names`, with the
    /// text that follows it up to the next (a line of facts drawn apart,
    /// wrapping only between them: the caller draws each argument). `None`
    /// where these words are no message placing each of `names` once.
    pub fn say_apart(&self, names: &[&str]) -> Option<(String, Vec<(usize, String)>)> {
        let Inner::Message(message, args) = &self.0 else {
            return None;
        };
        // A private-use character a name for each argument cut out.
        let mark = |at: usize| char::from_u32(0xF8E0 + u32::try_from(at).ok()?);
        let marked: Arc<[(&'static str, Arg)]> = args
            .iter()
            .map(|(n, a)| match names.iter().position(|name| name == n) {
                Some(at) => (
                    *n,
                    Arg::Text(Cow::Owned(mark(at).unwrap_or('\u{F8FF}').to_string())),
                ),
                None => (*n, a.clone()),
            })
            .collect();
        let said = Words(Inner::Message(message, marked)).say();
        let mut before = String::new();
        let mut placed: Vec<(usize, String)> = Vec::with_capacity(names.len());
        for c in said.chars() {
            match (0..names.len()).find(|&at| mark(at) == Some(c)) {
                Some(at) if placed.iter().any(|(seen, _)| *seen == at) => return None,
                Some(at) => placed.push((at, String::new())),
                None => match placed.last_mut() {
                    Some((_, after)) => after.push(c),
                    None => before.push(c),
                },
            }
        }
        (placed.len() == names.len()).then_some((before, placed))
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
        let (languages, formats) = SAID_IN.with_borrow_mut(|(at, languages)| {
            if *at != now {
                *languages = crate::locale::languages();
                *at = now;
            }
            (languages.clone(), formats::Formats::window())
        });
        formats::within(formats, || self.say_in(&languages))
    }

    /// Said in English: what agents, scripts and logs read, its numbers
    /// and dates as English writes them.
    pub fn english(&self) -> String {
        formats::within(formats::Formats::english(), || self.say_in(&[]))
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
            Inner::Carried(carried) => {
                if languages.first().is_some_and(|first| first == PSEUDO) {
                    return out.push_str(&pseudo(&carried.english));
                }
                return match find(&carried.domain, &carried.id, languages) {
                    Some((said, language)) => {
                        say(&said, &carried.args, &language, languages, out, false);
                    }
                    None => out.push_str(&carried.english),
                };
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
fn say<N: AsRef<str>>(
    elements: &[Element],
    args: &[(N, Arg)],
    language: &str,
    languages: &[String],
    out: &mut String,
    marked: bool,
) {
    let arg = |name: &str| {
        args.iter()
            .find(|(n, _)| n.as_ref() == name)
            .map(|(_, a)| a)
    };
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
        Arg::Count(n) => out.push_str(&numbers::grouped(&formats::numbers(), *n)),
        Arg::Decimal(value, digits) => {
            out.push_str(&numbers::decimal(&formats::numbers(), *value, *digits));
        }
        Arg::Moment(moment) => out.push_str(&dates::written(&formats::dates(), *moment)),
        Arg::Size(bytes) => crate::fs::size_words(*bytes).say_into(out, languages),
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
/// The locales numbers and dates are written in while words are said: the
/// system's for the window (`say`), English's for agents (`english`), and
/// English's for words said in given languages (`say_in`) outside both.
mod formats {
    use std::cell::RefCell;

    #[derive(Clone)]
    pub struct Formats {
        numbers: String,
        dates: String,
    }

    impl Formats {
        pub fn window() -> Self {
            Formats {
                numbers: crate::locale::numbers(),
                dates: crate::locale::dates(),
            }
        }

        pub fn english() -> Self {
            Formats {
                numbers: super::ENGLISH.to_owned(),
                dates: super::ENGLISH.to_owned(),
            }
        }
    }

    thread_local! {
        static NOW: RefCell<Option<Formats>> = const { RefCell::new(None) };
    }

    /// `f`'s answer, words said within it writing numbers and dates as
    /// `formats` says.
    pub fn within<T>(formats: Formats, f: impl FnOnce() -> T) -> T {
        let before = NOW.with_borrow_mut(|now| now.replace(formats));
        let answer = f();
        NOW.with_borrow_mut(|now| *now = before);
        answer
    }

    pub fn numbers() -> String {
        NOW.with_borrow(|now| {
            now.as_ref()
                .map_or_else(|| super::ENGLISH.to_owned(), |f| f.numbers.clone())
        })
    }

    pub fn dates() -> String {
        NOW.with_borrow(|now| {
            now.as_ref()
                .map_or_else(|| super::ENGLISH.to_owned(), |f| f.dates.clone())
        })
    }
}

/// Dates and times as each locale writes them (ICU4X, CLDR's data), in the
/// system's time zone; made once a locale and shape on each thread.
mod dates {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use icu_datetime::fieldsets;
    use icu_datetime::{DateTimeFormatter, NoCalendarFormatter};

    use super::{Moment, Shape};

    enum Formatter {
        Time(NoCalendarFormatter<fieldsets::T>),
        Weekday(DateTimeFormatter<fieldsets::E>),
        DayMonth(DateTimeFormatter<fieldsets::MD>),
        Date(DateTimeFormatter<fieldsets::YMD>),
        DateTime(DateTimeFormatter<fieldsets::YMDT>),
        MonthYear(DateTimeFormatter<fieldsets::YM>),
        Month(DateTimeFormatter<fieldsets::M>),
    }

    thread_local! {
        static FORMATTERS: RefCell<HashMap<(String, &'static str), Option<Formatter>>> =
            RefCell::new(HashMap::new());
    }

    fn made(locale: &str, shape: Shape) -> Option<Formatter> {
        let locale: icu_locale_core::Locale = locale.parse().ok()?;
        let prefs = || (&locale).into();
        Some(match shape {
            Shape::Time => {
                Formatter::Time(NoCalendarFormatter::try_new(prefs(), fieldsets::T::hm()).ok()?)
            }
            Shape::Weekday => {
                Formatter::Weekday(DateTimeFormatter::try_new(prefs(), fieldsets::E::long()).ok()?)
            }
            Shape::DayMonth => Formatter::DayMonth(
                DateTimeFormatter::try_new(prefs(), fieldsets::MD::medium()).ok()?,
            ),
            Shape::Date => {
                Formatter::Date(DateTimeFormatter::try_new(prefs(), fieldsets::YMD::medium()).ok()?)
            }
            Shape::DateTime => Formatter::DateTime(
                DateTimeFormatter::try_new(prefs(), fieldsets::YMD::medium().with_time_hm())
                    .ok()?,
            ),
            Shape::DateTimeSeconds => Formatter::DateTime(
                DateTimeFormatter::try_new(prefs(), fieldsets::YMD::medium().with_time_hms())
                    .ok()?,
            ),
            Shape::MonthYear => Formatter::MonthYear(
                DateTimeFormatter::try_new(prefs(), fieldsets::YM::long()).ok()?,
            ),
            Shape::Month => {
                Formatter::Month(DateTimeFormatter::try_new(prefs(), fieldsets::M::long()).ok()?)
            }
        })
    }

    /// `moment` as `locale` writes its shape, in the system's time zone;
    /// as ISO 8601 where the locale's data is missing.
    pub fn written(locale: &str, moment: Moment) -> String {
        let Ok(at) = jiff::Timestamp::from_second(moment.at) else {
            return "—".to_owned();
        };
        let local = at.to_zoned(jiff::tz::TimeZone::system()).datetime();
        let as_iso = || local.strftime("%Y-%m-%d %H:%M").to_string();
        let (Ok(date), Ok(time)) = (
            icu_calendar::Date::try_new_iso(
                i32::from(local.year()),
                local.month().unsigned_abs(),
                local.day().unsigned_abs(),
            ),
            icu_time::Time::try_new(
                local.hour().unsigned_abs(),
                local.minute().unsigned_abs(),
                local.second().unsigned_abs(),
                0,
            ),
        ) else {
            return as_iso();
        };
        let both = icu_time::DateTime { date, time };
        FORMATTERS.with_borrow_mut(|formatters| {
            let formatter = formatters
                .entry((locale.to_owned(), moment.shape.word()))
                .or_insert_with(|| made(locale, moment.shape));
            match formatter {
                None => as_iso(),
                Some(Formatter::Time(f)) => f.format(&time).to_string(),
                Some(Formatter::Weekday(f)) => f.format(&both).to_string(),
                Some(Formatter::DayMonth(f)) => f.format(&both).to_string(),
                Some(Formatter::Date(f)) => f.format(&both).to_string(),
                Some(Formatter::DateTime(f)) => f.format(&both).to_string(),
                Some(Formatter::MonthYear(f)) => f.format(&both).to_string(),
                Some(Formatter::Month(f)) => f.format(&both).to_string(),
            }
        })
    }
}

/// Numbers as each language writes them (ICU4X, CLDR's data): its digits,
/// its groups, its decimal sign. Made once a language on each thread that
/// asks, as the plurals are.
mod numbers {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use icu_decimal::DecimalFormatter;
    use icu_decimal::input::Decimal;
    use icu_decimal::options::DecimalFormatterOptions;

    thread_local! {
        static FORMATTERS: RefCell<HashMap<String, Option<DecimalFormatter>>> =
            RefCell::new(HashMap::new());
    }

    fn formatted(language: &str, number: &Decimal) -> String {
        FORMATTERS.with_borrow_mut(|formatters| {
            let formatter = formatters.entry(language.to_owned()).or_insert_with(|| {
                let locale: icu_locale_core::Locale = language.parse().ok()?;
                DecimalFormatter::try_new(locale.into(), DecimalFormatterOptions::default()).ok()
            });
            match formatter {
                Some(formatter) => formatter.format_to_string(number),
                None => number.to_string(),
            }
        })
    }

    /// `n` with its groups in `language`: "1,234", « 1 234 ».
    pub fn grouped(language: &str, n: i64) -> String {
        formatted(language, &Decimal::from(n))
    }

    /// `value` divided by ten to the `digits`, with that many digits after
    /// the point, in `language`: "1.5", « 1,5 ».
    pub fn decimal(language: &str, value: i64, digits: u8) -> String {
        let mut number = Decimal::from(value);
        let digits = i16::from(digits);
        number.multiply_pow10(-digits);
        number.pad_end(-digits);
        formatted(language, &number)
    }
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

impl serde::Serialize for Words {
    /// Its English text: what leaves the process is read by agents and
    /// scripts, in English (ADR-139, 7).
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.english())
    }
}

impl<'de> serde::Deserialize<'de> for Words {
    /// Text that comes in (an agent's plan, a file kept) is as written;
    /// words' data (`Words::to_data`) comes back as the words it carries.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        data::Data::deserialize(deserializer).map(data::Data::into_words)
    }
}

impl Words {
    /// These words as data, for what the window sends itself as JSON (a
    /// form `ui.ask` shows, a menu a plugin answers, a search's chips):
    /// they come back as the words they are (`Deserialize`), said in the
    /// reader's language, where `Serialize` writes English for agents. An
    /// agent's plain text reads as written either way.
    pub fn to_data(&self) -> serde_json::Value {
        data::of(self)
    }
}

/// Words' data: a message by its domain, id, English and typed arguments,
/// or text as written. Use `#[serde(with = "skwir_core::words::data")]` on a
/// field the window sends itself.
pub mod data {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::{Arg, Carried, Inner, Join, Kind, Words};

    /// `words` written as their data.
    pub fn serialize<S: serde::Serializer>(
        words: &Words,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&of(words), serializer)
    }

    /// Words read back from their data, or text as written.
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Words, D::Error> {
        serde::Deserialize::deserialize(deserializer)
    }

    /// `#[serde(with = "skwir_core::words::data::optional")]`: words that
    /// may be missing (`Option<Words>`), as their data.
    pub mod optional {
        use super::{Words, of};

        #[allow(clippy::ref_option)] // The signature serde's `with` calls.
        pub fn serialize<S: serde::Serializer>(
            words: &Option<Words>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match words {
                Some(words) => serializer.serialize_some(&of(words)),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Words>, D::Error> {
            serde::Deserialize::deserialize(deserializer)
        }
    }

    /// `#[serde(with = "skwir_core::words::data::many")]`: a list of words
    /// (`Vec<Words>`), each as its data.
    pub mod many {
        use super::{Words, of};

        pub fn serialize<S: serde::Serializer>(
            words: &[Words],
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            serializer.collect_seq(words.iter().map(of))
        }

        pub fn deserialize<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Vec<Words>, D::Error> {
            serde::Deserialize::deserialize(deserializer)
        }
    }

    pub(super) fn of(words: &Words) -> Value {
        match &words.0 {
            Inner::Message(message, args) => message_of(
                message.domain,
                message.id,
                &words.english(),
                args.iter().map(|(n, a)| (*n, a)),
            ),
            Inner::Carried(carried) => message_of(
                &carried.domain,
                &carried.id,
                &carried.english,
                carried.args.iter().map(|(n, a)| (n.as_str(), a)),
            ),
            Inner::Heading(inner) => json!({ "heading": of(inner) }),
            Inner::Written(_) | Inner::Error(_) => Value::String(words.english()),
        }
    }

    fn message_of<'a>(
        domain: &str,
        id: &str,
        english: &str,
        args: impl Iterator<Item = (&'a str, &'a Arg)>,
    ) -> Value {
        let args: serde_json::Map<String, Value> = args
            .map(|(name, arg)| (name.to_owned(), arg_of(arg)))
            .collect();
        json!({ "domain": domain, "id": id, "english": english, "args": args })
    }

    fn arg_of(arg: &Arg) -> Value {
        match arg {
            Arg::Text(text) => json!({ "text": text }),
            Arg::Name(name) => json!({ "name": name }),
            Arg::Folder(folder) => json!({ "folder": folder }),
            Arg::Count(n) => json!({ "count": n }),
            Arg::Decimal(value, digits) => json!({ "decimal": [value, digits] }),
            Arg::Moment(moment) => json!({ "moment": [moment.at, moment.shape.word()] }),
            Arg::Size(n) => json!({ "size": n }),
            Arg::Kind(word) => json!({ "kind": word }),
            Arg::Words(words) => json!({ "words": of(words) }),
            Arg::Error(_) => {
                let mut said = String::new();
                super::said_arg(arg, super::ENGLISH, &[], &mut said);
                json!({ "text": said })
            }
            Arg::List(join, items) => {
                let items: Vec<Value> = items.iter().map(arg_of).collect();
                match join {
                    Join::And => json!({ "and": items }),
                    Join::Or => json!({ "or": items }),
                }
            }
        }
    }

    /// What comes in: text, a heading, or a message.
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    pub(super) enum Data {
        Text(String),
        Heading {
            heading: Box<Data>,
        },
        Message {
            domain: String,
            id: String,
            english: String,
            #[serde(default)]
            args: BTreeMap<String, ArgData>,
        },
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub(super) enum ArgData {
        Text(String),
        Name(String),
        Folder(String),
        Count(i64),
        Decimal((i64, u8)),
        Moment((i64, String)),
        Size(u64),
        Kind(String),
        Words(Data),
        And(Vec<ArgData>),
        Or(Vec<ArgData>),
    }

    impl Data {
        pub(super) fn into_words(self) -> Words {
            match self {
                Data::Text(text) => Words::as_written(text),
                Data::Heading { heading } => heading.into_words().heading(),
                Data::Message {
                    domain,
                    id,
                    english,
                    args,
                } => Words(Inner::Carried(Arc::new(Carried {
                    domain,
                    id,
                    english,
                    args: args.into_iter().map(|(n, a)| (n, a.into_arg())).collect(),
                }))),
            }
        }
    }

    impl ArgData {
        fn into_arg(self) -> Arg {
            match self {
                ArgData::Text(text) => Arg::Text(text.into()),
                ArgData::Name(name) => Arg::Name(name),
                ArgData::Folder(folder) => Arg::Folder(folder),
                ArgData::Count(n) => Arg::Count(n),
                ArgData::Decimal((value, digits)) => Arg::Decimal(value, digits),
                ArgData::Moment((at, shape)) => Arg::Moment(super::Moment {
                    at,
                    shape: super::Shape::ALL
                        .into_iter()
                        .find(|s| s.word() == shape)
                        .unwrap_or(super::Shape::DateTime),
                }),
                ArgData::Size(n) => Arg::Size(n),
                ArgData::Kind(word) => Arg::Kind(
                    [Kind::File, Kind::Folder, Kind::Link, Kind::Item]
                        .into_iter()
                        .find(|kind| kind.word() == word)
                        .unwrap_or(Kind::Item)
                        .word(),
                ),
                ArgData::Words(data) => Arg::Words(Box::new(data.into_words())),
                ArgData::And(items) => Arg::List(
                    Join::And,
                    items.into_iter().map(ArgData::into_arg).collect(),
                ),
                ArgData::Or(items) => {
                    Arg::List(Join::Or, items.into_iter().map(ArgData::into_arg).collect())
                }
            }
        }
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
    use icu_casemap::options::{LeadingAdjustment, TitlecaseOptions, TrailingCase};
    use icu_locale_core::LanguageIdentifier;

    /// `said` with its first character in its title case in `language`
    /// where it is a letter, the rest as it is: a sentence that begins with
    /// a quote (a name's) is left as it is, so a name is never changed.
    pub fn heading(language: &str, said: &str) -> String {
        let language: LanguageIdentifier = language.parse().unwrap_or(LanguageIdentifier::UNKNOWN);
        let mut options = TitlecaseOptions::default();
        options.trailing_case = Some(TrailingCase::Unchanged);
        options.leading_adjustment = Some(LeadingAdjustment::None);
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
