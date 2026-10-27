//! Settings: a TOML file, validated against the schema plugins register,
//! read through typed accessors. Plugins own the `plugin_id.*` namespace.
//!
//! The file is shared: another skwir, a hand edit or Home Manager may
//! change it while this one runs. A save writes the keys this process
//! changed over the file as it stands (`persist`), never its whole table,
//! and what changed in the file is taken in as it changes (`reload`,
//! watched by the app). What skwir keeps of the window as it was left
//! (`SettingSpec::state`) goes to a file of its own, in the state folder
//! (ADR-056): the settings file may then be read-only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use parking_lot::{Mutex, RwLock};

use crate::command::Topic;
use crate::os_path::PathArg;
use serde::de::DeserializeOwned;
use std::io::Write;

pub const PRESERVE_ATTRIBUTES_ON_COPY: &str = "core.preserve_attributes_on_copy";
/// Whether files dropped without a key held ask what to do.
pub const DROP_ASKS: &str = "core.drop_asks";
/// Which pane closing the split takes away: `other`, `active` or `right`.
pub const CLOSE_SPLIT: &str = "core.close_split";
/// `last` or `folder`.
pub const STARTUP: &str = "core.startup";
pub const STARTUP_FOLDER: &str = "core.startup_folder";
/// Application data: where the window was when it closed.
pub const LAST_LOCATION: &str = "core.last_location";

/// The most read of a file of settings (one of skwir's, or one to check):
/// a larger one is not one.
const FILE_LIMIT: u64 = 16 << 20;

/// The Settings dialog's pages, by their subject (ADR-072): a setting
/// names the one it is shown on (`SettingSpec::page`), whoever owns its
/// key. The dialog draws each, and orders its sections.
pub mod page {
    /// What skwir does at startup, with tabs, the split, the address, and
    /// what is dragged and dropped.
    pub const GENERAL: &str = "general";
    pub const APPEARANCE: &str = "appearance";
    /// How folders show their files: the Narrow by bar, the grid, the
    /// list's columns, thumbnails.
    pub const VIEWS: &str = "views";
    /// Tags, then where notes are kept.
    pub const NOTES: &str = "notes";
    /// How documents, books, text and code read, and their engines.
    pub const READING: &str = "reading";
    /// Pictures, audio and video, 3D models and maps.
    pub const MEDIA: &str = "media";
    /// The terminal and the program commands run in.
    pub const TERMINAL: &str = "terminal";
    /// The servers of the sidebar's Network section, where those added by
    /// their address are kept, and the tools that open an address.
    pub const NETWORK: &str = "network";
    pub const PRIVACY: &str = "privacy";
    pub const KEYBOARD: &str = "keyboard";
}

/// The namespace of a dotted setting key: the owning plugin's id (or
/// `core`/`ui`). The one place that says which keys a plugin owns.
pub fn owner_of(key: &str) -> &str {
    key.split_once('.').map_or(key, |(owner, _)| owner)
}

/// How an editor should present a setting. The value's type is still the
/// type of `default`; the kind only picks the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKind {
    Toggle,
    /// One of `constraints.choices`.
    Choice,
    Integer,
    Number,
    Text,
    /// A filesystem path; editors offer completion or a picker.
    Path,
    /// A program to run: its name, looked for on the `PATH`, or its path.
    /// Run alone, never as a command line: `bash -l` is no program.
    Command,
    /// Application data managed by its owning commands, not by editors.
    Data,
    /// A list of records the user adds, edits and removes (tile servers):
    /// each a table of `constraints.fields` plus an `id`, edited as JSON.
    Records,
}

/// One field of a record in a [`SettingKind::Records`] setting.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecordField {
    pub key: String,
    pub title: String,
    pub description: String,
    pub kind: FieldKind,
    /// A record without it is refused.
    pub required: bool,
}

/// What a record's field holds, which picks its control.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    Text {
        placeholder: String,
    },
    Integer {
        min: i64,
        max: i64,
    },
    /// Name and value pairs (HTTP headers), stored as `[[name, value], …]`.
    Pairs,
    /// A secret kept in the desktop's keyring, never in the settings: the
    /// field holds only [`SECRET_STORED`] or nothing, and the keyring item
    /// is found by the setting's key, the record's id and the field's key
    /// (`skwir_platform::secrets`).
    Secret,
}

/// What a [`FieldKind::Secret`] field holds when the keyring has its value.
pub const SECRET_STORED: &str = "keyring";

impl RecordField {
    fn new(key: &str, title: &str, description: &str, kind: FieldKind) -> Self {
        RecordField {
            key: key.into(),
            title: title.into(),
            description: description.into(),
            kind,
            required: false,
        }
    }

    pub fn text(key: &str, title: &str, description: &str, placeholder: &str) -> Self {
        Self::new(
            key,
            title,
            description,
            FieldKind::Text {
                placeholder: placeholder.into(),
            },
        )
    }

    pub fn integer(key: &str, title: &str, description: &str, (min, max): (i64, i64)) -> Self {
        Self::new(key, title, description, FieldKind::Integer { min, max })
    }

    pub fn pairs(key: &str, title: &str, description: &str) -> Self {
        Self::new(key, title, description, FieldKind::Pairs)
    }

    pub fn secret(key: &str, title: &str, description: &str) -> Self {
        Self::new(key, title, description, FieldKind::Secret)
    }

    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// A button on each record that runs `command` with `{ "id": <record id> }`
/// (a tile server's Test).
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecordAction {
    pub label: String,
    pub command: String,
}

/// Records their owner keeps elsewhere (rclone's remotes, in rclone's own
/// configuration, ADR-086): the dialog lists what `list` answers
/// (`{ "records": [{ "id": …, <field>: … }] }`, dispatched on the window's
/// thread, so answered from the owner's memory) and changes them only
/// through the owner's commands, each run with `{ "id": <record id> }`
/// (`add` with nothing): never edited in place, never moved. The setting's
/// own value is never read nor saved. The dialog lists them again when
/// `revision` moves ([`RecordSource::changed`]).
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecordSource {
    pub list: String,
    pub add: String,
    pub edit: String,
    pub remove: String,
    #[serde(skip)]
    pub revision: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl RecordSource {
    pub fn new(list: &str, add: &str, edit: &str, remove: &str) -> Self {
        RecordSource {
            list: list.into(),
            add: add.into(),
            edit: edit.into(),
            remove: remove.into(),
            revision: std::sync::Arc::default(),
        }
    }

    /// Say that what `list` answers changed: the dialog asks again.
    pub fn changed(&self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::Release);
    }

    /// What `changed` has moved it to.
    pub fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// The owner's own check of a value, after the kind's: what only it knows
/// (a tile address needs `{z}`, `{x}` and `{y}`). Runs wherever a value is
/// validated, in the dialog and in `settings.apply`, given the value
/// stored then, and in `settings.check`, given a file's value as it
/// stands ([`Beside`]): a check may spare what was already kept (a
/// shortcut for a plugin turned off, ADR-024).
#[derive(Clone)]
pub struct Validator(pub std::sync::Arc<ValidatorFn>);

/// What a [`Validator`] runs: the value, and what it is checked beside.
pub type ValidatorFn = dyn Fn(&toml::Value, Beside<'_>) -> anyhow::Result<()> + Send + Sync;

/// What a value is checked beside (see [`Validator`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Beside<'a> {
    /// A value to save over the one stored then (`None`: nothing is): the
    /// dialog's, `settings.apply`'s.
    Stored(Option<&'a toml::Value>),
    /// A file's value as it stands, nothing kept from before
    /// (`settings.check`): what may wait for a plugin that is off waits,
    /// what nothing could ever take is wrong.
    File,
}

impl std::fmt::Debug for Validator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Validator")
    }
}

/// One setting's declaration. Build it with the constructors below: the
/// owning plugin states the title, unit, choices and limits, so no editor
/// needs key-specific knowledge.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SettingSpec {
    /// Dotted key, e.g. `view.list.row_height` or `plugin_id.option`.
    pub key: String,
    /// Short human label ("Memory budget").
    pub title: String,
    pub description: String,
    /// The dialog's page it is shown on (`page::*`), by its subject,
    /// whoever owns the key (ADR-072); empty for its owner's own page.
    pub page: String,
    /// Optional group heading within its page. Sections stand in the
    /// order the page lists them, then in the order their first setting
    /// was declared, after what has none.
    pub section: String,
    /// Shown while its page is folded: among the few settings most people
    /// come to the page for (ADR-072). The others wait under the page's
    /// More settings.
    pub essential: bool,
    /// The switch this setting is shown under (its key), which it applies
    /// only while that is on; empty for none. The dialog draws it indented
    /// beneath the switch, unavailable while the switch is off.
    pub under: String,
    /// Unit shown after a numeric value ("MB").
    pub unit: String,
    pub default: toml::Value,
    pub kind: SettingKind,
    pub constraints: SettingConstraints,
    /// The change takes effect at the next start.
    pub restart_required: bool,
    /// The file it is kept in: the settings, or apart from them
    /// (`SettingSpec::state`, `SettingSpec::data`).
    pub store: Store,
    #[serde(skip)]
    pub validator: Option<Validator>,
    /// When its key was first declared in this process, which
    /// `Settings::declare` sets: a page shows its settings in that order,
    /// so the owner orders them by declaring them in it.
    #[serde(skip)]
    pub order: u64,
}

