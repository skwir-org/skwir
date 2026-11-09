//! Exclusions (ADR-125) across the plugins that walk: a tag's place
//! and a `tag:` search agree on what is hidden and what is left out (the
//! rules kept from searches, a cache's tag, a name beside its tool's
//! file, a trash), `excluded:true` reaches what the rules leave out but a
//! trash, and a search says how many folders it walked past. Tags in
//! extended attributes, a `.private`'s privacy: Unix's (ADR-035).
#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::ListingEvent;
use skwir_core::os_path::PathArg;

fn fixture(name: &str) -> PathBuf {
    let base = dirs::home_dir()
        .unwrap()
        .join(".cache/skwir-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn until(app: &App, seconds: u64, what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        app.pump();
        if ready() {
            return;
        }
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The files a search of `root` found, by their paths under it, and how
/// many folders it said it walked past.
fn search(app: &App, expr: &str, root: &Path) -> (BTreeSet<String>, usize) {
    let location = skwir_plugin_query::location_for(expr, Some(root));
    let open = app.open_location(&location).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let (mut found, mut left_out) = (BTreeSet::new(), 0);
    loop {
        match open
            .events()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the search completes")
        {
            ListingEvent::Added(batch) => {
                for e in batch {
                    let path = open.path_of(e.id).unwrap();
                    found.insert(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
            ListingEvent::LeftOut(n) => left_out = n,
            ListingEvent::Complete => break,
            ListingEvent::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    open.close();
    (found, left_out)
}

#[test]
fn a_tags_place_and_a_tag_search_agree_on_what_is_hidden_and_left_out() {
    let base = fixture("folder-rules");
    let root = base.join("it's home ü");
    for dir in [
        ".config/app's",
        "node_modules/pkg",
        "built",
        ".Trash-1000/files",
        "crate/target/debug",
        "photos/target",
    ] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("crate/Cargo.toml"), b"[package]").unwrap();
    std::fs::write(
        root.join("built/CACHEDIR.TAG"),
        b"Signature: 8a477f597d28d172789f06886806bc55\n",
    )
    .unwrap();
    let tagged = [
        "plain.txt",
        ".config/app's/x.conf",
        ".dot file",
        "node_modules/pkg/y.js",
        "built/z.o",
        ".Trash-1000/files/t",
        "crate/target/debug/out",
        "photos/target/shot.jpg",
    ];
    for name in tagged {
        std::fs::write(root.join(name), b"x").unwrap();
        if skwir_plugin_tags::write_tags(&root.join(name), &["Work".to_owned()]).is_err() {
            eprintln!("skipped: no user extended attributes here");
            return;
        }
    }
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    let set = |names: &[&str]| {
        names
            .iter()
            .map(|n| (*n).to_owned())
            .collect::<BTreeSet<_>>()
    };

    // A search without hidden files: neither what is hidden nor what is
    // left out; a photographer's `target` is no build's.
    let (found, left_out) = search(&app, "tag:work", &root);
    assert_eq!(found, set(&["plain.txt", "photos/target/shot.jpg"]));
    assert_eq!(
        left_out, 3,
        "node_modules, the cache, the crate's target (hidden ones not walked)"
    );
    // With hidden files: those hidden too, never a trash.
    let (found, left_out) = search(&app, "tag:work hidden:true", &root);
    assert_eq!(
        found,
        set(&[
            "plain.txt",
            "photos/target/shot.jpg",
            ".config/app's/x.conf",
            ".dot file",
        ])
    );
    assert_eq!(left_out, 4, "and the trash");
    // The folders left out too: everything but the trash.
    let (found, _) = search(&app, "tag:work hidden:true excluded:true", &root);
    assert_eq!(found.len(), tagged.len() - 1, "{found:?}");
    assert!(!found.iter().any(|f| f.starts_with(".Trash")));
    // A search of a folder left out walks it.
    let (found, _) = search(&app, "tag:work", &root.join("node_modules"));
    assert_eq!(found, set(&["pkg/y.js"]));

    // The tag's place of the folder: what `hidden:true` finds, marked
    // hidden where the pane hides it.
    let pane = app.workspace.read().active_id();
    let place = skwir_plugin_tags::location::Address {
        tag: Some("Work".to_owned()),
        scope: Some(root.clone()),
        ..skwir_plugin_tags::location::Address::default()
    }
    .location();
    app.navigate(pane, &place).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    until(&app, 20, "the tags of the folder counted", || {
        let Outcome::Value(v) = app
            .dispatch(&Invocation::with_args(
                "tags.counted",
                &serde_json::json!({ "root": PathArg::from(root.as_path()) }),
            ))
            .unwrap()
        else {
            panic!("tags.counted answers a value");
        };
        v["counting"] == false && v["counted"].is_u64()
    });
    until(&app, 20, "the place lists what was counted", || {
        let ws = app.workspace.read();
        ws.pane(pane)
            .unwrap()
            .listing
            .iter()
            .filter(|e| !e.is_place())
            .count()
            == 4
    });
    let listed: BTreeSet<(String, bool)> = {
        let ws = app.workspace.read();
        ws.pane(pane)
            .unwrap()
            .listing
            .iter()
            .filter(|e| !e.is_place())
            .map(|e| (e.name.to_string_lossy().into_owned(), e.is_hidden()))
            .collect()
    };
    assert_eq!(
        listed,
        BTreeSet::from([
            ("plain.txt".to_owned(), false),
            ("shot.jpg".to_owned(), false),
            ("x.conf".to_owned(), true),
            (".dot file".to_owned(), true),
        ]),
        "x.conf is hidden as the folder it is in"
    );
    let _ = std::fs::remove_dir_all(&base);
}

/// A pattern of files the user added: searches, tag counts and the
/// timeline's walk leave those files out, say so, and a folder that only
/// shares the pattern is walked as any other.
#[test]
fn a_pattern_of_files_is_left_out_of_a_search_and_a_folder_of_that_name_is_not() {
    let base = fixture("exclusion-files");
    let root = base.join("it's home ü");
    std::fs::create_dir_all(root.join("run.log")).unwrap();
    for name in ["keep.txt", "server.log", "run.log/inside.txt"] {
        std::fs::write(root.join(name), b"x").unwrap();
    }
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let entries = r#"{"logs": {"pattern": "*.log", "names": "files", "what": "Logs", "searches": true, "agents": false, "index": false}}"#;
    app.dispatch(&Invocation::with_args(
        "settings.apply",
        &serde_json::json!({ "changes": { "exclusions.entries": entries } }),
    ))
    .unwrap();
    until(&app, 10, "saved", || {
        app.settings.save_status().1 != skwir_core::settings::SaveState::Saving
    });
    let names = |names: &[&str]| {
        names
            .iter()
            .map(|n| (*n).to_owned())
            .collect::<BTreeSet<_>>()
    };
    let (found, left_out) = search(&app, "name:*", &root);
    assert_eq!(
        found,
        names(&["keep.txt", "run.log", "run.log/inside.txt"]),
        "the file named is left out, the folder of that name walked"
    );
    assert_eq!(left_out, 1, "the search says so");
    let (found, _) = search(&app, "name:* excluded:true", &root);
    assert!(found.contains("server.log"), "{found:?}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A folder of the user's, added to the rules kept from agents, is private
/// as skwir's own are; an agent asking to make it readable is refused.
#[test]
fn a_folder_you_keep_from_agents_is_private_and_stays_so_against_an_agent() {
    let base = fixture("folder-rules-private");
    let secret = base.join("taxes ü");
    std::fs::create_dir_all(&secret).unwrap();
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let entries = format!(
        r#"{{"taxes": {{"pattern": "{}", "what": "Taxes", "agents": true, "searches": false, "index": false}}}}"#,
        secret.display()
    );
    app.dispatch(&Invocation::with_args(
        "settings.apply",
        &serde_json::json!({ "changes": { "exclusions.entries": entries } }),
    ))
    .unwrap();
    until(&app, 10, "saved", || {
        app.settings.save_status().1 != skwir_core::settings::SaveState::Saving
    });
    assert!(app.privacy().holds(&secret.join("2025.pdf")));
    let undo = serde_json::json!({ "changes": { "exclusions.entries": "{}" } });
    let refused = skwir_core::permissions::as_agent(app.privacy(), || {
        app.dispatch(&Invocation::with_args("settings.apply", &undo))
    });
    let said = format!("{:#}", refused.expect_err("an agent may not lower privacy"));
    assert!(
        said.contains("may not make private folders readable"),
        "{said}"
    );
    assert!(app.privacy().holds(&secret.join("2025.pdf")));
    let _ = std::fs::remove_dir_all(&base);
}
