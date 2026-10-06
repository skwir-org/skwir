//! The Tag chip and Find Similar, through the commands the UI runs, with
//! the tags plugin reading real extended attributes.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use bibli_core::app::{App, AppConfig};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;
use bibli_core::workspace::PaneId;

/// A folder where extended attributes work: not `/tmp`, often a tmpfs.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("search-tags-")
        .tempdir_in(base)
        .unwrap()
}

fn names(app: &App, pane: PaneId) -> BTreeSet<String> {
    let ws = app.workspace.read();
    ws.pane(pane)
        .unwrap()
        .listing
        .iter()
        .map(|e| e.name_str().into_owned())
        .collect()
}

fn value(app: &App, name: &str, args: serde_json::Value) -> serde_json::Value {
    let invocation = Invocation {
        name: name.into(),
        args,
    };
    match app.dispatch(&invocation).unwrap() {
        Outcome::Value(v) => v,
        other => panic!("{name}: {other:?}"),
    }
}

#[test]
fn the_tag_chip_offers_the_tags_found_and_find_similar_shares_them() {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's tagged");
    std::fs::create_dir(&root).unwrap();
    for (name, tags) in [
        ("a.txt", Some("work,to do")),
        ("b.txt", Some("work")),
        ("c.txt", None),
        ("d.png", Some("holiday")),
    ] {
        let path = root.join(name);
        std::fs::write(&path, name).unwrap();
        if let Some(tags) = tags
            && bibli_core::fs::xattr::set(&path, "user.xdg.tags", tags.as_bytes()).is_err()
        {
            eprintln!("skipped: no user extended attributes here");
            return;
        }
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));

    // An empty search, as Enter on nothing starts one; its listing has
    // read no tags, so the picker reads them from the files.
    app.dispatch(&Invocation::with_args(
        "query.search",
        &serde_json::json!({ "expr": "" }),
    ))
    .unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    let chips = value(&app, "query.describe", serde_json::json!({}));
    let tag_chip = chips["chips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "tag")
        .unwrap()
        .clone();
    assert_eq!(tag_chip["choices"][0]["command"]["name"], "ui.choose");
    let source: Invocation =
        serde_json::from_value(tag_chip["choices"][0]["command"]["args"]["source"].clone())
            .unwrap();
    let answer = value(&app, &source.name, source.args.clone());
    let titles: Vec<&str> = answer["choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["work", "holiday", "to do", "Another tag…"],
        "most used first"
    );
    assert_eq!(answer["choices"][0]["detail"], "2 files");
    // Choosing one refines the search.
    let work: Invocation = serde_json::from_value(answer["choices"][0]["command"].clone()).unwrap();
    app.dispatch(&work).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    assert_eq!(
        names(&app, pane),
        BTreeSet::from(["a.txt".into(), "b.txt".into()])
    );

    // Find Similar from a.txt, by its tags (any of them) and then from
    // b.txt by its kind, in the folder the search looks in.
    app.dispatch(&Invocation::with_args(
        "select",
        &serde_json::json!({ "names": ["a.txt"] }),
    ))
    .unwrap();
    let similar = |by: &str| {
        let before = app.pane_location(pane);
        app.dispatch(&Invocation::with_args(
            "query.similar",
            &serde_json::json!({ "by": by }),
        ))
        .unwrap();
        // It builds the search on a worker, then navigates.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while app.pane_location(pane) == before {
            assert!(std::time::Instant::now() < deadline, "{by}: no search");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let location = app.pane_location(pane).unwrap();
        assert_eq!(
            bibli_plugin_query::lang::parse(&location.rest)
                .unwrap()
                .root,
            root
        );
        names(&app, pane)
    };
    assert_eq!(
        similar("tags"),
        BTreeSet::from(["a.txt".into(), "b.txt".into()])
    );
    app.dispatch(&Invocation::with_args(
        "select",
        &serde_json::json!({ "names": ["b.txt"] }),
    ))
    .unwrap();
    assert_eq!(
        similar("kind"),
        BTreeSet::from(["a.txt".into(), "b.txt".into(), "c.txt".into()])
    );
}
