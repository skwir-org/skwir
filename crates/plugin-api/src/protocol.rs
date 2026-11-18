//! The methods skwir and a plugin from outside call, and their JSON
//! (ADR-045). A program speaks them as JSON-RPC 2.0, one message per line
//! on its standard input and output ([`Message`]); a WebAssembly module
//! answers them through its one `call` export, and calls skwir through
//! its imports, `read` and `log` (`wit/plugin.wit`).
//!
//! skwir asks ([`method`]): `initialize` (a program, first), `field`,
//! `thumbnail`, `inspect`, `convert`, `page`, `command`. A plugin asks
//! back, while it
//! answers: `read` (a file of the call, by its handle), `log`. skwir
//! sends a program `$/cancel` when it stops waiting for an answer, and
//! closes its standard input when it is no longer needed.
//!
//! Protocol 2 (ADR-139, 14): skwir tells the plugin the reader's languages
//! (`InitializeParams::languages`; a module asks its `languages` import),
//! and what a plugin says to a person may be a message of its own
//! catalogs ([`Words`]), which skwir says in each reader's language.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The version of the protocol this crate speaks (`api` in a manifest).
/// The protocol is experimental (ADR-047, its sixth point): until it is
/// declared stable, a new version may change what an older one said,
/// and [`OLDEST`] then follows it. Version 2 adds the reader's languages
/// and [`Words`] (ADR-139, 14).
pub const VERSION: u32 = 2;

/// The oldest version skwir still speaks: a plugin written for it runs
/// unchanged, and is shown as it writes.
pub const OLDEST: u32 = 1;

/// The version a manifest that names none speaks: the first, so that a
/// plugin written before the second runs as it did.
pub const UNNAMED: u32 = 1;

/// The longest message, in bytes with its newline: a thumbnail's PNG or a
/// page of bytes read, in base64.
pub const MESSAGE_LIMIT: usize = 64 * 1024 * 1024;

/// The most bytes one `read` gives.
pub const READ_LIMIT: u32 = 4 * 1024 * 1024;

/// The methods' names.
pub mod method {
    /// skwir → a program, first: [`InitializeParams`](super::InitializeParams).
    pub const INITIALIZE: &str = "initialize";
    /// skwir → plugin: [`FieldParams`](super::FieldParams) → [`FieldAnswer`](super::FieldAnswer).
    pub const FIELD: &str = "field";
    /// skwir → plugin: [`ThumbnailParams`](super::ThumbnailParams) →
    /// [`ThumbnailAnswer`](super::ThumbnailAnswer).
    pub const THUMBNAIL: &str = "thumbnail";
    /// skwir → plugin: [`InspectParams`](super::InspectParams) → an
    /// [`Inspection`](super::Inspection) or null.
    pub const INSPECT: &str = "inspect";
    /// skwir → plugin: [`ConvertParams`](super::ConvertParams)
    /// → a [`ConvertAnswer`](super::ConvertAnswer) or null.
    pub const CONVERT: &str = "convert";
    /// skwir → plugin: [`PageParams`](super::PageParams) →
    /// a [`PageAnswer`](super::PageAnswer) or null.
    pub const PAGE: &str = "page";
    /// skwir → plugin: [`CommandParams`](super::CommandParams) →
    /// [`CommandAnswer`](super::CommandAnswer).
    pub const COMMAND: &str = "command";
    /// skwir → a program, a notification: stop answering `{ "id" }`.
    pub const CANCEL: &str = "$/cancel";
    /// plugin → skwir: [`ReadParams`](super::ReadParams) → [`ReadAnswer`](super::ReadAnswer).
    pub const READ: &str = "read";
    /// plugin → skwir, a notification: [`LogParams`](super::LogParams).
    pub const LOG: &str = "log";
}

/// A file a call is about. Its bytes are read through skwir by `handle`,
/// valid while the call is answered; its path and name are for showing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub handle: u32,
    /// Its name, as text (a name that is no UTF-8 is shown lossily).
    pub name: String,
    /// Its path, as text, for showing.
    pub path: String,
    /// Its MIME type, when known.
    #[serde(default)]
    pub mime: Option<String>,
    /// Its size in bytes, when known.
    #[serde(default)]
    pub size: Option<u64>,
}

