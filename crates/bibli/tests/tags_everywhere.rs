//! A tag renamed or removed everywhere: the tags plugin finds the files
//! with the query plugin's search, proposes a plan for review, and the
//! sidebar's Tags follow what was applied.

use std::sync::Arc;
use std::time::{Duration, Instant};

use std::collections::BTreeSet;

use bibli_core::app::{App, AppConfig};
use bibli_core::command::Invocation;
use bibli_core::location::{ListingEvent, Location, OpenLocation};
use bibli_core::op::Plan;
use bibli_core::os_path::PathArg;
use bibli_plugin_tags::{read_tags, write_tags};

/// A folder where extended attributes work: not `/tmp`, often a tmpfs.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("tags-everywhere-")
        .tempdir_in(base)
        .unwrap()
}

fn tags(t: &[&str]) -> Vec<String> {
    t.iter().map(|&s| s.to_owned()).collect()
}

/// Wait for `f` to answer, pumping the app.
fn until<T>(app: &App, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.pump();
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "waited for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn proposal(app: &App) -> Plan {
    until(app, "a proposal", || app.take_proposals().pop())
}

/// The sidebar's tags, in order.
fn sidebar(app: &App) -> Vec<String> {
    app.registry
        .places
        .all()
        .into_iter()
        .filter(|p| p.group == "tags" && p.color.is_some())
        .map(|p| p.title)
        .collect()
}

