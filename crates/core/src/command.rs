//! Commands are data: a name plus JSON arguments, validated against a schema,
//! dispatched to a typed handler. The keymap, the palette, the CLI, the MCP
//! server, and any agent all speak this one surface.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::op::Plan;
use crate::words::Words;

/// A command invocation as it travels: over a keybinding, a palette entry, a
/// socket, or an agent's tool call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Invocation {
    pub name: Arc<str>,
    #[serde(default = "empty_args")]
    pub args: serde_json::Value,
}

fn empty_args() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

impl Invocation {
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Invocation {
            name: name.into(),
            args: empty_args(),
        }
    }

    /// An invocation with typed arguments. Arguments that cannot be
    /// represented in JSON (a path that is not UTF-8 in a plain `PathBuf`
    /// field: use [`crate::os_path::PathArg`]) are logged and replaced by
    /// `null`, which every command refuses at dispatch: the mistake surfaces
    /// as an error, never as a crash or as a command run with defaults.
    pub fn with_args<A: Serialize>(name: impl Into<Arc<str>>, args: &A) -> Self {
        let name = name.into();
        let args = serde_json::to_value(args).unwrap_or_else(|error| {
            tracing::error!(command = %name, "arguments cannot be serialized: {error}");
            serde_json::Value::Null
        });
        Invocation { name, args }
    }

    /// This invocation with `key` set to `value` among its arguments (made
    /// an object if they were not one): how a surface adds what it knows,
    /// the pane it acts on first of all.
    #[must_use]
    pub fn with(mut self, key: &str, value: impl Serialize) -> Self {
        if !self.args.is_object() {
            self.args = empty_args();
        }
        self.args[key] = serde_json::to_value(value).unwrap_or_else(|error| {
            tracing::error!(command = %self.name, key, "an argument cannot be serialized: {error}");
            serde_json::Value::Null
        });
        self
    }

    /// `navigate` to `location`.
    pub fn navigate(location: &crate::location::Location) -> Self {
        Invocation::new("navigate").with("location", location.arg())
    }

    /// Like [`Invocation::with_args`], reporting a serialization failure.
    pub fn try_with_args<A: Serialize>(
        name: impl Into<Arc<str>>,
        args: &A,
    ) -> serde_json::Result<Self> {
        Ok(Invocation {
            name: name.into(),
            args: serde_json::to_value(args)?,
        })
    }
}

/// The error of a command that acts on the selection when nothing is
/// selected. Typed, so a surface can answer it as the hint it is ("select
/// something first") rather than as a failure, whatever context wraps it:
/// `anyhow::Error::from(NothingSelected)`, test with [`is_nothing_selected`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NothingSelected;

impl std::fmt::Display for NothingSelected {
    /// Its words: English, or the reader's language while an error's chain
    /// is said.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(
            &crate::words!(
                command_nothing_selected = "nothing selected",
                note = "Why a command that acts on the selection did nothing. Begins in lower case: said after other words too."
            ),
            f,
        )
    }
}

impl std::error::Error for NothingSelected {}

/// Whether `error`, or anything it wraps, is [`NothingSelected`].
pub fn is_nothing_selected(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(<dyn std::error::Error + 'static>::is::<NothingSelected>)
}

/// The error of a command that takes what the clipboard holds when Skwir's
/// own holds nothing (ADR-062): a window reads the system's clipboard and
/// runs the command again with what it holds as [`ClipboardArg`]; an agent
/// or a script is told there is nothing to paste. Test with
/// [`wants_clipboard`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WantsClipboard;

impl std::fmt::Display for WantsClipboard {
    /// Its words, as [`NothingSelected`]'s.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(
            &crate::words!(
                command_nothing_to_paste = "nothing to paste",
                note = "Why Paste did nothing: neither Skwir nor the system's clipboard holds files. Begins in lower case."
            ),
            f,
        )
    }
}

impl std::error::Error for WantsClipboard {}

/// Whether `error`, or anything it wraps, is [`WantsClipboard`].
pub fn wants_clipboard(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(<dyn std::error::Error + 'static>::is::<WantsClipboard>)
}

/// What the system's clipboard holds, as the window read it for a command
/// that answered [`WantsClipboard`], under [`ClipboardArg::FIELD`]: files
/// another application copied or cut, each read from the disk on a worker.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct ClipboardArg {
    /// The files, by absolute path.
    #[serde(default)]
    pub entries: Vec<crate::op::EntryArg>,
    /// They were cut: pasting moves them.
    #[serde(default)]
    pub cut: bool,
}

impl ClipboardArg {
    /// The argument a command that wants the clipboard takes it as.
    pub const FIELD: &'static str = "clipboard";
}

/// Arguments of a command that takes none.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct NoArgs {}

/// What a command produced. Commands that would mutate the filesystem return
/// a plan; the caller decides whether to apply it.
#[derive(Debug)]
pub enum Outcome {
    Done,
    Plan(Plan),
    Value(serde_json::Value),
    /// A text for the clipboard, and what to say once it is there ("Copied
    /// 3 paths"), said where it is shown: the window puts it there; a
    /// script or an agent gets the text.
    Clipboard {
        text: String,
        said: crate::words::Words,
    },
    /// Files copied or cut (`cut`), for the clipboard: the window puts them
    /// on the system's as file managers do (ADR-062); a script or an agent
    /// gets the paths.
    Files {
        paths: Vec<std::path::PathBuf>,
        cut: bool,
    },
}

/// What a command does, which decides what it may do when an agent runs
/// it (ADR-044, `permissions::for_agent`). Declared at registration
/// (`register(..).reads()`); a command that says nothing acts, so an
/// agent's call asks the user first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Answers about files and the window; changes nothing.
    Reads,
    /// Changes only what the window shows: where a pane is, how it shows
    /// it, what is selected, its tabs and panels, a prompt for the user.
    Shows,
    /// Changes files only through a plan, which its caller applies or
    /// proposes for review.
    Plans,
    /// Anything else: changes a preference, the sidebar, the clipboard,
    /// starts a program, applies without a plan. An agent asks first.
    #[default]
    Acts,
    /// Answers what the window asked the user, or presses what the
    /// user's hand presses (a review's Apply, a menu's item): never run
    /// for an agent.
    UserOnly,
}

