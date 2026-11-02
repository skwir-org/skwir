//! The Tag chip and Find Similar, through the commands the UI runs, with
//! the tags plugin reading real extended attributes.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::Location;
use skwir_core::workspace::PaneId;

/// A folder where extended attributes work: not `/tmp`, often a tmpfs.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
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
            && skwir_core::fs::xattr::set(&path, "user.xdg.tags", tags.as_bytes()).is_err()
        {
            eprintln!("skipped: no user extended attributes here");
            return;
        }
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
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
            skwir_plugin_query::lang::parse(&location.rest)
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
    // The tags chosen are the pane's lens (ADR-107): a search found as
    // similar keeps them, until they are taken off.
    assert_eq!(
        similar("kind"),
        BTreeSet::from(["a.txt".into(), "b.txt".into()])
    );
    let lens = app
        .workspace
        .read()
        .pane(pane)
        .and_then(|p| p.lens().map(|l| l.words.to_string()))
        .unwrap_or_default();
    assert!(
        lens.contains("kind:text") && lens.contains("tag:work"),
        "{lens}"
    );
}

/// `tag:` offers the tags of the folder's files first, then every other
/// tag the user has (here, those a listing elsewhere read), each once
/// whatever its case; words name the folder's tags where they filter it,
/// any of the user's in a scope.
#[test]
fn tag_offers_the_folders_tags_then_the_users_others() {
    let tmp = home_tempdir();
    let (here, elsewhere) = (tmp.path().join("it's here"), tmp.path().join("élsewhere"));
    for (dir, name, tags) in [
        (&here, "a.txt", Some("work,Places/France")),
        (&here, "c.txt", None),
        (&elsewhere, "b.txt", Some("holiday,WORK")),
    ] {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, name).unwrap();
        if let Some(tags) = tags
            && skwir_core::fs::xattr::set(&path, "user.xdg.tags", tags.as_bytes()).is_err()
        {
            eprintln!("skipped: no user extended attributes here");
            return;
        }
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    // Each folder shown and its files' notes read, as rows in view are.
    let show = |dir: &std::path::Path| {
        app.navigate(pane, &Location::from_path(dir)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let ids: Vec<skwir_core::EntryId> = {
            let ws = app.workspace.read();
            ws.pane(pane)
                .unwrap()
                .listing
                .iter()
                .map(|e| e.id)
                .collect()
        };
        app.dispatch(&Invocation::with_args(
            "hydrate",
            &serde_json::json!({ "ids": ids, "pane": pane }),
        ))
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.pump();
            let read = {
                let ws = app.workspace.read();
                let p = ws.pane(pane).unwrap();
                p.listing.iter().all(|e| e.metadata.is_hydrated())
            };
            if read {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "notes never read");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    show(&elsewhere);
    show(&here);

    let labels = |text: &str| -> Vec<String> {
        let v = value(
            &app,
            "query.complete",
            serde_json::json!({ "text": text, "pane": pane }),
        );
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["label"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(labels("tag:"), ["Places/France", "work", "holiday"]);
    assert_eq!(labels("#"), ["Places/France", "work", "holiday"]);
    assert_eq!(labels("tag:ho"), ["holiday"]);
    let answer = value(&app, "tags.known", serde_json::json!({}));
    let known: Vec<&str> = answer["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert_eq!(known.len(), 3, "{known:?}: each tag once");
    // Asked with the number of the tags held, while nothing changed: the
    // number alone, the tags not sent again.
    let generation = answer["generation"].clone();
    let again = value(
        &app,
        "tags.known",
        serde_json::json!({ "generation": generation }),
    );
    assert_eq!(again["generation"], generation);
    assert!(again.get("tags").is_none(), "{again}");
    // Asked with another number, the tags are sent.
    let stale = value(&app, "tags.known", serde_json::json!({ "generation": 0 }));
    assert_eq!(stale["tags"].as_array().map(Vec::len), Some(3), "{stale}");

    // The field's own rows: in the folder, `holiday` is a name; in a
    // scope, a tag of the user's.
    let field = app
        .registry
        .suggesters
        .all()
        .into_iter()
        .find(|s| s.0.id() == "query" && s.0.searches())
        .expect("the query plugin's field");
    let reading = |scope: Option<Location>| {
        let ask = skwir_core::suggest::Ask {
            text: "holiday".into(),
            caret: 7,
            typed: skwir_core::suggest::Typed::Words,
            scope,
            shown: Some(Location::from_path(&here)),
            dir: Some(here.clone()),
            pane: Some(pane),
            lens: None,
        };
        field
            .0
            .suggest(&app, &ask, &skwir_core::cancel::CancelToken::new())
            .rows
            .into_iter()
            .find(|r| r.section == skwir_core::suggest::Section::Reading)
            .and_then(|r| r.completion)
    };
    assert_eq!(reading(None), None);
    assert_eq!(
        reading(Some(Location::from_path(tmp.path()))).as_deref(),
        Some("tag:holiday")
    );
}
