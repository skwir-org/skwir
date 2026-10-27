//! `plugin.toml`: what a plugin from outside skwir is, what it may do,
//! what it provides, and how it runs (ADR-045).
//!
//! ```toml
//! id = "word-count"
//! title = "Word count"
//! description = "How many words a text holds, in Properties"
//! category = "files"
//! capabilities = ["read"]
//!
//! [run]
//! module = "word_count.wasm"   # or: program = "bin/word-count"
//!
//! [[fields]]
//! key = "word-count.words"
//! label = "Words"
//! types = ["text/plain"]
//! ```

use serde::{Deserialize, Serialize};

use crate::protocol::{OLDEST, VERSION};

/// What a plugin may do (skwir's `permissions::Capability`). A plugin
/// gets what it declares once the user turns it on, and nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Read what the files it is asked about hold.
    Read,
    /// Change files: answer a command with a plan.
    Write,
    /// Use the network (a plugin that runs as a program).
    Network,
    /// Start programs (inside the plugin's own isolation).
    Programs,
}

/// Where a plugin is listed among the others (skwir's `plugin::Category`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Browsing,
    #[default]
    Files,
    Previews,
    Desktop,
}

/// What a command is about (skwir's `command::Topic`): where F1 lists
/// its keys. A command that names none is its plugin's category's: going
/// places for browsing, files for files and desktop, previews for
/// previews.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    Go,
    Select,
    Files,
    Find,
    Notes,
    Tabs,
    View,
    Preview,
    Player,
    Terminal,
    Places,
    Window,
    Skwir,
}

/// What a command does (skwir's `command::Effect`), which decides what
/// an agent may run without asking.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Answers; changes nothing.
    Reads,
    /// Changes only what the window shows.
    Shows,
    /// Answers with a plan: needs the `write` capability.
    Plans,
    /// Anything else.
    #[default]
    Acts,
}

/// A plugin's `plugin.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Its name among plugins and the prefix of its commands and fields:
    /// lowercase letters, digits and dashes.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: Category,
    /// The protocol's version it speaks ([`VERSION`]).
    #[serde(default = "version")]
    pub api: u32,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    pub run: Run,
    #[serde(default)]
    pub fields: Vec<FieldDecl>,
    #[serde(default)]
    pub thumbnails: Vec<ThumbnailDecl>,
    #[serde(default)]
    pub previews: Vec<PreviewDecl>,
    #[serde(default)]
    pub commands: Vec<CommandDecl>,
}

fn version() -> u32 {
    VERSION
}

/// How it runs: one of a module or a program, relative to its folder.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    /// A WebAssembly module (or component) of the `plugin` world.
    #[serde(default)]
    pub module: Option<String>,
    /// A program that speaks the protocol on its standard input and output.
    #[serde(default)]
    pub program: Option<String>,
    /// The program's arguments.
    #[serde(default)]
    pub args: Vec<String>,
}

/// A field it reads on demand, shown in Properties.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldDecl {
    /// `<id>.name`.
    pub key: String,
    /// What Properties calls it; none keeps it out of Properties.
    #[serde(default)]
    pub label: Option<String>,
    /// The MIME types of the files it reads it from.
    pub types: Vec<String>,
}

/// The files it draws thumbnails of.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThumbnailDecl {
    pub types: Vec<String>,
}

/// The files it previews: an inspection (text, code, hex rows, a table),
/// with `converts` another file skwir shows, or with `pages` a page at a
/// time.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewDecl {
    pub types: Vec<String>,
    /// Higher is tried first; a fallback says a negative one.
    #[serde(default)]
    pub priority: i32,
    /// It shows any file as its bytes: the view every file offers (View
    /// as hex).
    #[serde(default)]
    pub shows_bytes: bool,
    /// It answers `convert`: the file is shown as the one it makes (a PDF,
    /// a picture, Markdown…), by skwir's own preview of that type.
    #[serde(default)]
    pub converts: bool,
    /// It answers `page`: the file is shown a page at a time, each a
    /// picture it draws when shown. With `converts`, for the files it
    /// does not convert.
    #[serde(default)]
    pub pages: bool,
}

