//! A search skwird's index covers (ADR-150), through the command people,
//! scripts and agents use: answered from skwird where it proves its root,
//! its holes walked, a name it gives that is gone since left out, and said
//! in the listing and in `entries`; the plugin turned off, the search walks
//! again. skwird's engine is the client's fake, on a socket of the test's
//! own. Sockets: Unix's (ADR-035).
#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::coverage::Proof;
use skwir_core::location::RestsOn;
use skwird_client::fake::{Fake, Script};
use skwird_client::protocol;

const SINCE: i64 = 1_800_000_007;

fn entry(path: &Path) -> protocol::Entry {
    protocol::Entry {
        path: protocol::WirePath::of(path),
        kind: protocol::Kind::File,
        id: protocol::Id { dev: 1, ino: 2 },
        folder: protocol::Id { dev: 1, ino: 1 },
        attrs: None,
    }
}

/// What the pane's search of `expr` found, relative to `root`, and the
/// answer of `entries`. The same search again on the same pane is the
/// listing it shows: another text searches anew.
fn search(app: &App, expr: &str, root: &Path) -> (BTreeSet<String>, serde_json::Value) {
    let pane = app.workspace.read().active_id();
    app.dispatch(&Invocation::with_args(
        "query.search",
        &serde_json::json!({"expr": expr, "root": root}),
    ))
    .unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
    let Outcome::Value(answer) = app
        .dispatch(&Invocation::with_args("entries", &serde_json::json!({})))
        .unwrap()
    else {
        panic!("entries answers a value");
    };
    let found = answer["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            Path::new(e["path"].as_str().unwrap())
                .strip_prefix(root)
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    (found, answer)
}

#[test]
fn a_search_skwird_covers_is_answered_from_its_index() {
    let tmp = tempfile::tempdir().unwrap();
    // The real path: skwird knows no other.
    let root = std::fs::canonicalize(tmp.path()).unwrap().join("search");
    for name in [
        "report.txt",
        "notes.txt",
        "shut/report in a hole.txt",
        "unnamed/report nobody named.txt",
    ] {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "fixture").unwrap();
    }
    let since = protocol::Instant {
        sec: SINCE,
        nsec: 0,
    };
    let fake = Fake::start(
        tmp.path().join("skwird.socket"),
        Script {
            roots: vec![root.clone()],
            coverage: vec![(
                root.clone(),
                protocol::Coverage {
                    names: protocol::Proof::Live(since),
                    attrs: protocol::Proof::Live(since),
                    holes: vec![protocol::Hole {
                        path: protocol::WirePath::of(&root.join("shut")),
                        why: protocol::Why::Denied,
                    }],
                },
            )],
            entries: vec![
                entry(&root.join("report.txt")),
                entry(&root.join("notes.txt")),
                entry(&root.join("gone report.txt")),
            ],
            ..Script::default()
        },
    )
    .unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins.add(
        Arc::new(skwir_plugin_skwird::SkwirdPlugin::at(fake.socket.clone())),
        &app,
    );

    let (found, answer) = search(&app, "report", &root);
    // What it named, as found; its hole walked; what it does not name and
    // no hole holds, not looked for: it proves the rest.
    assert_eq!(
        found,
        BTreeSet::from(["report.txt".into(), "shut/report in a hole.txt".into()])
    );
    let pane = app.workspace.read().active_id();
    let rests_on = app
        .workspace
        .read()
        .pane(pane)
        .unwrap()
        .listing
        .rests_on()
        .cloned();
    assert_eq!(
        rests_on,
        Some(RestsOn {
            by: skwir_plugin_skwird::by(),
            proof: Proof::Live {
                since: std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_007),
            },
        })
    );
    assert_eq!(
        answer["index"],
        serde_json::json!({"by": "skwird's index", "proof": "live", "at": SINCE})
    );
    assert!(answer.get("as_of").is_none(), "{answer}");

    // Turned off, it is asked nothing: the search walks.
    assert!(
        app.plugins
            .set_enabled(skwir_plugin_skwird::ID, false, &app)
    );
    let (found, answer) = search(&app, "report type:file", &root);
    assert_eq!(
        found,
        BTreeSet::from([
            "report.txt".into(),
            "shut/report in a hole.txt".into(),
            "unnamed/report nobody named.txt".into(),
        ])
    );
    assert!(answer.get("index").is_none(), "{answer}");
}

/// A headless test's app asks none of the machine's engines, whatever it
/// runs: the plugin built into skwir answers nothing there.
#[test]
fn a_test_s_app_asks_no_engine_of_the_machine_s() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(root.join("report.txt"), "fixture").unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    assert!(!app.index_services());
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_skwird::SkwirdPlugin::default()), &app);
    let cancel = skwir_core::cancel::CancelToken::new();
    assert!(
        app.names.open(&root, &cancel).is_empty(),
        "no index answers"
    );
    let (found, answer) = search(&app, "report", &root);
    assert_eq!(found, BTreeSet::from(["report.txt".into()]));
    assert!(answer.get("index").is_none(), "{answer}");
}

/// Sizes and times are asked of skwird only where it says them live
/// (ADR-150, 5 and D2): there a dated search with no name is answered from
/// its index, each file it names checked; elsewhere a search with a name
/// is asked of it by that name alone, and one with none walks.
#[test]
fn times_are_asked_of_skwird_only_where_it_holds_them_live() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap().join("dated");
    std::fs::create_dir_all(&root).unwrap();
    for (name, secs) in [
        ("new report.txt", 1_900_000_000),
        ("old report.txt", 1_000_000_000),
    ] {
        let path = root.join(name);
        std::fs::write(&path, "fixture").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }
    let since = protocol::Instant {
        sec: SINCE,
        nsec: 0,
    };
    let answered = |attrs: protocol::Proof, expr: &str| {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::start(
            dir.path().join("skwird.socket"),
            Script {
                roots: vec![root.clone()],
                coverage: vec![(
                    root.clone(),
                    protocol::Coverage {
                        names: protocol::Proof::Live(since),
                        attrs,
                        holes: Vec::new(),
                    },
                )],
                entries: vec![
                    entry(&root.join("new report.txt")),
                    entry(&root.join("old report.txt")),
                ],
                ..Script::default()
            },
        )
        .unwrap();
        let app = App::new(AppConfig::ephemeral(dir.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
        app.plugins.add(
            Arc::new(skwir_plugin_skwird::SkwirdPlugin::at(fake.socket.clone())),
            &app,
        );
        let (found, answer) = search(&app, expr, &root);
        (found, answer.get("index").is_some())
    };
    let new = BTreeSet::from(["new report.txt".to_owned()]);
    let dated = "modified>=2030-01-01";
    assert_eq!(
        answered(protocol::Proof::Live(since), dated),
        (new.clone(), true),
        "live: answered from the index"
    );
    let checked = protocol::Proof::Checked(since);
    assert_eq!(
        answered(checked, dated),
        (new.clone(), false),
        "checked, and no name: walked"
    );
    assert_eq!(
        answered(checked, &format!("report {dated}")),
        (new, true),
        "checked: asked by its name, the date tested on what it names"
    );
}