/// The file a setting is kept in (ADR-056, ADR-057).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    /// The settings file: preferences, which a configuration may declare,
    /// and link read-only.
    #[default]
    Settings,
    /// The state file (`folders::state`): the window as it was left,
    /// skwir's own.
    State,
    /// The data file (`folders::data`): what the user made in skwir, as
    /// saved searches, pinned tags and tags' colours.
    Data,
}

impl Store {
    /// What its file is called in a sentence.
    fn file_words(self) -> &'static str {
        match self {
            Store::Settings => "settings file",
            Store::State => "state file",
            Store::Data => "data file",
        }
    }
}

/// Optional restrictions shared by editors and command validation.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SettingConstraints {
    pub choices: Vec<String>,
    /// Human labels parallel to `choices` (empty: show the values).
    pub choice_labels: Vec<String>,
    pub integer_range: Option<(i64, i64)>,
    /// Increment of a numeric stepper.
    pub step: i64,
    /// The fields of a [`SettingKind::Records`] setting's records.
    pub fields: Vec<RecordField>,
    /// What one record is called ("tile server"), for "Add tile server".
    pub record_name: String,
    pub record_action: Option<RecordAction>,
    /// Records their owner keeps elsewhere, changed by its commands (see
    /// [`RecordSource`]).
    pub record_source: Option<RecordSource>,
    /// The choices are what is known here and now: others may be found
    /// where skwir runs (the themes a user added to bat). A file's value
    /// outside them is noted by `Settings::check`, not refused.
    pub more_choices: String,
}

impl SettingSpec {
    fn new(key: &str, title: &str, description: &str, default: toml::Value) -> Self {
        let kind = match default {
            toml::Value::Boolean(_) => SettingKind::Toggle,
            toml::Value::Integer(_) => SettingKind::Integer,
            toml::Value::Float(_) => SettingKind::Number,
            toml::Value::String(_) => SettingKind::Text,
            _ => SettingKind::Data,
        };
        SettingSpec {
            key: key.into(),
            title: title.into(),
            description: description.into(),
            page: String::new(),
            section: String::new(),
            essential: false,
            under: String::new(),
            unit: String::new(),
            default,
            kind,
            constraints: SettingConstraints {
                step: 1,
                ..SettingConstraints::default()
            },
            restart_required: false,
            store: Store::Settings,
            validator: None,
            order: 0,
        }
    }

    pub fn toggle(key: &str, title: &str, description: &str, default: bool) -> Self {
        Self::new(key, title, description, toml::Value::Boolean(default))
    }

    /// `choices` are `(stored value, label)` pairs.
    pub fn choice(
        key: &str,
        title: &str,
        description: &str,
        default: &str,
        choices: &[(&str, &str)],
    ) -> Self {
        let mut spec = Self::new(key, title, description, toml::Value::String(default.into()));
        spec.kind = SettingKind::Choice;
        spec.constraints.choices = choices.iter().map(|(v, _)| (*v).to_owned()).collect();
        spec.constraints.choice_labels = choices.iter().map(|(_, l)| (*l).to_owned()).collect();
        spec
    }

    pub fn integer(
        key: &str,
        title: &str,
        description: &str,
        default: i64,
        range: (i64, i64),
    ) -> Self {
        let mut spec = Self::new(key, title, description, toml::Value::Integer(default));
        spec.constraints.integer_range = Some(range);
        spec
    }

    pub fn number(key: &str, title: &str, description: &str, default: f64) -> Self {
        Self::new(key, title, description, toml::Value::Float(default))
    }

    pub fn text(key: &str, title: &str, description: &str, default: &str) -> Self {
        Self::new(key, title, description, toml::Value::String(default.into()))
    }

    pub fn path(key: &str, title: &str, description: &str, default: &str) -> Self {
        let mut spec = Self::text(key, title, description, default);
        spec.kind = SettingKind::Path;
        spec
    }

    /// A program to run (`SettingKind::Command`): a value with words after
    /// a name is refused, which would not start (`bash -l`), saying why. A
    /// path may hold spaces.
    pub fn command(key: &str, title: &str, description: &str, default: &str) -> Self {
        let mut spec = Self::text(key, title, description, default).validate_with(|value, _| {
            let program = value.as_str().unwrap_or_default().trim();
            anyhow::ensure!(
                !program.contains(char::is_whitespace)
                    || std::path::Path::new(program).is_absolute(),
                "Give the program alone, without arguments: “bash”, not “bash -l”"
            );
            Ok(())
        });
        spec.kind = SettingKind::Command;
        spec
    }

    /// What the user makes in skwir rather than chooses in its Settings
    /// (saved searches, pinned tags, tags' colours): kept in the data file,
    /// apart from the settings (ADR-057), so it is kept when the settings
    /// are read-only. Not shown in the dialog.
    pub fn data(key: &str, description: &str, default: toml::Value) -> Self {
        let mut spec = Self::new(key, key, description, default);
        spec.kind = SettingKind::Data;
        spec.store = Store::Data;
        spec
    }

    /// What skwir keeps of the window as it was left (its layout, its tabs,
    /// the hints it showed): data, in the state file rather than the
    /// settings (ADR-056), so the settings may be read-only.
    pub fn state(key: &str, description: &str, default: toml::Value) -> Self {
        let mut spec = Self::data(key, description, default);
        spec.store = Store::State;
        spec
    }

    /// A list of records, each made of `fields` (see [`SettingKind::Records`]).
    pub fn records(
        key: &str,
        title: &str,
        description: &str,
        record_name: &str,
        fields: Vec<RecordField>,
    ) -> Self {
        let mut spec = Self::new(key, title, description, toml::Value::Array(Vec::new()));
        spec.kind = SettingKind::Records;
        spec.constraints.fields = fields;
        spec.constraints.record_name = record_name.into();
        spec
    }

    /// A button on each record, running `command` with the record's id.
    #[must_use]
    pub fn record_action(mut self, label: &str, command: &str) -> Self {
        self.constraints.record_action = Some(RecordAction {
            label: label.into(),
            command: command.into(),
        });
        self
    }

    /// The records are kept by their owner elsewhere, listed and changed
    /// through its commands (see [`RecordSource`]): the list is never the
    /// settings'.
    #[must_use]
    pub fn records_kept_by(mut self, source: RecordSource) -> Self {
        self.constraints.record_source = Some(source);
        self
    }