/// What a command is about, as a person looks for it: F1 lists the keys
/// that work here under these, in this order (`Topic::ALL`). Every command
/// names one where it is registered (`CommandRegistry::register`), the
/// plugins' too; a plugin from outside that names none takes its
/// category's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// Locations, history, the address, places to jump to, opening.
    Go,
    /// The cursor and the selection.
    Select,
    /// Doing something to files: the clipboard, renaming, copying,
    /// trashing, making, compressing, the applications that open them.
    Files,
    /// Searching, filtering, narrowing, finding a name.
    Find,
    /// Tags, ratings, comments, a picture's position.
    Notes,
    /// Windows, tabs and the split.
    Tabs,
    /// How the files are shown and which panels are: views, columns,
    /// zoom, sort, the sidebar, the preview panel, the timeline.
    View,
    /// Inside a preview: pages, reading, finding text, a model, a map.
    Preview,
    /// The player.
    Player,
    /// The terminal and commands run in a shell.
    Terminal,
    /// The sidebar's places, volumes, bookmarks and servers.
    Places,
    /// Moving about the window: focus, buttons, menus, dialogs.
    Window,
    /// Skwir itself: the palette, settings, plugins, help, quitting.
    Skwir,
}

impl Topic {
    /// Whether a window answering another program's Open or Save dialog
    /// offers its commands (ADR-144, 1), unless a command says otherwise
    /// (`Declared::in_dialogs`, `not_in_dialogs`, `in_dialogs_by`) or
    /// changes files or opens another program
    /// (`CommandSpec::in_dialogs_by_default`): what finds and chooses a
    /// file (going places, selecting, finding, notes, views, previews and
    /// what plays in them, places, the window's own parts), never what
    /// does something else (files, tabs and the split, the terminal, Skwir
    /// itself).
    pub fn in_dialogs(self) -> bool {
        match self {
            Topic::Go
            | Topic::Select
            | Topic::Find
            | Topic::Notes
            | Topic::View
            | Topic::Preview
            | Topic::Player
            | Topic::Places
            | Topic::Window => true,
            Topic::Files | Topic::Tabs | Topic::Terminal | Topic::Skwir => false,
        }
    }

    /// In the order F1 lists them.
    pub const ALL: [Topic; 13] = [
        Topic::Go,
        Topic::Select,
        Topic::Files,
        Topic::Find,
        Topic::Notes,
        Topic::Tabs,
        Topic::View,
        Topic::Preview,
        Topic::Player,
        Topic::Terminal,
        Topic::Places,
        Topic::Window,
        Topic::Skwir,
    ];

    /// Its heading, as F1's list of keys and the keyboard page head the
    /// commands about it.
    pub fn title(self) -> Words {
        use crate::words;
        match self {
            Topic::Go => words!(
                topic_go = "Going places",
                note = "A heading of the list of keys: going to folders and places, history, the address."
            ),
            Topic::Select => words!(
                topic_select = "Selecting",
                note = "A heading of the list of keys: the cursor and the selection."
            ),
            Topic::Files => words!(
                topic_files = "Files",
                note = "A heading of the list of keys: doing something to files (copying, renaming, trashing)."
            ),
            Topic::Find => words!(
                topic_find = "Finding",
                note = "A heading of the list of keys: searching and filtering."
            ),
            Topic::Notes => words!(
                topic_notes = "Tags and notes",
                note = "A heading of the list of keys: tags, ratings and comments."
            ),
            Topic::Tabs => words!(
                topic_tabs = "Tabs and panes",
                note = "A heading of the list of keys: windows, tabs and the split."
            ),
            Topic::View => words!(
                topic_view = "Views and panels",
                note = "A heading of the list of keys: how files are shown, and which panels are."
            ),
            Topic::Preview => words!(
                topic_preview = "Previews",
                note = "A heading of the list of keys: inside a file's preview."
            ),
            Topic::Player => words!(
                topic_player = "Playing",
                note = "A heading of the list of keys: the audio and video player."
            ),
            Topic::Terminal => words!(
                topic_terminal = "Terminal",
                note = "A heading of the list of keys: the terminal panel."
            ),
            Topic::Places => words!(
                topic_places = "Places and servers",
                note = "A heading of the list of keys: the sidebar's places, drives and servers."
            ),
            Topic::Window => words!(
                topic_window = "The window",
                note = "A heading of the list of keys: moving about the window, its menus and dialogs."
            ),
            Topic::Skwir => words!(
                topic_skwir = "Skwir",
                note = "A heading of the list of keys: the program itself (settings, plugins, help, quitting). Skwir is its name."
            ),
        }
    }
}

/// The JSON schema of a command's arguments, made the first time it is
/// asked for (an agent's list of tools, the palette's required
/// arguments): made for every command as it was declared, it held up the
/// first frame and kept a tree of each.
#[derive(Clone)]
pub struct ArgsSchema(Arc<ArgsSchemaInner>);

struct ArgsSchemaInner {
    made: std::sync::OnceLock<schemars::Schema>,
    make: fn() -> schemars::Schema,
}

impl ArgsSchema {
    /// The schema of `A`, made when asked for.
    fn of<A: JsonSchema>() -> Self {
        fn make<A: JsonSchema>() -> schemars::Schema {
            schemars::schema_for!(A)
        }
        ArgsSchema(Arc::new(ArgsSchemaInner {
            made: std::sync::OnceLock::new(),
            make: make::<A>,
        }))
    }

    /// A schema given whole.
    fn given(schema: schemars::Schema) -> Self {
        fn none() -> schemars::Schema {
            schemars::json_schema!({ "type": "object" })
        }
        ArgsSchema(Arc::new(ArgsSchemaInner {
            made: std::sync::OnceLock::from(schema),
            make: none,
        }))
    }

    pub fn get(&self) -> &schemars::Schema {
        self.0.made.get_or_init(self.0.make)
    }
}

/// The names of the arguments a typed command takes, read from its schema
/// the first time it is called: serde leaves out a name its type does not
/// have, so a misspelled `entries` would be dropped and the command would
/// act on the selection. Refused instead, as `register_dynamic` refuses
/// one its schema does not hold.
struct Takes {
    schema: ArgsSchema,
    names: std::sync::OnceLock<Option<std::collections::BTreeSet<String>>>,
}

