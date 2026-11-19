//! What Skwir says, as data (ADR-139, `docs/invariants/words.md`): a
//! crate's message and its typed arguments, said where it is drawn or sent,
//! in the language of who reads it. Written as English beside an id
//! (`words!`), which the macro makes a static [`Message`] of; translated in
//! each crate's Fluent catalogs ([`Catalog`]), said by Skwir's own
//! resolver; English last, always whole.
//!
//! As data that leaves the process (JSON for agents, scripts and the CLI),
//! `Words` is its English text: agents read English (ADR-139, 7).
//!
//! Where the window draws them (`Words::say`), a name, a folder and a path
//! named in a sentence are isolated from it (`isolated`): between FSI and
//! PDI, inside a name's quotes; and so is a name drawn alone
//! (`Words::name_alone`). Nothing else carries those marks: not English, not
//! words said in given languages, not text that leaves the window
//! (`Words::say_plain`).
//!
//! What a language needs to say is its catalogs' data, never code: the
//! quotes around a name are two of core's messages (`quotes`), and a noun
//! of Skwir's (a place's title: `words!(…, noun)`) carries its gender and
//! forms as its translation's attributes, which a sentence naming it reads
//! (`GENDER($place)`, `FORM($place, case: "dative")`, `noun_attribute`).

mod catalog;
mod pattern;

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

#[cfg(test)]
pub(crate) use catalog::described_indexed;
#[cfg(test)]
pub(crate) use catalog::was_read;
pub use catalog::{
    Catalog, Offer, Share, Shipped, describe_with, find, find_attribute, languages_of, register,
    register_shipped, register_shipped_of_program, share_of, ship, ship_shares, shipped_languages,
    withdraw,
};
pub use pattern::{Element, Key, Variant, from_fluent};

/// What a translation's functions and a noun's attributes are named
/// (ADR-139, 3 and 5).
pub mod grammar {
    pub use super::pattern::{CASE, FORM, GENDER, GENDER_ATTRIBUTE, NO_GENDER};
}

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
    /// Words that are a name, of someone else's making (`Words::shown`,
    /// `Words::name_alone`).
    Shown(Box<Words>),
    /// An error, each link of its chain said in the reader's language
    /// where it is words (`Words::of_error`).
    Error(Failure),
    /// Words that begin what is shown (a toast, a row), their first letter
    /// cased as the reader's language begins a sentence (`Words::heading`).
    Heading(Box<Words>),
    /// A message come back from its data (`Words::to_data`): said from the
    /// catalogs by its domain and id, else from its English as carried.
    Carried(Arc<Carried>),
    /// Words describing a plugin, said from its catalogs whether they are
    /// registered or not (`Words::describing`).
    Describing(Box<Words>),
}

/// A message known by its domain and id, as its data carries it.
#[derive(Debug, PartialEq, Eq, Hash)]
struct Carried {
    domain: String,
    id: String,
    /// Its English as English said it, for data that carries no pattern.
    english: String,
    /// Its English's shape, which says it with its arguments as the code's
    /// own message does (its names isolated where the window draws them).
    pattern: Option<Vec<Element>>,
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
    /// its bidirectional controls shown, never obeyed, isolated from the
    /// sentence where the window draws it.
    Name(String),
    /// A folder's name where it is a destination ("to Docs"): unquoted,
    /// its bidirectional controls shown, never obeyed, isolated from the
    /// sentence where the window draws it.
    Folder(String),
    /// A path (`/a/Docs/b.txt`), made of names: said as a folder is,
    /// unquoted, its bidirectional controls shown, never obeyed, isolated
    /// from the sentence where the window draws it. From a `&Path`.
    Path(String),
    /// Keys a shortcut is pressed with ("Ctrl+Shift+C"), as they are named
    /// where they are drawn (`Keys`): left to right in every language,
    /// isolated from the sentence where the window draws it (LRI, PDI), so a
    /// sentence written right to left never reorders its strokes; plain
    /// elsewhere.
    Keys(String),
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

/// Keys a shortcut is pressed with, for `Arg::Keys`: as the window names
/// them (`Ctrl+Shift+C`, `Strg+Umschalt+C`), each stroke's keys joined by
/// `+`, strokes by a space.
pub struct Keys(pub String);

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
///
/// A year, a month and a day of the month (`Year`, `MonthYear`, `Month`,
/// `MonthShort`, `Day`) name a period of the calendar Skwir counts in
/// (`skwir_platform::calendar`, the Gregorian): they are written on a
/// calendar whose months and years are its own, the locale's where they
/// are (the Buddhist counts its years from elsewhere), else the
/// Gregorian, in the locale's words. A Persian month is not a Gregorian
/// one: "March 2024" is never written as the Persian month it began in.
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
    /// The date with its month's whole name, as it is said aloud: "12
    /// March 2024", "March 12, 2024" (what a screen reader hears).
    DateLong,
    /// The date and the time: "12 Mar 2024, 14:02".
    DateTime,
    /// The date and the time to the second.
    DateTimeSeconds,
    /// The month and the year: "March 2024".
    MonthYear,
    /// The month: "March".
    Month,
    /// The month, shortened as the locale shortens it: "Mar", « mars »
    /// (a label in the room of a bar).
    MonthShort,
    /// The year: "2024" (« 2024年 » in Japanese).
    Year,
    /// The day of the month: "12" (« 12日 » in Japanese).
    Day,
}