#[test]
fn a_tag_is_renamed_on_every_file_after_review_and_the_sidebar_follows() {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's home");
    std::fs::create_dir_all(root.join("sub/deeper")).unwrap();
    std::fs::create_dir_all(root.join(".hidden")).unwrap();
    let files = [
        ("a.txt", &["Work", "x"][..]),
        ("it's b.txt", &["work", "Job"][..]),
        ("c.txt", &["other"][..]),
        ("sub/deeper/d.txt", &["WORK"][..]),
        (".hidden/e.txt", &["work"][..]),
    ];
    for (name, t) in files {
        std::fs::write(root.join(name), b"x").unwrap();
        write_tags(&root.join(name), &tags(t)).unwrap();
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    let at = |name: &str| read_tags(&root.join(name));
    let dispatch =
        |name: &str, args: serde_json::Value| app.dispatch(&Invocation::with_args(name, &args));
    let root_arg = serde_json::json!(PathArg::from(root.as_path()));

    dispatch(
        "tags.rename",
        serde_json::json!({ "tag": "work", "to": "Job", "root": root_arg }),
    )
    .unwrap();
    let plan = proposal(&app);
    assert_eq!(plan.title, "rename the tag “work” to “Job”");
    assert_eq!(plan.steps.len(), 3, "a, b and d; not the hidden one");
    let note = plan.note.clone().unwrap();
    assert!(note.starts_with("3 files in"), "{note}");
    assert!(note.contains("1 already had “Job”"), "{note}");
    // Nothing changed before it was applied.
    assert_eq!(at("a.txt"), ["Work", "x"]);

    let r = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(at("a.txt"), ["Job", "x"]);
    assert_eq!(at("it's b.txt"), ["Job"]);
    assert_eq!(at("sub/deeper/d.txt"), ["Job"]);
    assert_eq!(at(".hidden/e.txt"), ["work"]);
    // The index learned what the search read and what the plan wrote.
    until(&app, "the sidebar", || {
        (sidebar(&app) == ["Job", "other", "x"]).then_some(())
    });
    let job = app
        .registry
        .places
        .find(|p| p.group == "tags" && p.title == "Job")
        .unwrap();
    assert_eq!(&*job.location.scheme, "query");
    assert!(job.color.is_some());
    // Files dropped on it get it.
    let drop = job.drop.expect("a tag takes files dropped on it");
    assert_eq!(&*drop.name, "tags.add");
    assert_eq!(drop.args["tags"], serde_json::json!(["Job"]));

    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert_eq!(at("a.txt"), ["Work", "x"]);
    assert_eq!(at("it's b.txt"), ["work", "Job"]);

    // Several merged into one: one plan.
    dispatch(
        "tags.rename",
        serde_json::json!({ "tags": ["x", "other"], "to": "misc", "root": root_arg }),
    )
    .unwrap();
    let plan = proposal(&app);
    assert_eq!(plan.title, "merge the tags “x” and “other” into “misc”");
    assert_eq!(plan.steps.len(), 2, "a and c");
    app.executor.execute_blocking(&plan, app.lifetime());
    assert_eq!(at("a.txt"), ["Work", "misc"]);
    assert_eq!(at("c.txt"), ["misc"]);
    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert_eq!(at("c.txt"), ["other"]);

    // Removed from every file.
    dispatch(
        "tags.delete",
        serde_json::json!({ "tag": "X", "root": root_arg }),
    )
    .unwrap();
    let plan = proposal(&app);
    assert_eq!(plan.steps.len(), 1);
    app.executor.execute_blocking(&plan, app.lifetime());
    assert_eq!(at("a.txt"), ["Work"]);

    // What cannot be done says so at once.
    for args in [
        serde_json::json!({ "tag": "work", "to": "a, b" }),
        serde_json::json!({ "tag": "work", "to": "work" }),
        serde_json::json!({ "tag": " ", "to": "b" }),
    ] {
        assert!(dispatch("tags.rename", args.clone()).is_err(), "{args}");
    }
    // A tag no file has: a word, no plan.
    dispatch(
        "tags.delete",
        serde_json::json!({ "tag": "nowhere", "root": root_arg }),
    )
    .unwrap();
    until(&app, "the answer", || {
        app.take_notifications()
            .into_iter()
            .find(|n| n.text.starts_with("No file in"))
    });
    assert!(app.take_proposals().is_empty());
}

/// The sidebar's tags with whether each was pinned.
fn pins(app: &App) -> Vec<(String, bool)> {
    app.registry
        .places
        .all()
        .into_iter()
        .filter(|p| p.group == "tags" && p.color.is_some())
        .map(|p| {
            (
                p.title,
                !p.suggested && p.forget.is_some() && p.reorder.is_some(),
            )
        })
        .collect()
}

#[test]
fn pinned_tags_replace_the_suggestions_in_the_order_put() {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's home");
    std::fs::create_dir_all(&root).unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    let dispatch =
        |name: &str, args: serde_json::Value| app.dispatch(&Invocation::with_args(name, &args));
    // Tagged here, so the index learns them from the edits applied.
    for (name, t) in [("a.txt", &["Work", "x"][..]), ("b.txt", &["Été"][..])] {
        std::fs::write(root.join(name), b"x").unwrap();
        let entry = bibli_core::op::EntryRef::from_path(root.join(name)).unwrap();
        let Ok(bibli_core::command::Outcome::Plan(plan)) = dispatch(
            "tags.add",
            serde_json::json!({ "tags": t, "entries": [entry] }),
        ) else {
            panic!("a plan")
        };
        assert!(
            app.executor
                .execute_blocking(&plan, app.lifetime())
                .error
                .is_none()
        );
        assert_eq!(read_tags(&root.join(name)), t);
    }

    let until_sidebar = |want: &[(&str, bool)]| {
        let want: Vec<(String, bool)> = want.iter().map(|(t, p)| ((*t).to_owned(), *p)).collect();
        until(&app, "the sidebar", || (pins(&app) == want).then_some(()));
    };
    until_sidebar(&[("Work", false), ("x", false), ("Été", false)]);
    // Then every tag, the home folder's Tags view.
    let all = app
        .registry
        .places
        .find(|p| p.group == "tags" && p.color.is_none())
        .expect("All Tags");
    let open = all.activate.unwrap();
    assert_eq!(
        (&*open.name, &open.args["id"]),
        ("ui.view", &serde_json::json!("tags"))
    );

    // Pinned, the suggestions go; a pin goes last, or before another.
    dispatch("tags.pin", serde_json::json!({ "tag": "x" })).unwrap();
    until_sidebar(&[("x", true)]);
    dispatch(
        "tags.pin",
        serde_json::json!({ "tag": "Été", "before": "x" }),
    )
    .unwrap();
    until_sidebar(&[("Été", true), ("x", true)]);
    // Moved as the sidebar moves it: its reorder command, before another.
    let x = app.registry.places.find(|p| p.title == "x").unwrap();
    let mut moved = x.reorder.unwrap().command;
    moved.args["before"] = serde_json::json!("Été");
    app.dispatch(&moved).unwrap();
    until_sidebar(&[("x", true), ("Été", true)]);
    // Undone from the toast: back where it was.
    let undo = until(&app, "the toast", || {
        app.take_notifications()
            .into_iter()
            .rev()
            .find(|n| n.text == "Moved the tag “x”")
    });
    let undo = undo.action.expect("an Undo").command;
    app.dispatch(&undo).unwrap();
    until_sidebar(&[("Été", true), ("x", true)]);

    // Unpinned, both: the suggestions come back.
    let forget = app
        .registry
        .places
        .find(|p| p.title == "Été")
        .unwrap()
        .forget
        .unwrap();
    app.dispatch(&forget).unwrap();
    until_sidebar(&[("x", true)]);
    dispatch("tags.unpin", serde_json::json!({ "tag": "X" })).unwrap();
    until_sidebar(&[("Work", false), ("x", false), ("Été", false)]);
    assert!(dispatch("tags.pin", serde_json::json!({ "tag": " " })).is_err());
}

#[test]
fn the_tags_view_counts_the_files_under_its_folder_and_what_is_applied() {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's home");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::create_dir_all(tmp.path().join("elsewhere")).unwrap();
    for (name, t) in [
        ("a.txt", &["Work", "x"][..]),
        ("sub/b.txt", &["work"][..]),
        ("c.txt", &[][..]),
    ] {
        std::fs::write(root.join(name), b"x").unwrap();
        write_tags(&root.join(name), &tags(t)).unwrap();
    }
    let outside = tmp.path().join("elsewhere/d.txt");
    std::fs::write(&outside, b"x").unwrap();
    write_tags(&outside, &tags(&["away"])).unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    let dispatch =
        |name: &str, args: serde_json::Value| app.dispatch(&Invocation::with_args(name, &args));
    dispatch("tags.lens", serde_json::json!({ "pane": pane })).unwrap();
    let counted = |want: &[(&str, u64)]| {
        until(&app, "the counts", || {
            let Ok(bibli_core::command::Outcome::Value(v)) =
                dispatch("tags.lens_status", serde_json::json!({ "pane": pane }))
            else {
                panic!("a value")
            };
            let have: Vec<(String, u64)> = v["tags"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        r["tag"].as_str().unwrap().to_owned(),
                        r["files"].as_u64().unwrap(),
                    )
                })
                .collect();
            let want: Vec<(String, u64)> =
                want.iter().map(|(t, n)| ((*t).to_owned(), *n)).collect();
            (v["walking"] == false && have == want).then_some(v)
        })
    };
    let v = counted(&[("Work", 2), ("x", 1)]);
    assert_eq!(v["files"], 2);
    assert_eq!(v["tags"][0]["pinned"], false);
    assert!(v["tags"][0]["color"].is_string());

    // What is applied is counted as it is, without walking again.
    let entry = bibli_core::op::EntryRef::from_path(root.join("c.txt")).unwrap();
    let Ok(bibli_core::command::Outcome::Plan(plan)) = dispatch(
        "tags.add",
        serde_json::json!({ "tags": ["x", "new"], "entries": [entry] }),
    ) else {
        panic!("a plan")
    };
    assert!(
        app.executor
            .execute_blocking(&plan, app.lifetime())
            .error
            .is_none()
    );
    counted(&[("new", 1), ("Work", 2), ("x", 2)]);

    // Renamed, a file keeps counting; trashed, it is gone.
    let b = root.join("sub/b.txt");
    let mut plan = Plan::new("rename");
    plan.push(bibli_core::op::Rename {
        entry: bibli_core::op::EntryRef::from_path(&b).unwrap(),
        new_name: "it's b.txt".into(),
    });
    assert!(
        app.executor
            .execute_blocking(&plan, app.lifetime())
            .error
            .is_none()
    );
    let mut plan = Plan::new("trash");
    plan.push(bibli_core::op::Trash {
        entry: bibli_core::op::EntryRef::from_path(root.join("sub/it's b.txt")).unwrap(),
    });
    assert!(
        app.executor
            .execute_blocking(&plan, app.lifetime())
            .error
            .is_none()
    );
    counted(&[("new", 1), ("Work", 1), ("x", 2)]);

    // Let go and opened again, the count is kept: no walk.
    dispatch("tags.lens_release", serde_json::json!({ "pane": pane })).unwrap();
    dispatch("tags.lens", serde_json::json!({ "pane": pane })).unwrap();
    let Ok(bibli_core::command::Outcome::Value(v)) =
        dispatch("tags.lens_status", serde_json::json!({ "pane": pane }))
    else {
        panic!("a value")
    };
    assert_eq!(
        (v["walking"].as_bool(), v["tags"].as_array().map(Vec::len)),
        (Some(false), Some(3))
    );

    // Let go, it shows nothing.
    dispatch("tags.lens_release", serde_json::json!({ "pane": pane })).unwrap();
    let Ok(bibli_core::command::Outcome::Value(v)) =
        dispatch("tags.lens_status", serde_json::json!({ "pane": pane }))
    else {
        panic!("a value")
    };
    assert!(v["root"].is_null() && v["tags"].as_array().unwrap().is_empty());
}

