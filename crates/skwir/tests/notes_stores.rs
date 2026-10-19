//! A file's notes kept in several stores of several plugins (ADR-027): the
//! desktop's attributes (the tags plugin's) and an XMP sidecar (the xmp
//! plugin's) read as one, edited together, and another application's later
//! edit winning; the sidecar hidden beside its file (ADR-028).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::Location;

fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("notes-stores-")
        .tempdir_in(base)
        .unwrap()
}

const SIDECAR: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/">
   <dc:subject><rdf:Bag><rdf:li>trip</rdf:li></rdf:Bag></dc:subject>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;

fn app(tmp: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(tmp.join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    app.plugins.add(Arc::new(skwir_plugin_xmp::XmpPlugin), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tagspaces::TagSpacesPlugin), &app);
    app
}

#[test]
fn the_desktops_tags_and_a_sidecar_are_one_set_of_notes() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("photos");
    std::fs::create_dir(&dir).unwrap();
    let photo = dir.join("it's a.jpg");
    std::fs::write(&photo, b"x").unwrap();
    std::fs::write(dir.join("it's a.jpg.xmp"), SIDECAR).unwrap();
    skwir_plugin_tags::write_tags(&photo, &["mine".into()]).unwrap();
    let app = app(tmp.path());
    let read = || app.notes.read_path(&photo, &app.notes.policy()).tag_names();
    assert_eq!(read(), ["mine", "trip"]);

    // An edit here reaches both.
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&dir)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    app.dispatch(&Invocation::with_args(
        "select",
        &serde_json::json!({ "names": ["it's a.jpg"] }),
    ))
    .unwrap();
    let Outcome::Plan(plan) = app
        .dispatch(&Invocation::with_args(
            "tags.add",
            &serde_json::json!({ "tags": ["Alps"] }),
        ))
        .unwrap()
    else {
        panic!("a plan")
    };
    assert!(
        app.executor
            .execute_blocking(&plan, app.lifetime())
            .error
            .is_none()
    );
    assert_eq!(
        skwir_core::fs::xattr::get(&photo, "user.xdg.tags")
            .unwrap()
            .as_deref(),
        Some(b"mine,trip,Alps".as_slice())
    );
    let sidecar = std::fs::read_to_string(dir.join("it's a.jpg.xmp")).unwrap();
    assert!(sidecar.contains("<rdf:li>Alps</rdf:li>") && sidecar.contains("<rdf:li>mine</rdf:li>"));

    // In a photo application, `trip` is taken away: its edit wins.
    std::fs::write(
        dir.join("it's a.jpg.xmp"),
        sidecar.replace("<rdf:li>trip</rdf:li>", ""),
    )
    .unwrap();
    let now = app.notes.read_one(&photo, &app.notes.policy());
    assert_eq!(now.notes.tag_names(), ["mine", "Alps"]);
    assert_eq!(now.differ, ["xdg"]);

    // The sidecar is not listed beside its file, even with hidden files.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.pump();
        let mut ws = app.workspace.write();
        let p = ws.pane_mut(pane).unwrap();
        p.sort.show_hidden = true;
        let names: Vec<String> = p
            .order()
            .iter()
            .filter_map(|id| Some(p.entry(*id)?.name_str().into_owned()))
            .collect();
        if names == ["it's a.jpg"] {
            break;
        }
        drop(ws);
        assert!(Instant::now() < deadline, "{names:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn one_edit_reaches_every_store_that_keeps_the_files_notes() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("docs");
    std::fs::create_dir_all(dir.join(".ts")).unwrap();
    let file = dir.join("it's a.jpg");
    std::fs::write(&file, b"x").unwrap();
    std::fs::write(dir.join("it's a.jpg.xmp"), SIDECAR).unwrap();
    std::fs::write(
        dir.join(".ts/it's a.jpg.json"),
        r##"{"id":"1","tags":[{"title":"trip","color":"#43a047"}]}"##,
    )
    .unwrap();
    let app = app(tmp.path());
    let read = || app.notes.read_one(&file, &app.notes.policy());
    assert_eq!(read().notes.tag_names(), ["trip"]);
    assert_eq!(
        read().notes.tag("trip").unwrap().color.as_deref(),
        Some("green")
    );
    let edit = app
        .notes
        .edit(skwir_core::op::EntryRef::from_path(&file).unwrap())
        .tags(vec!["trip".into(), "Été".into()])
        .comment(Some("the lake".into()));
    let mut plan = skwir_core::op::Plan::new("edit");
    plan.steps.push(Box::new(edit));
    assert!(
        app.executor
            .execute_blocking(&plan, app.lifetime())
            .error
            .is_none()
    );
    assert_eq!(
        skwir_core::fs::xattr::get(&file, "user.xdg.tags")
            .unwrap()
            .as_deref(),
        Some("trip,Été".as_bytes())
    );
    let xmp = std::fs::read_to_string(dir.join("it's a.jpg.xmp")).unwrap();
    assert!(xmp.contains("<rdf:li>Été</rdf:li>") && xmp.contains("the lake"));
    let ts: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join(".ts/it's a.jpg.json")).unwrap()).unwrap();
    assert_eq!(ts["tags"][1]["title"], "Été");
    assert_eq!(ts["description"], "the lake");
    assert!(read().differ.is_empty(), "{:?}", read().differ);
    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("it's a.jpg.xmp")).unwrap(),
        SIDECAR
    );
    assert!(
        skwir_core::fs::xattr::get(&file, "user.xdg.tags")
            .unwrap()
            .is_none()
    );
}