/// A command it runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDecl {
    /// `<id>.name`.
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub effect: Effect,
    /// What it is about; its plugin's category's when it names none (see
    /// [`Topic`]).
    #[serde(default)]
    pub topic: Option<Topic>,
    /// The JSON schema of its arguments, an object; none for none. The
    /// files it acts on come as `entries`, a pane's selection when the
    /// caller names none.
    #[serde(default)]
    pub arguments: Option<serde_json::Value>,
}

impl Manifest {
    /// Whether it holds together: an id of the allowed letters, names and
    /// keys under it, one way to run, types for what it provides, the
    /// protocol's version, and a plan only from a plugin that may write.
    ///
    /// # Errors
    /// What does not hold, in words.
    pub fn check(&self) -> Result<(), String> {
        check_id(&self.id)?;
        if !(OLDEST..=VERSION).contains(&self.api) {
            return Err(format!(
                "it speaks version {} of the protocol; skwir speaks {OLDEST} to {VERSION}",
                self.api
            ));
        }
        if self
            .previews
            .iter()
            .any(|p| (p.converts || p.pages) && p.shows_bytes)
        {
            return Err("a preview of any file's bytes shows them, as they are".into());
        }
        match (&self.run.module, &self.run.program) {
            (Some(_), None) | (None, Some(_)) => {}
            _ => return Err("[run] names a module or a program, one of them".into()),
        }
        if self.run.module.is_some() && self.capabilities.contains(&Capability::Network) {
            return Err("a module cannot use the network yet: run it as a program".into());
        }
        let under = |name: &str| {
            name.strip_prefix(&self.id)
                .and_then(|rest| rest.strip_prefix('.'))
                .is_some_and(|rest| !rest.is_empty())
        };
        for field in &self.fields {
            if !under(&field.key) {
                return Err(format!("field `{}`: named `{}.…`", field.key, self.id));
            }
            if field.types.is_empty() {
                return Err(format!("field `{}`: which types?", field.key));
            }
        }
        for command in &self.commands {
            if !under(&command.name) {
                return Err(format!("command `{}`: named `{}.…`", command.name, self.id));
            }
            if command.effect == Effect::Plans && !self.capabilities.contains(&Capability::Write) {
                return Err(format!(
                    "command `{}` plans: the plugin needs the `write` capability",
                    command.name
                ));
            }
            if let Some(arguments) = &command.arguments
                && arguments.get("type").and_then(|t| t.as_str()) != Some("object")
            {
                return Err(format!(
                    "command `{}`: its arguments are an object schema",
                    command.name
                ));
            }
        }
        if self.thumbnails.iter().any(|t| t.types.is_empty())
            || self.previews.iter().any(|p| p.types.is_empty())
        {
            return Err("a thumbnail or a preview names its types".into());
        }
        Ok(())
    }
}

/// The namespaces of skwir's own commands, settings and fields that are
/// not a built-in plugin's id: the core's and the shell's, and those a
/// built-in plugin names some of its own under. A plugin from outside may
/// take none as its id, whose commands and fields are named under it: they
/// would sit among skwir's, and a command of the same name would stand for
/// skwir's own. A built-in plugin's id is refused where plugins are found
/// (`skwir_extend::discover`), and `crates/skwir/tests/outside_ids.rs`
/// checks that this list leaves out none of the namespaces of skwir's
/// commands and settings. `keys` is the keymap's (`keys.like`).
pub const RESERVED: &[&str] = &[
    "archive", "core", "keyboard", "keys", "names", "notes", "player", "plugins", "privacy",
    "reading", "settings", "tab", "ui",
];