#[test]
fn a_tags_pin_and_color_follow_it_renamed_everywhere_and_come_back_with_undo() {
    let tmp = home_tempdir();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    app.settings.set(
        "tags.pinned",
        toml::Value::Array(
            ["a", "trip", "b"]
                .map(|t| toml::Value::String(t.into()))
                .into(),
        ),
    );
    let mut colors = toml::Table::new();
    colors.insert("trip".into(), toml::Value::String("teal".into()));
    app.settings.set("tags.colors", toml::Value::Table(colors));
    // The step a rename on every file ends with, as a plan read back holds it.
    let step = app
        .ops
        .build(&bibli_core::op::OpData {
            kind: "tags_follow".into(),
            args: serde_json::json!({ "from": ["Trip"], "to": "travel", "root": PathArg::from(tmp.path()) }),
        })
        .unwrap();
    assert_eq!(
        step.describe().to_string(),
        "keep “travel” pinned and colored as “Trip” were"
    );
    let plan = Plan::with_steps("rename", vec![step]);
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    let pins = || app.settings.get::<Vec<String>>("tags.pinned").unwrap();
    let color = |t: &str| {
        app.settings
            .get::<toml::Table>("tags.colors")
            .unwrap()
            .get(t)
            .and_then(|c| c.as_str().map(str::to_owned))
    };
    assert_eq!(pins(), ["a", "travel", "b"]);
    assert_eq!(
        (color("travel").as_deref(), color("trip")),
        (Some("teal"), None)
    );
    until(&app, "the sidebar", || {
        (sidebar(&app) == ["a", "travel", "b"]).then_some(())
    });
    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert_eq!(pins(), ["a", "trip", "b"]);
    assert_eq!(
        (color("travel"), color("trip").as_deref()),
        (None, Some("teal"))
    );
}