/// What a plugin says to a person: text as it writes it, shown as written
/// in every language; or, on protocol 2 (ADR-139, 14), a message of its own
/// catalogs (`l10n/<language>.ftl` beside its manifest, `en.ftl` its
/// English), which skwir says in each reader's language: the window's for
/// what the window shows, English for agents. A message missing in the
/// reader's language is said in English, one missing in English too as its
/// id.
///
/// JSON: a string, or `{ "id": "count-copied", "args": { "count": 3 } }`
/// (`args` may be left out).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Words {
    /// Text as the plugin wrote it.
    Written(String),
    /// Message `id` of the plugin's catalogs, its arguments by name.
    Message {
        id: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        args: BTreeMap<String, Arg>,
    },
}

impl Default for Words {
    /// No words: an empty text, what a notice says when there is none.
    fn default() -> Self {
        Words::Written(String::new())
    }
}

impl From<&str> for Words {
    fn from(text: &str) -> Self {
        Words::Written(text.to_owned())
    }
}

impl From<String> for Words {
    fn from(text: String) -> Self {
        Words::Written(text)
    }
}

impl Words {
    /// Message `id` of the plugin's catalogs, with no arguments yet
    /// ([`Words::with`]).
    pub fn message(id: impl Into<String>) -> Self {
        Words::Message {
            id: id.into(),
            args: BTreeMap::new(),
        }
    }

    /// These words with the argument `name` given `value`: a message's
    /// argument (text as written stays as it is).
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: impl Into<Arg>) -> Self {
        if let Words::Message { args, .. } = &mut self {
            args.insert(name.into(), value.into());
        }
        self
    }

    /// Whether they say nothing: an empty text.
    pub fn is_empty(&self) -> bool {
        matches!(self, Words::Written(text) if text.is_empty())
    }

    /// The words an answer's error `text` carries: the message it is the
    /// JSON of (what a module, whose error is a string, answers to say its
    /// error as a message, and what `String::from(words)` makes), else
    /// none: the text is the error, as written.
    pub fn in_error(text: &str) -> Option<Words> {
        if !text.starts_with('{') {
            return None;
        }
        match serde_json::from_str::<Words>(text).ok()? {
            message @ Words::Message { .. } => Some(message),
            Words::Written(_) => None,
        }
    }
}

impl From<Words> for String {
    /// Words as an answer's error, for a method that fails with a `String`
    /// (`Err(Words::message("no-pairs").into())`): text as written is
    /// itself; a message is its JSON, which skwir reads back as the message
    /// it is (protocol 2: [`RpcError::of`], [`Words::in_error`]).
    fn from(words: Words) -> Self {
        match words {
            Words::Written(text) => text,
            message @ Words::Message { .. } => serde_json::to_string(&message).unwrap_or_default(),
        }
    }
}

/// An argument of a message, said as its kind is in the reader's language
/// (ADR-139, 5). JSON: a number is a count, a string text, and an object
/// of one key the kind it names: `{ "name": … }`, `{ "folder": … }`,
/// `{ "size": … }`, `{ "decimal": "1.50" }`, `{ "date": … }`,
/// `{ "time": … }`, `{ "datetime": … }`; anything else is said as its JSON
/// text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    /// A count: chooses the plural (`[one]`, `*[other]`, an exact `[0]`)
    /// and is written with the reader's digits and groups ("1,234",
    /// « 1 234 »).
    Count(i64),
    /// Text as written (a path, a number that counts nothing, a word a
    /// selector chooses a variant by).
    Text(String),
    /// A file's or a place's name: quoted as the reader's language quotes,
    /// its bidirectional controls shown, never obeyed.
    Name(String),
    /// A size in bytes, written as the reader's size setting says ("1.5
    /// kB", "1.5 KiB").
    Size(u64),
    /// A folder's name where it is a destination ("to Docs"): unquoted, its
    /// bidirectional controls shown, never obeyed.
    Folder(String),
    /// A number with digits after its point, written with the reader's
    /// decimal sign and groups ("1.5", « 1,5 »): `value` scaled by ten to
    /// the power of `digits` (15 and 1 for 1.5). It chooses no plural.
    Decimal { value: i64, digits: u8 },
    /// A moment, in seconds since 1970 (UTC), written as the reader's
    /// dates and times are, in their time zone.
    Moment { at: i64, shape: Shape },
}

