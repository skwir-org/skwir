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
}

impl Runner {
    /// Ask the plugin `method`; it reads through `call`.
    pub fn call(&self, method: &str, params: Value, call: &Call) -> anyhow::Result<Value> {
        let transport = {
            let mut live = self.live.lock();
            if let Some(transport) = &*live {
                Arc::clone(transport)
            } else {
                let started = self.start()?;
                *live = Some(Arc::clone(&started));
                started
            }
        };
        transport.call(method, params, call)
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

    /// Let go of what runs it.
    pub fn stop(&self) {
        if let Some(transport) = self.live.lock().take() {
            transport.stop();
        }
    }
}