    /// The owner's own check, after the kind's (see [`Validator`]): of
    /// the value, beside the one stored or as a file holds it.
    #[must_use]
    pub fn validate_with<F>(mut self, check: F) -> Self
    where
        F: Fn(&toml::Value, Beside<'_>) -> anyhow::Result<()> + Send + Sync + 'static,
    {
        self.validator = Some(Validator(std::sync::Arc::new(check)));
        self
    }

    /// Choices others may join where skwir runs, said in a few words
    /// ("themes you added to bat"): see `SettingConstraints::more_choices`.
    #[must_use]
    pub fn more_choices(mut self, which: &str) -> Self {
        self.constraints.more_choices = which.into();
        self
    }

    #[must_use]
    pub fn section(mut self, section: &str) -> Self {
        section.clone_into(&mut self.section);
        self
    }

    /// Shown on `page` (one of [`page`]), by its subject, rather than on
    /// its owner's own page.
    #[must_use]
    pub fn page(mut self, page: &str) -> Self {
        page.clone_into(&mut self.page);
        self
    }

    /// Among the few settings shown while its page is folded (ADR-072):
    /// people are divided on its value, it decides what other programs or
    /// machines see, or it acts on files without asking. A budget keeps
    /// them few (`crates/skwir/tests/settings.rs`).
    #[must_use]
    pub fn essential(mut self) -> Self {
        self.essential = true;
        self
    }

    /// Shown under the switch `parent`, applying only while it is on (the
    /// desktop's font under the desktop's colors). Declare it right after
    /// the switch, in the same section.
    #[must_use]
    pub fn under(mut self, parent: &str) -> Self {
        parent.clone_into(&mut self.under);
        self
    }

    #[must_use]
    pub fn unit(mut self, unit: &str) -> Self {
        unit.clone_into(&mut self.unit);
        self
    }

    #[must_use]
    pub fn step(mut self, step: i64) -> Self {
        self.constraints.step = step.max(1);
        self
    }

    #[must_use]
    pub fn restart_required(mut self) -> Self {
        self.restart_required = true;
        self
    }

    /// The settings namespace, which is the owning plugin's id (or `core`/`ui`).
    pub fn owner(&self) -> &str {
        owner_of(&self.key)
    }

    /// The label of a choice value, falling back to the value itself.
    pub fn choice_label<'a>(&'a self, value: &'a str) -> &'a str {
        self.constraints
            .choices
            .iter()
            .position(|choice| choice == value)
            .and_then(|index| self.constraints.choice_labels.get(index))
            .map_or(value, String::as_str)
    }

    /// Collections are application data (e.g. saved queries), managed by their
    /// owning commands rather than the preferences editor.
    pub fn editable(&self) -> bool {
        self.kind != SettingKind::Data
    }

    /// The text an editor shows and sends back: a string as itself, a list
    /// or a table (records) as JSON, anything else as TOML writes it.
    pub fn format_value(value: &toml::Value) -> String {
        match value {
            toml::Value::String(text) => text.clone(),
            toml::Value::Array(_) | toml::Value::Table(_) => {
                serde_json::to_string(value).unwrap_or_default()
            }
            _ => value.to_string(),
        }
    }

    /// Records from their JSON text, each checked against the fields. The
    /// dialog edits a record by its id, which each needs there; a file's
    /// (`Beside::File`, Home Manager's) may leave them out, as what reads
    /// the records does (shortcuts, private folders).
    fn parse_records(&self, text: &str, beside: Beside<'_>) -> anyhow::Result<toml::Value> {
        let records: Vec<serde_json::Map<String, serde_json::Value>> =
            serde_json::from_str(if text.trim().is_empty() { "[]" } else { text })
                .map_err(|error| anyhow::anyhow!("expected a list of records: {error}"))?;
        let mut ids = std::collections::BTreeSet::new();
        let mut out = Vec::with_capacity(records.len());
        for record in records {
            let id = record
                .get("id")
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty());
            anyhow::ensure!(
                id.is_some() || matches!(beside, Beside::File),
                "every record needs an id"
            );
            let mut table = toml::Table::new();
            if let Some(id) = id {
                anyhow::ensure!(ids.insert(id.to_owned()), "two records share the id {id}");
                table.insert("id".into(), toml::Value::String(id.into()));
            }
            for field in &self.constraints.fields {
                let value = record.get(&field.key).filter(|v| !v.is_null());
                let empty = match value {
                    None => true,
                    Some(serde_json::Value::String(s)) => s.trim().is_empty(),
                    Some(serde_json::Value::Array(a)) => a.is_empty(),
                    Some(_) => false,
                };
                anyhow::ensure!(!(field.required && empty), "{} is needed", field.title);
                let Some(value) = value.filter(|_| !empty) else {
                    continue;
                };
                let converted = match &field.kind {
                    FieldKind::Text { .. } => toml::Value::String(
                        value
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("{} is text", field.title))?
                            .trim()
                            .to_owned(),
                    ),
                    FieldKind::Integer { min, max } => {
                        let n = value
                            .as_i64()
                            .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
                            .ok_or_else(|| anyhow::anyhow!("{} is a whole number", field.title))?;
                        anyhow::ensure!(
                            (*min..=*max).contains(&n),
                            "{}: enter a whole number from {min} to {max}",
                            field.title
                        );
                        toml::Value::Integer(n)
                    }
                    FieldKind::Pairs => {
                        let pairs = value
                            .as_array()
                            .ok_or_else(|| anyhow::anyhow!("{} is a list of pairs", field.title))?;
                        let mut list = Vec::with_capacity(pairs.len());
                        for pair in pairs {
                            let [name, value] =
                                pair.as_array().map(Vec::as_slice).unwrap_or_default()
                            else {
                                anyhow::bail!("{} is a list of name and value pairs", field.title);
                            };
                            let (Some(name), Some(value)) = (name.as_str(), value.as_str()) else {
                                anyhow::bail!("{}: names and values are text", field.title);
                            };
                            if name.trim().is_empty() && value.trim().is_empty() {
                                continue;
                            }
                            anyhow::ensure!(
                                !name.trim().is_empty(),
                                "{}: a value needs a name",
                                field.title
                            );
                            list.push(toml::Value::Array(vec![
                                toml::Value::String(name.trim().into()),
                                toml::Value::String(value.trim().into()),
                            ]));
                        }
                        if list.is_empty() {
                            continue;
                        }
                        toml::Value::Array(list)
                    }
                    FieldKind::Secret => {
                        // The secret itself goes to the keyring; the settings
                        // file only says that it is there.
                        anyhow::ensure!(
                            value.as_str() == Some(SECRET_STORED),
                            "{} is kept in the keyring, not in the settings",
                            field.title
                        );
                        toml::Value::String(SECRET_STORED.into())
                    }
                };
                table.insert(field.key.clone(), converted);
            }
            out.push(toml::Value::Table(table));
        }
        Ok(toml::Value::Array(out))
    }

    /// Whether `value`, read from a file rather than typed, is one this
    /// setting takes: its kind, then whatever the dialog checks of what is
    /// typed (choices, ranges, the owner's own check). Data takes any
    /// value of its default's kind; its owner reads it.
    ///
    /// # Errors
    /// What is wrong, in the words the dialog uses.
    pub fn check(&self, value: &toml::Value) -> anyhow::Result<()> {
        let kind = |value: &toml::Value| match value {
            toml::Value::String(_) => "text",
            toml::Value::Integer(_) => "a whole number",
            toml::Value::Float(_) => "a number",
            toml::Value::Boolean(_) => "true or false",
            toml::Value::Datetime(_) => "a date",
            toml::Value::Array(_) => "a list",
            toml::Value::Table(_) => "a table",
        };
        let fits = std::mem::discriminant(value) == std::mem::discriminant(&self.default)
            || matches!(
                (&self.default, value),
                (toml::Value::Float(_), toml::Value::Integer(_))
            );
        anyhow::ensure!(
            fits,
            "expected {}, not {}",
            kind(&self.default),
            kind(value)
        );
        if !self.editable() {
            return Ok(());
        }
        self.parse_beside(&Self::format_value(value), Beside::File)
            .map(|_| ())
    }

    /// The value `text` says, checked as if nothing were stored.
    ///
    /// # Errors
    /// What is wrong, in the words the dialog uses.
    pub fn parse(&self, text: &str) -> anyhow::Result<toml::Value> {
        self.parse_over(text, None)
    }

    /// The value `text` says, to replace `stored`: the owner's check sees
    /// both (see [`Validator`]).
    ///
    /// # Errors
    /// What is wrong, in the words the dialog uses.
    pub fn parse_over(
        &self,
        text: &str,
        stored: Option<&toml::Value>,
    ) -> anyhow::Result<toml::Value> {
        self.parse_beside(text, Beside::Stored(stored))
    }

    fn parse_beside(&self, text: &str, beside: Beside<'_>) -> anyhow::Result<toml::Value> {
        anyhow::ensure!(
            self.editable(),
            "{} is managed by its owning commands",
            self.key
        );
        let value = match self.default {
            _ if self.kind == SettingKind::Records => self.parse_records(text, beside)?,
            toml::Value::String(_) => toml::Value::String(text.into()),
            toml::Value::Boolean(_) => toml::Value::Boolean(
                text.trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("expected true or false"))?,
            ),
            toml::Value::Integer(_) => toml::Value::Integer(
                text.trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("expected a whole number"))?,
            ),
            toml::Value::Float(_) => {
                let number: f64 = text
                    .trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("expected a number"))?;
                anyhow::ensure!(number.is_finite(), "expected a finite number");
                toml::Value::Float(number)
            }
            _ => anyhow::bail!("{} is managed by its owning commands", self.key),
        };
        anyhow::ensure!(
            self.constraints.choices.is_empty()
                || self
                    .constraints
                    .choices
                    .iter()
                    .any(|s| Some(s.as_str()) == value.as_str()),
            "choose one of {}",
            self.constraints.choices.join(", ")
        );
        if let Some((min, max)) = self.constraints.integer_range {
            anyhow::ensure!(
                value.as_integer().is_some_and(|n| (min..=max).contains(&n)),
                "enter a whole number from {min} to {max}"
            );
        }
        if let Some(Validator(check)) = &self.validator {
            check(&value, beside)?;
        }
        Ok(value)
    }
}

