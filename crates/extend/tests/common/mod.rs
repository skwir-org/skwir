//! What the tests run: a plugin from outside skwir built as a module or a
//! program by the cargo that builds the tests, then laid out in a folder
//! of plugins as an installed one is.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use skwir_core::App;
use skwir_core::app::AppConfig;

/// The workspace's root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/extend is two folders down")
        .to_owned()
}

/// `package`'s library built as a WebAssembly module, for the plugin
/// world: its path.
pub fn module(package: &str) -> PathBuf {
    built(
        &[
            "rustc",
            "--release",
            "--lib",
            "--crate-type",
            "cdylib",
            "--target",
            "wasm32-unknown-unknown",
            "-p",
            package,
        ],
        |name| Path::new(name).extension().is_some_and(|e| e == "wasm"),
    )
}

/// `package`'s program built: its path.
pub fn program(package: &str) -> PathBuf {
    built(&["build", "--bins", "-p", package], |_| true)
}

/// Run cargo with `args`; the last artifact it names that `wanted` says.
fn built(args: &[&str], wanted: impl Fn(&str) -> bool) -> PathBuf {
    let output = Command::new(env!("CARGO"))
        .args(args)
        .args(["--message-format", "json-render-diagnostics"])
        .current_dir(root())
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo {}: {}\n(a module needs the wasm32-unknown-unknown target: the dev shell has it)",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .flat_map(|message| {
            let executable = message["executable"].as_str().map(str::to_owned);
            let files: Vec<String> = message["filenames"]
                .as_array()
                .map(|files| {
                    files
                        .iter()
                        .filter_map(|f| f.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            executable.into_iter().chain(files)
        })
        .filter(|name| wanted(name))
        .last()
        .map(PathBuf::from)
        .expect("cargo built it")
}

/// A folder of plugins holding `id`: `manifest` as its `plugin.toml`,
/// `built` copied in as `as_name`.
pub fn install(plugins: &Path, id: &str, manifest: &str, built: &Path, as_name: &str) -> PathBuf {
    let folder = plugins.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("plugin.toml"), manifest).unwrap();
    std::fs::copy(built, folder.join(as_name)).unwrap();
    folder
}

/// A temporary folder on the home filesystem.
pub fn home_tempdir() -> tempfile::TempDir {
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("home")
        .join(".cache")
        .join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("extend-")
        .tempdir_in(base)
        .unwrap()
}

/// The helper that runs modules, built, and named where skwir looks for
/// it first (`SKWIR_MODULE_WORKER`): a test's program is not beside it.
pub fn module_worker() {
    let worker = program("skwir-module-worker");
    // SAFETY: nextest runs each test in a process of its own, and this
    // runs before any thread of the test's reads the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var(skwir_extend::MODULE_WORKER.variable, worker);
    }
}

/// An app with the plugins of `plugins` added and turned on.
pub fn app_with(state: &Path, plugins: &Path, taken: &[&str]) -> Arc<App> {
    module_worker();
    let app = App::new(AppConfig::ephemeral(state)).unwrap();
    for plugin in skwir_extend::discover(&[plugins.to_owned()], taken) {
        let id = skwir_core::plugin::Plugin::id(&*plugin);
        app.plugins.add(plugin, &app);
        assert!(app.plugins.set_enabled(id, true, &app), "{id} turned on");
    }
    app
}
