//! A paste on a place that takes files (`Verbs::drop`): the clipboard's
//! files go to its drop command with only the arguments it takes (the
//! shelf's and the trash's take the files alone; a tag's is told whether
//! they move and from where), and a cut pasted on a tag is done by that
//! paste's plan alone. The trash takes no copy.

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
#[cfg(unix)]
use skwir_core::command::Outcome;
use skwir_core::location::Location;
use skwir_core::workspace::PaneId;

/// A folder on a real file system (the trash, extended attributes): not
/// `/tmp`, often a tmpfs.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("paste-on-places-")
        .tempdir_in(base)
        .unwrap()
}

struct Setup {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    app: Arc<App>,
    pane: PaneId,
}

fn setup() -> Setup {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's a folder");
    std::fs::create_dir_all(&root).unwrap();
    for name in ["notes one.txt", "draft's.txt"] {
        std::fs::write(root.join(name), b"x").unwrap();
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins.add(
        Arc::new(skwir_plugin_ops_basic::OpsBasicPlugin::default()),
        &app,
    );
    app.plugins
        .add(Arc::new(skwir_plugin_shelf::ShelfPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_trash::TrashPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    Setup {
        _tmp: tmp,
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

/// Select `name` in the folder and run `command` (a copy or a cut).
fn take(s: &Setup, name: &str, command: &str) {
    go(s, &Location::from_path(&s.root));
    s.app
        .dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": [name] }),
        ))
        .unwrap();
    s.app.dispatch(&Invocation::new(command)).unwrap();
}

fn listed(s: &Setup) -> Vec<String> {
    let ws = s.app.workspace.read();
    let pane = ws.pane(s.pane).unwrap();
    pane.listing
        .iter()
        .map(|e| e.name_str().into_owned())
        .collect()
}

/// Wait for `f` to hold, pumping the app.
fn until(app: &App, what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f() {
        app.pump();
        assert!(Instant::now() < deadline, "waited for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Ctrl+V in the Shelf: the shelf takes the files alone (`shelf.add`
/// refused `move` once the registry checked arguments).
#[test]
fn files_copied_or_cut_are_pasted_on_the_shelf() {
    let s = setup();
    take(&s, "notes one.txt", "clipboard_copy");
    go(&s, &Location::parse("shelf:"));
    s.app
        .dispatch(&Invocation::new("clipboard_paste"))
        .expect("the shelf takes a paste");
    until(&s.app, "the file on the shelf", || {
        listed(&s).contains(&"notes one.txt".to_owned())
    });

    take(&s, "draft's.txt", "clipboard_cut");
    go(&s, &Location::parse("shelf:"));
    s.app
        .dispatch(&Invocation::new("clipboard_paste"))
        .expect("the shelf takes a cut");
    until(&s.app, "the cut file on the shelf", || {
        listed(&s).contains(&"draft's.txt".to_owned())
    });
    assert!(
        s.root.join("draft's.txt").exists(),
        "the shelf keeps a reference"
    );
}

/// Ctrl+V in the Trash: the files cut go to the trash (`trash.put`
/// refused `move` once the registry checked arguments).
#[cfg(target_os = "linux")]
#[test]
fn files_cut_are_pasted_in_the_trash() {
    let s = setup();
    take(&s, "notes one.txt", "clipboard_cut");
    go(&s, &Location::parse("trash:"));
    let outcome = s
        .app
        .dispatch(&Invocation::new("clipboard_paste"))
        .expect("the trash takes a paste");
    let Outcome::Plan(plan) = outcome else {
        panic!("a plan that trashes them")
    };
    let r = s.app.executor.execute_blocking(&plan, s.app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(!s.root.join("notes one.txt").exists(), "in the trash");
}

/// Ctrl+V in the Trash of files copied, not cut: refused, since the
/// trash keeps files and not copies of them. The file stays where it
/// is, and copied: pasted in a folder, it is copied there.
#[cfg(target_os = "linux")]
#[test]
fn files_copied_are_not_pasted_in_the_trash() {
    let s = setup();
    take(&s, "notes one.txt", "clipboard_copy");
    go(&s, &Location::parse("trash:"));
    let refused = s
        .app
        .dispatch(&Invocation::new("clipboard_paste"))
        .expect_err("the trash takes no copy");
    assert!(refused.to_string().contains("cut them"), "{refused}");
    assert!(s.root.join("notes one.txt").exists(), "left where it is");
    go(&s, &Location::from_path(&s.root));
    let Outcome::Plan(plan) = s.app.dispatch(&Invocation::new("clipboard_paste")).unwrap() else {
        panic!("a plan that copies it")
    };
    assert!(plan.title.starts_with("copy to "), "{}", plan.title);
}

/// A cut pasted on a tag, its plan dismissed: another plan's notes on the
/// file (another tag) leave it cut; the paste's own plan, applied later,
/// is what makes it done.
#[cfg(unix)]
#[test]
fn a_cut_pasted_on_a_tag_is_done_by_that_plan_alone() {
    let s = setup();
    s.app
        .plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &s.app);
    let file: &Path = &s.root.join("draft's.txt");
    take(&s, "draft's.txt", "clipboard_cut");
    let family = skwir_plugin_tags::location::Address {
        tag: Some("family".to_owned()),
        scope: Some(s.root.clone()),
        ..Default::default()
    }
    .location();
    go(&s, &family);
    let Outcome::Plan(paste) = s.app.dispatch(&Invocation::new("clipboard_paste")).unwrap() else {
        panic!("a plan that gives the tag")
    };
    // Dismissed: never applied. Another tag given to the file.
    let Outcome::Plan(other) = s
        .app
        .dispatch(&Invocation::with_args(
            "tags.toggle",
            &serde_json::json!({
                "tag": "later",
                "on": true,
                "entries": [skwir_core::os_path::PathArg::from(file)],
            }),
        ))
        .unwrap()
    else {
        panic!("a plan that gives the other tag")
    };
    let r = s.app.executor.execute_blocking(&other, s.app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    s.app.pump();
    assert_eq!(s.app.pending.get().len(), 1, "still cut");
    // The paste's own plan, applied: done.
    let r = s.app.executor.execute_blocking(&paste, s.app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    until(&s.app, "the cut done", || s.app.pending.get().is_empty());
}
