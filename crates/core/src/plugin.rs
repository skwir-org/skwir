//! Plugins: compile-time crates that register contributions into the core
//! and can be toggled at runtime. A plugin depends only on the core, never
//! on another plugin.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::app::App;

/// A plugin. `register` contributes commands, operations, providers, views,
/// places, settings, and so on, all tagged with `id()` so they can be
/// withdrawn on disable.
pub trait Plugin: Send + Sync {
    /// Stable identifier, used as the owner tag and the settings namespace.
    fn id(&self) -> &'static str;
    fn title(&self) -> &str;
    fn description(&self) -> &'static str {
        ""
    }
    /// Contribute to the app. Called on enable.
    fn register(&self, app: &App);
    /// Apply changed preferences on a worker. Read-through settings need no hook.
    /// Refresh only affected contributions, preserving unrelated plugin state.
    fn on_settings_changed(&self, _app: &App, _keys: &[String]) {}
    /// Release resources the registries do not know about. Called on disable.
    fn on_disable(&self, _app: &App) {}
}

/// Summary of a plugin for settings UIs.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PluginInfo {
    pub id: &'static str,
    pub title: String,
    pub description: String,
    pub enabled: bool,
}

/// Holds the plugins and their enabled state.
#[derive(Default)]
pub struct PluginHost {
    plugins: RwLock<Vec<(Arc<dyn Plugin>, bool)>>,
    /// Serializes lifecycle callbacks without holding registry/state locks.
    /// Callbacks must not recursively enable/disable plugins.
    lifecycle: parking_lot::Mutex<()>,
}

impl PluginHost {
    fn setting_key(id: &str) -> String {
        format!("plugins.{id}.enabled")
    }

    /// Add a plugin. It is enabled unless `plugins.<id>.enabled = false` in settings.
    pub fn add(&self, plugin: Arc<dyn Plugin>, app: &App) {
        let _lifecycle = self.lifecycle.lock();
        let enabled = app
            .settings
            .get::<bool>(&Self::setting_key(plugin.id()))
            .unwrap_or(true);
        if enabled {
            plugin.register(app);
        }
        self.plugins.write().push((plugin, enabled));
    }

    /// Toggle at runtime. Persists the choice in settings.
    pub fn set_enabled(&self, id: &str, enabled: bool, app: &App) -> bool {
        let _lifecycle = self.lifecycle.lock();
        let plugin = {
            let mut plugins = self.plugins.write();
            let Some(slot) = plugins.iter_mut().find(|(p, _)| p.id() == id) else {
                return false;
            };
            if slot.1 == enabled {
                return true;
            }
            slot.1 = enabled;
            Arc::clone(&slot.0)
        };
        if enabled {
            plugin.register(app);
        } else {
            app.unregister_owner(id);
            plugin.on_disable(app);
        }
        app.settings
            .set(&Self::setting_key(id), toml::Value::Boolean(enabled));
        true
    }

    /// Worker-only: serialize refreshes against enable/disable so a late
    /// settings callback cannot restore a disabled plugin's contributions.
    pub fn settings_changed(&self, keys: &[String], app: &App) {
        let _lifecycle = self.lifecycle.lock();
        let plugins: Vec<_> = self
            .plugins
            .read()
            .iter()
            .filter(|(plugin, enabled)| {
                *enabled
                    && keys
                        .iter()
                        .any(|key| key.starts_with(&format!("{}.", plugin.id())))
            })
            .map(|(plugin, _)| Arc::clone(plugin))
            .collect();
        for plugin in plugins {
            let owned: Vec<_> = keys
                .iter()
                .filter(|key| key.starts_with(&format!("{}.", plugin.id())))
                .cloned()
                .collect();
            plugin.on_settings_changed(app, &owned);
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.plugins.read().iter().any(|(p, e)| p.id() == id && *e)
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        self.plugins
            .read()
            .iter()
            .map(|(p, e)| PluginInfo {
                id: p.id(),
                title: p.title().into(),
                description: p.description().into(),
                enabled: *e,
            })
            .collect()
    }
}
