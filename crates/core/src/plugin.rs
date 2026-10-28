//! Plugins: compile-time crates that register contributions into the core
//! and can be toggled at runtime. A plugin depends only on the core, never
//! on another plugin.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::app::App;
use crate::command::Topic;

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
    /// Where the plugin is listed among the others.
    fn category(&self) -> Category;
    /// Whether the plugin is on until the user turns it off. A plugin that
    /// changes what standard keys do (a key scheme) waits to be asked.
    fn enabled_by_default(&self) -> bool {
        true
    }
    /// Contribute to the app. Called on enable.
    fn register(&self, app: &App);
    /// Apply changed preferences on a worker. Read-through settings need no hook.
    /// Refresh only affected contributions, preserving unrelated plugin state.
    fn on_settings_changed(&self, _app: &App, _keys: &[String]) {}
    /// Release resources the registries do not know about. Called on disable.
    fn on_disable(&self, _app: &App) {}
    /// What it may do beyond naming and listing files (ADR-044): read what
    /// files hold, change them, use the network, start programs. A plugin
    /// built into skwir says it, for the Plugins page; one from outside
    /// gets what it declares and nothing more, from its host.
    fn capabilities(&self) -> crate::permissions::Capabilities {
        crate::permissions::Capabilities::NONE
    }
    /// Where a plugin from outside skwir comes from (its folder), said on
    /// the Plugins page; `None` for one built in.
    fn origin(&self) -> Option<String> {
        None
    }
    /// External tools and workers this plugin uses. Probes run on a worker
    /// when a settings UI asks; they are never called during registration.
    fn requirements(&self) -> Vec<Requirement> {
        Vec::new()
    }
}

/// What a plugin is about, so a list of plugins can be browsed by subject.
/// The declaration order is the order they are listed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Getting to files: places, search, jumping, views of a tree.
    Browsing,
    /// Acting on files: operations, renaming, collecting, tagging.
    Files,
    /// Showing what a file holds: previews and thumbnails.
    Previews,
    /// Working with the rest of the desktop: applications, terminals, media keys.
    Desktop,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Browsing,
        Category::Files,
        Category::Previews,
        Category::Desktop,
    ];

    /// The heading a settings UI shows above the category's plugins.
    pub fn title(self) -> &'static str {
        match self {
            Category::Browsing => "Browsing and finding",
            Category::Files => "Working with files",
            Category::Previews => "Previews and thumbnails",
            Category::Desktop => "Desktop integration",
        }
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
    crate::os_path::find_program(program).is_some()
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

/// Plugin management as commands, so the settings UI, the CLI and agents
/// share one surface. Lifecycle callbacks can block (a plugin joins its
/// threads on disable): with a window, the toggle runs on a worker, as a
/// plugin it stops may be the one whose connection asked (`settings.apply`
/// does the same). Without one (`skwir cmd`), it runs where it is asked
/// and answers once saved: the process may end as soon as it answers.
pub(crate) fn register(app: &App) {
    app.commands.register::<SetEnabledArgs, _>(
        "core", Topic::Skwir,
        "plugins.set_enabled",
        "Enable or disable a plugin",
        "Toggle a plugin at runtime and remember the choice; with the window open, completion is signalled by a wake and `plugins.list`",
        |args, app| {
            anyhow::ensure!(
                app.plugins.list().iter().any(|p| p.id == args.id),
                "unknown plugin: {}",
                args.id
            );
            if !crate::scheduler::has_window() {
                app.plugins.set_enabled(&args.id, args.enabled, app);
                app.settings
                    .save()
                    .map_err(|error| anyhow::anyhow!("The plugin choice was not saved: {error:#}"))?;
                return Ok(crate::command::Outcome::Done);
            }
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
    app.commands
        .register::<crate::command::NoArgs, _>(
            "core",
            Topic::Skwir,
            "plugins.list",
            "List plugins",
            "Every plugin with its title, description and enabled state",
            |_, app| {
                Ok(crate::command::Outcome::Value(serde_json::to_value(
                    app.plugins.list(),
                )?))
            },
        )
        .data();
}

/// Summary of a plugin for settings UIs.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PluginInfo {
    pub id: &'static str,
    pub title: String,
    pub description: String,
    pub category: Category,
    pub enabled: bool,
    /// Whether skwir comes with it on (`Plugin::enabled_by_default`).
    pub enabled_by_default: bool,
    /// What it may do (`Plugin::capabilities`).
    pub capabilities: crate::permissions::Capabilities,
    /// Where it comes from, for one from outside skwir.
    pub origin: Option<String>,
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

    /// Add a plugin. It is enabled as `plugins.<id>.enabled` says in the
    /// settings, else as the plugin says (`Plugin::enabled_by_default`).
    pub fn add(&self, plugin: Arc<dyn Plugin>, app: &App) {
        let _lifecycle = self.lifecycle.lock();
        let enabled = app
            .settings
            .get::<bool>(&Self::setting_key(plugin.id()))
            .unwrap_or_else(|| plugin.enabled_by_default());
        app.registry.set_owner_enabled(plugin.id(), enabled);
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
        // The gate closes before the withdrawal, so a worker of the plugin
        // that publishes in between cannot bring its items back.
        app.registry.set_owner_enabled(id, enabled);
        let readers = app.registry.filter_readers.owners();
        if enabled {
            plugin.register(app);
        } else {
            app.unregister_owner(id);
            plugin.on_disable(app);
        }
        // What a pane's filter keeps is the reader's there is now.
        if app.registry.filter_readers.owners() != readers {
            app.read_filters_again();
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
            .filter(|(_, enabled)| *enabled)
            .map(|(plugin, _)| Arc::clone(plugin))
            .collect();
        for plugin in plugins {
            let owned: Vec<_> = keys
                .iter()
                .filter(|key| crate::settings::owner_of(key) == plugin.id())
                .cloned()
                .collect();
            if !owned.is_empty() {
                plugin.on_settings_changed(app, &owned);
            }
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
                category: p.category(),
                enabled: *e,
                enabled_by_default: p.enabled_by_default(),
                capabilities: p.capabilities(),
                origin: p.origin(),
            })
            .collect()
    }
}