impl Takes {
    fn new(schema: ArgsSchema) -> Arc<Self> {
        Arc::new(Takes {
            schema,
            names: std::sync::OnceLock::new(),
        })
    }

    /// Whether every argument `args` names is one the command takes;
    /// arguments that are not an object are left to serde to refuse.
    fn check(&self, args: &serde_json::Value) -> Result<(), serde_json::Error> {
        use serde::de::Error as _;
        let Some(object) = args.as_object() else {
            return Ok(());
        };
        let names = self
            .names
            .get_or_init(|| property_names(self.schema.get().as_value()));
        match names {
            Some(names) => match object.keys().find(|key| !names.contains(*key)) {
                Some(unknown) => Err(serde_json::Error::custom(format!(
                    "unknown field `{unknown}`{}",
                    expected(names)
                ))),
                None => Ok(()),
            },
            None => Ok(()),
        }
    }
}

/// ", expected `a` or `b`", as serde says it.
fn expected(names: &std::collections::BTreeSet<String>) -> String {
    let names: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    match names.as_slice() {
        [] => ", there are none".to_owned(),
        [one] => format!(", expected {one}"),
        [rest @ .., last] => format!(", expected one of {}, {last}", rest.join(", ")),
    }
}

/// The names an object `root` describes may hold, or `None` when it may
/// hold any (a map, a `serde_json::Value`, a schema this does not read).
/// Flattened fields arrive as `allOf`, enums as `oneOf`/`anyOf`, each
/// variant's names taken together; `$ref`s are followed into `$defs`.
fn property_names(root: &serde_json::Value) -> Option<std::collections::BTreeSet<String>> {
    use serde_json::Value;
    fn walk(
        root: &Value,
        schema: &Value,
        names: &mut std::collections::BTreeSet<String>,
        depth: u8,
    ) -> Option<()> {
        // A cycle of `$ref`s, or nesting no argument type has: take any.
        if depth > 16 {
            return None;
        }
        let object = schema.as_object()?;
        if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
            let target = reference
                .strip_prefix("#/$defs/")
                .and_then(|name| root.get("$defs")?.get(name))
                .or_else(|| {
                    let name = reference.strip_prefix("#/definitions/")?;
                    root.get("definitions")?.get(name)
                })?;
            walk(root, target, names, depth + 1)?;
        }
        match object.get("additionalProperties") {
            None | Some(Value::Bool(false)) => {}
            Some(_) => return None,
        }
        if let Some(properties) = object.get("properties") {
            names.extend(properties.as_object()?.keys().cloned());
        }
        if let Some(all) = object.get("allOf") {
            for member in all.as_array()? {
                walk(root, member, names, depth + 1)?;
            }
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(choices) = object.get(key) {
                for choice in choices.as_array()? {
                    // A choice that is no object (`null` of an `Option`, a
                    // variant named by a string) names nothing.
                    if may_be_object(choice) {
                        walk(root, choice, names, depth + 1)?;
                    }
                }
            }
        }
        let described = ["$ref", "properties", "allOf", "oneOf", "anyOf"]
            .iter()
            .any(|key| object.contains_key(*key));
        // An object said to be one, with nothing said of what it holds,
        // holds nothing (`NoArgs`); anything else is not read here.
        (described || object.get("type") == Some(&Value::from("object"))).then_some(())
    }
    fn may_be_object(schema: &Value) -> bool {
        match schema.get("type") {
            Some(Value::String(t)) => t == "object",
            Some(Value::Array(types)) => types.iter().any(|t| t == "object"),
            Some(_) => false,
            None => {
                schema.get("const").is_none_or(Value::is_object)
                    && schema
                        .get("enum")
                        .and_then(Value::as_array)
                        .is_none_or(|values| values.iter().any(Value::is_object))
            }
        }
    }
    let mut names = std::collections::BTreeSet::new();
    walk(root, root, &mut names, 0).map(|()| names)
}

impl std::ops::Deref for ArgsSchema {
    type Target = schemars::Schema;
    fn deref(&self) -> &schemars::Schema {
        self.get()
    }
}

impl std::fmt::Debug for ArgsSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
    }
}

impl Serialize for ArgsSchema {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.get().serialize(serializer)
    }
}

/// Arguments that make a command one thing to do, with that thing's
/// title: `sort {"key": "size"}` is Sort by size (ADR-024).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Form {
    pub args: serde_json::Value,
    /// What it is called, said where it is shown; in English to agents
    /// and scripts (ADR-139, 7).
    pub title: Words,
    /// What it shares with the forms of its command that only go
    /// elsewhere: Extend the selection up, down, to the first item are
    /// "Extend the selection". F1 lists a family's keys on one row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<Family>,
}

/// The family of forms a [`Form`] is one of: which, as data (what its
/// forms meet by, never their words, ADR-139, 8), and what its row says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Family {
    /// Unique among its command's families (`cursor.extend`).
    pub id: String,
    /// "Extend the selection", said where it is drawn.
    pub title: Words,
}

impl Form {
    pub fn new(args: serde_json::Value, title: impl Into<Words>) -> Self {
        Form {
            args,
            title: title.into(),
            family: None,
        }
    }

    /// One of the family `id`, its row saying `title` (see
    /// [`Form::family`]).
    #[must_use]
    pub fn in_family(mut self, id: impl Into<String>, title: impl Into<Words>) -> Self {
        self.family = Some(Family {
            id: id.into(),
            title: title.into(),
        });
        self
    }
}

/// `title` with the arguments said after it, as a command is called when
/// no form names them: `Sort (key name)`. The arguments are as written
/// (their names and values, as a script writes them), never translated.
pub fn with_args(title: impl Into<Words>, args: &serde_json::Value) -> Words {
    let title = title.into();
    match args.as_object().filter(|args| !args.is_empty()) {
        None => title,
        Some(args) => {
            let written: Vec<String> = args
                .iter()
                .map(|(name, value)| match value.as_str() {
                    Some(text) => format!("{name} {text}"),
                    None => format!("{name} {value}"),
                })
                .collect();
            crate::words!(
                command_with_args = "{ $title } ({ $args })",
                title = title,
                args = written.join(", "),
                note = "A command named with the arguments it runs with, where none of its forms names them (Sort (key name)). args: the arguments as a script writes them, never translated."
            )
        }
    }
}

