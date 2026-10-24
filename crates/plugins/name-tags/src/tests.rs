// Helpers of the tests that run on Unix alone (ADR-035).
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]
use super::*;
use skwir_core::app::AppConfig;
use skwir_core::command::Invocation;
use skwir_core::location::Location;
use skwir_core::op::EntryRef;
use std::time::Duration;

fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("name-tags-")
        .tempdir_in(base)
        .unwrap()
}

#[test]
fn names_split_and_join_as_each_convention_writes_them() {
    let t = Convention::Tagspaces;
    assert_eq!(
        t.split("IMG-2653[vacation alps].jpg"),
        (
            "IMG-2653".into(),
            vec!["vacation".into(), "alps".into()],
            Some("jpg".into())
        )
    );
    assert_eq!(
        t.split("it's [x].tar.gz"),
        ("it's [x].tar".into(), vec![], Some("gz".into())),
        "tags stand right before the extension"
    );
    assert_eq!(
        t.split("[only].txt").1,
        Vec::<String>::new(),
        "a name is more than tags"
    );
    assert_eq!(
        t.join("IMG", &["a".into(), "b".into()], Some("jpg")),
        "IMG[a b].jpg"
    );
    let f = Convention::Filetags;
    assert_eq!(
        f.split("2021-01-01 notes -- work draft.txt"),
        (
            "2021-01-01 notes".into(),
            vec!["work".into(), "draft".into()],
            Some("txt".into())
        )
    );
    assert_eq!(f.join("notes", &["x".into()], None), "notes -- x");
    assert!(!t.holds("two words") && !t.holds("[x]") && f.holds("[x]"));
}

fn app(tmp: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(tmp.join("state"))).unwrap();
    app.plugins.add(Arc::new(NameTagsPlugin), &app);
    app
}

/// A store of the file's own attributes (the desktop's), for the tags a
/// name gives away to stay somewhere.
struct Attributes;

impl NotesStore for Attributes {
    fn id(&self) -> &'static str {
        "attributes"
    }
    fn title(&self) -> &'static str {
        "Attributes"
    }
    fn kept(&self) -> Kept {
        Kept::Attributes
    }
    fn holds(&self) -> Holds {
        Holds {
            tags: true,
            ..Holds::default()
        }
    }
    fn default_write(&self) -> Write {
        Write::Create
    }
    fn read(&self, _: &Path, files: &[Probe<'_>], _: Option<&Listed<'_>>) -> Vec<Option<Notes>> {
        files
            .iter()
            .map(|f| {
                let v = skwir_core::fs::xattr::get(f.path, "user.test.tags").ok()??;
                let text = String::from_utf8(v).ok()?;
                Some(Notes {
                    tags: text.split(',').map(Tag::named).collect(),
                    ..Notes::default()
                })
            })
            .collect()
    }
    fn keys(&self, _: &Probe<'_>, _: bool) -> Vec<RawKey> {
        vec![RawKey::Attribute {
            name: "user.test.tags".into(),
        }]
    }
    fn write(&self, f: &Probe<'_>, notes: &Notes, _: bool) -> std::io::Result<Wrote> {
        if notes.tags.is_empty() {
            let _ = skwir_core::fs::xattr::remove(f.path, "user.test.tags");
        } else {
            skwir_core::fs::xattr::set(
                f.path,
                "user.test.tags",
                notes.tag_names().join(",").as_bytes(),
            )?;
        }
        Ok(Wrote::Written)
    }
}

fn plan(app: &App, name: &str, args: &serde_json::Value) -> Plan {
    match app.dispatch(&Invocation::with_args(name, args)).unwrap() {
        Outcome::Plan(plan) => plan,
        _ => panic!("a plan"),
    }
}

