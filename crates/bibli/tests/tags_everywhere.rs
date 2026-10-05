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
    // It opens the tag's folder of the home folder (ADR-032).
    assert_eq!(job.location.to_string(), "tags:/Job");
    assert!(job.color.is_some());
    // Files dropped on it get it: what its tag folder takes.
    let drop = app
        .verbs(&job.location)
        .drop
        .expect("a tag takes files dropped on it");
    assert_eq!(&*drop.name, "tags.drop");
    assert_eq!(drop.args["location"], serde_json::json!("tags:/Job"));

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
    // Then every tag: the home folder's tags, browsed as folders.
    let all = app
        .registry
        .places
        .find(|p| p.group == "tags" && p.color.is_none())
        .expect("All Tags");
    assert_eq!(all.location.to_string(), "tags:/");
    assert!(all.activate.is_none());

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

/// The tags `root`'s tag location lists at its top, with how many files
/// each holds, by name.
fn tags_at_top(app: &App, pane: bibli_core::workspace::PaneId) -> Vec<(String, u64)> {
    let ws = app.workspace.read();
    let mut have: Vec<(String, u64)> = ws
        .pane(pane)
        .unwrap()
        .listing
        .iter()
        .filter(|e| e.is_place())
        .map(|e| {
            let files = e
                .metadata
                .value(bibli_core::entry::keys::FILES)
                .and_then(bibli_core::Value::as_u64)
                .unwrap_or_default();
            (e.name_str().into_owned(), files)
        })
        .collect();
    have.sort_by_key(|(t, _)| t.to_lowercase());
    have
}

/// The top of `root`'s tags, shown in `pane`, listed whole.
fn show_tags_of(app: &App, pane: bibli_core::workspace::PaneId, root: &std::path::Path) {
    let location = bibli_plugin_tags::location::Address {
        scope: Some(root.to_path_buf()),
        ..Default::default()
    }
    .location();
    app.navigate(pane, &location).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
}

#[test]
fn a_tag_location_counts_the_files_under_its_folder_and_what_is_applied() {
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
    let dispatch =
        |name: &str, args: serde_json::Value| app.dispatch(&Invocation::with_args(name, &args));
    show_tags_of(&app, pane, &root);
    let counted = |want: &[(&str, u64)]| {
        let want: Vec<(String, u64)> = want.iter().map(|(t, n)| ((*t).to_owned(), *n)).collect();
        until(&app, "the counts", || {
            (tags_at_top(&app, pane) == want).then_some(())
        });
    };
    counted(&[("Work", 2), ("x", 1)]);

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

    // Shown again, the folder is covered: the counts are those of the
    // count, as of when it began.
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    show_tags_of(&app, pane, &root);
    counted(&[("new", 1), ("Work", 1), ("x", 2)]);
    let Ok(bibli_core::command::Outcome::Value(v)) = dispatch(
        "tags.counted",
        serde_json::json!({ "root": PathArg::from(root.as_path()) }),
    ) else {
        panic!("a value")
    };
    assert!(v["counted"].is_u64() && v["counting"] == false, "{v}");
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
fn a_folder_counted_is_checked_after_a_restart_for_what_others_changed() {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's home");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    for (name, t) in [("a.txt", &["work"][..]), ("sub/b.txt", &["work", "x"][..])] {
        std::fs::write(root.join(name), b"x").unwrap();
        write_tags(&root.join(name), &tags(t)).unwrap();
    }
    let start = || {
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
        app.plugins
            .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        // Complete once counted, or checked when a count covers it.
        show_tags_of(&app, pane, &root);
        (app, pane)
    };
    let owned = |v: &[(&str, u64)]| {
        v.iter()
            .map(|(t, n)| ((*t).to_owned(), *n))
            .collect::<Vec<_>>()
    };
    {
        let (app, pane) = start();
        assert_eq!(tags_at_top(&app, pane), owned(&[("work", 2), ("x", 1)]));
        // The index, covering the folder, is written for the next run.
        let slot = bibli_core::cache::Cache::new(tmp.path().join("state/cache")).namespace("tags");
        until(&app, "the index written", || {
            slot.get(b"index.4")
                .filter(|b| String::from_utf8_lossy(b).contains("it's home"))
                .filter(|b| String::from_utf8_lossy(b).contains("\"covered\":[{"))
        });
    }
    // Closed, another application tags, untags, and adds a file.
    std::thread::sleep(Duration::from_millis(20));
    write_tags(&root.join("a.txt"), &tags(&["work", "late"])).unwrap();
    write_tags(&root.join("sub/b.txt"), &[]).unwrap();
    std::fs::write(root.join("sub/c.txt"), b"x").unwrap();
    write_tags(&root.join("sub/c.txt"), &tags(&["x"])).unwrap();
    let (app, pane) = start();
    until(&app, "the counts checked", || {
        (tags_at_top(&app, pane) == owned(&[("late", 1), ("work", 1), ("x", 1)])).then_some(())
    });
}

