//! A file's notes kept in several stores of several plugins (ADR-027): the
//! desktop's attributes (the tags plugin's) and an XMP sidecar (the xmp
//! plugin's) read as one, edited together, and another application's later
//! edit winning; the sidecar hidden beside its file (ADR-028).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bibli_core::app::{App, AppConfig};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;

fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
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
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    app.plugins.add(Arc::new(bibli_plugin_xmp::XmpPlugin), &app);
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
    bibli_plugin_tags::write_tags(&photo, &["mine".into()]).unwrap();
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
        xattr::get(&photo, "user.xdg.tags").unwrap().as_deref(),
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
