//! A pane's lens (ADR-107): what of its filter or its search the listing
//! can decide of a file, kept as the pane shows other locations until it
//! is taken off: the trash filtered by it, a tag's place too, as a folder
//! is, while each keeps what it is.
// The freedesktop trash (Linux).
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::Location;
use skwir_core::workspace::PaneId;

/// A folder on a real file system (the trash): not `/tmp`, often a tmpfs.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("lens-")
        .tempdir_in(base)
        .unwrap()
}

struct Setup {
    tmp: tempfile::TempDir,
    root: PathBuf,
    app: Arc<App>,
    pane: PaneId,
}

fn setup() -> Setup {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's a folder");
    std::fs::create_dir_all(root.join("inside")).unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins.add(
        Arc::new(skwir_plugin_ops_basic::OpsBasicPlugin::default()),
        &app,
    );
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_trash::TrashPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    Setup {
        tmp,
        root,
        app,
        pane,
    }
}

fn go(s: &Setup, location: &Location) {
    s.app.navigate(s.pane, location).unwrap();
    assert!(
        s.app.pump_until_complete(s.pane, Duration::from_secs(10)),
        "{location} listed"
    );
}

/// The names the pane shows.
fn shown(s: &Setup) -> BTreeSet<String> {
    let ws = s.app.workspace.read();
    let p = ws.pane(s.pane).unwrap();
    p.order()
        .iter()
        .filter_map(|id| p.entry(*id))
        .map(|e| e.name_str().into_owned())
        .collect()
}

fn lens(s: &Setup) -> Option<String> {
    let ws = s.app.workspace.read();
    ws.pane(s.pane).unwrap().lens().map(|l| l.words.to_string())
}

/// The trash is filtered by the lens as a folder is, and the lens stays
/// as the pane goes on from it; a search from it holds the lens in its
/// terms.
#[test]
fn the_trash_is_seen_through_the_lens() {
    let s = setup();
    // Names of their own: the tests share one trash.
    let mark = s
        .tmp
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let (picture, text) = (format!("{mark} it's.jpg"), format!("{mark} notes.txt"));
    for name in [&picture, &text] {
        std::fs::write(s.root.join(name), b"x").unwrap();
    }
    go(&s, &Location::from_path(&s.root));
    s.app
        .dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": [&picture, &text] }),
        ))
        .unwrap();
    let Outcome::Plan(plan) = s.app.dispatch(&Invocation::new("trash")).unwrap() else {
        panic!("a plan that trashes them")
    };
    let r = s.app.executor.execute_blocking(&plan, s.app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);

    s.app
        .dispatch(&Invocation::with_args(
            "query.narrow",
            &serde_json::json!({ "facet": "kind", "value": "image" }),
        ))
        .unwrap();
    // The folder lists them gone as its watcher says.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while shown(&s) != BTreeSet::from(["inside".to_owned()]) {
        assert!(std::time::Instant::now() < deadline, "{:?}", shown(&s));
        s.app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    go(&s, &Location::parse("trash:"));
    assert_eq!(lens(&s).as_deref(), Some("kind:image"));
    let trashed = shown(&s);
    assert!(trashed.contains(&picture), "{trashed:?}");
    assert!(!trashed.contains(&text), "left out by the lens");
    // Back in a folder, the lens is still there.
    go(&s, &Location::from_path(s.root.join("inside")));
    assert_eq!(lens(&s).as_deref(), Some("kind:image"));
    // Taken off, the trash shows everything again.
    s.app.dispatch(&Invocation::new("query.narrow")).unwrap();
    go(&s, &Location::parse("trash:"));
    assert_eq!(lens(&s), None);
    let trashed = shown(&s);
    assert!(
        trashed.contains(&picture) && trashed.contains(&text),
        "{trashed:?}"
    );
}