#[test]
fn without_the_search_there_is_neither_a_tags_group_nor_a_rename() {
    let tmp = home_tempdir();
    let file = tmp.path().join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    write_tags(&file, &tags(&["work"])).unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    let err = app
        .dispatch(&Invocation::with_args(
            "tags.rename",
            &serde_json::json!({ "tag": "work", "to": "job" }),
        ))
        .unwrap_err();
    assert!(err.to_string().contains("Search plugin"), "{err}");
    assert!(sidebar(&app).is_empty());
}

/// What an open search shows, kept up to date from its events.
struct Shown {
    open: Arc<dyn OpenLocation>,
    names: std::collections::BTreeMap<bibli_core::entry::EntryId, String>,
    complete: bool,
}

impl Shown {
    fn new(app: &App, tag: &str, root: &std::path::Path) -> Self {
        let bibli_core::command::Outcome::Value(v) = app
            .dispatch(&Invocation::with_args(
                "query.location",
                &serde_json::json!({ "facet": "tag", "value": tag, "root": PathArg::from(root) }),
            ))
            .unwrap()
        else {
            panic!("a location")
        };
        let open = app
            .open_location(&Location::parse(v["location"].as_str().unwrap()))
            .unwrap();
        Shown {
            open,
            names: std::collections::BTreeMap::new(),
            complete: false,
        }
    }

