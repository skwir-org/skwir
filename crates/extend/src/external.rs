//! A plugin from outside bibli, as the plugin host knows plugins.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use bibli_core::App;
use bibli_core::permissions::{Capabilities, Capability};
use bibli_core::plugin::{Category, Plugin};
use bibli_plugin_api::Manifest;
use bibli_plugin_api::manifest::{self, Category as Declared};
use parking_lot::Mutex;
use serde_json::Value;

use crate::call::{Call, Transport};

/// A plugin a folder holds: its manifest, read and checked, and what runs
/// it, started when first asked.
pub struct External {
    id: &'static str,
    description: &'static str,
    manifest: Manifest,
    folder: PathBuf,
    capabilities: Capabilities,
    pub(crate) runner: Arc<Runner>,
}

impl External {
    /// The plugin of `folder`, from its `plugin.toml`.
    ///
    /// # Errors
    /// No manifest, one that does not parse or hold together, or what it
    /// runs is outside its folder or missing.
    pub fn load(folder: &Path) -> anyhow::Result<External> {
        use anyhow::Context as _;
        let path = folder.join("plugin.toml");
        let text = std::fs::read_to_string(&path).with_context(|| format!("{}", path.display()))?;
        let manifest: Manifest =
            toml::from_str(&text).with_context(|| format!("{}", path.display()))?;
        manifest
            .check()
            .map_err(|why| anyhow::anyhow!("{}: {why}", path.display()))?;
        let folder = std::path::absolute(folder)?;
        let inside = |name: &str| -> anyhow::Result<PathBuf> {
            let relative = Path::new(name);
            anyhow::ensure!(
                relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
                "{}: `{name}` is a path inside the plugin's folder",
                path.display()
            );
            let full = folder.join(relative);
            anyhow::ensure!(full.is_file(), "{}: no `{name}`", path.display());
            Ok(full)
        };
        let kind = match (&manifest.run.module, &manifest.run.program) {
            (Some(module), _) => Kind::Module(inside(module)?),
            (None, Some(program)) => Kind::Program {
                program: inside(program)?,
                args: manifest.run.args.clone(),
            },
            (None, None) => unreachable!("checked"),
        };
        let capabilities: Capabilities = manifest
            .capabilities
            .iter()
            .map(|capability| match capability {
                manifest::Capability::Read => Capability::Read,
                manifest::Capability::Write => Capability::Write,
                manifest::Capability::Network => Capability::Network,
                manifest::Capability::Programs => Capability::Programs,
            })
            .collect();
        let id = crate::intern(&manifest.id);
        Ok(External {
            id,
            description: crate::intern(&manifest.description),
            runner: Arc::new(Runner {
                id,
                title: manifest.title.clone(),
                kind,
                folder: folder.clone(),
                granted: capabilities,
                compiled: Mutex::new(None),
                live: Mutex::new(None),
                watch: Mutex::new(Watch::default()),
                turn: bibli_platform::process::Turns::new(1),
            }),
            manifest,
            folder,
            capabilities,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }
}

impl Plugin for External {
    fn id(&self) -> &'static str {
        self.id
    }
    fn title(&self) -> &str {
        &self.manifest.title
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn category(&self) -> Category {
        match self.manifest.category {
            Declared::Browsing => Category::Browsing,
            Declared::Files => Category::Files,
            Declared::Previews => Category::Previews,
            Declared::Desktop => Category::Desktop,
        }
    }
    /// Off until the user turns it on, having read what it may do.
    fn enabled_by_default(&self) -> bool {
        false
    }
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }
    fn origin(&self) -> Option<String> {
        Some(self.folder.display().to_string())
    }
    fn register(&self, app: &App) {
        *self.runner.compiled.lock() = Some(app.cache.namespace(self.id).folder("module"));
        *self.runner.watch.lock() = Watch {
            app: Some(app.weak()),
            off: false,
            broken: 0,
        };
        crate::providers::register(app, self);
        crate::commands::register(app, self);
    }
    fn on_disable(&self, _app: &App) {
        self.runner.stop();
    }
    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        match self.runner.kind {
            Kind::Module(_) => vec![
                crate::MODULE_WORKER
                    .requirement("Running this plugin's WebAssembly module")
                    .required(),
            ],
            Kind::Program { .. } => Vec::new(),
        }
    }
}

/// What runs a plugin.
pub(crate) enum Kind {
    Module(PathBuf),
    Program { program: PathBuf, args: Vec<String> },
}

/// A plugin's transport, started when first asked and let go of when the
/// plugin is turned off.
pub(crate) struct Runner {
    pub id: &'static str,
    /// What the user knows it as: what a review says proposed a plan.
    pub title: String,
    pub kind: Kind,
    folder: PathBuf,
    pub granted: Capabilities,
    /// Where a module is kept compiled: the plugin's folder of bibli's
    /// cache, known once the plugin is registered.
    compiled: Mutex<Option<PathBuf>>,
    live: Mutex<Option<Arc<dyn Transport>>>,
    watch: Mutex<Watch>,
    /// Its one turn: the plugin answers one call at a time, so a
    /// thumbnail asked meanwhile is set aside rather than holding a
    /// worker while it waits (`providers::Thumbnails`).
    pub turn: bibli_platform::process::Turns,
}

