//! Settings: a TOML file, validated against the schema plugins register,
//! read through typed accessors. Plugins own the `plugin_id.*` namespace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::{Mutex, RwLock};
use serde::de::DeserializeOwned;
use std::io::Write;

pub const PRESERVE_TAGS_ON_COPY: &str = "core.preserve_tags_on_copy";

/// One setting's declaration.
#[derive(Clone, Debug)]
pub struct SettingSpec {
    /// Dotted key, e.g. `view.list.row_height` or `plugin_id.option`.
    pub key: String,
    pub description: String,
    pub default: toml::Value,
    pub constraints: SettingConstraints,
}

/// Optional restrictions shared by editors and command validation.
#[derive(Clone, Debug, Default)]
pub struct SettingConstraints {
    pub choices: Vec<String>,
    pub integer_range: Option<(i64, i64)>,
}

impl SettingSpec {
    /// Collections are application data (e.g. saved queries), managed by their
    /// owning commands rather than the preferences editor.
    pub fn editable(&self) -> bool {
        matches!(
            self.default,
            toml::Value::String(_)
                | toml::Value::Boolean(_)
                | toml::Value::Integer(_)
                | toml::Value::Float(_)
        )
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
            toml::Value::Boolean(_) => toml::Value::Boolean(text.parse()?),
            toml::Value::Integer(_) => toml::Value::Integer(text.trim().parse()?),
            toml::Value::Float(_) => {
                let number: f64 = text.trim().parse()?;
                anyhow::ensure!(number.is_finite(), "expected a finite number");
                toml::Value::Float(number)
            }
            _ => unreachable!(),
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
                "expected {min}–{max}"
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
    app.settings.declare(SettingSpec {
        key: PRESERVE_TAGS_ON_COPY.into(),
        description: "Preserve file and folder tags when copying. If tags cannot be preserved, the copy fails; disable to copy without tags.".into(),
        default: toml::Value::Boolean(true),
        constraints: SettingConstraints::default(),
    });
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

/// Loaded settings plus declared schema.
pub struct Settings {
    path: Option<PathBuf>,
    values: RwLock<toml::Table>,
    schema: RwLock<BTreeMap<String, SettingSpec>>,
    /// Serializes writers; readers never wait for file I/O.
    writer: Mutex<()>,
    status: Mutex<(u64, SaveState)>,
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
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Declare a setting. Registered by plugins and the core.
    pub fn declare(&self, spec: SettingSpec) {
        self.schema.write().insert(spec.key.clone(), spec);
    }

    pub fn undeclare(&self, key: &str) {
        self.schema.write().remove(key);
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
