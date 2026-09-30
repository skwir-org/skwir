use super::*;
use bibli_core::app::AppConfig;
use bibli_core::op::{EntryRef, Plan, Rename};

fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("xmp-")
        .tempdir_in(base)
        .unwrap()
}

fn app(tmp: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(tmp.join("state"))).unwrap();
    app.plugins.add(Arc::new(XmpPlugin), &app);
    app
}

fn run(app: &App, op: Box<dyn bibli_core::op::Operation>) -> Option<String> {
    let mut plan = Plan::new("op");
    plan.steps.push(op);
    app.executor.execute_blocking(&plan, app.lifetime()).error
}

const SIDECAR: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/" xmp:Rating="2">
   <dc:subject><rdf:Bag><rdf:li>trip</rdf:li></rdf:Bag></dc:subject>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;

#[test]
fn a_sidecar_is_read_updated_and_undone_and_created_only_when_asked() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("photos");
    std::fs::create_dir(&dir).unwrap();
    let (a, b) = (dir.join("it's a.jpg"), dir.join("b.jpg"));
    std::fs::write(&a, b"x").unwrap();
    std::fs::write(&b, b"x").unwrap();
    std::fs::write(dir.join("it's a.jpg.xmp"), SIDECAR).unwrap();
    let app = app(tmp.path());
    let read = |p: &Path| app.notes.read_path(p, &app.notes.policy());
    assert_eq!(read(&a).tag_names(), ["trip"]);
    assert_eq!(read(&a).rating, Some(4));

    let edit = app
        .notes
        .edit(EntryRef::from_path(&a).unwrap())
        .tags(vec!["trip".into(), "Alps".into()])
        .rating(Some(9));
    assert_eq!(run(&app, Box::new(edit)), None);
    let written = std::fs::read_to_string(dir.join("it's a.jpg.xmp")).unwrap();
    assert!(written.contains("<rdf:li>Alps</rdf:li>") && written.contains(r#"xmp:Rating="5""#));
    assert_eq!(read(&a).tag_names(), ["trip", "Alps"]);
    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("it's a.jpg.xmp")).unwrap(),
        SIDECAR
    );

    // Without a sidecar and with the store set to update only: nothing
    // to keep the notes in, and the edit says so.
    let edit = app
        .notes
        .edit(EntryRef::from_path(&b).unwrap())
        .tags(vec!["x".into()]);
    assert!(run(&app, Box::new(edit)).is_some());
    app.settings
        .set("notes.xmp.write", toml::Value::String("create".into()));
    app.apply_notes_settings();
    let edit = app
        .notes
        .edit(EntryRef::from_path(&b).unwrap())
        .tags(vec!["x".into()]);
    assert_eq!(run(&app, Box::new(edit)), None);
    assert_eq!(read(&b).tag_names(), ["x"]);
    assert!(dir.join("b.jpg.xmp").exists());
}

#[test]
fn a_sidecar_follows_its_file_adobes_only_while_its_stem_is_the_files_alone() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("photos");
    std::fs::create_dir(&dir).unwrap();
    for name in [
        "a.jpg",
        "a.jpg.xmp",
        "b.CR3",
        "b.xmp",
        "c.CR3",
        "c.JPG",
        "c.xmp",
    ] {
        std::fs::write(dir.join(name), SIDECAR).unwrap();
    }
    let app = app(tmp.path());
    let rename = |from: &str, to: &str| {
        run(
            &app,
            Box::new(Rename {
                entry: EntryRef::from_path(dir.join(from)).unwrap(),
                new_name: to.into(),
            }),
        )
    };
    assert_eq!(rename("a.jpg", "it's a.jpg"), None);
    assert!(dir.join("it's a.jpg.xmp").exists() && !dir.join("a.jpg.xmp").exists());
    assert_eq!(rename("b.CR3", "d.CR3"), None);
    assert!(
        dir.join("d.xmp").exists(),
        "Adobe's name, its stem the raw's alone"
    );
    assert_eq!(rename("c.CR3", "e.CR3"), None);
    assert!(dir.join("c.xmp").exists(), "c.JPG still claims it");
    // The listing's rule: only `x.jpg.xmp` names its file.
    assert_eq!(
        XmpCompanion.owner(OsStr::new("a.jpg.xmp")).as_deref(),
        Some(OsStr::new("a.jpg"))
    );
    assert_eq!(XmpCompanion.owner(OsStr::new("a.xmp")), None);
    assert_eq!(XmpCompanion.owner(OsStr::new("a.jpg")), None);
}

#[test]
fn xmp_inside_a_file_is_read_for_details() {
    let tmp = home_tempdir();
    let f = tmp.path().join("it's in.jpg");
    let mut bytes = b"\xff\xd8\xff\xe1\x00\x10http://ns.adobe.com/xap/1.0/\0".to_vec();
    bytes.extend(SIDECAR.as_bytes());
    std::fs::write(&f, bytes).unwrap();
    assert!(Embedded.accepts("xmp.embedded", OsStr::new("it's in.JPG")));
    assert!(!Embedded.accepts("xmp.embedded", OsStr::new("notes.txt")));
    let value = Embedded
        .read("xmp.embedded", &f, &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(value.as_str(), Some("Keywords trip · 2 stars"));
}
