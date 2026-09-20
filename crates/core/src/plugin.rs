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
    /// External tools and workers this plugin uses. Probes run on a worker
    /// when a settings UI asks; they are never called during registration.
    fn requirements(&self) -> Vec<Requirement> {
        Vec::new()
    }
}

/// An external tool a plugin relies on, with a probe for its presence.
#[derive(Clone)]
pub struct Requirement {
    /// Program or library name as the user would install it ("ffmpeg").
    pub name: String,
    /// What stops working without it.
    pub purpose: String,
    /// `true` when the plugin is useless without it, `false` for a fallback.
    pub required: bool,
    /// May touch the filesystem; call from a worker.
    pub probe: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl Requirement {
    pub fn new(
        name: &str,
        purpose: &str,
        probe: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Requirement {
            name: name.into(),
            purpose: purpose.into(),
            required: false,
            probe: Arc::new(probe),
        }
    }

    /// A program looked up on `PATH`.
    pub fn program(name: &'static str, purpose: &str) -> Self {
        Self::new(name, purpose, move || on_path(name))
    }

    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// Whether an executable called `program` is on `PATH`.
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// The outcome of probing one [`Requirement`].
#[derive(Clone, Debug, serde::Serialize)]
pub struct RequirementStatus {
    pub plugin: &'static str,
    pub name: String,
    pub purpose: String,
    pub required: bool,
    pub found: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SetEnabledArgs {
    /// Plugin id, as listed by `plugins.list`.
    id: String,
    enabled: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct NoArgs {}

/// Plugin management as commands, so the settings UI, the CLI and agents
/// share one surface. Lifecycle callbacks can block (a plugin joins its
/// threads on disable), so the toggle runs on a worker.
pub(crate) fn register(app: &App) {
    app.commands.register::<SetEnabledArgs, _>(
        "core",
        "plugins.set_enabled",
        "Enable or disable a plugin",
        "Toggle a plugin at runtime and remember the choice; completion is signalled by a wake and `plugins.list`",
        |args, app| {
            anyhow::ensure!(
                app.plugins.list().iter().any(|p| p.id == args.id),
                "unknown plugin: {}",
                args.id
            );
            let weak = app.weak();
            app.scheduler.spawn(
                crate::scheduler::Priority::Normal,
                "plugin-toggle",
                app.lifetime().child(),
                move || {
                    let Some(app) = weak.upgrade() else { return };
                    app.plugins.set_enabled(&args.id, args.enabled, &app);
                    if let Err(error) = app.settings.save() {
                        app.notify(
                            crate::app::NotifyLevel::Error,
                            format!("The plugin choice was not saved: {error:#}"),
                        );
                    }
                    app.wake();
                },
            );
            Ok(crate::command::Outcome::Done)
        },
    );
    app.commands.register::<NoArgs, _>(
        "core",
        "plugins.list",
        "List plugins",
        "Every plugin with its title, description and enabled state",
        |_, app| {
            Ok(crate::command::Outcome::Value(serde_json::to_value(
                app.plugins.list(),
            )?))
        },
    );
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
    /// Advances on every enable/disable/add.
    revision: std::sync::atomic::AtomicU64,
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
        self.bump();
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }

    fn bump(&self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    /// Probe every requirement of enabled and disabled plugins alike.
    /// Worker-only: probes touch the filesystem.
    pub fn probe_requirements(&self) -> Vec<RequirementStatus> {
        let plugins: Vec<_> = self
            .plugins
            .read()
            .iter()
            .map(|(plugin, _)| Arc::clone(plugin))
            .collect();
        plugins
            .iter()
            .flat_map(|plugin| {
                let id = plugin.id();
                plugin.requirements().into_iter().map(move |r| {
                    let found = (r.probe)();
                    RequirementStatus {
                        plugin: id,
                        name: r.name,
                        purpose: r.purpose,
                        required: r.required,
                        found,
                    }
                })
            })
            .collect()
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
        self.bump();
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