// Extended attributes: Unix's.
#[cfg(unix)]
#[test]
fn tagspaces_names_are_read_where_it_is_used_and_moved_in_and_out_of_names() {
    let tmp = home_tempdir();
    let used = tmp.path().join("location");
    let other = tmp.path().join("downloads");
    std::fs::create_dir_all(used.join(".ts")).unwrap();
    std::fs::create_dir_all(used.join("sub")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let tagged = used.join("sub/IMG[vacation alps].jpg");
    let video = other.join("A talk [dQw4w9WgXcQ].mp4");
    std::fs::write(&tagged, b"x").unwrap();
    std::fs::write(&video, b"x").unwrap();
    let app = app(tmp.path());
    app.register_notes_store("test", Arc::new(Attributes));
    let read = |p: &Path| app.notes.read_path(p, &app.notes.policy()).tag_names();
    assert_eq!(read(&tagged), ["vacation", "alps"]);
    assert!(read(&video).is_empty(), "no TagSpaces location here");

    // Out of the name: the tags stay, in the attributes.
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(used.join("sub")))
        .unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    let entries = serde_json::json!([EntryRef::from_path(&tagged).unwrap()]);
    let out = plan(
        &app,
        "names.untag",
        &serde_json::json!({ "entries": entries }),
    );
    let r = app.executor.execute_blocking(&out, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    let plain = used.join("sub/IMG.jpg");
    assert!(plain.exists() && !tagged.exists());
    assert_eq!(read(&plain), ["vacation", "alps"]);
    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(tagged.exists());
    assert!(
        skwir_core::fs::xattr::get(&tagged, "user.test.tags")
            .unwrap()
            .is_none()
    );

    // Into the name: those it can hold, the rest said to be left out.
    skwir_core::fs::xattr::set(&tagged, "user.test.tags", b"alps,new,two words").unwrap();
    let entries = serde_json::json!([EntryRef::from_path(&tagged).unwrap()]);
    let into = plan(
        &app,
        "names.tag",
        &serde_json::json!({ "entries": entries }),
    );
    assert!(into.note.as_deref().unwrap().contains("two words"));
    let r = app.executor.execute_blocking(&into, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(used.join("sub/IMG[vacation alps new].jpg").exists());
}

/// Two files that differ only by their tags would get one name: the plan
/// is refused before anything is renamed, never half applied.
#[test]
fn tags_out_of_names_refuse_two_files_one_name() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("location");
    std::fs::create_dir_all(dir.join(".ts")).unwrap();
    let first = dir.join("a[x].jpg");
    let second = dir.join("a[y].jpg");
    std::fs::write(&first, b"x").unwrap();
    std::fs::write(&second, b"y").unwrap();
    let app = app(tmp.path());
    app.register_notes_store("test", Arc::new(Attributes));
    let entries = serde_json::json!([
        EntryRef::from_path(&first).unwrap(),
        EntryRef::from_path(&second).unwrap()
    ]);
    let refused = app
        .dispatch(&Invocation::with_args(
            "names.untag",
            &serde_json::json!({ "entries": entries }),
        ))
        .expect_err("refused");
    assert!(refused.to_string().contains("“a.jpg”"), "{refused}");
    assert!(first.exists() && second.exists());
}

#[test]
fn a_name_holding_tags_follows_them_in_its_order() {
    let tags = |t: &[&str]| t.iter().map(|t| (*t).to_owned()).collect::<Vec<_>>();
    let t = Convention::Tagspaces;
    assert_eq!(
        t.retagged(
            "IMG[vacation alps].jpg",
            &tags(&["alps", "vacation", "new"])
        ),
        Some("IMG[vacation alps new].jpg".into()),
        "its own first, as they were"
    );
    assert_eq!(
        t.retagged("IMG[vacation alps].jpg", &tags(&["vacation"])),
        Some("IMG[vacation].jpg".into())
    );
    assert_eq!(
        t.retagged("IMG[vacation].jpg", &[]),
        Some("IMG.jpg".into()),
        "the last tag taken leaves the plain name"
    );
    assert_eq!(
        t.retagged("IMG[Vacation alps].jpg", &tags(&["alps", "vacation"])),
        None,
        "case aside, it says them already"
    );
    assert_eq!(
        t.retagged("IMG[a  a b].jpg", &tags(&["b", "a"])),
        None,
        "a name is renamed for its tags, never its spelling"
    );
    assert_eq!(
        t.retagged(
            "IMG[a].jpg",
            &tags(&["a", "two words", "Places/France", "[x]"])
        ),
        None,
        "what a name cannot hold stays in the other stores"
    );
    assert_eq!(
        t.retagged("IMG.jpg", &tags(&["a"])),
        None,
        "a name is never started"
    );
    let f = Convention::Filetags;
    assert_eq!(
        f.retagged("notes -- work.txt", &tags(&["work", "[draft]"])),
        Some("notes -- work [draft].txt".into())
    );
}

/// An edit's plan, applied all or nothing as a gesture applies it, and
/// how it ended.
fn apply_whole(app: &App, plan: Plan) -> skwir_core::op::ExecEvent {
    // What an undo before said is not this plan's.
    while app.executor.events().try_recv().is_ok() {}
    app.apply_whole(plan);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let event = app
            .executor
            .events()
            .recv_deadline(deadline)
            .expect("the plan finishes");
        if matches!(event, skwir_core::op::ExecEvent::Finished { .. }) {
            return event;
        }
    }
}

