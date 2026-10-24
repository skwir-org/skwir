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
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("nothing selected")
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
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("nothing to paste")
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
    /// 3 paths"): the window puts it there; a script or an agent gets the
    /// text.
    Clipboard {
        text: String,
        said: String,
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
    pub title: String,
}

impl Form {
    pub fn new(args: serde_json::Value, title: impl Into<String>) -> Self {
        Form {
            args,
            title: title.into(),
        }
    }
}

/// `title` with the arguments said after it, as a command is called when
/// no form names them: `Sort (key name)`.
pub fn with_args(title: &str, args: &serde_json::Value) -> String {
    match args.as_object().filter(|args| !args.is_empty()) {
        None => title.to_owned(),
        Some(args) => {
            let said: Vec<String> = args
                .iter()
                .map(|(name, value)| match value.as_str() {
                    Some(text) => format!("{name} {text}"),
                    None => format!("{name} {value}"),
                })
                .collect();
            format!("{title} ({})", said.join(", "))
        }
    }
}

/// When a key held down runs a command again, by the arguments it runs
/// with (`CommandSpec::repeats`).
pub type Repeats = fn(&serde_json::Value) -> bool;

/// Again with every repeat of its key: a command that only steps (the
/// cursor, a page, the next tab).
pub fn always(_: &serde_json::Value) -> bool {
    true
}

/// Declared metadata of a command, including the JSON schema of its arguments.
#[derive(Clone, Debug, Serialize)]
pub struct CommandSpec {
    pub name: Arc<str>,
    pub title: String,
    pub description: String,
    pub args_schema: ArgsSchema,
    /// Plugin id, or `core`.
    pub owner: Arc<str>,
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
    /// When a key held down runs it again (ADR-066): a step along
    /// something (the cursor, a page, the volume, the next tab), never an
    /// act or a switch, which the key's repeats would run over and over.
    /// `None`, the default: once a press. Only keys repeat, so it is not
    /// said to agents.
    #[serde(skip)]
    pub repeats: Option<Repeats>,
}

impl CommandSpec {
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

    /// The form of these arguments: `None` for none declared.
    pub fn form(&self, args: &serde_json::Value) -> Option<&Form> {
        let none = serde_json::Value::Object(serde_json::Map::new());
        let args = if args.is_null() { &none } else { args };
        self.forms.iter().find(|form| form.args == *args)
    }

    /// What it is called run with `args`: their form's title, else its
    /// own, with the arguments no form names said after it, so two ways
    /// to run it never read alike.
    pub fn title_for(&self, args: &serde_json::Value) -> String {
        match self.form(args) {
            Some(form) => form.title.clone(),
            None => with_args(&self.title, args),
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

type Handler = Arc<dyn Fn(serde_json::Value, &App) -> anyhow::Result<Outcome> + Send + Sync>;
/// Whether arguments are a command's, without running it.
type Validator = Arc<dyn Fn(&serde_json::Value) -> Result<(), serde_json::Error> + Send + Sync>;

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
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("unknown command `{0}`")]
    Unknown(Arc<str>),
    /// The reason is in the message, not a source: a chain printed whole
    /// (`{:#}`) would say it twice.
    #[error("invalid arguments for `{name}`: {reason}")]
    InvalidArgs {
        name: Arc<str>,
        reason: serde_json::Error,
    },
    /// Its handler panicked: a fault of skwir's or of a plugin's, never the
    /// caller's. What it said is in the log.
    #[error("“{0}” stopped on a fault of its own (the log says where)")]
    Panicked(Arc<str>),
}

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
    commands: RwLock<IndexMap<Arc<str>, Vec<(CommandSpec, Handler, Validator)>>>,
}

impl CommandRegistry {
    /// Register a typed handler. `A` must be deserializable from the JSON
    /// arguments and describable by a schema.
    pub fn register<A, F>(
        &self,
        owner: &str,
        name: &str,
        title: &str,
        description: &str,
        handler: F,
    ) -> Declared<'_>
    where
        A: DeserializeOwned + JsonSchema + 'static,
        F: Fn(A, &App) -> anyhow::Result<Outcome> + Send + Sync + 'static,
    {
        let n: Arc<str> = name.into();
        let takes = Takes::new(ArgsSchema::of::<A>());
        let check = Arc::clone(&takes);
        let h: Handler = Arc::new(move |args, app| {
            let invalid = |reason| CommandError::InvalidArgs {
                name: n.clone(),
                reason,
            };
            check.check(&args).map_err(invalid)?;
            let typed: A = serde_json::from_value(args).map_err(invalid)?;
            handler(typed, app)
        });
        self.insert::<A>(owner, name, title, description, takes, h)
    }

