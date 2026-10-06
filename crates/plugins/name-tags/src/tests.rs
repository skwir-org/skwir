// Helpers of the tests that run on Unix alone (ADR-035).
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]
use super::*;
use bibli_core::app::AppConfig;
use bibli_core::command::Invocation;
use bibli_core::location::Location;
use std::time::Duration;

fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
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
                let v = bibli_core::fs::xattr::get(f.path, "user.test.tags").ok()??;
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
            let _ = bibli_core::fs::xattr::remove(f.path, "user.test.tags");
        } else {
            bibli_core::fs::xattr::set(
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
    assert!(
        !app.settings
            .schema()
            .iter()
            .any(|s| s.key == "notes.tagspaces-names.write"),
        "a name is written by renaming"
    );

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
        bibli_core::fs::xattr::get(&tagged, "user.test.tags")
            .unwrap()
            .is_none()
    );

    // Into the name: those it can hold, the rest said to be left out.
    bibli_core::fs::xattr::set(&tagged, "user.test.tags", b"alps,new,two words").unwrap();
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