/// What of a moment is written ([`Arg::Moment`]), each as the reader's
/// locale writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The date: "12 Mar 2024" (`{ "date": … }`).
    Date,
    /// The time of day: "14:02" (`{ "time": … }`).
    Time,
    /// The date and the time: "12 Mar 2024, 14:02" (`{ "datetime": … }`).
    DateTime,
}

impl Shape {
    /// Its JSON key.
    fn key(self) -> &'static str {
        match self {
            Shape::Date => "date",
            Shape::Time => "time",
            Shape::DateTime => "datetime",
        }
    }
}

/// The most digits a decimal keeps after its point.
const DECIMAL_DIGITS: u8 = 9;

impl Arg {
    /// A folder's name, where it is a destination.
    pub fn folder(name: impl Into<String>) -> Self {
        Arg::Folder(name.into())
    }

    /// `number` with `digits` after its point (at most nine).
    pub fn decimal(number: f64, digits: u8) -> Self {
        let digits = digits.min(DECIMAL_DIGITS);
        let scaled = (number * 10f64.powi(i32::from(digits))).round();
        // A number past what is held is said as the most held.
        #[allow(clippy::cast_possible_truncation)]
        let value = if scaled.is_finite() { scaled as i64 } else { 0 };
        Arg::Decimal { value, digits }
    }

    /// A moment, `at` seconds since 1970 (UTC), written as `shape` says.
    pub fn moment(at: i64, shape: Shape) -> Self {
        Arg::Moment { at, shape }
    }

    /// A decimal written as JSON writes it: its digits as written,
    /// `"1.50"` two after its point. `None` for what is no decimal.
    fn decimal_of(text: &str) -> Option<Arg> {
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        if whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let fraction = &fraction[..fraction.len().min(usize::from(DECIMAL_DIGITS))];
        let digits = u8::try_from(fraction.len()).ok()?;
        let value: i64 = format!("{whole}{fraction}").parse().ok()?;
        Some(Arg::Decimal {
            value: if negative { -value } else { value },
            digits,
        })
    }

    /// A decimal written `value` scaled by `digits`, as JSON's text.
    fn decimal_text(value: i64, digits: u8) -> String {
        let sign = if value < 0 { "-" } else { "" };
        let magnitude = value.unsigned_abs().to_string();
        let digits = usize::from(digits);
        if digits == 0 {
            return format!("{sign}{magnitude}");
        }
        let padded = format!("{magnitude:0>width$}", width = digits + 1);
        let (whole, fraction) = padded.split_at(padded.len() - digits);
        format!("{sign}{whole}.{fraction}")
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

impl From<&str> for Arg {
    fn from(text: &str) -> Self {
        Arg::Text(text.to_owned())
    }
}

impl From<String> for Arg {
    fn from(text: String) -> Self {
        Arg::Text(text)
    }
}

impl Serialize for Arg {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Arg::Count(n) => serializer.serialize_i64(*n),
            Arg::Text(text) => serializer.serialize_str(text),
            Arg::Name(name) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("name", name)?;
                map.end()
            }
            Arg::Size(bytes) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("size", bytes)?;
                map.end()
            }
            Arg::Folder(name) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("folder", name)?;
                map.end()
            }
            Arg::Decimal { value, digits } => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("decimal", &Arg::decimal_text(*value, *digits))?;
                map.end()
            }
            Arg::Moment { at, shape } => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(shape.key(), at)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Arg {
    /// Never refused: what is no argument of a kind it names is said as its
    /// JSON text, so one odd argument never costs a whole answer.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Arg::of(&Value::deserialize(deserializer)?))
    }
}