/// What `Settings::check` found: problems, by key, and notes on keys it
/// cannot judge.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Checked {
    pub problems: Vec<(String, String)>,
    pub notes: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SaveState {
    #[default]
    Idle,
    Saving,
    Saved,
    Failed(String),
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CheckArgs {
    /// A settings file, by absolute path.
    #[serde(default)]
    file: Option<PathArg>,
    /// Settings as a table (JSON), in place of a file.
    #[serde(default)]
    settings: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ApplyArgs {
    /// Dotted setting keys and their editor text (strings need no TOML quotes).
    changes: BTreeMap<String, String>,
}

/// Register the nonblocking settings command. Validation is repeated on the
/// worker in case a plugin was disabled after the command was queued.
pub(crate) fn register(app: &crate::App) {
    app.settings.declare(
        SettingSpec::choice(
            STARTUP,
            "Open at startup",
            "Where a new window starts when no folder is given on the command line.",
            "folder",
            &[
                ("folder", "A chosen folder"),
                ("last", "The folders and tabs left open"),
            ],
        )
        .section("Startup")
        .essential(),
    );
    app.settings.declare(
        SettingSpec::path(
            STARTUP_FOLDER,
            "Startup folder",
            "Used when a chosen folder opens at startup. Leave empty for your home folder.",
            "",
        )
        .section("Startup")
        .essential(),
    );
    app.settings.declare(
        SettingSpec::toggle(
            PRESERVE_ATTRIBUTES_ON_COPY,
            "Keep tags and attributes when copying",
            "Copies carry their source's tags, rating, comment and where it was downloaded from. Where the destination keeps no attributes they are kept beside the copy, as a Mac does; the copy fails only when nothing can keep them. Turn this off to copy without them.",
            true,
        )
        .page(page::NOTES)
        .section("Tags"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            DROP_ASKS,
            "Ask what dropped files do",
            "Files dropped in a folder with no key held open a menu: move, copy or link them there. Turn this off to move them within a drive and copy them to another. Shift moves, Ctrl copies and Ctrl+Shift links, either way.",
            true,
        )
        .section("Drag and drop")
        .essential(),
    );
    app.settings.declare(
        SettingSpec::toggle(
            crate::companions::SHOW,
            "Show companion files",
            "With hidden files shown, also list the files that keep another file's notes beside it: a Mac's ._ files, XMP and TagSpaces sidecars. Properties lists a file's companions either way.",
            false,
        )
        .page(page::NOTES)
        .section("Tags"),
    );
    app.companions
        .set_shown(app.settings.get(crate::companions::SHOW).unwrap_or(false));
    app.settings.declare(
        SettingSpec::choice(
            CLOSE_SPLIT,
            "Pane that closes",
            "Which pane goes when you close the split with Split or F3. Other keeps the pane you are in. Active closes it and keeps the other. Right always closes the right pane, or the lower one when the panes are stacked.",
            "other",
            &[("other", "Other"), ("active", "Active"), ("right", "Right")],
        )
        .section("Split"),
    );
    app.settings.declare(SettingSpec::state(
        LAST_LOCATION,
        "The folder shown when the window was last closed",
        toml::Value::String(String::new()),
    ));
    app.commands
        .register::<CheckArgs, _>(
            "core", Topic::Skwir,
            "settings.check",
            "Check settings",
            "Check a settings file, or settings given as a table, against the settings this skwir declares: fails with what is wrong (a value a setting does not take, a key no setting has), and notes the keys of owners it does not know",
            |args, app| {
                let table: toml::Table = match (&args.file, &args.settings) {
                    (Some(file), None) => {
                        let path = crate::os_path::absolute(file)?;
                        // Whatever an agent names: a pipe or a device is no
                        // settings file, nor is one larger than any.
                        let text = crate::fs::file::read_regular_text(path, FILE_LIMIT).map_err(|error| {
                            anyhow::anyhow!(
                                "“{}” cannot be read: {}",
                                path.display(),
                                crate::op::io_words(&error)
                            )
                        })?;
                        text.parse()
                            .map_err(|error: toml::de::Error| {
                                anyhow::anyhow!("“{}” is not valid: {}", path.display(), toml_problem(&text, &error))
                            })?
                    }
                    (None, Some(settings)) => toml::Table::try_from(settings)
                        .map_err(|error| anyhow::anyhow!("not settings: {error}"))?,
                    _ => anyhow::bail!("give a file or settings, one of them"),
                };
                let plugins: Vec<String> =
                    app.plugins.list().into_iter().map(|plugin| plugin.id.to_owned()).collect();
                let checked = app.settings.check(&table, &plugins);
                if !checked.problems.is_empty() {
                    let said: Vec<String> = checked
                        .problems
                        .iter()
                        .map(|(key, problem)| format!("{key}: {problem}"))
                        .collect();
                    anyhow::bail!("{}", said.join("; "));
                }
                Ok(crate::command::Outcome::Value(serde_json::to_value(&checked)?))
            },
        )
        .data();
    app.commands.register::<ApplyArgs, _>(
        "core", Topic::Skwir,
        "settings.apply",
        "Save settings",
        "Change preferences, each key with the text its row in Settings takes; all of them are saved, or none",
        |args, app| {
            if !crate::scheduler::has_window()
                && let Some(reason) = &app.settings.saving_unavailable
            {
                // An interactive session can warn and keep temporary values.
                // A headless command cannot claim it saved those values.
                anyhow::bail!("{reason}");
            }
            app.settings.validate(&args.changes)?;
            let mut status = app.settings.status.lock();
            anyhow::ensure!(
                status.1 != SaveState::Saving,
                "settings are already being saved"
            );
            status.0 += 1;
            status.1 = SaveState::Saving;
            let id = status.0;
            drop(status);
            // The guard owns the Saving state: whether the job runs, panics, or
            // is dropped unexecuted (cancelled lifetime), the state machine
            // always leaves Saving, so a later apply is never rejected forever.
            let mut guard = SaveGuard {
                weak: app.weak(),
                id,
                state: None,
            };
            let save = move || -> anyhow::Result<()> {
                let app = guard.weak.upgrade()
                    .ok_or_else(|| anyhow::anyhow!("settings save did not run"))?;
                let result = app.settings.apply(&args.changes).map(|keys| {
                    changed(&app, &keys);
                });
                guard.state = Some(match &result {
                    Ok(()) => SaveState::Saved,
                    Err(error) => SaveState::Failed(format!("{error:#}")),
                });
                drop(guard);
                result
            };
            if crate::scheduler::has_window() {
                // The window follows SaveState, including saves requested by
                // its control connection. Plugin callbacks can stop that
                // connection, so it must not wait for them on its own runtime.
                app.scheduler.spawn(
                    crate::scheduler::Priority::Normal,
                    "settings-save",
                    app.lifetime().child(),
                    move || { let _ = save(); },
                );
            } else {
                // A headless caller has no event loop to observe completion
                // and may exit as soon as dispatch returns. Save before
                // answering, and report persistence failures to that caller.
                save()?;
            }
            Ok(crate::command::Outcome::Value(serde_json::json!(id)))
        },
    );
}

/// What follows a change of `keys`, made in the dialog or found in the
/// file (`Settings::reload`): what the core keeps of them is set again, a
/// plugin turned on or off in the file is, and the plugins hear of theirs.
/// On a worker.
pub(crate) fn changed(app: &crate::App, keys: &[String]) {
    if keys.iter().any(|key| key == crate::tabs::SLEEP_KEY) {
        app.apply_tabs_sleep();
    }
    if keys
        .iter()
        .any(|key| owner_of(key) == crate::notes::SETTINGS)
    {
        app.apply_notes_settings();
    }
    if keys.iter().any(|key| key == crate::companions::SHOW) {
        app.companions
            .set_shown(app.settings.get(crate::companions::SHOW).unwrap_or(false));
        for pane in app.workspace.write().panes_mut() {
            pane.companions_changed();
        }
        app.wake();
    }
    for key in keys {
        if let Some(id) = key
            .strip_prefix("plugins.")
            .and_then(|rest| rest.strip_suffix(".enabled"))
            && let Some(enabled) = app.settings.get::<bool>(key)
            && app.plugins.is_enabled(id) != enabled
        {
            app.plugins.set_enabled(id, enabled, app);
        }
    }
    app.plugins.settings_changed(keys, app);
}

/// Take in what changed in the settings file and the data file, then
/// follow them: the folders holding them watched until the app ends. On a
/// worker: a folder is made if missing, and watching may wait for its
/// drive.
pub(crate) fn follow(app: &std::sync::Arc<crate::App>) {
    if app.settings.path().is_none() {
        return;
    }
    // The files followed, by folder: one watch where they share one.
    let mut folders: BTreeMap<PathBuf, Vec<std::ffi::OsString>> = BTreeMap::new();
    for path in [app.settings.path(), app.settings.data_path()]
        .into_iter()
        .flatten()
    {
        if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
            folders
                .entry(dir.to_path_buf())
                .or_default()
                .push(name.to_os_string());
        }
    }
    for (dir, names) in folders {
        if let Err(error) = std::fs::create_dir_all(&dir) {
            tracing::warn!(%error, "a folder of the settings cannot be made: not followed");
            continue;
        }
        let (scheduler, weak, cancel) = (app.scheduler.clone(), app.weak(), app.lifetime().child());
        let reloading = cancel.clone();
        let watched = crate::fs::watch::watch(
            &dir,
            std::time::Duration::from_millis(200),
            &cancel,
            move |changes| {
                if !changes.rescan && !names.iter().any(|name| changes.names.contains(name)) {
                    return;
                }
                let weak = weak.clone();
                // Read on a worker, never on the watcher's thread.
                scheduler.spawn(
                    crate::scheduler::Priority::Background,
                    "settings-reload",
                    reloading.clone(),
                    move || {
                        let Some(app) = weak.upgrade() else { return };
                        if let Some(keys) = app.settings.reload()
                            && !keys.is_empty()
                        {
                            changed(&app, &keys);
                            app.wake();
                        }
                    },
                );
            },
        );
        match watched {
            Ok(handle) => app.settings.followed.lock().push(handle),
            Err(error) => tracing::warn!(%error, "a file of the settings cannot be followed"),
        }
    }
}

/// Publishes the outcome of one save, or a failure if the job never completed.
struct SaveGuard {
    weak: std::sync::Weak<crate::App>,
    id: u64,
    state: Option<SaveState>,
}

impl Drop for SaveGuard {
    fn drop(&mut self) {
        let Some(app) = self.weak.upgrade() else {
            return;
        };
        let state = self
            .state
            .take()
            .unwrap_or_else(|| SaveState::Failed("settings save did not run".into()));
        let mut status = app.settings.status.lock();
        if status.0 == self.id {
            status.1 = state;
        }
        drop(status);
        app.wake();
    }
}

/// Where a window opens when the command line names no folder: the last
/// folder or the chosen one, per `core.startup`, falling back to `home`.
/// The chosen one is typed: `~` is `home`, as in the address field.
pub fn startup_folder(settings: &Settings, home: &Path) -> PathBuf {
    let stored = |key: &str| {
        settings
            .get::<PathArg>(key)
            .map(PathArg::into_inner)
            .filter(|path| !path.as_os_str().is_empty())
    };
    let typed = |path: PathBuf| match path.strip_prefix("~") {
        Ok(rest) if rest.as_os_str().is_empty() => home.to_path_buf(),
        Ok(rest) => home.join(rest),
        Err(_) => path,
    };
    let choice = match settings.get::<String>(STARTUP).as_deref() {
        Some("last") => stored(LAST_LOCATION),
        _ => stored(STARTUP_FOLDER).map(typed),
    };
    choice.unwrap_or_else(|| home.to_path_buf())
}

/// Record `folder` as the one to reopen, when the startup preference
/// says to come back to it (`tabs::remember`, at exit). Whether it changed;
/// the caller saves.
pub fn remember_location(settings: &Settings, folder: &Path) -> bool {
    if settings.get::<String>(STARTUP).as_deref() != Some("last") {
        return false;
    }
    // A string when the path is UTF-8, its bytes otherwise (`os_path`): a
    // folder whose name is not UTF-8 reopens as itself.
    if settings.get::<PathArg>(LAST_LOCATION).as_deref() == Some(folder) {
        return false;
    }
    let Ok(value) = toml::Value::try_from(PathArg::from(folder)) else {
        return false;
    };
    settings.set(LAST_LOCATION, value);
    true
}

/// Loaded settings plus declared schema.
pub struct Settings {
    path: Option<PathBuf>,
    /// The files kept apart from the settings (`with_state`, `with_data`):
    /// the keys declared for one are kept there, or saved nowhere when it
    /// is `None` (a data file that could neither be read nor set aside). A
    /// store not here keeps its keys in the settings file, with the rest.
    apart: BTreeMap<Store, Option<PathBuf>>,
    /// What each file kept apart held when last read or written: a key
    /// declared kept apart takes its value from there whole (`declare`),
    /// over a copy the settings file may still hold (an earlier skwir's, a
    /// hand's), which is read only until skwir keeps its own.
    apart_read: Mutex<BTreeMap<Store, toml::Table>>,
    /// The keys ever declared kept apart in this process, and where: a
    /// plugin turned off keeps them where they were.
    kept: RwLock<BTreeMap<String, Store>>,
    values: RwLock<toml::Table>,
    schema: RwLock<BTreeMap<String, SettingSpec>>,
    /// Every key declared in this process, by when it first was
    /// (`SettingSpec::order`): declared again, or by a plugin turned on
    /// again, a setting keeps its place.
    declared: Mutex<BTreeMap<String, u64>>,
    /// Serializes writers (readers never wait for file I/O), and holds the
    /// keys this process changed since its last save: what a save writes
    /// over the file as it stands.
    writer: Mutex<BTreeSet<String>>,
    /// The watches on the settings' and the data's folders (`follow`),
    /// while the app runs.
    followed: Mutex<Vec<crate::fs::watch::WatchHandle>>,
    status: Mutex<(u64, SaveState)>,
    /// Advances when the schema or a live value changes, so editors compare
    /// one atomic instead of re-reading the schema on every frame.
    revision: std::sync::atomic::AtomicU64,
    /// Why the file could not be read, until the app says it
    /// (`take_problem`).
    problem: Mutex<Option<String>>,
    /// A file-backed instance fell back to memory after a load failure.
    /// Kept even after the startup warning is consumed; unlike an explicit
    /// in-memory instance, headless preference changes cannot claim a save.
    saving_unavailable: Option<String>,
}

/// A TOML error in a few words, with its line: "expected `=` on line 3".
fn toml_problem(text: &str, error: &toml::de::Error) -> String {
    let message = error.message().trim_end_matches('.');
    match error.span() {
        Some(span) => {
            let line = text
                .get(..span.start)
                .map_or(1, |before| before.matches('\n').count() + 1);
            format!("{message} on line {line}")
        }
        None => message.to_owned(),
    }
}

/// Move an unreadable settings file out of the way, beside itself, under
/// a name no other one has: `settings.toml.unreadable-<seconds>`.
fn set_aside(path: &Path) -> std::io::Result<PathBuf> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".unreadable-{seconds}"));
    let aside = path.with_file_name(name);
    std::fs::rename(path, &aside)?;
    Ok(aside)
}