/// Whether `id` can be a plugin's: lowercase letters, digits and dashes,
/// starting with a letter, and none of skwir's own namespaces
/// ([`RESERVED`]).
///
/// # Errors
/// Why it cannot, in words.
pub fn check_id(id: &str) -> Result<(), String> {
    let id_ok = id.len() >= 2
        && id.len() <= 64
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !id_ok {
        return Err(format!(
            "id `{id}`: lowercase letters, digits and dashes, starting with a letter"
        ));
    }
    if RESERVED.contains(&id) {
        return Err(format!(
            "id `{id}`: skwir's own commands and settings are named under it"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str) -> Manifest {
        Manifest {
            id: id.into(),
            title: "Test".into(),
            description: String::new(),
            category: Category::Files,
            api: VERSION,
            capabilities: vec![Capability::Read],
            run: Run {
                module: Some("test.wasm".into()),
                ..Run::default()
            },
            fields: Vec::new(),
            thumbnails: Vec::new(),
            previews: Vec::new(),
            commands: Vec::new(),
        }
    }

    #[test]
    fn a_manifest_holds_together_or_says_why() {
        assert_eq!(manifest("word-count").check(), Ok(()));
        assert!(manifest("Word").check().is_err());
        assert!(manifest("x").check().is_err());
        // Skwir's own namespaces: `plugins.set_enabled`, `settings.apply`,
        // the shell's `ui.*`.
        for id in ["plugins", "settings", "ui", "tab"] {
            assert!(manifest(id).check().unwrap_err().contains("skwir's own"));
        }
        let mut both = manifest("both");
        both.run.program = Some("bin/both".into());
        assert!(both.check().is_err());
        let mut network = manifest("net");
        network.capabilities.push(Capability::Network);
        assert!(network.check().unwrap_err().contains("network"));
        let mut outside = manifest("count");
        outside.commands.push(CommandDecl {
            name: "other.count".into(),
            title: "Count".into(),
            description: String::new(),
            effect: Effect::Reads,
            topic: None,
            arguments: None,
        });
        assert!(outside.check().is_err());
        let mut plans = manifest("rename");
        plans.commands.push(CommandDecl {
            name: "rename.all".into(),
            title: "Rename".into(),
            description: String::new(),
            effect: Effect::Plans,
            topic: Some(Topic::Files),
            arguments: None,
        });
        assert!(plans.check().unwrap_err().contains("write"));
    }

    #[test]
    fn a_plugin_speaks_a_version_skwir_speaks() {
        let preview = |converts| PreviewDecl {
            types: vec!["image/x-portable-pixmap".into()],
            priority: 0,
            shows_bytes: false,
            converts,
            pages: false,
        };
        let mut first = manifest("first");
        first.api = OLDEST;
        first.previews.push(preview(true));
        first.previews.push(PreviewDecl {
            pages: true,
            ..preview(false)
        });
        assert_eq!(first.check(), Ok(()));
        first.api = VERSION + 1;
        assert!(first.check().unwrap_err().contains("version"));
        first.api = OLDEST - 1;
        assert!(first.check().unwrap_err().contains("version"));
        let mut bytes = manifest("bytes");
        bytes.previews.push(PreviewDecl {
            shows_bytes: true,
            ..preview(true)
        });
        assert!(bytes.check().is_err());
        // Written in a plugin.toml, as a plugin says it.
        let said: Manifest = toml::from_str(
            "id = \"said\"\ntitle = \"Said\"\napi = 1\n[run]\nmodule = \"s.wasm\"\n\
             [[previews]]\ntypes = [\"text/x-said\"]\nconverts = true\n",
        )
        .unwrap();
        assert!(said.previews[0].converts);
        assert_eq!(said.check(), Ok(()));
    }

    /// A command says what it is about, or leaves it to its category.
    #[test]
    fn a_command_names_its_topic_or_none() {
        let said: Manifest = toml::from_str(
            "id = \"said\"\ntitle = \"Said\"\n[run]\nmodule = \"s.wasm\"\n\
             [[commands]]\nname = \"said.find\"\ntitle = \"Find\"\ntopic = \"find\"\n\
             [[commands]]\nname = \"said.other\"\ntitle = \"Other\"\n",
        )
        .unwrap();
        assert_eq!(said.commands[0].topic, Some(Topic::Find));
        assert_eq!(said.commands[1].topic, None);
        assert!(
            toml::from_str::<Manifest>(
                "id = \"said\"\ntitle = \"Said\"\n[run]\nmodule = \"s.wasm\"\n\
             [[commands]]\nname = \"said.x\"\ntitle = \"X\"\ntopic = \"misc\"\n",
            )
            .is_err()
        );
    }
}