impl Arg {
    /// The argument `value` says (`Arg`'s JSON).
    fn of(value: &Value) -> Arg {
        /// A whole number, one past what a count holds counted as the
        /// most it holds.
        fn count(value: &Value) -> Option<i64> {
            value.as_i64().or_else(|| value.as_u64().map(|_| i64::MAX))
        }
        let typed = |value: &Value| -> Option<Arg> {
            let Value::Object(object) = value else {
                return None;
            };
            let mut entries = object.iter();
            let (Some((kind, inner)), None) = (entries.next(), entries.next()) else {
                return None;
            };
            Some(match kind.as_str() {
                "name" => Arg::Name(inner.as_str()?.to_owned()),
                "folder" => Arg::Folder(inner.as_str()?.to_owned()),
                "size" => Arg::Size(inner.as_u64()?),
                "count" => Arg::Count(count(inner)?),
                "text" => Arg::Text(inner.as_str()?.to_owned()),
                // Its digits as written ("1.50"), or a JSON number's.
                "decimal" => match inner {
                    Value::String(text) => Arg::decimal_of(text)?,
                    Value::Number(number) => Arg::decimal_of(&number.to_string())?,
                    _ => return None,
                },
                "date" => Arg::Moment {
                    at: inner.as_i64()?,
                    shape: Shape::Date,
                },
                "time" => Arg::Moment {
                    at: inner.as_i64()?,
                    shape: Shape::Time,
                },
                "datetime" => Arg::Moment {
                    at: inner.as_i64()?,
                    shape: Shape::DateTime,
                },
                _ => return None,
            })
        };
        match value {
            Value::String(text) => Arg::Text(text.clone()),
            Value::Number(_) if count(value).is_some() => {
                Arg::Count(count(value).unwrap_or_default())
            }
            _ => typed(value).unwrap_or_else(|| Arg::Text(value.to_string())),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeParams {
    /// The protocol's version skwir speaks.
    pub api: u32,
    /// What the plugin may do, as granted.
    pub capabilities: Vec<crate::manifest::Capability>,
    /// The languages the window speaks, most wanted first, English last
    /// (BCP 47 tags: `fr`, `zh-Hans`, `en`): what a plugin writes in a
    /// language of its own is written in the first of them it can. Told a
    /// plugin on protocol 2, none on protocol 1. A message ([`Words`]) is
    /// said by skwir in each reader's language whatever these are: the
    /// window's, English for agents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeAnswer {
    /// The protocol's version the plugin speaks.
    pub api: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldParams {
    /// One of the manifest's field keys.
    pub key: String,
    pub file: File,
}

/// A field's value: a string, a number, a boolean or a list of them;
/// null when the file says nothing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FieldAnswer {
    #[serde(default)]
    pub value: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailParams {
    pub file: File,
    /// The side of the square the picture fits, in pixels.
    pub size: u32,
}

/// A thumbnail: a PNG, in base64; null when it has none for the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailAnswer {
    #[serde(default)]
    pub png: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectParams {
    pub file: File,
    /// Where the page asked starts: a byte, a row, a section, as the
    /// plugin numbers them (its `previous` and `next`).
    #[serde(default)]
    pub offset: u64,
    /// What to find, from `offset` on: the answer's `position` says where.
    #[serde(default)]
    pub query: Option<String>,
    /// Asked for its source rather than its presentation.
    #[serde(default)]
    pub source: bool,
    /// A member it listed (a row's `target`), asked for.
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvertParams {
    pub file: File,
}

/// A preview that is another file: one of a format skwir shows (a PDF, a
/// PNG, Markdown, CSV, GPX…), which skwir previews as it would that file,
/// typed by its name and bytes. Null: the plugin shows nothing for this
/// one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvertAnswer {
    /// Its name, whose extension says its type (`page.pdf`).
    pub name: String,
    /// Its bytes, in base64: at most what a message holds.
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageParams {
    pub file: File,
    /// Which page, from 1.
    pub page: u32,
    /// The longest side to draw it at, in pixels; 0 asks no picture (how
    /// many pages there are, a page's text alone).
    pub side: u32,
    /// Its text asked too, where it lies on the page.
    #[serde(default)]
    pub text: bool,
}

/// A page of a preview shown a page at a time: how many pages the file
/// has, and the one asked: its picture as a PNG, in base64 (null past the
/// last, or when no picture was asked), its own size, and its text when
/// asked. Null: the plugin shows nothing for this file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageAnswer {
    pub pages: u32,
    #[serde(default)]
    pub png: Option<String>,
    /// The page's own size, in CSS pixels (1/96 inch): what 100% shows.
    /// Its picture's, as drawn, when not said.
    #[serde(default)]
    pub width: Option<f32>,
    #[serde(default)]
    pub height: Option<f32>,
    /// Its text, in reading order: what is selected, copied and searched.
    #[serde(default)]
    pub text: Vec<TextRun>,
    /// Its links, when its text is asked: where each lies, and where it
    /// leads.
    #[serde(default)]
    pub links: Vec<PageLink>,
    /// The file's outline, the same whichever page is asked: skwir reads
    /// it once, with how many pages there are.
    #[serde(default)]
    pub outline: Vec<OutlineEntry>,
}

/// A link on a page: the box it lies in, as fractions of the page as a
/// run's is, and where it leads: a page of the file, or an address (a web
/// page, a mail address), followed only when the plugin may use the
/// network.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageLink {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// A page of the file, from 1.
    #[serde(default)]
    pub page: Option<u32>,
    /// An address.
    #[serde(default)]
    pub uri: Option<String>,
}

/// A heading of a file's outline: its title, how deep it is (0 at the
/// top), and the page it starts on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineEntry {
    pub title: String,
    #[serde(default)]
    pub depth: u32,
    #[serde(default)]
    pub page: Option<u32>,
}

/// A run of a page's text (a word, a line) and the box it lies in on the
/// page, as fractions of the page's width and height from its top left.
/// skwir puts a space between two runs on one line, a line break between
/// lines.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Where each of its characters starts, as a fraction of the page's
    /// width, one a character in order: a selection then follows each
    /// glyph. Without them, each character is an equal part of the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<f32>,
}

/// How an inspection is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionKind {
    /// Paragraphs of text.
    #[default]
    Text,
    /// Lines of code, in a monospaced font.
    Code,
    /// Rows of an offset, bytes and their characters.
    Hex,
    /// Rows and columns.
    Table,
}