/// When a key held down runs a command again, by the arguments it runs
/// with (`CommandSpec::repeats`).
pub type Repeats = fn(&serde_json::Value) -> bool;

/// What a command says of a window answering another program's Open or
/// Save dialog where it is registered (ADR-144, 1), over what its topic
/// and effect say (`CommandSpec::in_dialogs_by_default`).
#[derive(Clone, Copy, Debug)]
pub enum DialogRule {
    /// Offered there whatever its topic or effect (`Declared::in_dialogs`):
    /// what a dialog has always offered among the files' commands (making
    /// a folder, renaming, the Trash, the clipboard), the notes a file is
    /// found by, and `open`, which answers the dialog there.
    Offered,
    /// Not offered there whatever its topic (`Declared::not_in_dialogs`):
    /// what is about a window of the user's own (Activity, the pinned
    /// player), a lens's own, what changes the user's servers.
    Not,
    /// As its arguments make it there (`Declared::in_dialogs_by`):
    /// `ui.layout` placing the preview is offered, the split's panes are
    /// not; `ui.view` a listing's shape, never a lens; the sidebar's
    /// Ctrl+Enter on a place runs as the middle click does there, opening
    /// it in place, listed nowhere, being Enter's (`InDialogs::Asked`).
    By(fn(&App, &serde_json::Value) -> InDialogs),
}

/// Where what a command opens is shown (`CommandSpec::opens`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Opens {
    /// In Skwir's own window, or nothing to open.
    #[default]
    Here,
    /// In another program (`Declared::opens_elsewhere`): the desktop's
    /// application for a file, the browser, the desktop's maps. A window
    /// answering another program's dialog offers none unless it says so
    /// (`open`, which answers the dialog there).
    Elsewhere,
}

/// The commands a command run with these arguments runs in turn
/// (`Declared::leads_to`): a picker's source and choices, a prompt's
/// command, what an item of the preview's menu runs with what it shows. A
/// window answering another program's dialog offers it only where it
/// offers each of them (`App::in_dialogs`): a picker of applications to
/// open a file with is about files, whoever draws it.
pub type LeadsTo = fn(&serde_json::Value) -> Vec<Invocation>;

/// How a window answering another program's Open or Save dialog takes a
/// command run with given arguments (`App::in_dialogs`, ADR-144, 1): the
/// one answer every way to run a command there asks (keys and F1's list,
/// the palette, menus, buttons and the address field's rows, pickers and
/// prompts, Properties' links, a toast's buttons). Every command is
/// offered in a window of the user's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InDialogs {
    /// Offered: listed, drawn, run.
    Offered,
    /// Run where a part of the window asks it with what it needs, and
    /// listed nowhere (`CommandSpec::data`, `asked_only`): the data a
    /// row reads, the save of a setting a menu's switch changes.
    Asked,
    /// Not offered: drawn and listed nowhere, refused when asked, and a
    /// key bound to it does what it would with nothing bound.
    Not,
}

impl InDialogs {
    /// Whether it is listed and drawn there.
    pub fn offered(self) -> bool {
        self == InDialogs::Offered
    }

    /// Whether it runs there when asked.
    pub fn runs(self) -> bool {
        self != InDialogs::Not
    }
}

/// What `App::in_dialogs` reads of a command, copied out of the registry
/// so that a rule reading the app's other registries runs without its
/// lock.
#[derive(Clone, Copy)]
pub(crate) struct DialogTerms {
    pub(crate) said: Option<DialogRule>,
    pub(crate) by_default: bool,
    /// Data or asked only: run where asked, listed nowhere.
    pub(crate) asked: bool,
    pub(crate) leads_to: Option<LeadsTo>,
}

/// Again with every repeat of its key: a command that only steps (the
/// cursor, a page, the next tab).
pub fn always(_: &serde_json::Value) -> bool {
    true
}

/// Declared metadata of a command, including the JSON schema of its arguments.
#[derive(Clone, Debug, Serialize)]
pub struct CommandSpec {
    pub name: Arc<str>,
    /// What every list of actions calls it, said where it is drawn; in
    /// English to agents and scripts (ADR-139, 7).
    pub title: Words,
    pub description: Words,
    pub args_schema: ArgsSchema,
    /// Plugin id, or `core`.
    pub owner: Arc<str>,
    /// What it is about (see [`Topic`]).
    pub topic: Topic,
    /// What it does: what an agent may run without asking.
    pub effect: Effect,
    /// The arguments that make it one thing to do, each with its title:
    /// what every list of actions calls it (`CommandSpec::title_for`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forms: Vec<Form>,
    /// Its keys may change, never all go: what gets the keyboard out of a
    /// dialog, or to every command (ADR-024).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub keeps_a_key: bool,
    /// It only answers whoever asks: an agent or a script, or a part of
    /// the window that draws the answer (a menu's items, Properties' rows,
    /// the timeline's bars). Run by a person from a list of actions it
    /// would show nothing, so the palette and the keyboard's lists leave
    /// it out (`Declared::data`). A command that changes nothing yet shows
    /// something run from the window (copying paths, a plugin from
    /// outside's answer) is `Effect::Reads` without being data.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub data: bool,
    /// The command it is a twin of, which does the same thing for a person
    /// (the core's `window.close` and the window's `ui.close_window`): a
    /// list of actions shows one of them (`Declared::same_as`). Told by
    /// this, never by their titles (`docs/invariants/words.md`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_as: Option<Arc<str>>,
    /// It is run only where it is asked for, with the arguments that place
    /// makes (`Declared::asked_only`): a question's buttons
    /// (`updates.answer`), the Settings window's save (`settings.apply`),
    /// its Plugins page's switch (`plugins.set_enabled`). No list of
    /// actions offers it (the palette, the Keyboard page's), which would
    /// run it without them. Only those lists read it, so it is not said to
    /// agents.
    #[serde(skip)]
    pub asked_only: bool,
    /// Whether a window answering another program's dialog offers it,
    /// said where it is registered (`Declared::in_dialogs`,
    /// `not_in_dialogs`, `in_dialogs_by`); `None`, as its topic and
    /// effect say (`CommandSpec::in_dialogs_by_default`, ADR-144). Only
    /// the window reads it, so it is not said to agents.
    #[serde(skip)]
    pub in_dialogs: Option<DialogRule>,
    /// What it runs in turn with the arguments it is given
    /// (`Declared::leads_to`, see [`LeadsTo`]).
    #[serde(skip)]
    pub leads_to: Option<LeadsTo>,
    /// Where what it opens is shown (see [`Opens`]).
    #[serde(skip)]
    pub opens: Opens,
    /// When a key held down runs it again (ADR-066): a step along
    /// something (the cursor, a page, the volume, the next tab), never an
    /// act or a switch, which the key's repeats would run over and over.
    /// `None`, the default: once a press. Only keys repeat, so it is not
    /// said to agents.
    #[serde(skip)]
    pub repeats: Option<Repeats>,
}

