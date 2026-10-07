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
}

/// Declared metadata of a command, including the JSON schema of its arguments.
#[derive(Clone, Debug, Serialize)]
pub struct CommandSpec {
    pub name: Arc<str>,
    pub title: String,
    pub description: String,
    pub args_schema: schemars::Schema,
    /// Plugin id, or `core`.
    pub owner: Arc<str>,
}

impl CommandSpec {
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
}

/// The command registry.
///
/// A name registered by a second owner shadows the first rather than
/// replacing it: when that owner is disabled, the earlier command comes
/// back instead of the name disappearing. The same owner registering a
/// name again replaces its own command.
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
    ) where
        A: DeserializeOwned + JsonSchema + 'static,
        F: Fn(A, &App) -> anyhow::Result<Outcome> + Send + Sync + 'static,
    {
        let n: Arc<str> = name.into();
        let h: Handler = Arc::new(move |args, app| {
            let typed: A =
                serde_json::from_value(args).map_err(|reason| CommandError::InvalidArgs {
                    name: n.clone(),
                    reason,
                })?;
            handler(typed, app)
        });
        self.insert::<A>(owner, name, title, description, h);
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
    ) where
        A: DeserializeOwned + JsonSchema + 'static,
        F: Fn(serde_json::Value, &App) -> anyhow::Result<Outcome> + Send + Sync + 'static,
    {
        let n: Arc<str> = name.into();
        let h: Handler = Arc::new(move |args, app| {
            serde_json::from_value::<A>(args.clone()).map_err(|reason| {
                CommandError::InvalidArgs {
                    name: n.clone(),
                    reason,
                }
            })?;
            handler(args, app)
        });
        self.insert::<A>(owner, name, title, description, h);
    }

    fn insert<A>(&self, owner: &str, name: &str, title: &str, description: &str, h: Handler)
    where
        A: DeserializeOwned + JsonSchema + 'static,
    {
        let name: Arc<str> = name.into();
        let spec = CommandSpec {
            name: name.clone(),
            title: title.into(),
            description: description.into(),
            args_schema: schemars::schema_for!(A),
            owner: owner.into(),
        };
        let v: Validator = Arc::new(|args| serde_json::from_value::<A>(args.clone()).map(|_| ()));
        let mut commands = self.commands.write();
        let stack = commands.entry(name).or_default();
        if let Some(prev) = stack.last()
            && prev.0.owner != spec.owner
        {
            tracing::debug!(command = %spec.name, owner = %spec.owner, shadowed = %prev.0.owner, "command shadows another");
        }
        stack.retain(|(s, _, _)| s.owner != spec.owner);
        stack.push((spec, h, v));
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
        handler(inv.args.clone(), app)
    }
}
