//! Tag preservation through the user-facing copy commands and saved plans.
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bibli_core::app::{App, AppConfig};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;
use bibli_core::op::{Copy, EntryRef, Operation, Plan};
use bibli_core::settings::{PRESERVE_TAGS_ON_COPY, SaveState};
use bibli_plugin_tags::{read_tags, write_tags};
use serde_json::{Value, json};

fn tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache/bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("copy-tags-")
        .tempdir_in(base)
        .unwrap()
}

fn dispatch(app: &App, command: &str, args: &Value) -> Outcome {
    app.dispatch(&Invocation::with_args(command, args)).unwrap()
}

fn navigate(app: &App, path: &Path) {
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(path)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
}

fn preference(app: &App, enabled: bool) {
    dispatch(
        app,
        "settings.apply",
        &json!({"changes": {PRESERVE_TAGS_ON_COPY: enabled.to_string()}}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.settings.save_status().1 == SaveState::Saving {
        assert!(Instant::now() < deadline, "settings save timed out");
        std::thread::yield_now();
    }
    assert_eq!(app.settings.save_status().1, SaveState::Saved);
}

#[test]
fn copy_commands_preserve_tags_and_capture_the_live_preference() {
    for command in ["copy", "clipboard_paste", "shelf.copy_here"] {
        for preserve in [true, false] {
            let tmp = tempdir();
            let source = tmp.path().join("Trip d'Example – été");
            let dest = tmp.path().join("destination");
            std::fs::create_dir_all(source.join("nested")).unwrap();
            std::fs::create_dir(&dest).unwrap();
            std::fs::write(source.join("nested/photo.txt"), "synthetic content").unwrap();
            std::fs::write(source.join("untagged.txt"), "plain").unwrap();
            for path in [
                &source,
                &source.join("nested"),
                &source.join("nested/photo.txt"),
            ] {
                write_tags(path, &["review".into(), "été".into()]).unwrap();
            }
            xattr::set(
                source.join("nested/photo.txt"),
                "user.unrelated",
                b"leave behind",
            )
            .unwrap();
            std::fs::set_permissions(
                source.join("nested/photo.txt"),
                std::fs::Permissions::from_mode(0o444),
            )
            .unwrap();
            symlink("nested/photo.txt", source.join("link")).unwrap();
            symlink("missing.txt", source.join("broken-link")).unwrap();

            let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
            app.plugins.add(
                Arc::new(bibli_plugin_ops_basic::OpsBasicPlugin::default()),
                &app,
            );
            app.plugins
                .add(Arc::new(bibli_plugin_shelf::ShelfPlugin::default()), &app);
            assert_eq!(app.settings.get::<bool>(PRESERVE_TAGS_ON_COPY), Some(true));
            if !preserve {
                preference(&app, false);
            }
            navigate(&app, tmp.path());
            dispatch(
                &app,
                "select",
                &json!({"names": [source.file_name().unwrap().to_str().unwrap()]}),
            );
            let outcome = match command {
                "clipboard_paste" => {
                    dispatch(&app, "clipboard_copy", &json!({}));
                    navigate(&app, &dest);
                    dispatch(&app, command, &json!({}))
                }
                "shelf.copy_here" => {
                    dispatch(&app, "shelf.add", &json!({}));
                    navigate(&app, &dest);
                    dispatch(&app, command, &json!({}))
                }
                _ => dispatch(&app, command, &json!({"dest": dest})),
            };
            let Outcome::Plan(plan) = outcome else {
                panic!("expected copy plan")
            };
            assert_eq!(plan.steps[0].args()["preserve_tags"], preserve);
            let plan = Plan::from_data(&plan.to_data(), &app.ops).unwrap();
            // Settings changed after planning must not change apply or redo.
            preference(&app, !preserve);
            let result = app.executor.execute_blocking(&plan, app.lifetime());
            assert!(result.error.is_none(), "{:?}", result.error);
            assert_eq!(result.applied, 1);
            let copied = dest.join(source.file_name().unwrap());
            for suffix in ["", "nested", "nested/photo.txt"] {
                assert_eq!(
                    read_tags(&copied.join(suffix)),
                    if preserve {
                        vec!["review", "été"]
                    } else {
                        vec![]
                    }
                );
            }
            assert_eq!(
                std::fs::read(copied.join("nested/photo.txt")).unwrap(),
                b"synthetic content"
            );
            assert_eq!(
                std::fs::metadata(copied.join("nested/photo.txt"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o444
            );
            assert!(
                xattr::get(copied.join("nested/photo.txt"), "user.unrelated")
                    .unwrap()
                    .is_none()
            );
            assert!(read_tags(&copied.join("untagged.txt")).is_empty());
            assert_eq!(
                std::fs::read_link(copied.join("link")).unwrap(),
                Path::new("nested/photo.txt")
            );
            assert_eq!(
                std::fs::read_link(copied.join("broken-link")).unwrap(),
                Path::new("missing.txt")
            );
            let repeated = app.executor.execute_blocking(&plan, app.lifetime());
            assert_eq!(repeated.applied, 0);
            assert_eq!(repeated.skipped, 1);
            let undo = app.executor.undo_blocking(app.lifetime()).unwrap();
            assert!(undo.error.is_none(), "{:?}", undo.error);
            assert!(!copied.exists());
            let redo = app.executor.redo_blocking(app.lifetime()).unwrap();
            assert!(redo.error.is_none(), "{:?}", redo.error);
            assert_eq!(!read_tags(&copied).is_empty(), preserve);
            assert_eq!(read_tags(&source), ["review", "été"]);
            assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 1);
        }
    }
}

#[test]
fn failed_copy_does_not_publish_a_partial_tree_or_touch_an_existing_target() {
    let tmp = tempdir();
    let source = tmp.path().join("source");
    let dest = tmp.path().join("dest");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&dest).unwrap();
    std::fs::write(source.join("good.txt"), "synthetic").unwrap();
    // A socket cannot be copied as an ordinary file: fail after creating the
    // staged directory, without requiring a privileged mount or real files.
    let _socket = std::os::unix::net::UnixListener::bind(source.join("socket")).unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    let copy = Copy::from_settings(
        EntryRef::from_path(&source).unwrap(),
        dest.clone(),
        &app.settings,
    );
    assert!(copy.apply().is_err());
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 0);
    std::fs::remove_file(source.join("socket")).unwrap();
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o555)).unwrap();
    std::fs::write(dest.join("source"), "existing destination").unwrap();
    assert!(copy.apply().is_err());
    assert_eq!(
        std::fs::read(dest.join("source")).unwrap(),
        b"existing destination"
    );
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 1);
    assert_eq!(
        std::fs::metadata(&source).unwrap().permissions().mode() & 0o777,
        0o555
    );
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn old_serialized_copy_plans_default_to_preserving_tags() {
    let tmp = tempdir();
    let source = tmp.path().join("source.txt");
    std::fs::write(&source, "synthetic").unwrap();
    let copy: Copy = serde_json::from_value(json!({
        "entry": EntryRef::from_path(source).unwrap(),
        "dest_dir": tmp.path(),
        "new_name": "copy.txt"
    }))
    .unwrap();
    assert!(copy.preserve_tags);
}