impl CommandSpec {
    /// Whether a window answering another program's dialog offers it by
    /// what its topic and effect say, where it says nothing of dialogs
    /// (ADR-144, 1): by its topic (`Topic::in_dialogs`), but never what
    /// changes files (`Effect::Plans`) nor what opens another program
    /// (`Opens::Elsewhere`), whatever it is about, which a dialog offers
    /// only where they say so (`Declared::in_dialogs`).
    pub fn in_dialogs_by_default(&self) -> bool {
        self.topic.in_dialogs() && self.effect != Effect::Plans && self.opens == Opens::Here
    }

    /// Whether a key held down runs it again with `args`.
    pub fn repeats_with(&self, args: &serde_json::Value) -> bool {
        self.repeats.is_some_and(|rule| rule(args))
    }

    /// Whether it takes the argument `name`, as its schema declares.
    pub fn takes(&self, name: &str) -> bool {
        self.args_schema
            .as_object()
            .and_then(|o| o.get("properties"))
            .and_then(|p| p.as_object())
            .is_some_and(|p| p.contains_key(name))
    }

    /// Whether its arguments may name `name`, as the registry checks them
    /// (`Takes`): its schema declares it, in a flattened or referred part
    /// too, or takes any (a `serde_json::Value`). What a caller adds on its
    /// own (the pane a button stands in) goes only where it is accepted.
    pub fn accepts(&self, name: &str) -> bool {
        property_names(self.args_schema.get().as_value()).is_none_or(|names| names.contains(name))
    }

    /// The form of these arguments: `None` for none declared.
    pub fn form(&self, args: &serde_json::Value) -> Option<&Form> {
        let none = serde_json::Value::Object(serde_json::Map::new());
        let args = if args.is_null() { &none } else { args };
        self.forms.iter().find(|form| form.args == *args)
    }

    /// What it is called run with `args`, said where it is shown: their
    /// form's title, else its own, with the arguments no form names said
    /// after it, so two ways to run it never read alike.
    pub fn title_for(&self, args: &serde_json::Value) -> Words {
        match self.form(args) {
            Some(form) => form.title.clone(),
            None => with_args(self.title.clone(), args),
        }
    }