/// What a preview shows of a file: text, or rows under columns, a page at
/// a time. What it says to a person (its title, its columns' headers, its
/// notice, its details' labels) is [`Words`]; what it shows of the file
/// (paragraphs, cells, values) is the file's, as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inspection {
    #[serde(default)]
    pub kind: InspectionKind,
    pub title: Words,
    /// Paragraphs, or lines of code.
    #[serde(default)]
    pub text: Vec<String>,
    #[serde(default)]
    pub columns: Vec<Words>,
    #[serde(default)]
    pub rows: Vec<Row>,
    /// Where the page before starts, if any.
    #[serde(default)]
    pub previous: Option<u64>,
    /// Where the page after starts, if any.
    #[serde(default)]
    pub next: Option<u64>,
    /// Where what was asked (found) is.
    #[serde(default)]
    pub position: u64,
    /// A line over the page: what a search found, what was left out.
    #[serde(default)]
    pub notice: Words,
    /// Facts the format knows, for Properties.
    #[serde(default)]
    pub details: Vec<Detail>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub cells: Vec<String>,
    /// Its size, for a member of a container.
    #[serde(default)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub depth: usize,
    #[serde(default)]
    pub branch: bool,
    /// What `target` asks for this row.
    #[serde(default)]
    pub target: Option<String>,
}

/// A fact the format knows: what it is (words), and its value (the
/// file's, as it is).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub label: Words,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandParams {
    /// One of the manifest's command names.
    pub command: String,
    /// Its arguments, as its schema says.
    #[serde(default)]
    pub args: Value,
    /// The files it acts on: those its caller named, else a pane's
    /// selection.
    #[serde(default)]
    pub entries: Vec<File>,
}

