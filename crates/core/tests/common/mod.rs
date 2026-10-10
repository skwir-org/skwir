//! Shared helpers for integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::op::Plan;
use skwir_core::workspace::PaneId;

/// A temp dir on the home filesystem so the freedesktop trash works
/// (tmpfs would need a top-level `.Trash` directory).
pub fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir()
        .expect("home")
        .join(".cache")
        .join("skwir-tests");
    std::fs::create_dir_all(&base).expect("create test base");
    tempfile::Builder::new()
        .prefix("t-")
        .tempdir_in(base)
        .expect("tempdir")
}

pub fn app(root: &Path) -> Arc<App> {
    App::new(AppConfig::ephemeral(root)).expect("app")
}

pub fn active(app: &App) -> PaneId {
    app.workspace.read().active_id()
}

pub fn navigate(app: &App, dir: &Path) {
    let inv = Invocation::with_args(
        "navigate",
        &serde_json::json!({ "location": dir.to_string_lossy() }),
    );
    app.dispatch(&inv).expect("navigate");
    assert!(
        app.pump_until_complete(active(app), Duration::from_secs(10)),
        "listing did not complete"
    );
}

pub fn select(app: &App, names: &[&str]) {
    let inv = Invocation::with_args("select", &serde_json::json!({ "names": names }));
    app.dispatch(&inv).expect("select");
}

/// Dispatch a command that must return a plan.
#[allow(clippy::needless_pass_by_value)]
pub fn plan(app: &App, name: &str, args: serde_json::Value) -> Plan {
    match app
        .dispatch(&Invocation::with_args(name, &args))
        .unwrap_or_else(|e| panic!("{name}: {e:#}"))
    {
        Outcome::Plan(p) => p,
        other => panic!("{name}: expected a plan, got {other:?}"),
    }
}

/// Apply a plan synchronously and require full success.
pub fn apply(app: &App, plan: &Plan) -> skwir_core::op::ExecResult {
    let r = app.executor.execute_blocking(plan, app.lifetime());
    assert!(
        r.error.is_none(),
        "plan `{}` failed: {:?}",
        plan.title,
        r.error
    );
    r
}

/// Sorted relative paths of everything under `root`, with file contents hashed
/// so a copy/move round trip is checked for content, not just names.
pub fn snapshot(root: &Path) -> Vec<(PathBuf, String)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("read_dir")
            .flatten()
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let p = e.path();
            let rel = p.strip_prefix(root).expect("under root").to_path_buf();
            let m = e.metadata().expect("metadata");
            if m.is_dir() {
                out.push((rel, "<dir>".into()));
                walk(root, &p, out);
            } else {
                out.push((
                    rel,
                    blake3::hash(&std::fs::read(&p).expect("read"))
                        .to_hex()
                        .to_string(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

/// Whether the tests run under Wine: `skwir_core::system::under_wine`.
pub fn under_wine(what: &str) -> bool {
    skwir_core::system::under_wine(what)
}