    /// Names of the required arguments, from the schema.
    pub fn required_args(&self) -> Vec<String> {
        self.args_schema
            .as_object()
            .and_then(|o| o.get("required"))
            .and_then(|r| r.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A command's handler as JSON reaches it: read in place, copied only for
/// a handler that keeps its arguments as they came (`register_raw`,
/// `register_dynamic`).
type Handler = Arc<dyn Fn(&serde_json::Value, &App) -> anyhow::Result<Outcome> + Send + Sync>;
/// A typed command's handler, handed its arguments as the caller made them
/// (`CommandRegistry::dispatch_args`), of the type it registered.
type TypedHandler =
    Arc<dyn Fn(Box<dyn std::any::Any + Send>, &App) -> anyhow::Result<Outcome> + Send + Sync>;
/// Whether arguments are a command's, without running it.
type Validator = Arc<dyn Fn(&serde_json::Value) -> Result<(), serde_json::Error> + Send + Sync>;

/// What runs a command: from JSON, and for a typed command from its
/// arguments' own type, which no JSON is made for.
#[derive(Clone)]
struct Handlers {
    json: Handler,
    typed: Option<(std::any::TypeId, TypedHandler)>,
}

/// A command just registered, to say what it does
/// (`register(..).reads()`); left alone, it acts.
pub struct Declared<'a> {
    registry: &'a CommandRegistry,
    name: Arc<str>,
    owner: Arc<str>,
}

impl Declared<'_> {
    fn update(&self, change: impl FnOnce(&mut CommandSpec)) {
        let mut commands = self.registry.commands.write();
        if let Some((spec, _, _)) = commands
            .get_mut(&self.name)
            .and_then(|stack| stack.iter_mut().find(|(s, _, _)| s.owner == self.owner))
        {
            change(spec);
        }
    }

    /// The arguments that make it one thing to do, each with its title
    /// (see [`Form`]).
    pub fn forms(&self, forms: impl IntoIterator<Item = Form>) {
        self.update(|spec| spec.forms.extend(forms));
    }

    /// Its keys may change, never all go (see `CommandSpec::keeps_a_key`).
    pub fn keeps_a_key(&self) {
        self.update(|spec| spec.keeps_a_key = true);
    }

    /// It does what `twin` does, for a person (see
    /// `CommandSpec::same_as`): a list of actions shows one of them.
    pub fn same_as(&self, twin: &str) {
        self.update(|spec| spec.same_as = Some(twin.into()));
    }

    /// It is run only where it is asked for (see `CommandSpec::asked_only`):
    /// no list of actions offers it.
    pub fn asked_only(&self) {
        self.update(|spec| spec.asked_only = true);
    }

    /// A window answering another program's dialog offers it, whatever
    /// its topic and effect say (see [`DialogRule::Offered`]).
    // Said as it is declared: what it returns only goes on to say more
    // (`.in_dialogs().plans()`), and may be dropped.
    #[allow(clippy::return_self_not_must_use)]
    pub fn in_dialogs(self) -> Self {
        self.update(|spec| spec.in_dialogs = Some(DialogRule::Offered));
        self
    }

    /// A window answering another program's dialog does not offer it,
    /// whatever its topic says (see [`DialogRule::Not`]).
    // Said as it is declared: what it returns only goes on to say more
    // (`.in_dialogs().plans()`), and may be dropped.
    #[allow(clippy::return_self_not_must_use)]
    pub fn not_in_dialogs(self) -> Self {
        self.update(|spec| spec.in_dialogs = Some(DialogRule::Not));
        self
    }

    /// A window answering another program's dialog offers it where `rule`
    /// says, by the arguments it runs with (see [`DialogRule::By`]).
    pub fn in_dialogs_by(&self, rule: fn(&App, &serde_json::Value) -> InDialogs) {
        self.update(|spec| spec.in_dialogs = Some(DialogRule::By(rule)));
    }

    /// What it runs in turn with its arguments (see [`LeadsTo`]).
    pub fn leads_to(&self, leads_to: LeadsTo) {
        self.update(|spec| spec.leads_to = Some(leads_to));
    }

    /// It opens something in another program (see [`Opens::Elsewhere`]).
    // Said as it is declared: what it returns only goes on to say more
    // (`.opens_elsewhere().reads()`), and may be dropped.
    #[allow(clippy::return_self_not_must_use)]
    pub fn opens_elsewhere(self) -> Self {
        self.update(|spec| spec.opens = Opens::Elsewhere);
        self
    }

    /// A key held down runs it again where `rule` says, by its arguments
    /// (see `CommandSpec::repeats`); [`always`] for a command that only
    /// steps.
    pub fn repeats(&self, rule: Repeats) {
        self.update(|spec| spec.repeats = Some(rule));
    }

    /// Say what it does.
    pub fn effect(self, effect: Effect) {
        self.update(|spec| spec.effect = effect);
    }

    /// It answers, and changes nothing.
    pub fn reads(self) {
        self.effect(Effect::Reads);
    }

    /// It answers whoever asks and nothing else: it changes nothing, and
    /// run from the window it would show nothing (see
    /// `CommandSpec::data`).
    pub fn data(self) {
        self.update(|spec| {
            spec.effect = Effect::Reads;
            spec.data = true;
        });
    }

    /// It changes only what the window shows.
    pub fn shows(self) {
        self.effect(Effect::Shows);
    }

    /// It changes files only through a plan.
    pub fn plans(self) {
        self.effect(Effect::Plans);
    }

    /// It answers the user's own question or presses for the user's hand.
    pub fn user_only(self) {
        self.effect(Effect::UserOnly);
    }
}

/// Errors from dispatch.
#[derive(Debug)]
pub enum CommandError {
    Unknown(Arc<str>),
    /// The reason is in the message, not a source: a chain printed whole
    /// (`{:#}`) would say it twice.
    InvalidArgs {
        name: Arc<str>,
        reason: serde_json::Error,
    },
    /// Its handler panicked: a fault of skwir's or of a plugin's, never the
    /// caller's. What it said is in the log.
    Panicked(Arc<str>),
}

impl CommandError {
    /// What it says, in words.
    pub fn words(&self) -> Words {
        use crate::words;
        match self {
            CommandError::Unknown(name) => words!(
                command_unknown = "unknown command `{ $command }`",
                command = name.to_string(),
                note = "Said to an agent, a script or a key bound by the user: no action has this name (an identifier, as written)."
            ),
            CommandError::InvalidArgs { name, reason } => words!(
                command_invalid_args = "invalid arguments for `{ $command }`: { $reason }",
                command = name.to_string(),
                reason = reason.to_string(),
                note = "Said to an agent or a script: the action's name, then why its arguments were refused, as the JSON reader wrote it (English)."
            ),
            CommandError::Panicked(name) => words!(
                command_panicked =
                    "{ $command } stopped on a fault of its own (the log says where)",
                command = crate::words::Name(name.to_string()),
                note = "An action failed through a fault of Skwir's or of a plugin's: its name (an identifier, as written)."
            ),
        }
    }
}

impl std::fmt::Display for CommandError {
    /// Its words: English, or the reader's language while an error's chain
    /// is said.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.words(), f)
    }
}

impl std::error::Error for CommandError {}

/// The command registry.
///
/// A name registered by a second owner shadows the first rather than
/// replacing it: when that owner is disabled, the earlier command comes
/// back instead of the name disappearing. The same owner registering a
/// name again replaces its own command. A plugin from outside
/// (`register_dynamic`) shadows nothing: a name another owner holds is
/// refused to it.
///
/// Arguments are checked before a handler runs: an object's names must be
/// ones the command's schema holds (a typed command's, read from its type),
/// so a misspelled argument is refused, never dropped.
#[derive(Default)]
pub struct CommandRegistry {
    /// Per name, every owner's registration; the last one is active.
    commands: RwLock<IndexMap<Arc<str>, Vec<(CommandSpec, Handlers, Validator)>>>,
}

impl CommandRegistry {
    /// Register a typed handler. `A` must be deserializable from the JSON
    /// arguments and describable by a schema. `topic` is what it is about
    /// (see [`Topic`]).
    pub fn register<A, F>(
        &self,
        owner: &str,
        topic: Topic,
        name: &str,
        title: impl Into<Words>,
        description: impl Into<Words>,
        handler: F,
    ) -> Declared<'_>
    where
        A: DeserializeOwned + JsonSchema + 'static,
        F: Fn(A, &App) -> anyhow::Result<Outcome> + Send + Sync + 'static,
    {
        let n: Arc<str> = name.into();
        let takes = Takes::new(ArgsSchema::of::<A>());
        let check = Arc::clone(&takes);
        let handler = Arc::new(handler);
        let from_json = Arc::clone(&handler);
        let json: Handler = Arc::new(move |args, app| {
            let invalid = |reason| CommandError::InvalidArgs {
                name: n.clone(),
                reason,
            };
            check.check(args).map_err(invalid)?;
            let typed = A::deserialize(args).map_err(invalid)?;
            from_json(typed, app)
        });
        // Its own type needs no checking: it is the handler's.
        let typed: TypedHandler =
            Arc::new(move |args: Box<dyn std::any::Any + Send>, app: &App| {
                match args.downcast::<A>() {
                    Ok(typed) => handler(*typed, app),
                    Err(_) => anyhow::bail!("arguments of another type"),
                }
            });
        let h = Handlers {
            json,
            typed: Some((std::any::TypeId::of::<A>(), typed)),
        };
        self.insert::<A>(owner, topic, name, title, description, takes, h)
    }