impl Settings {
    /// The system's settings file of skwir's
    /// (`$XDG_CONFIG_HOME/skwir/settings.toml` on Linux): in
    /// `crate::folders::config`.
    pub fn default_path() -> PathBuf {
        crate::folders::config()
            .unwrap_or_else(|| std::env::temp_dir().join("skwir"))
            .join("settings.toml")
    }

    /// The system's file of skwir's state (`$XDG_STATE_HOME/skwir/state.toml`
    /// on Linux): in `crate::folders::state`.
    pub fn default_state_path() -> Option<PathBuf> {
        crate::folders::state().map(|dir| dir.join("state.toml"))
    }

    /// In-memory settings with no file.
    pub fn in_memory() -> Self {
        Settings {
            path: None,
            apart: BTreeMap::new(),
            apart_read: Mutex::new(BTreeMap::new()),
            kept: RwLock::new(BTreeMap::new()),
            values: RwLock::new(toml::Table::new()),
            schema: RwLock::new(BTreeMap::new()),
            declared: Mutex::new(BTreeMap::new()),
            writer: Mutex::new(BTreeSet::new()),
            followed: Mutex::new(Vec::new()),
            status: Mutex::new((0, SaveState::Idle)),
            revision: std::sync::atomic::AtomicU64::new(0),
            problem: Mutex::new(None),
            saving_unavailable: None,
        }
    }