impl Shape {
    fn word(self) -> &'static str {
        match self {
            Shape::Time => "time",
            Shape::Weekday => "weekday",
            Shape::DayMonth => "day_month",
            Shape::Date => "date",
            Shape::DateLong => "date_long",
            Shape::DateTime => "date_time",
            Shape::DateTimeSeconds => "date_time_seconds",
            Shape::MonthYear => "month_year",
            Shape::Month => "month",
            Shape::MonthShort => "month_short",
            Shape::Year => "year",
            Shape::Day => "day",
        }
    }

    /// Whether it names a period of Skwir's calendar (`Shape`).
    fn is_period(self) -> bool {
        matches!(
            self,
            Shape::MonthYear | Shape::Month | Shape::MonthShort | Shape::Year | Shape::Day
        )
    }

    const ALL: [Shape; 12] = [
        Shape::Time,
        Shape::Weekday,
        Shape::DayMonth,
        Shape::Date,
        Shape::DateLong,
        Shape::DateTime,
        Shape::DateTimeSeconds,
        Shape::MonthYear,
        Shape::Month,
        Shape::MonthShort,
        Shape::Year,
        Shape::Day,
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

    /// The civil day `year`-`month`-`day` (a period's first, a bar's, a
    /// group's), written as `shape` says: its noon in the zone moments
    /// are written in, so the day written is the one given whatever that
    /// zone, and whatever calendar counted it. `None` for a date there is
    /// not, or past what a date can say.
    pub fn on(year: i64, month: u32, day: u32, shape: Shape) -> Option<Self> {
        let date = jiff::civil::Date::new(
            i16::try_from(year).ok()?,
            i8::try_from(month).ok()?,
            i8::try_from(day).ok()?,
        )
        .ok()?;
        let noon = date
            .at(12, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::system())
            .ok()?;
        Some(Moment {
            at: noon.timestamp().as_second(),
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
    /// Written for the window, as the system's locale writes dates, or as
    /// ISO 8601 does where the window's dates setting says so.
    pub fn said(self) -> String {
        if crate::locale::iso_dates() {
            dates::iso(self)
        } else {
            dates::written(&crate::locale::dates(), self)
        }
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

impl From<&std::path::Path> for Arg {
    fn from(path: &std::path::Path) -> Self {
        Arg::Path(path.display().to_string())
    }
}

impl From<&std::path::PathBuf> for Arg {
    fn from(path: &std::path::PathBuf) -> Self {
        Arg::Path(path.display().to_string())
    }
}

impl From<Keys> for Arg {
    fn from(keys: Keys) -> Self {
        Arg::Keys(keys.0)
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

    /// A name drawn on its own, as rows, tiles, tabs, the trail and a
    /// window's title draw a file's or a folder's (ADR-139, 5): its
    /// bidirectional controls shown as tags, never obeyed, in every
    /// language and to every reader, and isolated where the window draws it
    /// (`say`: FSI before it, PDI after), nothing else added: no quotes, no
    /// case. As data it is the name as it is (`to_data`), which a search, a
    /// copy or an operation would see. In a sentence (as `Arg::Words`), it
    /// is isolated once, as a `Folder` is.
    pub fn name_alone(name: impl Into<String>) -> Self {
        Words::shown(Words::as_written(name.into()))
    }

    /// `words` that are a name of someone else's making, whose text Skwir
    /// does not choose: a plugin from outside's title, as its manifest or
    /// its catalogs write it (ADR-139, 5 and 14). Said as a name alone is
    /// ([`Words::name_alone`]): their bidirectional controls shown as tags,
    /// never obeyed, isolated where the window draws them, never cased; in
    /// a sentence, isolated once, so no control of theirs reorders the words
    /// around them. As data, the words they are, said so again.
    pub fn shown(words: Words) -> Self {
        match words.0 {
            Inner::Shown(_) => words,
            _ => Words(Inner::Shown(Box::new(words))),
        }
    }

    /// Message `id` of `domain`, known as the program runs rather than
    /// written in its code: what a plugin from outside says (protocol 2,
    /// ADR-139, 14), whose catalogs it brings. Said in the reader's
    /// languages from the catalogs registered for `domain` (`register`),
    /// else from `english`, its English (the plugin's `en.ftl`), else as its
    /// id: always whole, never nothing. Agents read the English.
    pub fn of_catalog(
        domain: impl Into<String>,
        id: impl Into<String>,
        english: Option<&[Element]>,
        args: Vec<(String, Arg)>,
    ) -> Self {
        let id = id.into();
        let said = match english {
            Some(pattern) => formats::within(formats::Formats::english(), || {
                let mut said = String::new();
                say(pattern, &args, ENGLISH, &[], &mut said, false, &mut false);
                said
            }),
            None => id.clone(),
        };
        Words(Inner::Carried(Arc::new(Carried {
            domain: domain.into(),
            id,
            english: said,
            pattern: english.map(<[Element]>::to_vec),
            args,
        })))
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
        match &self.0 {
            Inner::Written(text) => text.is_empty(),
            Inner::Shown(words) | Inner::Describing(words) => words.is_empty(),
            _ => false,
        }
    }

    /// The message these words say, by its domain and id (a heading's, one
    /// come back from its data): what a noun's attributes are found by.
    /// `None` for text as written or an error.
    fn said_message(&self) -> Option<(&str, &str)> {
        match &self.0 {
            Inner::Message(message, _) => Some((message.domain, message.id)),
            Inner::Heading(words) | Inner::Describing(words) => words.said_message(),
            Inner::Carried(carried) => Some((&carried.domain, &carried.id)),
            Inner::Written(_) | Inner::Shown(_) | Inner::Error(_) => None,
        }
    }

    /// Its message, `None` for text as written or an error.
    pub fn message_of(&self) -> Option<&'static Message> {
        match &self.0 {
            Inner::Message(message, _) => Some(message),
            Inner::Heading(words) | Inner::Describing(words) => words.message_of(),
            Inner::Written(_) | Inner::Shown(_) | Inner::Error(_) | Inner::Carried(_) => None,
        }
    }

    /// Whether these words are a message: one written in the code, one a
    /// plugin from outside says (`of_catalog`), or one come back from its
    /// data; not text as written, a name or an error. What a sentence is
    /// given as words, keeping a noun's grammar, where anything else is
    /// named (`Name`, `Folder`, said plainly).
    pub fn is_message(&self) -> bool {
        self.said_message().is_some()
    }

    /// These words where they describe a plugin (its title, what it does,
    /// what it needs a program for: `PluginHost::list`, `probe_requirements`),
    /// said whether it is on or off. A plugin's catalogs are registered only
    /// while it is on (a contribution, `register_shipped`); words describing
    /// it are said from them then, and while it is off from the catalogs
    /// Skwir ships for its crate, or those its host keeps for a plugin from
    /// outside (`describe_with`): read for these words alone, registering
    /// nothing. Only core makes them, where a plugin is described: nothing
    /// else a plugin that is off says (its commands, its answers) is said
    /// from its catalogs. As data, they are the words they wrap.
    #[must_use]
    pub(crate) fn describing(self) -> Words {
        match self.0 {
            Inner::Describing(_) => self,
            _ => Words(Inner::Describing(Box::new(self))),
        }
    }

    /// These words, describing a plugin where `describing` says so.
    fn describing_if(self, describing: bool) -> Words {
        if describing { self.describing() } else { self }
    }

    /// Its message and arguments, where it is one written in the code, and
    /// whether the words describe a plugin: what a sentence is cut from
    /// (`say_apart`, `say_around`).
    fn cut_from(&self) -> Option<(&'static Message, &Arc<[(&'static str, Arg)]>, bool)> {
        match &self.0 {
            Inner::Message(message, args) => Some((message, args, false)),
            Inner::Describing(words) => match &words.0 {
                Inner::Message(message, args) => Some((message, args, true)),
                _ => None,
            },
            _ => None,
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
        match &self.0 {
            // A name is no sentence: never cased.
            Inner::Shown(_) => self.clone(),
            _ => Words(Inner::Heading(Box::new(self.clone()))),
        }
    }

    /// Said in the reader's languages, cut around its arguments `names`:
    /// the text before the first one placed, then each placed, in the
    /// order the language places them, by its index in `names`, with the
    /// text that follows it up to the next (a line of facts drawn apart,
    /// wrapping only between them: the caller draws each argument). `None`
    /// where these words are no message placing each of `names` once.
    ///
    /// Said for the window, as [`Words::say`] says it: the other names are
    /// isolated, each within one part. An argument cut out leaves no mark
    /// isolating it in the parts (the caller draws it, an element of its
    /// own that nothing beside it reorders); a name cut out leaves its
    /// quotes there, the sentence's (`cut_mark`).
    pub fn say_apart(&self, names: &[&str]) -> Option<(String, Vec<(usize, String)>)> {
        let (message, args, describing) = self.cut_from()?;
        // A private-use character a name for each argument cut out.
        let mark = |at: usize| char::from_u32(CUT + u32::try_from(at).ok()?);
        let marked: Arc<[(&'static str, Arg)]> = args
            .iter()
            .map(|(n, a)| match names.iter().position(|name| name == n) {
                Some(at) => (*n, cut_mark(a, mark(at).unwrap_or(CUT_ONE))),
                None => (*n, a.clone()),
            })
            .collect();
        let said = Words(Inner::Message(message, marked))
            .describing_if(describing)
            .said_for_window(Isolation::NotCut);
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
    /// once. Said for the window, its other names isolated, the one cut out
    /// leaving only its quotes, as [`Words::say_apart`] says.
    pub fn say_around(&self, name: &str) -> Option<(String, String)> {
        let (message, args, describing) = self.cut_from()?;
        let marked: Arc<[(&'static str, Arg)]> = args
            .iter()
            .map(|(n, a)| {
                if *n == name {
                    (*n, cut_mark(a, CUT_ONE))
                } else {
                    (*n, a.clone())
                }
            })
            .collect();
        let said = Words(Inner::Message(message, marked))
            .describing_if(describing)
            .said_for_window(Isolation::NotCut);
        let mut parts = said.split(CUT_ONE);
        let (before, after) = (parts.next()?, parts.next()?);
        parts
            .next()
            .is_none()
            .then(|| (before.to_owned(), after.to_owned()))
    }

    /// Said in the languages Skwir speaks to the user (`locale::languages`):
    /// what the window draws, each name isolated from the sentence around
    /// it (`isolated`). The languages are kept on each thread, read again
    /// only when they move (`revision`): a frame says many words.
    pub fn say(&self) -> String {
        self.said_for_window(Isolation::Names)
    }

    /// Said as [`Words::say`] says them (the window's languages, its
    /// numbers and dates), but with no mark isolating a name: for what
    /// leaves the window as text rather than being drawn (the clipboard, a
    /// file's name, words a name is made of, a name kept as data), where an
    /// invisible mark would be a character of the text.
    pub fn say_plain(&self) -> String {
        self.said_for_window(Isolation::None)
    }

    fn said_for_window(&self, isolation: Isolation) -> String {
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
            (languages.clone(), formats::Formats::window(isolation))
        });
        formats::within(formats, || self.say_in(&languages))
    }

    /// Said in English: what agents, scripts and logs read, its numbers
    /// and dates as English writes them, no name isolated.
    pub fn english(&self) -> String {
        formats::within(formats::Formats::english(), || self.say_in(&[]))
    }

    /// Said in the first of `languages` that has its message, English
    /// last. Its names are isolated only where `say` says it (an error's
    /// chain said for the window says its links through here): on its own,
    /// it writes no mark.
    pub fn say_in(&self, languages: &[String]) -> String {
        let mut out = String::new();
        self.say_into(&mut out, languages);
        out
    }

    fn say_into(&self, out: &mut String, languages: &[String]) {
        self.say_found(out, languages, catalog::find, false);
    }

    /// Said as `say_into` says it, its message found in the catalogs by
    /// `find`: those registered, or, for words describing a plugin, its own
    /// whether registered or not (`catalog::find_describing`). The words it
    /// places are said as they are, from the catalogs registered.
    ///
    /// As a `heading`, its own first letter is cased as the language it is
    /// said in begins a sentence: the message's own text where it comes
    /// first, or words it places first, cased so in theirs; never an
    /// argument that is no words of Skwir's (a name, a folder, a path, text
    /// as written), which the window and plain text alike show as it is.
    fn say_found(&self, out: &mut String, languages: &[String], find: Finder, heading: bool) {
        let (message, args) = match &self.0 {
            Inner::Written(text) if heading => {
                let language = languages.first().map_or(ENGLISH, String::as_str);
                return out.push_str(&casing::heading(language, text));
            }
            Inner::Written(text) => return out.push_str(text),
            Inner::Shown(words) => {
                // Said without the marks of what they hold, which their
                // controls shown would show too: isolated once, whole.
                let said = formats::unmarked(|| {
                    let mut said = String::new();
                    words.say_found(&mut said, languages, find, false);
                    said
                });
                return isolated(&shown_controls(&said), out);
            }
            Inner::Error(failure) if heading => {
                let mut said = String::new();
                say_error(failure, languages, &mut said);
                let language = languages.first().map_or(ENGLISH, String::as_str);
                return out.push_str(&casing::heading(language, &said));
            }
            Inner::Error(failure) => return say_error(failure, languages, out),
            Inner::Describing(words) => {
                return words.say_found(out, languages, catalog::find_describing, heading);
            }
            Inner::Heading(words) => return words.say_found(out, languages, find, true),
            Inner::Carried(carried) => {
                let mut heading = heading;
                if let Some(pseudo) = pseudo_of(languages) {
                    // As a message is: its arguments marked, where its
                    // pattern came with it.
                    let Some(english) = &carried.pattern else {
                        return out.push_str(&pseudo(&carried.english));
                    };
                    // Its bracket first: nothing cased.
                    let mut said = String::new();
                    say(
                        english,
                        &carried.args,
                        ENGLISH,
                        languages,
                        &mut said,
                        true,
                        &mut false,
                    );
                    return out.push_str(&pseudo(&said));
                }
                return match (
                    find(&carried.domain, &carried.id, languages),
                    &carried.pattern,
                ) {
                    (Some((said, language)), _) => {
                        say(
                            &said,
                            &carried.args,
                            &language,
                            languages,
                            out,
                            false,
                            &mut heading,
                        );
                    }
                    (None, Some(english)) => {
                        say(
                            english,
                            &carried.args,
                            ENGLISH,
                            languages,
                            out,
                            false,
                            &mut heading,
                        );
                    }
                    (None, None) if heading => {
                        out.push_str(&casing::heading(ENGLISH, &carried.english));
                    }
                    (None, None) => out.push_str(&carried.english),
                };
            }
            Inner::Message(message, args) => (*message, args),
        };
        let mut heading = heading;
        if let Some(pseudo) = pseudo_of(languages) {
            // Its English made strange, nested words said in the
            // pseudo-language too (bracketed apart from what is as written:
            // its bracket first, nothing cased).
            let mut english = String::new();
            say(
                message.english,
                args,
                ENGLISH,
                languages,
                &mut english,
                true,
                &mut false,
            );
            return out.push_str(&pseudo(&english));
        }
        match find(message.domain, message.id, languages) {
            Some((said, language)) => {
                say(&said, args, &language, languages, out, false, &mut heading);
            }
            None => say(
                message.english,
                args,
                ENGLISH,
                languages,
                out,
                false,
                &mut heading,
            ),
        }
    }
}

/// How a message is found in the catalogs, in the first of the reader's
/// languages that has it: [`catalog::find`], among those registered, or
/// [`catalog::find_describing`].
type Finder = fn(&str, &str, &[String]) -> Option<(catalog::Said, String)>;

/// Say `elements` with `args` in `language` (its plurals, its quotes);
/// nested words in `language` first, then `languages`, so that a sentence
/// said in one language holds words of that one, whole (ADR-139, 4): an
/// English sentence (its translation missing) English words, though the
/// reader's catalog has them. In the pseudo-language each argument is
/// marked, so what is placed wrong shows, and nested words are said in it
/// too. While `heading` holds, nothing is said yet: the first text the
/// message says itself is cased as `language` begins a sentence, and
/// words it places first are cased in theirs; any other argument said
/// first leaves the sentence as it is.
fn say<N: AsRef<str>>(
    elements: &[Element],
    args: &[(N, Arg)],
    language: &str,
    languages: &[String],
    out: &mut String,
    marked: bool,
    heading: &mut bool,
) {
    let arg = |name: &str| {
        args.iter()
            .find(|(n, _)| n.as_ref() == name)
            .map(|(_, a)| a)
    };
    // Nested words' languages: the sentence's first, made once asked.
    let mut kept: Option<Vec<String>> = None;
    for element in elements {
        match element {
            Element::Text(text) if *heading && !text.is_empty() => {
                *heading = false;
                out.push_str(&casing::heading(language, text));
            }
            Element::Text(text) => out.push_str(text),
            Element::Arg(name) => {
                if let Some(value) = arg(name) {
                    if marked {
                        out.push('‹');
                    }
                    let nested = nested(&mut kept, value, language, languages, marked);
                    placed(value, language, nested, out, heading);
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
                say(value, args, language, languages, out, marked, heading);
            }
            Element::Gender {
                on,
                variants,
                default,
            } => {
                let gender = arg(on).and_then(|value| {
                    let nested = nested(&mut kept, value, language, languages, marked);
                    noun_attribute(value, pattern::GENDER_ATTRIBUTE, language, nested)
                });
                let gender = gender.as_deref().map_or(pattern::NO_GENDER, str::trim);
                let value = pattern::choose(variants, *default, None, Some(gender), |_| {
                    pattern::NO_GENDER
                });
                say(value, args, language, languages, out, marked, heading);
            }
            Element::Form { of, case } => {
                let Some(value) = arg(of) else {
                    out.push_str("{$");
                    out.push_str(of);
                    out.push('}');
                    continue;
                };
                if marked {
                    out.push('‹');
                }
                let nested = nested(&mut kept, value, language, languages, marked);
                match noun_attribute(value, case, language, nested) {
                    // A noun's form, Skwir's words: cased where it begins a
                    // heading, as its own text would be.
                    Some(form) if *heading && !form.is_empty() => {
                        *heading = false;
                        out.push_str(&casing::heading(language, &form));
                    }
                    Some(form) => out.push_str(&form),
                    // No noun, or none in this language: as it is said.
                    None => placed(value, language, nested, out, heading),
                }
                if marked {
                    out.push('›');
                }
            }
        }
    }
}

/// The languages a sentence said in `language` says `value` in, where it
/// holds words (words, a size's, an error's, a list's): `language` first,
/// then the reader's `languages` (kept in `kept`, made once a sentence);
/// the reader's alone where they begin with it, where `value` holds no
/// words, or where the sentence is the pseudo-language's (`marked`), whose
/// nested words are said in it too.
fn nested<'a>(
    kept: &'a mut Option<Vec<String>>,
    value: &Arg,
    language: &str,
    languages: &'a [String],
    marked: bool,
) -> &'a [String] {
    let holds_words = matches!(
        value,
        Arg::Words(_) | Arg::Size(_) | Arg::Error(_) | Arg::List(..)
    );
    if !holds_words || marked || languages.first().is_some_and(|first| first == language) {
        return languages;
    }
    kept.get_or_insert_with(|| {
        std::iter::once(language.to_owned())
            .chain(languages.iter().cloned())
            .collect()
    })
}

/// Argument `value`, placed in a sentence said in `language`, its nested
/// words in `nested`. Where it begins a `heading`, words of Skwir's are
/// cased as they begin one; any other argument leaves it as it is.
fn placed(value: &Arg, language: &str, nested: &[String], out: &mut String, heading: &mut bool) {
    let before = out.len();
    match value {
        Arg::Words(words) if *heading => words.say_found(out, nested, catalog::find, true),
        _ => said_arg(value, language, nested, out),
    }
    if out.len() > before {
        *heading = false;
    }
}

/// Attribute `name` (`gender`, one of its forms) of the noun `arg` is, said
/// as the catalog that says the noun has it (`catalog::find_attribute`),
/// where that catalog's words are the sentence's: one language in one
/// script (`locale::same_writing`: `fr` and `fr-CA`, `zh-Hant` and `zh-HK`).
///
/// `None` where `arg` is no noun (no message of Skwir's: a file's name, a
/// path, text as written), where the noun is said in English, which gives
/// none, or in another language than the sentence's (a reader's language
/// the sentence's catalog lacks: its grammar is not the sentence's), and
/// where its catalog gives it no such attribute. Then a `GENDER` says
/// `other` and a `FORM` the argument as it is.
fn noun_attribute(arg: &Arg, name: &str, language: &str, languages: &[String]) -> Option<String> {
    let Arg::Words(words) = arg else {
        return None;
    };
    let (domain, id) = words.said_message()?;
    let (said, found_in) = catalog::find_attribute(domain, id, name, languages)?;
    if !crate::locale::same_writing(&found_in, language) {
        return None;
    }
    let mut out = String::new();
    say(
        &said?,
        &[] as &[(&str, Arg)],
        language,
        languages,
        &mut out,
        false,
        &mut false,
    );
    Some(out)
}

/// An argument said in `language`.
fn said_arg(arg: &Arg, language: &str, languages: &[String], out: &mut String) {
    match arg {
        Arg::Text(text) => out.push_str(text),
        Arg::Name(name) => quoted(language, &shown_controls(name), out),
        Arg::Folder(name) | Arg::Path(name) => isolated(&shown_controls(name), out),
        Arg::Keys(keys) => left_to_right(keys, out),
        Arg::Count(n) => out.push_str(&numbers::grouped(&formats::numbers(), *n)),
        Arg::Decimal(value, digits) => {
            out.push_str(&numbers::decimal(&formats::numbers(), *value, *digits));
        }
        Arg::Moment(moment) => out.push_str(&if formats::iso() {
            dates::iso(*moment)
        } else {
            dates::written(&formats::dates(), *moment)
        }),
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

/// `name` quoted as `language` quotes (`quotes`), isolated inside its quotes
/// where the window draws it (`isolated`).
fn quoted(language: &str, name: &str, out: &mut String) {
    quotes(language, |open, close| {
        out.push_str(open);
        isolated(name, out);
        out.push_str(close);
    });
}

/// `f` given the marks `language` opens and closes a quoted name with:
/// core's two messages said in it, its catalog's data (“…” in English, « … »
/// in French, „…“ in German, 「…」 in Japanese and Traditional Chinese), so
/// a language needs no code to quote as it does. Where the catalogs it
/// finds (as words find theirs: `fr-CA` finds `fr`) have none, English's.
/// Kept on each thread while what is said stays as it was (`revision`): a
/// frame quotes many names.
fn quotes<T>(language: &str, f: impl FnOnce(&str, &str) -> T) -> T {
    type Marks = std::rc::Rc<(String, String)>;
    thread_local! {
        // A few languages at most (a sentence's, English where one falls
        // back): looked through rather than hashed.
        static QUOTES: std::cell::RefCell<(u64, Vec<(String, Marks)>)> =
            const { std::cell::RefCell::new((u64::MAX, Vec::new())) };
    }
    let now = revision();
    let kept = QUOTES.with_borrow_mut(|(at, kept)| {
        if *at != now {
            kept.clear();
            *at = now;
        }
        kept.iter()
            .find(|(tag, _)| tag == language)
            .map(|(_, marks)| marks.clone())
    });
    let marks = kept.unwrap_or_else(|| {
        let languages = [language.to_owned()];
        let marks: Marks = std::rc::Rc::new((
            crate::words!(
                name_quote_open = "“",
                note = "The mark that opens a name Skwir quotes in a sentence (a file's, a place's, a tag's): your language's opening quotation mark, with the space it takes inside it, if any (in French, « then a narrow no-break space: «{ \"\\u202F\" }). Skwir puts it before every name it quotes: write no quotation marks around a name's argument in a message."
            )
            .say_in(&languages),
            crate::words!(
                name_quote_close = "”",
                note = "The mark that closes a name Skwir quotes in a sentence: your language's closing quotation mark, with the space it takes inside it, if any (in French, a narrow no-break space then »: { \"\\u202F\" }»)."
            )
            .say_in(&languages),
        ));
        QUOTES.with_borrow_mut(|(_, kept)| kept.push((language.to_owned(), marks.clone())));
        marks
    });
    f(&marks.0, &marks.1)
}

/// FIRST STRONG ISOLATE and POP DIRECTIONAL ISOLATE (UAX #9): what keeps a
/// name apart from the sentence it is said in, where the window draws it.
const FSI: char = '\u{2068}';
const PDI: char = '\u{2069}';

/// Whether names are isolated where words are being said (`isolated`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Isolation {
    /// None is: English (agents, logs, tests), words said in given
    /// languages alone (`say_in`), text that leaves the window
    /// (`say_plain`).
    None,
    /// Every name: what the window draws (`say`).
    Names,
    /// Every name but those a sentence is cut around (`say_around`,
    /// `say_apart`): their caller draws them apart.
    NotCut,
}

/// The private-use characters a sentence cut around its arguments stands in
/// their places with: `say_apart`'s, one an argument from `CUT` on, and
/// `say_around`'s.
const CUT: u32 = 0xF8E0;
const CUT_ONE: char = '\u{F8FF}';

/// What stands in the place of `arg` while a sentence is cut around it:
/// `mark`, quoted as the argument is where it is a name (the quotes belong
/// to the sentence, as they do around a name said in it), else bare.
fn cut_mark(arg: &Arg, mark: char) -> Arg {
    match arg {
        Arg::Name(_) => Arg::Name(mark.to_string()),
        _ => Arg::Text(Cow::Owned(mark.to_string())),
    }
}

/// `name`, said in a sentence: between FSI and PDI where the window draws
/// it (`Words::say`), so that a name written right to left (Hebrew,
/// Arabic), or one that ends in digits or punctuation, reorders nothing of
/// the sentence around it, nor the sentence anything of it, whatever the
/// sentence's direction (UAX #9, X5c: the isolate takes its direction from
/// the name's first strong letter). The quotes stay outside, the
/// sentence's: in it, they open and close in its direction, as its other
/// punctuation does; inside, a name in Hebrew would draw them reversed.
/// The name holds no control of its own that could end the isolate early
/// or reverse what follows it: `shown_controls` showed every one as a tag.
/// As it is wherever else words are said (`Isolation::None`), and where it
/// is a mark a sentence is cut around (`Isolation::NotCut`); an empty name
/// takes no marks.
fn isolated(name: &str, out: &mut String) {
    let cut = || {
        let mut chars = name.chars();
        matches!(
            (chars.next(), chars.next()),
            (Some(c), None) if (CUT..=u32::from(CUT_ONE)).contains(&u32::from(c))
        )
    };
    let isolates = match formats::isolation() {
        Isolation::None => false,
        Isolation::Names => true,
        Isolation::NotCut => !cut(),
    };
    if !isolates || name.is_empty() {
        return out.push_str(name);
    }
    out.push(FSI);
    out.push_str(name);
    out.push(PDI);
}

/// LEFT-TO-RIGHT ISOLATE (UAX #9): what keeps keys left to right inside
/// a sentence, whatever its direction, where the window draws it.
const LRI: char = '\u{2066}';

/// `keys`, said in a sentence: between LRI and PDI where the window draws
/// it (`Words::say`), so "Ctrl+Shift+C" reads so in a sentence written right
/// to left, and the sentence's punctuation around it stays the sentence's.
/// As it is wherever else words are said (`Isolation::None`: English, data,
/// text that leaves the window), and where it is a mark a sentence is cut
/// around (`Isolation::NotCut`: its caller draws it apart); empty keys take
/// no marks.
fn left_to_right(keys: &str, out: &mut String) {
    let cut = || {
        let mut chars = keys.chars();
        matches!(
            (chars.next(), chars.next()),
            (Some(c), None) if (CUT..=u32::from(CUT_ONE)).contains(&u32::from(c))
        )
    };
    let isolates = match formats::isolation() {
        Isolation::None => false,
        Isolation::Names => true,
        Isolation::NotCut => !cut(),
    };
    if !isolates || keys.is_empty() {
        return out.push_str(keys);
    }
    out.push(LRI);
    out.push_str(keys);
    out.push(PDI);
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

/// The locales numbers and dates are written in while words are said, and
/// whether names are isolated: the system's, names isolated, for the window
/// (`say`); English's, none isolated, for agents (`english`), and for words
/// said in given languages (`say_in`) outside both.
mod formats {
    use std::cell::RefCell;

    use super::Isolation;

    #[derive(Clone)]
    pub struct Formats {
        numbers: String,
        dates: String,
        /// Dates as ISO 8601 writes them (the window's dates setting).
        iso: bool,
        isolation: Isolation,
    }

    impl Formats {
        pub fn window(isolation: Isolation) -> Self {
            Formats {
                numbers: crate::locale::numbers(),
                dates: crate::locale::dates(),
                iso: crate::locale::iso_dates(),
                isolation,
            }
        }

        pub fn english() -> Self {
            Formats {
                numbers: super::ENGLISH.to_owned(),
                dates: super::ENGLISH.to_owned(),
                iso: false,
                isolation: Isolation::None,
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

    pub fn iso() -> bool {
        NOW.with_borrow(|now| now.as_ref().is_some_and(|f| f.iso))
    }

    pub fn isolation() -> Isolation {
        NOW.with_borrow(|now| now.as_ref().map_or(Isolation::None, |f| f.isolation))
    }

    /// `f`'s answer, words said within it as they are now but marking no
    /// name (`Isolation::None`).
    pub fn unmarked<T>(f: impl FnOnce() -> T) -> T {
        match NOW.with_borrow(Clone::clone) {
            Some(mut formats) => {
                formats.isolation = Isolation::None;
                within(formats, f)
            }
            None => f(),
        }
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
        Year(DateTimeFormatter<fieldsets::Y>),
        Day(DateTimeFormatter<fieldsets::D>),
    }

    thread_local! {
        static FORMATTERS: RefCell<HashMap<(String, &'static str), Option<Formatter>>> =
            RefCell::new(HashMap::new());
    }

    /// What `locale` writes `shape` with: its own preferences; for a
    /// period of Skwir's calendar, on the Gregorian calendar where the
    /// locale's months or years begin elsewhere (`Shape`).
    fn prefs_of(
        locale: &icu_locale_core::Locale,
        shape: Shape,
    ) -> icu_datetime::DateTimeFormatterPreferences {
        use icu_calendar::AnyCalendarKind;
        let mut prefs: icu_datetime::DateTimeFormatterPreferences = locale.into();
        let kind = AnyCalendarKind::try_new(locale.into()).unwrap_or(AnyCalendarKind::Gregorian);
        // Months and years that begin where the Gregorian's do, their
        // years counted from elsewhere or not.
        let gregorian_months = matches!(
            kind,
            AnyCalendarKind::Gregorian
                | AnyCalendarKind::Iso
                | AnyCalendarKind::Buddhist
                | AnyCalendarKind::Roc
        );
        if shape.is_period() && !gregorian_months {
            prefs.calendar_algorithm = Some(icu_calendar::preferences::CalendarAlgorithm::Gregory);
        }
        prefs
    }

    fn made(locale: &str, shape: Shape) -> Option<Formatter> {
        let locale: icu_locale_core::Locale = locale.parse().ok()?;
        let prefs = || prefs_of(&locale, shape);
        Some(match shape {
            Shape::Time => Formatter::Time(
                NoCalendarFormatter::try_new((&locale).into(), fieldsets::T::hm()).ok()?,
            ),
            Shape::Weekday => {
                Formatter::Weekday(DateTimeFormatter::try_new(prefs(), fieldsets::E::long()).ok()?)
            }
            Shape::DayMonth => Formatter::DayMonth(
                DateTimeFormatter::try_new(prefs(), fieldsets::MD::medium()).ok()?,
            ),
            Shape::Date => {
                Formatter::Date(DateTimeFormatter::try_new(prefs(), fieldsets::YMD::medium()).ok()?)
            }
            Shape::DateLong => {
                Formatter::Date(DateTimeFormatter::try_new(prefs(), fieldsets::YMD::long()).ok()?)
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
            Shape::MonthShort => {
                Formatter::Month(DateTimeFormatter::try_new(prefs(), fieldsets::M::medium()).ok()?)
            }
            Shape::Year => {
                Formatter::Year(DateTimeFormatter::try_new(prefs(), fieldsets::Y::medium()).ok()?)
            }
            Shape::Day => {
                Formatter::Day(DateTimeFormatter::try_new(prefs(), fieldsets::D::medium()).ok()?)
            }
        })
    }

    /// `moment` as `locale` writes its shape, in the system's time zone;
    /// as ISO 8601 where the locale's data is missing.
    /// `moment` as ISO 8601 writes it, in the system's zone: a date
    /// `2026-03-12`, a time `14:02`, a month `2026-03`. A weekday is its
    /// date (ISO names none); a month or a day labelling a bar, among its
    /// year's or its month's, is its two digits (`03`).
    pub fn iso(moment: Moment) -> String {
        let Ok(at) = jiff::Timestamp::from_second(moment.at) else {
            return "—".to_owned();
        };
        let local = at.to_zoned(jiff::tz::TimeZone::system()).datetime();
        let format = match moment.shape {
            Shape::Time => "%H:%M",
            Shape::Weekday | Shape::DayMonth | Shape::Date | Shape::DateLong => "%Y-%m-%d",
            Shape::DateTime => "%Y-%m-%d %H:%M",
            Shape::DateTimeSeconds => "%Y-%m-%d %H:%M:%S",
            Shape::MonthYear | Shape::Month => "%Y-%m",
            Shape::MonthShort => "%m",
            Shape::Year => "%Y",
            Shape::Day => "%d",
        };
        local.strftime(format).to_string()
    }

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
                Some(Formatter::Year(f)) => f.format(&both).to_string(),
                Some(Formatter::Day(f)) => f.format(&both).to_string(),
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

/// What English is made into where the first of `languages` is a
/// pseudo-language: the English said with each argument between ‹ and ›,
/// made strange: accented and longer (`pseudo`: CLDR's `en-XA`), or drawn
/// right to left (`direction::pseudo_rtl`: CLDR's `ar-XB`). `None` for a
/// real language.
fn pseudo_of(languages: &[String]) -> Option<fn(&str) -> String> {
    match languages.first().map(String::as_str) {
        Some(PSEUDO) => Some(pseudo),
        Some(crate::direction::PSEUDO_RTL) => Some(crate::direction::pseudo_rtl),
        _ => None,
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

/// Words' data: a message by its domain, id, English (as said, and its
/// pattern) and typed arguments, or text as written. Use
/// `#[serde(with = "skwir_core::words::data")]` on a field the window sends
/// itself.
pub mod data {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::{Arg, Carried, Element, Inner, Join, Key, Kind, Variant, Words};

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
                Some(message.english),
                args.iter().map(|(n, a)| (*n, a)),
            ),
            Inner::Carried(carried) => message_of(
                &carried.domain,
                &carried.id,
                &carried.english,
                carried.pattern.as_deref(),
                carried.args.iter().map(|(n, a)| (n.as_str(), a)),
            ),
            Inner::Heading(inner) => json!({ "heading": of(inner) }),
            // What a plugin is described by is the words it is, as data:
            // nothing that comes back from data is said from the catalogs of
            // a plugin that is off.
            Inner::Describing(inner) => of(inner),
            Inner::Shown(inner) => json!({ "alone": of(inner) }),
            Inner::Written(_) | Inner::Error(_) => Value::String(words.english()),
        }
    }

    fn message_of<'a>(
        domain: &str,
        id: &str,
        english: &str,
        pattern: Option<&[Element]>,
        args: impl Iterator<Item = (&'a str, &'a Arg)>,
    ) -> Value {
        let args: serde_json::Map<String, Value> = args
            .map(|(name, arg)| (name.to_owned(), arg_of(arg)))
            .collect();
        let mut data = json!({ "domain": domain, "id": id, "english": english, "args": args });
        if let Some(pattern) = pattern {
            data["pattern"] = pattern_of(pattern);
        }
        data
    }

    /// A message's English shape as data: text as a string, an argument
    /// as `{"arg": name}`, a selector as `{"select": on, "variants": [{"key":
    /// word or number, "value": [...]}], "default": index}` (`"gender"` for
    /// `"select"` where it chooses by a noun's gender), a noun's form as
    /// `{"form": of, "case": form}`. English chooses by no gender and names
    /// no form, but the shape says what any pattern holds.
    fn pattern_of(elements: &[Element]) -> Value {
        let variants_of = |variants: &[Variant]| -> Vec<Value> {
            variants
                .iter()
                .map(|variant| {
                    let key = match &variant.key {
                        Key::Word(word) => json!(word),
                        Key::Number(n) => json!(n),
                    };
                    json!({ "key": key, "value": pattern_of(&variant.value) })
                })
                .collect()
        };
        Value::Array(
            elements
                .iter()
                .map(|element| match element {
                    Element::Text(text) => Value::String(text.to_string()),
                    Element::Arg(name) => json!({ "arg": name }),
                    Element::Select {
                        on,
                        variants,
                        default,
                    } => {
                        json!({ "select": on, "variants": variants_of(variants), "default": default })
                    }
                    Element::Gender {
                        on,
                        variants,
                        default,
                    } => {
                        json!({ "gender": on, "variants": variants_of(variants), "default": default })
                    }
                    Element::Form { of, case } => json!({ "form": of, "case": case }),
                })
                .collect(),
        )
    }

    fn arg_of(arg: &Arg) -> Value {
        match arg {
            Arg::Text(text) => json!({ "text": text }),
            Arg::Name(name) => json!({ "name": name }),
            Arg::Folder(folder) => json!({ "folder": folder }),
            Arg::Path(path) => json!({ "path": path }),
            Arg::Keys(keys) => json!({ "keys": keys }),
            Arg::Count(n) => json!({ "count": n }),
            Arg::Decimal(value, digits) => json!({ "decimal": [value, digits] }),
            Arg::Moment(moment) => json!({ "moment": [moment.at, moment.shape.word()] }),
            Arg::Size(n) => json!({ "size": n }),
            Arg::Kind(word) => json!({ "kind": word }),
            Arg::Words(words) => json!({ "words": of(words) }),
            Arg::Error(_) => {
                // In English, whatever saying it is within: data carries no
                // mark.
                let said = super::formats::within(super::formats::Formats::english(), || {
                    let mut said = String::new();
                    super::said_arg(arg, super::ENGLISH, &[], &mut said);
                    said
                });
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
        /// Words that are a name (`Words::shown`): a name drawn alone.
        Alone {
            alone: Box<Data>,
        },
        Message {
            domain: String,
            id: String,
            english: String,
            #[serde(default)]
            pattern: Option<Vec<ElementData>>,
            #[serde(default)]
            args: BTreeMap<String, ArgData>,
        },
    }

    /// A part of a message's English shape, as `pattern_of` writes it.
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    pub(super) enum ElementData {
        Text(String),
        Arg {
            arg: String,
        },
        Select {
            select: String,
            variants: Vec<VariantData>,
            default: usize,
        },
        Gender {
            gender: String,
            variants: Vec<VariantData>,
            default: usize,
        },
        Form {
            form: String,
            case: String,
        },
    }

    #[derive(serde::Deserialize)]
    pub(super) struct VariantData {
        key: KeyData,
        value: Vec<ElementData>,
    }

    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    pub(super) enum KeyData {
        Word(String),
        Number(i64),
    }

    impl VariantData {
        fn into_variant(self) -> Variant {
            Variant {
                key: match self.key {
                    KeyData::Word(word) => Key::Word(word.into()),
                    KeyData::Number(n) => Key::Number(n),
                },
                value: self
                    .value
                    .into_iter()
                    .map(ElementData::into_element)
                    .collect(),
            }
        }
    }

    impl ElementData {
        fn into_element(self) -> Element {
            match self {
                ElementData::Text(text) => Element::Text(text.into()),
                ElementData::Arg { arg } => Element::Arg(arg.into()),
                ElementData::Select {
                    select,
                    variants,
                    default,
                } => Element::Select {
                    on: select.into(),
                    variants: variants
                        .into_iter()
                        .map(VariantData::into_variant)
                        .collect(),
                    default,
                },
                ElementData::Gender {
                    gender,
                    variants,
                    default,
                } => Element::Gender {
                    on: gender.into(),
                    variants: variants
                        .into_iter()
                        .map(VariantData::into_variant)
                        .collect(),
                    default,
                },
                ElementData::Form { form, case } => Element::Form {
                    of: form.into(),
                    case: case.into(),
                },
            }
        }
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub(super) enum ArgData {
        Text(String),
        Name(String),
        Folder(String),
        Path(String),
        Keys(String),
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
                Data::Alone { alone } => Words::shown(alone.into_words()),
                Data::Message {
                    domain,
                    id,
                    english,
                    pattern,
                    args,
                } => Words(Inner::Carried(Arc::new(Carried {
                    domain,
                    id,
                    english,
                    pattern: pattern.map(|pattern| {
                        pattern.into_iter().map(ElementData::into_element).collect()
                    }),
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
                ArgData::Path(path) => Arg::Path(path),
                ArgData::Keys(keys) => Arg::Keys(keys),
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
