use super::*;
use bibli_core::app::AppConfig;
use bibli_core::op::{EntryRef, Plan, Rename};

fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("tagspaces-")
        .tempdir_in(base)
        .unwrap()
}

fn app(tmp: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(tmp.join("state"))).unwrap();
    app.plugins.add(Arc::new(TagSpacesPlugin), &app);
    app
}

fn run(app: &App, op: Box<dyn bibli_core::op::Operation>) -> Option<String> {
    let mut plan = Plan::new("op");
    plan.steps.push(op);
    app.executor.execute_blocking(&plan, app.lifetime()).error
}

/// A sidecar as `TagSpaces` documents it.
const SIDECAR: &str = r##"{
  "id": "4194969c6bb84ad3acac779645c90e70",
  "tags": [
    { "title": "tag1", "type": "sidecar", "color": "#ffcc24", "textcolor": "#ffffff" },
    { "title": "done", "type": "sidecar", "color": "#008000ff", "textcolor": "white" }
  ],
  "description": "# Some description\n\nin *markdown* format"
}"##;

#[test]
fn colors_read_as_the_nearest_of_the_palette() {
    assert_eq!(nearest("#ffcc24"), Some("yellow"));
    assert_eq!(nearest("#008000ff"), Some("green"));
    assert_eq!(nearest("#e53935"), Some("red"));
    assert_eq!(nearest("#a47ae2"), Some("purple"));
    assert_eq!(nearest("#cccccc"), None, "a gray");
    assert_eq!(nearest("white"), None);
}

#[test]
fn a_sidecar_is_read_edited_in_place_and_undone() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("docs");
    std::fs::create_dir_all(dir.join(".ts")).unwrap();
    let file = dir.join("it's a.pdf");
    std::fs::write(&file, b"x").unwrap();
    std::fs::write(dir.join(".ts/it's a.pdf.json"), SIDECAR).unwrap();
    let app = app(tmp.path());
    let read = || app.notes.read_path(&file, &app.notes.policy());
    let notes = read();
    assert_eq!(notes.tag_names(), ["tag1", "done"]);
    assert_eq!(notes.tag("done").unwrap().color.as_deref(), Some("green"));
    assert_eq!(
        notes.comment.as_deref(),
        Some("# Some description\n\nin *markdown* format")
    );

    let edit = app
        .notes
        .edit(EntryRef::from_path(&file).unwrap())
        .tags(vec!["done".into(), "new".into()])
        .comment(Some("short".into()));
    assert_eq!(run(&app, Box::new(edit)), None);
    let json: Value =
        serde_json::from_slice(&std::fs::read(dir.join(".ts/it's a.pdf.json")).unwrap()).unwrap();
    assert_eq!(json["id"], "4194969c6bb84ad3acac779645c90e70", "kept");
    assert_eq!(
        json["tags"][0]["color"], "#008000ff",
        "a kept tag keeps its color"
    );
    assert_eq!(json["tags"][1]["title"], "new");
    assert_eq!(json["description"], "short");
    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join(".ts/it's a.pdf.json")).unwrap(),
        SIDECAR
    );
}

#[test]
fn a_folders_own_notes_are_inside_it_and_sidecars_follow_their_files() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("docs");
    let folder = dir.join("Trip d'été");
    std::fs::create_dir_all(folder.join(".ts")).unwrap();
    std::fs::create_dir_all(dir.join(".ts")).unwrap();
    std::fs::write(folder.join(".ts/tsm.json"), SIDECAR).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    std::fs::write(dir.join(".ts/a.txt.json"), SIDECAR).unwrap();
    std::fs::write(dir.join(".ts/a.txt.jpg"), b"thumbnail").unwrap();
    let app = app(tmp.path());
    assert_eq!(
        app.notes
            .read_path(&folder, &app.notes.policy())
            .tag_names(),
        ["tag1", "done"]
    );
    assert_eq!(
        run(
            &app,
            Box::new(Rename {
                entry: EntryRef::from_path(&file).unwrap(),
                new_name: "b.txt".into(),
            })
        ),
        None
    );
    assert!(dir.join(".ts/b.txt.json").exists() && dir.join(".ts/b.txt.jpg").exists());
    assert!(!dir.join(".ts/a.txt.json").exists());
    assert!(TagSpacesCompanion.is_folder(OsStr::new(".ts")));
}

#[test]
fn a_locations_index_hints_where_a_tag_is() {
    let tmp = home_tempdir();
    let root = tmp.path().join("location");
    std::fs::create_dir_all(root.join(".ts")).unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let index = serde_json::json!([
        { "path": root.join("sub/a.txt"), "tags": [{ "title": "Work" }] },
        { "path": "b.txt", "tags": [{ "title": "home" }] },
        { "path": root.join("c.txt"), "tags": [] },
    ]);
    std::fs::write(root.join(".ts/tsi.json"), index.to_string()).unwrap();
    let hint = IndexHint::default();
    let cancel = CancelToken::new();
    assert_eq!(
        hint.candidates(keys::TAGS, "work", &root, &cancel),
        Some(vec![root.join("sub/a.txt")])
    );
    assert_eq!(
        hint.candidates(keys::TAGS, "home", &root.join("sub"), &cancel),
        Some(Vec::new()),
        "under the search's root only"
    );
    assert_eq!(
        hint.candidates(keys::TAGS, "home", &root, &cancel),
        Some(vec![root.join("b.txt")])
    );
    assert_eq!(hint.candidates(keys::TAGS, "x", tmp.path(), &cancel), None);
}
