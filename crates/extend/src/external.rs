//! A plugin from outside skwir, as the plugin host knows plugins.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use skwir_core::App;
use skwir_core::permissions::{Capabilities, Capability};
use skwir_core::plugin::{Category, Plugin};
use skwir_core::words;
use skwir_plugin_api::Manifest;
use skwir_plugin_api::manifest::{self, Category as Declared};

use crate::call::{Answered, Call, Transport};
use crate::catalogs::Catalogs;

/// A plugin a folder holds: its manifest, read and checked, its catalogs
/// (protocol 2), and what runs it, started when first asked.
pub struct External {
    id: &'static str,
    description: skwir_core::words::Words,
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
        let catalogs = Catalogs::load(&folder, id, manifest.api);
        // Said as its id: what its author would want to know.
        for words in manifest.words() {
            if let skwir_plugin_api::protocol::Words::Message { id: message, .. } = words
                && !catalogs.says(message)
            {
                tracing::warn!(
                    plugin = id,
                    "its manifest names `{message}`, which its l10n/en.ftl does not say: said as its id"
                );
            }
        }
        Ok(External {
            id,
            description: catalogs.said(&manifest.description),
            runner: Arc::new(Runner {
                id,
                // A name of its author's making: its controls shown, isolated
                // wherever it is drawn, in a sentence or alone (ADR-139, 5).
                title: skwir_core::words::Words::shown(catalogs.said(&manifest.title)),
                api: manifest.api,
                catalogs,
                kind,
                folder: folder.clone(),
                granted: capabilities,
                compiled: Mutex::new(None),
                live: Mutex::new(None),
                watch: Mutex::new(Watch::default()),
                turn: skwir_platform::process::Turns::new(1),
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
    /// Its own words: as its manifest writes them, or a message of its
    /// catalogs (protocol 2).
    fn title(&self) -> skwir_core::words::Words {
        self.runner.title.clone()
    }
    fn description(&self) -> skwir_core::words::Words {
        self.description.clone()
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
        // Its words first: what it registers next says them.
        self.runner.catalogs.register(self.id);
        crate::providers::register(app, self);
        crate::commands::register(app, self);
    }
    fn on_disable(&self, _app: &App) {
        self.runner.stop();
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        match self.runner.kind {
            Kind::Module(_) => vec![
                crate::MODULE_WORKER
                    .requirement(words!(
                        extend_requirement_module = "Running this plugin's WebAssembly module",
                        note = "What a plugin from outside Skwir, a WebAssembly module, needs Skwir's helper for, in the settings' list of what a plugin needs."
                    ))
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
    pub title: skwir_core::words::Words,
    /// The protocol's version its manifest says it speaks: from 2, it is
    /// told the reader's languages and may say messages of its catalogs.
    pub api: u32,
    /// Its catalogs, which say its messages (protocol 2).
    pub catalogs: Catalogs,
    pub kind: Kind,
    folder: PathBuf,
    pub granted: Capabilities,
    /// Where a module is kept compiled: the plugin's folder of skwir's
    /// cache, known once the plugin is registered.
    compiled: Mutex<Option<PathBuf>>,
    live: Mutex<Option<Arc<dyn Transport>>>,
    watch: Mutex<Watch>,
    /// Its one turn: the plugin answers one call at a time, so a
    /// thumbnail asked meanwhile is set aside rather than holding a
    /// worker while it waits (`providers::Thumbnails`).
    pub turn: skwir_platform::process::Turns,
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
            title: skwir_core::words::Words::as_written("Test"),
            api: 1,
            catalogs: Catalogs::load(module, "test", 1),
            kind: Kind::Module(module.to_owned()),
            folder: module.parent().unwrap_or(module).to_owned(),
            granted: [Capability::Read].into_iter().collect(),
            compiled: Mutex::new(None),
            live: Mutex::new(Some(transport)),
            watch: Mutex::new(Watch::default()),
            turn: skwir_platform::process::Turns::new(1),
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
        _turn: &skwir_platform::process::Turn<'_>,
        method: &str,
        params: Value,
        call: &Call,
    ) -> anyhow::Result<Value> {
        let transport = {
            let mut live = self.live.lock();
            if self.watch.lock().off {
                anyhow::bail!(words!(
                    extend_plugin_off = "the plugin is off",
                    note = "An error: a plugin from outside Skwir was asked something once turned off. In lower case."
                ));
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
        let answer = answer.map_err(|error| self.said_error(error));
        let (turn_off, stopped) = {
            let mut watch = self.watch.lock();
            // Turned off while it was asked, before this call could.
            let stopped = watch.off;
            watch.broken = if broken { watch.broken + 1 } else { 0 };
            let turn_off = (watch.broken >= BROKEN_AT_MOST && !watch.off)
                .then(|| {
                    watch.off = true;
                    watch.app.as_ref().and_then(std::sync::Weak::upgrade)
                })
                .flatten();
            (turn_off, stopped)
        };
        if let Some(app) = turn_off {
            self.turned_off(&app);
        }
        match answer {
            // A module's failure may be its helper's, of another build of
            // skwir: said after it. A program of the plugin's own is never
            // asked which build it is.
            Err(error) if !stopped && matches!(self.kind, Kind::Module(_)) => {
                Err(match crate::MODULE_WORKER.find() {
                    Some(helper) => skwir_platform::process::explain(&helper, call.token(), error),
                    None => error,
                })
            }
            answer => answer,
        }
    }

    /// An error as the user reads it: one the plugin answered, its words
    /// (a message of its catalogs on protocol 2, its text as written on
    /// protocol 1); any other, Skwir's, as it is.
    fn said_error(&self, error: anyhow::Error) -> anyhow::Error {
        // Skwir's words around the plugin's (it did not start): as they are.
        if error.chain().nth(1).is_some() {
            return error;
        }
        match error.downcast::<Answered>() {
            Ok(Answered(answered)) => anyhow::Error::new(self.catalogs.error(self.api, &answered)),
            Err(error) => error,
        }
    }

    /// Turn the plugin off, its program having broken again and again,
    /// and say so with the way to its page.
    fn turned_off(&self, app: &App) {
        tracing::warn!(plugin = self.id, "turned off: its program kept breaking");
        if let Err(error) = app.dispatch(&skwir_core::command::Invocation::with_args(
            "plugins.set_enabled",
            &serde_json::json!({ "id": self.id, "enabled": false }),
        )) {
            tracing::warn!("{error:#}");
        }
        // Its title is its own words, as its manifest writes them.
        app.notify_with_action(
            skwir_core::app::NotifyLevel::Warning,
            words!(
                extend_turned_off = "{ $count ->
                    [one] The plugin “{ $plugin }” was turned off: it stopped answering
                   *[other] The plugin “{ $plugin }” was turned off: it stopped answering { $count } times in a row
                }",
                plugin = self.title.clone(),
                count = BROKEN_AT_MOST,
                note = "A notification: a plugin from outside Skwir (its title, its author's words) failed again and again, and Skwir turned it off."
            ),
            words!(
                extend_turned_off_action = "Plugins",
                note = "A notification's button: opens the Plugins page of the settings, where a plugin is turned on again."
            ),
            skwir_core::command::Invocation::with_args(
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
                    crate::MODULE_WORKER.require(words!(
                        extend_install_module_worker =
                            "Install { $program } to run plugins that are WebAssembly modules",
                        program = crate::MODULE_WORKER.executable,
                        note = "Why a plugin from outside Skwir written as a WebAssembly module does not start. program: the helper's name (skwir-module-worker)."
                    ))?,
                    args,
                    self.folder.clone(),
                    self.granted,
                    Isolation::WhereAvailable,
                    compiled,
                )
                .telling_languages(self.api >= 2))
            }
            Kind::Program { program, args } => Arc::new(
                Program::new(
                    self.id,
                    program.clone(),
                    args.clone(),
                    self.folder.clone(),
                    self.granted,
                    Isolation::Required,
                    None,
                )
                .telling_languages(self.api >= 2),
            ),
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