/// A name holding its tags, as `TagSpaces` writes them, set to follow them
/// (Tags and notes page): the tags plugin's edit renames the file beside the
/// desktop's attributes, the pane keeps it selected under its new name,
/// and Undo takes back both.
#[test]
fn tags_added_reach_a_name_holding_tags_once_it_is_set_to_follow_them() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("Été d'or");
    std::fs::create_dir_all(dir.join(".ts")).unwrap();
    let photo = dir.join("it's a photo[trip].jpg");
    std::fs::write(&photo, b"x").unwrap();
    let app = app(tmp.path());
    app.plugins
        .add(Arc::new(skwir_plugin_name_tags::NameTagsPlugin), &app);
    app.settings.set(
        "notes.tagspaces-names.write",
        toml::Value::String("update".into()),
    );
    app.apply_notes_settings();

    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&dir)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    app.dispatch(&Invocation::with_args(
        "select",
        &serde_json::json!({ "names": ["it's a photo[trip].jpg"] }),
    ))
    .unwrap();
    let Outcome::Plan(plan) = app
        .dispatch(&Invocation::with_args(
            "tags.add",
            &serde_json::json!({ "tags": ["Alps", "Places/France"] }),
        ))
        .unwrap()
    else {
        panic!("a plan")
    };
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    let renamed = dir.join("it's a photo[trip Alps].jpg");
    assert!(renamed.exists() && !photo.exists());
    assert_eq!(
        skwir_core::fs::xattr::get(&renamed, "user.xdg.tags")
            .unwrap()
            .as_deref(),
        Some(b"trip,Alps,Places/France".as_slice()),
        "a category stays in the attributes"
    );

    // The pane follows the rename, the file still selected.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.pump();
        let selected: Vec<_> = app
            .selected_refs(pane)
            .into_iter()
            .map(|r| r.path)
            .collect();
        if selected == [renamed.clone()] {
            break;
        }
        assert!(Instant::now() < deadline, "{selected:?}");
        std::thread::sleep(Duration::from_millis(5));
    }

    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(photo.exists() && !renamed.exists());
    assert_eq!(
        skwir_core::fs::xattr::get(&photo, "user.xdg.tags").unwrap(),
        None
    );
}