/// How many times in a row its program broke (`process::Broken`: no
/// answer in time, stopped, the protocol broken) past which the plugin is
/// turned off, and the user told: each time it is ended and started again
/// at the next call.
const BROKEN_AT_MOST: u32 = 3;

/// What `Runner` keeps of the plugin being on.
#[derive(Default)]
struct Watch {
    app: Option<std::sync::Weak<App>>,
    /// Turned off (`stop`): a call still under way starts nothing.
    off: bool,
    /// Its program's breaks in a row (`BROKEN_AT_MOST`).
    broken: u32,
}

impl Runner {
    /// A runner whose plugin is `transport`, already started: for tests.
    #[cfg(test)]
    pub(crate) fn answered_by(module: &Path, transport: Arc<dyn Transport>) -> Runner {
        Runner {
            id: "test",
            title: "Test".into(),
            kind: Kind::Module(module.to_owned()),
            folder: module.parent().unwrap_or(module).to_owned(),
            granted: [Capability::Read].into_iter().collect(),
            compiled: Mutex::new(None),
            live: Mutex::new(Some(transport)),
            watch: Mutex::new(Watch::default()),
            turn: bibli_platform::process::Turns::new(1),
        }
    }

    /// What runs it, the module or the program: a new version of it makes
    /// again what the plugin made (`made_key`).
    pub fn made_by(&self) -> &Path {
        match &self.kind {
            Kind::Module(module) => module,
            Kind::Program { program, .. } => program,
        }
    }

    /// Ask the plugin `method`; it reads through `call`. Its program
    /// breaking `BROKEN_AT_MOST` times in a row turns the plugin off.
    /// Waits for the plugin's turn, given up with the call.
    pub fn call(&self, method: &str, params: Value, call: &Call) -> anyhow::Result<Value> {
        let Some(turn) = self.turn.take(call.token()) else {
            anyhow::bail!("cancelled");
        };
        self.call_in(&turn, method, params, call)
    }

    /// [`Runner::call`], the plugin's turn taken already
    /// (`turn.try_take`).
    pub fn call_in(
        &self,
        _turn: &bibli_platform::process::Turn<'_>,
        method: &str,
        params: Value,
        call: &Call,
    ) -> anyhow::Result<Value> {
        let transport = {
            let mut live = self.live.lock();
            if self.watch.lock().off {
                anyhow::bail!("the plugin is off");
            }
            if let Some(transport) = &*live {
                Arc::clone(transport)
            } else {
                let started = self.start()?;
                *live = Some(Arc::clone(&started));
                started
            }
        };
        let answer = transport.call(method, params, call);
        let broken = answer
            .as_ref()
            .is_err_and(anyhow::Error::is::<crate::process::Broken>);
        let turn_off = {
            let mut watch = self.watch.lock();
            watch.broken = if broken { watch.broken + 1 } else { 0 };
            (watch.broken >= BROKEN_AT_MOST && !watch.off)
                .then(|| {
                    watch.off = true;
                    watch.app.as_ref().and_then(std::sync::Weak::upgrade)
                })
                .flatten()
        };
        if let Some(app) = turn_off {
            self.turned_off(&app);
        }
        answer
    }

    /// Turn the plugin off, its program having broken again and again,
    /// and say so with the way to its page.
    fn turned_off(&self, app: &App) {
        tracing::warn!(plugin = self.id, "turned off: its program kept breaking");
        if let Err(error) = app.dispatch(&bibli_core::command::Invocation::with_args(
            "plugins.set_enabled",
            &serde_json::json!({ "id": self.id, "enabled": false }),
        )) {
            tracing::warn!("{error:#}");
        }
        app.notify_with_action(
            bibli_core::app::NotifyLevel::Warning,
            format!(
                "{} was turned off: it stopped answering {BROKEN_AT_MOST} times in a row",
                self.title
            ),
            "Plugins",
            bibli_core::command::Invocation::with_args(
                "ui.settings",
                &serde_json::json!({ "page": "plugins" }),
            ),
        );
    }

    fn start(&self) -> anyhow::Result<Arc<dyn Transport>> {
        use crate::process::{Isolation, Program};
        Ok(match &self.kind {
            // Its helper, a program confined by the module's own walls:
            // isolated where the system can, run plainly where not.
            // It works in the plugin's folder of the cache, where it
            // keeps the module compiled (`--keep`): the next start loads
            // it instead of compiling it again.
            Kind::Module(module) => {
                let compiled = self.compiled.lock().clone();
                let mut args = vec![module.display().to_string()];
                if compiled.is_some() {
                    args.extend(["--keep".to_owned(), ".".to_owned()]);
                }
                Arc::new(Program::new(
                    self.id,
                    crate::MODULE_WORKER.require("run plugins that are WebAssembly modules")?,
                    args,
                    self.folder.clone(),
                    self.granted,
                    Isolation::WhereAvailable,
                    compiled,
                ))
            }
            Kind::Program { program, args } => Arc::new(Program::new(
                self.id,
                program.clone(),
                args.clone(),
                self.folder.clone(),
                self.granted,
                Isolation::Required,
                None,
            )),
        })
    }

    /// Let go of what runs it: a call still under way gives up, and none
    /// starts it again until the plugin is turned on (`register`).
    pub fn stop(&self) {
        self.watch.lock().off = true;
        if let Some(transport) = self.live.lock().take() {
            transport.stop();
        }
    }
}