    /// Register a command whose handler takes its arguments as they came,
    /// once they are known to be `A`'s: one carried out elsewhere (the
    /// window's `ui.*`, which waits for it as it was asked).
    pub fn register_raw<A, F>(
        &self,
        owner: &str,
        name: &str,
        title: &str,
        description: &str,
        handler: F,
    ) -> Declared<'_>
    where
        A: DeserializeOwned + JsonSchema + 'static,
        F: Fn(serde_json::Value, &App) -> anyhow::Result<Outcome> + Send + Sync + 'static,
    {
        let n: Arc<str> = name.into();
        let takes = Takes::new(ArgsSchema::of::<A>());
        let check = Arc::clone(&takes);
        let h: Handler = Arc::new(move |args, app| {
            let invalid = |reason| CommandError::InvalidArgs {
                name: n.clone(),
                reason,
            };
            check.check(&args).map_err(invalid)?;
            serde_json::from_value::<A>(args.clone()).map_err(invalid)?;
            handler(args, app)
        });
        self.insert::<A>(owner, name, title, description, takes, h)
    }

    /// Register a command whose arguments `schema` describes, handled as
    /// JSON: a plugin's from outside skwir (ADR-045), whose arguments are
    /// data, not a Rust type. They are an object; a name the schema's
    /// `properties` do not hold is refused, as is one its `required` lists
    /// missing.
    pub fn register_dynamic<F>(
        &self,
        owner: &str,
        name: &str,
        title: &str,
        description: &str,
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
        let h: Handler = Arc::new(move |args, app| {
            check(&args).map_err(|reason| CommandError::InvalidArgs {
                name: n.clone(),
                reason,
            })?;
            handler(args, app)
        });
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
            name,
            title,
            description,
            ArgsSchema::given(schema),
            v,
            h,
        )
    }

    fn insert<A>(
        &self,
        owner: &str,
        name: &str,
        title: &str,
        description: &str,
        takes: Arc<Takes>,
        h: Handler,
    ) -> Declared<'_>
    where
        A: DeserializeOwned + JsonSchema + 'static,
    {
        let schema = takes.schema.clone();
        let v: Validator = Arc::new(move |args| {
            takes.check(args)?;
            serde_json::from_value::<A>(args.clone()).map(|_| ())
        });
        self.insert_spec(owner, name, title, description, schema, v, h)
    }

    #[allow(clippy::too_many_arguments)] // A command's declaration, whole.
    fn insert_spec(
        &self,
        owner: &str,
        name: &str,
        title: &str,
        description: &str,
        args_schema: ArgsSchema,
        v: Validator,
        h: Handler,
    ) -> Declared<'_> {
        let name: Arc<str> = name.into();
        let spec = CommandSpec {
            name: name.clone(),
            title: title.into(),
            description: description.into(),
            args_schema,
            owner: owner.into(),
            effect: Effect::default(),
            forms: Vec::new(),
            keeps_a_key: false,
            data: false,
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

    /// Whether a command of that name is registered now.
    pub fn contains(&self, name: &str) -> bool {
        self.commands.read().contains_key(name)
    }

    /// What `name` run with `args` is called (`CommandSpec::title_for`),
    /// read in place: `None` for no such command.
    pub fn title(&self, name: &str, args: &serde_json::Value) -> Option<String> {
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
        let handler = self
            .commands
            .read()
            .get(&inv.name)
            .and_then(|stack| stack.last())
            .map(|(_, h, _)| Arc::clone(h));
        let handler = handler.ok_or_else(|| CommandError::Unknown(inv.name.clone()))?;
        // A handler's panic must not unwind into its caller (the window's
        // event loop, the control channel, a script): it is logged, and the
        // call fails, as a scheduler's job does.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(inv.args.clone(), app)
        }))
        .unwrap_or_else(|payload| {
            let message = crate::scheduler::panic_message(&*payload);
            tracing::error!(command = %inv.name, "command panicked: {message}");
            Err(CommandError::Panicked(inv.name.clone()).into())
        })
    }
}