    /// Register a command whose handler takes its arguments as they came,
    /// once they are known to be `A`'s: one carried out elsewhere (the
    /// window's `ui.*`, which waits for it as it was asked).
    pub fn register_raw<A, F>(
        &self,
        owner: &str,
        topic: Topic,
        name: &str,
        title: impl Into<Words>,
        description: impl Into<Words>,
        handler: F,
    ) -> Declared<'_>
    where
        A: DeserializeOwned + JsonSchema + 'static,
        F: Fn(serde_json::Value, &App) -> anyhow::Result<Outcome> + Send + Sync + 'static,
    {
        let n: Arc<str> = name.into();
        let takes = Takes::new(ArgsSchema::of::<A>());
        let check = Arc::clone(&takes);
        let json: Handler = Arc::new(move |args, app| {
            let invalid = |reason| CommandError::InvalidArgs {
                name: n.clone(),
                reason,
            };
            check.check(args).map_err(invalid)?;
            A::deserialize(args).map_err(invalid)?;
            handler(args.clone(), app)
        });
        let h = Handlers { json, typed: None };
        self.insert::<A>(owner, topic, name, title, description, takes, h)
    }

    /// Register a command whose arguments `schema` describes, handled as
    /// JSON: a plugin's from outside skwir (ADR-045), whose arguments are
    /// data, not a Rust type. They are an object; a name the schema's
    /// `properties` do not hold is refused, as is one its `required` lists
    /// missing.
    #[allow(clippy::too_many_arguments)] // A command's declaration, whole.
    pub fn register_dynamic<F>(
        &self,
        owner: &str,
        topic: Topic,
        name: &str,
        title: impl Into<Words>,
        description: impl Into<Words>,
        schema: serde_json::Value,
        handler: F,
    ) -> Declared<'_>
    where
        F: Fn(serde_json::Value, &App) -> anyhow::Result<Outcome> + Send + Sync + 'static,
    {
        let names = |key: &str| -> Option<Vec<String>> {
            match schema.get(key)? {
                serde_json::Value::Object(o) => Some(o.keys().cloned().collect()),
                serde_json::Value::Array(a) => Some(
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect(),
                ),
                _ => None,
            }
        };
        let (properties, required) = (names("properties"), names("required").unwrap_or_default());
        let v: Validator = Arc::new(move |args| {
            use serde::de::Error as _;
            let object = args
                .as_object()
                .ok_or_else(|| serde_json::Error::custom("the arguments are an object"))?;
            if let Some(properties) = &properties
                && let Some(unknown) = object.keys().find(|key| !properties.contains(key))
            {
                return Err(serde_json::Error::custom(format!(
                    "unknown field `{unknown}`"
                )));
            }
            match required.iter().find(|key| !object.contains_key(*key)) {
                Some(missing) => Err(serde_json::Error::custom(format!(
                    "missing field `{missing}`"
                ))),
                None => Ok(()),
            }
        });
        let n: Arc<str> = name.into();
        let check = Arc::clone(&v);
        let json: Handler = Arc::new(move |args, app| {
            check(args).map_err(|reason| CommandError::InvalidArgs {
                name: n.clone(),
                reason,
            })?;
            handler(args.clone(), app)
        });
        let h = Handlers { json, typed: None };
        let schema = serde_json::from_value(schema)
            .unwrap_or_else(|_| schemars::json_schema!({ "type": "object" }));
        // A plugin from outside never shadows a command another owner
        // registered (the core's, the window's, a plugin's): it would be
        // handed what was asked of that one, the user's settings or
        // selection (ADR-045). Its registration is refused, and the name
        // keeps answering as it did.
        if let Some(holder) = self
            .commands
            .read()
            .get(name)
            .and_then(|stack| stack.iter().find(|(spec, _, _)| &*spec.owner != owner))
        {
            tracing::warn!(command = name, owner, holder = %holder.0.owner, "a plugin from outside may not take another's command");
            return Declared {
                registry: self,
                name: name.into(),
                owner: owner.into(),
            };
        }
        self.insert_spec(
            owner,
            topic,
            name,
            title,
            description,
            ArgsSchema::given(schema),
            v,
            h,
        )
    }

    #[allow(clippy::too_many_arguments)] // A command's declaration, whole.
    fn insert<A>(
        &self,
        owner: &str,
        topic: Topic,
        name: &str,
        title: impl Into<Words>,
        description: impl Into<Words>,
        takes: Arc<Takes>,
        h: Handlers,
    ) -> Declared<'_>
    where
        A: DeserializeOwned + JsonSchema + 'static,
    {
        let schema = takes.schema.clone();
        let v: Validator = Arc::new(move |args| {
            takes.check(args)?;
            A::deserialize(args).map(|_| ())
        });
        self.insert_spec(owner, topic, name, title, description, schema, v, h)
    }

