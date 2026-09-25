//! Settings: a TOML file, validated against the schema plugins register,
//! read through typed accessors. Plugins own the `plugin_id.*` namespace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::{Mutex, RwLock};

use crate::os_path::PathArg;
use serde::de::DeserializeOwned;
use std::io::Write;

pub const PRESERVE_TAGS_ON_COPY: &str = "core.preserve_tags_on_copy";
/// `last` or `folder`.
pub const STARTUP: &str = "core.startup";
pub const STARTUP_FOLDER: &str = "core.startup_folder";
/// Application data: where the window was when it closed.
pub const LAST_LOCATION: &str = "core.last_location";

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
    /// A program or command line.
    Command,
    /// Application data managed by its owning commands, not by editors.
    Data,
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
    /// Optional group heading within the owner's page.
    pub section: String,
    /// Unit shown after a numeric value ("MB").
    pub unit: String,
    pub default: toml::Value,
    pub kind: SettingKind,
    pub constraints: SettingConstraints,
    /// The change takes effect at the next start.
    pub restart_required: bool,
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
            section: String::new(),
            unit: String::new(),
            default,
            kind,
            constraints: SettingConstraints {
                step: 1,
                ..SettingConstraints::default()
            },
            restart_required: false,
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

    pub fn command(key: &str, title: &str, description: &str, default: &str) -> Self {
        let mut spec = Self::text(key, title, description, default);
        spec.kind = SettingKind::Command;
        spec
    }

    /// Application data (e.g. saved queries) stored with the settings.
    pub fn data(key: &str, description: &str, default: toml::Value) -> Self {
        let mut spec = Self::new(key, key, description, default);
        spec.kind = SettingKind::Data;
        spec
    }

    #[must_use]
    pub fn section(mut self, section: &str) -> Self {
        section.clone_into(&mut self.section);
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

    pub fn format_value(value: &toml::Value) -> String {
        value
            .as_str()
            .map_or_else(|| value.to_string(), str::to_owned)
    }

    pub fn parse(&self, text: &str) -> anyhow::Result<toml::Value> {
        anyhow::ensure!(
            self.editable(),
            "{} is managed by its owning commands",
            self.key
        );
        let value = match self.default {
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
        Ok(value)
    }
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
struct ApplyArgs {
    /// Dotted setting keys and their editor text (strings need no TOML quotes).
    changes: BTreeMap<String, String>,
}

/// Register the nonblocking settings command. Validation is repeated on the
/// worker in case a plugin was disabled after the command was queued.
pub(crate) fn register(app: &crate::App) {
    app.settings.declare(
        SettingSpec::toggle(
            PRESERVE_TAGS_ON_COPY,
            "Keep tags when copying",
            "Copies carry the tags of their source. If the destination cannot store tags the copy fails; turn this off to copy without them.",
            true,
        )
        .section("Files"),
    );
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
        .section("Startup"),
    );
    app.settings.declare(
        SettingSpec::path(
            STARTUP_FOLDER,
            "Startup folder",
            "Used when a chosen folder opens at startup. Leave empty for your home folder.",
            "",
        )
        .section("Startup"),
    );
    app.settings.declare(SettingSpec::data(
        LAST_LOCATION,
        "The folder shown when the window was last closed",
        toml::Value::String(String::new()),
    ));
    app.commands.register::<ApplyArgs, _>(
        "core",
        "settings.apply",
        "Save settings",
        "Validate and atomically save preferences, then notify their owning plugins",
        |args, app| {
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
            app.scheduler.spawn(
                crate::scheduler::Priority::Normal,
                "settings-save",
                app.lifetime().child(),
                move || {
                    let Some(app) = guard.weak.upgrade() else {
                        return;
                    };
                    let result = app.settings.apply(&args.changes);
                    guard.state = Some(match result {
                        Ok(keys) => {
                            app.plugins.settings_changed(&keys, &app);
                            SaveState::Saved
                        }
                        Err(error) => SaveState::Failed(format!("{error:#}")),
                    });
                    drop(guard);
                },
            );
            Ok(crate::command::Outcome::Value(serde_json::json!(id)))
        },
    );
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
pub fn startup_folder(settings: &Settings, home: &Path) -> PathBuf {
    let stored = |key: &str| {
        settings
            .get::<PathArg>(key)
            .map(PathArg::into_inner)
            .filter(|path| !path.as_os_str().is_empty())
    };
    let choice = match settings.get::<String>(STARTUP).as_deref() {
        Some("last") => stored(LAST_LOCATION),
        _ => stored(STARTUP_FOLDER),
    };
    choice.unwrap_or_else(|| home.to_path_buf())
}

/// Record the folder to reopen. Writes the settings file on the calling
/// thread: for the moment the application closes, not for a UI callback.
pub fn remember_location(settings: &Settings, folder: &Path) {
    if settings.get::<String>(STARTUP).as_deref() != Some("last") {
        return;
    }
    // A string when the path is UTF-8, its bytes otherwise (`os_path`): a
    // folder whose name is not UTF-8 reopens as itself.
    if settings.get::<PathArg>(LAST_LOCATION).as_deref() == Some(folder) {
        return;
    }
    let Ok(value) = toml::Value::try_from(PathArg::from(folder)) else {
        return;
    };
    settings.set(LAST_LOCATION, value);
    if let Err(error) = settings.save() {
        tracing::warn!("cannot remember the last folder: {error:#}");
    }
}

/// Loaded settings plus declared schema.
pub struct Settings {
    path: Option<PathBuf>,
    values: RwLock<toml::Table>,
    schema: RwLock<BTreeMap<String, SettingSpec>>,
    /// Serializes writers; readers never wait for file I/O.
    writer: Mutex<()>,
    status: Mutex<(u64, SaveState)>,
    /// Advances when the schema or a live value changes, so editors compare
    /// one atomic instead of re-reading the schema on every frame.
    revision: std::sync::atomic::AtomicU64,
}

impl Settings {
    /// The platform config file (`$XDG_CONFIG_HOME/bibli/settings.toml` on Linux).
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("bibli")
            .join("settings.toml")
    }

    /// In-memory settings with no file.
    pub fn in_memory() -> Self {
        Settings {
            path: None,
            values: RwLock::new(toml::Table::new()),
            schema: RwLock::new(BTreeMap::new()),
            writer: Mutex::new(()),
            status: Mutex::new((0, SaveState::Idle)),
            revision: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Load from `path`; a missing file yields empty settings.
    pub fn load(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let values = match std::fs::read_to_string(&path) {
            Ok(s) => s.parse::<toml::Table>()?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Settings {
            path: Some(path),
            values: RwLock::new(values),
            schema: RwLock::new(BTreeMap::new()),
            writer: Mutex::new(()),
            status: Mutex::new((0, SaveState::Idle)),
            revision: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Declare a setting. Registered by plugins and the core.
    pub fn declare(&self, spec: SettingSpec) {
        self.schema.write().insert(spec.key.clone(), spec);
        self.bump();
    }

    /// Carry a value stored under a key's former name to its new one, when
    /// nothing is stored under the new name yet. The owner calls it before
    /// declaring the new key, so a moved preference keeps what the user
    /// chose; the file is rewritten at the next save, and until then the move
    /// is made again at each start, to the same effect.
    pub fn rename(&self, old: &str, new: &str) {
        let _writer = self.writer.lock();
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
        drop(values);
        self.bump();
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
        let _writer = self.writer.lock();
        if let Err(error) = Self::insert(&mut self.values.write(), key, value) {
            tracing::warn!(%error, "cannot set setting");
        }
        self.bump();
    }

    pub fn save_status(&self) -> (u64, SaveState) {
        self.status.lock().clone()
    }

    pub fn validate(
        &self,
        changes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, toml::Value>> {
        use anyhow::Context as _;
        let schema = self.schema.read();
        changes
            .iter()
            .map(|(key, text)| {
                let spec = schema
                    .get(key)
                    .ok_or_else(|| anyhow::anyhow!("unknown setting: {key}"))?;
                Ok((key.clone(), spec.parse(text).with_context(|| key.clone())?))
            })
            .collect()
    }

    fn persist(&self, values: &toml::Table) -> anyhow::Result<()> {
        if let Some(path) = &self.path {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            std::fs::create_dir_all(parent)?;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(toml::to_string_pretty(values)?.as_bytes())?;
            file.as_file().sync_all()?;
            file.persist(path)?;
            // The rename is durable only once the directory entry is flushed.
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }

    /// A transaction: failed validation or persistence leaves live values intact.
    /// Only changed keys are merged; unrelated settings survive an open editor.
    fn apply(&self, changes: &BTreeMap<String, String>) -> anyhow::Result<Vec<String>> {
        let _writer = self.writer.lock();
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
            self.persist(&values)?;
            *self.values.write() = values;
            self.bump();
        }
        Ok(keys)
    }

    /// Persist to the file, if any. Call from a worker, never during rendering.
    pub fn save(&self) -> anyhow::Result<()> {
        let _writer = self.writer.lock();
        let values = self.values.read().clone();
        self.persist(&values)
    }
}
