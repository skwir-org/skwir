//! Settings: a TOML file, validated against the schema plugins register,
//! read through typed accessors. Plugins own the `plugin_id.*` namespace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::RwLock;
use serde::de::DeserializeOwned;

/// One setting's declaration.
#[derive(Clone, Debug)]
pub struct SettingSpec {
    /// Dotted key, e.g. `view.list.row_height` or `plugin_id.option`.
    pub key: String,
    pub description: String,
    pub default: toml::Value,
}

/// Loaded settings plus declared schema.
pub struct Settings {
    path: Option<PathBuf>,
    values: RwLock<toml::Table>,
    schema: RwLock<BTreeMap<String, SettingSpec>>,
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
        let mut cur: Option<&toml::Value> = None;
        let mut t = table;
        for part in key.split('.') {
            let v = t.get(part)?;
            cur = Some(v);
            t = match v.as_table() {
                Some(t) => t,
                None => {
                    return if part == key.rsplit('.').next().unwrap_or(key) {
                        cur
                    } else {
                        None
                    };
                }
            };
        }
        cur
    }

    /// Typed read: the stored value, else the declared default, else `None`.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let values = self.values.read();
        let v = Self::lookup(&values, key)
            .cloned()
            .or_else(|| self.schema.read().get(key).map(|s| s.default.clone()))?;
        v.try_into().ok()
    }

    /// Set a dotted key, creating intermediate tables.
    pub fn set(&self, key: &str, value: toml::Value) {
        let mut values = self.values.write();
        let mut t: &mut toml::Table = &mut values;
        let parts: Vec<&str> = key.split('.').collect();
        for part in &parts[..parts.len() - 1] {
            t = t
                .entry((*part).to_owned())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .expect("intermediate setting is a table");
        }
        t.insert(parts[parts.len() - 1].to_owned(), value);
    }

    /// Persist to the file, if any.
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, toml::to_string_pretty(&*self.values.read())?)?;
        }
        Ok(())
    }
}