/// What a command answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum CommandAnswer {
    Done,
    /// A value for its caller (a script, an agent).
    Value {
        value: Value,
    },
    /// A text for the clipboard, and what to say once it is there.
    Text {
        text: String,
        said: Words,
    },
    /// A plan its caller applies or proposes: needs `write`.
    Plan {
        title: Words,
        #[serde(default)]
        note: Option<Words>,
        steps: Vec<Step>,
    },
}

/// A step of a plan, on the files of the call by handle, or at a path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Step {
    /// Give a file another name, in its folder.
    Rename { file: u32, name: String },
    /// Put a file in the trash.
    Trash { file: u32 },
    /// Move a file into the folder `to`, an absolute path.
    Move { file: u32, to: String },
    /// Copy a file into the folder `to`, an absolute path.
    Copy { file: u32, to: String },
    /// Make the folder `path`, an absolute path.
    Mkdir { path: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadParams {
    /// A handle of a file of the call being answered.
    pub file: u32,
    pub offset: u64,
    /// At most [`READ_LIMIT`].
    pub length: u32,
}

/// Bytes read, in base64: fewer than asked at the file's end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadAnswer {
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogParams {
    /// `error`, `warn`, `info` or `debug`.
    pub level: String,
    pub message: String,
}

/// A JSON-RPC 2.0 message, one line: a request (`id` and `method`), its
/// response (`id` and `result` or `error`), or a notification (`method`
/// alone).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    /// What the error says to a person as a message of the plugin's
    /// catalogs, `{ "id", "args" }` (protocol 2): `message` is then for
    /// logs (the Rust guest writes the message's id there).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// The error an answer's error `text` makes, as the protocol carries
    /// it: where `text` is the JSON of a message ([`Words::in_error`]: a
    /// module's error, `String::from(words)`), the message's id as its
    /// `message` and the message as its `data`; else `text` as its
    /// `message`.
    pub fn of(text: String) -> RpcError {
        match Words::in_error(&text) {
            Some(words @ Words::Message { .. }) => RpcError {
                code: -32000,
                message: match &words {
                    Words::Message { id, .. } => id.clone(),
                    Words::Written(text) => text.clone(),
                },
                data: serde_json::to_value(&words).ok(),
            },
            _ => RpcError {
                code: -32000,
                message: text,
                data: None,
            },
        }
    }

    /// What it says to a person (protocol 2): its `data` where that is a
    /// message, else its `message` where that is the JSON of one (what a
    /// module's helper sends of the module's error), else its `message`
    /// as written.
    pub fn words(&self) -> Words {
        self.data
            .as_ref()
            .and_then(|data| serde_json::from_value::<Words>(data.clone()).ok())
            .filter(|words| matches!(words, Words::Message { .. }))
            .or_else(|| Words::in_error(&self.message))
            .unwrap_or_else(|| Words::Written(self.message.clone()))
    }
}

impl Message {
    pub fn request(id: u64, method: &str, params: Value) -> Message {
        Message {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: Some(method.into()),
            params: Some(params),
            ..Message::default()
        }
    }

    pub fn notification(method: &str, params: Value) -> Message {
        Message {
            jsonrpc: "2.0".into(),
            method: Some(method.into()),
            params: Some(params),
            ..Message::default()
        }
    }

    pub fn answer(id: u64, answer: Result<Value, String>) -> Message {
        let (result, error) = match answer {
            Ok(value) => (Some(value), None),
            Err(message) => (
                None,
                Some(RpcError {
                    code: -32000,
                    message,
                    data: None,
                }),
            ),
        };
        Message {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result,
            error,
            ..Message::default()
        }
    }

    /// A response to a request (`id`, no `method`).
    pub fn is_response(&self) -> bool {
        self.id.is_some() && self.method.is_none()
    }

    /// The response to request `id` that is `error`.
    pub fn failed(id: u64, error: RpcError) -> Message {
        Message {
            jsonrpc: "2.0".into(),
            id: Some(id),
            error: Some(error),
            ..Message::default()
        }
    }