    /// Load from `path`; a missing file yields empty settings. skwir always
    /// starts: a file that is no TOML (edited by hand, say) is set aside
    /// under a name of its own, never written over, and the defaults are
    /// used; one that cannot be read or set aside leaves the settings in
    /// memory, saved nowhere. `take_problem` says which.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let (values, problem, saves) =
            Self::read_or_set_aside(&path, "Your settings", "with its defaults");
        Settings {
            path: saves.then_some(path),
            apart: BTreeMap::new(),
            apart_read: Mutex::new(BTreeMap::new()),
            kept: RwLock::new(BTreeMap::new()),
            values: RwLock::new(values),
            schema: RwLock::new(BTreeMap::new()),
            declared: Mutex::new(BTreeMap::new()),
            writer: Mutex::new(BTreeSet::new()),
            followed: Mutex::new(Vec::new()),
            status: Mutex::new((0, SaveState::Idle)),
            revision: std::sync::atomic::AtomicU64::new(0),
            saving_unavailable: (!saves).then(|| problem.clone()).flatten(),
            problem: Mutex::new(problem),
        }
    }

    /// The table in the file at `path` (empty when there is none), and
    /// what to tell the user when it could not be read: one that is no TOML
    /// set aside beside itself, never written over. False when nothing may
    /// be saved there: it could neither be read nor set aside. `whose` and
    /// `without` word the problem: "Your settings could not be read (…), so
    /// skwir started with its defaults."
    fn read_or_set_aside(
        path: &Path,
        whose: &str,
        without: &str,
    ) -> (toml::Table, Option<String>, bool) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (toml::Table::new(), None, true);
            }
            Err(error) => {
                let problem = format!(
                    "{whose} could not be read ({}), so skwir started {without}, and saves nothing \
                     there this time.",
                    crate::op::io_words(&error)
                );
                return (toml::Table::new(), Some(problem), false);
            }
        };
        let parsed = std::str::from_utf8(&bytes)
            .map_err(|_| "it is not text".to_owned())
            .and_then(|text| {
                text.parse::<toml::Table>()
                    .map_err(|e| toml_problem(text, &e))
            });
        let why = match parsed {
            Ok(values) => return (values, None, true),
            Err(why) => why,
        };
        tracing::warn!(path = %path.display(), "unreadable: {why}");
        match set_aside(path) {
            Ok(aside) => {
                let problem = format!(
                    "{whose} could not be read ({why}), so skwir started {without}. The file was \
                     set aside as “{}”.",
                    aside.file_name().unwrap_or_default().display()
                );
                (toml::Table::new(), Some(problem), true)
            }
            Err(error) => {
                let problem = format!(
                    "{whose} could not be read ({why}), so skwir started {without}, and saves \
                     nothing there until “{}” is fixed or removed ({}).",
                    path.display(),
                    crate::op::io_words(&error)
                );
                (toml::Table::new(), Some(problem), false)
            }
        }
    }

    /// The same settings, their state kept in the file at `path` (made at
    /// the first save): a key it holds is read from it, whole, over a copy
    /// the settings file may still have (`declare`). One that cannot be
    /// read is started over: it is skwir's own, and holds nothing the user
    /// chose.
    #[must_use]
    pub fn with_state(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        match Self::on_disk(&path) {
            Ok(state) => {
                self.apart_read
                    .get_mut()
                    .insert(Store::State, state.clone());
                Self::merge_into(&mut self.values.write(), state);
            }
            Err(error) => tracing::warn!("the window's state is started over: {error:#}"),
        }
        self.apart.insert(Store::State, Some(path));
        self
    }

    /// The same settings, what the user made in skwir (`SettingSpec::data`:
    /// saved searches, pinned tags, tags' colours) kept in the file at
    /// `path` (ADR-057), made at the first save: a key it holds is read
    /// from it, whole, over a copy the settings file may still have
    /// (`declare`). It is the user's: one that is no TOML is set aside, as
    /// the settings file is, and said; one that can be neither read nor set
    /// aside is never written, what changes kept in memory until skwir
    /// starts again.
    #[must_use]
    pub fn with_data(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let (data, problem, saves) = Self::read_or_set_aside(
            &path,
            "Your saved searches, pinned tags and tags’ colours",
            "without them",
        );
        self.apart_read.get_mut().insert(Store::Data, data.clone());
        Self::merge_into(&mut self.values.write(), data);
        if let Some(problem) = problem {
            let mut said = self.problem.lock();
            *said = Some(match said.take() {
                Some(before) => format!("{before} {problem}"),
                None => problem,
            });
        }
        self.apart.insert(Store::Data, saves.then_some(path));
        self
    }

    /// `other`'s values set in `base`, tables merged.
    fn merge_into(base: &mut toml::Table, other: toml::Table) {
        for (key, value) in other {
            match (base.get_mut(&key), value) {
                (Some(toml::Value::Table(inner)), toml::Value::Table(more)) => {
                    Self::merge_into(inner, more);
                }
                (_, value) => {
                    base.insert(key, value);
                }
            }
        }
    }

    /// The state file, where there is one.
    pub fn state_path(&self) -> Option<&Path> {
        self.apart.get(&Store::State)?.as_deref()
    }

    /// The data file, where there is one.
    pub fn data_path(&self) -> Option<&Path> {
        self.apart.get(&Store::Data)?.as_deref()
    }

    /// The system's file of what the user made in skwir
    /// (`$XDG_DATA_HOME/skwir/data.toml` on Linux): in `crate::folders::data`.
    pub fn default_data_path() -> Option<PathBuf> {
        crate::folders::data().map(|dir| dir.join("data.toml"))
    }

    /// The file apart from the settings `key` is kept in: a key declared
    /// for one, or one within it, where that store has a file. `None`: the
    /// settings file.
    fn apart_of(&self, kept: &BTreeMap<String, Store>, key: &str) -> Option<Store> {
        kept.iter()
            .find(|(declared, _)| {
                key == declared.as_str()
                    || key
                        .strip_prefix(declared.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            })
            .map(|(_, &store)| store)
            .filter(|store| self.apart.contains_key(store))
    }

    /// The keys kept in files apart from the settings, and which.
    fn kept_apart(&self) -> Vec<(String, Store)> {
        self.kept
            .read()
            .iter()
            .filter(|(_, store)| self.apart.contains_key(store))
            .map(|(key, &store)| (key.clone(), store))
            .collect()
    }

    /// Why the file was not read, once: the app says it to the user.
    pub fn take_problem(&self) -> Option<String> {
        self.problem.lock().take()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Whether what changes in the file is taken in as it changes
    /// (`follow`): the watch is set up on a worker once the app starts.
    pub fn is_followed(&self) -> bool {
        !self.followed.lock().is_empty()
    }

    /// Declare a setting. Registered by plugins and the core.
    pub fn declare(&self, mut spec: SettingSpec) {
        if spec.store != Store::Settings {
            self.kept.write().insert(spec.key.clone(), spec.store);
            self.read_apart(&spec.key, spec.store);
        }
        {
            let mut declared = self.declared.lock();
            let next = declared.len() as u64;
            spec.order = *declared.entry(spec.key.clone()).or_insert(next);
        }
        self.schema.write().insert(spec.key.clone(), spec);
        self.bump();
    }

    /// Carry a value stored under a key's former name to its new one, when
    /// nothing is stored under the new name yet. The owner calls it before
    /// declaring the new key, so a moved preference keeps what the user
    /// chose; the file is rewritten at the next save, and until then the move
    /// is made again at each start, to the same effect. Nothing calls it
    /// while no one uses skwir (keys are renamed outright): it is for the
    /// first rename after people do.
    pub fn rename(&self, old: &str, new: &str) {
        let mut pending = self.writer.lock();
        let mut values = self.values.write();
        if Self::lookup(&values, new).is_some() {
            return;
        }
        let Some(value) = Self::remove(&mut values, old) else {
            return;
        };
        if let Err(error) = Self::insert(&mut values, new, value) {
            tracing::warn!(%error, "cannot move a setting");
        }
        pending.extend([old.to_owned(), new.to_owned()]);
        drop(values);
        self.bump();
    }

    /// A key kept apart, as the file it is kept in holds it: whole, in
    /// place of a copy the settings file still has (an earlier skwir's, a
    /// hand's, Home Manager's), which is read only while that file has none.
    /// What this process changed and has not saved yet stays.
    fn read_apart(&self, key: &str, store: Store) {
        let pending = self.writer.lock();
        if pending.contains(key) {
            return;
        }
        let Some(value) = self
            .apart_read
            .lock()
            .get(&store)
            .and_then(|table| Self::lookup(table, key))
            .cloned()
        else {
            return;
        };
        let mut values = self.values.write();
        if Self::lookup(&values, key) == Some(&value) {
            return;
        }
        Self::remove(&mut values, key);
        if let Err(error) = Self::insert(&mut values, key, value) {
            tracing::warn!(%error, "cannot read a setting kept apart");
        }
        drop(values);
        drop(pending);
        self.bump();
    }

    /// Each key kept apart that `apart` holds, set in `table` whole: a copy
    /// `table` has (the settings file's) is never merged with it.
    fn replace_apart(
        table: &mut toml::Table,
        apart: &toml::Table,
        kept: impl IntoIterator<Item = String>,
    ) {
        for key in kept {
            if let Some(value) = Self::lookup(apart, &key) {
                Self::remove(table, &key);
                if let Err(error) = Self::insert(table, &key, value.clone()) {
                    tracing::warn!(%error, "cannot read a setting kept apart");
                }
            }
        }
    }

    /// Take a dotted key out, and the tables it leaves empty.
    fn remove(table: &mut toml::Table, key: &str) -> Option<toml::Value> {
        match key.split_once('.') {
            None => table.remove(key),
            Some((head, rest)) => {
                let inner = table.get_mut(head)?.as_table_mut()?;
                let value = Self::remove(inner, rest)?;
                if inner.is_empty() {
                    table.remove(head);
                }
                Some(value)
            }
        }
    }

    pub fn undeclare(&self, key: &str) {
        self.schema.write().remove(key);
        self.bump();
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }

    fn bump(&self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    pub fn spec(&self, key: &str) -> Option<SettingSpec> {
        self.schema.read().get(key).cloned()
    }

    pub fn schema(&self) -> Vec<SettingSpec> {
        self.schema.read().values().cloned().collect()
    }

    fn lookup<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
        let mut parts = key.split('.');
        let mut value = table.get(parts.next()?)?;
        for part in parts {
            value = value.as_table()?.get(part)?;
        }
        Some(value)
    }

    /// Typed read: the stored value, else the declared default, else `None`.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let values = self.values.read();
        let v = Self::lookup(&values, key)
            .cloned()
            .or_else(|| self.schema.read().get(key).map(|s| s.default.clone()))?;
        v.try_into().ok()
    }

    /// The live value as an editor shows it (strings without TOML quotes).
    pub fn text(&self, key: &str) -> Option<String> {
        self.get::<toml::Value>(key)
            .map(|value| SettingSpec::format_value(&value))
    }

    fn insert(table: &mut toml::Table, key: &str, value: toml::Value) -> anyhow::Result<()> {
        let parts: Vec<_> = key.split('.').collect();
        anyhow::ensure!(parts.iter().all(|p| !p.is_empty()), "invalid setting key");
        let mut table = table;
        for part in &parts[..parts.len() - 1] {
            table = table
                .entry((*part).to_owned())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .ok_or_else(|| anyhow::anyhow!("{key}: parent is not a table"))?;
        }
        table.insert(parts[parts.len() - 1].to_owned(), value);
        Ok(())
    }

    /// Set a dotted key, creating intermediate tables. Writers run off the UI
    /// thread; use `settings.apply` for user preference changes.
    pub fn set(&self, key: &str, value: toml::Value) {
        let mut pending = self.writer.lock();
        match Self::insert(&mut self.values.write(), key, value) {
            Ok(()) => {
                pending.insert(key.to_owned());
            }
            Err(error) => tracing::warn!(%error, "cannot set setting"),
        }
        self.bump();
    }

    /// What is wrong in `table`, a settings file's or one to be (Home
    /// Manager's declared settings): each value checked against its
    /// setting (`SettingSpec::check`), a plugin's `enabled` against the
    /// plugins `plugins` names. A key no setting has, under an owner that
    /// declares settings (a typo, most likely), is a problem; one under an
    /// owner nothing here declares (a plugin turned off, or from outside)
    /// is only noted. A key kept apart from the settings (the window's
    /// state, the user's data) is noted: read only until skwir keeps its
    /// own.
    pub fn check(&self, table: &toml::Table, plugins: &[String]) -> Checked {
        let schema = self.schema.read();
        let owners: BTreeSet<&str> = schema.values().map(SettingSpec::owner).collect();
        let mut checked = Checked::default();
        let mut walk = vec![(String::new(), table)];
        while let Some((prefix, table)) = walk.pop() {
            for (name, value) in table {
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                if let Some(spec) = schema.get(&key) {
                    // Kept apart, it is read from the settings only until
                    // skwir keeps its own, which then replaces it whole: a
                    // note, never a problem the file would carry forever.
                    let apart = match spec.store {
                        Store::Settings => None,
                        Store::State => Some(("the window's state", "state file")),
                        Store::Data => Some((
                            "what you make in skwir (saved searches, pinned tags, tags' colours)",
                            "data file",
                        )),
                    };
                    if let Some((what, file)) = apart {
                        let own = self
                            .apart_read
                            .lock()
                            .get(&spec.store)
                            .is_some_and(|kept| Self::lookup(kept, &key).is_some());
                        let note = if own {
                            format!(
                                "{what}, not a setting: skwir reads its own, in its {file}, \
                                 never this one"
                            )
                        } else {
                            format!(
                                "{what}, not a setting: read from here only until skwir keeps \
                                 its own, in its {file}"
                            )
                        };
                        checked.notes.push((key, note));
                        continue;
                    }
                    let other = !spec.constraints.more_choices.is_empty()
                        && value.as_str().is_some_and(|value| {
                            !spec
                                .constraints
                                .choices
                                .iter()
                                .any(|choice| choice == value)
                        });
                    if other {
                        checked.notes.push((
                            key,
                            format!(
                                "not one skwir knows here: it may be among {}",
                                spec.constraints.more_choices
                            ),
                        ));
                    } else if let Err(error) = spec.check(value) {
                        checked.problems.push((key, error.to_string()));
                    }
                    continue;
                }
                if let Some(id) = key
                    .strip_prefix("plugins.")
                    .filter(|id| !id.contains('.'))
                    .map(str::to_owned)
                {
                    match value.as_table() {
                        Some(plugin) if plugins.contains(&id) => {
                            for (field, value) in plugin {
                                match (field.as_str(), value) {
                                    ("enabled", toml::Value::Boolean(_)) => {}
                                    ("enabled", _) => checked
                                        .problems
                                        .push((format!("{key}.enabled"), "expected true or false".into())),
                                    _ => checked
                                        .problems
                                        .push((format!("{key}.{field}"), "no such setting".into())),
                                }
                            }
                        }
                        Some(_) => checked.notes.push((
                            key,
                            format!("no plugin “{id}” here: one from outside, or not built in, may take it"),
                        )),
                        None => checked.problems.push((key, "expected a table: enabled = true or false".into())),
                    }
                    continue;
                }
                let within = format!("{key}.");
                if let Some(inner) = value.as_table()
                    && (key == "plugins"
                        || schema.keys().any(|declared| declared.starts_with(&within)))
                {
                    walk.push((key, inner));
                    continue;
                }
                if owners.contains(owner_of(&key)) {
                    checked.problems.push((key, "no such setting".into()));
                } else {
                    checked.notes.push((
                        key.clone(),
                        format!(
                            "nothing here declares “{}”: a plugin turned off, or from outside, may",
                            owner_of(&key)
                        ),
                    ));
                }
            }
        }
        checked.problems.sort();
        checked.notes.sort();
        checked
    }

    pub fn save_status(&self) -> (u64, SaveState) {
        self.status.lock().clone()
    }

    pub fn validate(
        &self,
        changes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, toml::Value>> {
        use anyhow::Context as _;
        // What each holds now, read before the schema's lock is taken.
        let stored: BTreeMap<&String, Option<toml::Value>> = {
            let values = self.values.read();
            changes
                .keys()
                .map(|key| (key, Self::lookup(&values, key).cloned()))
                .collect()
        };
        let schema = self.schema.read();
        changes
            .iter()
            .map(|(key, text)| {
                let spec = schema
                    .get(key)
                    .ok_or_else(|| anyhow::anyhow!("unknown setting: {key}"))?;
                let value = spec
                    .parse_over(text, stored.get(key).and_then(Option::as_ref))
                    .with_context(|| key.clone())?;
                Ok((key.clone(), value))
            })
            .collect()
    }

    /// The file as it stands: empty when there is none.
    ///
    /// # Errors
    /// When it cannot be read, or is no TOML (a hand edit under way).
    fn on_disk(path: &Path) -> anyhow::Result<toml::Table> {
        let bytes = match crate::fs::file::read_regular(path, FILE_LIMIT) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(toml::Table::new()),
            Err(e) => anyhow::bail!(
                "“{}” could not be read: {}",
                path.display(),
                crate::op::io_words(&e)
            ),
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| anyhow::anyhow!("“{}” is not text", path.display()))?;
        text.parse::<toml::Table>().map_err(|e| {
            anyhow::anyhow!(
                "“{}” is not valid ({}): nothing is saved over it until it is fixed",
                path.display(),
                toml_problem(text, &e)
            )
        })
    }

    /// `base` with the `keys` of `values` over it: those `values` holds
    /// set, the others taken out.
    fn overlay(
        mut base: toml::Table,
        values: &toml::Table,
        keys: &BTreeSet<String>,
    ) -> anyhow::Result<toml::Table> {
        for key in keys {
            match Self::lookup(values, key) {
                Some(value) => Self::insert(&mut base, key, value.clone())?,
                None => {
                    Self::remove(&mut base, key);
                }
            }
        }
        Ok(base)
    }

    /// Write `table` to `path` whole, put in place at once: through a link
    /// to where it leads (a settings file kept with the user's other
    /// configuration), never over the link.
    ///
    /// # Errors
    /// When it cannot be written; a file that cannot be (a link into a
    /// store, a file made read-only) says it is read-only.
    fn write_whole(path: &Path, store: Store, table: &toml::Table) -> anyhow::Result<()> {
        let target = if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_symlink()) {
            std::fs::canonicalize(path)?
        } else {
            path.to_path_buf()
        };
        if Self::is_read_only(&target) {
            anyhow::bail!(Self::read_only_words(store, path, &target));
        }
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| match error.kind() {
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
                    anyhow::anyhow!(Self::read_only_words(store, path, &target))
                }
                _ => anyhow::Error::new(error),
            })?;
        file.write_all(toml::to_string_pretty(table)?.as_bytes())?;
        file.as_file().sync_all()?;
        crate::fs::file::take_place(file, &target)?;
        // The rename is durable only once the directory entry is
        // flushed, on Unix; Windows journals it, and opens no folder so.
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }

    /// Whether the file at `path` (where a link leads) is there and
    /// cannot be written.
    fn is_read_only(path: &Path) -> bool {
        std::fs::metadata(path).is_ok_and(|m| m.permissions().readonly())
    }

    fn read_only_words(store: Store, path: &Path, target: &Path) -> String {
        let file = store.file_words();
        if target == path {
            format!("the {file} is read-only, so the change is not kept")
        } else if store == Store::Settings {
            "the settings file is a link to one that cannot be written (Home Manager's, say), \
             so the change is not kept: change it where it is declared"
                .to_owned()
        } else {
            format!("the {file} is a link to one that cannot be written, so the change is not kept")
        }
    }

    /// Whether the settings cannot be saved because their file is
    /// read-only (a link into the Nix store, as Home Manager makes it): the
    /// dialog says so, and changes nothing.
    pub fn read_only(&self) -> bool {
        self.path.as_deref().is_some_and(Self::is_read_only)
    }

    /// Write the `keys` of `values` over the files as they stand, keeping
    /// what another process or a hand changed there: those kept apart in
    /// their files first (the state, then the user's data), the others in
    /// the settings file. A file none of whose keys changed is not touched
    /// (a read-only settings file never is by a change of the window's
    /// state or of a saved search), nor needs to be read: one that is no
    /// TOML (a hand edit under way) stops only a change of its own keys,
    /// after the others are saved. What is saved: the settings file's
    /// table with the others' (`values` itself where there is no file, or
    /// it cannot be read and none of its keys changed).
    fn persist(
        &self,
        values: &toml::Table,
        keys: &BTreeSet<String>,
    ) -> anyhow::Result<toml::Table> {
        let kept = self.kept.read().clone();
        let mut apart_keys: BTreeMap<Store, BTreeSet<String>> = BTreeMap::new();
        let mut settings = BTreeSet::new();
        for key in keys {
            match self.apart_of(&kept, key) {
                Some(store) => {
                    apart_keys.entry(store).or_default().insert(key.clone());
                }
                None => {
                    settings.insert(key.clone());
                }
            }
        }
        let mut apart = toml::Table::new();
        for (&store, path) in &self.apart {
            let (Some(keys), Some(path)) = (apart_keys.get(&store), path) else {
                // Nothing of it changed, or it is saved nowhere: as this
                // process holds it.
                for (key, _) in kept.iter().filter(|(_, s)| **s == store) {
                    if let Some(value) = Self::lookup(values, key) {
                        Self::insert(&mut apart, key, value.clone())?;
                    }
                }
                continue;
            };
            let disk = match store {
                // The user's: never written over when it cannot be read.
                Store::Data => Self::on_disk(path)?,
                // skwir's own: one that is no TOML is started over.
                _ => Self::on_disk(path).unwrap_or_default(),
            };
            let merged = Self::overlay(disk.clone(), values, keys)?;
            if merged != disk {
                Self::write_whole(path, store, &merged)?;
            }
            self.apart_read.lock().insert(store, merged.clone());
            Self::merge_into(&mut apart, merged);
        }
        let mut saved = match &self.path {
            Some(path) => match Self::on_disk(path) {
                Ok(disk) => {
                    let merged = Self::overlay(disk.clone(), values, &settings)?;
                    if merged != disk {
                        Self::write_whole(path, Store::Settings, &merged)?;
                    }
                    merged
                }
                // None of its keys changed: nothing of it is taken in
                // until it can be read again (`reload`).
                Err(error) if settings.is_empty() => {
                    tracing::debug!("{error:#}");
                    values.clone()
                }
                Err(error) => return Err(error),
            },
            None => values.clone(),
        };
        Self::merge_into(&mut saved, apart.clone());
        // A key kept apart is its file's, whole: never merged with a copy
        // the settings file still holds.
        let declared = kept
            .into_iter()
            .filter(|(_, store)| self.apart.contains_key(store))
            .map(|(key, _)| key);
        Self::replace_apart(&mut saved, &apart, declared);
        Ok(saved)
    }

    /// A transaction: failed validation or persistence leaves live values intact.
    /// Only changed keys are merged; unrelated settings survive an open editor.
    fn apply(&self, changes: &BTreeMap<String, String>) -> anyhow::Result<Vec<String>> {
        let mut pending = self.writer.lock();
        let parsed = self.validate(changes)?;
        let mut values = self.values.read().clone();
        let mut keys = Vec::new();
        for (key, value) in parsed {
            if self.get::<toml::Value>(&key).as_ref() != Some(&value) {
                Self::insert(&mut values, &key, value)?;
                keys.push(key);
            }
        }
        if !keys.is_empty() {
            let mut written = pending.clone();
            written.extend(keys.iter().cloned());
            let saved = self.persist(&values, &written)?;
            pending.clear();
            *self.values.write() = saved;
            self.bump();
        }
        Ok(keys)
    }

    /// Take in what changed in the files since this process read or saved
    /// them (another skwir, a hand edit, Home Manager): the settings file
    /// and the data file as they stand, with what this process changed and
    /// has not saved yet over them. The declared keys whose value changed,
    /// and the plugins' `enabled`; `None` when there is no settings file to
    /// follow. A settings file that cannot be read is left as it is (the
    /// next save of one of its keys says why), and the data file is taken
    /// in all the same.
    pub fn reload(&self) -> Option<Vec<String>> {
        let path = self.path.as_ref()?;
        let pending = self.writer.lock();
        let values = self.values.read().clone();
        let mut merged = match Self::on_disk(path) {
            Ok(disk) => match Self::overlay(disk, &values, &pending) {
                Ok(merged) => merged,
                Err(error) => {
                    tracing::warn!("the settings file cannot take this window's: {error:#}");
                    values.clone()
                }
            },
            Err(error) => {
                tracing::warn!("{error:#}");
                values.clone()
            }
        };
        // What is kept apart is its own file's, not the settings file's:
        // the state as this process holds it, the user's data as its file
        // stands (another skwir, `skwir cmd`), with what this process
        // changed over it.
        let data = self.data_path().and_then(|path| match Self::on_disk(path) {
            Ok(disk) => {
                self.apart_read.lock().insert(Store::Data, disk.clone());
                Self::overlay(disk, &values, &pending).ok()
            }
            Err(error) => {
                tracing::warn!("{error:#}");
                None
            }
        });
        for (key, store) in self.kept_apart() {
            let from = match (store, &data) {
                (Store::Data, Some(data)) => data,
                _ => &values,
            };
            // Whole, over the settings file's copy; with none of its own,
            // the settings file's is read until skwir keeps one.
            if let Some(value) = Self::lookup(from, &key) {
                Self::remove(&mut merged, &key);
                // A parent that is not a table is refused, and the value
                // stays out: what the settings file says wins.
                let _ = Self::insert(&mut merged, &key, value.clone());
            }
        }
        if merged == values {
            return Some(Vec::new());
        }
        let differs = |key: &str| Self::lookup(&values, key) != Self::lookup(&merged, key);
        let mut keys: Vec<String> = self
            .schema
            .read()
            .keys()
            .filter(|key| differs(key))
            .cloned()
            .collect();
        let plugins = |table: &toml::Table| {
            table
                .get("plugins")
                .and_then(toml::Value::as_table)
                .map(|plugins| plugins.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        };
        let mut ids = plugins(&values);
        ids.extend(plugins(&merged));
        ids.sort_unstable();
        ids.dedup();
        keys.extend(
            ids.into_iter()
                .map(|id| format!("plugins.{id}.enabled"))
                .filter(|key| differs(key)),
        );
        *self.values.write() = merged;
        drop(pending);
        self.bump();
        Some(keys)
    }

    /// Persist to the file, if any: what this process changed, over the
    /// file as it stands, whose other changes it takes in. Call from a
    /// worker, never during rendering.
    pub fn save(&self) -> anyhow::Result<()> {
        let mut pending = self.writer.lock();
        let values = self.values.read().clone();
        let saved = match self.persist(&values, &pending) {
            Ok(saved) => saved,
            // A change a read-only file cannot take is undone, once: the
            // settings are what the file says (what is kept apart was saved
            // first).
            Err(error) if self.read_only() => {
                pending.clear();
                if let Some(path) = &self.path
                    && let Ok(disk) = Self::on_disk(path)
                {
                    let mut back = disk;
                    for (key, _) in self.kept_apart() {
                        Self::remove(&mut back, &key);
                        if let Some(value) = Self::lookup(&values, &key) {
                            let _ = Self::insert(&mut back, &key, value.clone());
                        }
                    }
                    *self.values.write() = back;
                    self.bump();
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        pending.clear();
        if saved != values {
            *self.values.write() = saved;
            self.bump();
        }
        Ok(())
    }
}