fn edit_plan(app: &App, files: &[&Path], add: &[&str], remove: &[&str]) -> Plan {
    let change = skwir_core::notes::TagChange {
        add: add.iter().map(|t| (*t).to_owned()).collect(),
        remove: remove.iter().map(|t| (*t).to_owned()).collect(),
        ..Default::default()
    };
    let mut plan = Plan::new("change tags");
    for f in files {
        plan.push(
            app.notes
                .edit(EntryRef::from_path(f).unwrap())
                .change_tags(change.clone()),
        );
    }
    plan
}

fn attribute(path: &Path) -> Option<String> {
    let v = skwir_core::fs::xattr::get(path, "user.test.tags").unwrap()?;
    Some(String::from_utf8(v).unwrap())
}

fn finished_with(event: &skwir_core::op::ExecEvent) -> (usize, Option<String>) {
    match event {
        skwir_core::op::ExecEvent::Finished { applied, error, .. } => (*applied, error.clone()),
        _ => unreachable!(),
    }
}

// Extended attributes: Unix's.
#[cfg(unix)]
#[test]
fn an_edit_renames_a_name_holding_tags_once_set_to_update_it() {
    let tmp = home_tempdir();
    let used = tmp.path().join("it's a location");
    let other = tmp.path().join("downloads");
    std::fs::create_dir_all(used.join(".ts")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let tagged = used.join("IMG[vacation alps].jpg");
    let untagged = used.join("untagged.jpg");
    let video = other.join("A talk [dQw4w9WgXcQ].mp4");
    for f in [&tagged, &untagged, &video] {
        std::fs::write(f, b"x").unwrap();
    }
    let app = app(tmp.path());
    app.register_notes_store("test", Arc::new(Attributes));
    assert!(
        app.settings
            .schema()
            .iter()
            .any(|s| s.key == "notes.tagspaces-names.write"),
        "a name is written as an edit changes it, when asked"
    );

    // Off, as by default: the name stays, the attributes keep the tags.
    let r = apply_whole(&app, edit_plan(&app, &[&tagged], &["new"], &[]));
    assert_eq!(finished_with(&r), (1, None));
    assert!(tagged.exists());
    assert_eq!(attribute(&tagged).as_deref(), Some("vacation,alps,new"));
    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert_eq!(attribute(&tagged), None);

    app.settings.set(
        "notes.tagspaces-names.write",
        toml::Value::String("update".into()),
    );
    app.apply_notes_settings();

    // The review shows the rename beside the edit, before anything is done.
    let plan = edit_plan(&app, &[&tagged], &["new"], &[]);
    let diff = plan.diff(&app.companions);
    let renamed = used.join("IMG[vacation alps new].jpg");
    assert!(
        matches!(
            &diff.steps[..],
            [
                (skwir_core::op::Change::Metadata { .. }, _),
                (skwir_core::op::Change::Rename { from, to }, _),
            ] if *from == tagged && *to == renamed
        ),
        "{:?}",
        diff.steps
    );
    assert!(tagged.exists(), "a review changes nothing");

    let r = apply_whole(&app, plan.clone());
    assert_eq!(finished_with(&r), (2, None));
    assert!(renamed.exists() && !tagged.exists());
    assert_eq!(attribute(&renamed).as_deref(), Some("vacation,alps,new"));
    // Applied again, nothing changes.
    let r = apply_whole(&app, plan);
    assert_eq!(finished_with(&r), (0, None));
    assert!(renamed.exists());

    // Undo takes back the name and the attributes, Redo makes both again.
    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(tagged.exists() && !renamed.exists());
    assert_eq!(attribute(&tagged), None);
    let r = app.executor.redo_blocking(app.lifetime()).unwrap();
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(renamed.exists() && !tagged.exists());
    assert_eq!(attribute(&renamed).as_deref(), Some("vacation,alps,new"));

    // Taking a tag takes it from the name; one a name cannot hold leaves
    // the name as it is.
    let r = apply_whole(&app, edit_plan(&app, &[&renamed], &[], &["alps"]));
    assert_eq!(finished_with(&r), (2, None));
    let fewer = used.join("IMG[vacation new].jpg");
    assert!(fewer.exists());
    let r = apply_whole(&app, edit_plan(&app, &[&fewer], &["Places/France"], &[]));
    assert_eq!(finished_with(&r), (1, None));
    assert!(fewer.exists());
    assert_eq!(
        app.notes.read_path(&fewer, &app.notes.policy()).tag_names(),
        ["vacation", "new", "Places/France"]
    );

    // A name holding no tags, or one where TagSpaces is not used, is never
    // renamed.
    let r = apply_whole(&app, edit_plan(&app, &[&untagged, &video], &["x"], &[]));
    assert_eq!(finished_with(&r), (2, None));
    assert!(untagged.exists() && video.exists());
    assert_eq!(attribute(&video).as_deref(), Some("x"));
}

/// A name taken, before or by the plan itself: nothing is done, never
/// half the plan.
#[cfg(unix)]
#[test]
fn an_edit_renaming_onto_a_name_taken_does_nothing() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("location");
    std::fs::create_dir_all(dir.join(".ts")).unwrap();
    let app = app(tmp.path());
    app.register_notes_store("test", Arc::new(Attributes));
    app.settings.set(
        "notes.tagspaces-names.write",
        toml::Value::String("update".into()),
    );
    app.apply_notes_settings();
    let names = ["a[x].jpg", "a[x y].jpg", "b[x].jpg", "b[y].jpg"];
    for n in names {
        std::fs::write(dir.join(n), n).unwrap();
    }
    let path = |n: &str| dir.join(n);

    // `a[x].jpg` given `y` would be `a[x y].jpg`, which is there.
    let r = apply_whole(&app, edit_plan(&app, &[&path("a[x].jpg")], &["y"], &[]));
    let (applied, error) = finished_with(&r);
    assert_eq!(applied, 0);
    assert!(error.unwrap().contains("“a[x y].jpg”"));
    assert_eq!(attribute(&path("a[x].jpg")), None);

    // Both left with no tag would both be `b.jpg`.
    let r = apply_whole(
        &app,
        edit_plan(
            &app,
            &[&path("b[x].jpg"), &path("b[y].jpg")],
            &[],
            &["x", "y"],
        ),
    );
    let (applied, error) = finished_with(&r);
    assert_eq!(applied, 0);
    assert!(error.unwrap().contains("“b.jpg”"));
    for n in names {
        assert_eq!(std::fs::read_to_string(path(n)).unwrap(), n);
        assert_eq!(attribute(&path(n)), None);
    }
}

/// From the window's thread, nothing is read there: the plan is made on a
/// worker, then applied as the user's own action.
#[cfg(unix)]
#[test]
fn from_the_window_tags_go_into_names_off_its_thread() {
    let tmp = home_tempdir();
    let used = tmp.path().join("it's a location");
    std::fs::create_dir_all(used.join(".ts")).unwrap();
    let file = used.join("Été.jpg");
    std::fs::write(&file, b"x").unwrap();
    let app = app(tmp.path());
    app.register_notes_store("test", Arc::new(Attributes));
    skwir_core::fs::xattr::set(&file, "user.test.tags", b"alps").unwrap();
    skwir_core::scheduler::mark_window_thread(&app.scheduler);
    let entries = serde_json::json!([EntryRef::from_path(&file).unwrap()]);
    let answered = app
        .dispatch(&Invocation::with_args(
            "names.tag",
            &serde_json::json!({ "entries": entries }),
        ))
        .unwrap();
    assert!(matches!(answered, Outcome::Done), "no plan made here");
    let renamed = used.join("Été[alps].jpg");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !renamed.exists() {
        assert!(std::time::Instant::now() < deadline, "never renamed");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!file.exists());
}