#[test]
fn without_the_search_a_tag_is_not_renamed_everywhere() {
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
    // Finding every file that has it is the search's; the sidebar's tags
    // are not, being the tags plugin's own locations (ADR-032).
    assert!(err.to_string().contains("Search plugin"), "{err}");
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

/// A category renamed takes the tags within it; tags moved into one keep
/// their own names; their pins follow.
#[test]
fn a_category_carries_its_tags_and_tags_move_into_one() {
    let tmp = home_tempdir();
    let root = tmp.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    for (name, t) in [
        ("a.jpg", &["Places/France/Paris", "work"][..]),
        ("b.jpg", &["Places/Japan"][..]),
        ("c.jpg", &["Paris", "Placesque"][..]),
    ] {
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
        serde_json::json!({ "tag": "places", "to": "Lieux", "root": root_arg }),
    )
    .unwrap();
    let plan = proposal(&app);
    assert_eq!(plan.steps.len(), 2, "a and b; not Placesque");
    app.executor.execute_blocking(&plan, app.lifetime());
    assert_eq!(at("a.jpg"), ["Lieux/France/Paris", "work"]);
    assert_eq!(at("b.jpg"), ["Lieux/Japan"]);
    assert_eq!(at("c.jpg"), ["Paris", "Placesque"]);

    dispatch(
        "tags.move",
        serde_json::json!({ "tags": ["Paris", "work"], "into": " Lieux / France ", "root": root_arg }),
    )
    .unwrap();
    let plan = proposal(&app);
    assert_eq!(
        plan.title,
        "move the tags “Paris” and “work” into “Lieux/France”"
    );
    app.executor.execute_blocking(&plan, app.lifetime());
    assert_eq!(at("a.jpg"), ["Lieux/France/Paris", "Lieux/France/work"]);
    assert_eq!(at("c.jpg"), ["Lieux/France/Paris", "Placesque"]);

    // Out of its category, to the top.
    dispatch(
        "tags.move",
        serde_json::json!({ "tag": "Lieux/Japan", "root": root_arg }),
    )
    .unwrap();
    let plan = proposal(&app);
    app.executor.execute_blocking(&plan, app.lifetime());
    assert_eq!(at("b.jpg"), ["Japan"]);

    // Removed, a category takes its tags with it.
    dispatch(
        "tags.delete",
        serde_json::json!({ "tag": "Lieux", "root": root_arg }),
    )
    .unwrap();
    let plan = proposal(&app);
    app.executor.execute_blocking(&plan, app.lifetime());
    assert!(at("a.jpg").is_empty());
    assert_eq!(at("c.jpg"), ["Placesque"]);

    for args in [
        serde_json::json!({ "tag": "Placesque", "into": "Placesque/x" }),
        serde_json::json!({ "tag": "Japan" }),
    ] {
        assert!(dispatch("tags.move", args.clone()).is_err(), "{args}");
    }
}

/// A category marked as holding one tag at a time is a field: a value
/// given takes the place of the one there was, and the mark follows the
/// category renamed.
#[test]
fn a_field_holds_one_value_and_follows_its_category() {
    let tmp = home_tempdir();
    let root = tmp.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("report.pdf");
    std::fs::write(&file, b"x").unwrap();
    write_tags(&file, &tags(&["Status/Draft", "work"])).unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    let dispatch =
        |name: &str, args: serde_json::Value| app.dispatch(&Invocation::with_args(name, &args));
    let fields = || {
        app.settings
            .get::<Vec<String>>("tags.exclusive")
            .unwrap_or_default()
    };
    dispatch(
        "tags.exclusive",
        serde_json::json!({ "tag": "Status", "on": true }),
    )
    .unwrap();
    until(&app, "the field", || (fields() == ["Status"]).then_some(()));
    let entry = bibli_core::op::EntryRef::from_path(&file).unwrap();
    let plan = match dispatch(
        "tags.add",
        serde_json::json!({ "tags": ["Status/Final"], "entries": [entry] }),
    )
    .unwrap()
    {
        bibli_core::command::Outcome::Plan(plan) => plan,
        other => panic!("a plan, not {other:?}"),
    };
    app.executor.execute_blocking(&plan, app.lifetime());
    assert_eq!(read_tags(&file), ["work", "Status/Final"]);

    dispatch(
        "tags.rename",
        serde_json::json!({ "tag": "Status", "to": "State", "root": PathArg::from(root.as_path()) }),
    )
    .unwrap();
    let plan = proposal(&app);
    assert!(plan.note.is_some());
    // Under a folder other than the home folder the mark stays: the
    // category still names files elsewhere.
    app.executor.execute_blocking(&plan, app.lifetime());
    assert_eq!(read_tags(&file), ["work", "State/Final"]);
    assert_eq!(fields(), ["Status"]);
}
