//! Plugins from outside skwir (ADR-045, ADR-046): a folder holding a
//! `plugin.toml` (`skwir_plugin_api::Manifest`) and what runs it, a
//! program that speaks the protocol on its standard input and output
//! (Level 2, [`process`]), or a WebAssembly module of the plugin world
//! (Level 1), which skwir's helper `skwir-module-worker` runs and serves
//! as such a program: Wasmtime never enters skwir's own process. Each becomes a [`External`], a plugin like the
//! others: listed on the Plugins page with what it may do and where it
//! comes from, off until the user turns it on, its contributions withdrawn
//! when turned off.
//!
//! The host keeps it to what it declares (ADR-044): a program runs
//! isolated (`skwir_platform::sandbox::plugin`), without the network
//! unless it declares it; a module reaches nothing but skwir's two
//! imports, its helper isolated too where the system can. Either reads only the files of the call it answers, by handle
//! ([`call::Call`]), and only with the `read` capability; a private file
//! is never handed to it; a plan it answers needs `write`, and is proposed
//! for review.

mod call;
mod commands;
mod external;
mod pages;
mod process;
mod providers;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use external::External;
/// Whether an id can be a plugin's from outside (lowercase letters, digits
/// and dashes, none of skwir's own namespaces): what `discover` asks of a
/// manifest, beside the ids `taken`.
pub use skwir_plugin_api::manifest::check_id;

/// The helper that runs a plugin's WebAssembly module (ADR-046).
pub const MODULE_WORKER: skwir_platform::process::Worker = skwir_platform::process::Worker {
    variable: "SKWIR_MODULE_WORKER",
    executable: "skwir-module-worker",
};

/// The variable naming more folders of plugins, as `PATH` does: for
/// building and trying one.
pub const PATH_VARIABLE: &str = "SKWIR_PLUGIN_PATH";

/// Where installed plugins are looked for, each a folder of plugin
/// folders: `skwir/plugins` in the user's data folder and in the
/// system's, then those [`PATH_VARIABLE`] names.
pub fn folders() -> Vec<PathBuf> {
    let data = skwir_core::xdg::data_home()
        .into_iter()
        .chain(skwir_core::xdg::data_dirs())
        .map(|dir| dir.join("skwir").join("plugins"));
    let named = std::env::var_os(PATH_VARIABLE)
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    data.chain(named.into_iter().filter(|path| path.is_absolute()))
        .collect()
}

/// The plugins of `folders`: each folder in them that holds a
/// `plugin.toml`, in name order. One whose manifest does not hold
/// together, whose id is one of skwir's own namespaces ([`check_id`]), or
/// whose id is `taken` (a plugin built into skwir, or one found before),
/// is left out, and the log says why. Reads the folders: at startup,
/// before the window.
pub fn discover(folders: &[PathBuf], taken: &[&str]) -> Vec<Arc<External>> {
    let mut ids: HashSet<String> = taken.iter().map(|id| (*id).to_owned()).collect();
    let mut found = Vec::new();
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        let mut plugins: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.join("plugin.toml").is_file())
            .collect();
        plugins.sort();
        for path in plugins {
            match External::load(&path) {
                Ok(plugin) if ids.insert(plugin.manifest().id.clone()) => {
                    found.push(Arc::new(plugin));
                }
                Ok(plugin) => tracing::warn!(
                    id = %plugin.manifest().id,
                    "a plugin from outside skwir is left out: another has its id"
                ),
                Err(error) => tracing::warn!("a plugin from outside skwir is left out: {error:#}"),
            }
        }
    }
    found
}

/// `text`, kept for as long as skwir runs: a plugin's id and names, which
/// the registries take as `&'static str`. Each text is kept once.
pub(crate) fn intern(text: &str) -> &'static str {
    static KEPT: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut kept = KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let kept = kept.get_or_insert_with(HashSet::new);
    if let Some(text) = kept.get(text) {
        return text;
    }
    let text: &'static str = Box::leak(text.to_owned().into_boxed_str());
    kept.insert(text);
    text
}