    #[allow(clippy::too_many_arguments)] // A command's declaration, whole.
    fn insert_spec(
        &self,
        owner: &str,
        topic: Topic,
        name: &str,
        title: impl Into<Words>,
        description: impl Into<Words>,
        args_schema: ArgsSchema,
        v: Validator,
        h: Handlers,
    ) -> Declared<'_> {
        let name: Arc<str> = name.into();
        let spec = CommandSpec {
            name: name.clone(),
            title: title.into(),
            description: description.into(),
            args_schema,
            owner: owner.into(),
            topic,
            effect: Effect::default(),
            forms: Vec::new(),
            keeps_a_key: false,
            data: false,
            same_as: None,
            asked_only: false,
            in_dialogs: None,
            leads_to: None,
            opens: Opens::Here,
            repeats: None,
        };
        let mut commands = self.commands.write();
        let stack = commands.entry(name).or_default();
        if let Some(prev) = stack.last()
            && prev.0.owner != spec.owner
        {
            tracing::debug!(command = %spec.name, owner = %spec.owner, shadowed = %prev.0.owner, "command shadows another");
        }
        stack.retain(|(s, _, _)| s.owner != spec.owner);
        let declared = Declared {
            registry: self,
            name: spec.name.clone(),
            owner: spec.owner.clone(),
        };
        stack.push((spec, h, v));
        declared
    }

    /// Remove every command owned by `owner` (a plugin being disabled);
    /// a command it shadowed becomes active again.
    pub fn unregister_owner(&self, owner: &str) {
        let mut commands = self.commands.write();
        for stack in commands.values_mut() {
            stack.retain(|(spec, _, _)| &*spec.owner != owner);
        }
        commands.retain(|_, stack| !stack.is_empty());
    }

    pub fn get(&self, name: &str) -> Option<CommandSpec> {
        self.commands
            .read()
            .get(name)
            .and_then(|stack| stack.last())
            .map(|(s, _, _)| s.clone())
    }

    /// What `App::in_dialogs` reads of the command `name`, copied out in
    /// place: `None` for one that is not registered.
    pub(crate) fn dialog_terms(&self, name: &str) -> Option<DialogTerms> {
        self.commands
            .read()
            .get(name)
            .and_then(|stack| stack.last())
            .map(|(spec, _, _)| DialogTerms {
                said: spec.in_dialogs,
                by_default: spec.in_dialogs_by_default(),
                asked: spec.data || spec.asked_only,
                leads_to: spec.leads_to,
            })
    }

    /// Whether a command of that name is registered now.
    pub fn contains(&self, name: &str) -> bool {
        self.commands.read().contains_key(name)
    }

    /// What `name` run with `args` is called (`CommandSpec::title_for`),
    /// read in place: `None` for no such command.
    pub fn title_words(&self, name: &str, args: &serde_json::Value) -> Option<Words> {
        self.commands
            .read()
            .get(name)
            .and_then(|stack| stack.last())
            .map(|(spec, _, _)| spec.title_for(args))
    }

    /// Whether a key held down runs `name` again with `args`
    /// (`CommandSpec::repeats`), read in place: never for no such command.
    pub fn repeats(&self, name: &str, args: &serde_json::Value) -> bool {
        self.commands
            .read()
            .get(name)
            .and_then(|stack| stack.last())
            .is_some_and(|(spec, _, _)| spec.repeats_with(args))
    }

    pub fn list(&self) -> Vec<CommandSpec> {
        self.commands
            .read()
            .values()
            .filter_map(|stack| stack.last())
            .map(|(s, _, _)| s.clone())
            .collect()
    }

    /// Replace the forms of `owner`'s command `name` (see [`Form`]) with
    /// `forms`: those that follow what the user keeps (a pinned tag's),
    /// which every list of actions reads as they are when it opens.
    /// Nothing when `owner` registered no such command.
    pub fn set_forms(&self, owner: &str, name: &str, forms: Vec<Form>) {
        let mut commands = self.commands.write();
        if let Some((spec, _, _)) = commands
            .get_mut(name)
            .and_then(|stack| stack.iter_mut().find(|(s, _, _)| &*s.owner == owner))
        {
            spec.forms = forms;
        }
    }

    /// Whether `inv` names a command and gives it arguments it takes,
    /// without running it: a binding the user wrote, or a command the
    /// window carries out itself.
    ///
    /// # Errors
    /// [`CommandError::Unknown`], or [`CommandError::InvalidArgs`].
    pub fn check(&self, inv: &Invocation) -> Result<(), CommandError> {
        let validator = self
            .commands
            .read()
            .get(&inv.name)
            .and_then(|stack| stack.last())
            .map(|(_, _, v)| Arc::clone(v))
            .ok_or_else(|| CommandError::Unknown(inv.name.clone()))?;
        validator(&inv.args).map_err(|reason| CommandError::InvalidArgs {
            name: inv.name.clone(),
            reason,
        })
    }

    /// Dispatch an invocation. The handler runs on the caller's thread and
    /// must not block: long work goes through the scheduler or returns a plan.
    pub fn dispatch(&self, inv: &Invocation, app: &App) -> anyhow::Result<Outcome> {
        let handlers = self.handlers(&inv.name)?;
        Self::guarded(&inv.name, || (handlers.json)(&inv.args, app))
    }

    /// [`dispatch`](Self::dispatch) for a caller in this process holding
    /// typed arguments (the window's views: every row's id they ask
    /// metadata for, every change of a rubber band): a command registered
    /// with that type (`register::<A>`) is handed them as they are, without
    /// a JSON value made, copied and read back. Any other (a plugin's that
    /// took the name with arguments of its own, one from outside) gets them
    /// as JSON, as from `dispatch`. The same handler runs either way, on the
    /// caller's thread, its panic caught the same way.
    ///
    /// # Errors
    /// As [`dispatch`](Self::dispatch); [`CommandError::InvalidArgs`] when
    /// they cannot be made JSON for a command that takes JSON.
    pub fn dispatch_args<A: Serialize + Send + 'static>(
        &self,
        name: &str,
        args: A,
        app: &App,
    ) -> anyhow::Result<Outcome> {
        let handlers = self.handlers(name)?;
        match handlers.typed {
            Some((of, typed)) if of == std::any::TypeId::of::<A>() => {
                Self::guarded(name, || typed(Box::new(args), app))
            }
            _ => {
                let args =
                    serde_json::to_value(&args).map_err(|reason| CommandError::InvalidArgs {
                        name: name.into(),
                        reason,
                    })?;
                Self::guarded(name, || (handlers.json)(&args, app))
            }
        }
    }

    /// What runs the command `name` now: its last registration's.
    fn handlers(&self, name: &str) -> Result<Handlers, CommandError> {
        self.commands
            .read()
            .get(name)
            .and_then(|stack| stack.last())
            .map(|(_, h, _)| h.clone())
            .ok_or_else(|| CommandError::Unknown(name.into()))
    }

    /// Run a handler of the command `name`. Its panic must not unwind into
    /// its caller (the window's event loop, the control channel, a script):
    /// it is logged, and the call fails, as a scheduler's job does.
    fn guarded(
        name: &str,
        run: impl FnOnce() -> anyhow::Result<Outcome>,
    ) -> anyhow::Result<Outcome> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|payload| {
            let message = crate::scheduler::panic_message(&*payload);
            tracing::error!(command = %name, "command panicked: {message}");
            Err(CommandError::Panicked(name.into()).into())
        })
    }
}