    /// What a response says: its result, or its error's message.
    ///
    /// # Errors
    /// The error the response carries.
    pub fn outcome(self) -> Result<Value, String> {
        self.result().map_err(|error| error.message)
    }

    /// What a response says: its result, or its error whole (its words
    /// with it, protocol 2).
    ///
    /// # Errors
    /// The error the response carries.
    pub fn result(self) -> Result<Value, RpcError> {
        match (self.result, self.error) {
            (_, Some(error)) => Err(error),
            (Some(result), None) => Ok(result),
            (None, None) => Ok(Value::Null),
        }
    }

    /// The message as one line, its newline included.
    pub fn line(&self) -> Vec<u8> {
        let mut line = serde_json::to_vec(self).unwrap_or_default();
        line.push(b'\n');
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messages_are_json_rpc_lines() {
        let request = Message::request(7, method::FIELD, json!({ "key": "a.b" }));
        let line = request.line();
        assert_eq!(line.last(), Some(&b'\n'));
        let back: Message = serde_json::from_slice(&line).unwrap();
        assert_eq!(back, request);
        assert!(!back.is_response());
        let answer = Message::answer(7, Err("no such file".into()));
        assert!(answer.is_response());
        assert_eq!(answer.outcome(), Err("no such file".into()));
        let answer: Message =
            serde_json::from_value(json!({ "jsonrpc": "2.0", "id": 7, "result": { "value": 3 } }))
                .unwrap();
        assert_eq!(answer.outcome(), Ok(json!({ "value": 3 })));
    }

    /// What a plugin says to a person is text, or a message of its
    /// catalogs with typed arguments; an odd argument is said as its JSON,
    /// never refusing the answer.
    #[test]
    fn words_are_text_or_a_message_of_the_plugin_s_catalogs() {
        let text: Words = serde_json::from_value(json!("Hex")).unwrap();
        assert_eq!(text, Words::Written("Hex".into()));
        let message: Words = serde_json::from_value(json!({
            "id": "title",
            "args": {
                "size": { "size": 9800 },
                "count": 3,
                "offset": "0x64",
                "file": { "name": "it's a café.bin" },
                "odd": 1.5,
                "flag": true,
                "huge": u64::MAX,
            },
        }))
        .unwrap();
        assert_eq!(
            message,
            Words::message("title")
                .with("size", Arg::Size(9800))
                .with("count", 3)
                .with("offset", "0x64")
                .with("file", Arg::Name("it's a café.bin".into()))
                .with("odd", Arg::Text("1.5".into()))
                .with("flag", Arg::Text("true".into()))
                .with("huge", i64::MAX)
        );
        assert_eq!(
            serde_json::to_value(
                Words::message("t")
                    .with("n", 2)
                    .with("f", Arg::Name("a".into()))
                    .with("s", Arg::Size(5))
            )
            .unwrap(),
            json!({ "id": "t", "args": { "n": 2, "f": { "name": "a" }, "s": { "size": 5 } } })
        );
        assert_eq!(
            serde_json::to_value(Words::message("t")).unwrap(),
            json!({ "id": "t" })
        );
        // Text as written takes no arguments.
        assert_eq!(Words::from("as is").with("n", 1), Words::from("as is"));
        // An object that is no message is no words.
        assert!(serde_json::from_value::<Words>(json!({ "text": "x" })).is_err());
        assert!(Words::default().is_empty());
    }

    /// A folder, a decimal and a moment travel as objects of one key; a
    /// decimal keeps its digits as written, and what no kind reads is
    /// said as its JSON.
    #[test]
    fn folders_decimals_and_moments_are_arguments_of_their_kind() {
        let args = |value: serde_json::Value| -> Arg { serde_json::from_value(value).unwrap() };
        assert_eq!(args(json!({ "folder": "Docs" })), Arg::folder("Docs"));
        assert_eq!(
            args(json!({ "decimal": "1.50" })),
            Arg::Decimal {
                value: 150,
                digits: 2
            }
        );
        assert_eq!(
            args(json!({ "decimal": 1.5 })),
            Arg::Decimal {
                value: 15,
                digits: 1
            }
        );
        assert_eq!(
            args(json!({ "decimal": "-0.25" })),
            Arg::Decimal {
                value: -25,
                digits: 2
            }
        );
        assert_eq!(
            args(json!({ "decimal": "12" })),
            Arg::Decimal {
                value: 12,
                digits: 0
            }
        );
        assert_eq!(
            args(json!({ "date": 1_700_000_000 })),
            Arg::moment(1_700_000_000, Shape::Date)
        );
        assert_eq!(args(json!({ "time": 60 })), Arg::moment(60, Shape::Time));
        assert_eq!(
            args(json!({ "datetime": -1 })),
            Arg::moment(-1, Shape::DateTime)
        );
        // What no kind reads is its JSON text, never refused.
        assert_eq!(
            args(json!({ "decimal": "1,5" })),
            Arg::Text(r#"{"decimal":"1,5"}"#.into())
        );
        assert_eq!(
            args(json!({ "date": "today" })),
            Arg::Text(r#"{"date":"today"}"#.into())
        );
        // Written back as read.
        for value in [
            json!({ "folder": "Docs" }),
            json!({ "decimal": "1.50" }),
            json!({ "decimal": "-0.05" }),
            json!({ "decimal": "7" }),
            json!({ "date": 1_700_000_000 }),
            json!({ "datetime": 5 }),
        ] {
            assert_eq!(serde_json::to_value(args(value.clone())).unwrap(), value);
        }
        assert_eq!(
            Arg::decimal(1.005, 2),
            Arg::Decimal {
                value: 100,
                digits: 2
            }
        );
        assert_eq!(
            Arg::decimal(2.5, 20),
            Arg::Decimal {
                value: 2_500_000_000,
                digits: 9
            }
        );
    }

    /// An error's words travel as its `data` from a program, as its
    /// message's JSON from a module (whose error is a string); text that
    /// is no message stays as written.
    #[test]
    fn an_error_says_its_words_as_a_message() {
        let failed = String::from(Words::message("no-pairs").with("max", 4096));
        let error = RpcError::of(failed);
        assert_eq!(error.message, "no-pairs");
        assert_eq!(error.words(), Words::message("no-pairs").with("max", 4096));
        let helper = RpcError {
            code: -32000,
            message: r#"{"id":"no-pairs"}"#.into(),
            data: None,
        };
        assert_eq!(helper.words(), Words::message("no-pairs"));
        let plain = RpcError::of("no such file".into());
        assert_eq!(plain.data, None);
        assert_eq!(plain.words(), Words::from("no such file"));
        assert_eq!(
            RpcError::of("{ not json".into()).words(),
            Words::from("{ not json")
        );
        assert_eq!(
            RpcError::of(r#"{"said":"x"}"#.into()).words(),
            Words::from(r#"{"said":"x"}"#)
        );
        assert_eq!(String::from(Words::from("as written")), "as written");
        let line = Message::failed(3, RpcError::of(String::from(Words::message("gone")))).line();
        let back: Message = serde_json::from_slice(&line).unwrap();
        assert_eq!(back.result().unwrap_err().words(), Words::message("gone"));
        // Read as protocol 1 reads it: the message alone.
        let back: Message = serde_json::from_slice(&line).unwrap();
        assert_eq!(back.outcome(), Err("gone".into()));
    }

    #[test]
    fn a_command_answers_a_value_a_text_or_a_plan_of_steps() {
        let plan: CommandAnswer = serde_json::from_value(json!({
            "answer": "plan",
            "title": "Rename",
            "steps": [
                { "op": "rename", "file": 1, "name": "b.txt" },
                { "op": "move", "file": 2, "to": "/x" },
            ],
        }))
        .unwrap();
        let CommandAnswer::Plan { steps, .. } = plan else {
            panic!("a plan")
        };
        assert_eq!(
            steps[0],
            Step::Rename {
                file: 1,
                name: "b.txt".into()
            }
        );
        assert_eq!(
            serde_json::to_value(CommandAnswer::Done).unwrap(),
            json!({ "answer": "done" })
        );
    }
}