    fn drain(&mut self) -> BTreeSet<String> {
        while let Ok(event) = self.open.events().try_recv() {
            match event {
                ListingEvent::Added(entries) => {
                    for e in entries {
                        self.names.insert(e.id, e.name_str().into_owned());
                    }
                }
                ListingEvent::Removed(ids) => {
                    for id in ids {
                        self.names.remove(&id);
                    }
                }
                ListingEvent::Complete => self.complete = true,
                _ => {}
            }
        }
        self.names.values().cloned().collect()
    }
}

#[test]
fn an_open_search_sees_at_once_what_a_plan_changed() {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's home");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    for (name, t) in [("a.txt", &["work"][..]), ("sub/b.txt", &["Work", "x"][..])] {
        std::fs::write(root.join(name), b"x").unwrap();
        write_tags(&root.join(name), &tags(t)).unwrap();
    }
    std::fs::write(root.join("c.txt"), b"x").unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    let mut work = Shown::new(&app, "work", &root);
    let mut job = Shown::new(&app, "Job", &root);
    until(&app, "both searches", || {
        (work.drain().len() == 2 && job.drain().is_empty() && work.complete && job.complete)
            .then_some(())
    });

    let root_arg = serde_json::json!(PathArg::from(root.as_path()));
    app.dispatch(&Invocation::with_args(
        "tags.rename",
        &serde_json::json!({ "tag": "work", "to": "Job", "root": root_arg }),
    ))
    .unwrap();
    let plan = proposal(&app);
    app.executor.execute_blocking(&plan, app.lifetime());
    let both: BTreeSet<String> = ["a.txt", "b.txt"].map(str::to_owned).into();
    until(&app, "the searches to follow", || {
        (work.drain().is_empty() && job.drain() == both).then_some(())
    });

    // A file tagged here joins the search it now matches; one moved away
    // from the root leaves it.
    let give = Invocation::with_args(
        "tags.add",
        &serde_json::json!({
            "tags": ["Job"],
            "entries": [bibli_core::op::EntryRef::from_path(root.join("c.txt")).unwrap()],
        }),
    );
    let bibli_core::command::Outcome::Plan(plan) = app.dispatch(&give).unwrap() else {
        panic!("a plan")
    };
    app.executor.execute_blocking(&plan, app.lifetime());
    until(&app, "c to join", || {
        job.drain().contains("c.txt").then_some(())
    });
    app.executor.undo_blocking(app.lifetime()).unwrap();
    until(&app, "c to leave", || {
        (!job.drain().contains("c.txt")).then_some(())
    });
}
